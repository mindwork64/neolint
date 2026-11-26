//! Config-driven library analysis API regression tests.

use neolint::preprocessor::PreprocessSettings;
use neolint::{AnalysisEngine, AnalysisRun, EngineAnalysisOptions, LintConfig, Severity};

fn codes(analysis: &neolint::UnitEngineAnalysis) -> Vec<&'static str> {
    analysis
        .diagnostics()
        .iter()
        .map(|item| item.diagnostic().code)
        .collect()
}

#[test]
fn configured_analysis_applies_engine_suppression_and_severity_without_losing_locations() {
    let source = "integer g;\ninteger g;\ndefault { state_entry() { llSay(0); } }";
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("configured.lsl", source).unwrap();
    let unit = run.unit(index).unwrap();

    let baseline = unit.analyze_with_engine(AnalysisEngine::Ast).unwrap();
    let expected = baseline
        .diagnostics()
        .iter()
        .find(|item| item.diagnostic().code == "SEM001")
        .expect("duplicate global diagnostic");

    let config = LintConfig::parse("disable = SEM002\nseverity.SEM001 = warning\n").unwrap();
    config.validate_target_settings().unwrap();
    let configured = unit
        .analyze_with_config(&config, &EngineAnalysisOptions::default())
        .unwrap();

    assert!(codes(&configured).contains(&"SEM001"));
    assert!(!codes(&configured).contains(&"SEM002"));
    let actual = configured
        .diagnostics()
        .iter()
        .find(|item| item.diagnostic().code == "SEM001")
        .expect("configured duplicate global diagnostic");
    assert_eq!(actual.diagnostic().severity, Severity::Warning);
    assert_eq!(actual.diagnostic().span, expected.diagnostic().span);
    assert_eq!(actual.source(), expected.source());
    assert_eq!(actual.related(), expected.related());
    assert_eq!(
        unit.manager().file(actual.source()).unwrap().display_name(),
        "configured.lsl"
    );
}

#[test]
fn configured_analysis_preserves_preprocessor_related_locations() {
    let source = "#define ITEM 1\n#define ITEM 2\ndefault { state_entry() { } }\n";
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_text("macro-conflict.lsl", source, PreprocessSettings::full())
        .unwrap();
    let unit = run.unit(index).unwrap();

    let baseline = unit.analyze_with_engine(AnalysisEngine::Ast).unwrap();
    let expected = baseline
        .diagnostics()
        .iter()
        .find(|item| item.diagnostic().code == "PRE005")
        .expect("macro conflict diagnostic");
    assert_eq!(expected.related().len(), 1);

    let config = LintConfig::parse("severity.PRE005 = warning\n").unwrap();
    let configured = unit
        .analyze_with_config(&config, &EngineAnalysisOptions::default())
        .unwrap();
    let actual = configured
        .diagnostics()
        .iter()
        .find(|item| item.diagnostic().code == "PRE005")
        .expect("configured macro conflict diagnostic");

    assert_eq!(actual.diagnostic().severity, Severity::Warning);
    assert_eq!(actual.source(), expected.source());
    assert_eq!(actual.diagnostic().span, expected.diagnostic().span);
    assert_eq!(actual.related(), expected.related());
    assert_eq!(actual.related().len(), 1);
}

#[test]
fn configured_analysis_uses_the_selected_target_and_engine() {
    let source = "default { state_entry() { llGetCameraFOV(); llSay(0); } }";
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("target.lsl", source).unwrap();
    let unit = run.unit(index).unwrap();

    let opensim = LintConfig::parse("target = opensim-xengine\n").unwrap();
    opensim.validate_target_settings().unwrap();
    let configured = unit
        .analyze_with_config(&opensim, &EngineAnalysisOptions::default())
        .unwrap();
    assert!(codes(&configured).contains(&"SEM047"));

    let legacy = LintConfig::parse("engine = legacy\n").unwrap();
    let configured_legacy = unit
        .analyze_with_config(&legacy, &EngineAnalysisOptions::default())
        .unwrap();
    assert!(configured_legacy
        .diagnostics()
        .iter()
        .any(|item| item.diagnostic().code == "SEM002"));
    assert!(configured_legacy.comparison().is_none());
}

#[test]
fn configured_analysis_rejects_ast_on_plain_legacy_units() {
    let mut run = AnalysisRun::new().unwrap();
    let index = run
        .add_legacy_text("plain.lsl", "default { state_entry() { llSay(0); } }")
        .unwrap();
    let unit = run.unit(index).unwrap();
    let config = LintConfig::default();

    let error = unit
        .analyze_with_config(&config, &EngineAnalysisOptions::default())
        .unwrap_err();
    assert_eq!(
        error,
        neolint::EngineAnalysisError::PreparedSourceRequired { unit: unit.id() }
    );
}
