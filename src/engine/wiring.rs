//! Private parser-diagnostic separation for the engine dispatch.
//!
//! Parser findings are engine-local: Legacy semantic analysis must receive only
//! Legacy parser diagnostics and AST semantic analysis must receive only AST
//! parser diagnostics. Both analyzers accept `&[Diagnostic]`, so the compiler
//! cannot catch a swapped slice at those public boundaries. The private
//! [`LegacySyntax`] and [`AstSyntax`] newtypes and their narrow analyzer helpers
//! make such a swap a type error inside the engine.

use crate::ast::{Ast, AstParseOptions};
use crate::diagnostic::Diagnostic;
use crate::lexer::Token;

/// Legacy parser diagnostics, only consumable by the Legacy semantic pass.
pub(crate) struct LegacySyntax(Vec<Diagnostic>);

/// AST parser diagnostics, only consumable by the AST semantic pass.
pub(crate) struct AstSyntax(Vec<Diagnostic>);

impl LegacySyntax {
    /// Run the frozen Legacy parser once over the shared token slice.
    pub(crate) fn parse(tokens: &[Token]) -> Self {
        Self(crate::parse_legacy(tokens))
    }

    /// Borrow the raw parser findings for display rendering.
    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[Diagnostic] {
        &self.0
    }

    /// Consume the raw parser findings for display rendering.
    pub(crate) fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.0
    }
}

impl AstSyntax {
    /// Run the independent AST parser once with the prepared-source gates.
    pub(crate) fn parse(tokens: &[Token], options: AstParseOptions) -> (Ast, Self) {
        let (ast, diagnostics) = crate::ast::parse_ast_with_options(tokens, options);
        (ast, Self(diagnostics))
    }

    /// Consume the raw parser findings for display rendering.
    pub(crate) fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.0
    }
}

#[cfg(test)]
impl LegacySyntax {
    /// Test-only sentinel stream for behavioral separation tests.
    pub(crate) fn from_diagnostics(diagnostics: Vec<Diagnostic>) -> Self {
        Self(diagnostics)
    }
}

#[cfg(test)]
impl AstSyntax {
    /// Test-only sentinel stream for behavioral separation tests.
    pub(crate) fn from_diagnostics(diagnostics: Vec<Diagnostic>) -> Self {
        Self(diagnostics)
    }
}

/// Run the Legacy semantic analyzer with its own parser findings.
pub(crate) fn analyze_legacy(tokens: &[Token], syntax: &LegacySyntax) -> Vec<Diagnostic> {
    crate::semantic::analyze_with_syntax_diagnostics(tokens, &syntax.0)
}

/// Run the AST semantic analyzer with its own parser findings.
pub(crate) fn analyze_ast(ast: &Ast, syntax: &AstSyntax) -> Vec<Diagnostic> {
    crate::ast_semantic::analyze_ast(ast, &syntax.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No repository input currently makes cross-wiring observable for
    /// SEM001..=SEM023: both parsers honor the same suppression contract on
    /// suppression-relevant starts. The guarantee is therefore enforced by the
    /// private newtypes plus the two narrow helpers above (a swap is a type
    /// error), not by a discriminating fixture.
    #[test]
    fn corrected_slices_reproduce_both_semantic_streams_exactly() {
        let source =
            "integer g;\ninteger g;\ndefault { state_entry() { integer x = ; llSay(0); } }";
        let (tokens, lexical) = crate::lex(source);
        assert!(lexical.is_empty());

        let legacy_syntax = LegacySyntax::parse(&tokens);
        let expected_legacy =
            crate::semantic::analyze_with_syntax_diagnostics(&tokens, legacy_syntax.as_slice());
        let actual_legacy = analyze_legacy(&tokens, &legacy_syntax);
        assert_eq!(actual_legacy, expected_legacy);

        let (ast, ast_syntax) = AstSyntax::parse(&tokens, AstParseOptions::lsl());
        let expected_ast = crate::ast_semantic::analyze_ast(&ast, &ast_syntax.0);
        let actual_ast = analyze_ast(&ast, &ast_syntax);
        assert_eq!(actual_ast, expected_ast);

        // The report is built from these two raw streams by the comparator.
        let report = crate::engine::compare_semantic_diagnostics(&actual_legacy, &actual_ast);
        assert_eq!(report.by_rule.len(), crate::engine::SEM_CODES.len());
    }

    /// Each semantic analyzer consumes only its own private parser-finding
    /// wrapper. A swapped wrapper is a type error; this test additionally pins
    /// the behavior by placing a sentinel diagnostic inside the call argument
    /// list, where suppression is observable.
    #[test]
    fn sentinel_syntax_streams_suppress_only_their_own_analyzer() {
        let source = "default { state_entry() { llSay(0); } }";
        let (tokens, lexical) = crate::lex(source);
        assert!(lexical.is_empty());
        let argument = tokens
            .iter()
            .find(|token| token.lexeme == "0")
            .expect("integer argument token");
        let sentinel = Diagnostic {
            code: "PAR999",
            severity: crate::diagnostic::Severity::Error,
            kind: crate::diagnostic::DiagnosticKind::Syntax,
            message: "sentinel".to_owned(),
            span: argument.span,
        };
        let has_sem002 = |diagnostics: &[Diagnostic]| {
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "SEM002")
        };

        let legacy_empty = LegacySyntax::from_diagnostics(Vec::new());
        assert!(has_sem002(&analyze_legacy(&tokens, &legacy_empty)));
        let legacy_sentinel = LegacySyntax::from_diagnostics(vec![sentinel.clone()]);
        assert!(!has_sem002(&analyze_legacy(&tokens, &legacy_sentinel)));

        let (ast, _) = AstSyntax::parse(&tokens, AstParseOptions::lsl());
        let ast_empty = AstSyntax::from_diagnostics(Vec::new());
        assert!(has_sem002(&analyze_ast(&ast, &ast_empty)));
        let ast_sentinel = AstSyntax::from_diagnostics(vec![sentinel]);
        assert!(!has_sem002(&analyze_ast(&ast, &ast_sentinel)));
    }
}
