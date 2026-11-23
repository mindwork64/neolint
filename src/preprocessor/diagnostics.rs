//! Additive preprocessor diagnostic types.
//!
//! Preprocessor findings are born in original coordinates and therefore never
//! pass through prepared-span resolution.

use crate::diagnostic::{DiagnosticKind, Severity};
use crate::preprocessor::token::RawSpan;
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

/// Maximum number of PRE diagnostics per preprocessing run.
pub(crate) const MAX_PREPROCESS_DIAGNOSTICS: usize = 1_000;
/// Total rendered diagnostics retained per AnalysisUnit.
pub const MAX_TOTAL_DIAGNOSTICS: usize = 10_000;

pub(crate) const PRE_UNKNOWN_DIRECTIVE: &str = "PRE001";
pub(crate) const PRE_MALFORMED_DIRECTIVE: &str = "PRE002";
pub(crate) const PRE_INVALID_DEFINE: &str = "PRE003";
pub(crate) const PRE_INVALID_UNDEF: &str = "PRE004";
pub(crate) const PRE_CONFLICTING_REDEFINITION: &str = "PRE005";
pub(crate) const PRE_INVALID_IF_EXPRESSION: &str = "PRE006";
pub(crate) const PRE_INVALID_IFDEF: &str = "PRE007";
pub(crate) const PRE_DUPLICATE_ELSE: &str = "PRE008";
pub(crate) const PRE_ELIF_AFTER_ELSE: &str = "PRE009";
pub(crate) const PRE_ELIF_WITHOUT_IF: &str = "PRE010";
pub(crate) const PRE_ELSE_WITHOUT_IF: &str = "PRE011";
pub(crate) const PRE_ENDIF_WITHOUT_IF: &str = "PRE012";
pub(crate) const PRE_UNTERMINATED_CONDITIONAL: &str = "PRE013";
pub(crate) const PRE_MACRO_ARGUMENT_COUNT: &str = "PRE014";
pub(crate) const PRE_UNTERMINATED_MACRO_CALL: &str = "PRE015";
pub(crate) const PRE_DIRECT_RECURSION: &str = "PRE017";
pub(crate) const PRE_INDIRECT_RECURSION: &str = "PRE018";
pub(crate) const PRE_EXPANSION_DEPTH: &str = "PRE019";
pub(crate) const PRE_EXPANDED_BYTES_LIMIT: &str = "PRE020";
pub(crate) const PRE_EXPANDED_TOKENS_LIMIT: &str = "PRE021";
pub(crate) const PRE_DIAGNOSTIC_LIMIT: &str = "PRE022";
pub(crate) const PRE_CONDITIONAL_DEPTH: &str = "PRE025";
pub(crate) const PRE_DIVISION_BY_ZERO: &str = "PRE026";
pub(crate) const PRE_EXPRESSION_OVERFLOW: &str = "PRE027";
pub(crate) const PRE_INVALID_SHIFT: &str = "PRE028";
pub(crate) const PRE_MALFORMED_LITERAL: &str = "PRE029";
pub(crate) const PRE_MISSING_OPERAND: &str = "PRE030";
pub(crate) const PRE_INVALID_INCLUDE: &str = "PRE031";
pub(crate) const PRE_INCLUDE_NOT_FOUND: &str = "PRE032";
pub(crate) const PRE_INCLUDE_CYCLE: &str = "PRE033";
pub(crate) const PRE_INCLUDE_DEPTH: &str = "PRE034";
pub(crate) const PRE_INCLUDE_LIMIT: &str = "PRE035";
pub(crate) const PRE_UNKNOWN_PRAGMA: &str = "PRE036";
pub(crate) const PRE_FIRESTORM_DISABLED: &str = "PRE037";
pub(crate) const PRE_INVALID_MACRO_OPERATOR: &str = "PRE038";
pub(crate) const PRE_DIAGNOSTIC_OVERFLOW: &str = "PRE039";

#[derive(Debug, Clone)]
pub(crate) struct RawRelated {
    pub(crate) span: RawSpan,
    pub(crate) label: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct RawDiagnostic {
    pub(crate) code: &'static str,
    pub(crate) severity: Severity,
    pub(crate) message: String,
    pub(crate) primary: RawSpan,
    pub(crate) related: Vec<RawRelated>,
}

/// Deterministic, capped diagnostic collector.
#[derive(Debug, Default)]
pub(crate) struct DiagSink {
    items: Vec<RawDiagnostic>,
    capped: bool,
}

impl DiagSink {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn items(&self) -> &[RawDiagnostic] {
        &self.items
    }

    pub(crate) fn push(
        &mut self,
        code: &'static str,
        message: impl Into<String>,
        primary: RawSpan,
    ) {
        self.push_with_related(code, message, primary, Vec::new());
    }

    pub(crate) fn push_with_related(
        &mut self,
        code: &'static str,
        message: impl Into<String>,
        primary: RawSpan,
        related: Vec<RawRelated>,
    ) {
        if self.capped {
            return;
        }
        if self.items.len() >= MAX_PREPROCESS_DIAGNOSTICS - 1 {
            self.items.push(RawDiagnostic {
                code: PRE_DIAGNOSTIC_LIMIT,
                severity: Severity::Error,
                message: format!(
                    "preprocessor diagnostic limit ({MAX_PREPROCESS_DIAGNOSTICS}) reached"
                ),
                primary,
                related,
            });
            self.capped = true;
            return;
        }
        self.items.push(RawDiagnostic {
            code,
            severity: Severity::Error,
            message: message.into(),
            primary,
            related,
        });
    }

    pub(crate) fn extend_related(&mut self, index: usize, span: RawSpan, label: &'static str) {
        if let Some(item) = self.items.get_mut(index) {
            item.related.push(RawRelated { span, label });
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
}

pub(crate) fn cap_rendered(
    prepared: &crate::preprocessor::PreparedSource,
    rendered: &mut Vec<crate::diagnostic::resolve::RenderedDiagnostic>,
) -> Result<(), crate::source::map::SpanError> {
    cap_rendered_with(prepared, rendered, |_| true)
}

/// Drop diagnostics rejected by `keep` in place, then apply the rendered cap.
///
/// The predicate runs before the limit check so display-suppressed findings
/// cannot trigger [`PRE_DIAGNOSTIC_OVERFLOW`]. `retain` compacts the existing
/// vector; no second diagnostic collection is allocated.
pub(crate) fn cap_rendered_with(
    prepared: &crate::preprocessor::PreparedSource,
    rendered: &mut Vec<crate::diagnostic::resolve::RenderedDiagnostic>,
    mut keep: impl FnMut(&crate::diagnostic::resolve::RenderedDiagnostic) -> bool,
) -> Result<(), crate::source::map::SpanError> {
    use crate::diagnostic::resolve::RenderedDiagnostic;
    rendered.retain(&mut keep);
    if rendered.len() <= MAX_TOTAL_DIAGNOSTICS {
        return Ok(());
    }
    rendered.truncate(MAX_TOTAL_DIAGNOSTICS - 1);
    let diagnostic = PreprocessDiagnostic {
        code: PRE_DIAGNOSTIC_OVERFLOW,
        severity: Severity::Error,
        kind: DiagnosticKind::Syntax,
        message: format!("total diagnostic limit ({MAX_TOTAL_DIAGNOSTICS}) reached"),
        primary: prepared.map().eof_anchor(),
        related: Vec::new(),
    };
    rendered.push(RenderedDiagnostic::from_preprocess(diagnostic, prepared)?);
    Ok(())
}
