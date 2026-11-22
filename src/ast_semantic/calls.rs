//! Call-site view over the shared expression traversal.

use crate::ast::{CallExpression, ExpressionKind};

use super::expressions::{ExpressionFacts, ExpressionSite};
use super::scopes::ScopeId;

/// One call expression together with the lexical scope where it appears.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallSite<'a> {
    pub(crate) call: &'a CallExpression,
    pub(crate) scope: ScopeId,
}

/// Collect every call expression exactly once in shared traversal order.
pub(crate) fn collect_calls<'a>(facts: &ExpressionFacts<'a>) -> Vec<CallSite<'a>> {
    facts
        .expressions
        .iter()
        .copied()
        .filter_map(call_site)
        .collect()
}

fn call_site(site: ExpressionSite<'_>) -> Option<CallSite<'_>> {
    let ExpressionSite { expression, scope } = site;
    let ExpressionKind::Call(call) = &expression.kind else {
        return None;
    };
    Some(CallSite { call, scope })
}
