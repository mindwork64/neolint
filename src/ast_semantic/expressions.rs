//! Shared AST-native expression and initializer traversal.

use crate::ast::{
    Ast, Expression, ExpressionKind, Statement, StatementKind, TopLevel, TypeKind,
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

#[derive(Clone, Copy)]
enum WorkItem<'a> {
    Statement(&'a Statement, ScopeId),
    Expression(&'a Expression, ScopeId),
    Initializer(InitializerSite<'a>),
}

/// Visit every expression exactly once and every initializer declarator once.
///
/// Work is pushed in reverse syntactic order because the stack is LIFO. This
/// keeps both facts collections in source order while retaining an iterative
/// traversal for deeply nested expressions/statements.
pub(crate) fn collect_facts<'a>(ast: &'a Ast, scopes: &ScopeTree) -> ExpressionFacts<'a> {
    let mut facts = ExpressionFacts::default();
    let mut work = Vec::new();

    for item in ast.items.iter().rev() {
        match item {
            TopLevel::Global(global) => {
                for declarator in global.declarators.iter().rev() {
                    push_initializer(&mut work, declarator, global.ty.kind, scopes.root());
                }
            }
            TopLevel::Function(function) => {
                let Some(body) = &function.body else {
                    continue;
                };
                let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                push_statements(&mut work, &body.statements, scope);
            }
            TopLevel::State(state) => {
                for event in state.events.iter().rev() {
                    let Some(body) = &event.body else {
                        continue;
                    };
                    let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                    push_statements(&mut work, &body.statements, scope);
                }
            }
            TopLevel::Error(_) => {}
        }
    }

    while let Some(item) = work.pop() {
        match item {
            WorkItem::Initializer(site) => facts.initializers.push(site),
            WorkItem::Expression(expression, scope) => {
                facts.expressions.push(ExpressionSite { expression, scope });
                push_expression_children(&mut work, expression, scope);
            }
            WorkItem::Statement(statement, current_scope) => match &statement.kind {
                StatementKind::Local(local) => {
                    for declarator in local.declarators.iter().rev() {
                        push_initializer(&mut work, declarator, local.ty.kind, current_scope);
                    }
                }
                StatementKind::Block(inner) => {
                    let child = scopes.scope_for_block(inner).unwrap_or(current_scope);
                    push_statements(&mut work, &inner.statements, child);
                }
                StatementKind::If(if_statement) => {
                    if let Some(else_branch) = &if_statement.else_branch {
                        work.push(WorkItem::Statement(else_branch, current_scope));
                    }
                    work.push(WorkItem::Statement(
                        &if_statement.then_branch,
                        current_scope,
                    ));
                    work.push(WorkItem::Expression(&if_statement.condition, current_scope));
                }
                StatementKind::For(for_statement) => {
                    work.push(WorkItem::Statement(&for_statement.body, current_scope));
                    for increment in for_statement.increment.iter().rev() {
                        work.push(WorkItem::Expression(increment, current_scope));
                    }
                    work.push(WorkItem::Expression(
                        &for_statement.condition,
                        current_scope,
                    ));
                    for initializer in for_statement.initializer.iter().rev() {
                        work.push(WorkItem::Expression(initializer, current_scope));
                    }
                }
                StatementKind::While(while_statement) => {
                    work.push(WorkItem::Statement(&while_statement.body, current_scope));
                    work.push(WorkItem::Expression(
                        &while_statement.condition,
                        current_scope,
                    ));
                }
                StatementKind::DoWhile(do_while) => {
                    // The body precedes the trailing while-condition in source.
                    work.push(WorkItem::Expression(&do_while.condition, current_scope));
                    work.push(WorkItem::Statement(&do_while.body, current_scope));
                }
                StatementKind::Return(return_statement) => {
                    if let Some(value) = &return_statement.value {
                        work.push(WorkItem::Expression(value, current_scope));
                    }
                }
                StatementKind::Expression(expression) => {
                    work.push(WorkItem::Expression(&expression.expression, current_scope));
                }
                StatementKind::Switch(switch) => {
                    let switch_scope = scopes.scope_for_switch(switch).unwrap_or(current_scope);
                    for arm in switch.arms.iter().rev() {
                        for statement in arm.statements.iter().rev() {
                            work.push(WorkItem::Statement(statement, switch_scope));
                        }
                        if let crate::ast::SwitchLabel::Case(expression) = &arm.label {
                            work.push(WorkItem::Expression(expression, switch_scope));
                        }
                    }
                    work.push(WorkItem::Expression(&switch.condition, current_scope));
                }
                StatementKind::Empty
                | StatementKind::Jump(_)
                | StatementKind::StateChange(_)
                | StatementKind::Label(_)
                | StatementKind::Break(_)
                | StatementKind::Error(_) => {}
            },
        }
    }

    facts
}

fn push_statements<'a>(work: &mut Vec<WorkItem<'a>>, statements: &'a [Statement], scope: ScopeId) {
    for statement in statements.iter().rev() {
        work.push(WorkItem::Statement(statement, scope));
    }
}

fn push_initializer<'a>(
    work: &mut Vec<WorkItem<'a>>,
    declarator: &'a VariableDeclarator,
    expected: TypeKind,
    scope: ScopeId,
) {
    let Some(initializer) = &declarator.initializer else {
        return;
    };
    // The two fact streams are independently source ordered. Put both tasks on
    // the same stack so nested expressions complete before the next declarator.
    work.push(WorkItem::Expression(initializer, scope));
    work.push(WorkItem::Initializer(InitializerSite {
        declarator,
        expected,
        scope,
    }));
}

fn push_expression_children<'a>(
    work: &mut Vec<WorkItem<'a>>,
    expression: &'a Expression,
    scope: ScopeId,
) {
    match &expression.kind {
        ExpressionKind::Call(call) => {
            for argument in call.arguments.iter().rev() {
                work.push(WorkItem::Expression(argument, scope));
            }
            work.push(WorkItem::Expression(&call.callee, scope));
        }
        ExpressionKind::Parenthesized(inner) => {
            work.push(WorkItem::Expression(inner, scope));
        }
        ExpressionKind::List(list) => {
            for element in list.elements.iter().rev() {
                work.push(WorkItem::Expression(element, scope));
            }
        }
        ExpressionKind::Vector(vector) => {
            for component in vector.components.iter().rev() {
                work.push(WorkItem::Expression(component, scope));
            }
        }
        ExpressionKind::Rotation(rotation) => {
            for component in rotation.components.iter().rev() {
                work.push(WorkItem::Expression(component, scope));
            }
        }
        ExpressionKind::Component(component) => {
            work.push(WorkItem::Expression(&component.base, scope));
        }
        ExpressionKind::Unary(unary) => {
            work.push(WorkItem::Expression(&unary.operand, scope));
        }
        ExpressionKind::Binary(binary) => {
            work.push(WorkItem::Expression(&binary.right, scope));
            work.push(WorkItem::Expression(&binary.left, scope));
        }
        ExpressionKind::Assignment(assignment) => {
            work.push(WorkItem::Expression(&assignment.value, scope));
            work.push(WorkItem::Expression(&assignment.target, scope));
        }
        ExpressionKind::Cast(cast) => {
            work.push(WorkItem::Expression(&cast.operand, scope));
        }
        ExpressionKind::Increment(increment) => {
            work.push(WorkItem::Expression(&increment.operand, scope));
        }
        ExpressionKind::LazyIndex(lazy) => {
            for index in lazy.indices.iter().rev() {
                work.push(WorkItem::Expression(index, scope));
            }
            work.push(WorkItem::Expression(&lazy.base, scope));
        }
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Error(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::collect_facts;
    use crate::ast::parse_ast;
    use crate::ast_semantic::scopes::ScopeTree;
    use crate::lex;

    #[test]
    fn expression_and_initializer_facts_follow_source_order() {
        let source = r#"
            integer a; integer b; integer c; integer d; integer e;
            integer f; integer g; integer h; integer i; integer j;
            integer k; integer l; integer m;
            default {
                state_entry() {
                    if (a = 1) { b = 2; } else { c = 3; }
                    do { d = 4; } while (e = 5);
                    k = (l = 6) + (m = 7);
                    integer first = (a = 8), second = (b = 9);
                }
            }
        "#;
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast(&tokens);
        assert!(syntax.is_empty(), "{syntax:?}");
        let scopes = ScopeTree::build(&ast);
        let facts = collect_facts(&ast, &scopes);

        let expression_starts: Vec<_> = facts
            .expressions
            .iter()
            .map(|site| site.expression.span.start)
            .collect();
        assert!(
            expression_starts.windows(2).all(|pair| pair[0] <= pair[1]),
            "expression starts are not source ordered: {expression_starts:?}"
        );

        let initializer_starts: Vec<_> = facts
            .initializers
            .iter()
            .map(|site| site.declarator.name_span.start)
            .collect();
        assert!(
            initializer_starts.windows(2).all(|pair| pair[0] <= pair[1]),
            "initializer starts are not source ordered: {initializer_starts:?}"
        );
    }
}
