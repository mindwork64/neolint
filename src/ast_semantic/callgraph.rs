//! Bounded AST-native call graph for SEM026.
//!
//! Nodes are user functions with a complete declaration: a body, a non-empty
//! name and the first declaration for that name. Edges are matched direct
//! identifier calls from a function body. Roots are matched direct identifier
//! calls in event bodies and global initializers. Built-in callees and indirect
//! calls never create edges, and reachability is never inferred from spans.
//!
//! The graph is built iteratively with checked node, edge and work counters.
//! Any overflow returns a [`Reachability::suppressed`] result instead of a
//! partial graph, so SEM026 can emit nothing rather than report the remainder of
//! an incomplete analysis as unreachable.

use std::collections::HashMap;

use crate::ast::{Ast, ExpressionKind, FunctionDeclaration, TopLevel};
#[cfg(test)]
use crate::builtins;
use crate::builtins::Signature;
use crate::diagnostic::Span;

use super::calls::{CallOwner, CallSite, FunctionId};
use super::types::UserFunctionSignature;

/// Maximum number of user-function nodes retained in one call graph.
const MAX_CALL_GRAPH_NODES: usize = 4_096;
/// Maximum number of unique call edges retained in one call graph.
const MAX_CALL_GRAPH_EDGES: usize = 8_192;
/// Maximum number of node, call-site and reachability steps in one graph run.
const MAX_CALL_GRAPH_WORK: usize = 16_384;

/// One user function that no root can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnreachableFunction {
    /// Declaration identity: the owning function's name-span start.
    pub(crate) id: FunctionId,
    /// Declared function name.
    pub(crate) name: String,
    /// Declaration name span.
    pub(crate) name_span: Span,
}

/// Result of one call-graph reachability query.
pub(crate) struct Reachability {
    /// Unreachable user functions in declaration order.
    pub(crate) unreachable: Vec<UnreachableFunction>,
    /// Whether a graph budget overflow made the result incomplete.
    ///
    /// A suppressed result carries no unreachable functions; callers must emit
    /// nothing instead of treating the empty list as full reachability.
    pub(crate) suppressed: bool,
}

/// Checked resource limits for one call-graph run.
#[derive(Debug, Clone, Copy)]
struct Limits {
    nodes: usize,
    edges: usize,
    work: usize,
}

const PRODUCTION_LIMITS: Limits = Limits {
    nodes: MAX_CALL_GRAPH_NODES,
    edges: MAX_CALL_GRAPH_EDGES,
    work: MAX_CALL_GRAPH_WORK,
};

fn suppressed() -> Reachability {
    Reachability {
        unreachable: Vec::new(),
        suppressed: true,
    }
}

/// Compute which user functions no event handler or global initializer reaches.
pub(crate) fn unreachable_functions_with_builtins(
    ast: &Ast,
    calls: &[CallSite<'_>],
    functions: &HashMap<String, UserFunctionSignature>,
    builtin_functions: &HashMap<String, Signature>,
) -> Reachability {
    build(ast, calls, functions, builtin_functions, PRODUCTION_LIMITS)
}

fn build(
    ast: &Ast,
    calls: &[CallSite<'_>],
    functions: &HashMap<String, UserFunctionSignature>,
    builtin_functions: &HashMap<String, Signature>,
    limits: Limits,
) -> Reachability {
    let mut work = 0usize;

    let mut candidates: Vec<&FunctionDeclaration> = ast
        .items
        .iter()
        .filter_map(|item| match item {
            TopLevel::Function(function)
                if function.body.is_some() && !function.name.is_empty() =>
            {
                Some(function)
            }
            _ => None,
        })
        .collect();
    candidates.sort_by_key(|function| function.name_span.start);

    let mut nodes: Vec<UnreachableFunction> = Vec::new();
    let mut index_by_name: HashMap<&str, usize> = HashMap::new();
    let mut index_by_id: HashMap<FunctionId, usize> = HashMap::new();
    for function in candidates {
        // First-definition policy: the earliest declaration of a name wins, and
        // the shared signature map is the authority for which one that is.
        if index_by_name.contains_key(function.name.as_str()) {
            continue;
        }
        let Some(signature) = functions.get(function.name.as_str()) else {
            continue;
        };
        if signature.declaration_span.start != function.name_span.start {
            continue;
        }
        work = work.saturating_add(1);
        if work > limits.work || nodes.len() >= limits.nodes {
            return suppressed();
        }
        let index = nodes.len();
        index_by_name.insert(function.name.as_str(), index);
        index_by_id.insert(function.name_span.start, index);
        nodes.push(UnreachableFunction {
            id: function.name_span.start,
            name: function.name.clone(),
            name_span: function.name_span,
        });
    }

    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut roots: Vec<usize> = Vec::new();
    let mut edge_count = 0usize;
    for site in calls {
        work = work.saturating_add(1);
        if work > limits.work {
            return suppressed();
        }
        if !site.call.matched_close {
            continue;
        }
        let ExpressionKind::Identifier(callee) = &site.call.callee.kind else {
            continue;
        };
        // The existing call analyzer resolves canonical built-ins independently
        // of the user-function signature map. A same-named user definition must
        // not become reachable merely because a built-in call uses that spelling.
        if builtin_functions.contains_key(callee.name.as_str()) {
            continue;
        }
        let Some(&target) = index_by_name.get(callee.name.as_str()) else {
            continue;
        };
        match site.owner {
            CallOwner::Function(owner) => {
                let Some(&from) = index_by_id.get(&owner) else {
                    continue;
                };
                let edges = &mut adjacency[from];
                if edges.contains(&target) {
                    continue;
                }
                if edge_count >= limits.edges {
                    return suppressed();
                }
                edge_count += 1;
                edges.push(target);
            }
            CallOwner::EventHandler | CallOwner::GlobalInitializer => {
                if !roots.contains(&target) {
                    roots.push(target);
                }
            }
        }
    }

    let mut visited = vec![false; nodes.len()];
    let mut pending = roots;
    while let Some(index) = pending.pop() {
        work = work.saturating_add(1);
        if work > limits.work {
            return suppressed();
        }
        if visited[index] {
            continue;
        }
        visited[index] = true;
        pending.extend(adjacency[index].iter().copied());
    }

    let unreachable = nodes
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !visited[*index])
        .map(|(_, function)| function)
        .collect();
    Reachability {
        unreachable,
        suppressed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parse_ast;
    use crate::ast_semantic::calls::collect_calls;
    use crate::ast_semantic::scopes::ScopeTree;
    use crate::ast_semantic::types::collect_user_functions;
    use crate::lex;

    fn parsed(source: &str) -> Ast {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast(&tokens);
        assert!(syntax.is_empty(), "{syntax:?}");
        ast
    }

    fn graph(source: &str, limits: Limits) -> Reachability {
        let ast = parsed(source);
        let scopes = ScopeTree::build(&ast);
        let calls = collect_calls(&ast, &scopes);
        let functions = collect_user_functions(&ast);
        build(
            &ast,
            &calls,
            &functions,
            &builtins::definitions().functions,
            limits,
        )
    }

    fn names(reachability: &Reachability) -> Vec<&str> {
        reachability
            .unreachable
            .iter()
            .map(|function| function.name.as_str())
            .collect()
    }

    const LIMITS: Limits = Limits {
        nodes: 64,
        edges: 128,
        work: 1_024,
    };

    #[test]
    fn node_budget_overflow_suppresses_the_whole_graph() {
        let result = graph(
            "alpha() { }\nbeta() { }\ndefault { state_entry() { } }\n",
            Limits { nodes: 1, ..LIMITS },
        );
        assert!(result.suppressed);
        assert!(result.unreachable.is_empty());
    }

    #[test]
    fn edge_budget_overflow_suppresses_the_whole_graph() {
        let result = graph(
            "alpha() { beta(); }\nbeta() { }\ndefault { state_entry() { alpha(); } }\n",
            Limits { edges: 0, ..LIMITS },
        );
        assert!(result.suppressed);
        assert!(result.unreachable.is_empty());
    }

    #[test]
    fn work_budget_overflow_suppresses_the_whole_graph() {
        let result = graph(
            "alpha() { }\ndefault { state_entry() { } }\n",
            Limits { work: 0, ..LIMITS },
        );
        assert!(result.suppressed);
        assert!(result.unreachable.is_empty());
    }

    #[test]
    fn unreachable_nodes_are_reported_in_declaration_order() {
        let result = graph(
            "alpha() { }\nbeta() { }\ndefault { state_entry() { } }\n",
            LIMITS,
        );
        assert!(!result.suppressed);
        assert_eq!(names(&result), vec!["alpha", "beta"]);
        assert!(result
            .unreachable
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id));
    }

    #[test]
    fn roots_reach_callees_across_event_and_global_owners() {
        let result = graph(
            "integer g = global_helper();\nevent_helper() { }\nglobal_helper() { return 1; }\ndefault { custom_event() { event_helper(); } }\n",
            LIMITS,
        );
        assert!(!result.suppressed);
        assert!(names(&result).is_empty(), "{:?}", names(&result));
    }

    #[test]
    fn canonical_builtin_calls_do_not_reach_same_named_user_functions() {
        let result = graph(
            "llSay() { }\ndefault { state_entry() { llSay(); } }\n",
            LIMITS,
        );
        assert!(!result.suppressed);
        assert_eq!(names(&result), vec!["llSay"]);
    }

    #[test]
    fn calls_to_builtin_named_functions_do_not_reach_their_callees() {
        let result = graph(
            "helper() { }\nllSay() { helper(); }\ndefault { state_entry() { llSay(); } }\n",
            LIMITS,
        );
        assert!(!result.suppressed);
        assert_eq!(names(&result), vec!["helper", "llSay"]);
    }

    #[test]
    fn duplicate_names_keep_only_the_first_definition_as_a_node() {
        let result = graph(
            "duplicate() { }\nduplicate() { }\ndefault { state_entry() { } }\n",
            LIMITS,
        );
        assert!(!result.suppressed);
        assert_eq!(result.unreachable.len(), 1);
        assert_eq!(result.unreachable[0].name, "duplicate");
        assert_eq!(result.unreachable[0].id, 0);
    }

    #[test]
    fn matched_close_guard_prevents_edges_and_roots() {
        let source = "helper() { }\ndefault { state_entry() { helper(); } }\n";
        let mut ast = parsed(source);
        let mut mutated = 0usize;
        for item in &mut ast.items {
            let TopLevel::State(state) = item else {
                continue;
            };
            for event in &mut state.events {
                let Some(body) = &mut event.body else {
                    continue;
                };
                for statement in &mut body.statements {
                    let crate::ast::StatementKind::Expression(expression) = &mut statement.kind
                    else {
                        continue;
                    };
                    let ExpressionKind::Call(call) = &mut expression.expression.kind else {
                        continue;
                    };
                    call.matched_close = false;
                    mutated += 1;
                }
            }
        }
        assert_eq!(mutated, 1);
        let scopes = ScopeTree::build(&ast);
        let calls = collect_calls(&ast, &scopes);
        let functions = collect_user_functions(&ast);
        let result = build(
            &ast,
            &calls,
            &functions,
            &builtins::definitions().functions,
            LIMITS,
        );
        assert!(!result.suppressed);
        assert_eq!(names(&result), vec!["helper"]);
    }

    #[test]
    fn functions_without_bodies_and_recovered_signatures_are_not_nodes() {
        let source = "broken() default { state_entry() { } }\n";
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty());
        let (ast, syntax) = parse_ast(&tokens);
        assert!(!syntax.is_empty());
        assert!(ast
            .items
            .iter()
            .any(|item| matches!(item, TopLevel::Function(function) if function.body.is_none())));
        let scopes = ScopeTree::build(&ast);
        let calls = collect_calls(&ast, &scopes);
        let functions = collect_user_functions(&ast);
        let result = build(
            &ast,
            &calls,
            &functions,
            &builtins::definitions().functions,
            LIMITS,
        );
        assert!(!result.suppressed);
        assert!(result.unreachable.is_empty());
    }
}
