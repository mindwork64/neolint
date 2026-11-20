use crate::builtins::definitions;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};
use crate::lexer::{Token, TokenKind};
use std::collections::HashMap;

fn is_declaration_type(kind: TokenKind) -> bool {
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

fn is_value_type(kind: TokenKind) -> bool {
    lsl_type_name(kind).is_some()
}

/// Return (function-name index, opening-parenthesis index, return type).
/// LSL void functions omit a return type; "void" is only an internal type marker.
fn function_header_at(tokens: &[Token], index: usize) -> Option<(usize, usize, String)> {
    if let Some(return_type) = lsl_type_name(tokens.get(index)?.kind) {
        let name_index = index + 1;
        let open_index = index + 2;
        if tokens.get(name_index)?.kind == TokenKind::Identifier
            && tokens.get(open_index)?.kind == TokenKind::LParen
        {
            return Some((name_index, open_index, return_type.to_owned()));
        }
    } else if tokens[index].kind == TokenKind::Identifier
        && (index == 0 || !is_value_type(tokens[index - 1].kind))
        && tokens.get(index + 1)?.kind == TokenKind::LParen
    {
        return Some((index, index + 1, "void".to_owned()));
    }

    None
}

#[derive(Debug, Clone)]
struct VariableBinding {
    type_name: String,
    declaration_index: usize,
    span: crate::diagnostic::Span,
}

#[derive(Debug, Clone)]
struct VariableInitializer {
    name_index: usize,
    type_name: String,
    expression_start: usize,
    expression_end: usize,
}

#[derive(Debug, Default)]
struct LexicalScope {
    parent: Option<usize>,
    variables: HashMap<String, Vec<VariableBinding>>,
}

struct ScopeModel {
    scopes: Vec<LexicalScope>,
    scope_at: Vec<usize>,
    diagnostics: Vec<Diagnostic>,
    initializers: Vec<VariableInitializer>,
}

impl ScopeModel {
    fn build(tokens: &[Token]) -> Self {
        let mut scopes = vec![LexicalScope::default()];
        let mut stack = vec![0usize];
        let mut scope_at = vec![0usize; tokens.len()];

        for (index, token) in tokens.iter().enumerate() {
            let current = *stack.last().expect("root scope always exists");
            scope_at[index] = current;

            match token.kind {
                TokenKind::LBrace => {
                    let child = scopes.len();
                    scopes.push(LexicalScope {
                        parent: Some(current),
                        variables: HashMap::new(),
                    });
                    stack.push(child);
                }
                TokenKind::RBrace if stack.len() > 1 => {
                    stack.pop();
                }
                _ => {}
            }
        }

        let mut model = Self {
            scopes,
            scope_at,
            diagnostics: Vec::new(),
            initializers: Vec::new(),
        };
        model.register_function_parameters(tokens);
        model.register_event_parameters(tokens);
        model.register_variables(tokens);
        model
    }

    fn add_variable(
        &mut self,
        scope_id: usize,
        name: &Token,
        type_name: &str,
        declaration_index: usize,
    ) {
        if scope_id != 0 {
            if let Some(previous) = self
                .scopes
                .get(scope_id)
                .and_then(|scope| scope.variables.get(&name.lexeme))
                .and_then(|bindings| bindings.last())
            {
                self.diagnostics.push(Diagnostic {
                    code: "SEM007",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "duplicate variable '{}' in the same scope; first declared at {}:{}",
                        name.lexeme, previous.span.line, previous.span.column
                    ),
                    span: name.span,
                });
            }
        }

        if let Some(scope) = self.scopes.get_mut(scope_id) {
            scope
                .variables
                .entry(name.lexeme.clone())
                .or_default()
                .push(VariableBinding {
                    type_name: type_name.to_owned(),
                    declaration_index,
                    span: name.span,
                });
        }
    }

    fn resolve(&self, name: &str, use_index: usize) -> Option<String> {
        let mut scope_id = *self.scope_at.get(use_index)?;
        loop {
            let scope = self.scopes.get(scope_id)?;
            if let Some(bindings) = scope.variables.get(name) {
                if let Some(binding) = bindings
                    .iter()
                    .rev()
                    .find(|binding| binding.declaration_index <= use_index)
                {
                    return Some(binding.type_name.clone());
                }
            }

            let parent = scope.parent?;
            scope_id = parent;
        }
    }

    fn is_ancestor(&self, ancestor_id: usize, mut scope_id: usize) -> bool {
        loop {
            if scope_id == ancestor_id {
                return true;
            }
            let Some(parent) = self.scopes.get(scope_id).and_then(|scope| scope.parent) else {
                return false;
            };
            scope_id = parent;
        }
    }

    fn register_function_parameters(&mut self, tokens: &[Token]) {
        for index in 0..tokens.len() {
            if self.scope_at.get(index) != Some(&0) {
                continue;
            }

            let Some((_, open, _)) = function_header_at(tokens, index) else {
                continue;
            };
            let Some(close) = matching_paren(tokens, open) else {
                continue;
            };
            let body_open = close + 1;
            if tokens.get(body_open).map(|token| token.kind) != Some(TokenKind::LBrace) {
                continue;
            }

            let Some(body_scope) = self.scope_at.get(body_open + 1).copied() else {
                continue;
            };
            self.register_parameters(tokens, open, close, body_scope);
        }
    }

    fn register_event_parameters(&mut self, tokens: &[Token]) {
        let mut state_scopes = Vec::new();

        for (index, token) in tokens.iter().enumerate() {
            let body_open = match token.kind {
                TokenKind::KwDefault => index + 1,
                TokenKind::KwState => index + 2,
                _ => continue,
            };

            if tokens.get(body_open).map(|item| item.kind) == Some(TokenKind::LBrace) {
                if let Some(scope_id) = self.scope_at.get(body_open + 1).copied() {
                    state_scopes.push(scope_id);
                }
            }
        }

        let events = &definitions().events;
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !state_scopes.contains(self.scope_at.get(index).unwrap_or(&usize::MAX))
                || !events.contains_key(token.lexeme.as_str())
                || tokens.get(index + 1).map(|item| item.kind) != Some(TokenKind::LParen)
            {
                continue;
            }

            let open = index + 1;
            let Some(close) = matching_paren(tokens, open) else {
                continue;
            };
            let body_open = close + 1;
            if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
                continue;
            }

            let Some(body_scope) = self.scope_at.get(body_open + 1).copied() else {
                continue;
            };
            self.register_parameters(tokens, open, close, body_scope);
        }
    }

    fn register_parameters(
        &mut self,
        tokens: &[Token],
        open: usize,
        close: usize,
        scope_id: usize,
    ) {
        let mut index = open + 1;
        while index < close {
            if let Some(type_name) = lsl_type_name(tokens[index].kind) {
                if let Some(name) = tokens.get(index + 1) {
                    if name.kind == TokenKind::Identifier {
                        self.add_variable(scope_id, name, type_name, index + 1);
                        index += 2;
                        continue;
                    }
                }
            }
            index += 1;
        }
    }

    fn register_variables(&mut self, tokens: &[Token]) {
        let mut paren_depth = 0usize;

        for (index, token) in tokens.iter().enumerate() {
            match token.kind {
                TokenKind::LParen => {
                    paren_depth += 1;
                    continue;
                }
                TokenKind::RParen => {
                    paren_depth = paren_depth.saturating_sub(1);
                    continue;
                }
                _ => {}
            }

            if paren_depth != 0 || !is_value_type(token.kind) {
                continue;
            }

            let Some(name) = tokens.get(index + 1) else {
                continue;
            };
            if name.kind != TokenKind::Identifier {
                continue;
            }

            // A type followed by name '(' is a function declaration, not a variable.
            if tokens.get(index + 2).map(|item| item.kind) == Some(TokenKind::LParen) {
                continue;
            }

            let Some(scope_id) = self.scope_at.get(index).copied() else {
                continue;
            };
            let type_name = lsl_type_name(token.kind).expect("value type has a name");
            self.register_declaration_names(tokens, index, scope_id, type_name);
        }
    }

    fn register_declaration_names(
        &mut self,
        tokens: &[Token],
        type_index: usize,
        scope_id: usize,
        type_name: &str,
    ) {
        let Some(first_name) = tokens.get(type_index + 1) else {
            return;
        };
        self.add_variable(scope_id, first_name, type_name, type_index + 1);

        let mut name_index = type_index + 1;
        let mut initializer_start = None;
        let mut index = type_index + 2;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut vector_depth = 0usize;

        while index < tokens.len() {
            let kind = tokens[index].kind;
            let at_declarator_level =
                paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && vector_depth == 0;

            if at_declarator_level && kind == TokenKind::Equal && initializer_start.is_none() {
                initializer_start = Some(index + 1);
            }

            if at_declarator_level && matches!(kind, TokenKind::Comma | TokenKind::Semicolon) {
                if let Some(expression_start) = initializer_start.take() {
                    if expression_start < index {
                        self.initializers.push(VariableInitializer {
                            name_index,
                            type_name: type_name.to_owned(),
                            expression_start,
                            expression_end: index,
                        });
                    }
                }

                if kind == TokenKind::Semicolon {
                    break;
                }

                if let Some(name) = tokens.get(index + 1) {
                    if name.kind == TokenKind::Identifier {
                        name_index = index + 1;
                        self.add_variable(scope_id, name, type_name, name_index);
                        index += 2;
                        continue;
                    }
                }
            }

            match kind {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
                TokenKind::Less if starts_vector_literal(tokens, index) => vector_depth += 1,
                TokenKind::Greater if vector_depth > 0 => vector_depth -= 1,
                _ => {}
            }

            index += 1;
        }
    }
}

fn matching_paren(tokens: &[Token], open_index: usize) -> Option<usize> {
    if tokens.get(open_index)?.kind != TokenKind::LParen {
        return None;
    }

    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open_index) {
        match token.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn matching_brace(tokens: &[Token], open_index: usize) -> Option<usize> {
    if tokens.get(open_index)?.kind != TokenKind::LBrace {
        return None;
    }

    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open_index) {
        match token.kind {
            TokenKind::LBrace => depth += 1,
            TokenKind::RBrace => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

#[derive(Debug, Clone)]
struct UserFunctionSignature {
    return_type: String,
    parameter_types: Vec<String>,
    declaration_index: usize,
}

fn collect_user_functions(tokens: &[Token]) -> HashMap<String, UserFunctionSignature> {
    let mut functions = HashMap::new();
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LBrace => {
                brace_depth += 1;
                continue;
            }
            TokenKind::RBrace => {
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }
            TokenKind::LParen => {
                paren_depth += 1;
                continue;
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if brace_depth != 0 || paren_depth != 0 {
            continue;
        }

        let Some((name_index, open, return_type)) = function_header_at(tokens, index) else {
            continue;
        };
        let Some(close) = matching_paren(tokens, open) else {
            continue;
        };
        if tokens.get(close + 1).map(|item| item.kind) != Some(TokenKind::LBrace) {
            continue;
        }

        let name = &tokens[name_index];
        functions
            .entry(name.lexeme.clone())
            .or_insert(UserFunctionSignature {
                return_type,
                parameter_types: parameter_types(tokens, open, close),
                declaration_index: name_index,
            });
    }

    functions
}

/// Perform conservative semantic checks that do not require a full symbol table.
///
/// The first rule catches duplicate top-level variables and user-defined
/// functions. Parameters and local declarations are excluded by tracking
/// braces and parentheses.
pub fn analyze(tokens: &[Token]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut globals: HashMap<&str, &Token> = HashMap::new();
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LBrace => {
                brace_depth += 1;
                continue;
            }
            TokenKind::RBrace => {
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }
            TokenKind::LParen => {
                paren_depth += 1;
                continue;
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if brace_depth != 0 || paren_depth != 0 {
            continue;
        }

        let name = if is_declaration_type(token.kind) {
            tokens.get(index + 1)
        } else if token.kind == TokenKind::Identifier
            && (index == 0 || !is_declaration_type(tokens[index - 1].kind))
            && tokens.get(index + 1).map(|item| item.kind) == Some(TokenKind::LParen)
        {
            Some(token)
        } else {
            None
        };
        let Some(name) = name else {
            continue;
        };
        if name.kind != TokenKind::Identifier {
            continue;
        }

        if let Some(previous) = globals.get(name.lexeme.as_str()) {
            diagnostics.push(Diagnostic {
                code: "SEM001",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "duplicate global declaration '{}'; first declared at {}:{}",
                    name.lexeme, previous.span.line, previous.span.column
                ),
                span: name.span,
            });
        } else {
            globals.insert(name.lexeme.as_str(), name);
        }
    }

    let mut scopes = ScopeModel::build(tokens);
    diagnostics.extend(std::mem::take(&mut scopes.diagnostics));
    let user_functions = collect_user_functions(tokens);
    analyze_explicit_casts(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_variable_initializers(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_builtin_calls(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_deprecated_function_calls(tokens, &user_functions, &mut diagnostics);
    analyze_user_function_calls(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_function_returns(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_jump_labels(tokens, &scopes, &mut diagnostics);
    analyze_missing_return_paths(tokens, &mut diagnostics);
    analyze_event_signatures(tokens, &mut diagnostics);
    analyze_unresolved_variables(tokens, &scopes, &mut diagnostics);
    diagnostics
}

fn unknown_event_body_ranges(tokens: &[Token], scopes: &ScopeModel) -> Vec<(usize, usize)> {
    let events = &definitions().events;
    let mut ranges = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        let body_open = match token.kind {
            TokenKind::KwDefault => index + 1,
            TokenKind::KwState => index + 2,
            _ => continue,
        };
        if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
            continue;
        }

        let Some(state_close) = matching_brace(tokens, body_open) else {
            continue;
        };
        let Some(state_scope_id) = scopes.scope_at.get(body_open + 1).copied() else {
            continue;
        };

        let mut event_index = body_open + 1;
        while event_index < state_close {
            let event_token = &tokens[event_index];
            if scopes.scope_at.get(event_index) != Some(&state_scope_id)
                || event_token.kind != TokenKind::Identifier
                || tokens.get(event_index + 1).map(|item| item.kind) != Some(TokenKind::LParen)
            {
                event_index += 1;
                continue;
            }

            let open = event_index + 1;
            let Some(parameter_close) = matching_paren(tokens, open) else {
                event_index += 1;
                continue;
            };
            let event_body_open = parameter_close + 1;
            if tokens.get(event_body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
                event_index += 1;
                continue;
            }
            let Some(event_body_close) = matching_brace(tokens, event_body_open) else {
                event_index += 1;
                continue;
            };

            if !events.contains_key(event_token.lexeme.as_str()) {
                ranges.push((event_body_open + 1, event_body_close));
            }
            event_index = event_body_close + 1;
        }
    }

    ranges
}

fn unknown_function_argument_ranges(tokens: &[Token]) -> Vec<(usize, usize)> {
    let builtins = &definitions().functions;
    let events = &definitions().events;
    let user_functions = collect_user_functions(tokens);
    let mut ranges = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Identifier
            || tokens.get(index + 1).map(|item| item.kind) != Some(TokenKind::LParen)
            || builtins.contains_key(token.lexeme.as_str())
            || events.contains_key(token.lexeme.as_str())
            || user_functions.contains_key(token.lexeme.as_str())
        {
            continue;
        }

        let Some(close) = matching_paren(tokens, index + 1) else {
            continue;
        };
        if close > index + 2 {
            ranges.push((index + 2, close));
        }
    }

    ranges
}

fn analyze_unresolved_variables(
    tokens: &[Token],
    scopes: &ScopeModel,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let unknown_event_ranges = unknown_event_body_ranges(tokens, scopes);
    let unknown_call_argument_ranges = unknown_function_argument_ranges(tokens);
    let constants = &definitions().constants;

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Identifier {
            continue;
        }

        if unknown_event_ranges
            .iter()
            .chain(unknown_call_argument_ranges.iter())
            .any(|(start, end)| (*start..*end).contains(&index))
        {
            continue;
        }

        if scopes.resolve(&token.lexeme, index).is_some()
            || constants.contains_key(token.lexeme.as_str())
            || tokens.get(index + 1).map(|item| item.kind) == Some(TokenKind::LParen)
        {
            continue;
        }

        let previous = index.checked_sub(1).and_then(|item| tokens.get(item));
        if previous.is_some_and(|item| {
            matches!(
                item.kind,
                TokenKind::At | TokenKind::KwJump | TokenKind::KwState | TokenKind::Dot
            ) || is_value_type(item.kind)
        }) {
            continue;
        }

        diagnostics.push(Diagnostic {
            code: "SEM012",
            severity: Severity::Warning,
            kind: DiagnosticKind::Semantic,
            message: format!(
                "variable '{}' is not declared in the current script; verify that it is provided by the scripting environment",
                token.lexeme
            ),
            span: token.span,
        });
    }
}

fn analyze_deprecated_function_calls(
    tokens: &[Token],
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let deprecated_functions = &definitions().deprecated_functions;

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Identifier
            || !deprecated_functions.contains(&token.lexeme)
            || user_functions.contains_key(&token.lexeme)
            || tokens.get(index + 1).map(|item| item.kind) != Some(TokenKind::LParen)
        {
            continue;
        }

        // A declaration that happens to reuse a deprecated builtin name is not a call.
        if index > 0 && is_declaration_type(tokens[index - 1].kind) {
            continue;
        }

        if matching_paren(tokens, index + 1).is_none() {
            continue;
        }

        diagnostics.push(Diagnostic {
            code: "SEM013",
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: format!("call to deprecated LSL function '{}'", token.lexeme),
            span: token.span,
        });
    }
}

fn analyze_builtin_calls(
    tokens: &[Token],
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let builtins = definitions();

    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Identifier {
            continue;
        }

        // A built-in name used as a declaration is not a call expression.
        if index > 0 && is_declaration_type(tokens[index - 1].kind) {
            continue;
        }

        let Some(signature) = builtins.functions.get(token.lexeme.as_str()) else {
            continue;
        };
        if tokens.get(index + 1).map(|next| next.kind) != Some(TokenKind::LParen) {
            continue;
        }

        let Some((actual_count, closing_index)) = count_call_arguments(tokens, index + 1) else {
            continue;
        };
        let expected_count = signature.parameters.len();

        if actual_count != expected_count {
            diagnostics.push(Diagnostic {
                code: "SEM002",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "{} expects {} argument(s), but received {}",
                    signature.name, expected_count, actual_count
                ),
                span: token.span,
            });
            continue;
        }

        let ranges = call_argument_ranges(tokens, index + 1, closing_index);
        for (argument_index, (range, parameter)) in
            ranges.iter().zip(signature.parameters.iter()).enumerate()
        {
            let Some(actual_type) =
                infer_expression_type(tokens, range.0, range.1, scopes, user_functions)
            else {
                // Expressions without a provable type remain unchecked.
                continue;
            };

            if !can_implicitly_convert(&actual_type, &parameter.type_name) {
                let span = tokens[range.0].span;
                diagnostics.push(Diagnostic {
                    code: "SEM004",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "{} argument {} has type {}, expected {}",
                        signature.name,
                        argument_index + 1,
                        actual_type,
                        parameter.type_name
                    ),
                    span,
                });
            }
        }
    }
}

fn analyze_user_function_calls(
    tokens: &[Token],
    scopes: &ScopeModel,
    functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Identifier {
            continue;
        }

        let Some(signature) = functions.get(token.lexeme.as_str()) else {
            continue;
        };
        if signature.declaration_index == index
            || tokens.get(index + 1).map(|next| next.kind) != Some(TokenKind::LParen)
        {
            continue;
        }

        let Some((actual_count, closing_index)) = count_call_arguments(tokens, index + 1) else {
            continue;
        };
        if actual_count != signature.parameter_types.len() {
            diagnostics.push(Diagnostic {
                code: "SEM005",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "{} expects {} argument(s), but received {}",
                    token.lexeme,
                    signature.parameter_types.len(),
                    actual_count
                ),
                span: token.span,
            });
            continue;
        }

        let ranges = call_argument_ranges(tokens, index + 1, closing_index);
        for (argument_index, (range, expected_type)) in ranges
            .iter()
            .zip(signature.parameter_types.iter())
            .enumerate()
        {
            let Some(actual_type) =
                infer_expression_type(tokens, range.0, range.1, scopes, functions)
            else {
                continue;
            };

            if !can_implicitly_convert(&actual_type, expected_type) {
                diagnostics.push(Diagnostic {
                    code: "SEM006",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "{} argument {} has type {}, expected {}",
                        token.lexeme,
                        argument_index + 1,
                        actual_type,
                        expected_type
                    ),
                    span: tokens[range.0].span,
                });
            }
        }
    }
}

fn analyze_function_returns(
    tokens: &[Token],
    scopes: &ScopeModel,
    functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LBrace => {
                brace_depth += 1;
                continue;
            }
            TokenKind::RBrace => {
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }
            TokenKind::LParen => {
                paren_depth += 1;
                continue;
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if brace_depth != 0 || paren_depth != 0 {
            continue;
        }

        let Some((name_index, open, _)) = function_header_at(tokens, index) else {
            continue;
        };
        let Some(parameter_close) = matching_paren(tokens, open) else {
            continue;
        };
        let body_open = parameter_close + 1;
        if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
            continue;
        }
        let Some(body_close) = matching_brace(tokens, body_open) else {
            continue;
        };

        let name = &tokens[name_index].lexeme;
        let Some(signature) = functions.get(name) else {
            continue;
        };

        let mut cursor = body_open + 1;
        while cursor < body_close {
            if tokens[cursor].kind != TokenKind::KwReturn {
                cursor += 1;
                continue;
            }

            let expression_start = cursor + 1;
            let mut semicolon = expression_start;
            while semicolon < body_close && tokens[semicolon].kind != TokenKind::Semicolon {
                semicolon += 1;
            }
            if semicolon == body_close {
                break;
            }

            let has_value = expression_start < semicolon;
            if signature.return_type == "void" {
                if has_value {
                    diagnostics.push(Diagnostic {
                        code: "SEM008",
                        severity: Severity::Error,
                        kind: DiagnosticKind::Semantic,
                        message: format!(
                            "function {} does not return a value, but this return statement has one",
                            name
                        ),
                        span: tokens[expression_start].span,
                    });
                }
            } else if !has_value {
                diagnostics.push(Diagnostic {
                    code: "SEM008",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "function {} returns {}, but this return statement has no value",
                        name, signature.return_type
                    ),
                    span: tokens[cursor].span,
                });
            } else if let Some(actual_type) =
                infer_expression_type(tokens, expression_start, semicolon, scopes, functions)
            {
                if !can_implicitly_convert(&actual_type, &signature.return_type) {
                    diagnostics.push(Diagnostic {
                        code: "SEM008",
                        severity: Severity::Error,
                        kind: DiagnosticKind::Semantic,
                        message: format!(
                            "function {} returns {}, but this return expression has type {}",
                            name, signature.return_type, actual_type
                        ),
                        span: tokens[expression_start].span,
                    });
                }
            }

            cursor = semicolon + 1;
        }
    }
}

struct CodeScopeRange {
    name: String,
    start: usize,
    end: usize,
}

struct JumpLabel {
    scope_id: usize,
    span: crate::diagnostic::Span,
}

fn collect_jump_code_scopes(tokens: &[Token], scopes: &ScopeModel) -> Vec<CodeScopeRange> {
    let mut code_scopes = Vec::new();
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;

    // User-defined function scopes.
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LBrace => {
                brace_depth += 1;
                continue;
            }
            TokenKind::RBrace => {
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }
            TokenKind::LParen => {
                paren_depth += 1;
                continue;
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if brace_depth != 0 || paren_depth != 0 {
            continue;
        }

        let Some((name_index, open, _)) = function_header_at(tokens, index) else {
            continue;
        };
        let Some(parameter_close) = matching_paren(tokens, open) else {
            continue;
        };
        let body_open = parameter_close + 1;
        if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
            continue;
        }
        let Some(body_close) = matching_brace(tokens, body_open) else {
            continue;
        };

        code_scopes.push(CodeScopeRange {
            name: tokens[name_index].lexeme.clone(),
            start: body_open + 1,
            end: body_close,
        });
    }

    // Event handler scopes are separate from their containing state scope.
    let events = &definitions().events;
    for (index, token) in tokens.iter().enumerate() {
        let body_open = match token.kind {
            TokenKind::KwDefault => index + 1,
            TokenKind::KwState => index + 2,
            _ => continue,
        };
        if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
            continue;
        }
        let Some(state_close) = matching_brace(tokens, body_open) else {
            continue;
        };
        let Some(state_scope_id) = scopes.scope_at.get(body_open + 1).copied() else {
            continue;
        };

        let mut event_index = body_open + 1;
        while event_index < state_close {
            let event_token = &tokens[event_index];
            if scopes.scope_at.get(event_index) != Some(&state_scope_id)
                || event_token.kind != TokenKind::Identifier
                || !events.contains_key(event_token.lexeme.as_str())
                || tokens.get(event_index + 1).map(|item| item.kind) != Some(TokenKind::LParen)
            {
                event_index += 1;
                continue;
            }

            let open = event_index + 1;
            let Some(parameter_close) = matching_paren(tokens, open) else {
                event_index += 1;
                continue;
            };
            let event_body_open = parameter_close + 1;
            if tokens.get(event_body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
                event_index += 1;
                continue;
            }
            let Some(event_body_close) = matching_brace(tokens, event_body_open) else {
                event_index += 1;
                continue;
            };

            code_scopes.push(CodeScopeRange {
                name: event_token.lexeme.clone(),
                start: event_body_open + 1,
                end: event_body_close,
            });
            event_index = event_body_close + 1;
        }
    }

    code_scopes
}

fn analyze_jump_labels(tokens: &[Token], scopes: &ScopeModel, diagnostics: &mut Vec<Diagnostic>) {
    for code_scope in collect_jump_code_scopes(tokens, scopes) {
        let mut labels: HashMap<String, JumpLabel> = HashMap::new();

        // Collect labels first so forward jumps are valid.
        for index in code_scope.start..code_scope.end {
            if tokens[index].kind != TokenKind::At
                || tokens.get(index + 1).map(|item| item.kind) != Some(TokenKind::Identifier)
                || tokens.get(index + 2).map(|item| item.kind) != Some(TokenKind::Semicolon)
            {
                continue;
            }

            let label = &tokens[index + 1];
            let scope_id = scopes.scope_at[index];
            if let Some(previous) = labels.get(&label.lexeme) {
                diagnostics.push(Diagnostic {
                    code: "SEM009",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "duplicate jump label '{}' in {}; first declared at {}:{}",
                        label.lexeme, code_scope.name, previous.span.line, previous.span.column
                    ),
                    span: label.span,
                });
            } else {
                labels.insert(
                    label.lexeme.clone(),
                    JumpLabel {
                        scope_id,
                        span: label.span,
                    },
                );
            }
        }

        // A target must exist in this function/event and be in the same or an
        // enclosing lexical block. Jumping into a child block is not allowed.
        for index in code_scope.start..code_scope.end {
            if tokens[index].kind != TokenKind::KwJump
                || tokens.get(index + 1).map(|item| item.kind) != Some(TokenKind::Identifier)
            {
                continue;
            }

            let target = &tokens[index + 1];
            let Some(label) = labels.get(&target.lexeme) else {
                diagnostics.push(Diagnostic {
                    code: "SEM010",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "jump target '{}' is not defined in {}",
                        target.lexeme, code_scope.name
                    ),
                    span: target.span,
                });
                continue;
            };

            let jump_scope_id = scopes.scope_at[index];
            if !scopes.is_ancestor(label.scope_id, jump_scope_id) {
                diagnostics.push(Diagnostic {
                    code: "SEM010",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "jump target '{}' is inside a nested block and is not accessible from this jump in {}",
                        target.lexeme, code_scope.name
                    ),
                    span: target.span,
                });
            }
        }
    }
}

#[derive(Default)]
struct ControlFlowGraph {
    successors: Vec<Vec<usize>>,
    labels: HashMap<String, usize>,
    pending_jumps: Vec<(usize, String)>,
}

impl ControlFlowGraph {
    fn add_node(&mut self) -> usize {
        let id = self.successors.len();
        self.successors.push(Vec::new());
        id
    }

    fn add_edge(&mut self, from: usize, to: usize) {
        if let Some(edges) = self.successors.get_mut(from) {
            if !edges.contains(&to) {
                edges.push(to);
            }
        }
    }

    fn can_reach(&self, start: usize, target: usize) -> bool {
        let mut visited = vec![false; self.successors.len()];
        let mut pending = vec![start];

        while let Some(node) = pending.pop() {
            if node == target {
                return true;
            }
            if node >= visited.len() || visited[node] {
                continue;
            }
            visited[node] = true;
            if let Some(edges) = self.successors.get(node) {
                pending.extend(edges.iter().copied());
            }
        }

        false
    }
}

struct FlowFragment {
    entry: usize,
    exits: Vec<usize>,
    next_index: usize,
}

fn condition_possibilities(
    tokens: &[Token],
    start: usize,
    end: usize,
    empty_is_true: bool,
) -> (bool, bool) {
    let mut left = start;
    let mut right = end;

    while left < right
        && tokens[left].kind == TokenKind::LParen
        && matching_paren(tokens, left) == Some(right - 1)
    {
        left += 1;
        right -= 1;
    }

    if left == right {
        return if empty_is_true {
            (true, false)
        } else {
            (true, true)
        };
    }

    if right == left + 1 {
        let token = &tokens[left];
        let constant_value = match token.kind {
            TokenKind::IntegerLiteral | TokenKind::FloatLiteral => token.lexeme.parse::<f64>().ok(),
            TokenKind::Identifier => definitions()
                .constants
                .get(token.lexeme.as_str())
                .and_then(|constant| constant.value.parse::<f64>().ok()),
            _ => None,
        };

        if let Some(value) = constant_value {
            return (value != 0.0, value == 0.0);
        }
    }

    (true, true)
}

fn for_condition_range(tokens: &[Token], open: usize, close: usize) -> Option<(usize, usize)> {
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut first_semicolon = None;

    for (index, token) in tokens.iter().enumerate().take(close).skip(open + 1) {
        match token.kind {
            TokenKind::LParen => paren_depth += 1,
            TokenKind::RParen => paren_depth = paren_depth.checked_sub(1)?,
            TokenKind::LBracket => bracket_depth += 1,
            TokenKind::RBracket => bracket_depth = bracket_depth.checked_sub(1)?,
            TokenKind::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                if let Some(first) = first_semicolon {
                    return Some((first + 1, index));
                }
                first_semicolon = Some(index);
            }
            _ => {}
        }
    }

    None
}

fn connect_exits(graph: &mut ControlFlowGraph, exits: &[usize], target: usize) {
    for exit in exits {
        graph.add_edge(*exit, target);
    }
}

/// Build a small control-flow graph directly from the token stream.
/// The parser remains syntax-only; this graph is local to one function body.
fn build_control_flow_statement(
    tokens: &[Token],
    start: usize,
    limit: usize,
    graph: &mut ControlFlowGraph,
) -> Option<FlowFragment> {
    let kind = tokens.get(start)?.kind;

    match kind {
        TokenKind::LBrace => {
            let close = matching_brace(tokens, start)?;
            if close >= limit {
                return None;
            }

            let mut cursor = start + 1;
            let mut entry = None;
            let mut exits = Vec::new();

            while cursor < close {
                let statement = build_control_flow_statement(tokens, cursor, close, graph)?;
                if statement.next_index <= cursor {
                    return None;
                }

                if entry.is_some() {
                    connect_exits(graph, &exits, statement.entry);
                } else {
                    entry = Some(statement.entry);
                }

                exits = statement.exits;
                cursor = statement.next_index;
            }

            let entry = entry.unwrap_or_else(|| {
                let empty = graph.add_node();
                exits.push(empty);
                empty
            });

            Some(FlowFragment {
                entry,
                exits,
                next_index: close + 1,
            })
        }
        TokenKind::KwIf => {
            let open = start + 1;
            if tokens.get(open)?.kind != TokenKind::LParen {
                return None;
            }
            let close = matching_paren(tokens, open)?;
            if close >= limit || close == open + 1 {
                return None;
            }

            let then_branch = build_control_flow_statement(tokens, close + 1, limit, graph)?;
            let condition = graph.add_node();
            let (may_be_true, may_be_false) =
                condition_possibilities(tokens, open + 1, close, false);

            let mut exits = Vec::new();
            if may_be_true {
                graph.add_edge(condition, then_branch.entry);
                exits.extend(then_branch.exits.iter().copied());
            }

            if tokens.get(then_branch.next_index).map(|item| item.kind) == Some(TokenKind::KwElse) {
                let else_branch =
                    build_control_flow_statement(tokens, then_branch.next_index + 1, limit, graph)?;
                if may_be_false {
                    graph.add_edge(condition, else_branch.entry);
                    exits.extend(else_branch.exits.iter().copied());
                }

                Some(FlowFragment {
                    entry: condition,
                    exits,
                    next_index: else_branch.next_index,
                })
            } else {
                if may_be_false {
                    exits.push(condition);
                }
                Some(FlowFragment {
                    entry: condition,
                    exits,
                    next_index: then_branch.next_index,
                })
            }
        }
        TokenKind::KwFor | TokenKind::KwWhile => {
            let open = start + 1;
            if tokens.get(open)?.kind != TokenKind::LParen {
                return None;
            }
            let close = matching_paren(tokens, open)?;
            if close >= limit {
                return None;
            }

            let (condition_start, condition_end, empty_is_true) = if kind == TokenKind::KwFor {
                let (condition_start, condition_end) = for_condition_range(tokens, open, close)?;
                (condition_start, condition_end, true)
            } else {
                if close == open + 1 {
                    return None;
                }
                (open + 1, close, false)
            };

            let body = build_control_flow_statement(tokens, close + 1, limit, graph)?;
            let condition = graph.add_node();
            let (may_enter_body, may_exit_loop) =
                condition_possibilities(tokens, condition_start, condition_end, empty_is_true);

            if may_enter_body {
                graph.add_edge(condition, body.entry);
            }
            connect_exits(graph, &body.exits, condition);

            Some(FlowFragment {
                entry: condition,
                exits: if may_exit_loop {
                    vec![condition]
                } else {
                    Vec::new()
                },
                next_index: body.next_index,
            })
        }
        TokenKind::KwDo => {
            let body = build_control_flow_statement(tokens, start + 1, limit, graph)?;
            let while_index = body.next_index;
            if tokens.get(while_index).map(|item| item.kind) != Some(TokenKind::KwWhile)
                || tokens.get(while_index + 1).map(|item| item.kind) != Some(TokenKind::LParen)
            {
                return None;
            }

            let open = while_index + 1;
            let close = matching_paren(tokens, open)?;
            if close >= limit
                || close == open + 1
                || tokens.get(close + 1).map(|item| item.kind) != Some(TokenKind::Semicolon)
            {
                return None;
            }

            let condition = graph.add_node();
            connect_exits(graph, &body.exits, condition);
            let (may_repeat, may_exit_loop) =
                condition_possibilities(tokens, open + 1, close, false);
            if may_repeat {
                graph.add_edge(condition, body.entry);
            }

            Some(FlowFragment {
                entry: body.entry,
                exits: if may_exit_loop {
                    vec![condition]
                } else {
                    Vec::new()
                },
                next_index: close + 2,
            })
        }
        TokenKind::KwReturn => {
            let semicolon = statement_semicolon(tokens, start, limit)?;
            let node = graph.add_node();
            Some(FlowFragment {
                entry: node,
                exits: Vec::new(),
                next_index: semicolon + 1,
            })
        }
        TokenKind::KwJump => {
            let target = tokens.get(start + 1)?;
            if target.kind != TokenKind::Identifier {
                return None;
            }
            let semicolon = statement_semicolon(tokens, start, limit)?;
            if tokens[semicolon].kind != TokenKind::Semicolon {
                return None;
            }

            let node = graph.add_node();
            graph.pending_jumps.push((node, target.lexeme.clone()));
            Some(FlowFragment {
                entry: node,
                exits: Vec::new(),
                next_index: semicolon + 1,
            })
        }
        TokenKind::At => {
            let label = tokens.get(start + 1)?;
            if label.kind != TokenKind::Identifier
                || tokens.get(start + 2).map(|item| item.kind) != Some(TokenKind::Semicolon)
            {
                return None;
            }

            let node = graph.add_node();
            graph.labels.entry(label.lexeme.clone()).or_insert(node);
            Some(FlowFragment {
                entry: node,
                exits: vec![node],
                next_index: start + 3,
            })
        }
        _ => {
            let semicolon = statement_semicolon(tokens, start, limit)?;
            if tokens[semicolon].kind != TokenKind::Semicolon {
                return None;
            }
            let node = graph.add_node();
            Some(FlowFragment {
                entry: node,
                exits: vec![node],
                next_index: semicolon + 1,
            })
        }
    }
}

fn statement_semicolon(tokens: &[Token], start: usize, limit: usize) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    for (index, token) in tokens.iter().enumerate().take(limit).skip(start) {
        match token.kind {
            TokenKind::LParen => paren_depth += 1,
            TokenKind::RParen => paren_depth = paren_depth.checked_sub(1)?,
            TokenKind::LBracket => bracket_depth += 1,
            TokenKind::RBracket => bracket_depth = bracket_depth.checked_sub(1)?,
            TokenKind::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                return Some(index);
            }
            TokenKind::RBrace if paren_depth == 0 && bracket_depth == 0 => return None,
            _ => {}
        }
    }

    None
}

fn analyze_missing_return_paths(tokens: &[Token], diagnostics: &mut Vec<Diagnostic>) {
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LBrace => {
                brace_depth += 1;
                continue;
            }
            TokenKind::RBrace => {
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }
            TokenKind::LParen => {
                paren_depth += 1;
                continue;
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if brace_depth != 0 || paren_depth != 0 {
            continue;
        }

        let Some((name_index, open, return_type)) = function_header_at(tokens, index) else {
            continue;
        };
        if return_type == "void" {
            continue;
        }

        let Some(parameter_close) = matching_paren(tokens, open) else {
            continue;
        };
        let body_open = parameter_close + 1;
        if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
            continue;
        }
        let Some(body_close) = matching_brace(tokens, body_open) else {
            continue;
        };

        let body_start = tokens[body_open].span.start;
        let body_end = tokens[body_close].span.end;
        let has_invalid_jump = diagnostics.iter().any(|diagnostic| {
            matches!(diagnostic.code, "SEM009" | "SEM010")
                && diagnostic.span.start >= body_start
                && diagnostic.span.start <= body_end
        });
        if has_invalid_jump {
            continue;
        }

        let mut graph = ControlFlowGraph::default();
        let Some(body) =
            build_control_flow_statement(tokens, body_open, body_close + 1, &mut graph)
        else {
            continue;
        };

        let exit = graph.add_node();
        connect_exits(&mut graph, &body.exits, exit);

        let mut invalid_graph = false;
        for (jump_node, target_name) in std::mem::take(&mut graph.pending_jumps) {
            let Some(target_node) = graph.labels.get(&target_name).copied() else {
                invalid_graph = true;
                break;
            };
            graph.add_edge(jump_node, target_node);
        }
        if invalid_graph {
            continue;
        }

        if graph.can_reach(body.entry, exit) {
            let name = &tokens[name_index];
            diagnostics.push(Diagnostic {
                code: "SEM011",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "function {} may reach the end without returning a value",
                    name.lexeme
                ),
                span: name.span,
            });
        }
    }
}

fn analyze_event_signatures(tokens: &[Token], diagnostics: &mut Vec<Diagnostic>) {
    let events = &definitions().events;
    let mut brace_depth = 0usize;
    let mut state_depth = None;
    let mut pending_state = false;

    for (index, token) in tokens.iter().enumerate() {
        if brace_depth == 0 && matches!(token.kind, TokenKind::KwDefault | TokenKind::KwState) {
            pending_state = true;
        }

        match token.kind {
            TokenKind::LBrace => {
                brace_depth += 1;
                if pending_state {
                    state_depth = Some(brace_depth);
                    pending_state = false;
                }
                continue;
            }
            TokenKind::RBrace => {
                if state_depth == Some(brace_depth) {
                    state_depth = None;
                }
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if state_depth != Some(brace_depth) || token.kind != TokenKind::Identifier {
            continue;
        }

        let Some(signature) = events.get(token.lexeme.as_str()) else {
            continue;
        };
        if tokens.get(index + 1).map(|next| next.kind) != Some(TokenKind::LParen) {
            continue;
        }

        let Some((actual_count, closing_index)) = count_call_arguments(tokens, index + 1) else {
            continue;
        };

        if actual_count != signature.parameters.len() {
            diagnostics.push(Diagnostic {
                code: "SEM003",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "event {} expects {} parameter(s), but {} were declared",
                    signature.name,
                    signature.parameters.len(),
                    actual_count
                ),
                span: token.span,
            });
            continue;
        }

        let actual_types = parameter_types(tokens, index + 1, closing_index);
        for (parameter_index, (actual, expected)) in actual_types
            .iter()
            .zip(signature.parameters.iter())
            .enumerate()
        {
            if actual != &expected.type_name {
                diagnostics.push(Diagnostic {
                    code: "SEM003",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "event {} parameter {} has type {}, expected {}",
                        signature.name,
                        parameter_index + 1,
                        actual,
                        expected.type_name
                    ),
                    span: token.span,
                });
            }
        }
    }
}

fn call_argument_ranges(
    tokens: &[Token],
    open_index: usize,
    closing_index: usize,
) -> Vec<(usize, usize)> {
    if closing_index == open_index + 1 {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut argument_start = open_index + 1;
    let mut paren_depth = 1usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut vector_depth = 0usize;

    for index in (open_index + 1)..closing_index {
        match tokens[index].kind {
            TokenKind::LParen => paren_depth += 1,
            TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
            TokenKind::LBracket => bracket_depth += 1,
            TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
            TokenKind::LBrace => brace_depth += 1,
            TokenKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
            TokenKind::Less if starts_vector_literal(tokens, index) => vector_depth += 1,
            TokenKind::Greater if vector_depth > 0 => vector_depth -= 1,
            TokenKind::Comma
                if paren_depth == 1
                    && bracket_depth == 0
                    && brace_depth == 0
                    && vector_depth == 0 =>
            {
                if argument_start < index {
                    ranges.push((argument_start, index));
                }
                argument_start = index + 1;
            }
            _ => {}
        }
    }

    if argument_start < closing_index {
        ranges.push((argument_start, closing_index));
    }

    ranges
}

fn expression_binary_precedence(kind: TokenKind) -> Option<u8> {
    Some(match kind {
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
    })
}

fn is_unary_operator_position(tokens: &[Token], index: usize, start: usize) -> bool {
    if index == start {
        return true;
    }

    let Some(previous) = index
        .checked_sub(1)
        .and_then(|position| tokens.get(position))
    else {
        return true;
    };

    matches!(
        previous.kind,
        TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::Comma
            | TokenKind::Equal
            | TokenKind::PlusEqual
            | TokenKind::MinusEqual
            | TokenKind::StarEqual
            | TokenKind::SlashEqual
            | TokenKind::PercentEqual
            | TokenKind::AndEqual
            | TokenKind::OrEqual
            | TokenKind::XorEqual
            | TokenKind::ShiftLeftEqual
            | TokenKind::ShiftRightEqual
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::EqualEqual
            | TokenKind::BangEqual
            | TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual
            | TokenKind::And
            | TokenKind::Or
            | TokenKind::Xor
            | TokenKind::AndAnd
            | TokenKind::OrOr
            | TokenKind::ShiftLeft
            | TokenKind::ShiftRight
    )
}

/// Find the rightmost operator at the lowest (weakest) precedence level.
/// Choosing the rightmost equal-precedence operator preserves LSL's
/// left-associative grouping.
fn find_top_level_binary_operator(tokens: &[Token], start: usize, end: usize) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut vector_depth = 0usize;
    let mut selected: Option<(u8, usize)> = None;

    for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
        match token.kind {
            TokenKind::Less if starts_vector_literal(tokens, index) => {
                vector_depth += 1;
                continue;
            }
            TokenKind::Greater if vector_depth > 0 => {
                vector_depth -= 1;
                continue;
            }
            _ => {}
        }

        if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && vector_depth == 0 {
            if let Some(precedence) = expression_binary_precedence(token.kind) {
                if !matches!(token.kind, TokenKind::Plus | TokenKind::Minus)
                    || !is_unary_operator_position(tokens, index, start)
                {
                    let replace = selected.map_or(true, |(current, _)| precedence <= current);
                    if replace {
                        selected = Some((precedence, index));
                    }
                }
            }
        }

        match token.kind {
            TokenKind::LParen => paren_depth += 1,
            TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
            TokenKind::LBracket => bracket_depth += 1,
            TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
            TokenKind::LBrace => brace_depth += 1,
            TokenKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }

    selected.map(|(_, index)| index)
}

fn numeric_result_type(left: &str, right: &str) -> Option<&'static str> {
    let left_numeric = matches!(left, "integer" | "float");
    let right_numeric = matches!(right, "integer" | "float");
    if !left_numeric || !right_numeric {
        return None;
    }

    if left == "float" || right == "float" {
        Some("float")
    } else {
        Some("integer")
    }
}

fn infer_binary_result_type(operator: TokenKind, left: &str, right: &str) -> Option<&'static str> {
    match operator {
        TokenKind::OrOr | TokenKind::AndAnd => {
            if left == "integer" && right == "integer" {
                Some("integer")
            } else {
                None
            }
        }

        TokenKind::EqualEqual | TokenKind::BangEqual => {
            if left == right
                || numeric_result_type(left, right).is_some()
                || can_implicitly_convert(left, right)
                || can_implicitly_convert(right, left)
            {
                Some("integer")
            } else {
                None
            }
        }

        TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual => {
            if numeric_result_type(left, right).is_some() {
                Some("integer")
            } else {
                None
            }
        }

        TokenKind::And
        | TokenKind::Or
        | TokenKind::Xor
        | TokenKind::ShiftLeft
        | TokenKind::ShiftRight => {
            if left == "integer" && right == "integer" {
                Some("integer")
            } else {
                None
            }
        }

        TokenKind::Plus => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "string" && right == "string" {
                Some("string")
            } else if left == "list" || right == "list" {
                Some("list")
            } else if left == "vector"
                && (right == "vector" || matches!(right, "integer" | "float"))
            {
                Some("vector")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else {
                None
            }
        }

        TokenKind::Minus => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "vector" && right == "vector" {
                Some("vector")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else {
                None
            }
        }

        TokenKind::Star => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "vector" && right == "vector" {
                Some("float")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else if left == "vector"
                && (matches!(right, "integer" | "float") || right == "rotation")
            {
                Some("vector")
            } else {
                None
            }
        }

        TokenKind::Slash => {
            if let Some(numeric) = numeric_result_type(left, right) {
                Some(numeric)
            } else if left == "vector"
                && (matches!(right, "integer" | "float") || right == "rotation")
            {
                Some("vector")
            } else if left == "rotation" && right == "rotation" {
                Some("rotation")
            } else {
                None
            }
        }

        TokenKind::Percent => {
            if left == "integer" && right == "integer" {
                Some("integer")
            } else if left == "vector" && right == "vector" {
                Some("vector")
            } else {
                None
            }
        }

        _ => None,
    }
}

fn is_explicit_cast_supported(source: &str, target: &str) -> bool {
    match source {
        "integer" => matches!(target, "integer" | "float" | "string" | "list"),
        "float" => matches!(target, "integer" | "float" | "string" | "list"),
        "string" => matches!(
            target,
            "integer" | "float" | "string" | "key" | "list" | "vector" | "rotation"
        ),
        "key" => matches!(target, "string" | "key" | "list"),
        "list" => matches!(target, "string" | "list"),
        "vector" => matches!(target, "string" | "list" | "vector"),
        "rotation" => matches!(target, "string" | "list" | "rotation"),
        _ => false,
    }
}

fn matching_bracket(tokens: &[Token], open_index: usize) -> Option<usize> {
    if tokens.get(open_index)?.kind != TokenKind::LBracket {
        return None;
    }

    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open_index) {
        match token.kind {
            TokenKind::LBracket => depth += 1,
            TokenKind::RBracket => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn matching_vector_literal(tokens: &[Token], open_index: usize) -> Option<usize> {
    if !starts_vector_literal(tokens, open_index) {
        return None;
    }

    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open_index) {
        match token.kind {
            TokenKind::Less if starts_vector_literal(tokens, index) => depth += 1,
            TokenKind::Greater if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

/// Return the token index immediately after a prefix expression, including its postfix calls,
/// member access, and increment/decrement operators.
fn prefix_expression_end(tokens: &[Token], start: usize) -> Option<usize> {
    let mut index = start;

    loop {
        match tokens.get(index)?.kind {
            TokenKind::Bang
            | TokenKind::Tilde
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::PlusPlus
            | TokenKind::MinusMinus => index += 1,
            TokenKind::LParen
                if lsl_type_name(tokens.get(index + 1)?.kind).is_some()
                    && tokens.get(index + 2)?.kind == TokenKind::RParen =>
            {
                index += 3;
            }
            _ => break,
        }
    }

    index = match tokens.get(index)?.kind {
        TokenKind::Identifier
        | TokenKind::IntegerLiteral
        | TokenKind::FloatLiteral
        | TokenKind::StringLiteral => index + 1,
        TokenKind::LParen => matching_paren(tokens, index)? + 1,
        TokenKind::LBracket => matching_bracket(tokens, index)? + 1,
        TokenKind::Less if starts_vector_literal(tokens, index) => {
            matching_vector_literal(tokens, index)? + 1
        }
        _ => return None,
    };

    loop {
        match tokens.get(index).map(|token| token.kind) {
            Some(TokenKind::LParen) => index = matching_paren(tokens, index)? + 1,
            Some(TokenKind::Dot) if tokens.get(index + 1)?.kind == TokenKind::Identifier => {
                index += 2;
            }
            Some(TokenKind::PlusPlus | TokenKind::MinusMinus) => index += 1,
            _ => break,
        }
    }

    Some(index)
}

fn analyze_variable_initializers(
    tokens: &[Token],
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for initializer in &scopes.initializers {
        let Some(actual_type) = infer_expression_type(
            tokens,
            initializer.expression_start,
            initializer.expression_end,
            scopes,
            user_functions,
        ) else {
            continue;
        };

        if !can_implicitly_convert(&actual_type, &initializer.type_name) {
            let name = &tokens[initializer.name_index];
            diagnostics.push(Diagnostic {
                code: "SEM015",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "initializer for variable '{}' has type {}, expected {}",
                    name.lexeme, actual_type, initializer.type_name
                ),
                span: name.span,
            });
        }
    }
}

fn analyze_explicit_casts(
    tokens: &[Token],
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::LParen {
            continue;
        }

        let Some(target_type) = tokens
            .get(index + 1)
            .and_then(|item| lsl_type_name(item.kind))
        else {
            continue;
        };
        if tokens.get(index + 2).map(|item| item.kind) != Some(TokenKind::RParen) {
            continue;
        }

        let operand_start = index + 3;
        let Some(operand_end) = prefix_expression_end(tokens, operand_start) else {
            continue;
        };
        if operand_start >= operand_end {
            continue;
        }

        let Some(source_type) =
            infer_expression_type(tokens, operand_start, operand_end, scopes, user_functions)
        else {
            continue;
        };

        if !is_explicit_cast_supported(&source_type, target_type) {
            diagnostics.push(Diagnostic {
                code: "SEM014",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "cannot explicitly cast {} to {} in LSL",
                    source_type, target_type
                ),
                span: tokens[index + 1].span,
            });
        }
    }
}

fn infer_expression_type(
    tokens: &[Token],
    mut start: usize,
    mut end: usize,
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
) -> Option<String> {
    if start >= end {
        return None;
    }

    // Strip grouping parentheses while preserving cast syntax such as
    // (string)expression.
    loop {
        if !has_outer_parentheses(tokens, start, end) {
            break;
        }
        start += 1;
        end -= 1;
    }

    if start >= end {
        return None;
    }

    if matches!(tokens[start].kind, TokenKind::Plus | TokenKind::Minus) {
        let operand_type = infer_expression_type(tokens, start + 1, end, scopes, user_functions)?;
        return matches!(operand_type.as_str(), "integer" | "float").then_some(operand_type);
    }
    if matches!(tokens[start].kind, TokenKind::Bang | TokenKind::Tilde) {
        let operand_type = infer_expression_type(tokens, start + 1, end, scopes, user_functions)?;
        return (operand_type == "integer").then(|| "integer".to_owned());
    }

    if let Some(operator_index) = find_top_level_binary_operator(tokens, start, end) {
        let left_type =
            infer_expression_type(tokens, start, operator_index, scopes, user_functions)?;
        let right_type =
            infer_expression_type(tokens, operator_index + 1, end, scopes, user_functions)?;
        return infer_binary_result_type(tokens[operator_index].kind, &left_type, &right_type)
            .map(str::to_owned);
    }

    // A cast applies to its following operand. Binary operators are split first
    // so expressions such as (string)value + "suffix" keep the outer operation.
    if start + 2 < end
        && tokens[start].kind == TokenKind::LParen
        && tokens[start + 2].kind == TokenKind::RParen
    {
        if let Some(target_type) = lsl_type_name(tokens[start + 1].kind) {
            let source_type = infer_expression_type(tokens, start + 3, end, scopes, user_functions);
            if let Some(source_type) = source_type {
                if !is_explicit_cast_supported(&source_type, target_type) {
                    return None;
                }
            } else if !matches!(target_type, "string" | "list") {
                // Casting any LSL value type to string or list is supported.
                // Other targets require a known source type.
                return None;
            }

            return Some(target_type.to_owned());
        }
    }

    if end == start + 3
        && tokens[start].kind == TokenKind::Identifier
        && tokens[start + 1].kind == TokenKind::Dot
        && tokens[start + 2].kind == TokenKind::Identifier
    {
        let base_type = scopes.resolve(&tokens[start].lexeme, start)?;
        let component = tokens[start + 2].lexeme.as_str();
        return match base_type.as_str() {
            "vector" if matches!(component, "x" | "y" | "z") => Some("float".to_owned()),
            "rotation" if matches!(component, "x" | "y" | "z" | "s") => Some("float".to_owned()),
            _ => None,
        };
    }

    if start + 1 == end {
        let token = &tokens[start];
        return match token.kind {
            TokenKind::IntegerLiteral => Some("integer".to_owned()),
            TokenKind::FloatLiteral => Some("float".to_owned()),
            TokenKind::StringLiteral => Some("string".to_owned()),
            TokenKind::Identifier => scopes.resolve(&token.lexeme, start).or_else(|| {
                definitions()
                    .constants
                    .get(token.lexeme.as_str())
                    .map(|constant| constant.type_name.clone())
            }),
            _ => None,
        };
    }

    if tokens[start].kind == TokenKind::LBracket && tokens[end - 1].kind == TokenKind::RBracket {
        return Some("list".to_owned());
    }

    if tokens[start].kind == TokenKind::Less && tokens[end - 1].kind == TokenKind::Greater {
        let mut commas = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut nested_vector_depth = 0usize;

        for index in (start + 1)..(end - 1) {
            match tokens[index].kind {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
                TokenKind::Less if starts_vector_literal(tokens, index) => {
                    nested_vector_depth += 1;
                }
                TokenKind::Greater if nested_vector_depth > 0 => {
                    nested_vector_depth -= 1;
                }
                TokenKind::Comma
                    if paren_depth == 0
                        && bracket_depth == 0
                        && brace_depth == 0
                        && nested_vector_depth == 0 =>
                {
                    commas += 1;
                }
                _ => {}
            }
        }

        return match commas {
            2 => Some("vector".to_owned()),
            3 => Some("rotation".to_owned()),
            _ => None,
        };
    }

    if tokens[start].kind == TokenKind::Identifier
        && tokens.get(start + 1).map(|token| token.kind) == Some(TokenKind::LParen)
    {
        if let Some((_, closing_index)) = count_call_arguments(tokens, start + 1) {
            if closing_index + 1 == end {
                if let Some(signature) = user_functions.get(tokens[start].lexeme.as_str()) {
                    return Some(signature.return_type.clone());
                }
                return definitions()
                    .functions
                    .get(tokens[start].lexeme.as_str())
                    .map(|signature| signature.return_type.clone());
            }
        }
    }

    None
}

fn has_outer_parentheses(tokens: &[Token], start: usize, end: usize) -> bool {
    if end <= start + 1
        || tokens[start].kind != TokenKind::LParen
        || tokens[end - 1].kind != TokenKind::RParen
    {
        return false;
    }

    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
        match token.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index == end - 1;
                }
            }
            _ => {}
        }
    }

    false
}

fn can_implicitly_convert(actual: &str, expected: &str) -> bool {
    actual == expected
        || (actual == "integer" && expected == "float")
        || (actual == "string" && expected == "key")
}

fn parameter_types(tokens: &[Token], open_index: usize, closing_index: usize) -> Vec<String> {
    let mut types = Vec::new();
    let mut expect_type = true;

    for token in tokens.iter().take(closing_index).skip(open_index + 1) {
        if token.kind == TokenKind::Comma {
            expect_type = true;
            continue;
        }

        if expect_type {
            if let Some(type_name) = lsl_type_name(token.kind) {
                types.push(type_name.to_owned());
                expect_type = false;
            }
        }
    }

    types
}

fn lsl_type_name(kind: TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::KwInteger => Some("integer"),
        TokenKind::KwFloat => Some("float"),
        TokenKind::KwString => Some("string"),
        TokenKind::KwKey => Some("key"),
        TokenKind::KwList => Some("list"),
        TokenKind::KwVector => Some("vector"),
        TokenKind::KwRotation => Some("rotation"),
        _ => None,
    }
}

fn count_call_arguments(tokens: &[Token], open_index: usize) -> Option<(usize, usize)> {
    let mut paren_depth = 1usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut vector_depth = 0usize;
    let mut commas = 0usize;
    let mut has_argument = false;

    for (index, token) in tokens.iter().enumerate().skip(open_index + 1) {
        match token.kind {
            TokenKind::LParen => {
                paren_depth += 1;
                has_argument = true;
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.checked_sub(1)?;
                if paren_depth == 0 {
                    let count = if has_argument { commas + 1 } else { 0 };
                    return Some((count, index));
                }
            }
            TokenKind::LBracket => {
                bracket_depth += 1;
                has_argument = true;
            }
            TokenKind::RBracket => {
                bracket_depth = bracket_depth.saturating_sub(1);
            }
            TokenKind::LBrace => {
                brace_depth += 1;
                has_argument = true;
            }
            TokenKind::RBrace => {
                brace_depth = brace_depth.saturating_sub(1);
            }
            TokenKind::Less if starts_vector_literal(tokens, index) => {
                vector_depth += 1;
                has_argument = true;
            }
            TokenKind::Greater if vector_depth > 0 => {
                vector_depth -= 1;
            }
            TokenKind::Comma
                if paren_depth == 1
                    && bracket_depth == 0
                    && brace_depth == 0
                    && vector_depth == 0 =>
            {
                commas += 1;
                has_argument = true;
            }
            _ => {
                has_argument = true;
            }
        }
    }

    None
}

fn starts_vector_literal(tokens: &[Token], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return true;
    };

    matches!(
        previous.kind,
        TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBracket
            | TokenKind::Comma
            | TokenKind::Equal
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
            | TokenKind::OrOr
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex;

    fn analyze_source(source: &str) -> Vec<Diagnostic> {
        let (tokens, lexical_diagnostics) = lex(source);
        assert!(lexical_diagnostics.is_empty(), "{lexical_diagnostics:?}");
        analyze(&tokens)
    }

    #[test]
    fn reports_duplicate_global_variables() {
        let diagnostics = analyze_source("integer count; float count;");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "SEM001");
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Semantic);
        assert_eq!(diagnostics[0].span.line, 1);
    }

    #[test]
    fn reports_duplicate_user_functions() {
        let diagnostics =
            analyze_source("integer calculate() { return 1; } integer calculate() { return 2; }");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "SEM001");
    }

    #[test]
    fn ignores_repeated_parameter_and_local_names() {
        let diagnostics = analyze_source(
            "integer first(integer value) { integer result; return value; } \
             integer second(integer value) { integer result; return value; }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn warns_when_a_canonical_deprecated_lsl_function_is_called() {
        let diagnostics =
            analyze_source(r#"default { state_entry() { llSoundPreload("sound"); } }"#);
        let diagnostic = diagnostics
            .iter()
            .find(|item| item.code == "SEM013")
            .expect("deprecated builtin call should be reported");
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.kind, DiagnosticKind::Lint);
        assert!(diagnostic.message.contains("llSoundPreload"));
        assert!(!diagnostic.message.contains("ll.PreloadSound"));
    }

    #[test]
    fn does_not_warn_for_a_non_deprecated_builtin_function() {
        let diagnostics = analyze_source(r#"default { state_entry() { llOwnerSay("hello"); } }"#);
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM013"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn does_not_warn_when_deprecated_function_name_is_only_text() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llOwnerSay("llSoundPreload()"); // llSoundPreload("old");
            } }"#,
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM013"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_builtin_function_arity_mismatches() {
        let diagnostics =
            analyze_source(r#"default { state_entry() { llOwnerSay(); llGetPos(1); } }"#);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM002")
                .count(),
            2
        );
    }

    #[test]
    fn accepts_correct_builtin_arity_and_vector_arguments() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llOwnerSay("hello"); llSetPos(<1, 2, 3>); llGetPos(); } }"#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn does_not_count_nested_call_or_vector_commas_as_arguments() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llSetPos(<llFabs(-1), 2, 3>); llOwnerSay(llDumpList2String([1, 2], ",")); } }"#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn infers_numeric_binary_and_comparison_expression_types() {
        let diagnostics = analyze_source(
            "default { state_entry() { llOwnerSay(1 + 2); llOwnerSay(1 > 2); llSleep(1 + 2.0); } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_vector_plus_scalar_only_in_documented_operand_order() {
        let diagnostics = analyze_source(
            "default { state_entry() { vector position; llSetPos(position + 1.0); llOwnerSay(position + 1); llOwnerSay(1.0 + position); } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM004")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(
            mismatches[0]
                .message
                .contains("has type vector, expected string"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_string_list_and_vector_operator_result_types() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llOwnerSay("one" + "two"); llOwnerSay([1] + [2]); llOwnerSay(<1, 2, 3> * <4, 5, 6>); } }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_directional_vector_and_rotation_operator_types() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llOwnerSay(<1, 2, 3> * llGetRot()); llOwnerSay(<1, 2, 3> / llGetRot()); llOwnerSay(<1, 2, 3> * 2.0); llOwnerSay(llGetRot() + llGetRot()); llOwnerSay(llGetRot() * <1, 2, 3>); llOwnerSay(2.0 * <1, 2, 3>); llOwnerSay(<1, 2, 3> + 1.0); llOwnerSay(1.0 + <1, 2, 3>); } }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            4,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_lsl_subtraction_before_addition_precedence() {
        let diagnostics = analyze_source(
            "default { state_entry() { list values; vector start; vector delta; llOwnerSay(values + start - delta); } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM004")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(
            mismatches[0]
                .message
                .contains("argument 1 has type list, expected string"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_vector_and_rotation_component_access_as_float() {
        let diagnostics = analyze_source(
            "default { state_entry() { vector position; rotation orientation; llSleep(position.x); llSleep(orientation.s); llOwnerSay(position.z); llOwnerSay(orientation.y); } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM004")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 2, "{diagnostics:?}");
        assert!(
            mismatches
                .iter()
                .all(|item| item.message.contains("has type float, expected string")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_rotation_subtraction_result_type() {
        let diagnostics = analyze_source(
            "default { state_entry() { llSetRot(llGetRot() - llGetRot()); llOwnerSay(llGetRot() - llGetRot()); } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM004")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(
            mismatches[0].message.contains("expected string"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn does_not_infer_invalid_unary_operand_types() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llOwnerSay(-"text"); llOwnerSay(!"text"); llOwnerSay(-1); llOwnerSay(!TRUE); } }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn preserves_unary_operators_inside_binary_expressions() {
        let diagnostics =
            analyze_source("default { state_entry() { llOwnerSay(1 + -2); llSleep(1 * -2.0); } }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_obvious_builtin_argument_type_mismatches() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llOwnerSay(123); llSetPos("not a vector"); } }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            2
        );
    }

    #[test]
    fn allows_the_two_lsl_implicit_conversions() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llSleep(1); llGetObjectDetails("01234567-89ab-cdef-0123-456789abcdef", [OBJECT_NAME]); } }"#,
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_explicit_casts_and_known_builtin_return_types() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { llOwnerSay((string)123); llOwnerSay(llGetRegionName()); } }"#,
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn explicit_cast_matrix_matches_lsl_supported_pairs() {
        let valid = [
            ("integer", "integer"),
            ("integer", "float"),
            ("integer", "string"),
            ("integer", "list"),
            ("float", "integer"),
            ("float", "float"),
            ("float", "string"),
            ("float", "list"),
            ("string", "integer"),
            ("string", "float"),
            ("string", "string"),
            ("string", "key"),
            ("string", "list"),
            ("string", "vector"),
            ("string", "rotation"),
            ("key", "string"),
            ("key", "key"),
            ("key", "list"),
            ("list", "string"),
            ("list", "list"),
            ("vector", "string"),
            ("vector", "list"),
            ("vector", "vector"),
            ("rotation", "string"),
            ("rotation", "list"),
            ("rotation", "rotation"),
        ];
        for (source, target) in valid {
            assert!(
                is_explicit_cast_supported(source, target),
                "{source} -> {target} should be supported"
            );
        }

        let invalid = [
            ("integer", "key"),
            ("integer", "vector"),
            ("integer", "rotation"),
            ("float", "key"),
            ("float", "vector"),
            ("float", "rotation"),
            ("key", "integer"),
            ("key", "float"),
            ("key", "vector"),
            ("key", "rotation"),
            ("list", "integer"),
            ("list", "float"),
            ("list", "key"),
            ("list", "vector"),
            ("list", "rotation"),
            ("vector", "integer"),
            ("vector", "float"),
            ("vector", "key"),
            ("vector", "rotation"),
            ("rotation", "integer"),
            ("rotation", "float"),
            ("rotation", "key"),
            ("rotation", "vector"),
        ];
        for (source, target) in invalid {
            assert!(
                !is_explicit_cast_supported(source, target),
                "{source} -> {target} should be unsupported"
            );
        }
    }

    #[test]
    fn reports_explicit_casts_not_supported_by_lsl() {
        let diagnostics = analyze_source(
            "default { state_entry() { vector position; llSleep((integer)position); llSetPos((vector)\"<1, 2, 3>\"); llOwnerSay((string)position); } }",
        );
        let invalid_casts = diagnostics
            .iter()
            .filter(|item| item.code == "SEM014")
            .collect::<Vec<_>>();
        assert_eq!(invalid_casts.len(), 1, "{diagnostics:?}");
        assert!(
            invalid_casts[0]
                .message
                .contains("cannot explicitly cast vector to integer"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn detects_unsupported_casts_in_local_initializers() {
        let diagnostics = analyze_source(
            "default { state_entry() { vector position; integer invalid = (integer)position; } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM014")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn scans_casts_in_parenthesized_list_and_nested_expressions() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    vector position;
                    integer local_value = (integer)(position + position);
                    list values = [(integer)position, (string)llGetOwner()];
                    llOwnerSay((integer)(vector)position);
                }
            }"#,
        );
        let invalid_casts = diagnostics
            .iter()
            .filter(|item| item.code == "SEM014")
            .collect::<Vec<_>>();
        assert_eq!(invalid_casts.len(), 3, "{diagnostics:?}");
        assert!(
            invalid_casts.iter().all(|item| item
                .message
                .contains("cannot explicitly cast vector to integer")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn keeps_unknown_cast_source_types_conservative() {
        let diagnostics = analyze_source(
            "default { state_entry() { llOwnerSay((integer)llExtensionValue()); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM014"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn validates_global_and_local_variable_initializers() {
        let diagnostics = analyze_source(
            r#"integer global_integer = 1.5;
               float global_float = 1;
               key global_key = "00000000-0000-0000-0000-000000000000";
               default {
                   state_entry() {
                       integer local_integer = 2.5;
                       string local_string = 2;
                       vector position = <1, 2, 3>;
                       list values = [1, "one"];
                   }
               }"#,
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM015")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 3, "{diagnostics:?}");
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("global_integer")
                    && item.message.contains("float, expected integer")
            }),
            "{diagnostics:?}"
        );
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("local_integer")
                    && item.message.contains("float, expected integer")
            }),
            "{diagnostics:?}"
        );
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("local_string")
                    && item.message.contains("integer, expected string")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn validates_each_comma_separated_variable_initializer() {
        let diagnostics = analyze_source(
            "default { state_entry() { integer first = 1, second = 2.0, third = 3; } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM015")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(mismatches[0].message.contains("second"), "{diagnostics:?}");
    }

    #[test]
    fn leaves_unknown_variable_initializer_types_conservative() {
        let diagnostics =
            analyze_source("default { state_entry() { integer value = llExtensionValue(); } }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM015"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn resolves_local_and_global_variable_types() {
        let diagnostics = analyze_source(
            "integer globalAmount; default { state_entry() { integer localAmount; llOwnerSay(globalAmount); llOwnerSay(localAmount); llSleep(localAmount); } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn resolves_function_and_event_parameter_types() {
        let diagnostics = analyze_source(
            "integer send(integer amount) { llOwnerSay(amount); return amount; } default { touch_start(integer total) { llOwnerSay(total); } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn inner_scope_shadowing_uses_the_nearest_variable() {
        let diagnostics = analyze_source(
            "string value; default { state_entry() { integer value; llOwnerSay(value); } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn inner_block_variables_do_not_leak_into_parent_scope() {
        let diagnostics =
            analyze_source("default { state_entry() { { integer hidden; } llOwnerSay(hidden); } }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn unresolved_variable_types_remain_conservative() {
        let diagnostics =
            analyze_source(r#"default { state_entry() { llOwnerSay(notDeclared); } }"#);
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_duplicate_local_variables_in_the_same_scope() {
        let diagnostics =
            analyze_source("default { state_entry() { integer count; integer count; } }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM007")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_local_variable_redeclaration_of_a_parameter() {
        let diagnostics =
            analyze_source("integer process(integer value) { integer value; return value; }");
        assert!(diagnostics
            .iter()
            .any(|item| item.code == "SEM007" && item.message.contains("value")));
    }

    #[test]
    fn allows_shadowing_in_a_nested_block() {
        let diagnostics = analyze_source(
            r#"default { state_entry() { integer value; { string value; llOwnerSay(value); } } }"#,
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM007"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_user_function_arity_mismatches() {
        let diagnostics = analyze_source(
            "integer add(integer left, integer right) { return left + right; } default { state_entry() { add(1); } }",
        );
        assert!(diagnostics
            .iter()
            .any(|item| item.code == "SEM005" && item.message.contains("expects 2")));
    }

    #[test]
    fn reports_user_function_argument_type_mismatches() {
        let diagnostics = analyze_source(
            r#"string echo(string message) { return message; } default { state_entry() { echo(42); } }"#,
        );
        assert!(diagnostics
            .iter()
            .any(|item| item.code == "SEM006" && item.message.contains("expected string")));
    }

    #[test]
    fn infers_user_function_return_types_for_builtin_calls() {
        let diagnostics = analyze_source(
            r#"integer makeCount() { return 1; } default { state_entry() { llOwnerSay(makeCount()); } }"#,
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_valid_user_function_calls_and_return_types() {
        let diagnostics = analyze_source(
            r#"string getName() { return "Ada"; } integer addOne(integer value) { return value + 1; } default { state_entry() { llOwnerSay(getName()); addOne(2); } }"#,
        );
        assert!(
            !diagnostics
                .iter()
                .any(|item| matches!(item.code, "SEM004" | "SEM005" | "SEM006")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn analyzes_untyped_user_function_signatures() {
        let diagnostics = analyze_source(
            r#"say_message(string message) { llOwnerSay(message); } default { state_entry() { say_message(42); say_message(); } }"#,
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM005"),
            "{diagnostics:?}"
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM006"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn detects_duplicate_untyped_user_functions() {
        let diagnostics =
            analyze_source("say_message() { } say_message() { } default { state_entry() { } }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM001"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_missing_value_from_typed_function_return() {
        let diagnostics =
            analyze_source("integer get_value() { return; } default { state_entry() { } }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_wrong_typed_function_return_expression() {
        let diagnostics = analyze_source(r#"integer get_value() { return "wrong"; } default { }"#);
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_value_returned_from_no_return_function() {
        let diagnostics = analyze_source("do_work() { return 1; } default { state_entry() { } }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_matching_and_bare_function_returns() {
        let diagnostics = analyze_source(
            r#"integer get_count() { return 1; } float get_float() { return 2; } do_work() { return; } default { state_entry() { } }"#,
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn checks_return_expression_types_using_local_variables() {
        let diagnostics = analyze_source(
            "integer get_count() { string name; return name; } default { state_entry() { } }",
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_jump_to_label_in_same_scope() {
        let diagnostics = analyze_source(
            "integer get_value() { jump done; @done; return 1; } default { state_entry() { } }",
        );
        assert!(
            !diagnostics
                .iter()
                .any(|item| matches!(item.code, "SEM009" | "SEM010")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_jump_from_nested_scope_to_outer_label() {
        let diagnostics = analyze_source(
            "integer get_value() { @done; { jump done; } return 1; } default { state_entry() { } }",
        );
        assert!(
            !diagnostics
                .iter()
                .any(|item| matches!(item.code, "SEM009" | "SEM010")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_undefined_jump_targets() {
        let diagnostics =
            analyze_source("integer get_value() { jump missing; return 1; } default { }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM010"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_jumps_into_nested_blocks() {
        let diagnostics = analyze_source(
            "integer get_value() { jump inside; { @inside; } return 1; } default { }",
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM010"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_duplicate_labels_without_cascading_missing_return() {
        let diagnostics = analyze_source("integer get_value() { @same; { @same; } } default { }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM009"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn allows_same_jump_label_name_in_separate_functions() {
        let diagnostics = analyze_source(
            "integer first() { @done; return 1; } integer second() { @done; return 2; } default { }",
        );
        assert!(
            !diagnostics
                .iter()
                .any(|item| matches!(item.code, "SEM009" | "SEM010")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_jump_to_label_inside_event_handler() {
        let diagnostics = analyze_source("default { state_entry() { jump done; @done; } }");
        assert!(
            !diagnostics
                .iter()
                .any(|item| matches!(item.code, "SEM009" | "SEM010")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_undefined_jump_target_inside_event_handler() {
        let diagnostics = analyze_source("default { state_entry() { jump missing; } }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM010"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn allows_same_jump_label_name_in_separate_event_handlers() {
        let diagnostics =
            analyze_source("default { state_entry() { @done; } touch_end() { @done; } }");
        assert!(
            !diagnostics
                .iter()
                .any(|item| matches!(item.code, "SEM009" | "SEM010")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_value_function_without_a_return_statement() {
        let diagnostics = analyze_source(
            "integer get_value() { llOwnerSay(\"missing\"); } default { state_entry() { } }",
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_if_branch_that_can_fall_through() {
        let diagnostics =
            analyze_source("integer choose(integer flag) { if (flag) return 1; } default { }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_if_else_when_both_branches_return() {
        let diagnostics = analyze_source(
            "integer choose(integer flag) { if (flag) return 1; else return 2; } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_guard_returns_followed_by_a_fallback_return() {
        let diagnostics = analyze_source(
            "integer choose(integer flag) { if (flag == 1) return 1; if (flag == 2) return 2; return 0; } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn does_not_assume_a_while_loop_returns() {
        let diagnostics = analyze_source(
            "integer choose(integer flag) { while (flag) { return 1; } } default { }",
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_do_while_with_an_unconditional_returning_body() {
        let diagnostics =
            analyze_source("integer choose() { do { return 1; } while (1); } default { }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_missing_return_reachable_through_jump_label() {
        let diagnostics = analyze_source(
            "integer choose(integer flag) { if (flag) jump done; return 1; @done; } default { }",
        );
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_jump_to_fallback_return() {
        let diagnostics = analyze_source(
            "integer choose(integer flag) { if (flag) jump done; return 1; @done; return 2; } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_nested_jump_to_enclosing_label_before_fallback_return() {
        let diagnostics = analyze_source(
            "integer get_value() { { jump done; } return 1; @done; return 2; } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_missing_return_reached_from_nested_jump() {
        let diagnostics =
            analyze_source("integer get_value() { { jump done; } @done; } default { }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_backward_jump_that_cannot_reach_function_end() {
        let diagnostics = analyze_source("integer spin() { @again; jump again; } default { }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn recognizes_unconditional_loop_conditions() {
        let diagnostics = analyze_source(
            "integer spin() { while (TRUE) { } } integer spin_forever() { for (;;) { } } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn recognizes_constant_false_loop_conditions() {
        let diagnostics =
            analyze_source("integer choose() { while (FALSE) return 1; } default { }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn ignores_missing_return_cascade_when_jump_target_is_invalid() {
        let diagnostics =
            analyze_source("integer choose() { jump missing; return 1; } default { }");
        assert!(diagnostics.iter().any(|item| item.code == "SEM010"));
        assert!(!diagnostics.iter().any(|item| item.code == "SEM011"));
    }

    #[test]
    fn does_not_require_value_returns_from_no_return_functions() {
        let diagnostics = analyze_source("do_work() { llOwnerSay(\"done\"); } default { }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn warns_about_unresolved_variable_references() {
        let diagnostics = analyze_source("default { state_entry() { llOwnerSay(missing_name); } }");
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "SEM012" && item.severity == Severity::Warning),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_declared_variables_constants_and_vector_components() {
        let diagnostics = analyze_source(
            "vector position; default { state_entry() { integer amount; llOwnerSay((string)amount); llSetPos(position); llOwnerSay((string)position.x); llOwnerSay((string)TRUE); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM012"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn ignores_unknown_extension_function_names_and_arguments() {
        let diagnostics = analyze_source(
            "default { state_entry() { llExtensionSpecificCall(extension_value); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM012"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn ignores_bodies_of_unknown_extension_event_handlers() {
        let diagnostics = analyze_source(
            "default { custom_extension_event(integer supplied) { llOwnerSay(supplied); llOwnerSay(extension_value); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM012"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn ignores_state_names_jump_targets_and_label_names() {
        let diagnostics = analyze_source(
            "default { state_entry() { jump done; @done; state running; } } state running { state_entry() { } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM012"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_function_parameters_and_nested_local_variables() {
        let diagnostics = analyze_source(
            "integer add(integer left, integer right) { return left + right; } default { state_entry() { { string nested; llOwnerSay(nested); } llOwnerSay((string)add(1, 2)); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM012"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn warns_when_local_variable_is_used_before_its_declaration() {
        let diagnostics =
            analyze_source("default { state_entry() { llOwnerSay(later); string later; } }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM012"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_wrong_event_parameter_count() {
        let diagnostics = analyze_source("default { state_entry() { } touch_start() { } }");
        assert!(diagnostics
            .iter()
            .any(|item| item.code == "SEM003" && item.message.contains("touch_start")));
    }

    #[test]
    fn reports_wrong_event_parameter_type() {
        let diagnostics = analyze_source("default { touch_start(string total) { } }");
        assert!(diagnostics
            .iter()
            .any(|item| item.code == "SEM003" && item.message.contains("expected integer")));
    }

    #[test]
    fn accepts_known_event_signatures() {
        let diagnostics = analyze_source(
            "default { state_entry() { } touch_start(integer total) { } listen(integer channel, string name, key id, string message) { } }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn ignores_type_tokens_inside_global_function_parameters() {
        let diagnostics = analyze_source(
            "integer first(integer value) { return value; } float second(float amount) { return amount; }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
