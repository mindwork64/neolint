use neolint::ast::parse_ast;
use neolint::ast_semantic::analyze_ast_with_environment;
use neolint::diagnostic::{Diagnostic, DiagnosticKind, Severity};
use neolint::lex;

fn analyze(source: &str, reliable: bool) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(
        lexical.is_empty(),
        "unexpected lexical diagnostics: {lexical:?}"
    );
    let (ast, syntax) = parse_ast(&tokens);
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, reliable);
    (diagnostics, syntax)
}

fn sem044(source: &str) -> Vec<Diagnostic> {
    analyze(source, true)
        .0
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM044")
        .collect()
}

#[test]
fn warns_on_populated_states_without_a_path_from_default() {
    let source = "default { state_entry() { state active; } } \
                  state active { state_entry() { state finished; } } \
                  state finished { state_entry() { } } \
                  state abandoned { state_entry() { } }";
    let diagnostics = sem044(source);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "SEM044");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Lint);
    assert_eq!(
        &source[diagnostics[0].span.start..diagnostics[0].span.end],
        "abandoned"
    );
}

#[test]
fn reachability_is_transitive_and_transitions_back_to_default_are_valid() {
    let source = "default { state_entry() { state first; } } \
                  state first { state_entry() { state second; } } \
                  state second { state_entry() { state default; } }";
    assert!(sem044(source).is_empty());
}

#[test]
fn transitions_in_nested_and_conditional_bodies_are_over_approximated() {
    let source = "default { state_entry() { if (FALSE) { while (TRUE) { state nested; } } } } \
                  state nested { state_entry() { } }";
    assert!(sem044(source).is_empty());
}

#[test]
fn transitions_after_return_are_still_counted_conservatively() {
    let source = "default { state_entry() { return; state later; } } \
                  state later { state_entry() { } }";
    assert!(sem044(source).is_empty());
}

#[test]
fn unknown_state_targets_do_not_create_graph_edges() {
    let source = "default { state_entry() { state missing; } } \
                  state never_entered { state_entry() { } }";
    let diagnostics = sem044(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics[0].message,
        "state 'never_entered' has no transition path from the default state"
    );
}

#[test]
fn empty_states_remain_owned_by_sem038_without_a_duplicate_warning() {
    let source = "default { state_entry() { } } state empty_state { }";
    let diagnostics = analyze(source, true).0;
    assert!(diagnostics.iter().any(|item| item.code == "SEM038"));
    assert!(diagnostics.iter().all(|item| item.code != "SEM044"));
}

#[test]
fn state_changes_inside_global_functions_do_not_make_states_reachable() {
    let source = "change_state() { state target; } \
                  default { state_entry() { change_state(); } } \
                  state target { state_entry() { } }";
    let diagnostics = analyze(source, true).0;
    assert!(diagnostics.iter().any(|item| item.code == "SEM034"));
    assert!(diagnostics.iter().any(|item| item.code == "SEM044"));
}

#[test]
fn custom_event_bodies_are_included_as_possible_transition_sources() {
    let source = "default { custom_event() { state extension_target; } } \
                  state extension_target { custom_event() { } }";
    assert!(sem044(source).is_empty());
}

#[test]
fn duplicate_states_and_parser_errors_suppress_the_whole_graph() {
    let duplicate = "default { state_entry() { } } state same { state_entry() { } } state same { state_entry() { } }";
    assert!(sem044(duplicate).is_empty());

    let malformed =
        "default { state_entry() { } } state orphan { state_entry() { state missing } }";
    let (diagnostics, syntax) = analyze(malformed, true);
    assert!(!syntax.is_empty());
    assert!(diagnostics.iter().all(|item| item.code != "SEM044"));
}

#[test]
fn state_namespace_conflicts_suppress_secondary_reachability_warnings() {
    let source = "integer running; helper() { } default { state_entry() { } } \
                  state running { state_entry() { } } state helper { state_entry() { } }";
    let diagnostics = analyze(source, true).0;
    assert!(diagnostics.iter().any(|item| item.code == "SEM021"));
    assert!(diagnostics.iter().all(|item| item.code != "SEM044"));

    let builtin = "default { state_entry() { } } state llOwnerSay { state_entry() { } }";
    let diagnostics = analyze(builtin, true).0;
    assert!(diagnostics.iter().any(|item| item.code == "SEM021"));
    assert!(diagnostics.iter().all(|item| item.code != "SEM044"));

    // SEM021 deliberately permits these three god-only built-in collisions.
    let god_only = "default { state_entry() { } } state llGodLikeRezObject { state_entry() { } }";
    let diagnostics = analyze(god_only, true).0;
    assert!(diagnostics.iter().all(|item| item.code != "SEM021"));
    assert!(diagnostics.iter().any(|item| item.code == "SEM044"));
}

#[test]
fn unreliable_or_missing_default_context_suppresses_the_rule() {
    let source = "default { state_entry() { } } state orphan { state_entry() { } }";
    assert!(analyze(source, false)
        .0
        .iter()
        .all(|item| item.code != "SEM044"));
    assert!(sem044("state orphan { state_entry() { } }").is_empty());
}

#[test]
fn graph_work_and_depth_budget_exhaustion_suppress_partial_findings() {
    let mut wide = String::from("default { state_entry() { ");
    wide.push_str(&";".repeat(32_770));
    wide.push_str(" } } state orphan { state_entry() { } }");
    assert!(sem044(&wide).is_empty());

    let open = "{".repeat(130);
    let close = "}".repeat(130);
    let deep = format!(
        "default {{ state_entry() {{ {open}state orphan; {close} }} }} state orphan {{ state_entry() {{ }} }}"
    );
    assert!(sem044(&deep).is_empty());
}
