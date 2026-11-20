use neolint::{analyze, lex, parse, Diagnostic};

fn lex_and_parse(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical_diagnostics) = lex(source);
    assert!(
        lexical_diagnostics.is_empty(),
        "unexpected lexical diagnostics: {lexical_diagnostics:?}"
    );
    parse(&tokens)
}

fn analyze_source(source: &str) -> Vec<Diagnostic> {
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

#[test]
fn event_handlers_accept_named_and_default_state_changes() {
    let diagnostics = analyze_source(
        "default { \
             state_entry() { state running; } \
             touch_start(integer count) { state default; } \
         } \
         state running { state_entry() { state default; } }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn function_state_changes_are_currently_syntax_only() {
    let diagnostics = analyze_source(
        "integer first() { state running; return 1; } \
         integer second() { state default; return 2; } \
         default { state_entry() { } } \
         state running { state_entry() { } }",
    );
    assert!(
        diagnostics.is_empty(),
        "no context diagnostic is implemented yet: {diagnostics:?}"
    );
}

#[test]
fn nested_and_conditional_state_changes_are_syntax_only() {
    let diagnostics = analyze_source(
        "integer run(integer condition) { \
             { state running; } \
             if (condition) state running; \
             if (condition) state running; else state default; \
             while (condition) state running; \
             for (; condition; ) state running; \
             return 1; \
         } \
         default { state_entry() { } } \
         state running { state_entry() { } }",
    );
    assert!(
        diagnostics.is_empty(),
        "parser accepts these forms and no context diagnostic is implemented: {diagnostics:?}"
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
