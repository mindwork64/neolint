//! Shared AST-native expression and initializer traversal.

use crate::ast::{
    Ast, Block, Expression, ExpressionKind, Statement, StatementKind, TopLevel, TypeKind,
    VariableDeclarator,
};

use super::scopes::{ScopeId, ScopeTree};

/// One expression occurrence and its lexical scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExpressionSite<'a> {
    pub(crate) expression: &'a Expression,
    pub(crate) scope: ScopeId,
}

/// One successfully present declarator initializer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InitializerSite<'a> {
    pub(crate) declarator: &'a VariableDeclarator,
    pub(crate) expected: TypeKind,
    pub(crate) scope: ScopeId,
}

/// All expression and initializer occurrences in deterministic source order.
#[derive(Debug, Default)]
pub(crate) struct ExpressionFacts<'a> {
    pub(crate) expressions: Vec<ExpressionSite<'a>>,
    pub(crate) initializers: Vec<InitializerSite<'a>>,
}

/// Visit every expression exactly once and every initializer declarator once.
///
/// Scope propagation mirrors `ScopeTree`: globals at root, function/event body
/// scopes, nested blocks and switch scopes. Unknown extension events are
/// intentionally included.
pub(crate) fn collect_facts<'a>(ast: &'a Ast, scopes: &ScopeTree) -> ExpressionFacts<'a> {
    let mut facts = ExpressionFacts::default();
    for item in &ast.items {
        match item {
            TopLevel::Global(global) => {
                for declarator in &global.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        facts.initializers.push(InitializerSite {
                            declarator,
                            expected: global.ty.kind,
                            scope: scopes.root(),
                        });
                        collect_in_expression(initializer, scopes.root(), &mut facts);
                    }
                }
            }
            TopLevel::Function(function) => {
                let Some(body) = &function.body else {
                    continue;
                };
                let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                walk_block(body, scope, scopes, &mut facts);
            }
            TopLevel::State(state) => {
                for event in &state.events {
                    let Some(body) = &event.body else {
                        continue;
                    };
                    let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                    walk_block(body, scope, scopes, &mut facts);
                }
            }
            TopLevel::Error(_) => {}
        }
    }
    facts
}

fn walk_block<'a>(
    block: &'a Block,
    scope: ScopeId,
    scopes: &ScopeTree,
    facts: &mut ExpressionFacts<'a>,
) {
    let mut work: Vec<(&Statement, ScopeId)> = block
        .statements
        .iter()
        .rev()
        .map(|statement| (statement, scope))
        .collect();
    while let Some((statement, current_scope)) = work.pop() {
        match &statement.kind {
            StatementKind::Local(local) => {
                for declarator in &local.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        facts.initializers.push(InitializerSite {
                            declarator,
                            expected: local.ty.kind,
                            scope: current_scope,
                        });
                        collect_in_expression(initializer, current_scope, facts);
                    }
                }
            }
            StatementKind::Block(inner) => {
                let child = scopes.scope_for_block(inner).unwrap_or(current_scope);
                for statement in inner.statements.iter().rev() {
                    work.push((statement, child));
                }
            }
            StatementKind::If(if_statement) => {
                collect_in_expression(&if_statement.condition, current_scope, facts);
                work.push((&if_statement.then_branch, current_scope));
                if let Some(else_branch) = &if_statement.else_branch {
                    work.push((else_branch, current_scope));
                }
            }
            StatementKind::For(for_statement) => {
                for initializer in &for_statement.initializer {
                    collect_in_expression(initializer, current_scope, facts);
                }
                collect_in_expression(&for_statement.condition, current_scope, facts);
                for increment in &for_statement.increment {
                    collect_in_expression(increment, current_scope, facts);
                }
                work.push((&for_statement.body, current_scope));
            }
            StatementKind::While(while_statement) => {
                collect_in_expression(&while_statement.condition, current_scope, facts);
                work.push((&while_statement.body, current_scope));
            }
            StatementKind::DoWhile(do_while) => {
                collect_in_expression(&do_while.condition, current_scope, facts);
                work.push((&do_while.body, current_scope));
            }
            StatementKind::Return(return_statement) => {
                if let Some(value) = &return_statement.value {
                    collect_in_expression(value, current_scope, facts);
                }
            }
            StatementKind::Expression(expression) => {
                collect_in_expression(&expression.expression, current_scope, facts);
            }
            StatementKind::Switch(switch) => {
                collect_in_expression(&switch.condition, current_scope, facts);
                let switch_scope = scopes.scope_for_switch(switch).unwrap_or(current_scope);
                for arm in &switch.arms {
                    if let crate::ast::SwitchLabel::Case(expression) = &arm.label {
                        collect_in_expression(expression, switch_scope, facts);
                    }
                    for statement in arm.statements.iter().rev() {
                        work.push((statement, switch_scope));
                    }
                }
            }
            StatementKind::Empty
            | StatementKind::Jump(_)
            | StatementKind::StateChange(_)
            | StatementKind::Label(_)
            | StatementKind::Break(_)
            | StatementKind::Error(_) => {}
        }
    }
}

fn collect_in_expression<'a>(
    expression: &'a Expression,
    scope: ScopeId,
    facts: &mut ExpressionFacts<'a>,
) {
    let mut work: Vec<(&Expression, ScopeId)> = vec![(expression, scope)];
    while let Some((expression, scope)) = work.pop() {
        facts.expressions.push(ExpressionSite { expression, scope });
        match &expression.kind {
            ExpressionKind::Call(call) => {
                work.push((&call.callee, scope));
                for argument in call.arguments.iter().rev() {
                    work.push((argument, scope));
                }
            }
            ExpressionKind::Parenthesized(inner) => work.push((inner, scope)),
            ExpressionKind::List(list) => {
                for element in list.elements.iter().rev() {
                    work.push((element, scope));
                }
            }
            ExpressionKind::Vector(vector) => {
                for component in vector.components.iter().rev() {
                    work.push((component, scope));
                }
            }
            ExpressionKind::Rotation(rotation) => {
                for component in rotation.components.iter().rev() {
                    work.push((component, scope));
                }
            }
            ExpressionKind::Component(component) => work.push((&component.base, scope)),
            ExpressionKind::Unary(unary) => work.push((&unary.operand, scope)),
            ExpressionKind::Binary(binary) => {
                work.push((&binary.left, scope));
                work.push((&binary.right, scope));
            }
            ExpressionKind::Assignment(assignment) => {
                work.push((&assignment.target, scope));
                work.push((&assignment.value, scope));
            }
            ExpressionKind::Cast(cast) => work.push((&cast.operand, scope)),
            ExpressionKind::Increment(increment) => work.push((&increment.operand, scope)),
            ExpressionKind::LazyIndex(lazy) => {
                work.push((&lazy.base, scope));
                for index in lazy.indices.iter().rev() {
                    work.push((index, scope));
                }
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::String(_)
            | ExpressionKind::Error(_) => {}
        }
    }
}
