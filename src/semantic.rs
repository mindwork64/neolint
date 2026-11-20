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

    diagnostics
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
        let diagnostics = analyze_source(
            "integer calculate() { return 1; } integer calculate() { return 2; }",
        );
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
    fn ignores_type_tokens_inside_global_function_parameters() {
        let diagnostics = analyze_source(
            "integer first(integer value) { return value; } float second(float amount) { return amount; }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
