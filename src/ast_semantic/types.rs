//! Conservative AST expression typing and user-function signatures.

use std::collections::HashMap;

use crate::ast::{BinaryOperator, Component, Expression, ExpressionKind, TopLevel, UnaryOperator};
use crate::builtins;
use crate::diagnostic::Span;

use super::scopes::{type_name, ScopeId, ScopeTree};

const MAX_TYPE_DEPTH: usize = 256;

/// Coarse function signature derived from an AST declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserFunctionSignature {
    pub(crate) return_type: String,
    pub(crate) parameters: Vec<String>,
    pub(crate) declaration_span: Span,
}

/// Collect the first AST declaration for each user function with a body.
pub(crate) fn collect_user_functions(
    ast: &crate::ast::Ast,
) -> HashMap<String, UserFunctionSignature> {
    let mut candidates = Vec::new();
    for item in &ast.items {
        let TopLevel::Function(function) = item else {
            continue;
        };
        if function.body.is_none() || function.name.is_empty() {
            continue;
        }
        let return_type = function
            .return_type
            .map_or_else(|| "void".to_owned(), |ty| type_name(ty.kind).to_owned());
        candidates.push((
            function.name_span.start,
            function.name.clone(),
            UserFunctionSignature {
                return_type,
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| type_name(parameter.ty.kind).to_owned())
                    .collect(),
                declaration_span: function.name_span,
            },
        ));
    }
    for function in &ast.recovered_functions {
        candidates.push((
            function.name_span.start,
            function.name.clone(),
            UserFunctionSignature {
                return_type: function.return_type.clone(),
                parameters: function.parameter_types.clone(),
                declaration_span: function.name_span,
            },
        ));
    }
    candidates.sort_by_key(|(offset, _, _)| *offset);
    let mut functions = HashMap::new();
    for (_, name, signature) in candidates {
        functions.entry(name).or_insert(signature);
    }
    functions
}

/// Infer a provable LSL type from an AST expression. Unknown is `None`.
pub(crate) fn infer_expression_type(
    expression: &Expression,
    scopes: &ScopeTree,
    scope: ScopeId,
    functions: &HashMap<String, UserFunctionSignature>,
) -> Option<String> {
    infer_at(expression, scopes, scope, functions, 0)
}

fn infer_at(
    expression: &Expression,
    scopes: &ScopeTree,
    scope: ScopeId,
    functions: &HashMap<String, UserFunctionSignature>,
    depth: usize,
) -> Option<String> {
    if depth > MAX_TYPE_DEPTH {
        return None;
    }
    match &expression.kind {
        ExpressionKind::Identifier(identifier) => scopes
            .lookup_value_at(scope, &identifier.name, identifier.span.start)
            .and_then(|symbol| symbol.type_name.clone())
            .or_else(|| {
                builtins::definitions()
                    .constants
                    .get(identifier.name.as_str())
                    .map(|constant| constant.type_name.clone())
            }),
        ExpressionKind::Integer(_) => Some("integer".to_owned()),
        ExpressionKind::Float(_) => Some("float".to_owned()),
        ExpressionKind::String(_) => Some("string".to_owned()),
        ExpressionKind::Parenthesized(inner) => {
            infer_at(inner, scopes, scope, functions, depth + 1)
        }
        ExpressionKind::List(_) => Some("list".to_owned()),
        ExpressionKind::Vector(_) => Some("vector".to_owned()),
        ExpressionKind::Rotation(_) => Some("rotation".to_owned()),
        ExpressionKind::Call(call) => {
            let ExpressionKind::Identifier(callee) = &call.callee.kind else {
                return None;
            };
            if let Some(signature) = functions.get(callee.name.as_str()) {
                return Some(signature.return_type.clone());
            }
            builtins::definitions()
                .functions
                .get(callee.name.as_str())
                .map(|signature| signature.return_type.clone())
        }
        ExpressionKind::Component(component) => {
            let ExpressionKind::Identifier(base) = &component.base.kind else {
                return None;
            };
            let base_type = scopes
                .lookup_value_at(scope, &base.name, base.span.start)?
                .type_name
                .clone()?;
            match (base_type.as_str(), component.component) {
                ("vector", Component::X | Component::Y | Component::Z) => Some("float".to_owned()),
                ("rotation", Component::X | Component::Y | Component::Z | Component::S) => {
                    Some("float".to_owned())
                }
                _ => None,
            }
        }
        ExpressionKind::Unary(unary) => {
            let operand = infer_at(&unary.operand, scopes, scope, functions, depth + 1)?;
            match unary.operator {
                UnaryOperator::Plus => {
                    matches!(operand.as_str(), "integer" | "float").then_some(operand)
                }
                UnaryOperator::Minus => matches!(
                    operand.as_str(),
                    "integer" | "float" | "vector" | "rotation"
                )
                .then_some(operand),
                UnaryOperator::Bang | UnaryOperator::Tilde => {
                    (operand == "integer").then(|| "integer".to_owned())
                }
            }
        }
        ExpressionKind::Binary(binary) => {
            let left = infer_at(&binary.left, scopes, scope, functions, depth + 1)?;
            let right = infer_at(&binary.right, scopes, scope, functions, depth + 1)?;
            infer_binary_result_type(binary.operator, &left, &right).map(str::to_owned)
        }
        ExpressionKind::Cast(cast) => {
            let target = type_name(cast.ty.kind);
            let source = infer_at(&cast.operand, scopes, scope, functions, depth + 1);
            match source {
                Some(source) => {
                    if explicit_cast_supported(&source, target) {
                        Some(target.to_owned())
                    } else {
                        None
                    }
                }
                None if matches!(target, "string" | "list") => Some(target.to_owned()),
                None => None,
            }
        }
        ExpressionKind::Assignment(_)
        | ExpressionKind::Increment(_)
        | ExpressionKind::LazyIndex(_)
        | ExpressionKind::Error(_) => None,
    }
}

fn numeric_result_type(left: &str, right: &str) -> Option<&'static str> {
    let left_numeric = matches!(left, "integer" | "float");
    let right_numeric = matches!(right, "integer" | "float");
    if !left_numeric || !right_numeric {
        return None;
    }
    if left == "float" || right == "float" {
        Some("float")
    } else {
        Some("integer")
    }
}

pub(crate) fn can_implicitly_convert(actual: &str, expected: &str) -> bool {
    actual == expected
        || (actual == "integer" && expected == "float")
        || (actual == "string" && expected == "key")
}

fn explicit_cast_supported(source: &str, target: &str) -> bool {
    match source {
        "integer" => matches!(target, "integer" | "float" | "string" | "list"),
        "float" => matches!(target, "integer" | "float" | "string" | "list"),
        "string" => matches!(
            target,
            "integer" | "float" | "string" | "key" | "list" | "vector" | "rotation"
        ),
        "key" => matches!(target, "string" | "key" | "list"),
        "list" => matches!(target, "string" | "list"),
        "vector" => matches!(target, "string" | "list" | "vector"),
        "rotation" => matches!(target, "string" | "list" | "rotation"),
        _ => false,
    }
}

fn infer_binary_result_type(
    operator: BinaryOperator,
    left: &str,
    right: &str,
) -> Option<&'static str> {
    match operator {
        BinaryOperator::OrOr | BinaryOperator::AndAnd => {
            (left == "integer" && right == "integer").then_some("integer")
        }
        BinaryOperator::Equal | BinaryOperator::NotEqual => {
            if left == right
                || numeric_result_type(left, right).is_some()
                || can_implicitly_convert(left, right)
                || can_implicitly_convert(right, left)
            {
                Some("integer")
            } else {
                None
            }
        }
        BinaryOperator::Less
        | BinaryOperator::LessEqual
        | BinaryOperator::Greater
        | BinaryOperator::GreaterEqual => numeric_result_type(left, right).map(|_| "integer"),
        BinaryOperator::BitAnd
        | BinaryOperator::BitOr
        | BinaryOperator::BitXor
        | BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight => {
            (left == "integer" && right == "integer").then_some("integer")
        }
        BinaryOperator::Add => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "string" && right == "string" {
                Some("string")
            } else if left == "list" || right == "list" {
                Some("list")
            } else if left == "vector" && right == "vector" {
                Some("vector")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else {
                None
            }
        }
        BinaryOperator::Subtract => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "vector" && right == "vector" {
                Some("vector")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else {
                None
            }
        }
        BinaryOperator::Multiply => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "vector" && right == "vector" {
                Some("float")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else if (left == "vector"
                && (matches!(right, "integer" | "float") || right == "rotation"))
                || (matches!(left, "integer" | "float") && right == "vector")
            {
                Some("vector")
            } else {
                None
            }
        }
        BinaryOperator::Divide => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "vector"
                && (matches!(right, "integer" | "float") || right == "rotation")
            {
                Some("vector")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else {
                None
            }
        }
        BinaryOperator::Remainder => {
            if left == "integer" && right == "integer" {
                Some("integer")
            } else if left == "vector" && right == "vector" {
                Some("vector")
            } else {
                None
            }
        }
    }
}

/// Span of the first source token participating in an expression.
pub(crate) fn expression_first_token_span(expression: &Expression) -> Span {
    match &expression.kind {
        ExpressionKind::Identifier(identifier) => identifier.span,
        ExpressionKind::Integer(literal) => literal.span,
        ExpressionKind::Float(literal) => literal.span,
        ExpressionKind::String(literal) => literal.span,
        ExpressionKind::Parenthesized(_)
        | ExpressionKind::List(_)
        | ExpressionKind::Vector(_)
        | ExpressionKind::Rotation(_)
        | ExpressionKind::Cast(_) => one_character_span(expression.span),
        ExpressionKind::Call(call) => expression_first_token_span(&call.callee),
        ExpressionKind::Component(component) => expression_first_token_span(&component.base),
        ExpressionKind::Unary(unary) => unary.operator_span,
        ExpressionKind::Binary(binary) => expression_first_token_span(&binary.left),
        ExpressionKind::Assignment(assignment) => expression_first_token_span(&assignment.target),
        ExpressionKind::Increment(increment) => {
            if increment.prefix {
                increment.operator_span
            } else {
                expression_first_token_span(&increment.operand)
            }
        }
        ExpressionKind::LazyIndex(lazy) => expression_first_token_span(&lazy.base),
        ExpressionKind::Error(error) => error.span,
    }
}

fn one_character_span(span: Span) -> Span {
    Span {
        start: span.start,
        end: span.start.saturating_add(1),
        line: span.line,
        column: span.column,
        end_line: span.line,
        end_column: span.column.saturating_add(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{parse_ast, Statement, StatementKind, TopLevel};
    use crate::lex;

    fn infer_source(source: &str) -> Option<String> {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast(&tokens);
        assert!(syntax.is_empty(), "{syntax:?}");
        let scopes = ScopeTree::build(&ast);
        let functions = collect_user_functions(&ast);
        for item in &ast.items {
            let bodies: Vec<&crate::ast::Block> = match item {
                TopLevel::Function(function) => function.body.iter().collect(),
                TopLevel::State(state) => state
                    .events
                    .iter()
                    .filter_map(|event| event.body.as_ref())
                    .collect(),
                TopLevel::Global(_) | TopLevel::Error(_) => Vec::new(),
            };
            for body in bodies {
                let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                if let Some((initializer, scope)) =
                    find_target_in_statements(&body.statements, &scopes, scope)
                {
                    return infer_expression_type(initializer, &scopes, scope, &functions);
                }
            }
        }
        None
    }

    fn find_target_in_statements<'a>(
        statements: &'a [Statement],
        scopes: &ScopeTree,
        scope: ScopeId,
    ) -> Option<(&'a Expression, ScopeId)> {
        for statement in statements {
            match &statement.kind {
                StatementKind::Local(local) => {
                    if let Some(declarator) =
                        local.declarators.iter().find(|item| item.name == "target")
                    {
                        if let Some(initializer) = &declarator.initializer {
                            return Some((initializer, scope));
                        }
                    }
                }
                StatementKind::Block(block) => {
                    let child = scopes.scope_for_block(block).unwrap_or(scope);
                    if let Some(found) = find_target_in_statements(&block.statements, scopes, child)
                    {
                        return Some(found);
                    }
                }
                StatementKind::If(if_statement) => {
                    if let Some(found) =
                        find_target_in_statement(&if_statement.then_branch, scopes, scope)
                    {
                        return Some(found);
                    }
                    if let Some(else_branch) = &if_statement.else_branch {
                        if let Some(found) = find_target_in_statement(else_branch, scopes, scope) {
                            return Some(found);
                        }
                    }
                }
                StatementKind::For(for_statement) => {
                    if let Some(found) =
                        find_target_in_statement(&for_statement.body, scopes, scope)
                    {
                        return Some(found);
                    }
                }
                StatementKind::While(while_statement) => {
                    if let Some(found) =
                        find_target_in_statement(&while_statement.body, scopes, scope)
                    {
                        return Some(found);
                    }
                }
                StatementKind::DoWhile(do_while) => {
                    if let Some(found) = find_target_in_statement(&do_while.body, scopes, scope) {
                        return Some(found);
                    }
                }
                StatementKind::Switch(switch) => {
                    let switch_scope = scopes.scope_for_switch(switch).unwrap_or(scope);
                    for arm in &switch.arms {
                        if let Some(found) =
                            find_target_in_statements(&arm.statements, scopes, switch_scope)
                        {
                            return Some(found);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn find_target_in_statement<'a>(
        statement: &'a Statement,
        scopes: &ScopeTree,
        scope: ScopeId,
    ) -> Option<(&'a Expression, ScopeId)> {
        find_target_in_statements(std::slice::from_ref(statement), scopes, scope)
    }

    fn infer_with_declarations(expression: &str) -> Option<String> {
        infer_source(&format!(
            "default {{ state_entry() {{ integer i = 1; float f = 1.0; string s = \"s\"; key k = \"k\"; list l; vector v; rotation r; integer target = {expression}; }} }}"
        ))
    }

    fn infer_full(source: &str) -> Option<String> {
        infer_source(source)
    }

    #[test]
    fn binary_result_matrix_matches_legacy() {
        for (expression, expected) in [
            ("i + i", Some("integer")),
            ("i + f", Some("float")),
            ("f + f", Some("float")),
            ("i == f", Some("integer")),
            ("s == k", Some("integer")),
            ("i == s", None),
            ("i < f", Some("integer")),
            ("s < s", None),
            ("i & i", Some("integer")),
            ("i << i", Some("integer")),
            ("f & i", None),
            ("s + s", Some("string")),
            ("l + i", Some("list")),
            ("v + v", Some("vector")),
            ("r + r", Some("rotation")),
            ("v - v", Some("vector")),
            ("v - r", None),
            ("r - r", Some("rotation")),
            ("v * v", Some("float")),
            ("r * r", Some("rotation")),
            ("v * r", Some("vector")),
            ("f * v", Some("vector")),
            ("v * f", Some("vector")),
            ("r * v", None),
            ("v / f", Some("vector")),
            ("r / r", Some("rotation")),
            ("v / v", None),
            ("i % i", Some("integer")),
            ("v % v", Some("vector")),
            ("s % i", None),
            ("!i", Some("integer")),
            ("!f", None),
            ("-v", Some("vector")),
            ("+r", None),
        ] {
            assert_eq!(
                infer_with_declarations(expression).as_deref(),
                expected,
                "{expression}"
            );
        }
    }

    #[test]
    fn casts_components_and_literals_match_legacy() {
        for (expression, expected) in [
            ("(float)i", Some("float")),
            ("(string)i", Some("string")),
            ("(key)s", Some("key")),
            ("(vector)s", Some("vector")),
            ("(rotation)s", Some("rotation")),
            ("(string)l", Some("string")),
            ("(list)v", Some("list")),
            ("(key)i", None),
            ("(vector)f", None),
            ("(integer)k", None),
            ("(integer)l", None),
            ("(rotation)v", None),
            ("(vector)r", None),
            ("v.x", Some("float")),
            ("r.s", Some("float")),
            ("i.x", None),
            ("[1, 2]", Some("list")),
            ("<1, 2, 3>", Some("vector")),
            ("<1, 2, 3, 4>", Some("rotation")),
        ] {
            assert_eq!(
                infer_with_declarations(expression).as_deref(),
                expected,
                "{expression}"
            );
        }
    }

    #[test]
    fn identifiers_respect_source_order_shadowing_and_functions() {
        assert_eq!(infer_with_declarations("i").as_deref(), Some("integer"));
        assert_eq!(
            infer_full("default { state_entry() { integer target = later; integer later; } }")
                .as_deref(),
            None
        );
        assert_eq!(
            infer_full(
                "default { state_entry() { integer value = 1; { string value = \"\"; integer target = value; } } }"
            )
            .as_deref(),
            Some("string")
        );
        assert_eq!(
            infer_full(
                "integer f(string p) { integer target = p; return target; } default { state_entry() { } }"
            )
            .as_deref(),
            Some("string")
        );
        assert_eq!(
            infer_full("string g; default { state_entry() { string target = g; } }").as_deref(),
            Some("string")
        );
        assert_eq!(
            infer_full(
                "integer f() { return 1; } default { state_entry() { integer target = f; } }"
            )
            .as_deref(),
            None
        );
        assert_eq!(
            infer_full("default { state_entry() { integer target = TRUE; } }").as_deref(),
            Some("integer")
        );
    }

    #[test]
    fn call_return_types_match_builtin_and_user_signatures() {
        assert_eq!(
            infer_with_declarations("llGetListLength([1])").as_deref(),
            Some("integer")
        );
        assert_eq!(
            infer_with_declarations("llGetPos()").as_deref(),
            Some("vector")
        );
        assert_eq!(
            infer_with_declarations("llGetOwner()").as_deref(),
            Some("key")
        );
        assert_eq!(
            infer_full(
                "string f() { return \"\"; } default { state_entry() { string target = f(); } }"
            )
            .as_deref(),
            Some("string")
        );
        assert_eq!(
            infer_full("f() { } default { state_entry() { string target = f(); } }").as_deref(),
            Some("void")
        );
    }
}
