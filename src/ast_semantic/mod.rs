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
use crate::builtins;
use crate::cancellation::AnalysisCancellationToken;
use crate::diagnostic::Diagnostic;
use crate::target::{OsslAllowlist, TargetProfile};

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
/// assignment, detection-context, constant-math, state-reachability and
/// operator-type and binary-operator compatibility checks fail closed instead
/// of treating the surviving text as complete. The flag affects
/// only these AST-only rules; all SEM001–SEM023 diagnostics are computed exactly
/// as before.
pub fn analyze_ast_with_environment(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    analyze_ast_with_target_options(
        ast,
        syntax_diagnostics,
        environment_reliable,
        TargetProfile::default(),
        &OsslAllowlist::default(),
    )
}

/// Run AST semantic analysis for an explicit platform/runtime profile.
///
/// The target profile is independent from the preprocessor dialect. Rules that
/// lack verified behavior for a selected target fail closed rather than
/// inheriting Second Life Mono assumptions.
pub fn analyze_ast_with_target(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
    target: TargetProfile,
) -> Vec<Diagnostic> {
    analyze_ast_with_target_options(
        ast,
        syntax_diagnostics,
        environment_reliable,
        target,
        &OsslAllowlist::default(),
    )
}

/// Run AST semantic analysis with an explicit target and OSSL allowlist.
pub fn analyze_ast_with_target_options(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
    target: TargetProfile,
    ossl_allowlist: &OsslAllowlist,
) -> Vec<Diagnostic> {
    analyze_ast_with_target_options_and_god_mode(
        ast,
        syntax_diagnostics,
        environment_reliable,
        target,
        ossl_allowlist,
        false,
    )
}

/// Run target-aware AST semantics with an explicit project God Mode setting.
pub(crate) fn analyze_ast_with_target_options_and_god_mode(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
    target: TargetProfile,
    ossl_allowlist: &OsslAllowlist,
    god_mode_enabled: bool,
) -> Vec<Diagnostic> {
    analyze_ast_with_target_options_and_god_mode_controlled(
        ast,
        syntax_diagnostics,
        environment_reliable,
        target,
        ossl_allowlist,
        god_mode_enabled,
        None,
    )
    .expect("analysis without a cancellation token cannot be cancelled")
}

/// Cancellation-aware form used by the LSP worker. Checkpoints sit between
/// independent semantic passes, so a cancelled run never returns partial diagnostics.
pub(crate) fn analyze_ast_with_target_options_and_god_mode_cancellable(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
    target: TargetProfile,
    ossl_allowlist: &OsslAllowlist,
    god_mode_enabled: bool,
    cancellation: &AnalysisCancellationToken,
) -> Option<Vec<Diagnostic>> {
    analyze_ast_with_target_options_and_god_mode_controlled(
        ast,
        syntax_diagnostics,
        environment_reliable,
        target,
        ossl_allowlist,
        god_mode_enabled,
        Some(cancellation),
    )
}

#[allow(clippy::too_many_arguments)]
fn analyze_ast_with_target_options_and_god_mode_controlled(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
    target: TargetProfile,
    ossl_allowlist: &OsslAllowlist,
    god_mode_enabled: bool,
    cancellation: Option<&AnalysisCancellationToken>,
) -> Option<Vec<Diagnostic>> {
    macro_rules! checkpoint {
        () => {
            if cancellation.is_some_and(|token| token.is_cancelled()) {
                return None;
            }
        };
    }
    checkpoint!();
    let api_catalog = builtins::catalog_for_target(target);
    let standard_functions = &api_catalog.functions;
    let ossl_functions = &api_catalog.ossl_functions;
    let builtin_functions = &api_catalog.callable_functions;
    let builtin_events = &api_catalog.events;
    let scopes = scopes::ScopeTree::build_with_catalog(ast, api_catalog, target);
    checkpoint!();
    let functions = types::collect_user_functions(ast);
    checkpoint!();
    let expressions = expressions::collect_facts(ast, &scopes);
    checkpoint!();
    let calls = calls::collect_calls(ast, &scopes);
    checkpoint!();
    let mut diagnostics = Vec::new();
    diagnostics.extend(rules::duplicate_globals::check(ast));
    diagnostics.extend(rules::duplicate_locals::check(&scopes));
    diagnostics.extend(rules::duplicate_events::check(ast, builtin_events));
    diagnostics.extend(rules::state_namespace::check_with_builtins(
        ast,
        &scopes,
        builtin_functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::builtin_calls::check_with_builtins(
        &calls,
        &scopes,
        &functions,
        syntax_diagnostics,
        builtin_functions,
        target,
    ));
    checkpoint!();
    diagnostics.extend(rules::user_calls::check(
        &calls,
        &scopes,
        &functions,
        syntax_diagnostics,
    ));
    checkpoint!();
    diagnostics.extend(rules::event_signatures::check(ast, builtin_events));
    diagnostics.extend(rules::explicit_casts::check(
        &expressions.expressions,
        &scopes,
        &functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::variable_initializers::check(
        &expressions.initializers,
        &scopes,
        &functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::global_initializers::check(
        &expressions.initializers,
        &scopes,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::list_semantics::check_empty_states(
        ast,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::list_semantics::check(
        &expressions.expressions,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::firestorm_switches::check(
        ast,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::assignments::check(
        &expressions.expressions,
        &scopes,
        &functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::components::check(
        &expressions.expressions,
        &scopes,
        &functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::unresolved_variables::check_with_builtins(
        ast,
        &expressions.expressions,
        &calls,
        &scopes,
        &functions,
        builtin_functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::deprecated_calls::check_with_builtins(
        &calls,
        &functions,
        builtin_functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::state_targets::check(ast, builtin_events));
    diagnostics.extend(rules::legacy_compat::check(
        ast,
        &calls,
        &scopes,
        syntax_diagnostics,
        environment_reliable,
        target,
        god_mode_enabled,
    ));
    checkpoint!();
    diagnostics.extend(rules::state_transitions::check(
        ast,
        syntax_diagnostics,
        environment_reliable,
        target,
    ));
    checkpoint!();
    diagnostics.extend(rules::unreachable_states::check_with_builtins(
        ast,
        syntax_diagnostics,
        environment_reliable,
        builtin_functions,
    ));
    checkpoint!();
    let jumps = flow::check_jumps(ast, &scopes, target);
    diagnostics.extend(rules::return_hazards::check(
        ast,
        &scopes,
        syntax_diagnostics,
        environment_reliable,
        &jumps.invalid_code_scopes,
    ));
    checkpoint!();
    diagnostics.extend(rules::discarded_comparisons::check(ast, syntax_diagnostics));
    diagnostics.extend(rules::condition_assignments::check(
        ast,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::detection_context::check_with_builtins(
        &calls,
        &functions,
        syntax_diagnostics,
        environment_reliable,
        builtin_functions,
        builtin_events,
    ));
    checkpoint!();
    diagnostics.extend(rules::math_errors::check_with_builtins(
        &expressions.expressions,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
        builtin_functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::operator_types::check(
        &expressions.expressions,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::binary_operator_types::check(
        &expressions.expressions,
        &scopes,
        &functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(flow::check_returns(ast, &scopes, &functions));
    diagnostics.extend(jumps.diagnostics);
    diagnostics.extend(flow::check_missing_returns(
        ast,
        &scopes,
        &jumps.invalid_functions,
    ));
    checkpoint!();
    let used = usage::used_symbols(&expressions.uses, &scopes);
    let reliable =
        usage::unit_is_reliable(ast, syntax_diagnostics, environment_reliable, &expressions);
    diagnostics.extend(rules::unused_variables::check(&scopes, &used, reliable));
    diagnostics.extend(rules::unused_parameters::check(&scopes, &used, reliable));
    diagnostics.extend(rules::unreachable_functions::check_with_builtins(
        ast,
        &calls,
        &functions,
        reliable,
        builtin_functions,
    ));
    checkpoint!();
    diagnostics.extend(rules::target_api::check(
        &calls,
        &functions,
        standard_functions,
        target,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::ossl_allowlist::check(
        &calls,
        &functions,
        target,
        ossl_allowlist,
        ossl_functions,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::target_events::check(
        ast,
        &builtins::definitions().events,
        builtin_events,
        target,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    diagnostics.extend(rules::target_constants::check(
        &expressions.expressions,
        &scopes,
        &builtins::definitions().constants,
        scopes.builtin_constants(),
        target,
        syntax_diagnostics,
        environment_reliable,
    ));
    checkpoint!();
    Some(diagnostics)
}
