//! Engine comparator regression tests.
//!
//! The report is built from raw prepared-coordinate semantic streams. These
//! tests pin corpus parity, comparator multiset/order semantics, safety bounds,
//! determinism, correct stream wiring, and prepared-vs-original coordinates.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use neolint::ast::parse_ast_with_options;
use neolint::engine::compare::{
    compare_semantic_diagnostics, EngineComparisonReport, RuleComparison, SEM_CODES,
};
use neolint::preprocessor::PreprocessSettings;
use neolint::{
    lex, parse_legacy, AnalysisEngine, AnalysisRun, AstParseOptions, Diagnostic, DiagnosticKind,
    Severity, SourceFileKind, Span, MAX_COMPARE_DETAILS_PER_RULE,
    MAX_COMPARE_FINGERPRINTS_PER_ENGINE,
};

static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

fn temporary_directory() -> PathBuf {
    let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("neolint-engine-{}-{id}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn visit_lsl(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|value| value.to_str()) == Some("lsl") {
            files.push(path.to_path_buf());
        }
        return;
    }
    let mut entries: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for entry in entries {
        visit_lsl(&entry, files);
    }
}

fn span(start: usize, end: usize) -> Span {
    Span {
        start,
        end,
        line: start + 1,
        column: 1,
        end_line: end + 1,
        end_column: 1,
    }
}

fn diagnostic(code: &'static str) -> Diagnostic {
    Diagnostic {
        code,
        severity: Severity::Error,
        kind: DiagnosticKind::Semantic,
        message: "message".to_owned(),
        span: span(10, 12),
    }
}

fn rule<'a>(report: &'a EngineComparisonReport, code: &str) -> &'a RuleComparison {
    report
        .by_rule
        .iter()
        .find(|rule| rule.code == code)
        .expect("rule present")
}

const EXPECTED_CODES: [&str; 23] = [
    "SEM001", "SEM002", "SEM003", "SEM004", "SEM005", "SEM006", "SEM007", "SEM008", "SEM009",
    "SEM010", "SEM011", "SEM012", "SEM013", "SEM014", "SEM015", "SEM016", "SEM017", "SEM018",
    "SEM019", "SEM020", "SEM021", "SEM022", "SEM023",
];

fn assert_codes_in_order(report: &EngineComparisonReport) {
    assert_eq!(
        report
            .by_rule
            .iter()
            .map(|rule| rule.code)
            .collect::<Vec<_>>(),
        EXPECTED_CODES.to_vec()
    );
}

#[test]
fn corpus_compare_reports_full_parity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus");
    let mut files = Vec::new();
    visit_lsl(&root, &mut files);
    assert!(!files.is_empty(), "no corpus .lsl files discovered");

    let mut run = AnalysisRun::new().unwrap();
    for path in &files {
        let source = fs::read_to_string(path).unwrap();
        run.add_prepared_text(&path.display().to_string(), &source)
            .unwrap();
    }
    let analysis = run.analyze_with_engine(AnalysisEngine::Compare).unwrap();
    assert_eq!(
        analysis.comparisons().len(),
        files.len(),
        "expected one report per discovered fixture ({} fixtures)",
        files.len()
    );
    for comparison in analysis.comparisons() {
        let path = &files[comparison.unit_index];
        let report = &comparison.report;
        assert!(
            report.matched,
            "{}: comparator mismatch among {} fixtures: {report:?}",
            path.display(),
            files.len()
        );
        assert!(
            report.comparison_complete,
            "{}: comparison unexpectedly incomplete",
            path.display()
        );
        assert_eq!(
            report.by_rule.len(),
            SEM_CODES.len(),
            "{}: every rule must be reported",
            path.display()
        );
        assert_codes_in_order(report);
    }

    let total_legacy: usize = analysis
        .comparisons()
        .iter()
        .map(|comparison| comparison.report.legacy_count)
        .sum();
    assert!(
        total_legacy > 0,
        "corpus discovery produced no compared Legacy findings"
    );
}

#[test]
fn focused_sources_covering_rule_families_match() {
    let sources = [
        "integer g;\ninteger g;\ndefault { state_entry() { } }",
        "default { state_entry() { llSay(0); } }",
        "integer f(integer a) { return a; }\ndefault { state_entry() { f(); } }",
        "default { state_entry() { integer a; integer b; a == b; } }",
        "default { state_entry() { foo(); } }",
        "default { state_entry() { } state_entry() { } }",
        "state active { state_entry() { } }\ndefault { state_entry() { state missing; } }",
        "default { state_entry() { integer a; integer b; if (a) b = 1; } }",
    ];
    let mut run = AnalysisRun::new().unwrap();
    for (index, source) in sources.iter().enumerate() {
        run.add_prepared_text(&format!("focused{index}.lsl"), source)
            .unwrap();
    }
    let analysis = run.analyze_with_engine(AnalysisEngine::Compare).unwrap();
    assert_eq!(analysis.comparisons().len(), sources.len());
    let mut non_zero_rules = 0usize;
    for (comparison, source) in analysis.comparisons().iter().zip(sources.iter()) {
        let report = &comparison.report;
        assert!(report.matched, "focused source {source:?}: {report:?}");
        assert!(report.comparison_complete);
        non_zero_rules += report
            .by_rule
            .iter()
            .filter(|rule| rule.legacy_count != 0)
            .count();
    }
    assert!(
        non_zero_rules >= 4,
        "focused fixtures should cover several rules, got {non_zero_rules}"
    );
}

#[test]
fn every_fingerprint_field_is_significant() {
    let base = diagnostic("SEM001");
    let mut perturbed = vec![
        Diagnostic {
            code: "SEM002",
            ..base.clone()
        },
        Diagnostic {
            severity: Severity::Warning,
            ..base.clone()
        },
        Diagnostic {
            kind: DiagnosticKind::Lint,
            ..base.clone()
        },
        Diagnostic {
            message: "different".to_owned(),
            ..base.clone()
        },
    ];
    for (field, value) in [
        ("start", 11),
        ("end", 13),
        ("line", 12),
        ("column", 2),
        ("end_line", 14),
        ("end_column", 2),
    ] {
        let mut span = base.span;
        match field {
            "start" => span.start = value,
            "end" => span.end = value,
            "line" => span.line = value,
            "column" => span.column = value,
            "end_line" => span.end_line = value,
            "end_column" => span.end_column = value,
            _ => unreachable!(),
        }
        perturbed.push(Diagnostic {
            span,
            ..base.clone()
        });
    }

    let baseline =
        compare_semantic_diagnostics(std::slice::from_ref(&base), std::slice::from_ref(&base));
    assert!(baseline.matched);

    for (index, changed) in perturbed.iter().enumerate() {
        let report = compare_semantic_diagnostics(
            std::slice::from_ref(&base),
            std::slice::from_ref(changed),
        );
        assert_eq!(report.legacy_count, 1, "perturbation {index}");
        assert_eq!(report.ast_count, 1, "perturbation {index}");
        assert!(
            !report.matched,
            "perturbation {index} must be detected: {report:?}"
        );
    }
}

#[test]
fn multiplicity_is_preserved_in_both_directions() {
    let a = diagnostic("SEM001");

    let report = compare_semantic_diagnostics(&[a.clone(), a.clone()], std::slice::from_ref(&a));
    let sem001 = rule(&report, "SEM001");
    assert_eq!(sem001.missing_total, Some(1));
    assert_eq!(sem001.extra_total, Some(0));
    assert_eq!(sem001.missing_from_ast.len(), 1);
    assert!(sem001.extra_in_ast.is_empty());
    assert!(!report.matched);

    let legacy = [a.clone()];
    let report = compare_semantic_diagnostics(&legacy, &[a.clone(), a]);
    let sem001 = rule(&report, "SEM001");
    assert_eq!(sem001.missing_total, Some(0));
    assert_eq!(sem001.extra_total, Some(1));
    assert!(sem001.missing_from_ast.is_empty());
    assert_eq!(sem001.extra_in_ast.len(), 1);
    assert!(!report.matched);
}

#[test]
fn legacy_only_and_ast_only_rules_are_grouped_correctly() {
    let legacy_only = diagnostic("SEM001");
    let ast_only = diagnostic("SEM002");
    let report = compare_semantic_diagnostics(
        std::slice::from_ref(&legacy_only),
        std::slice::from_ref(&ast_only),
    );
    let first = rule(&report, "SEM001");
    assert_eq!((first.legacy_count, first.ast_count), (1, 0));
    assert_eq!((first.missing_total, first.extra_total), (Some(1), Some(0)));
    let second = rule(&report, "SEM002");
    assert_eq!((second.legacy_count, second.ast_count), (0, 1));
    assert_eq!(
        (second.missing_total, second.extra_total),
        (Some(0), Some(1))
    );
    for code in &SEM_CODES[2..] {
        let rule = rule(&report, code);
        assert_eq!(
            (
                rule.legacy_count,
                rule.ast_count,
                rule.missing_total,
                rule.extra_total
            ),
            (0, 0, Some(0), Some(0)),
            "{code} must stay zero"
        );
    }
    assert!(!report.matched);
    assert!(!report.details_truncated);
}

#[test]
fn per_rule_detail_samples_are_bounded() {
    let legacy: Vec<_> = (0..65)
        .map(|start| Diagnostic {
            span: span(start, start + 1),
            ..diagnostic("SEM001")
        })
        .collect();
    let report = compare_semantic_diagnostics(&legacy, &[]);
    let sem001 = rule(&report, "SEM001");
    assert_eq!(sem001.legacy_count, 65);
    assert_eq!(sem001.ast_count, 0);
    assert_eq!(sem001.missing_total, Some(65));
    assert_eq!(sem001.extra_total, Some(0));
    assert_eq!(sem001.missing_from_ast.len(), MAX_COMPARE_DETAILS_PER_RULE);
    assert_eq!(sem001.missing_from_ast.len(), 64);
    assert!(sem001.extra_in_ast.is_empty());
    assert!(report.details_truncated);
    assert!(!report.matched);
}

#[test]
fn comparison_completeness_flag_is_bound_aware() {
    let at_bound = vec![diagnostic("SEM001"); MAX_COMPARE_FINGERPRINTS_PER_ENGINE];
    let report = compare_semantic_diagnostics(&at_bound, &at_bound);
    assert!(report.comparison_complete);
    assert!(report.matched);
    assert!(!report.details_truncated);
    assert_eq!(report.legacy_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE);
    assert_eq!(report.ast_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE);
    assert_eq!(
        rule(&report, "SEM001").legacy_count,
        MAX_COMPARE_FINGERPRINTS_PER_ENGINE
    );
    assert!(
        report
            .by_rule
            .iter()
            .all(|rule| rule.missing_total == Some(0) && rule.extra_total == Some(0)),
        "an exactly-at-budget comparison must report exact zero totals"
    );

    let report = compare_semantic_diagnostics(&at_bound, &at_bound[..1]);
    assert!(report.comparison_complete);
    assert!(!report.matched);
    assert!(
        report.details_truncated,
        "a 99,999-element delta must be reported as truncated"
    );
    assert_eq!(
        rule(&report, "SEM001").missing_total,
        Some(MAX_COMPARE_FINGERPRINTS_PER_ENGINE - 1)
    );

    let small = vec![diagnostic("SEM001"); 2];
    let report = compare_semantic_diagnostics(&small, &small);
    assert!(report.comparison_complete);
    assert!(report.matched);

    // Equal multisets stay matched even when each rule holds more findings than
    // the detail sample can retain. Under the normative definition,
    // `details_truncated` tracks missing/extra totals, which are zero here; the
    // bound therefore does not affect `matched`.
    let large_equal = vec![diagnostic("SEM001"); MAX_COMPARE_DETAILS_PER_RULE + 1];
    let report = compare_semantic_diagnostics(&large_equal, &large_equal);
    assert!(report.comparison_complete);
    assert!(report.matched);
    assert!(!report.details_truncated);
    assert_eq!(rule(&report, "SEM001").missing_total, Some(0));
    assert_eq!(rule(&report, "SEM001").extra_total, Some(0));
}

#[test]
fn over_budget_stream_reports_unknown_totals_and_no_samples() {
    let too_many = vec![diagnostic("SEM001"); MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
    let report = compare_semantic_diagnostics(&too_many, &too_many[..1]);
    assert!(!report.comparison_complete);
    assert!(!report.matched);
    assert!(!report.details_truncated);
    assert_eq!(report.legacy_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1);
    assert_eq!(report.ast_count, 1);
    assert_eq!(report.by_rule.len(), SEM_CODES.len());
    assert_eq!(
        rule(&report, "SEM001").legacy_count,
        MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1
    );
    assert_eq!(rule(&report, "SEM001").ast_count, 1);
    for rule in &report.by_rule {
        // Unknown totals are structurally distinguishable from computed ones:
        // a performed comparison would always produce `Some(n)`.
        assert_eq!(rule.missing_total, None, "{}", rule.code);
        assert_eq!(rule.extra_total, None, "{}", rule.code);
        assert!(rule.missing_from_ast.is_empty(), "{}", rule.code);
        assert!(rule.extra_in_ast.is_empty(), "{}", rule.code);
    }

    // Either engine over budget selects the incomplete path.
    let reversed = compare_semantic_diagnostics(&too_many[..1], &too_many);
    assert!(!reversed.comparison_complete);
    assert!(!reversed.matched);
    assert!(!reversed.details_truncated);
    assert_eq!(reversed.legacy_count, 1);
    assert_eq!(reversed.ast_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1);
    assert!(reversed.by_rule.iter().all(|rule| {
        rule.missing_total.is_none()
            && rule.extra_total.is_none()
            && rule.missing_from_ast.is_empty()
            && rule.extra_in_ast.is_empty()
    }));
}

#[test]
fn fingerprint_budget_counts_only_sem_diagnostics() {
    let lexical = Diagnostic::lexical("LEX001", "bad", span(0, 1));
    let mut legacy = vec![lexical; MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
    legacy.push(diagnostic("SEM001"));
    let ast = vec![diagnostic("SEM001")];

    let report = compare_semantic_diagnostics(&legacy, &ast);
    assert!(
        report.comparison_complete,
        "raw non-SEM stream length must not trip the fingerprint budget"
    );
    assert!(report.matched, "{report:?}");
    assert_eq!(report.legacy_count, 1);
    assert_eq!(report.ast_count, 1);
    assert_eq!(rule(&report, "SEM001").legacy_count, 1);
    assert_eq!(rule(&report, "SEM001").missing_total, Some(0));
    assert_eq!(rule(&report, "SEM001").extra_total, Some(0));
}

#[test]
fn fingerprint_budget_applies_to_the_total_across_rules() {
    let half = MAX_COMPARE_FINGERPRINTS_PER_ENGINE / 2;

    // Two buckets each stay below the per-engine limit, but their sum does not;
    // the budget is on the total SEM fingerprint count.
    let mut over = vec![diagnostic("SEM001"); half + 1];
    over.extend(vec![diagnostic("SEM002"); half + 1]);
    assert_eq!(over.len(), MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 2);
    let report = compare_semantic_diagnostics(&over, &[]);
    assert!(!report.comparison_complete);
    assert!(!report.matched);
    assert_eq!(report.legacy_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 2);
    assert_eq!(rule(&report, "SEM001").legacy_count, half + 1);
    assert_eq!(rule(&report, "SEM002").legacy_count, half + 1);
    assert_eq!(rule(&report, "SEM001").missing_total, None);
    assert_eq!(rule(&report, "SEM002").missing_total, None);

    // Exactly at the summed limit remains a performed comparison.
    let mut at_bound = vec![diagnostic("SEM001"); half];
    at_bound.extend(vec![diagnostic("SEM002"); half]);
    assert_eq!(at_bound.len(), MAX_COMPARE_FINGERPRINTS_PER_ENGINE);
    let report = compare_semantic_diagnostics(&at_bound, &at_bound);
    assert!(report.comparison_complete);
    assert!(report.matched);
    assert_eq!(report.legacy_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE);
    assert_eq!(rule(&report, "SEM001").missing_total, Some(0));
    assert_eq!(rule(&report, "SEM002").extra_total, Some(0));
}

#[test]
fn fingerprint_budget_ignores_non_sem_ast_stream() {
    let lexical = Diagnostic::lexical("LEX001", "bad", span(0, 1));
    let mut ast = vec![lexical; MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
    ast.push(diagnostic("SEM001"));
    let legacy = vec![diagnostic("SEM001")];

    let report = compare_semantic_diagnostics(&legacy, &ast);
    assert!(
        report.comparison_complete,
        "a large non-SEM AST stream must not trip the fingerprint budget"
    );
    assert!(report.matched, "{report:?}");
    assert_eq!(report.legacy_count, 1);
    assert_eq!(report.ast_count, 1);
    assert_eq!(rule(&report, "SEM001").ast_count, 1);
    assert_eq!(rule(&report, "SEM001").missing_total, Some(0));
    assert_eq!(rule(&report, "SEM001").extra_total, Some(0));
}

#[test]
fn duplicate_multiplicity_beyond_the_detail_cap_stays_exact() {
    let a = diagnostic("SEM001");
    let legacy = vec![a.clone(); 100];
    let ast = vec![a; 10];
    let report = compare_semantic_diagnostics(&legacy, &ast);
    let sem001 = rule(&report, "SEM001");
    assert_eq!(sem001.legacy_count, 100);
    assert_eq!(sem001.ast_count, 10);
    assert_eq!(sem001.missing_total, Some(90));
    assert_eq!(sem001.extra_total, Some(0));
    assert_eq!(sem001.missing_from_ast.len(), MAX_COMPARE_DETAILS_PER_RULE);
    assert!(report.details_truncated);
    assert!(!report.matched);
}

#[test]
fn incomplete_report_keeps_per_rule_counts_exact() {
    let mut input = vec![diagnostic("SEM001"); MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
    input.extend(vec![diagnostic("SEM023"); 5]);

    let report = compare_semantic_diagnostics(&input, &[]);
    assert!(!report.comparison_complete);
    assert!(!report.matched);
    assert_eq!(report.legacy_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 6);
    assert_eq!(
        rule(&report, "SEM001").legacy_count,
        MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1
    );
    assert_eq!(rule(&report, "SEM023").legacy_count, 5);
    assert_eq!(rule(&report, "SEM001").ast_count, 0);
    assert_eq!(rule(&report, "SEM023").ast_count, 0);
    for rule in &report.by_rule {
        assert_eq!(rule.missing_total, None, "{}", rule.code);
        assert_eq!(rule.extra_total, None, "{}", rule.code);
    }
}

#[test]
fn near_miss_codes_are_not_compared_or_budgeted() {
    let near_misses = ["SEM01", "SEM0001", "sem001", "SEM024", "SEM027"];
    let input: Vec<Diagnostic> = (0..=MAX_COMPARE_FINGERPRINTS_PER_ENGINE)
        .map(|index| diagnostic(near_misses[index % near_misses.len()]))
        .collect();

    let report = compare_semantic_diagnostics(&input, &[]);
    assert!(
        report.comparison_complete,
        "near-miss codes must not trip the SEM fingerprint budget"
    );
    assert!(report.matched);
    assert_eq!(report.legacy_count, 0);
    assert_eq!(report.ast_count, 0);
    assert!(report.by_rule.iter().all(|rule| {
        rule.legacy_count == 0
            && rule.ast_count == 0
            && rule.missing_total == Some(0)
            && rule.extra_total == Some(0)
    }));
}

#[test]
fn ast_only_codes_are_ignored_by_the_comparator() {
    assert_eq!(SEM_CODES.len(), 23);
    for code in [
        "SEM024", "SEM025", "SEM026", "SEM027", "SEM028", "SEM029", "SEM030", "SEM031", "SEM032",
        "SEM033", "SEM034", "SEM035", "SEM036", "SEM037", "SEM038", "SEM039", "SEM040", "SEM041",
        "SEM042", "SEM043", "SEM044", "SEM045", "SEM046",
    ] {
        assert!(
            !SEM_CODES.contains(&code),
            "{code} must stay outside the compared Legacy rule set"
        );
    }

    let ast = [
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM024")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: "parameter 'value' is never read".to_owned(),
            ..diagnostic("SEM025")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: "function 'helper' is never called".to_owned(),
            ..diagnostic("SEM026")
        },
        diagnostic("SEM027"),
        diagnostic("SEM028"),
        diagnostic("SEM029"),
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM030")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM031")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM032")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM033")
        },
        diagnostic("SEM034"),
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM035")
        },
        diagnostic("SEM036"),
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM037")
        },
        diagnostic("SEM038"),
        diagnostic("SEM039"),
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM040")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM041")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM042")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM043")
        },
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            ..diagnostic("SEM044")
        },
        diagnostic("SEM045"),
        diagnostic("SEM046"),
    ];

    let report = compare_semantic_diagnostics(&[], &ast);
    assert!(report.matched, "{report:?}");
    assert!(report.comparison_complete);
    assert!(report.is_parity());
    assert_eq!(report.legacy_count, 0);
    assert_eq!(report.ast_count, 0);
    assert_codes_in_order(&report);
    assert!(report.by_rule.iter().all(|rule| {
        rule.legacy_count == 0
            && rule.ast_count == 0
            && rule.missing_total == Some(0)
            && rule.extra_total == Some(0)
    }));
}

#[test]
fn extra_detail_samples_are_bounded_and_sorted() {
    let ast: Vec<_> = (0..65)
        .map(|start| Diagnostic {
            span: span(start, start + 1),
            ..diagnostic("SEM001")
        })
        .collect();
    let report = compare_semantic_diagnostics(&[], &ast);
    let sem001 = rule(&report, "SEM001");
    assert_eq!(sem001.legacy_count, 0);
    assert_eq!(sem001.ast_count, 65);
    assert_eq!(sem001.extra_total, Some(65));
    assert_eq!(sem001.extra_in_ast.len(), MAX_COMPARE_DETAILS_PER_RULE);
    assert!(sem001.missing_from_ast.is_empty());
    assert!(report.details_truncated);
    let starts: Vec<usize> = sem001
        .extra_in_ast
        .iter()
        .map(|fingerprint| fingerprint.start)
        .collect();
    assert_eq!(
        starts,
        (0..MAX_COMPARE_DETAILS_PER_RULE).collect::<Vec<_>>()
    );

    let ast: Vec<_> = (0..MAX_COMPARE_DETAILS_PER_RULE)
        .map(|start| Diagnostic {
            span: span(start, start + 1),
            ..diagnostic("SEM001")
        })
        .collect();
    let report = compare_semantic_diagnostics(&[], &ast);
    let sem001 = rule(&report, "SEM001");
    assert_eq!(sem001.extra_total, Some(MAX_COMPARE_DETAILS_PER_RULE));
    assert_eq!(sem001.extra_in_ast.len(), MAX_COMPARE_DETAILS_PER_RULE);
    assert!(!report.details_truncated);
    assert!(!report.matched);
    assert_eq!(
        sem001
            .extra_in_ast
            .iter()
            .map(|fingerprint| fingerprint.start)
            .collect::<Vec<_>>(),
        (0..MAX_COMPARE_DETAILS_PER_RULE).collect::<Vec<_>>()
    );
}

#[test]
fn permuted_inputs_produce_identical_reports() {
    let a = Diagnostic {
        span: span(0, 1),
        ..diagnostic("SEM001")
    };
    let b = Diagnostic {
        span: span(1, 2),
        ..diagnostic("SEM001")
    };
    let c = Diagnostic {
        span: span(2, 3),
        ..diagnostic("SEM001")
    };
    let d = Diagnostic {
        span: span(10, 11),
        ..diagnostic("SEM001")
    };
    let first =
        compare_semantic_diagnostics(&[a.clone(), b.clone(), c.clone()], &[b.clone(), d.clone()]);
    let second = compare_semantic_diagnostics(&[c, b.clone(), a], &[d, b]);
    assert_eq!(first, second);
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
    let missing: Vec<usize> = rule(&first, "SEM001")
        .missing_from_ast
        .iter()
        .map(|fingerprint| fingerprint.start)
        .collect();
    assert_eq!(missing, vec![0, 2], "delta order is deterministic");
    assert_eq!(rule(&first, "SEM001").extra_total, Some(1));
}

#[test]
fn reports_are_deterministic_and_ordered() {
    let legacy = vec![
        diagnostic("SEM002"),
        diagnostic("SEM001"),
        Diagnostic {
            span: span(1, 2),
            ..diagnostic("SEM001")
        },
    ];
    let ast = vec![diagnostic("SEM002")];
    let first = compare_semantic_diagnostics(&legacy, &ast);
    let second = compare_semantic_diagnostics(&legacy, &ast);
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
    assert_codes_in_order(&first);
    assert_eq!(
        first
            .by_rule
            .iter()
            .map(|rule| rule.code)
            .collect::<Vec<_>>(),
        EXPECTED_CODES.to_vec()
    );
    assert!(!first.matched);
}

/// The engine runs each analyzer against its own parser findings, and the report
/// consumes exactly those raw semantic streams. On this fixture both engines
/// agree, so the report is complete with zero deltas.
///
/// No repository input currently makes a swapped wiring observable for
/// SEM001..=SEM023: the Legacy and AST parsers honor the same suppression
/// contract on suppression-relevant positions. Cross-wiring is prevented
/// structurally by the private `LegacySyntax`/`AstSyntax` split in
/// `src/engine/wiring.rs`, where a swapped slice is a type error.
#[test]
fn compare_consumes_independently_wired_semantic_streams() {
    let source = "integer g;\ninteger g;\ndefault { state_entry() { integer x = ; llSay(0); } }";
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("root.lsl", source).unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    let analysis = unit.analyze_with_engine(AnalysisEngine::Compare).unwrap();

    let (tokens, lexical) = lex(prepared.text());
    let legacy_syntax = parse_legacy(&tokens);
    let legacy_semantic =
        neolint::semantic::analyze_with_syntax_diagnostics(&tokens, &legacy_syntax);
    let (ast, ast_syntax) = parse_ast_with_options(
        &tokens,
        AstParseOptions::from_flags(prepared.switches_enabled(), prepared.lazy_lists_enabled()),
    );
    let ast_semantic = neolint::ast_semantic::analyze_ast(&ast, &ast_syntax);
    let oracle = compare_semantic_diagnostics(&legacy_semantic, &ast_semantic);

    assert_eq!(analysis.comparison(), Some(&oracle));
    assert!(oracle.comparison_complete);
    assert!(oracle.matched, "{oracle:?}");
    assert!(oracle
        .by_rule
        .iter()
        .all(|rule| rule.missing_total == Some(0) && rule.extra_total == Some(0)));

    // Compare displays the frozen Legacy stream, never `ast_semantic`.
    let expected: Vec<Diagnostic> = lexical
        .into_iter()
        .chain(legacy_syntax)
        .chain(legacy_semantic)
        .collect();
    assert_eq!(analysis.diagnostics().len(), expected.len());
    for (rendered, raw) in analysis.diagnostics().iter().zip(expected.iter()) {
        assert_eq!(rendered.diagnostic(), raw);
    }
}

#[test]
fn report_fingerprints_use_prepared_coordinates_while_display_maps_back() {
    let source =
        "#define WIDTH 2\ninteger g;\ninteger g;\ndefault { state_entry() { integer x = WIDTH; } }\n";
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_text("root.lsl", source, PreprocessSettings::full())
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    assert_ne!(
        prepared.prepared_len(),
        source.len(),
        "preprocessing must change the byte layout"
    );

    let analysis = unit.analyze_with_engine(AnalysisEngine::Compare).unwrap();
    assert!(analysis.comparison().unwrap().matched);

    let (tokens, _lexical) = lex(prepared.text());
    let legacy_syntax = parse_legacy(&tokens);
    let legacy_semantic =
        neolint::semantic::analyze_with_syntax_diagnostics(&tokens, &legacy_syntax);
    let sem001 = legacy_semantic
        .iter()
        .find(|diagnostic| diagnostic.code == "SEM001")
        .expect("duplicate global diagnostic");
    let prepared_start = sem001.span.start;
    let prepared_end = sem001.span.end;
    assert_eq!(&prepared.text()[prepared_start..prepared_end], "g");
    let token_at_prepared_start = tokens
        .iter()
        .find(|token| token.span.start == prepared_start)
        .expect("token starts at the raw fingerprint offset");
    assert_eq!(token_at_prepared_start.lexeme, "g");

    // The real report has no deltas (both engines agree), so exercise the
    // coordinate-carrying fingerprint seam by ablating the matching AST record.
    let (ast, ast_syntax) = parse_ast_with_options(
        &tokens,
        AstParseOptions::from_flags(prepared.switches_enabled(), prepared.lazy_lists_enabled()),
    );
    let ablated_ast: Vec<Diagnostic> = neolint::ast_semantic::analyze_ast(&ast, &ast_syntax)
        .into_iter()
        .filter(|diagnostic| diagnostic.code != "SEM001")
        .collect();
    let ablated = compare_semantic_diagnostics(&legacy_semantic, &ablated_ast);
    let fingerprint = &rule(&ablated, "SEM001").missing_from_ast[0];
    assert_eq!(fingerprint.start, prepared_start);
    assert_eq!(fingerprint.end, prepared_end);
    assert_eq!(&prepared.text()[fingerprint.start..fingerprint.end], "g");

    // The displayed diagnostic carries original-source coordinates instead.
    let displayed = analysis
        .diagnostics()
        .iter()
        .find(|rendered| rendered.diagnostic().code == "SEM001")
        .expect("displayed SEM001");
    let displayed_span = displayed.diagnostic().span;
    assert_eq!(displayed.source(), unit.root().unwrap());
    assert_ne!(displayed_span.start, prepared_start);
    assert_eq!(&source[displayed_span.start..displayed_span.end], "g");
    assert_eq!(
        displayed_span.start, 35,
        "original-source offset of the second `g`"
    );
    assert_eq!(
        prepared_start, 20,
        "prepared-coordinate offset of the same token"
    );
}

#[test]
fn include_originated_report_fingerprints_use_prepared_coordinates() {
    let directory = temporary_directory();
    fs::write(directory.join("inc.lsl"), "integer duplicate;\n").unwrap();
    fs::write(
        directory.join("root.lsl"),
        "integer duplicate;\n#include \"inc.lsl\"\ndefault { state_entry() { } }\n",
    )
    .unwrap();

    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_file(directory.join("root.lsl"), PreprocessSettings::full())
        .unwrap();
    let unit = run.unit(index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    assert!(
        prepared.text().matches("integer duplicate;").count() >= 2,
        "included declaration participates in prepared text"
    );
    let analysis = unit.analyze_with_engine(AnalysisEngine::Compare).unwrap();
    assert!(analysis.comparison().unwrap().matched);

    let (tokens, _lexical) = lex(prepared.text());
    let legacy_syntax = parse_legacy(&tokens);
    let legacy_semantic =
        neolint::semantic::analyze_with_syntax_diagnostics(&tokens, &legacy_syntax);
    let sem001 = legacy_semantic
        .iter()
        .find(|diagnostic| diagnostic.code == "SEM001")
        .expect("duplicate global diagnostic from included file");
    let prepared_start = sem001.span.start;
    let prepared_end = sem001.span.end;
    assert_eq!(&prepared.text()[prepared_start..prepared_end], "duplicate");
    let token_at_prepared_start = tokens
        .iter()
        .find(|token| token.span.start == prepared_start)
        .expect("token starts at the raw fingerprint offset");
    assert_eq!(token_at_prepared_start.lexeme, "duplicate");

    // The real report is matched, so exercise the coordinate-carrying
    // fingerprint seam by ablating the matching AST record.
    let (ast, ast_syntax) = parse_ast_with_options(
        &tokens,
        AstParseOptions::from_flags(prepared.switches_enabled(), prepared.lazy_lists_enabled()),
    );
    let ablated_ast: Vec<Diagnostic> = neolint::ast_semantic::analyze_ast(&ast, &ast_syntax)
        .into_iter()
        .filter(|diagnostic| diagnostic.code != "SEM001")
        .collect();
    let ablated = compare_semantic_diagnostics(&legacy_semantic, &ablated_ast);
    let fingerprint = &rule(&ablated, "SEM001").missing_from_ast[0];
    assert_eq!(fingerprint.start, prepared_start);
    assert_eq!(fingerprint.end, prepared_end);
    assert_eq!(
        &prepared.text()[fingerprint.start..fingerprint.end],
        "duplicate"
    );

    let displayed = analysis
        .diagnostics()
        .iter()
        .find(|rendered| rendered.diagnostic().code == "SEM001")
        .expect("displayed SEM001");
    let included = unit
        .manager()
        .files()
        .iter()
        .find(|file| file.kind() == SourceFileKind::Included)
        .expect("included source registered");
    assert_eq!(displayed.source(), included.id());
    let displayed_span = displayed.diagnostic().span;
    assert_eq!(
        &included.text()[displayed_span.start..displayed_span.end],
        "duplicate"
    );
    assert_ne!(displayed_span.start, prepared_start);
    assert!(displayed_span.end <= included.text().len());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn incomplete_but_not_truncated_is_reported_separately() {
    let huge = vec![diagnostic("SEM001"); MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
    let report = compare_semantic_diagnostics(&huge, &huge);
    assert!(!report.comparison_complete);
    assert!(!report.details_truncated);
    assert!(!report.matched);
    assert_eq!(report.legacy_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1);
    assert_eq!(report.ast_count, MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1);
    assert_eq!(rule(&report, "SEM001").missing_total, None);
    assert_eq!(rule(&report, "SEM001").extra_total, None);
    assert!(rule(&report, "SEM001").missing_from_ast.is_empty());
    assert!(rule(&report, "SEM001").extra_in_ast.is_empty());
}

#[test]
fn missing_detail_sample_is_bounded_at_exactly_sixty_four() {
    let at_limit: Vec<_> = (0..MAX_COMPARE_DETAILS_PER_RULE)
        .map(|start| Diagnostic {
            span: span(start, start + 1),
            ..diagnostic("SEM001")
        })
        .collect();
    let report = compare_semantic_diagnostics(&at_limit, &[]);
    let sem001 = rule(&report, "SEM001");
    assert_eq!(sem001.missing_from_ast.len(), 64);
    assert_eq!(sem001.missing_total, Some(64));
    assert!(!report.details_truncated);
    assert!(!report.matched);

    let over_limit: Vec<_> = (0..MAX_COMPARE_DETAILS_PER_RULE + 1)
        .map(|start| Diagnostic {
            span: span(start, start + 1),
            ..diagnostic("SEM001")
        })
        .collect();
    let report = compare_semantic_diagnostics(&over_limit, &[]);
    assert!(report.details_truncated);
    assert_eq!(rule(&report, "SEM001").missing_from_ast.len(), 64);
    assert_eq!(
        rule(&report, "SEM001").missing_total,
        Some(MAX_COMPARE_DETAILS_PER_RULE + 1)
    );
}

#[test]
fn every_migrated_code_is_collected() {
    for code in SEM_CODES {
        let report = compare_semantic_diagnostics(&[diagnostic(code)], &[]);
        assert_eq!(report.legacy_count, 1, "{code}");
        assert_eq!(report.ast_count, 0, "{code}");
        let rule = rule(&report, code);
        assert_eq!(rule.legacy_count, 1, "{code}");
        assert_eq!(rule.missing_total, Some(1), "{code}");
        assert_eq!(rule.missing_from_ast.len(), 1, "{code}");
    }
}

#[test]
fn absolute_budget_and_detail_limits_are_pinned() {
    assert_eq!(MAX_COMPARE_FINGERPRINTS_PER_ENGINE, 100_000);
    assert_eq!(MAX_COMPARE_DETAILS_PER_RULE, 64);
}
