//! Engine-aware analysis selection and result envelopes.
//!
//! The module is additive: [`AnalysisUnit::analyze`](crate::source::AnalysisUnit::analyze)
//! and [`AnalysisRun::analyze`](crate::pipeline::AnalysisRun::analyze) keep their
//! frozen Legacy behavior, while the engine API can additionally run the
//! independent AST analyzer or compare both semantic streams.
//!
//! Comparison fingerprints always carry raw prepared coordinates. Rendering into
//! original source coordinates happens only for the displayed diagnostic stream.

pub mod compare;
pub(crate) mod wiring;

use std::collections::HashSet;
use std::fmt;

use crate::diagnostic::resolve::RenderedDiagnostic;
use crate::lexer::Token;
use crate::source::file::AnalysisUnitId;
use crate::source::map::SpanError;

pub use compare::{
    compare_semantic_diagnostics, DiagnosticFingerprint, EngineComparisonReport, RuleComparison,
    MAX_COMPARE_DETAILS_PER_RULE, MAX_COMPARE_FINGERPRINTS_PER_ENGINE, SEM_CODES,
};

/// Which semantic analyzer the engine API runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalysisEngine {
    /// Frozen Legacy lexer/parser/semantic pipeline.
    #[default]
    Legacy,
    /// Independent typed-AST semantic analyzer.
    Ast,
    /// Run both analyzers and report their raw semantic streams.
    Compare,
}

impl AnalysisEngine {
    /// Parse the exact CLI/config selector strings.
    ///
    /// Accepts only `legacy`, `ast`, and `compare`; matching is case-sensitive
    /// and whitespace is not trimmed.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "legacy" => Some(Self::Legacy),
            "ast" => Some(Self::Ast),
            "compare" => Some(Self::Compare),
            _ => None,
        }
    }

    /// Stable lowercase selector name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Ast => "ast",
            Self::Compare => "compare",
        }
    }

    /// Whether this engine requires a prepared source and source map.
    pub const fn requires_prepared_source(self) -> bool {
        match self {
            Self::Legacy => false,
            Self::Ast | Self::Compare => true,
        }
    }
}

/// Failure to run the engine API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineAnalysisError {
    /// Existing prepared-span resolution failure, propagated fail-fast.
    Span(SpanError),
    /// The AST/Compare engines were requested for a plain no-map unit.
    PreparedSourceRequired {
        /// Unit that has no prepared source.
        unit: AnalysisUnitId,
    },
}

impl fmt::Display for EngineAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Span(error) => write!(formatter, "{error}"),
            Self::PreparedSourceRequired { unit } => write!(
                formatter,
                "analysis engine requires a prepared source for unit {}",
                unit.index()
            ),
        }
    }
}

impl std::error::Error for EngineAnalysisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Span(error) => Some(error),
            Self::PreparedSourceRequired { .. } => None,
        }
    }
}

impl From<SpanError> for EngineAnalysisError {
    fn from(error: SpanError) -> Self {
        Self::Span(error)
    }
}

/// Rule codes whose findings are suppressed from display.
///
/// Suppression is display-only: it does not change analysis or severity, and a
/// disabled rule still runs. The set participates before the rendered-diagnostic
/// cap, so findings suppressed here cannot trigger the terminal `PRE039`
/// overflow diagnostic. Severity overrides are not display suppression.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplaySuppression {
    codes: HashSet<String>,
}

impl DisplaySuppression {
    /// Build a suppression set from rule codes.
    pub fn new(codes: impl IntoIterator<Item = String>) -> Self {
        Self {
            codes: codes.into_iter().collect(),
        }
    }

    /// Whether `code` is suppressed from display.
    pub fn is_suppressed(&self, code: &str) -> bool {
        self.codes.contains(code)
    }

    /// Whether no rule code is suppressed.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

/// Engine analysis options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineAnalysisOptions {
    /// Retain the exact token vector produced by the single lexer call.
    pub collect_tokens: bool,
}

/// Result of one engine-aware unit analysis.
#[derive(Debug, Clone)]
pub struct UnitEngineAnalysis {
    diagnostics: Vec<RenderedDiagnostic>,
    comparison: Option<EngineComparisonReport>,
    tokens: Vec<Token>,
}

impl UnitEngineAnalysis {
    pub(crate) fn new(
        diagnostics: Vec<RenderedDiagnostic>,
        comparison: Option<EngineComparisonReport>,
        tokens: Vec<Token>,
    ) -> Self {
        Self {
            diagnostics,
            comparison,
            tokens,
        }
    }

    /// Displayed diagnostics in original source coordinates.
    pub fn diagnostics(&self) -> &[RenderedDiagnostic] {
        &self.diagnostics
    }

    /// Consume the displayed diagnostic stream.
    pub fn into_diagnostics(self) -> Vec<RenderedDiagnostic> {
        self.diagnostics
    }

    /// Comparison report, present exactly when the engine is
    /// [`AnalysisEngine::Compare`].
    pub fn comparison(&self) -> Option<&EngineComparisonReport> {
        self.comparison.as_ref()
    }

    /// Exact lexer tokens, empty unless `collect_tokens` was enabled.
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<RenderedDiagnostic>,
        Option<EngineComparisonReport>,
        Vec<Token>,
    ) {
        (self.diagnostics, self.comparison, self.tokens)
    }
}

/// Comparison attached to one run unit.
#[derive(Debug, Clone)]
pub struct UnitComparison {
    /// Dense unit index inside the run.
    pub unit_index: usize,
    /// Unit identity.
    pub unit_id: AnalysisUnitId,
    /// Deterministic per-rule comparison report.
    pub report: EngineComparisonReport,
}

/// Result of one engine-aware run analysis.
#[derive(Debug, Clone)]
pub struct RunEngineAnalysis {
    diagnostics: Vec<RenderedDiagnostic>,
    comparisons: Vec<UnitComparison>,
    unit_tokens: Vec<Vec<Token>>,
}

impl RunEngineAnalysis {
    pub(crate) fn new(
        diagnostics: Vec<RenderedDiagnostic>,
        comparisons: Vec<UnitComparison>,
        unit_tokens: Vec<Vec<Token>>,
    ) -> Self {
        Self {
            diagnostics,
            comparisons,
            unit_tokens,
        }
    }

    /// Concatenated displayed diagnostics in dense unit order.
    pub fn diagnostics(&self) -> &[RenderedDiagnostic] {
        &self.diagnostics
    }

    /// Per-unit reports in dense unit order; empty unless the engine is
    /// [`AnalysisEngine::Compare`].
    pub fn comparisons(&self) -> &[UnitComparison] {
        &self.comparisons
    }

    /// Tokens for one unit in dense unit order.
    ///
    /// Returns `None` for an unknown unit index. For a known unit the slice is
    /// empty unless [`EngineAnalysisOptions::collect_tokens`] was set; when it
    /// was set, this is the exact vector produced by that unit's single lexer
    /// call, shared with the analysis that produced the diagnostics.
    pub fn tokens(&self, unit_index: usize) -> Option<&[Token]> {
        self.unit_tokens.get(unit_index).map(Vec::as_slice)
    }

    /// Whether the run's semantic comparison asserts full parity.
    ///
    /// Returns `true` when the engine produced no comparison reports
    /// ([`AnalysisEngine::Legacy`] or [`AnalysisEngine::Ast`]), including an
    /// empty run. For [`AnalysisEngine::Compare`], returns `true` only when
    /// every unit's [`EngineComparisonReport::is_parity`] is `true`. An
    /// incomplete report (`comparison_complete == false`) is never parity even
    /// if an inconsistent report sets `matched`, so it makes this return
    /// `false`. This is the parity signal for exit-code decisions.
    pub fn all_matched(&self) -> bool {
        self.comparisons
            .iter()
            .all(|comparison| comparison.report.is_parity())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_legacy() {
        assert_eq!(AnalysisEngine::default(), AnalysisEngine::Legacy);
    }

    #[test]
    fn parse_accepts_exactly_the_selector_names() {
        assert_eq!(
            AnalysisEngine::parse("legacy"),
            Some(AnalysisEngine::Legacy)
        );
        assert_eq!(AnalysisEngine::parse("ast"), Some(AnalysisEngine::Ast));
        assert_eq!(
            AnalysisEngine::parse("compare"),
            Some(AnalysisEngine::Compare)
        );
        for rejected in ["", "Legacy", "AST", " legacy", "compare ", "astt", "lsl"] {
            assert_eq!(AnalysisEngine::parse(rejected), None, "{rejected:?}");
        }
    }

    #[test]
    fn selector_round_trips_and_requires_prepared_source() {
        for engine in [
            AnalysisEngine::Legacy,
            AnalysisEngine::Ast,
            AnalysisEngine::Compare,
        ] {
            assert_eq!(AnalysisEngine::parse(engine.as_str()), Some(engine));
        }
        assert!(!AnalysisEngine::Legacy.requires_prepared_source());
        assert!(AnalysisEngine::Ast.requires_prepared_source());
        assert!(AnalysisEngine::Compare.requires_prepared_source());
    }

    #[test]
    fn display_suppression_tracks_exact_rule_codes() {
        let empty = DisplaySuppression::default();
        assert!(empty.is_empty());
        assert!(!empty.is_suppressed("SEM012"));

        let suppression = DisplaySuppression::new(["SEM012".to_owned(), "PRE039".to_owned()]);
        assert!(!suppression.is_empty());
        assert!(suppression.is_suppressed("SEM012"));
        assert!(suppression.is_suppressed("PRE039"));
        assert!(!suppression.is_suppressed("sem012"));
        assert!(!suppression.is_suppressed("SEM013"));
    }

    fn report(matched: bool) -> EngineComparisonReport {
        EngineComparisonReport {
            matched,
            comparison_complete: true,
            details_truncated: false,
            legacy_count: 0,
            ast_count: 0,
            by_rule: Vec::new(),
        }
    }

    fn comparison(unit_index: usize, matched: bool) -> UnitComparison {
        UnitComparison {
            unit_index,
            unit_id: AnalysisUnitId::new(crate::source::file::RunId::new(0), unit_index as u32),
            report: report(matched),
        }
    }

    #[test]
    fn run_tokens_are_indexed_by_dense_unit_order() {
        let tokens = crate::lex("integer g;").0;
        let analysis =
            RunEngineAnalysis::new(Vec::new(), Vec::new(), vec![tokens.clone(), Vec::new()]);
        assert_eq!(analysis.tokens(0), Some(tokens.as_slice()));
        assert_eq!(analysis.tokens(1), Some(&[][..]));
        assert_eq!(analysis.tokens(2), None);
    }

    #[test]
    fn all_matched_is_true_without_comparisons_and_requires_every_report() {
        assert!(RunEngineAnalysis::new(Vec::new(), Vec::new(), Vec::new()).all_matched());
        assert!(RunEngineAnalysis::new(
            Vec::new(),
            vec![comparison(0, true), comparison(1, true)],
            vec![Vec::new(), Vec::new()],
        )
        .all_matched());
        assert!(!RunEngineAnalysis::new(
            Vec::new(),
            vec![comparison(0, true), comparison(1, false)],
            vec![Vec::new(), Vec::new()],
        )
        .all_matched());
    }

    #[test]
    fn all_matched_rejects_an_incomplete_inconsistent_report() {
        let mut report = report(true);
        report.comparison_complete = false;
        let inconsistent = UnitComparison {
            unit_index: 0,
            unit_id: AnalysisUnitId::new(crate::source::file::RunId::new(0), 0),
            report,
        };
        assert!(
            !RunEngineAnalysis::new(Vec::new(), vec![inconsistent], vec![Vec::new()]).all_matched()
        );
    }
}
