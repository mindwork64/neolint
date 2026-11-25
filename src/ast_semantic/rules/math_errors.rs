//! SEM043: statically provable division and mathematical-domain hazards.

use std::collections::HashMap;

use crate::ast::{
    AssignmentOperator, BinaryOperator, Expression, ExpressionKind, TypeKind, UnaryOperator,
};
use crate::builtins;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::super::types::{
    can_implicitly_convert, infer_binary_result_type, infer_expression_type, UserFunctionSignature,
};

const MAX_MATH_WORK: usize = 32_768;
const MAX_CONSTANT_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy)]
enum Number {
    Integer(i32),
    Float(f32),
}

impl Number {
    fn as_float(self) -> f32 {
        match self {
            Self::Integer(value) => value as f32,
            Self::Float(value) => value,
        }
    }

    fn is_zero(self) -> bool {
        match self {
            Self::Integer(value) => value == 0,
            Self::Float(value) => value == 0.0,
        }
    }

    fn is_finite(self) -> bool {
        match self {
            Self::Integer(_) => true,
            Self::Float(value) => value.is_finite(),
        }
    }

    fn is_integral(self) -> Option<bool> {
        match self {
            Self::Integer(_) => Some(true),
            Self::Float(value)
                if value.is_finite()
                    && (value as f64) >= i32::MIN as f64
                    && (value as f64) <= i32::MAX as f64 =>
            {
                Some(value.fract() == 0.0)
            }
            Self::Float(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Evaluation {
    Constant(Number),
    Unknown,
    Exhausted,
}

/// Warn on statically provable division-by-zero and imaginary math hazards.
/// Mutable variables and user-defined calls are deliberately not evaluated.
pub(crate) fn check(
    sites: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable
        || syntax_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Vec::new();
    }

    let mut work = MAX_MATH_WORK;
    let mut diagnostics = Vec::new();

    for site in sites {
        if !spend_work(&mut work) {
            return Vec::new();
        }

        match &site.expression.kind {
            ExpressionKind::Binary(binary)
                if matches!(
                    binary.operator,
                    BinaryOperator::Divide | BinaryOperator::Remainder
                ) =>
            {
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
                if infer_binary_result_type(binary.operator, &left_type, &right_type).is_none() {
                    continue;
                }

                match evaluate_number(&binary.right, site.scope, scopes, 0, &mut work) {
                    Evaluation::Constant(number) if number.is_zero() => {
                        diagnostics.push(diagnostic(binary.operator_span, binary.operator));
                    }
                    Evaluation::Exhausted => return Vec::new(),
                    Evaluation::Constant(_) | Evaluation::Unknown => {}
                }
            }
            ExpressionKind::Assignment(assignment)
                if matches!(
                    assignment.operator,
                    AssignmentOperator::DivideAssign | AssignmentOperator::RemainderAssign
                ) =>
            {
                let Some(target_type) =
                    infer_expression_type(&assignment.target, scopes, site.scope, functions)
                else {
                    continue;
                };
                let Some(value_type) =
                    infer_expression_type(&assignment.value, scopes, site.scope, functions)
                else {
                    continue;
                };
                let operator = match assignment.operator {
                    AssignmentOperator::DivideAssign => BinaryOperator::Divide,
                    AssignmentOperator::RemainderAssign => BinaryOperator::Remainder,
                    _ => continue,
                };
                if infer_binary_result_type(operator, &target_type, &value_type).is_none() {
                    continue;
                }

                match evaluate_number(&assignment.value, site.scope, scopes, 0, &mut work) {
                    Evaluation::Constant(number) if number.is_zero() => {
                        diagnostics.push(diagnostic(assignment.operator_span, operator));
                    }
                    Evaluation::Exhausted => return Vec::new(),
                    Evaluation::Constant(_) | Evaluation::Unknown => {}
                }
            }
            ExpressionKind::Call(call) => {
                let ExpressionKind::Identifier(callee) = &call.callee.kind else {
                    continue;
                };
                if !matches!(
                    callee.name.as_str(),
                    "llSqrt" | "llPow" | "llAcos" | "llAsin" | "llLog" | "llLog10"
                ) || functions.contains_key(callee.name.as_str())
                    || !call.matched_close
                    || !call.arguments_complete
                    || call.declared_argument_count != call.arguments.len()
                {
                    continue;
                }

                let Some(signature) = builtins::definitions().functions.get(callee.name.as_str())
                else {
                    continue;
                };
                let expected_argument_count = if callee.name == "llPow" { 2 } else { 1 };
                if signature.parameters.len() != expected_argument_count
                    || call.arguments.len() != expected_argument_count
                {
                    continue;
                }
                if call.arguments.len() > work {
                    return Vec::new();
                }
                work -= call.arguments.len();

                let arguments_valid = call.arguments.iter().zip(&signature.parameters).all(
                    |(argument, parameter)| {
                        infer_expression_type(argument, scopes, site.scope, functions).is_some_and(
                            |actual| can_implicitly_convert(&actual, &parameter.type_name),
                        )
                    },
                );
                if !arguments_valid {
                    continue;
                }

                let mut values = Vec::with_capacity(call.arguments.len());
                let mut unknown_argument = false;
                for argument in &call.arguments {
                    match evaluate_number(argument, site.scope, scopes, 0, &mut work) {
                        Evaluation::Constant(value) if value.is_finite() => values.push(value),
                        Evaluation::Constant(_) | Evaluation::Unknown => {
                            unknown_argument = true;
                            break;
                        }
                        Evaluation::Exhausted => return Vec::new(),
                    }
                }
                if unknown_argument {
                    continue;
                }

                let message = match callee.name.as_str() {
                    "llSqrt" if values.first().is_some_and(|value| value.as_float() < 0.0) => Some(
                        "llSqrt has a statically negative argument; this returns NaN in Mono and triggers a Math Error in LSO",
                    ),
                    "llPow"
                        if values.first().is_some_and(|value| value.as_float() < 0.0)
                            && values.get(1).is_some_and(|value| value.is_integral() == Some(false)) =>
                    {
                        Some(
                            "llPow has a statically negative base and non-integer exponent; this returns NaN in Mono and triggers a Math Error in LSO",
                        )
                    }
                    "llAcos" if values.first().is_some_and(|value| !(-1.0..=1.0).contains(&value.as_float())) => {
                        Some(
                            "llAcos has a statically out-of-range argument outside [-1.0, 1.0]; this returns NaN in Mono and triggers a Math Error in LSO",
                        )
                    }
                    "llAsin" if values.first().is_some_and(|value| !(-1.0..=1.0).contains(&value.as_float())) => {
                        Some(
                            "llAsin has a statically out-of-range argument outside [-1.0, 1.0]; this returns NaN in Mono and triggers a Math Error in LSO",
                        )
                    }
                    "llLog" if values.first().is_some_and(|value| value.as_float() <= 0.0) => {
                        Some(
                            "llLog has a statically non-positive argument; LSL returns 0.0 instead of the natural logarithm",
                        )
                    }
                    "llLog10" if values.first().is_some_and(|value| value.as_float() <= 0.0) => {
                        Some(
                            "llLog10 has a statically non-positive argument; LSL returns 0.0 instead of the base-10 logarithm",
                        )
                    }
                    _ => None,
                };
                if let Some(message) = message {
                    diagnostics.push(Diagnostic {
                        code: "SEM043",
                        severity: Severity::Warning,
                        kind: DiagnosticKind::Lint,
                        message: message.to_owned(),
                        span: callee.span,
                    });
                }
            }
            _ => {}
        }
    }

    diagnostics
}

fn diagnostic(span: crate::diagnostic::Span, operator: BinaryOperator) -> Diagnostic {
    let symbol = match operator {
        BinaryOperator::Divide => "/",
        BinaryOperator::Remainder => "%",
        _ => unreachable!("only division and remainder are diagnosed"),
    };
    Diagnostic {
        code: "SEM043",
        severity: Severity::Warning,
        kind: DiagnosticKind::Lint,
        message: format!("operator '{}' has a statically known zero divisor", symbol),
        span,
    }
}

fn spend_work(work: &mut usize) -> bool {
    if *work == 0 {
        return false;
    }
    *work -= 1;
    true
}

fn evaluate_number(
    expression: &Expression,
    scope: super::super::scopes::ScopeId,
    scopes: &ScopeTree,
    depth: usize,
    work: &mut usize,
) -> Evaluation {
    if depth > MAX_CONSTANT_DEPTH || !spend_work(work) {
        return Evaluation::Exhausted;
    }

    match &expression.kind {
        ExpressionKind::Integer(literal) => parse_integer(&literal.text)
            .map(Number::Integer)
            .map_or(Evaluation::Unknown, Evaluation::Constant),
        ExpressionKind::Float(literal) => literal
            .text
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Number::Float)
            .map_or(Evaluation::Unknown, Evaluation::Constant),
        ExpressionKind::Identifier(identifier) => {
            if scopes
                .lookup_value_at(scope, &identifier.name, identifier.span.start)
                .is_some()
            {
                return Evaluation::Unknown;
            }
            let Some(constant) = builtins::definitions().constants.get(&identifier.name) else {
                return Evaluation::Unknown;
            };
            match constant.type_name.as_str() {
                "integer" => parse_integer(&constant.value)
                    .map(Number::Integer)
                    .map_or(Evaluation::Unknown, Evaluation::Constant),
                "float" => constant
                    .value
                    .parse::<f32>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(Number::Float)
                    .map_or(Evaluation::Unknown, Evaluation::Constant),
                _ => Evaluation::Unknown,
            }
        }
        ExpressionKind::Parenthesized(inner) => {
            evaluate_number(inner, scope, scopes, depth + 1, work)
        }
        ExpressionKind::Unary(unary) => {
            let operand = evaluate_number(&unary.operand, scope, scopes, depth + 1, work);
            let Evaluation::Constant(value) = operand else {
                return operand;
            };
            match (unary.operator, value) {
                (UnaryOperator::Plus, value) => Evaluation::Constant(value),
                (UnaryOperator::Minus, Number::Integer(value)) => {
                    Evaluation::Constant(Number::Integer(value.wrapping_neg()))
                }
                (UnaryOperator::Minus, Number::Float(value)) => {
                    Evaluation::Constant(Number::Float(-value))
                }
                (UnaryOperator::Bang, Number::Integer(value)) => {
                    Evaluation::Constant(Number::Integer((value == 0) as i32))
                }
                (UnaryOperator::Tilde, Number::Integer(value)) => {
                    Evaluation::Constant(Number::Integer(!value))
                }
                _ => Evaluation::Unknown,
            }
        }
        ExpressionKind::Binary(binary) => {
            let left = evaluate_number(&binary.left, scope, scopes, depth + 1, work);
            if matches!(left, Evaluation::Exhausted) {
                return Evaluation::Exhausted;
            }
            let right = evaluate_number(&binary.right, scope, scopes, depth + 1, work);
            if matches!(right, Evaluation::Exhausted) {
                return Evaluation::Exhausted;
            }
            let (Evaluation::Constant(left), Evaluation::Constant(right)) = (left, right) else {
                return Evaluation::Unknown;
            };
            evaluate_binary(binary.operator, left, right)
                .filter(|value| value.is_finite())
                .map_or(Evaluation::Unknown, Evaluation::Constant)
        }
        ExpressionKind::Cast(cast) => {
            let operand = evaluate_number(&cast.operand, scope, scopes, depth + 1, work);
            let Evaluation::Constant(value) = operand else {
                return operand;
            };
            match (cast.ty.kind, value) {
                (TypeKind::Integer, Number::Integer(value)) => {
                    Evaluation::Constant(Number::Integer(value))
                }
                (TypeKind::Float, Number::Float(value)) => {
                    Evaluation::Constant(Number::Float(value))
                }
                (TypeKind::Float, Number::Integer(value)) => {
                    Evaluation::Constant(Number::Float(value as f32))
                }
                (TypeKind::Integer, Number::Float(value)) if value.abs() == 0.0 => {
                    Evaluation::Constant(Number::Integer(0))
                }
                _ => Evaluation::Unknown,
            }
        }
        _ => Evaluation::Unknown,
    }
}

fn evaluate_binary(operator: BinaryOperator, left: Number, right: Number) -> Option<Number> {
    use BinaryOperator::*;

    match (left, right) {
        (Number::Integer(left), Number::Integer(right)) => {
            let value = match operator {
                BitAnd => left & right,
                BitOr => left | right,
                BitXor => left ^ right,
                ShiftLeft if (0..32).contains(&right) => left.wrapping_shl(right as u32),
                ShiftRight if (0..32).contains(&right) => left.wrapping_shr(right as u32),
                Equal => (left == right) as i32,
                NotEqual => (left != right) as i32,
                Less => (left < right) as i32,
                LessEqual => (left <= right) as i32,
                Greater => (left > right) as i32,
                GreaterEqual => (left >= right) as i32,
                Add => left.wrapping_add(right),
                Subtract => left.wrapping_sub(right),
                Multiply => left.wrapping_mul(right),
                Divide if right != 0 => left.wrapping_div(right),
                Remainder if right != 0 => left.wrapping_rem(right),
                _ => return None,
            };
            Some(Number::Integer(value))
        }
        (left, right) => {
            let left = left.as_float();
            let right = right.as_float();
            let value = match operator {
                Equal => return Some(Number::Integer((left == right) as i32)),
                NotEqual => return Some(Number::Integer((left != right) as i32)),
                Less => return Some(Number::Integer((left < right) as i32)),
                LessEqual => return Some(Number::Integer((left <= right) as i32)),
                Greater => return Some(Number::Integer((left > right) as i32)),
                GreaterEqual => return Some(Number::Integer((left >= right) as i32)),
                Add => left + right,
                Subtract => left - right,
                Multiply => left * right,
                Divide if right != 0.0 => left / right,
                _ => return None,
            };
            Some(Number::Float(value))
        }
    }
}

fn parse_integer(text: &str) -> Option<i32> {
    let (negative, magnitude) = match text.strip_prefix('-') {
        Some(magnitude) => (true, magnitude),
        None => (false, text),
    };
    let value = if let Some(hex) = magnitude
        .strip_prefix("0x")
        .or_else(|| magnitude.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()? as i32
    } else {
        magnitude.parse::<u64>().ok()? as u32 as i32
    };
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_false_is_known_zero_but_canonical_true_is_not() {
        assert_eq!(
            parse_integer(
                &builtins::definitions()
                    .constants
                    .get("FALSE")
                    .unwrap()
                    .value
            ),
            Some(0)
        );
        assert_eq!(
            parse_integer(&builtins::definitions().constants.get("TRUE").unwrap().value),
            Some(1)
        );
    }
}
