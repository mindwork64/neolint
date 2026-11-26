//! SEM027: global initializers must use LSL simple-assignable values.

use std::collections::HashMap;

use crate::ast::{Expression, ExpressionKind, UnaryOperator};
use crate::builtins::ConstantDefinition;
use crate::diagnostic::Diagnostic;

use super::super::expressions::InitializerSite;
use super::super::scopes::{ScopeId, ScopeTree, SymbolKind};
use super::semantic_error;

const MAX_INITIALIZER_NODES: usize = 128;
const MAX_INITIALIZER_DEPTH: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AssignableMode {
    Any,
    ListElement,
    NumericComponent,
}

pub(crate) fn check(
    initializers: &[InitializerSite<'_>],
    scopes: &ScopeTree,
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable {
        return Vec::new();
    }
    let constants = scopes.builtin_constants();
    let mut diagnostics = Vec::new();

    for site in initializers {
        let declarator = site.declarator;
        if !site.is_global
            || declarator.name_error.is_some()
            || declarator.name.is_empty()
            || declarator.initializer_error.is_some()
        {
            continue;
        }
        let Some(initializer) = &declarator.initializer else {
            continue;
        };
        // Do not layer a semantic error over parser recovery. This scan is
        // bounded separately because unsupported operators may otherwise stop
        // assignability checking before a later Error node is visited.
        if expression_contains_error(initializer) != Some(false) {
            continue;
        }
        let mut budget = MAX_INITIALIZER_NODES;
        if is_simple_assignable(
            initializer,
            AssignableMode::Any,
            site.scope,
            scopes,
            constants,
            0,
            &mut budget,
        ) == Some(false)
        {
            diagnostics.push(semantic_error(
                "SEM027",
                format!(
                    "global initializer for variable '{}' must be a simple value, not an expression",
                    declarator.name
                ),
                initializer.span,
            ));
        }
    }

    diagnostics
}

fn expression_contains_error(root: &Expression) -> Option<bool> {
    let mut pending = vec![(root, 0usize)];
    let mut remaining = MAX_INITIALIZER_NODES;

    while let Some((expression, depth)) = pending.pop() {
        if depth > MAX_INITIALIZER_DEPTH || remaining == 0 {
            return None;
        }
        remaining -= 1;

        match &expression.kind {
            ExpressionKind::Error(_) => return Some(true),
            ExpressionKind::Parenthesized(inner) => pending.push((inner, depth + 1)),
            ExpressionKind::Call(call) => {
                pending.push((&call.callee, depth + 1));
                pending.extend(call.arguments.iter().map(|argument| (argument, depth + 1)));
            }
            ExpressionKind::List(list) => {
                pending.extend(list.elements.iter().map(|element| (element, depth + 1)));
            }
            ExpressionKind::Vector(vector) => {
                pending.extend(
                    vector
                        .components
                        .iter()
                        .map(|component| (component, depth + 1)),
                );
            }
            ExpressionKind::Rotation(rotation) => {
                pending.extend(
                    rotation
                        .components
                        .iter()
                        .map(|component| (component, depth + 1)),
                );
            }
            ExpressionKind::Component(component) => {
                pending.push((&component.base, depth + 1));
            }
            ExpressionKind::Unary(unary) => pending.push((&unary.operand, depth + 1)),
            ExpressionKind::Binary(binary) => {
                pending.push((&binary.left, depth + 1));
                pending.push((&binary.right, depth + 1));
            }
            ExpressionKind::Assignment(assignment) => {
                pending.push((&assignment.target, depth + 1));
                pending.push((&assignment.value, depth + 1));
            }
            ExpressionKind::Cast(cast) => pending.push((&cast.operand, depth + 1)),
            ExpressionKind::Increment(increment) => pending.push((&increment.operand, depth + 1)),
            ExpressionKind::LazyIndex(lazy) => {
                pending.push((&lazy.base, depth + 1));
                pending.extend(lazy.indices.iter().map(|index| (index, depth + 1)));
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::String(_) => {}
        }
    }

    Some(false)
}

fn is_simple_assignable(
    expression: &Expression,
    mode: AssignableMode,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &HashMap<String, ConstantDefinition>,
    depth: usize,
    budget: &mut usize,
) -> Option<bool> {
    if depth > MAX_INITIALIZER_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;

    match &expression.kind {
        ExpressionKind::Integer(_) | ExpressionKind::Float(_) => Some(true),
        ExpressionKind::String(_) => Some(mode != AssignableMode::NumericComponent),
        ExpressionKind::Identifier(identifier) => Some(identifier_is_assignable(
            identifier, mode, scope, scopes, constants,
        )),
        // LSL's simple-global-initializer form does not admit parenthesized
        // expressions, even when their inner expression is a literal.
        ExpressionKind::Parenthesized(_) => Some(false),
        ExpressionKind::Unary(unary) if unary.operator == UnaryOperator::Minus => {
            is_negative_numeric(&unary.operand, scope, scopes, constants, depth + 1, budget)
        }
        ExpressionKind::Vector(vector) if mode != AssignableMode::NumericComponent => {
            if vector.components.len() != 3 {
                return Some(false);
            }
            all_numeric_components(
                &vector.components,
                scope,
                scopes,
                constants,
                depth + 1,
                budget,
            )
        }
        ExpressionKind::Rotation(rotation) if mode != AssignableMode::NumericComponent => {
            if rotation.components.len() != 4 {
                return Some(false);
            }
            all_numeric_components(
                &rotation.components,
                scope,
                scopes,
                constants,
                depth + 1,
                budget,
            )
        }
        ExpressionKind::List(list) if mode == AssignableMode::Any => {
            let mut unknown = false;
            for element in &list.elements {
                match is_simple_assignable(
                    element,
                    AssignableMode::ListElement,
                    scope,
                    scopes,
                    constants,
                    depth + 1,
                    budget,
                ) {
                    Some(true) => {}
                    Some(false) => return Some(false),
                    None => unknown = true,
                }
            }
            if unknown {
                None
            } else {
                Some(true)
            }
        }
        ExpressionKind::Error(_) => None,
        _ => Some(false),
    }
}

fn identifier_is_assignable(
    identifier: &crate::ast::Identifier,
    mode: AssignableMode,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &HashMap<String, ConstantDefinition>,
) -> bool {
    let symbol_type = scopes
        .lookup_value_at(scope, &identifier.name, identifier.span.start)
        .filter(|symbol| symbol.kind == SymbolKind::GlobalVariable)
        .and_then(|symbol| symbol.type_name.as_deref());
    let constant_type = constants
        .get(identifier.name.as_str())
        .map(|constant| constant.type_name.as_str());
    let Some(type_name) = symbol_type.or(constant_type) else {
        return false;
    };

    mode != AssignableMode::NumericComponent || matches!(type_name, "integer" | "float")
}

fn all_numeric_components(
    expressions: &[Expression],
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &HashMap<String, ConstantDefinition>,
    depth: usize,
    budget: &mut usize,
) -> Option<bool> {
    let mut unknown = false;
    for expression in expressions {
        match is_simple_assignable(
            expression,
            AssignableMode::NumericComponent,
            scope,
            scopes,
            constants,
            depth,
            budget,
        ) {
            Some(true) => {}
            Some(false) => return Some(false),
            None => unknown = true,
        }
    }
    if unknown {
        None
    } else {
        Some(true)
    }
}

fn is_negative_numeric(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &HashMap<String, ConstantDefinition>,
    depth: usize,
    budget: &mut usize,
) -> Option<bool> {
    if depth > MAX_INITIALIZER_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;

    match &expression.kind {
        ExpressionKind::Integer(_) | ExpressionKind::Float(_) => Some(true),
        ExpressionKind::Parenthesized(_) => Some(false),
        ExpressionKind::Identifier(identifier) => {
            if scopes
                .lookup_value_at(scope, &identifier.name, identifier.span.start)
                .is_some()
            {
                return Some(false);
            }
            Some(
                !matches!(identifier.name.as_str(), "TRUE" | "FALSE")
                    && constants
                        .get(identifier.name.as_str())
                        .map(|constant| matches!(constant.type_name.as_str(), "integer" | "float"))
                        .unwrap_or(false),
            )
        }
        ExpressionKind::Error(_) => None,
        _ => Some(false),
    }
}
