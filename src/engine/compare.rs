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

/// Maximum number of missing/extra fingerprints retained per rule.
pub const MAX_COMPARE_DETAILS_PER_RULE: usize = 64;

/// Maximum raw diagnostics accepted per engine before a comparison is declared
/// incomplete.
pub const MAX_COMPARE_FINGERPRINTS_PER_ENGINE: usize = 100_000;

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
    /// Exact number of Legacy findings with no AST multiset partner.
    ///
    /// Always exact, including when the overall comparison is incomplete.
    pub missing_total: usize,
    /// Exact number of AST findings with no Legacy multiset partner.
    ///
    /// Always exact, including when the overall comparison is incomplete.
    pub extra_total: usize,
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
    /// False when a raw input exceeded the comparator's safety budget.
    ///
    /// Counts and deltas remain exact; an incomplete report means only that the
    /// budget was exceeded and parity must not be asserted. `matched` is forced
    /// to `false` in that case.
    pub comparison_complete: bool,
    /// True when at least one rule's missing/extra total exceeds the detail cap.
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

fn collect_by_rule(diagnostics: &[Diagnostic]) -> Vec<Vec<DiagnosticFingerprint>> {
    let mut by_rule: Vec<Vec<DiagnosticFingerprint>> =
        (0..SEM_CODES.len()).map(|_| Vec::new()).collect();
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

/// Subtract two sorted fingerprint multisets element-by-element.
///
/// Equal fingerprints consume one occurrence from each side, so duplicate
/// multiplicity is preserved. The returned vectors keep the deterministic sort
/// order of their inputs.
fn multiset_delta(
    legacy: &[DiagnosticFingerprint],
    ast: &[DiagnosticFingerprint],
) -> (Vec<DiagnosticFingerprint>, Vec<DiagnosticFingerprint>) {
    let mut missing = Vec::new();
    let mut extra = Vec::new();
    let mut legacy_index = 0usize;
    let mut ast_index = 0usize;
    while legacy_index < legacy.len() && ast_index < ast.len() {
        match legacy[legacy_index].cmp(&ast[ast_index]) {
            Ordering::Less => {
                missing.push(legacy[legacy_index].clone());
                legacy_index += 1;
            }
            Ordering::Greater => {
                extra.push(ast[ast_index].clone());
                ast_index += 1;
            }
            Ordering::Equal => {
                legacy_index += 1;
                ast_index += 1;
            }
        }
    }
    missing.extend_from_slice(&legacy[legacy_index..]);
    extra.extend_from_slice(&ast[ast_index..]);
    (missing, extra)
}

/// Compare two raw semantic streams, filtering to `SEM001`..=`SEM023`.
///
/// All counts and deltas are exact in every call. Per-rule delta details are
/// boundary samples of at most [`MAX_COMPARE_DETAILS_PER_RULE`] fingerprints.
/// When either input exceeds [`MAX_COMPARE_FINGERPRINTS_PER_ENGINE`] the
/// comparison is declared incomplete and `matched` is forced to `false`, but
/// the exact per-rule counts and multiset deltas are still computed and
/// reported; `comparison_complete == false` means only that the budget was
/// exceeded, so parity must not be asserted.
pub fn compare_semantic_diagnostics(
    legacy: &[Diagnostic],
    ast: &[Diagnostic],
) -> EngineComparisonReport {
    let comparison_complete = legacy.len() <= MAX_COMPARE_FINGERPRINTS_PER_ENGINE
        && ast.len() <= MAX_COMPARE_FINGERPRINTS_PER_ENGINE;

    let legacy_by_rule = collect_by_rule(legacy);
    let ast_by_rule = collect_by_rule(ast);
    let mut by_rule = Vec::with_capacity(SEM_CODES.len());
    let mut details_truncated = false;
    let mut matched = true;
    for (index, code) in SEM_CODES.iter().enumerate() {
        let (missing, extra) = multiset_delta(&legacy_by_rule[index], &ast_by_rule[index]);
        let missing_total = missing.len();
        let extra_total = extra.len();
        if missing_total > MAX_COMPARE_DETAILS_PER_RULE
            || extra_total > MAX_COMPARE_DETAILS_PER_RULE
        {
            details_truncated = true;
        }
        if missing_total != 0 || extra_total != 0 {
            matched = false;
        }
        by_rule.push(RuleComparison {
            code,
            legacy_count: legacy_by_rule[index].len(),
            ast_count: ast_by_rule[index].len(),
            missing_total,
            extra_total,
            missing_from_ast: missing
                .into_iter()
                .take(MAX_COMPARE_DETAILS_PER_RULE)
                .collect(),
            extra_in_ast: extra
                .into_iter()
                .take(MAX_COMPARE_DETAILS_PER_RULE)
                .collect(),
        });
    }

    EngineComparisonReport {
        matched: comparison_complete && matched,
        comparison_complete,
        details_truncated,
        legacy_count: legacy_by_rule.iter().map(Vec::len).sum(),
        ast_count: ast_by_rule.iter().map(Vec::len).sum(),
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
        let (missing, extra) = multiset_delta(&[a.clone(), a.clone()], std::slice::from_ref(&a));
        assert_eq!(missing, vec![a.clone()]);
        assert!(extra.is_empty());

        let (missing, extra) = multiset_delta(std::slice::from_ref(&a), &[a.clone(), a.clone()]);
        assert!(missing.is_empty());
        assert_eq!(extra, vec![a]);
    }

    #[test]
    fn multiset_delta_returns_sorted_boundaries() {
        let legacy = vec![fingerprint("SEM001", 0), fingerprint("SEM001", 1)];
        let ast = vec![fingerprint("SEM001", 1), fingerprint("SEM001", 2)];
        let (missing, extra) = multiset_delta(&legacy, &ast);
        assert_eq!(missing, vec![fingerprint("SEM001", 0)]);
        assert_eq!(extra, vec![fingerprint("SEM001", 2)]);
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
