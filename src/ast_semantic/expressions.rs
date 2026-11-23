//! Shared AST-native expression and initializer traversal.

use crate::ast::{
    Ast, Expression, ExpressionKind, Identifier, Statement, StatementKind, TopLevel, TypeKind,
    VariableDeclarator,
};

use super::scopes::{ScopeId, ScopeTree};

/// How an identifier occurrence uses the value bound to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UseContext {
    /// The identifier's value is observed.
    Read,
    /// The identifier is the target of a plain assignment.
    Write,
    /// The identifier's value is read and then written.
    ReadWrite,
    /// The identifier names a called function rather than a variable.
    Callee,
}

/// One identifier occurrence and the context in which it is used.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IdentifierUse<'a> {
    pub(crate) identifier: &'a Identifier,
    pub(crate) scope: ScopeId,
    pub(crate) context: UseContext,
}

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
    pub(crate) uses: Vec<IdentifierUse<'a>>,
}

#[derive(Clone, Copy)]
enum WorkItem<'a> {
    Statement(&'a Statement, ScopeId),
    Expression(&'a Expression, ScopeId, UseContext),
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
                    for reference in event.recovered_parameter_references.iter().rev() {
                        work.push(WorkItem::Expression(reference, scope, UseContext::Read));
                    }
                }
            }
            TopLevel::Error(_) => {}
        }
    }

    while let Some(item) = work.pop() {
        match item {
            WorkItem::Initializer(site) => facts.initializers.push(site),
            WorkItem::Expression(expression, scope, context) => {
                facts.expressions.push(ExpressionSite { expression, scope });
                if let ExpressionKind::Identifier(identifier) = &expression.kind {
                    facts.uses.push(IdentifierUse {
                        identifier,
                        scope,
                        context,
                    });
                }
                push_expression_children(&mut work, expression, scope, context);
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
                    work.push(WorkItem::Expression(
                        &if_statement.condition,
                        current_scope,
                        UseContext::Read,
                    ));
                }
                StatementKind::For(for_statement) => {
                    work.push(WorkItem::Statement(&for_statement.body, current_scope));
                    for increment in for_statement.increment.iter().rev() {
                        work.push(WorkItem::Expression(
                            increment,
                            current_scope,
                            UseContext::Read,
                        ));
                    }
                    work.push(WorkItem::Expression(
                        &for_statement.condition,
                        current_scope,
                        UseContext::Read,
                    ));
                    for initializer in for_statement.initializer.iter().rev() {
                        work.push(WorkItem::Expression(
                            initializer,
                            current_scope,
                            UseContext::Read,
                        ));
                    }
                }
                StatementKind::While(while_statement) => {
                    work.push(WorkItem::Statement(&while_statement.body, current_scope));
                    work.push(WorkItem::Expression(
                        &while_statement.condition,
                        current_scope,
                        UseContext::Read,
                    ));
                }
                StatementKind::DoWhile(do_while) => {
                    // The body precedes the trailing while-condition in source.
                    work.push(WorkItem::Expression(
                        &do_while.condition,
                        current_scope,
                        UseContext::Read,
                    ));
                    work.push(WorkItem::Statement(&do_while.body, current_scope));
                }
                StatementKind::Return(return_statement) => {
                    if let Some(value) = &return_statement.value {
                        work.push(WorkItem::Expression(value, current_scope, UseContext::Read));
                    }
                }
                StatementKind::Expression(expression) => {
                    work.push(WorkItem::Expression(
                        &expression.expression,
                        current_scope,
                        UseContext::Read,
                    ));
                }
                StatementKind::Switch(switch) => {
                    let switch_scope = scopes.scope_for_switch(switch).unwrap_or(current_scope);
                    for arm in switch.arms.iter().rev() {
                        for statement in arm.statements.iter().rev() {
                            work.push(WorkItem::Statement(statement, switch_scope));
                        }
                        if let crate::ast::SwitchLabel::Case(expression) = &arm.label {
                            work.push(WorkItem::Expression(
                                expression,
                                switch_scope,
                                UseContext::Read,
                            ));
                        }
                    }
                    work.push(WorkItem::Expression(
                        &switch.condition,
                        current_scope,
                        UseContext::Read,
                    ));
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
    work.push(WorkItem::Expression(initializer, scope, UseContext::Read));
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
    context: UseContext,
) {
    match &expression.kind {
        ExpressionKind::Call(call) => {
            for argument in call.arguments.iter().rev() {
                work.push(WorkItem::Expression(argument, scope, UseContext::Read));
            }
            work.push(WorkItem::Expression(
                &call.callee,
                scope,
                UseContext::Callee,
            ));
        }
        ExpressionKind::Parenthesized(inner) => {
            work.push(WorkItem::Expression(inner, scope, context));
        }
        ExpressionKind::List(list) => {
            for element in list.elements.iter().rev() {
                work.push(WorkItem::Expression(element, scope, UseContext::Read));
            }
        }
        ExpressionKind::Vector(vector) => {
            for component in vector.components.iter().rev() {
                work.push(WorkItem::Expression(component, scope, UseContext::Read));
            }
        }
        ExpressionKind::Rotation(rotation) => {
            for component in rotation.components.iter().rev() {
                work.push(WorkItem::Expression(component, scope, UseContext::Read));
            }
        }
        ExpressionKind::Component(component) => {
            work.push(WorkItem::Expression(&component.base, scope, context));
        }
        ExpressionKind::Unary(unary) => {
            work.push(WorkItem::Expression(
                &unary.operand,
                scope,
                UseContext::Read,
            ));
        }
        ExpressionKind::Binary(binary) => {
            work.push(WorkItem::Expression(&binary.right, scope, UseContext::Read));
            work.push(WorkItem::Expression(&binary.left, scope, UseContext::Read));
        }
        ExpressionKind::Assignment(assignment) => {
            work.push(WorkItem::Expression(
                &assignment.value,
                scope,
                UseContext::Read,
            ));
            let target_context = if assignment.operator == crate::ast::AssignmentOperator::Assign {
                UseContext::Write
            } else {
                UseContext::ReadWrite
            };
            work.push(WorkItem::Expression(
                &assignment.target,
                scope,
                target_context,
            ));
        }
        ExpressionKind::Cast(cast) => {
            work.push(WorkItem::Expression(&cast.operand, scope, UseContext::Read));
        }
        ExpressionKind::Increment(increment) => {
            work.push(WorkItem::Expression(
                &increment.operand,
                scope,
                UseContext::ReadWrite,
            ));
        }
        ExpressionKind::LazyIndex(lazy) => {
            for index in lazy.indices.iter().rev() {
                work.push(WorkItem::Expression(index, scope, UseContext::Read));
            }
            work.push(WorkItem::Expression(&lazy.base, scope, context));
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

    #[test]
    fn fact_composition_is_unchanged_by_use_context_threading() {
        let source = r#"integer g;
integer f(integer p) {
    integer a;
    a = g;
    integer b = (a = 2);
    f(a);
    vector v;
    v.x = 1.0;
    v = <1.0, 2.0, 3.0>;
    if (b) { g = 1; } else { g = 2; }
    for (a = 0; a < 3; a = a + 1) { }
    switch (a) { case 1: g = 3; }
    (integer)g[a];
    return a;
}
default { state_entry() { f(g); } }
"#;
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = crate::ast::parse_ast_with_options(
            &tokens,
            crate::ast::AstParseOptions::lsl()
                .with_switches_enabled(true)
                .with_lazy_lists_enabled(true),
        );
        assert!(syntax.is_empty(), "{syntax:?}");
        let scopes = ScopeTree::build(&ast);
        let facts = collect_facts(&ast, &scopes);

        // This sequence pins the historical pre-order composition: adding use
        // contexts must not add, remove, or reorder expression facts.
        let expected_expressions: &[(&str, usize, usize)] = &[
            ("a = g", 53, 58),
            ("a", 53, 54),
            ("g", 57, 58),
            ("(a = 2)", 76, 83),
            ("a = 2", 77, 82),
            ("a", 77, 78),
            ("2", 81, 82),
            ("f(a)", 89, 93),
            ("f", 89, 90),
            ("a", 91, 92),
            ("v.x = 1.0", 113, 122),
            ("v.x", 113, 116),
            ("v", 113, 114),
            ("1.0", 119, 122),
            ("v = <1.0, 2.0, 3.0>", 128, 147),
            ("v", 128, 129),
            ("<1.0, 2.0, 3.0>", 132, 147),
            ("1.0", 133, 136),
            ("2.0", 138, 141),
            ("3.0", 143, 146),
            ("b", 157, 158),
            ("g = 1", 162, 167),
            ("g", 162, 163),
            ("1", 166, 167),
            ("g = 2", 178, 183),
            ("g", 178, 179),
            ("2", 182, 183),
            ("a = 0", 196, 201),
            ("a", 196, 197),
            ("0", 200, 201),
            ("a < 3", 203, 208),
            ("a", 203, 204),
            ("3", 207, 208),
            ("a = a + 1", 210, 219),
            ("a", 210, 211),
            ("a + 1", 214, 219),
            ("a", 214, 215),
            ("1", 218, 219),
            ("a", 237, 238),
            ("1", 247, 248),
            ("g = 3", 250, 255),
            ("g", 250, 251),
            ("3", 254, 255),
            ("(integer)g[a]", 263, 276),
            ("g[a]", 272, 276),
            ("g", 272, 273),
            ("a", 274, 275),
            ("a", 289, 290),
            ("f(g)", 320, 324),
            ("f", 320, 321),
            ("g", 322, 323),
        ];
        let observed: Vec<_> = facts
            .expressions
            .iter()
            .map(|site| {
                (
                    &source[site.expression.span.start..site.expression.span.end],
                    site.expression.span.start,
                    site.expression.span.end,
                )
            })
            .collect();
        assert_eq!(observed, expected_expressions);
        assert!(
            facts
                .expressions
                .windows(2)
                .all(|pair| pair[0].expression.span.start <= pair[1].expression.span.start),
            "expression facts must stay source ordered"
        );

        let expected_initializers: &[(&str, usize)] = &[("b", 72)];
        let initializers: Vec<_> = facts
            .initializers
            .iter()
            .map(|site| {
                (
                    &source[site.declarator.name_span.start..site.declarator.name_span.end],
                    site.declarator.name_span.start,
                )
            })
            .collect();
        assert_eq!(initializers, expected_initializers);
        assert!(
            facts.initializers.windows(2).all(
                |pair| pair[0].declarator.name_span.start <= pair[1].declarator.name_span.start
            ),
            "initializer facts must stay source ordered"
        );
    }
}
