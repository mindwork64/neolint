use neolint::ast::parse_ast;
use neolint::ast_semantic::analyze_ast;
use neolint::semantic::analyze_with_syntax_diagnostics;
use neolint::{lex, parse_legacy, Diagnostic};
use std::fs;
use std::path::{Path, PathBuf};

const MIGRATED: &[&str] = &[
    "SEM001", "SEM002", "SEM003", "SEM004", "SEM005", "SEM006", "SEM007", "SEM008", "SEM009",
    "SEM010", "SEM011", "SEM014", "SEM015", "SEM016", "SEM017", "SEM018", "SEM019", "SEM020",
    "SEM021",
];

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
fn corpus_nineteen_migrated_rules_match_legacy_exactly() {
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
        "default { state_entry(bad, integer value) { integer value; } state_entry() { } }",
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

#[test]
fn malformed_parameter_recovery_keeps_later_valid_parameters() {
    let source = "default { state_entry(bad, integer value) { integer value; } state_entry() { } }";
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
    assert_eq!(state.events[0].parameters.len(), 1);
    assert_eq!(state.events[0].parameters[0].name, "value");
    assert_eq!(
        state.events[0].parameters[0].ty.kind,
        neolint::ast::TypeKind::Integer
    );
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

#[test]
fn suppressed_eof_does_not_hide_the_outer_state_error() {
    let source = "default {\n    state_entry(integer) {\n";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let legacy = parse_legacy(&tokens);
    let (_, ast) = parse_ast(&tokens);
    assert_eq!(normalize(&legacy), normalize(&ast));
    assert!(ast.iter().any(|diagnostic| diagnostic.code == "PAR007"));
}

#[test]
fn focused_call_and_event_parity() {
    for source in [
        "default { state_entry() { llOwnerSay(); } }",
        "default { state_entry() { llOwnerSay(\"a\", \"b\"); } }",
        "default { state_entry() { llOwnerSay(1); } }",
        "default { state_entry() { llSetPos(\"x\"); } }",
        "integer f(integer x) { return x; } default { state_entry() { f(); f(1, 2); f(\"x\"); f(1); } }",
        "integer f(integer x) { return x; } integer f(string y) { return 1; } default { state_entry() { f(\"x\"); } }",
        "llOwnerSay(integer x) { } default { state_entry() { llOwnerSay(1); llOwnerSay(\"x\"); } }",
        "default { state_entry(integer x) { } }",
        "default { touch_start(string x) { } }",
        "default { custom_event(integer x) { } }",
        "default { state_entry() { integer local = 1; if (local) llOwnerSay(local); } }",
        "default { state_entry() { integer local; local = llOwnerSay(); } }",
        "default { state_entry() { list values = [llOwnerSay()]; } }",
        "default { state_entry() { llOwnerSay((string)llGetListLength([1, 2, 3])); } }",
        "integer f(integer x) { return x; } default { state_entry() { integer y = f(f()); } }",
        "default { state_entry(bad, integer value) { integer value; } state_entry() { } }",
        "float f(integer x) { return 1.0; } default { state_entry() { llOwnerSay(f(\"x\")); } }",
        "default { state_entry() { vector v; float x = v.x; llOwnerSay(x); } }",
        "default { state_entry() { llOwnerSay([1]); } }",
        "default { state_entry() { llOwnerSay(<1, 2, 3>); } }",
        "default { state_entry() { llOwnerSay((1)); } }",
        "default { state_entry() { llOwnerSay(-1); } }",
        "default { state_entry() { llOwnerSay((integer)1); } }",
        "default { state_entry() { llOwnerSay(llGetListLength([])); } }",
        "default { state_entry() { llOwnerSay(1 + 1); } }",
        "default { state_entry() { llOwnerSay(TRUE); } }",
        "default { state_entry() { vector v; llOwnerSay(v.x); } }",
    ] {
        assert_parity(source);
    }
}

#[test]
fn sem002_and_sem004_exact_messages_and_spans() {
    let source = "default { state_entry() { llOwnerSay(); } }";
    let start = source.find("llOwnerSay").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM002",
            "llOwnerSay expects 1 argument(s), but received 0".to_owned(),
            source,
            start,
            "llOwnerSay".len()
        )]
    );

    let source = "default { state_entry() { llOwnerSay(1); } }";
    let callee = source.find("llOwnerSay").unwrap();
    let argument = source.find('1').unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM004",
            "llOwnerSay argument 1 has type integer, expected string".to_owned(),
            source,
            argument,
            1
        )]
    );
    assert!(callee < argument);
}

#[test]
fn sem005_and_sem006_exact_messages_and_spans() {
    let source = "integer f(integer x) { return x; } default { state_entry() { f(); } }";
    let start = source.rfind("f").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM005",
            "f expects 1 argument(s), but received 0".to_owned(),
            source,
            start,
            "f".len()
        )]
    );

    let source = "integer f(integer x) { return x; } default { state_entry() { f(\"bad\"); } }";
    let argument = source.find("\"bad\"").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM006",
            "f argument 1 has type string, expected integer".to_owned(),
            source,
            argument,
            "\"bad\"".len()
        )]
    );
}

#[test]
fn sem003_exact_arity_and_type_messages_and_spans() {
    let source = "default { state_entry(integer x) { } }";
    let start = source.find("state_entry").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM003",
            "event state_entry expects 0 parameter(s), but 1 were declared".to_owned(),
            source,
            start,
            "state_entry".len()
        )]
    );

    let source = "default { touch_start(string x) { } }";
    let start = source.find("touch_start").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM003",
            "event touch_start parameter 1 has type string, expected integer".to_owned(),
            source,
            start,
            "touch_start".len()
        )]
    );
}

#[test]
fn same_name_builtin_and_user_function_checks_are_independent() {
    let source =
        "llOwnerSay(integer x) { } default { state_entry() { llOwnerSay(1); llOwnerSay(\"x\"); } }";
    let diagnostics = semantic(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM004")
            .count(),
        1
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM006")
            .count(),
        1
    );
    assert_parity(source);
}

#[test]
fn calls_are_visited_once_in_every_expression_bearing_field() {
    let source = r#"
        integer probe(integer value) { return value; }
        integer global = probe();
        default {
            state_entry() {
                integer local = probe();
                if (probe()) { } else { probe(); }
                for (probe(); probe(); probe()) { probe(); }
                while (probe()) { probe(); }
                do { probe(); } while (probe());
                list values = [probe(), <probe(), 1, 2>, <probe(), 1, 2, 3>];
                return probe();
            }
        }
    "#;
    let diagnostics = semantic(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM005")
            .count(),
        16,
        "{diagnostics:?}"
    );
    assert_parity(source);
}

#[test]
fn syntax_error_inside_call_arguments_suppresses_call_rules() {
    use neolint::{DiagnosticKind, Severity};
    let source = "default { state_entry() { llOwnerSay(1); } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast(&tokens);
    assert!(syntax.is_empty());
    let call = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::State(state) = item {
                state.events.iter().find_map(|event| {
                    event.body.as_ref().and_then(|body| {
                        body.statements.iter().find_map(|statement| {
                            if let neolint::ast::StatementKind::Expression(expression) =
                                &statement.kind
                            {
                                if let neolint::ast::ExpressionKind::Call(call) =
                                    &expression.expression.kind
                                {
                                    return Some(call);
                                }
                            }
                            None
                        })
                    })
                })
            } else {
                None
            }
        })
        .unwrap();
    let injected = Diagnostic {
        code: "PAR099",
        severity: Severity::Error,
        kind: DiagnosticKind::Syntax,
        message: "injected syntax error".to_owned(),
        span: span(
            source,
            call.open_paren_span.end,
            call.close_paren_span.start - call.open_paren_span.end,
        ),
    };
    let diagnostics = analyze_ast(&ast, std::slice::from_ref(&injected));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code, "SEM002" | "SEM004")),
        "{diagnostics:?}"
    );
    assert!(analyze_ast(&ast, &[])
        .iter()
        .any(|diagnostic| diagnostic.code == "SEM004"));
}

#[test]
fn firestorm_switch_and_lazy_calls_are_analyzed_without_panic() {
    use neolint::ast::{parse_ast_with_options, AstParseOptions};
    let source = "integer probe(integer value) { return value; } default { state_entry() { list values; (integer)values[probe()]; switch (probe()) { case probe(): { probe(); } break; } } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast_with_options(
        &tokens,
        AstParseOptions::lsl()
            .with_switches_enabled(true)
            .with_lazy_lists_enabled(true),
    );
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = analyze_ast(&ast, &syntax);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM005")
            .count(),
        4,
        "{diagnostics:?}"
    );
}

#[test]
fn user_function_first_definition_wins() {
    let source = "integer f(integer x) { return x; } integer f(string x) { return 1; } default { state_entry() { f(\"bad\"); } }";
    let diagnostics = semantic(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM006")
            .count(),
        1,
        "{diagnostics:?}"
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM006"
            && diagnostic.message == "f argument 1 has type string, expected integer"
    }));
    assert_parity(source);
}

#[test]
fn untyped_function_return_is_void() {
    let source = "f() { } default { state_entry() { llOwnerSay(f()); } }";
    let diagnostics = semantic(source);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM004"
            && diagnostic.message == "llOwnerSay argument 1 has type void, expected string"
    }));
    assert_parity(source);
}

#[test]
fn implicit_integer_float_and_string_key_conversions_are_allowed() {
    let source = "integer f(float x) { return 1; } integer g(key k) { return 1; } default { state_entry() { f(1); g(\"s\"); f(\"s\"); g(1); } }";
    let diagnostics = semantic(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM006")
            .count(),
        2,
        "{diagnostics:?}"
    );
    assert_parity(source);
}

#[test]
fn focused_malformed_call_parity() {
    for source in [
        "default { state_entry() { llOwnerSay(\"x\",); } }",
        "default { state_entry() { llOwnerSay(, \"x\"); } }",
        "default { state_entry() { llOwnerSay(\"x\" \"y\"); } }",
        "integer f(integer x) { return x; } default { state_entry() { f(1,); f(, 1); } }",
        "default { state_entry() { llOwnerSay(); } }",
        "default { state_entry() { llOwnerSay(1 + ); } }",
        "default { state_entry() { llOwnerSay(1; llGetListLength(1)); } }",
        "default { state_entry(integer,) { } }",
        "default { touch_start(string) { } }",
        "default { touch_start(integer x,) { } }",
        "default {\n    state_entry(integer) {\n",
    ] {
        assert_parity(source);
    }
}

#[test]
fn malformed_call_recovery_keeps_declared_argument_count() {
    let source = "default { state_entry() { llOwnerSay(\"x\",); } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast(&tokens);
    let call = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::State(state) = item {
                state.events.iter().find_map(|event| {
                    event.body.as_ref().and_then(|body| {
                        body.statements.iter().find_map(|statement| {
                            if let neolint::ast::StatementKind::Expression(expression) =
                                &statement.kind
                            {
                                if let neolint::ast::ExpressionKind::Call(call) =
                                    &expression.expression.kind
                                {
                                    return Some(call);
                                }
                            }
                            None
                        })
                    })
                })
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(call.declared_argument_count, 2);
    assert_eq!(call.arguments.len(), 1);
    assert!(!call.arguments_complete);

    let diagnostics = analyze_ast(&ast, &syntax);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM002"
            && diagnostic.message == "llOwnerSay expects 1 argument(s), but received 2"
    }));
    assert_parity(source);
}

#[test]
fn switch_case_expressions_use_switch_scope() {
    use neolint::ast::{parse_ast_with_options, AstParseOptions};
    let source = "integer takes(integer value) { return value; } default { state_entry() { switch (1) { case 0: string text = \"x\"; case takes(text): ; } } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let legacy_syntax = parse_legacy(&tokens);
    let legacy = neolint::semantic::analyze_with_syntax_diagnostics(&tokens, &legacy_syntax);
    let (ast, syntax) =
        parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
    let ast_semantic = analyze_ast(&ast, &syntax);
    assert_eq!(normalize(&legacy), normalize(&ast_semantic));
    assert!(ast_semantic.iter().any(|diagnostic| {
        diagnostic.code == "SEM006"
            && diagnostic.message == "takes argument 1 has type string, expected integer"
    }));
}

#[test]
fn recovered_event_without_parameter_name_keeps_declared_type() {
    let source = "default { touch_start(string) { } }";
    let diagnostics = semantic(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM003")
            .count(),
        1,
        "{diagnostics:?}"
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "event touch_start parameter 1 has type string, expected integer"
    }));
    assert_parity(source);
}

#[test]
fn malformed_function_header_keeps_user_signature() {
    let source = "integer f(string) { return 1; } default { state_entry() { f(1); } }";
    let diagnostics = semantic(source);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM006"
            && diagnostic.message == "f argument 1 has type integer, expected string"
    }));
    assert_parity(source);
}

#[test]
fn recovered_first_definition_still_wins() {
    let source = "integer f(string) { return 1; } integer f(integer x) { return x; } default { state_entry() { f(\"x\"); f(1); } }";
    let diagnostics = semantic(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM006")
            .count(),
        1,
        "{diagnostics:?}"
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM006"
            && diagnostic.message == "f argument 1 has type integer, expected string"
    }));
    assert_parity(source);
}

#[test]
fn sem021_uses_recovered_malformed_function_signatures() {
    let source = "integer llOwnerSay(string) { return 1; } default { } state llOwnerSay { }";
    let diagnostics = semantic(source);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM021"
            && diagnostic.message == "state 'llOwnerSay' conflicts with a function declared at 1:9"
    }));
    assert_parity(source);

    let source = "integer phantom(string) { return 1; } default { } state phantom { }";
    let diagnostics = semantic(source);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM021"
            && diagnostic.message == "state 'phantom' conflicts with a function declared at 1:9"
    }));
    assert_parity(source);
}

#[test]
fn sem021_recovered_function_preserves_first_definition_span() {
    let source = "integer phantom(string) { return 1; } integer phantom(integer x) { return x; } default { } state phantom { }";
    let diagnostics = semantic(source);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "SEM021"
            && diagnostic.message == "state 'phantom' conflicts with a function declared at 1:9"
    }));
    assert_parity(source);
}

#[test]
fn focused_flow_rule_parity() {
    for source in [
        "integer get_value() { return; } default { state_entry() { } }",
        "integer get_value() { return \"wrong\"; } default { }",
        "do_work() { return 1; } default { state_entry() { } }",
        "integer get_count() { return 1; } float get_float() { return 2; } do_work() { return; } default { state_entry() { } }",
        "integer get_count() { string name; return name; } default { state_entry() { } }",
        "integer get_float() { return 1; } integer get_key() { return \"key\"; } default { }",
        "integer get_value() { return missing; } default { }",
        "integer first() { return 1; } integer first() { return \"wrong\"; } default { }",
        "integer first() { return \"wrong\"; } integer first() { return 1; } default { }",
        "integer get_value() { jump done; @done; return 1; } default { state_entry() { } }",
        "integer get_value() { @done; { jump done; } return 1; } default { state_entry() { } }",
        "integer get_value() { jump missing; return 1; } default { }",
        "integer get_value() { jump inside; { @inside; } return 1; } default { }",
        "integer get_value() { @same; { @same; } } default { }",
        "integer first() { @done; return 1; } integer second() { @done; return 2; } default { }",
        "default { state_entry() { jump done; @done; } }",
        "default { state_entry() { jump missing; } }",
        "default { state_entry() { @done; } touch_end() { @done; } }",
        "integer get_value() { llOwnerSay(\"missing\"); } default { state_entry() { } }",
        "integer choose(integer flag) { if (flag) return 1; } default { }",
        "integer choose(integer flag) { if (flag) return 1; else return 2; } default { }",
        "integer choose(integer flag) { if (flag == 1) return 1; if (flag == 2) return 2; return 0; } default { }",
        "integer choose(integer flag) { while (flag) { return 1; } } default { }",
        "integer choose() { do { return 1; } while (1); } default { }",
        "integer choose(integer flag) { if (flag) jump done; return 1; @done; } default { }",
        "integer choose(integer flag) { if (flag) jump done; return 1; @done; return 2; } default { }",
        "integer spin() { @again; jump again; } default { }",
        "integer spin() { while (TRUE) { } } integer spin_forever() { for (;;) { } } default { }",
        "integer choose() { while (FALSE) return 1; } default { }",
        "integer yes() { if (1 + 2) return 1; } integer no() { if (1 > 2) return 1; } default { }",
        "integer yes() { while (TRUE + FALSE) { } } integer no() { while (ALL_SIDES + 1) { } } default { }",
        "integer choose_param(integer TRUE) { if (TRUE) return 1; } integer TRUE = 0; default { }",
        "integer deep() { { { if (1) return 1; } return 2; } } default { }",
        "integer nested(integer n) { for (; n; ) { for (; n; ) { return 1; } } } default { }",
        "integer broken() { if (1 { return 1; } default { }",
        "integer broken() { jump; } default { }",
        "integer broken() { @; } default { }",
        "integer choose() { jump missing; return 1; } default { }",
        "do_work() { llOwnerSay(\"done\"); } default { }",
        "default { custom_event() { @same; @same; jump missing; } }",
        "default { state_entry() { @outer; { jump outer; } { jump inner; @inner; } } }",
        "integer choose() { @same; { @same; } jump same; return 1; } default { }",
        "integer choose() { jump missing; } default { }",
        "default { state_entry() { jump missing; } }",
        "integer spin() { while (0xFFFFFFFF) { } } integer no() { while (0x0) { } } default { }",
        "integer wrap() { while (0xFFFFFFFF + 1) { } } integer shift() { while (1 << 31) { } } default { }",
        "integer division() { while (1 / 0) { } } integer remainder() { while (1 % 0) { } } default { }",
        "integer float_yes() { while (1.0) { } } integer float_no() { while (0.0) { } } default { }",
        "integer known() { if (TRUE) return 1; } integer choose_param(integer TRUE) { if (TRUE) return 1; } default { }",
        "integer choose_local() { integer TRUE; if (TRUE) return 1; } integer choose_nested() { { integer TRUE; if (TRUE) return 1; } } default { }",
        "integer choose_global() { if (TRUE) return 1; } integer TRUE = 0; default { }",
        "integer equality() { while ((7 & 3) == 3 && (1 | 2) == 3) { } } default { }",
        "integer overflow() { while (2147483647 + 1) { } } default { }",
        "integer loop_jump_inside(integer flag) { while (flag) { jump done; @done; return 1; } } default { }",
        "default { state_entry() { @done; } } state running { state_entry() { @done; } }",
        "integer f() { @a; { jump a; } return 1; } integer g() { { @b; } jump b; return 1; } default { }",
        "integer f() { { @a; } { jump a; } return 1; } default { }",
        "integer f() { @a; { { jump a; } } return 1; } default { }",
        "integer f(integer flag) { if (missing_name) return 1; } default { }",
        "integer a() { while (1 << -1) { } } integer b() { while (1 >> 32) { } } default { }",
        "integer a() { while (0xFFFFFFFF + 1) { } } integer b() { while (2147483648) { } } default { }",
        "integer f() { jump missing; } default { }",
        "integer spin() { for ( ; ; ) { } } default { }",
        "do_work() { return ,; } default { }",
        "integer f() { return ,; } default { }",
        "integer f() { for (; ,; ) { } } default { }",
        "integer f() { for (; 1 + ; ) { } } default { }",
        "do_work() { return @done; jump done; } default { }",
        "do_work() { return jump missing; jump missing; } default { }",
    ] {
        assert_parity(source);
    }
}

#[test]
fn sem008_exact_return_messages_and_spans() {
    let source = "integer get_value() { return; } default { state_entry() { } }";
    let keyword = source.find("return").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM008",
            "function get_value returns integer, but this return statement has no value".to_owned(),
            source,
            keyword,
            "return".len()
        )]
    );

    let source = "do_work() { return 1; } default { state_entry() { } }";
    let value = source.find('1').unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM008",
            "function do_work does not return a value, but this return statement has one"
                .to_owned(),
            source,
            value,
            1
        )]
    );

    let source = "integer get_value() { return \"wrong\"; } default { }";
    let value = source.find("\"wrong\"").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM008",
            "function get_value returns integer, but this return expression has type string"
                .to_owned(),
            source,
            value,
            "\"wrong\"".len()
        )]
    );
}

#[test]
fn sem009_and_sem010_exact_messages_and_spans() {
    let source = "integer get_value() { @same; { @same; } } default { }";
    let first = source.find("@same").unwrap() + 1;
    let second = source.rfind("@same").unwrap() + 1;
    let first_span = span(source, first, "same".len());
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM009",
            format!(
                "duplicate jump label 'same' in get_value; first declared at {}:{}",
                first_span.line, first_span.column
            ),
            source,
            second,
            "same".len()
        )]
    );

    let source = "integer get_value() { jump inside; { @inside; } return 1; } default { }";
    let target = source.find("inside").unwrap();
    assert_eq!(
        migrated(&semantic(source)).into_iter().cloned().collect::<Vec<_>>(),
        vec![expected(
            "SEM010",
            "jump target 'inside' is inside a nested block and is not accessible from this jump in get_value".to_owned(),
            source,
            target,
            "inside".len()
        )]
    );

    let source = "integer get_value() { jump missing; return 1; } default { }";
    let target = source.find("missing").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM010",
            "jump target 'missing' is not defined in get_value".to_owned(),
            source,
            target,
            "missing".len()
        )]
    );
}

#[test]
fn sem011_exact_message_and_span() {
    let source = "integer get_value() { llOwnerSay(\"missing\"); } default { state_entry() { } }";
    let name = source.find("get_value").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM011",
            "function get_value may reach the end without returning a value".to_owned(),
            source,
            name,
            "get_value".len()
        )]
    );
}

#[test]
fn parser_preserves_return_keyword_and_empty_for_condition() {
    let source = "integer f() { for (;;) { return; } } default { }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, _) = parse_ast(&tokens);
    let function = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::Function(function) = item {
                Some(function)
            } else {
                None
            }
        })
        .unwrap();
    let body = function.body.as_ref().unwrap();
    let neolint::ast::StatementKind::For(for_statement) = &body.statements[0].kind else {
        panic!("expected for statement");
    };
    assert!(for_statement.condition_missing);
    let neolint::ast::StatementKind::Block(block) = &for_statement.body.kind else {
        panic!("expected block body");
    };
    let neolint::ast::StatementKind::Return(return_statement) = &block.statements[0].kind else {
        panic!("expected return");
    };
    let keyword = source.find("return").unwrap();
    assert_eq!(
        return_statement.keyword_span,
        span(source, keyword, "return".len())
    );
}

#[test]
fn firestorm_switch_and_break_suppress_missing_return_conservatively() {
    use neolint::ast::{parse_ast_with_options, AstParseOptions};
    let source = "integer choose(integer value) { switch (value) { case 1: break; } } default { state_entry() { } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) =
        parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = analyze_ast(&ast, &syntax);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM011"),
        "{diagnostics:?}"
    );
}

#[test]
fn cfg_node_budget_skips_only_the_oversized_function() {
    let boundary = format!(
        "integer boundary() {{ {} }} default {{ }}",
        ";".repeat(4_095)
    );
    assert!(analyze_ast(
        &parse_ast(&lex(&boundary).0).0,
        &parse_ast(&lex(&boundary).0).1
    )
    .iter()
    .any(|diagnostic| diagnostic.code == "SEM011"));

    let over = format!(
        "integer huge() {{ {} }} integer small() {{ }} default {{ }}",
        ";".repeat(4_096)
    );
    let (tokens, lexical) = lex(&over);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast(&tokens);
    let diagnostics = analyze_ast(&ast, &syntax)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM011")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("function small"));
}

#[test]
fn cfg_depth_budget_skips_only_the_oversized_function() {
    let depth = 130;
    let source = format!(
        "integer huge() {{ {}{}{} }} integer small() {{ }} default {{ }}",
        "{".repeat(depth),
        "",
        "}".repeat(depth)
    );
    let (tokens, lexical) = lex(&source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast(&tokens);
    let diagnostics = analyze_ast(&ast, &syntax)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM011")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("function small"));
}

#[test]
fn invalid_jump_suppresses_sem011_for_that_function() {
    let source = "integer choose() { jump missing; } default { }";
    let diagnostics = semantic(source);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "SEM010"));
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "SEM011"));
    assert_parity(source);
}

#[test]
fn malformed_return_expression_keeps_sem008_value_span() {
    let source = "do_work() { return ,; } default { }";
    let comma = source.find(',').unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM008",
            "function do_work does not return a value, but this return statement has one"
                .to_owned(),
            source,
            comma,
            1
        )]
    );
    assert_parity(source);
}

#[test]
fn malformed_for_condition_is_unknown_not_missing() {
    let source = "integer f() { for (; ,; ) { } } default { }";
    assert!(
        semantic(source)
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM011"),
        "malformed non-empty condition must stay unknown, not infinite"
    );
    assert_parity(source);

    let source = "integer f() { for (;;) { } } default { }";
    assert!(
        !semantic(source)
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM011"),
        "genuinely empty condition remains always true"
    );
    assert_parity(source);

    let (tokens, _) = lex(source);
    let (ast, _) = parse_ast(&tokens);
    let function = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::Function(function) = item {
                Some(function)
            } else {
                None
            }
        })
        .unwrap();
    let neolint::ast::StatementKind::For(for_statement) =
        &function.body.as_ref().unwrap().statements[0].kind
    else {
        panic!("expected for");
    };
    assert!(for_statement.condition_missing);
    assert!(!for_statement.condition_malformed);

    let source = "integer f() { for (; ,; ) { } } default { }";
    let (tokens, _) = lex(source);
    let (ast, _) = parse_ast(&tokens);
    let function = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::Function(function) = item {
                Some(function)
            } else {
                None
            }
        })
        .unwrap();
    let neolint::ast::StatementKind::For(for_statement) =
        &function.body.as_ref().unwrap().statements[0].kind
    else {
        panic!("expected for");
    };
    assert!(!for_statement.condition_missing);
    assert!(for_statement.condition_malformed);
}

#[test]
fn malformed_return_preserves_nested_recovered_label_scope() {
    let source = "do_work() { return { @done; }; jump done; } default { }";
    let diagnostics = semantic(source);
    let migrated_diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code, "SEM008" | "SEM009" | "SEM010"))
        .collect::<Vec<_>>();

    assert_eq!(
        migrated_diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        vec!["SEM008", "SEM010"],
        "{diagnostics:?}"
    );

    let jump_target = source.find("jump done").unwrap() + "jump ".len();
    assert_eq!(
        migrated_diagnostics[1].message,
        "jump target 'done' is inside a nested block and is not accessible from this jump in do_work"
    );
    assert_eq!(
        migrated_diagnostics[1].span,
        span(source, jump_target, "done".len())
    );
    assert_parity(source);

    let (tokens, _) = lex(source);
    let (ast, _) = parse_ast(&tokens);
    let function = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::Function(function) = item {
                Some(function)
            } else {
                None
            }
        })
        .unwrap();
    let neolint::ast::StatementKind::Return(return_statement) =
        &function.body.as_ref().unwrap().statements[0].kind
    else {
        panic!("expected recovered return");
    };
    assert_eq!(return_statement.recovered_labels.len(), 1);
    assert_eq!(return_statement.recovered_labels[0].scope_path.len(), 1);
}

#[test]
fn malformed_return_preserves_recovered_label_visibility() {
    let source = "do_work() { return @done; jump done; } default { }";
    let at = source.find('@').unwrap();
    let diagnostics = semantic(source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code, "SEM008" | "SEM009" | "SEM010"))
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        vec!["SEM008"],
        "{diagnostics:?}"
    );
    assert!(diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.code == "SEM008" && diagnostic.span.start == at }));
    assert_parity(source);

    let (tokens, _) = lex(source);
    let (ast, _) = parse_ast(&tokens);
    let function = ast
        .items
        .iter()
        .find_map(|item| {
            if let neolint::ast::TopLevel::Function(function) = item {
                Some(function)
            } else {
                None
            }
        })
        .unwrap();
    let neolint::ast::StatementKind::Return(return_statement) =
        &function.body.as_ref().unwrap().statements[0].kind
    else {
        panic!("expected recovered return");
    };
    assert_eq!(return_statement.recovered_labels.len(), 1);
    assert_eq!(return_statement.recovered_labels[0].label.name, "done");
    assert!(return_statement.recovered_jumps.is_empty());
}

#[test]
fn focused_expression_rule_parity() {
    for source in [
        "integer f() { return (integer)<1, 2, 3>; } default { }",
        "integer f() { return (vector)1; } default { }",
        "key f() { return (key)1; } default { }",
        "list f() { return (list)5; } default { }",
        "string f() { return (string)5; } default { }",
        "integer value = (integer)<1, 2, 3>; default { }",
        "integer x = \"s\"; float y = 1; key k = \"s\"; integer a = 1, b = \"s\", c = 2.0; default { }",
        "integer g; default { state_entry() { integer x = \"s\"; float y = 1; key k = \"s\"; integer a = 1, b = \"s\", c = 2.0; } }",
        "integer x; default { state_entry() { x = \"s\"; integer y; y = 1.0; (x) = \"s\"; } }",
        "integer i; float f; default { state_entry() { i += \"s\"; f += 1; i += 1.0; } }",
        "vector v; rotation r; integer i; default { state_entry() { v.x = 1; v.x = \"s\"; v.x += \"s\"; v.w = 1; r.s = 1; r.w = 1; i.x = 1; (v.z) = 2; ((v.z)) += 1; } }",
        "vector v; rotation r; integer i; default { state_entry() { float a = v.w; float b = v.x; float c = r.s; float d = r.w; float e = i.x; } }",
        "vector v; default { state_entry() { v.x = v.w; v.w = 1; (v.x) = v.w; } }",
        "default { custom_event() { integer x = \"s\"; x = 1.0; } }",
        "integer f(integer p) { integer x = p; x = \"s\"; return (integer)p; } default { }",
        "integer value = (integer)((string)1); default { }",
        "integer x; default { state_entry() { if ((x = \"s\")) { } for (x = \"s\"; x; x = \"s\") { } } }",
        "list values = [1, (integer)<1,2,3>]; default { }",
        "vector values = <(integer)<1,2,3>, 1, 2>; default { }",
        "integer x = y; integer y = 1; default { }",
        "integer x = x; default { }",
        "integer x = custom_call(); default { }",
        "integer x = ; default { }",
        r#"integer f(integer p) { integer x = "s"; { string x = "s"; } return (integer)p; } default { }"#,
        "integer i; default { state_entry() { i -= 1.0; i *= 1.0; i /= 1.0; i %= 1.0; } }",
        "vector v; default { state_entry() { v.x *= 2; v.x *= <1,2,3>; v.x -= 1.0; r.s += 1; } }",
        "vector v; default { state_entry() { float a = (v.w); (v.x) = v.w; } }",
        "integer f() { return (vector)1; } default { }",
        "integer x = (string)5; default { }",
        r#"default { custom_event() { integer x = "s"; } }"#,
    ] {
        assert_parity(source);
    }
}

#[test]
fn sem014_exact_cast_message_and_target_span() {
    let source = "integer f() { return (integer)<1, 2, 3>; } default { }";
    let target = source.rfind("integer").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM014",
            "cannot explicitly cast vector to integer in LSL".to_owned(),
            source,
            target,
            "integer".len()
        )]
    );
}

#[test]
fn sem015_exact_initializer_message_and_name_span() {
    let source = "integer x = \"s\"; default { }";
    let name = source.find("x =").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM015",
            "initializer for variable 'x' has type string, expected integer".to_owned(),
            source,
            name,
            "x".len()
        )]
    );
}

#[test]
fn sem016_exact_assignment_message_and_target_span() {
    let source = "integer x; default { state_entry() { x = \"s\"; } }";
    let target = source.rfind("x =").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM016",
            "assignment to variable 'x' has type string, expected integer".to_owned(),
            source,
            target,
            "x".len()
        )]
    );
}

#[test]
fn sem017_exact_messages_and_target_span() {
    let source = "integer i; default { state_entry() { i += \"s\"; } }";
    let target = source.rfind("i +=").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM017",
            "compound assignment '+=' is not supported for operand types integer and string"
                .to_owned(),
            source,
            target,
            "i".len()
        )]
    );

    let source = "integer i; default { state_entry() { i += 1.0; } }";
    let target = source.rfind("i +=").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM017",
            "compound assignment '+=' to variable 'i' yields type float, expected integer"
                .to_owned(),
            source,
            target,
            "i".len()
        )]
    );
}

#[test]
fn sem018_exact_messages_and_component_span() {
    let source = "vector v; default { state_entry() { v.w = 1; } }";
    let component = source.find("w =").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM018",
            "component 'w' is not valid for variable 'v' of type vector".to_owned(),
            source,
            component,
            "w".len()
        )]
    );

    let source = "vector v; default { state_entry() { v.x = \"s\"; } }";
    let component = source.find("x =").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM018",
            "assignment to component 'v.x' yields type string, expected float".to_owned(),
            source,
            component,
            "x".len()
        )]
    );

    let source = "vector v; default { state_entry() { v.x += \"s\"; } }";
    let component = source.find("x +=").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM018",
            "compound assignment '+=' is not supported for component types float and string"
                .to_owned(),
            source,
            component,
            "x".len()
        )]
    );
}

#[test]
fn sem019_exact_read_message_and_component_span() {
    let source = "vector v; default { state_entry() { float f = v.w; } }";
    let component = source.find("w;").unwrap();
    assert_eq!(
        migrated(&semantic(source))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected(
            "SEM019",
            "component access '.w' is not valid for variable 'v' of type vector".to_owned(),
            source,
            component,
            "w".len()
        )]
    );
}

#[test]
fn sem014_full_cast_matrix_matches_legacy() {
    let literals = [
        ("integer", "1"),
        ("float", "1.0"),
        ("string", "\"s\""),
        ("key", "\"k\""),
        ("list", "[]"),
        ("vector", "<1, 2, 3>"),
        ("rotation", "<1, 2, 3, 4>"),
    ];
    for (target, _) in literals {
        for (_, literal) in literals {
            let source = format!("default {{ state_entry() {{ ({target}){literal}; }} }}");
            assert_parity(&source);
        }
    }
}

#[test]
fn firestorm_switch_initializers_are_validated_without_speculation() {
    use neolint::ast::{parse_ast_with_options, AstParseOptions};
    let source = r#"integer f() { integer x = "s"; switch (1) { case 1: { integer y = "s"; } } return 1; } default { state_entry() { } }"#;
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) =
        parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = analyze_ast(&ast, &syntax);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM015")
            .count(),
        2,
        "{diagnostics:?}"
    );
}
