//! Semantic diagnostics for the opt-in Firestorm switch extension.

use neolint::ast::{parse_ast_with_options, AstParseOptions};
use neolint::{lex, Diagnostic, DiagnosticKind, Severity};

fn parse(source: &str) -> (neolint::ast::Ast, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true))
}

fn analyze(source: &str) -> Vec<Diagnostic> {
    let (ast, syntax) = parse(source);
    assert!(syntax.is_empty(), "{syntax:?}");
    neolint::ast_semantic::analyze_ast(&ast, &syntax)
}

fn diagnostics_for(source: &str, code: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == code)
        .collect()
}

#[test]
fn duplicate_defaults_are_an_error_at_the_second_default_label() {
    let source = "default { state_entry() { switch (value) { case 1: ; default: ; default: ; } } }";
    let diagnostics = diagnostics_for(source, "SEM028");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Semantic);
    let second = source.rfind("default").unwrap();
    assert_eq!(diagnostics[0].span.start, second);
    assert_eq!(diagnostics[0].span.end, second + "default".len());
    assert!(diagnostics[0].message.contains("more than one default"));
}

#[test]
fn known_incompatible_case_types_are_errors() {
    let source = r#"default { state_entry() {
        switch (1) { case "text": ; case <0, 0, 0>: ; default: ; }
    } }"#;
    let diagnostics = diagnostics_for(source, "SEM029");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.severity == Severity::Error));
    assert!(diagnostics[0].message.contains("string"));
    assert!(diagnostics[0].message.contains("integer"));
    assert!(diagnostics[1].message.contains("vector"));
    assert!(diagnostics[1].message.contains("integer"));
}

#[test]
fn duplicate_constant_case_values_are_warnings() {
    let source = "default { state_entry() { switch (value) { case 1: ; case 0x1: ; case TRUE: ; default: ; } } }";
    let diagnostics = diagnostics_for(source, "SEM030");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.severity == Severity::Warning));
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.kind == DiagnosticKind::Lint));
    assert!(diagnostics[0].message.contains("first used at"));
}

#[test]
fn string_truthiness_is_used_when_folding_case_values() {
    let source =
        "default { state_entry() { switch (value) { case !\"\": ; case TRUE: ; default: ; } } }";
    assert_eq!(diagnostics_for(source, "SEM030").len(), 1);
}

#[test]
fn duplicate_case_values_are_compared_across_numeric_types() {
    let source =
        "default { state_entry() { switch (value) { case 1: ; case 1.0: ; default: ; } } }";
    assert_eq!(diagnostics_for(source, "SEM030").len(), 1);
}

#[test]
fn expressions_and_builtin_constants_are_folded_for_duplicate_cases() {
    let source = "default { state_entry() { switch (value) { case 1 + 2: ; case 3: ; case PI: ; case 3.14159265: ; default: ; } } }";
    let diagnostics = diagnostics_for(source, "SEM030");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
}

#[test]
fn missing_default_is_a_warning_not_a_syntax_error() {
    let source = "default { state_entry() { switch (value) { case 1: ; } } }";
    let diagnostics = diagnostics_for(source, "SEM031");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Lint);
    assert!(diagnostics[0].message.contains("no default"));
}

#[test]
fn constant_switch_expression_is_warned_once_per_switch() {
    let source = "default { state_entry() { switch (1 + 2) { case 3: ; default: ; } } }";
    let diagnostics = diagnostics_for(source, "SEM032");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Lint);
    let expression_start = source.find("1 + 2").unwrap();
    assert_eq!(diagnostics[0].span.start, expression_start);
}

#[test]
fn list_switch_warns_about_length_comparison_semantics() {
    let source = "default { state_entry() { list values = [1]; switch (values) { case [1, 2]: ; default: ; } } }";
    let diagnostics = diagnostics_for(source, "SEM033");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Lint);
    assert!(diagnostics[0].message.contains("list lengths"));
}

#[test]
fn nested_switches_are_analyzed_independently() {
    let source = "default { state_entry() { switch (outer) { case 1: switch (inner) { case 1: ; case 1.0: ; } default: ; } } }";
    assert_eq!(diagnostics_for(source, "SEM031").len(), 1);
    assert_eq!(diagnostics_for(source, "SEM030").len(), 1);
}

#[test]
fn switch_break_inside_conditional_and_loop_bodies_targets_the_switch() {
    let source = r#"
        integer from_if(integer value) {
            switch (value) {
                case 1: if (value) break; else return 1;
                default: return 2;
            }
            return 3;
        }
        integer from_loop(integer value) {
            switch (value) {
                case 1: while (value) { break; } return 1;
                default: return 2;
            }
        }
        default { state_entry() { } }
    "#;
    let diagnostics = analyze(source);
    let missing_returns: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "SEM011")
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert_eq!(
        missing_returns,
        vec!["function from_loop may reach the end without returning a value"]
    );
}

#[test]
fn malformed_switch_does_not_receive_cascading_semantic_diagnostics() {
    let source = "default { state_entry() { switch (value) { case : ; default: ; default: ; } switch (1) { case 1: ; } } }";
    let (ast, syntax) = parse(source);
    assert!(!syntax.is_empty(), "fixture must exercise parser recovery");
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(
        diagnostics.iter().all(|diagnostic| {
            !matches!(
                diagnostic.code,
                "SEM028" | "SEM029" | "SEM030" | "SEM031" | "SEM032" | "SEM033"
            ) || diagnostic.span.start >= source.find("switch (1)").unwrap()
        }),
        "{diagnostics:?}"
    );
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "SEM032"));
}

#[test]
fn unreliable_source_environment_suppresses_switch_semantics() {
    let source = "default { state_entry() { switch (1) { case 1: ; } } }";
    let (ast, syntax) = parse(source);
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = neolint::ast_semantic::analyze_ast_with_environment(&ast, &syntax, false);
    assert!(diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code,
            "SEM028" | "SEM029" | "SEM030" | "SEM031" | "SEM032" | "SEM033"
        )
    }));
}

#[test]
fn switch_checks_are_inactive_without_the_switch_feature_gate() {
    let source = "default { state_entry() { switch (1) { case 1: ; default: ; } } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code,
            "SEM028" | "SEM029" | "SEM030" | "SEM031" | "SEM032" | "SEM033"
        )
    }));
}
