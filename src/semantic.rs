use crate::builtins::definitions;
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
use crate::lexer::{Token, TokenKind};
use std::collections::{HashMap, HashSet};

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
    analyze_variable_assignments(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_compound_assignments(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_component_assignments(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_component_reads(tokens, &scopes, &mut diagnostics);
    analyze_builtin_calls(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_deprecated_function_calls(tokens, &user_functions, &mut diagnostics);
    analyze_user_function_calls(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_function_returns(tokens, &scopes, &user_functions, &mut diagnostics);
    analyze_jump_labels(tokens, &scopes, &mut diagnostics);
    analyze_missing_return_paths(tokens, &scopes, &mut diagnostics);
    analyze_event_signatures(tokens, &mut diagnostics);
    analyze_state_topology(tokens, &scopes, &user_functions, &mut diagnostics);
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

const MAX_CFG_TOKENS_PER_FUNCTION: usize = 16_384;
const MAX_CFG_NODES: usize = 4_096;
const MAX_CFG_EDGES: usize = 8_192;
const MAX_CFG_DEPTH: usize = 128;

#[derive(Default)]
struct ControlFlowGraph {
    successors: Vec<Vec<usize>>,
    labels: HashMap<String, usize>,
    pending_jumps: Vec<(usize, String)>,
    edge_count: usize,
    limit_exceeded: bool,
}

impl ControlFlowGraph {
    fn add_node(&mut self) -> Option<usize> {
        if self.successors.len() >= MAX_CFG_NODES {
            self.limit_exceeded = true;
            return None;
        }
        let id = self.successors.len();
        self.successors.push(Vec::new());
        Some(id)
    }

    fn add_edge(&mut self, from: usize, to: usize) -> bool {
        let Some(edges) = self.successors.get_mut(from) else {
            return false;
        };
        if edges.contains(&to) {
            return true;
        }
        if self.edge_count >= MAX_CFG_EDGES {
            self.limit_exceeded = true;
            return false;
        }
        let Some(next_count) = self.edge_count.checked_add(1) else {
            self.limit_exceeded = true;
            return false;
        };
        self.edge_count = next_count;
        edges.push(to);
        true
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

fn parse_hex_integer_digits(digits: &str) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }

    let mut bits = 0u32;
    for ch in digits.chars() {
        bits = bits.checked_mul(16)?.checked_add(ch.to_digit(16)?)?;
    }
    Some(bits)
}

fn parse_constant_integer_value(value: &str) -> Option<i32> {
    let (negative, magnitude) = match value.strip_prefix('-') {
        Some(magnitude) => (true, magnitude),
        None => (false, value),
    };
    let parsed = if let Some(hex) = magnitude
        .strip_prefix("0x")
        .or_else(|| magnitude.strip_prefix("0X"))
    {
        parse_hex_integer_digits(hex)? as i32
    } else {
        magnitude.parse::<u32>().ok()? as i32
    };
    if negative {
        Some(parsed.wrapping_neg())
    } else {
        Some(parsed)
    }
}

fn parse_integer_literal_value(value: &str) -> Option<i32> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return parse_hex_integer_digits(hex).map(|bits| bits as i32);
    }
    Some(value.parse::<u64>().ok()? as u32 as i32)
}

fn canonical_integer_constant(name: &str) -> Option<i32> {
    let constant = definitions().constants.get(name)?;
    if constant.type_name != "integer" {
        return None;
    }
    parse_constant_integer_value(&constant.value)
}

/// Evaluate a condition expression to a provable 32-bit integer constant.
/// Unknown identifiers, calls, unsupported syntax, and division/remainder by
/// zero return None so SEM011 keeps both control-flow outcomes.
const MAX_CONSTANT_EXPRESSION_TOKENS: usize = 128;
const MAX_CONSTANT_EXPRESSION_DEPTH: usize = 64;

fn evaluate_constant_integer_expression(
    tokens: &[Token],
    start: usize,
    end: usize,
    scopes: &ScopeModel,
) -> Option<i32> {
    if end.saturating_sub(start) > MAX_CONSTANT_EXPRESSION_TOKENS {
        return None;
    }
    evaluate_constant_integer_expression_at_depth(tokens, start, end, scopes, 0)
}

fn evaluate_constant_integer_expression_at_depth(
    tokens: &[Token],
    start: usize,
    end: usize,
    scopes: &ScopeModel,
    depth: usize,
) -> Option<i32> {
    if start >= end
        || depth > MAX_CONSTANT_EXPRESSION_DEPTH
        || end.saturating_sub(start) > MAX_CONSTANT_EXPRESSION_TOKENS
    {
        return None;
    }

    let mut left = start;
    let mut right = end;
    while left < right && has_outer_parentheses(tokens, left, right) {
        left += 1;
        right -= 1;
    }
    if left >= right {
        return None;
    }

    if let Some(operator) = find_top_level_binary_operator(tokens, left, right) {
        let left_value = evaluate_constant_integer_expression_at_depth(
            tokens,
            left,
            operator,
            scopes,
            depth + 1,
        )?;
        let right_value = evaluate_constant_integer_expression_at_depth(
            tokens,
            operator + 1,
            right,
            scopes,
            depth + 1,
        )?;
        return match tokens[operator].kind {
            TokenKind::Plus => Some(left_value.wrapping_add(right_value)),
            TokenKind::Minus => Some(left_value.wrapping_sub(right_value)),
            TokenKind::Star => Some(left_value.wrapping_mul(right_value)),
            TokenKind::Slash => {
                if right_value == 0 {
                    None
                } else {
                    Some(left_value.wrapping_div(right_value))
                }
            }
            TokenKind::Percent => {
                if right_value == 0 {
                    None
                } else {
                    Some(left_value.wrapping_rem(right_value))
                }
            }
            TokenKind::ShiftLeft => {
                if !(0..32).contains(&right_value) {
                    None
                } else {
                    Some(left_value.wrapping_shl(right_value as u32))
                }
            }
            TokenKind::ShiftRight => {
                if !(0..32).contains(&right_value) {
                    None
                } else {
                    Some(left_value.wrapping_shr(right_value as u32))
                }
            }
            TokenKind::And => Some(left_value & right_value),
            TokenKind::Or => Some(left_value | right_value),
            TokenKind::Xor => Some(left_value ^ right_value),
            TokenKind::AndAnd => Some((left_value != 0 && right_value != 0) as i32),
            TokenKind::OrOr => Some((left_value != 0 || right_value != 0) as i32),
            TokenKind::EqualEqual => Some((left_value == right_value) as i32),
            TokenKind::BangEqual => Some((left_value != right_value) as i32),
            TokenKind::Less => Some((left_value < right_value) as i32),
            TokenKind::LessEqual => Some((left_value <= right_value) as i32),
            TokenKind::Greater => Some((left_value > right_value) as i32),
            TokenKind::GreaterEqual => Some((left_value >= right_value) as i32),
            _ => None,
        };
    }

    if matches!(
        tokens[left].kind,
        TokenKind::Bang | TokenKind::Tilde | TokenKind::Plus | TokenKind::Minus
    ) {
        let value = evaluate_constant_integer_expression_at_depth(
            tokens,
            left + 1,
            right,
            scopes,
            depth + 1,
        )?;
        return match tokens[left].kind {
            TokenKind::Bang => Some((value == 0) as i32),
            TokenKind::Tilde => Some(!value),
            TokenKind::Plus => Some(value),
            TokenKind::Minus => Some(value.wrapping_neg()),
            _ => None,
        };
    }

    if right != left + 1 {
        return None;
    }

    match tokens[left].kind {
        TokenKind::IntegerLiteral => parse_integer_literal_value(&tokens[left].lexeme),
        TokenKind::Identifier => {
            if scopes.resolve(&tokens[left].lexeme, left).is_some() {
                None
            } else {
                canonical_integer_constant(&tokens[left].lexeme)
            }
        }
        _ => None,
    }
}

fn condition_possibilities(
    tokens: &[Token],
    start: usize,
    end: usize,
    empty_is_true: bool,
    scopes: &ScopeModel,
) -> (bool, bool) {
    if end.saturating_sub(start) > MAX_CONSTANT_EXPRESSION_TOKENS {
        return (true, true);
    }

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

    if let Some(value) = evaluate_constant_integer_expression(tokens, left, right, scopes) {
        return (value != 0, value == 0);
    }

    if right == left + 1 {
        let token = &tokens[left];
        let constant_value = match token.kind {
            TokenKind::IntegerLiteral | TokenKind::FloatLiteral => token.lexeme.parse::<f64>().ok(),
            TokenKind::Identifier => {
                if scopes.resolve(token.lexeme.as_str(), left).is_some() {
                    None
                } else {
                    definitions()
                        .constants
                        .get(token.lexeme.as_str())
                        .and_then(|constant| constant.value.parse::<f64>().ok())
                }
            }
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

fn connect_exits(graph: &mut ControlFlowGraph, exits: &[usize], target: usize) -> bool {
    for exit in exits {
        if !graph.add_edge(*exit, target) {
            return false;
        }
    }
    true
}

/// Build a small control-flow graph directly from the token stream.
/// The parser remains syntax-only; this graph is local to one function body.
fn build_control_flow_statement(
    tokens: &[Token],
    start: usize,
    limit: usize,
    scopes: &ScopeModel,
    graph: &mut ControlFlowGraph,
    depth: usize,
) -> Option<FlowFragment> {
    if depth > MAX_CFG_DEPTH {
        graph.limit_exceeded = true;
        return None;
    }
    if graph.limit_exceeded {
        return None;
    }
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
                let statement =
                    build_control_flow_statement(tokens, cursor, close, scopes, graph, depth + 1)?;
                if statement.next_index <= cursor {
                    return None;
                }

                if entry.is_some() {
                    if !connect_exits(graph, &exits, statement.entry) {
                        return None;
                    }
                } else {
                    entry = Some(statement.entry);
                }

                exits = statement.exits;
                cursor = statement.next_index;
            }

            let entry = match entry {
                Some(entry) => entry,
                None => {
                    let empty = graph.add_node()?;
                    exits.push(empty);
                    empty
                }
            };

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

            let then_branch =
                build_control_flow_statement(tokens, close + 1, limit, scopes, graph, depth + 1)?;
            let condition = graph.add_node()?;
            let (may_be_true, may_be_false) =
                condition_possibilities(tokens, open + 1, close, false, scopes);

            let mut exits = Vec::new();
            if may_be_true {
                if !graph.add_edge(condition, then_branch.entry) {
                    return None;
                }
                exits.extend(then_branch.exits.iter().copied());
            }

            if tokens.get(then_branch.next_index).map(|item| item.kind) == Some(TokenKind::KwElse) {
                let else_branch = build_control_flow_statement(
                    tokens,
                    then_branch.next_index + 1,
                    limit,
                    scopes,
                    graph,
                    depth + 1,
                )?;
                if may_be_false {
                    if !graph.add_edge(condition, else_branch.entry) {
                        return None;
                    }
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

            let body =
                build_control_flow_statement(tokens, close + 1, limit, scopes, graph, depth + 1)?;
            let condition = graph.add_node()?;
            let (may_enter_body, may_exit_loop) = condition_possibilities(
                tokens,
                condition_start,
                condition_end,
                empty_is_true,
                scopes,
            );

            if may_enter_body && !graph.add_edge(condition, body.entry) {
                return None;
            }
            if !connect_exits(graph, &body.exits, condition) {
                return None;
            }

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
            let body =
                build_control_flow_statement(tokens, start + 1, limit, scopes, graph, depth + 1)?;
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

            let condition = graph.add_node()?;
            if !connect_exits(graph, &body.exits, condition) {
                return None;
            }
            let (may_repeat, may_exit_loop) =
                condition_possibilities(tokens, open + 1, close, false, scopes);
            if may_repeat && !graph.add_edge(condition, body.entry) {
                return None;
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
            let node = graph.add_node()?;
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

            let node = graph.add_node()?;
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

            let node = graph.add_node()?;
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
            let node = graph.add_node()?;
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

fn analyze_missing_return_paths(
    tokens: &[Token],
    scopes: &ScopeModel,
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

        let body_token_count = body_close.saturating_sub(body_open).saturating_add(1);
        if body_token_count > MAX_CFG_TOKENS_PER_FUNCTION {
            continue;
        }

        let mut graph = ControlFlowGraph::default();
        let Some(body) =
            build_control_flow_statement(tokens, body_open, body_close + 1, scopes, &mut graph, 0)
        else {
            continue;
        };

        let Some(exit) = graph.add_node() else {
            continue;
        };
        if !connect_exits(&mut graph, &body.exits, exit) {
            continue;
        }

        let mut invalid_graph = false;
        for (jump_node, target_name) in std::mem::take(&mut graph.pending_jumps) {
            let Some(target_node) = graph.labels.get(&target_name).copied() else {
                invalid_graph = true;
                break;
            };
            if !graph.add_edge(jump_node, target_node) {
                invalid_graph = true;
                break;
            }
        }
        if invalid_graph || graph.limit_exceeded {
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

#[derive(Debug)]
struct StateInfo {
    name: String,
    span: Span,
    body_open: usize,
    body_close: usize,
    events: Vec<EventInfo>,
}

#[derive(Debug)]
struct EventInfo {
    name: String,
    span: Span,
    body_start: usize,
    body_end: usize,
}

fn collect_state_info(tokens: &[Token], scopes: &ScopeModel) -> Vec<StateInfo> {
    let mut states = Vec::new();
    let mut brace_depth = 0usize;

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
            _ => {}
        }
        if brace_depth != 0 {
            continue;
        }

        let (name, name_index, body_open) = match token.kind {
            TokenKind::KwDefault => ("default".to_owned(), None, index + 1),
            TokenKind::KwState => {
                let Some(name) = tokens.get(index + 1) else {
                    continue;
                };
                if name.kind != TokenKind::Identifier {
                    continue;
                }
                (name.lexeme.clone(), Some(index + 1), index + 2)
            }
            _ => continue,
        };
        if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
            continue;
        }
        let Some(body_close) = matching_brace(tokens, body_open) else {
            continue;
        };
        let span = name_index.map_or(token.span, |index| tokens[index].span);
        states.push(StateInfo {
            name,
            span,
            body_open,
            body_close,
            events: Vec::new(),
        });
    }

    let events = &definitions().events;
    for state in &mut states {
        let Some(state_scope_id) = scopes.scope_at.get(state.body_open + 1).copied() else {
            continue;
        };
        let mut event_index = state.body_open + 1;
        while event_index < state.body_close {
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
            let body_open = parameter_close + 1;
            if tokens.get(body_open).map(|item| item.kind) != Some(TokenKind::LBrace) {
                event_index += 1;
                continue;
            }
            let Some(body_close) = matching_brace(tokens, body_open) else {
                event_index += 1;
                continue;
            };

            state.events.push(EventInfo {
                name: event_token.lexeme.clone(),
                span: event_token.span,
                body_start: body_open + 1,
                body_end: body_close,
            });
            event_index = body_close + 1;
        }
    }

    states
}

fn analyze_state_topology(
    tokens: &[Token],
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let states = collect_state_info(tokens, scopes);
    let global_variables: HashMap<&str, Span> = scopes
        .scopes
        .first()
        .into_iter()
        .flat_map(|scope| scope.variables.iter())
        .filter_map(|(name, bindings)| {
            bindings
                .first()
                .map(|binding| (name.as_str(), binding.span))
        })
        .collect();
    let mut seen_states: HashMap<&str, Span> = HashMap::new();
    for state in &states {
        // Parser owns the mandatory/unique default-state checks (PAR013/PAR014).
        if state.name == "default" {
            continue;
        }
        if let Some(previous) = seen_states.insert(&state.name, state.span) {
            diagnostics.push(Diagnostic {
                code: "SEM021",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "duplicate state '{}'; first declared at {}:{}",
                    state.name, previous.line, previous.column
                ),
                span: state.span,
            });
        } else if let Some(previous) = global_variables.get(state.name.as_str()) {
            diagnostics.push(Diagnostic {
                code: "SEM021",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "state '{}' conflicts with a global variable declared at {}:{}",
                    state.name, previous.line, previous.column
                ),
                span: state.span,
            });
        } else if let Some(function) = user_functions.get(state.name.as_str()) {
            let previous = tokens[function.declaration_index].span;
            diagnostics.push(Diagnostic {
                code: "SEM021",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "state '{}' conflicts with a function declared at {}:{}",
                    state.name, previous.line, previous.column
                ),
                span: state.span,
            });
        }
    }

    let mut known_states = HashSet::new();
    for state in &states {
        known_states.insert(state.name.as_str());
        let mut seen_events: HashMap<&str, Span> = HashMap::new();
        for event in &state.events {
            if let Some(previous) = seen_events.insert(&event.name, event.span) {
                diagnostics.push(Diagnostic {
                    code: "SEM020",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "duplicate event handler '{}' in state '{}'; first declared at {}:{}",
                        event.name, state.name, previous.line, previous.column
                    ),
                    span: event.span,
                });
            }
        }
    }

    for state in &states {
        for event in &state.events {
            for index in event.body_start..event.body_end {
                if tokens[index].kind != TokenKind::KwState
                    || tokens.get(index + 1).map(|item| item.kind) != Some(TokenKind::Identifier)
                    || tokens.get(index + 2).map(|item| item.kind) != Some(TokenKind::Semicolon)
                {
                    continue;
                }
                let target = &tokens[index + 1];
                if !known_states.contains(target.lexeme.as_str()) {
                    diagnostics.push(Diagnostic {
                        code: "SEM022",
                        severity: Severity::Error,
                        kind: DiagnosticKind::Semantic,
                        message: format!(
                            "state target '{}' is not defined; add that state or use state default",
                            target.lexeme
                        ),
                        span: target.span,
                    });
                }
            }
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
            } else if left == "vector" && right == "vector" {
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
            } else if (left == "vector"
                && (matches!(right, "integer" | "float") || right == "rotation"))
                || (matches!(left, "integer" | "float") && right == "vector")
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

fn assignment_expression_end(tokens: &[Token], start: usize) -> Option<usize> {
    if start >= tokens.len() {
        return None;
    }

    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut vector_depth = 0usize;

    for index in start..tokens.len() {
        let kind = tokens[index].kind;
        let at_expression_level =
            paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && vector_depth == 0;

        if at_expression_level
            && matches!(
                kind,
                TokenKind::Comma
                    | TokenKind::Semicolon
                    | TokenKind::RParen
                    | TokenKind::RBracket
                    | TokenKind::RBrace
            )
        {
            return Some(index);
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
    }

    Some(tokens.len())
}

fn component_assignment_target(tokens: &[Token], operator_index: usize) -> Option<(usize, usize)> {
    let mut end = operator_index;
    let mut grouped_start = None;

    // Strip grouping parentheses around a component assignment target.
    while tokens.get(end.checked_sub(1)?)?.kind == TokenKind::RParen {
        let mut depth = 0usize;
        let mut matching_open = None;
        for (index, token) in tokens.iter().enumerate().take(end).rev() {
            match token.kind {
                TokenKind::RParen => depth += 1,
                TokenKind::LParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        matching_open = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }

        grouped_start = Some(matching_open?.checked_add(1)?);
        end -= 1;
    }

    let start = grouped_start.or_else(|| end.checked_sub(3))?;

    if end != start + 3
        || tokens.get(start)?.kind != TokenKind::Identifier
        || tokens.get(start + 1)?.kind != TokenKind::Dot
        || tokens.get(start + 2)?.kind != TokenKind::Identifier
    {
        return None;
    }

    Some((start, start + 2))
}

fn is_valid_component(base_type: &str, component: &str) -> bool {
    match base_type {
        "vector" => matches!(component, "x" | "y" | "z"),
        "rotation" => matches!(component, "x" | "y" | "z" | "s"),
        _ => false,
    }
}

fn analyze_component_reads(
    tokens: &[Token],
    scopes: &ScopeModel,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Component writes belong exclusively to SEM018. Reuse its target parser
    // so grouped lvalues such as (position.x) are not reported as reads.
    let mut assignment_components = vec![false; tokens.len()];
    for (operator_index, operator) in tokens.iter().enumerate() {
        if !matches!(
            operator.kind,
            TokenKind::Equal
                | TokenKind::PlusEqual
                | TokenKind::MinusEqual
                | TokenKind::StarEqual
                | TokenKind::SlashEqual
                | TokenKind::PercentEqual
        ) {
            continue;
        }

        if let Some((_, component_index)) = component_assignment_target(tokens, operator_index) {
            assignment_components[component_index] = true;
        }
    }

    for (base_index, base) in tokens.iter().enumerate() {
        if base.kind != TokenKind::Identifier
            || tokens.get(base_index + 1).map(|token| token.kind) != Some(TokenKind::Dot)
            || tokens.get(base_index + 2).map(|token| token.kind) != Some(TokenKind::Identifier)
            || assignment_components
                .get(base_index + 2)
                .copied()
                .unwrap_or(false)
        {
            continue;
        }

        // Unknown extension-provided values keep the analyzer conservative.
        let Some(base_type) = scopes.resolve(&base.lexeme, base_index) else {
            continue;
        };
        let component = &tokens[base_index + 2];
        if is_valid_component(&base_type, &component.lexeme) {
            continue;
        }

        diagnostics.push(Diagnostic {
            code: "SEM019",
            severity: Severity::Error,
            kind: DiagnosticKind::Semantic,
            message: format!(
                "component access '.{}' is not valid for variable '{}' of type {}",
                component.lexeme, base.lexeme, base_type
            ),
            span: component.span,
        });
    }
}

fn analyze_component_assignments(
    tokens: &[Token],
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (operator_index, operator) in tokens.iter().enumerate() {
        let binary_operator = match operator.kind {
            TokenKind::Equal => None,
            TokenKind::PlusEqual => Some(TokenKind::Plus),
            TokenKind::MinusEqual => Some(TokenKind::Minus),
            TokenKind::StarEqual => Some(TokenKind::Star),
            TokenKind::SlashEqual => Some(TokenKind::Slash),
            TokenKind::PercentEqual => Some(TokenKind::Percent),
            _ => continue,
        };

        let Some((base_index, component_index)) =
            component_assignment_target(tokens, operator_index)
        else {
            continue;
        };
        let base = &tokens[base_index];
        let component = &tokens[component_index];
        let Some(base_type) = scopes.resolve(&base.lexeme, base_index) else {
            continue;
        };

        if !is_valid_component(&base_type, &component.lexeme) {
            diagnostics.push(Diagnostic {
                code: "SEM018",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "component '{}' is not valid for variable '{}' of type {}",
                    component.lexeme, base.lexeme, base_type
                ),
                span: component.span,
            });
            continue;
        }

        let expression_start = operator_index + 1;
        let Some(expression_end) = assignment_expression_end(tokens, expression_start) else {
            continue;
        };
        if expression_start >= expression_end {
            continue;
        }

        let Some(value_type) = infer_expression_type(
            tokens,
            expression_start,
            expression_end,
            scopes,
            user_functions,
        ) else {
            continue;
        };

        let result_type = if let Some(binary_operator) = binary_operator {
            let Some(result_type) = infer_binary_result_type(binary_operator, "float", &value_type)
            else {
                diagnostics.push(Diagnostic {
                    code: "SEM018",
                    severity: Severity::Error,
                    kind: DiagnosticKind::Semantic,
                    message: format!(
                        "compound assignment '{}' is not supported for component types float and {}",
                        operator.lexeme, value_type
                    ),
                    span: component.span,
                });
                continue;
            };
            result_type
        } else {
            value_type.as_str()
        };

        if !can_implicitly_convert(result_type, "float") {
            let operator_name = if binary_operator.is_some() {
                format!("compound assignment '{}'", operator.lexeme)
            } else {
                "assignment".to_owned()
            };
            diagnostics.push(Diagnostic {
                code: "SEM018",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "{} to component '{}.{}' yields type {}, expected float",
                    operator_name, base.lexeme, component.lexeme, result_type
                ),
                span: component.span,
            });
        }
    }
}

fn analyze_compound_assignments(
    tokens: &[Token],
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (operator_index, operator) in tokens.iter().enumerate() {
        let binary_operator = match operator.kind {
            TokenKind::PlusEqual => TokenKind::Plus,
            TokenKind::MinusEqual => TokenKind::Minus,
            TokenKind::StarEqual => TokenKind::Star,
            TokenKind::SlashEqual => TokenKind::Slash,
            TokenKind::PercentEqual => TokenKind::Percent,
            _ => continue,
        };
        if operator_index == 0 {
            continue;
        }

        let name_index = operator_index - 1;
        let name = &tokens[name_index];
        if name.kind != TokenKind::Identifier {
            continue;
        }

        // Only ordinary variable targets are analyzed. Member targets and
        // declaration initializers are outside this conservative check.
        if tokens
            .get(name_index.wrapping_sub(1))
            .is_some_and(|previous| is_value_type(previous.kind) || previous.kind == TokenKind::Dot)
            || scopes
                .initializers
                .iter()
                .any(|initializer| initializer.name_index == name_index)
        {
            continue;
        }

        let Some(target_type) = scopes.resolve(&name.lexeme, name_index) else {
            continue;
        };
        let expression_start = operator_index + 1;
        let Some(expression_end) = assignment_expression_end(tokens, expression_start) else {
            continue;
        };
        if expression_start >= expression_end {
            continue;
        }

        let Some(value_type) = infer_expression_type(
            tokens,
            expression_start,
            expression_end,
            scopes,
            user_functions,
        ) else {
            continue;
        };
        let Some(result_type) =
            infer_binary_result_type(binary_operator, &target_type, &value_type)
        else {
            diagnostics.push(Diagnostic {
                code: "SEM017",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "compound assignment '{}' is not supported for operand types {} and {}",
                    operator.lexeme, target_type, value_type
                ),
                span: name.span,
            });
            continue;
        };

        if !can_implicitly_convert(result_type, &target_type) {
            diagnostics.push(Diagnostic {
                code: "SEM017",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "compound assignment '{}' to variable '{}' yields type {}, expected {}",
                    operator.lexeme, name.lexeme, result_type, target_type
                ),
                span: name.span,
            });
        }
    }
}

fn analyze_variable_assignments(
    tokens: &[Token],
    scopes: &ScopeModel,
    user_functions: &HashMap<String, UserFunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (operator_index, operator) in tokens.iter().enumerate() {
        if operator.kind != TokenKind::Equal || operator_index == 0 {
            continue;
        }

        let name_index = operator_index - 1;
        let name = &tokens[name_index];
        if name.kind != TokenKind::Identifier {
            continue;
        }

        // Declarations are checked by SEM015. Only a plain variable is a valid
        // assignment target in this deliberately conservative first pass.
        if tokens
            .get(name_index.wrapping_sub(1))
            .is_some_and(|previous| is_value_type(previous.kind) || previous.kind == TokenKind::Dot)
            || scopes
                .initializers
                .iter()
                .any(|initializer| initializer.name_index == name_index)
        {
            continue;
        }

        let Some(expected_type) = scopes.resolve(&name.lexeme, name_index) else {
            continue;
        };
        let expression_start = operator_index + 1;
        let Some(expression_end) = assignment_expression_end(tokens, expression_start) else {
            continue;
        };
        if expression_start >= expression_end {
            continue;
        }

        let Some(actual_type) = infer_expression_type(
            tokens,
            expression_start,
            expression_end,
            scopes,
            user_functions,
        ) else {
            continue;
        };

        if !can_implicitly_convert(&actual_type, &expected_type) {
            diagnostics.push(Diagnostic {
                code: "SEM016",
                severity: Severity::Error,
                kind: DiagnosticKind::Semantic,
                message: format!(
                    "assignment to variable '{}' has type {}, expected {}",
                    name.lexeme, actual_type, expected_type
                ),
                span: name.span,
            });
        }
    }
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
        return match operand_type.as_str() {
            "integer" | "float" => Some(operand_type),
            "vector" | "rotation" if tokens[start].kind == TokenKind::Minus => Some(operand_type),
            _ => None,
        };
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

fn closes_explicit_cast_parentheses(tokens: &[Token], close_index: usize) -> bool {
    if tokens.get(close_index).map(|token| token.kind) != Some(TokenKind::RParen) {
        return false;
    }

    let Some(open_index) = close_index.checked_sub(2) else {
        return false;
    };
    tokens.get(open_index).map(|token| token.kind) == Some(TokenKind::LParen)
        && tokens
            .get(open_index + 1)
            .is_some_and(|token| lsl_type_name(token.kind).is_some())
}

fn starts_vector_literal(tokens: &[Token], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return true;
    };

    match previous.kind {
        // `return` introduces an expression without a delimiter token.
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
        // Only an explicit cast such as `(vector)<...>` can put a vector
        // literal immediately after a closing parenthesis. Otherwise this is
        // a relational operator, e.g. `(left) < right`.
        TokenKind::RParen => closes_explicit_cast_parentheses(tokens, index - 1),
        _ => false,
    }
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

    fn evaluate_constant_source(source: &str) -> Option<i32> {
        let (tokens, lexical_diagnostics) = lex(source);
        assert!(lexical_diagnostics.is_empty(), "{lexical_diagnostics:?}");
        let scopes = ScopeModel::build(&tokens);
        evaluate_constant_integer_expression(&tokens, 0, tokens.len().saturating_sub(1), &scopes)
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
    fn leaves_unlisted_vector_scalar_addition_uninferred() {
        let diagnostics = analyze_source(
            "default { state_entry() { vector position; llOwnerSay(position + 1.0); llOwnerSay(position + 1); llOwnerSay(1.0 + position); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
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
            r#"default { state_entry() { llOwnerSay(<1, 2, 3> * llGetRot()); llOwnerSay(<1, 2, 3> / llGetRot()); llOwnerSay(<1, 2, 3> * 2.0); llOwnerSay(llGetRot() + llGetRot()); llOwnerSay(llGetRot() * <1, 2, 3>); llOwnerSay(2.0 * <1, 2, 3>); } }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            5,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn covers_confirmed_binary_type_matrix_rows() {
        let cases: &[(TokenKind, &str, &str, Option<&'static str>)] = &[
            (TokenKind::Plus, "integer", "list", Some("list")),
            (TokenKind::Plus, "list", "integer", Some("list")),
            (TokenKind::Plus, "string", "list", Some("list")),
            (TokenKind::Plus, "list", "string", Some("list")),
            (TokenKind::Plus, "vector", "list", Some("list")),
            (TokenKind::Plus, "list", "rotation", Some("list")),
            (TokenKind::Plus, "string", "key", None),
            (TokenKind::Percent, "vector", "vector", Some("vector")),
            (TokenKind::Percent, "integer", "integer", Some("integer")),
            (TokenKind::And, "integer", "integer", Some("integer")),
            (TokenKind::Or, "integer", "integer", Some("integer")),
            (TokenKind::Xor, "integer", "integer", Some("integer")),
            (TokenKind::ShiftLeft, "integer", "integer", Some("integer")),
            (TokenKind::ShiftRight, "integer", "integer", Some("integer")),
            (TokenKind::AndAnd, "integer", "integer", Some("integer")),
            (TokenKind::OrOr, "integer", "integer", Some("integer")),
            (TokenKind::And, "integer", "float", None),
            (TokenKind::ShiftLeft, "integer", "float", None),
            (TokenKind::Star, "integer", "vector", Some("vector")),
            (TokenKind::Star, "float", "vector", Some("vector")),
            (TokenKind::Star, "vector", "integer", Some("vector")),
            (TokenKind::Star, "vector", "float", Some("vector")),
            (TokenKind::Star, "vector", "vector", Some("float")),
            (TokenKind::Star, "rotation", "vector", None),
            (TokenKind::Slash, "vector", "vector", None),
            (TokenKind::Minus, "list", "list", None),
            (TokenKind::Plus, "vector", "integer", None),
        ];

        for &(operator, left, right, expected) in cases {
            assert_eq!(
                infer_binary_result_type(operator, left, right),
                expected,
                "{operator:?} {left} {right}"
            );
        }
    }

    #[test]
    fn infers_matrix_rows_in_typed_contexts() {
        let diagnostics = analyze_source(
            r#"default { state_entry() {
                llDumpList2String("x" + [1], ",");
                llDumpList2String([1] + "x", ",");
                llSetPos(<1, 2, 3> % <4, 5, 6>);
                llOwnerSay("x" + [1]);
                llOwnerSay([1] + "x");
                llOwnerSay(<1, 2, 3> % <4, 5, 6>);
                llOwnerSay(1 & 2);
                llOwnerSay(1 | 2);
                llOwnerSay(1 ^ 2);
                llOwnerSay(1 << 2);
                llOwnerSay(1 >> 2);
                llOwnerSay(1 && 2);
                llOwnerSay(1 || 2);
            } }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM004")
                .count(),
            10,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_scalar_vector_multiplication_both_orders() {
        let diagnostics = analyze_source(
            "default { state_entry() { llSetPos(2 * <1, 2, 3>); llSetPos(2.0 * <1, 2, 3>); llOwnerSay(2 * <1, 2, 3>); llOwnerSay(2.0 * <1, 2, 3>); } }",
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
    fn infers_vector_and_rotation_unary_minus() {
        let diagnostics = analyze_source(
            "default { state_entry() { llSetPos(-<1, 2, 3>); llSetRot(-llGetRot()); llOwnerSay(-<1, 2, 3>); llOwnerSay(-llGetRot()); } }",
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
    fn leaves_rotation_times_vector_unresolved() {
        let diagnostics =
            analyze_source("default { state_entry() { llOwnerSay(llGetRot() * <1, 2, 3>); } }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
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
    fn validates_vector_and_rotation_component_assignment_types() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    vector position;
                    rotation orientation;
                    integer count;

                    position.x = "wrong";
                    position.s = 1.0;
                    (position.z) = "wrong";
                    count.x = 1.0;
                    orientation.s = 1;
                    orientation.x += 0.5;
                    (position.y) = 2;
                }
            }"#,
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM018")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 4, "{diagnostics:?}");
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("position.x")
                    && item.message.contains("type string, expected float")
            }),
            "{diagnostics:?}"
        );
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("position.z")
                    && item.message.contains("type string, expected float")
            }),
            "{diagnostics:?}"
        );
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("component 's'")
                    && item.message.contains("variable 'position'")
                    && item.message.contains("vector")
            }),
            "{diagnostics:?}"
        );
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("component 'x'")
                    && item.message.contains("variable 'count'")
                    && item.message.contains("integer")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_invalid_component_reads_for_known_variable_types() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    vector position;
                    integer count;
                    rotation orientation;

                    llOwnerSay((string)position.s);
                    llOwnerSay((string)count.x);
                    llOwnerSay((string)orientation.w);
                }
            }"#,
        );
        let invalid_reads = diagnostics
            .iter()
            .filter(|item| item.code == "SEM019")
            .collect::<Vec<_>>();
        assert_eq!(invalid_reads.len(), 3, "{diagnostics:?}");
        assert!(invalid_reads
            .iter()
            .any(|item| item.message.contains("'.s'") && item.message.contains("vector")));
        assert!(invalid_reads
            .iter()
            .any(|item| item.message.contains("'.x'") && item.message.contains("integer")));
        assert!(invalid_reads
            .iter()
            .any(|item| item.message.contains("'.w'") && item.message.contains("rotation")));
        assert!(invalid_reads
            .iter()
            .all(|item| item.severity == Severity::Error && item.kind == DiagnosticKind::Semantic));
    }

    #[test]
    fn accepts_all_declared_vector_and_rotation_component_reads() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    vector position;
                    rotation orientation;

                    float px = position.x;
                    float py = position.y;
                    float pz = position.z;
                    float ox = orientation.x;
                    float oy = orientation.y;
                    float oz = orientation.z;
                    float os = orientation.s;
                }
            }"#,
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM019"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn keeps_unknown_extension_component_reads_conservative() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    llOwnerSay((string)extensionVector.x);
                }
            }"#,
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM019"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn component_writes_are_reported_by_sem018_not_sem019() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    vector position;
                    integer count;
                    rotation orientation;

                    position.s = 1.0;
                    count.x = 1;
                    orientation.w = 0.0;
                }
            }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM018")
                .count(),
            3,
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM019"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_component_compound_assignments_with_known_unsupported_operand_pairs() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    vector position;
                    position.x += "wrong";
                    position.y %= 2;
                }
            }"#,
        );
        let invalid = diagnostics
            .iter()
            .filter(|item| item.code == "SEM018")
            .collect::<Vec<_>>();
        assert_eq!(invalid.len(), 2, "{diagnostics:?}");
        assert!(
            invalid
                .iter()
                .all(|item| item.message.contains("not supported")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn skips_component_assignments_with_unknown_base_or_rhs_types() {
        let diagnostics = analyze_source(
            "default { state_entry() { llExtensionVector().x = 1.0; vector position; position.x = llExtensionValue(); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM018"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn validates_compound_assignment_result_types() {
        let diagnostics = analyze_source(
            r#"integer count;
               float ratio;
               string label;
               list values;
               vector position;
               rotation orientation;
               default {
                   state_entry() {
                       count += 1.5;
                       count -= 2.5;
                       count *= 3.5;
                       count /= 4.5;
                       count %= 2;
                       ratio += 1;
                       label += " units";
                       values += [1, "one"];
                       position += <1.0, 1.0, 1.0>;
                       orientation += orientation;
                   }
               }"#,
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM017")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 4, "{diagnostics:?}");
        assert!(
            mismatches
                .iter()
                .all(|item| { item.message.contains("yields type float, expected integer") }),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM016"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_compound_assignments_with_known_unsupported_operand_pairs() {
        let diagnostics = analyze_source(
            r#"default {
                state_entry() {
                    integer count;
                    float ratio;
                    string label;
                    count += "wrong";
                    ratio %= 2;
                    label -= "wrong";
                }
            }"#,
        );
        let invalid = diagnostics
            .iter()
            .filter(|item| item.code == "SEM017")
            .collect::<Vec<_>>();
        assert_eq!(invalid.len(), 3, "{diagnostics:?}");
        assert!(
            invalid
                .iter()
                .any(|item| item.message.contains("integer and string")),
            "{diagnostics:?}"
        );
        assert!(
            invalid
                .iter()
                .any(|item| item.message.contains("float and integer")),
            "{diagnostics:?}"
        );
        assert!(
            invalid
                .iter()
                .any(|item| item.message.contains("string and string")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn skips_compound_assignments_with_unknown_or_unresolved_result_types() {
        let diagnostics = analyze_source(
            "default { state_entry() { integer count; count += llExtensionValue(); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM017"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn validates_assignments_to_global_and_local_variables() {
        let diagnostics = analyze_source(
            r#"integer global_count;
               key global_id;
               default {
                   state_entry() {
                       integer local_count;
                       float local_ratio;
                       local_count = 2.5;
                       local_ratio = 1;
                       global_id = "00000000-0000-0000-0000-000000000000";
                       local_count = "wrong";
                   }
               }"#,
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM016")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 2, "{diagnostics:?}");
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("local_count")
                    && item.message.contains("float, expected integer")
            }),
            "{diagnostics:?}"
        );
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("local_count")
                    && item.message.contains("string, expected integer")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn validates_assignments_inside_control_flow_and_call_arguments() {
        let diagnostics = analyze_source(
            "default { state_entry() { integer count; if (count = 1.5) llOwnerSay(\"bad\"); llOwnerSay((string)(count = 2.5)); } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM016")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn skips_unknown_assignment_values_and_declaration_initializers() {
        let diagnostics = analyze_source(
            "default { state_entry() { integer first = 1.5, second; second = llExtensionValue(); } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM015")
                .count(),
            1,
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM016"),
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
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM012"),
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
    fn reports_initializer_mismatches_for_resolved_scope_names() {
        let diagnostics = analyze_source(
            "integer global_integer = 1; string bad_global = global_integer; default { state_entry() { integer prior; string bad_prior = prior; integer i; for (i = 0; i < 1; i++) { integer inner = 1; string bad_inner = inner; } { float nested; string bad_nested = nested; } } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM015")
                .count(),
            4,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn resolves_parameter_and_nested_shadowing_before_globals() {
        let diagnostics = analyze_source(
            "string shadowed; integer process(integer shadowed) { { float shadowed; llSleep(shadowed); } llOwnerSay(shadowed); return shadowed; } default { touch_start(integer shadowed) { llOwnerSay(shadowed); } }",
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
    fn resolves_names_in_complex_initializers_and_loop_bodies() {
        let diagnostics = analyze_source(
            "integer global = 1; default { state_entry() { integer first = global + 1; integer second = first + 1; integer i; for (i = 0; i < second; i++) { integer inner = i; llOwnerSay(inner); } llOwnerSay(second); } }",
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
    fn keeps_unknown_extension_calls_in_initializers_conservative() {
        let diagnostics = analyze_source(
            "integer global = extensionOuter(extensionInner(missing)); default { state_entry() { integer local = extensionOuter(extensionInner(missing)); llOwnerSay((string)local); } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM012"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_unresolved_initializer_without_guessing_type() {
        let diagnostics =
            analyze_source("default { state_entry() { integer value = missing + 1; } }");
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "SEM012" && item.severity == Severity::Warning),
            "{diagnostics:?}"
        );
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
    fn evaluates_constant_integer_if_conditions() {
        let diagnostics = analyze_source(
            "integer yes() { if (1 + 2) return 1; } integer no() { if (1 > 2) return 1; } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function no"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_constant_integer_loop_conditions() {
        let diagnostics = analyze_source(
            "integer spin() { while (1 + 2) { } } integer dead() { while (1 > 2) { } } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function dead"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_canonical_integer_constants_in_conditions() {
        let diagnostics = analyze_source(
            "integer yes() { while (TRUE + FALSE) { } } integer hex() { while (AGENT_AUTOPILOT) { } } integer no() { while (ALL_SIDES + 1) { } } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function no"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_nested_constant_integer_expressions_directly() {
        assert_eq!(evaluate_constant_source("((1 + 2) * 3)"), Some(9));
        assert_eq!(evaluate_constant_source("1 - 2 + 3"), Some(2));
        assert_eq!(evaluate_constant_source("1 || 1 && 0"), Some(0));
    }

    #[test]
    fn evaluates_hex_integer_literals_directly() {
        assert_eq!(evaluate_constant_source("0xFFFFFFFF"), Some(-1));
        assert_eq!(evaluate_constant_source("0x80000000"), Some(i32::MIN));
        assert_eq!(evaluate_constant_source("0x7FFFFFFF"), Some(i32::MAX));
        assert_eq!(evaluate_constant_source("0x0000000000000001"), Some(1));
        assert_eq!(evaluate_constant_source("0x00000000FFFFFFFF"), Some(-1));
        assert_eq!(evaluate_constant_source("0x1e3"), Some(483));
        assert_eq!(evaluate_constant_source("0x1e+2"), Some(32));
        assert_eq!(evaluate_constant_source("0x1e-3"), Some(27));
        assert_eq!(evaluate_constant_source("0X2A"), Some(42));
        assert_eq!(evaluate_constant_source("0xFFFFFFFF + 1"), Some(0));
        for source in ["0x", "0xG", "0x1G", "0x1.2"] {
            assert_eq!(
                evaluate_constant_source(source),
                None,
                "malformed hex was evaluated: {source:?}"
            );
        }
    }

    #[test]
    fn rejects_hex_values_above_u32_max() {
        for source in [
            "0x100000000",
            "0x1FFFFFFFF",
            "0x0000000100000000",
            "0x123456789ABCDEF",
        ] {
            assert_eq!(
                evaluate_constant_source(source),
                None,
                "out-of-range hex was evaluated: {source:?}"
            );
        }
    }

    #[test]
    fn rejects_long_constant_integer_binary_chains() {
        let source = format!("1{}", " + 1".repeat(MAX_CONSTANT_EXPRESSION_TOKENS / 2 + 1));
        assert_eq!(evaluate_constant_source(&source), None);
    }

    #[test]
    fn rejects_long_constant_integer_unary_chains() {
        let source = format!("{}1", "!".repeat(MAX_CONSTANT_EXPRESSION_DEPTH + 2));
        assert_eq!(evaluate_constant_source(&source), None);
    }

    #[test]
    fn rejects_deeply_nested_constant_integer_parentheses() {
        let source = format!(
            "{}1{}",
            "(".repeat(MAX_CONSTANT_EXPRESSION_TOKENS),
            ")".repeat(MAX_CONSTANT_EXPRESSION_TOKENS)
        );
        assert_eq!(evaluate_constant_source(&source), None);
    }

    #[test]
    fn condition_range_limit_keeps_long_conditions_unresolved() {
        let condition = format!(
            "{}1{}",
            "(".repeat(MAX_CONSTANT_EXPRESSION_TOKENS),
            ")".repeat(MAX_CONSTANT_EXPRESSION_TOKENS)
        );
        let diagnostics = analyze_source(&format!(
            "integer long_condition() {{ while ({condition}) {{ }} }} default {{ }}"
        ));
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM011")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn rejects_incomplete_constant_integer_expressions() {
        for source in ["1 +", "(1 + 2", "()", "1 = 2", "1 &&"] {
            assert_eq!(
                evaluate_constant_source(source),
                None,
                "unexpected value for {source:?}"
            );
        }
    }

    #[test]
    fn rejects_out_of_range_shift_counts() {
        let diagnostics = analyze_source(
            "integer a() { while (1 << -1) { } }              integer b() { while (1 << 32) { } }              integer c() { while (1 >> 32) { } }              integer d() { while (-2147483648 >> -1) { } } default { }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM011")
                .count(),
            4,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_in_range_shift_counts() {
        let diagnostics = analyze_source(
            "integer a() { while (1 << 0) { } }              integer b() { while (1 << 31) { } }              integer c() { while (-1 >> 0) { } }              integer d() { while (-1 >> 31) { } }              integer e() { while (1 >> 31) { } } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(returns[0].message.contains("function e"), "{diagnostics:?}");
    }

    #[test]
    fn evaluates_unary_constant_conditions() {
        let diagnostics = analyze_source(
            "integer unary() { while (!0 && (~0 == -1) && (-1 + +1 == 0)) { } } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_bitwise_and_shift_conditions() {
        let diagnostics = analyze_source(
            "integer shift() { while ((1 << 3) == 8) { } } integer bits() { while ((7 & 3) == 3 && (1 | 2) == 3 && (5 ^ 1) == 4) { } } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_integer_division_and_remainder_signs() {
        let diagnostics = analyze_source(
            "integer division() { while (-15 / 2 == -7) { } } integer remainder() { while (-15 % 4 == -3) { } } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn preserves_float_constant_condition_truthiness() {
        let diagnostics = analyze_source(
            "integer yes() { while (1.0) { } } integer no() { while (0.0) { } } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function no"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn preserves_lsl_precedence_in_constant_conditions() {
        let diagnostics = analyze_source(
            "integer subtract_first() { while (1 - 2 + 3 == 2) { } } integer logical_group() { while (1 || 1 && 0) { } } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function logical_group"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn does_not_short_circuit_constant_logical_conditions() {
        let diagnostics = analyze_source(
            "integer or_division() { while (1 || (1 / 0)) { } } integer and_division() { while (0 && (1 / 0)) { } } default { }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM011")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_wrapping_32_bit_conditions() {
        let diagnostics = analyze_source(
            "integer overflow() { while (2147483647 + 1) { } }              integer shift() { while (1 << 31) { } } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_hex_integer_conditions() {
        let diagnostics = analyze_source(
            r#"integer true_high() { while (0xFFFFFFFF) { } }
            integer true_min() { while (0x80000000) { } }
            integer true_max() { while (0x7FFFFFFF) { } }
            integer false_zero() { while (0x0) { } }
            integer wrap_zero() { while (0xFFFFFFFF + 1) { } }
            integer exponent_digit() { while (0x1e3 == 483) { } }
            integer bitwise() { while (0xFF & 0x0F) { } }
            integer shifted() { while ((0x10 << 1) == 0x20) { } }
            default { }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM011")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn leaves_out_of_range_hex_conditions_unknown() {
        let diagnostics = analyze_source(
            r#"integer first() { while (0x100000000) { } }
            integer second() { while (0x1FFFFFFFF) { } }
            default { }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM011")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_hex_integer_literal_types() {
        let diagnostics =
            analyze_source("integer whole = 0xFFFFFFFF; float promoted = 0x10; default { }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM015"),
            "{diagnostics:?}"
        );

        let diagnostics = analyze_source("string invalid = 0x1; default { }");
        assert!(
            diagnostics.iter().any(|item| item.code == "SEM015"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn treats_shadowed_canonical_constant_names_as_unknown() {
        let diagnostics = analyze_source(
            r#"integer known() { if (TRUE) return 1; }
            integer choose_param(integer TRUE) { if (TRUE) return 1; }
            integer TRUE = 0;
            integer choose_global() { if (TRUE) return 1; }
            integer choose_local() { integer TRUE; if (TRUE) return 1; }
            integer choose_nested() { { integer TRUE; if (TRUE) return 1; } }
            default { }"#,
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 4, "{diagnostics:?}");
        for name in [
            "choose_param",
            "choose_global",
            "choose_local",
            "choose_nested",
        ] {
            assert!(
                returns
                    .iter()
                    .any(|item| item.message.contains(&format!("function {name}"))),
                "missing SEM011 for {name}: {diagnostics:?}"
            );
        }
        assert!(
            !returns
                .iter()
                .any(|item| item.message.contains("function known")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_wrapping_integer_literals() {
        let diagnostics = analyze_source(
            "integer yes() { while (2147483648) { } } integer no() { while (4294967296) { } } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function no"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn leaves_division_by_zero_and_unknown_conditions_unresolved() {
        let diagnostics = analyze_source(
            "integer division() { while (1 / 0) { } }              integer remainder() { while (1 % 0) { } }              integer unknown() { while (llExtensionValue() + 1) { } } default { }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM011")
                .count(),
            3,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_for_loop_conditions_in_cfg() {
        let diagnostics = analyze_source(
            "integer for_true() { for (; 1 + 2; ) { } }              integer for_false() { for (; 1 > 2; ) { } }              integer for_unknown(integer flag) { for (; flag; ) { } } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 2, "{diagnostics:?}");
        assert!(
            returns
                .iter()
                .all(|item| item.message.contains("for_false")
                    || item.message.contains("for_unknown")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_do_while_conditions_in_cfg() {
        let diagnostics = analyze_source(
            "integer do_true() { do { } while (1 + 2); }              integer do_false() { do { } while (1 > 2); }              integer do_unknown(integer flag) { do { } while (flag); } default { }",
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 2, "{diagnostics:?}");
        assert!(
            returns.iter().all(
                |item| item.message.contains("do_false") || item.message.contains("do_unknown")
            ),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn handles_nested_branches_and_else_if_chains() {
        let diagnostics = analyze_source(
            r#"integer all_return(integer a, integer b) {
                if (a) { if (b) return 1; else return 2; }
                else { if (b) return 3; else return 4; }
            }
            integer missing(integer a) { if (a) { if (a) return 1; } }
            default { }"#,
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function missing"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn handles_nested_and_sequential_loops() {
        let diagnostics = analyze_source(
            r#"integer forever_for() { for (; 1; ) { while (1) { return 1; } } }
            integer nested_for(integer n) { for (; n; ) { for (; n; ) { return 1; } } }
            integer nested_while(integer n) { while (n) { while (n) { return 1; } } }
            integer nested_do(integer n) { do { do { } while (n); } while (n); }
            integer sequential(integer n) { while (n) { return 1; } while (n) { return 2; } }
            integer do_return() { do { return 1; } while (1); }
            integer do_false() { do { } while (0); }
            integer do_unknown(integer n) { do { } while (n); }
            default { }"#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM011")
                .count(),
            6,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn handles_jump_combinations_and_sequential_returns() {
        let diagnostics = analyze_source(
            r#"integer jump_nested(integer flag) { if (flag) { { jump done; } } return 1; @done; }
            integer jump_to_fallback(integer flag) { if (flag) { jump done; } return 1; @done; return 2; }
            integer jump_loop(integer flag) { while (flag) { jump done; } @done; return 1; }
            integer backward() { @again; jump again; }
            integer label_after_return() { return 1; @dead; }
            integer jump_through_labels() { jump first; @first; @second; return 1; }
            integer first_return() { return 1; return 2; }
            integer middle_return(integer flag) { if (flag) return 1; return 2; }
            integer terminal_return(integer flag) { { if (flag) return 1; } return 2; }
            integer empty_missing(integer flag) { { } if (flag) { } }
            default { }"#,
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 2, "{diagnostics:?}");
        assert!(
            returns
                .iter()
                .all(|item| item.message.contains("jump_nested")
                    || item.message.contains("empty_missing")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn handles_many_labels_and_jumps() {
        let mut body = String::from("@label0;");
        for index in 1..=200 {
            body.push_str(&format!(" jump label{index}; @label{index};"));
        }
        body.push_str(" return 1;");
        let diagnostics = analyze_source(&format!("integer chain() {{ {body} }} default {{ }}"));
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM011"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn handles_labels_inside_control_flow() {
        let diagnostics = analyze_source(
            r#"integer loop_jump_inside(integer flag) { while (flag) { jump done; @done; return 1; } }
            integer if_jump_inside(integer flag) { if (flag) { jump done; @done; } return 1; }
            default { }"#,
        );
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function loop_jump_inside"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn evaluates_all_comparison_operators_and_composed_conditions() {
        let cases = [
            ("1 == 1", false),
            ("1 != 1", true),
            ("1 < 2", false),
            ("2 < 1", true),
            ("1 <= 1", false),
            ("2 <= 1", true),
            ("1 > 2", true),
            ("2 > 1", false),
            ("1 >= 1", false),
            ("1 >= 2", true),
            ("1 + 2 * 3 == 7", false),
            ("(1 + 2) * 3 == 7", true),
            ("1 || 1 && 0", true),
        ];
        for (expression, expected_missing) in cases {
            let source =
                format!("integer choose() {{ if ({expression}) return 1; }} default {{ }}");
            let diagnostics = analyze_source(&source);
            assert_eq!(
                diagnostics.iter().any(|item| item.code == "SEM011"),
                expected_missing,
                "{expression}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn enforces_cfg_node_and_edge_budgets_directly() {
        let mut nodes = ControlFlowGraph::default();
        for _ in 0..MAX_CFG_NODES {
            assert!(nodes.add_node().is_some());
        }
        assert!(nodes.add_node().is_none());
        assert!(nodes.limit_exceeded);

        let mut edges = ControlFlowGraph::default();
        for _ in 0..130 {
            assert!(edges.add_node().is_some());
        }
        let mut added = 0usize;
        'outer: for from in 0..130 {
            for to in 0..130 {
                if from == to {
                    continue;
                }
                if edges.add_edge(from, to) {
                    added += 1;
                } else {
                    break 'outer;
                }
            }
        }
        assert_eq!(added, MAX_CFG_EDGES);
        assert!(edges.limit_exceeded);
    }

    #[test]
    fn enforces_cfg_node_budget_and_continues() {
        let boundary = format!(
            "integer nodes() {{ {} }} default {{ }}",
            ";".repeat(MAX_CFG_NODES - 1)
        );
        assert!(
            analyze_source(&boundary)
                .iter()
                .any(|item| item.code == "SEM011"),
            "function at the node limit should still be analyzed"
        );

        let over = format!(
            "integer nodes() {{ {} }} integer small() {{ }} default {{ }}",
            ";".repeat(MAX_CFG_NODES)
        );
        let diagnostics = analyze_source(&over);
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function small"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn enforces_cfg_token_budget_boundaries() {
        let expression = |extra: usize| format!("1{}", " + 1".repeat(8188 + extra));
        let boundary = format!(
            "integer boundary() {{ if ({}) {{ }} }} default {{ }}",
            expression(0)
        );
        assert!(
            analyze_source(&boundary)
                .iter()
                .any(|item| item.code == "SEM011"),
            "function at the token limit should still be analyzed"
        );

        let over = format!(
            "integer boundary() {{ if ({}) {{ }} }} integer small() {{ }} default {{ }}",
            expression(1)
        );
        let diagnostics = analyze_source(&over);
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function small"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn enforces_cfg_depth_budget_boundaries() {
        let below = format!(
            "integer deep() {{ {}return 1;{} }} default {{ }}",
            "{".repeat(MAX_CFG_DEPTH - 1),
            "}".repeat(MAX_CFG_DEPTH - 1)
        );
        assert!(
            !analyze_source(&below)
                .iter()
                .any(|item| item.code == "SEM011"),
            "function below the depth limit should return normally"
        );

        let boundary = format!(
            "integer deep() {{ {}{} }} default {{ }}",
            "{".repeat(MAX_CFG_DEPTH),
            "}".repeat(MAX_CFG_DEPTH)
        );
        assert!(
            analyze_source(&boundary)
                .iter()
                .any(|item| item.code == "SEM011"),
            "function at the depth limit should still be analyzed"
        );

        let over = format!(
            "integer deep() {{ {}{} }} integer small() {{ }} default {{ }}",
            "{".repeat(MAX_CFG_DEPTH + 1),
            "}".repeat(MAX_CFG_DEPTH + 1)
        );
        let diagnostics = analyze_source(&over);
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function small"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn enforces_cfg_depth_budget_for_nested_if() {
        let boundary = format!(
            "integer deep_if(integer a) {{ {} }} default {{ }}",
            "if (a) {".repeat(MAX_CFG_DEPTH / 2) + &"}".repeat(MAX_CFG_DEPTH / 2)
        );
        assert!(
            analyze_source(&boundary)
                .iter()
                .any(|item| item.code == "SEM011"),
            "nested if at the depth limit should still be analyzed"
        );

        let over = format!(
            "integer deep_if(integer a) {{ {} }} integer small() {{ }} default {{ }}",
            "if (a) {".repeat(MAX_CFG_DEPTH / 2 + 1) + &"}".repeat(MAX_CFG_DEPTH / 2 + 1)
        );
        let diagnostics = analyze_source(&over);
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM011")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("function small"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn skips_malformed_cfg_without_sem011() {
        for source in [
            "integer broken() { if (1 { return 1; } default { }",
            "integer broken() { jump; } default { }",
            "integer broken() { do { } while (1; } default { }",
            "integer broken() { @; } default { }",
            "integer broken() { for (; ; ) { } } default { }",
        ] {
            let diagnostics = analyze_source(source);
            assert!(
                !diagnostics.iter().any(|item| item.code == "SEM011"),
                "{source}: {diagnostics:?}"
            );
        }
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
    fn reports_duplicate_known_event_handlers_in_a_state() {
        let diagnostics =
            analyze_source("default { state_entry() { } state_entry() { } state_entry() { } }");
        let duplicates = diagnostics
            .iter()
            .filter(|item| item.code == "SEM020")
            .collect::<Vec<_>>();
        assert_eq!(duplicates.len(), 2, "{diagnostics:?}");
        assert!(
            duplicates.iter().all(
                |item| item.message.contains("state_entry") && item.message.contains("default")
            ),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_duplicate_timer_handlers() {
        let diagnostics = analyze_source("default { timer() { } timer() { } }");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM020")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_duplicate_event_and_signature_errors_independently() {
        let diagnostics = analyze_source(
            "default { touch_start(string total) { } touch_start(string total) { } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM003")
                .count(),
            2,
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM020")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn allows_the_same_known_event_in_different_states() {
        let diagnostics =
            analyze_source("default { state_entry() { } } state running { state_entry() { } }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM020"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn ignores_duplicate_unknown_extension_events() {
        let diagnostics = analyze_source("default { custom_event() { } custom_event() { } }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM020"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_duplicate_named_states_but_not_default() {
        let diagnostics =
            analyze_source("default { } state running { } state running { } state running { }");
        let duplicates = diagnostics
            .iter()
            .filter(|item| item.code == "SEM021")
            .collect::<Vec<_>>();
        assert_eq!(duplicates.len(), 2, "{diagnostics:?}");
        assert!(
            duplicates
                .iter()
                .all(|item| item.message.contains("running")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_state_namespace_collisions_with_globals_and_functions() {
        let diagnostics = analyze_source(
            "integer running; integer idle() { return 1; } default { } state running { } state idle { }",
        );
        let collisions = diagnostics
            .iter()
            .filter(|item| item.code == "SEM021")
            .collect::<Vec<_>>();
        assert_eq!(collisions.len(), 2, "{diagnostics:?}");
        assert!(
            collisions
                .iter()
                .any(|item| item.message.contains("running")
                    && item.message.contains("global variable")),
            "{diagnostics:?}"
        );
        assert!(
            collisions
                .iter()
                .any(|item| item.message.contains("idle") && item.message.contains("function")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_undefined_state_change_targets_in_event_handlers() {
        let diagnostics = analyze_source("default { state_entry() { state missing; } }");
        let missing = diagnostics
            .iter()
            .filter(|item| item.code == "SEM022")
            .collect::<Vec<_>>();
        assert_eq!(missing.len(), 1, "{diagnostics:?}");
        assert!(missing[0].message.contains("missing"), "{diagnostics:?}");
    }

    #[test]
    fn accepts_declared_and_default_state_targets() {
        let diagnostics = analyze_source(
            "default { state_entry() { state running; } } state running { state_entry() { state default; } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM022"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn keeps_function_state_changes_and_unknown_event_bodies_out_of_scope() {
        let diagnostics = analyze_source(
            "change() { state missing; } default { custom_event() { state also_missing; } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM022"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn ignores_type_tokens_inside_global_function_parameters() {
        let diagnostics = analyze_source(
            "integer first(integer value) { return value; } float second(float amount) { return amount; }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn does_not_treat_parenthesized_comparison_as_vector_literal() {
        let diagnostics = analyze_source(
            "default { state_entry() { integer left; string text = (left) < 2, other = 3.5; } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM015")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 2, "{diagnostics:?}");
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("'text'") && item.message.contains("integer, expected string")
            }),
            "{diagnostics:?}"
        );
        assert!(
            mismatches.iter().any(|item| {
                item.message.contains("'other'") && item.message.contains("float, expected string")
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn does_not_treat_call_result_comparison_as_vector_literal() {
        let diagnostics = analyze_source(
            "default { state_entry() { string text; text = llGetListLength([]) < 1; } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM016")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(
            mismatches[0].message.contains("integer, expected string"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn does_not_count_call_arguments_past_parenthesized_comparison() {
        let diagnostics = analyze_source(
            "default { state_entry() { integer left; llOwnerSay((left) < 2, \"second\"); } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM002")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn keeps_vector_literal_after_explicit_cast_whole() {
        let (tokens, lexical_diagnostics) = lex(
            "default { state_entry() { vector first = (vector)<1, 2, 3>, second = <4, 5, 6>; } }",
        );
        assert!(lexical_diagnostics.is_empty(), "{lexical_diagnostics:?}");

        let scopes = ScopeModel::build(&tokens);
        assert_eq!(scopes.initializers.len(), 2);
        assert_eq!(
            tokens[scopes.initializers[0].expression_start].kind,
            TokenKind::LParen
        );
        assert_eq!(
            tokens[scopes.initializers[1].expression_start].kind,
            TokenKind::Less
        );
        for initializer in &scopes.initializers {
            assert_eq!(
                tokens[initializer.expression_end - 1].kind,
                TokenKind::Greater
            );
        }
        assert_eq!(
            tokens[scopes.initializers[0].expression_end].kind,
            TokenKind::Comma
        );
        assert_eq!(
            tokens[scopes.initializers[1].expression_end].kind,
            TokenKind::Semicolon
        );
    }

    #[test]
    fn reports_compound_assignment_with_direct_vector_literal_rhs() {
        for operator in ["+=", "-=", "*=", "/=", "%="] {
            let source = format!(
                "default {{ state_entry() {{ integer count; count {operator} <1, 2, 3>; }} }}"
            );
            let diagnostics = analyze_source(&source);
            let mismatches = diagnostics
                .iter()
                .filter(|item| item.code == "SEM017")
                .collect::<Vec<_>>();
            assert_eq!(mismatches.len(), 1, "{operator}: {diagnostics:?}");
            let expected = if operator == "*=" {
                "yields type vector, expected integer"
            } else {
                "integer and vector"
            };
            assert!(
                mismatches[0].message.contains(operator)
                    && mismatches[0].message.contains(expected),
                "{operator}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn reports_compound_assignment_vector_literal_in_for_increment() {
        let diagnostics = analyze_source(
            "default { state_entry() { integer i; integer count; for (i = 0; i < 1; count += <1, 2, 3>) { } } }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM017")
                .count(),
            1,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_valid_vector_compound_assignment_with_direct_literal() {
        let diagnostics =
            analyze_source("default { state_entry() { vector offset; offset += <1, 2, 3>; } }");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn accepts_vector_function_returning_direct_vector_literal() {
        let diagnostics = analyze_source("vector make() { return <1, 2, 3>; } default { }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_integer_function_returning_direct_vector_literal() {
        let diagnostics = analyze_source("integer make() { return <1, 2, 3>; } default { }");
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM008")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("type vector"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_rotation_function_returning_direct_rotation_literal() {
        let diagnostics = analyze_source("rotation make() { return <1, 2, 3, 4>; } default { }");
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_vector_function_returning_direct_rotation_literal() {
        let diagnostics = analyze_source("vector make() { return <1, 2, 3, 4>; } default { }");
        let returns = diagnostics
            .iter()
            .filter(|item| item.code == "SEM008")
            .collect::<Vec<_>>();
        assert_eq!(returns.len(), 1, "{diagnostics:?}");
        assert!(
            returns[0].message.contains("type rotation"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_grouped_and_cast_vector_and_rotation_returns() {
        let diagnostics = analyze_source(
            "vector grouped() { return (<1, 2, 3>); } \
             vector cast() { return (vector)<1, 2, 3>; } \
             rotation rotation_cast() { return (rotation)<1, 2, 3, 4>; } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_incompatible_grouped_and_cast_vector_returns() {
        let diagnostics = analyze_source(
            "integer grouped() { return (<1, 2, 3>); } \
             integer cast() { return (vector)<1, 2, 3>; } default { }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM008")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_arithmetic_expressions_starting_with_vector_literals() {
        let diagnostics = analyze_source(
            "vector add() { return <1, 2, 3> + <4, 5, 6>; } \
             vector scale() { return <1, 2, 3> * 2.0; } \
             float dot() { return <1, 2, 3> * <4, 5, 6>; } \
             rotation compose() { return <1, 2, 3, 4> * <5, 6, 7, 8>; } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_incompatible_arithmetic_expressions_starting_with_vector_literals() {
        let diagnostics = analyze_source(
            "integer add() { return <1, 2, 3> + <4, 5, 6>; } \
             vector dot() { return <1, 2, 3> * <4, 5, 6>; } default { }",
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|item| item.code == "SEM008")
                .count(),
            2,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_assignment_type_mismatch_after_vector_equality_literal() {
        let diagnostics = analyze_source(
            "default { state_entry() { vector left; string text; text = left == <1, 2, 3>; } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM016")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(
            mismatches[0].message.contains("integer, expected string"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_integer_assignment_from_vector_equality_literal() {
        let diagnostics = analyze_source(
            "default { state_entry() { vector left; integer result; result = left == <1, 2, 3>; } }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM016"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_initializer_type_mismatch_after_rotation_inequality_literal() {
        let diagnostics = analyze_source(
            "default { state_entry() { rotation left; string text = left != <1, 2, 3, 4>; } }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM015")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(
            mismatches[0].message.contains("integer, expected string"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn reports_return_type_mismatch_after_vector_equality_literal() {
        let diagnostics = analyze_source(
            "string compare() { vector left; return left == <1, 2, 3>; } default { }",
        );
        let mismatches = diagnostics
            .iter()
            .filter(|item| item.code == "SEM008")
            .collect::<Vec<_>>();
        assert_eq!(mismatches.len(), 1, "{diagnostics:?}");
        assert!(
            mismatches[0].message.contains("type integer"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_integer_return_from_vector_equality_literal() {
        let diagnostics = analyze_source(
            "integer compare() { vector left; return left == <1, 2, 3>; } default { }",
        );
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM008"),
            "{diagnostics:?}"
        );
    }
}
