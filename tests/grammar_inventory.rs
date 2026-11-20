use neolint::{lex, parse, Diagnostic, DiagnosticKind};

fn parse_source(source: &str) -> Vec<Diagnostic> {
    let (tokens, lexical_diagnostics) = lex(source);
    assert!(
        lexical_diagnostics.is_empty(),
        "unexpected lexical diagnostics: {lexical_diagnostics:?}"
    );
    parse(&tokens)
}

fn syntax_codes(diagnostics: &[Diagnostic]) -> Vec<&'static str> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::Syntax)
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn assert_has_code(diagnostics: &[Diagnostic], code: &str) {
    assert!(
        syntax_codes(diagnostics).contains(&code),
        "expected {code}: {diagnostics:?}"
    );
}

#[test]
fn mixed_program_inventory_positive() {
    let diagnostics = parse_source(
        r#"
        integer global_count = 0;
        float global_scale = 1.0;
        string global_text = "ready";
        key global_key = "00000000-0000-0000-0000-000000000000";
        list global_list = [1, "two", <1.0, 2.0, 3.0>];
        vector global_position = <1.0, 2.0, 3.0>;
        rotation global_rotation = <0.0, 0.0, 0.0, 1.0>;

        integer add(integer left, integer right) { return left + right; }
        announce(string message) { llOwnerSay(message); }

        default
        {
            state_entry() { announce("default"); }
            touch_start(integer count)
            {
                if (count > 0) state running; else state default;
            }
        }

        state running
        {
            state_entry()
            {
                integer local = add(1, 2);
                announce((string)local);
            }
            touch_end(integer count) { state default; }
        }
        "#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn declared_types_and_declarations_inventory_positive() {
    let diagnostics = parse_source(
        r#"
        integer global_integer;
        float global_float = 1.0;
        string global_string = "text";
        key global_key = "00000000-0000-0000-0000-000000000000";
        list global_list = [1, "two", <1, 2, 3>];
        vector global_vector = <1.0, 2.0, 3.0>;
        rotation global_rotation = <0.0, 0.0, 0.0, 1.0>;
        integer first, second;

        default
        {
            state_entry()
            {
                integer local_integer = 1;
                float local_float = 2.0;
                string local_string = "local";
                key local_key;
                list local_list;
                vector local_vector;
                rotation local_rotation;
                first = local_integer;
                second = 0;
            }
        }
        "#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn statements_inventory_positive() {
    let diagnostics = parse_source(
        r#"
        no_return() { return; }

        default
        {
            state_entry()
            {
                { integer nested; ; }
                integer i;
                integer j;
                i = 0;
                i += 1;
                i -= 1;
                i *= 2;
                i /= 2;
                i %= 2;
                if (i > 0) { ; } else { ; }
                for (i = 0, j = 0; i < 1; ++i, ++j) ;
                while (i > 0) i--;
                do { i++; } while (i < 1);
                @again;
                if (i > 0) jump done;
                @done;
                state running;
            }
        }

        state running { state_entry() { state default; } }
        "#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn expressions_inventory_positive() {
    let diagnostics = parse_source(
        r#"
        list items = [1, "two", <1, 2, 3>];
        vector position = <1, 2, 3>;
        rotation orientation = <1, 2, 3, 4>;
        integer add(integer left, integer right) { return left + right; }

        default
        {
            state_entry()
            {
                integer i = 1;
                integer j = 2;
                string text = "text";
                i = 1 + 2 * 3 - 4 / 2 % 2;
                i = (i + j) * 2;
                i = i & j | i ^ j;
                i = i << 1 >> 1;
                i = (i > j) && (i < j) || (i == j) || (i != j);
                i = !(i <= j) || (i >= j);
                i = -i + +j;
                i = ~i;
                i++;
                --i;
                i += 1;
                text = (string)i;
                items = items + [i];
                position = position + <1, 2, 3>;
                orientation = orientation * <1, 2, 3, 4>;
                float x = position.x;
                float s = orientation.s;
                i = add(i, j);
                i = (1) < 2;
                i = 1 > (0);
            }
        }
        "#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn for_expression_lists_inventory_positive() {
    let diagnostics = parse_source(
        "default { state_entry() { integer i; integer j; for (i = 0, j = 0; i < 1; ++i, ++j) ; } }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn focused_malformed_statement_diagnostics() {
    assert_has_code(
        &parse_source("default { state_entry() { integer value = 1 + ; } }"),
        "PAR005",
    );
    assert_has_code(
        &parse_source("default { state_entry() { if () { } } }"),
        "PAR006",
    );
    assert_has_code(&parse_source("default { integer value; }"), "PAR004");

    for source in [
        "default { state_entry() {",
        "integer f(integer value { return value; } default { }",
        "default { state_entry() { vector value = <1, 2, 3; } }",
        "default { state_entry() { list values = [1, 2; } }",
    ] {
        let diagnostics = parse_source(source);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == DiagnosticKind::Syntax),
            "expected a syntax error for {source:?}: {diagnostics:?}"
        );
    }
}
