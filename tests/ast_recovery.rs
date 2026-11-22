use neolint::ast::{
    parse_ast, StatementKind, TopLevel, AST_RECURSION_LIMIT, MAX_AST_RECURSION_DEPTH,
};
use neolint::{lex, Diagnostic, DiagnosticKind, Severity};

fn parse(source: &str) -> (neolint::ast::Ast, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    parse_ast(&tokens)
}

fn local_names(block: &neolint::ast::Block) -> Vec<&str> {
    block
        .statements
        .iter()
        .filter_map(|statement| {
            if let StatementKind::Local(local) = &statement.kind {
                local
                    .declarators
                    .first()
                    .map(|declarator| declarator.name.as_str())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn malformed_event_keeps_later_event_and_state() {
    let (ast, diagnostics) =
        parse("default { broken( { } state_entry() { } } state running { state_entry() { } }");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PAR003"));
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("default")
    };
    assert!(default
        .events
        .iter()
        .any(|event| event.name == "state_entry"));
    assert!(ast
        .items
        .iter()
        .any(|item| { matches!(item, TopLevel::State(state) if state.name == "running") }));
}

#[test]
fn malformed_function_keeps_later_state() {
    let (ast, diagnostics) = parse(
        "integer broken( { } default { state_entry() { } } state later { state_entry() { } }",
    );
    assert!(!diagnostics.is_empty());
    assert!(ast
        .items
        .iter()
        .any(|item| { matches!(item, TopLevel::State(state) if state.name == "default") }));
    assert!(ast
        .items
        .iter()
        .any(|item| { matches!(item, TopLevel::State(state) if state.name == "later") }));
}

#[test]
fn malformed_statement_keeps_later_independent_statement() {
    let (ast, diagnostics) =
        parse("default { state_entry() { integer first = ; integer later = 2; } }");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PAR006"));
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    let body = &default.events[0].body;
    assert!(body
        .statements
        .iter()
        .any(|statement| matches!(statement.kind, StatementKind::Error(_))));
    assert!(local_names(body).contains(&"later"));
}

#[test]
fn malformed_expression_keeps_later_events_and_statements() {
    let (ast, diagnostics) = parse(
        "default { state_entry() { integer value = 1 + ; } touch_start(integer n) { integer later = 2; } }",
    );
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PAR005"));
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    assert_eq!(default.events.len(), 2);
    assert_eq!(
        default.events[1]
            .body
            .statements
            .iter()
            .filter(|statement| matches!(statement.kind, StatementKind::Local(_)))
            .count(),
        1
    );
}

#[test]
fn extra_delimiters_are_recovered_without_panics() {
    for source in [
        "default { state_entry() { list values = [1, 2]]; integer later = 1; } }",
        "default { state_entry() { vector value = <1, 2, 3>>; integer later = 1; } }",
        "default { state_entry() { integer value = (1)); integer later = 1; } }",
        "default { state_entry() { } } }",
    ] {
        let (ast, diagnostics) = parse(source);
        assert!(!diagnostics.is_empty(), "{source:?}");
        assert!(!ast.items.is_empty(), "{source:?}");
    }
}

#[test]
fn empty_token_slice_is_safe() {
    let (ast, diagnostics) = parse_ast(&[]);
    assert_eq!(ast.items.len(), 0);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PAR013"));
}

fn paren_source(depth: usize) -> String {
    format!(
        "default {{ state_entry() {{ integer nested = {}1{}; integer later = 2; }} }}",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

fn unary_source(depth: usize) -> String {
    format!(
        "default {{ state_entry() {{ integer nested = {}1; integer later = 2; }} }}",
        "!".repeat(depth)
    )
}

fn assert_depth_boundary(source: String, expect_limit: bool) {
    let (ast, diagnostics) = parse(&source);
    let recursion: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == AST_RECURSION_LIMIT)
        .collect();
    if expect_limit {
        assert_eq!(recursion.len(), 1, "{diagnostics:?}");
        assert_eq!(recursion[0].severity, Severity::Error);
        assert_eq!(recursion[0].kind, DiagnosticKind::Syntax);
        assert!(recursion[0].message.contains("recursion"));
    } else {
        assert!(recursion.is_empty(), "{diagnostics:?}");
    }
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    let body = &default.events[0].body;
    assert!(local_names(body).contains(&"later"));
}

#[test]
fn expression_depth_boundaries_are_255_256_257() {
    assert_depth_boundary(paren_source(MAX_AST_RECURSION_DEPTH - 1), false);
    assert_depth_boundary(paren_source(MAX_AST_RECURSION_DEPTH), false);
    assert_depth_boundary(paren_source(MAX_AST_RECURSION_DEPTH + 1), true);
    assert_depth_boundary(unary_source(MAX_AST_RECURSION_DEPTH), false);
    assert_depth_boundary(unary_source(MAX_AST_RECURSION_DEPTH + 1), true);
}

#[test]
fn extreme_expression_depth_reports_once_and_continues() {
    let source = format!(
        "default {{ state_entry() {{ integer a = {}1{}; integer b = {}2{}; integer later = 3; }} }}",
        "(".repeat(10_000),
        ")".repeat(10_000),
        "(".repeat(10_000),
        ")".repeat(10_000)
    );
    let (ast, diagnostics) = parse(&source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == AST_RECURSION_LIMIT)
            .count(),
        1
    );
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    assert!(local_names(&default.events[0].body).contains(&"later"));
}

#[test]
fn deep_blocks_are_bounded_and_later_events_survive() {
    let mut body = String::new();
    for _ in 0..MAX_AST_RECURSION_DEPTH + 20 {
        body.push_str("{ ");
    }
    body.push_str("; ");
    for _ in 0..MAX_AST_RECURSION_DEPTH + 20 {
        body.push_str("} ");
    }
    let source = format!(
        "default {{ state_entry() {{ {body} }} touch_start(integer n) {{ integer later = 1; }} }}"
    );
    let (ast, diagnostics) = parse(&source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == AST_RECURSION_LIMIT)
            .count(),
        1
    );
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    assert!(default
        .events
        .iter()
        .any(|event| event.name == "touch_start"));
}

#[test]
fn recovered_expression_nodes_are_explicit_errors() {
    let (ast, _) = parse("default { state_entry() { integer value = ; integer later = 2; } }");
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    let body = &default.events[0].body;
    assert!(body
        .statements
        .iter()
        .any(|statement| matches!(statement.kind, StatementKind::Error(_))));
    assert!(local_names(body).contains(&"later"));
}
