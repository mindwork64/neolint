//! SEM002/SEM004: canonical built-in call arity and provable type mismatches.

use std::collections::HashMap;

use crate::ast::ExpressionKind;
use crate::builtins;
use crate::diagnostic::Diagnostic;

use super::super::calls::CallSite;
use super::super::scopes::ScopeTree;
use super::super::types::{
    can_implicitly_convert, expression_first_token_span, infer_expression_type,
    UserFunctionSignature,
};
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
        let Some(signature) = builtins::definitions().functions.get(callee.name.as_str()) else {
            continue;
        };
        if call_contains_syntax_error(site, syntax_diagnostics) {
            continue;
        }
        let expected_count = signature.parameters.len();
        if site.call.declared_argument_count != expected_count {
            diagnostics.push(semantic_error(
                "SEM002",
                format!(
                    "{} expects {} argument(s), but received {}",
                    signature.name, expected_count, site.call.declared_argument_count
                ),
                callee.span,
            ));
            continue;
        }
        if !site.call.arguments_complete {
            continue;
        }
        for (argument_index, (argument, parameter)) in site
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
            if !can_implicitly_convert(&actual_type, &parameter.type_name) {
                diagnostics.push(semantic_error(
                    "SEM004",
                    format!(
                        "{} argument {} has type {}, expected {}",
                        signature.name,
                        argument_index + 1,
                        actual_type,
                        parameter.type_name
                    ),
                    expression_first_token_span(argument),
                ));
            }
        }
    }
    diagnostics
}

pub(crate) fn call_contains_syntax_error(
    site: &CallSite<'_>,
    syntax_diagnostics: &[Diagnostic],
) -> bool {
    let open_end = site.call.open_paren_span.end;
    let close_start = site.call.close_paren_span.start;
    syntax_diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == crate::diagnostic::DiagnosticKind::Syntax
            && diagnostic.span.start >= open_end
            && diagnostic.span.start < close_start
    })
}
