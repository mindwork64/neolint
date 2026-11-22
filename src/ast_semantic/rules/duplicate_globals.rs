//! SEM001: duplicate top-level global/function declarations.

use std::collections::HashMap;

use crate::ast::{Ast, Expression, ExpressionKind, TopLevel};
use crate::diagnostic::{Diagnostic, Span};

use super::semantic_error;

pub(crate) fn check(ast: &Ast) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut globals: HashMap<&str, Span> = HashMap::new();
    for item in &ast.items {
        match item {
            TopLevel::Global(global) => {
                // Legacy SEM001 registers only the first declarator of a global
                // declaration; later declarators are visible to ScopeModel but
                // are not part of the SEM001 duplicate scan.
                if let Some(declarator) = global.declarators.first() {
                    if declarator.name_error.is_none() && !declarator.name.is_empty() {
                        add(
                            &declarator.name,
                            declarator.name_span,
                            &mut globals,
                            &mut diagnostics,
                        );
                    }
                }
                // Legacy also treats an identifier immediately followed by `(`
                // at top level as a function declaration. A global initializer
                // is still at paren depth zero, so top-level calls there
                // participate in the duplicate scan.
                let mut calls = Vec::new();
                for declarator in &global.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        scan_top_level_calls(initializer, &mut calls);
                    }
                }
                for (name, span) in calls {
                    add(name, span, &mut globals, &mut diagnostics);
                }
            }
            TopLevel::Function(function) => {
                if !function.name.is_empty() {
                    add(
                        &function.name,
                        function.name_span,
                        &mut globals,
                        &mut diagnostics,
                    );
                }
            }
            TopLevel::State(_) | TopLevel::Error(_) => {}
        }
    }
    diagnostics
}

fn add<'a>(
    name: &'a str,
    span: Span,
    globals: &mut HashMap<&'a str, Span>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(previous) = globals.get(name) {
        diagnostics.push(semantic_error(
            "SEM001",
            format!(
                "duplicate global declaration '{}'; first declared at {}:{}",
                name, previous.line, previous.column
            ),
            span,
        ));
    } else {
        globals.insert(name, span);
    }
}

fn scan_top_level_calls<'a>(expression: &'a Expression, calls: &mut Vec<(&'a str, Span)>) {
    match &expression.kind {
        ExpressionKind::Call(call) => {
            match &call.callee.kind {
                ExpressionKind::Identifier(callee) => calls.push((&callee.name, callee.span)),
                ExpressionKind::Component(component) => {
                    calls.push((&component.component_name, component.component_span));
                    scan_top_level_calls(&component.base, calls);
                }
                _ => scan_top_level_calls(&call.callee, calls),
            }
            // Arguments are inside the call's parentheses.
        }
        ExpressionKind::Parenthesized(_) | ExpressionKind::Error(_) => {}
        ExpressionKind::List(list) => {
            for element in &list.elements {
                scan_top_level_calls(element, calls);
            }
        }
        ExpressionKind::Vector(vector) => {
            for component in &vector.components {
                scan_top_level_calls(component, calls);
            }
        }
        ExpressionKind::Rotation(rotation) => {
            for component in &rotation.components {
                scan_top_level_calls(component, calls);
            }
        }
        ExpressionKind::Component(component) => scan_top_level_calls(&component.base, calls),
        ExpressionKind::Unary(unary) => scan_top_level_calls(&unary.operand, calls),
        ExpressionKind::Binary(binary) => {
            scan_top_level_calls(&binary.left, calls);
            scan_top_level_calls(&binary.right, calls);
        }
        ExpressionKind::Assignment(assignment) => {
            scan_top_level_calls(&assignment.target, calls);
            scan_top_level_calls(&assignment.value, calls);
        }
        ExpressionKind::Cast(cast) => scan_top_level_calls(&cast.operand, calls),
        ExpressionKind::Increment(increment) => scan_top_level_calls(&increment.operand, calls),
        ExpressionKind::LazyIndex(lazy) => {
            scan_top_level_calls(&lazy.base, calls);
            for index in &lazy.indices {
                scan_top_level_calls(index, calls);
            }
        }
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_) => {}
    }
}
