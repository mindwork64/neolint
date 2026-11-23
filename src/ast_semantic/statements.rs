//! Shared AST-native statement traversal for statement-owned rules.

use crate::ast::{Block, Statement, StatementKind};

/// Visit statements in deterministic source order, including nested blocks,
/// unbraced control bodies and switch arm statements. Real statement nodes are
/// traversed even when a nested block is unclosed; the caller decides whether
/// an unclosed block can be used as an independent scan root.
pub(crate) fn visit_statements(body: &Block, mut visit: impl FnMut(&Statement)) {
    let mut work: Vec<&Statement> = body.statements.iter().rev().collect();
    while let Some(statement) = work.pop() {
        visit(statement);
        match &statement.kind {
            StatementKind::Block(block) => {
                // Legacy scans statements inside an unmatched nested brace too
                // when an enclosing complete body gives the scanner a range.
                // The AST nodes are still real; the missing close only affects
                // whether this block can be used as an independent scan root.
                for statement in block.statements.iter().rev() {
                    work.push(statement);
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
