use neolint::ast::{
    parse_ast, Ast, Block, Expression, ExpressionKind, Statement, StatementKind, TopLevel, Type,
    VariableDeclarator,
};
use neolint::{lex, AnalysisRun, PreprocessSettings, SourceId, Span};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

fn temporary_directory() -> PathBuf {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("neolint-ast-bridge-{}-{id}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn visit_type(ty: &Type, visit: &mut impl FnMut(Span)) {
    visit(ty.span);
}

fn visit_declarator(declarator: &VariableDeclarator, visit: &mut impl FnMut(Span)) {
    visit(declarator.span);
    visit(declarator.name_span);
    if let Some(error) = &declarator.name_error {
        visit(error.span);
    }
    if let Some(initializer) = &declarator.initializer {
        visit_expression(initializer, visit);
    }
    if let Some(error) = &declarator.initializer_error {
        visit(error.span);
    }
}

fn visit_block(block: &Block, visit: &mut impl FnMut(Span)) {
    visit(block.span);
    for statement in &block.statements {
        visit_statement(statement, visit);
    }
}

fn visit_statement(statement: &Statement, visit: &mut impl FnMut(Span)) {
    visit(statement.span);
    match &statement.kind {
        StatementKind::Empty => {}
        StatementKind::Block(block) => visit_block(block, visit),
        StatementKind::Local(local) => {
            visit(local.span);
            visit_type(&local.ty, visit);
            for declarator in &local.declarators {
                visit_declarator(declarator, visit);
            }
        }
        StatementKind::If(if_statement) => {
            visit(if_statement.span);
            visit_expression(&if_statement.condition, visit);
            visit_statement(&if_statement.then_branch, visit);
            if let Some(else_branch) = &if_statement.else_branch {
                visit_statement(else_branch, visit);
            }
        }
        StatementKind::For(for_statement) => {
            visit(for_statement.span);
            for expression in &for_statement.initializer {
                visit_expression(expression, visit);
            }
            visit_expression(&for_statement.condition, visit);
            for expression in &for_statement.increment {
                visit_expression(expression, visit);
            }
            visit_statement(&for_statement.body, visit);
        }
        StatementKind::While(while_statement) => {
            visit(while_statement.span);
            visit_expression(&while_statement.condition, visit);
            visit_statement(&while_statement.body, visit);
        }
        StatementKind::DoWhile(do_while) => {
            visit(do_while.span);
            visit_statement(&do_while.body, visit);
            visit_expression(&do_while.condition, visit);
        }
        StatementKind::Return(return_statement) => {
            visit(return_statement.span);
            if let Some(value) = &return_statement.value {
                visit_expression(value, visit);
            }
        }
        StatementKind::Jump(jump) => {
            visit(jump.span);
            visit(jump.label_span);
        }
        StatementKind::StateChange(change) => {
            visit(change.span);
            visit(change.state_span);
        }
        StatementKind::Label(label) => {
            visit(label.span);
            visit(label.name_span);
        }
        StatementKind::Expression(expression) => {
            visit(expression.span);
            visit_expression(&expression.expression, visit);
        }
        StatementKind::Switch(switch) => {
            visit(switch.span);
            visit_expression(&switch.condition, visit);
            for arm in &switch.arms {
                visit(arm.span);
                match &arm.label {
                    neolint::ast::SwitchLabel::Case(expression) => {
                        visit_expression(expression, visit)
                    }
                    neolint::ast::SwitchLabel::Default => {}
                    neolint::ast::SwitchLabel::Error(error) => visit(error.span),
                }
                for statement in &arm.statements {
                    visit_statement(statement, visit);
                }
            }
        }
        StatementKind::Break(break_statement) => visit(break_statement.span),
        StatementKind::Error(error) => visit(error.span),
    }
}

fn visit_expression(expression: &Expression, visit: &mut impl FnMut(Span)) {
    visit(expression.span);
    match &expression.kind {
        ExpressionKind::Identifier(identifier) => visit(identifier.span),
        ExpressionKind::Integer(literal) => visit(literal.span),
        ExpressionKind::Float(literal) => visit(literal.span),
        ExpressionKind::String(literal) => visit(literal.span),
        ExpressionKind::Parenthesized(inner) => visit_expression(inner, visit),
        ExpressionKind::List(list) => {
            visit(list.span);
            for element in &list.elements {
                visit_expression(element, visit);
            }
        }
        ExpressionKind::Vector(vector) => {
            visit(vector.span);
            for component in &vector.components {
                visit_expression(component, visit);
            }
        }
        ExpressionKind::Rotation(rotation) => {
            visit(rotation.span);
            for component in &rotation.components {
                visit_expression(component, visit);
            }
        }
        ExpressionKind::Call(call) => {
            visit(call.span);
            visit_expression(&call.callee, visit);
            for argument in &call.arguments {
                visit_expression(argument, visit);
            }
        }
        ExpressionKind::Component(component) => {
            visit(component.span);
            visit_expression(&component.base, visit);
            visit(component.component_span);
        }
        ExpressionKind::Unary(unary) => {
            visit(unary.span);
            visit(unary.operator_span);
            visit_expression(&unary.operand, visit);
        }
        ExpressionKind::Binary(binary) => {
            visit(binary.span);
            visit_expression(&binary.left, visit);
            visit(binary.operator_span);
            visit_expression(&binary.right, visit);
        }
        ExpressionKind::Assignment(assignment) => {
            visit(assignment.span);
            visit_expression(&assignment.target, visit);
            visit(assignment.operator_span);
            visit_expression(&assignment.value, visit);
        }
        ExpressionKind::Cast(cast) => {
            visit(cast.span);
            visit_type(&cast.ty, visit);
            visit_expression(&cast.operand, visit);
        }
        ExpressionKind::Increment(increment) => {
            visit(increment.span);
            visit(increment.operator_span);
            visit_expression(&increment.operand, visit);
        }
        ExpressionKind::LazyIndex(lazy) => {
            visit(lazy.span);
            visit_expression(&lazy.base, visit);
            for index in &lazy.indices {
                visit_expression(index, visit);
            }
        }
        ExpressionKind::Error(error) => visit(error.span),
    }
}

fn visit_ast(ast: &Ast, visit: &mut impl FnMut(Span)) {
    visit(ast.span);
    for item in &ast.items {
        match item {
            TopLevel::Global(global) => {
                visit(global.span);
                visit_type(&global.ty, visit);
                for declarator in &global.declarators {
                    visit_declarator(declarator, visit);
                }
            }
            TopLevel::Function(function) => {
                visit(function.span);
                visit(function.name_span);
                if let Some(return_type) = function.return_type {
                    visit_type(&return_type, visit);
                }
                for parameter in &function.parameters {
                    visit(parameter.span);
                    visit_type(&parameter.ty, visit);
                    visit(parameter.name_span);
                }
                if let Some(body) = &function.body {
                    visit_block(body, visit);
                }
            }
            TopLevel::State(state) => {
                visit(state.span);
                visit(state.name_span);
                for event in &state.events {
                    visit(event.span);
                    visit(event.name_span);
                    for parameter in &event.parameters {
                        visit(parameter.span);
                        visit_type(&parameter.ty, visit);
                        visit(parameter.name_span);
                    }
                    if let Some(body) = &event.body {
                        visit_block(body, visit);
                    }
                }
            }
            TopLevel::Error(error) => visit(error.span),
        }
    }
}

fn resolve_spans(
    run: &AnalysisRun,
    index: usize,
    ast: &Ast,
    require_foreign_source: bool,
) -> Vec<SourceId> {
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    let root = unit.root().unwrap();
    let mut spans = Vec::new();
    visit_ast(ast, &mut |span| spans.push(span));
    assert!(!spans.is_empty());
    let mut sources = Vec::new();
    for span in spans {
        let prepared_span = prepared.prepared_span(span).unwrap();
        let resolved = prepared.map().resolve_span(prepared_span).unwrap();
        let primary = resolved.primary();
        assert!(primary.start() <= primary.end());
        let file = prepared.map().file(primary.source()).unwrap();
        assert!(primary.end() <= file.text().len());
        sources.push(primary.source());
    }
    if require_foreign_source {
        assert!(sources.iter().any(|source| *source != root));
    }
    sources
}

#[test]
fn every_identity_ast_node_span_bridges_and_resolves() {
    let source = r#"
        integer global = 1, second = global;
        integer add(integer left, float right) { return left + right; }
        default {
            state_entry() {
                integer local = 1;
                if (local > 0) { local += 1; } else local--;
                for (local = 0; local < 2; ++local) ;
                while (local > 0) --local;
                do { ++local; } while (local < 1);
                list values = [1, "two", <1, 2, 3>];
                vector position = <1, 2, 3>;
                rotation orientation = <0, 0, 0, 1>;
                position.x = 2.0;
                float s = orientation.s;
                jump done;
                @done;
                state other;
                return;
            }
        }
        state other { state_entry() { } }
    "#;
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("root.lsl", source).unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    let (tokens, lexical) = lex(prepared.text());
    assert!(lexical.is_empty());
    let (ast, diagnostics) = parse_ast(&tokens);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resolve_spans(&run, index, &ast, false);
}

#[test]
fn every_macro_prepared_ast_node_span_resolves_through_the_map() {
    let source = "#define VALUE 1\ninteger global = VALUE;\ndefault { state_entry() { integer local = VALUE; } }\n";
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_text("root.lsl", source, PreprocessSettings::full())
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    let (tokens, lexical) = lex(prepared.text());
    assert!(lexical.is_empty());
    let (ast, diagnostics) = parse_ast(&tokens);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resolve_spans(&run, index, &ast, false);
}

#[test]
fn included_ast_node_spans_resolve_to_included_sources() {
    let directory = temporary_directory();
    fs::write(directory.join("inc.lsl"), "integer included = 1;\n").unwrap();
    fs::write(
        directory.join("root.lsl"),
        "#include \"inc.lsl\"\ndefault { state_entry() { integer local = 2; } }\n",
    )
    .unwrap();
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_file(directory.join("root.lsl"), PreprocessSettings::full())
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    let (tokens, lexical) = lex(prepared.text());
    assert!(lexical.is_empty());
    let (ast, diagnostics) = parse_ast(&tokens);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resolve_spans(&run, index, &ast, true);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn every_firestorm_identity_ast_span_bridges_and_resolves() {
    let source = r#"
        default {
            state_entry() {
                switch (value) {
                    case 1: first(); break;
                    default: second();
                }
                myList[2] = "value";
                integer extracted = (integer)myList[index];
            }
        }
    "#;
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("root.lsl", source).unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    let (tokens, lexical) = lex(prepared.text());
    assert!(lexical.is_empty());
    let (ast, diagnostics) = neolint::ast::parse_ast_with_options(
        &tokens,
        neolint::ast::AstParseOptions::lsl()
            .with_switches_enabled(true)
            .with_lazy_lists_enabled(true),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resolve_spans(&run, index, &ast, false);
}

#[test]
fn every_macro_firestorm_ast_span_resolves_through_the_map() {
    let source = "#define USE_SWITCHES\n#define USE_LAZY_LISTS\ndefault { state_entry() { switch (value) { case 1: break; } myList[2] = \"value\"; } }\n";
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_text("root.lsl", source, PreprocessSettings::firestorm())
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    assert!(prepared.switches_enabled() && prepared.lazy_lists_enabled());
    let (tokens, lexical) = lex(prepared.text());
    assert!(lexical.is_empty());
    let (ast, diagnostics) = neolint::ast::parse_ast_with_options(
        &tokens,
        neolint::ast::AstParseOptions::from_flags(
            prepared.switches_enabled(),
            prepared.lazy_lists_enabled(),
        ),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resolve_spans(&run, index, &ast, false);
}

#[test]
fn every_included_firestorm_ast_span_resolves_to_included_sources() {
    let directory = temporary_directory();
    fs::write(
        directory.join("header.lsl"),
        "#define USE_SWITCHES\n#define USE_LAZY_LISTS\ninteger included = 1;\n",
    )
    .unwrap();
    fs::write(
        directory.join("root.lsl"),
        "#include \"header.lsl\"\ndefault { state_entry() { switch (value) { case 1: break; } myList[2] = \"value\"; } }\n",
    )
    .unwrap();
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_file(directory.join("root.lsl"), PreprocessSettings::firestorm())
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    assert!(prepared.switches_enabled() && prepared.lazy_lists_enabled());
    let (tokens, lexical) = lex(prepared.text());
    assert!(lexical.is_empty());
    let (ast, diagnostics) = neolint::ast::parse_ast_with_options(
        &tokens,
        neolint::ast::AstParseOptions::from_flags(
            prepared.switches_enabled(),
            prepared.lazy_lists_enabled(),
        ),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resolve_spans(&run, index, &ast, true);
    fs::remove_dir_all(directory).unwrap();
}
