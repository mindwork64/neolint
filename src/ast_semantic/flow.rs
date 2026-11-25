//! AST-native SEM008-SEM011 flow analysis.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::{
    AssignmentOperator, Ast, BinaryOperator, Block, Expression, ExpressionKind, ForStatement,
    IfStatement, ReturnStatement, Statement, StatementKind, TopLevel, TypeKind, UnaryOperator,
};
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

use super::rules::semantic_error;
use super::scopes::{ScopeId, ScopeTree, SymbolKind};
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
const MAX_CONSTANT_FACTS: usize = 4_096;
const MAX_DATAFLOW_WORK: usize = 131_072;
const MAX_DATAFLOW_FACT_WORK: usize = 1_048_576;
const MAX_DATAFLOW_EDGE_WORK: usize = 1_048_576;
const CONSTANT_PROPAGATION_DISABLED: usize = usize::MAX;

type ConstantEnvironment = HashMap<usize, ConstantNumber>;

/// Jump diagnostics and code scopes where an invalid jump graph must be ignored.
pub(crate) struct JumpReport {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) invalid_functions: HashSet<(usize, usize)>,
    pub(crate) invalid_code_scopes: HashSet<(usize, usize)>,
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
        invalid_code_scopes: HashSet::new(),
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
        if report.diagnostics.len() != before {
            let key = (code_scope.body.span.start, code_scope.body.span.end);
            report.invalid_code_scopes.insert(key);
            if code_scope.is_function {
                report.invalid_functions.insert(key);
            }
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
        let Some(flow) = analyze_body(body, scope, scopes, &HashSet::new()) else {
            continue;
        };
        if flow.reaches_end {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeCondition {
    Always,
    True,
    False,
}

#[derive(Debug, Clone, Copy)]
struct FlowEdge {
    target: usize,
    condition: EdgeCondition,
}

enum FlowTransfer {
    Local {
        declaration: crate::ast::LocalVariableDeclaration,
        scope: ScopeId,
    },
    Expression {
        expression: Expression,
        scope: ScopeId,
    },
    Switch {
        expression: Expression,
        scope: ScopeId,
        cases: Vec<(Expression, usize)>,
        defaults: Vec<usize>,
        unmatched_exit: usize,
    },
}

struct FlowCondition {
    expression: Expression,
    scope: ScopeId,
    empty_is_true: bool,
    malformed: bool,
}

struct FlowGraph {
    successors: Vec<Vec<FlowEdge>>,
    transfers: Vec<Option<FlowTransfer>>,
    conditions: Vec<Option<FlowCondition>>,
    labels: HashMap<String, usize>,
    pending_jumps: Vec<(usize, String)>,
    statement_entries: HashMap<(usize, usize), usize>,
    terminating_state_changes: HashSet<(usize, usize)>,
    edge_count: usize,
    work: usize,
    failed: bool,
}

pub(crate) struct FlowAnalysis {
    pub(crate) reaches_end: bool,
    unreachable_statements: HashSet<(usize, usize)>,
}

impl FlowAnalysis {
    pub(crate) fn statement_is_unreachable(&self, statement: &Statement) -> bool {
        self.unreachable_statements
            .contains(&(statement.span.start, statement.span.end))
    }
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
        self.transfers.push(None);
        self.conditions.push(None);
        Some(id)
    }

    fn edge(&mut self, from: usize, to: usize) -> bool {
        self.edge_with_condition(from, to, EdgeCondition::Always)
    }

    fn conditional_edge(&mut self, from: usize, to: usize, condition: EdgeCondition) -> bool {
        self.edge_with_condition(from, to, condition)
    }

    fn edge_with_condition(&mut self, from: usize, to: usize, condition: EdgeCondition) -> bool {
        let Some(edges) = self.successors.get_mut(from) else {
            self.failed = true;
            return false;
        };
        if edges
            .iter()
            .any(|edge| edge.target == to && edge.condition == condition)
        {
            return true;
        }
        if self.edge_count >= MAX_CFG_EDGES {
            self.failed = true;
            return false;
        }
        self.edge_count += 1;
        edges.push(FlowEdge {
            target: to,
            condition,
        });
        true
    }

    fn set_transfer(&mut self, node: usize, transfer: FlowTransfer) -> bool {
        let Some(slot) = self.transfers.get_mut(node) else {
            self.failed = true;
            return false;
        };
        *slot = Some(transfer);
        true
    }

    fn set_condition(&mut self, node: usize, condition: FlowCondition) -> bool {
        let Some(slot) = self.conditions.get_mut(node) else {
            self.failed = true;
            return false;
        };
        *slot = Some(condition);
        true
    }

    /// Forward must-analysis. A state is the set of local numeric values that
    /// agree on every reachable incoming path. Unknown facts are removed at
    /// joins; worklist iteration naturally reaches a fixed point through loops
    /// and non-local jump/label edges.
    fn solve_constants(
        &self,
        start: usize,
        scopes: &ScopeTree,
    ) -> Option<Vec<Option<ConstantEnvironment>>> {
        if start >= self.successors.len() {
            return None;
        }
        let mut incoming = vec![None; self.successors.len()];
        let mut queued = vec![false; self.successors.len()];
        let mut pending = VecDeque::new();
        incoming[start] = Some(ConstantEnvironment::new());
        pending.push_back(start);
        queued[start] = true;
        let mut work = 0usize;
        let mut fact_work = 0usize;
        let mut edge_work = 0usize;
        let mut switch_work = 0usize;

        while let Some(node) = pending.pop_front() {
            queued[node] = false;
            work = work.saturating_add(1);
            if work > MAX_DATAFLOW_WORK {
                return None;
            }
            let input = incoming.get(node)?.as_ref()?.clone();
            fact_work = fact_work.saturating_add(input.len());
            if fact_work > MAX_DATAFLOW_FACT_WORK {
                return None;
            }
            let mut output = input.clone();
            let mut switch_targets: Option<HashSet<usize>> = None;
            let (may_true, may_false) = if let Some(condition) = self.conditions.get(node)?.as_ref()
            {
                let possibilities = if condition.malformed {
                    Some((true, true))
                } else {
                    condition_possibilities(
                        &condition.expression,
                        condition.empty_is_true,
                        condition.scope,
                        scopes,
                        &input,
                    )
                }?;
                if !condition.empty_is_true {
                    apply_expression_effects(
                        &condition.expression,
                        condition.scope,
                        scopes,
                        &mut output,
                    );
                }
                possibilities
            } else {
                if let Some(transfer) = self.transfers.get(node)?.as_ref() {
                    match transfer {
                        FlowTransfer::Local { declaration, scope } => {
                            apply_local_declaration(declaration, *scope, scopes, &mut output);
                        }
                        FlowTransfer::Expression { expression, scope } => {
                            apply_expression_effects(expression, *scope, scopes, &mut output);
                        }
                        FlowTransfer::Switch {
                            expression,
                            scope,
                            cases,
                            defaults,
                            unmatched_exit,
                        } => {
                            switch_work = switch_work
                                .saturating_add(cases.len().saturating_add(defaults.len()));
                            if switch_work > MAX_DATAFLOW_EDGE_WORK {
                                return None;
                            }
                            switch_targets = Some(switch_dispatch_targets(
                                expression,
                                *scope,
                                scopes,
                                &input,
                                cases,
                                defaults,
                                *unmatched_exit,
                            ));
                            apply_expression_effects(expression, *scope, scopes, &mut output);
                        }
                    }
                }
                (true, true)
            };

            for edge in self.successors.get(node)? {
                edge_work = edge_work.saturating_add(1);
                if edge_work > MAX_DATAFLOW_EDGE_WORK {
                    return None;
                }
                let allowed = if let Some(targets) = &switch_targets {
                    targets.contains(&edge.target)
                } else {
                    match edge.condition {
                        EdgeCondition::Always => true,
                        EdgeCondition::True => may_true,
                        EdgeCondition::False => may_false,
                    }
                };
                if !allowed {
                    continue;
                }
                fact_work = fact_work.saturating_add(output.len());
                if fact_work > MAX_DATAFLOW_FACT_WORK {
                    return None;
                }
                let target_state = incoming.get_mut(edge.target)?;
                let changed = match target_state {
                    None => {
                        *target_state = Some(output.clone());
                        true
                    }
                    Some(existing) => {
                        let merged =
                            merge_constant_environments(&[existing.clone(), output.clone()]);
                        if merged != *existing {
                            *existing = merged;
                            true
                        } else {
                            false
                        }
                    }
                };
                if changed && !queued[edge.target] {
                    pending.push_back(edge.target);
                    queued[edge.target] = true;
                }
            }
        }
        Some(incoming)
    }
}

struct Fragment {
    entry: usize,
    exits: Vec<usize>,
}

/// Build one bounded CFG and expose statement reachability for SEM011/SEM037.
pub(crate) fn analyze_body(
    body: &Block,
    scope: ScopeId,
    scopes: &ScopeTree,
    terminating_state_changes: &HashSet<(usize, usize)>,
) -> Option<FlowAnalysis> {
    let mut graph = FlowGraph {
        successors: Vec::new(),
        transfers: Vec::new(),
        conditions: Vec::new(),
        labels: HashMap::new(),
        pending_jumps: Vec::new(),
        statement_entries: HashMap::new(),
        terminating_state_changes: terminating_state_changes.clone(),
        edge_count: 0,
        work: 0,
        failed: false,
    };
    // The builder records transfer operations; a separate CFG worklist pass
    // computes local values across joins, loops, and jump/label edges.
    let mut constants = ConstantEnvironment::new();
    let fragment = build_statements(
        &body.statements,
        scope,
        scopes,
        &mut graph,
        None,
        0,
        &mut constants,
    )?;
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
    let incoming = graph.solve_constants(fragment.entry, scopes)?;
    let reaches_end = incoming.get(exit)?.is_some();
    let unreachable_statements = graph
        .statement_entries
        .iter()
        .filter_map(|(key, node)| {
            incoming
                .get(*node)
                .is_some_and(Option::is_none)
                .then_some(*key)
        })
        .collect();
    Some(FlowAnalysis {
        reaches_end,
        unreachable_statements,
    })
}

fn build_statements(
    statements: &[Statement],
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
    constants: &mut ConstantEnvironment,
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
        // Once the preceding fragment has no ordinary exit, this statement is
        // unreachable along structured fallthrough. Still build its CFG nodes
        // for diagnostics, but never let its assignments leak into a live path.
        let has_live_fallthrough = entry.is_none() || !exits.is_empty();
        let mut next_constants = constants.clone();
        let fragment = build_statement(
            statement,
            scope,
            scopes,
            graph,
            break_target,
            depth + 1,
            &mut next_constants,
        )?;
        if entry.is_some() {
            if !connect_exits(graph, &exits, fragment.entry) {
                return None;
            }
        } else {
            entry = Some(fragment.entry);
        }
        exits = fragment.exits;
        if has_live_fallthrough {
            *constants = next_constants;
        }
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
    constants: &mut ConstantEnvironment,
) -> Option<Fragment> {
    let fragment = build_statement_inner(
        statement,
        scope,
        scopes,
        graph,
        break_target,
        depth,
        constants,
    )?;
    let key = (statement.span.start, statement.span.end);
    if graph
        .statement_entries
        .insert(key, fragment.entry)
        .is_some()
    {
        graph.failed = true;
        return None;
    }
    Some(fragment)
}

fn build_statement_inner(
    statement: &Statement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
    constants: &mut ConstantEnvironment,
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
            build_statements(
                &block.statements,
                child,
                scopes,
                graph,
                break_target,
                depth,
                constants,
            )
        }
        StatementKind::If(if_statement) => build_if(
            if_statement,
            scope,
            scopes,
            graph,
            break_target,
            depth,
            constants,
        ),
        StatementKind::For(for_statement) => build_for(
            for_statement,
            scope,
            scopes,
            graph,
            break_target,
            depth,
            constants,
        ),
        StatementKind::While(while_statement) => build_while(
            while_statement,
            scope,
            scopes,
            graph,
            break_target,
            depth,
            constants,
        ),
        StatementKind::DoWhile(do_while) => {
            clear_constant_facts(constants);
            let mut body_constants = empty_constant_environment_like(constants);
            let body = build_statement(
                &do_while.body,
                scope,
                scopes,
                graph,
                break_target,
                depth + 1,
                &mut body_constants,
            )?;
            let condition = graph.node()?;
            if !graph.set_condition(
                condition,
                FlowCondition {
                    expression: do_while.condition.clone(),
                    scope,
                    empty_is_true: false,
                    malformed: false,
                },
            ) || !connect_exits(graph, &body.exits, condition)
            {
                return None;
            }
            let loop_exit = graph.node()?;
            if !graph.conditional_edge(condition, body.entry, EdgeCondition::True)
                || !graph.conditional_edge(condition, loop_exit, EdgeCondition::False)
            {
                return None;
            }
            clear_constant_facts(constants);
            Some(Fragment {
                entry: body.entry,
                exits: vec![loop_exit],
            })
        }
        StatementKind::Return(_) => {
            let node = graph.node()?;
            Some(Fragment {
                entry: node,
                exits: Vec::new(),
            })
        }
        StatementKind::Switch(switch) => {
            build_switch(switch, scope, scopes, graph, depth, constants)
        }
        StatementKind::Error(_) => {
            graph.failed = true;
            None
        }
        StatementKind::StateChange(_)
            if graph
                .terminating_state_changes
                .contains(&(statement.span.start, statement.span.end)) =>
        {
            let node = graph.node()?;
            Some(Fragment {
                entry: node,
                exits: Vec::new(),
            })
        }
        StatementKind::Local(local) => {
            apply_local_declaration(local, scope, scopes, constants);
            let node = graph.node()?;
            if !graph.set_transfer(
                node,
                FlowTransfer::Local {
                    declaration: local.clone(),
                    scope,
                },
            ) {
                return None;
            }
            Some(Fragment {
                entry: node,
                exits: vec![node],
            })
        }
        StatementKind::Expression(expression) => {
            apply_expression_effects(&expression.expression, scope, scopes, constants);
            let node = graph.node()?;
            if !graph.set_transfer(
                node,
                FlowTransfer::Expression {
                    expression: expression.expression.clone(),
                    scope,
                },
            ) {
                return None;
            }
            Some(Fragment {
                entry: node,
                exits: vec![node],
            })
        }
        StatementKind::Jump(_) | StatementKind::Label(_) | StatementKind::Break(_) => {
            clear_constant_facts(constants);
            let node = graph.node()?;
            match &statement.kind {
                StatementKind::Jump(jump) => graph.pending_jumps.push((node, jump.label.clone())),
                StatementKind::Label(label) => {
                    if graph.labels.insert(label.name.clone(), node).is_some() {
                        graph.failed = true;
                        return None;
                    }
                }
                StatementKind::Break(_) => {
                    let Some(target) = break_target else {
                        graph.failed = true;
                        return None;
                    };
                    if !graph.edge(node, target) {
                        return None;
                    }
                }
                _ => unreachable!(),
            }
            Some(Fragment {
                entry: node,
                exits: if matches!(&statement.kind, StatementKind::Label(_)) {
                    vec![node]
                } else {
                    Vec::new()
                },
            })
        }
        StatementKind::Empty | StatementKind::StateChange(_) => {
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
    constants: &mut ConstantEnvironment,
) -> Option<Fragment> {
    let condition = graph.node()?;
    let (may_true, may_false) =
        condition_possibilities(&if_statement.condition, false, scope, scopes, constants)?;
    // The worklist pass decides which edge is feasible for the final incoming
    // state. Both edges must exist here because a later CFG predecessor may
    // weaken an initially-known local fact.
    if !graph.set_condition(
        condition,
        FlowCondition {
            expression: if_statement.condition.clone(),
            scope,
            empty_is_true: false,
            malformed: false,
        },
    ) {
        return None;
    }
    apply_expression_effects(&if_statement.condition, scope, scopes, constants);
    let branch_input = constants.clone();

    let mut then_constants = branch_input.clone();
    let then_branch = build_statement(
        &if_statement.then_branch,
        scope,
        scopes,
        graph,
        break_target,
        depth + 1,
        &mut then_constants,
    )?;
    if !graph.conditional_edge(condition, then_branch.entry, EdgeCondition::True) {
        return None;
    }
    let mut exits = then_branch.exits.clone();
    let mut live_environments = Vec::new();
    if may_true && !then_branch.exits.is_empty() {
        live_environments.push(then_constants);
    }

    if let Some(else_branch) = &if_statement.else_branch {
        let mut else_constants = branch_input.clone();
        let else_fragment = build_statement(
            else_branch,
            scope,
            scopes,
            graph,
            break_target,
            depth + 1,
            &mut else_constants,
        )?;
        if !graph.conditional_edge(condition, else_fragment.entry, EdgeCondition::False) {
            return None;
        }
        exits.extend(else_fragment.exits.iter().copied());
        if may_false && !else_fragment.exits.is_empty() {
            live_environments.push(else_constants);
        }
    } else {
        let fallthrough = graph.node()?;
        if !graph.conditional_edge(condition, fallthrough, EdgeCondition::False) {
            return None;
        }
        exits.push(fallthrough);
        if may_false {
            live_environments.push(branch_input);
        }
    }

    *constants = merge_constant_environments(&live_environments);
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
    constants: &mut ConstantEnvironment,
) -> Option<Fragment> {
    clear_constant_facts(constants);
    let mut loop_constants = empty_constant_environment_like(constants);

    // `for` initializers execute exactly once before the first condition.
    let mut initializer_entry = None;
    let mut initializer_exits = Vec::new();
    for expression in &for_statement.initializer {
        apply_expression_effects(expression, scope, scopes, &mut loop_constants);
        let node = graph.node()?;
        if !graph.set_transfer(
            node,
            FlowTransfer::Expression {
                expression: expression.clone(),
                scope,
            },
        ) {
            return None;
        }
        if initializer_entry.is_none() {
            initializer_entry = Some(node);
        }
        if !initializer_exits.is_empty() && !connect_exits(graph, &initializer_exits, node) {
            return None;
        }
        initializer_exits = vec![node];
    }

    let condition = graph.node()?;
    if !initializer_exits.is_empty() && !connect_exits(graph, &initializer_exits, condition) {
        return None;
    }
    if !graph.set_condition(
        condition,
        FlowCondition {
            expression: for_statement.condition.clone(),
            scope,
            empty_is_true: for_statement.condition_missing,
            malformed: for_statement.condition_malformed,
        },
    ) {
        return None;
    }
    if !for_statement.condition_missing {
        apply_expression_effects(&for_statement.condition, scope, scopes, &mut loop_constants);
    }
    let body = build_statement(
        &for_statement.body,
        scope,
        scopes,
        graph,
        break_target,
        depth + 1,
        &mut loop_constants,
    )?;
    if !graph.conditional_edge(condition, body.entry, EdgeCondition::True) {
        return None;
    }

    // Increment expressions run after each normally-completing body iteration,
    // immediately before the next condition evaluation.
    let mut increment_entry = None;
    let mut increment_exits = Vec::new();
    for expression in &for_statement.increment {
        apply_expression_effects(expression, scope, scopes, &mut loop_constants);
        let node = graph.node()?;
        if !graph.set_transfer(
            node,
            FlowTransfer::Expression {
                expression: expression.clone(),
                scope,
            },
        ) {
            return None;
        }
        if increment_entry.is_none() {
            increment_entry = Some(node);
        }
        if !increment_exits.is_empty() && !connect_exits(graph, &increment_exits, node) {
            return None;
        }
        increment_exits = vec![node];
    }
    if let Some(increment) = increment_entry {
        if !connect_exits(graph, &body.exits, increment)
            || !connect_exits(graph, &increment_exits, condition)
        {
            return None;
        }
    } else if !connect_exits(graph, &body.exits, condition) {
        return None;
    }

    let loop_exit = graph.node()?;
    if !graph.conditional_edge(condition, loop_exit, EdgeCondition::False) {
        return None;
    }
    clear_constant_facts(constants);
    Some(Fragment {
        entry: initializer_entry.unwrap_or(condition),
        exits: vec![loop_exit],
    })
}

fn build_while(
    while_statement: &crate::ast::WhileStatement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    break_target: Option<usize>,
    depth: usize,
    constants: &mut ConstantEnvironment,
) -> Option<Fragment> {
    clear_constant_facts(constants);
    let mut loop_constants = empty_constant_environment_like(constants);
    let condition = graph.node()?;
    if !graph.set_condition(
        condition,
        FlowCondition {
            expression: while_statement.condition.clone(),
            scope,
            empty_is_true: false,
            malformed: false,
        },
    ) {
        return None;
    }
    let body = build_statement(
        &while_statement.body,
        scope,
        scopes,
        graph,
        break_target,
        depth + 1,
        &mut loop_constants,
    )?;
    if !graph.conditional_edge(condition, body.entry, EdgeCondition::True)
        || !connect_exits(graph, &body.exits, condition)
    {
        return None;
    }
    let loop_exit = graph.node()?;
    if !graph.conditional_edge(condition, loop_exit, EdgeCondition::False) {
        return None;
    }
    clear_constant_facts(constants);
    Some(Fragment {
        entry: condition,
        exits: vec![loop_exit],
    })
}

fn build_switch(
    switch: &crate::ast::SwitchStatement,
    scope: ScopeId,
    scopes: &ScopeTree,
    graph: &mut FlowGraph,
    depth: usize,
    constants: &mut ConstantEnvironment,
) -> Option<Fragment> {
    clear_constant_facts(constants);
    if depth > MAX_CFG_DEPTH {
        graph.failed = true;
        return None;
    }
    // A switch owns its break target. Breaks in nested blocks and control-flow
    // bodies resolve here; a nested switch shadows this target with its own.
    let switch_exit = graph.node()?;
    let switch_scope = scopes.scope_for_switch(switch).unwrap_or(scope);
    let mut arms = Vec::with_capacity(switch.arms.len());
    let mut cases = Vec::new();
    let mut defaults = Vec::new();

    for arm in &switch.arms {
        if matches!(arm.label, crate::ast::SwitchLabel::Error(_)) {
            graph.failed = true;
            return None;
        }
        let mut arm_constants = empty_constant_environment_like(constants);
        let fragment = build_statements(
            &arm.statements,
            switch_scope,
            scopes,
            graph,
            Some(switch_exit),
            depth + 1,
            &mut arm_constants,
        )?;
        match &arm.label {
            crate::ast::SwitchLabel::Case(expression) => {
                cases.push((expression.clone(), fragment.entry));
            }
            crate::ast::SwitchLabel::Default => defaults.push(fragment.entry),
            crate::ast::SwitchLabel::Error(_) => {
                graph.failed = true;
                return None;
            }
        }
        arms.push(fragment);
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
    for (_, target) in &cases {
        if !graph.edge(dispatch, *target) {
            return None;
        }
    }
    for target in &defaults {
        if !graph.edge(dispatch, *target) {
            return None;
        }
    }
    if defaults.is_empty() && !graph.edge(dispatch, switch_exit) {
        return None;
    }
    if !graph.set_transfer(
        dispatch,
        FlowTransfer::Switch {
            expression: switch.condition.clone(),
            scope: switch_scope,
            cases,
            defaults,
            unmatched_exit: switch_exit,
        },
    ) {
        return None;
    }

    // Keep the structural switch exit even when it is currently unreachable.
    // Deferred `jump` edges are resolved after the enclosing function is built
    // and may make the exit reachable later.
    clear_constant_facts(constants);
    Some(Fragment {
        entry: dispatch,
        exits: vec![switch_exit],
    })
}

fn switch_dispatch_targets(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &ConstantEnvironment,
    cases: &[(Expression, usize)],
    defaults: &[usize],
    unmatched_exit: usize,
) -> HashSet<usize> {
    if let Some(switch_value) = evaluate_expression_result(expression, scope, scopes, constants) {
        return numeric_switch_dispatch_targets(
            switch_value,
            scope,
            scopes,
            cases,
            defaults,
            unmatched_exit,
        );
    }

    // The shared Firestorm evaluator also supports constant strings, keys,
    // vectors, rotations and list-length equality. It intentionally does not
    // consult flow-sensitive locals, so unknown local values stay conservative.
    let mut switch_budget = MAX_CONSTANT_NODES;
    let Some(switch_value) =
        crate::ast_semantic::rules::firestorm_switches::evaluate_switch_constant_value(
            expression,
            scope,
            scopes,
            &mut switch_budget,
        )
    else {
        return all_switch_targets(cases, defaults, unmatched_exit);
    };
    let mut case_budget = MAX_CONSTANT_NODES;
    let mut targets = HashSet::new();
    for (case, target) in cases {
        let Some(case_value) =
            crate::ast_semantic::rules::firestorm_switches::evaluate_switch_constant_value(
                case,
                scope,
                scopes,
                &mut case_budget,
            )
        else {
            return all_switch_targets(cases, defaults, unmatched_exit);
        };
        if switch_value.equals(&case_value) {
            targets.insert(*target);
        }
    }
    finish_switch_dispatch_targets(targets, defaults, unmatched_exit)
}

fn numeric_switch_dispatch_targets(
    switch_value: ConstantNumber,
    scope: ScopeId,
    scopes: &ScopeTree,
    cases: &[(Expression, usize)],
    defaults: &[usize],
    unmatched_exit: usize,
) -> HashSet<usize> {
    // Case labels are compile-time expressions. Do not let a flow-sensitive
    // local fact turn an otherwise invalid/non-constant label into a match.
    let empty_constants = ConstantEnvironment::new();
    let mut case_budget = MAX_CONSTANT_NODES;
    let mut targets = HashSet::new();
    for (case, target) in cases {
        let Some(value) =
            evaluate_constant_number(case, scope, scopes, &empty_constants, 0, &mut case_budget)
        else {
            return all_switch_targets(cases, defaults, unmatched_exit);
        };
        if constant_numbers_equal(switch_value, value) {
            targets.insert(*target);
        }
    }
    finish_switch_dispatch_targets(targets, defaults, unmatched_exit)
}

fn finish_switch_dispatch_targets(
    mut targets: HashSet<usize>,
    defaults: &[usize],
    unmatched_exit: usize,
) -> HashSet<usize> {
    if targets.is_empty() {
        if defaults.is_empty() {
            targets.insert(unmatched_exit);
        } else {
            targets.extend(defaults.iter().copied());
        }
    }
    targets
}

fn all_switch_targets(
    cases: &[(Expression, usize)],
    defaults: &[usize],
    unmatched_exit: usize,
) -> HashSet<usize> {
    let mut targets = HashSet::with_capacity(cases.len() + defaults.len() + 1);
    targets.extend(cases.iter().map(|(_, target)| *target));
    targets.extend(defaults.iter().copied());
    if defaults.is_empty() {
        targets.insert(unmatched_exit);
    }
    targets
}

fn constant_numbers_equal(left: ConstantNumber, right: ConstantNumber) -> bool {
    match (left, right) {
        (ConstantNumber::Integer(left), ConstantNumber::Integer(right)) => left == right,
        (left, right) => left.as_float() == right.as_float(),
    }
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

fn is_constant_propagation_disabled(constants: &ConstantEnvironment) -> bool {
    constants.contains_key(&CONSTANT_PROPAGATION_DISABLED)
}

fn empty_constant_environment_like(source: &ConstantEnvironment) -> ConstantEnvironment {
    let mut constants = ConstantEnvironment::new();
    if is_constant_propagation_disabled(source) {
        constants.insert(CONSTANT_PROPAGATION_DISABLED, ConstantNumber::Integer(0));
    }
    constants
}

fn clear_constant_facts(constants: &mut ConstantEnvironment) {
    let disabled = is_constant_propagation_disabled(constants);
    constants.clear();
    if disabled {
        constants.insert(CONSTANT_PROPAGATION_DISABLED, ConstantNumber::Integer(0));
    }
}

fn insert_constant_fact(constants: &mut ConstantEnvironment, key: usize, value: ConstantNumber) {
    if is_constant_propagation_disabled(constants) {
        return;
    }
    if constants.contains_key(&key) {
        constants.insert(key, value);
    } else if constants.len() >= MAX_CONSTANT_FACTS {
        // Keep memory/work bounded and fail closed for the rest of this body.
        clear_constant_facts(constants);
        constants.insert(CONSTANT_PROPAGATION_DISABLED, ConstantNumber::Integer(0));
    } else {
        constants.insert(key, value);
    }
}

fn merge_constant_environments(environments: &[ConstantEnvironment]) -> ConstantEnvironment {
    if let Some(disabled) = environments
        .iter()
        .find(|environment| is_constant_propagation_disabled(environment))
    {
        return empty_constant_environment_like(disabled);
    }
    let Some(first) = environments.first() else {
        return ConstantEnvironment::new();
    };
    let mut merged = first.clone();
    merged.retain(|key, value| {
        environments[1..]
            .iter()
            .all(|environment| environment.get(key) == Some(value))
    });
    merged
}

fn local_numeric_binding(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
) -> Option<(usize, &'static str)> {
    let mut target = expression;
    while let ExpressionKind::Parenthesized(inner) = &target.kind {
        target = inner;
    }
    let ExpressionKind::Identifier(identifier) = &target.kind else {
        return None;
    };
    let symbol = scopes.lookup_value_at(scope, &identifier.name, identifier.span.start)?;
    if symbol.kind != SymbolKind::Local {
        return None;
    }
    match symbol.type_name.as_deref()? {
        "integer" => Some((symbol.span.start, "integer")),
        "float" => Some((symbol.span.start, "float")),
        _ => None,
    }
}

fn constant_matches_type(value: ConstantNumber, type_name: &str) -> bool {
    matches!(
        (value, type_name),
        (ConstantNumber::Integer(_), "integer") | (ConstantNumber::Float(_), "float")
    )
}

fn apply_local_declaration(
    local: &crate::ast::LocalVariableDeclaration,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &mut ConstantEnvironment,
) {
    if is_constant_propagation_disabled(constants) {
        return;
    }
    for declarator in &local.declarators {
        let binding = scopes
            .lookup_value_at(scope, &declarator.name, declarator.name_span.start)
            .filter(|symbol| symbol.kind == SymbolKind::Local)
            .map(|symbol| (symbol.span.start, symbol.type_name.as_deref().unwrap_or("")));
        if let Some((key, _)) = binding {
            // A declaration without an initializer is not definitely known.
            constants.remove(&key);
        }
        if declarator.initializer_error.is_some() {
            clear_constant_facts(constants);
            if let Some((key, _)) = binding {
                constants.remove(&key);
            }
            continue;
        }
        let Some(initializer) = &declarator.initializer else {
            continue;
        };
        // Evaluate the initializer against the values available before this
        // declarator is written; a self-reference must not see stale data.
        let candidate = evaluate_expression_result(initializer, scope, scopes, constants);
        apply_expression_effects(initializer, scope, scopes, constants);
        if let Some((key, type_name)) = binding {
            if let Some(value) = candidate.filter(|value| constant_matches_type(*value, type_name))
            {
                insert_constant_fact(constants, key, value);
            } else {
                constants.remove(&key);
            }
        }
    }
}

fn apply_expression_effects(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &mut ConstantEnvironment,
) {
    if is_constant_propagation_disabled(constants) {
        return;
    }
    if side_effect_count(expression) == usize::MAX || contains_error(expression) {
        // Recovered or pathologically deep expressions can retain only part
        // of their side effects. Do not carry facts past such an expression.
        clear_constant_facts(constants);
        return;
    }
    match &expression.kind {
        ExpressionKind::Assignment(assignment) => {
            let binding = local_numeric_binding(&assignment.target, scope, scopes);
            // Assignment chains are represented as right-nested AST nodes,
            // so their result and writes can be evaluated in that same order.
            let candidate = binding
                .and_then(|_| evaluate_expression_result(expression, scope, scopes, constants));

            if binding.is_none() {
                let nested_effects = side_effect_count(&assignment.target)
                    .saturating_add(side_effect_count(&assignment.value));
                if nested_effects > 1 {
                    invalidate_written_local_facts(expression, scope, scopes, constants);
                    return;
                }
                apply_expression_effects(&assignment.target, scope, scopes, constants);
            }
            apply_expression_effects(&assignment.value, scope, scopes, constants);
            if let Some((key, type_name)) = binding {
                constants.remove(&key);
                if let Some(value) =
                    candidate.filter(|value| constant_matches_type(*value, type_name))
                {
                    insert_constant_fact(constants, key, value);
                }
            }
        }
        ExpressionKind::Increment(increment) => {
            if let Some((key, type_name)) = local_numeric_binding(&increment.operand, scope, scopes)
            {
                let one = match type_name {
                    "integer" => ConstantNumber::Integer(1),
                    "float" => ConstantNumber::Float(1.0),
                    _ => unreachable!(),
                };
                let operator = match increment.operator {
                    crate::ast::IncrementOperator::Increment => BinaryOperator::Add,
                    crate::ast::IncrementOperator::Decrement => BinaryOperator::Subtract,
                };
                let candidate = constants
                    .get(&key)
                    .copied()
                    .and_then(|value| evaluate_constant_binary(operator, value, one))
                    .filter(|value| constant_matches_type(*value, type_name));
                constants.remove(&key);
                if let Some(value) = candidate {
                    insert_constant_fact(constants, key, value);
                }
            } else {
                apply_expression_effects(&increment.operand, scope, scopes, constants);
            }
        }
        ExpressionKind::Parenthesized(inner)
        | ExpressionKind::Cast(crate::ast::CastExpression { operand: inner, .. })
        | ExpressionKind::Unary(crate::ast::UnaryExpression { operand: inner, .. }) => {
            apply_expression_effects(inner, scope, scopes, constants);
        }
        ExpressionKind::Binary(_)
        | ExpressionKind::Call(_)
        | ExpressionKind::Component(_)
        | ExpressionKind::LazyIndex(_)
        | ExpressionKind::List(_)
        | ExpressionKind::Vector(_)
        | ExpressionKind::Rotation(_) => match side_effect_count(expression) {
            0 => {}
            1 => apply_single_nested_effect(expression, scope, scopes, constants),
            _ => invalidate_written_local_facts(expression, scope, scopes, constants),
        },
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Error(_) => {}
    }
}

fn compound_assignment_binary_operator(operator: AssignmentOperator) -> Option<BinaryOperator> {
    match operator {
        AssignmentOperator::Assign => None,
        AssignmentOperator::AddAssign => Some(BinaryOperator::Add),
        AssignmentOperator::SubtractAssign => Some(BinaryOperator::Subtract),
        AssignmentOperator::MultiplyAssign => Some(BinaryOperator::Multiply),
        AssignmentOperator::DivideAssign => Some(BinaryOperator::Divide),
        AssignmentOperator::RemainderAssign => Some(BinaryOperator::Remainder),
    }
}

fn side_effect_count(expression: &Expression) -> usize {
    let mut budget = MAX_CONSTANT_NODES;
    side_effect_count_at(expression, 0, &mut budget)
}

fn side_effect_count_at(expression: &Expression, depth: usize, budget: &mut usize) -> usize {
    if depth > MAX_CONSTANT_DEPTH || *budget == 0 {
        return usize::MAX;
    }
    *budget -= 1;
    let next_depth = depth + 1;
    let nested = match &expression.kind {
        ExpressionKind::Parenthesized(inner)
        | ExpressionKind::Cast(crate::ast::CastExpression { operand: inner, .. })
        | ExpressionKind::Unary(crate::ast::UnaryExpression { operand: inner, .. }) => {
            side_effect_count_at(inner, next_depth, budget)
        }
        ExpressionKind::Binary(binary) => {
            let left = side_effect_count_at(&binary.left, next_depth, budget);
            if left == usize::MAX {
                usize::MAX
            } else {
                left.saturating_add(side_effect_count_at(&binary.right, next_depth, budget))
            }
        }
        ExpressionKind::Call(call) => {
            let callee = side_effect_count_at(&call.callee, next_depth, budget);
            if callee == usize::MAX {
                usize::MAX
            } else {
                callee.saturating_add(side_effect_count_many(
                    call.arguments.iter(),
                    next_depth,
                    budget,
                ))
            }
        }
        ExpressionKind::Component(component) => {
            side_effect_count_at(&component.base, next_depth, budget)
        }
        ExpressionKind::LazyIndex(lazy) => {
            let base = side_effect_count_at(&lazy.base, next_depth, budget);
            if base == usize::MAX {
                usize::MAX
            } else {
                base.saturating_add(side_effect_count_many(
                    lazy.indices.iter(),
                    next_depth,
                    budget,
                ))
            }
        }
        ExpressionKind::List(list) => {
            side_effect_count_many(list.elements.iter(), next_depth, budget)
        }
        ExpressionKind::Vector(vector) => {
            side_effect_count_many(vector.components.iter(), next_depth, budget)
        }
        ExpressionKind::Rotation(rotation) => {
            side_effect_count_many(rotation.components.iter(), next_depth, budget)
        }
        ExpressionKind::Assignment(assignment) => {
            let target = side_effect_count_at(&assignment.target, next_depth, budget);
            if target == usize::MAX {
                usize::MAX
            } else {
                target.saturating_add(side_effect_count_at(&assignment.value, next_depth, budget))
            }
        }
        ExpressionKind::Increment(increment) => {
            side_effect_count_at(&increment.operand, next_depth, budget)
        }
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Error(_) => 0,
    };
    match &expression.kind {
        ExpressionKind::Assignment(_) | ExpressionKind::Increment(_) => nested.saturating_add(1),
        _ => nested,
    }
}

fn side_effect_count_many<'a>(
    expressions: impl Iterator<Item = &'a Expression>,
    depth: usize,
    budget: &mut usize,
) -> usize {
    let mut count = 0usize;
    for expression in expressions {
        let nested = side_effect_count_at(expression, depth, budget);
        if nested == usize::MAX {
            return usize::MAX;
        }
        count = count.saturating_add(nested);
    }
    count
}

fn apply_single_nested_effect(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &mut ConstantEnvironment,
) {
    let mut apply_child = |child: &Expression| {
        if side_effect_count(child) == 1 {
            apply_expression_effects(child, scope, scopes, constants);
            true
        } else {
            false
        }
    };
    match &expression.kind {
        ExpressionKind::Binary(binary) => {
            if !apply_child(&binary.left) {
                apply_child(&binary.right);
            }
        }
        ExpressionKind::Call(call) => {
            if !apply_child(&call.callee) {
                for argument in &call.arguments {
                    if apply_child(argument) {
                        break;
                    }
                }
            }
        }
        ExpressionKind::Component(component) => {
            apply_child(&component.base);
        }
        ExpressionKind::LazyIndex(lazy) => {
            if !apply_child(&lazy.base) {
                for index in &lazy.indices {
                    if apply_child(index) {
                        break;
                    }
                }
            }
        }
        ExpressionKind::List(list) => {
            for element in &list.elements {
                if apply_child(element) {
                    break;
                }
            }
        }
        ExpressionKind::Vector(vector) => {
            for component in &vector.components {
                if apply_child(component) {
                    break;
                }
            }
        }
        ExpressionKind::Rotation(rotation) => {
            for component in &rotation.components {
                if apply_child(component) {
                    break;
                }
            }
        }
        _ => {}
    }
}

fn invalidate_written_local_facts(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &mut ConstantEnvironment,
) {
    match &expression.kind {
        ExpressionKind::Assignment(assignment) => {
            if let Some((key, _)) = local_numeric_binding(&assignment.target, scope, scopes) {
                constants.remove(&key);
            } else {
                invalidate_written_local_facts(&assignment.target, scope, scopes, constants);
            }
            invalidate_written_local_facts(&assignment.value, scope, scopes, constants);
        }
        ExpressionKind::Increment(increment) => {
            if let Some((key, _)) = local_numeric_binding(&increment.operand, scope, scopes) {
                constants.remove(&key);
            } else {
                invalidate_written_local_facts(&increment.operand, scope, scopes, constants);
            }
        }
        ExpressionKind::Parenthesized(inner)
        | ExpressionKind::Cast(crate::ast::CastExpression { operand: inner, .. })
        | ExpressionKind::Unary(crate::ast::UnaryExpression { operand: inner, .. }) => {
            invalidate_written_local_facts(inner, scope, scopes, constants);
        }
        ExpressionKind::Binary(binary) => {
            invalidate_written_local_facts(&binary.left, scope, scopes, constants);
            invalidate_written_local_facts(&binary.right, scope, scopes, constants);
        }
        ExpressionKind::Call(call) => {
            invalidate_written_local_facts(&call.callee, scope, scopes, constants);
            for argument in &call.arguments {
                invalidate_written_local_facts(argument, scope, scopes, constants);
            }
        }
        ExpressionKind::Component(component) => {
            invalidate_written_local_facts(&component.base, scope, scopes, constants);
        }
        ExpressionKind::LazyIndex(lazy) => {
            invalidate_written_local_facts(&lazy.base, scope, scopes, constants);
            for index in &lazy.indices {
                invalidate_written_local_facts(index, scope, scopes, constants);
            }
        }
        ExpressionKind::List(list) => {
            for element in &list.elements {
                invalidate_written_local_facts(element, scope, scopes, constants);
            }
        }
        ExpressionKind::Vector(vector) => {
            for component in &vector.components {
                invalidate_written_local_facts(component, scope, scopes, constants);
            }
        }
        ExpressionKind::Rotation(rotation) => {
            for component in &rotation.components {
                invalidate_written_local_facts(component, scope, scopes, constants);
            }
        }
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Error(_) => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

fn evaluate_expression_result(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &ConstantEnvironment,
) -> Option<ConstantNumber> {
    let mut budget = MAX_CONSTANT_NODES;
    evaluate_expression_result_at(expression, scope, scopes, constants, 0, &mut budget)
}

fn evaluate_expression_result_at(
    expression: &Expression,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &ConstantEnvironment,
    depth: usize,
    budget: &mut usize,
) -> Option<ConstantNumber> {
    if depth > MAX_CONSTANT_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;

    match &expression.kind {
        ExpressionKind::Parenthesized(inner) => {
            evaluate_expression_result_at(inner, scope, scopes, constants, depth + 1, budget)
        }
        ExpressionKind::Assignment(assignment) => {
            let (key, type_name) = local_numeric_binding(&assignment.target, scope, scopes)?;
            if assignment.operator == AssignmentOperator::Assign
                && expression_writes_local_binding(&assignment.value, key, scope, scopes)
            {
                // Do not infer a final value when the RHS also writes the
                // outer assignment target (for example an assignment of itself post-incremented).
                return None;
            }
            // A nested assignment chain is unambiguous because assignment is
            // right-associative in the AST. Other effectful compound operands
            // remain unknown unless they are themselves a supported scalar
            // assignment/increment result.
            let right = match assignment.operator {
                AssignmentOperator::Assign => evaluate_expression_result_at(
                    &assignment.value,
                    scope,
                    scopes,
                    constants,
                    depth + 1,
                    budget,
                )?,
                _ => {
                    // A compound assignment's LHS read and an effectful RHS
                    // may be unsequenced. Keep it unknown rather than infer a
                    // value that depends on an operand evaluation order.
                    if side_effect_count(&assignment.value) != 0 {
                        return None;
                    }
                    evaluate_constant_number(
                        &assignment.value,
                        scope,
                        scopes,
                        constants,
                        depth + 1,
                        budget,
                    )?
                }
            };
            let result = match assignment.operator {
                AssignmentOperator::Assign => right,
                operator => {
                    let left = constants.get(&key).copied()?;
                    evaluate_constant_binary(
                        compound_assignment_binary_operator(operator)?,
                        left,
                        right,
                    )?
                }
            };
            constant_matches_type(result, type_name).then_some(result)
        }
        ExpressionKind::Increment(increment) => {
            let (key, type_name) = local_numeric_binding(&increment.operand, scope, scopes)?;
            let old = constants.get(&key).copied()?;
            if !increment.prefix {
                return Some(old);
            }
            let one = match type_name {
                "integer" => ConstantNumber::Integer(1),
                "float" => ConstantNumber::Float(1.0),
                _ => return None,
            };
            let operator = match increment.operator {
                crate::ast::IncrementOperator::Increment => BinaryOperator::Add,
                crate::ast::IncrementOperator::Decrement => BinaryOperator::Subtract,
            };
            let result = evaluate_constant_binary(operator, old, one)?;
            constant_matches_type(result, type_name).then_some(result)
        }
        _ => evaluate_constant_number(expression, scope, scopes, constants, depth, budget),
    }
}

fn expression_writes_local_binding(
    expression: &Expression,
    binding: usize,
    scope: ScopeId,
    scopes: &ScopeTree,
) -> bool {
    fn visit(
        expression: &Expression,
        binding: usize,
        scope: ScopeId,
        scopes: &ScopeTree,
        depth: usize,
        budget: &mut usize,
    ) -> bool {
        if depth > MAX_CONSTANT_DEPTH || *budget == 0 {
            // Unknown after a budget/depth limit means that the target may
            // have been written; callers must not use the candidate value.
            return true;
        }
        *budget -= 1;
        let contains = |child: &Expression, budget: &mut usize| {
            visit(child, binding, scope, scopes, depth + 1, budget)
        };
        match &expression.kind {
            ExpressionKind::Assignment(assignment) => {
                local_numeric_binding(&assignment.target, scope, scopes)
                    .is_some_and(|(key, _)| key == binding)
                    || contains(&assignment.target, budget)
                    || contains(&assignment.value, budget)
            }
            ExpressionKind::Increment(increment) => {
                local_numeric_binding(&increment.operand, scope, scopes)
                    .is_some_and(|(key, _)| key == binding)
                    || contains(&increment.operand, budget)
            }
            ExpressionKind::Parenthesized(inner)
            | ExpressionKind::Cast(crate::ast::CastExpression { operand: inner, .. })
            | ExpressionKind::Unary(crate::ast::UnaryExpression { operand: inner, .. }) => {
                contains(inner, budget)
            }
            ExpressionKind::Binary(binary) => {
                contains(&binary.left, budget) || contains(&binary.right, budget)
            }
            ExpressionKind::Call(call) => {
                if contains(&call.callee, budget) {
                    return true;
                }
                for argument in &call.arguments {
                    if contains(argument, budget) {
                        return true;
                    }
                }
                false
            }
            ExpressionKind::Component(component) => contains(&component.base, budget),
            ExpressionKind::LazyIndex(lazy) => {
                if contains(&lazy.base, budget) {
                    return true;
                }
                for index in &lazy.indices {
                    if contains(index, budget) {
                        return true;
                    }
                }
                false
            }
            ExpressionKind::List(list) => list.elements.iter().any(|item| contains(item, budget)),
            ExpressionKind::Vector(vector) => {
                vector.components.iter().any(|item| contains(item, budget))
            }
            ExpressionKind::Rotation(rotation) => rotation
                .components
                .iter()
                .any(|item| contains(item, budget)),
            ExpressionKind::Identifier(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::String(_)
            | ExpressionKind::Error(_) => false,
        }
    }

    let mut budget = MAX_CONSTANT_NODES;
    visit(expression, binding, scope, scopes, 0, &mut budget)
}

fn condition_possibilities(
    expression: &Expression,
    empty_is_true: bool,
    scope: ScopeId,
    scopes: &ScopeTree,
    constants: &ConstantEnvironment,
) -> Option<(bool, bool)> {
    if empty_is_true {
        return Some((true, false));
    }
    if contains_error(expression) {
        return None;
    }

    let Some(value) = evaluate_expression_result(expression, scope, scopes, constants) else {
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
    constants: &ConstantEnvironment,
    depth: usize,
    budget: &mut usize,
) -> Option<ConstantNumber> {
    if depth > MAX_CONSTANT_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;

    match &expression.kind {
        ExpressionKind::Parenthesized(inner) => {
            evaluate_constant_number(inner, scope, scopes, constants, depth + 1, budget)
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
            if let Some(symbol) =
                scopes.lookup_value_at(scope, &identifier.name, identifier.span.start)
            {
                if symbol.kind == SymbolKind::Local
                    && matches!(symbol.type_name.as_deref(), Some("integer" | "float"))
                {
                    return constants.get(&symbol.span.start).copied();
                }
                // Parameters, globals and non-numeric locals are mutable and
                // never become constants merely from their declaration.
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
            let value = evaluate_constant_number(
                &unary.operand,
                scope,
                scopes,
                constants,
                depth + 1,
                budget,
            )?;
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
            let left = evaluate_constant_number(
                &binary.left,
                scope,
                scopes,
                constants,
                depth + 1,
                budget,
            )?;
            let right = evaluate_constant_number(
                &binary.right,
                scope,
                scopes,
                constants,
                depth + 1,
                budget,
            )?;
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
            let value = evaluate_constant_number(
                &cast.operand,
                scope,
                scopes,
                constants,
                depth + 1,
                budget,
            )?;
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
mod constant_environment_tests {
    use std::collections::HashMap;

    use super::{
        insert_constant_fact, is_constant_propagation_disabled, ConstantEnvironment,
        ConstantNumber, MAX_CONSTANT_FACTS,
    };

    #[test]
    fn local_fact_budget_disables_the_environment_without_partial_facts() {
        let mut constants: ConstantEnvironment = HashMap::new();
        for key in 0..MAX_CONSTANT_FACTS {
            insert_constant_fact(&mut constants, key, ConstantNumber::Integer(key as i32));
        }
        assert_eq!(constants.len(), MAX_CONSTANT_FACTS);
        assert!(!is_constant_propagation_disabled(&constants));

        insert_constant_fact(
            &mut constants,
            MAX_CONSTANT_FACTS,
            ConstantNumber::Integer(1),
        );
        assert!(is_constant_propagation_disabled(&constants));
        assert_eq!(constants.len(), 1);
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
    fn local_values_propagate_and_merge_only_when_definite() {
        let source = r#"
            integer initialized_local() {
                integer value = 1;
                if (value == 1) return 1;
            }
            integer assigned_local() {
                integer value;
                value = 2;
                if (value > 1) return 1;
            }
            integer equal_branch_values(integer condition) {
                integer value;
                if (condition) value = 5;
                else value = 5;
                if (value == 5) return 1;
            }
            integer different_branch_values(integer condition) {
                integer value;
                if (condition) value = 1;
                else value = 0;
                if (value == 1) return 1;
            }
            integer terminating_branch(integer condition) {
                integer value = 1;
                if (condition) return 1;
                value = 2;
                if (value == 2) return 1;
            }
            integer parameter_stays_unknown(integer value) {
                if (value) return 1;
            }
            integer mutable_global;
            integer global_stays_unknown() {
                if (mutable_global) return 1;
            }
            integer local_float() {
                float value = 1.5;
                if (value == 1.5) return 1;
            }
            integer self_referential_assignment() {
                integer value = 1;
                value = value + 1;
                if (value == 2) return 1;
            }
            integer assignment_in_condition_updates_local() {
                integer value = 1;
                if ((value = 0)) return 1;
                if (value == 0) return 1;
            }
            integer jump_label_values_stay_unknown() {
                integer value = 0;
                jump after_write;
                value = 1;
                @after_write;
                if (value == 0) return 1;
            }
            integer loop_carried_value_stays_unknown() {
                integer value = 0;
                while (value == 0) {
                    value = 1;
                }
                if (value) return 1;
            }
            integer increment_invalidates_value() {
                integer value = 1;
                ++value;
                if (value == 2) return 1;
            }
            integer compound_assignment_invalidates_value() {
                integer value = 1;
                value += 1;
                if (value == 2) return 1;
            }
            integer parenthesized_assignment_invalidates_value() {
                integer value = 0;
                (value) = 1;
                if (value == 0) return 1;
            }
            integer unordered_side_effects_stay_unknown() {
                integer value = 0;
                (value = 1) + (value = 2);
                if (value == 2) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function different_branch_values may reach the end without returning a value",
                "function parameter_stays_unknown may reach the end without returning a value",
                "function global_stays_unknown may reach the end without returning a value",
                "function loop_carried_value_stays_unknown may reach the end without returning a value",
                "function parenthesized_assignment_invalidates_value may reach the end without returning a value",
                "function unordered_side_effects_stay_unknown may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn declaration_initializers_capture_assignment_and_increment_results() {
        let source = r#"
            integer declaration_captures_postfix_result() {
                integer value = 1;
                integer previous = value++;
                if (previous == 1 && value == 2) return 1;
            }
            integer declaration_captures_compound_assignment_result() {
                integer value = 1;
                integer updated = (value += 2);
                if (updated == 3 && value == 3) return 1;
            }
            integer declaration_captures_nested_assignment_result() {
                integer first = 0;
                integer second = 0;
                integer assigned = (first = second = 4);
                if (assigned == 4 && first == 4 && second == 4) return 1;
            }
            default { state_entry() { } }
        "#;

        assert!(missing_return_functions(source).is_empty());
    }

    #[test]
    fn assignment_does_not_reuse_rhs_write_to_the_same_local() {
        let source = r#"
            integer assignment_from_postfix_same_local_stays_unknown() {
                integer value = 0;
                if (value = value++) return 1;
            }
            integer assignment_from_prefix_same_local_stays_unknown() {
                integer value = 0;
                if (value = ++value) return 1;
            }
            integer assignment_from_other_local_increment_is_known() {
                integer value = 0;
                integer other = 1;
                if (value = other++) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function assignment_from_postfix_same_local_stays_unknown may reach the end without returning a value",
                "function assignment_from_prefix_same_local_stays_unknown may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn right_associative_assignment_chains_update_each_local_and_return_the_value() {
        let source = r#"
            integer chained_assignments_update_both_values() {
                integer first = 0;
                integer second = 0;
                first = second = 3;
                if (first == 3 && second == 3) return 1;
            }
            integer chained_assignment_drives_condition() {
                integer first = 0;
                integer second = 0;
                if (first = second = 1) return 1;
            }
            integer chained_compound_assignment_result() {
                integer first = 1;
                integer second = 2;
                if (first = (second += 1)) return 1;
            }
            default { state_entry() { } }
        "#;

        assert!(missing_return_functions(source).is_empty());
    }

    #[test]
    fn assignment_and_increment_results_drive_condition_edges() {
        let source = r#"
            integer assignment_true_condition() {
                integer value = 0;
                if (value = 1) return 1;
            }
            integer assignment_false_condition() {
                integer value = 1;
                if (value = 0) return 1;
            }
            integer postfix_condition_uses_old_value() {
                integer value = 0;
                if (value++) return 1;
            }
            integer prefix_condition_uses_new_value() {
                integer value = 0;
                if (++value) return 1;
            }
            integer compound_assignment_condition_uses_result() {
                integer value = 0;
                if (value += 1) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function assignment_false_condition may reach the end without returning a value",
                "function postfix_condition_uses_old_value may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn side_effects_update_known_values_and_preserve_unwritten_facts() {
        let source = r#"
            integer increment_updates_value() {
                integer value = 1;
                value++;
                if (value == 2) return 1;
            }
            integer compound_assignment_updates_value() {
                integer value = 1;
                value += 2;
                if (value == 3) return 1;
            }
            integer single_nested_write_updates_value() {
                integer value = 0;
                (value = 2) + 1;
                if (value == 2) return 1;
            }
            integer multiple_writes_preserve_unrelated_value() {
                integer first = 0;
                integer second = 0;
                integer untouched = 5;
                (first = 1) + (second = 2);
                if (untouched == 5) return 1;
            }
            integer multiple_writes_to_same_value_stay_unknown() {
                integer value = 0;
                (value = 1) + (value = 2);
                if (value == 1) return 1;
            }
            integer compound_same_operand_write_stays_unknown() {
                integer value = 0;
                if ((value += (value = 1)) == 1) return 1;
            }
            integer nested_call_argument_write_updates_value() {
                integer value = 0;
                llOwnerSay((string)(value = 2));
                if (value == 2) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function multiple_writes_to_same_value_stay_unknown may reach the end without returning a value",
                "function compound_same_operand_write_stays_unknown may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn worklist_reaches_a_fixed_point_for_loop_carried_values_and_for_clauses() {
        let source = r#"
            integer loop_condition_changes_after_iteration() {
                integer value = 1;
                while (value == 1) {
                    value = 2;
                }
                if (value == 1) return 1;
            }
            integer stable_local_condition_keeps_infinite_loop() {
                integer value = 1;
                while (value == 1) { }
            }
            integer for_initializer_runs_before_condition() {
                integer value = 0;
                for (value = 1; value == 1; ) {
                    return 1;
                }
            }
            integer for_increment_changes_loop_fact() {
                integer value = 0;
                for (; value < 1; value++) { }
                if (value == 0) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function loop_condition_changes_after_iteration may reach the end without returning a value",
                "function for_increment_changes_loop_fact may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn jump_label_joins_merge_all_reachable_incoming_constant_states() {
        let source = r#"
            integer jump_over_write_preserves_the_incoming_value() {
                integer value = 0;
                jump after_write;
                value = 1;
                @after_write;
                if (value == 0) return 1;
            }
            integer jump_and_fallthrough_paths_disagree(integer condition) {
                integer value = 0;
                if (condition) {
                    value = 1;
                    jump joined;
                }
                value = 2;
                @joined;
                if (value == 1) return 1;
            }
            default { state_entry() { } }
        "#;

        assert_eq!(
            missing_return_functions(source),
            vec![
                "function jump_and_fallthrough_paths_disagree may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn local_values_also_prune_unreachable_statement_edges() {
        let source = r#"
            integer local_condition() {
                integer value = 0;
                if (value) {
                    llSay(0, "unreachable");
                }
                return 1;
            }
            default { state_entry() { } }
        "#;
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast(&tokens);
        assert!(syntax.is_empty(), "{syntax:?}");
        let diagnostics = crate::ast_semantic::analyze_ast(&ast, &syntax);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "SEM037"),
            "expected SEM037 for the unreachable local-condition branch, got {diagnostics:?}"
        );
    }

    fn switch_diagnostics(source: &str) -> Vec<Diagnostic> {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) =
            parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
        assert!(syntax.is_empty(), "{syntax:?}");
        crate::ast_semantic::analyze_ast(&ast, &syntax)
    }

    #[test]
    fn constant_switch_values_prune_dispatch_but_preserve_fallthrough_and_unknowns() {
        let source = r#"
            integer known_case_prunes_other_arms() {
                integer selected = 1;
                switch (selected) {
                    case 1: return 1;
                    case 2: llSay(0, "unreachable");
                }
            }
            integer known_case_falls_through() {
                integer selected = 1;
                switch (selected) {
                    case 1: ;
                    case 2: return 2;
                    default: llSay(0, "unreachable");
                }
            }
            integer assignment_switch_condition_prunes_arms() {
                integer selected = 0;
                switch (selected = 1) {
                    case 1: return 1;
                    case 2: llSay(0, "unreachable");
                }
            }
            integer known_value_selects_default() {
                integer selected = 1;
                switch (selected) {
                    case 2: return 2;
                    default: llSay(0, "reachable default");
                }
            }
            integer unmatched_case_keeps_switch_exit() {
                integer selected = 1;
                switch (selected) {
                    case 2: return 2;
                }
            }
            integer unknown_value_keeps_all_arms(integer selected) {
                switch (selected) {
                    case 1: return 1;
                    default: llSay(0, "reachable default");
                }
            }
            integer unknown_case_label_keeps_all_arms() {
                integer selected = 1;
                switch (selected) {
                    case selected: return 1;
                    default: llSay(0, "reachable default for unknown label");
                }
            }
            integer unknown_string_value_keeps_all_arms(string selected) {
                switch (selected) {
                    case "alpha": return 1;
                    default: llSay(0, "reachable default for unknown string");
                }
            }
            default { state_entry() { } }
        "#;

        let diagnostics = switch_diagnostics(source);
        let unreachable_case_phrase = source
            .find("unreachable")
            .expect("unreachable switch statement must exist");
        let unreachable_case_start = source[..unreachable_case_phrase]
            .rfind("llSay")
            .expect("unreachable call start must exist");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "SEM037" && diagnostic.span.start == unreachable_case_start
            }),
            "the non-matching case arm must be unreachable: {diagnostics:?}"
        );

        let missing_returns: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM011")
            .map(|diagnostic| diagnostic.message.as_str())
            .collect();
        assert_eq!(
            missing_returns,
            vec![
                "function known_value_selects_default may reach the end without returning a value",
                "function unmatched_case_keeps_switch_exit may reach the end without returning a value",
                "function unknown_value_keeps_all_arms may reach the end without returning a value",
                "function unknown_case_label_keeps_all_arms may reach the end without returning a value",
                "function unknown_string_value_keeps_all_arms may reach the end without returning a value",
            ]
        );
    }

    #[test]
    fn nonnumeric_constant_switch_values_use_firestorm_equality() {
        let source = r#"
            integer string_switch_dispatches_one_case() {
                switch ("alpha") {
                    case "alpha": return 1;
                    case "beta": llSay(0, "unreachable string arm");
                }
            }
            integer vector_switch_dispatches_one_case() {
                switch (<1.0, 2.0, 3.0>) {
                    case <1.0, 2.0, 3.0>: return 1;
                    case <4.0, 5.0, 6.0>: llSay(0, "unreachable vector arm");
                }
            }
            integer list_switch_uses_length_equality() {
                switch ([1, 2]) {
                    case [3, 4]: return 1;
                    case [5]: llSay(0, "unreachable list arm");
                }
            }
            integer rotation_switch_dispatches_one_case() {
                switch (ZERO_ROTATION) {
                    case <0.0, 0.0, 0.0, 1.0>: return 1;
                    case <0.0, 0.0, 1.0, 0.0>: llSay(0, "unreachable rotation arm");
                }
            }
            default { state_entry() { } }
        "#;

        let diagnostics = switch_diagnostics(source);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "SEM011"),
            "all selected constant switch arms return: {diagnostics:?}"
        );
        for phrase in [
            "unreachable string arm",
            "unreachable vector arm",
            "unreachable list arm",
            "unreachable rotation arm",
        ] {
            let phrase_start = source.find(phrase).expect("test arm must exist");
            let statement_start = source[..phrase_start]
                .rfind("llSay")
                .expect("statement start must exist");
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "SEM037" && diagnostic.span.start == statement_start
                }),
                "expected the constant nonmatching arm {phrase:?} to be unreachable: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn nested_switch_and_loop_breaks_target_the_nearest_switch() {
        let source = r#"
            integer nested_switch_break_stays_inside_outer_switch(integer selected) {
                switch (selected) {
                    case 1:
                        switch (selected) {
                            case 1: break;
                            default: return 2;
                        }
                        return 1;
                    default: return 3;
                }
            }
            integer switch_inside_loop_break_does_not_break_loop() {
                integer selected = 1;
                while (selected) {
                    switch (selected) {
                        case 1: break;
                        default: break;
                    }
                    return 1;
                }
            }
            integer loop_inside_switch_break_exits_switch() {
                integer selected = 1;
                switch (selected) {
                    case 1:
                        while (TRUE) { break; }
                        llSay(0, "unreachable after switch break");
                    default: return 2;
                }
                return 3;
            }
            integer for_inside_switch_break_exits_switch() {
                switch (TRUE) {
                    case TRUE:
                        for (; TRUE; ) { break; }
                        llSay(0, "unreachable after for break");
                    default: return 2;
                }
                return 3;
            }
            integer do_inside_switch_break_exits_switch() {
                switch (TRUE) {
                    case TRUE:
                        do { break; } while (TRUE);
                        llSay(0, "unreachable after do break");
                    default: return 2;
                }
                return 3;
            }
            integer if_inside_switch_break_exits_switch() {
                switch (TRUE) {
                    case TRUE:
                        if (TRUE) { break; }
                        llSay(0, "unreachable after if break");
                    default: return 2;
                }
                return 3;
            }
            integer switch_inside_for_break_keeps_following_return() {
                integer selected = 1;
                for (; selected; selected = 0) {
                    switch (selected) {
                        case 1: break;
                        default: break;
                    }
                    return 1;
                }
            }
            integer switch_inside_do_break_keeps_following_return() {
                integer selected = 1;
                do {
                    switch (selected) {
                        case 1: break;
                        default: break;
                    }
                    return 1;
                } while (FALSE);
            }
            default { state_entry() { } }
        "#;

        let diagnostics = switch_diagnostics(source);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "SEM011"),
            "unexpected missing-return diagnostic: {diagnostics:?}"
        );
        let phrase = source
            .find("unreachable after switch break")
            .expect("test statement must exist");
        let unreachable_after_break = source[..phrase]
            .rfind("llSay")
            .expect("call start must exist");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "SEM037" && diagnostic.span.start == unreachable_after_break
            }),
            "expected SEM037 on the statement after a break inside a loop nested in a switch: {diagnostics:?}"
        );
        for marker in [
            "                        return 1;",
            "                    return 1;",
        ] {
            let return_start = source.find(marker).expect("reachable return must exist");
            assert!(
                diagnostics.iter().all(|diagnostic| {
                    diagnostic.code != "SEM037" || diagnostic.span.start != return_start
                }),
                "a break in a nested switch must not hide the following return: {diagnostics:?}"
            );
        }
        for phrase in [
            "unreachable after switch break",
            "unreachable after for break",
            "unreachable after do break",
            "unreachable after if break",
        ] {
            let phrase_start = source.find(phrase).expect("unreachable call must exist");
            let call_start = source[..phrase_start]
                .rfind("llSay")
                .expect("call start must exist");
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "SEM037" && diagnostic.span.start == call_start
                }),
                "expected SEM037 at {phrase:?}: {diagnostics:?}"
            );
        }
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

        let source = r#"
            integer switch_condition_assignment() {
                integer value = 0;
                switch (value = 1) {
                    case 1: break;
                    default: return 0;
                }
                if (value == 1) return 1;
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
            "switch condition side effect was not propagated: {diagnostics:?}"
        );
    }
}
