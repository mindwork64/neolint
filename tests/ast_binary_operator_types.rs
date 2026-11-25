use neolint::ast::parse_ast;
use neolint::ast_semantic::analyze_ast_with_environment;
use neolint::diagnostic::{Diagnostic, DiagnosticKind, Severity};
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

fn sem046(source: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM046")
        .collect()
}

#[test]
fn reports_known_incompatible_arithmetic_operands() {
    let source = r#"
        default {
            state_entry() {
                integer i = 1;
                float f = 1.0;
                string s = "s";
                vector v = <1, 2, 3>;
                rotation r = ZERO_ROTATION;
                integer bad_add = s + i;
                integer bad_subtract = s - i;
                integer bad_multiply = s * i;
                vector bad_divide = v / v;
                integer bad_remainder = f % i;
                integer bad_rotation_order = r * v;
            }
        }
    "#;
    let diagnostics = sem046(source);

    assert_eq!(diagnostics.len(), 6, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == Severity::Error
            && diagnostic.kind == DiagnosticKind::Semantic
            && diagnostic
                .message
                .contains("does not support operand types")
    }));

    for operator in ["+", "-", "*", "/", "%"] {
        let start = source.find(operator).unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.span.start == start),
            "missing operator diagnostic for {operator}: {diagnostics:?}"
        );
    }
    let rotation_order = source.find("r * v").unwrap() + 2;
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span.start == rotation_order),
        "missing diagnostic for rotation * vector: {diagnostics:?}"
    );
}

#[test]
fn reports_known_incompatible_comparison_operands() {
    let source = r#"
        default {
            state_entry() {
                integer i = 1;
                float f = 1.0;
                string s = "s";
                vector v = <1, 2, 3>;
                integer bad_equal = i == s;
                integer bad_not_equal = v != f;
                integer bad_less = s < s;
                integer bad_less_equal = v <= v;
                integer bad_greater = i > v;
                integer bad_greater_equal = v >= i;
            }
        }
    "#;
    let diagnostics = sem046(source);

    assert_eq!(diagnostics.len(), 6, "{diagnostics:?}");
    for operator in ["==", "!=", "<", "<=", ">", ">="] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains(&format!("operator '{operator}'"))),
            "missing diagnostic for {operator}: {diagnostics:?}"
        );
    }
}

#[test]
fn accepts_confirmed_numeric_string_list_vector_rotation_and_equality_rows() {
    let diagnostics = sem046(
        r#"
        default {
            state_entry() {
                integer i = 1;
                float f = 1.0;
                string s = "s";
                key k = "k";
                list l = [1];
                vector v = <1, 2, 3>;
                rotation r = ZERO_ROTATION;
                integer a = i + i;
                float b = i + f;
                string c = s + s;
                list d = l + i;
                list e = i + l;
                vector g = v + v;
                rotation h = r + r;
                vector j = v - v;
                rotation m = r - r;
                float n = v * v;
                rotation p = r * r;
                vector q = v * r;
                vector t = f * v;
                vector u = v * f;
                vector w = v / f;
                rotation x = r / r;
                integer y = i % i;
                vector z = v % v;
                integer eq1 = i == f;
                integer eq2 = s == k;
                integer eq3 = l == l;
                integer lt = i < f;
            }
        }
        "#,
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn keeps_existing_logical_bitwise_and_shift_checks_in_sem045() {
    let diagnostics = analyze(
        "default { state_entry() { float f; integer i; integer a = f && i; integer b = i | f; integer c = i << f; } }",
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM045")
            .count(),
        3,
        "{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM046"),
        "{diagnostics:?}"
    );
}

#[test]
fn unknown_operand_types_are_not_guessed() {
    let diagnostics = sem046(
        "default { state_entry() { integer a = unknown + 1; integer b = llUnknownExtension() - 1; } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn parser_errors_and_unreliable_environments_suppress_the_rule() {
    let source = "default { state_entry() { integer bad = 1 + \"x\"; integer broken = ; } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(!syntax.is_empty());
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, true);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM046"),
        "{diagnostics:?}"
    );

    let (tokens, lexical) = lex("default { state_entry() { integer bad = 1 + \"x\"; } }");
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, false);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM046"),
        "{diagnostics:?}"
    );
}
