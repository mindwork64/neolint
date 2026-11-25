//! SEM027 regression coverage for compile-time global initializer values.

use neolint::ast::{parse_ast_with_options, AstParseOptions};
use neolint::{lex, Diagnostic, DiagnosticKind, Severity, Span};

fn analyze(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    neolint::ast_semantic::analyze_ast(&ast, &syntax)
}

fn sem027(source: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM027")
        .collect()
}

fn expected(source: &str, expression: &str, variable: &str) -> Diagnostic {
    let start = source.find(expression).unwrap();
    let end = start + expression.len();
    let line = source[..start].matches('\n').count() + 1;
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let end_line = source[..end].matches('\n').count() + 1;
    let end_line_start = source[..end].rfind('\n').map_or(0, |index| index + 1);
    Diagnostic {
        code: "SEM027",
        severity: Severity::Error,
        kind: DiagnosticKind::Semantic,
        message: format!("global initializer for variable '{variable}' must be a simple value, not an expression"),
        span: Span {
            start,
            end,
            line,
            column: source[line_start..start].chars().count() + 1,
            end_line,
            end_column: source[end_line_start..end].chars().count() + 1,
        },
    }
}

#[test]
fn accepts_literals_builtin_constants_and_simple_composite_values() {
    let source = r#"
        integer count = 42, negative = -7, flag = TRUE;
        float ratio = -PI;
        string label = "hello";
        key owner = NULL_KEY;
        vector origin = ZERO_VECTOR;
        rotation orientation = ZERO_ROTATION;
        vector point = <1.0, -2.0, PI>;
        rotation turn = <0.0, 0.0, 0.0, 1.0>;
        list values = [1, -2, "text", TRUE, <1.0, 2.0, 3.0>, <0.0, 0.0, 0.0, 1.0>];
        list empty = [];
        default { state_entry() { } }
    "#;
    assert!(sem027(source).is_empty());
}

#[test]
fn comma_separated_global_declarators_are_checked_independently() {
    let source =
        "integer first = 1, second = 2 + 3, third = llGetTime();\ndefault { state_entry() { } }\n";
    assert_eq!(
        sem027(source),
        vec![
            expected(source, "2 + 3", "second"),
            expected(source, "llGetTime()", "third"),
        ]
    );
}

#[test]
fn global_references_must_be_visible_at_the_initializer() {
    let source = "integer before = later;\ninteger later = 1;\ndefault { state_entry() { } }\n";
    assert_eq!(sem027(source), vec![expected(source, "later", "before")]);
}

#[test]
fn reports_arithmetic_and_function_calls_in_global_initializers() {
    let source = "integer calculated = 1 + 2;\nfloat sampled = llGetTime();\ndefault { state_entry() { } }\n";
    assert_eq!(
        sem027(source),
        vec![
            expected(source, "1 + 2", "calculated"),
            expected(source, "llGetTime()", "sampled"),
        ]
    );
}

#[test]
fn accepts_references_to_visible_global_variables_even_when_they_shadow_builtins() {
    let source = "integer seed = 5;\ninteger copied = seed;\ninteger PI = 3;\nfloat shadowed = PI;\ndefault { state_entry() { } }\n";
    assert!(sem027(source).is_empty());
}

#[test]
fn composite_and_list_elements_must_also_be_simple_constants() {
    let source = "vector point = <1 + 2, 0.0, 0.0>;\nlist values = [1, 2 * 3];\nlist nested = [[1]];\ndefault { state_entry() { } }\n";
    assert_eq!(
        sem027(source),
        vec![
            expected(source, "<1 + 2, 0.0, 0.0>", "point"),
            expected(source, "[1, 2 * 3]", "values"),
            expected(source, "[[1]]", "nested"),
        ]
    );
}

#[test]
fn vector_and_rotation_components_must_be_numeric_constants() {
    let source = "vector bad_string = <\"text\", 0, 0>;\nvector bad_vector = <ZERO_VECTOR, 0, 0>;\nvector good = <TRUE, -2, PI>;\ndefault { state_entry() { } }\n";
    assert_eq!(
        sem027(source),
        vec![
            expected(source, "<\"text\", 0, 0>", "bad_string"),
            expected(source, "<ZERO_VECTOR, 0, 0>", "bad_vector"),
        ]
    );
}

#[test]
fn parenthesized_values_are_not_simple_global_initializers() {
    let source = "integer parenthesized = (1);\ndefault { state_entry() { } }\n";
    assert_eq!(
        sem027(source),
        vec![expected(source, "(1)", "parenthesized")]
    );
}

#[test]
fn rejects_negative_boolean_but_accepts_negative_numeric_constants() {
    let source = "integer bad = -TRUE;\nfloat good = -PI;\ndefault { state_entry() { } }\n";
    assert_eq!(sem027(source), vec![expected(source, "-TRUE", "bad")]);
}

#[test]
fn local_initializers_are_outside_sem027() {
    let source = "integer global_value = 1;\ndefault { state_entry() { integer local_value = llGetTime(); } }\n";
    assert!(sem027(source).is_empty());
}

#[test]
fn nested_parser_errors_suppress_sem027_instead_of_adding_a_cascade() {
    let source = "vector malformed = <1 + 2, , 3>;\ndefault { state_entry() { } }\n";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "fixture must exercise nested parser recovery"
    );
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "SEM027"));
}

#[test]
fn unreliable_preprocessor_or_lexical_environment_suppresses_sem027() {
    let source = "float sampled = missing_global;\ndefault { state_entry() { } }\n";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = neolint::ast_semantic::analyze_ast_with_environment(&ast, &syntax, false);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "SEM027"));
}

#[test]
fn initializer_work_budget_fails_closed() {
    let values = std::iter::repeat("1")
        .take(140)
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("list values = [{values}];\ndefault {{ state_entry() {{ }} }}\n");
    assert!(sem027(&source).is_empty());
}

#[test]
fn missing_or_malformed_global_initializers_are_not_reclassified() {
    let source = "integer no_initializer;\ninteger malformed = ;\ndefault { state_entry() { } }\n";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
    assert!(!syntax.is_empty(), "fixture must exercise parser recovery");
    let diagnostics = neolint::ast_semantic::analyze_ast(&ast, &syntax);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "SEM027"));
}
