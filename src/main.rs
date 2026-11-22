//! Command-line entry point for neolint.
//!
//! Pipeline:
//! argument parsing → configuration loading/validation → input collection
//! (files, directories, stdin) → source reading → [`neolint::lex`] →
//! [`neolint::parse`] → [`neolint::analyze`] → configuration application →
//! human/JSON rendering → exit code.
//!
//! Exit codes are `0` when no error-severity diagnostics remain, `1` when at
//! least one error-severity diagnostic remains, and `2` for command-line,
//! configuration, input, or file-read errors. Warnings/info do not fail by
//! default. Directory traversal is sorted and skips symlinks; explicit files are
//! accepted regardless of extension; stdin (`-`) may appear only once.
use neolint::config::LintConfig;
use neolint::semantic::analyze_with_syntax_diagnostics;
use neolint::{
    lex, parse, AnalysisRun, Diagnostic, PreprocessDialect, PreprocessOptions, Severity,
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
        return run_preprocessed(&paths, &config, format, show_tokens);
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

fn run_preprocessed(
    paths: &[String],
    config: &LintConfig,
    format: OutputFormat,
    show_tokens: bool,
) -> i32 {
    let options = PreprocessOptions::full()
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
            run.add_preprocessed_text("<stdin>", &input, options.clone())
        } else {
            run.add_preprocessed_file(path, options.clone())
        };
        match result {
            Ok(index) => unit_indexes.push(index),
            Err(error) => {
                eprintln!("neolint: cannot prepare '{path}': {error}");
                input_failed = true;
            }
        }
    }

    let mut results = Vec::new();
    for index in unit_indexes {
        let Some(unit) = run.unit(index) else {
            continue;
        };
        let prepared = match unit.prepared_source() {
            Some(prepared) => prepared,
            None => continue,
        };
        let rendered = match unit.analyze() {
            Ok(rendered) => rendered,
            Err(error) => {
                eprintln!("neolint: cannot analyze unit {}: {error}", index);
                input_failed = true;
                continue;
            }
        };
        let mut groups: Vec<(neolint::SourceId, Vec<Diagnostic>)> = Vec::new();
        for diagnostic in rendered {
            let source = diagnostic.source();
            if let Some((_, items)) = groups.iter_mut().find(|(id, _)| *id == source) {
                items.push(diagnostic.diagnostic().clone());
            } else {
                groups.push((source, vec![diagnostic.diagnostic().clone()]));
            }
        }
        let root = unit.root();
        let prepared_tokens = if show_tokens {
            lex(prepared.text()).0
        } else {
            Vec::new()
        };
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
                    prepared_tokens.clone()
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
            if result.diagnostics.len() > remaining {
                result.diagnostics.truncate(remaining);
                remaining = 0;
            } else {
                remaining -= result.diagnostics.len();
            }
            if remaining == 0 {
                break;
            }
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
        "neolint {} — CLI-only LSL linter\n\nUSAGE:\n    neolint [OPTIONS] <FILE|DIRECTORY...|->\n\nOPTIONS:\n    --format <human|json>  Diagnostic output format (default: human)\n    --config <FILE>        Load rule settings from a config file\n    --disable <RULES>      Disable comma-separated rule codes\n    --severity <R=LEVEL>   Override a rule severity (error, warning, info)\n    --tokens               Print lexer tokens (development aid)\n    -h, --help             Print help\n    -V, --version          Print version\n\nCONFIG FORMAT:\n    disable = SEM001, SEM003\n    severity.SEM004 = warning\n\nDIRECTORIES:\n    Directories are searched recursively for .lsl files (case-insensitive).\n\nEXIT CODES:\n    0  No error-severity diagnostics\n    1  At least one error-severity diagnostic\n    2  Input, configuration or command-line error",
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
