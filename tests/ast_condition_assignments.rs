//! SEM041 regression coverage for accidental assignments in `if` conditions.

use neolint::ast::{parse_ast_with_options, AstParseOptions};
use neolint::{lex, Diagnostic, DiagnosticKind, Severity};

fn analyze(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    neolint::ast_semantic::analyze_ast(&ast, &syntax)
}

fn only(source: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM041")
        .collect()
}

#[test]
fn sem041_warns_on_a_bare_plain_assignment_in_if_condition() {
    let source = r#"
        key target;
        default {
            state_entry() {
                if (target = llGetOwner()) { llOwnerSay("matched"); }
            }
        }
    "#;
    let diagnostics = only(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Lint);
    assert_eq!(
        diagnostics[0].span.start,
        source.find("= llGetOwner").unwrap()
    );
    assert!(diagnostics[0].message.contains("use '==' for comparison"));
}

#[test]
fn sem041_parentheses_mark_an_assignment_as_intentional() {
    let source = r#"
        key target;
        default {
            state_entry() {
                if ((target = llGetOwner())) { llOwnerSay("assigned"); }
            }
        }
    "#;
    assert!(only(source).is_empty());
}

#[test]
fn sem041_ignores_comparisons_and_compound_assignments() {
    let source = r#"
        key target;
        integer counter;
        default {
            state_entry() {
                if (target == llGetOwner()) { }
                if (counter += 1) { }
            }
        }
    "#;
    assert!(only(source).is_empty());
}

#[test]
fn sem041_finds_nested_and_function_if_conditions_in_source_order() {
    let source = r#"
        integer value;
        check() {
            if (value = 1) { }
        }
        default {
            state_entry() {
                if (value = 2) {
                    if (value = 3) { }
                } else if (value = 4) { }
                while (value < 10) {
                    if (value = 5) { }
                    value++;
                }
            }
        }
    "#;
    let diagnostics = only(source);
    assert_eq!(diagnostics.len(), 5, "{diagnostics:?}");
    let starts: Vec<_> = diagnostics.iter().map(|item| item.span.start).collect();
    assert!(
        starts.windows(2).all(|pair| pair[0] < pair[1]),
        "{starts:?}"
    );
}

#[test]
fn sem041_does_not_report_assignments_nested_inside_other_expressions() {
    let source = r#"
        integer value;
        default {
            state_entry() {
                if ((value = 1) == 1) { }
                if ((value = 2) || TRUE) { }
            }
        }
    "#;
    assert!(only(source).is_empty());
}

#[test]
fn sem041_skips_malformed_conditions() {
    let source = "integer value; default { state_entry() { if (value = ) { } } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "fixture must contain a parser diagnostic"
    );
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(diagnostics.iter().all(|item| item.code != "SEM041"));
}

#[test]
fn sem041_skips_unreliable_preprocessor_or_lexical_environments() {
    let source = "integer value; default { state_entry() { if (value = 1) { } } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = neolint::ast_semantic::analyze_ast_with_environment(&ast, &syntax, false);
    assert!(diagnostics.iter().all(|item| item.code != "SEM041"));
}
