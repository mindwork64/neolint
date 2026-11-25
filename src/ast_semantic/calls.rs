//! Single-pass AST traversal for expression call sites.

use crate::ast::{
    Ast, Block, CallExpression, Expression, ExpressionKind, Statement, StatementKind, TopLevel,
};

use super::scopes::{ScopeId, ScopeTree};

/// Identity of a user function: the start offset of its declaration name.
///
/// This is the same identity the first-definition policy uses for duplicate
/// declarations, so call ownership and function lookup cannot disagree.
pub(crate) type FunctionId = usize;

/// The declaration whose body contains a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallOwner {
    /// Initializer of a top-level variable declaration.
    GlobalInitializer,
    /// Body of a canonical or extension event handler in any state.
    EventHandler,
    /// Body of the user function with this declaration identity.
    Function(FunctionId),
}

/// One call expression together with the lexical scope and owner where it
/// appears.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallSite<'a> {
    pub(crate) call: &'a CallExpression,
    pub(crate) scope: ScopeId,
    pub(crate) owner: CallOwner,
    /// Event name when this call appears directly inside an event handler.
    pub(crate) event_name: Option<&'a str>,
}

/// Collect every call expression exactly once, including nested arguments,
/// switch/lazy expressions and all expression-bearing AST fields.
pub(crate) fn collect_calls<'a>(ast: &'a Ast, scopes: &ScopeTree) -> Vec<CallSite<'a>> {
    let mut calls = Vec::new();
    for item in &ast.items {
        match item {
            TopLevel::Global(global) => {
                for declarator in &global.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        collect_in_expression(
                            initializer,
                            scopes.root(),
                            CallOwner::GlobalInitializer,
                            None,
                            &mut calls,
                        );
                    }
                }
            }
            TopLevel::Function(function) => {
                let Some(body) = &function.body else {
                    continue;
                };
                let owner = CallOwner::Function(function.name_span.start);
                let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                walk_block(body, scope, owner, None, scopes, &mut calls);
            }
            TopLevel::State(state) => {
                for event in &state.events {
                    let Some(body) = &event.body else {
                        continue;
                    };
                    let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                    walk_block(
                        body,
                        scope,
                        CallOwner::EventHandler,
                        Some(event.name.as_str()),
                        scopes,
                        &mut calls,
                    );
                }
            }
            TopLevel::Error(_) => {}
        }
    }
    calls
}

fn walk_block<'a>(
    block: &'a Block,
    scope: ScopeId,
    owner: CallOwner,
    event_name: Option<&'a str>,
    scopes: &ScopeTree,
    calls: &mut Vec<CallSite<'a>>,
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
                        collect_in_expression(initializer, current_scope, owner, event_name, calls);
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
                collect_in_expression(
                    &if_statement.condition,
                    current_scope,
                    owner,
                    event_name,
                    calls,
                );
                work.push((&if_statement.then_branch, current_scope));
                if let Some(else_branch) = &if_statement.else_branch {
                    work.push((else_branch, current_scope));
                }
            }
            StatementKind::For(for_statement) => {
                for initializer in &for_statement.initializer {
                    collect_in_expression(initializer, current_scope, owner, event_name, calls);
                }
                collect_in_expression(
                    &for_statement.condition,
                    current_scope,
                    owner,
                    event_name,
                    calls,
                );
                for increment in &for_statement.increment {
                    collect_in_expression(increment, current_scope, owner, event_name, calls);
                }
                work.push((&for_statement.body, current_scope));
            }
            StatementKind::While(while_statement) => {
                collect_in_expression(
                    &while_statement.condition,
                    current_scope,
                    owner,
                    event_name,
                    calls,
                );
                work.push((&while_statement.body, current_scope));
            }
            StatementKind::DoWhile(do_while) => {
                collect_in_expression(&do_while.condition, current_scope, owner, event_name, calls);
                work.push((&do_while.body, current_scope));
            }
            StatementKind::Return(return_statement) => {
                if let Some(value) = &return_statement.value {
                    collect_in_expression(value, current_scope, owner, event_name, calls);
                }
            }
            StatementKind::Expression(expression) => {
                collect_in_expression(
                    &expression.expression,
                    current_scope,
                    owner,
                    event_name,
                    calls,
                );
            }
            StatementKind::Switch(switch) => {
                collect_in_expression(&switch.condition, current_scope, owner, event_name, calls);
                let switch_scope = scopes.scope_for_switch(switch).unwrap_or(current_scope);
                for arm in &switch.arms {
                    if let crate::ast::SwitchLabel::Case(expression) = &arm.label {
                        collect_in_expression(expression, switch_scope, owner, event_name, calls);
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
    owner: CallOwner,
    event_name: Option<&'a str>,
    calls: &mut Vec<CallSite<'a>>,
) {
    let mut work: Vec<(&Expression, ScopeId)> = vec![(expression, scope)];
    while let Some((expression, scope)) = work.pop() {
        match &expression.kind {
            ExpressionKind::Call(call) => {
                calls.push(CallSite {
                    call,
                    scope,
                    owner,
                    event_name,
                });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parse_ast;
    use crate::ast_semantic::rules::{builtin_calls, user_calls};
    use crate::ast_semantic::scopes::ScopeTree;
    use crate::ast_semantic::types::collect_user_functions;
    use crate::lex;

    fn parsed(source: &str) -> Ast {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast(&tokens);
        assert!(syntax.is_empty(), "{syntax:?}");
        ast
    }

    fn callee_name<'a>(site: &CallSite<'a>) -> &'a str {
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            panic!("expected identifier callee");
        };
        &callee.name
    }

    #[test]
    fn owners_are_assigned_by_the_single_traversal() {
        let source = "integer g = global_value();\nglobal_value() { return 1; }\nfunction_owner() { function_value(); }\nfunction_value() { }\ndefault {\n    state_entry() { builtin(); function_owner(); }\n    custom_event() { function_value(); }\n}\nstate active {\n    state_entry() { function_owner(); }\n}\n";
        let ast = parsed(source);
        let scopes = ScopeTree::build(&ast);
        let calls = collect_calls(&ast, &scopes);

        let names: Vec<&str> = calls.iter().map(callee_name).collect();
        assert_eq!(
            names,
            vec![
                "global_value",
                "function_value",
                "builtin",
                "function_owner",
                "function_value",
                "function_owner",
            ]
        );

        let function_owner = source.find("function_owner").unwrap();
        let expected = [
            CallOwner::GlobalInitializer,
            CallOwner::Function(function_owner),
            CallOwner::EventHandler,
            CallOwner::EventHandler,
            CallOwner::EventHandler,
            CallOwner::EventHandler,
        ];
        assert_eq!(
            calls.iter().map(|site| site.owner).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            calls.iter().map(|site| site.event_name).collect::<Vec<_>>(),
            vec![
                None,
                None,
                Some("state_entry"),
                Some("state_entry"),
                Some("custom_event"),
                Some("state_entry"),
            ]
        );

        // Every call still resolves through the lexical scope it appeared in.
        let default_scope = scopes
            .scope_for_block(match &ast.items[4] {
                TopLevel::State(state) => state.events[0].body.as_ref().unwrap(),
                _ => panic!("expected default state"),
            })
            .unwrap();
        assert_eq!(calls[2].scope, default_scope);
        assert_eq!(calls[3].scope, default_scope);
    }

    #[test]
    fn builtin_and_user_call_rules_are_unchanged_by_call_owners() {
        let source = "integer user(integer value) { return value; }\ndefault { state_entry() { llSay(); user(\"bad\"); user(1, 2); } }\n";
        let ast = parsed(source);
        let scopes = ScopeTree::build(&ast);
        let calls = collect_calls(&ast, &scopes);
        let functions = collect_user_functions(&ast);
        assert_eq!(
            calls.iter().map(|site| site.owner).collect::<Vec<_>>(),
            vec![CallOwner::EventHandler; 3]
        );

        let builtin = builtin_calls::check(&calls, &scopes, &functions, &[]);
        assert_eq!(
            builtin
                .iter()
                .map(|diagnostic| (diagnostic.code, diagnostic.message.as_str()))
                .collect::<Vec<_>>(),
            vec![("SEM002", "llSay expects 2 argument(s), but received 0")]
        );

        let user = user_calls::check(&calls, &scopes, &functions, &[]);
        assert_eq!(
            user.iter()
                .map(|diagnostic| (diagnostic.code, diagnostic.message.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "SEM006",
                    "user argument 1 has type string, expected integer"
                ),
                ("SEM005", "user expects 1 argument(s), but received 2"),
            ]
        );
    }
}
