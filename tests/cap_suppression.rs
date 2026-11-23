//! Suppression-before-rendered-cap regression tests.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

fn temporary_directory() -> PathBuf {
    let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "neolint-cap-suppression-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn neolint() -> Command {
    Command::new(env!("CARGO_BIN_EXE_neolint"))
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = neolint()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn write_file(directory: &std::path::Path, name: &str, contents: &str) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn unresolved_variable_flood(statements: usize) -> String {
    let mut source = String::from("default { state_entry() {\n");
    for _ in 0..statements {
        source.push_str("x = 1;\n");
    }
    source.push_str("} }\n");
    source
}

fn json_code_count(text: &str, code: &str) -> usize {
    text.matches(&format!("\"code\":\"{code}\"")).count()
}

fn json_last_code(text: &str) -> Option<&str> {
    const MARKER: &str = "\"code\":\"";
    let start = text.rfind(MARKER)? + MARKER.len();
    text[start..].split('"').next()
}

#[test]
fn enabled_unresolved_variable_flood_is_capped_with_terminal_pre039() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = unresolved_variable_flood(limit + 2);
    let output = run_with_stdin(
        &["--engine", "ast", "--format", "json", "-"],
        source.as_bytes(),
    );

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("\"code\":\"").count(), limit);
    assert_eq!(json_code_count(&stdout, "SEM012"), limit - 1);
    assert_eq!(json_code_count(&stdout, "PRE039"), 1);
    assert_eq!(
        stdout
            .matches("\"code\":\"PRE039\",\"severity\":\"error\"")
            .count(),
        1
    );
    assert_eq!(json_last_code(&stdout), Some("PRE039"));
}

#[test]
fn cli_disabled_unresolved_variable_flood_does_not_trigger_pre039() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = unresolved_variable_flood(limit + 2);
    let output = run_with_stdin(
        &[
            "--engine",
            "ast",
            "--format",
            "json",
            "--disable",
            "SEM012",
            "-",
        ],
        source.as_bytes(),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), "[]");
    assert_eq!(json_code_count(&stdout, "SEM012"), 0);
    assert_eq!(json_code_count(&stdout, "PRE039"), 0);
}

#[test]
fn configured_disabled_unresolved_variable_flood_does_not_trigger_pre039() {
    let directory = temporary_directory();
    let config = write_file(&directory, "no-sem012.conf", "disable = SEM012\n");
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = unresolved_variable_flood(limit + 2);
    let output = run_with_stdin(
        &[
            "--config",
            config.to_str().unwrap(),
            "--engine",
            "ast",
            "--format",
            "json",
            "-",
        ],
        source.as_bytes(),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), "[]");
    assert_eq!(json_code_count(&stdout, "SEM012"), 0);
    assert_eq!(json_code_count(&stdout, "PRE039"), 0);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn default_ast_enabled_flood_is_capped_with_terminal_pre039() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = unresolved_variable_flood(limit + 2);
    let output = run_with_stdin(&["--format", "json", "-"], source.as_bytes());

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("\"code\":\"").count(), limit);
    assert_eq!(json_code_count(&stdout, "SEM012"), limit - 1);
    assert_eq!(json_code_count(&stdout, "PRE039"), 1);
    assert_eq!(json_last_code(&stdout), Some("PRE039"));

    // The human renderer must apply the same cap: exactly `limit` diagnostic
    // lines and a terminal PRE039, not an extra line or a truncated tail.
    let human = run_with_stdin(&["-"], source.as_bytes());
    assert_eq!(human.status.code(), Some(1));
    let human_stdout = String::from_utf8(human.stdout).unwrap();
    assert_eq!(human_stdout.lines().count(), limit);
    assert_eq!(human_stdout.matches("[PRE039]").count(), 1);
    let last_line = human_stdout.lines().last().unwrap();
    assert!(
        last_line.contains("error[PRE039]"),
        "human cap tail is not PRE039: {last_line}"
    );
}

#[test]
fn default_ast_disabled_flood_does_not_trigger_pre039() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = unresolved_variable_flood(limit + 2);
    let output = run_with_stdin(
        &["--format", "json", "--disable", "SEM012", "-"],
        source.as_bytes(),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), "[]");
    assert_eq!(json_code_count(&stdout, "SEM012"), 0);
    assert_eq!(json_code_count(&stdout, "PRE039"), 0);
}

#[test]
fn explicit_legacy_direct_flood_remains_uncapped() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let source = unresolved_variable_flood(limit + 2);
    let directory = temporary_directory();
    let config = write_file(&directory, "legacy.conf", "engine = legacy\n");
    let config_path = config.to_str().unwrap();

    for (selection, args) in [
        (
            "--engine legacy",
            vec!["--engine", "legacy", "--format", "json", "-"],
        ),
        (
            "--engine=legacy",
            vec!["--engine=legacy", "--format", "json", "-"],
        ),
        (
            "config engine = legacy",
            vec!["--config", config_path, "--format", "json", "-"],
        ),
    ] {
        let output = run_with_stdin(&args, source.as_bytes());
        assert_eq!(output.status.code(), Some(0), "{selection}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(json_code_count(&stdout, "SEM012"), limit + 2, "{selection}");
        assert_eq!(json_code_count(&stdout, "PRE039"), 0, "{selection}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn suppression_does_not_mask_overflow_from_other_rules() {
    let limit = neolint::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
    let mut source = "integer duplicate;\n".repeat(limit + 2);
    source.push_str("default { state_entry() { x = 1; y = 2; } }\n");
    let output = run_with_stdin(
        &[
            "--engine",
            "ast",
            "--format",
            "json",
            "--disable",
            "SEM012",
            "-",
        ],
        source.as_bytes(),
    );

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("\"code\":\"").count(), limit);
    assert_eq!(json_code_count(&stdout, "SEM012"), 0);
    assert_eq!(json_code_count(&stdout, "SEM001"), limit - 1);
    assert_eq!(json_code_count(&stdout, "PRE039"), 1);
    assert_eq!(json_last_code(&stdout), Some("PRE039"));
}
