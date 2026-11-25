use neolint::ast::parse_ast;
use neolint::ast_semantic::analyze_ast_with_environment;
use neolint::diagnostic::Diagnostic;
use neolint::lex;

fn analyze(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical) = lex(source);
    assert!(
        lexical.is_empty(),
        "unexpected lexical diagnostics: {lexical:?}"
    );
    let (ast, syntax) = parse_ast(&tokens);
    analyze_ast_with_environment(&ast, &syntax, true)
}

fn sem042(source: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM042")
        .collect()
}

#[test]
fn warns_when_detection_functions_are_called_from_regular_events() {
    let diagnostics =
        sem042("default { state_entry() { llDetectedKey(0); } timer() { llDetectedName(0); } }");

    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics[0].message.contains("llDetectedKey"));
    assert!(diagnostics[1].message.contains("llDetectedName"));
}

#[test]
fn accepts_general_detection_functions_in_detection_and_damage_events() {
    let diagnostics = sem042(
        "default { \
            collision_start(integer n) { llDetectedKey(0); llDetectedLinkNumber(0); } \
            collision(integer n) { llDetectedName(0); } \
            collision_end(integer n) { llDetectedPos(0); } \
            touch_start(integer n) { llDetectedOwner(0); llDetectedLinkNumber(0); llDetectedTouchFace(0); llDetectedGrab(0); } \
            touch(integer n) { llDetectedType(0); } \
            touch_end(integer n) { llDetectedVel(0); } \
            sensor(integer n) { llDetectedRezzer(0); } \
            on_damage(integer n) { llDetectedKey(0); llDetectedDamage(0); llAdjustDamage(0, 1.0); } \
            final_damage(integer n) { llDetectedKey(0); llDetectedDamage(0); } \
        }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn reports_function_specific_event_restrictions() {
    let diagnostics = sem042(
        "default { \
            collision_start(integer n) { llDetectedGrab(0); } \
            sensor(integer n) { llDetectedTouchFace(0); } \
            on_damage(integer n) { llDetectedLinkNumber(0); } \
            touch_start(integer n) { llDetectedDamage(0); } \
            final_damage(integer n) { llAdjustDamage(0, 1.0); } \
        }",
    );

    assert_eq!(diagnostics.len(), 5, "{diagnostics:?}");
    let messages: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert!(messages
        .iter()
        .any(|message| message.contains("llDetectedGrab")));
    assert!(messages
        .iter()
        .any(|message| message.contains("llDetectedTouchFace")));
    assert!(messages
        .iter()
        .any(|message| message.contains("llDetectedLinkNumber")));
    assert!(messages
        .iter()
        .any(|message| message.contains("llDetectedDamage")));
    assert!(messages
        .iter()
        .any(|message| message.contains("llAdjustDamage")));
}

#[test]
fn propagates_detection_context_through_nested_user_functions() {
    let diagnostics = sem042(
        "read_detection() { llDetectedKey(0); } \
         relay() { read_detection(); } \
         default { touch_start(integer n) { relay(); } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn warns_when_a_helper_is_reachable_from_both_valid_and_invalid_events() {
    let diagnostics = sem042(
        "read_detection() { llDetectedKey(0); } \
         default { \
             touch_start(integer n) { read_detection(); } \
             state_entry() { read_detection(); } \
         }",
    );

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("llDetectedKey"));
}

#[test]
fn recursive_helpers_converge_without_losing_event_contexts() {
    let diagnostics = sem042(
        "first() { second(); } \
         second() { first(); llDetectedKey(0); } \
         default { \
             touch_start(integer n) { first(); } \
             state_entry() { first(); } \
         }",
    );

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("llDetectedKey"));
}

#[test]
fn ignores_helpers_reachable_only_from_unknown_extension_events() {
    let diagnostics = sem042(
        "read_detection() { llDetectedKey(0); } \
         default { custom_extension_event() { read_detection(); } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn ignores_unreferenced_helpers_but_checks_global_initializers() {
    let unreferenced = sem042(
        "unused_helper() { llDetectedKey(0); } \
         default { state_entry() { } }",
    );
    assert!(unreferenced.is_empty(), "{unreferenced:?}");

    let initialized = sem042(
        "key detected = llDetectedKey(0); \
         default { state_entry() { } }",
    );
    assert_eq!(initialized.len(), 1, "{initialized:?}");
}

#[test]
fn suppresses_context_analysis_when_the_ast_has_errors_or_environment_is_unreliable() {
    let source = "helper() { llDetectedKey(0); } default { state_entry() { helper( ; } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(!syntax.is_empty());
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, true);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "SEM042"));

    let (tokens, lexical) = lex("default { state_entry() { llDetectedKey(0); } }");
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, false);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "SEM042"));
}
