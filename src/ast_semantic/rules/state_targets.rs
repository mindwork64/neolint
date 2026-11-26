//! SEM022: state-change targets that are not defined.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, StatementKind, TopLevel};
use crate::diagnostic::Diagnostic;

use super::super::statements::visit_statements;
use super::semantic_error;

pub(crate) fn check(
    ast: &Ast,
    events: &HashMap<String, crate::builtins::Signature>,
) -> Vec<Diagnostic> {
    let mut known_states = HashSet::new();
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.body_missing || !state.body_closed || state.name.is_empty() {
            continue;
        }
        known_states.insert(state.name.as_str());
    }

    let mut diagnostics = Vec::new();
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.body_missing || !state.body_closed {
            continue;
        }
        for event in &state.events {
            if event.name.is_empty() || !events.contains_key(event.name.as_str()) {
                continue;
            }
            let Some(body) = &event.body else {
                continue;
            };
            if !body.closed {
                continue;
            }
            visit_statements(body, |statement| {
                let StatementKind::StateChange(change) = &statement.kind else {
                    return;
                };
                // Legacy does not treat the `default` keyword token as an
                // Identifier target for SEM022.
                if change.state == "default" || known_states.contains(change.state.as_str()) {
                    return;
                }
                diagnostics.push(semantic_error(
                    "SEM022",
                    format!(
                        "state target '{}' is not defined; add that state or use state default",
                        change.state
                    ),
                    change.state_span,
                ));
            });
        }
    }
    diagnostics
}
