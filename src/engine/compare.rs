//! Deterministic comparison of raw Legacy and AST semantic diagnostic streams.
//!
//! The comparator works on the raw [`Diagnostic`] values produced by each
//! semantic analyzer, before any prepared-coordinate rendering. It only looks at
//! the migrated rule codes `SEM001`..=`SEM023`; parser and lexer findings are
//! deliberately out of scope.

use std::cmp::Ordering;

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

/// Every semantic rule code compared by the engine report, in report order.
pub const SEM_CODES: [&str; 23] = [
    "SEM001", "SEM002", "SEM003", "SEM004", "SEM005", "SEM006", "SEM007", "SEM008", "SEM009",
    "SEM010", "SEM011", "SEM012", "SEM013", "SEM014", "SEM015", "SEM016", "SEM017", "SEM018",
    "SEM019", "SEM020", "SEM021", "SEM022", "SEM023",
];

/// Maximum number of missing/extra fingerprints retained per rule and
/// direction.
///
/// This bounds only the detail samples; exact delta totals are always
/// accumulated, so exceeding it sets [`EngineComparisonReport::details_truncated`]
/// without making the comparison incomplete.
pub const MAX_COMPARE_DETAILS_PER_RULE: usize = 64;

/// Maximum number of `SEM001`..=`SEM023` fingerprints materialised per engine
/// before a comparison is declared incomplete.
///
/// The budget is checked by a cheap per-rule counting pass before any
/// fingerprint is built. It counts only compared semantic codes, so a large
/// stream of lexer, parser, or preprocessor findings never trips it. When
/// either engine exceeds the budget, no fingerprint collection, sorting, or
/// multiset delta occurs; per-rule delta totals are then reported as unknown
/// (`None`) rather than zero.
pub const MAX_COMPARE_FINGERPRINTS_PER_ENGINE: usize = 100_000;

/// Number of compared semantic rules, derived from [`SEM_CODES`].
const RULE_COUNT: usize = SEM_CODES.len();

/// One raw semantic diagnostic reduced to an orderable value.
///
/// Message and span fields are part of identity and ordering, so two findings
/// that differ in any field are never treated as the same multiset element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticFingerprint {
    /// Stable rule code.
    pub code: &'static str,
    /// Effective severity.
    pub severity: Severity,
    /// Origin family.
    pub kind: DiagnosticKind,
    /// Human-readable message.
    pub message: String,
    /// Zero-based start offset in prepared coordinates.
    pub start: usize,
    /// Zero-based end offset in prepared coordinates.
    pub end: usize,
    /// One-based start line in prepared coordinates.
    pub line: usize,
    /// One-based start column in prepared coordinates.
    pub column: usize,
    /// One-based end line in prepared coordinates.
    pub end_line: usize,
    /// One-based end column in prepared coordinates.
    pub end_column: usize,
}

impl DiagnosticFingerprint {
    fn from_diagnostic(diagnostic: &Diagnostic) -> Self {
        Self {
            code: diagnostic.code,
            severity: diagnostic.severity,
            kind: diagnostic.kind,
            message: diagnostic.message.clone(),
            start: diagnostic.span.start,
            end: diagnostic.span.end,
            line: diagnostic.span.line,
            column: diagnostic.span.column,
            end_line: diagnostic.span.end_line,
            end_column: diagnostic.span.end_column,
        }
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn kind_rank(kind: DiagnosticKind) -> u8 {
    match kind {
        DiagnosticKind::Lexical => 0,
        DiagnosticKind::Syntax => 1,
        DiagnosticKind::Semantic => 2,
        DiagnosticKind::Lint => 3,
    }
}

impl Ord for DiagnosticFingerprint {
    fn cmp(&self, other: &Self) -> Ordering {
        severity_rank(self.severity)
            .cmp(&severity_rank(other.severity))
            .then_with(|| kind_rank(self.kind).cmp(&kind_rank(other.kind)))
            .then_with(|| self.code.cmp(other.code))
            .then_with(|| self.message.cmp(&other.message))
            .then_with(|| self.start.cmp(&other.start))
            .then_with(|| self.end.cmp(&other.end))
            .then_with(|| self.line.cmp(&other.line))
            .then_with(|| self.column.cmp(&other.column))
            .then_with(|| self.end_line.cmp(&other.end_line))
            .then_with(|| self.end_column.cmp(&other.end_column))
    }
}

impl PartialOrd for DiagnosticFingerprint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Per-rule comparison result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleComparison {
    /// Stable rule code (`SEM001`..=`SEM023`).
    pub code: &'static str,
    /// Exact number of Legacy findings for this rule.
    pub legacy_count: usize,
    /// Exact number of AST findings for this rule.
    pub ast_count: usize,
    /// Exact number of Legacy findings with no AST multiset partner, or
    /// `None` when the comparison was not performed because the SEM
    /// fingerprint budget was exceeded.
    ///
    /// An unknown total is never reported as `0`; callers must distinguish
    /// "no delta" (`Some(0)`) from "not computed" (`None`).
    pub missing_total: Option<usize>,
    /// Exact number of AST findings with no Legacy multiset partner, or
    /// `None` when the comparison was not performed because the SEM
    /// fingerprint budget was exceeded.
    ///
    /// An unknown total is never reported as `0`; callers must distinguish
    /// "no delta" (`Some(0)`) from "not computed" (`None`).
    pub extra_total: Option<usize>,
    /// Bounded sample of the first `MAX_COMPARE_DETAILS_PER_RULE` missing
    /// findings in deterministic order.
    pub missing_from_ast: Vec<DiagnosticFingerprint>,
    /// Bounded sample of the first `MAX_COMPARE_DETAILS_PER_RULE` extra findings
    /// in deterministic order.
    pub extra_in_ast: Vec<DiagnosticFingerprint>,
}

/// Deterministic comparison of the two semantic streams for one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineComparisonReport {
    /// True when the comparison is complete and every rule multiset matches.
    pub matched: bool,
    /// True when the comparison was actually performed over the full inputs.
    ///
    /// False when either engine's SEM fingerprint count exceeds
    /// [`MAX_COMPARE_FINGERPRINTS_PER_ENGINE`]. The incomplete path performs no
    /// fingerprint collection, sorting, or multiset delta: per-rule
    /// `missing_total`/`extra_total` are unknown (`None`) instead of zero, and
    /// `matched` is forced to `false`. This flag is independent of
    /// [`Self::details_truncated`], which only bounds retained detail samples.
    pub comparison_complete: bool,
    /// True when at least one rule's missing/extra total exceeds the detail cap.
    ///
    /// Computed from the exact delta totals of a completed comparison, so it
    /// tracks whether a detail sample list was shortened. The incomplete path
    /// sets this to `false` because no sample lists were produced or shortened.
    pub details_truncated: bool,
    /// Exact Legacy findings across all compared rules.
    pub legacy_count: usize,
    /// Exact AST findings across all compared rules.
    pub ast_count: usize,
    /// One entry per migrated rule, always in `SEM001`..=`SEM023` order.
    pub by_rule: Vec<RuleComparison>,
}

impl EngineComparisonReport {
    /// Whether this report asserts full semantic parity.
    ///
    /// This is the exit-code parity signal. Both [`Self::matched`] and
    /// [`Self::comparison_complete`] must be true; an incomplete comparison is
    /// never parity, even if an inconsistent report sets `matched`.
    pub fn is_parity(&self) -> bool {
        self.matched && self.comparison_complete
    }
}

fn sem_index(code: &str) -> Option<usize> {
    SEM_CODES.binary_search(&code).ok()
}

/// Count compared semantic diagnostics per rule without materialising anything.
///
/// The returned array is the only allocation-sized state on the budget-check
/// path: O(1) memory, no fingerprints, no sorting, no delta vectors.
fn count_by_rule(diagnostics: &[Diagnostic]) -> [usize; RULE_COUNT] {
    let mut counts = [0usize; RULE_COUNT];
    for diagnostic in diagnostics {
        if let Some(index) = sem_index(diagnostic.code) {
            counts[index] += 1;
        }
    }
    counts
}

fn collect_by_rule(diagnostics: &[Diagnostic]) -> Vec<Vec<DiagnosticFingerprint>> {
    let mut by_rule: Vec<Vec<DiagnosticFingerprint>> =
        (0..RULE_COUNT).map(|_| Vec::new()).collect();
    for diagnostic in diagnostics {
        if let Some(index) = sem_index(diagnostic.code) {
            by_rule[index].push(DiagnosticFingerprint::from_diagnostic(diagnostic));
        }
    }
    for group in &mut by_rule {
        group.sort();
    }
    by_rule
}

/// Exact multiset-delta totals with bounded leading detail samples.
#[derive(Debug, Default, PartialEq, Eq)]
struct MultisetDelta {
    missing_total: usize,
    extra_total: usize,
    missing_sample: Vec<DiagnosticFingerprint>,
    extra_sample: Vec<DiagnosticFingerprint>,
}

impl MultisetDelta {
    fn push_missing(&mut self, fingerprint: &DiagnosticFingerprint) {
        self.missing_total += 1;
        if self.missing_sample.len() < MAX_COMPARE_DETAILS_PER_RULE {
            self.missing_sample.push(fingerprint.clone());
        }
    }

    fn push_extra(&mut self, fingerprint: &DiagnosticFingerprint) {
        self.extra_total += 1;
        if self.extra_sample.len() < MAX_COMPARE_DETAILS_PER_RULE {
            self.extra_sample.push(fingerprint.clone());
        }
    }
}

/// Subtract two sorted fingerprint multisets element-by-element.
///
/// Equal fingerprints consume one occurrence from each side, so duplicate
/// multiplicity is preserved. Totals are exact, while each direction retains
/// only the first [`MAX_COMPARE_DETAILS_PER_RULE`] samples in deterministic sort
/// order. Sample memory is therefore O(64), not O(delta), and no full delta
/// vector is ever built.
fn multiset_delta(
    legacy: &[DiagnosticFingerprint],
    ast: &[DiagnosticFingerprint],
) -> MultisetDelta {
    let mut delta = MultisetDelta::default();
    let mut legacy_index = 0usize;
    let mut ast_index = 0usize;
    while legacy_index < legacy.len() && ast_index < ast.len() {
        match legacy[legacy_index].cmp(&ast[ast_index]) {
            Ordering::Less => {
                delta.push_missing(&legacy[legacy_index]);
                legacy_index += 1;
            }
            Ordering::Greater => {
                delta.push_extra(&ast[ast_index]);
                ast_index += 1;
            }
            Ordering::Equal => {
                legacy_index += 1;
                ast_index += 1;
            }
        }
    }
    for fingerprint in &legacy[legacy_index..] {
        delta.push_missing(fingerprint);
    }
    for fingerprint in &ast[ast_index..] {
        delta.push_extra(fingerprint);
    }
    delta
}

/// Report every rule as present but with unknown delta totals.
fn incomplete_report(
    legacy_counts: &[usize; RULE_COUNT],
    ast_counts: &[usize; RULE_COUNT],
    legacy_count: usize,
    ast_count: usize,
) -> EngineComparisonReport {
    let by_rule = SEM_CODES
        .iter()
        .enumerate()
        .map(|(index, code)| RuleComparison {
            code,
            legacy_count: legacy_counts[index],
            ast_count: ast_counts[index],
            missing_total: None,
            extra_total: None,
            missing_from_ast: Vec::new(),
            extra_in_ast: Vec::new(),
        })
        .collect();
    EngineComparisonReport {
        matched: false,
        comparison_complete: false,
        details_truncated: false,
        legacy_count,
        ast_count,
        by_rule,
    }
}

/// Compare two raw semantic streams, filtering to `SEM001`..=`SEM023`.
///
/// The [`MAX_COMPARE_FINGERPRINTS_PER_ENGINE`] budget applies to the number of
/// compared SEM fingerprints, not to the raw stream length: a cheap counting
/// pass runs first, and a non-SEM diagnostic never counts against the budget.
/// When either engine is over budget the function returns immediately without
/// collecting, sorting, or diffing fingerprints. The incomplete report keeps
/// exact `legacy_count`/`ast_count` but reports every rule's
/// `missing_total`/`extra_total` as `None`, forces `matched = false`, and leaves
/// the detail samples empty.
///
/// Within the budget all totals are exact (`Some(n)`) and per-rule detail
/// samples retain at most [`MAX_COMPARE_DETAILS_PER_RULE`] fingerprints per
/// direction.
pub fn compare_semantic_diagnostics(
    legacy: &[Diagnostic],
    ast: &[Diagnostic],
) -> EngineComparisonReport {
    let legacy_counts = count_by_rule(legacy);
    let ast_counts = count_by_rule(ast);
    let legacy_count: usize = legacy_counts.iter().sum();
    let ast_count: usize = ast_counts.iter().sum();

    if legacy_count > MAX_COMPARE_FINGERPRINTS_PER_ENGINE
        || ast_count > MAX_COMPARE_FINGERPRINTS_PER_ENGINE
    {
        return incomplete_report(&legacy_counts, &ast_counts, legacy_count, ast_count);
    }

    let legacy_by_rule = collect_by_rule(legacy);
    let ast_by_rule = collect_by_rule(ast);
    let mut by_rule = Vec::with_capacity(RULE_COUNT);
    let mut details_truncated = false;
    let mut matched = true;
    for (index, code) in SEM_CODES.iter().enumerate() {
        let delta = multiset_delta(&legacy_by_rule[index], &ast_by_rule[index]);
        if delta.missing_total > MAX_COMPARE_DETAILS_PER_RULE
            || delta.extra_total > MAX_COMPARE_DETAILS_PER_RULE
        {
            details_truncated = true;
        }
        if delta.missing_total != 0 || delta.extra_total != 0 {
            matched = false;
        }
        by_rule.push(RuleComparison {
            code,
            legacy_count: legacy_by_rule[index].len(),
            ast_count: ast_by_rule[index].len(),
            missing_total: Some(delta.missing_total),
            extra_total: Some(delta.extra_total),
            missing_from_ast: delta.missing_sample,
            extra_in_ast: delta.extra_sample,
        });
    }

    EngineComparisonReport {
        matched,
        comparison_complete: true,
        details_truncated,
        legacy_count,
        ast_count,
        by_rule,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Span;

    fn span(start: usize, end: usize) -> Span {
        Span {
            start,
            end,
            line: 1,
            column: start + 1,
            end_line: 1,
            end_column: end + 1,
        }
    }

    fn diagnostic(code: &'static str, start: usize) -> Diagnostic {
        Diagnostic {
            code,
            severity: Severity::Warning,
            kind: DiagnosticKind::Semantic,
            message: "message".to_owned(),
            span: span(start, start + 1),
        }
    }

    fn fingerprint(code: &'static str, start: usize) -> DiagnosticFingerprint {
        DiagnosticFingerprint::from_diagnostic(&diagnostic(code, start))
    }

    #[test]
    fn fingerprint_order_follows_severity_then_kind_then_fields() {
        let error = DiagnosticFingerprint {
            severity: Severity::Error,
            ..fingerprint("SEM001", 0)
        };
        let warning = DiagnosticFingerprint {
            severity: Severity::Warning,
            ..fingerprint("SEM001", 0)
        };
        assert!(error < warning);

        let lexical = DiagnosticFingerprint {
            severity: Severity::Error,
            kind: DiagnosticKind::Lexical,
            ..fingerprint("SEM001", 0)
        };
        let semantic_error = DiagnosticFingerprint {
            severity: Severity::Error,
            kind: DiagnosticKind::Semantic,
            ..fingerprint("SEM001", 0)
        };
        assert!(lexical < semantic_error);

        let mut values = [
            fingerprint("SEM002", 0),
            fingerprint("SEM001", 5),
            fingerprint("SEM001", 1),
        ];
        values.sort();
        assert_eq!(
            values
                .iter()
                .map(|value| (value.code, value.start))
                .collect::<Vec<_>>(),
            vec![("SEM001", 1), ("SEM001", 5), ("SEM002", 0)]
        );
    }

    #[test]
    fn multiset_delta_preserves_duplicate_multiplicity() {
        let a = fingerprint("SEM001", 0);
        let delta = multiset_delta(&[a.clone(), a.clone()], std::slice::from_ref(&a));
        assert_eq!(delta.missing_total, 1);
        assert_eq!(delta.extra_total, 0);
        assert_eq!(delta.missing_sample, vec![a.clone()]);
        assert!(delta.extra_sample.is_empty());

        let delta = multiset_delta(std::slice::from_ref(&a), &[a.clone(), a.clone()]);
        assert_eq!(delta.missing_total, 0);
        assert_eq!(delta.extra_total, 1);
        assert!(delta.missing_sample.is_empty());
        assert_eq!(delta.extra_sample, vec![a]);
    }

    #[test]
    fn multiset_delta_returns_sorted_boundaries() {
        let legacy = vec![fingerprint("SEM001", 0), fingerprint("SEM001", 1)];
        let ast = vec![fingerprint("SEM001", 1), fingerprint("SEM001", 2)];
        let delta = multiset_delta(&legacy, &ast);
        assert_eq!(delta.missing_total, 1);
        assert_eq!(delta.extra_total, 1);
        assert_eq!(delta.missing_sample, vec![fingerprint("SEM001", 0)]);
        assert_eq!(delta.extra_sample, vec![fingerprint("SEM001", 2)]);
    }

    #[test]
    fn multiset_delta_bounds_samples_but_keeps_exact_totals() {
        let legacy: Vec<_> = (0..MAX_COMPARE_DETAILS_PER_RULE + 1)
            .map(|start| fingerprint("SEM001", start))
            .collect();
        let delta = multiset_delta(&legacy, &[]);
        assert_eq!(delta.missing_total, MAX_COMPARE_DETAILS_PER_RULE + 1);
        assert_eq!(delta.missing_sample.len(), MAX_COMPARE_DETAILS_PER_RULE);
        assert_eq!(delta.extra_total, 0);
        assert!(delta.extra_sample.is_empty());
    }

    #[test]
    fn count_by_rule_counts_only_compared_semantic_codes() {
        let lexical = Diagnostic::lexical("LEX001", "bad", span(0, 1));
        let parser = Diagnostic {
            code: "PAR001",
            severity: Severity::Error,
            kind: DiagnosticKind::Syntax,
            message: "bad".to_owned(),
            span: span(0, 1),
        };
        let diagnostics = [
            diagnostic("SEM001", 0),
            diagnostic("SEM001", 1),
            diagnostic("SEM023", 2),
            lexical,
            parser,
        ];
        let counts = count_by_rule(&diagnostics);
        assert_eq!(counts[0], 2);
        assert_eq!(counts[RULE_COUNT - 1], 1);
        assert_eq!(counts.iter().sum::<usize>(), 3);
    }

    #[test]
    fn over_budget_sem_stream_returns_unknown_totals_without_deltas() {
        let diagnostics = vec![diagnostic("SEM001", 0); MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
        let report = compare_semantic_diagnostics(&diagnostics, &[]);
        assert!(!report.matched);
        assert!(!report.comparison_complete);
        assert!(!report.details_truncated);
        assert_eq!(report.legacy_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1);
        assert_eq!(report.ast_count, 0);
        assert_eq!(report.by_rule.len(), RULE_COUNT);
        for rule in &report.by_rule {
            assert_eq!(rule.missing_total, None, "{}", rule.code);
            assert_eq!(rule.extra_total, None, "{}", rule.code);
            assert!(rule.missing_from_ast.is_empty(), "{}", rule.code);
            assert!(rule.extra_in_ast.is_empty(), "{}", rule.code);
        }
        assert_eq!(
            report.by_rule[0].legacy_count,
            MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1
        );
    }

    #[test]
    fn non_semantic_stream_length_does_not_trip_the_budget() {
        let mut legacy = vec![
            Diagnostic::lexical("LEX001", "bad", span(0, 1));
            MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1
        ];
        legacy.push(diagnostic("SEM001", 0));
        let ast = vec![diagnostic("SEM001", 0)];
        let report = compare_semantic_diagnostics(&legacy, &ast);
        assert!(report.comparison_complete);
        assert!(report.matched, "{report:?}");
        assert_eq!(report.legacy_count, 1);
        assert_eq!(report.ast_count, 1);
        assert_eq!(report.by_rule[0].missing_total, Some(0));
        assert_eq!(report.by_rule[0].extra_total, Some(0));
    }

    #[test]
    fn empty_streams_match_with_all_rules_present() {
        let report = compare_semantic_diagnostics(&[], &[]);
        assert!(report.matched);
        assert!(report.comparison_complete);
        assert_eq!(report.by_rule.len(), SEM_CODES.len());
        assert_eq!(
            report
                .by_rule
                .iter()
                .map(|rule| rule.code)
                .collect::<Vec<_>>(),
            SEM_CODES.to_vec()
        );
    }

    #[test]
    fn non_semantic_codes_are_ignored() {
        let lexical = Diagnostic::lexical("LEX001", "bad", span(0, 1));
        let report = compare_semantic_diagnostics(&[lexical], &[]);
        assert!(report.matched);
        assert_eq!(report.legacy_count, 0);
        assert_eq!(report.ast_count, 0);
    }

    #[test]
    fn is_parity_requires_matched_and_complete() {
        for (matched, comparison_complete) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let report = EngineComparisonReport {
                matched,
                comparison_complete,
                details_truncated: false,
                legacy_count: 0,
                ast_count: 0,
                by_rule: Vec::new(),
            };
            assert_eq!(
                report.is_parity(),
                matched && comparison_complete,
                "matched={matched}, comparison_complete={comparison_complete}"
            );
        }
    }
}
