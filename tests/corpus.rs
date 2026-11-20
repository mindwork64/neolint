use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DiagnosticKey {
    code: String,
    severity: String,
}

#[derive(Debug)]
struct Fixture {
    id: String,
    size: String,
    path: String,
    origin: String,
    license: String,
    syntax_valid: bool,
    expected: BTreeMap<DiagnosticKey, usize>,
    exit: i32,
    limitations: String,
    purpose: String,
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

fn parse_expected(
    value: &str,
    line_number: usize,
) -> Result<BTreeMap<DiagnosticKey, usize>, String> {
    let mut expected = BTreeMap::new();
    if value == "none" {
        return Ok(expected);
    }

    for item in value.split(',') {
        let parts: Vec<&str> = item.split(':').collect();
        if parts.len() != 3 {
            return Err(format!(
                "manifest line {line_number}: expected CODE:COUNT:SEVERITY, got {item:?}"
            ));
        }
        let code = parts[0];
        if code.len() != 6
            || !code[..3].bytes().all(|byte| byte.is_ascii_uppercase())
            || !code[3..].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(format!(
                "manifest line {line_number}: invalid diagnostic code {code:?}"
            ));
        }
        let count = parts[1]
            .parse::<usize>()
            .map_err(|_| format!("manifest line {line_number}: invalid diagnostic count"))?;
        if count == 0 {
            return Err(format!(
                "manifest line {line_number}: diagnostic count must be positive"
            ));
        }
        let severity = parts[2];
        if !matches!(severity, "error" | "warning" | "info") {
            return Err(format!(
                "manifest line {line_number}: invalid severity {severity:?}"
            ));
        }
        let key = DiagnosticKey {
            code: code.to_owned(),
            severity: severity.to_owned(),
        };
        if expected.insert(key, count).is_some() {
            return Err(format!(
                "manifest line {line_number}: duplicate diagnostic {code}:{severity}"
            ));
        }
    }

    Ok(expected)
}

fn parse_manifest() -> Result<Vec<Fixture>, String> {
    let manifest_path = corpus_root().join("manifest.txt");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let mut fixtures = Vec::new();
    let mut ids = HashSet::new();

    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() != 10 {
            return Err(format!(
                "manifest line {line_number}: expected 10 pipe-separated fields"
            ));
        }

        let id = fields[0].trim();
        if id.is_empty() || !ids.insert(id.to_owned()) {
            return Err(format!(
                "manifest line {line_number}: empty or duplicate fixture id {id:?}"
            ));
        }
        let size = fields[1].trim();
        if !matches!(size, "small" | "medium" | "large" | "invalid") {
            return Err(format!(
                "manifest line {line_number}: invalid size category {size:?}"
            ));
        }
        let path = fields[2].trim();
        let relative = Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || relative.extension().and_then(|value| value.to_str()) != Some("lsl")
        {
            return Err(format!(
                "manifest line {line_number}: invalid fixture path {path:?}"
            ));
        }
        let origin = fields[3].trim();
        let license = fields[4].trim();
        if origin.is_empty() || license.is_empty() {
            return Err(format!(
                "manifest line {line_number}: origin and license are required"
            ));
        }
        let syntax = fields[5].trim();
        let syntax_valid = match syntax {
            "valid" => true,
            "invalid" => false,
            _ => {
                return Err(format!(
                    "manifest line {line_number}: syntax must be valid or invalid"
                ))
            }
        };
        let expected = parse_expected(fields[6].trim(), line_number)?;
        let has_syntax_error = expected
            .keys()
            .any(|key| key.code.starts_with("PAR") || key.code.starts_with("LEX"));
        if syntax_valid && has_syntax_error {
            return Err(format!(
                "manifest line {line_number}: valid fixture has syntax diagnostics"
            ));
        }
        if !syntax_valid && !has_syntax_error {
            return Err(format!(
                "manifest line {line_number}: invalid fixture lacks syntax diagnostics"
            ));
        }
        let exit = fields[7]
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("manifest line {line_number}: invalid exit code"))?;
        if !matches!(exit, 0 | 1) {
            return Err(format!("manifest line {line_number}: exit must be 0 or 1"));
        }
        let limitations = fields[8].trim();
        let purpose = fields[9].trim();
        if limitations.is_empty() || purpose.is_empty() {
            return Err(format!(
                "manifest line {line_number}: limitations and purpose are required"
            ));
        }

        fixtures.push(Fixture {
            id: id.to_owned(),
            size: size.to_owned(),
            path: path.to_owned(),
            origin: origin.to_owned(),
            license: license.to_owned(),
            syntax_valid,
            expected,
            exit,
            limitations: limitations.to_owned(),
            purpose: purpose.to_owned(),
        });
    }

    if fixtures.is_empty() {
        return Err("manifest contains no fixtures".to_owned());
    }
    Ok(fixtures)
}

fn collect_lsl_files(root: &Path, directory: &Path, files: &mut BTreeSet<String>) {
    for entry in fs::read_dir(directory).expect("read corpus directory") {
        let entry = entry.expect("read corpus entry");
        let file_type = entry.file_type().expect("read corpus file type");
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_lsl_files(root, &path, files);
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("lsl")
        {
            let relative = path.strip_prefix(root).expect("corpus path");
            let relative = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.insert(relative);
        }
    }
}

fn assert_manifest_is_valid(fixtures: &[Fixture]) {
    let root = corpus_root();
    let mut paths = BTreeSet::new();
    for fixture in fixtures {
        assert!(
            root.join(&fixture.path).is_file(),
            "{}: missing fixture {}",
            fixture.id,
            fixture.path
        );
        assert!(
            paths.insert(fixture.path.clone()),
            "{}: duplicate path {}",
            fixture.id,
            fixture.path
        );
        assert!(
            !fixture.origin.is_empty() && !fixture.license.is_empty(),
            "{}: missing provenance",
            fixture.id
        );
        assert!(
            !fixture.limitations.is_empty(),
            "{}: missing limitations",
            fixture.id
        );
        assert!(
            !fixture.purpose.is_empty(),
            "{}: missing purpose",
            fixture.id
        );
    }

    let mut actual = BTreeSet::new();
    collect_lsl_files(&root, &root, &mut actual);
    assert_eq!(actual, paths, "manifest and corpus file sets differ");
}

fn parse_output(stdout: &str) -> BTreeMap<DiagnosticKey, usize> {
    let mut diagnostics = BTreeMap::new();
    for line in stdout.lines() {
        for (marker, severity) in [
            ("error[", "error"),
            ("warning[", "warning"),
            ("info[", "info"),
        ] {
            let Some(start) = line.find(marker) else {
                continue;
            };
            let code_start = start + marker.len();
            let Some(end) = line[code_start..].find(']') else {
                continue;
            };
            let code = &line[code_start..code_start + end];
            let key = DiagnosticKey {
                code: code.to_owned(),
                severity: severity.to_owned(),
            };
            *diagnostics.entry(key).or_insert(0) += 1;
            break;
        }
    }
    diagnostics
}

fn neolint() -> Command {
    Command::new(env!("CARGO_BIN_EXE_neolint"))
}

#[test]
fn corpus_manifest_is_valid_and_complete() {
    let fixtures = parse_manifest().expect("valid manifest");
    assert_manifest_is_valid(&fixtures);

    let mut sizes = BTreeMap::new();
    for fixture in &fixtures {
        *sizes.entry(fixture.size.as_str()).or_insert(0) += 1;
    }
    assert_eq!(
        fixtures.len(),
        25,
        "update corpus report when adding fixtures"
    );
    assert_eq!(sizes.get("small"), Some(&9));
    assert_eq!(sizes.get("medium"), Some(&6));
    assert_eq!(sizes.get("large"), Some(&3));
    assert_eq!(sizes.get("invalid"), Some(&7));
    assert!(
        fixtures
            .iter()
            .all(|fixture| fixture.origin == "original" && fixture.license == "project-authored"),
        "this corpus contains only project-authored fixtures"
    );
}

#[test]
fn every_fixture_matches_manifest_through_cli() {
    let fixtures = parse_manifest().expect("valid manifest");
    let root = corpus_root();

    for fixture in &fixtures {
        let path = root.join(&fixture.path);
        let output = neolint()
            .arg(&path)
            .output()
            .unwrap_or_else(|error| panic!("{}: cannot run CLI: {error}", fixture.id));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let actual = parse_output(&stdout);
        assert_eq!(
            actual, fixture.expected,
            "{} ({}) diagnostics mismatch\nstdout:\n{stdout}",
            fixture.id, fixture.path
        );
        assert_eq!(
            output.status.code(),
            Some(fixture.exit),
            "{} ({}) exit mismatch\nstdout:\n{stdout}\nstderr:\n{}",
            fixture.id,
            fixture.path,
            String::from_utf8_lossy(&output.stderr)
        );
        let has_syntax_error = actual
            .keys()
            .any(|key| key.code.starts_with("PAR") || key.code.starts_with("LEX"));
        assert_eq!(
            has_syntax_error, !fixture.syntax_valid,
            "{} ({}) syntax classification mismatch",
            fixture.id, fixture.path
        );
    }
}

#[test]
fn recursive_directory_run_matches_manifest_aggregate() {
    let fixtures = parse_manifest().expect("valid manifest");
    let root = corpus_root();
    let started = Instant::now();
    let output = neolint().arg(&root).output().expect("run corpus directory");
    let elapsed = started.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let actual = parse_output(&stdout);

    let mut expected = BTreeMap::new();
    for fixture in &fixtures {
        for (key, count) in &fixture.expected {
            *expected.entry(key.clone()).or_insert(0) += count;
        }
    }
    assert_eq!(
        actual, expected,
        "recursive corpus diagnostics mismatch\nstdout:\n{stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "corpus contains error fixtures"
    );
    for fixture in &fixtures {
        let path = root.join(&fixture.path);
        assert!(
            stdout.contains(&path.to_string_lossy().to_string()),
            "{} ({}) was not processed by directory traversal",
            fixture.id,
            fixture.path
        );
    }
    assert!(
        elapsed < Duration::from_secs(10),
        "corpus directory run exceeded smoke budget: {elapsed:?}"
    );
}
