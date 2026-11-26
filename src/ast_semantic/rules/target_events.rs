//! SEM049: canonical Second Life LSL event handlers absent from the target compiler.

use crate::ast::{Ast, TopLevel};
use crate::builtins::Signature;
use crate::diagnostic::Diagnostic;
use crate::target::{TargetPlatform, TargetProfile};

use super::semantic_error;

/// Report canonical Second Life event handlers not present in the selected OpenSim API version.
pub(crate) fn check(
    ast: &Ast,
    canonical_events: &std::collections::HashMap<String, Signature>,
    target_events: &std::collections::HashMap<String, Signature>,
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

    let version = target
        .opensim_api_version()
        .expect("OpenSim target always has an API version");
    let mut diagnostics = Vec::new();
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.body_missing || !state.body_closed {
            continue;
        }
        for event in &state.events {
            if event.name.is_empty()
                || !canonical_events.contains_key(event.name.as_str())
                || target_events.contains_key(event.name.as_str())
                || event.body.as_ref().map_or(true, |body| !body.closed)
            {
                continue;
            }
            diagnostics.push(semantic_error(
                "SEM049",
                format!(
                    "event '{}' is not supported by OpenSim {} (OS_APIVERSION {})",
                    event.name,
                    version.as_str(),
                    version.api_version()
                ),
                event.name_span,
            ));
        }
    }
    diagnostics
}
