//! Lexical analysis for LSL source text.
//!
//! The lexer is a single-pass byte-oriented scanner. It preserves UTF-8 byte
//! offsets together with one-based line/column positions ([`Span`]), emits
//! keyword, identifier, literal, punctuation, and operator tokens, and reports
//! unterminated strings/comments and unsupported characters as lexical
//! diagnostics. It never panics on malformed input; a lexical error is returned
//! alongside the successfully recovered token stream.
//!
//! Identifiers are intentionally ASCII-only. Unicode is accepted inside strings
//! and comments, but an identifier such as `café` produces a lexical diagnostic.
use crate::diagnostic::{Diagnostic, Span};

/// Every token category recognized by the LSL lexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Identifier or unclassified name.
    Identifier,
    /// Decimal or hexadecimal integer literal.
    IntegerLiteral,
    /// Floating-point literal, including exponent forms.
    FloatLiteral,
    /// Quoted string literal (the lexeme retains quotes/escapes).
    StringLiteral,

    /// Keyword `integer`.
    KwInteger,
    /// Keyword `float`.
    KwFloat,
    /// Keyword `string`.
    KwString,
    /// Keyword `key`.
    KwKey,
    /// Keyword `list`.
    KwList,
    /// Keyword `vector`.
    KwVector,
    /// Keyword `rotation`.
    KwRotation,
    /// Keyword `void` (rejected as a source value type).
    KwVoid,
    /// Keyword `default`.
    KwDefault,
    /// Keyword `state`.
    KwState,
    /// Keyword `if`.
    KwIf,
    /// Keyword `else`.
    KwElse,
    /// Keyword `for`.
    KwFor,
    /// Keyword `while`.
    KwWhile,
    /// Keyword `do`.
    KwDo,
    /// Keyword `jump`.
    KwJump,
    /// Keyword `return`.
    KwReturn,

    /// Left parenthesis `(`.
    LParen,
    /// Right parenthesis `)`.
    RParen,
    /// Left brace `{`.
    LBrace,
    /// Right brace `}`.
    RBrace,
    /// Left bracket `[`.
    LBracket,
    /// Right bracket `]`.
    RBracket,
    /// Less-than/vector-literal opener `<`.
    Less,
    /// Greater-than/vector-literal closer `>`.
    Greater,
    /// Comma `,`.
    Comma,
    /// Semicolon `;`.
    Semicolon,
    /// Colon `:`.
    Colon,
    /// Label marker `@`.
    At,
    /// Question mark `?` (unsupported ternary syntax).
    Question,
    /// Member/component selector `.`.
    Dot,

    /// Addition or unary plus `+`.
    Plus,
    /// Subtraction or unary minus `-`.
    Minus,
    /// Multiplication `*`.
    Star,
    /// Division `/`.
    Slash,
    /// Remainder `%`.
    Percent,
    /// Increment `++`.
    PlusPlus,
    /// Decrement `--`.
    MinusMinus,
    /// Compound assignment `+=`.
    PlusEqual,
    /// Compound assignment `-=`.
    MinusEqual,
    /// Compound assignment `*=`.
    StarEqual,
    /// Compound assignment `/=`.
    SlashEqual,
    /// Compound assignment `%=`.
    PercentEqual,
    /// Assignment `=`.
    Equal,
    /// Equality `==`.
    EqualEqual,
    /// Logical negation `!`.
    Bang,
    /// Inequality `!=`.
    BangEqual,
    /// Less-than-or-equal `<=`.
    LessEqual,
    /// Greater-than-or-equal `>=`.
    GreaterEqual,
    /// Bitwise AND `&`.
    And,
    /// Bitwise OR `|`.
    Or,
    /// Bitwise XOR `^`.
    Xor,
    /// Bitwise NOT `~`.
    Tilde,
    /// Logical AND `&&`.
    AndAnd,
    /// Logical OR `||`.
    OrOr,
    /// Left shift `<<`.
    ShiftLeft,
    /// Right shift `>>`.
    ShiftRight,

    /// End-of-input sentinel.
    Eof,
}

/// One token produced by the lexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Token category.
    pub kind: TokenKind,
    /// Original source lexeme, including quotes/escapes for strings.
    pub lexeme: String,
    /// Source range and one-based start/end coordinates.
    pub span: Span,
}

/// Single-pass scanner state over borrowed UTF-8 source.
struct Lexer<'a> {
    source: &'a str,
    offset: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Scan until EOF, retaining recovered tokens and lexical diagnostics.
    fn run(mut self) -> (Vec<Token>, Vec<Diagnostic>) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
                continue;
            }

            if ch == '/' && self.peek_next() == Some('/') {
                self.skip_line_comment();
                continue;
            }

            if ch == '/' && self.peek_next() == Some('*') {
                self.skip_block_comment();
                continue;
            }

            let start = self.mark();

            if is_identifier_start(ch) {
                self.scan_identifier(start);
            } else if ch.is_ascii_digit()
                || (ch == '.' && self.peek_next().is_some_and(|next| next.is_ascii_digit()))
            {
                self.scan_number(start);
            } else if ch == '"' {
                self.scan_string(start);
            } else {
                self.scan_punctuation(start);
            }
        }

        let end = self.mark();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            lexeme: String::new(),
            span: self.span_from(end),
        });

        (self.tokens, self.diagnostics)
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.offset..)?.chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        self.source.get(self.offset..)?.chars().nth(1)
    }

    fn peek_nth(&self, index: usize) -> Option<char> {
        self.source.get(self.offset..)?.chars().nth(index)
    }

    fn mark(&self) -> Mark {
        Mark {
            offset: self.offset,
            line: self.line,
            column: self.column,
        }
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();

        if ch == '\r' {
            // Treat CRLF as one logical newline while retaining byte-accurate spans.
            if self.peek() == Some('\n') {
                self.offset += 1;
            }
            self.line += 1;
            self.column = 1;
        } else if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }

        Some(ch)
    }

    fn consume_if(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn lexeme_from(&self, start: Mark) -> String {
        self.source[start.offset..self.offset].to_owned()
    }

    fn span_from(&self, start: Mark) -> Span {
        Span {
            start: start.offset,
            end: self.offset,
            line: start.line,
            column: start.column,
            end_line: self.line,
            end_column: self.column,
        }
    }

    fn emit(&mut self, kind: TokenKind, start: Mark) {
        self.tokens.push(Token {
            kind,
            lexeme: self.lexeme_from(start),
            span: self.span_from(start),
        });
    }

    fn scan_identifier(&mut self, start: Mark) {
        self.advance();
        while self.peek().is_some_and(is_identifier_continue) {
            self.advance();
        }

        let text = &self.source[start.offset..self.offset];
        let kind = match text {
            "integer" => TokenKind::KwInteger,
            "float" => TokenKind::KwFloat,
            "string" => TokenKind::KwString,
            "key" => TokenKind::KwKey,
            "list" => TokenKind::KwList,
            "vector" => TokenKind::KwVector,
            "rotation" => TokenKind::KwRotation,
            "void" => TokenKind::KwVoid,
            "default" => TokenKind::KwDefault,
            "state" => TokenKind::KwState,
            "if" => TokenKind::KwIf,
            "else" => TokenKind::KwElse,
            "for" => TokenKind::KwFor,
            "while" => TokenKind::KwWhile,
            "do" => TokenKind::KwDo,
            "jump" => TokenKind::KwJump,
            "return" => TokenKind::KwReturn,
            _ => TokenKind::Identifier,
        };

        self.emit(kind, start);
    }

    fn scan_number(&mut self, start: Mark) {
        if self.peek() == Some('0')
            && matches!(self.peek_next(), Some('x' | 'X'))
            && self.peek_nth(2).is_some_and(|ch| ch.is_ascii_hexdigit())
        {
            self.advance();
            self.advance();
            self.consume_hex_digits();
            self.emit(TokenKind::IntegerLiteral, start);
            return;
        }

        let mut is_float = false;

        if self.peek() == Some('.') {
            is_float = true;
            self.advance();
            self.consume_digits();
        } else {
            self.consume_digits();
            if self.peek() == Some('.') {
                is_float = true;
                self.advance();
                self.consume_digits();
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            let exponent_start = self.mark();
            self.advance();
            if matches!(self.peek(), Some('+' | '-')) {
                self.advance();
            }

            if self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                is_float = true;
                self.consume_digits();
            } else {
                // Do not swallow a malformed exponent; it remains a separate token.
                self.offset = exponent_start.offset;
                self.line = exponent_start.line;
                self.column = exponent_start.column;
            }
        }

        self.emit(
            if is_float {
                TokenKind::FloatLiteral
            } else {
                TokenKind::IntegerLiteral
            },
            start,
        );
    }

    fn consume_digits(&mut self) {
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.advance();
        }
    }

    fn consume_hex_digits(&mut self) {
        while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
            self.advance();
        }
    }

    fn scan_string(&mut self, start: Mark) {
        self.advance(); // Opening quote.
        let mut terminated = false;

        while let Some(ch) = self.peek() {
            match ch {
                '"' => {
                    self.advance();
                    terminated = true;
                    break;
                }
                '\\' => {
                    self.advance();
                    if self.peek().is_some() {
                        self.advance();
                    }
                }
                '\n' | '\r' => break,
                _ => {
                    self.advance();
                }
            }
        }

        if terminated {
            self.emit(TokenKind::StringLiteral, start);
        } else {
            self.diagnostics.push(Diagnostic::lexical(
                "LEX001",
                "unterminated string literal",
                self.span_from(start),
            ));
        }
    }

    fn skip_line_comment(&mut self) {
        self.advance();
        self.advance();
        while !matches!(self.peek(), None | Some('\n' | '\r')) {
            self.advance();
        }
    }

    fn skip_block_comment(&mut self) {
        let start = self.mark();
        self.advance();
        self.advance();

        while let Some(ch) = self.peek() {
            if ch == '*' && self.peek_next() == Some('/') {
                self.advance();
                self.advance();
                return;
            }
            self.advance();
        }

        self.diagnostics.push(Diagnostic::lexical(
            "LEX002",
            "unterminated block comment",
            self.span_from(start),
        ));
    }

    fn scan_punctuation(&mut self, start: Mark) {
        let first = self
            .advance()
            .expect("scan_punctuation is called at a character");

        let kind = match first {
            '(' => Some(TokenKind::LParen),
            ')' => Some(TokenKind::RParen),
            '{' => Some(TokenKind::LBrace),
            '}' => Some(TokenKind::RBrace),
            '[' => Some(TokenKind::LBracket),
            ']' => Some(TokenKind::RBracket),
            ',' => Some(TokenKind::Comma),
            ';' => Some(TokenKind::Semicolon),
            ':' => Some(TokenKind::Colon),
            '@' => Some(TokenKind::At),
            '?' => Some(TokenKind::Question),
            '.' => Some(TokenKind::Dot),

            '+' => Some(if self.consume_if('+') {
                TokenKind::PlusPlus
            } else if self.consume_if('=') {
                TokenKind::PlusEqual
            } else {
                TokenKind::Plus
            }),
            '-' => Some(if self.consume_if('-') {
                TokenKind::MinusMinus
            } else if self.consume_if('=') {
                TokenKind::MinusEqual
            } else {
                TokenKind::Minus
            }),
            '*' => Some(if self.consume_if('=') {
                TokenKind::StarEqual
            } else {
                TokenKind::Star
            }),
            '/' => Some(if self.consume_if('=') {
                TokenKind::SlashEqual
            } else {
                TokenKind::Slash
            }),
            '%' => Some(if self.consume_if('=') {
                TokenKind::PercentEqual
            } else {
                TokenKind::Percent
            }),
            '=' => Some(if self.consume_if('=') {
                TokenKind::EqualEqual
            } else {
                TokenKind::Equal
            }),
            '!' => Some(if self.consume_if('=') {
                TokenKind::BangEqual
            } else {
                TokenKind::Bang
            }),
            '<' => Some(if self.consume_if('<') {
                TokenKind::ShiftLeft
            } else if self.consume_if('=') {
                TokenKind::LessEqual
            } else {
                TokenKind::Less
            }),
            '>' => Some(if self.consume_if('>') {
                TokenKind::ShiftRight
            } else if self.consume_if('=') {
                TokenKind::GreaterEqual
            } else {
                TokenKind::Greater
            }),
            '&' => Some(if self.consume_if('&') {
                TokenKind::AndAnd
            } else {
                TokenKind::And
            }),
            '|' => Some(if self.consume_if('|') {
                TokenKind::OrOr
            } else {
                TokenKind::Or
            }),
            '^' => Some(TokenKind::Xor),
            '~' => Some(TokenKind::Tilde),
            _ => None,
        };

        if let Some(kind) = kind {
            self.emit(kind, start);
        } else {
            self.diagnostics.push(Diagnostic::lexical(
                "LEX000",
                format!("unexpected character {first:?}"),
                self.span_from(start),
            ));
        }
    }
}

#[derive(Clone, Copy)]
struct Mark {
    offset: usize,
    line: usize,
    column: usize,
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

/// Tokenize LSL source and return tokens plus lexical diagnostics.
/// Lex an LSL source string.
///
/// # Arguments
///
/// * `source` — complete UTF-8 source text.
///
/// # Returns
///
/// A token vector ending in [`TokenKind::Eof`] plus lexical diagnostics. Tokens
/// recovered before an error are retained; the lexer does not panic on malformed
/// input.
///
/// # Errors
///
/// Lexical problems are returned as diagnostics rather than Rust errors:
/// [`crate::diagnostic::DiagnosticKind::Lexical`] with `LEX000`–`LEX002`.
pub fn lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    Lexer::new(source).run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        lex(source).0.into_iter().map(|token| token.kind).collect()
    }

    #[test]
    fn recognizes_lsl_keywords_and_identifiers() {
        assert_eq!(
            kinds("integer count; default { state running {} }"),
            vec![
                TokenKind::KwInteger,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::KwDefault,
                TokenKind::LBrace,
                TokenKind::KwState,
                TokenKind::Identifier,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn recognizes_integer_float_and_exponent_literals() {
        assert_eq!(
            kinds("42 3.14 .5 1e-3 2E+4"),
            vec![
                TokenKind::IntegerLiteral,
                TokenKind::FloatLiteral,
                TokenKind::FloatLiteral,
                TokenKind::FloatLiteral,
                TokenKind::FloatLiteral,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn skips_line_and_block_comments() {
        assert_eq!(
            kinds("integer x; // ignored\n/* ignored */ float y;"),
            vec![
                TokenKind::KwInteger,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::KwFloat,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn recognizes_longest_operator_first() {
        assert_eq!(
            kinds("a << 2; b != 0 && c >= 1;"),
            vec![
                TokenKind::Identifier,
                TokenKind::ShiftLeft,
                TokenKind::IntegerLiteral,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::BangEqual,
                TokenKind::IntegerLiteral,
                TokenKind::AndAnd,
                TokenKind::Identifier,
                TokenKind::GreaterEqual,
                TokenKind::IntegerLiteral,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn splits_unsupported_compound_operators_into_existing_tokens() {
        assert_eq!(
            kinds("a &= 1; b |= 2; c ^= 3; d <<= 4; e >>= 5;"),
            vec![
                TokenKind::Identifier,
                TokenKind::And,
                TokenKind::Equal,
                TokenKind::IntegerLiteral,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::Or,
                TokenKind::Equal,
                TokenKind::IntegerLiteral,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::Xor,
                TokenKind::Equal,
                TokenKind::IntegerLiteral,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::ShiftLeft,
                TokenKind::Equal,
                TokenKind::IntegerLiteral,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::ShiftRight,
                TokenKind::Equal,
                TokenKind::IntegerLiteral,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_literal_keeps_escaped_quotes() {
        let (tokens, diagnostics) = lex(r#""hello \"world\"""#);
        assert!(diagnostics.is_empty());
        assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
        assert_eq!(tokens[0].lexeme, r#""hello \"world\"""#);
    }

    #[test]
    fn unterminated_string_reports_location() {
        let (tokens, diagnostics) = lex("integer x;\nstring s = \"broken");
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "LEX001");
        assert_eq!(diagnostics[0].span.line, 2);
        assert_eq!(diagnostics[0].span.column, 12);
    }

    #[test]
    fn unterminated_block_comment_reports_error() {
        let (_, diagnostics) = lex("/* never closed");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "LEX002");
    }

    #[test]
    fn recognizes_hex_integer_literals() {
        let (tokens, diagnostics) = lex("0x0 0X2A 0xFFFFFFFF 0x80000000 0x7FFFFFFF 0x1e3");
        assert!(diagnostics.is_empty());
        assert_eq!(
            tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::IntegerLiteral,
                TokenKind::IntegerLiteral,
                TokenKind::IntegerLiteral,
                TokenKind::IntegerLiteral,
                TokenKind::IntegerLiteral,
                TokenKind::IntegerLiteral,
                TokenKind::Eof,
            ]
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::IntegerLiteral)
                .map(|token| token.lexeme.as_str())
                .collect::<Vec<_>>(),
            vec![
                "0x0",
                "0X2A",
                "0xFFFFFFFF",
                "0x80000000",
                "0x7FFFFFFF",
                "0x1e3"
            ]
        );
    }

    #[test]
    fn hex_integer_literals_do_not_consume_exponents_or_identifiers() {
        let (tokens, diagnostics) = lex("0x1e+2 0x1e-3 0x1G 0x1.2");
        assert!(diagnostics.is_empty());
        assert_eq!(
            tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::IntegerLiteral,
                TokenKind::Plus,
                TokenKind::IntegerLiteral,
                TokenKind::IntegerLiteral,
                TokenKind::Minus,
                TokenKind::IntegerLiteral,
                TokenKind::IntegerLiteral,
                TokenKind::Identifier,
                TokenKind::IntegerLiteral,
                TokenKind::FloatLiteral,
                TokenKind::Eof,
            ]
        );
        let lexemes = tokens
            .iter()
            .map(|token| token.lexeme.as_str())
            .collect::<Vec<_>>();
        assert_eq!(lexemes[0], "0x1e");
        assert_eq!(lexemes[1], "+");
        assert_eq!(lexemes[2], "2");
        assert_eq!(lexemes[3], "0x1e");
        assert_eq!(lexemes[4], "-");
        assert_eq!(lexemes[5], "3");
        assert_eq!(lexemes[6], "0x1");
        assert_eq!(lexemes[7], "G");
        assert_eq!(lexemes[8], "0x1");
        assert_eq!(lexemes[9], ".2");
    }

    #[test]
    fn malformed_hex_prefixes_fall_back_to_decimal_and_identifier_tokens() {
        let (tokens, diagnostics) = lex("0x 0xG 0X_ 0x.");
        assert!(diagnostics.is_empty());
        assert_eq!(
            tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::IntegerLiteral,
                TokenKind::Identifier,
                TokenKind::IntegerLiteral,
                TokenKind::Identifier,
                TokenKind::IntegerLiteral,
                TokenKind::Identifier,
                TokenKind::IntegerLiteral,
                TokenKind::Identifier,
                TokenKind::Dot,
                TokenKind::Eof,
            ]
        );
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.lexeme.as_str())
                .collect::<Vec<_>>(),
            vec!["0", "x", "0", "xG", "0", "X_", "0", "x", ".", ""]
        );
    }

    #[test]
    fn hex_integer_literal_spans_cover_the_original_lexeme() {
        let (tokens, diagnostics) = lex("integer x = 0x1;");
        assert!(diagnostics.is_empty());
        let hex = tokens
            .iter()
            .find(|token| token.lexeme == "0x1")
            .expect("hex literal token");
        assert_eq!(hex.span.start, 12);
        assert_eq!(hex.span.end, 15);
        assert_eq!(hex.span.line, 1);
        assert_eq!(hex.span.column, 13);
    }

    #[test]
    fn handles_crlf_and_unicode_in_comments() {
        let (tokens, diagnostics) = lex("// привет\r\ninteger x;");
        assert!(diagnostics.is_empty());
        assert_eq!(tokens[0].span.line, 2);
        assert_eq!(tokens[0].span.column, 1);
        assert_eq!(tokens[1].span.column, 9);
    }

    #[test]
    fn unexpected_unicode_outside_string_is_reported_without_panic() {
        let (_, diagnostics) = lex("integer café;");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "LEX000");
    }
}
