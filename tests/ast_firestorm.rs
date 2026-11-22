use neolint::ast::{
    parse_ast, parse_ast_with_options, Ast, AstParseOptions, ExpressionKind, StatementKind,
    SwitchLabel, TopLevel,
};
use neolint::{lex, AnalysisRun, Diagnostic, PreprocessSettings};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

fn temporary_directory() -> PathBuf {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("neolint-firestorm-{}-{id}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn analyze(source: &str, settings: PreprocessSettings) -> (Ast, Vec<Diagnostic>, bool, bool) {
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_text("root.lsl", source, settings)
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    let switches = prepared.switches_enabled();
    let lazy_lists = prepared.lazy_lists_enabled();
    let (tokens, _lexical) = lex(prepared.text());
    let (ast, diagnostics) =
        parse_ast_with_options(&tokens, AstParseOptions::from_flags(switches, lazy_lists));
    (ast, diagnostics, switches, lazy_lists)
}

fn parse_lsl(source: &str) -> (Ast, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    parse_ast(&tokens)
}

fn parse_switches(source: &str) -> (Ast, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true))
}

fn parse_lazy(source: &str) -> (Ast, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    parse_ast_with_options(
        &tokens,
        AstParseOptions::lsl().with_lazy_lists_enabled(true),
    )
}

fn event_body(ast: &Ast) -> &neolint::ast::Block {
    let state = ast
        .items
        .iter()
        .find_map(|item| {
            if let TopLevel::State(state) = item {
                Some(state)
            } else {
                None
            }
        })
        .expect("state");
    state.events[0].body.as_ref().unwrap()
}

fn switch_statements(ast: &Ast) -> Vec<&neolint::ast::SwitchStatement> {
    event_body(ast)
        .statements
        .iter()
        .filter_map(|statement| {
            if let StatementKind::Switch(switch) = &statement.kind {
                Some(switch)
            } else {
                None
            }
        })
        .collect()
}

fn switch_program(body: &str) -> String {
    format!("default {{ state_entry() {{ {body} }} }}")
}

#[test]
fn switches_and_lazy_lists_are_independently_gated() {
    let source = "default { state_entry() { integer x; } }";
    let cases = [
        (PreprocessSettings::identity(), false, false),
        (PreprocessSettings::full(), false, false),
        (PreprocessSettings::firestorm(), false, false),
        (
            PreprocessSettings::firestorm().with_switches_enabled(true),
            true,
            false,
        ),
        (
            PreprocessSettings::firestorm().with_lazy_lists_enabled(true),
            false,
            true,
        ),
        (
            PreprocessSettings::firestorm()
                .with_switches_enabled(true)
                .with_lazy_lists_enabled(true),
            true,
            true,
        ),
        (
            PreprocessSettings::full().with_switches_enabled(true),
            false,
            false,
        ),
    ];
    for (settings, switches, lazy_lists) in cases {
        let (_, _, actual_switches, actual_lazy) = analyze(source, settings);
        assert_eq!(actual_switches, switches);
        assert_eq!(actual_lazy, lazy_lists);
    }
}

#[test]
fn use_macros_latch_only_in_firestorm_and_only_their_own_extension() {
    let (_, _, switches, lazy) = analyze(
        "#define USE_SWITCHES\ndefault { state_entry() { } }",
        PreprocessSettings::firestorm(),
    );
    assert!(switches);
    assert!(!lazy);

    let (_, _, switches, lazy) = analyze(
        "#define USE_LAZY_LISTS\ndefault { state_entry() { } }",
        PreprocessSettings::firestorm(),
    );
    assert!(!switches);
    assert!(lazy);

    let (_, _, switches, lazy) = analyze(
        "#define USE_SWITCHES\n#define USE_LAZY_LISTS\ndefault { state_entry() { } }",
        PreprocessSettings::firestorm(),
    );
    assert!(switches && lazy);

    let (_, _, switches, lazy) = analyze(
        "#define USE_SWITCHES\n#define USE_LAZY_LISTS\ndefault { state_entry() { } }",
        PreprocessSettings::full(),
    );
    assert!(!switches && !lazy);

    // The directive newline is emitted while the macro is defined, so
    // Firestorm's per-token check latches the feature before the following undef.
    let (_, _, switches, lazy) = analyze(
        "#define USE_SWITCHES\n#undef USE_SWITCHES\ndefault { state_entry() { } }",
        PreprocessSettings::firestorm(),
    );
    assert!(switches);
    assert!(!lazy);

    // Once an emitted token observes the definition, the extension remains
    // enabled even after a later #undef.
    let (_, _, switches, lazy) = analyze(
        "#define USE_SWITCHES\ninteger reached;\n#undef USE_SWITCHES\ndefault { state_entry() { } }",
        PreprocessSettings::firestorm(),
    );
    assert!(switches);
    assert!(!lazy);
}

#[test]
fn use_macros_ignore_inactive_branches_comments_and_strings() {
    for source in [
        "#if 0\n#define USE_SWITCHES\n#endif\ndefault { state_entry() { } }",
        "// #define USE_SWITCHES\ndefault { state_entry() { } }",
        "/* #define USE_LAZY_LISTS */\ndefault { state_entry() { } }",
        "string text = \"#define USE_SWITCHES\";\ndefault { state_entry() { } }",
    ] {
        let (_, _, switches, lazy) = analyze(source, PreprocessSettings::firestorm());
        assert!(!switches, "{source}");
        assert!(!lazy, "{source}");
    }
}

#[test]
fn kill_switch_disables_requested_extensions_and_use_macros() {
    for source in [
        "//fspreprocessor off\n#define USE_SWITCHES\ndefault { state_entry() { switch (1) { case 1: break; } } }",
        "//fspreprocessor off\ndefault { state_entry() { switch (1) { case 1: break; } } }",
    ] {
        let settings = PreprocessSettings::firestorm()
            .with_switches_enabled(true)
            .with_lazy_lists_enabled(true);
        let (ast, _, switches, lazy) = analyze(source, settings);
        assert!(!switches);
        assert!(!lazy);
        assert!(switch_statements(&ast).is_empty());
    }
}

#[test]
fn include_defined_use_macro_enables_only_that_extension() {
    let directory = temporary_directory();
    fs::write(directory.join("header.lsl"), "#define USE_SWITCHES\n").unwrap();
    fs::write(
        directory.join("root.lsl"),
        "#include \"header.lsl\"\ndefault { state_entry() { switch (1) { case 1: break; } } }\n",
    )
    .unwrap();
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_file(directory.join("root.lsl"), PreprocessSettings::firestorm())
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    assert!(prepared.switches_enabled());
    assert!(!prepared.lazy_lists_enabled());
    let (tokens, lexical) = lex(prepared.text());
    assert!(lexical.is_empty());
    let (ast, diagnostics) =
        parse_ast_with_options(&tokens, AstParseOptions::from_flags(true, false));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(switch_statements(&ast).len(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn switch_arms_preserve_order_fallthrough_default_and_break() {
    let source = switch_program(
        "switch (value) { case 1: first(); case 2: second(); break; default: third(); break; }",
    );
    let (ast, diagnostics) = parse_switches(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let switches = switch_statements(&ast);
    assert_eq!(switches.len(), 1);
    let arms = &switches[0].arms;
    assert_eq!(arms.len(), 3);
    assert!(matches!(arms[0].label, SwitchLabel::Case(_)));
    assert!(matches!(arms[1].label, SwitchLabel::Case(_)));
    assert!(matches!(arms[2].label, SwitchLabel::Default));
    assert!(matches!(
        arms[1].statements[1].kind,
        StatementKind::Break(_)
    ));
    assert!(matches!(
        arms[2].statements[1].kind,
        StatementKind::Break(_)
    ));
    assert!(!arms[0]
        .statements
        .iter()
        .any(|statement| matches!(statement.kind, StatementKind::Break(_))));
}

#[test]
fn empty_cases_case_blocks_and_missing_default_are_preserved() {
    let (ast, diagnostics) = parse_switches(&switch_program(
        "switch (value) { case 1: case 2: { integer local; } break; }",
    ));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let arms = &switch_statements(&ast)[0].arms;
    assert_eq!(arms.len(), 2);
    assert!(arms[0].statements.is_empty());
    assert!(matches!(
        arms[1].statements[0].kind,
        StatementKind::Block(_)
    ));
    assert!(matches!(
        arms[1].statements[1].kind,
        StatementKind::Break(_)
    ));
    assert!(!arms
        .iter()
        .any(|arm| matches!(arm.label, SwitchLabel::Default)));
}

#[test]
fn nested_switches_and_switch_inside_control_flow_work() {
    let source = switch_program(
        "if (value) { switch (a) { case 1: switch (b) { case 2: break; } break; } } while (value) { switch (a) { default: break; } }",
    );
    let (ast, diagnostics) = parse_switches(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let body = event_body(&ast);
    let StatementKind::If(if_statement) = &body.statements[0].kind else {
        panic!("if")
    };
    let StatementKind::Block(if_block) = &if_statement.then_branch.kind else {
        panic!("if block")
    };
    let StatementKind::Switch(outer) = &if_block.statements[0].kind else {
        panic!("outer switch")
    };
    let StatementKind::Switch(inner) = &outer.arms[0].statements[0].kind else {
        panic!("inner switch")
    };
    assert_eq!(inner.arms.len(), 1);
    assert!(matches!(body.statements[1].kind, StatementKind::While(_)));
}

#[test]
fn duplicate_cases_and_defaults_keep_every_ast_arm() {
    let (ast, diagnostics) = parse_switches(&switch_program(
        "switch (value) { case 1: break; case 1: break; default: break; default: break; }",
    ));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let arms = &switch_statements(&ast)[0].arms;
    assert_eq!(arms.len(), 4);
    assert_eq!(
        arms.iter()
            .filter(|arm| matches!(arm.label, SwitchLabel::Default))
            .count(),
        2
    );
}

#[test]
fn switch_spans_cover_labels_statements_and_break() {
    let source = switch_program("switch (value) { case 1: first(); break; default: second(); }");
    let (ast, diagnostics) = parse_switches(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let switch = switch_statements(&ast)[0];
    assert_eq!(
        &source[switch.span.start..switch.span.end],
        "switch (value) { case 1: first(); break; default: second(); }"
    );
    assert_eq!(
        &source[switch.arms[0].span.start..switch.arms[0].span.end],
        "case 1: first(); break;"
    );
    assert_eq!(
        &source[switch.arms[1].span.start..switch.arms[1].span.end],
        "default: second();"
    );
    assert_eq!(
        &source[switch.arms[0].statements[1].span.start..switch.arms[0].statements[1].span.end],
        "break;"
    );
}

#[test]
fn switches_remain_identifiers_without_the_feature_gate() {
    let source = "integer switch; default { state_entry() { switch = 1; } }";
    let (ast, diagnostics) = parse_lsl(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(switch_statements(&ast).is_empty());
    let (ast, _) = parse_switches("default { state_entry() { break; } }");
    let body = event_body(&ast);
    assert!(matches!(
        body.statements[0].kind,
        StatementKind::Expression(_)
    ));
    assert!(!body
        .statements
        .iter()
        .any(|statement| matches!(statement.kind, StatementKind::Break(_))));
}

#[test]
fn malformed_switch_recovers_and_keeps_later_statements() {
    for source in [
        switch_program("switch () { case 1: break; } integer later;"),
        switch_program("switch value { case 1: break; } integer later;"),
        switch_program("switch (value) case 1: break; integer later;"),
        switch_program("switch (value) { case 1 integer x; case 2: break; } integer later;"),
        switch_program("switch (value) { default integer x; } integer later;"),
    ] {
        let (ast, diagnostics) = parse_switches(&source);
        assert!(!diagnostics.is_empty(), "{source}");
        let body = event_body(&ast);
        assert!(
            body.statements.iter().any(|statement| {
                matches!(&statement.kind, StatementKind::Local(local) if local.declarators[0].name == "later")
            }),
            "later statement lost for {source:?}: {body:#?}"
        );
    }

    let unterminated = switch_program("switch (value) { case 1: break; integer later;");
    let (ast, diagnostics) = parse_switches(&unterminated);
    assert!(!diagnostics.is_empty());
    let switch = &switch_statements(&ast)[0];
    assert!(switch.arms.iter().flat_map(|arm| &arm.statements).any(|statement| {
        matches!(&statement.kind, StatementKind::Local(local) if local.declarators[0].name == "later")
    }));
}

#[test]
fn lazy_index_access_assignment_and_cast_types_are_typed() {
    let source = switch_program(
        "(integer)myList[2]; (string)myList[index]; (vector)myList[index]; (rotation)myList[index]; (float)myList[index]; (key)myList[index]; (list)myList[index]; (list)myList[first,last]; myList[2] = \"value\";",
    );
    let (ast, diagnostics) = parse_lazy(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let body = event_body(&ast);
    assert_eq!(body.statements.len(), 9);
    for statement in &body.statements[..8] {
        let StatementKind::Expression(statement) = &statement.kind else {
            panic!("expression")
        };
        let ExpressionKind::Cast(cast) = &statement.expression.kind else {
            panic!("cast")
        };
        assert!(matches!(cast.operand.kind, ExpressionKind::LazyIndex(_)));
    }
    let StatementKind::Expression(assignment) = &body.statements[8].kind else {
        panic!("assignment")
    };
    let ExpressionKind::Assignment(assignment) = &assignment.expression.kind else {
        panic!("assignment expr")
    };
    assert!(matches!(
        assignment.target.kind,
        ExpressionKind::LazyIndex(_)
    ));
}

#[test]
fn lazy_index_expressions_preserve_calls_nesting_and_rhs() {
    let source = switch_program(
        "myList[getIndex() + 1] = makeValue((integer)other[0]); (integer)(myList[2]);",
    );
    let (ast, diagnostics) = parse_lazy(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let body = event_body(&ast);
    let StatementKind::Expression(statement) = &body.statements[0].kind else {
        panic!("assignment")
    };
    let ExpressionKind::Assignment(assignment) = &statement.expression.kind else {
        panic!("assignment expr")
    };
    let ExpressionKind::LazyIndex(index) = &assignment.target.kind else {
        panic!("lazy lhs")
    };
    assert_eq!(index.indices.len(), 1);
    assert!(matches!(index.indices[0].kind, ExpressionKind::Binary(_)));
    assert!(matches!(assignment.value.kind, ExpressionKind::Call(_)));
}

#[test]
fn malformed_lazy_constructs_diagnose_and_recover() {
    for source in [
        switch_program("myList[0]; integer later;"),
        switch_program("consume(myList[0]); integer later;"),
        switch_program("myList[other[0]] = 1; integer later;"),
        switch_program("myList[]; integer later;"),
        switch_program("myList[1; integer later;"),
        switch_program("myList[1,2,3]; integer later;"),
        switch_program("(1 + 2)[0]; integer later;"),
        switch_program("myList[1,2] = 3; integer later;"),
        switch_program("myList[1] += 3; integer later;"),
        switch_program("(string)myList[1,2]; integer later;"),
    ] {
        let (ast, diagnostics) = parse_lazy(&source);
        assert!(!diagnostics.is_empty(), "{source}");
        let body = event_body(&ast);
        assert!(
            body.statements.iter().any(|statement| {
                matches!(&statement.kind, StatementKind::Local(local) if local.declarators[0].name == "later")
            }),
            "later statement lost for {source:?}"
        );
    }
}

#[test]
fn lazy_index_reads_require_a_firestorm_typed_cast() {
    for source in [
        switch_program("myList[0]; integer later;"),
        switch_program("consume(myList[index]); integer later;"),
        switch_program("myList[index] + 1; integer later;"),
    ] {
        let (ast, diagnostics) = parse_lazy(&source);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "PAR007"),
            "bare lazy-list read was accepted: {source:?}"
        );
        assert!(event_body(&ast).statements.iter().any(|statement| {
            matches!(&statement.kind, StatementKind::Local(local) if local.declarators[0].name == "later")
        }));
    }

    for source in [
        switch_program("(integer)myList[0];"),
        switch_program("(integer)(myList[0]);"),
        switch_program("(list)myList[first,last];"),
        switch_program("myList[0] = value;"),
    ] {
        let (_, diagnostics) = parse_lazy(&source);
        assert!(
            diagnostics.is_empty(),
            "supported Firestorm lazy-list form was rejected: {source:?}: {diagnostics:?}"
        );
    }
}

#[test]
fn lazy_feature_gate_keeps_ordinary_lsl_and_lists_unchanged() {
    let source = switch_program("list values = [1, 2, 3]; values[0] = 9;");
    let (ast, diagnostics) = parse_lsl(&source);
    assert!(!diagnostics.is_empty());
    fn expression_has_lazy(expression: &neolint::ast::Expression) -> bool {
        match &expression.kind {
            ExpressionKind::LazyIndex(_) => true,
            ExpressionKind::Parenthesized(inner) => expression_has_lazy(inner),
            ExpressionKind::Unary(unary) => expression_has_lazy(&unary.operand),
            ExpressionKind::Binary(binary) => {
                expression_has_lazy(&binary.left) || expression_has_lazy(&binary.right)
            }
            ExpressionKind::Assignment(assignment) => {
                expression_has_lazy(&assignment.target) || expression_has_lazy(&assignment.value)
            }
            ExpressionKind::Cast(cast) => expression_has_lazy(&cast.operand),
            ExpressionKind::Increment(increment) => expression_has_lazy(&increment.operand),
            ExpressionKind::Call(call) => {
                expression_has_lazy(&call.callee) || call.arguments.iter().any(expression_has_lazy)
            }
            ExpressionKind::Component(component) => expression_has_lazy(&component.base),
            ExpressionKind::List(list) => list.elements.iter().any(expression_has_lazy),
            ExpressionKind::Vector(vector) => vector.components.iter().any(expression_has_lazy),
            ExpressionKind::Rotation(rotation) => {
                rotation.components.iter().any(expression_has_lazy)
            }
            _ => false,
        }
    }
    fn statement_has_lazy(statement: &neolint::ast::Statement) -> bool {
        match &statement.kind {
            StatementKind::Expression(expression) => expression_has_lazy(&expression.expression),
            StatementKind::Local(local) => local.declarators.iter().any(|declarator| {
                declarator
                    .initializer
                    .as_ref()
                    .is_some_and(expression_has_lazy)
            }),
            StatementKind::Block(block) => block.statements.iter().any(statement_has_lazy),
            StatementKind::If(if_statement) => {
                expression_has_lazy(&if_statement.condition)
                    || statement_has_lazy(&if_statement.then_branch)
                    || if_statement
                        .else_branch
                        .as_ref()
                        .is_some_and(|branch| statement_has_lazy(branch))
            }
            _ => false,
        }
    }
    assert!(!event_body(&ast).statements.iter().any(statement_has_lazy));
    let (ast, diagnostics) = parse_lazy(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let body = event_body(&ast);
    let StatementKind::Local(list) = &body.statements[0].kind else {
        panic!("list")
    };
    assert!(matches!(
        list.declarators[0].initializer.as_ref().unwrap().kind,
        ExpressionKind::List(_)
    ));
    let StatementKind::Expression(assignment) = &body.statements[1].kind else {
        panic!("assignment")
    };
    assert!(matches!(
        assignment.expression.kind,
        ExpressionKind::Assignment(_)
    ));
}

#[test]
fn deeply_nested_switches_are_bounded_and_keep_later_statements() {
    let depth = neolint::ast::MAX_AST_RECURSION_DEPTH + 20;
    let mut body = String::new();
    for _ in 0..depth {
        body.push_str("switch (1) { case 1: ");
    }
    body.push_str("break; ");
    for _ in 0..depth {
        body.push_str("} ");
    }
    body.push_str("integer later;");
    let (ast, diagnostics) = parse_switches(&switch_program(&body));
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "AST001")
            .count(),
        1,
        "{diagnostics:?}"
    );
    let body = event_body(&ast);
    assert!(body.statements.iter().any(|statement| {
        matches!(&statement.kind, StatementKind::Local(local) if local.declarators[0].name == "later")
    }));
}

#[test]
fn deeply_nested_lazy_indices_are_bounded_and_keep_later_statements() {
    let depth = neolint::ast::MAX_AST_RECURSION_DEPTH + 20;
    let mut expression = String::new();
    for _ in 0..depth {
        expression.push_str("myList[");
    }
    expression.push('0');
    for _ in 0..depth {
        expression.push(']');
    }
    let source = switch_program(&format!("integer value = {expression}; integer later;"));
    let (ast, diagnostics) = parse_lazy(&source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "AST001")
            .count(),
        1,
        "{diagnostics:?}"
    );
    let body = event_body(&ast);
    assert!(body.statements.iter().any(|statement| {
        matches!(&statement.kind, StatementKind::Local(local) if local.declarators[0].name == "later")
    }));
}

#[test]
fn extra_closing_brace_after_switch_is_diagnosed_without_panic() {
    let (_, diagnostics) = parse_switches(&switch_program(
        "switch (value) { case 1: break; } } integer later;",
    ));
    assert!(!diagnostics.is_empty());
}

#[test]
fn switch_break_inside_nested_block_and_default_block_work() {
    let source =
        switch_program("switch (value) { case 1: { break; } default { integer local; break; } }");
    let (ast, diagnostics) = parse_switches(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let arms = &switch_statements(&ast)[0].arms;
    assert_eq!(arms.len(), 2);
    let StatementKind::Block(case_block) = &arms[0].statements[0].kind else {
        panic!("case block")
    };
    assert_eq!(
        &source[arms[0].statements[0].span.start..arms[0].statements[0].span.end],
        "{ break; }"
    );
    assert!(matches!(
        case_block.statements[0].kind,
        StatementKind::Break(_)
    ));
    assert!(matches!(arms[1].label, SwitchLabel::Default));
    let StatementKind::Block(default_block) = &arms[1].statements[0].kind else {
        panic!("default block")
    };
    assert!(matches!(
        default_block.statements[1].kind,
        StatementKind::Break(_)
    ));
}

#[test]
fn lazy_index_spans_cover_access_range_and_assignment() {
    let source = switch_program(
        "(integer)myList[getIndex() + 1]; (list)myList[first,last]; myList[index] = makeValue();",
    );
    let (ast, diagnostics) = parse_lazy(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let body = event_body(&ast);
    let StatementKind::Expression(first) = &body.statements[0].kind else {
        panic!("first")
    };
    let ExpressionKind::Cast(first) = &first.expression.kind else {
        panic!("cast")
    };
    let ExpressionKind::LazyIndex(first) = &first.operand.kind else {
        panic!("lazy")
    };
    assert_eq!(
        &source[first.span.start..first.span.end],
        "myList[getIndex() + 1]"
    );
    let StatementKind::Expression(range) = &body.statements[1].kind else {
        panic!("range")
    };
    let ExpressionKind::Cast(range) = &range.expression.kind else {
        panic!("range cast")
    };
    let ExpressionKind::LazyIndex(range) = &range.operand.kind else {
        panic!("range lazy")
    };
    assert_eq!(
        &source[range.span.start..range.span.end],
        "myList[first,last]"
    );
    let StatementKind::Expression(assignment) = &body.statements[2].kind else {
        panic!("assignment")
    };
    let ExpressionKind::Assignment(assignment) = &assignment.expression.kind else {
        panic!("assignment expr")
    };
    assert_eq!(
        &source[assignment.target.span.start..assignment.target.span.end],
        "myList[index]"
    );
}

#[test]
fn parse_ast_without_options_matches_legacy_on_extension_syntax() {
    use neolint::parse_legacy;
    for source in [
        switch_program("switch (value) { case 1: break; }"),
        switch_program("values[0] = 1;"),
        switch_program("(integer)values[0];"),
    ] {
        let (tokens, lexical) = lex(&source);
        assert!(lexical.is_empty());
        let legacy = parse_legacy(&tokens);
        let (_, ast) = parse_ast(&tokens);
        let normalize = |diagnostics: &[Diagnostic]| {
            let mut values: Vec<_> = diagnostics
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.code,
                        diagnostic.message.clone(),
                        diagnostic.span.start,
                        diagnostic.span.end,
                    )
                })
                .collect();
            values.sort();
            values
        };
        assert_eq!(normalize(&legacy), normalize(&ast), "{source}");
    }
}
