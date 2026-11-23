//! Focused SEM024 regression coverage over the AST semantic analyzer.

use neolint::ast::{parse_ast, parse_ast_with_options, Ast, AstParseOptions};
use neolint::ast_semantic::{analyze_ast, analyze_ast_with_environment};
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

fn sem025(diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "SEM025")
        .cloned()
        .collect()
}

fn sem026(diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "SEM026")
        .cloned()
        .collect()
}

fn expected_unused_parameter(source: &str, start: usize, name: &str) -> Diagnostic {
    Diagnostic {
        code: "SEM025",
        severity: Severity::Warning,
        kind: DiagnosticKind::Lint,
        message: format!("parameter '{name}' is never read"),
        span: span(source, start, name.len()),
    }
}

fn expected_unreachable_function(source: &str, start: usize, name: &str) -> Diagnostic {
    Diagnostic {
        code: "SEM026",
        severity: Severity::Warning,
        kind: DiagnosticKind::Lint,
        message: format!("function '{name}' is never called"),
        span: span(source, start, name.len()),
    }
}

#[test]
fn unused_function_parameter_reports_exact_diagnostic() {
    let source = "integer callee(integer unused) { return 1; }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    let start = source.find("unused").unwrap();
    assert_eq!(
        sem025(&semantic(source)),
        vec![expected_unused_parameter(source, start, "unused")]
    );
}

#[test]
fn used_function_parameter_produces_no_finding() {
    let source = "integer callee(integer value) { return value; }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    assert!(sem025(&semantic(source)).is_empty());
}

#[test]
fn unused_canonical_event_parameter_reports_exact_diagnostic() {
    let source = "default { touch_start(integer unused) { } }\n";
    let start = source.find("unused").unwrap();
    assert_eq!(
        sem025(&semantic(source)),
        vec![expected_unused_parameter(source, start, "unused")]
    );
}

#[test]
fn used_canonical_event_parameter_produces_no_finding() {
    let source = "default { touch_start(integer count) { llOwnerSay((string)count); } }\n";
    assert!(sem025(&semantic(source)).is_empty());
}

#[test]
fn parameter_assigned_but_never_read_is_still_reported() {
    let source = "integer callee(integer value) { value = 1; return 1; }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    let start = source.find("value").unwrap();
    assert_eq!(
        sem025(&semantic(source)),
        vec![expected_unused_parameter(source, start, "value")]
    );
}

#[test]
fn parameter_read_only_inside_a_nested_block_or_expression_counts() {
    let source = "integer callee(integer value) { { llOwnerSay((string)llAbs(value)); } return 1; }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    assert!(sem025(&semantic(source)).is_empty());
}

#[test]
fn local_shadowing_reads_do_not_mark_the_parameter_used() {
    let source = "integer callee(integer value) { integer result = 1; { integer value; result = value; } return result; }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    let parameter_start = source.find("value").unwrap();
    assert_eq!(
        sem025(&semantic(source)),
        vec![expected_unused_parameter(source, parameter_start, "value")]
    );
}

#[test]
fn parameter_reads_do_not_mark_a_later_shadowing_local_used() {
    let source = "integer callee(integer value) { integer result = value; integer value; return result; }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    let local_start = source.match_indices("value").nth(2).unwrap().0;
    assert!(sem025(&semantic(source)).is_empty());
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, local_start, "value")]
    );
}

#[test]
fn parameter_read_only_in_loop_clauses_counts() {
    let source = "integer scan(integer limit) { integer index; for (index = 0; index < limit; index++) { } return index; }\ndefault { state_entry() { llOwnerSay((string)scan(2)); } }\n";
    assert!(sem025(&semantic(source)).is_empty());
}

#[test]
fn parameter_read_only_in_switch_expressions_counts() {
    let source = "integer choose(integer selector) { switch (selector) { case 1: break; } return 1; }\ndefault { state_entry() { llOwnerSay((string)choose(1)); } }\n";
    let options = AstParseOptions::lsl().with_switches_enabled(true);
    assert!(sem025(&semantic_valid_with(source, options)).is_empty());
}

#[test]
fn parameter_read_only_in_lazy_list_expressions_counts() {
    let source = "integer pick(list values, integer index) { return (integer)values[index]; }\ndefault { state_entry() { llOwnerSay((string)pick([1], 0)); } }\n";
    let options = AstParseOptions::lsl().with_lazy_lists_enabled(true);
    assert!(sem025(&semantic_valid_with(source, options)).is_empty());
}

#[test]
fn malformed_parameter_produces_no_sem025() {
    let source = "integer broken(integer ) { return 1; }\ndefault { state_entry() { } }\n";
    let (_, syntax) = parse(source, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "malformed fixture must be a syntax error"
    );
    assert!(sem025(&semantic_with_syntax_error(source)).is_empty());
}

#[test]
fn unknown_extension_event_parameters_produce_no_sem025() {
    let source = "default { custom_extension_event(integer unused) { } }\n";
    assert!(sem025(&semantic(source)).is_empty());
}

#[test]
fn separate_analysis_units_do_not_share_parameter_state() {
    let unused_source = "integer first(integer unused) { return 1; }\ndefault { state_entry() { llOwnerSay((string)first(1)); } }\n";
    let used_source = "integer second(integer value) { return value; }\ndefault { state_entry() { llOwnerSay((string)second(1)); } }\n";
    let start = unused_source.find("unused").unwrap();
    assert_eq!(
        sem025(&semantic(unused_source)),
        vec![expected_unused_parameter(unused_source, start, "unused")]
    );
    assert!(sem025(&semantic(used_source)).is_empty());
    // Re-analysing the first unit after the second one is unaffected.
    assert_eq!(
        sem025(&semantic(unused_source)),
        vec![expected_unused_parameter(unused_source, start, "unused")]
    );
}

#[test]
fn function_never_referenced_reports_exact_diagnostic() {
    let source = "unused_helper() { }\ndefault { state_entry() { } }\n";
    let start = source.find("unused_helper").unwrap();
    assert_eq!(
        sem026(&semantic(source)),
        vec![expected_unreachable_function(
            source,
            start,
            "unused_helper"
        )]
    );
}

#[test]
fn function_called_directly_from_an_event_is_reachable() {
    let source = "helper() { }\ndefault { state_entry() { helper(); } }\n";
    assert!(sem026(&semantic(source)).is_empty());
}

#[test]
fn transitive_call_chain_from_an_event_is_reachable() {
    let source = "leaf() { }\nmiddle() { leaf(); }\ntop() { middle(); }\ndefault { state_entry() { top(); } }\n";
    assert!(sem026(&semantic(source)).is_empty());
}

#[test]
fn function_called_only_from_an_unreachable_function_is_reported() {
    let source = "leaf() { }\nparent() { leaf(); }\ndefault { state_entry() { } }\n";
    let leaf_start = source.find("leaf").unwrap();
    let parent_start = source.find("parent").unwrap();
    assert_eq!(
        sem026(&semantic(source)),
        vec![
            expected_unreachable_function(source, leaf_start, "leaf"),
            expected_unreachable_function(source, parent_start, "parent"),
        ]
    );
}

#[test]
fn self_recursion_is_unreachable_without_an_entry_path() {
    let source = "looper() { looper(); }\ndefault { state_entry() { } }\n";
    let start = source.find("looper").unwrap();
    let second = source.match_indices("looper").nth(1).unwrap().0;
    assert_eq!(
        sem026(&semantic(source)),
        vec![expected_unreachable_function(source, start, "looper")]
    );
    assert!(second > start);
}

#[test]
fn self_recursion_with_an_entry_path_is_reachable() {
    let source = "looper() { looper(); }\ndefault { state_entry() { looper(); } }\n";
    assert!(sem026(&semantic(source)).is_empty());
}

#[test]
fn mutually_recursive_cycle_without_roots_reports_both_functions() {
    let source = "alpha() { beta(); }\nbeta() { alpha(); }\ndefault { state_entry() { } }\n";
    let alpha_start = source.find("alpha").unwrap();
    let beta_start = source.match_indices("beta").nth(1).unwrap().0;
    assert_eq!(
        sem026(&semantic(source)),
        vec![
            expected_unreachable_function(source, alpha_start, "alpha"),
            expected_unreachable_function(source, beta_start, "beta"),
        ]
    );
}

#[test]
fn calls_from_events_in_multiple_states_are_roots() {
    let source = "helper() { }\ndefault { state_entry() { } }\nstate active { state_entry() { helper(); } }\n";
    assert!(sem026(&semantic(source)).is_empty());
}

#[test]
fn extension_event_handler_calls_are_roots() {
    let source = "helper() { }\ndefault { custom_extension_event() { helper(); } }\n";
    assert!(sem026(&semantic(source)).is_empty());
}

#[test]
fn builtin_name_call_does_not_make_same_named_user_function_reachable() {
    let source = "llSay() { }\ndefault { state_entry() { llSay(); } }\n";
    let function_start = source.find("llSay").unwrap();
    let diagnostics = semantic(source);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM002"),
        "the call is analyzed as the canonical built-in: {diagnostics:?}"
    );
    assert_eq!(
        sem026(&diagnostics),
        vec![expected_unreachable_function(
            source,
            function_start,
            "llSay"
        )]
    );
}

#[test]
fn calls_from_a_global_initializer_are_roots() {
    let source =
        "integer helper() { return 1; }\ninteger g = helper();\ndefault { state_entry() { } }\n";
    assert!(sem026(&semantic(source)).is_empty());
}

#[test]
fn calls_in_every_expression_bearing_context_create_edges() {
    let nested = "integer helper() { return 1; }\ndefault { state_entry() { llOwnerSay((string)(helper() + 1)); } }\n";
    assert!(sem026(&semantic(nested)).is_empty(), "{nested}");

    let loop_clauses =
        "integer helper() { return 1; }\ndefault { state_entry() { for (helper(); helper(); helper()) { } } }\n";
    assert!(sem026(&semantic(loop_clauses)).is_empty(), "{loop_clauses}");

    let switch_arms = "integer helper() { return 1; }\ndefault { state_entry() { switch (helper()) { case helper(): helper(); break; } } }\n";
    let options = AstParseOptions::lsl().with_switches_enabled(true);
    assert!(
        sem026(&semantic_valid_with(switch_arms, options)).is_empty(),
        "{switch_arms}"
    );

    let lazy_expressions = "integer helper() { return 1; }\ndefault { state_entry() { list values; (integer)values[helper()]; } }\n";
    let options = AstParseOptions::lsl().with_lazy_lists_enabled(true);
    assert!(
        sem026(&semantic_valid_with(lazy_expressions, options)).is_empty(),
        "{lazy_expressions}"
    );
}

#[test]
fn prototype_without_a_body_is_never_reported() {
    let source = "integer broken() default { state_entry() { } }\n";
    let (ast, syntax) = parse(source, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "prototype fixture must be a syntax error"
    );
    assert!(ast.items.iter().any(
        |item| matches!(item, neolint::ast::TopLevel::Function(function) if function.body.is_none())
    ));
    assert!(sem026(&analyze_ast(&ast, &syntax)).is_empty());
}

#[test]
fn duplicate_function_declarations_report_only_the_first_definition() {
    let source = "duplicate() { }\nduplicate() { }\ndefault { state_entry() { } }\n";
    let first_start = source.find("duplicate").unwrap();
    let diagnostics = semantic(source);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM001"),
        "duplicate declarations must still be covered by SEM001"
    );
    assert_eq!(
        sem026(&diagnostics),
        vec![expected_unreachable_function(
            source,
            first_start,
            "duplicate"
        )]
    );
}

#[test]
fn recovered_function_signature_is_never_reported() {
    let source = "integer broken(, ) { return 1; }\ndefault { state_entry() { } }\n";
    let (ast, syntax) = parse(source, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "recovered fixture must be a syntax error"
    );
    assert!(
        !ast.recovered_functions.is_empty(),
        "fixture must produce a recovered function signature"
    );
    assert!(sem026(&analyze_ast(&ast, &syntax)).is_empty());
}

#[test]
fn call_without_a_matched_close_parenthesis_does_not_create_an_edge() {
    let source = "helper() { }\ndefault { state_entry() { helper(); } }\n";
    let (mut ast, syntax) = parse(source, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    let mut mutated = 0usize;
    for item in &mut ast.items {
        let neolint::ast::TopLevel::State(state) = item else {
            continue;
        };
        for event in &mut state.events {
            let Some(body) = &mut event.body else {
                continue;
            };
            for statement in &mut body.statements {
                let neolint::ast::StatementKind::Expression(expression) = &mut statement.kind
                else {
                    continue;
                };
                let neolint::ast::ExpressionKind::Call(call) = &mut expression.expression.kind
                else {
                    continue;
                };
                call.matched_close = false;
                mutated += 1;
            }
        }
    }
    assert_eq!(mutated, 1);
    let start = source.find("helper").unwrap();
    assert_eq!(
        sem026(&analyze_ast(&ast, &syntax)),
        vec![expected_unreachable_function(source, start, "helper")]
    );
}

#[test]
fn incomplete_function_body_produces_no_sem026() {
    let source = "helper() { }\ndefault { state_entry() { } }\n";
    let (mut ast, syntax) = parse(source, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    let function = ast
        .items
        .iter_mut()
        .find_map(|item| match item {
            neolint::ast::TopLevel::Function(function) => Some(function),
            _ => None,
        })
        .expect("function declaration");
    function.body.as_mut().expect("function body").closed = false;
    assert!(sem026(&analyze_ast(&ast, &syntax)).is_empty());
}

#[test]
fn parser_diagnostics_suppress_sem024_sem025_and_sem026() {
    let source = "integer unused_global;\nunused_function(integer unused_parameter) { }\ndefault { state_entry() { integer broken = ; } }\n";
    let (_, syntax) = parse(source, AstParseOptions::lsl());
    assert!(
        !syntax.is_empty(),
        "malformed fixture must be a syntax error"
    );
    let diagnostics = semantic_with_syntax_error(source);
    assert!(sem024(&diagnostics).is_empty());
    assert!(sem025(&diagnostics).is_empty());
    assert!(sem026(&diagnostics).is_empty());
}

#[test]
fn call_graph_budget_overflow_suppresses_sem026() {
    let mut source = String::new();
    for index in 0..4_100 {
        source.push_str(&format!("function_{index}() {{ }}\n"));
    }
    source.push_str("default { state_entry() { } }\n");
    let (ast, syntax) = parse(&source, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    assert!(sem026(&analyze_ast(&ast, &syntax)).is_empty());
}

#[test]
fn production_edge_budget_overflow_suppresses_sem026() {
    let mut source = String::new();
    for index in 0..4_096 {
        let first = (index + 1) % 4_096;
        let second = (index + 2) % 4_096;
        let third = (index + 3) % 4_096;
        source.push_str(&format!(
            "f{index}() {{ f{first}(); f{second}(); f{third}(); }}\n"
        ));
    }
    source.push_str("default { state_entry() { } }\n");
    let (ast, syntax) = parse(&source, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    assert!(sem026(&analyze_ast(&ast, &syntax)).is_empty());
}

#[test]
fn production_work_budget_overflow_suppresses_sem026() {
    let mut source = String::new();
    for index in 0..4_096 {
        let next = (index + 1) % 4_096;
        // Unique edges stay below the edge cap while repeated call sites push
        // the checked work counter past its production limit.
        source.push_str(&format!(
            "f{index}() {{ f{next}(); f{next}(); f{next}(); f{next}(); }}\n"
        ));
    }
    source.push_str("default { state_entry() { } }\n");
    let (ast, syntax) = parse(&source, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");
    assert!(sem026(&analyze_ast(&ast, &syntax)).is_empty());
}

#[test]
fn sem025_and_sem026_are_deterministic_and_ordered_by_declaration() {
    let source = "integer callee(integer unused) { return 1; }\nalpha() { }\nbeta() { }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    let parameter_start = source.find("unused").unwrap();
    let alpha_start = source.find("alpha").unwrap();
    let beta_start = source.find("beta").unwrap();
    let first = semantic(source);
    let second = semantic(source);
    assert_eq!(first, second);
    let selected: Vec<Diagnostic> = first
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code, "SEM025" | "SEM026"))
        .cloned()
        .collect();
    assert_eq!(
        selected,
        vec![
            expected_unused_parameter(source, parameter_start, "unused"),
            expected_unreachable_function(source, alpha_start, "alpha"),
            expected_unreachable_function(source, beta_start, "beta"),
        ]
    );
}

#[test]
fn sem024_keeps_reads_inside_unreachable_functions_while_sem026_reports_the_function() {
    let source = "integer global_read;\ndead_reader() {\n    integer local_read;\n    llOwnerSay((string)global_read + (string)local_read);\n}\nreachable_helper() { }\ndefault { state_entry() { } }\nstate active { state_entry() { reachable_helper(); } }\n";
    let dead_start = source.find("dead_reader").unwrap();
    let diagnostics = semantic(source);

    // SEM024 is syntactic: reads inside a function that SEM026 reports as
    // unreachable still count, so neither variable is reported as unused.
    assert!(
        sem024(&diagnostics).is_empty(),
        "reads in an unreachable function must still count: {diagnostics:?}"
    );
    assert_eq!(
        sem026(&diagnostics),
        vec![expected_unreachable_function(
            source,
            dead_start,
            "dead_reader"
        )]
    );

    // A function reached only through a non-default state's event is a root.
    assert_eq!(sem026(&diagnostics).len(), 1);
    assert!(!sem026(&diagnostics)
        .iter()
        .any(|diagnostic| diagnostic.message.contains("reachable_helper")));
}

#[test]
fn chained_assignment_is_write_write_read() {
    let source = "default { state_entry() { integer a; integer b; integer c; a = b = c; } }\n";
    let a_start = source.find("integer a;").unwrap() + "integer ".len();
    let b_start = source.find("integer b;").unwrap() + "integer ".len();
    assert_eq!(
        sem024(&semantic(source)),
        vec![
            expected_unused(source, a_start, "a"),
            expected_unused(source, b_start, "b"),
        ]
    );
}

#[test]
fn assignment_inside_a_call_argument_is_a_write() {
    let source =
        "default { state_entry() { integer target; llOwnerSay((string)(target = 1)); } }\n";
    let start = source.find("integer target;").unwrap() + "integer ".len();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "target")]
    );
}

#[test]
fn component_increment_is_rejected_by_the_parser_and_suppresses_sem024() {
    // LSL increment/decrement applies to variables, not components. The parser
    // rejects both forms, so the unit fails closed instead of guessing that the
    // base variable was read.
    let source = "default { state_entry() { vector v; v.x++; ++v.x; } }\n";
    let (ast, syntax) = parse(source, AstParseOptions::lsl());
    assert_eq!(
        syntax
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        vec!["PAR011", "PAR011"]
    );
    assert!(sem024(&analyze_ast(&ast, &syntax)).is_empty());
}

#[test]
fn lazy_list_simple_write_is_a_base_write() {
    let source = "default { state_entry() { list l; l[0] = 1; } }\n";
    let start = source.find("list l;").unwrap() + "list ".len();
    let options = AstParseOptions::lsl().with_lazy_lists_enabled(true);
    assert_eq!(
        sem024(&semantic_valid_with(source, options)),
        vec![expected_unused(source, start, "l")]
    );
}

#[test]
fn parenthesized_assignment_target_is_a_write() {
    let source = "default { state_entry() { integer a; (a) = 1; } }\n";
    let start = source.find("integer a;").unwrap() + "integer ".len();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "a")]
    );
}

#[test]
fn for_initializer_assignment_is_a_write() {
    let source = "default { state_entry() { integer index; for (index = 0; 0; ) { } } }\n";
    let start = source.find("integer index;").unwrap() + "integer ".len();
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, start, "index")]
    );
}

#[test]
fn assignment_inside_a_switch_case_expression_is_a_write() {
    let source =
        "default { state_entry() { integer target; switch (1) { case (target = 1): break; } } }\n";
    let start = source.find("integer target;").unwrap() + "integer ".len();
    let options = AstParseOptions::lsl().with_switches_enabled(true);
    assert_eq!(
        sem024(&semantic_valid_with(source, options)),
        vec![expected_unused(source, start, "target")]
    );
}

#[test]
fn self_referential_initializer_counts_as_a_read() {
    let source = "default { state_entry() { integer a = a; } }\n";
    assert!(sem024(&semantic(source)).is_empty());
}

#[test]
fn use_before_declaration_binds_to_the_outer_declaration() {
    let source = "default { state_entry() { integer outer; { llOwnerSay((string)outer); integer outer; } } }\n";
    let inner_start = source.match_indices("outer").nth(2).unwrap().0;
    assert_eq!(
        sem024(&semantic(source)),
        vec![expected_unused(source, inner_start, "outer")]
    );
}

#[test]
fn identifier_matching_is_case_sensitive() {
    let source = "default { state_entry() { integer Value; llOwnerSay((string)value); } }\n";
    let value_start = source.find("Value").unwrap();
    let diagnostics = semantic(source);
    assert_eq!(
        sem024(&diagnostics),
        vec![expected_unused(source, value_start, "Value")]
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM012" && diagnostic.message.contains("'value'")),
        "case-variant use must remain unresolved: {diagnostics:?}"
    );
}

#[test]
fn nested_scope_unused_variables_are_reported_in_source_order() {
    let source = "default { state_entry() { integer a; { integer b; } integer c; } }\n";
    let a_start = source.find("integer a;").unwrap() + "integer ".len();
    let b_start = source.find("integer b;").unwrap() + "integer ".len();
    let c_start = source.find("integer c;").unwrap() + "integer ".len();
    assert_eq!(
        sem024(&semantic(source)),
        vec![
            expected_unused(source, a_start, "a"),
            expected_unused(source, b_start, "b"),
            expected_unused(source, c_start, "c"),
        ]
    );
}

#[test]
fn environment_failure_suppresses_only_the_phase7_rules() {
    let source = "integer unused_global;\ninteger callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n";
    let (ast, syntax) = parse(source, AstParseOptions::lsl());
    assert!(syntax.is_empty(), "{syntax:?}");

    let baseline = analyze_ast(&ast, &syntax);
    for code in ["SEM024", "SEM025", "SEM026"] {
        assert!(
            baseline.iter().any(|diagnostic| diagnostic.code == code),
            "baseline missing {code}: {baseline:?}"
        );
    }

    let suppressed = analyze_ast_with_environment(&ast, &syntax, false);
    for code in ["SEM024", "SEM025", "SEM026"] {
        assert!(
            !suppressed.iter().any(|diagnostic| diagnostic.code == code),
            "unreliable environment must suppress {code}: {suppressed:?}"
        );
    }

    fn is_phase7(diagnostic: &Diagnostic) -> bool {
        matches!(diagnostic.code, "SEM024" | "SEM025" | "SEM026")
    }
    let baseline_other: Vec<Diagnostic> = baseline
        .iter()
        .filter(|diagnostic| !is_phase7(diagnostic))
        .cloned()
        .collect();
    let suppressed_other: Vec<Diagnostic> = suppressed
        .iter()
        .filter(|diagnostic| !is_phase7(diagnostic))
        .cloned()
        .collect();
    assert_eq!(
        suppressed_other, baseline_other,
        "environment flag must not change SEM001-SEM023 output"
    );
}
