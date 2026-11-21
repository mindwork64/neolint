use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

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
