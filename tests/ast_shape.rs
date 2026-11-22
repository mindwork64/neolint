use neolint::ast::{
    parse_ast, AssignmentOperator, Ast, BinaryOperator, Component, ExpressionKind,
    IncrementOperator, StatementKind, TopLevel, TypeKind, UnaryOperator,
};
use neolint::{lex, Diagnostic};

fn parse(source: &str) -> (Ast, Vec<Diagnostic>) {
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    parse_ast(&tokens)
}

fn clean(source: &str) -> Ast {
    let (ast, diagnostics) = parse(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    ast
}

fn event_body(state: &neolint::ast::StateDeclaration, index: usize) -> &neolint::ast::Block {
    &state.events[index].body
}

#[test]
fn declarations_parameters_states_and_events_have_typed_shape() {
    let ast = clean(
        r#"
        integer global = 1, other;
        float add(float left, integer right) { return left + right; }
        announce(string message) { }
        default { state_entry() { } }
        state running { touch_start(integer count) { } state_exit() { } }
        "#,
    );
    assert_eq!(ast.items.len(), 5);
    let TopLevel::Global(global) = &ast.items[0] else {
        panic!("global")
    };
    assert_eq!(global.ty.kind, TypeKind::Integer);
    assert_eq!(global.declarators.len(), 2);
    assert_eq!(global.declarators[0].name, "global");
    assert!(global.declarators[0].initializer.is_some());
    assert_eq!(global.declarators[1].name, "other");
    assert!(global.declarators[1].initializer.is_none());

    let TopLevel::Function(function) = &ast.items[1] else {
        panic!("function")
    };
    assert_eq!(
        function.return_type.map(|ty| ty.kind),
        Some(TypeKind::Float)
    );
    assert_eq!(function.name, "add");
    assert_eq!(function.parameters.len(), 2);
    assert_eq!(function.parameters[0].ty.kind, TypeKind::Float);
    assert_eq!(function.parameters[1].name, "right");
    assert_eq!(function.body.statements.len(), 1);

    let TopLevel::Function(function) = &ast.items[2] else {
        panic!("untyped")
    };
    assert!(function.return_type.is_none());

    let TopLevel::State(default) = &ast.items[3] else {
        panic!("default")
    };
    assert!(default.is_default);
    assert_eq!(default.name, "default");
    assert_eq!(default.events.len(), 1);

    let TopLevel::State(running) = &ast.items[4] else {
        panic!("running")
    };
    assert!(!running.is_default);
    assert_eq!(running.name, "running");
    assert_eq!(running.events.len(), 2);
    assert_eq!(running.events[1].parameters.len(), 0);
}

#[test]
fn every_statement_category_is_typed() {
    let ast = clean(
        r#"
        default
        {
            state_entry()
            {
                ;
                { integer nested; }
                integer local = 1, second;
                if (local > 0) { local += 1; } else local--;
                for (local = 0; local < 2; ++local) ;
                while (local > 0) --local;
                do { ++local; } while (local < 1);
                jump done;
                @done;
                state running;
                return;
                local = 2;
            }
        }
        state running { state_entry() { } }
        "#,
    );
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    let body = event_body(default, 0);
    assert_eq!(body.statements.len(), 12);
    assert!(matches!(body.statements[0].kind, StatementKind::Empty));
    assert!(matches!(body.statements[1].kind, StatementKind::Block(_)));
    assert!(matches!(body.statements[2].kind, StatementKind::Local(_)));
    assert!(matches!(body.statements[3].kind, StatementKind::If(_)));
    assert!(matches!(body.statements[4].kind, StatementKind::For(_)));
    assert!(matches!(body.statements[5].kind, StatementKind::While(_)));
    assert!(matches!(body.statements[6].kind, StatementKind::DoWhile(_)));
    assert!(matches!(body.statements[7].kind, StatementKind::Jump(_)));
    assert!(matches!(body.statements[8].kind, StatementKind::Label(_)));
    assert!(matches!(
        body.statements[9].kind,
        StatementKind::StateChange(_)
    ));
    assert!(matches!(body.statements[10].kind, StatementKind::Return(_)));
    assert!(matches!(
        body.statements[11].kind,
        StatementKind::Expression(_)
    ));
}

#[test]
fn expression_categories_and_operator_kinds_are_typed() {
    let ast = clean(
        r#"
        integer helper(integer a, integer b) { return a + b; }
        default
        {
            state_entry()
            {
                integer i = 1 + 2 * 3 - 4 / 2 % 2;
                integer j = (i + 1) * 2;
                i = i & 1 | 2 ^ 3;
                i += (integer)j;
                i++;
                --i;
                list values = [1, "two", <1, 2, 3>];
                vector position = <1, 2, 3>;
                rotation orientation = <0, 0, 0, 1>;
                position.x = 2.0;
                float s = orientation.s;
                i = helper(i, j);
            }
        }
        "#,
    );
    let TopLevel::State(default) = &ast.items[1] else {
        panic!("state")
    };
    let body = event_body(default, 0);
    let StatementKind::Local(first) = &body.statements[0].kind else {
        panic!("locals")
    };
    let expression = first.declarators[0].initializer.as_ref().unwrap();
    assert!(matches!(expression.kind, ExpressionKind::Binary(_)));
    if let ExpressionKind::Binary(binary) = &expression.kind {
        assert_eq!(binary.operator, BinaryOperator::Add);
        let ExpressionKind::Binary(right) = &binary.right.kind else {
            panic!("subtraction binds tighter than addition in LSL")
        };
        assert_eq!(right.operator, BinaryOperator::Subtract);
    }

    let StatementKind::Local(second) = &body.statements[1].kind else {
        panic!("local")
    };
    let expression = second.declarators[0].initializer.as_ref().unwrap();
    assert!(matches!(expression.kind, ExpressionKind::Binary(_)));
    if let ExpressionKind::Binary(binary) = &expression.kind {
        assert_eq!(binary.operator, BinaryOperator::Multiply);
        assert!(matches!(binary.left.kind, ExpressionKind::Parenthesized(_)));
    }

    let StatementKind::Expression(assignment) = &body.statements[2].kind else {
        panic!("assign")
    };
    assert!(matches!(
        assignment.expression.kind,
        ExpressionKind::Assignment(_)
    ));
    if let ExpressionKind::Assignment(assignment) = &assignment.expression.kind {
        assert_eq!(assignment.operator, AssignmentOperator::Assign);
        assert!(matches!(assignment.value.kind, ExpressionKind::Binary(_)));
    }

    let StatementKind::Expression(compound) = &body.statements[3].kind else {
        panic!("compound")
    };
    if let ExpressionKind::Assignment(assignment) = &compound.expression.kind {
        assert_eq!(assignment.operator, AssignmentOperator::AddAssign);
        assert!(matches!(assignment.value.kind, ExpressionKind::Cast(_)));
    } else {
        panic!("compound assignment")
    }

    let StatementKind::Expression(postfix) = &body.statements[4].kind else {
        panic!("postfix")
    };
    if let ExpressionKind::Increment(increment) = &postfix.expression.kind {
        assert_eq!(increment.operator, IncrementOperator::Increment);
        assert!(!increment.prefix);
    } else {
        panic!("postfix")
    }
    let StatementKind::Expression(prefix) = &body.statements[5].kind else {
        panic!("prefix")
    };
    if let ExpressionKind::Increment(increment) = &prefix.expression.kind {
        assert_eq!(increment.operator, IncrementOperator::Decrement);
        assert!(increment.prefix);
    } else {
        panic!("prefix")
    }

    let StatementKind::Local(list) = &body.statements[6].kind else {
        panic!("list")
    };
    let list = list.declarators[0].initializer.as_ref().unwrap();
    let ExpressionKind::List(list) = &list.kind else {
        panic!("list expr")
    };
    assert_eq!(list.elements.len(), 3);
    assert!(matches!(list.elements[2].kind, ExpressionKind::Vector(_)));

    let StatementKind::Local(vector) = &body.statements[7].kind else {
        panic!("vector")
    };
    let vector = vector.declarators[0].initializer.as_ref().unwrap();
    let ExpressionKind::Vector(vector) = &vector.kind else {
        panic!("vector expr")
    };
    assert_eq!(vector.components.len(), 3);

    let StatementKind::Local(rotation) = &body.statements[8].kind else {
        panic!("rotation")
    };
    let rotation = rotation.declarators[0].initializer.as_ref().unwrap();
    let ExpressionKind::Rotation(rotation) = &rotation.kind else {
        panic!("rotation expr")
    };
    assert_eq!(rotation.components.len(), 4);

    let StatementKind::Expression(component) = &body.statements[9].kind else {
        panic!("component")
    };
    if let ExpressionKind::Assignment(assignment) = &component.expression.kind {
        let ExpressionKind::Component(access) = &assignment.target.kind else {
            panic!("component target")
        };
        assert_eq!(access.component, Component::X);
        assert!(matches!(access.base.kind, ExpressionKind::Identifier(_)));
    } else {
        panic!("component assignment")
    }

    let StatementKind::Local(s) = &body.statements[10].kind else {
        panic!("s")
    };
    let s = s.declarators[0].initializer.as_ref().unwrap();
    let ExpressionKind::Component(access) = &s.kind else {
        panic!("component s")
    };
    assert_eq!(access.component, Component::S);

    let StatementKind::Expression(call) = &body.statements[11].kind else {
        panic!("call")
    };
    let ExpressionKind::Assignment(assignment) = &call.expression.kind else {
        panic!("call assign")
    };
    let ExpressionKind::Call(call) = &assignment.value.kind else {
        panic!("call expr")
    };
    assert_eq!(call.arguments.len(), 2);
    assert!(matches!(call.callee.kind, ExpressionKind::Identifier(_)));

    let TopLevel::Function(helper) = &ast.items[0] else {
        panic!("helper")
    };
    let StatementKind::Return(ret) = &helper.body.statements[0].kind else {
        panic!("return")
    };
    let ret = ret.value.as_ref().unwrap();
    let ExpressionKind::Binary(binary) = &ret.kind else {
        panic!("binary")
    };
    assert_eq!(binary.operator, BinaryOperator::Add);
}

#[test]
fn unary_operators_are_typed() {
    let ast = clean(
        r#"
        default { state_entry() { integer i = !~-+1; } }
        "#,
    );
    let TopLevel::State(default) = &ast.items[0] else {
        panic!("state")
    };
    let body = event_body(default, 0);
    let StatementKind::Local(local) = &body.statements[0].kind else {
        panic!("local")
    };
    let mut expression = local.declarators[0].initializer.as_ref().unwrap();
    let mut operators = Vec::new();
    while let ExpressionKind::Unary(unary) = &expression.kind {
        operators.push(unary.operator);
        expression = &unary.operand;
    }
    assert_eq!(
        operators,
        vec![
            UnaryOperator::Bang,
            UnaryOperator::Tilde,
            UnaryOperator::Minus,
            UnaryOperator::Plus
        ]
    );
    assert!(matches!(expression.kind, ExpressionKind::Integer(_)));
}

#[test]
fn spans_cover_exact_constructs_and_utf8_neighbours() {
    let source = "// é comment\ninteger global = \"héllo\";\ndefault { state_entry() { integer local = (1 + 2); } }";
    let ast = clean(source);
    let TopLevel::Global(global) = &ast.items[0] else {
        panic!("global")
    };
    assert_eq!(
        &source[global.span.start..global.span.end],
        "integer global = \"héllo\";"
    );
    let initializer = global.declarators[0].initializer.as_ref().unwrap();
    assert_eq!(
        &source[initializer.span.start..initializer.span.end],
        "\"héllo\""
    );
    let TopLevel::State(default) = &ast.items[1] else {
        panic!("state")
    };
    let body = event_body(default, 0);
    let StatementKind::Local(local) = &body.statements[0].kind else {
        panic!("local")
    };
    assert_eq!(
        &source[local.span.start..local.span.end],
        "integer local = (1 + 2);"
    );
    let expression = local.declarators[0].initializer.as_ref().unwrap();
    assert_eq!(
        &source[expression.span.start..expression.span.end],
        "(1 + 2)"
    );
    let ExpressionKind::Parenthesized(inner) = &expression.kind else {
        panic!("paren")
    };
    assert_eq!(&source[inner.span.start..inner.span.end], "1 + 2");
    let ExpressionKind::Binary(binary) = &inner.kind else {
        panic!("binary")
    };
    assert_eq!(&source[binary.left.span.start..binary.left.span.end], "1");
    assert_eq!(&source[binary.right.span.start..binary.right.span.end], "2");
}

#[test]
fn empty_and_utf8_leading_input_have_total_spans() {
    let (ast, diagnostics) = parse("");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PAR013"));
    assert_eq!(ast.span.start, 0);
    assert_eq!(ast.span.end, 0);

    let source = "// é\n   \n";
    let (ast, diagnostics) = parse(source);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PAR013"));
    assert_eq!(ast.span.start, 0);
    assert_eq!(ast.span.end, 0);
}

#[test]
fn multi_declarator_initializers_bind_to_the_right_names() {
    let ast = clean(
        "integer first = other, second = first; integer other; default { state_entry() { } }",
    );
    let TopLevel::Global(global) = &ast.items[0] else {
        panic!("global")
    };
    assert_eq!(global.declarators[0].name, "first");
    assert_eq!(global.declarators[1].name, "second");
    let first = global.declarators[0].initializer.as_ref().unwrap();
    let second = global.declarators[1].initializer.as_ref().unwrap();
    let ExpressionKind::Identifier(first) = &first.kind else {
        panic!("first init")
    };
    let ExpressionKind::Identifier(second) = &second.kind else {
        panic!("second init")
    };
    assert_eq!(first.name, "other");
    assert_eq!(second.name, "first");
}
