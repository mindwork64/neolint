//! Preprocessor configuration, entry points, and prepared source.
//!
//! A prepared unit owns exactly one [`PreparedSource`] and one matching
//! [`SourceMap`]. The prepared buffer is not a
//! source file and has no `SourceId`; its bytes are addressed only by
//! [`PreparedSpan`].

pub mod config;
pub mod diagnostics;
pub mod identity;

pub(crate) mod directive;
pub(crate) mod expand;
pub(crate) mod expr;
pub(crate) mod macros;
pub(crate) mod token;

use std::sync::Arc;

use crate::diagnostic::Span;
use crate::source::file::AnalysisUnitId;
use crate::source::map::{PreparedSpan, SourceMap, SpanError};

pub use config::{PreprocessConfig, PreprocessMode, PreprocessOptions};
pub use diagnostics::{PreprocessDiagnostic, RelatedLocation};
pub use identity::preprocess_identity;

/// Preprocess a loaded root into a prepared source.
///
/// Identity mode delegates to [`identity::preprocess_identity`]. Full mode runs
/// the Phase 2 directive and macro engine. User-code findings are returned as
/// `PreprocessDiagnostic`s on the prepared source; malformed internal
/// coordinates or map construction fail with a Phase 1 [`SpanError`].
pub fn preprocess(
    manager: &crate::source::manager::SourceManager,
    root: crate::source::SourceId,
    config: impl Into<PreprocessOptions>,
) -> Result<PreparedSource, SpanError> {
    match config.into().mode() {
        PreprocessMode::Identity => identity::preprocess_identity(manager, root),
        PreprocessMode::Full => directive::preprocess_full(manager, root),
    }
}

/// Immutable prepared buffer plus its mapping and preprocessor findings.
#[derive(Debug)]
pub struct PreparedSource {
    unit: AnalysisUnitId,
    text: Arc<str>,
    map: Arc<SourceMap>,
    preprocess_diagnostics: Vec<PreprocessDiagnostic>,
}

impl PreparedSource {
    pub(crate) fn new(
        unit: AnalysisUnitId,
        text: Arc<str>,
        map: Arc<SourceMap>,
        preprocess_diagnostics: Vec<PreprocessDiagnostic>,
    ) -> Self {
        Self {
            unit,
            text,
            map,
            preprocess_diagnostics,
        }
    }

    /// Unit that owns this prepared buffer.
    pub fn unit(&self) -> AnalysisUnitId {
        self.unit
    }

    /// Prepared text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Shared prepared text.
    pub fn shared_text(&self) -> &Arc<str> {
        &self.text
    }

    /// Prepared length in UTF-8 bytes.
    pub fn prepared_len(&self) -> usize {
        self.text.len()
    }

    /// Matching source map.
    pub fn map(&self) -> &SourceMap {
        &self.map
    }

    /// Shared matching source map.
    pub fn shared_map(&self) -> &Arc<SourceMap> {
        &self.map
    }

    /// Preprocessor findings (empty in identity mode).
    pub fn preprocess_diagnostics(&self) -> &[PreprocessDiagnostic] {
        &self.preprocess_diagnostics
    }

    /// The only public `Span -> PreparedSpan` adapter.
    ///
    /// `Span` carries no source identity, so this validates the byte range and
    /// stamps this source's unit; it cannot prove that the span was produced by
    /// this particular prepared buffer. Engines must only be given the
    /// `PreparedSource` they lexed.
    pub fn prepared_span(&self, span: Span) -> Result<PreparedSpan, SpanError> {
        if span.start > span.end {
            return Err(SpanError::InvertedRange);
        }
        if span.end > self.text.len() {
            return Err(SpanError::OutOfBounds);
        }
        if !self.text.is_char_boundary(span.start) || !self.text.is_char_boundary(span.end) {
            return Err(SpanError::NotCharBoundary);
        }
        Ok(PreparedSpan::new(self.unit, span.start, span.end))
    }

    /// A zero-length prepared point at the end of the buffer.
    pub fn eof_span(&self) -> PreparedSpan {
        PreparedSpan::new(self.unit, self.text.len(), self.text.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::file::RunId;
    use crate::source::manager::SourceManager;

    fn prepared(text: &str, index: u32) -> PreparedSource {
        let unit = AnalysisUnitId::new(RunId::new(1), index);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        identity::preprocess_identity(&manager, root).unwrap()
    }

    #[test]
    fn identity_text_is_byte_identical_and_stamped_with_unit() {
        let source = "default {\r\n  state_entry() { }\r\n}\r\n";
        let prepared = prepared(source, 0);
        assert_eq!(prepared.text(), source);
        assert_eq!(prepared.prepared_len(), source.len());
        assert!(prepared.preprocess_diagnostics().is_empty());
        assert_eq!(prepared.eof_span().start(), source.len());
        assert_eq!(prepared.eof_span().end(), source.len());
        assert_eq!(prepared.eof_span().unit(), prepared.unit());
    }

    #[test]
    fn adapter_validates_range_and_ignores_display_metadata() {
        let prepared = prepared("aé", 0);
        let span = crate::diagnostic::Span {
            start: 1,
            end: 3,
            line: 99,
            column: 99,
            end_line: 99,
            end_column: 99,
        };
        let mapped = prepared.prepared_span(span).unwrap();
        assert_eq!((mapped.start(), mapped.end()), (1, 3));
        assert_eq!(mapped.unit(), prepared.unit());
        assert_eq!(
            prepared.prepared_span(crate::diagnostic::Span {
                start: 1,
                end: 2,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 2,
            }),
            Err(SpanError::NotCharBoundary)
        );
        assert_eq!(
            prepared.prepared_span(crate::diagnostic::Span {
                start: 2,
                end: 1,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 2,
            }),
            Err(SpanError::InvertedRange)
        );
        assert_eq!(
            prepared.prepared_span(crate::diagnostic::Span {
                start: 0,
                end: 4,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 4,
            }),
            Err(SpanError::OutOfBounds)
        );
    }

    #[test]
    fn empty_identity_prepares_empty_buffer_and_eof_point() {
        let prepared = prepared("", 0);
        assert_eq!(prepared.text(), "");
        assert_eq!(prepared.prepared_len(), 0);
        let eof = prepared.eof_span();
        assert_eq!((eof.start(), eof.end()), (0, 0));
        assert_eq!(
            prepared.map().resolve_point(eof).unwrap(),
            prepared.map().eof_anchor()
        );
    }
}
