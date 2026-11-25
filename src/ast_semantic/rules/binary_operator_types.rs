//! SEM046: known-invalid ordinary binary operator operand types.

use std::collections::HashMap;

use crate::ast::{BinaryOperator, ExpressionKind};
use crate::diagnostic::Diagnostic;

use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::super::types::{infer_binary_result_type, infer_expression_type, UserFunctionSignature};
use super::semantic_error;

const MAX_BINARY_OPERATOR_SITES: usize = 65_536;

/// Validate the ordinary arithmetic and comparison operators against the
/// shared, conservative LSL result-type matrix.
///
/// A missing operand type is unknown and is skipped. A known pair for which
/// the shared matrix has no result is a definite incompatibility.
pub(crate) fn check(
    sites: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable
        || sites.len() > MAX_BINARY_OPERATOR_SITES
        || syntax_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::diagnostic::Severity::Error)
    {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    for site in sites {
        let ExpressionKind::Binary(binary) = &site.expression.kind else {
            continue;
        };
        if !is_checked_operator(binary.operator) {
            continue;
        }

        let Some(left_type) = infer_expression_type(&binary.left, scopes, site.scope, functions)
        else {
            continue;
        };
        let Some(right_type) = infer_expression_type(&binary.right, scopes, site.scope, functions)
        else {
            continue;
        };

        if infer_binary_result_type(binary.operator, &left_type, &right_type).is_some() {
            continue;
        }

        diagnostics.push(semantic_error(
            "SEM046",
            format!(
                "operator '{}' does not support operand types {} and {}",
                operator_symbol(binary.operator),
                left_type,
                right_type
            ),
            binary.operator_span,
        ));
    }

    diagnostics
}

fn is_checked_operator(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Remainder
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
    )
}

fn operator_symbol(operator: BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Add => "+",
        BinaryOperator::Subtract => "-",
        BinaryOperator::Multiply => "*",
        BinaryOperator::Divide => "/",
        BinaryOperator::Remainder => "%",
        BinaryOperator::Equal => "==",
        BinaryOperator::NotEqual => "!=",
        BinaryOperator::Less => "<",
        BinaryOperator::LessEqual => "<=",
        BinaryOperator::Greater => ">",
        BinaryOperator::GreaterEqual => ">=",
        _ => unreachable!("only ordinary arithmetic and comparison operators are checked"),
    }
}
