use neolint::{lex, parse_ast, Token, TokenKind};

fn parse(source: &str) -> neolint::ast::Ast {
    let (tokens, _) = lex(source);
    let (ast, _) = parse_ast(&tokens);
    ast
}

#[test]
fn every_prefix_of_a_representative_program_is_safe_and_deterministic() {
    let source = r#"
        integer global = 1, second;
        integer add(integer a, float b) { if (a > 0) return a; else return b; }
        default {
            state_entry() {
                integer i = 0;
                for (i = 0; i < 10; ++i) { while (i > 0) i--; }
                list values = [1, "two", <1, 2, 3>];
                jump done;
                @done;
                state other;
            }
        }
        state other { state_entry() { } }
    "#;
    for end in 0..=source.len() {
        if !source.is_char_boundary(end) {
            continue;
        }
        let prefix = &source[..end];
        let first = parse(prefix);
        let second = parse(prefix);
        assert_eq!(first, second, "non-deterministic AST at byte {end}");
    }
}

fn generated_sources() -> Vec<String> {
    const PIECES: &[&str] = &[
        "default", "state", "{", "}", "(", ")", "[", "]", "<", ">", ",", ";", ":", "@", "integer",
        "float", "string", "key", "list", "vector", "rotation", "if", "else", "for", "while", "do",
        "return", "jump", "x", "y", "z", "s", ".", "=", "+", "-", "*", "/", "%", "!", "~", "++",
        "--", "==", "!=", "<=", ">=", "&&", "||", "1", "1.0", "\"text\"", "eof",
    ];
    let mut state = 0x1234_5678_9abc_def0u64;
    let mut sources = Vec::new();
    for _ in 0..600 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let count = (state as usize % 12) + 1;
        let mut source = String::new();
        for _ in 0..count {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push_str(PIECES[state as usize % PIECES.len()]);
            source.push(' ');
        }
        sources.push(source);
    }
    sources
}

#[test]
fn deterministic_malformed_token_soup_never_panics() {
    for source in generated_sources() {
        let (tokens, _) = lex(&source);
        let first = parse_ast(&tokens);
        let second = parse_ast(&tokens);
        assert_eq!(first, second, "non-deterministic parse for {source:?}");
    }
}

#[test]
fn token_stream_without_eof_or_with_repeated_eof_is_safe() {
    let (mut tokens, _) = lex("default { state_entry() { integer x = (1); } }");
    assert_eq!(tokens.last().map(|token| token.kind), Some(TokenKind::Eof));
    tokens.pop();
    let _ = parse_ast(&tokens);

    let eof = Token {
        kind: TokenKind::Eof,
        lexeme: String::new(),
        span: tokens.last().map_or(
            neolint::Span {
                start: 0,
                end: 0,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            },
            |token| token.span,
        ),
    };
    let repeated = vec![eof.clone(), eof.clone(), eof];
    let (ast, diagnostics) = parse_ast(&repeated);
    assert!(ast.items.is_empty());
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "PAR013"));
}

#[test]
fn unicode_strings_and_comments_do_not_break_coordinates() {
    let source =
        "// é\nstring text = \"héllo 🌍\";\ndefault { state_entry() { string copy = text; } }";
    let (tokens, lexical) = lex(source);
    assert!(lexical.is_empty());
    let (ast, diagnostics) = parse_ast(&tokens);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let first_token = source.find("string").unwrap();
    assert_eq!(ast.span.start, first_token);
    assert_eq!(ast.span.end, source.len());
}
