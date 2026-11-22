//! SEM020: duplicate canonical event handler within one state.

use std::collections::HashMap;

use crate::ast::{Ast, TopLevel};
use crate::builtins;
use crate::diagnostic::Diagnostic;

use super::semantic_error;

pub(crate) fn check(ast: &Ast) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let canonical_events = &builtins::definitions().events;
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.name_missing || state.name.is_empty() || state.body_missing || !state.body_closed {
            continue;
        }
        let mut seen: HashMap<&str, crate::diagnostic::Span> = HashMap::new();
        for event in &state.events {
            if event.name.is_empty()
                || !canonical_events.contains_key(event.name.as_str())
                || event.body.as_ref().map_or(true, |body| !body.closed)
            {
                continue;
            }
            if let Some(previous) = seen.insert(event.name.as_str(), event.name_span) {
                diagnostics.push(semantic_error(
                    "SEM020",
                    format!(
                        "duplicate event handler '{}' in state '{}'; first declared at {}:{}",
                        event.name, state.name, previous.line, previous.column
                    ),
                    event.name_span,
                ));
            }
        }
    }
    diagnostics
}
