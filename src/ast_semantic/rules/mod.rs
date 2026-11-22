//! Focused structural SEM rule implementations for the AST engine.

pub(crate) mod assignments;
pub(crate) mod builtin_calls;
pub(crate) mod components;
pub(crate) mod deprecated_calls;
pub(crate) mod duplicate_events;
pub(crate) mod duplicate_globals;
pub(crate) mod duplicate_locals;
pub(crate) mod event_signatures;
pub(crate) mod explicit_casts;
pub(crate) mod state_namespace;
pub(crate) mod unresolved_variables;
pub(crate) mod user_calls;
pub(crate) mod variable_initializers;

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
