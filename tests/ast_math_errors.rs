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

fn sem043(source: &str) -> Vec<Diagnostic> {
    analyze(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "SEM043")
        .collect()
}

#[test]
fn warns_on_integer_and_float_division_by_literal_zero() {
    let diagnostics = sem043(
        "integer divide_integer(integer value) { return value / 0; } \
         float divide_float(float value) { return value / 0.0; } \
         default { state_entry() { float result = 1.0 / -0.0; } }",
    );

    assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == neolint::diagnostic::Severity::Warning
            && diagnostic.kind == neolint::diagnostic::DiagnosticKind::Lint
    }));
}

#[test]
fn warns_on_integer_remainder_by_zero_but_not_vector_cross_product() {
    let diagnostics = sem043(
        "default { state_entry() { \
            integer value = 7 % (3 - 3); \
            vector left = <1.0, 2.0, 3.0>; \
            vector right = <0.0, 0.0, 0.0>; \
            vector cross = left % right; \
        } }",
    );

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("%"), "{diagnostics:?}");
}

#[test]
fn evaluates_negative_builtin_integer_constants() {
    let diagnostics = sem043("default { state_entry() { integer value = 1 / (LINK_SET + 1); } }");

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
}

#[test]
fn warns_on_vector_division_by_a_scalar_zero() {
    let diagnostics = sem043(
        "default { state_entry() { vector direction = <1.0, 0.0, 0.0>; vector invalid = direction / 0; } }",
    );

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
}

#[test]
fn warns_on_divide_and_remainder_compound_assignments() {
    let diagnostics =
        sem043("default { state_entry() { integer value = 4; value /= 0; value %= FALSE; } }");

    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics
        .iter()
        .all(|diagnostic| { diagnostic.message.contains("statically known zero divisor") }));
}

#[test]
fn evaluates_parentheses_unary_operators_casts_and_constant_arithmetic() {
    let diagnostics = sem043(
        "default { state_entry() { \
            integer a = 1 / ((2 - 2)); \
            integer b = 1 / (0x10 & 0); \
            integer c = 1 / !TRUE; \
            float d = 1.0 / (float)0; \
            integer e = 1 / (integer)0.0; \
        } }",
    );

    assert_eq!(diagnostics.len(), 5, "{diagnostics:?}");
}

#[test]
fn does_not_guess_mutable_variables_calls_or_nonzero_expressions() {
    let diagnostics = sem043(
        "integer helper() { return 0; } \
         default { state_entry() { \
             integer zero = 0; \
             integer value = 1 / zero; \
             integer call = 1 / helper(); \
             integer safe_a = 1 / 1; \
             integer safe_b = 1 / (2 - 1); \
             float safe_c = 1.0 / 0.5; \
         } }",
    );

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn respects_local_shadowing_of_canonical_constants() {
    let diagnostics =
        sem043("default { state_entry() { integer FALSE = 1; integer value = 10 / FALSE; } }");

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn reports_each_operator_and_uses_the_operator_span() {
    let source = "default { state_entry() { integer a = 5 / 0; integer b = 5 % 0; } }";
    let diagnostics = sem043(source);

    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    let slash = source.find('/').unwrap();
    let remainder = source.find('%').unwrap();
    assert_eq!(diagnostics[0].span.start, slash);
    assert_eq!(diagnostics[1].span.start, remainder);
}

#[test]
fn parser_errors_and_unreliable_environments_suppress_the_rule() {
    let source = "default { state_entry() { integer value = 1 / 0; integer broken = ; } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(!syntax.is_empty());
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, true);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "SEM043"));

    let (tokens, lexical) = lex("default { state_entry() { integer value = 1 / 0; } }");
    assert!(lexical.is_empty(), "{lexical:?}");
    let (ast, syntax) = parse_ast(&tokens);
    assert!(syntax.is_empty(), "{syntax:?}");
    let diagnostics = analyze_ast_with_environment(&ast, &syntax, false);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "SEM043"));
}
