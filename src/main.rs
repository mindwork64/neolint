//! Command-line entry point for neolint.
//!
//! Pipeline:
//! argument parsing → configuration loading/validation → input collection
//! (files, directories, stdin) → source reading → engine selection → per-unit
//! preparation (identity or full preprocessing, or the direct Legacy path) →
//! [`neolint::lex`] → the selected engine ([`neolint::parse_legacy`] with
//! [`neolint::semantic::analyze_with_syntax_diagnostics`], [`neolint::parse_ast`]
//! with AST semantic analysis, or Compare) → configuration application →
//! human/JSON rendering → exit code.
//!
//! The default analysis engine is AST. With no preprocessing, units use
//! identity preparation so the engine API has a source map. Explicit
//! `--engine legacy` (or `engine = legacy`) without preprocessing keeps the
//! unchanged direct plain-Legacy loop, which is uncapped at run level. With
//! `--preprocess`, every engine uses the full preprocessor. `--engine compare`
//! prints a semantic-parity report on stderr and exits with code 3 when any
//! unit's comparison is unmatched.
//!
//! Exit codes are `0` when no error-severity diagnostics remain, `1` when at
//! least one error-severity diagnostic remains, `2` for command-line,
//! configuration, input, or file-read errors, and `3` in Compare mode when a
//! report is unmatched (unless a failure already requires `2`). Warnings/info do
//! not fail by default. Directory traversal is sorted and skips symlinks;
//! explicit files are accepted regardless of extension; stdin (`-`) may appear
//! only once.
use neolint::config::LintConfig;
use neolint::semantic::analyze_with_syntax_diagnostics;
use neolint::{
    lex, parse, AnalysisEngine, AnalysisRun, Diagnostic, DiagnosticFingerprint,
    EngineAnalysisOptions, EngineComparisonReport, OpenSimApiVersion, PreprocessDialect,
    PreprocessSettings, Severity, TargetProfile,
};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Human,
    Json,
}

struct JsonRelatedLocation {
    file: String,
    label: String,
    span: neolint::Span,
}

struct OutputDiagnostic {
    diagnostic: Diagnostic,
    related: Vec<JsonRelatedLocation>,
}

struct FileResult {
    path: String,
    tokens: Vec<neolint::Token>,
    diagnostics: Vec<OutputDiagnostic>,
}

/// How a unit-based run constructs its prepared source.
#[derive(Clone, Copy)]
enum UnitPreparation {
    /// Full preprocessing with the configured dialect and include directories.
    Preprocessed,
    /// Byte-identical identity preparation (no preprocessing).
    Identity,
}

/// One unit's Compare-mode report and the display name used for its root.
struct CompareUnitReport {
    unit_index: usize,
    path: String,
    report: EngineComparisonReport,
}

fn main() {
    let code = run();
    process::exit(code);
}

/// Execute the complete CLI pipeline and return the process exit code.
fn run() -> i32 {
    let mut format = OutputFormat::Human;
    let mut show_tokens = false;
    let mut input_args = Vec::new();
    let mut config_path = None;
    let mut disabled_rules = Vec::new();
    let mut severity_overrides = Vec::new();
    let mut preprocess_flag = false;
    let mut dialect_override = None;
    let mut target_override = None;
    let mut target_version_override = None;
    let mut ossl_functions_cli = Vec::new();
    let mut strict_ossl_cli = false;
    let mut include_dirs_cli = Vec::new();
    let mut engine_override = None;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            "-V" | "--version" => {
                println!("neolint {}", env!("CARGO_PKG_VERSION"));
                return 0;
            }
            "--tokens" => show_tokens = true,
            "--preprocess" => preprocess_flag = true,
            "--dialect" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --dialect requires 'lsl' or 'firestorm'");
                    return 2;
                };
                dialect_override = match value.as_str() {
                    "lsl" => Some(PreprocessDialect::Lsl),
                    "firestorm" => Some(PreprocessDialect::Firestorm),
                    _ => {
                        eprintln!("neolint: invalid dialect '{value}' (expected lsl or firestorm)");
                        return 2;
                    }
                };
            }
            "--strict-ossl" => {
                strict_ossl_cli = true;
            }
            "--ossl-function" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --ossl-function requires an OSSL function name");
                    return 2;
                };
                ossl_functions_cli.push(value);
            }
            "--target-version" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --target-version requires an OpenSim API version");
                    return 2;
                };
                target_version_override = match OpenSimApiVersion::parse(&value) {
                    Some(version) => Some(version),
                    None => {
                        eprintln!("neolint: invalid target version '{value}' (expected 0.9.2.2, 0.9.3.0, 0.9.3.1 or master-2026-11-25)");
                        return 2;
                    }
                };
            }
            "--target" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --target requires a target profile");
                    return 2;
                };
                target_override = match TargetProfile::parse(&value) {
                    Some(target) => Some(target),
                    None => {
                        eprintln!("neolint: invalid target '{value}' (expected second-life-mono, second-life-lso, second-life-luau, opensim-xengine or opensim-yengine)");
                        return 2;
                    }
                };
            }
            "--engine" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --engine requires 'legacy', 'ast' or 'compare'");
                    return 2;
                };
                let Some(engine) = AnalysisEngine::parse(&value) else {
                    eprintln!(
                        "neolint: invalid engine '{value}' (expected legacy, ast or compare)"
                    );
                    return 2;
                };
                engine_override = Some(engine);
            }
            "-I" | "--include-dir" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: {arg} requires a directory path");
                    return 2;
                };
                include_dirs_cli.push(PathBuf::from(value));
            }
            _ if arg.starts_with("-I") && arg.len() > 2 => {
                include_dirs_cli.push(PathBuf::from(&arg[2..]));
            }
            _ if arg.starts_with("--include-dir=") => {
                let value = &arg["--include-dir=".len()..];
                if value.is_empty() {
                    eprintln!("neolint: --include-dir requires a directory path");
                    return 2;
                }
                include_dirs_cli.push(PathBuf::from(value));
            }
            _ if arg.starts_with("--dialect=") => {
                let value = &arg["--dialect=".len()..];
                dialect_override = match value {
                    "lsl" => Some(PreprocessDialect::Lsl),
                    "firestorm" => Some(PreprocessDialect::Firestorm),
                    _ => {
                        eprintln!("neolint: invalid dialect '{value}' (expected lsl or firestorm)");
                        return 2;
                    }
                };
            }
            _ if arg.starts_with("--ossl-function=") => {
                let value = &arg["--ossl-function=".len()..];
                if value.is_empty() {
                    eprintln!("neolint: --ossl-function requires an OSSL function name");
                    return 2;
                }
                ossl_functions_cli.push(value.to_owned());
            }
            _ if arg.starts_with("--target-version=") => {
                let value = &arg["--target-version=".len()..];
                target_version_override = match OpenSimApiVersion::parse(value) {
                    Some(version) => Some(version),
                    None => {
                        eprintln!("neolint: invalid target version '{value}' (expected 0.9.2.2, 0.9.3.0, 0.9.3.1 or master-2026-11-25)");
                        return 2;
                    }
                };
            }
            _ if arg.starts_with("--target=") => {
                let value = &arg["--target=".len()..];
                target_override = match TargetProfile::parse(value) {
                    Some(target) => Some(target),
                    None => {
                        eprintln!("neolint: invalid target '{value}' (expected second-life-mono, second-life-lso, second-life-luau, opensim-xengine or opensim-yengine)");
                        return 2;
                    }
                };
            }
            _ if arg.starts_with("--engine=") => {
                let value = &arg["--engine=".len()..];
                if value.is_empty() {
                    eprintln!("neolint: --engine requires 'legacy', 'ast' or 'compare'");
                    return 2;
                }
                let Some(engine) = AnalysisEngine::parse(value) else {
                    eprintln!(
                        "neolint: invalid engine '{value}' (expected legacy, ast or compare)"
                    );
                    return 2;
                };
                engine_override = Some(engine);
            }
            "--format" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --format requires 'human' or 'json'");
                    return 2;
                };
                format = match value.as_str() {
                    "human" => OutputFormat::Human,
                    "json" => OutputFormat::Json,
                    _ => {
                        eprintln!("neolint: unsupported output format '{value}'");
                        return 2;
                    }
                };
            }
            "--config" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --config requires a file path");
                    return 2;
                };
                config_path = Some(value);
            }
            "--disable" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --disable requires a comma-separated rule list");
                    return 2;
                };
                disabled_rules.extend(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|rule| !rule.is_empty())
                        .map(str::to_owned),
                );
            }
            "--severity" => {
                let Some(value) = args.next() else {
                    eprintln!("neolint: --severity requires RULE=error|warning|info");
                    return 2;
                };
                severity_overrides.push(value);
            }
            _ if arg.starts_with('-') && arg != "-" => {
                eprintln!("neolint: unknown option '{arg}'");
                eprintln!("Try 'neolint --help' for usage.");
                return 2;
            }
            _ => input_args.push(arg),
        }
    }

    if input_args.is_empty() {
        print_help();
        return 2;
    }

    if show_tokens && format == OutputFormat::Json {
        eprintln!("neolint: --tokens cannot be combined with --format json");
        return 2;
    }

    let mut config = if let Some(path) = config_path {
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("neolint: cannot read config '{path}': {error}");
                return 2;
            }
        };
        match LintConfig::parse(&source) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("neolint: invalid config '{path}': {error}");
                return 2;
            }
        }
    } else {
        LintConfig::default()
    };

    if let Err(error) = config.disable(disabled_rules) {
        eprintln!("neolint: invalid --disable value: {error}");
        return 2;
    }

    for setting in severity_overrides {
        let Some((rule, value)) = setting.split_once('=') else {
            eprintln!("neolint: invalid --severity '{setting}' (expected RULE=error|warning|info)");
            return 2;
        };
        let severity = match parse_severity(value) {
            Some(severity) => severity,
            None => {
                eprintln!("neolint: invalid severity '{value}' (expected error, warning or info)");
                return 2;
            }
        };
        if let Err(error) = config.set_severity(rule.trim(), severity) {
            eprintln!("neolint: invalid --severity value: {error}");
            return 2;
        }
    }

    if preprocess_flag {
        config.set_preprocess(true);
    }
    if let Some(dialect) = dialect_override {
        config.set_dialect(dialect);
    }
    if !include_dirs_cli.is_empty() {
        config.set_include_dirs(include_dirs_cli);
    }
    if let Some(engine) = engine_override {
        config.set_engine(engine);
    }
    if let Some(target) = target_override {
        config.set_target(target);
    }
    if let Some(version) = target_version_override {
        if let Err(error) = config.set_target_version(version) {
            eprintln!("neolint: invalid target version: {error}");
            return 2;
        }
    }
    if strict_ossl_cli {
        config.enable_ossl_allowlist();
    }
    for function in ossl_functions_cli {
        if let Err(error) = config.add_ossl_function(&function) {
            eprintln!("neolint: invalid --ossl-function value: {error}");
            return 2;
        }
    }
    if let Err(error) = config.validate_target_settings() {
        eprintln!("neolint: invalid target configuration: {error}");
        return 2;
    }
    if config.target_explicit() && config.engine() == AnalysisEngine::Legacy {
        eprintln!("neolint: target profiles require --engine ast or compare; the frozen Legacy engine is not target-profile aware");
        return 2;
    }
    if !config.preprocess_enabled() && !config.include_dirs().is_empty() {
        eprintln!("neolint: include directories require --preprocess or --dialect firestorm");
        return 2;
    }
    for directory in config.include_dirs() {
        match fs::metadata(directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                eprintln!(
                    "neolint: include path '{}' is not a directory",
                    directory.display()
                );
                return 2;
            }
            Err(error) => {
                eprintln!(
                    "neolint: cannot access include directory '{}': {error}",
                    directory.display()
                );
                return 2;
            }
        }
    }

    let paths = match collect_input_paths(&input_args) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("neolint: {error}");
            return 2;
        }
    };

    if config.preprocess_enabled() {
        return run_units(
            &paths,
            &config,
            format,
            show_tokens,
            UnitPreparation::Preprocessed,
        );
    }

    // AST (the default) and Compare without preprocessing still need a prepared
    // source (identity preparation). Only explicit Legacy keeps the direct,
    // uncapped plain loop below.
    if config.engine() != AnalysisEngine::Legacy {
        return run_units(
            &paths,
            &config,
            format,
            show_tokens,
            UnitPreparation::Identity,
        );
    }

    let mut results = Vec::new();
    let mut input_failed = false;
    let mut stdin_used = false;

    for path in paths {
        let source = if path == "-" {
            if stdin_used {
                eprintln!("neolint: stdin ('-') may only be specified once");
                input_failed = true;
                continue;
            }
            stdin_used = true;
            let mut input = String::new();
            if let Err(error) = io::stdin().read_to_string(&mut input) {
                eprintln!("neolint: cannot read stdin: {error}");
                input_failed = true;
                continue;
            }
            input
        } else {
            match fs::read_to_string(&path) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("neolint: cannot read '{path}': {error}");
                    input_failed = true;
                    continue;
                }
            }
        };

        let (tokens, mut diagnostics) = lex(&source);
        let syntax_diagnostics = parse(&tokens);
        diagnostics.extend(syntax_diagnostics.iter().cloned());
        diagnostics.extend(analyze_with_syntax_diagnostics(
            &tokens,
            &syntax_diagnostics,
        ));
        config.apply(&mut diagnostics);

        let diagnostics = diagnostics
            .into_iter()
            .map(|diagnostic| OutputDiagnostic {
                diagnostic,
                related: Vec::new(),
            })
            .collect();
        results.push(FileResult {
            path,
            tokens,
            diagnostics,
        });
    }

    match format {
        OutputFormat::Human => print_human(&results, show_tokens),
        OutputFormat::Json => print_json(&results),
    }

    let has_errors = results
        .iter()
        .flat_map(|result| &result.diagnostics)
        .any(|item| item.diagnostic.severity == Severity::Error);

    if input_failed {
        2
    } else if has_errors {
        1
    } else {
        0
    }
}

fn run_units(
    paths: &[String],
    config: &LintConfig,
    format: OutputFormat,
    show_tokens: bool,
    preparation: UnitPreparation,
) -> i32 {
    let preprocess_options = PreprocessSettings::full()
        .with_dialect(config.dialect())
        .with_include_dirs(config.include_dirs().to_vec());
    let mut run = match AnalysisRun::new() {
        Ok(run) => run,
        Err(error) => {
            eprintln!("neolint: cannot initialize analysis run: {error}");
            return 2;
        }
    };
    let mut unit_indexes = Vec::new();
    let mut input_failed = false;
    let mut stdin_used = false;
    for path in paths {
        let result = if path == "-" {
            if stdin_used {
                eprintln!("neolint: stdin ('-') may only be specified once");
                input_failed = true;
                continue;
            }
            stdin_used = true;
            let mut input = String::new();
            if let Err(error) = io::stdin().read_to_string(&mut input) {
                eprintln!("neolint: cannot read stdin: {error}");
                input_failed = true;
                continue;
            }
            add_prepared_unit(
                &mut run,
                "<stdin>",
                Some(&input),
                preparation,
                &preprocess_options,
            )
        } else {
            add_prepared_unit(&mut run, path, None, preparation, &preprocess_options)
        };
        match result {
            Ok(index) => unit_indexes.push(index),
            Err(error) => {
                eprintln!("neolint: cannot prepare '{path}': {error}");
                input_failed = true;
            }
        }
    }

    // AST and Compare require a prepared source. Unit construction above is
    // engine-driven, so this defensive preflight is unreachable by design; it
    // exists so a partially analyzed multi-unit run is impossible.
    if config.engine().requires_prepared_source() {
        for &index in &unit_indexes {
            let Some(unit) = run.unit(index) else {
                eprintln!(
                    "neolint: cannot analyze unit {index}: unit is missing from the analysis run"
                );
                input_failed = true;
                continue;
            };
            if unit.prepared_source().is_none() {
                let error =
                    neolint::EngineAnalysisError::PreparedSourceRequired { unit: unit.id() };
                eprintln!("neolint: cannot analyze unit {index}: {error}");
                return 2;
            }
        }
    }

    let engine = config.engine();
    let options = EngineAnalysisOptions {
        collect_tokens: show_tokens,
    };
    let suppression = config.display_suppression();
    let expected_units = unit_indexes.len();
    let mut analyzed_units = 0usize;
    let mut results = Vec::new();
    let mut reports = Vec::new();
    for index in unit_indexes {
        let Some(unit) = run.unit(index) else {
            eprintln!(
                "neolint: cannot analyze unit {index}: unit is missing from the analysis run"
            );
            input_failed = true;
            continue;
        };
        let Some(prepared) = unit.prepared_source() else {
            let error = neolint::EngineAnalysisError::PreparedSourceRequired { unit: unit.id() };
            eprintln!("neolint: cannot analyze unit {index}: {error}");
            input_failed = true;
            continue;
        };
        let analysis = match unit.analyze_with_target_options_suppressed(
            engine,
            &options,
            config.target(),
            config.ossl_allowlist(),
            &suppression,
        ) {
            Ok(analysis) => analysis,
            Err(error) => {
                eprintln!("neolint: cannot analyze unit {index}: {error}");
                input_failed = true;
                continue;
            }
        };
        analyzed_units += 1;

        if engine == AnalysisEngine::Compare {
            match analysis.comparison() {
                Some(report) => reports.push(CompareUnitReport {
                    unit_index: index,
                    path: prepared_path(unit),
                    report: report.clone(),
                }),
                None => {
                    eprintln!(
                        "neolint: cannot analyze unit {index}: compare engine produced no report"
                    );
                    input_failed = true;
                }
            }
        }

        let mut groups: Vec<(neolint::SourceId, Vec<OutputDiagnostic>)> = Vec::new();
        for diagnostic in analysis.diagnostics() {
            let related = match json_related_locations(unit, diagnostic.related()) {
                Ok(related) => related,
                Err(error) => {
                    eprintln!("neolint: cannot render related locations for unit {index}: {error}");
                    input_failed = true;
                    continue;
                }
            };
            let output = OutputDiagnostic {
                diagnostic: diagnostic.diagnostic().clone(),
                related,
            };
            let source = diagnostic.source();
            if let Some((_, items)) = groups.iter_mut().find(|(id, _)| *id == source) {
                items.push(output);
            } else {
                groups.push((source, vec![output]));
            }
        }
        let root = unit.root();
        // The token vector comes from the single lex inside this unit's engine
        // analysis; there is no second `lex(prepared.text())` call.
        let unit_tokens = analysis.tokens().to_vec();
        let nodes: Vec<(neolint::SourceId, String)> = prepared
            .include_graph()
            .nodes()
            .iter()
            .map(|node| (node.source(), node.display_name().to_owned()))
            .collect();
        for (source, path) in nodes {
            let mut diagnostics = groups
                .iter()
                .position(|(id, _)| *id == source)
                .map(|position| groups.remove(position).1)
                .unwrap_or_default();
            if Some(source) != root && diagnostics.is_empty() {
                continue;
            }
            diagnostics.retain_mut(|item| config.apply_to_diagnostic(&mut item.diagnostic));
            results.push(FileResult {
                path,
                tokens: if Some(source) == root {
                    unit_tokens.clone()
                } else {
                    Vec::new()
                },
                diagnostics,
            });
        }
        for (source, mut diagnostics) in groups {
            diagnostics.retain_mut(|item| config.apply_to_diagnostic(&mut item.diagnostic));
            let path = unit
                .source_map()
                .and_then(|map| map.file(source))
                .map(|file| file.display_name().to_owned())
                .unwrap_or_else(|| prepared_path(unit));
            results.push(FileResult {
                path,
                tokens: Vec::new(),
                diagnostics,
            });
        }
    }

    let total: usize = results.iter().map(|result| result.diagnostics.len()).sum();
    if total > neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS {
        let mut remaining = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS - 1;
        for result in &mut results {
            // A unit-level cap may already have appended a terminal diagnostic.
            // The CLI-level overflow must leave exactly one terminal diagnostic.
            result
                .diagnostics
                .retain(|item| item.diagnostic.code != "PRE039");
            if result.diagnostics.len() > remaining {
                result.diagnostics.truncate(remaining);
            }
            remaining -= result.diagnostics.len();
        }
        if let Some(first) = results.first_mut() {
            first.diagnostics.push(OutputDiagnostic {
                diagnostic: Diagnostic {
                    code: "PRE039",
                    severity: Severity::Error,
                    kind: neolint::DiagnosticKind::Syntax,
                    message: "total diagnostic limit (10000) reached".to_owned(),
                    span: neolint::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        column: 1,
                        end_line: 1,
                        end_column: 1,
                    },
                },
                related: Vec::new(),
            });
        }
    }
    match format {
        OutputFormat::Human => print_human(&results, show_tokens),
        OutputFormat::Json => print_json(&results),
    }

    let all_matched = !input_failed
        && analyzed_units == expected_units
        && reports.len() == expected_units
        && reports.iter().all(|item| item.report.is_parity());
    if engine == AnalysisEngine::Compare {
        print_compare_report(&reports, format, all_matched);
    }

    let has_errors = results
        .iter()
        .flat_map(|result| &result.diagnostics)
        .any(|item| item.diagnostic.severity == Severity::Error);
    if input_failed {
        2
    } else if engine == AnalysisEngine::Compare && !all_matched {
        3
    } else if has_errors {
        1
    } else {
        0
    }
}

/// Add one unit using the selected preparation path.
fn add_prepared_unit(
    run: &mut AnalysisRun,
    display_name: &str,
    text: Option<&str>,
    preparation: UnitPreparation,
    options: &PreprocessSettings,
) -> Result<usize, neolint::SetupError> {
    match (preparation, text) {
        (UnitPreparation::Preprocessed, Some(text)) => {
            run.add_preprocessed_text(display_name, text, options.clone())
        }
        (UnitPreparation::Preprocessed, None) => {
            run.add_preprocessed_file(display_name, options.clone())
        }
        (UnitPreparation::Identity, Some(text)) => run.add_prepared_text(display_name, text),
        (UnitPreparation::Identity, None) => run.add_prepared_file(display_name),
    }
}

/// Emit the Compare-mode report on stderr only.
fn print_compare_report(reports: &[CompareUnitReport], format: OutputFormat, all_matched: bool) {
    match format {
        OutputFormat::Human => print_compare_human(reports),
        OutputFormat::Json => print_compare_json(reports, all_matched),
    }
}

/// Compact, greppable human Compare report.
fn print_compare_human(reports: &[CompareUnitReport]) {
    eprint!("{}", compare_report_human(reports));
}

/// Render the human Compare report, showing unknown totals as `?`.
fn compare_report_human(reports: &[CompareUnitReport]) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    for item in reports {
        let _ = writeln!(
            output,
            "neolint: compare unit {} {}: matched={} complete={} truncated={} legacy={} ast={}",
            item.unit_index,
            item.path,
            item.report.matched,
            item.report.comparison_complete,
            item.report.details_truncated,
            item.report.legacy_count,
            item.report.ast_count
        );
        for rule in &item.report.by_rule {
            // Zero counts on both sides prove a zero delta in both the complete
            // and incomplete paths, so those rules carry nothing to render. No
            // unknown (`None`) total is ever coerced to a number here.
            if rule.legacy_count == 0 && rule.ast_count == 0 {
                continue;
            }
            let _ = writeln!(
                output,
                "neolint: compare unit {} rule {}: legacy={} ast={} missing={} extra={}",
                item.unit_index,
                rule.code,
                rule.legacy_count,
                rule.ast_count,
                format_compare_total(rule.missing_total),
                format_compare_total(rule.extra_total)
            );
            for fingerprint in &rule.missing_from_ast {
                let _ = writeln!(
                    output,
                    "{}",
                    compare_fingerprint_line(item.unit_index, "missing", fingerprint)
                );
            }
            for fingerprint in &rule.extra_in_ast {
                let _ = writeln!(
                    output,
                    "{}",
                    compare_fingerprint_line(item.unit_index, "extra", fingerprint)
                );
            }
        }
    }
    output
}

/// Render one per-rule total; `None` means the comparison was not performed.
fn format_compare_total(total: Option<usize>) -> String {
    match total {
        Some(value) => value.to_string(),
        None => "?".to_owned(),
    }
}

/// Render one exact fingerprint on a single greppable line.
fn compare_fingerprint_line(
    unit_index: usize,
    direction: &str,
    fingerprint: &DiagnosticFingerprint,
) -> String {
    format!(
        "neolint: compare unit {unit_index} {direction} {} {} {} message={:?} start={} end={} line={} column={} end_line={} end_column={}",
        fingerprint.code,
        fingerprint.severity.as_str(),
        fingerprint.kind.as_str(),
        fingerprint.message,
        fingerprint.start,
        fingerprint.end,
        fingerprint.line,
        fingerprint.column,
        fingerprint.end_line,
        fingerprint.end_column
    )
}

/// Single documented JSON object for the Compare report on stderr.
fn print_compare_json(reports: &[CompareUnitReport], all_matched: bool) {
    eprint!("{}", compare_report_json(reports, all_matched));
}

/// Render the Compare report JSON object, emitting `null` for unknown totals.
fn compare_report_json(reports: &[CompareUnitReport], all_matched: bool) -> String {
    let mut output = String::new();
    output.push('{');
    push_json_key(&mut output, "engine");
    push_json_string(&mut output, "compare");
    output.push(',');
    push_json_key(&mut output, "units");
    output.push('[');
    for (item_index, item) in reports.iter().enumerate() {
        if item_index > 0 {
            output.push(',');
        }
        output.push('{');
        push_json_key(&mut output, "unit");
        output.push_str(&item.unit_index.to_string());
        output.push(',');
        push_json_key(&mut output, "path");
        push_json_string(&mut output, &item.path);
        output.push(',');
        push_json_key(&mut output, "matched");
        output.push_str(bool_str(item.report.matched));
        output.push(',');
        push_json_key(&mut output, "comparison_complete");
        output.push_str(bool_str(item.report.comparison_complete));
        output.push(',');
        push_json_key(&mut output, "details_truncated");
        output.push_str(bool_str(item.report.details_truncated));
        output.push(',');
        push_json_key(&mut output, "legacy_count");
        output.push_str(&item.report.legacy_count.to_string());
        output.push(',');
        push_json_key(&mut output, "ast_count");
        output.push_str(&item.report.ast_count.to_string());
        output.push(',');
        push_json_key(&mut output, "by_rule");
        output.push('[');
        for (rule_index, rule) in item.report.by_rule.iter().enumerate() {
            if rule_index > 0 {
                output.push(',');
            }
            output.push('{');
            push_json_key(&mut output, "code");
            push_json_string(&mut output, rule.code);
            output.push(',');
            push_json_key(&mut output, "legacy_count");
            output.push_str(&rule.legacy_count.to_string());
            output.push(',');
            push_json_key(&mut output, "ast_count");
            output.push_str(&rule.ast_count.to_string());
            output.push(',');
            push_json_key(&mut output, "missing_total");
            push_compare_total(&mut output, rule.missing_total);
            output.push(',');
            push_json_key(&mut output, "extra_total");
            push_compare_total(&mut output, rule.extra_total);
            output.push(',');
            push_json_key(&mut output, "missing_from_ast");
            push_fingerprint_array(&mut output, &rule.missing_from_ast);
            output.push(',');
            push_json_key(&mut output, "extra_in_ast");
            push_fingerprint_array(&mut output, &rule.extra_in_ast);
            output.push('}');
        }
        output.push(']');
        output.push('}');
    }
    output.push(']');
    output.push(',');
    push_json_key(&mut output, "matched");
    output.push_str(bool_str(all_matched));
    output.push('}');
    output.push('\n');
    output
}

/// Append one per-rule JSON total; `null` marks a total that was not computed.
fn push_compare_total(output: &mut String, total: Option<usize>) {
    match total {
        Some(value) => output.push_str(&value.to_string()),
        None => output.push_str("null"),
    }
}

fn bool_str(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn push_json_key(output: &mut String, key: &str) {
    output.push('"');
    output.push_str(key);
    output.push('"');
    output.push(':');
}

fn push_json_string(output: &mut String, value: &str) {
    output.push('"');
    output.push_str(&json_escape(value));
    output.push('"');
}

fn push_fingerprint_array(output: &mut String, fingerprints: &[DiagnosticFingerprint]) {
    output.push('[');
    for (index, fingerprint) in fingerprints.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('{');
        push_json_key(output, "code");
        push_json_string(output, fingerprint.code);
        output.push(',');
        push_json_key(output, "severity");
        push_json_string(output, fingerprint.severity.as_str());
        output.push(',');
        push_json_key(output, "kind");
        push_json_string(output, fingerprint.kind.as_str());
        output.push(',');
        push_json_key(output, "message");
        push_json_string(output, &fingerprint.message);
        output.push(',');
        push_json_key(output, "start");
        output.push_str(&fingerprint.start.to_string());
        output.push(',');
        push_json_key(output, "end");
        output.push_str(&fingerprint.end.to_string());
        output.push(',');
        push_json_key(output, "line");
        output.push_str(&fingerprint.line.to_string());
        output.push(',');
        push_json_key(output, "column");
        output.push_str(&fingerprint.column.to_string());
        output.push(',');
        push_json_key(output, "end_line");
        output.push_str(&fingerprint.end_line.to_string());
        output.push(',');
        push_json_key(output, "end_column");
        output.push_str(&fingerprint.end_column.to_string());
        output.push('}');
    }
    output.push(']');
}

fn prepared_path(unit: &neolint::AnalysisUnit) -> String {
    unit.root()
        .and_then(|root| unit.manager().file(root))
        .map(|file| file.display_name().to_owned())
        .unwrap_or_else(|| "<prepared>".to_owned())
}

fn parse_severity(value: &str) -> Option<Severity> {
    match value {
        "error" => Some(Severity::Error),
        "warning" => Some(Severity::Warning),
        "info" => Some(Severity::Info),
        _ => None,
    }
}

/// Resolve explicit files and recursively expand directories into sorted .lsl files.
/// Symlinks encountered during traversal are skipped to avoid directory cycles.
/// Expand explicit files/directories and stdin into a deterministic input list.
///
/// Directory inputs are recursively scanned for case-insensitive `.lsl` files;
/// entries are sorted by name and symlinks are skipped. Duplicate displayed
/// paths are removed. `-` is retained at most once.
///
/// # Errors
///
/// Returns an error for inaccessible inputs, unsupported file types, or an empty
/// expanded input set.
fn collect_input_paths(inputs: &[String]) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for input in inputs {
        if input == "-" {
            if seen.insert(input.clone()) {
                paths.push(input.clone());
            }
            continue;
        }

        let path = PathBuf::from(input);
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("cannot access '{}': {error}", path.display()))?;

        if metadata.is_dir() {
            collect_directory(&path, &mut paths, &mut seen)?;
        } else if metadata.is_file() {
            let display = path.to_string_lossy().into_owned();
            if seen.insert(display.clone()) {
                paths.push(display);
            }
        } else {
            return Err(format!(
                "'{}' is not a regular file or directory",
                path.display()
            ));
        }
    }

    if paths.is_empty() {
        return Err("no input files found (directories must contain .lsl files)".to_owned());
    }

    Ok(paths)
}

/// Recursively collect `.lsl` files from one directory in sorted name order.
fn collect_directory(
    directory: &Path,
    paths: &mut Vec<String>,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot read directory '{}': {error}", directory.display()))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot list directory '{}': {error}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect '{}': {error}", path.display()))?;

        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            collect_directory(&path, paths, seen)?;
        } else if file_type.is_file() && has_lsl_extension(&path) {
            let display = path.to_string_lossy().into_owned();
            if seen.insert(display.clone()) {
                paths.push(display);
            }
        }
    }

    Ok(())
}

fn has_lsl_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lsl"))
}

fn print_help() {
    println!(
        "neolint {} — CLI-only LSL linter\n\nUSAGE:\n    neolint [OPTIONS] <FILE|DIRECTORY...|->\n\nOPTIONS:\n    --format <human|json>  Diagnostic output format (default: human)\n    --config <FILE>        Load rule settings from a config file\n    --disable <RULES>      Disable comma-separated rule codes\n    --severity <R=LEVEL>   Override a rule severity (error, warning, info)\n    --engine <ENGINE>      Analysis engine: ast (default), legacy, or compare\n    --preprocess           Enable standard LSL preprocessing\n    --dialect <DIALECT>    Select lsl or firestorm preprocessing; firestorm\n                           enables preprocessing\n    --target <PROFILE>     Select second-life-mono (default), second-life-lso,\n                           second-life-luau, opensim-xengine or opensim-yengine\n                           (independent of --dialect; AST/Compare only)\n    --target-version <VER> Select pinned OpenSim API version: 0.9.2.2, 0.9.3.0,\n                           0.9.3.1 or master-2026-11-25 (OpenSim only)\n    --strict-ossl          Check all known OSSL calls against an explicit allowlist\n                           (requires an OpenSim target)\n    --ossl-function <NAME> Add an OpenSim OSSL function to the project allowlist\n                           (repeatable; requires an OpenSim target)\n    -I <DIR>, --include-dir <DIR>\n                           Add an include search directory (repeatable)\n    -IDIR and --include-dir=DIR are accepted; --dialect=DIALECT is also accepted.\n    --tokens               Print lexer tokens (development aid)\n    -h, --help             Print help\n    -V, --version          Print version\n\nCONFIG FORMAT:\n    disable = SEM001, SEM003\n    severity.SEM004 = warning\n    engine = ast\n    target = opensim-yengine\n    target_version = 0.9.3.1\n    ossl_allowlist = strict\n    ossl_function = osGetAgentIP\n\nDIRECTORIES:\n    Directories are searched recursively for .lsl files (case-insensitive).\n\nEXIT CODES:\n    0  No error-severity diagnostics\n    1  At least one error-severity diagnostic\n    2  Input, configuration or command-line error\n    3  Compare mode: at least one unit's semantic comparison is unmatched",
        env!("CARGO_PKG_VERSION")
    );
}

/// Render tokens (when requested) and diagnostics in `path:line:column` form.
fn print_human(results: &[FileResult], show_tokens: bool) {
    for result in results {
        if show_tokens {
            for token in &result.tokens {
                println!(
                    "{}:{}:{}: {:?} {:?}",
                    result.path, token.span.line, token.span.column, token.kind, token.lexeme
                );
            }
        }

        if result.diagnostics.is_empty() {
            if !show_tokens {
                println!("{}: no diagnostics", result.path);
            }
        } else {
            for item in &result.diagnostics {
                let diagnostic = &item.diagnostic;
                println!(
                    "{}:{}:{}: {}[{}]: {}",
                    result.path,
                    diagnostic.span.line,
                    diagnostic.span.column,
                    diagnostic.severity.as_str(),
                    diagnostic.code,
                    diagnostic.message
                );
            }
        }
    }
}

/// Render all diagnostics as a JSON array with stable field ordering.
fn print_json(results: &[FileResult]) {
    let mut first = true;
    print!("[");
    for result in results {
        for item in &result.diagnostics {
            if !first {
                print!(",");
            }
            first = false;
            let diagnostic = &item.diagnostic;
            print!(
                "{{\"file\":\"{}\",\"code\":\"{}\",\"severity\":\"{}\",\"kind\":\"{}\",\"message\":\"{}\",\"line\":{},\"column\":{},\"end_line\":{},\"end_column\":{},\"start\":{},\"end\":{}",
                json_escape(&result.path),
                diagnostic.code,
                diagnostic.severity.as_str(),
                diagnostic.kind.as_str(),
                json_escape(&diagnostic.message),
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.span.end_line,
                diagnostic.span.end_column,
                diagnostic.span.start,
                diagnostic.span.end
            );
            // Preserve the existing kind vocabulary for JSON v1 consumers.
            if diagnostic.code.starts_with("PRE") && diagnostic.code != "PRE039" {
                print!(",\"origin\":\"preprocessor\"");
            }
            if !item.related.is_empty() {
                print!(",\"related\":[");
                for (index, related) in item.related.iter().enumerate() {
                    if index > 0 {
                        print!(",");
                    }
                    print!(
                        "{{\"file\":\"{}\",\"label\":\"{}\",\"line\":{},\"column\":{},\"end_line\":{},\"end_column\":{},\"start\":{},\"end\":{}}}",
                        json_escape(&related.file),
                        json_escape(&related.label),
                        related.span.line,
                        related.span.column,
                        related.span.end_line,
                        related.span.end_column,
                        related.span.start,
                        related.span.end
                    );
                }
                print!("]");
            }
            print!("}}");
        }
    }
    println!("]");
}

fn json_related_locations(
    unit: &neolint::AnalysisUnit,
    related: &[neolint::RelatedLocation],
) -> Result<Vec<JsonRelatedLocation>, String> {
    if related.is_empty() {
        return Ok(Vec::new());
    }
    let map = unit
        .source_map()
        .ok_or_else(|| "analysis unit has no source map".to_owned())?;
    let mut output = Vec::with_capacity(related.len());
    for location in related {
        let file = unit
            .manager()
            .file(location.span.source())
            .ok_or_else(|| "related location references an unknown source file".to_owned())?;
        let span = map
            .render_span(location.span)
            .map_err(|error| format!("cannot resolve related source span: {error}"))?;
        output.push(JsonRelatedLocation {
            file: file.display_name().to_owned(),
            label: location.label.clone(),
            span,
        });
    }
    Ok(output)
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{
        collect_input_paths, compare_report_human, compare_report_json, format_compare_total,
        has_lsl_extension, json_escape, push_compare_total, CompareUnitReport,
    };
    use neolint::{EngineComparisonReport, RuleComparison};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

    fn temporary_directory() -> PathBuf {
        let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("neolint-cli-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn comparison_report(
        comparison_complete: bool,
        missing_total: Option<usize>,
        extra_total: Option<usize>,
    ) -> EngineComparisonReport {
        EngineComparisonReport {
            matched: false,
            comparison_complete,
            details_truncated: false,
            legacy_count: 3,
            ast_count: 2,
            by_rule: vec![RuleComparison {
                code: "SEM001",
                legacy_count: 3,
                ast_count: 2,
                missing_total,
                extra_total,
                missing_from_ast: Vec::new(),
                extra_in_ast: Vec::new(),
            }],
        }
    }

    fn unit_report(report: EngineComparisonReport) -> CompareUnitReport {
        CompareUnitReport {
            unit_index: 0,
            path: "unknown.lsl".to_owned(),
            report,
        }
    }

    #[test]
    fn compare_renderers_show_unknown_totals_without_panicking() {
        assert_eq!(format_compare_total(Some(0)), "0");
        assert_eq!(format_compare_total(Some(7)), "7");
        assert_eq!(format_compare_total(None), "?");
        let mut total = String::new();
        push_compare_total(&mut total, None);
        assert_eq!(total, "null");

        let reports = [unit_report(comparison_report(false, None, None))];

        let human = compare_report_human(&reports);
        assert!(
            human.contains("rule SEM001: legacy=3 ast=2 missing=? extra=?"),
            "{human}"
        );
        assert!(human.contains("complete=false"), "{human}");
        assert!(!human.contains("missing=0"), "{human}");

        let json = compare_report_json(&reports, false);
        assert!(json.contains("\"comparison_complete\":false"), "{json}");
        assert!(json.contains("\"missing_total\":null"), "{json}");
        assert!(json.contains("\"extra_total\":null"), "{json}");
        assert!(json.contains("\"missing_from_ast\":[]"), "{json}");
        assert!(json.contains("\"extra_in_ast\":[]"), "{json}");
    }

    #[test]
    fn compare_renderers_keep_exact_totals_numeric() {
        let reports = [unit_report(comparison_report(true, Some(0), Some(2)))];
        let human = compare_report_human(&reports);
        assert!(
            human.contains("rule SEM001: legacy=3 ast=2 missing=0 extra=2"),
            "{human}"
        );
        let json = compare_report_json(&reports, false);
        assert!(json.contains("\"missing_total\":0"), "{json}");
        assert!(json.contains("\"extra_total\":2"), "{json}");
        assert!(!json.contains("null"), "{json}");
    }

    #[test]
    fn json_escaping_handles_quotes_slashes_and_control_chars() {
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
    }

    #[test]
    fn detects_lsl_extension_without_case_sensitivity() {
        assert!(has_lsl_extension(std::path::Path::new("script.lsl")));
        assert!(has_lsl_extension(std::path::Path::new("SCRIPT.LSL")));
        assert!(!has_lsl_extension(std::path::Path::new("script.txt")));
    }

    #[test]
    fn recursively_collects_lsl_files_and_ignores_other_files() {
        let root = temporary_directory();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("main.lsl"), "default {}").unwrap();
        fs::write(root.join("notes.txt"), "not an LSL script").unwrap();
        fs::write(nested.join("extra.LSL"), "default {}").unwrap();

        let paths = collect_input_paths(&[root.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("main.lsl"));
        assert!(paths[1].ends_with("extra.LSL"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_files_are_kept_even_without_lsl_extension() {
        let root = temporary_directory();
        let file = root.join("snippet.source");
        fs::write(&file, "default {}").unwrap();

        let paths = collect_input_paths(&[file.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("snippet.source"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_empty_directories_without_lsl_files() {
        let root = temporary_directory();
        let error = collect_input_paths(&[root.to_string_lossy().into_owned()]).unwrap_err();
        assert!(error.contains("no input files found"));
        fs::remove_dir_all(root).unwrap();
    }
}
