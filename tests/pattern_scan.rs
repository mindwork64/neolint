use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

fn neolint() -> Command {
    Command::new(env!("CARGO_BIN_EXE_neolint"))
}

fn run_with_diff(arguments: &[&str], diff: &str) -> std::process::Output {
    let mut child = neolint()
        .arg("scan-diff")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(diff.as_bytes())
        .unwrap();

    child.wait_with_output().unwrap()
}

const DIFF: &str = concat!(
    "diff --git a/applet.js b/applet.js\n",
    "new file mode 100644\n",
    "--- /dev/null\n",
    "+++ b/applet.js\n",
    "@@ -0,0 +1,2 @@\n",
    "+const directory = \".local/share\";\n",
    "+GLib.spawn_command_line_sync(\"slow-command\");\n",
);

#[test]
fn built_in_cinnamon_rules_report_added_lines_without_failing_by_default() {
    let output = run_with_diff(&[], DIFF);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("warning[XDG001]"), "{stdout}");
    assert!(stdout.contains("warning[ASYNC001]"), "{stdout}");
    assert!(stdout.contains("applet.js:1:"), "{stdout}");
    assert!(stdout.contains("applet.js:2:"), "{stdout}");
}

#[test]
fn fail_on_match_turns_findings_into_exit_code_one() {
    let output = run_with_diff(&["--fail-on-match"], DIFF);

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("XDG001"), "{stdout}");
}

#[test]
fn json_format_emits_machine_readable_fields() {
    let output = run_with_diff(&["--format", "json"], DIFF);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("[{"), "{stdout}");
    assert!(stdout.contains("\"file\":\"applet.js\""), "{stdout}");
    assert!(stdout.contains("\"code\":\"XDG001\""), "{stdout}");
    assert!(stdout.contains("\"severity\":\"warning\""), "{stdout}");
    assert!(stdout.ends_with("]\n"), "{stdout}");
}

#[test]
fn extension_filter_narrows_the_builtin_rule_set() {
    let output = run_with_diff(&["--extensions", ".py"], DIFF);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), "No pattern matches.");
}

#[test]
fn custom_toml_rule_file_replaces_the_builtin_profile() {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("neolint-pattern-test-{}-{id}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let rules_path = directory.join("rules.toml");
    fs::write(
        &rules_path,
        r#"
[[rules]]
id = "CUSTOM001"
name = "Custom token"
severity = "info"
pattern = "custom_token"
message = "Project-specific pattern matched."
"#,
    )
    .unwrap();

    let output = run_with_diff(
        &["--rules", rules_path.to_str().unwrap()],
        &DIFF.replace(".local/share", "custom_token"),
    );

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("info[CUSTOM001]"), "{stdout}");
    assert!(!stdout.contains("XDG001"), "{stdout}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_custom_rule_file_returns_cli_error() {
    let output = run_with_diff(&["--rules", "/this/path/does/not/exist.toml"], DIFF);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("cannot read rules"), "{stderr}");
}

#[test]
fn normal_lsl_invocation_is_still_the_default_mode() {
    let output = neolint().arg("--version").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("neolint "), "{stdout}");
}
