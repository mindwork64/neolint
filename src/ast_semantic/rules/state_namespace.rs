//! SEM021: duplicate states and state/global/function/built-in collisions.

use std::collections::HashMap;

use crate::ast::{Ast, TopLevel};
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

use super::super::scopes::{ScopeTree, SymbolKind};
use super::semantic_error;

const GOD_ONLY_BUILTIN_FUNCTIONS: &[&str] = &[
    "llGodLikeRezObject",
    "llSetInventoryPermMask",
    "llSetObjectPermMask",
];

pub(crate) fn check(ast: &Ast, scopes: &ScopeTree) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let root = scopes.scope(scopes.root());
    let mut globals: HashMap<&str, Span> = HashMap::new();
    let mut functions: HashMap<&str, Span> = HashMap::new();
    if let Some(root) = root {
        for symbol in &root.symbols {
            match symbol.kind {
                SymbolKind::GlobalVariable => {
                    globals.entry(symbol.name.as_str()).or_insert(symbol.span);
                }
                SymbolKind::Function => {
                    functions.entry(symbol.name.as_str()).or_insert(symbol.span);
                }
                SymbolKind::Parameter | SymbolKind::Local => {}
            }
        }
    }

    let mut seen_states: HashMap<&str, Span> = HashMap::new();
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.name_missing || state.name.is_empty() || state.body_missing || !state.body_closed {
            continue;
        }
        // Parser owns mandatory/unique default checks (PAR013/PAR014).
        if state.is_default || state.name == "default" {
            continue;
        }
        if let Some(previous) = seen_states.insert(state.name.as_str(), state.name_span) {
            diagnostics.push(semantic_error(
                "SEM021",
                format!(
                    "duplicate state '{}'; first declared at {}:{}",
                    state.name, previous.line, previous.column
                ),
                state.name_span,
            ));
        } else if let Some(previous) = globals.get(state.name.as_str()) {
            diagnostics.push(semantic_error(
                "SEM021",
                format!(
                    "state '{}' conflicts with a global variable declared at {}:{}",
                    state.name, previous.line, previous.column
                ),
                state.name_span,
            ));
        } else if let Some(previous) = functions.get(state.name.as_str()) {
            diagnostics.push(semantic_error(
                "SEM021",
                format!(
                    "state '{}' conflicts with a function declared at {}:{}",
                    state.name, previous.line, previous.column
                ),
                state.name_span,
            ));
        } else if builtins::definitions()
            .functions
            .contains_key(state.name.as_str())
            && !GOD_ONLY_BUILTIN_FUNCTIONS.contains(&state.name.as_str())
        {
            diagnostics.push(semantic_error(
                "SEM021",
                format!(
                    "state '{}' conflicts with a canonical built-in function",
                    state.name
                ),
                state.name_span,
            ));
        }
    }
    diagnostics
}
