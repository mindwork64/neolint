//! SEM012: unresolved variable reference warnings.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, ExpressionKind, TopLevel};
use crate::builtins;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::calls::CallSite;
use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::super::types::UserFunctionSignature;

#[derive(Clone, Copy)]
struct ByteRange {
    start: usize,
    end: usize,
}

impl ByteRange {
    fn contains(self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

pub(crate) fn check(
    ast: &Ast,
    sites: &[ExpressionSite<'_>],
    calls: &[CallSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<Diagnostic> {
    let mut excluded = unknown_event_ranges(ast);
    excluded.extend(unknown_call_argument_ranges(calls, functions));
    let mut callees = HashSet::new();
    for site in calls {
        if let ExpressionKind::Identifier(callee) = &site.call.callee.kind {
            callees.insert((callee.span.start, callee.span.end));
        }
    }
    let constants = &builtins::definitions().constants;
    let mut diagnostics = Vec::new();
    for site in sites {
        let ExpressionKind::Identifier(identifier) = &site.expression.kind else {
            continue;
        };
        if excluded
            .iter()
            .any(|range| range.contains(identifier.span.start))
        {
            continue;
        }
        if callees.contains(&(identifier.span.start, identifier.span.end))
            || constants.contains_key(identifier.name.as_str())
            || scopes
                .lookup_value_at(site.scope, &identifier.name, identifier.span.start)
                .is_some()
        {
            continue;
        }
        diagnostics.push(Diagnostic {
            code: "SEM012",
            severity: Severity::Warning,
            kind: DiagnosticKind::Semantic,
            message: format!(
                "variable '{}' is not declared in the current script; verify that it is provided by the scripting environment",
                identifier.name
            ),
            span: identifier.span,
        });
    }
    diagnostics
}

fn unknown_event_ranges(ast: &Ast) -> Vec<ByteRange> {
    let canonical = &builtins::definitions().events;
    let mut ranges = Vec::new();
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.body_missing || !state.body_closed {
            continue;
        }
        for event in &state.events {
            if event.name.is_empty() || canonical.contains_key(event.name.as_str()) {
                continue;
            }
            let Some(body) = &event.body else {
                continue;
            };
            if !body.closed {
                continue;
            }
            ranges.push(ByteRange {
                start: body.span.start.saturating_add(1),
                end: body.span.end.saturating_sub(1),
            });
        }
    }
    ranges
}

fn unknown_call_argument_ranges(
    calls: &[CallSite<'_>],
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<ByteRange> {
    let definitions = builtins::definitions();
    let mut ranges = Vec::new();
    for site in calls {
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        if definitions.functions.contains_key(callee.name.as_str())
            || definitions.events.contains_key(callee.name.as_str())
            || functions.contains_key(callee.name.as_str())
        {
            continue;
        }
        if !site.call.matched_close {
            continue;
        }
        let open = &site.call.open_paren_span;
        let close = &site.call.close_paren_span;
        if close.start < open.end || site.call.span.end < close.end {
            continue;
        }
        ranges.push(ByteRange {
            start: open.end,
            end: close.start,
        });
    }
    ranges
}
