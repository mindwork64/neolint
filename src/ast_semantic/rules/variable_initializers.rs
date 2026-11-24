//! SEM015: incompatible variable initializer types.

use std::collections::HashMap;

use crate::diagnostic::Diagnostic;

use super::super::expressions::InitializerSite;
use super::super::scopes::{type_name, ScopeTree};
use super::super::types::{can_implicitly_convert, infer_expression_type, UserFunctionSignature};
use super::semantic_error;

pub(crate) fn check(
    initializers: &[InitializerSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for site in initializers {
        let declarator = site.declarator;
        if declarator.name_error.is_some()
            || declarator.name.is_empty()
            || declarator.initializer_error.is_some()
        {
            continue;
        }
        let Some(initializer) = &declarator.initializer else {
            continue;
        };
        let expected = type_name(site.expected);
        let Some(actual_type) = infer_expression_type(initializer, scopes, site.scope, functions)
        else {
            continue;
        };
        if !can_implicitly_convert(&actual_type, expected) {
            diagnostics.push(semantic_error(
                "SEM015",
                format!(
                    "initializer for variable '{}' has type {}, expected {}",
                    declarator.name, actual_type, expected
                ),
                declarator.name_span,
            ));
        }
    }
    diagnostics
}
