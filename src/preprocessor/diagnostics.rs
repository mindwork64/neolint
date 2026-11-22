//! Additive preprocessor diagnostic types.
//!
//! Preprocessor findings are born in original coordinates and therefore never
//! pass through prepared-span resolution.

use crate::diagnostic::{DiagnosticKind, Severity};
use crate::source::map::SourceSpan;

/// One secondary location attached to a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedLocation {
    /// Original-file span.
    pub span: SourceSpan,
    /// Human-readable note label.
    pub label: String,
}

impl RelatedLocation {
    /// Construct a related location.
    pub fn new(span: SourceSpan, label: impl Into<String>) -> Self {
        Self {
            span,
            label: label.into(),
        }
    }
}

/// A preprocessor finding with a direct original-file location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessDiagnostic {
    /// Stable diagnostic code, for example `PRE001`.
    pub code: &'static str,
    /// Effective severity.
    pub severity: Severity,
    /// Origin family.
    pub kind: DiagnosticKind,
    /// Human-readable message.
    pub message: String,
    /// Primary original-file span.
    pub primary: SourceSpan,
    /// Secondary original-file spans.
    pub related: Vec<RelatedLocation>,
}

impl PreprocessDiagnostic {
    /// Construct a preprocessor diagnostic.
    pub fn new(
        code: &'static str,
        severity: Severity,
        message: impl Into<String>,
        primary: SourceSpan,
    ) -> Self {
        Self {
            code,
            severity,
            kind: DiagnosticKind::Syntax,
            message: message.into(),
            primary,
            related: Vec::new(),
        }
    }
}
