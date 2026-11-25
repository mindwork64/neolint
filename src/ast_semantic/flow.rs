//! AST-native SEM008-SEM011 flow analysis.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    Ast, BinaryOperator, Block, Expression, ExpressionKind, ForStatement, IfStatement,
    ReturnStatement, Statement, StatementKind, TopLevel, UnaryOperator,
};
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

use super::rules::semantic_error;
use super::scopes::{ScopeId, ScopeTree};
use super::types::{
    can_implicitly_convert, expression_first_token_span, infer_expression_type,
    UserFunctionSignature,
};

const MAX_CFG_NODES: usize = 4_096;
const MAX_CFG_EDGES: usize = 8_192;
const MAX_CFG_DEPTH: usize = 128;
const MAX_CFG_WORK: usize = 16_384;
const MAX_CONSTANT_NODES: usize = 128;
const MAX_CONSTANT_DEPTH: usize = 64;

/// Per-function jump diagnostics plus the function bodies invalidated for SEM011.
pub(crate) struct JumpReport {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) invalid_functions: HashSet<(usize, usize)>,
}

struct LabelSite<'a> {
    name: &'a str,
    span: Span,
    scope: ScopeId,
    recovery_scope_path: Vec<Span>,
}

struct JumpSite<'a> {
    name: &'a str,
    span: Span,
    scope: ScopeId,
    recovery_scope_path: Vec<Span>,
}

struct BodyFacts<'a> {
    labels: Vec<LabelSite<'a>>,
    jumps: Vec<JumpSite<'a>>,
    returns: Vec<(&'a ReturnStatement, ScopeId)>,
    has_error_statement: bool,
}

struct CodeScope<'a> {
    name: &'a str,
    body: &'a Block,
    scope: ScopeId,
    is_function: bool,
}

pub(crate) fn check_returns(
    ast: &Ast,
    scopes: &ScopeTree,
    functions: &HashMap<String, UserFunctionSignature>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for item in &ast.items {
        let TopLevel::Function(function) = item else {
            continue;
        };
        let Some(body) = &function.body else {
            continue;
        };
        if !body.closed || function.name.is_empty() {
            continue;
        }
        let Some(signature) = functions.get(function.name.as_str()) else {
            continue;
        };
        let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
        let facts = walk_body(body, scope, scopes);
        for (statement, return_scope) in facts.returns {
            let Some(value) = &statement.value else {
                if signature.return_type != "void" {
                    diagnostics.push(semantic_error(
                        "SEM008",
                        format!(
                            "function {} returns {}, but this return statement has no value",
                            function.name, signature.return_type
                        ),
                        statement.keyword_span,
                    ));
                }
                continue;
            };
            if signature.return_type == "void" {
                diagnostics.push(semantic_error(
                    "SEM008",
                    format!(
                        "function {} does not return a value, but this return statement has one",
                        function.name
                    ),
                    expression_first_token_span(value),
                ));
                continue;
            }
            if let Some(actual_type) = infer_expression_type(value, scopes, return_scope, functions)
            {
                if !can_implicitly_convert(&actual_type, &signature.return_type) {
                    diagnostics.push(semantic_error(
                        "SEM008",
                        format!(
                            "function {} returns {}, but this return expression has type {}",
                            function.name, signature.return_type, actual_type
                        ),
                        expression_first_token_span(value),
                    ));
                }
            }
        }
    }
    diagnostics
}

/// SEM009/SEM010 for functions and canonical event handlers.
pub(crate) fn check_jumps(ast: &Ast, scopes: &ScopeTree) -> JumpReport {
    let mut report = JumpReport {
        diagnostics: Vec::new(),
        invalid_functions: HashSet::new(),
    };
    for code_scope in collect_code_scopes(ast, scopes) {
        let facts = walk_body(code_scope.body, code_scope.scope, scopes);
        let before = report.diagnostics.len();
        let mut labels: HashMap<&str, (Span, ScopeId, Vec<Span>)> = HashMap::new();
        for label in facts.labels {
            if let Some((previous, _, _)) = labels.get(label.name) {
                report.diagnostics.push(semantic_error(
                    "SEM009",
                    format!(
                        "duplicate jump label '{}' in {}; first declared at {}:{}",
                        label.name, code_scope.name, previous.line, previous.column
                    ),
                    label.span,
                ));
            } else {
                labels.insert(
                    label.name,
                    (label.span, label.scope, label.recovery_scope_path),
                );
            }
        }
        for jump in facts.jumps {
            let Some((_, target_scope, target_recovery_path)) = labels.get(jump.name) else {
                report.diagnostics.push(semantic_error(
                    "SEM010",
                    format!(
                        "jump target '{}' is not defined in {}",
                        jump.name, code_scope.name
                    ),
                    jump.span,
                ));
                continue;
            };
            if !label_scope_is_accessible(
                *target_scope,
                target_recovery_path,
                jump.scope,
                &jump.recovery_scope_path,
                scopes,
            ) {
                report.diagnostics.push(semantic_error(
                    "SEM010",
                    format!(
                        "jump target '{}' is inside a nested block and is not accessible from this jump in {}",
                        jump.name, code_scope.name
                    ),
                    jump.span,
                ));
            }
        }
        if code_scope.is_function && report.diagnostics.len() != before {
            report
                .invalid_functions
                .insert((code_scope.body.span.start, code_scope.body.span.end));
        }
    }
    report
}

/// SEM011 for value-returning functions.
pub(crate) fn check_missing_returns(
    ast: &Ast,
    scopes: &ScopeTree,
    invalid_functions: &HashSet<(usize, usize)>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for item in &ast.items {
        let TopLevel::Function(function) = item else {
            continue;
        };
        if function.return_type.is_none() || function.name.is_empty() {
            continue;
        }
        let Some(body) = &function.body else {
            continue;
        };
        if !body.closed || invalid_functions.contains(&(body.span.start, body.span.end)) {
            continue;
        }
        let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
        let facts = walk_body(body, scope, scopes);
        if facts.has_error_statement {
            continue;
        }
        let Some(reaches_end) = analyze_function_body(body, scope, scopes) else {
            continue;
        };
        if reaches_end {
            diagnostics.push(semantic_error(
                "SEM011",
                format!(
                    "function {} may reach the end without returning a value",
                    function.name
                ),
                function.name_span,
            ));
        }
    }
    diagnostics
}

fn label_scope_is_accessible(
    target_scope: ScopeId,
    target_recovery_path: &[Span],
    jump_scope: ScopeId,
    jump_recovery_path: &[Span],
    scopes: &ScopeTree,
) -> bool {
    if target_recovery_path.is_empty() {
        return scopes.is_ancestor(target_scope, jump_scope);
    }

    target_scope == jump_scope && jump_recovery_path.starts_with(target_recovery_path)
}

fn collect_code_scopes<'a>(ast: &'a Ast, scopes: &ScopeTree) -> Vec<CodeScope<'a>> {
    let mut code_scopes = Vec::new();
    for item in &ast.items {
        let TopLevel::Function(function) = item else {
            continue;
        };
        let Some(body) = &function.body else {
            continue;
        };
        if !body.closed || function.name.is_empty() {
            continue;
        }
        let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
        code_scopes.push(CodeScope {
            name: &function.name,
            body,
            scope,
            is_function: true,
        });
    }
    let events = &builtins::definitions().events;
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.body_missing || !state.body_closed {
            continue;
        }
        for event in &state.events {
            let Some(body) = &event.body else {
                continue;
            };
            if !body.closed || event.name.is_empty() || !events.contains_key(event.name.as_str()) {
                continue;
            }
            let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
            code_scopes.push(CodeScope {
                name: &event.name,
                body,
                scope,
                is_function: false,
            });
        }
    }
    code_scopes
}

fn walk_body<'a>(body: &'a Block, scope: ScopeId, scopes: &ScopeTree) -> BodyFacts<'a> {
    let mut facts = BodyFacts {
        labels: Vec::new(),
        jumps: Vec::new(),
        returns: Vec::new(),
        has_error_statement: false,
    };
    let mut work: Vec<(&Statement, ScopeId)> = body
        .statements
        .iter()
        .rev()
        .map(|statement| (statement, scope))
        .collect();
    while let Some((statement, current)) = work.pop() {
        match &statement.kind {
            StatementKind::Block(block) => {
                let child = scopes.scope_for_block(block).unwrap_or(current);
                for statement in block.statements.iter().rev() {
                    work.push((statement, child));
                }
            }
            StatementKind::Return(return_statement) => {
                facts.returns.push((return_statement, current));
                for recovered in &return_statement.recovered_labels {
                    let label = &recovered.label;
                    if !label.name.is_empty() {
                        facts.labels.push(LabelSite {
                            name: &label.name,
                            span: label.name_span,
                            scope: current,
                            recovery_scope_path: recovered.scope_path.clone(),
                        });
                    }
                }
                for recovered in &return_statement.recovered_jumps {
                    let jump = &recovered.jump;
                    if !jump.label.is_empty() {
                        facts.jumps.push(JumpSite {
                            name: &jump.label,
                            span: jump.label_span,
                            scope: current,
                            recovery_scope_path: recovered.scope_path.clone(),
                        });
                    }
                }
            }
            StatementKind::Jump(jump) => {
                if !jump.label.is_empty() {
                    facts.jumps.push(JumpSite {
                        name: &jump.label,
                        span: jump.label_span,
                        scope: current,
                        recovery_scope_path: Vec::new(),
                    });
                }
            }
            StatementKind::Label(label) => {
                if !label.name.is_empty() {
                    facts.labels.push(LabelSite {
                        name: &label.name,
                        span: label.name_span,
                        scope: current,
                        recovery_scope_path: Vec::new(),
                    });
                }
            }
            StatementKind::If(if_statement) => {
                if let Some(else_branch) = &if_statement.else_branch {
                    work.push((else_branch, current));
                }
                work.push((&if_statement.then_branch, current));
            }
            StatementKind::For(for_statement) => {
                work.push((&for_statement.body, current));
            }
            StatementKind::While(while_statement) => {
                work.push((&while_statement.body, current));
            }
            StatementKind::DoWhile(do_while) => {
                work.push((&do_while.body, current));
            }
            StatementKind::Switch(switch) => {
                let switch_scope = scopes.scope_for_switch(switch).unwrap_or(current);
                for arm in switch.arms.iter().rev() {
                    for statement in arm.statements.iter().rev() {
                        work.push((statement, switch_scope));
                    }
                }
            }
            StatementKind::Break(_) => {}
            StatementKind::Error(_) => facts.has_error_statement = true,
            StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::StateChange(_)
            | StatementKind::Expression(_) => {}
        }
    }
    facts
}

struct FlowGraph {
    successors: Vec<Vec<usize>>,
    labels: HashMap<String, usize>,
    pending_jumps: Vec<(usize, String)>,
    edge_count: usize,
    work: usize,
    failed: bool,
}

impl FlowGraph {
    fn node(&mut self) -> Option<usize> {
        self.work = self.work.saturating_add(1);
        if self.work > MAX_CFG_WORK || self.successors.len() >= MAX_CFG_NODES {
            self.failed = true;
            return None;
        }
        let id = self.successors.len();
        self.successors.push(Vec::new());
        Some(id)
    }

    fn edge(&mut self, from: usize, to: usize) -> bool {
        let Some(edges) = self.successors.get_mut(from) else {
            self.failed = true;
            return false;
        };
        if edges.contains(&to) {
            return true;
        }
        if self.edge_count >= MAX_CFG_EDGES {
            self.failed = true;
            return false;
        }
        self.edge_count += 1;
        edges.push(to);
        true
    }

    fn can_reach(&self, start: usize, target: usize) -> bool {
        let mut visited = vec![false; self.successors.len()];
        let mut pending = vec![start];
        while let Some(node) = pending.pop() {
            if node == target {
                return true;
            }
            if node >= visited.len() || visited[node] {
                continue;
            }
            visited[node] = true;
            if let Some(edges) = self.successors.get(node) {
                pending.extend(edges.iter().copied());
            }
        }
        false
    }
}

struct Fragment {
    entry: usize,
    exits: Vec<usize>,
}

fn analyze_function_body(body: &Block, scope: ScopeId, scopes: &ScopeTree) -> Option<bool> {
    let mut graph = FlowGraph {
        successors: Vec::new(),
        labels: HashMap::new(),
        pending_jumps: Vec::new(),
        edge_count: 0,
        work: 0,
        failed: false,
    };
    let fragment = build_statements(&body.statements, scope, scopes, &mut graph, None, 0)?;
    let exit = graph.node()?;
    if !connect_exits(&mut graph, &fragment.exits, exit) {
        return None;
    }
    for (jump_node, target) in std::mem::take(&mut graph.pending_jumps) {
        let target_node = graph.labels.get(&target).copied()?;
        if !graph.edge(jump_node, target_node) {
            return None;
        }
    }
    if graph.failed {
        return None;
    }
    Some(graph.can_reach(fragment.entry, exit))
}

fn build_statements(
    statements: &[Statement],
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
) -> Option<Fragment> {
    if depth > MAX_CFG_DEPTH {
        graph.failed = true;
        return None;
    }
    if statements.is_empty() {
        let node = graph.node()?;
        return Some(Fragment {
            entry: node,
            exits: vec![node],
        });
    }
    let mut entry = None;
    let mut exits = Vec::new();
    for statement in statements {
        let fragment = build_statement(statement, scope, scopes, graph, break_target, depth + 1)?;
        if entry.is_some() {
            if !connect_exits(graph, &exits, fragment.entry) {
                return None;
            }
        } else {
            entry = Some(fragment.entry);
        }
        exits = fragment.exits;
    }
    Some(Fragment {
        entry: entry?,
        exits,
    })
}

fn build_statement(
    statement: &Statement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
) -> Option<Fragment> {
    if depth > MAX_CFG_DEPTH {
        graph.failed = true;
        return None;
    }
    graph.work = graph.work.saturating_add(1);
    if graph.work > MAX_CFG_WORK {
        graph.failed = true;
        return None;
    }
    match &statement.kind {
        StatementKind::Block(block) => {
            let child = scopes.scope_for_block(block).unwrap_or(scope);
            build_statements(&block.statements, child, scopes, graph, break_target, depth)
        }
        StatementKind::If(if_statement) => {
            build_if(if_statement, scope, scopes, graph, break_target, depth)
        }
        StatementKind::For(for_statement) => {
            build_for(for_statement, scope, scopes, graph, break_target, depth)
        }
        StatementKind::While(while_statement) => {
            build_while(while_statement, scope, scopes, graph, break_target, depth)
        }
        StatementKind::DoWhile(do_while) => {
            let body = build_statement(
                &do_while.body,
                scope,
                scopes,
                graph,
                break_target,
                depth + 1,
            )?;
            let condition = graph.node()?;
            if !connect_exits(graph, &body.exits, condition) {
                return None;
            }
            let (may_repeat, may_exit) =
                condition_possibilities(&do_while.condition, false, scope, scopes)?;
            if may_repeat && !graph.edge(condition, body.entry) {
                return None;
            }
            Some(Fragment {
                entry: body.entry,
                exits: if may_exit {
                    vec![condition]
                } else {
                    Vec::new()
                },
            })
        }
        StatementKind::Return(_) => {
            let node = graph.node()?;
            Some(Fragment {
                entry: node,
                exits: Vec::new(),
            })
        }
        StatementKind::Jump(jump) => {
            let node = graph.node()?;
            graph.pending_jumps.push((node, jump.label.clone()));
            Some(Fragment {
                entry: node,
                exits: Vec::new(),
            })
        }
        StatementKind::Label(label) => {
            let node = graph.node()?;
            graph.labels.entry(label.name.clone()).or_insert(node);
            Some(Fragment {
                entry: node,
                exits: vec![node],
            })
        }
        StatementKind::Switch(switch) => build_switch(switch, scope, scopes, graph, depth),
        StatementKind::Break(_) => {
            let node = graph.node()?;
            let Some(target) = break_target else {
                graph.failed = true;
                return None;
            };
            if !graph.edge(node, target) {
                return None;
            }
            Some(Fragment {
                entry: node,
                exits: Vec::new(),
            })
        }
        StatementKind::Error(_) => {
            graph.failed = true;
            None
        }
        StatementKind::Empty
        | StatementKind::Local(_)
        | StatementKind::StateChange(_)
        | StatementKind::Expression(_) => {
            let node = graph.node()?;
            Some(Fragment {
                entry: node,
                exits: vec![node],
            })
        }
    }
}

fn build_if(
    if_statement: &IfStatement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
) -> Option<Fragment> {
    let condition = graph.node()?;
    let (may_true, may_false) =
        condition_possibilities(&if_statement.condition, false, scope, scopes)?;
    let then_branch = build_statement(
        &if_statement.then_branch,
        scope,
        scopes,
        graph,
        break_target,
        depth + 1,
    )?;
    let mut exits = Vec::new();
    if may_true {
        if !graph.edge(condition, then_branch.entry) {
            return None;
        }
        exits.extend(then_branch.exits);
    }
    if let Some(else_branch) = &if_statement.else_branch {
        let else_fragment =
            build_statement(else_branch, scope, scopes, graph, break_target, depth + 1)?;
        if may_false {
            if !graph.edge(condition, else_fragment.entry) {
                return None;
            }
            exits.extend(else_fragment.exits);
        }
    } else if may_false {
        exits.push(condition);
    }
    Some(Fragment {
        entry: condition,
        exits,
    })
}

fn build_for(
    for_statement: &ForStatement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
) -> Option<Fragment> {
    let condition = graph.node()?;
    let (may_enter, may_exit) = if for_statement.condition_missing {
        (true, false)
    } else if for_statement.condition_malformed {
        (true, true)
    } else {
        condition_possibilities(&for_statement.condition, false, scope, scopes)?
    };
    let body = build_statement(
        &for_statement.body,
        scope,
        scopes,
        graph,
        break_target,
        depth + 1,
    )?;
    if may_enter && !graph.edge(condition, body.entry) {
        return None;
    }
    if !connect_exits(graph, &body.exits, condition) {
        return None;
    }
    Some(Fragment {
        entry: condition,
        exits: if may_exit {
            vec![condition]
        } else {
            Vec::new()
        },
    })
}

fn build_while(
    while_statement: &crate::ast::WhileStatement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
) -> Option<Fragment> {
    let condition = graph.node()?;
    let (may_enter, may_exit) =
        condition_possibilities(&while_statement.condition, false, scope, scopes)?;
    let body = build_statement(
        &while_statement.body,
        scope,
        scopes,
        graph,
        break_target,
        depth + 1,
    )?;
    if may_enter && !graph.edge(condition, body.entry) {
        return None;
    }
    if !connect_exits(graph, &body.exits, condition) {
        return None;
    }
    Some(Fragment {
        entry: condition,
        exits: if may_exit {
            vec![condition]
        } else {
            Vec::new()
        },
    })
}

fn build_switch(
    switch: &crate::ast::SwitchStatement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    depth: usize,
) -> Option<Fragment> {
    if depth > MAX_CFG_DEPTH {
        graph.failed = true;
        return None;
    }
    // A switch owns its break target. Breaks in nested blocks and control-flow
    // bodies resolve here; a nested switch shadows this target with its own.
    let switch_exit = graph.node()?;
    let switch_scope = scopes.scope_for_switch(switch).unwrap_or(scope);
    let mut arms = Vec::with_capacity(switch.arms.len());
    let mut has_default = false;

    for arm in &switch.arms {
        if matches!(arm.label, crate::ast::SwitchLabel::Error(_)) {
            graph.failed = true;
            return None;
        }
        has_default |= matches!(arm.label, crate::ast::SwitchLabel::Default);
        arms.push(build_statements(
            &arm.statements,
            switch_scope,
            scopes,
            graph,
            Some(switch_exit),
            depth + 1,
        )?);
    }

    // Arms fall through in source order. Break statements already have a
    // direct edge to switch_exit and therefore do not appear among exits.
    for index in 0..arms.len() {
        let next = arms.get(index + 1).map_or(switch_exit, |arm| arm.entry);
        if !connect_exits(graph, &arms[index].exits, next) {
            return None;
        }
    }

    let dispatch = graph.node()?;
    for (arm, fragment) in switch.arms.iter().zip(&arms) {
        match arm.label {
            crate::ast::SwitchLabel::Case(_) | crate::ast::SwitchLabel::Default => {
                if !graph.edge(dispatch, fragment.entry) {
                    return None;
                }
            }
            crate::ast::SwitchLabel::Error(_) => {
                graph.failed = true;
                return None;
            }
        }
    }
    if !has_default && !graph.edge(dispatch, switch_exit) {
        return None;
    }

    // Keep the structural switch exit even when it is currently unreachable.
    // Deferred `jump` edges are resolved after the enclosing function is built
    // and may make the exit reachable later.
    Some(Fragment {
        entry: dispatch,
        exits: vec![switch_exit],
    })
}

fn connect_exits(graph: &mut FlowGraph, exits: &[usize], target: usize) -> bool {
    for exit in exits {
        if !graph.edge(*exit, target) {
            return false;
        }
    }
    true
}

fn condition_possibilities(
    expression: &Expression,
    empty_is_true: bool,
    scope: ScopeId,
    scopes: &ScopeTree,
) -> Option<(bool, bool)> {
    if empty_is_true {
        return Some((true, false));
    }
    if contains_error(expression) {
        return None;
    }
    let mut budget = MAX_CONSTANT_NODES;
    if let Some(value) = evaluate_integer(expression, scope, scopes, 0, &mut budget) {
        return Some((value != 0, value == 0));
    }
    let unwrapped = strip_parentheses(expression);
    match &unwrapped.kind {
        ExpressionKind::Float(literal) => {
            if let Ok(value) = literal.text.parse::<f64>() {
                Some((value != 0.0, value == 0.0))
            } else {
                Some((true, true))
            }
        }
        ExpressionKind::Identifier(identifier) => {
            if scopes
                .lookup_value_at(scope, &identifier.name, identifier.span.start)
                .is_some()
            {
                return Some((true, true));
            }
            let value = builtins::definitions()
                .constants
                .get(identifier.name.as_str())
                .and_then(|constant| constant.value.parse::<f64>().ok());
            match value {
                Some(value) => Some((value != 0.0, value == 0.0)),
                None => Some((true, true)),
            }
        }
        _ => Some((true, true)),
    }
}

fn strip_parentheses(mut expression: &Expression) -> &Expression {
    while let ExpressionKind::Parenthesized(inner) = &expression.kind {
        expression = inner;
    }
    expression
}

fn contains_error(expression: &Expression) -> bool {
    match &expression.kind {
        ExpressionKind::Error(_) => true,
        ExpressionKind::Parenthesized(inner) => contains_error(inner),
        ExpressionKind::Call(call) => {
            contains_error(&call.callee) || call.arguments.iter().any(contains_error)
        }
        ExpressionKind::Component(component) => contains_error(&component.base),
        ExpressionKind::Unary(unary) => contains_error(&unary.operand),
        ExpressionKind::Binary(binary) => {
            contains_error(&binary.left) || contains_error(&binary.right)
        }
        ExpressionKind::Assignment(assignment) => {
            contains_error(&assignment.target) || contains_error(&assignment.value)
        }
        ExpressionKind::Cast(cast) => contains_error(&cast.operand),
        ExpressionKind::Increment(increment) => contains_error(&increment.operand),
        ExpressionKind::LazyIndex(lazy) => {
            contains_error(&lazy.base) || lazy.indices.iter().any(contains_error)
        }
        ExpressionKind::List(list) => list.elements.iter().any(contains_error),
        ExpressionKind::Vector(vector) => vector.components.iter().any(contains_error),
        ExpressionKind::Rotation(rotation) => rotation.components.iter().any(contains_error),
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_) => false,
    }
}

fn evaluate_integer(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    depth: usize,
    budget: &mut usize,
) -> Option<i32> {
    if depth > MAX_CONSTANT_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;
    match &expression.kind {
        ExpressionKind::Parenthesized(inner) => {
            evaluate_integer(inner, scope, scopes, depth + 1, budget)
        }
        ExpressionKind::Integer(literal) => parse_integer_text(&literal.text),
        ExpressionKind::Identifier(identifier) => {
            if scopes
                .lookup_value_at(scope, &identifier.name, identifier.span.start)
                .is_some()
            {
                None
            } else {
                canonical_integer(&identifier.name)
            }
        }
        ExpressionKind::Unary(unary) => {
            let value = evaluate_integer(&unary.operand, scope, scopes, depth + 1, budget)?;
            match unary.operator {
                UnaryOperator::Plus => Some(value),
                UnaryOperator::Minus => Some(value.wrapping_neg()),
                UnaryOperator::Bang => Some((value == 0) as i32),
                UnaryOperator::Tilde => Some(!value),
            }
        }
        ExpressionKind::Binary(binary) => {
            let left = evaluate_integer(&binary.left, scope, scopes, depth + 1, budget)?;
            let right = evaluate_integer(&binary.right, scope, scopes, depth + 1, budget)?;
            match binary.operator {
                BinaryOperator::OrOr => Some((left != 0 || right != 0) as i32),
                BinaryOperator::AndAnd => Some((left != 0 && right != 0) as i32),
                BinaryOperator::BitOr => Some(left | right),
                BinaryOperator::BitXor => Some(left ^ right),
                BinaryOperator::BitAnd => Some(left & right),
                BinaryOperator::Equal => Some((left == right) as i32),
                BinaryOperator::NotEqual => Some((left != right) as i32),
                BinaryOperator::Less => Some((left < right) as i32),
                BinaryOperator::LessEqual => Some((left <= right) as i32),
                BinaryOperator::Greater => Some((left > right) as i32),
                BinaryOperator::GreaterEqual => Some((left >= right) as i32),
                BinaryOperator::ShiftLeft => (0..32)
                    .contains(&right)
                    .then(|| left.wrapping_shl(right as u32)),
                BinaryOperator::ShiftRight => (0..32)
                    .contains(&right)
                    .then(|| left.wrapping_shr(right as u32)),
                BinaryOperator::Add => Some(left.wrapping_add(right)),
                BinaryOperator::Subtract => Some(left.wrapping_sub(right)),
                BinaryOperator::Multiply => Some(left.wrapping_mul(right)),
                BinaryOperator::Divide => (right != 0).then(|| left.wrapping_div(right)),
                BinaryOperator::Remainder => (right != 0).then(|| left.wrapping_rem(right)),
            }
        }
        _ => None,
    }
}

fn parse_integer_text(text: &str) -> Option<i32> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok().map(|bits| bits as i32);
    }
    text.parse::<u64>().ok().map(|value| value as u32 as i32)
}

fn canonical_integer(name: &str) -> Option<i32> {
    let constant = builtins::definitions().constants.get(name)?;
    if constant.type_name != "integer" {
        return None;
    }
    let (negative, magnitude) = match constant.value.strip_prefix('-') {
        Some(magnitude) => (true, magnitude),
        None => (false, constant.value.as_str()),
    };
    let value = if let Some(hex) = magnitude
        .strip_prefix("0x")
        .or_else(|| magnitude.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()? as i32
    } else {
        magnitude.parse::<u32>().ok()? as i32
    };
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

#[cfg(test)]
mod limit_parity_tests {
    use super::{
        MAX_CFG_DEPTH, MAX_CFG_EDGES, MAX_CFG_NODES, MAX_CFG_WORK, MAX_CONSTANT_DEPTH,
        MAX_CONSTANT_NODES,
    };

    #[test]
    fn ast_and_legacy_analysis_budgets_remain_numerically_aligned() {
        // These assertions enforce policy parity, not identical algorithms or
        // units: AST counts CFG work / expression nodes, while Legacy counts
        // function / expression tokens.
        assert_eq!(
            MAX_CFG_NODES,
            crate::semantic::MAX_CFG_NODES,
            "AST and Legacy CFG node budgets must stay aligned"
        );
        assert_eq!(
            MAX_CFG_EDGES,
            crate::semantic::MAX_CFG_EDGES,
            "AST and Legacy CFG edge budgets must stay aligned"
        );
        assert_eq!(
            MAX_CFG_DEPTH,
            crate::semantic::MAX_CFG_DEPTH,
            "AST and Legacy CFG depth budgets must stay aligned"
        );
        assert_eq!(
            MAX_CFG_WORK,
            crate::semantic::MAX_CFG_TOKENS_PER_FUNCTION,
            "AST CFG work and Legacy per-function token budgets must stay numerically aligned"
        );
        assert_eq!(
            MAX_CONSTANT_NODES,
            crate::semantic::MAX_CONSTANT_EXPRESSION_TOKENS,
            "AST constant nodes and Legacy constant-expression tokens must stay numerically aligned"
        );
        assert_eq!(
            MAX_CONSTANT_DEPTH,
            crate::semantic::MAX_CONSTANT_EXPRESSION_DEPTH,
            "AST and Legacy constant-evaluation depth budgets must stay aligned"
        );
    }
}
