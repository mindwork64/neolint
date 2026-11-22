//! Stable diagnostic data model shared by all analysis stages.
//!
//! A [`Diagnostic`] couples a stable code and severity with a [`Span`] and a
//! source message. Spans carry zero-based UTF-8 byte offsets plus one-based
//! line/column coordinates. The model is serialization-agnostic; the CLI renders
//! it as human-readable text or JSON.
use std::fmt;

pub mod resolve;

/// Zero-based byte offsets and one-based human-readable line/column positions.
/// Source range and position metadata for a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Zero-based UTF-8 byte offset of the start.
    pub start: usize,
    /// Zero-based UTF-8 byte offset of the end (exclusive).
    pub end: usize,
    /// One-based start line.
    pub line: usize,
    /// One-based start column.
    pub column: usize,
    /// One-based end line.
    pub end_line: usize,
    /// One-based end column.
    pub end_column: usize,
}

/// Diagnostic severity used by configuration and CLI exit-code decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A correctness violation; causes exit code 1 unless disabled/downgraded.
    Error,
    /// A likely issue that does not fail the run by default.
    Warning,
    /// Informational output that does not affect the exit code.
    Info,
}

impl Severity {
    /// Return the stable lowercase name used in human and JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// Origin family of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// Lexer error.
    Lexical,
    /// Parser/syntax error.
    Syntax,
    /// Semantic correctness check.
    Semantic,
    /// Style or advisory lint check.
    Lint,
}

impl DiagnosticKind {
    /// Return the stable lowercase kind used in JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Syntax => "syntax",
            Self::Semantic => "semantic",
            Self::Lint => "lint",
        }
    }
}

/// One stable linter finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable code such as `SEM004`, `PAR007`, or `LEX001`.
    pub code: &'static str,
    /// Effective severity after configuration.
    pub severity: Severity,
    /// Origin family of the finding.
    pub kind: DiagnosticKind,
    /// Human-readable explanation.
    pub message: String,
    /// Source range associated with the finding.
    pub span: Span,
}

impl Diagnostic {
    /// Construct an error-level lexical diagnostic.
    ///
    /// # Arguments
    ///
    /// * `code` — stable `LEXxxx` code.
    /// * `message` — human-readable explanation.
    /// * `span` — source range for the offending text.
    pub fn lexical(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: Severity::Error,
            kind: DiagnosticKind::Lexical,
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}[{}]: {}",
            self.span.line,
            self.span.column,
            self.severity.as_str(),
            self.code,
            self.message
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_display_includes_location_code_and_message() {
        let diagnostic = Diagnostic::lexical(
            "LEX001",
            "unexpected character",
            Span {
                start: 4,
                end: 5,
                line: 2,
                column: 3,
                end_line: 2,
                end_column: 4,
            },
        );

        assert_eq!(
            diagnostic.to_string(),
            "2:3: error[LEX001]: unexpected character"
        );
    }
}
