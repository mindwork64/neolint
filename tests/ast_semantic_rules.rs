//! Focused SEM024 regression coverage over the AST semantic analyzer.

use neolint::ast::{parse_ast, parse_ast_with_options, Ast, AstParseOptions};
use neolint::ast_semantic::analyze_ast;
use neolint::{lex, Diagnostic, DiagnosticKind, Severity, Span};

fn parse(source: &str, options: AstParseOptions) -> (Ast, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    parse_ast_with_options(&tokens, options)
}

fn semantic_with(source: &str, options: AstParseOptions) -> Vec<Diagnostic> {
    let (ast, syntax) = parse(source, options);
    analyze_ast(&ast, &syntax)
}

fn semantic_valid_with(source: &str, options: AstParseOptions) -> Vec<Diagnostic> {
    let (ast, syntax) = parse(source, options);
    assert!(
        syntax.is_empty(),
        "fixture must be syntactically valid: {syntax:?}"
    );
    analyze_ast(&ast, &syntax)
}

fn semantic(source: &str) -> Vec<Diagnostic> {
    semantic_valid_with(source, AstParseOptions::lsl())
}

fn semantic_with_syntax_error(source: &str) -> Vec<Diagnostic> {
    semantic_with(source, AstParseOptions::lsl())
}

fn sem024(diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "SEM024")
        .cloned()
        .collect()
}

fn span(source: &str, start: usize, len: usize) -> Span {
    let end = start + len;
    let line = source[..start].matches('\n').count() + 1;
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let end_line = source[..end].matches('\n').count() + 1;
    let end_line_start = source[..end].rfind('\n').map_or(0, |index| index + 1);
    Span {
        start,
        end,
        line,
        column: source[line_start..start].chars().count() + 1,
        end_line,
        end_column: source[end_line_start..end].chars().count() + 1,
    }
}

fn expected_unused(source: &str, start: usize, name: &str) -> Diagnostic {
    Diagnostic {
        code: "SEM024",
        severity: Severity::Warning,
        kind: DiagnosticKind::Lint,
        message: format!("variable '{name}' is declared but never read"),
        span: span(source, start, name.len()),
    }
}

#[test]
fn unused_global_reports_exact_diagnostic() {
    let source = "integer unused;\ndefault { state_entry() { } }\n";
    let start = source.find("unused").unwrap();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "unused")]
    );
}

#[test]
fn unused_local_reports_exact_diagnostic() {
    let source = "default { state_entry() { integer unused; } }\n";
    let start = source.find("unused").unwrap();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "unused")]
    );
}

#[test]
fn used_global_and_local_produce_no_findings() {
    let source = "integer used_global;\ndefault { state_entry() { integer used_local; used_local = used_global; if (used_local) { } } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn multiple_declarators_report_only_unused_in_registration_order() {
    let source = "integer used, unused;\ndefault { state_entry() { integer local_used, local_unused; llOwnerSay((string)used + (string)local_used); } }\n";
    let global_start = source.find("unused").unwrap();
    let local_start = source.find("local_unused").unwrap();
    assert_eq!(
        sem024(&semantic(source)),
        vec![
            expected_unused(source, global_start, "unused"),
            expected_unused(source, local_start, "local_unused"),
        ]
    );
}

#[test]
fn nested_shadowing_keeps_declarations_distinct() {
    let inner_used = "default { state_entry() { integer value; { integer value; llOwnerSay((string)value); } } }\n";
    let outer_start = inner_used.find("value").unwrap();
    assert_eq!(
        sem024(&semantic(inner_used)),
        vec![expected_unused(inner_used, outer_start, "value")]
    );

    let outer_used = "default { state_entry() { integer value; { integer value; } llOwnerSay((string)value); } }\n";
    let inner_start = outer_used.match_indices("value").nth(1).unwrap().0;
    assert_eq!(
        sem024(&semantic(outer_used)),
        vec![expected_unused(outer_used, inner_start, "value")]
    );
}

#[test]
fn same_spelling_in_different_functions_is_independent() {
    let source = "integer first_reader() { integer value; return value; }\ninteger second_reader() { integer value; return 1; }\ndefault { state_entry() { } }\n";
    let unread_start = source.match_indices("value").nth(2).unwrap().0;
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, unread_start, "value")]
    );
}

#[test]
fn use_before_declaration_does_not_resolve_to_the_later_declaration() {
    let source = "default { state_entry() { llOwnerSay((string)value); integer value; } }\n";
    let declaration_start = source.rfind("value").unwrap();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, declaration_start, "value")]
    );
}

#[test]
fn read_in_another_function_or_event_marks_a_global_used() {
    let source = "integer by_function;\ninteger by_event;\ninteger read_function() { return by_function; }\ndefault { state_entry() { llOwnerSay((string)by_event); } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn read_only_inside_a_nested_call_argument_counts() {
    let source = "default { state_entry() { integer value; llOwnerSay((string)llAbs(value)); } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn loop_condition_and_for_clauses_count_as_reads() {
    let source = "default { state_entry() { integer while_value; integer init_value; integer condition_value; integer increment_value; while (while_value) { } for (init_value; condition_value; increment_value) { } } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn switch_condition_and_case_expression_count_as_reads() {
    let source = "default { state_entry() { integer condition; integer case_value; switch (condition) { case case_value: break; } } }\n";
    let options = AstParseOptions::lsl().with_switches_enabled(true);
    assert!(sem024(&semantic_valid_with(source, options)).is_empty());
}

#[test]
fn lazy_list_base_and_index_count_as_reads() {
    let source =
        "default { state_entry() { list values; integer index; (integer)values[index]; } }\n";
    let options = AstParseOptions::lsl().with_lazy_lists_enabled(true);
    assert!(sem024(&semantic_valid_with(source, options)).is_empty());
}

#[test]
fn plain_assignment_target_is_not_a_read() {
    let source = "default { state_entry() { integer value; value = 1; } }\n";
    let start = source.find("value").unwrap();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "value")]
    );
}

#[test]
fn compound_assignment_target_is_a_read() {
    let source = "default { state_entry() { integer value; value += 1; } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn prefix_and_postfix_increment_and_decrement_are_reads() {
    let source = "default { state_entry() { integer pre_inc; integer post_inc; integer pre_dec; integer post_dec; ++pre_inc; post_inc++; --pre_dec; post_dec--; } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn component_read_counts_as_a_read() {
    let source = "default { state_entry() { vector value; llOwnerSay((string)value.x); } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn simple_component_assignment_is_not_a_read() {
    let source = "default { state_entry() { vector value; value.x = 1.0; } }\n";
    let start = source.find("value").unwrap();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "value")]
    );
}

#[test]
fn compound_component_assignment_is_a_read() {
    let source = "default { state_entry() { vector value; value.x += 1.0; } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn call_callee_is_not_a_variable_read() {
    let source =
        "integer helper() { return 1; }\ndefault { state_entry() { integer helper; helper(); } }\n";
    let start = source.match_indices("helper").nth(1).unwrap().0;
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "helper")]
    );
}

#[test]
fn malformed_initializer_produces_no_sem024() {
    let source = "integer x = ;\ndefault { state_entry() { } }\n";
    let (_, syntax) = parse(source, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "malformed fixture must be a syntax error"
    );
    assert!(sem024(&semantic_with_syntax_error(source)).is_empty());
}

#[test]
fn any_syntax_error_suppresses_sem024_for_the_whole_unit() {
    let source = "integer unused;\ndefault { state_entry() { integer broken = ; } }\n";
    let (_, syntax) = parse(source, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "malformed fixture must be a syntax error"
    );
    assert!(sem024(&semantic_with_syntax_error(source)).is_empty());
}

#[test]
fn repeated_runs_are_deterministic() {
    let source = "integer used_global, unused_global;\ndefault { state_entry() { integer used_local, unused_local; llOwnerSay((string)used_global + (string)used_local); } }\n";
    let first = sem024(&semantic(source));
    let second = sem024(&semantic(source));
    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    let unused_global = source.find("unused_global").unwrap();
    let unused_local = source.find("unused_local").unwrap();
    assert_eq!(
        first,
        vec![
            expected_unused(source, unused_global, "unused_global"),
            expected_unused(source, unused_local, "unused_local"),
        ]
    );
}

#[test]
fn plain_parse_ast_path_remains_available() {
    let source = "integer unused;\ndefault { state_entry() { } }\n";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, syntax) = parse_ast(&tokens);
    assert!(syntax.is_empty());
    assert_eq!(sem024(&analyze_ast(&ast, &syntax)).len(), 1);
}
