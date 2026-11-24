//! Integration regression for the runtime `--help` surface.
//!
//! Runs the built binary rather than calling `print_help()` so the test covers
//! exactly what a user sees. The assertions are deliberately per-option instead
//! of a snapshot: spacing and wrapping may change, but an implemented option
//! disappearing from `--help` must fail with a message naming it.

use std::process::Command;

fn help_stdout() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_neolint"))
        .arg("--help")
        .output()
        .expect("failed to run the neolint binary with --help");
    assert!(
        output.status.success(),
        "neolint --help exited with {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("neolint --help output is not valid UTF-8")
}

#[test]
fn help_lists_the_implemented_preprocessing_options() {
    let help = help_stdout();

    for option in ["--preprocess", "--dialect", "-I", "--include-dir"] {
        assert!(
            help.contains(option),
            "help output does not mention `{option}`:\n{help}"
        );
    }

    for dialect in ["lsl", "firestorm"] {
        assert!(
            help.contains(dialect),
            "help output does not list the `{dialect}` dialect:\n{help}"
        );
    }

    assert!(
        help.contains("enables") && help.contains("preprocessing"),
        "help output does not say that the firestorm dialect enables preprocessing:\n{help}"
    );
    assert!(
        help.contains("repeatable"),
        "help output does not say that include directories are repeatable:\n{help}"
    );
}

#[test]
fn help_lists_the_accepted_equals_and_compact_forms() {
    let help = help_stdout();

    for form in ["--dialect=", "--include-dir=", "-IDIR"] {
        assert!(
            help.contains(form),
            "help output does not mention the accepted form `{form}`:\n{help}"
        );
    }
}

#[test]
fn help_keeps_the_existing_core_options() {
    let help = help_stdout();

    for option in [
        "--format",
        "--config",
        "--disable",
        "--severity",
        "--engine",
        "--tokens",
        "--help",
        "--version",
    ] {
        assert!(
            help.contains(option),
            "help output lost the existing option `{option}`:\n{help}"
        );
    }
}
