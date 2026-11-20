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

    fs::write(
        &source,
        r#"default { state_entry() { llOwnerSay(42); } }"#,
    )
    .unwrap();

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
