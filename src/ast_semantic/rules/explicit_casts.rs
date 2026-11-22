//! SEM014: unsupported explicit casts.

use std::collections::HashMap;

use crate::ast::ExpressionKind;
use crate::diagnostic::Diagnostic;

use super::super::expressions::ExpressionSite;
use super::super::scopes::{type_name, ScopeTree};
use super::super::types::{explicit_cast_supported, infer_expression_type, UserFunctionSignature};
use super::semantic_error;

pub(crate) fn check(
    sites: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for site in sites {
        let ExpressionKind::Cast(cast) = &site.expression.kind else {
            continue;
        };
        let Some(source_type) = infer_expression_type(&cast.operand, scopes, site.scope, functions)
        else {
            continue;
        };
        let target_type = type_name(cast.ty.kind);
        if !explicit_cast_supported(&source_type, target_type) {
            diagnostics.push(semantic_error(
                "SEM014",
                format!(
                    "cannot explicitly cast {} to {} in LSL",
                    source_type, target_type
                ),
                cast.ty.span,
            ));
        }
    }
    diagnostics
}
