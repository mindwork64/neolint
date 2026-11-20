use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
use crate::lexer::{Token, TokenKind};

struct Parser<'a> {
    tokens: &'a [Token],
    current: usize,
    diagnostics: Vec<Diagnostic>,
    states_started: bool,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            current: 0,
            diagnostics: Vec::new(),
            states_started: false,
        }
    }

    fn run(mut self) -> Vec<Diagnostic> {
        while !self.at_end() {
            let before = self.current;
            if self.is_type() || self.is_untyped_function_declaration() {
                if self.states_started && self.is_function_declaration() {
                    self.error_here(
                        "PAR009",
                        "user-defined functions must be declared before state definitions",
                    );
                }
                self.parse_global_declaration();
            } else if self.check(TokenKind::KwDefault) || self.check(TokenKind::KwState) {
                self.states_started = true;
                self.parse_state();
            } else {
                self.error_here(
                    "PAR001",
                    "expected a global declaration or state definition",
                );
                self.synchronize_top_level();
            }
            if self.current == before {
                self.advance();
            }
        }
        self.diagnostics
    }

    fn parse_global_declaration(&mut self) {
        // A value type before the name denotes a returning function or global variable.
        // LSL void functions omit the return type entirely.
        if self.is_type() {
            self.advance();
        }

        if !self.expect(TokenKind::Identifier, "expected a declaration name") {
            self.synchronize_top_level();
            return;
        }

        if self.matches(TokenKind::LParen) {
            self.parse_parameters();
            self.parse_block();
        } else {
            self.finish_variable_declaration();
        }
    }

    fn finish_variable_declaration(&mut self) {
        if self.matches(TokenKind::Equal) {
            self.parse_expression(1);
        }
        while self.matches(TokenKind::Comma) {
            self.expect(TokenKind::Identifier, "expected a variable name after ','");
            if self.matches(TokenKind::Equal) {
                self.parse_expression(1);
            }
        }
        self.expect(
            TokenKind::Semicolon,
            "expected ';' after global declaration",
        );
    }

    fn parse_parameters(&mut self) {
        if self.matches(TokenKind::RParen) {
            return;
        }
        loop {
            if !self.parse_type() {
                self.error_here("PAR003", "expected a parameter type");
                self.synchronize_parameters();
                return;
            }
            self.expect(TokenKind::Identifier, "expected a parameter name");
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "expected ')' after parameter list");
    }

    fn parse_state(&mut self) {
        if self.matches(TokenKind::KwDefault) {
            // The default state has no explicit name.
        } else {
            self.advance(); // state
            self.expect(TokenKind::Identifier, "expected a state name after 'state'");
        }

        if !self.expect(TokenKind::LBrace, "expected '{' to begin state body") {
            self.synchronize_top_level();
            return;
        }

        while !self.at_end() && !self.check(TokenKind::RBrace) {
            let before = self.current;
            if !self.check(TokenKind::Identifier) {
                self.error_here("PAR004", "expected an event handler inside state");
                self.synchronize_state();
            } else {
                self.advance(); // event name
                if self.expect(TokenKind::LParen, "expected '(' after event name") {
                    self.parse_parameters();
                    self.parse_block();
                } else {
                    self.synchronize_state();
                }
            }
            if self.current == before {
                self.advance();
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' after state body");
    }

    fn parse_block(&mut self) {
        if !self.expect(TokenKind::LBrace, "expected '{' to begin block") {
            return;
        }
        while !self.at_end() && !self.check(TokenKind::RBrace) {
            let before = self.current;
            self.parse_statement();
            if self.current == before {
                self.advance();
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' after block");
    }

    fn parse_statement(&mut self) {
        if self.check(TokenKind::LBrace) {
            self.parse_block();
        } else if self.matches(TokenKind::KwIf) {
            self.parse_if_statement();
        } else if self.matches(TokenKind::KwFor) {
            self.parse_for_statement();
        } else if self.matches(TokenKind::KwWhile) {
            self.parse_while_statement();
        } else if self.matches(TokenKind::KwDo) {
            self.parse_do_while_statement();
        } else if self.matches(TokenKind::KwReturn) {
            if !self.check(TokenKind::Semicolon) {
                self.parse_expression(1);
            }
            self.expect(TokenKind::Semicolon, "expected ';' after return statement");
        } else if self.matches(TokenKind::KwJump) {
            self.expect(TokenKind::Identifier, "expected a label name after 'jump'");
            self.expect(TokenKind::Semicolon, "expected ';' after jump statement");
        } else if self.matches(TokenKind::KwState) {
            self.expect(TokenKind::Identifier, "expected a state name after 'state'");
            self.expect(TokenKind::Semicolon, "expected ';' after state change");
        } else if self.matches(TokenKind::At) {
            self.expect(TokenKind::Identifier, "expected a label name after '@'");
            self.expect(TokenKind::Semicolon, "expected ';' after label");
        } else if self.is_type() {
            self.advance();
            self.parse_local_declaration();
        } else {
            self.parse_expression(1);
            self.expect(TokenKind::Semicolon, "expected ';' after expression");
        }
    }

    fn parse_if_statement(&mut self) {
        self.expect(TokenKind::LParen, "expected '(' after 'if'");
        self.parse_expression(1);
        self.expect(TokenKind::RParen, "expected ')' after if condition");
        self.parse_statement();
        if self.matches(TokenKind::KwElse) {
            self.parse_statement();
        }
    }

    fn parse_for_statement(&mut self) {
        self.expect(TokenKind::LParen, "expected '(' after 'for'");
        if self.is_type() {
            self.error_here(
                "PAR008",
                "LSL for-loop initializers must be expressions; declare variables before the loop",
            );
            while !self.at_end()
                && !self.check(TokenKind::Semicolon)
                && !self.check(TokenKind::RParen)
            {
                self.advance();
            }
        } else if !self.check(TokenKind::Semicolon) {
            self.parse_expression(1);
        }
        self.expect(TokenKind::Semicolon, "expected ';' after for initializer");
        if !self.check(TokenKind::Semicolon) {
            self.parse_expression(1);
        }
        self.expect(TokenKind::Semicolon, "expected ';' after for condition");
        if !self.check(TokenKind::RParen) {
            self.parse_expression(1);
        }
        self.expect(TokenKind::RParen, "expected ')' after for clauses");
        self.parse_statement();
    }

    fn parse_while_statement(&mut self) {
        self.expect(TokenKind::LParen, "expected '(' after 'while'");
        self.parse_expression(1);
        self.expect(TokenKind::RParen, "expected ')' after while condition");
        self.parse_statement();
    }

    fn parse_do_while_statement(&mut self) {
        self.parse_statement();
        self.expect(TokenKind::KwWhile, "expected 'while' after do-body");
        self.expect(TokenKind::LParen, "expected '(' after 'while'");
        self.parse_expression(1);
        self.expect(TokenKind::RParen, "expected ')' after do-while condition");
        self.expect(
            TokenKind::Semicolon,
            "expected ';' after do-while statement",
        );
    }

    fn parse_local_declaration(&mut self) {
        self.expect(TokenKind::Identifier, "expected a local variable name");
        if self.matches(TokenKind::Equal) {
            self.parse_expression(1);
        }
        while self.matches(TokenKind::Comma) {
            self.expect(TokenKind::Identifier, "expected a variable name after ','");
            if self.matches(TokenKind::Equal) {
                self.parse_expression(1);
            }
        }
        self.expect(TokenKind::Semicolon, "expected ';' after local declaration");
    }

    // Precedence climbing validates expressions without constructing the AST yet.
    fn parse_expression(&mut self, min_precedence: u8) -> bool {
        if !self.parse_prefix() {
            return false;
        }
        loop {
            let Some((precedence, right_associative)) = binary_precedence(self.peek_kind()) else {
                break;
            };
            if precedence < min_precedence {
                break;
            }
            self.advance();
            let next_min = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            if !self.parse_expression(next_min) {
                self.error_here("PAR005", "expected an expression after operator");
                break;
            }
        }
        true
    }

    fn parse_prefix(&mut self) -> bool {
        if matches!(
            self.peek_kind(),
            TokenKind::Bang
                | TokenKind::Tilde
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::PlusPlus
                | TokenKind::MinusMinus
        ) {
            self.advance();
            return self.parse_prefix();
        }

        if self.check(TokenKind::LParen)
            && is_type_token(self.peek_kind_at(1))
            && self.peek_kind_at(2) == TokenKind::RParen
        {
            self.advance();
            self.advance();
            self.advance();
            return self.parse_prefix();
        }

        if !self.parse_primary() {
            return false;
        }
        loop {
            if self.matches(TokenKind::LParen) {
                self.parse_argument_list();
            } else if self.matches(TokenKind::Dot) {
                self.expect(TokenKind::Identifier, "expected a member name after '.'");
            } else if self.matches(TokenKind::PlusPlus) || self.matches(TokenKind::MinusMinus) {
                // postfix increment/decrement
            } else {
                break;
            }
        }
        true
    }

    fn parse_primary(&mut self) -> bool {
        match self.peek_kind() {
            TokenKind::Identifier
            | TokenKind::IntegerLiteral
            | TokenKind::FloatLiteral
            | TokenKind::StringLiteral => {
                self.advance();
                true
            }
            TokenKind::LParen => {
                self.advance();
                self.parse_expression(1);
                self.expect(TokenKind::RParen, "expected ')' after expression");
                true
            }
            TokenKind::LBracket => {
                self.advance();
                self.parse_delimited_expressions(TokenKind::RBracket);
                true
            }
            TokenKind::Less => {
                self.advance();
                // A top-level '>' closes the vector/rotation literal. Use a
                // precedence floor above relational operators so it is not
                // consumed as a comparison while parsing a component.
                self.parse_expression(10);
                self.expect(
                    TokenKind::Comma,
                    "expected ',' after first vector component",
                );
                self.parse_expression(10);
                self.expect(
                    TokenKind::Comma,
                    "expected ',' after second vector component",
                );
                self.parse_expression(10);
                if self.matches(TokenKind::Comma) {
                    self.parse_expression(10);
                }
                self.expect(
                    TokenKind::Greater,
                    "expected '>' after vector or rotation literal",
                );
                true
            }
            _ => {
                self.error_here("PAR006", "expected an expression");
                false
            }
        }
    }

    fn parse_argument_list(&mut self) {
        if self.matches(TokenKind::RParen) {
            return;
        }
        loop {
            self.parse_expression(1);
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "expected ')' after arguments");
    }

    fn parse_delimited_expressions(&mut self, closing: TokenKind) {
        if self.matches(closing) {
            return;
        }
        loop {
            self.parse_expression(1);
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        self.expect(closing, "expected closing delimiter");
    }

    fn parse_type(&mut self) -> bool {
        if is_type_token(self.peek_kind()) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn is_type(&self) -> bool {
        is_type_token(self.peek_kind())
    }

    fn is_untyped_function_declaration(&self) -> bool {
        self.check(TokenKind::Identifier) && self.peek_kind_at(1) == TokenKind::LParen
    }

    fn is_function_declaration(&self) -> bool {
        if self.is_type() {
            self.peek_kind_at(1) == TokenKind::Identifier
                && self.peek_kind_at(2) == TokenKind::LParen
        } else {
            self.is_untyped_function_declaration()
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &'static str) -> bool {
        if self.matches(kind) {
            true
        } else {
            self.error_here("PAR007", message);
            false
        }
    }

    fn matches(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn peek_kind(&self) -> TokenKind {
        self.peek_kind_at(0)
    }

    fn peek_kind_at(&self, distance: usize) -> TokenKind {
        self.tokens
            .get(self.current + distance)
            .map(|token| token.kind)
            .unwrap_or(TokenKind::Eof)
    }

    fn at_end(&self) -> bool {
        self.check(TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        if !self.at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current.saturating_sub(1)]
    }

    fn error_here(&mut self, code: &'static str, message: impl Into<String>) {
        let span = self
            .tokens
            .get(self.current)
            .or_else(|| self.tokens.last())
            .map(|token| token.span)
            .unwrap_or(Span {
                start: 0,
                end: 0,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            });
        self.push_error(code, message, span);
    }

    fn push_error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic {
            code,
            severity: Severity::Error,
            kind: DiagnosticKind::Syntax,
            message: message.into(),
            span,
        });
    }

    fn synchronize_top_level(&mut self) {
        while !self.at_end() {
            if self.current > 0 && self.previous().kind == TokenKind::RBrace {
                return;
            }
            if self.is_type()
                || self.check(TokenKind::KwVoid)
                || self.check(TokenKind::KwDefault)
                || self.check(TokenKind::KwState)
            {
                return;
            }
            self.advance();
        }
    }

    fn synchronize_state(&mut self) {
        while !self.at_end() && !self.check(TokenKind::RBrace) {
            if self.current > 0 && self.previous().kind == TokenKind::RBrace {
                return;
            }
            if self.check(TokenKind::Identifier) {
                return;
            }
            self.advance();
        }
    }

    fn synchronize_parameters(&mut self) {
        while !self.at_end() && !self.check(TokenKind::RParen) {
            if self.matches(TokenKind::Comma) {
                return;
            }
            self.advance();
        }
    }
}

fn is_type_token(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::KwInteger
            | TokenKind::KwFloat
            | TokenKind::KwString
            | TokenKind::KwKey
            | TokenKind::KwList
            | TokenKind::KwVector
            | TokenKind::KwRotation
    )
}

fn binary_precedence(kind: TokenKind) -> Option<(u8, bool)> {
    let precedence = match kind {
        TokenKind::Equal
        | TokenKind::PlusEqual
        | TokenKind::MinusEqual
        | TokenKind::StarEqual
        | TokenKind::SlashEqual
        | TokenKind::PercentEqual
        | TokenKind::AndEqual
        | TokenKind::OrEqual
        | TokenKind::XorEqual
        | TokenKind::ShiftLeftEqual
        | TokenKind::ShiftRightEqual => return Some((1, true)),
        TokenKind::OrOr => 3,
        TokenKind::AndAnd => 4,
        TokenKind::Or => 5,
        TokenKind::Xor => 6,
        TokenKind::And => 7,
        TokenKind::EqualEqual | TokenKind::BangEqual => 8,
        TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual => 9,
        TokenKind::ShiftLeft | TokenKind::ShiftRight => 10,
        TokenKind::Plus | TokenKind::Minus => 11,
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => 12,
        _ => return None,
    };
    Some((precedence, false))
}

/// Parse a token stream and return syntax diagnostics. Lexical diagnostics are
/// produced separately by lexing and must be retained by the caller.
pub fn parse(tokens: &[Token]) -> Vec<Diagnostic> {
    Parser::new(tokens).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex;

    fn parse_source(source: &str) -> Vec<Diagnostic> {
        let (tokens, lexical_diagnostics) = lex(source);
        assert!(lexical_diagnostics.is_empty(), "{lexical_diagnostics:?}");
        parse(&tokens)
    }

    #[test]
    fn accepts_default_state_with_multiple_events() {
        let diagnostics = parse_source(
            r#"
            integer gCount = 0;
            default
            {
                state_entry() { llOwnerSay("ready"); }
                touch_start(integer total) { gCount += total; }
            }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_user_functions_declared_after_states() {
        let diagnostics = parse_source("default { state_entry() { } } later() { }");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "PAR009"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_user_functions_without_return_type() {
        let diagnostics = parse_source(
            r#"
            say_message(string message) { llOwnerSay(message); }
            default { state_entry() { say_message("ready"); } }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_void_as_a_parameter_list_marker() {
        let diagnostics = parse_source("integer value(void) { return 1; } default { }");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "PAR003"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_explicit_void_return_type() {
        let diagnostics =
            parse_source("void say_message(string message) { llOwnerSay(message); } default {}");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "PAR001"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_user_functions_and_named_states() {
        let diagnostics = parse_source(
            r#"
            float add(float a, float b) { return a + b; }
            state running
            {
                state_entry() { vector p = <1.0, 2.0, 3.0>; }
            }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn accepts_lists_casts_loops_and_conditionals() {
        let diagnostics = parse_source(
            r#"
            default
            {
                state_entry()
                {
                    list values = [1, "two", <1, 2, 3>];
                    integer i = 0;
                    for (i = 0; i < 3; i++)
                    {
                        if (i != 2) { llOwnerSay((string)i); }
                        else { llOwnerSay("last"); }
                    }
                    while (i > 0) i -= 1;
                    do { i += 1; } while (i < 1);
                    return;
                }
            }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_variable_declarations_in_for_initializers() {
        let diagnostics =
            parse_source("default { state_entry() { for (integer i = 0; i < 3; ++i) { } } }");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "PAR008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_missing_semicolon() {
        let diagnostics = parse_source("default { state_entry() { integer x = 1 } }");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "PAR007"));
    }

    #[test]
    fn accepts_global_declarations_without_state_during_parser_pass() {
        let diagnostics = parse_source("integer x = 1;");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn expression_parser_accepts_nested_calls_and_logical_conditions() {
        let diagnostics = parse_source(
            r#"
            default
            {
                state_entry()
                {
                    integer x = (1 + 2) > 2 && llAbs(-1) > 0;
                }
            }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_unsupported_ternary_operator() {
        let diagnostics = parse_source("default { state_entry() { integer value = 1 ? 2 : 3; } }");
        assert!(!diagnostics.is_empty(), "ternary syntax is not part of LSL");
    }
}
