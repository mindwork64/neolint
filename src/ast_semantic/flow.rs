//! AST-native SEM008-SEM011 flow analysis.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    Ast, BinaryOperator, Block, Expression, ExpressionKind, ForStatement, IfStatement,
    ReturnStatement, Statement, StatementKind, TopLevel, TypeKind, UnaryOperator,
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

#[derive(Debug, Clone, Copy)]
enum ConstantNumber {
    Integer(i32),
    Float(f32),
}

impl ConstantNumber {
    fn truthy(self) -> bool {
        match self {
            Self::Integer(value) => value != 0,
            Self::Float(value) => value != 0.0,
        }
    }

    fn as_float(self) -> f32 {
        match self {
            Self::Integer(value) => value as f32,
            Self::Float(value) => value,
        }
    }
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
    let Some(value) = evaluate_constant_number(expression, scope, scopes, 0, &mut budget) else {
        // Unknown values deliberately preserve both paths. In particular, a
        // mutable local/global is never treated as a constant merely because
        // its initializer happens to be a literal.
        return Some((true, true));
    };
    let truthy = value.truthy();
    Some((truthy, !truthy))
}

fn evaluate_constant_number(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    depth: usize,
    budget: &mut usize,
) -> Option<ConstantNumber> {
    if depth > MAX_CONSTANT_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;

    match &expression.kind {
        ExpressionKind::Parenthesized(inner) => {
            evaluate_constant_number(inner, scope, scopes, depth + 1, budget)
        }
        ExpressionKind::Integer(literal) => {
            parse_integer_text(&literal.text).map(ConstantNumber::Integer)
        }
        ExpressionKind::Float(literal) => literal
            .text
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .map(ConstantNumber::Float),
        ExpressionKind::Identifier(identifier) => {
            if scopes
                .lookup_value_at(scope, &identifier.name, identifier.span.start)
                .is_some()
            {
                return None;
            }
            let constant = builtins::definitions()
                .constants
                .get(identifier.name.as_str())?;
            match constant.type_name.as_str() {
                "integer" => canonical_integer(&identifier.name).map(ConstantNumber::Integer),
                "float" => constant
                    .value
                    .parse::<f32>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(ConstantNumber::Float),
                _ => None,
            }
        }
        ExpressionKind::Unary(unary) => {
            let value = evaluate_constant_number(&unary.operand, scope, scopes, depth + 1, budget)?;
            match (unary.operator, value) {
                (UnaryOperator::Plus, value) => Some(value),
                (UnaryOperator::Minus, ConstantNumber::Integer(value)) => {
                    Some(ConstantNumber::Integer(value.wrapping_neg()))
                }
                (UnaryOperator::Minus, ConstantNumber::Float(value)) => {
                    let result = -value;
                    result.is_finite().then_some(ConstantNumber::Float(result))
                }
                (UnaryOperator::Bang, ConstantNumber::Integer(value)) => {
                    Some(ConstantNumber::Integer((value == 0) as i32))
                }
                (UnaryOperator::Bang, ConstantNumber::Float(_)) => None,
                (UnaryOperator::Tilde, ConstantNumber::Integer(value)) => {
                    Some(ConstantNumber::Integer(!value))
                }
                (UnaryOperator::Tilde, ConstantNumber::Float(_)) => None,
            }
        }
        ExpressionKind::Binary(binary) => {
            // LSL evaluates both operands of && and || (there is no
            // short-circuiting). Both values must therefore be statically
            // known before the logical result can be folded.
            let left = evaluate_constant_number(&binary.left, scope, scopes, depth + 1, budget)?;
            let right = evaluate_constant_number(&binary.right, scope, scopes, depth + 1, budget)?;
            if matches!(
                binary.operator,
                BinaryOperator::AndAnd | BinaryOperator::OrOr
            ) {
                let (ConstantNumber::Integer(left), ConstantNumber::Integer(right)) = (left, right)
                else {
                    return None;
                };
                return Some(ConstantNumber::Integer(match binary.operator {
                    BinaryOperator::AndAnd => (left != 0 && right != 0) as i32,
                    BinaryOperator::OrOr => (left != 0 || right != 0) as i32,
                    _ => unreachable!(),
                }));
            }
            evaluate_constant_binary(binary.operator, left, right)
        }
        ExpressionKind::Cast(cast) => {
            let value = evaluate_constant_number(&cast.operand, scope, scopes, depth + 1, budget)?;
            match (cast.ty.kind, value) {
                (TypeKind::Integer, ConstantNumber::Integer(value)) => {
                    Some(ConstantNumber::Integer(value))
                }
                (TypeKind::Float, ConstantNumber::Integer(value)) => {
                    let result = value as f32;
                    result.is_finite().then_some(ConstantNumber::Float(result))
                }
                (TypeKind::Float, ConstantNumber::Float(value)) => {
                    Some(ConstantNumber::Float(value))
                }
                // Both signed-zero bit patterns convert unambiguously to
                // integer zero. Other float-to-integer casts stay unknown
                // rather than relying on rounding or out-of-range rules.
                (TypeKind::Integer, ConstantNumber::Float(value))
                    if value.to_bits() & 0x7fff_ffff == 0 =>
                {
                    Some(ConstantNumber::Integer(0))
                }
                _ => None,
            }
        }
        ExpressionKind::Error(_) => None,
        ExpressionKind::String(_)
        | ExpressionKind::List(_)
        | ExpressionKind::Vector(_)
        | ExpressionKind::Rotation(_)
        | ExpressionKind::Call(_)
        | ExpressionKind::Component(_)
        | ExpressionKind::Assignment(_)
        | ExpressionKind::Increment(_)
        | ExpressionKind::LazyIndex(_) => None,
    }
}

fn evaluate_constant_binary(
    operator: BinaryOperator,
    left: ConstantNumber,
    right: ConstantNumber,
) -> Option<ConstantNumber> {
    use BinaryOperator::*;

    if let (ConstantNumber::Integer(left), ConstantNumber::Integer(right)) = (left, right) {
        let result = match operator {
            BitOr => left | right,
            BitXor => left ^ right,
            BitAnd => left & right,
            Equal => (left == right) as i32,
            NotEqual => (left != right) as i32,
            Less => (left < right) as i32,
            LessEqual => (left <= right) as i32,
            Greater => (left > right) as i32,
            GreaterEqual => (left >= right) as i32,
            ShiftLeft if (0..32).contains(&right) => left.wrapping_shl(right as u32),
            ShiftRight if (0..32).contains(&right) => left.wrapping_shr(right as u32),
            Add => left.wrapping_add(right),
            Subtract => left.wrapping_sub(right),
            Multiply => left.wrapping_mul(right),
            Divide if right != 0 => left.wrapping_div(right),
            Remainder if right != 0 => left.wrapping_rem(right),
            _ => return None,
        };
        return Some(ConstantNumber::Integer(result));
    }

    let left = left.as_float();
    let right = right.as_float();
    let result = match operator {
        Equal => return Some(ConstantNumber::Integer((left == right) as i32)),
        NotEqual => return Some(ConstantNumber::Integer((left != right) as i32)),
        Less => return Some(ConstantNumber::Integer((left < right) as i32)),
        LessEqual => return Some(ConstantNumber::Integer((left <= right) as i32)),
        Greater => return Some(ConstantNumber::Integer((left > right) as i32)),
        GreaterEqual => return Some(ConstantNumber::Integer((left >= right) as i32)),
        Add => left + right,
        Subtract => left - right,
        Multiply => left * right,
        Divide if right != 0.0 => left / right,
        // Modulo/bitwise/shift semantics are not assumed for mixed or float
        // operands. Leaving them unknown is safer than inventing a conversion.
        _ => return None,
    };
    result.is_finite().then_some(ConstantNumber::Float(result))
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

#[cfg(test)]
mod constant_condition_tests {
    use crate::ast::{parse_ast, parse_ast_with_options, AstParseOptions};
    use crate::diagnostic::Diagnostic;
    use crate::lex;

    fn missing_return_functions(source: &str) -> Vec<String> {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast(&tokens);
        assert!(syntax.is_empty(), "{syntax:?}");
        crate::ast_semantic::analyze_ast(&ast, &syntax)
            .into_iter()
            .filter(|diagnostic| diagnostic.code == "SEM011")
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn float_arithmetic_and_comparisons_prune_impossible_cfg_edges() {
        let source = r#"
            integer true_float_if() {
                if (1.5 + 0.5 == 2.0) return 1;
            }
            integer false_float_if() {
                if (1.5 + 0.5 != 2.0) return 1;
            }
            integer false_float_while() {
                while (2.0 - 2.0) { return 1; }
            }
            integer true_float_while() {
                while (2.0 - 1.0) { }
            }
            integer false_float_for() {
                for (; 0.0; ) { return 1; }
            }
            integer true_float_for() {
                for (; 2.0 - 1.0; ) { }
            }
            integer canonical_pi() {
                if (PI > 3.0) return 1;
            }
            integer canonical_two_pi_true() {
                if (TWO_PI > 3.0) return 1;
            }
            integer do_return() {
                do { return 1; } while (0.0);
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function false_float_if may reach the end without returning a value",
                "function false_float_while may reach the end without returning a value",
                "function false_float_for may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn logical_conditions_require_both_operands_to_be_known() {
        let source = r#"
            integer false_and_with_error() {
                if (FALSE && (1 / 0)) return 1;
            }
            integer true_or_with_call() {
                if (TRUE || unknown()) return 1;
            }
            integer true_and() {
                if (TRUE && (2 > 1)) return 1;
            }
            integer false_or() {
                if (FALSE || (0.0 == 1.0)) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function false_and_with_error may reach the end without returning a value",
                "function true_or_with_call may reach the end without returning a value",
                "function false_or may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn mutable_values_and_non_numeric_conditions_remain_unknown() {
        let source = r#"
            integer mutable_condition(integer condition) {
                if (condition) return 1;
            }
            integer non_finite_condition() {
                if (1.0 / 0.0) return 1;
            }
            integer invalid_float_logical() {
                if (1.0 && 1) return 1;
            }
            integer invalid_float_not() {
                if (!1.0) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function mutable_condition may reach the end without returning a value",
                "function non_finite_condition may reach the end without returning a value",
                "function invalid_float_logical may reach the end without returning a value",
                "function invalid_float_not may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn float_cast_from_integer_is_known_but_unsafe_reverse_cast_is_not() {
        let source = r#"
            integer known_float_cast() {
                if ((float)1 > 0.0) return 1;
            }
            integer unknown_integer_cast() {
                if ((integer)1.5) return 1;
            }
            integer zero_float_cast() {
                if ((integer)0.0 == 0) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec!["function unknown_integer_cast may reach the end without returning a value",]
        );
    }

    #[test]
    fn switch_cfg_keeps_existing_gate_and_analysis_contract() {
        let source = r#"
            integer switch_return(integer value) {
                switch (value) {
                    case 1: return 1;
                    default: return 2;
                }
            }
            default { state_entry() { } }
        "#;
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) =
            parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
        assert!(syntax.is_empty(), "{syntax:?}");
        let diagnostics = crate::ast_semantic::analyze_ast(&ast, &syntax);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic: &Diagnostic| diagnostic.code != "SEM011"),
            "{diagnostics:?}"
        );
    }
}
