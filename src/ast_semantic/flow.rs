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
                    }
                }
                (true, true)
            };

            for edge in self.successors.get(node)? {
                let allowed = match edge.condition {
                    EdgeCondition::Always => true,
                    EdgeCondition::True => may_true,
                    EdgeCondition::False => may_false,
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
    let mut has_default = false;

    for arm in &switch.arms {
        if matches!(arm.label, crate::ast::SwitchLabel::Error(_)) {
            graph.failed = true;
            return None;
        }
        has_default |= matches!(arm.label, crate::ast::SwitchLabel::Default);
        let mut arm_constants = empty_constant_environment_like(constants);
        arms.push(build_statements(
            &arm.statements,
            switch_scope,
            scopes,
            graph,
            Some(switch_exit),
            depth + 1,
            &mut arm_constants,
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
    if !graph.set_transfer(
        dispatch,
        FlowTransfer::Expression {
            expression: switch.condition.clone(),
            scope: switch_scope,
        },
    ) {
        return None;
    }
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
    clear_constant_facts(constants);
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

fn contains_side_effects(expression: &Expression) -> bool {
    match &expression.kind {
        ExpressionKind::Assignment(_) | ExpressionKind::Increment(_) => true,
        ExpressionKind::Parenthesized(inner)
        | ExpressionKind::Cast(crate::ast::CastExpression { operand: inner, .. })
        | ExpressionKind::Unary(crate::ast::UnaryExpression { operand: inner, .. }) => {
            contains_side_effects(inner)
        }
        ExpressionKind::Binary(binary) => {
            contains_side_effects(&binary.left) || contains_side_effects(&binary.right)
        }
        ExpressionKind::Call(call) => {
            contains_side_effects(&call.callee) || call.arguments.iter().any(contains_side_effects)
        }
        ExpressionKind::Component(component) => contains_side_effects(&component.base),
        ExpressionKind::LazyIndex(lazy) => {
            contains_side_effects(&lazy.base) || lazy.indices.iter().any(contains_side_effects)
        }
        ExpressionKind::List(list) => list.elements.iter().any(contains_side_effects),
        ExpressionKind::Vector(vector) => vector.components.iter().any(contains_side_effects),
        ExpressionKind::Rotation(rotation) => rotation.components.iter().any(contains_side_effects),
        ExpressionKind::Identifier(_)
        | ExpressionKind::Integer(_)
        | ExpressionKind::Float(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Error(_) => false,
    }
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
        let candidate = {
            let mut budget = MAX_CONSTANT_NODES;
            evaluate_constant_number(initializer, scope, scopes, constants, 0, &mut budget)
        };
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
    if contains_error(expression) {
        // Recovered expressions can retain only part of their original side
        // effects. Do not carry facts past a syntactically damaged expression.
        clear_constant_facts(constants);
        return;
    }
    match &expression.kind {
        ExpressionKind::Assignment(assignment) => {
            let binding = local_numeric_binding(&assignment.target, scope, scopes);
            // For `x = x + 1`, read the old value before replacing it. The
            // constant evaluator rejects RHS expressions with their own side
            // effects, so this candidate cannot hide a nested write.
            let candidate = if assignment.operator == AssignmentOperator::Assign {
                let mut budget = MAX_CONSTANT_NODES;
                evaluate_constant_number(
                    &assignment.value,
                    scope,
                    scopes,
                    constants,
                    0,
                    &mut budget,
                )
            } else {
                None
            };
            if binding.is_none() {
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
            if let Some((key, _)) = local_numeric_binding(&increment.operand, scope, scopes) {
                constants.remove(&key);
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
        | ExpressionKind::Rotation(_) => {
            if contains_side_effects(expression) {
                // Operand/argument evaluation order is not assumed when a
                // compound expression contains multiple writes.
                clear_constant_facts(constants);
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

    let mut budget = MAX_CONSTANT_NODES;
    let Some(value) =
        evaluate_constant_number(expression, scope, scopes, constants, 0, &mut budget)
    else {
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
                "function increment_invalidates_value may reach the end without returning a value",
                "function compound_assignment_invalidates_value may reach the end without returning a value",
                "function parenthesized_assignment_invalidates_value may reach the end without returning a value",
                "function unordered_side_effects_stay_unknown may reach the end without returning a value",
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
