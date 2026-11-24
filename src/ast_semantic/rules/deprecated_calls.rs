//! SEM013: calls to canonical deprecated LSL functions.

use std::collections::HashMap;

use crate::ast::ExpressionKind;
use crate::builtins;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::calls::CallSite;
use super::super::types::UserFunctionSignature;

pub(crate) fn check(
    calls: &[CallSite<'_>],
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<Diagnostic> {
    let deprecated = &builtins::definitions().deprecated_functions;
    let mut candidates = Vec::new();
    for site in calls {
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        if !deprecated.contains(&callee.name)
            || functions.contains_key(callee.name.as_str())
            || !site.call.matched_close
            || site.call.close_paren_span.start < site.call.open_paren_span.end
        {
            continue;
        }
        candidates.push((callee.span.start, callee.name.as_str(), callee.span));
    }
    candidates.sort_by_key(|(offset, _, _)| *offset);
    candidates
        .into_iter()
        .map(|(_, name, span)| Diagnostic {
            code: "SEM013",
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: format!("call to deprecated LSL function '{}'", name),
            span,
        })
        .collect()
}
