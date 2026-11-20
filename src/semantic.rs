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
            | TokenKind::KwVoid
    )
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

        if brace_depth != 0 || paren_depth != 0 || !is_declaration_type(token.kind) {
            continue;
        }

        let Some(name) = tokens.get(index + 1) else {
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

    analyze_builtin_calls(tokens, &mut diagnostics);
    analyze_event_signatures(tokens, &mut diagnostics);
    diagnostics
}

fn analyze_builtin_calls(tokens: &[Token], diagnostics: &mut Vec<Diagnostic>) {
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
            let Some(actual_type) = infer_expression_type(tokens, range.0, range.1) else {
                // Variables and expressions without a provable type are left
                // alone until full scope-aware type inference is available.
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

fn infer_expression_type(tokens: &[Token], mut start: usize, mut end: usize) -> Option<String> {
    if start >= end {
        return None;
    }

    // Strip grouping parentheses while preserving cast syntax such as
    // (string)expression.
    loop {
        if start + 2 < end
            && tokens[start].kind == TokenKind::LParen
            && tokens[start + 2].kind == TokenKind::RParen
        {
            if let Some(type_name) = lsl_type_name(tokens[start + 1].kind) {
                return Some(type_name.to_owned());
            }
        }

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
        return infer_expression_type(tokens, start + 1, end);
    }
    if matches!(tokens[start].kind, TokenKind::Bang | TokenKind::Tilde) {
        return Some("integer".to_owned());
    }

    if start + 1 == end {
        let token = &tokens[start];
        return match token.kind {
            TokenKind::IntegerLiteral => Some("integer".to_owned()),
            TokenKind::FloatLiteral => Some("float".to_owned()),
            TokenKind::StringLiteral => Some("string".to_owned()),
            TokenKind::Identifier => definitions()
                .constants
                .get(token.lexeme.as_str())
                .map(|constant| constant.type_name.clone()),
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
    fn leaves_unknown_variable_types_unchecked() {
        let diagnostics =
            analyze_source(r#"default { state_entry() { integer amount; llOwnerSay(amount); } }"#);
        assert!(
            !diagnostics.iter().any(|item| item.code == "SEM004"),
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
