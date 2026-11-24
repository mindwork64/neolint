//! SEM026: user functions that no event handler or global initializer reaches.
//!
//! All graph policy, budgets and first-definition handling live in
//! [`super::super::callgraph`]. This rule only renders one finding per
//! unreachable node in declaration order, and emits nothing when the unit is not
//! reliable or the graph was suppressed by a budget overflow.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::callgraph::unreachable_functions;
use super::super::calls::CallSite;
use super::super::types::UserFunctionSignature;

pub(crate) fn check(
    ast: &Ast,
    calls: &[CallSite<'_>],
    functions: &HashMap<String, UserFunctionSignature>,
    reliable: bool,
) -> Vec<Diagnostic> {
    if !reliable {
        return Vec::new();
    }

    let reachability = unreachable_functions(ast, calls, functions);
    if reachability.suppressed {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for function in reachability.unreachable {
        debug_assert_eq!(
            function.id, function.name_span.start,
            "call-graph node identity is the declaration name-span start"
        );
        diagnostics.push(Diagnostic {
            code: "SEM026",
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: format!("function '{}' is never called", function.name),
            span: function.name_span,
        });
    }
    diagnostics
}
