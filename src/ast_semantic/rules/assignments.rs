//! SEM016/SEM017: plain and arithmetic compound variable assignments.

use std::collections::HashMap;

use crate::ast::{AssignmentOperator, BinaryOperator, ExpressionKind};
use crate::diagnostic::Diagnostic;

use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::super::types::{
    can_implicitly_convert, infer_binary_result_type, infer_expression_type, UserFunctionSignature,
};
use super::semantic_error;

pub(crate) fn check(
    sites: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for site in sites {
        let ExpressionKind::Assignment(assignment) = &site.expression.kind else {
            continue;
        };
        let ExpressionKind::Identifier(target) = &assignment.target.kind else {
            continue;
        };
        let Some(symbol) = scopes.lookup_value_at(site.scope, &target.name, target.span.start)
        else {
            continue;
        };
        let Some(expected_type) = symbol.type_name.as_deref() else {
            continue;
        };
        let Some(actual_type) =
            infer_expression_type(&assignment.value, scopes, site.scope, functions)
        else {
            continue;
        };
        match assignment.operator {
            AssignmentOperator::Assign => {
                if !can_implicitly_convert(&actual_type, expected_type) {
                    diagnostics.push(semantic_error(
                        "SEM016",
                        format!(
                            "assignment to variable '{}' has type {}, expected {}",
                            target.name, actual_type, expected_type
                        ),
                        target.span,
                    ));
                }
            }
            _ => {
                let Some((binary, symbol)) = compound_operator(assignment.operator) else {
                    continue;
                };
                let Some(result_type) =
                    infer_binary_result_type(binary, expected_type, &actual_type)
                else {
                    diagnostics.push(semantic_error(
                        "SEM017",
                        format!(
                            "compound assignment '{}' is not supported for operand types {} and {}",
                            symbol, expected_type, actual_type
                        ),
                        target.span,
                    ));
                    continue;
                };
                if !can_implicitly_convert(result_type, expected_type) {
                    diagnostics.push(semantic_error(
                        "SEM017",
                        format!(
                            "compound assignment '{}' to variable '{}' yields type {}, expected {}",
                            symbol, target.name, result_type, expected_type
                        ),
                        target.span,
                    ));
                }
            }
        }
    }
    diagnostics
}

pub(crate) fn compound_operator(
    operator: AssignmentOperator,
) -> Option<(BinaryOperator, &'static str)> {
    match operator {
        AssignmentOperator::AddAssign => Some((BinaryOperator::Add, "+=")),
        AssignmentOperator::SubtractAssign => Some((BinaryOperator::Subtract, "-=")),
        AssignmentOperator::MultiplyAssign => Some((BinaryOperator::Multiply, "*=")),
        AssignmentOperator::DivideAssign => Some((BinaryOperator::Divide, "/=")),
        AssignmentOperator::RemainderAssign => Some((BinaryOperator::Remainder, "%=")),
        AssignmentOperator::Assign => None,
    }
}
