//! SEM005/SEM006: user-function call arity and provable type mismatches.

use std::collections::HashMap;

use crate::ast::ExpressionKind;
use crate::diagnostic::Diagnostic;

use super::super::calls::CallSite;
use super::super::scopes::ScopeTree;
use super::super::types::{
    can_implicitly_convert, expression_first_token_span, infer_expression_type,
    UserFunctionSignature,
};
use super::builtin_calls::call_contains_syntax_error;
use super::semantic_error;

pub(crate) fn check(
    calls: &[CallSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for site in calls {
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        let Some(signature) = functions.get(callee.name.as_str()) else {
            continue;
        };
        if !site.call.matched_close || call_contains_syntax_error(site, syntax_diagnostics) {
            continue;
        }
        if site.call.declared_argument_count != signature.parameters.len() {
            diagnostics.push(semantic_error(
                "SEM005",
                format!(
                    "{} expects {} argument(s), but received {}",
                    callee.name,
                    signature.parameters.len(),
                    site.call.declared_argument_count
                ),
                callee.span,
            ));
            continue;
        }
        if !site.call.arguments_complete {
            continue;
        }
        for (argument_index, (argument, expected_type)) in site
            .call
            .arguments
            .iter()
            .zip(signature.parameters.iter())
            .enumerate()
        {
            let Some(actual_type) = infer_expression_type(argument, scopes, site.scope, functions)
            else {
                continue;
            };
            if !can_implicitly_convert(&actual_type, expected_type) {
                diagnostics.push(semantic_error(
                    "SEM006",
                    format!(
                        "{} argument {} has type {}, expected {}",
                        callee.name,
                        argument_index + 1,
                        actual_type,
                        expected_type
                    ),
                    expression_first_token_span(argument),
                ));
            }
        }
    }
    diagnostics
}
