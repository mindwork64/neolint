use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use neolint::MAX_COMPARE_FINGERPRINTS_PER_ENGINE;

static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

fn temporary_directory() -> PathBuf {
    let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "neolint-cli-integration-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn neolint() -> Command {
    Command::new(env!("CARGO_BIN_EXE_neolint"))
}

fn run_args(args: &[&str]) -> std::process::Output {
    neolint().args(args).output().unwrap()
}

fn run_args_with_stdin(args: &[&str], input: &[u8]) -> std::process::Output {
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

/// Assert that no `--engine` selection is byte-for-byte explicit AST.
fn assert_default_matches_explicit_ast(args: &[&str]) {
    let default = run_args(args);
    let mut with_ast = vec!["--engine", "ast"];
    with_ast.extend_from_slice(args);
    let explicit_ast = run_args(&with_ast);
    assert_eq!(
        default.status.code(),
        explicit_ast.status.code(),
        "exit status differs for {args:?}"
    );
    assert_eq!(
        default.stdout, explicit_ast.stdout,
        "stdout differs for {args:?}"
    );
    assert_eq!(
        default.stderr, explicit_ast.stderr,
        "stderr differs for {args:?}"
    );
}

#[test]
fn unresolved_variable_warning_does_not_fail_cli_by_default() {
    let directory = temporary_directory();
    let source = directory.join("unresolved.lsl");

    fs::write(
        &source,
        "default { state_entry() { llOwnerSay(missing_name); } }",
    )
    .unwrap();

    let output = neolint().arg(&source).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("warning[SEM012]"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn discarded_comparison_warning_does_not_fail_and_can_be_configured() {
    let directory = temporary_directory();
    let source = directory.join("discarded.lsl");
    fs::write(
        &source,
        "default { state_entry() { integer a; integer b; a == b; } }",
    )
    .unwrap();

    let default_output = neolint().arg(&source).output().unwrap();
    assert!(default_output.status.success());
    let default_stdout = String::from_utf8(default_output.stdout).unwrap();
    assert!(
        default_stdout.contains("warning[SEM023]"),
        "{default_stdout}"
    );

    let escalated = neolint()
        .arg("--severity")
        .arg("SEM023=error")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(escalated.status.code(), Some(1));
    let escalated_stdout = String::from_utf8(escalated.stdout).unwrap();
    assert!(
        escalated_stdout.contains("error[SEM023]"),
        "{escalated_stdout}"
    );

    let disabled = neolint()
        .arg("--disable")
        .arg("SEM023")
        .arg(&source)
        .output()
        .unwrap();
    assert!(disabled.status.success());
    let disabled_stdout = String::from_utf8(disabled.stdout).unwrap();
    assert!(!disabled_stdout.contains("SEM023"), "{disabled_stdout}");
    assert!(
        disabled_stdout.contains("no diagnostics"),
        "{disabled_stdout}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem024_ast_engines_emit_warning_but_legacy_does_not() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused.lsl",
        "default { state_entry() { integer unused; } }\n",
    );
    let source = source.to_str().unwrap();

    let default_output = run_args(&[source]);
    let explicit_ast = run_args(&["--engine", "ast", source]);
    for output in [&default_output, &explicit_ast] {
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("warning[SEM024]"), "{stdout}");
        assert!(
            stdout.contains("variable 'unused' is declared but never read"),
            "{stdout}"
        );
    }
    assert_eq!(default_output.stdout, explicit_ast.stdout);
    assert_eq!(default_output.stderr, explicit_ast.stderr);
    assert_eq!(default_output.status.code(), explicit_ast.status.code());

    let legacy = run_args(&["--engine", "legacy", source]);
    assert_eq!(legacy.status.code(), Some(0));
    let legacy_stdout = String::from_utf8(legacy.stdout).unwrap();
    assert!(!legacy_stdout.contains("SEM024"), "{legacy_stdout}");
    assert_eq!(
        legacy_stdout,
        format!("{source}: no diagnostics\n"),
        "legacy output must remain unchanged"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem024_can_be_disabled_by_cli_and_config() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused.lsl",
        "default { state_entry() { integer unused; } }\n",
    );
    let config = write_file(&directory, "disable.conf", "disable = SEM024\n");
    let source = source.to_str().unwrap();

    let disabled = run_args(&["--disable", "SEM024", source]);
    assert_eq!(disabled.status.code(), Some(0));
    let disabled_stdout = String::from_utf8(disabled.stdout).unwrap();
    assert!(!disabled_stdout.contains("SEM024"), "{disabled_stdout}");
    assert!(
        disabled_stdout.contains("no diagnostics"),
        "{disabled_stdout}"
    );

    let configured = run_args(&["--config", config.to_str().unwrap(), source]);
    assert_eq!(configured.status.code(), Some(0));
    let configured_stdout = String::from_utf8(configured.stdout).unwrap();
    assert!(!configured_stdout.contains("SEM024"), "{configured_stdout}");
    assert!(
        configured_stdout.contains("no diagnostics"),
        "{configured_stdout}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem024_can_be_escalated_to_an_error() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused.lsl",
        "default { state_entry() { integer unused; } }\n",
    );

    let output = run_args(&["--severity", "SEM024=error", source.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("error[SEM024]"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem024_appears_in_json_output_with_the_existing_schema() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused.lsl",
        "default { state_entry() { integer unused; } }\n",
    );

    let output = run_args(&["--format", "json", source.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with('['), "{stdout}");
    assert!(stdout.trim_end().ends_with(']'), "{stdout}");
    for field in [
        "\"code\":\"SEM024\"",
        "\"severity\":\"warning\"",
        "\"kind\":\"lint\"",
        "\"message\":\"variable 'unused' is declared but never read\"",
        "\"line\":1",
        "\"column\":35",
        "\"start\":34",
        "\"end\":40",
    ] {
        assert!(stdout.contains(field), "missing {field}: {stdout}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem025_and_sem026_ast_engines_emit_warnings_but_legacy_does_not() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused-analysis.lsl",
        "integer callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { integer unused_local; llOwnerSay((string)callee(1)); } }\n",
    );
    let source = source.to_str().unwrap();

    let default_output = run_args(&[source]);
    let explicit_ast = run_args(&["--engine", "ast", source]);
    for output in [&default_output, &explicit_ast] {
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("warning[SEM024]"), "{stdout}");
        assert!(stdout.contains("warning[SEM025]"), "{stdout}");
        assert!(stdout.contains("warning[SEM026]"), "{stdout}");
        assert!(
            stdout.contains("parameter 'unused_parameter' is never read"),
            "{stdout}"
        );
        assert!(
            stdout.contains("function 'unused_function' is never called"),
            "{stdout}"
        );
    }
    assert_eq!(default_output.stdout, explicit_ast.stdout);
    assert_eq!(default_output.stderr, explicit_ast.stderr);
    assert_eq!(default_output.status.code(), explicit_ast.status.code());

    let legacy = run_args(&["--engine", "legacy", source]);
    assert_eq!(legacy.status.code(), Some(0));
    let legacy_stdout = String::from_utf8(legacy.stdout).unwrap();
    for code in ["SEM024", "SEM025", "SEM026"] {
        assert!(!legacy_stdout.contains(code), "{legacy_stdout}");
    }
    assert_eq!(
        legacy_stdout,
        format!("{source}: no diagnostics\n"),
        "legacy output must remain unchanged"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem025_and_sem026_can_be_disabled_by_cli_and_config() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused-analysis.lsl",
        "integer callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n",
    );
    let config = write_file(&directory, "disable.conf", "disable = SEM025, SEM026\n");
    let source = source.to_str().unwrap();

    let disabled_parameter = run_args(&["--disable", "SEM025", source]);
    assert_eq!(disabled_parameter.status.code(), Some(0));
    let stdout = String::from_utf8(disabled_parameter.stdout).unwrap();
    assert!(!stdout.contains("SEM025"), "{stdout}");
    assert!(stdout.contains("warning[SEM026]"), "{stdout}");

    let disabled_function = run_args(&["--disable", "SEM026", source]);
    assert_eq!(disabled_function.status.code(), Some(0));
    let stdout = String::from_utf8(disabled_function.stdout).unwrap();
    assert!(!stdout.contains("SEM026"), "{stdout}");
    assert!(stdout.contains("warning[SEM025]"), "{stdout}");

    let configured = run_args(&["--config", config.to_str().unwrap(), source]);
    assert_eq!(configured.status.code(), Some(0));
    let stdout = String::from_utf8(configured.stdout).unwrap();
    assert!(!stdout.contains("SEM025"), "{stdout}");
    assert!(!stdout.contains("SEM026"), "{stdout}");
    assert!(stdout.contains("no diagnostics"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem026_can_be_escalated_to_an_error() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused-function.lsl",
        "unused_function() { }\ndefault { state_entry() { } }\n",
    );

    let output = run_args(&["--severity", "SEM026=error", source.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("error[SEM026]"), "{stdout}");
    assert!(
        stdout.contains("function 'unused_function' is never called"),
        "{stdout}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sem025_and_sem026_appear_in_json_output_with_the_existing_schema() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused-analysis.lsl",
        "integer callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { llOwnerSay((string)callee(1)); } }\n",
    );

    let output = run_args(&["--format", "json", source.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with('['), "{stdout}");
    assert!(stdout.trim_end().ends_with(']'), "{stdout}");
    for field in [
        "\"code\":\"SEM025\"",
        "\"code\":\"SEM026\"",
        "\"severity\":\"warning\"",
        "\"kind\":\"lint\"",
        "\"message\":\"parameter 'unused_parameter' is never read\"",
        "\"message\":\"function 'unused_function' is never called\"",
        "\"line\":1",
        "\"line\":2",
        "\"file\":",
        "\"column\":",
        "\"start\":",
        "\"end\":",
    ] {
        assert!(stdout.contains(field), "missing {field}: {stdout}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn compare_ignores_sem024_sem025_and_sem026() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "unused-analysis.lsl",
        "integer callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { integer unused_local; llOwnerSay((string)callee(1)); } }\n",
    );
    let source = source.to_str().unwrap();

    let output = run_args(&["--engine", "compare", source]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    for code in ["SEM024", "SEM025", "SEM026"] {
        assert!(!stdout.contains(code), "{stdout}");
    }
    assert_eq!(
        stdout,
        format!("{source}: no diagnostics\n"),
        "compare displays only the Legacy stream"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("matched=true"), "{stderr}");
    assert!(stderr.contains("complete=true"), "{stderr}");
    assert!(!stderr.contains("SEM025"), "{stderr}");
    assert!(!stderr.contains("SEM026"), "{stderr}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn preprocess_include_failure_suppresses_sem024_sem025_and_sem026() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "missing-include.lsl",
        "#include \"missing.lsl\"\ninteger unused_global;\ninteger unused_callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { llOwnerSay((string)unused_callee(1)); } }\n",
    );
    let display = source.to_str().unwrap();

    let output = run_args(&["--preprocess", display]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("error[PRE032]"), "{stdout}");
    for code in ["SEM024", "SEM025", "SEM026"] {
        assert!(
            !stdout.contains(code),
            "unreliable unit emitted {code}: {stdout}"
        );
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn preprocess_error_directive_suppresses_sem024_sem025_and_sem026() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "error-directive.lsl",
        "#error stop\ninteger unused_global;\ninteger unused_callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { llOwnerSay((string)unused_callee(1)); } }\n",
    );
    let display = source.to_str().unwrap();

    let output = run_args(&["--preprocess", display]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("error[PRE001]"), "{stdout}");
    for code in ["SEM024", "SEM025", "SEM026"] {
        assert!(
            !stdout.contains(code),
            "unreliable unit emitted {code}: {stdout}"
        );
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn informational_preprocessor_notice_does_not_suppress_unused_analysis() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "kill-switch.lsl",
        "//fspreprocessor off\ninteger unused_global;\ndefault { state_entry() { } }\n",
    );
    let display = source.to_str().unwrap();

    let output = run_args(&["--dialect", "firestorm", display]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("info[PRE037]"), "{stdout}");
    assert!(
        stdout.contains("warning[SEM024]: variable 'unused_global' is declared but never read"),
        "{stdout}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn successful_preprocess_include_keeps_unused_analysis_enabled() {
    let directory = temporary_directory();
    let includes = directory.join("includes");
    fs::create_dir_all(&includes).unwrap();
    write_file(
        &includes,
        "usage.lsl",
        "integer read_global() { return shared_global; }\n",
    );
    const ROOT_SOURCE: &str = "integer shared_global;\ninteger root_unused;\n#include \"usage.lsl\"\ndefault { state_entry() { llOwnerSay((string)read_global()); } }\n";
    let source = write_file(&directory, "root.lsl", ROOT_SOURCE);
    let display = source.to_str().unwrap();

    let output = run_args(&["--preprocess", "-I", includes.to_str().unwrap(), display]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        expected_unused_diagnostic(display, ROOT_SOURCE, "root_unused", "SEM024"),
        "successful preprocessing must keep Phase 7 analysis enabled"
    );
    assert!(
        !stdout.contains("shared_global"),
        "the included read must keep the global used: {stdout}"
    );
    assert!(!stdout.contains("SEM025"), "{stdout}");
    assert!(!stdout.contains("SEM026"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn lexical_error_suppresses_sem024_sem025_and_sem026() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "lexical-error.lsl",
        "integer unused_global;\ninteger unused_callee(integer unused_parameter) { return 1; }\nunused_function() { }\ndefault { state_entry() { llOwnerSay((string)unused_callee(1)); \u{a7} } }\n",
    );
    let display = source.to_str().unwrap();

    let output = run_args(&[display]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("error[LEX000]"), "{stdout}");
    for code in ["SEM024", "SEM025", "SEM026"] {
        assert!(
            !stdout.contains(code),
            "unreliable unit emitted {code}: {stdout}"
        );
    }

    fs::remove_dir_all(directory).unwrap();
}

/// Render the exact human line for one AST-only unused-analysis finding.
fn expected_unused_diagnostic(path: &str, source: &str, name: &str, code: &str) -> String {
    let start = source.find(name).unwrap();
    let line = source[..start].matches('\n').count() + 1;
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let column = source[line_start..start].chars().count() + 1;
    let message = match code {
        "SEM024" => format!("variable '{name}' is declared but never read"),
        "SEM025" => format!("parameter '{name}' is never read"),
        "SEM026" => format!("function '{name}' is never called"),
        other => panic!("unexpected AST-only code: {other}"),
    };
    format!("{path}:{line}:{column}: warning[{code}]: {message}\n")
}

#[test]
fn unused_analysis_isolates_each_analysis_unit_across_files_directory_and_stdin() {
    let directory = temporary_directory();
    let tree = directory.join("tree");
    fs::create_dir_all(&tree).unwrap();

    const FIRST_SOURCE: &str = "integer shared_value;\ninteger first_callee(integer first_parameter) { return 1; }\nshared_function() { }\ndefault { state_entry() { llOwnerSay((string)first_callee(1)); } }\n";
    const SECOND_SOURCE: &str = "integer second_value;\ninteger second_callee(integer second_parameter) { return 1; }\nsecond_dead() { }\ndefault { state_entry() { llOwnerSay((string)second_callee(1)); } }\n";
    // `shared_value` is read and `shared_function` is called here. The first
    // unit declares both names without using them, so any state leak would
    // change one unit's findings.
    const NESTED_SOURCE: &str = "integer shared_value;\ninteger nested_value;\ninteger nested_callee(integer nested_parameter) { return shared_value; }\nshared_function() { }\nnested_dead() { }\ndefault { state_entry() { shared_function(); llOwnerSay((string)nested_callee(1)); } }\n";
    const STDIN_SOURCE: &str = "integer stdin_value;\ninteger stdin_callee(integer stdin_parameter) { return 1; }\nstdin_dead() { }\ndefault { state_entry() { llOwnerSay((string)stdin_callee(1)); } }\n";

    let first = write_file(&directory, "first.lsl", FIRST_SOURCE);
    let second = write_file(&directory, "second.lsl", SECOND_SOURCE);
    let nested = write_file(&tree, "nested.lsl", NESTED_SOURCE);
    let first_path = first.to_str().unwrap();
    let second_path = second.to_str().unwrap();
    let tree_path = tree.to_str().unwrap();
    let nested_path = nested.to_str().unwrap();
    let args = [first_path, second_path, tree_path, "-"];

    let first_run = run_args_with_stdin(&args, STDIN_SOURCE.as_bytes());
    let second_run = run_args_with_stdin(&args, STDIN_SOURCE.as_bytes());
    assert_eq!(first_run.status.code(), Some(0));
    assert_eq!(first_run.status.code(), second_run.status.code());
    assert_eq!(first_run.stdout, second_run.stdout);
    assert_eq!(first_run.stderr, second_run.stderr);
    assert!(first_run.stderr.is_empty(), "{first_run:?}");

    let expected = [
        expected_unused_diagnostic(first_path, FIRST_SOURCE, "shared_value", "SEM024"),
        expected_unused_diagnostic(first_path, FIRST_SOURCE, "first_parameter", "SEM025"),
        expected_unused_diagnostic(first_path, FIRST_SOURCE, "shared_function", "SEM026"),
        expected_unused_diagnostic(second_path, SECOND_SOURCE, "second_value", "SEM024"),
        expected_unused_diagnostic(second_path, SECOND_SOURCE, "second_parameter", "SEM025"),
        expected_unused_diagnostic(second_path, SECOND_SOURCE, "second_dead", "SEM026"),
        expected_unused_diagnostic(nested_path, NESTED_SOURCE, "nested_value", "SEM024"),
        expected_unused_diagnostic(nested_path, NESTED_SOURCE, "nested_parameter", "SEM025"),
        expected_unused_diagnostic(nested_path, NESTED_SOURCE, "nested_dead", "SEM026"),
        expected_unused_diagnostic("<stdin>", STDIN_SOURCE, "stdin_value", "SEM024"),
        expected_unused_diagnostic("<stdin>", STDIN_SOURCE, "stdin_parameter", "SEM025"),
        expected_unused_diagnostic("<stdin>", STDIN_SOURCE, "stdin_dead", "SEM026"),
    ]
    .concat();
    assert_eq!(String::from_utf8(first_run.stdout).unwrap(), expected);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn directory_walk_and_stdin_unit_order_is_deterministic() {
    let directory = temporary_directory();
    let tree = directory.join("tree");
    let nested = tree.join("nested");
    fs::create_dir_all(&nested).unwrap();

    const A_SOURCE: &str = "integer unused_a;\ndefault { state_entry() { } }\n";
    const M_SOURCE: &str = "integer unused_m;\ndefault { state_entry() { } }\n";
    const Z_SOURCE: &str = "integer unused_z;\ndefault { state_entry() { } }\n";
    const STDIN_SOURCE: &str = "integer unused_stdin;\ndefault { state_entry() { } }\n";

    let a = write_file(&tree, "a.lsl", A_SOURCE);
    let m = write_file(&nested, "m.lsl", M_SOURCE);
    let z = write_file(&tree, "z.lsl", Z_SOURCE);
    let tree_path = tree.to_str().unwrap();
    let args = [tree_path, "-"];

    let first_run = run_args_with_stdin(&args, STDIN_SOURCE.as_bytes());
    let second_run = run_args_with_stdin(&args, STDIN_SOURCE.as_bytes());
    assert_eq!(first_run.status.code(), Some(0));
    assert_eq!(first_run.status.code(), second_run.status.code());
    assert_eq!(first_run.stdout, second_run.stdout);
    assert_eq!(first_run.stderr, second_run.stderr);

    let expected = [
        expected_unused_diagnostic(a.to_str().unwrap(), A_SOURCE, "unused_a", "SEM024"),
        expected_unused_diagnostic(m.to_str().unwrap(), M_SOURCE, "unused_m", "SEM024"),
        expected_unused_diagnostic(z.to_str().unwrap(), Z_SOURCE, "unused_z", "SEM024"),
        expected_unused_diagnostic("<stdin>", STDIN_SOURCE, "unused_stdin", "SEM024"),
    ]
    .concat();
    assert_eq!(String::from_utf8(first_run.stdout).unwrap(), expected);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unresolved_variable_warning_can_be_escalated_or_disabled() {
    let directory = temporary_directory();
    let source = directory.join("unresolved.lsl");

    fs::write(
        &source,
        "default { state_entry() { llOwnerSay(missing_name); } }",
    )
    .unwrap();

    let escalated = neolint()
        .arg("--severity")
        .arg("SEM012=error")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(escalated.status.code(), Some(1));
    let escalated_stdout = String::from_utf8(escalated.stdout).unwrap();
    assert!(
        escalated_stdout.contains("error[SEM012]"),
        "{escalated_stdout}"
    );

    let disabled = neolint()
        .arg("--disable")
        .arg("SEM012")
        .arg(&source)
        .output()
        .unwrap();
    assert!(disabled.status.success());
    let disabled_stdout = String::from_utf8(disabled.stdout).unwrap();
    assert!(
        disabled_stdout.contains("no diagnostics"),
        "{disabled_stdout}"
    );
    assert!(!disabled_stdout.contains("SEM012"), "{disabled_stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn deprecated_builtin_warning_can_be_escalated_or_disabled() {
    let directory = temporary_directory();
    let source = directory.join("deprecated.lsl");

    fs::write(
        &source,
        r#"default { state_entry() { llSoundPreload("sound"); } }"#,
    )
    .unwrap();

    let default_output = neolint().arg(&source).output().unwrap();
    assert!(default_output.status.success());
    let default_stdout = String::from_utf8(default_output.stdout).unwrap();
    assert!(
        default_stdout.contains("warning[SEM013]"),
        "{default_stdout}"
    );

    let escalated = neolint()
        .arg("--severity")
        .arg("SEM013=error")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(escalated.status.code(), Some(1));
    let escalated_stdout = String::from_utf8(escalated.stdout).unwrap();
    assert!(
        escalated_stdout.contains("error[SEM013]"),
        "{escalated_stdout}"
    );

    let disabled = neolint()
        .arg("--disable")
        .arg("SEM013")
        .arg(&source)
        .output()
        .unwrap();
    assert!(disabled.status.success());
    let disabled_stdout = String::from_utf8(disabled.stdout).unwrap();
    assert!(
        disabled_stdout.contains("no diagnostics"),
        "{disabled_stdout}"
    );
    assert!(!disabled_stdout.contains("SEM013"), "{disabled_stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn severity_override_changes_exit_code_and_output() {
    let directory = temporary_directory();
    let source = directory.join("wrong-argument.lsl");
    let config = directory.join("neolint.conf");

    fs::write(&source, r#"default { state_entry() { llOwnerSay(42); } }"#).unwrap();
    fs::write(&config, "severity.SEM004 = warning\n").unwrap();

    let output = neolint()
        .arg("--config")
        .arg(&config)
        .arg(&source)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("warning[SEM004]"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn command_line_can_disable_a_rule() {
    let directory = temporary_directory();
    let source = directory.join("wrong-argument.lsl");

    fs::write(&source, r#"default { state_entry() { llOwnerSay(42); } }"#).unwrap();

    let output = neolint()
        .arg("--disable")
        .arg("SEM004")
        .arg(&source)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("no diagnostics"), "{stdout}");
    assert!(!stdout.contains("SEM004"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn directory_argument_checks_nested_lsl_files() {
    let directory = temporary_directory();
    let nested = directory.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join("valid.LSL"),
        "default { state_entry() { llOwnerSay(\"hello\"); } }",
    )
    .unwrap();
    fs::write(directory.join("ignore.txt"), "not an LSL file").unwrap();

    let output = neolint().arg(&directory).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("valid.LSL"), "{stdout}");
    assert!(!stdout.contains("ignore.txt"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stdin_is_supported_with_json_output() {
    let mut child = neolint()
        .arg("--format")
        .arg("json")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"default { state_entry() { llOwnerSay(\"ok\"); } }")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "[]");
}

#[test]
fn empty_stdin_is_a_syntax_error_for_missing_default_state() {
    let mut child = neolint()
        .arg("--format")
        .arg("json")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"").unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"code\":\"PAR013\""), "{stdout}");
}

#[test]
fn invalid_configuration_returns_input_error_exit_code() {
    let directory = temporary_directory();
    let source = directory.join("neolint.conf");
    fs::write(&source, "severity.SEM004 = fatal\n").unwrap();

    let output = neolint()
        .arg("--config")
        .arg(&source)
        .arg("-")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("invalid config"), "{stderr}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn checks_multiple_explicit_files_in_one_invocation() {
    let directory = temporary_directory();
    let first = directory.join("duplicate.lsl");
    let second = directory.join("wrong-type.lsl");

    fs::write(
        &first,
        "integer repeated; integer repeated; default { state_entry() {} }",
    )
    .unwrap();
    fs::write(&second, r#"default { state_entry() { llOwnerSay(123); } }"#).unwrap();

    let output = neolint()
        .arg("--format")
        .arg("json")
        .arg(&first)
        .arg(&second)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("duplicate.lsl"), "{stdout}");
    assert!(stdout.contains("wrong-type.lsl"), "{stdout}");
    assert!(stdout.contains("SEM001"), "{stdout}");
    assert!(stdout.contains("SEM004"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn documented_help_and_version_flags_succeed() {
    let help = neolint().arg("--help").output().unwrap();
    assert!(help.status.success());
    let help_stdout = String::from_utf8(help.stdout).unwrap();
    assert!(help_stdout.contains("USAGE:"), "{help_stdout}");
    assert!(help_stdout.contains("--format"), "{help_stdout}");

    let version = neolint().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("neolint {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn documented_clean_valid_script_exits_zero() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.lsl");
    let output = neolint().arg(&fixture).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("no diagnostics"), "{stdout}");
}

#[test]
fn documented_diagnostic_example_exits_one_with_expected_code() {
    let directory = temporary_directory();
    let source = directory.join("invalid.lsl");
    fs::write(&source, "default { state_entry() { llOwnerSay(42); } }").unwrap();

    let output = neolint().arg(&source).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("error[SEM004]"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn documented_json_shape_contains_all_fields() {
    let directory = temporary_directory();
    let source = directory.join("duplicate.lsl");
    fs::write(&source, "default { state_entry() { } state_entry() { } }").unwrap();

    let output = neolint()
        .arg("--format")
        .arg("json")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with('['), "{stdout}");
    assert!(stdout.trim_end().ends_with(']'), "{stdout}");
    for field in [
        "\"file\":",
        "\"code\":\"SEM020\"",
        "\"severity\":\"error\"",
        "\"kind\":\"semantic\"",
        "\"message\":",
        "\"line\":",
        "\"column\":",
        "\"end_line\":",
        "\"end_column\":",
        "\"start\":",
        "\"end\":",
    ] {
        assert!(stdout.contains(field), "missing {field}: {stdout}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn documented_config_example_disables_and_downgrades() {
    let directory = temporary_directory();
    let source = directory.join("configured.lsl");
    let config = directory.join("neolint.conf");
    fs::write(
        &source,
        "integer repeated; integer repeated; default { state_entry() { llOwnerSay(42); } }",
    )
    .unwrap();
    fs::write(
        &config,
        "# project defaults\ndisable = SEM001\nseverity.SEM004 = warning\n",
    )
    .unwrap();

    let output = neolint()
        .arg("--config")
        .arg(&config)
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("warning[SEM004]"), "{stdout}");
    assert!(!stdout.contains("SEM001"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn documented_tokens_flag_keeps_diagnostics_visible() {
    let directory = temporary_directory();
    let source = directory.join("invalid.lsl");
    fs::write(&source, "default { state_entry() { llOwnerSay(42); } }").unwrap();

    let output = neolint().arg("--tokens").arg(&source).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Identifier"), "{stdout}");
    assert!(stdout.contains("error[SEM004]"), "{stdout}");
    assert!(!stdout.contains("no diagnostics"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn documented_unknown_option_exits_input_error() {
    let output = neolint().arg("--not-a-real-option").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown option"), "{stderr}");
}

#[test]
fn documented_tokens_flag_clean_file_suppresses_no_diagnostics_line() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.lsl");
    let output = neolint().arg("--tokens").arg(&fixture).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Identifier"), "{stdout}");
    assert!(!stdout.contains("no diagnostics"), "{stdout}");
}

#[test]
fn plain_invocation_stays_unpreprocessed() {
    let directory = temporary_directory();
    let source = directory.join("plain.lsl");
    fs::write(
        &source,
        "#define X 1\ninteger x = X;\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let output = neolint().arg(&source).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("LEX000"), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn preprocess_flag_expands_macros_without_lexical_hash_errors() {
    let directory = temporary_directory();
    let source = directory.join("pre.lsl");
    fs::write(
        &source,
        "#define X 1\ninteger x = X;\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let output = neolint().arg("--preprocess").arg(&source).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("LEX000"), "{stdout}");
    assert!(!stdout.contains("PAR013"), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn firestorm_kill_switch_is_info_and_does_not_fail() {
    let directory = temporary_directory();
    let source = directory.join("off.lsl");
    fs::write(
        &source,
        "//fspreprocessor off\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let output = neolint()
        .arg("--dialect")
        .arg("firestorm")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("info[PRE037]"), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn include_directory_flags_and_json_shape_work() {
    let directory = temporary_directory();
    let includes = directory.join("includes");
    fs::create_dir_all(&includes).unwrap();
    fs::write(includes.join("shared.lsl"), "integer shared = 1;\n").unwrap();
    let source = directory.join("root.lsl");
    fs::write(
        &source,
        "#include <shared.lsl>\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let output = neolint()
        .arg("--preprocess")
        .arg("-I")
        .arg(&includes)
        .arg("--include-dir")
        .arg(&includes)
        .arg("--format")
        .arg("json")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with('['), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn json_preprocessor_diagnostics_expose_origin_and_related_locations() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "macro-conflict.lsl",
        "#define ITEM 1\n#define ITEM 2\ndefault { state_entry() { } }\n",
    );
    let display = source.to_str().unwrap();

    let output = run_args(&["--preprocess", "--format", "json", display]);
    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(json_array_is_structurally_valid(&stdout), "{stdout}");
    assert!(stdout.contains("\"code\":\"PRE005\""), "{stdout}");
    // v1 keeps the existing kind value; origin disambiguates the PRE stage.
    assert!(stdout.contains("\"kind\":\"syntax\""), "{stdout}");
    assert!(stdout.contains("\"origin\":\"preprocessor\""), "{stdout}");

    let related = stdout
        .split_once("\"related\":[")
        .and_then(|(_, tail)| tail.split_once(']').map(|(array, _)| array))
        .expect("PRE005 must expose the previous definition as a related location");
    assert!(related.contains("\"file\":\""), "{related}");
    assert!(
        related.contains("\"label\":\"previous macro definition\""),
        "{related}"
    );
    assert!(related.contains("\"line\":1"), "{related}");
    for field in ["column", "end_line", "end_column", "start", "end"] {
        assert!(related.contains(&format!("\"{field}\":")), "{related}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn json_non_preprocessor_diagnostics_keep_the_existing_v1_shape() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "semantic-error.lsl",
        "default { state_entry() { llOwnerSay(42); } }\n",
    );
    let display = source.to_str().unwrap();
    let output = run_args(&["--format", "json", display]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(json_array_is_structurally_valid(&stdout), "{stdout}");
    assert!(stdout.contains("\"code\":\"SEM004\""), "{stdout}");
    assert!(stdout.contains("\"kind\":\"semantic\""), "{stdout}");
    assert!(!stdout.contains("\"origin\""), "{stdout}");
    assert!(!stdout.contains("\"related\""), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn config_preprocess_dialect_and_include_dir_are_supported() {
    let directory = temporary_directory();
    let includes = directory.join("includes");
    fs::create_dir_all(&includes).unwrap();
    fs::write(includes.join("shared.lsl"), "integer shared = 1;\n").unwrap();
    let source = directory.join("root.lsl");
    fs::write(
        &source,
        "#include \"shared.lsl\"\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let config = directory.join("neolint.conf");
    fs::write(
        &config,
        format!(
            "preprocess = true\ndialect = lsl\ninclude_dir = {}\n",
            includes.display()
        ),
    )
    .unwrap();
    let output = neolint()
        .arg("--config")
        .arg(&config)
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_preprocess_cli_combinations_exit_two() {
    for arguments in [
        vec!["--dialect", "bogus"],
        vec!["--include-dir"],
        vec!["-I"],
    ] {
        let output = neolint().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{:?}", output);
    }
    let directory = temporary_directory();
    let source = directory.join("a.lsl");
    fs::write(&source, "default { state_entry() { } }\n").unwrap();
    let output = neolint()
        .arg("-I")
        .arg(&directory)
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("include directories require"));
    let output = neolint()
        .arg("--preprocess")
        .arg("-I")
        .arg(directory.join("does-not-exist"))
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stdin_include_requires_explicit_include_directory() {
    let directory = temporary_directory();
    fs::write(directory.join("shared.lsl"), "integer shared = 1;\n").unwrap();
    let mut child = neolint()
        .arg("--preprocess")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"#include \"shared.lsl\"\ndefault { state_entry() { } }\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stdout).unwrap().contains("PRE032"));

    let mut child = neolint()
        .arg("--preprocess")
        .arg("-I")
        .arg(&directory)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"#include \"shared.lsl\"\ndefault { state_entry() { } }\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cli_include_dirs_override_config_and_preprocess_flag_overrides_false() {
    let directory = temporary_directory();
    let right = directory.join("right");
    let wrong = directory.join("wrong");
    fs::create_dir_all(&right).unwrap();
    fs::create_dir_all(&wrong).unwrap();
    fs::write(right.join("shared.lsl"), "integer shared = 1;\n").unwrap();
    let source = directory.join("root.lsl");
    fs::write(
        &source,
        "#include \"shared.lsl\"\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let config = directory.join("neolint.conf");
    fs::write(
        &config,
        format!("preprocess = false\ninclude_dir = {}\n", wrong.display()),
    )
    .unwrap();
    let output = neolint()
        .arg("--config")
        .arg(&config)
        .arg("--preprocess")
        .arg("-I")
        .arg(&right)
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tokens_flag_works_with_preprocessing() {
    let directory = temporary_directory();
    let source = directory.join("tokens.lsl");
    fs::write(
        &source,
        "#define X 1\ninteger x = X;\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let output = neolint()
        .arg("--preprocess")
        .arg("--tokens")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Identifier"), "{stdout}");
    assert!(!stdout.contains("no diagnostics"), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn included_file_diagnostics_use_the_included_path_and_keep_json_shape() {
    let directory = temporary_directory();
    fs::write(directory.join("inc.lsl"), "bad !!!\n").unwrap();
    let source = directory.join("root.lsl");
    fs::write(
        &source,
        "#include \"inc.lsl\"\ndefault { state_entry() { } }\n",
    )
    .unwrap();
    let output = neolint()
        .arg("--preprocess")
        .arg("--format")
        .arg("json")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("inc.lsl"), "{stdout}");
    assert!(stdout.contains("\"code\":\"PAR001\""), "{stdout}");
    assert!(!stdout.contains("\"notes\""), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

fn json_is_structurally_valid(text: &str, opening: char, closing: char) -> bool {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            } else if ch.is_control() {
                return false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '[' => stack.push(']'),
            '{' => stack.push('}'),
            ']' | '}' if stack.pop() != Some(ch) => return false,
            ']' | '}' => {}
            _ => {}
        }
    }
    !in_string
        && stack.is_empty()
        && text.trim_start().starts_with(opening)
        && text.trim_end().ends_with(closing)
}

fn json_array_is_structurally_valid(text: &str) -> bool {
    json_is_structurally_valid(text, '[', ']')
}

fn json_object_is_structurally_valid(text: &str) -> bool {
    json_is_structurally_valid(text, '{', '}')
}

/// Rules whose human compare line reports a non-zero or unknown delta.
fn divergent_rules(stderr: &str) -> std::collections::BTreeSet<String> {
    stderr
        .lines()
        .filter_map(|line| {
            let (_, after_rule) = line.split_once(" rule ")?;
            let (code, fields) = after_rule.split_once(':')?;
            if !code.starts_with("SEM") {
                return None;
            }
            let field_value = |name: &str| {
                fields
                    .split_whitespace()
                    .find_map(|field| field.strip_prefix(name))
            };
            let missing = field_value("missing=")?;
            let extra = field_value("extra=")?;
            (missing != "0" || extra != "0").then(|| code.to_owned())
        })
        .collect()
}

/// Assert the only cross-engine semantic divergence is the documented SEM002.
fn assert_only_sem002_diverges(stderr: &str) {
    assert_eq!(
        divergent_rules(stderr),
        ["SEM002".to_owned()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        "expected exactly SEM002 to diverge: {stderr}"
    );
}

#[test]
fn global_cli_diagnostic_cap_truncates_all_later_files_and_keeps_one_pre039() {
    let directory = temporary_directory();
    let first = directory.join("a.lsl");
    let second = directory.join("b.lsl");
    let mut source = String::new();
    for _ in 0..10_100 {
        source.push_str("integer duplicate;\n");
    }
    fs::write(&first, &source).unwrap();
    fs::write(&second, &source).unwrap();
    let output = neolint()
        .arg("--preprocess")
        .arg("--format")
        .arg("json")
        .arg(&first)
        .arg(&second)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        json_array_is_structurally_valid(&stdout),
        "invalid JSON output"
    );
    let diagnostic_count = stdout.matches("\"code\":\"").count();
    assert!(
        diagnostic_count <= 10_000,
        "diagnostics: {diagnostic_count}"
    );
    assert_eq!(stdout.matches("PRE039").count(), 1);
    assert!(
        !stdout.contains("b.lsl"),
        "later file retained diagnostics beyond the budget"
    );
    fs::remove_dir_all(directory).unwrap();
}

const CLEAN_SOURCE: &str = "default { state_entry() { llSay(0, \"hi\"); } }\n";
const ERROR_SOURCE: &str = "default { state_entry() { llOwnerSay(42); } }\n";
const DISC_SOURCE: &str = "default { llSay() { } }\n";
// Deliberately corrupted input: an unbalanced `while (i < n(` condition, a
// function that can reach its end without returning, and an event named
// `llSay`. Its only cross-engine divergence is the event-header SEM002 delta;
// SEM011 and the syntax findings agree. This is not a parser-recovery artifact
// and not a parity defect.
const MISMATCH_SOURCE: &str = "integer i;\ninteger n;\ninteger f() {\n}\ndefault {\n    llSay() {\n        while (i < n( {\n    }\n}\n";
const FIRESTORM_SIMPLE: &str =
    "#define USE_SWITCHES\ndefault { state_entry() { llOwnerSay(\"ok\"); } }\n";
const FIRESTORM_SWITCH: &str = "#define USE_SWITCHES\ndefault {\n    state_entry() {\n        integer value = 1;\n        switch (value) {\n            case 1:\n                llOwnerSay(\"one\");\n                break;\n            default:\n                break;\n        }\n    }\n}\n";
const FIRESTORM_SWITCH_UNDECLARED: &str = "#define USE_SWITCHES\ndefault {\n    state_entry() {\n        integer value = 1;\n        switch (value) {\n            case 1:\n                counter = 1;\n                break;\n            default:\n                break;\n        }\n    }\n}\n";

fn write_file(directory: &std::path::Path, name: &str, contents: &str) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn engine_defaults_to_ast_and_old_config_without_engine_uses_ast() {
    let directory = temporary_directory();
    let source = write_file(&directory, "event.lsl", DISC_SOURCE);
    let config = write_file(
        &directory,
        "old.conf",
        "disable = SEM001\nseverity.SEM004 = warning\n",
    );
    let source = source.to_str().unwrap();
    let config = config.to_str().unwrap();

    let default = run_args(&[source]);
    let explicit_ast = run_args(&["--engine", "ast", source]);
    let configured = run_args(&["--config", config, source]);
    let explicit_legacy = run_args(&["--engine", "legacy", source]);

    // The default and an old config without `engine` both select AST.
    for output in [&default, &explicit_ast, &configured] {
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("SEM002"), "{stdout}");
    }
    assert_eq!(default.stdout, explicit_ast.stdout);
    assert_eq!(default.stderr, explicit_ast.stderr);
    assert_eq!(default.status.code(), explicit_ast.status.code());
    assert_eq!(default.stdout, configured.stdout);
    assert_eq!(default.stderr, configured.stderr);
    assert_eq!(default.status.code(), configured.status.code());

    // Explicit Legacy keeps the historical SEM002 / exit-1 behavior.
    assert_eq!(explicit_legacy.status.code(), Some(1));
    let legacy_stdout = String::from_utf8(explicit_legacy.stdout).unwrap();
    assert!(legacy_stdout.contains("SEM002"), "{legacy_stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn engine_flag_accepts_separate_and_equals_forms_for_each_engine() {
    let directory = temporary_directory();
    let source = write_file(&directory, "clean.lsl", CLEAN_SOURCE);
    let source = source.to_str().unwrap();

    for arguments in [
        vec!["--engine", "legacy"],
        vec!["--engine=legacy"],
        vec!["--engine", "ast"],
        vec!["--engine=ast"],
        vec!["--engine", "compare"],
        vec!["--engine=compare"],
    ] {
        let mut args = arguments.clone();
        args.push(source);
        let output = run_args(&args);
        assert_eq!(output.status.code(), Some(0), "{arguments:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        if arguments
            .iter()
            .any(|argument| argument.contains("compare"))
        {
            assert!(stderr.contains("compare unit 0"), "{arguments:?}: {stderr}");
            assert!(stderr.contains("matched=true"), "{arguments:?}: {stderr}");
        } else {
            assert!(stderr.is_empty(), "{arguments:?}: {stderr}");
        }
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn config_engine_selects_each_engine_and_cli_overrides() {
    let directory = temporary_directory();
    let source = write_file(&directory, "event.lsl", DISC_SOURCE);
    let legacy_config = write_file(&directory, "legacy.conf", "engine = legacy\n");
    let ast_config = write_file(&directory, "ast.conf", "engine = ast\n");
    let compare_config = write_file(&directory, "compare.conf", "engine = compare\n");
    let source = source.to_str().unwrap();
    let legacy_config = legacy_config.to_str().unwrap();
    let ast_config = ast_config.to_str().unwrap();
    let compare_config = compare_config.to_str().unwrap();

    let configured_legacy = run_args(&["--config", legacy_config, source]);
    assert_eq!(configured_legacy.status.code(), Some(1));
    let stdout = String::from_utf8(configured_legacy.stdout).unwrap();
    assert!(stdout.contains("SEM002"), "{stdout}");
    assert!(configured_legacy.stderr.is_empty());

    let configured_ast = run_args(&["--config", ast_config, source]);
    assert_eq!(configured_ast.status.code(), Some(0));
    let stdout = String::from_utf8(configured_ast.stdout).unwrap();
    assert!(!stdout.contains("SEM002"), "{stdout}");
    assert!(configured_ast.stderr.is_empty());

    let overridden = run_args(&["--config", ast_config, "--engine", "compare", source]);
    assert_eq!(overridden.status.code(), Some(3));
    let stdout = String::from_utf8(overridden.stdout).unwrap();
    let stderr = String::from_utf8(overridden.stderr).unwrap();
    assert!(stdout.contains("SEM002"), "{stdout}");
    assert!(stderr.contains("compare unit 0"), "{stderr}");
    assert!(stderr.contains("matched=false"), "{stderr}");

    let configured_compare = run_args(&["--config", compare_config, source]);
    assert_eq!(configured_compare.status.code(), Some(3));
    assert!(String::from_utf8(configured_compare.stderr)
        .unwrap()
        .contains("compare unit 0"));

    let ast_over_compare = run_args(&["--config", compare_config, "--engine", "ast", source]);
    assert_eq!(ast_over_compare.status.code(), Some(0));
    let stdout = String::from_utf8(ast_over_compare.stdout).unwrap();
    assert!(!stdout.contains("SEM002"), "{stdout}");
    assert!(ast_over_compare.stderr.is_empty());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_missing_and_empty_engine_values_exit_two() {
    let directory = temporary_directory();
    let source = write_file(&directory, "clean.lsl", CLEAN_SOURCE);
    let source = source.to_str().unwrap();

    let invalid = run_args(&["--engine", "bogus", source]);
    assert_eq!(invalid.status.code(), Some(2));
    let stderr = String::from_utf8(invalid.stderr).unwrap();
    assert!(stderr.contains("invalid engine 'bogus'"), "{stderr}");
    assert!(
        stderr.contains("expected legacy, ast or compare"),
        "{stderr}"
    );

    let uppercase = run_args(&["--engine", "AST", source]);
    assert_eq!(uppercase.status.code(), Some(2));
    assert!(String::from_utf8(uppercase.stderr)
        .unwrap()
        .contains("invalid engine 'AST'"));

    let missing = run_args(&["--engine"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8(missing.stderr)
        .unwrap()
        .contains("--engine requires 'legacy', 'ast' or 'compare'"));

    let empty = run_args(&["--engine=", source]);
    assert_eq!(empty.status.code(), Some(2));
    assert!(String::from_utf8(empty.stderr)
        .unwrap()
        .contains("--engine requires 'legacy', 'ast' or 'compare'"));

    let config = write_file(&directory, "bad.conf", "engine = faster\n");
    let bad_config = run_args(&["--config", config.to_str().unwrap(), source]);
    assert_eq!(bad_config.status.code(), Some(2));
    let stderr = String::from_utf8(bad_config.stderr).unwrap();
    assert!(stderr.contains("invalid config"), "{stderr}");
    assert!(
        stderr.contains("line 1: invalid engine value 'faster'"),
        "{stderr}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn ast_and_compare_work_without_preprocessing_via_identity_preparation() {
    let directory = temporary_directory();
    let clean = write_file(&directory, "clean.lsl", CLEAN_SOURCE);
    let macro_source = write_file(
        &directory,
        "macro.lsl",
        "#define X 1\ninteger x = X;\ndefault { state_entry() { } }\n",
    );

    let ast_clean = run_args(&["--engine", "ast", clean.to_str().unwrap()]);
    assert_eq!(ast_clean.status.code(), Some(0));

    let compare_clean = run_args(&["--engine", "compare", clean.to_str().unwrap()]);
    assert_eq!(compare_clean.status.code(), Some(0));
    let stderr = String::from_utf8(compare_clean.stderr).unwrap();
    assert!(stderr.contains("matched=true"), "{stderr}");

    let ast_macro = run_args(&["--engine", "ast", macro_source.to_str().unwrap()]);
    assert_eq!(ast_macro.status.code(), Some(1));
    let ast_stdout = String::from_utf8(ast_macro.stdout.clone()).unwrap();
    assert!(ast_stdout.contains("LEX000"), "{ast_stdout}");

    // No flag is now AST, so the Legacy-only `define` token does not appear.
    let default_macro = run_args(&[macro_source.to_str().unwrap()]);
    assert_eq!(default_macro.status.code(), Some(1));
    assert_eq!(default_macro.stdout, ast_macro.stdout);
    let default_stdout = String::from_utf8(default_macro.stdout).unwrap();
    assert!(default_stdout.contains("LEX000"), "{default_stdout}");
    assert!(
        !default_stdout.contains("variable 'define'"),
        "{default_stdout}"
    );

    let legacy_macro = run_args(&["--engine", "legacy", macro_source.to_str().unwrap()]);
    assert_eq!(legacy_macro.status.code(), Some(1));
    let legacy_stdout = String::from_utf8(legacy_macro.stdout).unwrap();
    assert!(legacy_stdout.contains("LEX000"), "{legacy_stdout}");
    assert!(
        legacy_stdout.contains("variable 'define'"),
        "{legacy_stdout}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn engines_support_preprocess_includes_and_stdin() {
    let directory = temporary_directory();
    let includes = directory.join("includes");
    fs::create_dir_all(&includes).unwrap();
    write_file(&includes, "defs.lsl", "#define GREETING \"hi\"\n");
    let source = write_file(
        &directory,
        "root.lsl",
        "#include \"defs.lsl\"\ndefault { state_entry() { llOwnerSay(GREETING); } }\n",
    );

    for engine in ["legacy", "ast", "compare"] {
        let output = run_args(&[
            "--engine",
            engine,
            "--preprocess",
            "-I",
            includes.to_str().unwrap(),
            source.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(0), "{engine}: {output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        if engine == "compare" {
            assert!(stderr.contains("matched=true"), "{stderr}");
        } else {
            assert!(stderr.is_empty(), "{engine}: {stderr}");
        }
    }

    for engine in ["legacy", "ast", "compare"] {
        let output = run_args_with_stdin(&["--engine", engine, "-"], ERROR_SOURCE.as_bytes());
        assert_eq!(output.status.code(), Some(1), "{engine}: {output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("error[SEM004]"), "{engine}: {stdout}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        if engine == "compare" {
            assert!(stderr.contains("<stdin>"), "{stderr}");
            assert!(stderr.contains("matched=true"), "{stderr}");
        } else {
            assert!(stderr.is_empty(), "{engine}: {stderr}");
        }
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn engines_support_firestorm_dialect() {
    let directory = temporary_directory();
    let simple = write_file(&directory, "simple.lsl", FIRESTORM_SIMPLE);
    let switch = write_file(&directory, "switch.lsl", FIRESTORM_SWITCH);

    for engine in ["legacy", "ast", "compare"] {
        let output = run_args(&[
            "--engine",
            engine,
            "--dialect",
            "firestorm",
            simple.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(0), "{engine}: {output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        if engine == "compare" {
            assert!(stderr.contains("matched=true"), "{stderr}");
        } else {
            assert!(stderr.is_empty(), "{engine}: {stderr}");
        }
    }

    let ast_switch = run_args(&[
        "--engine",
        "ast",
        "--dialect",
        "firestorm",
        switch.to_str().unwrap(),
    ]);
    assert_eq!(ast_switch.status.code(), Some(0));
    let ast_stdout = String::from_utf8(ast_switch.stdout.clone()).unwrap();
    assert!(ast_stdout.contains("no diagnostics"), "{ast_stdout}");

    // With no engine flag the default AST engine accepts the switch too.
    let default_switch = run_args(&["--dialect", "firestorm", switch.to_str().unwrap()]);
    assert_eq!(default_switch.status.code(), Some(0));
    assert_eq!(default_switch.stdout, ast_switch.stdout);
    assert!(default_switch.stderr.is_empty());

    let legacy_switch = run_args(&[
        "--engine",
        "legacy",
        "--dialect",
        "firestorm",
        switch.to_str().unwrap(),
    ]);
    assert_eq!(legacy_switch.status.code(), Some(1));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn compare_report_uses_stderr_and_json_stdout_keeps_old_schema() {
    let directory = temporary_directory();
    let source = write_file(&directory, "mismatch.lsl", MISMATCH_SOURCE);

    let output = run_args(&[
        "--engine",
        "compare",
        "--format",
        "json",
        source.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(3));

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(json_array_is_structurally_valid(&stdout), "{stdout}");
    assert!(!stdout.contains("\"engine\""), "{stdout}");
    for field in [
        "\"file\":",
        "\"code\":\"SEM002\"",
        "\"severity\":",
        "\"kind\":",
        "\"message\":",
        "\"line\":",
        "\"column\":",
        "\"end_line\":",
        "\"end_column\":",
        "\"start\":",
        "\"end\":",
    ] {
        assert!(stdout.contains(field), "missing {field}: {stdout}");
    }

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(json_object_is_structurally_valid(&stderr), "{stderr}");
    for field in [
        "\"engine\":\"compare\"",
        "\"units\":[",
        "\"unit\":0",
        "\"path\":",
        "\"matched\":false",
        "\"comparison_complete\":true",
        "\"details_truncated\":false",
        "\"legacy_count\":",
        "\"ast_count\":",
        "\"by_rule\":[",
        "\"missing_total\":",
        "\"extra_total\":",
        "\"missing_from_ast\":[",
        "\"extra_in_ast\":[",
        "\"message\":",
        "\"start\":",
        "\"end_line\":",
        "\"end_column\":",
    ] {
        assert!(stderr.contains(field), "missing {field}: {stderr}");
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn compare_over_budget_reports_incomplete_unit_and_exits_three() {
    let directory = temporary_directory();
    // Each unresolved assignment emits one SEM012 in both engines. A duplicate
    // global flood also works but takes ~85s in the debug build; SEM012 is the
    // same comparator path and keeps the CLI regression test reasonably fast.
    let statements = MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1;
    let mut source = String::from("default { state_entry() {\n");
    for _ in 0..statements {
        source.push_str("x = 1;\n");
    }
    source.push_str("} }\n");
    let source = write_file(&directory, "over-budget.lsl", &source);

    let output = run_args(&[
        "--engine",
        "compare",
        "--format",
        "json",
        source.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(3));

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        json_array_is_structurally_valid(&stdout),
        "stdout lost the Legacy JSON diagnostics array: {stdout:.200}"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let expected = MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1;
    assert!(stderr.contains("\"comparison_complete\":false"), "{stderr}");
    assert!(stderr.contains("\"matched\":false"), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "\"code\":\"SEM012\",\"legacy_count\":{expected},\"ast_count\":{expected},\"missing_total\":null,\"extra_total\":null"
        )),
        "{stderr}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn compare_exit_codes_follow_mismatch_and_failure_precedence() {
    let directory = temporary_directory();
    let clean = write_file(&directory, "clean.lsl", CLEAN_SOURCE);
    let error = write_file(&directory, "error.lsl", ERROR_SOURCE);
    let mismatch = write_file(&directory, "mismatch.lsl", MISMATCH_SOURCE);

    let matched_clean = run_args(&["--engine", "compare", clean.to_str().unwrap()]);
    assert_eq!(matched_clean.status.code(), Some(0));

    let matched_error = run_args(&["--engine", "compare", error.to_str().unwrap()]);
    assert_eq!(matched_error.status.code(), Some(1));
    let stdout = String::from_utf8(matched_error.stdout.clone()).unwrap();
    assert!(stdout.contains("error[SEM004]"), "{stdout}");
    assert!(String::from_utf8(matched_error.stderr)
        .unwrap()
        .contains("matched=true"));

    let downgraded = run_args(&[
        "--engine",
        "compare",
        "--severity",
        "SEM004=warning",
        error.to_str().unwrap(),
    ]);
    assert_eq!(downgraded.status.code(), Some(0));

    let mismatched = run_args(&["--engine", "compare", mismatch.to_str().unwrap()]);
    assert_eq!(mismatched.status.code(), Some(3));
    let stderr = String::from_utf8(mismatched.stderr).unwrap();
    assert!(stderr.contains("matched=false"), "{stderr}");
    assert!(
        stderr.contains("rule SEM002: legacy=1 ast=0 missing=1 extra=0"),
        "{stderr}"
    );
    assert_only_sem002_diverges(&stderr);
    assert!(
        stderr.contains("missing SEM002 error semantic message="),
        "{stderr}"
    );
    assert!(stderr.contains("start="), "{stderr}");
    assert!(stderr.contains("end_line="), "{stderr}");
    assert!(stderr.contains("complete=true truncated=false"), "{stderr}");

    let missing_input = run_args(&["--engine", "compare", "does-not-exist.lsl"]);
    assert_eq!(missing_input.status.code(), Some(2));

    let bad_config = write_file(&directory, "bad.conf", "engine = faster\n");
    let config_failure = run_args(&[
        "--engine",
        "compare",
        "--config",
        bad_config.to_str().unwrap(),
        clean.to_str().unwrap(),
    ]);
    assert_eq!(config_failure.status.code(), Some(2));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn compare_report_has_one_section_per_unit_in_input_order() {
    let directory = temporary_directory();
    let first = write_file(&directory, "a.lsl", CLEAN_SOURCE);
    let second = write_file(&directory, "b.lsl", CLEAN_SOURCE);
    let tree = directory.join("tree");
    fs::create_dir_all(&tree).unwrap();
    write_file(&tree, "z.lsl", CLEAN_SOURCE);
    write_file(&tree, "a.lsl", CLEAN_SOURCE);

    let multi = run_args(&[
        "--engine",
        "compare",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert_eq!(multi.status.code(), Some(0));
    let stderr = String::from_utf8(multi.stderr).unwrap();
    assert_eq!(stderr.matches("neolint: compare unit 0 ").count(), 1);
    assert_eq!(stderr.matches("neolint: compare unit 1 ").count(), 1);
    let first_label = format!("compare unit 0 {}", first.display());
    let second_label = format!("compare unit 1 {}", second.display());
    let first_position = stderr.find(&first_label).unwrap();
    let second_position = stderr.find(&second_label).unwrap();
    assert!(first_position < second_position, "{stderr}");

    let directory_run = run_args(&["--engine", "compare", tree.to_str().unwrap()]);
    assert_eq!(directory_run.status.code(), Some(0));
    let stderr = String::from_utf8(directory_run.stderr).unwrap();
    assert_eq!(stderr.matches("neolint: compare unit 0 ").count(), 1);
    assert_eq!(stderr.matches("neolint: compare unit 1 ").count(), 1);
    let sorted_first = stderr
        .find(&format!("compare unit 0 {}", tree.join("a.lsl").display()))
        .unwrap();
    let sorted_second = stderr
        .find(&format!("compare unit 1 {}", tree.join("z.lsl").display()))
        .unwrap();
    assert!(sorted_first < sorted_second, "{stderr}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tokens_are_identical_across_engines_and_json_combination_is_rejected() {
    let directory = temporary_directory();
    let clean = write_file(&directory, "clean.lsl", CLEAN_SOURCE);
    let preprocessed = write_file(
        &directory,
        "pre.lsl",
        "#define X 1\ninteger x = X;\ndefault { state_entry() { } }\n",
    );

    let default = run_args(&["--tokens", clean.to_str().unwrap()]);
    let legacy = run_args(&["--engine", "legacy", "--tokens", clean.to_str().unwrap()]);
    let ast = run_args(&["--engine", "ast", "--tokens", clean.to_str().unwrap()]);
    let compare = run_args(&["--engine", "compare", "--tokens", clean.to_str().unwrap()]);
    for output in [&default, &legacy, &ast, &compare] {
        assert_eq!(output.status.code(), Some(0));
    }
    assert_eq!(default.stdout, ast.stdout);
    assert_eq!(legacy.stdout, ast.stdout);
    assert_eq!(legacy.stdout, compare.stdout);
    assert!(default.stderr.is_empty());
    assert!(legacy.stderr.is_empty());
    assert!(ast.stderr.is_empty());
    assert!(String::from_utf8(compare.stderr)
        .unwrap()
        .contains("compare unit 0"));

    let legacy_pre = run_args(&[
        "--engine",
        "legacy",
        "--preprocess",
        "--tokens",
        preprocessed.to_str().unwrap(),
    ]);
    let default_pre = run_args(&["--preprocess", "--tokens", preprocessed.to_str().unwrap()]);
    let ast_pre = run_args(&[
        "--engine",
        "ast",
        "--preprocess",
        "--tokens",
        preprocessed.to_str().unwrap(),
    ]);
    let compare_pre = run_args(&[
        "--engine",
        "compare",
        "--preprocess",
        "--tokens",
        preprocessed.to_str().unwrap(),
    ]);
    for output in [&legacy_pre, &default_pre, &ast_pre, &compare_pre] {
        assert_eq!(output.status.code(), Some(0));
    }
    let sem024 = format!(
        "{}:2:9: warning[SEM024]: variable 'x' is declared but never read\n",
        preprocessed.display()
    );
    let legacy_pre_stdout = String::from_utf8(legacy_pre.stdout).unwrap();
    let default_pre_stdout = String::from_utf8(default_pre.stdout).unwrap();
    let ast_pre_stdout = String::from_utf8(ast_pre.stdout).unwrap();
    let compare_pre_stdout = String::from_utf8(compare_pre.stdout).unwrap();
    assert_eq!(default_pre_stdout, ast_pre_stdout);
    assert_eq!(
        legacy_pre_stdout, compare_pre_stdout,
        "Legacy and Compare must display identical tokens"
    );
    assert_eq!(
        ast_pre_stdout,
        format!("{legacy_pre_stdout}{sem024}"),
        "AST must keep the exact Legacy token lines and append only SEM024"
    );

    for engine in ["legacy", "ast", "compare"] {
        let output = run_args(&[
            "--engine",
            engine,
            "--tokens",
            "--format",
            "json",
            clean.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(2), "{engine}");
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("--tokens cannot be combined with --format json"));
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn compare_report_is_computed_before_config_filtering() {
    let directory = temporary_directory();
    let source = write_file(&directory, "event.lsl", DISC_SOURCE);
    let source = source.to_str().unwrap();

    let disabled = run_args(&["--engine", "compare", "--disable", "SEM002", source]);
    assert_eq!(disabled.status.code(), Some(3));
    let stdout = String::from_utf8(disabled.stdout).unwrap();
    assert!(!stdout.contains("SEM002"), "{stdout}");
    let stderr = String::from_utf8(disabled.stderr).unwrap();
    assert!(
        stderr.contains("rule SEM002: legacy=1 ast=0 missing=1 extra=0"),
        "{stderr}"
    );
    assert_only_sem002_diverges(&stderr);
    assert!(stderr.contains("missing SEM002 error semantic"), "{stderr}");

    let downgraded = run_args(&[
        "--engine",
        "compare",
        "--severity",
        "SEM002=warning",
        source,
    ]);
    assert_eq!(downgraded.status.code(), Some(3));
    let stdout = String::from_utf8(downgraded.stdout).unwrap();
    assert!(stdout.contains("warning[SEM002]"), "{stdout}");
    let stderr = String::from_utf8(downgraded.stderr).unwrap();
    assert_only_sem002_diverges(&stderr);
    assert!(stderr.contains("missing SEM002 error semantic"), "{stderr}");
    assert!(!stderr.contains("missing SEM002 warning"), "{stderr}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invocation_cap_applies_to_ast_and_compare() {
    let directory = temporary_directory();
    let first = write_file(&directory, "a.lsl", &"integer duplicate;\n".repeat(10_100));
    let second = write_file(&directory, "b.lsl", &"integer duplicate;\n".repeat(10_100));

    for engine in ["ast", "compare"] {
        let output = run_args(&[
            "--engine",
            engine,
            "--format",
            "json",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(1), "{engine}: {output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            json_array_is_structurally_valid(&stdout),
            "{engine}: {stdout}"
        );
        let diagnostic_count = stdout.matches("\"code\":\"").count();
        assert!(
            diagnostic_count <= 10_000,
            "{engine}: diagnostics: {diagnostic_count}"
        );
        assert_eq!(stdout.matches("PRE039").count(), 1, "{engine}");
        assert!(
            !stdout.contains("b.lsl"),
            "{engine}: later file retained diagnostics beyond the budget"
        );
        if engine == "compare" {
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.contains("\"engine\":\"compare\""), "{stderr}");
            assert!(stderr.contains("\"unit\":0"), "{stderr}");
        }
    }

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn help_documents_engine_and_compare_exit_code() {
    let output = run_args(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "--engine",
        "ast (default)",
        "legacy",
        "compare",
        "engine = ast",
        "0  No error-severity diagnostics",
        "1  At least one error-severity diagnostic",
        "2  Input, configuration or command-line error",
        "3  Compare mode: at least one unit's semantic comparison is unmatched",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
    assert!(
        !stdout.contains("legacy (default)"),
        "stale default text: {stdout}"
    );
}

#[test]
fn cli_engine_override_to_legacy_restores_legacy_output() {
    let directory = temporary_directory();
    let source = write_file(&directory, "event.lsl", DISC_SOURCE);
    let source = source.to_str().unwrap();
    let explicit_legacy = run_args(&["--engine", "legacy", source]);
    assert_eq!(explicit_legacy.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&explicit_legacy.stdout).contains("SEM002"));

    for configured in ["ast", "compare"] {
        let config = write_file(
            &directory,
            &format!("{configured}.conf"),
            &format!("engine = {configured}\n"),
        );
        let overridden = run_args(&[
            "--config",
            config.to_str().unwrap(),
            "--engine",
            "legacy",
            source,
        ]);
        assert_eq!(overridden.status.code(), Some(1), "{configured}");
        assert_eq!(overridden.stdout, explicit_legacy.stdout, "{configured}");
        assert_eq!(overridden.stderr, explicit_legacy.stderr, "{configured}");
        assert!(overridden.stderr.is_empty(), "{configured}");
    }

    // Qualified default: `engine = legacy` in config and no CLI flag => Legacy.
    let legacy_config = write_file(&directory, "legacy.conf", "engine = legacy\n");
    let configured_legacy = run_args(&["--config", legacy_config.to_str().unwrap(), source]);
    assert_eq!(configured_legacy.status.code(), Some(1));
    assert_eq!(configured_legacy.stdout, explicit_legacy.stdout);
    assert_eq!(configured_legacy.stderr, explicit_legacy.stderr);

    // Reverse override: config Legacy + CLI AST => AST.
    let ast_override = run_args(&[
        "--config",
        legacy_config.to_str().unwrap(),
        "--engine",
        "ast",
        source,
    ]);
    assert_eq!(ast_override.status.code(), Some(0));
    let ast_stdout = String::from_utf8(ast_override.stdout).unwrap();
    assert!(!ast_stdout.contains("SEM002"), "{ast_stdout}");
    assert!(ast_override.stderr.is_empty());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stdin_source_label_follows_engine_selection() {
    let directory = temporary_directory();
    let legacy_config = write_file(&directory, "legacy.conf", "engine = legacy\n");
    let ast_config = write_file(&directory, "ast.conf", "engine = ast\n");

    let default_human = run_args_with_stdin(&["-"], ERROR_SOURCE.as_bytes());
    assert_eq!(default_human.status.code(), Some(1));
    let stdout = String::from_utf8(default_human.stdout).unwrap();
    assert!(stdout.contains("<stdin>:1:"), "{stdout}");
    assert!(!stdout.contains("-:1:"), "{stdout}");

    let legacy_human = run_args_with_stdin(&["--engine", "legacy", "-"], ERROR_SOURCE.as_bytes());
    assert_eq!(legacy_human.status.code(), Some(1));
    let stdout = String::from_utf8(legacy_human.stdout).unwrap();
    assert!(stdout.contains("-:1:"), "{stdout}");
    assert!(!stdout.contains("<stdin>"), "{stdout}");

    let configured_legacy = run_args_with_stdin(
        &["--config", legacy_config.to_str().unwrap(), "-"],
        ERROR_SOURCE.as_bytes(),
    );
    assert_eq!(configured_legacy.status.code(), Some(1));
    let stdout = String::from_utf8(configured_legacy.stdout).unwrap();
    assert!(stdout.contains("-:1:"), "{stdout}");
    assert!(!stdout.contains("<stdin>"), "{stdout}");

    let configured_ast = run_args_with_stdin(
        &["--config", ast_config.to_str().unwrap(), "-"],
        ERROR_SOURCE.as_bytes(),
    );
    assert_eq!(configured_ast.status.code(), Some(1));
    let stdout = String::from_utf8(configured_ast.stdout).unwrap();
    assert!(stdout.contains("<stdin>:1:"), "{stdout}");
    assert!(!stdout.contains("-:1:"), "{stdout}");

    let default_json = run_args_with_stdin(&["--format", "json", "-"], ERROR_SOURCE.as_bytes());
    assert_eq!(default_json.status.code(), Some(1));
    let stdout = String::from_utf8(default_json.stdout).unwrap();
    assert!(stdout.contains("\"file\":\"<stdin>\""), "{stdout}");

    let legacy_json = run_args_with_stdin(
        &["--engine", "legacy", "--format", "json", "-"],
        ERROR_SOURCE.as_bytes(),
    );
    assert_eq!(legacy_json.status.code(), Some(1));
    let stdout = String::from_utf8(legacy_json.stdout).unwrap();
    assert!(stdout.contains("\"file\":\"-\""), "{stdout}");
    assert!(!stdout.contains("\"file\":\"<stdin>\""), "{stdout}");

    let configured_legacy_json = run_args_with_stdin(
        &[
            "--config",
            legacy_config.to_str().unwrap(),
            "--format",
            "json",
            "-",
        ],
        ERROR_SOURCE.as_bytes(),
    );
    assert_eq!(configured_legacy_json.status.code(), Some(1));
    let stdout = String::from_utf8(configured_legacy_json.stdout).unwrap();
    assert!(stdout.contains("\"file\":\"-\""), "{stdout}");
    assert!(!stdout.contains("\"file\":\"<stdin>\""), "{stdout}");

    let configured_ast_json = run_args_with_stdin(
        &[
            "--config",
            ast_config.to_str().unwrap(),
            "--format",
            "json",
            "-",
        ],
        ERROR_SOURCE.as_bytes(),
    );
    assert_eq!(configured_ast_json.status.code(), Some(1));
    let stdout = String::from_utf8(configured_ast_json.stdout).unwrap();
    assert!(stdout.contains("\"file\":\"<stdin>\""), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn compare_stays_opt_in_under_the_default_engine() {
    let directory = temporary_directory();
    let source = write_file(&directory, "mismatch.lsl", MISMATCH_SOURCE);
    let source = source.to_str().unwrap();

    let default = run_args(&[source]);
    assert_eq!(default.status.code(), Some(1));
    assert!(default.stderr.is_empty(), "{:?}", default.stderr);
    let stdout = String::from_utf8(default.stdout.clone()).unwrap();
    assert!(stdout.contains("error[SEM011]"), "{stdout}");
    assert!(!stdout.contains("SEM002"), "{stdout}");

    let explicit_ast = run_args(&["--engine", "ast", source]);
    assert_eq!(default.status.code(), explicit_ast.status.code());
    assert_eq!(default.stdout, explicit_ast.stdout);
    assert_eq!(default.stderr, explicit_ast.stderr);
    assert_ne!(default.status.code(), Some(3));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn default_invocation_equals_explicit_ast_for_representative_inputs() {
    let directory = temporary_directory();
    let clean = write_file(&directory, "clean.lsl", CLEAN_SOURCE);
    let error = write_file(&directory, "error.lsl", ERROR_SOURCE);
    let macro_source = write_file(
        &directory,
        "macro.lsl",
        "#define X 1\ninteger x = X;\ndefault { state_entry() { } }\n",
    );
    let config = write_file(
        &directory,
        "old.conf",
        "disable = SEM001\nseverity.SEM004 = warning\n",
    );
    let tree = directory.join("tree");
    fs::create_dir_all(tree.join("nested")).unwrap();
    write_file(&tree, "a.lsl", ERROR_SOURCE);
    write_file(&tree.join("nested"), "b.lsl", CLEAN_SOURCE);
    let missing = directory.join("missing.lsl");
    let missing = missing.to_str().unwrap();
    let clean = clean.to_str().unwrap();
    let error = error.to_str().unwrap();
    let macro_source = macro_source.to_str().unwrap();
    let config = config.to_str().unwrap();
    let tree = tree.to_str().unwrap();

    assert_default_matches_explicit_ast(&[clean]);
    assert_default_matches_explicit_ast(&["--format", "json", error]);
    assert_default_matches_explicit_ast(&["--tokens", error]);
    assert_default_matches_explicit_ast(&["--preprocess", "--tokens", macro_source]);
    assert_default_matches_explicit_ast(&[clean, error]);
    assert_default_matches_explicit_ast(&[tree]);
    assert_default_matches_explicit_ast(&["--config", config, error]);
    assert_default_matches_explicit_ast(&["--severity", "SEM004=warning", error]);
    assert_default_matches_explicit_ast(&[missing]);
    assert_default_matches_explicit_ast(&["-I", tree, clean]);
    assert_default_matches_explicit_ast(&["--tokens", "--format", "json", error]);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn default_ast_directory_and_multi_unit_order_is_deterministic() {
    let directory = temporary_directory();
    let tree = directory.join("tree");
    fs::create_dir_all(tree.join("nested")).unwrap();
    write_file(&tree, "a.lsl", ERROR_SOURCE);
    write_file(&tree.join("nested"), "m.lsl", ERROR_SOURCE);
    write_file(&tree, "z.lsl", ERROR_SOURCE);
    let first = write_file(&directory, "first.lsl", ERROR_SOURCE);
    let second = write_file(&directory, "second.lsl", ERROR_SOURCE);

    let directory_first = run_args(&[tree.to_str().unwrap()]);
    let directory_second = run_args(&[tree.to_str().unwrap()]);
    assert_eq!(directory_first.status.code(), Some(1));
    assert_eq!(
        directory_first.status.code(),
        directory_second.status.code()
    );
    assert_eq!(directory_first.stdout, directory_second.stdout);
    assert_eq!(directory_first.stderr, directory_second.stderr);
    assert_eq!(
        directory_first.stdout,
        run_args(&["--engine", "ast", tree.to_str().unwrap()]).stdout
    );
    let stdout = String::from_utf8(directory_first.stdout).unwrap();
    let a = stdout
        .find(&format!("{}:", tree.join("a.lsl").display()))
        .unwrap();
    let m = stdout
        .find(&format!("{}:", tree.join("nested").join("m.lsl").display()))
        .unwrap();
    let z = stdout
        .find(&format!("{}:", tree.join("z.lsl").display()))
        .unwrap();
    assert!(
        a < m && m < z,
        "directory unit order is not sorted: {stdout}"
    );

    let multi_first = run_args(&[first.to_str().unwrap(), second.to_str().unwrap()]);
    let multi_second = run_args(&[first.to_str().unwrap(), second.to_str().unwrap()]);
    assert_eq!(multi_first.status.code(), Some(1));
    assert_eq!(multi_first.stdout, multi_second.stdout);
    assert_eq!(multi_first.stderr, multi_second.stderr);
    let stdout = String::from_utf8(multi_first.stdout).unwrap();
    let first_position = stdout.find(&format!("{}:", first.display())).unwrap();
    let second_position = stdout.find(&format!("{}:", second.display())).unwrap();
    assert!(
        first_position < second_position,
        "explicit multi-unit order changed: {stdout}"
    );

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn firestorm_switch_body_diagnostics_are_reported_by_default_ast() {
    let directory = temporary_directory();
    let source = write_file(
        &directory,
        "switch_counter.lsl",
        FIRESTORM_SWITCH_UNDECLARED,
    );
    let source = source.to_str().unwrap();

    let default = run_args(&["--dialect", "firestorm", source]);
    assert_eq!(default.status.code(), Some(0));
    let stdout = String::from_utf8(default.stdout.clone()).unwrap();
    assert!(
        stdout.contains(":7:17: warning[SEM012]: variable 'counter'"),
        "switch-arm diagnostic was skipped: {stdout}"
    );

    let explicit_ast = run_args(&["--engine", "ast", "--dialect", "firestorm", source]);
    assert_eq!(default.status.code(), explicit_ast.status.code());
    assert_eq!(default.stdout, explicit_ast.stdout);
    assert_eq!(default.stderr, explicit_ast.stderr);

    let legacy = run_args(&["--engine", "legacy", "--dialect", "firestorm", source]);
    assert_eq!(legacy.status.code(), Some(1));
    let stdout = String::from_utf8(legacy.stdout).unwrap();
    assert!(stdout.contains("PAR007"), "{stdout}");

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn default_ast_ordering_is_deterministic_for_divergent_corpus_input() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/invalid/calls_and_assignments.lsl");
    let display = fixture.to_str().unwrap();

    let first = run_args(&[display]);
    let second = run_args(&[display]);
    let explicit_ast = run_args(&["--engine", "ast", display]);
    assert_eq!(first.status.code(), Some(1));
    assert_eq!(first.status.code(), second.status.code());
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
    assert_eq!(first.stdout, explicit_ast.stdout);
    assert!(
        first.stderr.is_empty(),
        "default AST corpus run wrote stderr: {:?}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_ne!(
        first.status.code(),
        Some(3),
        "Compare must never become the default on corpus input"
    );

    let stdout = String::from_utf8(first.stdout).unwrap();
    let first_line = stdout.lines().next().unwrap();
    assert!(
        first_line.contains(":10:18: error[SEM004]"),
        "AST order changed: {first_line}"
    );

    let legacy = run_args(&["--engine", "legacy", display]);
    let stdout = String::from_utf8(legacy.stdout).unwrap();
    let legacy_first_line = stdout.lines().next().unwrap();
    assert!(
        legacy_first_line.contains(":13:9: error[SEM016]"),
        "Legacy order changed: {legacy_first_line}"
    );
    assert_ne!(first_line, legacy_first_line);
}

#[test]
fn compare_tokens_come_from_the_single_lex() {
    const SOURCE: &str = "#define X 1\ninteger x = X;\ndefault { state_entry() { } }\n";
    let directory = temporary_directory();
    let source = write_file(&directory, "tokens.lsl", SOURCE);
    let display = source.to_str().unwrap();

    let mut run = neolint::AnalysisRun::new().unwrap();
    let index = run
        .add_preprocessed_text(display, SOURCE, neolint::PreprocessSettings::full())
        .unwrap();
    let prepared = run.unit(index).unwrap().prepared_source().unwrap();
    let mut expected = String::new();
    for token in neolint::lex(prepared.text()).0 {
        expected.push_str(&format!(
            "{display}:{}:{}: {:?} {:?}\n",
            token.span.line, token.span.column, token.kind, token.lexeme
        ));
    }

    let output = run_args(&["--tokens", "--preprocess", "--engine", "compare", display]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout, expected);
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("compare unit 0"));

    // The default AST path obtains the same tokens from its single engine lex,
    // then appends the syntactically diagnosed unused global.
    let default_output = run_args(&["--tokens", "--preprocess", display]);
    assert_eq!(default_output.status.code(), Some(0));
    let sem024 =
        format!("{display}:2:9: warning[SEM024]: variable 'x' is declared but never read\n");
    assert_eq!(
        String::from_utf8(default_output.stdout).unwrap(),
        format!("{expected}{sem024}")
    );
    assert!(default_output.stderr.is_empty());

    fs::remove_dir_all(directory).unwrap();
}
