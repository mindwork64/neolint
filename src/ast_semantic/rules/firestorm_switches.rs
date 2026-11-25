//! AST-only semantic checks for the opt-in Firestorm switch extension.

use std::collections::HashMap;

use crate::ast::{
    Ast, BinaryOperator, Expression, ExpressionKind, Statement, StatementKind, SwitchLabel,
    SwitchStatement, TopLevel, UnaryOperator,
};
use crate::builtins::{self, ConstantDefinition};
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};

use super::super::scopes::{ScopeId, ScopeTree};
use super::super::types::{infer_binary_result_type, infer_expression_type, UserFunctionSignature};
use super::semantic_error;

const MAX_SWITCH_RULE_WORK: usize = 16_384;
const MAX_CONSTANT_NODES: usize = 256;
const MAX_CONSTANT_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
enum ConstantValue {
    Integer(i32),
    Float(f32),
    String(String),
    Key(String),
    Vector(Vec<ConstantValue>),
    Rotation(Vec<ConstantValue>),
    ListLength(usize),
}

/// Opaque constant value shared with the flow-sensitive switch dispatcher.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SwitchConstantValue(ConstantValue);

impl SwitchConstantValue {
    pub(crate) fn is_compatible_with(&self, other: &Self) -> bool {
        matches!(
            (&self.0, &other.0),
            (ConstantValue::Integer(_), ConstantValue::Integer(_))
                | (ConstantValue::Integer(_), ConstantValue::Float(_))
                | (ConstantValue::Float(_), ConstantValue::Integer(_))
                | (ConstantValue::Float(_), ConstantValue::Float(_))
                | (ConstantValue::String(_), ConstantValue::String(_))
                | (ConstantValue::String(_), ConstantValue::Key(_))
                | (ConstantValue::Key(_), ConstantValue::String(_))
                | (ConstantValue::Key(_), ConstantValue::Key(_))
                | (ConstantValue::Vector(_), ConstantValue::Vector(_))
                | (ConstantValue::Rotation(_), ConstantValue::Rotation(_))
                | (ConstantValue::ListLength(_), ConstantValue::ListLength(_))
        )
    }

    pub(crate) fn equals(&self, other: &Self) -> bool {
        self.0.equals(&other.0)
    }
}

impl ConstantValue {
    fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => left == right,
            (Self::Float(left), Self::Float(right)) => left == right,
            (Self::Integer(integer), Self::Float(float))
            | (Self::Float(float), Self::Integer(integer)) => (*integer as f32) == *float,
            (Self::String(left), Self::String(right))
            | (Self::Key(left), Self::Key(right))
            | (Self::String(left), Self::Key(right))
            | (Self::Key(left), Self::String(right)) => left == right,
            (Self::Vector(left), Self::Vector(right))
            | (Self::Rotation(left), Self::Rotation(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| left.equals(right))
            }
            (Self::ListLength(left), Self::ListLength(right)) => left == right,
            _ => false,
        }
    }

    fn is_finite(&self) -> bool {
        match self {
            Self::Float(value) => value.is_finite(),
            Self::Vector(values) | Self::Rotation(values) => values.iter().all(Self::is_finite),
            Self::Integer(_) | Self::String(_) | Self::Key(_) | Self::ListLength(_) => true,
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Integer(value) => *value != 0,
            Self::Float(value) => *value != 0.0,
            Self::String(value) => !value.is_empty(),
            Self::Key(value) => is_nonzero_key(value),
            Self::Vector(values) => values.iter().any(Self::truthy),
            Self::Rotation(values) => values.iter().enumerate().any(|(index, value)| {
                if index == 3 {
                    !value.equals(&Self::Float(1.0))
                } else {
                    value.truthy()
                }
            }),
            Self::ListLength(length) => *length > 0,
        }
    }

    fn as_integer(&self) -> Option<i32> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    fn as_float(&self) -> Option<f32> {
        match self {
            Self::Integer(value) => Some(*value as f32),
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }
}

pub(crate) fn check(
    ast: &Ast,
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable {
        return Vec::new();
    }
    let (switches, overflow) = collect_switches(ast, scopes);
    if overflow {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let mut work = switches.len();
    let constants = &builtins::definitions().constants;

    for (switch, scope) in switches {
        if has_syntax_error_in_switch(switch, syntax_diagnostics)
            || contains_error(&switch.condition)
            || switch.arms.iter().any(|arm| match &arm.label {
                SwitchLabel::Error(_) => true,
                SwitchLabel::Case(expression) => contains_error(expression),
                SwitchLabel::Default => false,
            })
        {
            continue;
        }

        work = work.saturating_add(switch.arms.len());
        if work > MAX_SWITCH_RULE_WORK {
            return Vec::new();
        }

        let switch_type = infer_expression_type(&switch.condition, scopes, scope, functions);
        let switch_value = evaluate_constant(&switch.condition, scope, scopes, constants);

        if switch_value.is_some() {
            diagnostics.push(lint_warning(
                "SEM032",
                "switch expression is constant",
                switch.condition.span,
            ));
        }
        if switch_type.as_deref() == Some("list") {
            diagnostics.push(lint_warning(
                "SEM033",
                "switch expression has list type; list equality compares list lengths",
                switch.condition.span,
            ));
        }

        let mut default_count = 0usize;
        let mut previous_cases: Vec<(ConstantValue, Span)> = Vec::new();

        for arm in &switch.arms {
            match &arm.label {
                SwitchLabel::Default => {
                    default_count += 1;
                    if default_count == 2 {
                        diagnostics.push(semantic_error(
                            "SEM028",
                            "switch statement contains more than one default label".to_owned(),
                            default_label_span(arm.span),
                        ));
                    }
                }
                SwitchLabel::Case(expression) => {
                    let case_type = infer_expression_type(expression, scopes, scope, functions);
                    if let (Some(switch_type), Some(case_type)) =
                        (switch_type.as_deref(), case_type.as_deref())
                    {
                        if infer_binary_result_type(BinaryOperator::Equal, switch_type, case_type)
                            .is_none()
                        {
                            diagnostics.push(semantic_error(
                                "SEM029",
                                format!(
                                    "case label type '{}' is incompatible with switch expression type '{}'",
                                    case_type, switch_type
                                ),
                                expression.span,
                            ));
                            continue;
                        }
                    }

                    let Some(value) = evaluate_constant(expression, scope, scopes, constants)
                    else {
                        continue;
                    };

                    for (previous, previous_span) in &previous_cases {
                        work = work.saturating_add(1);
                        if work > MAX_SWITCH_RULE_WORK {
                            return Vec::new();
                        }
                        if value.equals(previous) {
                            diagnostics.push(lint_warning(
                                "SEM030",
                                format!(
                                    "duplicate switch case value; first used at {}:{}",
                                    previous_span.line, previous_span.column
                                ),
                                expression.span,
                            ));
                            break;
                        }
                    }
                    previous_cases.push((value, expression.span));
                }
                SwitchLabel::Error(_) => {}
            }
        }

        if default_count == 0 {
            diagnostics.push(lint_warning(
                "SEM031",
                "switch statement has no default label",
                switch.span,
            ));
        }
    }

    diagnostics
}

fn collect_switches<'a>(
    ast: &'a Ast,
    scopes: &ScopeTree,
) -> (Vec<(&'a SwitchStatement, ScopeId)>, bool) {
    let mut result = Vec::new();
    let mut work: Vec<(&Statement, ScopeId)> = Vec::new();

    for item in ast.items.iter().rev() {
        match item {
            TopLevel::Function(function) => {
                if let Some(body) = &function.body {
                    let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                    work.extend(
                        body.statements
                            .iter()
                            .rev()
                            .map(|statement| (statement, scope)),
                    );
                }
            }
            TopLevel::State(state) => {
                for event in state.events.iter().rev() {
                    if let Some(body) = &event.body {
                        let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                        work.extend(
                            body.statements
                                .iter()
                                .rev()
                                .map(|statement| (statement, scope)),
                        );
                    }
                }
            }
            TopLevel::Global(_) | TopLevel::Error(_) => {}
        }
    }

    let mut visited = 0usize;
    while let Some((statement, scope)) = work.pop() {
        visited = visited.saturating_add(1);
        if visited > MAX_SWITCH_RULE_WORK {
            return (Vec::new(), true);
        }

        match &statement.kind {
            StatementKind::Block(block) => {
                let child = scopes.scope_for_block(block).unwrap_or(scope);
                work.extend(
                    block
                        .statements
                        .iter()
                        .rev()
                        .map(|statement| (statement, child)),
                );
            }
            StatementKind::If(if_statement) => {
                if let Some(else_branch) = &if_statement.else_branch {
                    work.push((else_branch, scope));
                }
                work.push((&if_statement.then_branch, scope));
            }
            StatementKind::For(for_statement) => work.push((&for_statement.body, scope)),
            StatementKind::While(while_statement) => work.push((&while_statement.body, scope)),
            StatementKind::DoWhile(do_while) => work.push((&do_while.body, scope)),
            StatementKind::Switch(switch) => {
                let switch_scope = scopes.scope_for_switch(switch).unwrap_or(scope);
                result.push((switch, switch_scope));
                for arm in switch.arms.iter().rev() {
                    work.extend(
                        arm.statements
                            .iter()
                            .rev()
                            .map(|statement| (statement, switch_scope)),
                    );
                }
            }
            StatementKind::Return(_)
            | StatementKind::Jump(_)
            | StatementKind::Label(_)
            | StatementKind::Break(_)
            | StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::StateChange(_)
            | StatementKind::Expression(_)
            | StatementKind::Error(_) => {}
        }
    }

    (result, false)
}

fn has_syntax_error_in_switch(switch: &SwitchStatement, diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::Syntax
            && diagnostic.severity == Severity::Error
            && if diagnostic.span.start == diagnostic.span.end {
                switch.span.start <= diagnostic.span.start
                    && diagnostic.span.start <= switch.span.end
            } else {
                diagnostic.span.start < switch.span.end && switch.span.start < diagnostic.span.end
            }
    })
}

fn default_label_span(arm_span: Span) -> Span {
    Span {
        start: arm_span.start,
        end: arm_span.start.saturating_add("default".len()),
        line: arm_span.line,
        column: arm_span.column,
        end_line: arm_span.line,
        end_column: arm_span.column.saturating_add("default".len()),
    }
}

fn lint_warning(code: &'static str, message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic {
        code,
        severity: Severity::Warning,
        kind: DiagnosticKind::Lint,
        message: message.into(),
        span,
    }
}

fn contains_error(expression: &Expression) -> bool {
    let mut pending = vec![(expression, 0usize)];
    let mut visited = 0usize;
    while let Some((expression, depth)) = pending.pop() {
        visited += 1;
        if depth > MAX_CONSTANT_DEPTH || visited > MAX_CONSTANT_NODES {
            return true;
        }
        match &expression.kind {
            ExpressionKind::Error(_) => return true,
            ExpressionKind::Parenthesized(inner) => pending.push((inner, depth + 1)),
            ExpressionKind::Call(call) => {
                pending.push((&call.callee, depth + 1));
                pending.extend(call.arguments.iter().map(|argument| (argument, depth + 1)));
            }
            ExpressionKind::Component(component) => pending.push((&component.base, depth + 1)),
            ExpressionKind::Unary(unary) => pending.push((&unary.operand, depth + 1)),
            ExpressionKind::Binary(binary) => {
                pending.push((&binary.left, depth + 1));
                pending.push((&binary.right, depth + 1));
            }
            ExpressionKind::Assignment(assignment) => {
                pending.push((&assignment.target, depth + 1));
                pending.push((&assignment.value, depth + 1));
            }
            ExpressionKind::Cast(cast) => pending.push((&cast.operand, depth + 1)),
            ExpressionKind::Increment(increment) => pending.push((&increment.operand, depth + 1)),
            ExpressionKind::LazyIndex(lazy) => {
                pending.push((&lazy.base, depth + 1));
                pending.extend(lazy.indices.iter().map(|index| (index, depth + 1)));
            }
            ExpressionKind::List(list) => {
                pending.extend(list.elements.iter().map(|element| (element, depth + 1)));
            }
            ExpressionKind::Vector(vector) => {
                pending.extend(
                    vector
                        .components
                        .iter()
                        .map(|component| (component, depth + 1)),
                );
            }
            ExpressionKind::Rotation(rotation) => {
                pending.extend(
                    rotation
                        .components
                        .iter()
                        .map(|component| (component, depth + 1)),
                );
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::String(_) => {}
        }
    }
    false
}

/// Evaluate one Firestorm switch expression or case label with the same
/// constant semantics used by SEM028–SEM033. The caller owns the shared node
/// budget so a large switch cannot multiply the per-expression work limit.
pub(crate) fn evaluate_switch_constant_value(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    budget: &mut usize,
) -> Option<SwitchConstantValue> {
    let value = evaluate_at(
        expression,
        scope,
        scopes,
        &builtins::definitions().constants,
        0,
        budget,
    )?;
    value.is_finite().then_some(SwitchConstantValue(value))
}

fn evaluate_constant(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &HashMap<String, ConstantDefinition>,
) -> Option<ConstantValue> {
    let mut budget = MAX_CONSTANT_NODES;
    evaluate_at(expression, scope, scopes, constants, 0, &mut budget)
}

fn evaluate_at(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &HashMap<String, ConstantDefinition>,
    depth: usize,
    budget: &mut usize,
) -> Option<ConstantValue> {
    if depth > MAX_CONSTANT_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;

    match &expression.kind {
        ExpressionKind::Integer(literal) => {
            parse_integer(&literal.text).map(ConstantValue::Integer)
        }
        ExpressionKind::Float(literal) => {
            literal.text.parse::<f32>().ok().map(ConstantValue::Float)
        }
        ExpressionKind::String(literal) => Some(ConstantValue::String(
            unquote_string(&literal.text).to_owned(),
        )),
        ExpressionKind::Identifier(identifier) => {
            if scopes
                .lookup_value_at(scope, &identifier.name, identifier.span.start)
                .is_some()
            {
                return None;
            }
            constants
                .get(identifier.name.as_str())
                .and_then(parse_builtin_constant)
        }
        ExpressionKind::Parenthesized(inner) => {
            evaluate_at(inner, scope, scopes, constants, depth + 1, budget)
        }
        ExpressionKind::Unary(unary) => {
            let value = evaluate_at(&unary.operand, scope, scopes, constants, depth + 1, budget)?;
            match unary.operator {
                UnaryOperator::Plus => match value {
                    ConstantValue::Integer(_) | ConstantValue::Float(_) => Some(value),
                    _ => None,
                },
                UnaryOperator::Minus => match value {
                    ConstantValue::Integer(value) => {
                        Some(ConstantValue::Integer(value.wrapping_neg()))
                    }
                    ConstantValue::Float(value) => Some(ConstantValue::Float(-value)),
                    _ => None,
                },
                UnaryOperator::Bang => Some(ConstantValue::Integer((!value.truthy()) as i32)),
                UnaryOperator::Tilde => value
                    .as_integer()
                    .map(|value| ConstantValue::Integer(!value)),
            }
        }
        ExpressionKind::Binary(binary) => {
            let left = evaluate_at(&binary.left, scope, scopes, constants, depth + 1, budget)?;
            let right = evaluate_at(&binary.right, scope, scopes, constants, depth + 1, budget)?;
            evaluate_binary(binary.operator, left, right)
        }
        ExpressionKind::Vector(vector) => {
            let mut components = Vec::with_capacity(vector.components.len());
            for component in &vector.components {
                let value = evaluate_at(component, scope, scopes, constants, depth + 1, budget)?;
                value.as_float()?;
                components.push(value);
            }
            (components.len() == 3).then_some(ConstantValue::Vector(components))
        }
        ExpressionKind::Rotation(rotation) => {
            let mut components = Vec::with_capacity(rotation.components.len());
            for component in &rotation.components {
                let value = evaluate_at(component, scope, scopes, constants, depth + 1, budget)?;
                value.as_float()?;
                components.push(value);
            }
            (components.len() == 4).then_some(ConstantValue::Rotation(components))
        }
        ExpressionKind::List(list) => {
            for element in &list.elements {
                evaluate_at(element, scope, scopes, constants, depth + 1, budget)?;
            }
            Some(ConstantValue::ListLength(list.elements.len()))
        }
        ExpressionKind::Call(_)
        | ExpressionKind::Component(_)
        | ExpressionKind::Assignment(_)
        | ExpressionKind::Cast(_)
        | ExpressionKind::Increment(_)
        | ExpressionKind::LazyIndex(_)
        | ExpressionKind::Error(_) => None,
    }
}

fn parse_builtin_constant(constant: &ConstantDefinition) -> Option<ConstantValue> {
    match constant.type_name.as_str() {
        "integer" => parse_integer(&constant.value).map(ConstantValue::Integer),
        "float" => constant.value.parse::<f32>().ok().map(ConstantValue::Float),
        "string" => Some(ConstantValue::String(
            unquote_string(&constant.value).to_owned(),
        )),
        "key" => Some(ConstantValue::Key(
            unquote_string(&constant.value).to_owned(),
        )),
        "vector" | "rotation" => {
            let components = parse_builtin_components(&constant.value)?;
            if constant.type_name == "vector" && components.len() == 3 {
                Some(ConstantValue::Vector(components))
            } else if constant.type_name == "rotation" && components.len() == 4 {
                Some(ConstantValue::Rotation(components))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_builtin_components(value: &str) -> Option<Vec<ConstantValue>> {
    let inner = value.trim().strip_prefix('<')?.strip_suffix('>')?;
    inner
        .split(',')
        .map(|component| {
            component
                .trim()
                .parse::<f32>()
                .ok()
                .map(ConstantValue::Float)
        })
        .collect()
}

fn parse_integer(text: &str) -> Option<i32> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok().map(|value| value as i32);
    }
    text.parse::<u64>().ok().map(|value| value as u32 as i32)
}

fn unquote_string(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text)
}

fn is_nonzero_key(value: &str) -> bool {
    if value.len() != 36
        || ![8, 13, 18, 23]
            .iter()
            .all(|&index| value.as_bytes()[index] == b'-')
    {
        return false;
    }
    value
        .bytes()
        .filter(|&byte| byte != b'-')
        .any(|byte| byte != b'0')
        && value
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_hexdigit())
}

fn evaluate_binary(
    operator: BinaryOperator,
    left: ConstantValue,
    right: ConstantValue,
) -> Option<ConstantValue> {
    use BinaryOperator::*;

    if matches!(operator, Equal | NotEqual) {
        let equal = left.equals(&right);
        return Some(ConstantValue::Integer(if operator == Equal {
            equal as i32
        } else {
            (!equal) as i32
        }));
    }
    if matches!(operator, AndAnd | OrOr) {
        let left = left.truthy();
        let right = right.truthy();
        return Some(ConstantValue::Integer(match operator {
            AndAnd => (left && right) as i32,
            OrOr => (left || right) as i32,
            _ => unreachable!(),
        }));
    }

    match (left, right) {
        (ConstantValue::Integer(left), ConstantValue::Integer(right)) => {
            let value = match operator {
                Less => (left < right) as i32,
                LessEqual => (left <= right) as i32,
                Greater => (left > right) as i32,
                GreaterEqual => (left >= right) as i32,
                BitAnd => left & right,
                BitOr => left | right,
                BitXor => left ^ right,
                ShiftLeft if (0..32).contains(&right) => left.wrapping_shl(right as u32),
                ShiftRight if (0..32).contains(&right) => left.wrapping_shr(right as u32),
                Add => left.wrapping_add(right),
                Subtract => left.wrapping_sub(right),
                Multiply => left.wrapping_mul(right),
                Divide if right != 0 => left.wrapping_div(right),
                Remainder if right != 0 => left.wrapping_rem(right),
                _ => return None,
            };
            Some(ConstantValue::Integer(value))
        }
        (left, right) if left.as_float().is_some() && right.as_float().is_some() => {
            let left = left.as_float()?;
            let right = right.as_float()?;
            let value = match operator {
                Less => return Some(ConstantValue::Integer((left < right) as i32)),
                LessEqual => return Some(ConstantValue::Integer((left <= right) as i32)),
                Greater => return Some(ConstantValue::Integer((left > right) as i32)),
                GreaterEqual => return Some(ConstantValue::Integer((left >= right) as i32)),
                Add => left + right,
                Subtract => left - right,
                Multiply => left * right,
                Divide if right != 0.0 => left / right,
                Remainder if right != 0.0 => left % right,
                _ => return None,
            };
            value.is_finite().then_some(ConstantValue::Float(value))
        }
        (ConstantValue::String(left), ConstantValue::String(right)) if operator == Add => {
            Some(ConstantValue::String(left + &right))
        }
        _ => None,
    }
}
