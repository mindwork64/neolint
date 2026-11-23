//! SEM023: standalone comparison expressions whose result is discarded.

use crate::ast::{Ast, BinaryOperator, Expression, ExpressionKind, StatementKind, TopLevel};
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::statements::visit_statements;

pub(crate) fn check(ast: &Ast, syntax_diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for item in &ast.items {
        match item {
            TopLevel::Function(function) => {
                let Some(body) = &function.body else {
                    continue;
                };
                if body.closed {
                    scan_body(body, syntax_diagnostics, &mut diagnostics);
                }
            }
            TopLevel::State(state) => {
                for event in &state.events {
                    let Some(body) = &event.body else {
                        continue;
                    };
                    if body.closed {
                        scan_body(body, syntax_diagnostics, &mut diagnostics);
                    }
                }
            }
            TopLevel::Global(_) | TopLevel::Error(_) => {}
        }
    }
    diagnostics
}

fn scan_body(
    body: &crate::ast::Block,
    syntax_diagnostics: &[Diagnostic],
    diagnostics: &mut Vec<Diagnostic>,
) {
    visit_statements(body, |statement| {
        let StatementKind::Expression(expression_statement) = &statement.kind else {
            return;
        };
        let expression = strip_parentheses(&expression_statement.expression);
        let ExpressionKind::Binary(binary) = &expression.kind else {
            return;
        };
        if !comparison_operator(binary.operator)
            || has_side_effect_or_error(&expression_statement.expression)
            || syntax_diagnostics.iter().any(|diagnostic| {
                diagnostic.span.start >= statement.span.start
                    && diagnostic.span.start <= statement.span.end
            })
        {
            return;
        }
        diagnostics.push(Diagnostic {
            code: "SEM023",
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: "comparison result is discarded; use '=' for assignment or use the comparison in a condition".to_owned(),
            span: binary.operator_span,
        });
    });
}

fn strip_parentheses(mut expression: &Expression) -> &Expression {
    while let ExpressionKind::Parenthesized(inner) = &expression.kind {
        expression = inner;
    }
    expression
}

fn comparison_operator(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
    )
}

fn has_side_effect_or_error(expression: &Expression) -> bool {
    let mut work = vec![expression];
    while let Some(expression) = work.pop() {
        match &expression.kind {
            ExpressionKind::Call(_)
            | ExpressionKind::Assignment(_)
            | ExpressionKind::Increment(_)
            | ExpressionKind::Error(_) => return true,
            ExpressionKind::Parenthesized(inner) => work.push(inner),
            ExpressionKind::List(list) => work.extend(list.elements.iter()),
            ExpressionKind::Vector(vector) => work.extend(vector.components.iter()),
            ExpressionKind::Rotation(rotation) => work.extend(rotation.components.iter()),
            ExpressionKind::Component(component) => work.push(&component.base),
            ExpressionKind::Unary(unary) => work.push(&unary.operand),
            ExpressionKind::Binary(binary) => {
                work.push(&binary.left);
                work.push(&binary.right);
            }
            ExpressionKind::Cast(cast) => work.push(&cast.operand),
            ExpressionKind::LazyIndex(lazy) => {
                work.push(&lazy.base);
                work.extend(lazy.indices.iter());
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::String(_) => {}
        }
    }
    false
}
