use neolint::{analyze, lex, parse, Diagnostic};

fn analyze_source(source: &str) -> Vec<Diagnostic> {
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

    analyze(&tokens)
}

fn count_code(diagnostics: &[Diagnostic], code: &str) -> usize {
    diagnostics.iter().filter(|item| item.code == code).count()
}

fn assert_no_code(diagnostics: &[Diagnostic], code: &str) {
    assert_eq!(
        count_code(diagnostics, code),
        0,
        "unexpected {code}: {diagnostics:?}"
    );
}

#[test]
fn declaration_initializers_cover_literal_expressions_and_commas() {
    let diagnostics = analyze_source(
        "vector sum = <1, 2, 3> + <4, 5, 6>; \
         integer equal = <1, 2, 3> == <4, 5, 6>; \
         vector cast = (vector)<1, 2, 3>; \
         default { }",
    );
    assert_no_code(&diagnostics, "SEM015");

    let diagnostics =
        analyze_source("string equal = <1, 2, 3> == <4, 5, 6>, other = 1.5; default { }");
    assert_eq!(count_code(&diagnostics, "SEM015"), 2, "{diagnostics:?}");
}

#[test]
fn assignment_rhs_covers_literal_expressions() {
    let diagnostics = analyze_source(
        "default { state_entry() { \
             vector sum; integer equal; \
             sum = <1, 2, 3> + <4, 5, 6>; \
             equal = <1, 2, 3> == <4, 5, 6>; \
         } }",
    );
    assert_no_code(&diagnostics, "SEM016");

    let diagnostics =
        analyze_source("default { state_entry() { string text; text = <1, 2, 3> != <4, 5, 6>; } }");
    assert_eq!(count_code(&diagnostics, "SEM016"), 1, "{diagnostics:?}");
}

#[test]
fn compound_assignment_rhs_covers_literal_expressions() {
    let diagnostics =
        analyze_source("default { state_entry() { vector offset; offset += <1, 2, 3>; } }");
    assert_no_code(&diagnostics, "SEM017");

    let diagnostics =
        analyze_source("default { state_entry() { integer count; count += <1, 2, 3>; } }");
    assert_eq!(count_code(&diagnostics, "SEM017"), 1, "{diagnostics:?}");
}

#[test]
fn call_arguments_cover_literal_expressions_and_counting() {
    let diagnostics = analyze_source(
        "default { state_entry() { \
             llSetPos(<1, 2, 3>); \
             llOwnerSay((string)(<1, 2, 3> == <4, 5, 6>)); \
         } }",
    );
    assert_no_code(&diagnostics, "SEM002");
    assert_no_code(&diagnostics, "SEM004");

    let diagnostics = analyze_source(
        "default { state_entry() { \
             llSetPos(<1, 2, 3> + <4, 5, 6>, 1); \
             llOwnerSay(<1, 2, 3>); \
         } }",
    );
    assert_eq!(count_code(&diagnostics, "SEM002"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM004"), 1, "{diagnostics:?}");
}

#[test]
fn return_expressions_cover_literal_expressions_and_casts() {
    let diagnostics = analyze_source(
        "vector add() { return <1, 2, 3> + <4, 5, 6>; } \
         integer equal() { return <1, 2, 3> == <4, 5, 6>; } \
         vector cast() { return (vector)<1, 2, 3>; } \
         default { }",
    );
    assert_no_code(&diagnostics, "SEM008");

    let diagnostics = analyze_source(
        "integer add() { return <1, 2, 3> + <4, 5, 6>; } \
         string equal() { return <1, 2, 3> == <4, 5, 6>; } \
         integer cast() { return (vector)<1, 2, 3>; } \
         default { }",
    );
    assert_eq!(count_code(&diagnostics, "SEM008"), 3, "{diagnostics:?}");
}

#[test]
fn explicit_cast_contexts_cover_vector_literals() {
    let diagnostics = analyze_source(
        "vector value = (vector)<1, 2, 3>; \
         vector make() { return (vector)<1, 2, 3>; } \
         default { }",
    );
    assert_no_code(&diagnostics, "SEM008");
    assert_no_code(&diagnostics, "SEM015");

    let diagnostics = analyze_source(
        "string value = (vector)<1, 2, 3>; \
         integer make() { return (vector)<1, 2, 3>; } \
         default { }",
    );
    assert_eq!(count_code(&diagnostics, "SEM008"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM015"), 1, "{diagnostics:?}");
}

#[test]
fn equality_contexts_cover_vector_literal_results() {
    let diagnostics = analyze_source(
        "integer equal = <1, 2, 3> == <4, 5, 6>; \
         integer compare() { return <1, 2, 3> != <4, 5, 6>; } \
         default { }",
    );
    assert_no_code(&diagnostics, "SEM008");
    assert_no_code(&diagnostics, "SEM015");

    let diagnostics = analyze_source(
        "string equal = <1, 2, 3> == <4, 5, 6>; \
         string compare() { return <1, 2, 3> != <4, 5, 6>; } \
         default { }",
    );
    assert_eq!(count_code(&diagnostics, "SEM008"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM015"), 1, "{diagnostics:?}");
}

#[test]
fn arithmetic_contexts_cover_vector_literal_results() {
    let diagnostics = analyze_source(
        "vector sum = <1, 2, 3> + <4, 5, 6>; \
         float dot = <1, 2, 3> * <4, 5, 6>; \
         rotation compose = <1, 2, 3, 4> * <5, 6, 7, 8>; \
         default { }",
    );
    assert_no_code(&diagnostics, "SEM015");

    let diagnostics = analyze_source(
        "string sum = <1, 2, 3> + <4, 5, 6>; \
         integer dot = <1, 2, 3> * <4, 5, 6>; \
         default { }",
    );
    assert_eq!(count_code(&diagnostics, "SEM015"), 2, "{diagnostics:?}");
}

#[test]
fn relational_angle_brackets_remain_conservative() {
    let diagnostics = analyze_source(
        "string grouped_lt = (1) < 2; \
         integer grouped_lt_ok = (1) < 2; \
         string grouped_gt = (1) > 2; \
         integer grouped_gt_ok = (1) > 2; \
         string vector_lt = <1, 2, 3> < <4, 5, 6>; \
         string vector_gt = <1, 2, 3> > <4, 5, 6>; \
         string int_lt_vector = 1 < <1, 2, 3>; \
         string int_gt_vector = 1 > <1, 2, 3>; \
         default { }",
    );
    assert_eq!(
        count_code(&diagnostics, "SEM015"),
        2,
        "only the grouped integer comparisons have known types: {diagnostics:?}"
    );
}

#[test]
fn rotation_literal_assignment_and_return_contexts() {
    let diagnostics = analyze_source(
        "rotation global_orientation = <1, 2, 3, 4>; \
         rotation make() { return <1, 2, 3, 4>; } \
         default { state_entry() { \
             rotation local_orientation; \
             local_orientation = <1, 2, 3, 4>; \
         } }",
    );
    assert_no_code(&diagnostics, "SEM008");
    assert_no_code(&diagnostics, "SEM015");
    assert_no_code(&diagnostics, "SEM016");

    let diagnostics = analyze_source(
        "vector global_position = <1, 2, 3, 4>; \
         vector make() { return <1, 2, 3, 4>; } \
         default { state_entry() { \
             vector local_position; \
             local_position = <1, 2, 3, 4>; \
         } }",
    );
    assert_eq!(count_code(&diagnostics, "SEM008"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM015"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM016"), 1, "{diagnostics:?}");
}

#[test]
fn rotation_literal_in_llsetrot_arguments() {
    let diagnostics = analyze_source("default { state_entry() { llSetRot(<1, 2, 3, 4>); } }");
    assert_no_code(&diagnostics, "SEM002");
    assert_no_code(&diagnostics, "SEM004");

    let diagnostics = analyze_source("default { state_entry() { llSetRot(<1, 2, 3>); } }");
    assert_eq!(count_code(&diagnostics, "SEM004"), 1, "{diagnostics:?}");

    let diagnostics = analyze_source("default { state_entry() { llSetRot(<1, 2, 3, 4>, 1); } }");
    assert_eq!(count_code(&diagnostics, "SEM002"), 1, "{diagnostics:?}");
}

#[test]
fn explicit_rotation_cast_contexts() {
    let diagnostics = analyze_source(
        "rotation global_orientation = (rotation)<1, 2, 3, 4>; \
         rotation make() { return (rotation)<1, 2, 3, 4>; } \
         default { }",
    );
    assert_no_code(&diagnostics, "SEM008");
    assert_no_code(&diagnostics, "SEM014");
    assert_no_code(&diagnostics, "SEM015");

    let diagnostics = analyze_source(
        "vector global_position = (rotation)<1, 2, 3, 4>; \
         vector make() { return (rotation)<1, 2, 3, 4>; } \
         vector invalid = (integer)<1, 2, 3>; \
         string text; \
         default { state_entry() { text = (rotation)<1, 2, 3, 4>; } }",
    );
    assert_eq!(count_code(&diagnostics, "SEM008"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM014"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM015"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM016"), 1, "{diagnostics:?}");
}

#[test]
fn ambiguous_relational_assignments_and_returns_remain_conservative() {
    let diagnostics = analyze_source(
        "string assign_lt; \
         string assign_gt; \
         string return_lt() { return <1, 2, 3> < <4, 5, 6>; } \
         string return_gt() { return 1 > <1, 2, 3>; } \
         default { state_entry() { \
             assign_lt = <1, 2, 3> < <4, 5, 6>; \
             assign_gt = 1 > <1, 2, 3>; \
         } }",
    );
    assert_no_code(&diagnostics, "SEM008");
    assert_no_code(&diagnostics, "SEM016");

    let diagnostics = analyze_source(
        "string assign_known; \
         string return_known() { return (1) < 2; } \
         default { state_entry() { assign_known = (1) < 2; } }",
    );
    assert_eq!(count_code(&diagnostics, "SEM008"), 1, "{diagnostics:?}");
    assert_eq!(count_code(&diagnostics, "SEM016"), 1, "{diagnostics:?}");
}

#[test]
fn unknown_extension_expressions_remain_conservative() {
    let diagnostics = analyze_source(
        "vector make() { return llExtensionValue(); } \
         integer compare() { return llExtensionValue() == <1, 2, 3>; } \
         default { state_entry() { \
             string text = llExtensionValue(); \
             vector value; value = llExtensionValue(); \
         } }",
    );
    assert_no_code(&diagnostics, "SEM008");
    assert_no_code(&diagnostics, "SEM015");
    assert_no_code(&diagnostics, "SEM016");
}
