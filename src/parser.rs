use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
use crate::lexer::{Token, TokenKind};

struct Parser<'a> {
    tokens: &'a [Token],
    current: usize,
    diagnostics: Vec<Diagnostic>,
    states_started: bool,
    default_state_seen: bool,
    missing_default_reported: bool,
    eof_reported: bool,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            current: 0,
            diagnostics: Vec::new(),
            states_started: false,
            default_state_seen: false,
            missing_default_reported: false,
            eof_reported: false,
        }
    }

    fn run(mut self) -> Vec<Diagnostic> {
        while !self.at_end() {
            let before = self.current;
            if self.is_type() || self.is_untyped_function_declaration() {
                if self.states_started {
                    self.error_here(
                        "PAR009",
                        "global declarations and user-defined functions must be declared before state definitions",
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

        if !self.default_state_seen && !self.missing_default_reported {
            self.error_here("PAR013", "the default state must be defined");
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
            if self.parse_parameters() {
                self.parse_block();
            } else {
                self.synchronize_top_level();
            }
        } else {
            self.finish_variable_declaration();
        }
    }

    fn finish_variable_declaration(&mut self) {
        if self.matches(TokenKind::Equal) && !self.parse_expression(1) {
            self.synchronize_statement();
            return;
        }
        while self.matches(TokenKind::Comma) {
            self.expect(TokenKind::Identifier, "expected a variable name after ','");
            if self.matches(TokenKind::Equal) && !self.parse_expression(1) {
                self.synchronize_statement();
                return;
            }
        }
        self.expect(
            TokenKind::Semicolon,
            "expected ';' after global declaration",
        );
    }

    fn parse_parameters(&mut self) -> bool {
        if self.matches(TokenKind::RParen) {
            return true;
        }
        loop {
            if !self.parse_type() {
                self.error_here("PAR003", "expected a parameter type");
                self.synchronize_parameters();
                return false;
            }
            if !self.expect(TokenKind::Identifier, "expected a parameter name") {
                self.synchronize_parameters();
                return false;
            }
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "expected ')' after parameter list")
    }

    fn parse_state(&mut self) {
        if self.check(TokenKind::KwDefault) {
            if self.default_state_seen {
                self.error_here("PAR014", "only one default state is allowed");
            }
            self.advance();
            self.default_state_seen = true;
        } else {
            if !self.default_state_seen && !self.missing_default_reported {
                self.error_here(
                    "PAR013",
                    "the default state must be defined before other states",
                );
                self.missing_default_reported = true;
            }
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
                self.synchronize_event();
            } else {
                self.advance(); // event name
                let header_ok = if self.expect(TokenKind::LParen, "expected '(' after event name") {
                    self.parse_parameters()
                } else {
                    false
                };
                if header_ok {
                    self.parse_block();
                } else {
                    self.synchronize_event();
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
            let parsed = self.parse_statement();
            if !parsed && self.current == before {
                self.advance();
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' after block");
    }

    fn parse_statement(&mut self) -> bool {
        if self.matches(TokenKind::Semicolon) {
            return true;
        }
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
            if !self.check(TokenKind::Semicolon) && !self.parse_expression(1) {
                self.synchronize_statement();
                return false;
            }
            self.expect(TokenKind::Semicolon, "expected ';' after return statement");
        } else if self.matches(TokenKind::KwJump) {
            self.expect(TokenKind::Identifier, "expected a label name after 'jump'");
            self.expect(TokenKind::Semicolon, "expected ';' after jump statement");
        } else if self.matches(TokenKind::KwState) {
            // `default` is a state name when used as a state-change target.
            if !self.matches(TokenKind::Identifier) && !self.matches(TokenKind::KwDefault) {
                self.error_here("PAR007", "expected a state name after 'state'");
            }
            self.expect(TokenKind::Semicolon, "expected ';' after state change");
        } else if self.matches(TokenKind::At) {
            self.expect(TokenKind::Identifier, "expected a label name after '@'");
            self.expect(TokenKind::Semicolon, "expected ';' after label");
        } else if self.is_type() {
            self.advance();
            self.parse_local_declaration();
        } else {
            let expression_start = self.current;
            if !self.parse_expression(1) {
                if self.current > expression_start {
                    self.synchronize_statement();
                }
                return false;
            }
            self.expect(TokenKind::Semicolon, "expected ';' after expression");
        }
        true
    }

    fn parse_if_statement(&mut self) {
        self.expect(TokenKind::LParen, "expected '(' after 'if'");
        if !self.parse_expression(1) {
            self.synchronize_condition();
        } else {
            self.expect(TokenKind::RParen, "expected ')' after if condition");
        }
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
            self.expect(TokenKind::Semicolon, "expected ';' after for initializer");
        } else if self.parse_for_expression_list(TokenKind::Semicolon) {
            self.expect(TokenKind::Semicolon, "expected ';' after for initializer");
        }

        if !self.parse_expression(1) {
            self.synchronize_for_condition();
        } else {
            self.expect(TokenKind::Semicolon, "expected ';' after for condition");
        }

        if self.parse_for_expression_list(TokenKind::RParen) {
            self.expect(TokenKind::RParen, "expected ')' after for clauses");
        }
        self.parse_statement();
    }

    fn parse_for_expression_list(&mut self, end: TokenKind) -> bool {
        if self.check(end) {
            return true;
        }
        if !self.parse_expression(1) {
            self.synchronize_for_list(end);
            return false;
        }
        while self.matches(TokenKind::Comma) {
            if !self.parse_expression(1) {
                self.synchronize_for_list(end);
                return false;
            }
        }
        true
    }

    fn parse_while_statement(&mut self) {
        self.expect(TokenKind::LParen, "expected '(' after 'while'");
        if !self.parse_expression(1) {
            self.synchronize_condition();
        } else {
            self.expect(TokenKind::RParen, "expected ')' after while condition");
        }
        self.parse_statement();
    }

    fn parse_do_while_statement(&mut self) {
        self.parse_statement();
        self.expect(TokenKind::KwWhile, "expected 'while' after do-body");
        self.expect(TokenKind::LParen, "expected '(' after 'while'");
        if !self.parse_expression(1) {
            self.synchronize_condition();
        } else {
            self.expect(TokenKind::RParen, "expected ')' after do-while condition");
        }
        self.expect(
            TokenKind::Semicolon,
            "expected ';' after do-while statement",
        );
    }

    fn parse_local_declaration(&mut self) {
        self.expect(TokenKind::Identifier, "expected a local variable name");
        if self.matches(TokenKind::Equal) && !self.parse_expression(1) {
            self.synchronize_statement();
            return;
        }
        while self.matches(TokenKind::Comma) {
            self.expect(TokenKind::Identifier, "expected a variable name after ','");
            if self.matches(TokenKind::Equal) && !self.parse_expression(1) {
                self.synchronize_statement();
                return;
            }
        }
        self.expect(TokenKind::Semicolon, "expected ';' after local declaration");
    }

    // Precedence climbing validates expressions without constructing the AST yet.
    fn parse_expression(&mut self, min_precedence: u8) -> bool {
        let expression_start = self.current;
        if !self.parse_prefix() {
            return false;
        }
        while let Some((precedence, right_associative)) = binary_precedence(self.peek_kind()) {
            if precedence < min_precedence {
                break;
            }

            let operator = self.peek_kind();
            if matches!(
                operator,
                TokenKind::Equal
                    | TokenKind::PlusEqual
                    | TokenKind::MinusEqual
                    | TokenKind::StarEqual
                    | TokenKind::SlashEqual
                    | TokenKind::PercentEqual
            ) && !is_assignment_lvalue(self.tokens, expression_start, self.current)
            {
                self.error_here(
                    "PAR010",
                    "left-hand side of assignment must be a variable or vector/rotation component",
                );
            }

            self.advance();
            let next_min = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            let diagnostic_count = self.diagnostics.len();
            if !self.parse_expression(next_min) {
                if self.diagnostics.len() == diagnostic_count {
                    self.error_here("PAR005", "expected an expression after operator");
                } else if self.diagnostics.len() == diagnostic_count + 1
                    && self
                        .diagnostics
                        .last()
                        .is_some_and(|diagnostic| diagnostic.code == "PAR006")
                {
                    if let Some(diagnostic) = self.diagnostics.last_mut() {
                        diagnostic.code = "PAR005";
                        diagnostic.message = "expected an expression after operator".to_owned();
                    }
                }
                return false;
            }
        }
        true
    }

    fn parse_prefix(&mut self) -> bool {
        if matches!(
            self.peek_kind(),
            TokenKind::Bang | TokenKind::Tilde | TokenKind::Plus | TokenKind::Minus
        ) {
            self.advance();
            return self.parse_prefix();
        }

        if matches!(
            self.peek_kind(),
            TokenKind::PlusPlus | TokenKind::MinusMinus
        ) {
            self.advance();
            let operand_start = self.current;
            if !self.parse_prefix() {
                return false;
            }
            if !is_variable_lvalue(self.tokens, operand_start, self.current) {
                self.error_here("PAR011", "increment/decrement operand must be a variable");
            }
            return true;
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

        let primary_start = self.current;
        let mut call_suffix_allowed = self.check(TokenKind::Identifier);
        if !self.parse_primary() {
            return false;
        }
        loop {
            if self.check(TokenKind::LParen) {
                if !call_suffix_allowed {
                    self.error_here("PAR007", "function call suffix requires an identifier");
                }
                self.advance();
                if !self.parse_argument_list() {
                    return false;
                }
                call_suffix_allowed = false;
            } else if self.matches(TokenKind::Dot) {
                let base_is_variable =
                    is_variable_lvalue(self.tokens, primary_start, self.current - 1);
                let component_is_valid = self.tokens.get(self.current).is_some_and(|token| {
                    token.kind == TokenKind::Identifier
                        && matches!(token.lexeme.as_str(), "x" | "y" | "z" | "s")
                });
                if !base_is_variable || !component_is_valid {
                    self.error_here(
                        "PAR012",
                        "vector/rotation component access requires a variable and a valid component name",
                    );
                }
                if !self.expect(TokenKind::Identifier, "expected a component name after '.'") {
                    return false;
                }
                call_suffix_allowed = false;
            } else if self.check(TokenKind::PlusPlus) || self.check(TokenKind::MinusMinus) {
                if !is_variable_lvalue(self.tokens, primary_start, self.current) {
                    self.error_here("PAR011", "increment/decrement operand must be a variable");
                }
                self.advance();
                call_suffix_allowed = false;
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
                if !self.parse_expression(1) {
                    return false;
                }
                self.expect(TokenKind::RParen, "expected ')' after expression")
            }
            TokenKind::LBracket => {
                self.advance();
                self.parse_delimited_expressions(TokenKind::RBracket)
            }
            TokenKind::Less => {
                self.advance();
                // A top-level '>' closes the vector/rotation literal. Use a
                // precedence floor above relational operators so it is not
                // consumed as a comparison while parsing a component.
                if !self.parse_expression(10) {
                    return false;
                }
                if !self.expect(
                    TokenKind::Comma,
                    "expected ',' after first vector component",
                ) {
                    return false;
                }
                if !self.parse_expression(10) {
                    return false;
                }
                if !self.expect(
                    TokenKind::Comma,
                    "expected ',' after second vector component",
                ) {
                    return false;
                }
                if !self.parse_expression(10) {
                    return false;
                }
                if self.matches(TokenKind::Comma) && !self.parse_expression(10) {
                    return false;
                }
                self.expect(
                    TokenKind::Greater,
                    "expected '>' after vector or rotation literal",
                )
            }
            _ => {
                self.error_here("PAR006", "expected an expression");
                false
            }
        }
    }

    fn parse_argument_list(&mut self) -> bool {
        if self.matches(TokenKind::RParen) {
            return true;
        }
        loop {
            if !self.parse_expression(1) {
                self.synchronize_arguments();
                return false;
            }
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "expected ')' after arguments")
    }

    fn parse_delimited_expressions(&mut self, closing: TokenKind) -> bool {
        if self.matches(closing) {
            return true;
        }
        loop {
            if !self.parse_expression(1) {
                return false;
            }
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        self.expect(closing, "expected closing delimiter")
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

    fn expect(&mut self, kind: TokenKind, message: &'static str) -> bool {
        if self.matches(kind) {
            true
        } else {
            if self.at_end() && self.eof_reported {
                return false;
            }
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
        if self.at_end() && self.eof_reported {
            return;
        }
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
        if self.at_end() {
            self.eof_reported = true;
        }
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
        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        while !self.at_end() {
            if brace_depth == 0
                && paren_depth == 0
                && bracket_depth == 0
                && (self.is_type()
                    || self.check(TokenKind::KwVoid)
                    || self.check(TokenKind::KwDefault)
                    || self.check(TokenKind::KwState))
            {
                return;
            }

            match self.peek_kind() {
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => {
                    if brace_depth == 0 {
                        self.advance();
                        continue;
                    }
                    brace_depth -= 1;
                }
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    // Recover from a malformed event header or body. Braces that belong to a
    // malformed handler are skipped, while the containing state's closing
    // brace and the next event-like declaration are left for the caller.
    fn synchronize_event(&mut self) {
        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        while !self.at_end() {
            if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
                if self.check(TokenKind::RBrace) {
                    return;
                }
                if self.check(TokenKind::Identifier) && self.peek_kind_at(1) == TokenKind::LParen {
                    return;
                }
            }

            match self.peek_kind() {
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => {
                    if brace_depth == 0 {
                        return;
                    }
                    brace_depth -= 1;
                }
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    fn synchronize_for_list(&mut self, end: TokenKind) {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        while !self.at_end() {
            if paren_depth == 0 && bracket_depth == 0 {
                if self.check(end) {
                    self.advance();
                    return;
                }
                if self.check(TokenKind::LBrace) || self.check(TokenKind::RBrace) {
                    return;
                }
            }

            match self.peek_kind() {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    fn synchronize_condition(&mut self) {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        while !self.at_end() {
            if paren_depth == 0 && bracket_depth == 0 {
                if self.check(TokenKind::RParen) {
                    self.advance();
                    return;
                }
                if self.check(TokenKind::LBrace) {
                    return;
                }
            }

            match self.peek_kind() {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    fn synchronize_for_condition(&mut self) {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        while !self.at_end() {
            if paren_depth == 0 && bracket_depth == 0 {
                if self.check(TokenKind::Semicolon) {
                    self.advance();
                    return;
                }
                if self.check(TokenKind::RParen) || self.check(TokenKind::LBrace) {
                    return;
                }
            }

            match self.peek_kind() {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    fn synchronize_arguments(&mut self) {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        while !self.at_end() {
            match self.peek_kind() {
                TokenKind::RParen if paren_depth == 0 && bracket_depth == 0 => {
                    self.advance();
                    return;
                }
                TokenKind::RBrace if paren_depth == 0 && bracket_depth == 0 => return,
                TokenKind::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    // A semicolon followed by ')' is part of the malformed
                    // call; otherwise it terminates the containing statement.
                    if self.peek_kind_at(1) == TokenKind::RParen {
                        self.advance();
                        self.advance();
                    }
                    return;
                }
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    fn synchronize_statement(&mut self) {
        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        while !self.at_end() {
            if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
                if self.check(TokenKind::RBrace) {
                    return;
                }
                if self.matches(TokenKind::Semicolon) {
                    return;
                }
            }

            match self.peek_kind() {
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => {
                    if brace_depth == 0 {
                        return;
                    }
                    brace_depth -= 1;
                }
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    fn synchronize_parameters(&mut self) {
        while !self.at_end() {
            if matches!(
                self.peek_kind(),
                TokenKind::Comma
                    | TokenKind::RParen
                    | TokenKind::LBrace
                    | TokenKind::RBrace
                    | TokenKind::Semicolon
            ) {
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

fn is_variable_lvalue(tokens: &[Token], mut start: usize, mut end: usize) -> bool {
    loop {
        if end <= start + 1
            || tokens.get(start).map(|token| token.kind) != Some(TokenKind::LParen)
            || tokens.get(end - 1).map(|token| token.kind) != Some(TokenKind::RParen)
        {
            break;
        }

        let mut depth = 0usize;
        let mut matching_close = None;
        for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
            match token.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        matching_close = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }

        if matching_close != Some(end - 1) {
            break;
        }
        start += 1;
        end -= 1;
    }

    end == start + 1
        && tokens
            .get(start)
            .is_some_and(|token| token.kind == TokenKind::Identifier)
}

fn is_assignment_lvalue(tokens: &[Token], mut start: usize, mut end: usize) -> bool {
    loop {
        if end <= start + 1
            || tokens.get(start).map(|token| token.kind) != Some(TokenKind::LParen)
            || tokens.get(end - 1).map(|token| token.kind) != Some(TokenKind::RParen)
        {
            break;
        }

        let mut depth = 0usize;
        let mut matching_close = None;
        for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
            match token.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        matching_close = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }

        if matching_close != Some(end - 1) {
            break;
        }
        start += 1;
        end -= 1;
    }

    if end == start + 1 {
        return tokens
            .get(start)
            .is_some_and(|token| token.kind == TokenKind::Identifier);
    }

    end == start + 3
        && tokens
            .get(start)
            .is_some_and(|token| token.kind == TokenKind::Identifier)
        && tokens
            .get(start + 1)
            .is_some_and(|token| token.kind == TokenKind::Dot)
        && tokens.get(start + 2).is_some_and(|token| {
            token.kind == TokenKind::Identifier
                && matches!(token.lexeme.as_str(), "x" | "y" | "z" | "s")
        })
}

fn binary_precedence(kind: TokenKind) -> Option<(u8, bool)> {
    let precedence = match kind {
        TokenKind::Equal
        | TokenKind::PlusEqual
        | TokenKind::MinusEqual
        | TokenKind::StarEqual
        | TokenKind::SlashEqual
        | TokenKind::PercentEqual => return Some((1, true)),
        // LSL gives && and || equal precedence; both are left-associative.
        TokenKind::OrOr | TokenKind::AndAnd => 3,
        TokenKind::Or => 5,
        TokenKind::Xor => 6,
        TokenKind::And => 7,
        TokenKind::EqualEqual | TokenKind::BangEqual => 8,
        TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual => 9,
        TokenKind::ShiftLeft | TokenKind::ShiftRight => 10,
        // In LSL, subtraction binds more tightly than addition.
        TokenKind::Plus => 11,
        TokenKind::Minus => 12,
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => 13,
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
    fn rejects_global_variables_declared_after_states() {
        let diagnostics = parse_source("default { state_entry() { } } integer value;");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "PAR009" && diagnostic.message.contains("global declarations")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_global_declarations_before_states_and_states_after_states() {
        let diagnostics = parse_source(
            "integer value; default { state_entry() { } } state other { state_entry() { } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
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
            default { state_entry() { } }
            state running
            {
                state_entry() { vector p = <1.0, 2.0, 3.0>; }
            }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_empty_source_without_default_state() {
        let diagnostics = parse_source("");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR013")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_global_variables_without_states() {
        let diagnostics = parse_source("integer value;");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR013")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_global_functions_without_states() {
        let diagnostics = parse_source("integer value() { return 1; }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR013")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_named_state_before_default() {
        let diagnostics =
            parse_source("state running { state_entry() { } } default { state_entry() { } }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR013")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_named_state_without_default() {
        let diagnostics = parse_source("state running { state_entry() { } }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR013")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_duplicate_default_states() {
        let diagnostics =
            parse_source("default { state_entry() { } } default { state_entry() { } }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR014")
                .count(),
            1,
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR013")
                .count(),
            0,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_single_default_state_without_named_states() {
        let diagnostics = parse_source("default { state_entry() { } }");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn accepts_default_followed_by_multiple_named_states() {
        let diagnostics = parse_source(
            "default { state_entry() { } } state first { state_entry() { } } state second { state_entry() { } }",
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
    fn accepts_empty_statements() {
        let diagnostics = parse_source(
            r#"
            default
            {
                state_entry()
                {
                    ;
                    integer i;
                    if (TRUE) ; else ;
                    for (i = 0; i < 1; ++i) ;
                    while (i > 0) ;
                    do ; while (FALSE);
                }
            }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn accepts_for_expression_lists() {
        let diagnostics = parse_source(
            "default { state_entry() { integer i; integer j; for (i = 0, j = 0; i < 1; ++i, ++j) { } } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn accepts_empty_for_initializer_and_increment_with_condition() {
        let diagnostics =
            parse_source("default { state_entry() { integer i; for (; i < 1; ) ; } }");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_empty_for_condition_without_cascading_errors() {
        let diagnostics = parse_source("default { state_entry() { for (;;) ; } }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR006")
                .count(),
            1,
            "{diagnostics:?}"
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
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
    fn accepts_hex_integer_literals_in_declarations_and_expressions() {
        let diagnostics = parse_source(
            r#"integer a = 0xFFFFFFFF;
            integer b = 0x80000000;
            integer c = 0x7FFFFFFF;
            default { state_entry() { integer local = 0x1e3; local = 0x10 + 0X2; } }"#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn accepts_hex_integer_literal_as_function_argument() {
        let diagnostics = parse_source(r#"default { state_entry() { llSay(0x0, "hex"); } }"#);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_malformed_hex_prefixes_as_syntax_errors() {
        for source in [
            "integer value = 0x; default { }",
            "integer value = 0xG; default { }",
            "integer value = 0x1G; default { }",
            "integer value = 0x1.2; default { }",
        ] {
            let diagnostics = parse_source(source);
            assert!(
                diagnostics
                    .iter()
                    .any(|item| item.kind == DiagnosticKind::Syntax),
                "malformed hex was accepted: {source:?}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn reports_missing_semicolon() {
        let diagnostics = parse_source("default { state_entry() { integer x = 1 } }");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "PAR007"));
    }

    #[test]
    fn logical_and_or_operators_share_lsl_precedence() {
        assert_eq!(binary_precedence(TokenKind::OrOr), Some((3, false)));
        assert_eq!(binary_precedence(TokenKind::AndAnd), Some((3, false)));

        let diagnostics =
            parse_source("default { state_entry() { integer result = 1 || 1 && 0; } }");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn subtraction_has_higher_precedence_than_addition() {
        let addition = binary_precedence(TokenKind::Plus).unwrap().0;
        let subtraction = binary_precedence(TokenKind::Minus).unwrap().0;
        assert!(subtraction > addition);
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
    fn rejects_increment_and_decrement_of_non_variables() {
        for operand in ["1", "(first + second)", "llGetPos()"] {
            for operator in ["++", "--"] {
                let prefix = format!(
                    "default {{ state_entry() {{ integer first; integer second; {operator}{operand}; }} }}"
                );
                let prefix_diagnostics = parse_source(&prefix);
                assert!(
                    prefix_diagnostics.iter().any(|item| item.code == "PAR011"),
                    "invalid prefix operand {operator}{operand} was accepted: {prefix_diagnostics:?}"
                );

                let postfix = format!(
                    "default {{ state_entry() {{ integer first; integer second; {operand}{operator}; }} }}"
                );
                let postfix_diagnostics = parse_source(&postfix);
                assert!(
                    postfix_diagnostics.iter().any(|item| item.code == "PAR011"),
                    "invalid postfix operand {operand}{operator} was accepted: {postfix_diagnostics:?}"
                );
            }
        }
    }

    #[test]
    fn rejects_component_increment_and_decrement_targets() {
        let diagnostics = parse_source(
            "default { state_entry() { vector vec; rotation rot; vec.x++; --vec.y; rot.s++; --rot.z; } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "PAR011")
                .count(),
            4,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_increment_and_decrement_of_variables() {
        let diagnostics = parse_source(
            "default { state_entry() { integer first; integer second; ++first; second++; --(first); (second)--; } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_component_access_on_literals_and_function_results() {
        for source in [
            "default { state_entry() { float x = <1, 2, 3>.x; } }",
            "default { state_entry() { float x = llGetPos().x; } }",
        ] {
            let diagnostics = parse_source(source);
            assert!(
                diagnostics.iter().any(|item| item.code == "PAR012"),
                "invalid component access was accepted: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn accepts_component_access_on_vector_and_rotation_variables() {
        let diagnostics = parse_source(
            "default { state_entry() { vector position; rotation orientation; float x = position.x; float s = orientation.s; float grouped = (position).y; } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_assignment_to_non_variable_expressions() {
        let left_sides = ["1", "first + second", "llGetPos()"];
        let operators = ["=", "+=", "-=", "*=", "/=", "%="];

        for left_side in left_sides {
            for operator in operators {
                let source = format!(
                    "default {{ state_entry() {{ integer first; integer second; {left_side} {operator} 1; }} }}"
                );
                let diagnostics = parse_source(&source);
                assert!(
                    diagnostics.iter().any(|item| item.code == "PAR010"),
                    "non-variable assignment target {left_side} {operator} was accepted: {diagnostics:?}"
                );
            }
        }
    }

    #[test]
    fn accepts_vector_and_rotation_component_assignment_targets() {
        let diagnostics = parse_source(
            "default { state_entry() { vector vec; rotation rot; vec.x = 3.0; vec.y = 2.0; vec.z = 1.0; rot.x = 0.0; rot.y = 0.0; rot.z = 0.0; rot.s = 1.0; (vec.x) = 4.0; (rot.s) = 0.5; } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_component_assignment_from_non_variable_bases() {
        let diagnostics = parse_source(
            "default { state_entry() { llGetPos().x = 1.0; <1, 2, 3>.x = 2.0; integer count; count.w = 3; } }",
        );
        assert!(
            diagnostics
                .iter()
                .filter(|item| item.code == "PAR010")
                .count()
                >= 2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_variable_assignment_targets_and_chained_assignments() {
        let diagnostics = parse_source(
            "default { state_entry() { integer first; integer second; first = second = 1; (first) = second; } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_unsupported_bitwise_and_shift_compound_assignments() {
        for operator in ["&=", "|=", "^=", "<<=", ">>="] {
            let source =
                format!("default {{ state_entry() {{ integer value; value {operator} 1; }} }}");
            let diagnostics = parse_source(&source);
            assert!(
                !diagnostics.is_empty(),
                "unsupported LSL compound assignment {operator} was accepted"
            );
        }
    }

    #[test]
    fn accepts_call_suffixes_only_on_identifiers() {
        let diagnostics = parse_source(
            r#"default { state_entry() {
                llOwnerSay("text");
                user_function();
                user_function(1, 2);
                outer(inner(1));
            } }"#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_call_suffixes_on_non_identifiers() {
        for source in [
            "default { state_entry() { 1(); } }",
            "default { state_entry() { \"text\"(); } }",
            "default { state_entry() { llGetPos()(); } }",
            "default { state_entry() { vector position; position.x(); } }",
            "default { state_entry() { integer first; integer second; (first + second)(); } }",
            "default { state_entry() { [](); } }",
            "default { state_entry() { [1, \"text\"](); } }",
            "default { state_entry() { <1, 2, 3>(); } }",
            "default { state_entry() { <1, 2, 3, 4>(); } }",
        ] {
            let diagnostics = parse_source(source);
            assert_eq!(
                diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.code == "PAR007")
                    .count(),
                1,
                "unexpected diagnostics for {source:?}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn rejects_unsupported_ternary_operator() {
        let diagnostics = parse_source("default { state_entry() { integer value = 1 ? 2 : 3; } }");
        assert!(!diagnostics.is_empty(), "ternary syntax is not part of LSL");
    }

    #[test]
    fn accepts_state_change_to_default() {
        let diagnostics = parse_source(
            "default { state_entry() { state default; } } \
             state running { state_entry() { state default; } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn recovers_from_a_malformed_event_body_before_a_valid_event() {
        let diagnostics =
            parse_source(r#"default { bad( { } state_entry() { llOwnerSay("ok"); } }"#);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR003");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (1, 16)
        );
    }

    #[test]
    fn malformed_event_body_does_not_mistake_nested_calls_for_handlers() {
        let diagnostics =
            parse_source(r#"default { bad( { llOwnerSay("inside"); } state_entry() { } }"#);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR003");
        assert!(
            !diagnostics.iter().any(|item| item.code == "PAR004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn recovers_from_a_malformed_function_parameter_list_before_a_state() {
        let diagnostics =
            parse_source("integer f(integer value { return value; } default { state_entry() { } }");
        let syntax = diagnostics
            .iter()
            .filter(|item| item.kind == DiagnosticKind::Syntax)
            .collect::<Vec<_>>();
        assert_eq!(syntax.len(), 1, "{diagnostics:?}");
        assert_eq!(syntax[0].code, "PAR007");
        assert!(
            !diagnostics.iter().any(|item| item.code == "PAR013"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn missing_function_body_recovers_to_default() {
        let diagnostics = parse_source("integer f() default { state_entry() { } }");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR007");
        assert!(
            !diagnostics.iter().any(|item| item.code == "PAR013"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn recovers_from_a_malformed_state_before_default() {
        let diagnostics =
            parse_source("default { } state broken( { } state running { state_entry() { } }");
        let syntax = diagnostics
            .iter()
            .filter(|item| item.kind == DiagnosticKind::Syntax)
            .collect::<Vec<_>>();
        assert_eq!(syntax.len(), 1, "{diagnostics:?}");
        assert_eq!(syntax[0].code, "PAR007");
    }

    #[test]
    fn recovers_from_a_malformed_event_header_before_a_valid_event() {
        let diagnostics = parse_source("default { bad( ; state_entry() { } }");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR003");
    }

    #[test]
    fn recovers_from_a_malformed_handler_before_a_valid_state() {
        let diagnostics = parse_source("default { bad( { } } state running { state_entry() { } }");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR003");
        assert!(
            !diagnostics.iter().any(|item| item.code == "PAR013"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn malformed_declarations_recover_without_losing_later_declarations() {
        for (source, code, column) in [
            (
                r#"default { state_entry() { integer first, ; integer second; llOwnerSay("ok"); } }"#,
                "PAR007",
                42,
            ),
            (
                "integer first; ? integer second; default { state_entry() { } }",
                "PAR001",
                16,
            ),
            (
                "integer first integer second; default { state_entry() { } }",
                "PAR007",
                15,
            ),
        ] {
            let diagnostics = parse_source(source);
            assert_eq!(diagnostics.len(), 1, "source {source:?}: {diagnostics:?}");
            assert_eq!(diagnostics[0].code, code, "source {source:?}");
            assert_eq!(
                (diagnostics[0].span.line, diagnostics[0].span.column),
                (1, column),
                "source {source:?}"
            );
        }
    }

    #[test]
    fn malformed_expression_reports_one_primary_diagnostic() {
        let diagnostics =
            parse_source(r#"default { state_entry() { ? integer value; llOwnerSay("ok"); } }"#);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR006");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (1, 27)
        );
    }

    #[test]
    fn missing_initializer_does_not_consume_the_next_declaration() {
        let diagnostics =
            parse_source("default { state_entry() { integer first = ; integer second; } }");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR006");
    }

    #[test]
    fn multiple_malformed_events_report_one_error_each() {
        let diagnostics =
            parse_source("default { first_bad( { } second_bad( { } state_entry() { } }");
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics.iter().all(|item| item.code == "PAR003"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "PAR001"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn malformed_argument_list_recovers_to_the_outer_statement() {
        let diagnostics =
            parse_source(r#"default { state_entry() { llOwnerSay(1,;); llOwnerSay("ok"); } }"#);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR006");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (1, 40)
        );
    }

    #[test]
    fn missing_call_close_preserves_later_statement_and_error() {
        let diagnostics = parse_source("default { state_entry() { llOwnerSay(1,;\nif () { }\n} }");
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR006");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (1, 40)
        );
        assert_eq!(diagnostics[1].code, "PAR006");
        assert_eq!(
            (diagnostics[1].span.line, diagnostics[1].span.column),
            (2, 5)
        );
    }

    #[test]
    fn malformed_parenthesized_expression_recovers_at_statement_boundary() {
        let diagnostics = parse_source(
            "default { state_entry() { integer value = (1 + * 2);\nllOwnerSay(\"later\");\n} }",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR005");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (1, 48)
        );
    }

    #[test]
    fn malformed_list_and_vector_components_recover_at_statement_boundary() {
        for (source, column) in [
            (
                "default { state_entry() { list values = [1, * 2];\nllOwnerSay(\"later\");\n} }",
                45,
            ),
            (
                "default { state_entry() { vector value = <1, * 2, 3>;\nllOwnerSay(\"later\");\n} }",
                46,
            ),
        ] {
            let diagnostics = parse_source(source);
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert_eq!(diagnostics[0].code, "PAR006");
            assert_eq!(
                (diagnostics[0].span.line, diagnostics[0].span.column),
                (1, column),
                "source {source:?}"
            );
        }
    }

    #[test]
    fn malformed_conditions_and_return_recover_per_statement() {
        for (source, column) in [
            (
                "default { state_entry() { if (1 + * 2) { }\nllOwnerSay(\"later\");\n} }",
                35,
            ),
            (
                "default { state_entry() { while (1 + * 2) { }\nllOwnerSay(\"later\");\n} }",
                38,
            ),
            (
                "default { state_entry() { for (; 1 + * 2; ) { }\nllOwnerSay(\"later\");\n} }",
                38,
            ),
            (
                "default { state_entry() { do { } while (1 + * 2);\nllOwnerSay(\"later\");\n} }",
                45,
            ),
            (
                "default { state_entry() { return 1 + * 2;\nllOwnerSay(\"later\");\n} }",
                38,
            ),
        ] {
            let diagnostics = parse_source(source);
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert_eq!(diagnostics[0].code, "PAR005");
            assert_eq!(
                (diagnostics[0].span.line, diagnostics[0].span.column),
                (1, column),
                "source {source:?}"
            );
        }
    }

    #[test]
    fn malformed_for_initializer_recovers_at_its_semicolon() {
        let diagnostics = parse_source(
            "default { state_entry() { integer index;\nfor (index + * 2; TRUE; ) { }\nllOwnerSay(\"later\");\n} }",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR005");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (2, 14)
        );
    }

    #[test]
    fn malformed_for_increment_recovers_at_the_closing_paren() {
        let diagnostics = parse_source(
            "default { state_entry() { integer index;\nfor (; TRUE; index + * 2) { }\nllOwnerSay(\"later\");\n} }",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR005");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (2, 22)
        );
    }

    #[test]
    fn malformed_for_lists_do_not_hide_a_later_independent_error() {
        for source in [
            "default { state_entry() { integer index;\nfor (index + * 2; TRUE; ) { }\nif () { }\n} }",
            "default { state_entry() { integer index;\nfor (; TRUE; index + * 2) { }\nif () { }\n} }",
        ] {
            let diagnostics = parse_source(source);
            assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
            assert_eq!(diagnostics[0].code, "PAR005", "{diagnostics:?}");
            assert_eq!(diagnostics[1].code, "PAR006", "{diagnostics:?}");
            assert_eq!(
                (diagnostics[1].span.line, diagnostics[1].span.column),
                (3, 5),
                "{diagnostics:?}"
            );
        }
    }

    #[test]
    fn repeated_malformed_statements_are_bounded() {
        let malformed = "? ".repeat(64);
        let diagnostics = parse_source(&format!(
            r#"default {{ state_entry() {{ {malformed}llOwnerSay("ok"); }} }}"#
        ));
        assert_eq!(diagnostics.len(), 64, "{diagnostics:?}");
        assert!(
            diagnostics.iter().all(|item| item.code == "PAR006"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn deeply_nested_expressions_do_not_panic() {
        let open = "(".repeat(128);
        let close = ")".repeat(128);
        let source = format!("default {{ state_entry() {{ integer value = {open}1{close}; }} }}");
        let diagnostics = parse_source(&source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn incomplete_list_at_eof_reports_one_primary_diagnostic() {
        let diagnostics = parse_source(r#"default { state_entry() { list values = [1, 2,"#);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR006");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (1, 47)
        );
    }

    #[test]
    fn incomplete_block_at_eof_reports_one_primary_diagnostic() {
        let diagnostics = parse_source("default { state_entry() {");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "PAR007");
        assert_eq!(
            (diagnostics[0].span.line, diagnostics[0].span.column),
            (1, 26)
        );
    }

    #[test]
    fn rejects_state_change_without_target() {
        let diagnostics = parse_source("default { state_entry() { state ; } }");
        assert!(
            diagnostics.iter().any(|item| item.code == "PAR007"),
            "{diagnostics:?}"
        );
    }
}
