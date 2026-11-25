//! SEM041: likely accidental plain assignment in an `if` condition.

use crate::ast::{
    AssignmentOperator, Ast, Block, Expression, ExpressionKind, Statement, StatementKind, TopLevel,
};
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

const MAX_CONDITION_ASSIGNMENT_WORK: usize = 65_536;

/// Warn when an `if` condition is a bare assignment rather than a comparison.
///
/// An additional pair of parentheses explicitly marks the assignment as
/// intentional and suppresses the warning, matching the documented LSLint
/// convention. Only plain `=` is checked; compound assignments are explicit.
pub(crate) fn check(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable {
        return Vec::new();
    }

    let mut pending = Vec::new();
    for item in ast.items.iter().rev() {
        match item {
            TopLevel::Function(function) => {
                if let Some(body) = &function.body {
                    push_block(&mut pending, body);
                }
            }
            TopLevel::State(state) => {
                for event in state.events.iter().rev() {
                    if let Some(body) = &event.body {
                        push_block(&mut pending, body);
                    }
                }
            }
            TopLevel::Global(_) | TopLevel::Error(_) => {}
        }
    }

    if pending.len() > MAX_CONDITION_ASSIGNMENT_WORK {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let mut work = 0usize;

    while let Some(statement) = pending.pop() {
        if !spend_work(&mut work, 1) {
            return Vec::new();
        }

        match &statement.kind {
            StatementKind::Block(block) => push_block(&mut pending, block),
            StatementKind::If(if_statement) => {
                if let ExpressionKind::Assignment(assignment) = &if_statement.condition.kind {
                    if assignment.operator == AssignmentOperator::Assign {
                        match expression_is_reliable(
                            &if_statement.condition,
                            syntax_diagnostics,
                            &mut work,
                        ) {
                            Some(true) => diagnostics.push(Diagnostic {
                                code: "SEM041",
                                severity: Severity::Warning,
                                kind: DiagnosticKind::Lint,
                                message: "plain assignment in an if condition is likely accidental; use '==' for comparison or wrap the assignment in parentheses to mark it as intentional".to_owned(),
                                span: assignment.operator_span,
                            }),
                            Some(false) => {}
                            None => return Vec::new(),
                        }
                    }
                }
                if let Some(else_branch) = &if_statement.else_branch {
                    pending.push(else_branch);
                }
                pending.push(&if_statement.then_branch);
            }
            StatementKind::For(for_statement) => pending.push(&for_statement.body),
            StatementKind::While(while_statement) => pending.push(&while_statement.body),
            StatementKind::DoWhile(do_while) => pending.push(&do_while.body),
            StatementKind::Switch(switch) => {
                for arm in switch.arms.iter().rev() {
                    pending.extend(arm.statements.iter().rev());
                }
            }
            StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::Return(_)
            | StatementKind::Jump(_)
            | StatementKind::StateChange(_)
            | StatementKind::Label(_)
            | StatementKind::Expression(_)
            | StatementKind::Break(_)
            | StatementKind::Error(_) => {}
        }

        if pending.len() > MAX_CONDITION_ASSIGNMENT_WORK.saturating_sub(work) {
            return Vec::new();
        }
    }

    diagnostics
}

fn push_block<'a>(pending: &mut Vec<&'a Statement>, block: &'a Block) {
    pending.extend(block.statements.iter().rev());
}

fn expression_is_reliable(
    expression: &Expression,
    syntax_diagnostics: &[Diagnostic],
    work: &mut usize,
) -> Option<bool> {
    let mut pending = vec![expression];
    while let Some(expression) = pending.pop() {
        if !spend_work(work, 1) {
            return None;
        }
        match &expression.kind {
            ExpressionKind::Error(_) => return Some(false),
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
        if pending.len() > MAX_CONDITION_ASSIGNMENT_WORK.saturating_sub(*work) {
            return None;
        }
    }

    for diagnostic in syntax_diagnostics {
        if !spend_work(work, 1) {
            return None;
        }
        if spans_overlap(expression.span, diagnostic.span) {
            return Some(false);
        }
    }

    Some(true)
}

fn spend_work(work: &mut usize, amount: usize) -> bool {
    *work = work.saturating_add(amount);
    *work <= MAX_CONDITION_ASSIGNMENT_WORK
}

fn spans_overlap(range: crate::diagnostic::Span, candidate: crate::diagnostic::Span) -> bool {
    if candidate.start == candidate.end {
        range.start <= candidate.start && candidate.start <= range.end
    } else {
        candidate.start < range.end && range.start < candidate.end
    }
}
