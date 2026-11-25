//! AST-engine golden baselines for Phase 6H (GitHub Issue #103), Slice 0.
//!
//! # Provenance
//!
//! Every expected file in `tests/golden/ast/` was captured from the
//! **unmodified** base binary at
//! `ab49154dee10e6e52629bf95c869bf9b33798764`, before any production-source
//! edit. See `tests/golden/ast/PROVENANCE.md` for the full record: tree SHA,
//! toolchain versions, binary and fixture SHA-256 values, and the exact capture
//! commands.
//!
//! # Manifest
//!
//! [`CASES`] is the single source of truth for the AST matrix. Each entry names
//! the fixture working directory under `tests/golden/<case>/` (the original
//! inputs, which these tests only read) and the relative argv. The expected
//! `stdout.txt`, `stderr.txt`, and `exit.txt` live in the separate
//! `tests/golden/ast/<case>/` tree.
//!
//! The binary always runs with `current_dir` set to the fixture directory and
//! with relative input paths, so path-bearing output never contains absolute
//! paths, temporary directories, PIDs, or machine specifics.
//!
//! # Baseline immutability
//!
//! [`GOLDEN_DIGESTS`] pins the SHA-256 of every captured artifact. Before any
//! process output is compared, [`run_case`] verifies the on-disk goldens
//! against those constants; a mismatch fails with an explicit "baseline
//! goldens were modified" message. Regenerating these files from a modified
//! binary is therefore a deliberate, review-visible edit rather than an
//! incidental "fix" for a broken case.
//!
//! # Scope
//!
//! Both accepted spellings are covered against the same goldens:
//! `--engine ast` and `--engine=ast`. The default invocation (no `--engine`)
//! now also selects AST, so [`run_cases`] is called with `&[]` as well. A
//! separate byte-for-byte equivalence test asserts that the default invocation
//! and explicit `--engine ast` produce identical stdout, stderr, and exit
//! status for every case.
//!
//! The Firestorm fixture uses a switch statement. The immutable Phase 6H
//! baseline records no diagnostics. The later CFG refinement has one explicit
//! expected stdout delta: its known selector reaches case 1, whose break makes
//! the default arm unreachable (SEM037 at line 10:17). The Legacy golden still
//! records the pre-change diagnostics (exit 1). The stdin case pins the AST
//! source label `<stdin>`; the Legacy golden labels the same input `-`.
//!
//! These fixtures are LF-only and CI is Linux; path rendering uses `/`. The
//! `directory` case pins recursive collection order (`a.lsl` before
//! `nested/b.lsl`) and would be platform-sensitive if fixtures or CI moved to
//! a platform with different path separators.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One recorded AST CLI invocation and its expected artifacts.
struct AstGoldenCase {
    /// Fixture directory under `tests/golden/` that is also the working directory.
    name: &'static str,
    /// Relative argv appended after the engine-selection arguments.
    args: &'static [&'static str],
    /// Optional stdin fixture, relative to the case directory.
    stdin: Option<&'static str>,
}

/// The complete AST golden matrix. Keep it in sync with the fixture directories
/// and captured `tests/golden/ast/<case>/{stdout,stderr,exit}.txt` files.
const CASES: &[AstGoldenCase] = &[
    AstGoldenCase {
        name: "plain",
        args: &["plain.lsl"],
        stdin: None,
    },
    AstGoldenCase {
        name: "preprocess_include",
        args: &["--preprocess", "-I", "inc", "root.lsl"],
        stdin: None,
    },
    AstGoldenCase {
        name: "firestorm",
        args: &["--dialect", "firestorm", "firestorm.lsl"],
        stdin: None,
    },
    AstGoldenCase {
        name: "stdin",
        args: &["-"],
        stdin: Some("input.lsl"),
    },
    AstGoldenCase {
        name: "directory",
        args: &["tree"],
        stdin: None,
    },
    AstGoldenCase {
        name: "tokens_preprocess",
        args: &["--tokens", "--preprocess", "-I", "inc", "root.lsl"],
        stdin: None,
    },
];

/// SHA-256 of each captured artifact for one case. These constants are the
/// immutability anchor for the Phase 6H baseline: changing any golden file
/// requires a deliberate, reviewed change to this manifest.
struct GoldenDigests {
    case: &'static str,
    stdout: &'static str,
    stderr: &'static str,
    exit: &'static str,
}

/// Digest manifest captured from the base binary and recorded in
/// `tests/golden/ast/PROVENANCE.md`. Keep every value in sync with the
/// committed golden files; the guard in [`run_case`] fails loudly otherwise.
const GOLDEN_DIGESTS: &[GoldenDigests] = &[
    GoldenDigests {
        case: "plain",
        stdout: "66976e4fb535c14a92cacbfd6c812073e8747e401edfb78c99afe2831ff9a52c",
        stderr: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        exit: "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865",
    },
    GoldenDigests {
        case: "preprocess_include",
        stdout: "31bc1969b67f2b4d6db519f1ccd5524a8811b95bf611af6908cf4ad7354363ac",
        stderr: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        exit: "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865",
    },
    GoldenDigests {
        case: "firestorm",
        stdout: "d8cb113eb17a942a1b115686d59cb8b3525e83b06efb14e5ce26ab00a8c156e2",
        stderr: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        exit: "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa",
    },
    GoldenDigests {
        case: "stdin",
        stdout: "f2fe6dd25a06dfd860e3830625bd54a40ec11e7e0c77f1f6ce9b02ae52a3fe56",
        stderr: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        exit: "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa",
    },
    GoldenDigests {
        case: "directory",
        stdout: "0032e2c4bc5263c57c32e6781f6efb5985194bb31c6746e862d1f30ceca01027",
        stderr: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        exit: "4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865",
    },
    GoldenDigests {
        case: "tokens_preprocess",
        stdout: "8d6ac0495225443762d371d76f4264b4e1d9e733585b29eb3e4e759e8e48ec59",
        stderr: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        exit: "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa",
    },
];

fn digest_manifest(name: &str) -> &'static GoldenDigests {
    GOLDEN_DIGESTS
        .iter()
        .find(|entry| entry.case == name)
        .unwrap_or_else(|| panic!("no digest manifest entry for AST golden case '{name}'"))
}

fn fixture_directory(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

fn golden_directory(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("ast")
        .join(name)
}

fn read_golden(directory: &Path, case: &AstGoldenCase, file: &str) -> Vec<u8> {
    let path = directory.join(file);
    fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{}: cannot read AST golden {}: {error}",
            case.name,
            path.display()
        )
    })
}

// Keep the Phase 6H capture immutable while making later intentional AST
// analysis refinements explicit in the expected-current-output layer.
fn expected_stdout(case: &AstGoldenCase, baseline: &[u8]) -> Vec<u8> {
    match case.name {
        "firestorm" => {
            assert_eq!(
                baseline, b"firestorm.lsl: no diagnostics\n",
                "the immutable Firestorm AST baseline changed"
            );
            b"firestorm.lsl:10:17: warning[SEM037]: unreachable statement\n".to_vec()
        }
        _ => baseline.to_vec(),
    }
}

fn assert_bytes(expected: &[u8], actual: &[u8], case: &str, stream: &str) {
    if actual != expected {
        panic!(
            "{case}: {stream} differs from the captured AST golden\n\
             --- expected ---\n{}\n\
             --- actual ---\n{}",
            String::from_utf8_lossy(expected),
            String::from_utf8_lossy(actual)
        );
    }
}

/// Verify the on-disk goldens against [`GOLDEN_DIGESTS`] before any process
/// output is compared. This makes editing a baseline a deliberate act.
fn verify_golden_digests(directory: &Path, case: &AstGoldenCase) -> Result<(), String> {
    let expected = digest_manifest(case.name);
    let files = [
        ("stdout.txt", expected.stdout),
        ("stderr.txt", expected.stderr),
        ("exit.txt", expected.exit),
    ];
    for (file, expected_digest) in files {
        let path = directory.join(file);
        let bytes = fs::read(&path).map_err(|error| {
            format!(
                "AST baseline goldens are immutable, but {} could not be read: {error}",
                path.display()
            )
        })?;
        let actual_digest = sha256_hex(&bytes);
        if actual_digest.as_str() != expected_digest {
            return Err(format!(
                "AST baseline goldens were modified: {} has SHA-256 {actual_digest}, but the captured Phase 6H digest is {expected_digest}. Phase 6H requires the baseline goldens to remain immutable; they must not be regenerated from a modified binary.",
                path.display()
            ));
        }
    }
    Ok(())
}

fn assert_golden_digests(directory: &Path, case: &AstGoldenCase) {
    if let Err(message) = verify_golden_digests(directory, case) {
        panic!("{message}");
    }
}

/// SHA-256 round constants (FIPS 180-4).
const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// Minimal dependency-free SHA-256 (FIPS 180-4).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = (bytes.len() as u64).wrapping_mul(8);

    let mut padded = Vec::with_capacity(bytes.len() + 72);
    padded.extend_from_slice(bytes);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut schedule = [0u32; 64];
        for (word, quad) in schedule.iter_mut().zip(chunk.chunks_exact(4)) {
            *word = u32::from_be_bytes([quad[0], quad[1], quad[2], quad[3]]);
        }
        let mut index = 16;
        while index < 64 {
            let x = schedule[index - 15];
            let y = schedule[index - 2];
            let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
            index += 1;
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        let mut index = 0;
        while index < 64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(SHA256_K[index])
                .wrapping_add(schedule[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
            index += 1;
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    let mut hex = String::with_capacity(64);
    {
        use std::fmt::Write as _;
        for word in state {
            write!(&mut hex, "{word:08x}").expect("writing to String cannot fail");
        }
    }
    hex
}

fn run_case_output(case: &AstGoldenCase, engine_args: &[&str]) -> std::process::Output {
    let directory = fixture_directory(case.name);
    let mut command = Command::new(env!("CARGO_BIN_EXE_neolint"));
    command.current_dir(&directory);
    command.args(engine_args);
    command.args(case.args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    if let Some(stdin_name) = case.stdin {
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
    }
}

fn run_case(case: &AstGoldenCase, engine_args: &[&str]) {
    let golden = golden_directory(case.name);
    assert_golden_digests(&golden, case);
    let output = run_case_output(case, engine_args);

    let expected_exit = read_golden(&golden, case, "exit.txt");
    let status = output
        .status
        .code()
        .unwrap_or_else(|| panic!("{}: neolint terminated without an exit code", case.name));
    let actual_exit = format!("{status}\n");
    if actual_exit.as_bytes() != expected_exit.as_slice() {
        panic!(
            "{}: exit status differs from the captured AST golden\n\
             --- expected ---\n{}\n\
             --- actual ---\n{}\n\
             --- stderr ---\n{}",
            case.name,
            String::from_utf8_lossy(&expected_exit),
            actual_exit,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let baseline_stdout = read_golden(&golden, case, "stdout.txt");
    let expected_stdout = expected_stdout(case, &baseline_stdout);
    assert_bytes(&expected_stdout, &output.stdout, case.name, "stdout");
    assert_bytes(
        &read_golden(&golden, case, "stderr.txt"),
        &output.stderr,
        case.name,
        "stderr",
    );
}

/// Run the full AST matrix with `engine_args` inserted before each case's
/// positional inputs. The default-invocation test calls this with `&[]`.
fn run_cases(engine_args: &[&str]) {
    for case in CASES {
        run_case(case, engine_args);
    }
}

#[test]
fn ast_engine_space_form_matches_captured_goldens() {
    run_cases(&["--engine", "ast"]);
}

#[test]
fn ast_engine_equals_form_matches_captured_goldens() {
    run_cases(&["--engine=ast"]);
}

#[test]
fn default_invocation_matches_ast_goldens() {
    run_cases(&[]);
}

/// The no-option default must be byte-for-byte the explicit AST invocation.
#[test]
fn default_invocation_is_identical_to_explicit_ast() {
    for case in CASES {
        let default = run_case_output(case, &[]);
        let explicit = run_case_output(case, &["--engine", "ast"]);
        assert_eq!(
            default.status.code(),
            explicit.status.code(),
            "{}: default and explicit AST exit statuses differ",
            case.name
        );
        assert_eq!(
            default.stdout, explicit.stdout,
            "{}: default and explicit AST stdout differ",
            case.name
        );
        assert_eq!(
            default.stderr, explicit.stderr,
            "{}: default and explicit AST stderr differ",
            case.name
        );
    }
}

#[test]
fn sha256_hex_matches_known_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn digest_guard_rejects_tampered_golden() {
    let fixture = golden_directory("plain");
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("ast-golden-guard-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for file in ["stdout.txt", "stderr.txt", "exit.txt"] {
        fs::copy(fixture.join(file), scratch.join(file)).unwrap();
    }

    let mut tampered = fs::read(scratch.join("stdout.txt")).unwrap();
    tampered.push(b'!');
    fs::write(scratch.join("stdout.txt"), tampered).unwrap();

    let case = CASES
        .iter()
        .find(|case| case.name == "plain")
        .expect("plain case is in CASES");
    let error =
        verify_golden_digests(&scratch, case).expect_err("tampered golden must be rejected");
    assert!(
        error.contains("baseline goldens were modified"),
        "unexpected guard error: {error}"
    );
    assert!(
        error.contains("stdout.txt") && error.contains("Phase 6H"),
        "unexpected guard error: {error}"
    );

    fs::remove_dir_all(&scratch).unwrap();
}
