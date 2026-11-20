use std::fmt;

/// Zero-based byte offsets and one-based human-readable line/column positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Lexical,
    Syntax,
    Semantic,
    Lint,
}

impl DiagnosticKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Syntax => "syntax",
            Self::Semantic => "semantic",
            Self::Lint => "lint",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
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
