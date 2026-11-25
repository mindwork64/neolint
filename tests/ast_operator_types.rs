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

fn sem045(source: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM045")
        .collect()
}

#[test]
fn reports_unary_operators_with_known_incompatible_operand_types() {
    let source = "default { state_entry() { float f; string s; key k; \
        integer a = !f; integer b = ~s; integer c = -k; } }";
    let diagnostics = sem045(source);

    assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == Severity::Error && diagnostic.kind == DiagnosticKind::Semantic
    }));

    for operator in ["!", "~", "-"] {
        let offset = source[source.find("integer a").unwrap()..]
            .find(operator)
            .map(|relative| source.find("integer a").unwrap() + relative)
            .unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.span.start == offset),
            "missing diagnostic for {operator}: {diagnostics:?}"
        );
    }
}

#[test]
fn accepts_all_supported_unary_operand_types() {
    let diagnostics = sem045(
        "default { state_entry() { \
            integer i = 1; float f = 1.0; vector v = <1, 2, 3>; rotation r = ZERO_ROTATION; \
            integer a = !i; integer b = ~i; vector d = -v; rotation e = -r; \
        } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn reports_non_integer_bitwise_and_shift_operands() {
    let diagnostics = sem045(
        "default { state_entry() { \
            integer i = 1; float f = 1.0; string s = \"s\"; \
            integer a = f & i; integer b = i | s; integer c = f ^ i; \
            integer d = i << f; integer e = i >> s; \
            integer logical_and = f && i; integer logical_or = i || s; \
        } }",
    );

    assert_eq!(diagnostics.len(), 7, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == Severity::Error
            && diagnostic.message.contains("requires integer operands")
    }));
}

#[test]
fn accepts_integer_bitwise_and_shift_operands() {
    let diagnostics = sem045(
        "default { state_entry() { integer i = 1; integer j = 2; \
            integer a = i & j; integer b = i | j; integer c = i ^ j; \
            integer d = i << j; integer e = i >> j; \
            integer logical_and = i && j; integer logical_or = i || j; } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn validates_increment_and_decrement_variable_types() {
    let diagnostics = sem045(
        "default { state_entry() { \
            integer i; float f; string s; vector v; \
            i++; ++f; f--; --i; s++; ++v; \
        } }",
    );

    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic
            .message
            .contains("requires an integer or float variable")
    }));
}

#[test]
fn does_not_allow_incrementing_builtin_constants() {
    let diagnostics = sem045("default { state_entry() { TRUE++; --FALSE; } }");

    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic
            .message
            .contains("cannot modify built-in constant")
    }));
}

#[test]
fn skips_unknown_types_instead_of_guessing() {
    let diagnostics = sem045(
        "default { state_entry() { \
            integer a = unknown & 1; integer b = !unknown; \
        } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn respects_shadowing_when_resolving_operator_operand_types() {
    let diagnostics = sem045(
        "float value; default { state_entry() { \
            integer value = 1; integer result = ~value; \
        } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn parser_errors_and_unreliable_environments_suppress_the_rule() {
    let source = "default { state_entry() { integer bad = !1.0; integer broken = ; } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(!syntax.is_empty());
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, true);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM045"),
        "{diagnostics:?}"
    );

    let (tokens, lexical) = lex("default { state_entry() { integer bad = !1.0; } }");
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, false);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM045"),
        "{diagnostics:?}"
    );
}
