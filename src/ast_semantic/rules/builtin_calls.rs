//! SEM002/SEM004: canonical built-in call arity and provable type mismatches.

use std::collections::HashMap;

use crate::ast::ExpressionKind;
use crate::builtins::FunctionCatalog;
use crate::diagnostic::Diagnostic;
use crate::target::TargetProfile;

use super::super::calls::CallSite;
use super::super::scopes::ScopeTree;
use super::super::types::{
    best_builtin_overload, can_implicitly_convert, expression_first_token_span,
    infer_expression_type_with_builtins, UserFunctionSignature,
};
use super::semantic_error;

#[cfg(test)]
pub(crate) fn check(
    calls: &[CallSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
) -> Vec<Diagnostic> {
    check_with_builtins(
        calls,
        scopes,
        functions,
        syntax_diagnostics,
        crate::builtins::functions_for_target(TargetProfile::default()),
        TargetProfile::default(),
    )
}

pub(crate) fn check_with_builtins(
    calls: &[CallSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
    builtin_functions: &FunctionCatalog,
    target: TargetProfile,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for site in calls {
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        let Some(signatures) = builtin_functions.get(callee.name.as_str()) else {
            continue;
        };
        if !site.call.matched_close || call_contains_syntax_error(site, syntax_diagnostics) {
            continue;
        }
        let mut expected_counts: Vec<usize> = signatures
            .iter()
            .map(|signature| signature.parameters.len())
            .collect();
        expected_counts.sort_unstable();
        expected_counts.dedup();
        if !expected_counts.contains(&site.call.declared_argument_count) {
            let expected = expected_counts
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(" or ");
            diagnostics.push(semantic_error(
                "SEM002",
                format!(
                    "{} expects {} argument(s), but received {}",
                    callee.name, expected, site.call.declared_argument_count
                ),
                callee.span,
            ));
            continue;
        }
        if !site.call.arguments_complete {
            continue;
        }
        let argument_types: Vec<_> = site
            .call
            .arguments
            .iter()
            .map(|argument| {
                infer_expression_type_with_builtins(
                    argument,
                    scopes,
                    site.scope,
                    functions,
                    builtin_functions,
                )
            })
            .collect();
        let Some(signature) = best_builtin_overload(signatures, &argument_types, target) else {
            continue;
        };
        for (argument_index, (argument, parameter)) in site
            .call
            .arguments
            .iter()
            .zip(signature.parameters.iter())
            .enumerate()
        {
            let Some(actual_type) = argument_types
                .get(argument_index)
                .and_then(Option::as_deref)
            else {
                continue;
            };
            if !can_convert_argument_for_target(actual_type, &parameter.type_name, target) {
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

fn can_convert_argument_for_target(
    actual_type: &str,
    expected_type: &str,
    target: TargetProfile,
) -> bool {
    can_implicitly_convert(actual_type, expected_type)
        // OpenSim's LSL key wrapper exposes an implicit conversion to
        // System.String. This is a runtime compiler detail, not standard LSL.
        || (target.is_opensim()
            && matches!(
                (actual_type, expected_type),
                ("key", "string") | ("string", "key")
            ))
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

#[cfg(test)]
mod target_conversion_tests {
    use super::*;

    #[test]
    fn opensim_key_may_convert_to_interface_string_but_sl_does_not() {
        let opensim = TargetProfile::parse("opensim-yengine").unwrap();
        let mono = TargetProfile::default();
        assert!(can_convert_argument_for_target("key", "string", opensim));
        assert!(!can_convert_argument_for_target("key", "string", mono));
        assert!(can_convert_argument_for_target("string", "key", opensim));
    }
}
