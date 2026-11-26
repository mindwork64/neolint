//! SEM045: invalid operand types for unary, logical, bitwise and increment operators.

use crate::ast::{BinaryOperator, ExpressionKind, IncrementOperator, UnaryOperator};
use crate::diagnostic::{Diagnostic, Severity};

use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::super::types::{infer_expression_type, UserFunctionSignature};
use super::semantic_error;

const MAX_OPERATOR_SITES: usize = 65_536;

/// Report operands that violate confirmed LSL unary, bitwise and increment rules.
///
/// Unknown expression types are deliberately skipped. This rule does not try
/// to complete the partial binary operator matrix or guess extension types.
pub(crate) fn check(
    sites: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    functions: &std::collections::HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable
        || sites.len() > MAX_OPERATOR_SITES
        || syntax_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    for site in sites {
        match &site.expression.kind {
            // Unary plus is intentionally outside this rule until its LSL
            // acceptance and type semantics are established independently.
            ExpressionKind::Unary(unary) if unary.operator != UnaryOperator::Plus => {
                let Some(operand_type) =
                    infer_expression_type(&unary.operand, scopes, site.scope, functions)
                else {
                    continue;
                };
                if unary_operand_is_valid(unary.operator, &operand_type) {
                    continue;
                }

                diagnostics.push(semantic_error(
                    "SEM045",
                    format!(
                        "unary operator '{}' does not support an operand of type {}",
                        unary_operator_symbol(unary.operator),
                        operand_type
                    ),
                    unary.operator_span,
                ));
            }
            ExpressionKind::Binary(binary) if is_integer_only_binary(binary.operator) => {
                let Some(left_type) =
                    infer_expression_type(&binary.left, scopes, site.scope, functions)
                else {
                    continue;
                };
                let Some(right_type) =
                    infer_expression_type(&binary.right, scopes, site.scope, functions)
                else {
                    continue;
                };
                if left_type == "integer" && right_type == "integer" {
                    continue;
                }

                diagnostics.push(semantic_error(
                    "SEM045",
                    format!(
                        "operator '{}' requires integer operands, found {} and {}",
                        binary_operator_symbol(binary.operator),
                        left_type,
                        right_type
                    ),
                    binary.operator_span,
                ));
            }
            ExpressionKind::Increment(increment) => {
                let crate::ast::ExpressionKind::Identifier(identifier) = &increment.operand.kind
                else {
                    continue;
                };
                let Some(symbol) =
                    scopes.lookup_value_at(site.scope, &identifier.name, identifier.span.start)
                else {
                    if scopes
                        .builtin_constants()
                        .contains_key(identifier.name.as_str())
                    {
                        diagnostics.push(semantic_error(
                            "SEM045",
                            format!(
                                "operator '{}' cannot modify built-in constant '{}'",
                                increment_operator_symbol(increment.operator),
                                identifier.name
                            ),
                            increment.operator_span,
                        ));
                    }
                    // Unresolved names remain the responsibility of SEM012.
                    continue;
                };
                let Some(operand_type) = symbol.type_name.as_deref() else {
                    continue;
                };
                if matches!(operand_type, "integer" | "float") {
                    continue;
                }

                diagnostics.push(semantic_error(
                    "SEM045",
                    format!(
                        "operator '{}' requires an integer or float variable, found {}",
                        increment_operator_symbol(increment.operator),
                        operand_type
                    ),
                    increment.operator_span,
                ));
            }
            _ => {}
        }
    }

    diagnostics
}

fn unary_operand_is_valid(operator: UnaryOperator, operand_type: &str) -> bool {
    match operator {
        UnaryOperator::Plus => matches!(operand_type, "integer" | "float"),
        UnaryOperator::Minus => {
            matches!(operand_type, "integer" | "float" | "vector" | "rotation")
        }
        UnaryOperator::Bang | UnaryOperator::Tilde => operand_type == "integer",
    }
}

fn is_integer_only_binary(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::AndAnd
            | BinaryOperator::OrOr
            | BinaryOperator::BitAnd
            | BinaryOperator::BitOr
            | BinaryOperator::BitXor
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
    )
}

fn unary_operator_symbol(operator: UnaryOperator) -> &'static str {
    match operator {
        UnaryOperator::Plus => "+",
        UnaryOperator::Minus => "-",
        UnaryOperator::Bang => "!",
        UnaryOperator::Tilde => "~",
    }
}

fn binary_operator_symbol(operator: BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::AndAnd => "&&",
        BinaryOperator::OrOr => "||",
        BinaryOperator::BitAnd => "&",
        BinaryOperator::BitOr => "|",
        BinaryOperator::BitXor => "^",
        BinaryOperator::ShiftLeft => "<<",
        BinaryOperator::ShiftRight => ">>",
        _ => unreachable!("only integer-only binary operators are diagnosed"),
    }
}

fn increment_operator_symbol(operator: IncrementOperator) -> &'static str {
    match operator {
        IncrementOperator::Increment => "++",
        IncrementOperator::Decrement => "--",
    }
}
