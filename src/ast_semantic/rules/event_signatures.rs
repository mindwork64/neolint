//! SEM003: canonical event handler parameter count and type checks.

use crate::ast::{Ast, TopLevel};
use crate::builtins;
use crate::diagnostic::Diagnostic;

use super::super::scopes::type_name;
use super::semantic_error;

pub(crate) fn check(ast: &Ast) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let events = &builtins::definitions().events;
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        for event in &state.events {
            if event.name.is_empty() {
                continue;
            }
            let Some(signature) = events.get(event.name.as_str()) else {
                continue;
            };
            let actual_count = event.declared_parameter_count;
            if actual_count != signature.parameters.len() {
                diagnostics.push(semantic_error(
                    "SEM003",
                    format!(
                        "event {} expects {} parameter(s), but {} were declared",
                        signature.name,
                        signature.parameters.len(),
                        actual_count
                    ),
                    event.name_span,
                ));
                continue;
            }
            for (parameter_index, (actual, expected)) in event
                .parameters
                .iter()
                .zip(signature.parameters.iter())
                .enumerate()
            {
                let actual_type = type_name(actual.ty.kind);
                if actual_type != expected.type_name {
                    diagnostics.push(semantic_error(
                        "SEM003",
                        format!(
                            "event {} parameter {} has type {}, expected {}",
                            signature.name,
                            parameter_index + 1,
                            actual_type,
                            expected.type_name
                        ),
                        event.name_span,
                    ));
                }
            }
        }
    }
    diagnostics
}
