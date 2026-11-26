//! SEM048: OSSL calls outside the project's explicit region allowlist.

use std::collections::HashMap;

use crate::ast::ExpressionKind;
use crate::builtins::FunctionCatalog;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};
use crate::target::{OsslAllowlist, TargetProfile};

use super::super::calls::CallSite;
use super::super::types::UserFunctionSignature;

/// Warn when a known OSSL call is not in the explicitly configured project allowlist.
///
/// The rule does not claim to know the live region security configuration. It
/// checks only the API name catalogue and the user's declared project expectation.
pub(crate) fn check(
    calls: &[CallSite<'_>],
    user_functions: &HashMap<String, UserFunctionSignature>,
    target: TargetProfile,
    allowlist: &OsslAllowlist,
    known_functions: &FunctionCatalog,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !target.is_opensim()
        || !allowlist.is_explicit()
        || !environment_reliable
        || !syntax_diagnostics.is_empty()
    {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    for site in calls {
        if !site.call.matched_close {
            continue;
        }
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        if user_functions.contains_key(callee.name.as_str())
            || !known_functions.contains_key(callee.name.as_str())
            || allowlist.allows(callee.name.as_str())
        {
            continue;
        }
        diagnostics.push(Diagnostic {
            code: "SEM048",
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: format!(
                "{} is not listed in the project's OpenSim OSSL allowlist; verify the region's actual OSSL configuration",
                callee.name
            ),
            span: callee.span,
        });
    }
    diagnostics
}
