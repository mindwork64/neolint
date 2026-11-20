use neolint::{analyze, lex, parse};

#[test]
fn representative_multistate_lsl_sample_passes_all_current_checks() {
    let source = include_str!("fixtures/hello.lsl");
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

    let semantic_diagnostics = analyze(&tokens);
    assert!(
        semantic_diagnostics.is_empty(),
        "unexpected semantic diagnostics: {semantic_diagnostics:?}"
    );
}

#[test]
fn state_change_to_default_passes_all_current_checks() {
    let source = "default { state_entry() { state running; } } \
                  state running { state_entry() { state default; } }";
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

    let semantic_diagnostics = analyze(&tokens);
    assert!(
        semantic_diagnostics.is_empty(),
        "unexpected semantic diagnostics: {semantic_diagnostics:?}"
    );
}
