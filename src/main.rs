//! Command-line entry point for neolint.
//!
//! Pipeline:
//! argument parsing → configuration loading/validation → input collection
//! (files, directories, stdin) → source reading → [`neolint::lex`] →
//! [`neolint::parse`] → [`neolint::analyze`] → configuration application →
//! human/JSON rendering → exit code.
//!
//! The default plain-LSL Legacy path is unchanged. With `--preprocess`, units
//! use the full preprocessor; with `--engine ast`/`--engine compare` and no
//! preprocessing, units use identity preparation so the engine API has a source
//! map. `compare` prints a semantic-parity report on stderr and exits with code
//! 3 when any unit's comparison is unmatched.
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
    EngineAnalysisOptions, EngineComparisonReport, PreprocessDialect, PreprocessSettings, Severity,
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

struct FileResult {
    path: String,
    tokens: Vec<neolint::Token>,
    diagnostics: Vec<Diagnostic>,
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

    // Ast/Compare without preprocessing still need a prepared source (identity
    // preparation). Plain Legacy keeps the direct, uncapped loop below.
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
        .any(|diagnostic| diagnostic.severity == Severity::Error);

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
        let analysis = match unit.analyze_with_engine_options(engine, &options) {
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

        let mut groups: Vec<(neolint::SourceId, Vec<Diagnostic>)> = Vec::new();
        for diagnostic in analysis.diagnostics() {
            let source = diagnostic.source();
            if let Some((_, items)) = groups.iter_mut().find(|(id, _)| *id == source) {
                items.push(diagnostic.diagnostic().clone());
            } else {
                groups.push((source, vec![diagnostic.diagnostic().clone()]));
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
            config.apply(&mut diagnostics);
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
            config.apply(&mut diagnostics);
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
                .retain(|diagnostic| diagnostic.code != "PRE039");
            if result.diagnostics.len() > remaining {
                result.diagnostics.truncate(remaining);
            }
            remaining -= result.diagnostics.len();
        }
        if let Some(first) = results.first_mut() {
            first.diagnostics.push(Diagnostic {
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
        .any(|diagnostic| diagnostic.severity == Severity::Error);
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
    for item in reports {
        eprintln!(
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
            if rule.legacy_count == 0
                && rule.ast_count == 0
                && rule.missing_total == 0
                && rule.extra_total == 0
            {
                continue;
            }
            eprintln!(
                "neolint: compare unit {} rule {}: legacy={} ast={} missing={} extra={}",
                item.unit_index,
                rule.code,
                rule.legacy_count,
                rule.ast_count,
                rule.missing_total,
                rule.extra_total
            );
            for fingerprint in &rule.missing_from_ast {
                print_compare_fingerprint(item.unit_index, "missing", fingerprint);
            }
            for fingerprint in &rule.extra_in_ast {
                print_compare_fingerprint(item.unit_index, "extra", fingerprint);
            }
        }
    }
}

/// Print one exact fingerprint on a single greppable line.
fn print_compare_fingerprint(
    unit_index: usize,
    direction: &str,
    fingerprint: &DiagnosticFingerprint,
) {
    eprintln!(
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
    );
}

/// Single documented JSON object for the Compare report on stderr.
fn print_compare_json(reports: &[CompareUnitReport], all_matched: bool) {
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
            output.push_str(&rule.missing_total.to_string());
            output.push(',');
            push_json_key(&mut output, "extra_total");
            output.push_str(&rule.extra_total.to_string());
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
    eprint!("{output}");
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
        "neolint {} — CLI-only LSL linter\n\nUSAGE:\n    neolint [OPTIONS] <FILE|DIRECTORY...|->\n\nOPTIONS:\n    --format <human|json>  Diagnostic output format (default: human)\n    --config <FILE>        Load rule settings from a config file\n    --disable <RULES>      Disable comma-separated rule codes\n    --severity <R=LEVEL>   Override a rule severity (error, warning, info)\n    --engine <ENGINE>      Analysis engine: legacy (default), ast, or compare\n    --tokens               Print lexer tokens (development aid)\n    -h, --help             Print help\n    -V, --version          Print version\n\nCONFIG FORMAT:\n    disable = SEM001, SEM003\n    severity.SEM004 = warning\n    engine = legacy\n\nDIRECTORIES:\n    Directories are searched recursively for .lsl files (case-insensitive).\n\nEXIT CODES:\n    0  No error-severity diagnostics\n    1  At least one error-severity diagnostic\n    2  Input, configuration or command-line error\n    3  Compare mode: at least one unit's semantic comparison is unmatched",
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
            for diagnostic in &result.diagnostics {
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
        for diagnostic in &result.diagnostics {
            if !first {
                print!(",");
            }
            first = false;
            print!(
                "{{\"file\":\"{}\",\"code\":\"{}\",\"severity\":\"{}\",\"kind\":\"{}\",\"message\":\"{}\",\"line\":{},\"column\":{},\"end_line\":{},\"end_column\":{},\"start\":{},\"end\":{}}}",
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
        }
    }
    println!("]");
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
    use super::{collect_input_paths, has_lsl_extension, json_escape};
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
