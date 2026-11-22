use neolint::ast::parse_ast;
use neolint::ast_semantic::analyze_ast;
use neolint::semantic::analyze_with_syntax_diagnostics;
use neolint::{lex, parse_legacy, Diagnostic};
use std::fs;
use std::path::{Path, PathBuf};

const MIGRATED: &[&str] = &["SEM001", "SEM007", "SEM020", "SEM021"];

type Normalized = (
    &'static str,
    u8,
    u8,
    String,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
);

fn severity_rank(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.severity {
        neolint::Severity::Error => 0,
        neolint::Severity::Warning => 1,
        neolint::Severity::Info => 2,
    }
}

fn kind_rank(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.kind {
        neolint::DiagnosticKind::Lexical => 0,
        neolint::DiagnosticKind::Syntax => 1,
        neolint::DiagnosticKind::Semantic => 2,
        neolint::DiagnosticKind::Lint => 3,
    }
}

fn normalize(diagnostics: &[Diagnostic]) -> Vec<Normalized> {
    let mut values: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| MIGRATED.contains(&diagnostic.code))
        .map(|diagnostic| {
            (
                diagnostic.code,
                severity_rank(diagnostic),
                kind_rank(diagnostic),
                diagnostic.message.clone(),
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.span.end_line,
                diagnostic.span.end_column,
            )
        })
        .collect();
    values.sort();
    values
}

fn compare(source: &str) -> Result<(), String> {
    let (tokens, lexical) = lex(source);
    let syntax = parse_legacy(&tokens);
    let legacy = analyze_with_syntax_diagnostics(&tokens, &syntax);
    let (ast, ast_syntax) = parse_ast(&tokens);
    let ast_semantic = analyze_ast(&ast, &ast_syntax);
    let legacy = normalize(&legacy);
    let ast_semantic = normalize(&ast_semantic);
    if legacy == ast_semantic {
        Ok(())
    } else {
        Err(format!(
            "lexical={lexical:?}\nlegacy={legacy:#?}\nast={ast_semantic:#?}"
        ))
    }
}

fn visit_lsl(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|value| value.to_str()) == Some("lsl") {
            files.push(path.to_path_buf());
        }
        return;
    }
    let mut entries: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for entry in entries {
        visit_lsl(&entry, files);
    }
}

#[test]
fn corpus_sem001_007_020_021_match_legacy_exactly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus");
    let mut files = Vec::new();
    visit_lsl(&root, &mut files);
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    for path in &files {
        let source = fs::read_to_string(path).unwrap();
        if let Err(error) = compare(&source) {
            failures.push(format!("{}:\n{error}", path.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "checked {} fixtures:\n{}",
        files.len(),
        failures.join("\n\n")
    );
}

fn assert_parity(source: &str) {
    if let Err(error) = compare(source) {
        panic!("semantic parity failure for {source:?}:\n{error}");
    }
}

#[test]
fn focused_structural_rule_parity() {
    for source in [
        "integer a; float a;",
        "integer a; integer a() { return 1; }",
        "integer a() { return 1; } float a() { return 2.0; }",
        "integer first, duplicate; integer duplicate;",
        "integer parse(integer value) { integer value; return value; }",
        "default { state_entry() { integer value; integer value; } }",
        "default { state_entry() { integer value; { string value; } } }",
        "default { custom_event() { } custom_event() { } }",
        "default { state_entry() { } state_entry() { } }",
        "default { state_entry() { } } state running { state_entry() { } }",
        "default { } state running { } state running { }",
        "integer running; default { } state running { }",
        "integer idle() { return 1; } default { } state idle { }",
        "default { } state llOwnerSay { }",
        "default { } state llGodLikeRezObject { }",
        "default state_entry() { }",
        "integer phantom()\ndefault { }\nstate phantom { }",
        "default { state_entry(integer) { integer local; integer local; } state_entry() { } }",
    ] {
        assert_parity(source);
    }
}

#[test]
fn ast_scope_tree_is_ast_derived_and_deterministic() {
    let source = "integer global; integer f(integer parameter) { integer local; { integer nested; } return parameter; } default { state_entry() { integer event_local; } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast(&tokens);
    let first = analyze_ast(&ast, &syntax);
    let second = analyze_ast(&ast, &syntax);
    assert_eq!(first, second);
    assert!(!first
        .iter()
        .any(|diagnostic| MIGRATED.contains(&diagnostic.code)));
}

fn semantic(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    analyze_ast(&ast, &syntax)
}

fn span(source: &str, start: usize, len: usize) -> neolint::Span {
    let end = start + len;
    let line = source[..start].matches('\n').count() + 1;
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let end_line = source[..end].matches('\n').count() + 1;
    let end_line_start = source[..end].rfind('\n').map_or(0, |index| index + 1);
    neolint::Span {
        start,
        end,
        line,
        column: source[line_start..start].chars().count() + 1,
        end_line,
        end_column: source[end_line_start..end].chars().count() + 1,
    }
}

fn expected(
    code: &'static str,
    message: String,
    source: &str,
    start: usize,
    len: usize,
) -> Diagnostic {
    Diagnostic {
        code,
        severity: neolint::Severity::Error,
        kind: neolint::DiagnosticKind::Semantic,
        message,
        span: span(source, start, len),
    }
}

fn migrated(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| MIGRATED.contains(&diagnostic.code))
        .collect()
}

#[test]
fn sem001_exact_messages_and_spans() {
    let source = "integer count;\nfloat count;\n";
    let start = source.rfind("count").unwrap();
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM001",
            "duplicate global declaration 'count'; first declared at 1:9".to_owned(),
            source,
            start,
            "count".len()
        )]
    );

    let source = "integer name;\nname() { }\n";
    let start = source.rfind("name").unwrap();
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM001",
            "duplicate global declaration 'name'; first declared at 1:9".to_owned(),
            source,
            start,
            "name".len()
        )]
    );
}

#[test]
fn sem007_exact_messages_spans_and_shadowing() {
    let source = "default { state_entry() { integer value; integer value; } }";
    let first = source.find("value").unwrap();
    let second = source.rfind("value").unwrap();
    let first_span = span(source, first, "value".len());
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM007",
            format!(
                "duplicate variable 'value' in the same scope; first declared at {}:{}",
                first_span.line, first_span.column
            ),
            source,
            second,
            "value".len()
        )]
    );

    let source = "integer process(integer value) { integer value; return value; }";
    let parameter = source.find("value").unwrap();
    let local = source.match_indices("value").nth(1).unwrap().0;
    let parameter_span = span(source, parameter, "value".len());
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM007",
            format!(
                "duplicate variable 'value' in the same scope; first declared at {}:{}",
                parameter_span.line, parameter_span.column
            ),
            source,
            local,
            "value".len()
        )]
    );

    assert!(migrated(&semantic(
        "default { state_entry() { integer value; { string value; } } }"
    ))
    .is_empty());
}

#[test]
fn sem020_exact_message_and_scope_boundary() {
    let source = "default { state_entry() { } state_entry() { } }";
    let first = source.find("state_entry").unwrap();
    let second = source.rfind("state_entry").unwrap();
    let first_span = span(source, first, "state_entry".len());
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM020",
            format!(
                "duplicate event handler 'state_entry' in state 'default'; first declared at {}:{}",
                first_span.line, first_span.column
            ),
            source,
            second,
            "state_entry".len()
        )]
    );
    assert!(migrated(&semantic(
        "default { custom_event() { } custom_event() { } }"
    ))
    .is_empty());
    assert!(migrated(&semantic(
        "default { state_entry() { } } state running { state_entry() { } }"
    ))
    .is_empty());
}

#[test]
fn sem021_exact_duplicate_and_namespace_collisions() {
    let source = "default { } state running { } state running { }";
    let first = source.find("running").unwrap();
    let second = source.rfind("running").unwrap();
    let first_span = span(source, first, "running".len());
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM021",
            format!(
                "duplicate state 'running'; first declared at {}:{}",
                first_span.line, first_span.column
            ),
            source,
            second,
            "running".len()
        )]
    );

    let source = "integer running; default { } state running { }";
    let start = source.rfind("running").unwrap();
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM021",
            "state 'running' conflicts with a global variable declared at 1:9".to_owned(),
            source,
            start,
            "running".len()
        )]
    );

    let source = "integer idle() { return 1; } default { } state idle { }";
    let start = source.rfind("idle").unwrap();
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM021",
            "state 'idle' conflicts with a function declared at 1:9".to_owned(),
            source,
            start,
            "idle".len()
        )]
    );

    let source = "default { } state llOwnerSay { }";
    let start = source.find("llOwnerSay").unwrap();
    assert_eq!(
        semantic(source),
        vec![expected(
            "SEM021",
            "state 'llOwnerSay' conflicts with a canonical built-in function".to_owned(),
            source,
            start,
            "llOwnerSay".len()
        )]
    );
    assert!(migrated(&semantic(
        "default { } state llGodLikeRezObject { } state llSetInventoryPermMask { } state llSetObjectPermMask { }"
    ))
    .is_empty());
}

#[test]
fn malformed_recovery_nodes_do_not_fabricate_declarations() {
    assert!(migrated(&semantic("state ;")).is_empty());
    assert!(migrated(&semantic("default state_entry() { }")).is_empty());
    assert!(migrated(&semantic("integer value = ; default { state_entry() { } }")).is_empty());
    assert!(migrated(&semantic(
        "integer first = 1, ; default { state_entry() { } }"
    ))
    .is_empty());
}

#[test]
fn switch_blocks_have_their_own_scope_without_leakage() {
    use neolint::ast::{parse_ast_with_options, AstParseOptions};
    let source = "default { state_entry() { integer outer; switch (value) { case 1: integer inner; break; } integer inner; } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) =
        parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
    let diagnostics = analyze_ast(&ast, &syntax);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM007"),
        "{diagnostics:?}"
    );

    let source = "default { state_entry() { switch (value) { case 1: { integer duplicate; integer duplicate; } break; } } }";
    let (tokens, _) = lex(source);
    let (ast, syntax) =
        parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
    assert_eq!(
        analyze_ast(&ast, &syntax)
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM007")
            .count(),
        1
    );
}

#[test]
fn deeply_nested_parser_asts_do_not_overflow_semantic_traversal() {
    let depth = 200;
    let mut body = String::new();
    for _ in 0..depth {
        body.push_str("{ integer nested; ");
    }
    for _ in 0..depth {
        body.push_str("} ");
    }
    let source = format!("default {{ state_entry() {{ {body} }} }}");
    let diagnostics = semantic(&source);
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| MIGRATED.contains(&diagnostic.code)));
}

#[test]
fn sem021_ignores_function_declaration_without_body() {
    let source = "integer phantom()\ndefault { }\nstate phantom { }";
    let diagnostics = semantic(source);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM021"),
        "{diagnostics:?}"
    );
}

#[test]
fn malformed_event_parameters_keep_event_body_semantics() {
    let source =
        "default { state_entry(integer) { integer local; integer local; } state_entry() { } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast(&tokens);
    let state = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::State(state) = item {
                Some(state)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(state.events.len(), 2);
    assert!(state.events[0].body.is_some());

    let diagnostics = analyze_ast(&ast, &syntax);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM007")
            .count(),
        1,
        "{diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM020")
            .count(),
        1,
        "{diagnostics:?}"
    );
    assert_parity(source);
}
