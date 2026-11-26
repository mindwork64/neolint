//! Useful LSL compatibility checks retained from LSLint.
use std::collections::HashMap;

use super::super::{
    calls::CallSite,
    flow,
    scopes::{ScopeId, ScopeTree},
};
use crate::ast::{
    AssignmentOperator, Ast, BinaryOperator, Block, Expression, ExpressionKind, Statement,
    StatementKind, TopLevel, TypeKind,
};
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};
use crate::target::{ScriptRuntime, TargetPlatform, TargetProfile};
const GOD: &[&str] = &[
    "llGodLikeRezObject",
    "llSetInventoryPermMask",
    "llSetObjectPermMask",
];
pub(crate) fn check(
    ast: &Ast,
    calls: &[CallSite<'_>],
    scopes: &ScopeTree,
    syntax: &[Diagnostic],
    reliable: bool,
    target: TargetProfile,
    god: bool,
) -> Vec<Diagnostic> {
    if !reliable {
        return Vec::new();
    }
    let mut out = Vec::new();
    let global_integer_facts = immutable_global_integer_facts(ast, scopes);
    for item in &ast.items {
        match item {
            TopLevel::Function(f) => {
                if let Some(b) = &f.body {
                    let s = scopes.scope_for_block(b).unwrap_or(scopes.root());
                    walk(b, b, s, scopes, syntax, &global_integer_facts, &mut out)
                }
            }
            TopLevel::State(st) => {
                for ev in &st.events {
                    if let Some(b) = &ev.body {
                        let s = scopes.scope_for_block(b).unwrap_or(scopes.root());
                        walk(b, b, s, scopes, syntax, &global_integer_facts, &mut out);
                        if target.platform() == TargetPlatform::SecondLife {
                            same_state(b, &st.name, st.is_default, &mut out)
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if target.platform() == TargetPlatform::SecondLife && !god {
        for c in calls {
            if let ExpressionKind::Identifier(id) = &c.call.callee.kind {
                if GOD.contains(&id.name.as_str()) {
                    out.push(diag("SEM058",Severity::Error,format!("{} requires Second Life God Mode; use --god-mode only for scripts permitted to use privileged functions",id.name),c.call.span))
                }
            }
        }
    }
    if target.platform() == TargetPlatform::SecondLife {
        for c in calls {
            if let ExpressionKind::Identifier(id) = &c.call.callee.kind {
                if id.name == "print" {
                    let msg = if target.runtime() == ScriptRuntime::Mono {
                        "print() causes compilation to fail on the Second Life Mono compiler"
                    } else {
                        "print() may cause a runtime error on this Second Life target"
                    };
                    out.push(diag("SEM057", Severity::Warning, msg.into(), c.call.span))
                }
            }
        }
    }
    out
}
fn walk(
    b: &Block,
    root_block: &Block,
    scope: ScopeId,
    scopes: &ScopeTree,
    syntax: &[Diagnostic],
    global_integer_facts: &HashMap<usize, i32>,
    out: &mut Vec<Diagnostic>,
) {
    for s in &b.statements {
        match &s.kind {
            StatementKind::If(i) => {
                if matches!(i.then_branch.kind, StatementKind::Empty)
                    && i.else_branch
                        .as_deref()
                        .map_or(true, |e| matches!(e.kind, StatementKind::Empty))
                    && !bad(i.span.start, i.span.end, syntax)
                {
                    out.push(diag(
                        "SEM055",
                        Severity::Warning,
                        "if statement has no effect because both branches are empty".into(),
                        i.condition.span,
                    ))
                }
                if !bad(i.condition.span.start, i.condition.span.end, syntax) {
                    report_condition(
                        &i.condition,
                        true,
                        scope,
                        scopes,
                        root_block,
                        global_integer_facts,
                        out,
                    );
                }
                walk_stmt(
                    &i.then_branch,
                    root_block,
                    scope,
                    scopes,
                    syntax,
                    global_integer_facts,
                    out,
                );
                if let Some(e) = &i.else_branch {
                    walk_stmt(
                        e,
                        root_block,
                        scope,
                        scopes,
                        syntax,
                        global_integer_facts,
                        out,
                    )
                }
            }
            StatementKind::While(w) => {
                report_condition(
                    &w.condition,
                    false,
                    scope,
                    scopes,
                    root_block,
                    global_integer_facts,
                    out,
                );
                walk_stmt(
                    &w.body,
                    root_block,
                    scope,
                    scopes,
                    syntax,
                    global_integer_facts,
                    out,
                );
            }
            StatementKind::For(f) => {
                if !f.condition_missing && !f.condition_malformed {
                    report_condition(
                        &f.condition,
                        false,
                        scope,
                        scopes,
                        root_block,
                        global_integer_facts,
                        out,
                    );
                }
                walk_stmt(
                    &f.body,
                    root_block,
                    scope,
                    scopes,
                    syntax,
                    global_integer_facts,
                    out,
                )
            }
            StatementKind::DoWhile(w) => {
                report_condition(
                    &w.condition,
                    false,
                    scope,
                    scopes,
                    root_block,
                    global_integer_facts,
                    out,
                );
                walk_stmt(
                    &w.body,
                    root_block,
                    scope,
                    scopes,
                    syntax,
                    global_integer_facts,
                    out,
                );
            }
            StatementKind::Block(x) => walk(
                x,
                root_block,
                scopes.scope_for_block(x).unwrap_or(scope),
                scopes,
                syntax,
                global_integer_facts,
                out,
            ),
            StatementKind::Switch(x) => {
                let switch_scope = scopes.scope_for_switch(x).unwrap_or(scope);
                for a in &x.arms {
                    for st in &a.statements {
                        walk_stmt(
                            st,
                            root_block,
                            switch_scope,
                            scopes,
                            syntax,
                            global_integer_facts,
                            out,
                        )
                    }
                }
            }
            _ => {}
        }
    }
}
fn report_condition(
    expression: &Expression,
    warn_if_true: bool,
    scope: ScopeId,
    scopes: &ScopeTree,
    root_block: &Block,
    global_integer_facts: &HashMap<usize, i32>,
    out: &mut Vec<Diagnostic>,
) {
    let value = flow::evaluate_static_condition_with_integer_facts(
        expression,
        scope,
        scopes,
        global_integer_facts,
    )
    .or_else(|| evaluate_empty_string_comparison(expression, root_block, scope, scopes))
    .or_else(|| evaluate_assignment_truthiness(expression, scope, scopes))
    .or_else(|| flow::evaluate_static_condition(expression, scope, scopes));
    if let Some(value) = value {
        if !value || warn_if_true {
            out.push(diag(
                "SEM056",
                Severity::Warning,
                if value {
                    "condition is always true"
                } else {
                    "condition is always false"
                }
                .into(),
                expression.span,
            ));
        }
    }
}

fn walk_stmt(
    s: &Statement,
    root_block: &Block,
    scope: ScopeId,
    scopes: &ScopeTree,
    syntax: &[Diagnostic],
    global_integer_facts: &HashMap<usize, i32>,
    out: &mut Vec<Diagnostic>,
) {
    if let StatementKind::Block(b) = &s.kind {
        walk(
            b,
            root_block,
            scopes.scope_for_block(b).unwrap_or(scope),
            scopes,
            syntax,
            global_integer_facts,
            out,
        )
    } else {
        let b = Block {
            span: s.span,
            closed: true,
            statements: vec![s.clone()],
        };
        walk(
            &b,
            root_block,
            scope,
            scopes,
            syntax,
            global_integer_facts,
            out,
        )
    }
}

fn evaluate_assignment_truthiness(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
) -> Option<bool> {
    let expression = unwrap_parentheses(expression);
    let ExpressionKind::Assignment(assignment) = &expression.kind else {
        return None;
    };
    if assignment.operator != AssignmentOperator::Assign {
        return None;
    }
    let ExpressionKind::Identifier(target) = &unwrap_parentheses(&assignment.target).kind else {
        return None;
    };
    let symbol = scopes.lookup_value_at(scope, &target.name, target.span.start)?;
    match &unwrap_parentheses(&assignment.value).kind {
        ExpressionKind::String(literal) if symbol.type_name.as_deref() == Some("string") => {
            Some(!is_empty_string_literal(&literal.text))
        }
        _ => flow::evaluate_static_condition(expression, scope, scopes),
    }
}

fn evaluate_empty_string_comparison(
    expression: &Expression,
    root_block: &Block,
    scope: ScopeId,
    scopes: &ScopeTree,
) -> Option<bool> {
    let expression = unwrap_parentheses(expression);
    let ExpressionKind::Binary(binary) = &expression.kind else {
        return None;
    };
    if !matches!(
        binary.operator,
        BinaryOperator::Equal | BinaryOperator::NotEqual
    ) {
        return None;
    }

    let identifier = empty_string_pair(&binary.left, &binary.right)
        .or_else(|| empty_string_pair(&binary.right, &binary.left))?;

    let symbol = scopes.lookup_value_at(scope, &identifier.name, identifier.span.start)?;
    if symbol.kind != super::super::scopes::SymbolKind::Local
        || symbol.type_name.as_deref() != Some("string")
    {
        return None;
    }
    let key = symbol.span.start;
    if !find_local_initializer(root_block, key).is_some_and(is_empty_string_expression) {
        return None;
    }
    // Loops and legacy jump control flow can revisit a condition after a later
    // write. This deliberately keeps local initializer facts out of bodies
    // where source order alone cannot prove the value is stable.
    if block_contains_loop_or_jump(root_block)
        || block_has_prior_write(root_block, &symbol.name, expression.span.start)
    {
        return None;
    }

    Some(binary.operator == BinaryOperator::Equal)
}

fn empty_string_pair<'a>(
    candidate: &'a Expression,
    other: &'a Expression,
) -> Option<&'a crate::ast::Identifier> {
    let candidate = unwrap_parentheses(candidate);
    let other = unwrap_parentheses(other);
    let ExpressionKind::Identifier(identifier) = &candidate.kind else {
        return None;
    };
    let ExpressionKind::String(literal) = &other.kind else {
        return None;
    };
    is_empty_string_literal(&literal.text).then_some(identifier)
}

fn unwrap_parentheses(mut expression: &Expression) -> &Expression {
    while let ExpressionKind::Parenthesized(inner) = &expression.kind {
        expression = inner;
    }
    expression
}

fn is_empty_string_literal(text: &str) -> bool {
    text == r#""""#
}

fn is_empty_string_expression(expression: &Expression) -> bool {
    matches!(
        &unwrap_parentheses(expression).kind,
        ExpressionKind::String(literal) if is_empty_string_literal(&literal.text)
    )
}

fn find_local_initializer(block: &Block, key: usize) -> Option<&Expression> {
    for statement in &block.statements {
        if let Some(initializer) = find_local_initializer_in_statement(statement, key) {
            return Some(initializer);
        }
    }
    None
}

fn find_local_initializer_in_statement(statement: &Statement, key: usize) -> Option<&Expression> {
    match &statement.kind {
        StatementKind::Block(block) => find_local_initializer(block, key),
        StatementKind::Local(local) => local
            .declarators
            .iter()
            .find(|declarator| declarator.name_span.start == key)
            .and_then(|declarator| declarator.initializer.as_ref()),
        StatementKind::If(if_statement) => {
            find_local_initializer_in_statement(&if_statement.then_branch, key).or_else(|| {
                if_statement
                    .else_branch
                    .as_deref()
                    .and_then(|branch| find_local_initializer_in_statement(branch, key))
            })
        }
        StatementKind::For(for_statement) => {
            find_local_initializer_in_statement(&for_statement.body, key)
        }
        StatementKind::While(while_statement) => {
            find_local_initializer_in_statement(&while_statement.body, key)
        }
        StatementKind::DoWhile(do_while_statement) => {
            find_local_initializer_in_statement(&do_while_statement.body, key)
        }
        StatementKind::Switch(switch_statement) => switch_statement
            .arms
            .iter()
            .flat_map(|arm| arm.statements.iter())
            .find_map(|nested| find_local_initializer_in_statement(nested, key)),
        _ => None,
    }
}

fn block_contains_loop_or_jump(block: &Block) -> bool {
    block.statements.iter().any(statement_contains_loop_or_jump)
}

fn statement_contains_loop_or_jump(statement: &Statement) -> bool {
    match &statement.kind {
        StatementKind::Block(block) => block_contains_loop_or_jump(block),
        StatementKind::If(if_statement) => {
            statement_contains_loop_or_jump(&if_statement.then_branch)
                || if_statement
                    .else_branch
                    .as_deref()
                    .is_some_and(statement_contains_loop_or_jump)
        }
        StatementKind::For(_)
        | StatementKind::While(_)
        | StatementKind::DoWhile(_)
        | StatementKind::Jump(_) => true,
        StatementKind::Switch(switch_statement) => switch_statement
            .arms
            .iter()
            .flat_map(|arm| arm.statements.iter())
            .any(statement_contains_loop_or_jump),
        _ => false,
    }
}

fn block_has_prior_write(block: &Block, name: &str, before: usize) -> bool {
    block
        .statements
        .iter()
        .any(|statement| statement_has_prior_write(statement, name, before))
}

fn statement_has_prior_write(statement: &Statement, name: &str, before: usize) -> bool {
    if statement.span.start >= before {
        return false;
    }
    match &statement.kind {
        StatementKind::Block(block) => block_has_prior_write(block, name, before),
        StatementKind::Local(local) => local.declarators.iter().any(|declarator| {
            declarator
                .initializer
                .as_ref()
                .is_some_and(|expr| expression_has_prior_write(expr, name, before))
        }),
        StatementKind::If(if_statement) => {
            expression_has_prior_write(&if_statement.condition, name, before)
                || statement_has_prior_write(&if_statement.then_branch, name, before)
                || if_statement
                    .else_branch
                    .as_deref()
                    .is_some_and(|branch| statement_has_prior_write(branch, name, before))
        }
        StatementKind::For(for_statement) => {
            for_statement
                .initializer
                .iter()
                .chain(std::iter::once(&for_statement.condition))
                .chain(for_statement.increment.iter())
                .any(|expr| expression_has_prior_write(expr, name, before))
                || statement_has_prior_write(&for_statement.body, name, before)
        }
        StatementKind::While(while_statement) => {
            expression_has_prior_write(&while_statement.condition, name, before)
                || statement_has_prior_write(&while_statement.body, name, before)
        }
        StatementKind::DoWhile(do_while_statement) => {
            statement_has_prior_write(&do_while_statement.body, name, before)
                || expression_has_prior_write(&do_while_statement.condition, name, before)
        }
        StatementKind::Return(return_statement) => return_statement
            .value
            .as_ref()
            .is_some_and(|expr| expression_has_prior_write(expr, name, before)),
        StatementKind::Expression(expression_statement) => {
            expression_has_prior_write(&expression_statement.expression, name, before)
        }
        StatementKind::Switch(switch_statement) => {
            expression_has_prior_write(&switch_statement.condition, name, before)
                || switch_statement.arms.iter().any(|arm| {
                    matches!(
                        &arm.label,
                        crate::ast::SwitchLabel::Case(expression)
                            if expression_has_prior_write(expression, name, before)
                    ) || arm
                        .statements
                        .iter()
                        .any(|nested| statement_has_prior_write(nested, name, before))
                })
        }
        _ => false,
    }
}

fn expression_has_prior_write(expression: &Expression, name: &str, before: usize) -> bool {
    if expression.span.start >= before {
        return false;
    }
    match &expression.kind {
        ExpressionKind::Assignment(assignment) => {
            matches!(
                &assignment.target.kind,
                ExpressionKind::Identifier(identifier) if identifier.name == name
            ) || expression_has_prior_write(&assignment.target, name, before)
                || expression_has_prior_write(&assignment.value, name, before)
        }
        ExpressionKind::Increment(increment) => {
            matches!(
                &increment.operand.kind,
                ExpressionKind::Identifier(identifier) if identifier.name == name
            ) || expression_has_prior_write(&increment.operand, name, before)
        }
        ExpressionKind::Parenthesized(inner)
        | ExpressionKind::Cast(crate::ast::CastExpression { operand: inner, .. })
        | ExpressionKind::Unary(crate::ast::UnaryExpression { operand: inner, .. }) => {
            expression_has_prior_write(inner, name, before)
        }
        ExpressionKind::Binary(binary) => {
            expression_has_prior_write(&binary.left, name, before)
                || expression_has_prior_write(&binary.right, name, before)
        }
        ExpressionKind::Call(call) => {
            expression_has_prior_write(&call.callee, name, before)
                || call
                    .arguments
                    .iter()
                    .any(|argument| expression_has_prior_write(argument, name, before))
        }
        ExpressionKind::Component(component) => {
            expression_has_prior_write(&component.base, name, before)
        }
        ExpressionKind::LazyIndex(lazy) => {
            expression_has_prior_write(&lazy.base, name, before)
                || lazy
                    .indices
                    .iter()
                    .any(|index| expression_has_prior_write(index, name, before))
        }
        ExpressionKind::List(list) => list
            .elements
            .iter()
            .any(|element| expression_has_prior_write(element, name, before)),
        ExpressionKind::Vector(vector) => vector
            .components
            .iter()
            .any(|component| expression_has_prior_write(component, name, before)),
        ExpressionKind::Rotation(rotation) => rotation
            .components
            .iter()
            .any(|component| expression_has_prior_write(component, name, before)),
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Error(_) => false,
    }
}

fn immutable_global_integer_facts(ast: &Ast, scopes: &ScopeTree) -> HashMap<usize, i32> {
    let mut facts = HashMap::new();
    for item in &ast.items {
        let TopLevel::Global(global) = item else {
            continue;
        };
        if global.ty.kind != TypeKind::Integer {
            continue;
        }
        for declarator in &global.declarators {
            let Some(initializer) = declarator.initializer.as_ref() else {
                continue;
            };
            let Some(symbol) =
                scopes.lookup_value_at(scopes.root(), &declarator.name, declarator.name_span.start)
            else {
                continue;
            };
            if symbol.kind != super::super::scopes::SymbolKind::GlobalVariable
                || ast_has_write_to_name(ast, &declarator.name)
            {
                continue;
            }
            if let Some(value) = flow::evaluate_static_integer(initializer, scopes.root(), scopes) {
                facts.insert(symbol.span.start, value);
            }
        }
    }
    facts
}

fn ast_has_write_to_name(ast: &Ast, name: &str) -> bool {
    ast.items.iter().any(|item| match item {
        TopLevel::Global(global) => global.declarators.iter().any(|declarator| {
            declarator
                .initializer
                .as_ref()
                .is_some_and(|expr| expression_writes_name(expr, name))
        }),
        TopLevel::Function(function) => function
            .body
            .as_ref()
            .is_some_and(|body| block_writes_name(body, name)),
        TopLevel::State(state) => state.events.iter().any(|event| {
            event
                .body
                .as_ref()
                .is_some_and(|body| block_writes_name(body, name))
        }),
        TopLevel::Error(_) => false,
    })
}

fn block_writes_name(block: &Block, name: &str) -> bool {
    block
        .statements
        .iter()
        .any(|statement| statement_writes_name(statement, name))
}

fn statement_writes_name(statement: &Statement, name: &str) -> bool {
    match &statement.kind {
        StatementKind::Block(block) => block_writes_name(block, name),
        StatementKind::Local(local) => local.declarators.iter().any(|declarator| {
            declarator
                .initializer
                .as_ref()
                .is_some_and(|expr| expression_writes_name(expr, name))
        }),
        StatementKind::If(if_statement) => {
            expression_writes_name(&if_statement.condition, name)
                || statement_writes_name(&if_statement.then_branch, name)
                || if_statement
                    .else_branch
                    .as_deref()
                    .is_some_and(|branch| statement_writes_name(branch, name))
        }
        StatementKind::For(for_statement) => {
            for_statement
                .initializer
                .iter()
                .chain(std::iter::once(&for_statement.condition))
                .chain(for_statement.increment.iter())
                .any(|expr| expression_writes_name(expr, name))
                || statement_writes_name(&for_statement.body, name)
        }
        StatementKind::While(while_statement) => {
            expression_writes_name(&while_statement.condition, name)
                || statement_writes_name(&while_statement.body, name)
        }
        StatementKind::DoWhile(do_while_statement) => {
            statement_writes_name(&do_while_statement.body, name)
                || expression_writes_name(&do_while_statement.condition, name)
        }
        StatementKind::Return(return_statement) => return_statement
            .value
            .as_ref()
            .is_some_and(|expr| expression_writes_name(expr, name)),
        StatementKind::Expression(expression_statement) => {
            expression_writes_name(&expression_statement.expression, name)
        }
        StatementKind::Switch(switch_statement) => {
            expression_writes_name(&switch_statement.condition, name)
                || switch_statement.arms.iter().any(|arm| {
                    matches!(
                        &arm.label,
                        crate::ast::SwitchLabel::Case(expression)
                            if expression_writes_name(expression, name)
                    ) || arm
                        .statements
                        .iter()
                        .any(|nested| statement_writes_name(nested, name))
                })
        }
        _ => false,
    }
}

fn expression_writes_name(expression: &Expression, name: &str) -> bool {
    match &expression.kind {
        ExpressionKind::Assignment(assignment) => {
            matches!(
                &assignment.target.kind,
                ExpressionKind::Identifier(identifier) if identifier.name == name
            ) || expression_writes_name(&assignment.target, name)
                || expression_writes_name(&assignment.value, name)
        }
        ExpressionKind::Increment(increment) => {
            matches!(
                &increment.operand.kind,
                ExpressionKind::Identifier(identifier) if identifier.name == name
            ) || expression_writes_name(&increment.operand, name)
        }
        ExpressionKind::Parenthesized(inner)
        | ExpressionKind::Cast(crate::ast::CastExpression { operand: inner, .. })
        | ExpressionKind::Unary(crate::ast::UnaryExpression { operand: inner, .. }) => {
            expression_writes_name(inner, name)
        }
        ExpressionKind::Binary(binary) => {
            expression_writes_name(&binary.left, name)
                || expression_writes_name(&binary.right, name)
        }
        ExpressionKind::Call(call) => {
            expression_writes_name(&call.callee, name)
                || call
                    .arguments
                    .iter()
                    .any(|argument| expression_writes_name(argument, name))
        }
        ExpressionKind::Component(component) => expression_writes_name(&component.base, name),
        ExpressionKind::LazyIndex(lazy) => {
            expression_writes_name(&lazy.base, name)
                || lazy
                    .indices
                    .iter()
                    .any(|index| expression_writes_name(index, name))
        }
        ExpressionKind::List(list) => list
            .elements
            .iter()
            .any(|element| expression_writes_name(element, name)),
        ExpressionKind::Vector(vector) => vector
            .components
            .iter()
            .any(|component| expression_writes_name(component, name)),
        ExpressionKind::Rotation(rotation) => rotation
            .components
            .iter()
            .any(|component| expression_writes_name(component, name)),
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Error(_) => false,
    }
}

fn same_state(b: &Block, n: &str, d: bool, out: &mut Vec<Diagnostic>) {
    for s in &b.statements {
        match &s.kind {
            StatementKind::StateChange(c)
                if (d && c.state == "default") || (!d && c.state == n) =>
            {
                out.push(diag(
                    "SEM053",
                    Severity::Warning,
                    "state change targets the current state".into(),
                    c.state_span,
                ))
            }
            StatementKind::Block(x) => same_state(x, n, d, out),
            StatementKind::If(i) => {
                same_state_stmt(&i.then_branch, n, d, out);
                if let Some(e) = &i.else_branch {
                    same_state_stmt(e, n, d, out)
                }
            }
            StatementKind::While(w) => same_state_stmt(&w.body, n, d, out),
            StatementKind::For(f) => same_state_stmt(&f.body, n, d, out),
            StatementKind::DoWhile(w) => same_state_stmt(&w.body, n, d, out),
            StatementKind::Switch(sw) => {
                for arm in &sw.arms {
                    for statement in &arm.statements {
                        same_state_stmt(statement, n, d, out);
                    }
                }
            }
            _ => {}
        }
    }
}
fn same_state_stmt(s: &Statement, n: &str, d: bool, out: &mut Vec<Diagnostic>) {
    let b = Block {
        span: s.span,
        closed: true,
        statements: vec![s.clone()],
    };
    same_state(&b, n, d, out)
}
fn bad(a: usize, b: usize, ds: &[Diagnostic]) -> bool {
    ds.iter()
        .any(|d| d.severity == Severity::Error && d.span.start >= a && d.span.start <= b)
}
fn diag(
    code: &'static str,
    severity: Severity,
    message: String,
    span: crate::diagnostic::Span,
) -> Diagnostic {
    Diagnostic {
        code,
        severity,
        kind: if severity == Severity::Error {
            DiagnosticKind::Semantic
        } else {
            DiagnosticKind::Lint
        },
        message,
        span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parse_ast;
    use crate::lex;
    fn run(src: &str, target: &str, god: bool) -> Vec<Diagnostic> {
        let (t, l) = lex(src);
        assert!(l.is_empty(), "{l:?}");
        let (a, s) = parse_ast(&t);
        let profile = TargetProfile::parse(target).unwrap();
        crate::ast_semantic::analyze_ast_with_target_options_and_god_mode(
            &a,
            &s,
            true,
            profile,
            &crate::target::OsslAllowlist::default(),
            god,
        )
    }
    fn has(ds: &[Diagnostic], code: &str) -> bool {
        ds.iter().any(|d| d.code == code)
    }
    #[test]
    fn warns_for_constant_conditions_but_not_mutable_variables() {
        let d = run(
            "default { state_entry() { if (1 == 1) ; while (0) ; integer x; if (x) ; } }",
            "second-life-mono",
            false,
        );
        assert!(has(&d, "SEM056"));
        assert_eq!(d.iter().filter(|x| x.code == "SEM056").count(), 2)
    }
    #[test]
    fn constant_string_assignments_are_truthy_or_falsey() {
        let diagnostics = run(
            r#"default { state_entry() {
                string value;
                if (value = "ALLOWHIVE") ;
                if (value = "") ;
            } }"#,
            "second-life-mono",
            false,
        );
        let conditions: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM056")
            .collect();
        assert_eq!(conditions.len(), 2, "{diagnostics:?}");
        assert!(conditions[0].message.contains("always true"));
        assert!(conditions[1].message.contains("always false"));
    }

    #[test]
    fn unresolved_or_type_incompatible_assignment_is_not_folded() {
        for source in [
            r#"default { state_entry() { if (missing = "value") ; } }"#,
            r#"default { state_entry() { integer value; if (value = "text") ; } }"#,
        ] {
            let diagnostics = run(source, "second-life-mono", false);
            assert!(
                diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.code != "SEM056"),
                "{source}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn unchanged_empty_local_string_comparison_is_constant() {
        let diagnostics = run(
            r#"default { state_entry() {
                string result = "";
                if (result != "") ;
            } }"#,
            "second-life-mono",
            false,
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "SEM056"
                    && diagnostic.message.contains("always false")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn local_string_write_invalidates_empty_string_fact() {
        let diagnostics = run(
            r#"default { state_entry() {
                string result = "";
                result = "ready";
                if (result != "") ;
            } }"#,
            "second-life-mono",
            false,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "SEM056"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn immutable_global_integer_initializer_is_propagated() {
        let diagnostics = run(
            r#"integer menuOperateLock = FALSE;
            showMenu() { if (menuOperateLock == TRUE) ; }
            default { state_entry() {} }"#,
            "second-life-mono",
            false,
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "SEM056"
                    && diagnostic.message.contains("always false")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn local_string_fact_is_not_carried_through_a_loop_back_edge() {
        let diagnostics = run(
            r#"default { state_entry() {
                string result = "";
                while (llGetTime() > 0.0) {
                    if (result == "") ;
                    result = "ready";
                }
            } }"#,
            "second-life-mono",
            false,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "SEM056"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn global_integer_write_invalidates_initializer_fact() {
        let diagnostics = run(
            r#"integer menuOperateLock = FALSE;
            setMenu() { menuOperateLock = TRUE; }
            showMenu() { if (menuOperateLock == TRUE) ; }
            default { state_entry() {} }"#,
            "second-life-mono",
            false,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "SEM056"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn warns_for_empty_if() {
        let d = run(
            "default { state_entry() { if (llGetTime() > 0.0) ; } }",
            "second-life-mono",
            false,
        );
        assert!(has(&d, "SEM055"))
    }
    #[test]
    fn warns_for_transition_to_current_state_only() {
        let d=run("default { state_entry() { state default; } } state other { state_entry() { state other; } }","second-life-mono",false);
        assert_eq!(d.iter().filter(|x| x.code == "SEM053").count(), 2)
    }
    #[test]
    fn print_warning_is_second_life_only() {
        assert!(has(
            &run(
                "default { state_entry() { print(\"x\"); } }",
                "second-life-mono",
                false
            ),
            "SEM057"
        ));
        assert!(!has(
            &run(
                "default { state_entry() { print(\"x\"); } }",
                "opensim-yengine",
                false
            ),
            "SEM057"
        ))
    }
    #[test]
    fn god_mode_calls_require_explicit_opt_in() {
        let src = "default { state_entry() { llGodLikeRezObject(NULL_KEY,ZERO_VECTOR); } }";
        assert!(has(&run(src, "second-life-mono", false), "SEM058"));
        assert!(!has(&run(src, "second-life-mono", true), "SEM058"));
        assert!(!has(&run(src, "opensim-yengine", false), "SEM058"))
    }
}
