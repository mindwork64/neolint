//! SEM042: detection-context-only calls outside their valid events.

use std::collections::{HashMap, VecDeque};

use crate::ast::ExpressionKind;
use crate::builtins;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::calls::{CallOwner, CallSite, FunctionId};
use super::super::types::UserFunctionSignature;

const MAX_CONTEXT_NODES: usize = 4_096;
const MAX_CONTEXT_EDGES: usize = 8_192;
const MAX_CONTEXT_WORK: usize = 16_384;

const CONTEXT_TOUCH: u16 = 1 << 0;
const CONTEXT_COLLISION: u16 = 1 << 1;
const CONTEXT_SENSOR: u16 = 1 << 2;
const CONTEXT_ON_DAMAGE: u16 = 1 << 3;
const CONTEXT_FINAL_DAMAGE: u16 = 1 << 4;
const CONTEXT_OTHER: u16 = 1 << 5;

const CONTEXT_DAMAGE: u16 = CONTEXT_ON_DAMAGE | CONTEXT_FINAL_DAMAGE;

/// Warn when a detection-related function is reachable from an event that
/// cannot provide the detection data it requires.
///
/// Context is propagated through direct user-function calls. Unknown extension
/// events do not contribute a context, and unreferenced functions are ignored.
pub(crate) fn check(
    calls: &[CallSite<'_>],
    functions: &HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable
        || syntax_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Vec::new();
    }

    let Some(contexts) = function_contexts(calls, functions) else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    for site in calls {
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        if functions.contains_key(callee.name.as_str()) {
            continue;
        }
        let Some(allowed) = allowed_context(callee.name.as_str()) else {
            continue;
        };

        let actual = match site.owner {
            CallOwner::GlobalInitializer => CONTEXT_OTHER,
            CallOwner::EventHandler => site.event_name.and_then(event_context).unwrap_or_default(),
            CallOwner::Function(owner) => contexts.get(&owner).copied().unwrap_or_default(),
        };
        if actual == 0 || actual & !allowed == 0 {
            continue;
        }

        diagnostics.push(Diagnostic {
            code: "SEM042",
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: format!(
                "{} is not available in every event context that can reach this call",
                callee.name
            ),
            span: callee.span,
        });
    }

    diagnostics.sort_by_key(|diagnostic| diagnostic.span.start);
    diagnostics
}

/// Find the event contexts that can reach each user function.
///
/// The graph is intentionally bounded. An incomplete graph suppresses this
/// rule rather than producing findings from partial call information.
fn function_contexts(
    calls: &[CallSite<'_>],
    functions: &HashMap<String, UserFunctionSignature>,
) -> Option<HashMap<FunctionId, u16>> {
    if functions.len() > MAX_CONTEXT_NODES {
        return None;
    }

    let function_ids: HashMap<&str, FunctionId> = functions
        .iter()
        .map(|(name, signature)| (name.as_str(), signature.declaration_span.start))
        .collect();

    let mut adjacency: HashMap<FunctionId, Vec<FunctionId>> = HashMap::new();
    let mut contexts: HashMap<FunctionId, u16> = HashMap::new();
    let mut pending = VecDeque::new();
    let mut edge_count = 0usize;
    let mut work = 0usize;

    for site in calls {
        spend_work(&mut work, 1)?;
        if !site.call.matched_close
            || site.call.close_paren_span.start < site.call.open_paren_span.end
        {
            continue;
        }

        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        if builtins::definitions()
            .functions
            .contains_key(callee.name.as_str())
        {
            continue;
        }
        let Some(&target) = function_ids.get(callee.name.as_str()) else {
            continue;
        };

        match site.owner {
            CallOwner::Function(owner) => {
                edge_count = edge_count.checked_add(1)?;
                if edge_count > MAX_CONTEXT_EDGES {
                    return None;
                }
                adjacency.entry(owner).or_default().push(target);
            }
            CallOwner::GlobalInitializer => {
                add_context(target, CONTEXT_OTHER, &mut contexts, &mut pending)?;
            }
            CallOwner::EventHandler => {
                let Some(context) = site.event_name.and_then(event_context) else {
                    continue;
                };
                add_context(target, context, &mut contexts, &mut pending)?;
            }
        }
    }

    while let Some(owner) = pending.pop_front() {
        spend_work(&mut work, 1)?;
        let context = contexts.get(&owner).copied().unwrap_or_default();
        let Some(targets) = adjacency.get(&owner) else {
            continue;
        };
        for &target in targets {
            spend_work(&mut work, 1)?;
            add_context(target, context, &mut contexts, &mut pending)?;
        }
    }

    Some(contexts)
}

fn add_context(
    function: FunctionId,
    context: u16,
    contexts: &mut HashMap<FunctionId, u16>,
    pending: &mut VecDeque<FunctionId>,
) -> Option<()> {
    let previous = contexts.get(&function).copied().unwrap_or_default();
    let combined = previous | context;
    if combined == previous {
        return Some(());
    }
    if !contexts.contains_key(&function) && contexts.len() >= MAX_CONTEXT_NODES {
        return None;
    }
    contexts.insert(function, combined);
    pending.push_back(function);
    Some(())
}

fn spend_work(work: &mut usize, amount: usize) -> Option<()> {
    *work = work.checked_add(amount)?;
    (*work <= MAX_CONTEXT_WORK).then_some(())
}

/// Return a context only for canonical events whose behavior is known.
fn event_context(name: &str) -> Option<u16> {
    match name {
        "touch" | "touch_start" | "touch_end" => Some(CONTEXT_TOUCH),
        "collision" | "collision_start" | "collision_end" => Some(CONTEXT_COLLISION),
        "sensor" => Some(CONTEXT_SENSOR),
        "on_damage" => Some(CONTEXT_ON_DAMAGE),
        "final_damage" => Some(CONTEXT_FINAL_DAMAGE),
        _ => builtins::definitions()
            .events
            .contains_key(name)
            .then_some(CONTEXT_OTHER),
    }
}

/// Event masks for canonical functions that require detection data.
fn allowed_context(name: &str) -> Option<u16> {
    if !builtins::definitions().functions.contains_key(name) {
        return None;
    }

    match name {
        "llAdjustDamage" => Some(CONTEXT_ON_DAMAGE),
        "llDetectedDamage" => Some(CONTEXT_DAMAGE),
        "llDetectedGrab" => Some(CONTEXT_TOUCH),
        "llDetectedLinkNumber" => Some(CONTEXT_TOUCH | CONTEXT_COLLISION),
        name if name.starts_with("llDetectedTouch") => Some(CONTEXT_TOUCH),
        name if name.starts_with("llDetected") => {
            Some(CONTEXT_TOUCH | CONTEXT_COLLISION | CONTEXT_SENSOR | CONTEXT_DAMAGE)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_detection_functions_have_known_context_rules() {
        let definitions = builtins::definitions();
        let detected: Vec<&str> = definitions
            .functions
            .keys()
            .filter(|name| name.starts_with("llDetected"))
            .map(String::as_str)
            .collect();

        assert!(!detected.is_empty());
        for name in detected {
            assert!(
                allowed_context(name).is_some(),
                "missing event-context policy for {name}"
            );
        }
        assert_eq!(allowed_context("llAdjustDamage"), Some(CONTEXT_ON_DAMAGE));
        assert!(definitions.events.contains_key("on_damage"));
        assert!(definitions.events.contains_key("final_damage"));
    }
}
