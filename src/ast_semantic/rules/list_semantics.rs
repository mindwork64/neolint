//! SEM038–SEM040: empty states, nested-list elements and list equality hazards.

use crate::ast::{Ast, BinaryOperator, Expression, ExpressionKind, TopLevel};
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};

use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::super::types::{infer_expression_type, UserFunctionSignature};
use super::semantic_error;

const MAX_LIST_SEMANTIC_WORK: usize = 65_536;

/// Check list expressions using only types that the AST can establish.
pub(crate) fn check(
    expressions: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    functions: &std::collections::HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let mut work = 0usize;

    for site in expressions {
        work = work.saturating_add(1);
        if work > MAX_LIST_SEMANTIC_WORK {
            return Vec::new();
        }

        match &site.expression.kind {
            ExpressionKind::List(list) => {
                for element in &list.elements {
                    work = work.saturating_add(1);
                    if work > MAX_LIST_SEMANTIC_WORK {
                        return Vec::new();
                    }
                    match expression_is_unreliable(element, syntax_diagnostics, &mut work) {
                        Some(true) => continue,
                        Some(false) => {}
                        None => return Vec::new(),
                    }
                    if infer_expression_type(element, scopes, site.scope, functions).as_deref()
                        == Some("list")
                    {
                        diagnostics.push(semantic_error(
                            "SEM039",
                            "a list cannot contain an element whose type is list; flatten the value or explicitly serialize it as a string".to_owned(),
                            element.span,
                        ));
                    }
                }
            }
            ExpressionKind::Binary(binary)
                if matches!(
                    binary.operator,
                    BinaryOperator::Equal | BinaryOperator::NotEqual
                ) =>
            {
                match expression_is_unreliable(site.expression, syntax_diagnostics, &mut work) {
                    Some(true) => continue,
                    Some(false) => {}
                    None => return Vec::new(),
                }
                let left_is_list =
                    infer_expression_type(&binary.left, scopes, site.scope, functions).as_deref()
                        == Some("list");
                let right_is_list =
                    infer_expression_type(&binary.right, scopes, site.scope, functions).as_deref()
                        == Some("list");
                if left_is_list && right_is_list {
                    diagnostics.push(Diagnostic {
                        code: "SEM040",
                        severity: Severity::Warning,
                        kind: DiagnosticKind::Lint,
                        message: if binary.operator == BinaryOperator::Equal {
                            "list equality compares lengths only, not list contents; compare elements when content equality is intended".to_owned()
                        } else {
                            "list inequality returns the difference between list lengths, not a boolean; compare lengths explicitly or compare elements".to_owned()
                        },
                        span: binary.operator_span,
                    });
                }
            }
            _ => {}
        }
    }

    diagnostics
}

fn expression_is_unreliable(
    expression: &Expression,
    syntax_diagnostics: &[Diagnostic],
    work: &mut usize,
) -> Option<bool> {
    if contains_error(expression, work)? {
        return Some(true);
    }
    for diagnostic in syntax_diagnostics {
        *work = work.saturating_add(1);
        if *work > MAX_LIST_SEMANTIC_WORK {
            return None;
        }
        if span_is_overlapped(expression.span, diagnostic.span) {
            return Some(true);
        }
    }
    Some(false)
}

fn contains_error(expression: &Expression, work: &mut usize) -> Option<bool> {
    let mut pending = vec![expression];
    while let Some(expression) = pending.pop() {
        *work = work.saturating_add(1);
        if *work > MAX_LIST_SEMANTIC_WORK {
            return None;
        }
        match &expression.kind {
            ExpressionKind::Error(_) => return Some(true),
            ExpressionKind::Parenthesized(inner) => pending.push(inner),
            ExpressionKind::List(list) => pending.extend(list.elements.iter()),
            ExpressionKind::Vector(vector) => pending.extend(vector.components.iter()),
            ExpressionKind::Rotation(rotation) => pending.extend(rotation.components.iter()),
            ExpressionKind::Call(call) => {
                pending.push(&call.callee);
                pending.extend(call.arguments.iter());
            }
            ExpressionKind::Component(component) => pending.push(&component.base),
            ExpressionKind::Unary(unary) => pending.push(&unary.operand),
            ExpressionKind::Binary(binary) => {
                pending.push(&binary.left);
                pending.push(&binary.right);
            }
            ExpressionKind::Assignment(assignment) => {
                pending.push(&assignment.target);
                pending.push(&assignment.value);
            }
            ExpressionKind::Cast(cast) => pending.push(&cast.operand),
            ExpressionKind::Increment(increment) => pending.push(&increment.operand),
            ExpressionKind::LazyIndex(lazy) => {
                pending.push(&lazy.base);
                pending.extend(lazy.indices.iter());
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::String(_) => {}
        }
    }
    Some(false)
}

fn span_is_overlapped(range: Span, candidate: Span) -> bool {
    if candidate.start == candidate.end {
        range.start <= candidate.start && candidate.start <= range.end
    } else {
        candidate.start < range.end && range.start < candidate.end
    }
}

/// SEM038: every complete state declaration must contain at least one event.
pub(crate) fn check_empty_states(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.name_missing
            || state.name.is_empty()
            || state.body_missing
            || !state.body_closed
            || !state.events.is_empty()
        {
            continue;
        }
        if syntax_diagnostics
            .iter()
            .any(|diagnostic| span_is_overlapped(state.span, diagnostic.span))
        {
            continue;
        }

        diagnostics.push(semantic_error(
            "SEM038",
            format!(
                "state '{}' must contain at least one event handler",
                state.name
            ),
            state.name_span,
        ));
    }
    diagnostics
}
