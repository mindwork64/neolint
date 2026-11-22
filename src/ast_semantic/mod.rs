//! Independent structural semantic analysis over the typed AST.
//!
//! Phase 6A migrates only SEM001, SEM007, SEM020 and SEM021. The module is not
//! wired into the CLI or pipeline and does not import preprocessor or SourceMap
//! internals.

pub mod scopes;

pub(crate) mod calls;
pub(crate) mod expressions;
pub(crate) mod flow;
pub(crate) mod rules;
pub(crate) mod types;

use crate::ast::Ast;
use crate::diagnostic::Diagnostic;

/// Run the Phase 6A structural AST semantic rules.
///
/// `syntax_diagnostics` is accepted for symmetry with the future AST semantic
/// engine; these four structural rules do not require syntax suppression.
pub fn analyze_ast(ast: &Ast, syntax_diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    let _ = syntax_diagnostics;
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
    diagnostics.extend(flow::check_returns(ast, &scopes, &functions));
    let jumps = flow::check_jumps(ast, &scopes);
    diagnostics.extend(jumps.diagnostics);
    diagnostics.extend(flow::check_missing_returns(
        ast,
        &scopes,
        &jumps.invalid_functions,
    ));
    diagnostics
}
