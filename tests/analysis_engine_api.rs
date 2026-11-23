//! Engine-aware library API regression tests.
//!
//! These tests pin selector parsing, Legacy compatibility, prepared-source
//! preflight, per-unit/run envelopes, token collection, and error signatures.

use neolint::preprocessor::PreprocessSettings;
use neolint::{
    lex, parser::parse_legacy, AnalysisEngine, AnalysisRun, AstParseOptions, DiagnosticKind,
    DisplaySuppression, EngineAnalysisError, EngineAnalysisOptions, RenderedDiagnostic, Severity,
    Span, SpanError,
};

fn assert_legacy_equal(left: &[RenderedDiagnostic], right: &[RenderedDiagnostic]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right.iter()) {
        assert_eq!(left.diagnostic(), right.diagnostic());
        assert_eq!(left.source(), right.source());
        assert_eq!(left.related(), right.related());
    }
}

#[test]
fn selector_contract_is_exact() {
    assert_eq!(AnalysisEngine::default(), AnalysisEngine::Ast);
    for (value, expected) in [
        ("legacy", AnalysisEngine::Legacy),
        ("ast", AnalysisEngine::Ast),
        ("compare", AnalysisEngine::Compare),
    ] {
        assert_eq!(AnalysisEngine::parse(value), Some(expected));
        assert_eq!(AnalysisEngine::parse(expected.as_str()), Some(expected));
    }
    for rejected in ["", "Legacy", "AST", " legacy", "compare ", "astt", "lsl"] {
        assert_eq!(AnalysisEngine::parse(rejected), None, "{rejected:?}");
    }
    assert!(!AnalysisEngine::Legacy.requires_prepared_source());
    assert!(AnalysisEngine::Ast.requires_prepared_source());
    assert!(AnalysisEngine::Compare.requires_prepared_source());
}

#[test]
fn legacy_parse_alias_is_identical_to_parse_legacy() {
    for source in [
        "integer g;\ndefault { state_entry() { llSay(0); } }",
        "default { state_entry() { integer x = ; llSay(0); } }",
        "#define WIDTH 2\ninteger x = WIDTH;\ndefault { state_entry() { } }\n",
    ] {
        let (tokens, _) = lex(source);
        assert_eq!(neolint::parse(&tokens), parse_legacy(&tokens), "{source:?}");
    }
}

#[test]
fn legacy_engine_equals_frozen_analyze_for_plain_and_prepared_units() {
    let source = "integer g;\ninteger g;\ndefault { state_entry() { integer x = ; llSay(0); } }";
    let mut run = AnalysisRun::new().unwrap();
    let plain = run.add_legacy_text("plain.lsl", source).unwrap();
    let prepared = run.add_prepared_text("prepared.lsl", source).unwrap();

    for index in [plain, prepared] {
        let unit = run.unit(index).unwrap();
        let frozen = unit.analyze().unwrap();
        let legacy = unit.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
        assert_legacy_equal(&frozen, legacy.diagnostics());
        assert!(legacy.comparison().is_none());
        assert!(legacy.tokens().is_empty());
    }
}

#[test]
fn ast_and_compare_reject_plain_units_without_touching_them() {
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_legacy_text("plain.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    let unit = run.unit(index).unwrap();
    let expected_id = unit.id();
    for engine in [AnalysisEngine::Ast, AnalysisEngine::Compare] {
        assert_eq!(
            unit.analyze_with_engine(engine).unwrap_err(),
            EngineAnalysisError::PreparedSourceRequired { unit: expected_id }
        );
        assert!(unit.prepared_source().is_none());
        assert!(unit.analyze().is_ok());
    }
}

#[test]
fn ast_engine_returns_exact_ast_codes_and_spans() {
    let source = "integer g;\ninteger g;\ndefault { state_entry() { llSay(0); } }";
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("root.lsl", source).unwrap();
    let analysis = run
        .unit(index)
        .unwrap()
        .analyze_with_engine(AnalysisEngine::Ast)
        .unwrap();

    let summary: Vec<_> = analysis
        .diagnostics()
        .iter()
        .map(|rendered| {
            let diagnostic = rendered.diagnostic();
            (
                diagnostic.code,
                diagnostic.severity,
                diagnostic.kind,
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.span.line,
                diagnostic.span.column,
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "SEM001",
                Severity::Error,
                DiagnosticKind::Semantic,
                19,
                20,
                2,
                9
            ),
            (
                "SEM002",
                Severity::Error,
                DiagnosticKind::Semantic,
                48,
                53,
                3,
                27
            ),
            (
                "SEM024",
                Severity::Warning,
                DiagnosticKind::Lint,
                8,
                9,
                1,
                9
            ),
            (
                "SEM024",
                Severity::Warning,
                DiagnosticKind::Lint,
                19,
                20,
                2,
                9
            ),
        ]
    );
    assert_eq!(
        &source[summary[0].3..summary[0].4],
        "g",
        "SEM001 spans the second declaration name"
    );
    assert_eq!(
        &source[summary[1].3..summary[1].4],
        "llSay",
        "SEM002 spans the callee identifier"
    );
    assert_eq!(
        &source[summary[2].3..summary[2].4],
        "g",
        "first SEM024 spans the first declaration name"
    );
    assert_eq!(
        &source[summary[3].3..summary[3].4],
        "g",
        "second SEM024 spans the second declaration name"
    );
    assert!(analysis.comparison().is_none());
}

#[test]
fn run_preflight_rejects_a_mixed_run_before_analysis() {
    let mut run = AnalysisRun::new().unwrap();
    let prepared = run
        .add_prepared_text("prepared.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    let legacy = run
        .add_legacy_text("legacy.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    // Both sources would otherwise produce SEM002; the error must be the
    // preflight rejection naming the Legacy unit, not a partial stream.
    let error = run
        .analyze_with_engine(AnalysisEngine::Ast)
        .expect_err("mixed run must be rejected");
    assert_eq!(
        error,
        EngineAnalysisError::PreparedSourceRequired {
            unit: run.unit(legacy).unwrap().id()
        }
    );
    assert_ne!(
        run.unit(prepared).unwrap().id(),
        run.unit(legacy).unwrap().id()
    );
    assert!(run.analyze().is_ok());
    assert!(run.analyze_with_engine(AnalysisEngine::Legacy).is_ok());
}

#[test]
fn multi_unit_compare_reports_are_ordered_and_independent() {
    let mut run = AnalysisRun::new().unwrap();
    let first = run
        .add_prepared_text(
            "first.lsl",
            "integer g;\ninteger g;\ndefault { state_entry() { } }",
        )
        .unwrap();
    let second = run
        .add_prepared_text("second.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    let analysis = run.analyze_with_engine(AnalysisEngine::Compare).unwrap();
    assert_eq!(analysis.comparisons().len(), 2);
    for (position, comparison) in analysis.comparisons().iter().enumerate() {
        assert_eq!(comparison.unit_index, position);
        assert_eq!(comparison.unit_id, run.unit(position).unwrap().id());
        assert!(comparison.report.matched);
        assert!(comparison.report.comparison_complete);
        assert_eq!(comparison.report.by_rule.len(), 23);
    }
    let first_rule = |index: usize| &analysis.comparisons()[index].report.by_rule;
    assert_eq!(first_rule(0)[0].legacy_count, 1, "SEM001 only in unit 0");
    assert_eq!(first_rule(0)[1].legacy_count, 0, "no SEM002 in unit 0");
    assert_eq!(first_rule(1)[0].legacy_count, 0, "no SEM001 in unit 1");
    assert_eq!(first_rule(1)[1].legacy_count, 1, "SEM002 only in unit 1");
    assert_eq!(first, 0);
    assert_eq!(second, 1);
}

#[test]
fn token_collection_matches_the_single_lex_call() {
    let source = "integer g;\ndefault { state_entry() { llSay(0); } }";
    let mut run = AnalysisRun::new().unwrap();
    let prepared = run.add_prepared_text("prepared.lsl", source).unwrap();
    let plain = run.add_legacy_text("plain.lsl", source).unwrap();

    let prepared_unit = run.unit(prepared).unwrap();
    let prepared_without = prepared_unit
        .analyze_with_engine(AnalysisEngine::Ast)
        .unwrap();
    assert!(prepared_without.tokens().is_empty());

    let prepared_with = prepared_unit
        .analyze_with_engine_options(
            AnalysisEngine::Ast,
            &EngineAnalysisOptions {
                collect_tokens: true,
            },
        )
        .unwrap();
    let expected_prepared = lex(prepared_unit.prepared_source().unwrap().text()).0;
    assert_eq!(prepared_with.tokens(), expected_prepared.as_slice());

    let plain_unit = run.unit(plain).unwrap();
    let plain_with = plain_unit
        .analyze_with_engine_options(
            AnalysisEngine::Legacy,
            &EngineAnalysisOptions {
                collect_tokens: true,
            },
        )
        .unwrap();
    let root = plain_unit.legacy_root().unwrap();
    let expected_plain = lex(plain_unit.manager().text(root).unwrap()).0;
    assert_eq!(plain_with.tokens(), expected_plain.as_slice());
}

#[test]
fn frozen_analyze_signatures_and_error_conversion_are_stable() {
    fn consume_unit(
        result: Result<Vec<RenderedDiagnostic>, SpanError>,
    ) -> Result<Vec<RenderedDiagnostic>, SpanError> {
        result
    }
    fn consume_run(
        result: Result<Vec<RenderedDiagnostic>, SpanError>,
    ) -> Result<Vec<RenderedDiagnostic>, SpanError> {
        result
    }

    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_legacy_text("plain.lsl", "default { }").unwrap();
    let unit = run.unit(index).unwrap();
    assert!(consume_unit(unit.analyze()).is_ok());
    assert!(consume_run(run.analyze()).is_ok());

    let converted: EngineAnalysisError = SpanError::OutOfBounds.into();
    assert_eq!(converted, EngineAnalysisError::Span(SpanError::OutOfBounds));
    assert_eq!(
        format!("{converted}"),
        "coordinate is outside its source",
        "Display delegates to SpanError"
    );
    let required = EngineAnalysisError::PreparedSourceRequired { unit: unit.id() };
    assert!(format!("{required}").contains(&unit.id().index().to_string()));
}

#[test]
fn frozen_run_analyze_equals_concatenated_unit_analyses() {
    let mut run = AnalysisRun::new().unwrap();
    let plain = run
        .add_legacy_text(
            "plain.lsl",
            "integer g;\ninteger g;\ndefault { state_entry() { llSay(0); } }",
        )
        .unwrap();
    let prepared = run
        .add_prepared_text(
            "prepared.lsl",
            "default { state_entry() { integer x = ; llSay(0); } }",
        )
        .unwrap();

    let mut expected = Vec::new();
    for index in [plain, prepared] {
        expected.extend(run.unit(index).unwrap().analyze().unwrap());
    }
    let run_level = run.analyze().unwrap();
    assert!(!run_level.is_empty());
    assert_legacy_equal(&run_level, &expected);

    let legacy_engine = run.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
    assert_legacy_equal(&run_level, legacy_engine.diagnostics());
}

#[test]
fn empty_and_malformed_inputs_do_not_panic_in_any_engine() {
    let malformed = "default { state_entry() { integer x = ; llSay(0); } }";
    for source in ["", malformed] {
        for engine in [
            AnalysisEngine::Legacy,
            AnalysisEngine::Ast,
            AnalysisEngine::Compare,
        ] {
            let mut run = AnalysisRun::new().unwrap();
            let prepared = run.add_prepared_text("prepared.lsl", source).unwrap();
            let analysis = run
                .unit(prepared)
                .unwrap()
                .analyze_with_engine(engine)
                .unwrap();
            assert_eq!(
                analysis.comparison().is_some(),
                engine == AnalysisEngine::Compare
            );

            let plain = run.add_legacy_text("plain.lsl", source).unwrap();
            let result = run.unit(plain).unwrap().analyze_with_engine(engine);
            if engine == AnalysisEngine::Legacy {
                assert!(result.is_ok());
            } else {
                assert!(matches!(
                    result,
                    Err(EngineAnalysisError::PreparedSourceRequired { .. })
                ));
            }
        }
    }
}

#[test]
fn preprocessed_units_accept_every_engine() {
    let source = "#define WIDTH 2\ninteger g;\ninteger g;\ndefault { state_entry() { integer x = WIDTH; } }\n";
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_text("root.lsl", source, PreprocessSettings::full())
        .unwrap();
    let unit = run.unit(index).unwrap();
    for engine in [
        AnalysisEngine::Legacy,
        AnalysisEngine::Ast,
        AnalysisEngine::Compare,
    ] {
        let analysis = unit.analyze_with_engine(engine).unwrap();
        assert_eq!(
            analysis.comparison().is_some(),
            engine == AnalysisEngine::Compare
        );
        assert!(!analysis.diagnostics().is_empty());
    }
    assert_eq!(
        parse_legacy(&lex(unit.prepared_source().unwrap().text()).0).len(),
        0
    );
}

#[test]
fn plain_unit_inline_cap_boundary_is_exact() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    for (lines, expect_cap) in [(limit + 1, false), (limit + 2, true)] {
        let source = format!("{}default {{ }}\n", "integer duplicate;\n".repeat(lines));
        let mut run = AnalysisRun::new().unwrap();
        let index = run.add_legacy_text("plain.lsl", &source).unwrap();
        let unit = run.unit(index).unwrap();
        let frozen = unit.analyze().unwrap();
        let legacy = unit.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
        assert_eq!(frozen.len(), limit, "{lines} lines");
        assert_eq!(legacy.diagnostics().len(), limit, "{lines} lines");
        let frozen_last = frozen.last().unwrap().diagnostic().code;
        let legacy_last = legacy.diagnostics().last().unwrap().diagnostic().code;
        assert_eq!(frozen_last == "PRE039", expect_cap, "{lines} lines");
        assert_eq!(legacy_last == "PRE039", expect_cap, "{lines} lines");
        for (frozen, legacy) in frozen.iter().zip(legacy.diagnostics()).take(limit - 1) {
            assert_eq!(frozen.diagnostic(), legacy.diagnostic(), "{lines} lines");
        }
        if expect_cap {
            assert_eq!(frozen_last, "PRE039");
        } else {
            assert!(!frozen
                .iter()
                .any(|diagnostic| diagnostic.diagnostic().code == "PRE039"));
        }
    }
}

#[test]
fn prepared_cap_boundary_is_exact() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    for (lines, expect_cap) in [(limit + 1, false), (limit + 2, true)] {
        let source = format!("{}default {{ }}\n", "integer duplicate;\n".repeat(lines));
        let mut run = AnalysisRun::new().unwrap();
        let index = run.add_prepared_text("prepared.lsl", &source).unwrap();
        let unit = run.unit(index).unwrap();
        assert!(unit.prepared_source().is_some());
        let frozen = unit.analyze().unwrap();
        let legacy = unit.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
        assert_eq!(frozen.len(), limit, "{lines} lines");
        assert_eq!(legacy.diagnostics().len(), limit, "{lines} lines");
        let frozen_last = frozen.last().unwrap().diagnostic().code;
        let legacy_last = legacy.diagnostics().last().unwrap().diagnostic().code;
        assert_eq!(frozen_last == "PRE039", expect_cap, "{lines} lines");
        assert_eq!(legacy_last == "PRE039", expect_cap, "{lines} lines");
        for (frozen, legacy) in frozen.iter().zip(legacy.diagnostics()).take(limit - 1) {
            assert_eq!(frozen.diagnostic(), legacy.diagnostic(), "{lines} lines");
        }
    }
}

#[test]
fn suppression_precedes_prepared_cap_but_frozen_entry_points_do_not_suppress() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = unresolved_variable_flood(limit + 2);
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("prepared.lsl", &source).unwrap();
    let unit = run.unit(index).unwrap();
    let options = EngineAnalysisOptions::default();

    let suppression = DisplaySuppression::new(["SEM012".to_owned()]);
    let suppressed = unit
        .analyze_with_engine_options_suppressed(AnalysisEngine::Ast, &options, &suppression)
        .unwrap();
    assert!(suppressed.diagnostics().is_empty());

    for diagnostics in [
        unit.analyze_with_engine(AnalysisEngine::Ast)
            .unwrap()
            .into_diagnostics(),
        unit.analyze().unwrap(),
        run.analyze().unwrap(),
    ] {
        assert_eq!(diagnostics.len(), limit);
        assert_eq!(diagnostics.last().unwrap().diagnostic().code, "PRE039");
    }

    let unrelated = DisplaySuppression::new(["SEM001".to_owned()]);
    let still_capped = unit
        .analyze_with_engine_options_suppressed(AnalysisEngine::Ast, &options, &unrelated)
        .unwrap();
    assert_eq!(still_capped.diagnostics().len(), limit);
    assert_eq!(
        still_capped.diagnostics().last().unwrap().diagnostic().code,
        "PRE039"
    );
}

#[test]
fn suppression_precedes_plain_cap_but_frozen_analyze_does_not_suppress() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = format!(
        "{}default {{ }}\n",
        "integer duplicate;\n".repeat(limit + 2)
    );
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_legacy_text("plain.lsl", &source).unwrap();
    let unit = run.unit(index).unwrap();
    let suppression = DisplaySuppression::new(["SEM001".to_owned()]);

    let suppressed = unit
        .analyze_with_engine_options_suppressed(
            AnalysisEngine::Legacy,
            &EngineAnalysisOptions::default(),
            &suppression,
        )
        .unwrap();
    assert!(suppressed.diagnostics().is_empty());

    let frozen = unit.analyze().unwrap();
    assert_eq!(frozen.len(), limit);
    assert_eq!(frozen.last().unwrap().diagnostic().code, "PRE039");
}

fn unresolved_variable_flood(statements: usize) -> String {
    let mut source = String::from("default { state_entry() {\n");
    for _ in 0..statements {
        source.push_str("x = 1;\n");
    }
    source.push_str("} }\n");
    source
}

#[test]
fn run_collect_tokens_populates_every_dense_unit() {
    let sources = [
        "integer g;\ndefault { state_entry() { llSay(0); } }",
        "default { state_entry() { integer x; x = 1; } }",
    ];
    let mut run = AnalysisRun::new().unwrap();
    for (index, source) in sources.iter().enumerate() {
        run.add_prepared_text(&format!("unit{index}.lsl"), source)
            .unwrap();
    }

    let without = run.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
    assert_eq!(without.tokens(0), Some(&[][..]));
    assert_eq!(without.tokens(1), Some(&[][..]));
    assert_eq!(without.tokens(2), None);

    let with = run
        .analyze_with_engine_options(
            AnalysisEngine::Legacy,
            &EngineAnalysisOptions {
                collect_tokens: true,
            },
        )
        .unwrap();
    for (index, source) in sources.iter().enumerate() {
        let expected = lex(source).0;
        assert_eq!(
            with.tokens(index),
            Some(expected.as_slice()),
            "unit {index}"
        );
    }
    assert_eq!(with.tokens(2), None);
}

#[test]
fn all_matched_tracks_compare_report_parity() {
    let mut run = AnalysisRun::new().unwrap();
    run.add_prepared_text(
        "first.lsl",
        "integer g;\ninteger g;\ndefault { state_entry() { } }",
    )
    .unwrap();
    run.add_prepared_text("second.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    assert!(run
        .analyze_with_engine(AnalysisEngine::Legacy)
        .unwrap()
        .all_matched());
    assert!(run
        .analyze_with_engine(AnalysisEngine::Ast)
        .unwrap()
        .all_matched());
    let compare = run.analyze_with_engine(AnalysisEngine::Compare).unwrap();
    assert!(compare.all_matched());
    assert_eq!(compare.comparisons().len(), 2);
}

fn expected_ast_display(
    prepared: &neolint::PreparedSource,
    options: AstParseOptions,
) -> Vec<RenderedDiagnostic> {
    let mut rendered = Vec::new();
    for diagnostic in prepared.preprocess_diagnostics() {
        rendered.push(RenderedDiagnostic::from_preprocess(diagnostic.clone(), prepared).unwrap());
    }
    let (tokens, lexical) = lex(prepared.text());
    let (ast, ast_syntax) = neolint::ast::parse_ast_with_options(&tokens, options);
    let ast_semantic = neolint::ast_semantic::analyze_ast(&ast, &ast_syntax);
    for diagnostic in lexical.into_iter().chain(ast_syntax).chain(ast_semantic) {
        rendered.push(
            RenderedDiagnostic::from_prepared(
                neolint::PreparedDiagnostic::new(
                    diagnostic.code,
                    diagnostic.severity,
                    diagnostic.kind,
                    diagnostic.message,
                    diagnostic.span,
                ),
                prepared,
            )
            .unwrap(),
        );
    }
    rendered
}

fn assert_rendered_equal(left: &[RenderedDiagnostic], right: &[RenderedDiagnostic]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right.iter()) {
        assert_eq!(left.diagnostic(), right.diagnostic());
        assert_eq!(left.source(), right.source());
        assert_eq!(left.related(), right.related());
    }
}

fn dialect_source(directives: &str) -> String {
    format!(
        "{directives}default\n{{\n    state_entry()\n    {{\n        integer choice;\n        list values;\n        switch (choice)\n        {{\n            case 1:\n                values[0] = \"a\";\n                break;\n        }}\n        (string)values[0];\n    }}\n}}\n"
    )
}

#[test]
fn engine_uses_prepared_firestorm_dialect_flags_in_order() {
    use neolint::PreprocessDialect;

    let cases = [
        ("#define USE_SWITCHES\n", true, false),
        ("#define USE_LAZY_LISTS\n", false, true),
        ("#define USE_SWITCHES\n#define USE_LAZY_LISTS\n", true, true),
    ];
    let settings = PreprocessSettings::full().with_dialect(PreprocessDialect::Firestorm);
    for (index, (directives, switches, lazy_lists)) in cases.iter().enumerate() {
        let source = dialect_source(directives);
        let mut run = AnalysisRun::new().unwrap();
        let unit_index = run
            .add_preprocessed_text(&format!("firestorm{index}.lsl"), &source, settings.clone())
            .unwrap();
        let unit = run.unit(unit_index).unwrap();
        let prepared = unit.prepared_source().unwrap();
        assert_eq!(prepared.switches_enabled(), *switches, "case {index}");
        assert_eq!(prepared.lazy_lists_enabled(), *lazy_lists, "case {index}");

        let options = AstParseOptions::from_flags(*switches, *lazy_lists);
        let expected = expected_ast_display(prepared, options);
        let ast = unit.analyze_with_engine(AnalysisEngine::Ast).unwrap();
        assert_rendered_equal(ast.diagnostics(), &expected);

        let comparison = unit.analyze_with_engine(AnalysisEngine::Compare).unwrap();
        let (tokens, _lexical) = lex(prepared.text());
        let legacy_syntax = parse_legacy(&tokens);
        let legacy_semantic =
            neolint::semantic::analyze_with_syntax_diagnostics(&tokens, &legacy_syntax);
        let (ast_parsed, ast_syntax) = neolint::ast::parse_ast_with_options(&tokens, options);
        let ast_semantic = neolint::ast_semantic::analyze_ast(&ast_parsed, &ast_syntax);
        let oracle = neolint::compare_semantic_diagnostics(&legacy_semantic, &ast_semantic);
        assert_eq!(comparison.comparison(), Some(&oracle), "case {index}");
    }

    // A plain-LSL prepared unit must use both gates disabled.
    let plain_source = dialect_source("");
    let mut run = AnalysisRun::new().unwrap();
    let unit_index = run.add_prepared_text("plain.lsl", &plain_source).unwrap();
    let unit = run.unit(unit_index).unwrap();
    let prepared = unit.prepared_source().unwrap();
    assert!(!prepared.switches_enabled());
    assert!(!prepared.lazy_lists_enabled());
    let expected = expected_ast_display(prepared, AstParseOptions::from_flags(false, false));
    let ast = unit.analyze_with_engine(AnalysisEngine::Ast).unwrap();
    assert_rendered_equal(ast.diagnostics(), &expected);
}

#[test]
fn compare_preflight_rejects_mixed_runs_like_ast() {
    let mut run = AnalysisRun::new().unwrap();
    run.add_prepared_text("prepared.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    let legacy = run
        .add_legacy_text("legacy.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    let expected = run.unit(legacy).unwrap().id();
    for engine in [AnalysisEngine::Ast, AnalysisEngine::Compare] {
        assert_eq!(
            run.analyze_with_engine(engine).unwrap_err(),
            EngineAnalysisError::PreparedSourceRequired { unit: expected }
        );
    }
}

#[test]
fn run_stream_has_no_cap_and_flat_dense_order() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let huge = format!(
        "{}default {{ }}\n",
        "integer duplicate;\n".repeat(limit + 2)
    );
    let mut capped = AnalysisRun::new().unwrap();
    for index in 0..2 {
        capped
            .add_prepared_text(&format!("capped{index}.lsl"), &huge)
            .unwrap();
    }
    let analysis = capped.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
    assert_eq!(analysis.diagnostics().len(), limit * 2);
    let pre039 = analysis
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.diagnostic().code == "PRE039")
        .count();
    assert_eq!(pre039, 2, "each unit keeps its own terminal PRE039");

    let mut ordered = AnalysisRun::new().unwrap();
    ordered
        .add_prepared_text(
            "first.lsl",
            "integer g;\ninteger g;\ndefault { state_entry() { } }",
        )
        .unwrap();
    ordered
        .add_prepared_text("second.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    let flat = ordered.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
    let unit_indexes: Vec<u32> = flat
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.source().unit().index())
        .collect();
    assert!(!unit_indexes.is_empty());
    assert!(
        unit_indexes.windows(2).all(|window| window[0] <= window[1]),
        "{unit_indexes:?}"
    );
    assert_eq!(unit_indexes.first(), Some(&0));
    assert_eq!(unit_indexes.last(), Some(&1));
    assert!(unit_indexes.contains(&0) && unit_indexes.contains(&1));
}

fn assert_identity_preparation(unit: &neolint::AnalysisUnit, source: &str, display: &str) {
    let prepared = unit.prepared_source().expect("identity-prepared unit");
    assert_eq!(prepared.text(), source);
    assert_eq!(prepared.prepared_len(), source.len());

    // The public API does not expose the origin arena. Resolving the complete
    // prepared buffer to the complete root, with no related locations and one
    // include-graph node, pins the single identity (Direct) mapping.
    let root = unit.root().expect("root source");
    let map = prepared.map();
    let prepared_span = prepared
        .prepared_span(Span {
            start: 0,
            end: source.len(),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        })
        .expect("full prepared span");
    let resolved = map.resolve_span(prepared_span).expect("full resolution");
    assert_eq!(
        resolved.primary(),
        map.source_span(root, 0, source.len())
            .expect("full root span")
    );
    assert!(resolved.related().is_empty());
    let nodes = prepared.include_graph().nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].display_name(), display);
}

#[test]
fn identity_preparation_preserves_root_bytes_through_pipeline() {
    let source = "integer g;\ndefault { state_entry() { llSay(0); } }\n";
    let mut run = AnalysisRun::new().unwrap();

    let text_index = run.add_prepared_text("text.lsl", source).unwrap();
    assert_identity_preparation(run.unit(text_index).unwrap(), source, "text.lsl");

    let directory = std::env::temp_dir().join(format!(
        "neolint-identity-preparation-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("file.lsl");
    std::fs::write(&path, source).unwrap();
    let display = path.to_string_lossy().into_owned();
    let file_index = run.add_prepared_file(&path).unwrap();
    assert_identity_preparation(run.unit(file_index).unwrap(), source, &display);
    std::fs::remove_dir_all(directory).unwrap();
}
