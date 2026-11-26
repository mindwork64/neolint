//! SEM047: canonical Second Life LSL functions absent from an OpenSim runtime API.

use std::collections::HashMap;

use crate::ast::ExpressionKind;
use crate::builtins::{self, FunctionCatalog};
use crate::diagnostic::Diagnostic;
use crate::target::{TargetPlatform, TargetProfile};

use super::super::calls::CallSite;
use super::super::types::UserFunctionSignature;
use super::semantic_error;

/// Report calls to canonical LL functions missing from the selected OpenSim interface.
pub(crate) fn check(
    calls: &[CallSite<'_>],
    user_functions: &HashMap<String, UserFunctionSignature>,
    builtin_functions: &FunctionCatalog,
    target: TargetProfile,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if target.platform() != TargetPlatform::OpenSim
        || !environment_reliable
        || !syntax_diagnostics.is_empty()
    {
        return Vec::new();
    }

    let canonical = &builtins::definitions().functions;
    let mut diagnostics = Vec::new();
    for site in calls {
        if !site.call.matched_close {
            continue;
        }
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        if user_functions.contains_key(callee.name.as_str())
            || !canonical.contains_key(callee.name.as_str())
            || builtin_functions.contains_key(callee.name.as_str())
        {
            continue;
        }
        diagnostics.push(semantic_error(
            "SEM047",
            format!(
                "{} is not exposed by the selected OpenSim {} LSL API",
                callee.name,
                target.runtime().as_str()
            ),
            callee.span,
        ));
    }
    diagnostics
}
