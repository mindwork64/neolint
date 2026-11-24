//! SEM018/SEM019: vector/rotation component writes and reads.

use std::collections::{HashMap, HashSet};

use crate::ast::{AssignmentOperator, ComponentAccessExpression, Expression, ExpressionKind};
use crate::diagnostic::Diagnostic;

use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::super::types::{
    can_implicitly_convert, infer_binary_result_type, infer_expression_type, UserFunctionSignature,
};
use super::assignments::compound_operator;
use super::semantic_error;

pub(crate) fn check(
    sites: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut writes = HashSet::new();
    for site in sites {
        let ExpressionKind::Assignment(assignment) = &site.expression.kind else {
            continue;
        };
        let Some(component) = component_target(&assignment.target) else {
            continue;
        };
        writes.insert(component_key(component));
        let ExpressionKind::Identifier(base) = &component.base.kind else {
            continue;
        };
        let Some(symbol) = scopes.lookup_value_at(site.scope, &base.name, base.span.start) else {
            continue;
        };
        let Some(base_type) = symbol.type_name.as_deref() else {
            continue;
        };
        if !valid_component(base_type, &component.component_name) {
            diagnostics.push(semantic_error(
                "SEM018",
                format!(
                    "component '{}' is not valid for variable '{}' of type {}",
                    component.component_name, base.name, base_type
                ),
                component.component_span,
            ));
            continue;
        }
        let Some(value_type) =
            infer_expression_type(&assignment.value, scopes, site.scope, functions)
        else {
            continue;
        };
        let (result_type, operator_name) = match assignment.operator {
            AssignmentOperator::Assign => (value_type.as_str(), "assignment".to_owned()),
            _ => {
                let Some((binary, symbol)) = compound_operator(assignment.operator) else {
                    continue;
                };
                let Some(result_type) = infer_binary_result_type(binary, "float", &value_type)
                else {
                    diagnostics.push(semantic_error(
                        "SEM018",
                        format!(
                            "compound assignment '{}' is not supported for component types float and {}",
                            symbol, value_type
                        ),
                        component.component_span,
                    ));
                    continue;
                };
                (result_type, format!("compound assignment '{}'", symbol))
            }
        };
        if !can_implicitly_convert(result_type, "float") {
            diagnostics.push(semantic_error(
                "SEM018",
                format!(
                    "{} to component '{}.{}' yields type {}, expected float",
                    operator_name, base.name, component.component_name, result_type
                ),
                component.component_span,
            ));
        }
    }

    for site in sites {
        let ExpressionKind::Component(component) = &site.expression.kind else {
            continue;
        };
        if writes.contains(&component_key(component)) {
            continue;
        }
        let ExpressionKind::Identifier(base) = &component.base.kind else {
            continue;
        };
        let Some(symbol) = scopes.lookup_value_at(site.scope, &base.name, base.span.start) else {
            continue;
        };
        let Some(base_type) = symbol.type_name.as_deref() else {
            continue;
        };
        if valid_component(base_type, &component.component_name) {
            continue;
        }
        diagnostics.push(semantic_error(
            "SEM019",
            format!(
                "component access '.{}' is not valid for variable '{}' of type {}",
                component.component_name, base.name, base_type
            ),
            component.component_span,
        ));
    }
    diagnostics
}

fn component_target(expression: &Expression) -> Option<&ComponentAccessExpression> {
    let mut expression = expression;
    loop {
        match &expression.kind {
            ExpressionKind::Parenthesized(inner) => expression = inner,
            ExpressionKind::Component(component) => {
                return matches!(component.base.kind, ExpressionKind::Identifier(_))
                    .then_some(component);
            }
            _ => return None,
        }
    }
}

fn component_key(component: &ComponentAccessExpression) -> (usize, usize) {
    (component.span.start, component.span.end)
}

fn valid_component(base_type: &str, component: &str) -> bool {
    match base_type {
        "vector" => matches!(component, "x" | "y" | "z"),
        "rotation" => matches!(component, "x" | "y" | "z" | "s"),
        _ => false,
    }
}
