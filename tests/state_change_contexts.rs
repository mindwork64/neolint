use neolint::ast::{parse_ast_with_options, AstParseOptions};
use neolint::{analyze, lex, parse, Diagnostic};

fn lex_and_parse(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical_diagnostics) = lex(source);
    assert!(
        lexical_diagnostics.is_empty(),
        "unexpected lexical diagnostics: {lexical_diagnostics:?}"
    );
    parse(&tokens)
}

fn analyze_legacy_source(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical_diagnostics) = lex(source);
    assert!(
        lexical_diagnostics.is_empty(),
        "unexpected lexical diagnostics: {lexical_diagnostics:?}"
    );
    let syntax_diagnostics = parse(&tokens);
    assert!(
        syntax_diagnostics.is_empty(),
        "unexpected syntax diagnostics: {syntax_diagnostics:?}"
    );
    analyze(&tokens)
}

fn analyze_ast_source(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical_diagnostics) = lex(source);
    assert!(
        lexical_diagnostics.is_empty(),
        "unexpected lexical diagnostics: {lexical_diagnostics:?}"
    );
    let (ast, syntax_diagnostics) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(
        syntax_diagnostics.is_empty(),
        "unexpected syntax diagnostics: {syntax_diagnostics:?}"
    );
    neolint::ast_semantic::analyze_ast(&ast, &syntax_diagnostics)
}

fn state_context_diagnostics(source: &str) -> Vec<Diagnostic> {
    analyze_ast_source(source)
        .into_iter()
        .filter(|item| matches!(item.code, "SEM034" | "SEM035"))
        .collect()
}

#[test]
fn event_handlers_accept_named_and_default_state_changes() {
    let source = "default { \
        state_entry() { state running; } \
        touch_start(integer count) { state default; } \
    } \
    state running { state_entry() { state default; } }";
    assert!(analyze_legacy_source(source).is_empty());
    assert!(state_context_diagnostics(source).is_empty());
}

#[test]
fn legacy_engine_keeps_function_state_changes_out_of_its_diagnostic_stream() {
    let diagnostics = analyze_legacy_source(
        "integer first() { state running; return 1; } \
         integer second() { state default; return 2; } \
         default { state_entry() { } } \
         state running { state_entry() { } }",
    );
    assert!(
        diagnostics.is_empty(),
        "the Legacy compatibility stream must remain unchanged: {diagnostics:?}"
    );
}

#[test]
fn ast_engine_rejects_direct_global_function_state_changes() {
    let source = "integer first() { state running; return 1; } \
                  integer second() { state default; return 2; } \
                  default { state_entry() { } } \
                  state running { state_entry() { } }";
    let diagnostics = state_context_diagnostics(source);
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|item| item.code == "SEM034"));
    assert!(diagnostics
        .iter()
        .all(|item| item.severity == neolint::Severity::Error));
    assert!(diagnostics
        .iter()
        .all(|item| item.kind == neolint::DiagnosticKind::Semantic));
    assert!(diagnostics[0]
        .message
        .contains("global functions cannot change script state on Mono"));
    assert!(diagnostics[0].span.start > source.find("state running").unwrap());
    assert!(diagnostics[1].span.start > source.find("state default").unwrap());
}

#[test]
fn mono_if_without_else_compatibility_form_is_warned_not_accepted_as_portable() {
    let source = "integer change(integer condition) { \
        if (condition) state running; \
        state default; \
        return 1; \
    } \
    default { state_entry() { } } \
    state running { state_entry() { } }";
    let diagnostics = state_context_diagnostics(source);
    assert_eq!(
        diagnostics.iter().map(|item| item.code).collect::<Vec<_>>(),
        vec!["SEM035", "SEM034"],
        "{diagnostics:?}"
    );
    assert_eq!(diagnostics[0].severity, neolint::Severity::Warning);
    assert_eq!(diagnostics[0].kind, neolint::DiagnosticKind::Lint);
    assert!(diagnostics[0]
        .message
        .contains("unsupported Mono if-without-else workaround"));
    assert_eq!(diagnostics[1].severity, neolint::Severity::Error);
}

#[test]
fn only_a_direct_if_then_statement_without_else_uses_the_compatibility_warning() {
    let source = "integer run(integer condition) { \
        { state running; } \
        if (condition) { state running; } \
        if (condition) state running; else state default; \
        while (condition) state running; \
        for (; condition; ) state running; \
        return 1; \
    } \
    default { state_entry() { } } \
    state running { state_entry() { } }";
    let diagnostics = state_context_diagnostics(source);
    assert_eq!(diagnostics.len(), 6, "{diagnostics:?}");
    assert!(
        diagnostics.iter().all(|item| item.code == "SEM034"),
        "{diagnostics:?}"
    );
}

#[test]
fn a_function_called_from_an_event_is_still_a_global_function() {
    let source = "change() { state running; } \
                  default { state_entry() { change(); } } \
                  state running { state_entry() { } }";
    let diagnostics = state_context_diagnostics(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "SEM034");
}

#[test]
fn unclosed_functions_and_statement_syntax_errors_are_suppressed() {
    let unclosed =
        "change() { state running; default { state_entry() { } } state running { state_entry() { } }";
    let (tokens, lexical) = lex(unclosed);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(!syntax.is_empty());
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(
        diagnostics
            .iter()
            .all(|item| !matches!(item.code, "SEM034" | "SEM035")),
        "{diagnostics:?}"
    );

    let malformed =
        "change() { state running } default { state_entry() { } } state running { state_entry() { } }";
    let (tokens, lexical) = lex(malformed);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(!syntax.is_empty());
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(
        diagnostics
            .iter()
            .all(|item| !matches!(item.code, "SEM034" | "SEM035")),
        "{diagnostics:?}"
    );
}

#[test]
fn unreliable_environment_suppresses_context_diagnostics() {
    let source = "change() { state running; } default { state_entry() { } } state running { state_entry() { } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty());
    let diagnostics = neolint::ast_semantic::analyze_ast_with_environment(&ast, &syntax, false);
    assert!(
        diagnostics
            .iter()
            .all(|item| !matches!(item.code, "SEM034" | "SEM035")),
        "{diagnostics:?}"
    );
}

#[test]
fn state_change_without_target_is_still_a_syntax_error() {
    let diagnostics = lex_and_parse("default { state_entry() { state ; } }");
    assert!(
        diagnostics.iter().any(|item| item.code == "PAR007"),
        "{diagnostics:?}"
    );
}
