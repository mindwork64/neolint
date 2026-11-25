//! Independent semantic analysis over the typed AST.
//!
//! Phases 6A–6F implement all existing SEM001–SEM023 diagnostics without token
//! scanning. Later AST-only rules add diagnostics outside the Legacy/Compare
//! parity contract; the module remains independent of preprocessor and
//! SourceMap internals.

pub mod scopes;

pub(crate) mod callgraph;
pub(crate) mod calls;
pub(crate) mod expressions;
pub(crate) mod flow;
pub(crate) mod rules;
pub(crate) mod statements;
pub(crate) mod types;
pub(crate) mod usage;

use crate::ast::Ast;
use crate::diagnostic::Diagnostic;

/// Run the AST semantic rules with an assumed-reliable lexical environment.
///
/// `syntax_diagnostics` must be the AST parser diagnostics for the same parse.
/// Several rules use that slice to suppress findings in malformed ranges, so
/// passing diagnostics from another parse produces incorrect results. This
/// wrapper delegates to [`analyze_ast_with_environment`] with
/// `environment_reliable = true`; callers that know the same unit had an
/// error-severity preprocessor or lexical diagnostic must use that entry point
/// instead.
pub fn analyze_ast(ast: &Ast, syntax_diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    analyze_ast_with_environment(ast, syntax_diagnostics, true)
}

/// Run the AST semantic rules with an explicit environment-reliability flag.
///
/// `syntax_diagnostics` must be the AST parser diagnostics for the same parse.
/// `environment_reliable` is false when the same analysis unit produced an
/// error-severity preprocessor or lexical diagnostic. Code may have been
/// dropped before parsing in that case, so the unused-variable, unused-parameter,
/// unreachable-function, global-initializer, state/list semantic, Firestorm-switch,
/// Mono state-transition-context, return/control-flow hazard, if-condition
/// assignment, detection-context, constant-math and state-reachability checks
/// fail closed instead of treating the surviving text as complete. The flag affects
/// only these AST-only rules; all SEM001–SEM023 diagnostics are computed exactly
/// as before.
pub fn analyze_ast_with_environment(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    let scopes = scopes::ScopeTree::build(ast);
    let functions = types::collect_user_functions(ast);
    let expressions = expressions::collect_facts(ast, &scopes);
    let calls = calls::collect_calls(ast, &scopes);
    let mut diagnostics = Vec::new();
    diagnostics.extend(rules::duplicate_globals::check(ast));
    diagnostics.extend(rules::duplicate_locals::check(&scopes));
    diagnostics.extend(rules::duplicate_events::check(ast));
    diagnostics.extend(rules::state_namespace::check(ast, &scopes));
    diagnostics.extend(rules::builtin_calls::check(
        &calls,
        &scopes,
        &functions,
        syntax_diagnostics,
    ));
    diagnostics.extend(rules::user_calls::check(
        &calls,
        &scopes,
        &functions,
        syntax_diagnostics,
    ));
    diagnostics.extend(rules::event_signatures::check(ast));
    diagnostics.extend(rules::explicit_casts::check(
        &expressions.expressions,
        &scopes,
        &functions,
    ));
    diagnostics.extend(rules::variable_initializers::check(
        &expressions.initializers,
        &scopes,
        &functions,
    ));
    diagnostics.extend(rules::global_initializers::check(
        &expressions.initializers,
        &scopes,
        environment_reliable,
    ));
    diagnostics.extend(rules::list_semantics::check_empty_states(
        ast,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::list_semantics::check(
        &expressions.expressions,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::firestorm_switches::check(
        ast,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::assignments::check(
        &expressions.expressions,
        &scopes,
        &functions,
    ));
    diagnostics.extend(rules::components::check(
        &expressions.expressions,
        &scopes,
        &functions,
    ));
    diagnostics.extend(rules::unresolved_variables::check(
        ast,
        &expressions.expressions,
        &calls,
        &scopes,
        &functions,
    ));
    diagnostics.extend(rules::deprecated_calls::check(&calls, &functions));
    diagnostics.extend(rules::state_targets::check(ast));
    diagnostics.extend(rules::state_transitions::check(
        ast,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::unreachable_states::check(
        ast,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::return_hazards::check(
        ast,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::discarded_comparisons::check(ast, syntax_diagnostics));
    diagnostics.extend(rules::condition_assignments::check(
        ast,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::detection_context::check(
        &calls,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(rules::math_errors::check(
        &expressions.expressions,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    diagnostics.extend(flow::check_returns(ast, &scopes, &functions));
    let jumps = flow::check_jumps(ast, &scopes);
    diagnostics.extend(jumps.diagnostics);
    diagnostics.extend(flow::check_missing_returns(
        ast,
        &scopes,
        &jumps.invalid_functions,
    ));
    let used = usage::used_symbols(&expressions.uses, &scopes);
    let reliable =
        usage::unit_is_reliable(ast, syntax_diagnostics, environment_reliable, &expressions);
    diagnostics.extend(rules::unused_variables::check(&scopes, &used, reliable));
    diagnostics.extend(rules::unused_parameters::check(&scopes, &used, reliable));
    diagnostics.extend(rules::unreachable_functions::check(
        ast, &calls, &functions, reliable,
    ));
    diagnostics
}
