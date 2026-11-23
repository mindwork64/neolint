//! Pre-change CLI golden baselines for Phase 6G (GitHub Issue #101).
//!
//! # Provenance
//!
//! Every expected file in `tests/golden/` was captured from the **unmodified**
//! base tree, before any production-source edit:
//!
//! - base commit: `4dd01ca16539e6944a6714f133e9d2636adb6a95`
//!   (`origin/exp/preprocessor-ast`, the Phase 6F squash merge);
//! - branch: `exp-preprocessor-ast-phase-06g-engine-integration`;
//! - `git status --porcelain` was empty when the captures were taken;
//! - `cargo build --bin neolint` built the base binary used below.
//!
//! Capture procedure, run from the repository root:
//!
//! ```text
//! cargo build --bin neolint
//! BIN="$PWD/target/debug/neolint"
//! (cd tests/golden/plain              && "$BIN" plain.lsl >stdout.txt 2>stderr.txt); echo $?
//! (cd tests/golden/preprocess_include && "$BIN" --preprocess -I inc root.lsl >stdout.txt 2>stderr.txt); echo $?
//! (cd tests/golden/firestorm          && "$BIN" --dialect firestorm firestorm.lsl >stdout.txt 2>stderr.txt); echo $?
//! (cd tests/golden/stdin              && "$BIN" - <input.lsl >stdout.txt 2>stderr.txt); echo $?
//! (cd tests/golden/directory          && "$BIN" tree >stdout.txt 2>stderr.txt); echo $?
//! (cd tests/golden/tokens_preprocess  && "$BIN" --tokens --preprocess -I inc root.lsl >stdout.txt 2>stderr.txt); echo $?
//! ```
//!
//! The statuses printed by those commands are recorded in [`CASES`]. The
//! expected outputs were never produced by a modified binary.
//!
//! # Manifest
//!
//! [`CASES`] is the single source of truth: each entry names the fixture
//! working directory under `tests/golden/`, the relative argv, the optional
//! stdin fixture, the expected exit status, and the expected `stdout.txt` /
//! `stderr.txt` files. The binary always runs with `current_dir` set to that
//! fixture directory and with relative input paths, so path-bearing output
//! never contains absolute paths, temporary directories, PIDs, or machine
//! specifics.
//!
//! The Firestorm fixture uses a `switch` statement, which the AST engine
//! accepts only in the Firestorm dialect. The base CLI is Legacy-only, so the
//! recorded golden deliberately pins the base Legacy diagnostics for that
//! construct; that is exactly the pre-change behavior a later `--engine legacy`
//! variant must preserve. The fixture also defines `USE_SWITCHES`, the
//! Firestorm extension latch, so the dialect's preprocessing path is exercised.
//!
//! `run_cases` takes extra argv that is inserted before the positional inputs.
//! The default test and the explicit `--engine legacy` test both assert the same
//! pre-change goldens, so selecting Legacy through the new flag is byte-for-byte
//! identical to the unmodified base.
//!
//! These fixtures are LF-only and CI is Linux; path rendering uses `/`. The
//! `directory` case pins recursive collection order (`a.lsl` before
//! `nested/b.lsl`) and would be platform-sensitive if fixtures or CI moved to
//! a platform with different path separators.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One recorded CLI invocation and its expected artifacts.
struct GoldenCase {
    /// Directory under `tests/golden/` that is also the process working directory.
    name: &'static str,
    /// Relative argv appended after any future engine-selection arguments.
    args: &'static [&'static str],
    /// Optional stdin fixture, relative to the case directory.
    stdin: Option<&'static str>,
    /// Expected process exit status.
    status: i32,
}

/// The complete golden matrix. This is the manifest; keep it in sync with the
/// fixture directories and captured `stdout.txt` / `stderr.txt` files.
const CASES: &[GoldenCase] = &[
    GoldenCase {
        name: "plain",
        args: &["plain.lsl"],
        stdin: None,
        status: 1,
    },
    GoldenCase {
        name: "preprocess_include",
        args: &["--preprocess", "-I", "inc", "root.lsl"],
        stdin: None,
        status: 1,
    },
    GoldenCase {
        name: "firestorm",
        args: &["--dialect", "firestorm", "firestorm.lsl"],
        stdin: None,
        status: 1,
    },
    GoldenCase {
        name: "stdin",
        args: &["-"],
        stdin: Some("input.lsl"),
        status: 0,
    },
    GoldenCase {
        name: "directory",
        args: &["tree"],
        stdin: None,
        status: 1,
    },
    GoldenCase {
        name: "tokens_preprocess",
        args: &["--tokens", "--preprocess", "-I", "inc", "root.lsl"],
        stdin: None,
        status: 0,
    },
];

fn fixture_directory(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

fn read_golden(directory: &Path, case: &GoldenCase, file: &str) -> Vec<u8> {
    let path = directory.join(file);
    fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{}: cannot read golden {}: {error}",
            case.name,
            path.display()
        )
    })
}

fn assert_bytes(expected: &[u8], actual: &[u8], case: &str, stream: &str) {
    if actual != expected {
        panic!(
            "{case}: {stream} differs from the pre-change golden\n\
             --- expected ---\n{}\n\
             --- actual ---\n{}",
            String::from_utf8_lossy(expected),
            String::from_utf8_lossy(actual)
        );
    }
}

fn run_case(case: &GoldenCase, extra_args: &[&str]) {
    let directory = fixture_directory(case.name);
    let mut command = Command::new(env!("CARGO_BIN_EXE_neolint"));
    command.current_dir(&directory);
    command.args(extra_args);
    command.args(case.args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = if let Some(stdin_name) = case.stdin {
        let input = fs::read(directory.join(stdin_name)).unwrap_or_else(|error| {
            panic!(
                "{}: cannot read stdin fixture {stdin_name}: {error}",
                case.name
            )
        });
        let mut child = command
            .stdin(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("{}: cannot spawn neolint: {error}", case.name));
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(&input)
            .unwrap_or_else(|error| panic!("{}: cannot feed stdin: {error}", case.name));
        child
            .wait_with_output()
            .unwrap_or_else(|error| panic!("{}: cannot wait for neolint: {error}", case.name))
    } else {
        command
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|error| panic!("{}: cannot run neolint: {error}", case.name))
    };

    assert_eq!(
        output.status.code(),
        Some(case.status),
        "{}: exit status differs from the pre-change golden; stderr:\n{}",
        case.name,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_bytes(
        &read_golden(&directory, case, "stdout.txt"),
        &output.stdout,
        case.name,
        "stdout",
    );
    assert_bytes(
        &read_golden(&directory, case, "stderr.txt"),
        &output.stderr,
        case.name,
        "stderr",
    );
}

fn run_cases(extra_args: &[&str]) {
    for case in CASES {
        run_case(case, extra_args);
    }
}

#[test]
fn default_invocation_matches_pre_change_goldens() {
    run_cases(&[]);
}

#[test]
fn explicit_legacy_engine_matches_pre_change_goldens() {
    run_cases(&["--engine", "legacy"]);
}
