//! SEM044: named states with no transition path from `default`.

use std::collections::{HashMap, VecDeque};

use crate::ast::{Ast, Block, Statement, StatementKind, SwitchLabel, TopLevel};
use crate::builtins::Signature;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

const MAX_STATE_GRAPH_WORK: usize = 32_768;
const MAX_STATE_GRAPH_DEPTH: usize = 128;
const MAX_STATE_GRAPH_STATES: usize = 4_096;
const GOD_ONLY_BUILTIN_FUNCTIONS: &[&str] = &[
    "llGodLikeRezObject",
    "llSetInventoryPermMask",
    "llSetObjectPermMask",
];

/// Warn about populated named states that cannot be reached from `default`
/// through state-change statements in event handlers.
///
/// The graph deliberately over-approximates control flow: every transition in
/// an event body is considered possible, including transitions in conditional
/// and otherwise unreachable branches. This can hide a finding, but cannot
/// create one merely because a condition or branch was guessed incorrectly.
pub(crate) fn check_with_builtins(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
    builtin_functions: &HashMap<String, Signature>,
) -> Vec<Diagnostic> {
    if ast.items.len() > MAX_STATE_GRAPH_WORK
        || !environment_reliable
        || syntax_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Vec::new();
    }

    let mut states = Vec::new();
    let mut indices = HashMap::new();
    let mut default_index = None;
    let mut work = 0usize;

    // A state that collides with a global, function or a canonical built-in
    // covered by SEM021 is already invalid. Its identity cannot be used safely
    // as a graph node, so suppress the whole reachability result.
    let mut conflicting_names = HashMap::new();
    for item in &ast.items {
        work = work.saturating_add(1);
        if work > MAX_STATE_GRAPH_WORK {
            return Vec::new();
        }
        match item {
            TopLevel::Global(global) => {
                for declarator in &global.declarators {
                    work = work.saturating_add(1);
                    if work > MAX_STATE_GRAPH_WORK {
                        return Vec::new();
                    }
                    if !declarator.name.is_empty() {
                        conflicting_names.insert(declarator.name.as_str(), ());
                    }
                }
            }
            TopLevel::Function(function)
                if function.body.is_some() && !function.name.is_empty() =>
            {
                conflicting_names.insert(function.name.as_str(), ());
            }
            _ => {}
        }
    }
    for function in &ast.recovered_functions {
        work = work.saturating_add(1);
        if work > MAX_STATE_GRAPH_WORK {
            return Vec::new();
        }
        conflicting_names.insert(function.name.as_str(), ());
    }

    for item in &ast.items {
        let TopLevel::State(state) = item else {
            if matches!(item, TopLevel::Error(_)) {
                return Vec::new();
            }
            continue;
        };

        work = work.saturating_add(1);
        if work > MAX_STATE_GRAPH_WORK
            || states.len() >= MAX_STATE_GRAPH_STATES
            || state.body_missing
            || !state.body_closed
            || state.name_missing
            || (!state.is_default && state.name.is_empty())
        {
            return Vec::new();
        }
        for event in &state.events {
            work = work.saturating_add(1);
            if work > MAX_STATE_GRAPH_WORK
                || event.name.is_empty()
                || event.body.as_ref().map_or(true, |body| !body.closed)
            {
                return Vec::new();
            }
        }

        let name = if state.is_default {
            if default_index.is_some() {
                return Vec::new();
            }
            "default".to_owned()
        } else {
            if conflicting_names.contains_key(state.name.as_str())
                || (builtin_functions.contains_key(state.name.as_str())
                    && !GOD_ONLY_BUILTIN_FUNCTIONS.contains(&state.name.as_str()))
            {
                return Vec::new();
            }
            state.name.clone()
        };
        let index = states.len();
        if indices.insert(name, index).is_some() {
            // Duplicate state names are diagnosed elsewhere (SEM021); their
            // targets cannot be used as unique graph nodes.
            return Vec::new();
        }
        if state.is_default {
            default_index = Some(index);
        }
        states.push(state);
    }

    let Some(default_index) = default_index else {
        return Vec::new();
    };
    // The default state must itself be a valid starting point. SEM038 owns the
    // empty-state error; do not cascade from an empty default state.
    if states[default_index].events.is_empty() {
        return Vec::new();
    }

    let mut graph = vec![Vec::new(); states.len()];
    for (source_index, state) in states.iter().enumerate() {
        for event in &state.events {
            let Some(body) = &event.body else {
                return Vec::new();
            };
            if !collect_transitions(body, source_index, &indices, &mut graph, &mut work) {
                // Never emit findings from a partial graph.
                return Vec::new();
            }
        }
    }

    let mut reachable = vec![false; states.len()];
    let mut pending = VecDeque::from([default_index]);
    reachable[default_index] = true;
    while let Some(source) = pending.pop_front() {
        for &target in &graph[source] {
            if !reachable[target] {
                reachable[target] = true;
                pending.push_back(target);
            }
        }
    }

    states
        .iter()
        .enumerate()
        .filter_map(|(index, state)| {
            if reachable[index] || state.is_default || state.events.is_empty() {
                return None;
            }
            Some(Diagnostic {
                code: "SEM044",
                severity: Severity::Warning,
                kind: DiagnosticKind::Lint,
                message: format!(
                    "state '{}' has no transition path from the default state",
                    state.name
                ),
                span: state.name_span,
            })
        })
        .collect()
}

fn collect_transitions(
    body: &Block,
    source_index: usize,
    state_indices: &HashMap<String, usize>,
    graph: &mut [Vec<usize>],
    work: &mut usize,
) -> bool {
    if !body.closed {
        return false;
    }

    let mut pending: Vec<(&Statement, usize)> = body
        .statements
        .iter()
        .rev()
        .map(|statement| (statement, 1))
        .collect();

    while let Some((statement, depth)) = pending.pop() {
        *work = work.saturating_add(1);
        if *work > MAX_STATE_GRAPH_WORK || depth > MAX_STATE_GRAPH_DEPTH {
            return false;
        }

        match &statement.kind {
            StatementKind::StateChange(change) => {
                if let Some(&target_index) = state_indices.get(&change.state) {
                    graph[source_index].push(target_index);
                }
            }
            StatementKind::Block(block) => {
                if !block.closed {
                    return false;
                }
                pending.extend(
                    block
                        .statements
                        .iter()
                        .rev()
                        .map(|child| (child, depth + 1)),
                );
            }
            StatementKind::If(if_statement) => {
                pending.push((&if_statement.then_branch, depth + 1));
                if let Some(else_branch) = &if_statement.else_branch {
                    pending.push((else_branch, depth + 1));
                }
            }
            StatementKind::For(for_statement) => {
                pending.push((&for_statement.body, depth + 1));
            }
            StatementKind::While(while_statement) => {
                pending.push((&while_statement.body, depth + 1));
            }
            StatementKind::DoWhile(do_while) => {
                pending.push((&do_while.body, depth + 1));
            }
            StatementKind::Switch(switch) => {
                for arm in switch.arms.iter().rev() {
                    *work = work.saturating_add(1);
                    if *work > MAX_STATE_GRAPH_WORK || matches!(arm.label, SwitchLabel::Error(_)) {
                        return false;
                    }
                    pending.extend(arm.statements.iter().rev().map(|child| (child, depth + 1)));
                }
            }
            StatementKind::Error(_) => return false,
            StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::Return(_)
            | StatementKind::Jump(_)
            | StatementKind::Label(_)
            | StatementKind::Expression(_)
            | StatementKind::Break(_) => {}
        }
    }

    true
}
