//! Unified diagnostic conversion with strict analysis-unit ownership.
//!
//! Resolution failures are represented by [`SpanError`]. A malformed or foreign
//! coordinate is returned to the caller; it is never silently dropped, and no
//! local file is substituted.

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
use crate::preprocessor::{PreprocessDiagnostic, RelatedLocation};
use crate::source::file::{AnalysisUnitId, SourceId};
use crate::source::map::{PreparedSpan, SourceMap, SpanError};
use crate::source::unit::AnalysisUnit;

/// A frozen legacy diagnostic tagged with the unit that produced it.
///
/// Fields are private and there is no public constructor. The unit pipeline
/// creates it through [`AnalysisUnit::legacy_diagnostic`].
#[derive(Debug, Clone)]
pub struct LegacyDiagnostic {
    diagnostic: Diagnostic,
    unit: AnalysisUnitId,
}

impl LegacyDiagnostic {
    pub(crate) fn new(diagnostic: Diagnostic, unit: AnalysisUnitId) -> Self {
        Self { diagnostic, unit }
    }

    /// Frozen diagnostic value.
    pub fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }

    /// Unit that produced the diagnostic.
    pub fn unit(&self) -> AnalysisUnitId {
        self.unit
    }
}

/// An engine diagnostic carrying a prepared-coordinate `Span`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDiagnostic {
    /// Stable diagnostic code.
    pub code: &'static str,
    /// Effective severity.
    pub severity: Severity,
    /// Origin family.
    pub kind: DiagnosticKind,
    /// Human-readable message.
    pub message: String,
    /// Prepared-coordinate span, exactly the frozen `Span` shape.
    pub span: Span,
    /// Additional prepared ranges.
    pub related: Vec<PreparedSpan>,
}

impl PreparedDiagnostic {
    /// Construct a prepared diagnostic.
    pub fn new(
        code: &'static str,
        severity: Severity,
        kind: DiagnosticKind,
        message: impl Into<String>,
        span: Span,
    ) -> Self {
        Self {
            code,
            severity,
            kind,
            message: message.into(),
            span,
            related: Vec::new(),
        }
    }
}

/// Everything the unified layer accepts.
#[derive(Debug, Clone)]
pub enum UnifiedDiagnosticInput {
    /// Preprocessor finding in original coordinates.
    Preprocess(PreprocessDiagnostic),
    /// Lexer/parser/semantic finding in prepared coordinates.
    Prepared(PreparedDiagnostic),
    /// Plain-LSL finding tagged with its producing unit.
    Legacy(LegacyDiagnostic),
}

/// A rendered diagnostic in original coordinates.
#[derive(Debug, Clone)]
pub struct RenderedDiagnostic {
    diagnostic: Diagnostic,
    source: SourceId,
    related: Vec<RelatedLocation>,
}

impl RenderedDiagnostic {
    /// Frozen rendered diagnostic.
    pub fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }

    /// Original file that owns the primary location.
    pub fn source(&self) -> SourceId {
        self.source
    }

    /// Related original locations.
    pub fn related(&self) -> &[RelatedLocation] {
        &self.related
    }

    /// Convert a preprocessor finding. Every span must belong to `map`'s unit.
    pub fn from_preprocess(
        diagnostic: PreprocessDiagnostic,
        map: &SourceMap,
    ) -> Result<Self, SpanError> {
        let span = map.render_span(diagnostic.primary)?;
        // Explicit ownership check independent of rendering.
        map.location(diagnostic.primary)?;
        let mut related = Vec::with_capacity(diagnostic.related.len());
        for location in diagnostic.related {
            map.location(location.span)?;
            map.render_span(location.span)?;
            related.push(location);
        }
        Ok(Self {
            diagnostic: Diagnostic {
                code: diagnostic.code,
                severity: diagnostic.severity,
                kind: diagnostic.kind,
                message: diagnostic.message,
                span,
            },
            source: diagnostic.primary.source(),
            related,
        })
    }

    /// Convert a prepared-coordinate diagnostic.
    pub fn from_prepared(
        diagnostic: PreparedDiagnostic,
        prepared: &crate::preprocessor::PreparedSource,
        map: &SourceMap,
    ) -> Result<Self, SpanError> {
        if prepared.unit() != map.unit() {
            return Err(SpanError::ForeignPreparedSource);
        }
        let prepared_span = prepared.prepared_span(diagnostic.span)?;
        let resolved = map.resolve_span(prepared_span)?;
        let primary = resolved.primary();
        let span = map.render_span(primary)?;
        let mut related = Vec::new();
        for prepared_related in diagnostic.related {
            let resolved = map.resolve_span(prepared_related)?;
            let source_span = resolved.primary();
            if source_span == primary {
                continue;
            }
            if related
                .iter()
                .any(|location: &RelatedLocation| location.span == source_span)
            {
                continue;
            }
            map.render_span(source_span)?;
            related.push(RelatedLocation::new(source_span, "related location"));
        }
        Ok(Self {
            diagnostic: Diagnostic {
                code: diagnostic.code,
                severity: diagnostic.severity,
                kind: diagnostic.kind,
                message: diagnostic.message,
                span,
            },
            source: primary.source(),
            related,
        })
    }

    /// Convert a Legacy diagnostic without touching any source map.
    pub fn from_legacy(
        diagnostic: LegacyDiagnostic,
        unit: &AnalysisUnit,
    ) -> Result<Self, SpanError> {
        if diagnostic.unit != unit.id() {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        let source = unit.legacy_root().ok_or(SpanError::NotLegacyUnit)?;
        Ok(Self {
            diagnostic: diagnostic.diagnostic,
            source,
            related: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::AnalysisRun;

    fn span(start: usize, end: usize) -> Span {
        Span {
            start,
            end,
            line: 9,
            column: 9,
            end_line: 9,
            end_column: 9,
        }
    }

    fn legacy_diagnostic() -> Diagnostic {
        Diagnostic {
            code: "SEM999",
            severity: Severity::Warning,
            kind: DiagnosticKind::Semantic,
            message: "legacy".to_owned(),
            span: span(1, 3),
        }
    }

    #[test]
    fn legacy_round_trip_preserves_every_span_field() {
        let mut run = AnalysisRun::new().unwrap();
        let index = run.add_legacy_text("root.lsl", "default {}").unwrap();
        let unit = run.unit(index).unwrap();
        let tagged = unit.legacy_diagnostic(legacy_diagnostic()).unwrap();
        let rendered = RenderedDiagnostic::from_legacy(tagged, unit).unwrap();
        assert_eq!(rendered.diagnostic(), &legacy_diagnostic());
        assert_eq!(rendered.source(), unit.root().unwrap());
        assert!(rendered.related().is_empty());
    }

    #[test]
    fn legacy_tag_from_another_unit_is_rejected() {
        let mut run = AnalysisRun::new().unwrap();
        let first = run.add_legacy_text("a.lsl", "default {}").unwrap();
        let second = run.add_legacy_text("b.lsl", "default {}").unwrap();
        let first_unit = run.unit(first).unwrap();
        let second_unit = run.unit(second).unwrap();
        let tagged = second_unit.legacy_diagnostic(legacy_diagnostic()).unwrap();
        assert_eq!(
            RenderedDiagnostic::from_legacy(tagged, first_unit).unwrap_err(),
            SpanError::ForeignAnalysisUnit
        );
    }

    #[test]
    fn prepared_unit_has_no_legacy_diagnostic_or_root() {
        let mut run = AnalysisRun::new().unwrap();
        let index = run.add_prepared_text("root.lsl", "default {}").unwrap();
        let unit = run.unit(index).unwrap();
        assert!(unit.legacy_root().is_none());
        assert_eq!(
            unit.legacy_diagnostic(legacy_diagnostic()).unwrap_err(),
            SpanError::NotLegacyUnit
        );
        assert!(unit.prepared_source().is_some());
        assert!(unit.source_map().is_some());
    }

    #[test]
    fn prepared_conversion_uses_prepared_bridge_and_recomputes_positions() {
        let mut run = AnalysisRun::new().unwrap();
        let text = "abc\ndef\n";
        let index = run.add_prepared_text("root.lsl", text).unwrap();
        let unit = run.unit(index).unwrap();
        let prepared = unit.prepared_source().unwrap();
        let map = unit.source_map().unwrap();
        let diagnostic = PreparedDiagnostic::new(
            "PAR999",
            Severity::Error,
            DiagnosticKind::Syntax,
            "prepared",
            span(4, 7),
        );
        let rendered = RenderedDiagnostic::from_prepared(diagnostic, prepared, map).unwrap();
        let rendered_span = rendered.diagnostic().span;
        assert_eq!((rendered_span.start, rendered_span.end), (4, 7));
        assert_eq!((rendered_span.line, rendered_span.column), (2, 1));
        assert_eq!((rendered_span.end_line, rendered_span.end_column), (2, 4));
        assert_eq!(rendered.source(), unit.root().unwrap());
    }

    #[test]
    fn prepared_conversion_rejects_mismatched_map_and_foreign_spans() {
        let mut run = AnalysisRun::new().unwrap();
        let first = run.add_prepared_text("a.lsl", "abc").unwrap();
        let second = run.add_prepared_text("b.lsl", "abc").unwrap();
        let first_unit = run.unit(first).unwrap();
        let second_unit = run.unit(second).unwrap();
        let diagnostic = PreparedDiagnostic::new(
            "PAR999",
            Severity::Error,
            DiagnosticKind::Syntax,
            "prepared",
            span(0, 1),
        );
        assert_eq!(
            RenderedDiagnostic::from_prepared(
                diagnostic.clone(),
                first_unit.prepared_source().unwrap(),
                second_unit.source_map().unwrap()
            )
            .unwrap_err(),
            SpanError::ForeignPreparedSource
        );
        let mut foreign = diagnostic;
        foreign.span = span(0, 10);
        assert_eq!(
            RenderedDiagnostic::from_prepared(
                foreign,
                first_unit.prepared_source().unwrap(),
                first_unit.source_map().unwrap()
            )
            .unwrap_err(),
            SpanError::OutOfBounds
        );
    }

    #[test]
    fn preprocess_conversion_is_direct_and_checks_related_ownership() {
        let mut run = AnalysisRun::new().unwrap();
        let first = run.add_prepared_text("a.lsl", "abc").unwrap();
        let second = run.add_prepared_text("b.lsl", "abc").unwrap();
        let first_unit = run.unit(first).unwrap();
        let second_unit = run.unit(second).unwrap();
        let first_map = first_unit.source_map().unwrap();
        let second_map = second_unit.source_map().unwrap();
        let primary = first_map
            .source_span(first_unit.root().unwrap(), 0, 1)
            .unwrap();
        let mut diagnostic =
            PreprocessDiagnostic::new("PRE001", Severity::Error, "direct", primary);
        diagnostic
            .related
            .push(RelatedLocation::new(primary, "same unit"));
        let rendered = RenderedDiagnostic::from_preprocess(diagnostic, first_map).unwrap();
        assert_eq!(rendered.source(), first_unit.root().unwrap());
        assert_eq!(rendered.related().len(), 1);

        let mut foreign = PreprocessDiagnostic::new("PRE001", Severity::Error, "foreign", primary);
        foreign.related.push(RelatedLocation::new(
            second_map
                .source_span(second_unit.root().unwrap(), 0, 1)
                .unwrap(),
            "foreign",
        ));
        assert_eq!(
            RenderedDiagnostic::from_preprocess(foreign, first_map).unwrap_err(),
            SpanError::ForeignAnalysisUnit
        );
    }
}
