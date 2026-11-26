//! Independent recursive-descent AST parser for the frozen LSL grammar.
//!
//! Diagnostics intentionally mirror the legacy parser's `PARxxx` contract.
//! Recovery is delimiter-aware and every loop is guaranteed to make progress.

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
use crate::lexer::{Token, TokenKind};

use super::ext::{
    AstParseOptions, BreakStatement, LazyIndexExpression, SwitchArm, SwitchLabel, SwitchStatement,
};
use super::node::*;
use super::types::*;

/// Maximum recursive AST nesting depth.
pub const MAX_AST_RECURSION_DEPTH: usize = 256;

/// Stable recursion-limit diagnostic code.
pub const AST_RECURSION_LIMIT: &str = "AST001";
/// Stable safe-failure diagnostic code when the parser worker cannot run.
pub const AST_WORKER_UNAVAILABLE: &str = "AST002";

struct ParsedArguments {
    arguments: Vec<Expression>,
    complete: bool,
    close_span: Span,
    declared_count: usize,
    matched_close: bool,
}

struct Parser<'a> {
    tokens: &'a [Token],
    current: usize,
    diagnostics: Vec<Diagnostic>,
    states_started: bool,
    default_state_seen: bool,
    missing_default_reported: bool,
    eof_reported: bool,
    statement_depth: usize,
    recursion_reported: bool,
    options: AstParseOptions,
    switch_depth: usize,
    suppress_diagnostics: usize,
    recovered_functions: Vec<RecoveredFunction>,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], options: AstParseOptions) -> Self {
        Self {
            tokens,
            current: 0,
            diagnostics: Vec::new(),
            states_started: false,
            default_state_seen: false,
            missing_default_reported: false,
            eof_reported: false,
            statement_depth: 0,
            recursion_reported: false,
            options,
            switch_depth: 0,
            suppress_diagnostics: 0,
            recovered_functions: Vec::new(),
        }
    }

    fn run(mut self) -> (Ast, Vec<Diagnostic>) {
        let mut items = Vec::new();
        while !self.at_end() {
            let before = self.current;
            if self.is_type() || self.is_untyped_function_declaration() {
                if self.states_started {
                    self.error_here(
                        "PAR009",
                        "global declarations and user-defined functions must be declared before state definitions",
                    );
                }
                if let Some(item) = self.parse_global_declaration() {
                    items.push(item);
                }
            } else if self.check(TokenKind::KwDefault) || self.check(TokenKind::KwState) {
                self.states_started = true;
                items.push(TopLevel::State(self.parse_state()));
            } else {
                let span = self.here_span();
                self.error_here(
                    "PAR001",
                    "expected a global declaration or state definition",
                );
                self.synchronize_top_level();
                items.push(TopLevel::Error(ErrorNode {
                    span,
                    code: Some("PAR001"),
                }));
            }
            if self.current == before {
                self.advance();
            }
        }

        if !self.default_state_seen && !self.missing_default_reported {
            self.error_here("PAR013", "the default state must be defined");
        }

        let end = self.last_non_eof_end();
        let span = if end == 0 {
            self.fallback_span()
        } else {
            self.span_from(0, end)
        };
        (
            Ast {
                items,
                recovered_functions: self.recovered_functions,
                span,
            },
            self.diagnostics,
        )
    }

    fn last_non_eof_end(&self) -> usize {
        self.tokens
            .iter()
            .rposition(|token| token.kind != TokenKind::Eof)
            .map_or(0, |index| index + 1)
    }

    fn parse_global_declaration(&mut self) -> Option<TopLevel> {
        let start = self.current;
        let return_type = if self.is_type() {
            self.parse_type()
        } else {
            None
        };

        if !self.expect(TokenKind::Identifier, "expected a declaration name") {
            self.synchronize_top_level();
            return Some(TopLevel::Error(ErrorNode {
                span: self.span_from(start, self.current),
                code: Some("PAR007"),
            }));
        }
        let name_index = self.current - 1;
        let name = self.tokens[name_index].lexeme.clone();
        let name_span = self.tokens[name_index].span;

        if self.matches(TokenKind::LParen) {
            let parameter_open = self.current - 1;
            let (parameters, header_ok) = self.parse_parameters();
            if !header_ok {
                if let Some(close) = self.event_parameter_close_before_body() {
                    let parameter_types = self
                        .parameter_type_kinds_between(parameter_open, close)
                        .into_iter()
                        .map(type_kind_name)
                        .map(str::to_owned)
                        .collect();
                    self.recovered_functions.push(RecoveredFunction {
                        name: name.clone(),
                        name_span,
                        return_type: return_type
                            .map(|ty| type_kind_name(ty.kind).to_owned())
                            .unwrap_or_else(|| "void".to_owned()),
                        parameter_types,
                    });
                }
                self.synchronize_top_level();
                return Some(TopLevel::Error(ErrorNode {
                    span: self.span_from(start, self.current),
                    code: Some("PAR007"),
                }));
            }
            let body = self.parse_block();
            return Some(TopLevel::Function(FunctionDeclaration {
                span: self.span_from(start, self.current),
                return_type,
                name,
                name_span,
                parameters,
                body,
            }));
        }

        let ty = return_type?;
        let declarators = self.finish_variable_declaration(name_index);
        Some(TopLevel::Global(GlobalVariableDeclaration {
            span: self.span_from(start, self.current),
            ty,
            declarators,
        }))
    }

    fn finish_variable_declaration(&mut self, name_index: usize) -> Vec<VariableDeclarator> {
        let mut declarators = Vec::new();
        let mut pending_initializer = None;
        if self.matches(TokenKind::Equal) {
            pending_initializer = self.parse_expression(1);
            if pending_initializer.is_none() {
                let error_span = self.here_span();
                self.synchronize_statement();
                let mut declarator = self.declarator(name_index, None);
                declarator.initializer_error = Some(ErrorNode {
                    span: error_span,
                    code: Some("PAR006"),
                });
                declarator.span = join_spans(declarator.span, error_span);
                declarators.push(declarator);
                return declarators;
            }
        }
        declarators.push(self.declarator(name_index, pending_initializer));
        while self.matches(TokenKind::Comma) {
            if !self.expect(TokenKind::Identifier, "expected a variable name after ','") {
                let error_span = self.here_span();
                self.synchronize_statement();
                declarators.push(self.missing_declarator(error_span));
                return declarators;
            }
            let name_index = self.current - 1;
            let mut initializer = None;
            if self.matches(TokenKind::Equal) {
                initializer = self.parse_expression(1);
                if initializer.is_none() {
                    let error_span = self.here_span();
                    self.synchronize_statement();
                    let mut declarator = self.declarator(name_index, None);
                    declarator.initializer_error = Some(ErrorNode {
                        span: error_span,
                        code: Some("PAR006"),
                    });
                    declarator.span = join_spans(declarator.span, error_span);
                    declarators.push(declarator);
                    return declarators;
                }
            }
            declarators.push(self.declarator(name_index, initializer));
        }
        self.expect(
            TokenKind::Semicolon,
            "expected ';' after global declaration",
        );
        declarators
    }

    fn missing_declarator(&self, error_span: Span) -> VariableDeclarator {
        VariableDeclarator {
            span: error_span,
            name: String::new(),
            name_span: error_span,
            name_error: Some(ErrorNode {
                span: error_span,
                code: Some("PAR007"),
            }),
            initializer: None,
            initializer_error: None,
        }
    }

    fn declarator(&self, name_index: usize, initializer: Option<Expression>) -> VariableDeclarator {
        let name_span = self.tokens[name_index].span;
        let end = initializer
            .as_ref()
            .map_or(name_span.end, |expression| expression.span.end);
        VariableDeclarator {
            span: Span {
                start: name_span.start,
                end,
                line: name_span.line,
                column: name_span.column,
                end_line: initializer
                    .as_ref()
                    .map_or(name_span.end_line, |expression| expression.span.end_line),
                end_column: initializer
                    .as_ref()
                    .map_or(name_span.end_column, |expression| {
                        expression.span.end_column
                    }),
            },
            name: self.tokens[name_index].lexeme.clone(),
            name_span,
            name_error: None,
            initializer,
            initializer_error: None,
        }
    }

    fn parse_parameters(&mut self) -> (Vec<Parameter>, bool) {
        let mut parameters = Vec::new();
        if self.matches(TokenKind::RParen) {
            return (parameters, true);
        }
        loop {
            let start = self.current;
            let Some(ty) = self.parse_type() else {
                self.error_here("PAR003", "expected a parameter type");
                self.synchronize_parameters();
                return (parameters, false);
            };
            if !self.expect(TokenKind::Identifier, "expected a parameter name") {
                self.synchronize_parameters();
                return (parameters, false);
            }
            let name_index = self.current - 1;
            parameters.push(Parameter {
                span: self.span_from(start, self.current),
                ty,
                name: self.tokens[name_index].lexeme.clone(),
                name_span: self.tokens[name_index].span,
            });
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        let ok = self.expect(TokenKind::RParen, "expected ')' after parameter list");
        (parameters, ok)
    }

    fn parse_state(&mut self) -> StateDeclaration {
        let start = self.current;
        let mut is_default = false;
        let (name, name_span) = if self.check(TokenKind::KwDefault) {
            if self.default_state_seen {
                self.error_here("PAR014", "only one default state is allowed");
            }
            let span = self.here_span();
            self.advance();
            self.default_state_seen = true;
            is_default = true;
            ("default".to_owned(), span)
        } else {
            if !self.default_state_seen && !self.missing_default_reported {
                self.error_here(
                    "PAR013",
                    "the default state must be defined before other states",
                );
                self.missing_default_reported = true;
            }
            self.advance();
            if self.expect(TokenKind::Identifier, "expected a state name after 'state'") {
                let index = self.current - 1;
                (self.tokens[index].lexeme.clone(), self.tokens[index].span)
            } else {
                (String::new(), self.here_span())
            }
        };

        let mut events = Vec::new();
        let has_body = self.expect(TokenKind::LBrace, "expected '{' to begin state body");
        if !has_body {
            self.synchronize_top_level();
        } else {
            while !self.at_end() && !self.check(TokenKind::RBrace) {
                let before = self.current;
                if !self.check(TokenKind::Identifier) {
                    self.error_here("PAR004", "expected an event handler inside state");
                    self.synchronize_event();
                } else {
                    let event_start = self.current;
                    let event_name_index = self.current;
                    self.advance();
                    if !self.expect(TokenKind::LParen, "expected '(' after event name") {
                        self.synchronize_event();
                    } else {
                        let parameter_open = self.current - 1;
                        let (parameters, ok) = self.parse_parameters();
                        if ok {
                            let body = self.parse_block();
                            events.push(EventHandler {
                                span: self.span_from(event_start, self.current),
                                name: self.tokens[event_name_index].lexeme.clone(),
                                name_span: self.tokens[event_name_index].span,
                                declared_parameter_count: parameters.len(),
                                declared_parameter_types: parameters
                                    .iter()
                                    .map(|parameter| parameter.ty.kind)
                                    .collect(),
                                recovered_parameter_references: Vec::new(),
                                parameters,
                                body,
                            });
                        } else if let Some(close) = self.event_parameter_close_before_body() {
                            let parameters = self.recover_parameters(parameter_open, close);
                            while self.current <= close {
                                self.advance();
                            }
                            // Keep a recoverable event shape for the AST semantic
                            // engine without adding syntax findings the Legacy
                            // parser would have skipped with synchronize_event.
                            self.suppress_diagnostics += 1;
                            let body = self.parse_block();
                            self.suppress_diagnostics -= 1;
                            if let Some(body) = body {
                                events.push(EventHandler {
                                    span: self.span_from(event_start, self.current),
                                    name: self.tokens[event_name_index].lexeme.clone(),
                                    name_span: self.tokens[event_name_index].span,
                                    parameters,
                                    declared_parameter_count: self
                                        .count_declared_parameters(parameter_open, close),
                                    declared_parameter_types: self
                                        .parameter_type_kinds_between(parameter_open, close),
                                    recovered_parameter_references: self
                                        .recover_parameter_references(parameter_open, close),
                                    body: Some(body),
                                });
                            } else {
                                self.synchronize_event();
                            }
                        } else {
                            self.synchronize_event();
                        }
                    }
                }
                if self.current == before {
                    self.advance();
                }
            }
        }
        let body_closed = if has_body {
            self.expect(TokenKind::RBrace, "expected '}' after state body")
        } else {
            false
        };
        let name_missing = name.is_empty();
        StateDeclaration {
            span: self.span_from(start, self.current),
            name,
            name_span,
            name_missing,
            is_default,
            body_missing: !has_body,
            body_closed,
            events,
        }
    }

    fn parse_block(&mut self) -> Option<Block> {
        let start = self.current;
        if !self.expect(TokenKind::LBrace, "expected '{' to begin block") {
            return None;
        }
        let mut statements = Vec::new();
        while !self.at_end() && !self.check(TokenKind::RBrace) {
            let before = self.current;
            if let Some(statement) = self.parse_statement() {
                statements.push(statement);
            }
            if self.current == before {
                self.advance();
            }
        }
        let closed = self.expect(TokenKind::RBrace, "expected '}' after block");
        Some(Block {
            span: self.span_from(start, self.current),
            closed,
            statements,
        })
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        if self.statement_depth >= MAX_AST_RECURSION_DEPTH {
            let span = self.here_span();
            self.report_recursion(span);
            self.skip_deep_statement();
            return Some(Statement {
                span,
                kind: StatementKind::Error(ErrorNode {
                    span,
                    code: Some(AST_RECURSION_LIMIT),
                }),
            });
        }
        self.statement_depth += 1;
        let statement = self.parse_statement_inner();
        self.statement_depth -= 1;
        statement
    }

    fn parse_statement_inner(&mut self) -> Option<Statement> {
        let start = self.current;
        if self.options.switches_enabled()
            && self.check_identifier("switch")
            && self.peek_kind_at(1) == TokenKind::LParen
        {
            return Some(self.parse_switch_statement(start));
        }
        if self.options.switches_enabled()
            && self.switch_depth > 0
            && self.check_identifier("break")
            && self.peek_kind_at(1) == TokenKind::Semicolon
        {
            self.advance();
            self.advance();
            return Some(self.statement(
                start,
                StatementKind::Break(BreakStatement {
                    span: self.span_from(start, self.current),
                }),
            ));
        }
        if self.matches(TokenKind::Semicolon) {
            return Some(self.statement(start, StatementKind::Empty));
        }
        if self.check(TokenKind::LBrace) {
            return match self.parse_block() {
                Some(block) => Some(self.statement(start, StatementKind::Block(block))),
                None => Some(self.error_statement_code(start, "PAR007")),
            };
        }
        if self.matches(TokenKind::KwIf) {
            return Some(self.parse_if_statement(start));
        }
        if self.matches(TokenKind::KwFor) {
            return Some(self.parse_for_statement(start));
        }
        if self.matches(TokenKind::KwWhile) {
            return Some(self.parse_while_statement(start));
        }
        if self.matches(TokenKind::KwDo) {
            return Some(self.parse_do_while_statement(start));
        }
        if self.matches(TokenKind::KwReturn) {
            let keyword_span = self.token_span(start);
            let mut semicolon_consumed = false;
            let mut recovered_labels = Vec::new();
            let mut recovered_jumps = Vec::new();
            let value = if !self.check(TokenKind::Semicolon) {
                let value_start = self.current;
                let expression = self.parse_expression(1);
                if expression.is_none() {
                    self.synchronize_statement();
                    if self.current == 0
                        || self.tokens[self.current - 1].kind != TokenKind::Semicolon
                    {
                        return Some(self.error_statement(start));
                    }
                    semicolon_consumed = true;
                    let semicolon = self.current - 1;
                    (recovered_labels, recovered_jumps) =
                        self.recover_return_flow_nodes(value_start, semicolon);
                    let first_span = self.tokens[value_start].span;
                    Some(Expression {
                        span: first_span,
                        kind: ExpressionKind::Error(ErrorNode {
                            span: first_span,
                            code: None,
                        }),
                    })
                } else {
                    expression
                }
            } else {
                None
            };
            if !semicolon_consumed {
                self.expect(TokenKind::Semicolon, "expected ';' after return statement");
            }
            return Some(self.statement(
                start,
                StatementKind::Return(ReturnStatement {
                    span: self.span_from(start, self.current),
                    keyword_span,
                    value,
                    recovered_labels,
                    recovered_jumps,
                }),
            ));
        }
        if self.matches(TokenKind::KwJump) {
            let label_index =
                if self.expect(TokenKind::Identifier, "expected a label name after 'jump'") {
                    Some(self.current - 1)
                } else {
                    None
                };
            self.expect(TokenKind::Semicolon, "expected ';' after jump statement");
            let Some(label_index) = label_index else {
                return Some(self.error_statement_code(start, "PAR007"));
            };
            let label_span = self.token_span(label_index);
            return Some(self.statement(
                start,
                StatementKind::Jump(JumpStatement {
                    span: self.span_from(start, self.current),
                    label: self.token_text(label_index),
                    label_span,
                }),
            ));
        }
        if self.matches(TokenKind::KwState) {
            let name_index =
                if self.matches(TokenKind::Identifier) || self.matches(TokenKind::KwDefault) {
                    Some(self.current - 1)
                } else {
                    self.error_here("PAR007", "expected a state name after 'state'");
                    None
                };
            self.expect(TokenKind::Semicolon, "expected ';' after state change");
            let Some(name_index) = name_index else {
                return Some(self.error_statement_code(start, "PAR007"));
            };
            let state_span = self.token_span(name_index);
            return Some(self.statement(
                start,
                StatementKind::StateChange(StateChangeStatement {
                    span: self.span_from(start, self.current),
                    state: self.token_text(name_index),
                    state_span,
                }),
            ));
        }
        if self.matches(TokenKind::At) {
            let name_index =
                if self.expect(TokenKind::Identifier, "expected a label name after '@'") {
                    Some(self.current - 1)
                } else {
                    None
                };
            self.expect(TokenKind::Semicolon, "expected ';' after label");
            let Some(name_index) = name_index else {
                return Some(self.error_statement_code(start, "PAR007"));
            };
            let name_span = self.token_span(name_index);
            return Some(self.statement(
                start,
                StatementKind::Label(LabelStatement {
                    span: self.span_from(start, self.current),
                    name: self.token_text(name_index),
                    name_span,
                }),
            ));
        }
        if self.is_type() {
            let ty = self.parse_type()?;
            return Some(self.parse_local_declaration(start, ty));
        }

        let expression_start = self.current;
        let expression = self.parse_expression(1);
        let Some(expression) = expression else {
            if self.current > expression_start {
                self.synchronize_statement();
            }
            return Some(self.error_statement(start));
        };
        self.expect(TokenKind::Semicolon, "expected ';' after expression");
        Some(self.statement(
            start,
            StatementKind::Expression(ExpressionStatement {
                span: self.span_from(start, self.current),
                expression,
            }),
        ))
    }

    fn check_identifier(&self, name: &str) -> bool {
        self.tokens
            .get(self.current)
            .is_some_and(|token| token.kind == TokenKind::Identifier && token.lexeme == name)
    }

    fn check_case_label(&self) -> bool {
        self.options.switches_enabled() && self.check_identifier("case")
    }

    fn check_default_label(&self) -> bool {
        self.options.switches_enabled() && self.check(TokenKind::KwDefault)
    }

    fn parse_switch_statement(&mut self, start: usize) -> Statement {
        self.advance(); // switch
        self.expect(TokenKind::LParen, "expected '(' after 'switch'");
        let condition = if let Some(expression) = self.parse_expression(1) {
            self.expect(TokenKind::RParen, "expected ')' after switch condition");
            expression
        } else {
            self.synchronize_condition();
            self.error_expression(self.here_span())
        };
        if !self.expect(TokenKind::LBrace, "expected '{' to begin switch body") {
            self.synchronize_statement();
            return self.error_statement_code(start, "PAR007");
        }
        self.switch_depth += 1;
        let mut arms = Vec::new();
        while !self.at_end() && !self.check(TokenKind::RBrace) {
            let before = self.current;
            if self.check_case_label() || self.check_default_label() {
                arms.push(self.parse_switch_arm());
            } else {
                let arm_start = self.current;
                let label_span = self.here_span();
                self.error_here("PAR007", "expected 'case' or 'default' inside switch");
                let mut statements = Vec::new();
                if let Some(statement) = self.parse_statement() {
                    statements.push(statement);
                }
                arms.push(SwitchArm {
                    span: self.span_from(arm_start, self.current),
                    label: SwitchLabel::Error(ErrorNode {
                        span: label_span,
                        code: Some("PAR007"),
                    }),
                    statements,
                });
            }
            if self.current == before {
                self.advance();
            }
        }
        self.switch_depth = self.switch_depth.saturating_sub(1);
        self.expect(TokenKind::RBrace, "expected '}' after switch body");
        self.statement(
            start,
            StatementKind::Switch(SwitchStatement {
                span: self.span_from(start, self.current),
                condition,
                arms,
            }),
        )
    }

    fn parse_switch_arm(&mut self) -> SwitchArm {
        let start = self.current;
        let label = if self.check_case_label() {
            self.advance();
            match self.parse_expression(1) {
                Some(expression) => SwitchLabel::Case(expression),
                None => SwitchLabel::Error(ErrorNode {
                    span: self.here_span(),
                    code: Some("PAR006"),
                }),
            }
        } else {
            self.advance(); // default
            SwitchLabel::Default
        };
        let mut statements = Vec::new();
        if self.matches(TokenKind::Colon) {
            // Colon form: collect fallthrough statements below.
        } else if self.check(TokenKind::LBrace) {
            let block_start = self.current;
            if let Some(block) = self.parse_block() {
                statements.push(self.statement(block_start, StatementKind::Block(block)));
            }
        } else {
            self.error_here("PAR007", "expected ':' or '{' after switch case label");
            self.synchronize_switch_arm();
        }
        while !self.at_end()
            && !self.check(TokenKind::RBrace)
            && !self.check_case_label()
            && !self.check_default_label()
        {
            let before = self.current;
            if let Some(statement) = self.parse_statement() {
                statements.push(statement);
            }
            if self.current == before {
                self.advance();
            }
        }
        SwitchArm {
            span: self.span_from(start, self.current),
            label,
            statements,
        }
    }

    fn synchronize_switch_arm(&mut self) {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        while !self.at_end() {
            if paren_depth == 0 && bracket_depth == 0 {
                if self.check(TokenKind::RBrace)
                    || self.check_case_label()
                    || self.check_default_label()
                {
                    return;
                }
                if self.check(TokenKind::Semicolon) {
                    self.advance();
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

    fn parse_if_statement(&mut self, start: usize) -> Statement {
        self.expect(TokenKind::LParen, "expected '(' after 'if'");
        let condition = if let Some(expression) = self.parse_expression(1) {
            self.expect(TokenKind::RParen, "expected ')' after if condition");
            expression
        } else {
            self.synchronize_condition();
            self.error_expression(self.here_span())
        };
        let then_branch = self
            .parse_statement()
            .unwrap_or_else(|| self.error_statement(self.current));
        let else_branch = if self.matches(TokenKind::KwElse) {
            Some(Box::new(
                self.parse_statement()
                    .unwrap_or_else(|| self.error_statement(self.current)),
            ))
        } else {
            None
        };
        self.statement(
            start,
            StatementKind::If(IfStatement {
                span: self.span_from(start, self.current),
                condition,
                then_branch: Box::new(then_branch),
                else_branch,
            }),
        )
    }

    fn parse_for_statement(&mut self, start: usize) -> Statement {
        self.expect(TokenKind::LParen, "expected '(' after 'for'");
        let mut initializer = Vec::new();
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
        } else {
            let (expressions, ok) = self.parse_for_expression_list(TokenKind::Semicolon);
            initializer = expressions;
            if ok {
                self.expect(TokenKind::Semicolon, "expected ';' after for initializer");
            }
        }

        let condition_missing = self.check(TokenKind::Semicolon);
        let (condition, condition_malformed) = if let Some(expression) = self.parse_expression(1) {
            self.expect(TokenKind::Semicolon, "expected ';' after for condition");
            (expression, false)
        } else {
            self.synchronize_for_condition();
            let malformed = !condition_missing;
            (self.error_expression(self.here_span()), malformed)
        };

        let (increment, ok) = self.parse_for_expression_list(TokenKind::RParen);
        if ok {
            self.expect(TokenKind::RParen, "expected ')' after for clauses");
        }
        let body = self
            .parse_statement()
            .unwrap_or_else(|| self.error_statement(self.current));
        self.statement(
            start,
            StatementKind::For(ForStatement {
                span: self.span_from(start, self.current),
                initializer,
                condition,
                condition_missing,
                condition_malformed,
                increment,
                body: Box::new(body),
            }),
        )
    }

    fn parse_for_expression_list(&mut self, end: TokenKind) -> (Vec<Expression>, bool) {
        let mut expressions = Vec::new();
        if self.check(end) {
            return (expressions, true);
        }
        let Some(expression) = self.parse_expression(1) else {
            self.synchronize_for_list(end);
            return (expressions, false);
        };
        expressions.push(expression);
        while self.matches(TokenKind::Comma) {
            let Some(expression) = self.parse_expression(1) else {
                self.synchronize_for_list(end);
                return (expressions, false);
            };
            expressions.push(expression);
        }
        (expressions, true)
    }

    fn parse_while_statement(&mut self, start: usize) -> Statement {
        self.expect(TokenKind::LParen, "expected '(' after 'while'");
        let condition = if let Some(expression) = self.parse_expression(1) {
            self.expect(TokenKind::RParen, "expected ')' after while condition");
            expression
        } else {
            self.synchronize_condition();
            self.error_expression(self.here_span())
        };
        let body = self
            .parse_statement()
            .unwrap_or_else(|| self.error_statement(self.current));
        self.statement(
            start,
            StatementKind::While(WhileStatement {
                span: self.span_from(start, self.current),
                condition,
                body: Box::new(body),
            }),
        )
    }

    fn parse_do_while_statement(&mut self, start: usize) -> Statement {
        let body = self
            .parse_statement()
            .unwrap_or_else(|| self.error_statement(self.current));
        self.expect(TokenKind::KwWhile, "expected 'while' after do-body");
        self.expect(TokenKind::LParen, "expected '(' after 'while'");
        let condition = if let Some(expression) = self.parse_expression(1) {
            self.expect(TokenKind::RParen, "expected ')' after do-while condition");
            expression
        } else {
            self.synchronize_condition();
            self.error_expression(self.here_span())
        };
        self.expect(
            TokenKind::Semicolon,
            "expected ';' after do-while statement",
        );
        self.statement(
            start,
            StatementKind::DoWhile(DoWhileStatement {
                span: self.span_from(start, self.current),
                body: Box::new(body),
                condition,
            }),
        )
    }

    fn parse_local_declaration(&mut self, start: usize, ty: Type) -> Statement {
        let mut declarators = Vec::new();
        let name_index = if self.expect(TokenKind::Identifier, "expected a local variable name") {
            Some(self.current - 1)
        } else {
            None
        };
        let missing_name = name_index.is_none();
        let mut initializer = None;
        if self.matches(TokenKind::Equal) {
            initializer = self.parse_expression(1);
            if initializer.is_none() {
                self.synchronize_statement();
                return self.error_statement(start);
            }
        }
        if let Some(index) = name_index {
            declarators.push(self.declarator(index, initializer));
        }
        while self.matches(TokenKind::Comma) {
            if !self.expect(TokenKind::Identifier, "expected a variable name after ','") {
                break;
            }
            let index = self.current - 1;
            let mut initializer = None;
            if self.matches(TokenKind::Equal) {
                initializer = self.parse_expression(1);
                if initializer.is_none() {
                    self.synchronize_statement();
                    return self.error_statement(start);
                }
            }
            declarators.push(self.declarator(index, initializer));
        }
        self.expect(TokenKind::Semicolon, "expected ';' after local declaration");
        if missing_name {
            return self.error_statement_code(start, "PAR007");
        }
        self.statement(
            start,
            StatementKind::Local(LocalVariableDeclaration {
                span: self.span_from(start, self.current),
                ty,
                declarators,
            }),
        )
    }

    fn parse_expression(&mut self, min_precedence: u8) -> Option<Expression> {
        let expression = self.parse_expression_at(min_precedence, 0)?;
        if self.options.lazy_lists_enabled() {
            self.validate_lazy_expression(&expression);
        }
        Some(expression)
    }

    /// Validate lazy-list nodes against the forms actually transformed by
    /// Firestorm: typed extraction (with an optional single parenthesis pair)
    /// and single-index assignment. A bare indexed read is not a valid form.
    fn validate_lazy_expression(&mut self, expression: &Expression) {
        match &expression.kind {
            ExpressionKind::LazyIndex(lazy) => {
                self.push_error(
                    "PAR007",
                    "lazy-list indexing requires a supported typed cast or simple indexed assignment",
                    lazy.span,
                );
                self.validate_lazy_index_children(lazy);
            }
            ExpressionKind::Assignment(assignment) => {
                if let ExpressionKind::LazyIndex(lazy) = &assignment.target.kind {
                    if lazy.indices.len() != 1 && lazy.indices.len() <= 2 {
                        self.push_error(
                            "PAR007",
                            "lazy-list indexed assignment requires exactly one index",
                            lazy.span,
                        );
                    }
                    if assignment.operator != AssignmentOperator::Assign {
                        self.push_error(
                            "PAR010",
                            "compound assignment is not supported for lazy-list indexing",
                            assignment.operator_span,
                        );
                    }
                    self.validate_lazy_index_children(lazy);
                } else {
                    self.validate_lazy_expression(&assignment.target);
                }
                self.validate_lazy_expression(&assignment.value);
            }
            ExpressionKind::Cast(cast) => {
                let quaternion_alias = self.is_firestorm_quaternion_alias(&cast.ty);
                // Firestorm's typed-reference regex allows one optional pair of
                // parentheses around the identifier/index expression.
                let operand = match &cast.operand.kind {
                    ExpressionKind::Parenthesized(inner) => inner.as_ref(),
                    _ => cast.operand.as_ref(),
                };
                if let ExpressionKind::LazyIndex(lazy) = &operand.kind {
                    let valid_count = if cast.ty.kind == TypeKind::List {
                        lazy.indices.len() == 2
                    } else {
                        lazy.indices.len() == 1
                    };
                    // parse_lazy_index already reports the >2 case, so do not
                    // emit a second arity diagnostic for the same source span.
                    if !valid_count && lazy.indices.len() <= 2 {
                        let message = if cast.ty.kind == TypeKind::List {
                            "list lazy-list extraction requires both start and end indices"
                        } else {
                            "scalar lazy-list extraction requires exactly one index"
                        };
                        self.push_error("PAR007", message, lazy.span);
                    }
                    self.validate_lazy_index_children(lazy);
                } else {
                    if quaternion_alias {
                        self.push_error(
                            "PAR007",
                            "Firestorm 'quaternion' is only a type alias for lazy-list extraction",
                            cast.span,
                        );
                    }
                    self.validate_lazy_expression(&cast.operand);
                }
            }
            ExpressionKind::Parenthesized(inner) => self.validate_lazy_expression(inner),
            ExpressionKind::List(list) => {
                for element in &list.elements {
                    self.validate_lazy_expression(element);
                }
            }
            ExpressionKind::Vector(vector) => {
                for component in &vector.components {
                    self.validate_lazy_expression(component);
                }
            }
            ExpressionKind::Rotation(rotation) => {
                for component in &rotation.components {
                    self.validate_lazy_expression(component);
                }
            }
            ExpressionKind::Call(call) => {
                self.validate_lazy_expression(&call.callee);
                for argument in &call.arguments {
                    self.validate_lazy_expression(argument);
                }
            }
            ExpressionKind::Component(component) => {
                self.validate_lazy_expression(&component.base);
            }
            ExpressionKind::Unary(unary) => self.validate_lazy_expression(&unary.operand),
            ExpressionKind::Binary(binary) => {
                self.validate_lazy_expression(&binary.left);
                self.validate_lazy_expression(&binary.right);
            }
            ExpressionKind::Increment(increment) => {
                self.validate_lazy_expression(&increment.operand);
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::String(_)
            | ExpressionKind::Error(_) => {}
        }
    }

    fn is_firestorm_quaternion_alias(&self, ty: &Type) -> bool {
        let Ok(index) = self
            .tokens
            .binary_search_by_key(&ty.span.start, |token| token.span.start)
        else {
            return false;
        };
        self.tokens
            .get(index)
            .is_some_and(|token| token.span == ty.span && token.lexeme == "quaternion")
    }

    fn validate_lazy_index_children(&mut self, lazy: &LazyIndexExpression) {
        for index in &lazy.indices {
            self.validate_lazy_expression(index);
        }
    }

    fn parse_expression_at(&mut self, min_precedence: u8, depth: usize) -> Option<Expression> {
        if depth > MAX_AST_RECURSION_DEPTH {
            let span = self.here_span();
            self.report_recursion(span);
            return None;
        }
        let expression_start = self.current;
        let mut left = self.parse_prefix(depth)?;
        while let Some((precedence, right_associative)) = binary_precedence(self.peek_kind()) {
            if precedence < min_precedence {
                break;
            }
            let operator_kind = self.peek_kind();
            let lazy_lvalue = self.options.lazy_lists_enabled()
                && matches!(left.kind, ExpressionKind::LazyIndex(_));
            if is_assignment(operator_kind)
                && !lazy_lvalue
                && !is_assignment_lvalue(self.tokens, expression_start, self.current)
            {
                self.error_here(
                    "PAR010",
                    "left-hand side of assignment must be a variable or vector/rotation component",
                );
            }
            let operator_span = self.here_span();
            self.advance();
            let next_min = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            let diagnostic_count = self.diagnostics.len();
            let right = self.parse_expression_at(next_min, depth + 1);
            let Some(right) = right else {
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
                return None;
            };
            let span = join_spans(left.span, right.span);
            let kind = if let Some(operator) = assignment_operator(operator_kind) {
                ExpressionKind::Assignment(AssignmentExpression {
                    span,
                    target: Box::new(left),
                    operator,
                    operator_span,
                    value: Box::new(right),
                })
            } else {
                let operator = binary_operator(operator_kind)?;
                ExpressionKind::Binary(BinaryExpression {
                    span,
                    left: Box::new(left),
                    operator,
                    operator_span,
                    right: Box::new(right),
                })
            };
            left = Expression { span, kind };
        }
        Some(left)
    }

    fn parse_prefix(&mut self, depth: usize) -> Option<Expression> {
        if depth > MAX_AST_RECURSION_DEPTH {
            let span = self.here_span();
            self.report_recursion(span);
            return None;
        }
        let start = self.current;
        if matches!(
            self.peek_kind(),
            TokenKind::Bang | TokenKind::Tilde | TokenKind::Plus | TokenKind::Minus
        ) {
            let operator_span = self.here_span();
            let operator = unary_operator(self.peek_kind())?;
            self.advance();
            let operand = self.parse_prefix(depth + 1)?;
            let span = join_spans(operator_span, operand.span);
            return Some(Expression {
                span,
                kind: ExpressionKind::Unary(UnaryExpression {
                    span,
                    operator,
                    operator_span,
                    operand: Box::new(operand),
                }),
            });
        }
        if matches!(
            self.peek_kind(),
            TokenKind::PlusPlus | TokenKind::MinusMinus
        ) {
            let operator_span = self.here_span();
            let operator = increment_operator(self.peek_kind())?;
            self.advance();
            let operand_start = self.current;
            let operand = self.parse_prefix(depth + 1)?;
            if !is_variable_lvalue(self.tokens, operand_start, self.current) {
                self.error_here("PAR011", "increment/decrement operand must be a variable");
            }
            let span = join_spans(operator_span, operand.span);
            return Some(Expression {
                span,
                kind: ExpressionKind::Increment(IncrementExpression {
                    span,
                    operator,
                    operator_span,
                    prefix: true,
                    operand: Box::new(operand),
                }),
            });
        }
        let standard_cast = self.check(TokenKind::LParen)
            && is_type_token(self.peek_kind_at(1))
            && self.peek_kind_at(2) == TokenKind::RParen;
        let lazy_quaternion_cast = self.options.lazy_lists_enabled()
            && self.check(TokenKind::LParen)
            && self.peek_kind_at(1) == TokenKind::Identifier
            && self.peek_kind_at(2) == TokenKind::RParen
            && self
                .tokens
                .get(self.current + 1)
                .is_some_and(|token| token.lexeme == "quaternion");
        if standard_cast || lazy_quaternion_cast {
            let start_span = self.here_span();
            self.advance();
            let ty = if lazy_quaternion_cast {
                let token = self.tokens.get(self.current)?;
                let ty = Type {
                    kind: TypeKind::Rotation,
                    span: token.span,
                };
                self.advance();
                ty
            } else {
                self.parse_type()?
            };
            self.advance(); // ')'
            let operand = self.parse_prefix(depth + 1)?;
            let span = join_spans(start_span, operand.span);
            return Some(Expression {
                span,
                kind: ExpressionKind::Cast(CastExpression {
                    span,
                    ty,
                    operand: Box::new(operand),
                }),
            });
        }

        let primary_start = self.current;
        let mut call_suffix_allowed = self.check(TokenKind::Identifier);
        let mut expression = self.parse_primary(depth)?;
        loop {
            if self.check(TokenKind::LParen) {
                if !call_suffix_allowed {
                    self.error_here("PAR007", "function call suffix requires an identifier");
                }
                let open_paren_span = self.here_span();
                self.advance();
                let parsed = self.parse_argument_list(depth + 1)?;
                let span = join_spans(expression.span, parsed.close_span);
                expression = Expression {
                    span,
                    kind: ExpressionKind::Call(CallExpression {
                        span,
                        callee: Box::new(expression),
                        open_paren_span,
                        close_paren_span: parsed.close_span,
                        declared_argument_count: parsed.declared_count,
                        arguments_complete: parsed.complete,
                        matched_close: parsed.matched_close,
                        arguments: parsed.arguments,
                    }),
                };
                call_suffix_allowed = false;
            } else if self.check(TokenKind::Dot) {
                let dot_index = self.current;
                self.advance();
                let base_is_variable = is_variable_lvalue(self.tokens, primary_start, dot_index);
                let component_token = self.tokens.get(self.current);
                let component = component_token.and_then(|token| {
                    if token.kind == TokenKind::Identifier {
                        component_from_name(&token.lexeme)
                    } else {
                        None
                    }
                });
                if !base_is_variable || component.is_none() {
                    self.error_here(
                        "PAR012",
                        "vector/rotation component access requires a variable and a valid component name",
                    );
                }
                if !self.expect(TokenKind::Identifier, "expected a component name after '.'") {
                    return None;
                }
                let component_index = self.current - 1;
                let component = component.unwrap_or(Component::Unknown);
                let component_name = self.tokens[component_index].lexeme.clone();
                let span = join_spans(expression.span, self.tokens[component_index].span);
                expression = Expression {
                    span,
                    kind: ExpressionKind::Component(ComponentAccessExpression {
                        span,
                        base: Box::new(expression),
                        component,
                        component_name,
                        component_span: self.tokens[component_index].span,
                    }),
                };
                call_suffix_allowed = false;
            } else if self.options.lazy_lists_enabled() && self.check(TokenKind::LBracket) {
                expression = self.parse_lazy_index(expression, depth)?;
                call_suffix_allowed = false;
            } else if self.check(TokenKind::PlusPlus) || self.check(TokenKind::MinusMinus) {
                if !is_variable_lvalue(self.tokens, primary_start, self.current) {
                    self.error_here("PAR011", "increment/decrement operand must be a variable");
                }
                let operator_span = self.here_span();
                let operator = increment_operator(self.peek_kind())?;
                self.advance();
                let span = join_spans(expression.span, operator_span);
                expression = Expression {
                    span,
                    kind: ExpressionKind::Increment(IncrementExpression {
                        span,
                        operator,
                        operator_span,
                        prefix: false,
                        operand: Box::new(expression),
                    }),
                };
                call_suffix_allowed = false;
            } else {
                break;
            }
        }
        let _ = start;
        Some(expression)
    }

    fn parse_lazy_index(&mut self, base: Expression, depth: usize) -> Option<Expression> {
        let bracket_start = self.current;
        let base_is_identifier = matches!(base.kind, ExpressionKind::Identifier(_));
        self.advance();
        let mut indices = Vec::new();
        if self.check(TokenKind::RBracket) {
            self.error_here("PAR006", "lazy-list index requires an expression");
            self.advance();
            return Some(self.error_expression(self.span_from(bracket_start, self.current)));
        }
        loop {
            let Some(index) = self.parse_expression_at(1, depth + 1) else {
                self.synchronize_lazy_index();
                return None;
            };
            indices.push(index);
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        if !self.expect(TokenKind::RBracket, "expected ']' after lazy-list index") {
            return None;
        }
        if indices.len() > 2 {
            self.push_error(
                "PAR007",
                "lazy-list index accepts at most two expressions",
                self.span_from(bracket_start, self.current),
            );
        }
        if !base_is_identifier {
            self.push_error(
                "PAR012",
                "lazy-list indexing requires an identifier base",
                self.span_from(bracket_start, self.current),
            );
        }
        let span = join_spans(base.span, self.previous_span());
        Some(Expression {
            span,
            kind: ExpressionKind::LazyIndex(LazyIndexExpression {
                span,
                base: Box::new(base),
                indices,
            }),
        })
    }

    fn synchronize_lazy_index(&mut self) {
        let mut bracket_depth = 0usize;
        let mut paren_depth = 0usize;
        while !self.at_end() {
            if bracket_depth == 0 && paren_depth == 0 {
                if self.check(TokenKind::RBracket) {
                    self.advance();
                    return;
                }
                if self.check(TokenKind::RBrace) || self.check(TokenKind::Semicolon) {
                    return;
                }
            }
            match self.peek_kind() {
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    fn parse_primary(&mut self, depth: usize) -> Option<Expression> {
        let start = self.current;
        let expression = match self.peek_kind() {
            TokenKind::Identifier => {
                let token = self.advance();
                Expression {
                    span: token.span,
                    kind: ExpressionKind::Identifier(Identifier {
                        name: token.lexeme.clone(),
                        span: token.span,
                    }),
                }
            }
            TokenKind::IntegerLiteral => {
                let token = self.advance();
                Expression {
                    span: token.span,
                    kind: ExpressionKind::Integer(IntegerLiteral {
                        text: token.lexeme.clone(),
                        span: token.span,
                    }),
                }
            }
            TokenKind::FloatLiteral => {
                let token = self.advance();
                Expression {
                    span: token.span,
                    kind: ExpressionKind::Float(FloatLiteral {
                        text: token.lexeme.clone(),
                        span: token.span,
                    }),
                }
            }
            TokenKind::StringLiteral => {
                let token = self.advance();
                Expression {
                    span: token.span,
                    kind: ExpressionKind::String(StringLiteral {
                        text: token.lexeme.clone(),
                        span: token.span,
                    }),
                }
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expression_at(1, depth + 1)?;
                if !self.expect(TokenKind::RParen, "expected ')' after expression") {
                    return None;
                }
                let span = self.span_from(start, self.current);
                Expression {
                    span,
                    kind: ExpressionKind::Parenthesized(Box::new(inner)),
                }
            }
            TokenKind::LBracket => {
                self.advance();
                let elements = self.parse_delimited_expressions(TokenKind::RBracket, depth + 1)?;
                let span = self.span_from(start, self.current);
                Expression {
                    span,
                    kind: ExpressionKind::List(ListExpression { span, elements }),
                }
            }
            TokenKind::Less => {
                self.advance();
                let mut components = Vec::new();
                for component_index in 0..3 {
                    let component = self.parse_expression_at(10, depth + 1)?;
                    components.push(component);
                    if component_index < 2
                        && !self.expect(
                            TokenKind::Comma,
                            if component_index == 0 {
                                "expected ',' after first vector component"
                            } else {
                                "expected ',' after second vector component"
                            },
                        )
                    {
                        return None;
                    }
                }
                if self.matches(TokenKind::Comma) {
                    let component = self.parse_expression_at(10, depth + 1)?;
                    components.push(component);
                }
                if !self.expect(
                    TokenKind::Greater,
                    "expected '>' after vector or rotation literal",
                ) {
                    return None;
                }
                let span = self.span_from(start, self.current);
                if components.len() == 4 {
                    Expression {
                        span,
                        kind: ExpressionKind::Rotation(RotationExpression { span, components }),
                    }
                } else {
                    Expression {
                        span,
                        kind: ExpressionKind::Vector(VectorExpression { span, components }),
                    }
                }
            }
            _ => {
                self.error_here("PAR006", "expected an expression");
                return None;
            }
        };
        Some(expression)
    }

    fn parse_argument_list(&mut self, depth: usize) -> Option<ParsedArguments> {
        let open = self.current.checked_sub(1)?;
        let mut arguments = Vec::new();
        if self.matches(TokenKind::RParen) {
            return Some(ParsedArguments {
                arguments,
                complete: true,
                close_span: self.previous_span(),
                declared_count: 0,
                matched_close: true,
            });
        }
        loop {
            let Some(expression) = self.parse_expression_at(1, depth) else {
                self.synchronize_arguments();
                if self.current > 0 && self.tokens[self.current - 1].kind == TokenKind::RParen {
                    let close = self.current - 1;
                    return Some(ParsedArguments {
                        arguments,
                        complete: false,
                        close_span: self.tokens[close].span,
                        declared_count: self.count_call_arguments(open, close),
                        matched_close: true,
                    });
                }
                if arguments.is_empty() && self.check(TokenKind::Semicolon) {
                    let recovered = self.recover_argument_identifiers(open + 1, self.current);
                    if !recovered.is_empty() {
                        return Some(ParsedArguments {
                            declared_count: recovered.len(),
                            arguments: recovered,
                            complete: false,
                            close_span: self.here_span(),
                            matched_close: false,
                        });
                    }
                }
                return None;
            };
            arguments.push(expression);
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        if self.expect(TokenKind::RParen, "expected ')' after arguments") {
            let close = self.current - 1;
            return Some(ParsedArguments {
                declared_count: arguments.len(),
                arguments,
                complete: true,
                close_span: self.tokens[close].span,
                matched_close: true,
            });
        }
        if let Some(close) = self.matching_call_close(open) {
            while self.current <= close {
                self.advance();
            }
            let declared_count = self.count_call_arguments(open, close);
            return Some(ParsedArguments {
                arguments,
                complete: false,
                close_span: self.tokens[close].span,
                declared_count,
                matched_close: true,
            });
        }
        if self.check(TokenKind::Semicolon) {
            return Some(ParsedArguments {
                declared_count: arguments.len(),
                arguments,
                complete: false,
                close_span: self.here_span(),
                matched_close: false,
            });
        }
        None
    }

    fn matching_call_close(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(open) {
            match token.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => {
                    if brace_depth == 0 {
                        return None;
                    }
                    brace_depth -= 1;
                }
                TokenKind::Semicolon if bracket_depth == 0 && brace_depth == 0 && depth == 1 => {
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    fn count_call_arguments(&self, open: usize, close: usize) -> usize {
        if close == open + 1 {
            return 0;
        }
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut vector_depth = 0usize;
        let mut commas = 0usize;
        let mut has_argument = false;
        for (index, token) in self.tokens.iter().enumerate().take(close).skip(open + 1) {
            match token.kind {
                TokenKind::LParen => {
                    paren_depth += 1;
                    has_argument = true;
                }
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => {
                    bracket_depth += 1;
                    has_argument = true;
                }
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::LBrace => {
                    brace_depth += 1;
                    has_argument = true;
                }
                TokenKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
                TokenKind::Less if self.starts_vector_literal(index) => {
                    vector_depth += 1;
                    has_argument = true;
                }
                TokenKind::Greater if vector_depth > 0 => vector_depth -= 1,
                TokenKind::Comma
                    if paren_depth == 0
                        && bracket_depth == 0
                        && brace_depth == 0
                        && vector_depth == 0 =>
                {
                    commas += 1;
                    has_argument = true;
                }
                _ => has_argument = true,
            }
        }
        if has_argument {
            commas + 1
        } else {
            0
        }
    }

    fn starts_vector_literal(&self, index: usize) -> bool {
        let Some(previous) = index
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
        else {
            return true;
        };
        match previous.kind {
            TokenKind::KwReturn
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::Comma
            | TokenKind::Equal
            | TokenKind::PlusEqual
            | TokenKind::MinusEqual
            | TokenKind::StarEqual
            | TokenKind::SlashEqual
            | TokenKind::PercentEqual
            | TokenKind::EqualEqual
            | TokenKind::BangEqual
            | TokenKind::Question
            | TokenKind::Colon
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::And
            | TokenKind::Or
            | TokenKind::Xor
            | TokenKind::AndAnd
            | TokenKind::OrOr => true,
            TokenKind::RParen => self.closes_explicit_cast_parentheses(index - 1),
            _ => false,
        }
    }

    fn closes_explicit_cast_parentheses(&self, close: usize) -> bool {
        let Some(open) = close.checked_sub(2) else {
            return false;
        };
        self.tokens.get(close).map(|token| token.kind) == Some(TokenKind::RParen)
            && self.tokens.get(open).map(|token| token.kind) == Some(TokenKind::LParen)
            && self
                .tokens
                .get(open + 1)
                .is_some_and(|token| is_type_token(token.kind))
    }

    fn parse_delimited_expressions(
        &mut self,
        closing: TokenKind,
        depth: usize,
    ) -> Option<Vec<Expression>> {
        let mut expressions = Vec::new();
        if self.matches(closing) {
            return Some(expressions);
        }
        loop {
            let expression = self.parse_expression_at(1, depth)?;
            expressions.push(expression);
            if !self.matches(TokenKind::Comma) {
                break;
            }
        }
        if self.expect(closing, "expected closing delimiter") {
            Some(expressions)
        } else {
            None
        }
    }

    fn parse_type(&mut self) -> Option<Type> {
        let token = self.tokens.get(self.current)?;
        let kind = type_kind(token.kind)?;
        let ty = Type {
            kind,
            span: token.span,
        };
        self.advance();
        Some(ty)
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

    fn previous_span(&self) -> Span {
        self.tokens
            .get(self.current.saturating_sub(1))
            .map_or_else(|| self.fallback_span(), |token| token.span)
    }

    fn here_span(&self) -> Span {
        self.tokens
            .get(self.current)
            .or_else(|| self.tokens.last())
            .map_or_else(|| self.fallback_span(), |token| token.span)
    }

    fn token_span(&self, index: usize) -> Span {
        self.tokens
            .get(index)
            .or_else(|| self.tokens.last())
            .map_or_else(|| self.fallback_span(), |token| token.span)
    }

    fn token_text(&self, index: usize) -> String {
        self.tokens
            .get(index)
            .map(|token| token.lexeme.clone())
            .unwrap_or_default()
    }

    fn fallback_span(&self) -> Span {
        Span {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        }
    }

    fn span_from(&self, start: usize, end_exclusive: usize) -> Span {
        if end_exclusive > start {
            join_spans(self.token_span(start), self.token_span(end_exclusive - 1))
        } else {
            self.token_span(start)
        }
    }

    fn statement(&self, start: usize, kind: StatementKind) -> Statement {
        Statement {
            span: self.span_from(start, self.current),
            kind,
        }
    }

    fn error_statement(&self, start: usize) -> Statement {
        self.error_statement_code(start, "PAR007")
    }

    fn error_statement_code(&self, start: usize, code: &'static str) -> Statement {
        let span = self.span_from(start, self.current);
        Statement {
            span,
            kind: StatementKind::Error(ErrorNode {
                span,
                code: Some(code),
            }),
        }
    }

    fn error_expression(&self, span: Span) -> Expression {
        Expression {
            span,
            kind: ExpressionKind::Error(ErrorNode { span, code: None }),
        }
    }

    fn error_here(&mut self, code: &'static str, message: impl Into<String>) {
        if self.at_end() && self.eof_reported {
            return;
        }
        let span = self.here_span();
        self.push_error(code, message, span);
        if self.at_end() && self.suppress_diagnostics == 0 {
            self.eof_reported = true;
        }
    }

    fn push_error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        if self.suppress_diagnostics > 0 {
            return;
        }
        self.diagnostics.push(Diagnostic {
            code,
            severity: Severity::Error,
            kind: DiagnosticKind::Syntax,
            message: message.into(),
            span,
        });
    }

    fn report_recursion(&mut self, span: Span) {
        if self.recursion_reported {
            return;
        }
        self.recursion_reported = true;
        self.push_error(
            AST_RECURSION_LIMIT,
            format!("AST recursion depth exceeds {MAX_AST_RECURSION_DEPTH} levels"),
            span,
        );
    }

    fn skip_deep_statement(&mut self) {
        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut do_seen = false;
        while !self.at_end() {
            let continuation = self.peek_kind_at(1) == TokenKind::KwElse
                || (do_seen && self.peek_kind_at(1) == TokenKind::KwWhile);
            match self.peek_kind() {
                TokenKind::RBrace if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 => {
                    return;
                }
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => {
                    brace_depth = brace_depth.saturating_sub(1);
                    self.advance();
                    if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 && !continuation {
                        return;
                    }
                    continue;
                }
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::KwDo => do_seen = true,
                TokenKind::Semicolon
                    if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 =>
                {
                    self.advance();
                    if !continuation {
                        return;
                    }
                    continue;
                }
                _ => {}
            }
            self.advance();
        }
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

    fn recover_parameters(&self, open: usize, close: usize) -> Vec<Parameter> {
        let mut parameters = Vec::new();
        let mut index = open.saturating_add(1);
        while index < close {
            let Some(ty_kind) = type_kind(self.tokens[index].kind) else {
                index += 1;
                continue;
            };
            let Some(name) = self.tokens.get(index + 1) else {
                break;
            };
            if name.kind != TokenKind::Identifier || index + 1 >= close {
                index += 1;
                continue;
            }
            let ty = Type {
                kind: ty_kind,
                span: self.tokens[index].span,
            };
            parameters.push(Parameter {
                span: join_spans(ty.span, name.span),
                ty,
                name: name.lexeme.clone(),
                name_span: name.span,
            });
            index += 2;
        }
        parameters
    }

    fn parameter_type_kinds_between(&self, open: usize, close: usize) -> Vec<TypeKind> {
        let mut types = Vec::new();
        let mut expect_type = true;
        for token in self.tokens.iter().take(close).skip(open + 1) {
            if token.kind == TokenKind::Comma {
                expect_type = true;
                continue;
            }
            if expect_type {
                if let Some(kind) = type_kind(token.kind) {
                    types.push(kind);
                    expect_type = false;
                }
            }
        }
        types
    }

    fn count_declared_parameters(&self, open: usize, close: usize) -> usize {
        if close == open + 1 {
            return 0;
        }
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut vector_depth = 0usize;
        let mut commas = 0usize;
        let mut has_argument = false;
        for token in self.tokens.iter().take(close).skip(open + 1) {
            match token.kind {
                TokenKind::LParen => {
                    paren_depth += 1;
                    has_argument = true;
                }
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => {
                    bracket_depth += 1;
                    has_argument = true;
                }
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::LBrace => {
                    brace_depth += 1;
                    has_argument = true;
                }
                TokenKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
                TokenKind::Less => {
                    vector_depth += 1;
                    has_argument = true;
                }
                TokenKind::Greater if vector_depth > 0 => vector_depth -= 1,
                TokenKind::Comma
                    if paren_depth == 0
                        && bracket_depth == 0
                        && brace_depth == 0
                        && vector_depth == 0 =>
                {
                    commas += 1;
                    has_argument = true;
                }
                _ => has_argument = true,
            }
        }
        if has_argument {
            commas + 1
        } else {
            0
        }
    }

    fn event_parameter_close_before_body(&self) -> Option<usize> {
        let mut index = self.current;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        while let Some(token) = self.tokens.get(index) {
            if paren_depth == 0 && bracket_depth == 0 {
                if token.kind == TokenKind::RParen {
                    return self
                        .tokens
                        .get(index + 1)
                        .is_some_and(|next| next.kind == TokenKind::LBrace)
                        .then_some(index);
                }
                if matches!(
                    token.kind,
                    TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon
                ) {
                    return None;
                }
            }
            match token.kind {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            index += 1;
        }
        None
    }

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

    fn recover_parameter_references(&self, open: usize, close: usize) -> Vec<Expression> {
        let mut expressions = Vec::new();
        let mut index = open + 1;
        while index < close {
            if self.tokens[index].kind != TokenKind::Identifier {
                index += 1;
                continue;
            }
            let previous = index
                .checked_sub(1)
                .and_then(|index| self.tokens.get(index));
            let next = self.tokens.get(index + 1);
            if previous.is_some_and(|token| {
                matches!(
                    token.kind,
                    TokenKind::Dot | TokenKind::At | TokenKind::KwJump | TokenKind::KwState
                ) || is_type_token(token.kind)
            }) || next.is_some_and(|token| token.kind == TokenKind::LParen)
            {
                index += 1;
                continue;
            }
            expressions.push(Expression {
                span: self.tokens[index].span,
                kind: ExpressionKind::Identifier(Identifier {
                    name: self.tokens[index].lexeme.clone(),
                    span: self.tokens[index].span,
                }),
            });
            index += 1;
        }
        expressions
    }

    fn recover_argument_identifiers(&self, start: usize, end: usize) -> Vec<Expression> {
        let mut expressions = Vec::new();
        let mut index = start;
        while index < end {
            if self.tokens[index].kind != TokenKind::Identifier {
                index += 1;
                continue;
            }
            let previous = index
                .checked_sub(1)
                .and_then(|index| self.tokens.get(index));
            let next = self.tokens.get(index + 1);
            if previous.is_some_and(|token| {
                matches!(
                    token.kind,
                    TokenKind::Dot | TokenKind::At | TokenKind::KwJump | TokenKind::KwState
                ) || is_type_token(token.kind)
            }) || next.is_some_and(|token| token.kind == TokenKind::LParen)
            {
                index += 1;
                continue;
            }
            expressions.push(Expression {
                span: self.tokens[index].span,
                kind: ExpressionKind::Identifier(Identifier {
                    name: self.tokens[index].lexeme.clone(),
                    span: self.tokens[index].span,
                }),
            });
            index += 1;
        }
        expressions
    }

    fn recover_return_flow_nodes(
        &self,
        start: usize,
        semicolon: usize,
    ) -> (Vec<RecoveredLabelStatement>, Vec<RecoveredJumpStatement>) {
        let mut labels = Vec::new();
        let mut jumps = Vec::new();
        let mut scope_path = Vec::new();
        let mut index = start;
        while index <= semicolon {
            match self.tokens[index].kind {
                TokenKind::LBrace => {
                    scope_path.push(self.token_span(index));
                    index += 1;
                }
                TokenKind::RBrace => {
                    scope_path.pop();
                    index += 1;
                }
                TokenKind::At
                    if index + 2 <= semicolon
                        && self.tokens[index + 1].kind == TokenKind::Identifier
                        && self.tokens[index + 2].kind == TokenKind::Semicolon =>
                {
                    labels.push(RecoveredLabelStatement {
                        label: LabelStatement {
                            span: join_spans(self.tokens[index].span, self.tokens[index + 2].span),
                            name: self.token_text(index + 1),
                            name_span: self.token_span(index + 1),
                        },
                        scope_path: scope_path.clone(),
                    });
                    index += 3;
                }
                TokenKind::KwJump
                    if index < semicolon
                        && self.tokens[index + 1].kind == TokenKind::Identifier =>
                {
                    jumps.push(RecoveredJumpStatement {
                        jump: JumpStatement {
                            span: join_spans(self.tokens[index].span, self.tokens[index + 1].span),
                            label: self.token_text(index + 1),
                            label_span: self.token_span(index + 1),
                        },
                        scope_path: scope_path.clone(),
                    });
                    index += 2;
                }
                _ => index += 1,
            }
        }
        (labels, jumps)
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

fn join_spans(first: Span, second: Span) -> Span {
    Span {
        start: first.start,
        end: second.end,
        line: first.line,
        column: first.column,
        end_line: second.end_line,
        end_column: second.end_column,
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

fn type_kind_name(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Integer => "integer",
        TypeKind::Float => "float",
        TypeKind::String => "string",
        TypeKind::Key => "key",
        TypeKind::List => "list",
        TypeKind::Vector => "vector",
        TypeKind::Rotation => "rotation",
    }
}

fn type_kind(kind: TokenKind) -> Option<TypeKind> {
    match kind {
        TokenKind::KwInteger => Some(TypeKind::Integer),
        TokenKind::KwFloat => Some(TypeKind::Float),
        TokenKind::KwString => Some(TypeKind::String),
        TokenKind::KwKey => Some(TypeKind::Key),
        TokenKind::KwList => Some(TypeKind::List),
        TokenKind::KwVector => Some(TypeKind::Vector),
        TokenKind::KwRotation => Some(TypeKind::Rotation),
        _ => None,
    }
}

fn is_assignment(kind: TokenKind) -> bool {
    assignment_operator(kind).is_some()
}

fn assignment_operator(kind: TokenKind) -> Option<AssignmentOperator> {
    match kind {
        TokenKind::Equal => Some(AssignmentOperator::Assign),
        TokenKind::PlusEqual => Some(AssignmentOperator::AddAssign),
        TokenKind::MinusEqual => Some(AssignmentOperator::SubtractAssign),
        TokenKind::StarEqual => Some(AssignmentOperator::MultiplyAssign),
        TokenKind::SlashEqual => Some(AssignmentOperator::DivideAssign),
        TokenKind::PercentEqual => Some(AssignmentOperator::RemainderAssign),
        _ => None,
    }
}

fn unary_operator(kind: TokenKind) -> Option<UnaryOperator> {
    match kind {
        TokenKind::Plus => Some(UnaryOperator::Plus),
        TokenKind::Minus => Some(UnaryOperator::Minus),
        TokenKind::Bang => Some(UnaryOperator::Bang),
        TokenKind::Tilde => Some(UnaryOperator::Tilde),
        _ => None,
    }
}

fn increment_operator(kind: TokenKind) -> Option<IncrementOperator> {
    match kind {
        TokenKind::PlusPlus => Some(IncrementOperator::Increment),
        TokenKind::MinusMinus => Some(IncrementOperator::Decrement),
        _ => None,
    }
}

fn component_from_name(name: &str) -> Option<Component> {
    match name {
        "x" => Some(Component::X),
        "y" => Some(Component::Y),
        "z" => Some(Component::Z),
        "s" => Some(Component::S),
        _ => None,
    }
}

fn binary_operator(kind: TokenKind) -> Option<BinaryOperator> {
    match kind {
        TokenKind::OrOr => Some(BinaryOperator::OrOr),
        TokenKind::AndAnd => Some(BinaryOperator::AndAnd),
        TokenKind::Or => Some(BinaryOperator::BitOr),
        TokenKind::Xor => Some(BinaryOperator::BitXor),
        TokenKind::And => Some(BinaryOperator::BitAnd),
        TokenKind::EqualEqual => Some(BinaryOperator::Equal),
        TokenKind::BangEqual => Some(BinaryOperator::NotEqual),
        TokenKind::Less => Some(BinaryOperator::Less),
        TokenKind::LessEqual => Some(BinaryOperator::LessEqual),
        TokenKind::Greater => Some(BinaryOperator::Greater),
        TokenKind::GreaterEqual => Some(BinaryOperator::GreaterEqual),
        TokenKind::ShiftLeft => Some(BinaryOperator::ShiftLeft),
        TokenKind::ShiftRight => Some(BinaryOperator::ShiftRight),
        TokenKind::Plus => Some(BinaryOperator::Add),
        TokenKind::Minus => Some(BinaryOperator::Subtract),
        TokenKind::Star => Some(BinaryOperator::Multiply),
        TokenKind::Slash => Some(BinaryOperator::Divide),
        TokenKind::Percent => Some(BinaryOperator::Remainder),
        _ => None,
    }
}

fn binary_precedence(kind: TokenKind) -> Option<(u8, bool)> {
    let precedence = match kind {
        TokenKind::Equal
        | TokenKind::PlusEqual
        | TokenKind::MinusEqual
        | TokenKind::StarEqual
        | TokenKind::SlashEqual
        | TokenKind::PercentEqual => return Some((1, true)),
        TokenKind::OrOr | TokenKind::AndAnd => 3,
        TokenKind::Or => 5,
        TokenKind::Xor => 6,
        TokenKind::And => 7,
        TokenKind::EqualEqual | TokenKind::BangEqual => 8,
        TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual => 9,
        TokenKind::ShiftLeft | TokenKind::ShiftRight => 10,
        TokenKind::Plus => 11,
        TokenKind::Minus => 12,
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => 13,
        _ => return None,
    };
    Some((precedence, false))
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

/// Parse a prepared token stream into a typed AST and syntax diagnostics.
///
/// The explicit grammar recursion guard stops at
/// [`MAX_AST_RECURSION_DEPTH`]. Parsing runs on a dedicated, fixed-size stack so
/// the guard itself is reached safely even in unoptimized debug builds; the
/// thread is scoped to this call and no state is shared between invocations.
pub fn parse_ast(tokens: &[Token]) -> (Ast, Vec<Diagnostic>) {
    parse_ast_with_options(tokens, AstParseOptions::lsl())
}

/// Parse with independent Firestorm extension gates.
pub fn parse_ast_with_options(
    tokens: &[Token],
    options: AstParseOptions,
) -> (Ast, Vec<Diagnostic>) {
    const PARSER_STACK_BYTES: usize = 16 * 1024 * 1024;
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("neolint-ast-parser".to_owned())
            .stack_size(PARSER_STACK_BYTES)
            .spawn_scoped(scope, || Parser::new(tokens, options).run());
        match worker {
            Ok(handle) => match handle.join() {
                Ok(result) => result,
                Err(_) => {
                    worker_failure_result(tokens, "AST parser worker terminated unexpectedly")
                }
            },
            Err(_) => worker_failure_result(tokens, "AST parser worker stack could not be created"),
        }
    })
}

fn worker_failure_result(tokens: &[Token], reason: &str) -> (Ast, Vec<Diagnostic>) {
    let span = tokens
        .iter()
        .find(|token| token.kind != TokenKind::Eof)
        .or_else(|| tokens.last())
        .map_or(
            Span {
                start: 0,
                end: 0,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            },
            |token| token.span,
        );
    (
        Ast {
            items: Vec::new(),
            recovered_functions: Vec::new(),
            span,
        },
        vec![Diagnostic {
            code: AST_WORKER_UNAVAILABLE,
            severity: Severity::Error,
            kind: DiagnosticKind::Syntax,
            message: format!("{reason}; parsing was aborted safely"),
            span,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_failure_returns_empty_ast_and_one_safe_diagnostic() {
        let tokens = vec![Token {
            kind: TokenKind::Eof,
            lexeme: String::new(),
            span: Span {
                start: 3,
                end: 3,
                line: 1,
                column: 4,
                end_line: 1,
                end_column: 4,
            },
        }];
        let (ast, diagnostics) = worker_failure_result(&tokens, "worker unavailable");
        assert!(ast.items.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, AST_WORKER_UNAVAILABLE);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Syntax);
    }
}
