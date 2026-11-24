//! Unified diagnostic conversion with strict analysis-unit ownership.
//!
//! Resolution failures are represented by [`SpanError`]. A malformed or foreign
//! coordinate is returned to the caller; it is never silently dropped, and no
//! local file is substituted.

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
use crate::preprocessor::{PreprocessDiagnostic, RelatedLocation};
use crate::source::file::{AnalysisUnitId, SourceId};
use crate::source::map::{PreparedSpan, SourceMap, SourceSpan, SpanError};
use crate::source::unit::AnalysisUnit;

/// A frozen legacy diagnostic tagged with the unit that produced it.
///
/// Fields are private and there is no public constructor. Only the unit
/// pipeline creates it.
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

    /// Convert a preprocessor finding. The map is obtained from the
    /// `PreparedSource`; every span must belong to that unit.
    pub fn from_preprocess(
        diagnostic: PreprocessDiagnostic,
        prepared: &crate::preprocessor::PreparedSource,
    ) -> Result<Self, SpanError> {
        let map = prepared.map();
        if prepared.unit() != map.unit()
            || prepared.prepared_len() != map.prepared_len()
            || prepared.text() != map.prepared_text()
        {
            return Err(SpanError::ForeignPreparedSource);
        }
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
    ///
    /// The map is always obtained from the `PreparedSource`; callers cannot
    /// supply a different map for the same unit. All related locations from the
    /// primary range and from every explicit related prepared range are carried
    /// into the rendered diagnostic, deduplicated and excluding the primary.
    pub fn from_prepared(
        diagnostic: PreparedDiagnostic,
        prepared: &crate::preprocessor::PreparedSource,
    ) -> Result<Self, SpanError> {
        validate_prepared(prepared)?;
        Self::from_prepared_validated(diagnostic, prepared)
    }

    /// Convert a prepared diagnostic after the caller validated the source once.
    ///
    /// Resolution is called once per diagnostic, so the whole-buffer equality
    /// check that proves the prepared source and its map agree is hoisted out of
    /// this path: comparing a multi-hundred-kilobyte buffer per diagnostic made
    /// rendering quadratic in the diagnostic count. Callers must have called
    /// [`validate_prepared`] for the same `prepared` value.
    pub(crate) fn from_prepared_validated(
        diagnostic: PreparedDiagnostic,
        prepared: &crate::preprocessor::PreparedSource,
    ) -> Result<Self, SpanError> {
        let map = prepared.map();
        let prepared_span = prepared.prepared_span(diagnostic.span)?;
        let resolved = map.resolve_span(prepared_span)?;
        let primary = resolved.primary();
        let span = map.render_span(primary)?;
        let mut related = Vec::new();
        for source_span in resolved.related() {
            push_related(map, primary, *source_span, &mut related)?;
        }
        for prepared_related in diagnostic.related {
            let resolved = map.resolve_span(prepared_related)?;
            push_related(map, primary, resolved.primary(), &mut related)?;
            for source_span in resolved.related() {
                push_related(map, primary, *source_span, &mut related)?;
            }
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

    /// Prove that a prepared source and its map describe the same buffer.
    ///
    /// This is the expensive whole-buffer check that must run once per unit, not
    /// once per rendered diagnostic.
    pub(crate) fn validate(
        prepared: &crate::preprocessor::PreparedSource,
    ) -> Result<(), SpanError> {
        validate_prepared(prepared)
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

fn push_related(
    map: &SourceMap,
    primary: SourceSpan,
    span: SourceSpan,
    related: &mut Vec<RelatedLocation>,
) -> Result<(), SpanError> {
    if span == primary {
        return Ok(());
    }
    map.render_span(span)?;
    if related.iter().any(|location| location.span == span) {
        return Ok(());
    }
    related.push(RelatedLocation::new(span, "related location"));
    Ok(())
}

/// Shared prepared-source validation used by the per-unit and per-diagnostic paths.
pub(crate) fn validate_prepared(
    prepared: &crate::preprocessor::PreparedSource,
) -> Result<(), SpanError> {
    let map = prepared.map();
    if prepared.unit() != map.unit()
        || prepared.prepared_len() != map.prepared_len()
        || prepared.text() != map.prepared_text()
    {
        return Err(SpanError::ForeignPreparedSource);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::AnalysisRun;
    use crate::preprocessor::PreparedSource;
    use crate::source::file::RunId;
    use crate::source::manager::SourceManager;
    use crate::source::map::{OriginKind, SourceMapBuilder};
    use std::sync::Arc;

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
        let diagnostic = PreparedDiagnostic::new(
            "PAR999",
            Severity::Error,
            DiagnosticKind::Syntax,
            "prepared",
            span(4, 7),
        );
        let rendered = RenderedDiagnostic::from_prepared(diagnostic, prepared).unwrap();
        let rendered_span = rendered.diagnostic().span;
        assert_eq!((rendered_span.start, rendered_span.end), (4, 7));
        assert_eq!((rendered_span.line, rendered_span.column), (2, 1));
        assert_eq!((rendered_span.end_line, rendered_span.end_column), (2, 4));
        assert_eq!(rendered.source(), unit.root().unwrap());
    }

    #[test]
    fn prepared_conversion_keeps_all_related_locations_and_deduplicates() {
        let unit = crate::source::file::AnalysisUnitId::new(RunId::new(70), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", "abcdefghij").unwrap();
        let mut builder =
            SourceMapBuilder::new(unit, Arc::from("abc"), manager.files().to_vec()).unwrap();
        let origins = [
            builder
                .push_origin(
                    OriginKind::Direct,
                    builder.source_span(root, 4, 5).unwrap(),
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            builder
                .push_origin(
                    OriginKind::Direct,
                    builder.source_span(root, 6, 7).unwrap(),
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            builder
                .push_origin(
                    OriginKind::Direct,
                    builder.source_span(root, 8, 9).unwrap(),
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
        ];
        for (index, origin) in origins.iter().enumerate() {
            builder
                .push_segment(PreparedSpan::new(unit, index, index + 1), *origin)
                .unwrap();
        }
        builder
            .set_eof_anchor(builder.source_span(root, 10, 10).unwrap())
            .unwrap();
        let map = builder.build().unwrap();
        let prepared = PreparedSource::new(
            unit,
            Arc::from("abc"),
            Arc::new(map),
            Vec::new(),
            Arc::new(crate::preprocessor::IncludeGraph::default()),
        );

        let mut diagnostic = PreparedDiagnostic::new(
            "PAR999",
            Severity::Error,
            DiagnosticKind::Syntax,
            "prepared",
            span(0, 3),
        );
        diagnostic
            .related
            .push(prepared.prepared_span(span(0, 2)).unwrap());
        let rendered = RenderedDiagnostic::from_prepared(diagnostic, &prepared).unwrap();
        let primary = prepared.map().source_span(root, 4, 5).unwrap();
        assert_eq!(rendered.diagnostic().span.start, primary.start());
        let related: Vec<_> = rendered.related().iter().map(|item| item.span).collect();
        assert_eq!(
            related,
            vec![
                prepared.map().source_span(root, 6, 7).unwrap(),
                prepared.map().source_span(root, 8, 9).unwrap(),
            ],
            "deterministic related order"
        );
        assert!(!related.contains(&primary));
    }

    #[test]
    fn prepared_conversion_rejects_a_mismatched_map_length() {
        let unit = crate::source::file::AnalysisUnitId::new(RunId::new(71), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", "abc").unwrap();
        let mut builder =
            SourceMapBuilder::new(unit, Arc::from("ab"), manager.files().to_vec()).unwrap();
        let origin = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 2).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 2), origin)
            .unwrap();
        builder
            .set_eof_anchor(builder.source_span(root, 2, 2).unwrap())
            .unwrap();
        let prepared = PreparedSource::new(
            unit,
            Arc::from("abc"),
            Arc::new(builder.build().unwrap()),
            Vec::new(),
            Arc::new(crate::preprocessor::IncludeGraph::default()),
        );
        let diagnostic = PreparedDiagnostic::new(
            "PAR999",
            Severity::Error,
            DiagnosticKind::Syntax,
            "x",
            span(0, 1),
        );
        assert_eq!(
            RenderedDiagnostic::from_prepared(diagnostic, &prepared).unwrap_err(),
            SpanError::ForeignPreparedSource
        );
    }

    #[test]
    fn prepared_conversion_rejects_same_length_map_with_different_text() {
        let unit = crate::source::file::AnalysisUnitId::new(RunId::new(72), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", "abc").unwrap();
        let mut builder =
            SourceMapBuilder::new(unit, Arc::from("abc"), manager.files().to_vec()).unwrap();
        let origin = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 3).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 3), origin)
            .unwrap();
        builder
            .set_eof_anchor(builder.source_span(root, 3, 3).unwrap())
            .unwrap();
        let prepared = PreparedSource::new(
            unit,
            Arc::from("abd"),
            Arc::new(builder.build().unwrap()),
            Vec::new(),
            Arc::new(crate::preprocessor::IncludeGraph::default()),
        );
        let diagnostic = PreparedDiagnostic::new(
            "PAR999",
            Severity::Error,
            DiagnosticKind::Syntax,
            "x",
            span(0, 1),
        );
        assert_eq!(
            RenderedDiagnostic::from_prepared(diagnostic, &prepared).unwrap_err(),
            SpanError::ForeignPreparedSource
        );
    }

    #[test]
    fn prepared_conversion_rejects_foreign_spans_without_a_map_argument() {
        let mut run = AnalysisRun::new().unwrap();
        let first = run.add_prepared_text("a.lsl", "abc").unwrap();
        let first_unit = run.unit(first).unwrap();
        let diagnostic = PreparedDiagnostic::new(
            "PAR999",
            Severity::Error,
            DiagnosticKind::Syntax,
            "prepared",
            span(0, 10),
        );
        assert_eq!(
            RenderedDiagnostic::from_prepared(diagnostic, first_unit.prepared_source().unwrap())
                .unwrap_err(),
            SpanError::OutOfBounds
        );
        // `from_prepared` takes no map argument; the map is obtained from the
        // PreparedSource, so a same-unit foreign map cannot be supplied.
        assert_eq!(
            first_unit.prepared_source().unwrap().map().unit(),
            first_unit.id()
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
        let rendered =
            RenderedDiagnostic::from_preprocess(diagnostic, first_unit.prepared_source().unwrap())
                .unwrap();
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
            RenderedDiagnostic::from_preprocess(foreign, first_unit.prepared_source().unwrap())
                .unwrap_err(),
            SpanError::ForeignAnalysisUnit
        );
    }
}
