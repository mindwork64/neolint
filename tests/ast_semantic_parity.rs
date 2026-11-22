use neolint::ast::parse_ast;
use neolint::ast_semantic::analyze_ast;
use neolint::semantic::analyze_with_syntax_diagnostics;
use neolint::{lex, parse_legacy, Diagnostic};
use std::fs;
use std::path::{Path, PathBuf};

const MIGRATED: &[&str] = &[
    "SEM001", "SEM002", "SEM003", "SEM004", "SEM005", "SEM006", "SEM007", "SEM020", "SEM021",
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
fn corpus_nine_migrated_rules_match_legacy_exactly() {
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
