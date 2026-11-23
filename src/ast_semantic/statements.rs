//! Shared AST-native statement traversal for statement-owned rules.

use crate::ast::{Block, Statement, StatementKind};

/// Visit statements in deterministic source order, including nested blocks,
/// unbraced control bodies and switch arm statements. Unclosed nested blocks
/// are not traversed.
pub(crate) fn visit_statements(body: &Block, mut visit: impl FnMut(&Statement)) {
    let mut work: Vec<&Statement> = body.statements.iter().rev().collect();
    while let Some(statement) = work.pop() {
        visit(statement);
        match &statement.kind {
            StatementKind::Block(block) => {
                if block.closed {
                    for statement in block.statements.iter().rev() {
                        work.push(statement);
                    }
                }
            }
            StatementKind::If(if_statement) => {
                if let Some(else_branch) = &if_statement.else_branch {
                    work.push(else_branch);
                }
                work.push(&if_statement.then_branch);
            }
            StatementKind::For(for_statement) => work.push(&for_statement.body),
            StatementKind::While(while_statement) => work.push(&while_statement.body),
            StatementKind::DoWhile(do_while) => work.push(&do_while.body),
            StatementKind::Switch(switch) => {
                for arm in switch.arms.iter().rev() {
                    for statement in arm.statements.iter().rev() {
                        work.push(statement);
                    }
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
    }
}
