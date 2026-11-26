//! SEM021: duplicate states and state/global/function/built-in collisions.

use std::collections::HashMap;

use crate::ast::{Ast, TopLevel};
use crate::builtins::FunctionCatalog;
use crate::diagnostic::{Diagnostic, Span};

use super::super::scopes::{ScopeTree, SymbolKind};
use super::semantic_error;

const GOD_ONLY_BUILTIN_FUNCTIONS: &[&str] = &[
    "llGodLikeRezObject",
    "llSetInventoryPermMask",
    "llSetObjectPermMask",
];

pub(crate) fn check_with_builtins(
    ast: &Ast,
    scopes: &ScopeTree,
    builtin_functions: &FunctionCatalog,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let root = scopes.scope(scopes.root());
    let mut globals: HashMap<&str, Span> = HashMap::new();
    let mut functions: HashMap<&str, Span> = HashMap::new();
    if let Some(root) = root {
        for symbol in &root.symbols {
            if symbol.kind == SymbolKind::GlobalVariable {
                globals.entry(symbol.name.as_str()).or_insert(symbol.span);
            }
        }
    }
    // Legacy collect_user_functions only treats a declaration as a user
    // function when its parameter list is followed by a body. Sort normal and
    // recoverable declarations by source position so the first definition wins.
    let mut function_candidates = Vec::new();
    for item in &ast.items {
        if let TopLevel::Function(function) = item {
            if function.body.is_some() && !function.name.is_empty() {
                function_candidates.push((
                    function.name_span.start,
                    function.name.as_str(),
                    function.name_span,
                ));
            }
        }
    }
    for function in &ast.recovered_functions {
        function_candidates.push((
            function.name_span.start,
            function.name.as_str(),
            function.name_span,
        ));
    }
    function_candidates.sort_by_key(|(offset, _, _)| *offset);
    for (_, name, span) in function_candidates {
        functions.entry(name).or_insert(span);
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
        } else if builtin_functions.contains_key(state.name.as_str())
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
