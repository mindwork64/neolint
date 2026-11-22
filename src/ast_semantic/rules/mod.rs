//! Focused structural SEM rule implementations for the AST engine.

pub(crate) mod duplicate_events;
pub(crate) mod duplicate_globals;
pub(crate) mod duplicate_locals;
pub(crate) mod state_namespace;

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};

pub(crate) fn semantic_error(code: &'static str, message: String, span: Span) -> Diagnostic {
    Diagnostic {
        code,
        severity: Severity::Error,
        kind: DiagnosticKind::Semantic,
        message,
        span,
    }
}
