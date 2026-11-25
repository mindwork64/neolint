//! SEM038–SEM040 regression coverage for state and list semantics.

use neolint::ast::{parse_ast_with_options, AstParseOptions};
use neolint::{lex, Diagnostic, DiagnosticKind, Severity};

fn analyze(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    neolint::ast_semantic::analyze_ast(&ast, &syntax)
}

fn analyze_with_environment(source: &str, reliable: bool) -> Vec<Diagnostic> {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    neolint::ast_semantic::analyze_ast_with_environment(&ast, &syntax, reliable)
}

fn only(source: &str, code: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == code)
        .collect()
}

#[test]
fn sem038_reports_empty_default_and_named_states() {
    let source = "default {}\nstate dormant {}\nstate active { state_entry() {} }\n";
    let diagnostics = only(source, "SEM038");
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == Severity::Error
            && diagnostic.kind == DiagnosticKind::Semantic
            && diagnostic
                .message
                .contains("must contain at least one event handler")
    }));
    assert_eq!(diagnostics[0].span.start, source.find("default").unwrap());
    assert_eq!(diagnostics[1].span.start, source.find("dormant").unwrap());
}

#[test]
fn sem038_accepts_states_with_any_event_handler() {
    let source = "default { touch_start(integer count) {} }\nstate dormant { timer() {} }\n";
    assert!(only(source, "SEM038").is_empty());
}

#[test]
fn sem038_skips_malformed_states_and_unreliable_environments() {
    let malformed = "default { broken( {}\nstate okay { state_entry() {} }\n";
    let (tokens, lexical) = lex(malformed);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(!syntax.is_empty(), "fixture must be malformed");
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(diagnostics.iter().all(|item| item.code != "SEM038"));

    let source = "default {}\n";
    assert!(analyze_with_environment(source, false)
        .iter()
        .all(|item| item.code != "SEM038"));
}

#[test]
fn sem039_rejects_direct_nested_list_literals() {
    let source = "default { state_entry() { list nested = [[1]]; llOwnerSay((string)nested); } }\n";
    let diagnostics = only(source, "SEM039");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Semantic);
    assert_eq!(diagnostics[0].span.start, source.find("[1]").unwrap());
}

#[test]
fn sem039_rejects_list_variables_and_list_returning_calls_as_elements() {
    let source = r#"
        list source = [1];
        list make_list() { return []; }
        default {
            state_entry() {
                list nested = [source, make_list()];
                llOwnerSay((string)nested);
            }
        }
    "#;
    let diagnostics = only(source, "SEM039");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics[0].span.start < diagnostics[1].span.start);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.severity == Severity::Error));
}

#[test]
fn sem039_accepts_flat_lists_and_unknown_element_types() {
    let source = r#"
        default {
            state_entry() {
                integer unknown;
                list flat = [1, 2.0, "three", <0,0,0>, unknown_extension()];
                llOwnerSay((string)flat);
            }
        }
    "#;
    assert!(only(source, "SEM039").is_empty());
}

#[test]
fn sem039_reports_list_concatenation_when_used_as_an_element() {
    let source = r#"
        list left = [1];
        list right = [2];
        default {
            state_entry() {
                list nested = [left + right];
                llOwnerSay((string)nested);
            }
        }
    "#;
    assert_eq!(only(source, "SEM039").len(), 1);
}

#[test]
fn sem040_warns_for_list_equality_and_inequality_of_known_list_types() {
    let source = r#"
        list left = [1, 2];
        list right = [3, 4];
        default {
            state_entry() {
                if (left == right) llOwnerSay("same length");
                if (left != []) llOwnerSay("different length");
            }
        }
    "#;
    let diagnostics = only(source, "SEM040");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == Severity::Warning && diagnostic.kind == DiagnosticKind::Lint
    }));
    assert!(diagnostics[0].message.contains("compares lengths only"));
    assert!(diagnostics[1]
        .message
        .contains("difference between list lengths"));
    assert!(diagnostics[0].span.start < diagnostics[1].span.start);
}

#[test]
fn sem040_infers_lists_from_function_calls_and_parenthesized_expressions() {
    let source = r#"
        list get_list() { return []; }
        default {
            state_entry() {
                if ((get_list()) == []) llOwnerSay("same length");
            }
        }
    "#;
    assert_eq!(only(source, "SEM040").len(), 1);
}

#[test]
fn sem040_does_not_warn_for_unresolved_or_non_equality_comparisons() {
    let source = r#"
        default {
            state_entry() {
                if (unknown_a == unknown_b) llOwnerSay("unknown");
                if (1 < 2) llOwnerSay("numeric");
            }
        }
    "#;
    assert!(only(source, "SEM040").is_empty());
}

#[test]
fn sem039_and_sem040_fail_closed_for_unreliable_environments() {
    let source = r#"
        list global_list = [1];
        default {
            state_entry() {
                list nested = [global_list];
                if (global_list == []) llOwnerSay("check");
            }
        }
    "#;
    let diagnostics = analyze_with_environment(source, false);
    assert!(diagnostics
        .iter()
        .all(|item| !matches!(item.code, "SEM039" | "SEM040")));
}

#[test]
fn sem038_skips_syntax_diagnostics_inside_a_complete_empty_state() {
    let source = "default { @; }\n";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(!syntax.is_empty(), "fixture must have a parser diagnostic");
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM038"),
        "{diagnostics:?}"
    );
}

#[test]
fn sem039_and_sem040_skip_syntax_damaged_expressions() {
    let source = r#"
        list child = [1];
        default {
            state_entry() {
                list nested = [child,];
                if (child == ) llOwnerSay("damaged");
            }
        }
    "#;
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(!syntax.is_empty(), "fixture must have parser diagnostics");
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "SEM039" | "SEM040")),
        "{diagnostics:?}"
    );
}

#[test]
fn sem039_fails_closed_when_its_work_budget_is_exceeded() {
    let mut source = String::from("list child = []; default { state_entry() { list nested = [");
    for index in 0..32_768 {
        if index != 0 {
            source.push(',');
        }
        source.push_str("child");
    }
    source.push_str("]; llOwnerSay((string)nested); } }");

    let (tokens, lexical) = lex(&source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM039"),
        "the rule must suppress partial output on budget overflow: {diagnostics:?}"
    );
}
