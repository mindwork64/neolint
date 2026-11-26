use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new() -> Self {
        let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("neolint-project-cli-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn write(&self, path: &str, contents: &str) -> PathBuf {
        let path = self.root.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    fn manifest(&self, contents: &str) -> PathBuf {
        self.write("neolint.project", contents)
    }

    fn run(&self, extra: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_neolint"));
        command
            .arg("--project")
            .arg(self.root.join("neolint.project"));
        command.args(extra);
        command.output().unwrap()
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const SAME_NAME_SCRIPT: &str = r#"
integer shared_value;
integer helper() { return shared_value; }
default {
    state_entry() {
        llOwnerSay((string)helper());
    }
}
"#;

#[test]
fn project_keeps_same_named_globals_and_functions_in_separate_compilation_units() {
    let project = TempProject::new();
    project.write("Scripts/first.lsl", SAME_NAME_SCRIPT);
    project.write("Scripts/second.lsl", SAME_NAME_SCRIPT);
    project.manifest("script = Scripts/first.lsl\nscript = Scripts/second.lsl\n");

    let output = project.run(&[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Scripts/first.lsl: no diagnostics"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Scripts/second.lsl: no diagnostics"),
        "{stdout}"
    );
    assert!(!stdout.contains("SEM001"), "{stdout}");
}

#[test]
fn project_configs_are_overlaid_per_script_and_reported_in_manifest_order() {
    let project = TempProject::new();
    project.write(".neolint.conf", "engine = ast\nseverity.SEM024 = warning\n");
    project.write("config/first.conf", "disable = SEM024\n");
    project.write("config/second.conf", "severity.SEM024 = info\n");
    for name in ["first", "second", "third"] {
        project.write(
            &format!("Scripts/{name}.lsl"),
            &format!("integer unused_{name};\ndefault {{ state_entry() {{ }} }}\n"),
        );
    }
    project.manifest(
        "config = .neolint.conf\n\
         script = Scripts/first.lsl\n\
         script = Scripts/second.lsl\n\
         script = Scripts/third.lsl\n\
         script_config = Scripts/first.lsl => config/first.conf\n\
         script_config = Scripts/second.lsl => config/second.conf\n",
    );

    let output = project.run(&["--format", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains("Scripts/first.lsl"), "{stdout}");
    assert!(
        stdout.contains("\"file\":\"Scripts/second.lsl\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"file\":\"Scripts/third.lsl\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"severity\":\"info\""), "{stdout}");
    assert!(stdout.contains("\"severity\":\"warning\""), "{stdout}");
    assert!(
        stdout.find("Scripts/second.lsl").unwrap() < stdout.find("Scripts/third.lsl").unwrap(),
        "diagnostics did not follow manifest order: {stdout}"
    );
}

#[test]
fn project_cli_overrides_have_highest_precedence_for_every_script() {
    let project = TempProject::new();
    project.write(".neolint.conf", "severity.SEM024 = warning\n");
    project.write(
        "Scripts/a.lsl",
        "integer unused_a;\ndefault { state_entry() {} }\n",
    );
    project.write(
        "Scripts/b.lsl",
        "integer unused_b;\ndefault { state_entry() {} }\n",
    );
    project.manifest("config = .neolint.conf\nscript = Scripts/a.lsl\nscript = Scripts/b.lsl\n");

    let output = project.run(&["--format", "json", "--severity", "SEM024=info"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.matches("\"severity\":\"info\"").count(),
        2,
        "{stdout}"
    );
    assert!(!stdout.contains("\"severity\":\"warning\""), "{stdout}");
}

#[test]
fn project_cli_config_file_overrides_project_and_per_script_configs() {
    let project = TempProject::new();
    project.write(".neolint.conf", "engine = ast\nseverity.SEM024 = warning\n");
    project.write("config/a.conf", "severity.SEM024 = info\n");
    project.write("cli/overrides.conf", "severity.SEM024 = error\n");
    project.write(
        "Scripts/a.lsl",
        "integer unused_a;\ndefault { state_entry() {} }\n",
    );
    project.write(
        "Scripts/b.lsl",
        "integer unused_b;\ndefault { state_entry() {} }\n",
    );
    project.manifest(
        "config = .neolint.conf\n\
         script = Scripts/a.lsl\n\
         script = Scripts/b.lsl\n\
         script_config = Scripts/a.lsl => config/a.conf\n",
    );

    let cli_config = project.root.join("cli/overrides.conf");
    let output = project.run(&["--format", "json", "--config", cli_config.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.matches("\"severity\":\"error\"").count(),
        2,
        "{stdout}"
    );
    assert!(!stdout.contains("\"severity\":\"info\""), "{stdout}");
    assert!(!stdout.contains("\"severity\":\"warning\""), "{stdout}");
}

#[test]
fn project_ossl_function_additions_are_inherited_but_strict_overlay_resets_them() {
    let project = TempProject::new();
    project.write(
        ".neolint.conf",
        "target = opensim-yengine\nossl_allowlist = strict\nossl_function = osGetAgentIP\n",
    );
    project.write("config/strict.conf", "ossl_allowlist = strict\n");
    let source = "default { state_entry() { osGetAgentIP(llGetOwner()); } }\n";
    project.write("Scripts/strict.lsl", source);
    project.write("Scripts/inherited.lsl", source);
    project.manifest(
        "config = .neolint.conf\n\
         script = Scripts/strict.lsl\n\
         script = Scripts/inherited.lsl\n\
         script_config = Scripts/strict.lsl => config/strict.conf\n",
    );

    let output = project.run(&["--format", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("\"file\":\"Scripts/strict.lsl\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"code\":\"SEM048\""), "{stdout}");
    let inherited_start = stdout.find("Scripts/inherited.lsl");
    assert!(
        inherited_start.is_none(),
        "clean inherited file should not add an empty JSON entry: {stdout}"
    );
    assert_eq!(stdout.matches("SEM048").count(), 1, "{stdout}");
}

#[test]
fn project_supports_a_per_script_legacy_engine_override() {
    let project = TempProject::new();
    project.write(".neolint.conf", "engine = ast\n");
    project.write("config/legacy.conf", "engine = legacy\n");
    project.write(
        "Scripts/legacy.lsl",
        "default { state_entry() { llOwnerSay(42); } }\n",
    );
    project.write(
        "Scripts/ast.lsl",
        "integer unused_ast;\ndefault { state_entry() {} }\n",
    );
    project.manifest(
        "config = .neolint.conf\n\
         script = Scripts/legacy.lsl\n\
         script = Scripts/ast.lsl\n\
         script_config = Scripts/legacy.lsl => config/legacy.conf\n",
    );

    let output = project.run(&[]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("Scripts/legacy.lsl:1:9: warning[SEM024]"),
        "{stdout}"
    );
    assert!(stdout.contains("Scripts/legacy.lsl"), "{stdout}");
    assert!(stdout.contains("error[SEM004]"), "{stdout}");
    assert!(
        stdout.contains("Scripts/ast.lsl:1:9: warning[SEM024]"),
        "{stdout}"
    );
}

#[test]
fn project_resolves_each_script_include_directories_from_its_config_file() {
    let project = TempProject::new();
    project.write(
        "Scripts/first.lsl",
        "#include \"helper.lsl\"\ndefault { state_entry() {} }\n",
    );
    project.write("includes/helper.lsl", "integer included_unused;\n");
    project.write(
        "config/first.conf",
        "preprocess = true\ninclude_dir = ../includes\n",
    );
    project.manifest(
        "script = Scripts/first.lsl\nscript_config = Scripts/first.lsl => config/first.conf\n",
    );

    let output = project.run(&[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Scripts/first.lsl: no diagnostics"),
        "{stdout}"
    );
    assert!(stdout.contains("helper.lsl"), "{stdout}");
    assert!(stdout.contains("warning[SEM024]"), "{stdout}");
}

#[test]
fn replaced_missing_include_directories_are_not_validated_before_overlays() {
    let project = TempProject::new();
    project.write(
        ".neolint.conf",
        "preprocess = true\ninclude_dir = missing-from-project\n",
    );
    project.write(
        "config-include/helper.lsl",
        "integer included_config_unused;\n",
    );
    project.write("cli-include/helper.lsl", "integer included_cli_unused;\n");
    project.write(
        "Scripts/per-script.lsl",
        "#include \"helper.lsl\"\ndefault { state_entry() {} }\n",
    );
    project.write(
        "Scripts/cli-override.lsl",
        "#include \"helper.lsl\"\ndefault { state_entry() {} }\n",
    );
    project.write(
        "config/per-script.conf",
        "include_dir = ../config-include\n",
    );

    project.manifest(
        "config = .neolint.conf\n\
         script = Scripts/per-script.lsl\n\
         script_config = Scripts/per-script.lsl => config/per-script.conf\n",
    );
    let per_script = project.run(&[]);
    assert_eq!(
        per_script.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&per_script.stdout),
        String::from_utf8_lossy(&per_script.stderr)
    );
    assert!(
        String::from_utf8_lossy(&per_script.stdout).contains("helper.lsl"),
        "{}",
        String::from_utf8_lossy(&per_script.stdout)
    );

    project.manifest("config = .neolint.conf\nscript = Scripts/cli-override.lsl\n");
    let cli_include = project.root.join("cli-include");
    let cli_override = project.run(&["-I", cli_include.to_str().unwrap()]);
    assert_eq!(
        cli_override.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&cli_override.stdout),
        String::from_utf8_lossy(&cli_override.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cli_override.stdout).contains("helper.lsl"),
        "{}",
        String::from_utf8_lossy(&cli_override.stdout)
    );
}

#[test]
fn project_rejects_missing_include_directory_after_all_overlays() {
    let project = TempProject::new();
    project.write(
        ".neolint.conf",
        "preprocess = true\ninclude_dir = missing-from-project\n",
    );
    project.write("Scripts/a.lsl", "default { state_entry() {} }\n");
    project.manifest("config = .neolint.conf\nscript = Scripts/a.lsl\n");

    let output = project.run(&[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("cannot access include directory"),
        "{stderr}"
    );
    assert!(stderr.contains("missing-from-project"), "{stderr}");
}

#[test]
fn project_json_related_locations_use_manifest_path_for_root_script() {
    let project = TempProject::new();
    project.write(
        "Scripts/repeat.lsl",
        "#define ITEM 1\n#define ITEM 2\ndefault { state_entry() {} }\n",
    );
    project.manifest("script = Scripts/repeat.lsl\n");

    let output = project.run(&["--preprocess", "--format", "json"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"code\":\"PRE005\""), "{stdout}");
    assert_eq!(
        stdout.matches("\"file\":\"Scripts/repeat.lsl\"").count(),
        2,
        "both primary and related root locations must use the manifest spelling: {stdout}"
    );
    assert!(
        !stdout.contains(project.root.to_str().unwrap()),
        "absolute project path leaked into JSON related locations: {stdout}"
    );
}

#[test]
fn project_rejects_positional_inputs_and_missing_manifest_entries() {
    let project = TempProject::new();
    project.write("Scripts/a.lsl", "default { state_entry() {} }\n");
    project.manifest("script = Scripts/a.lsl\n");
    let path = project.root.join("Scripts/a.lsl");
    let positional = Command::new(env!("CARGO_BIN_EXE_neolint"))
        .arg("--project")
        .arg(project.root.join("neolint.project"))
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(positional.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&positional.stderr).contains("cannot be combined"),
        "{}",
        String::from_utf8_lossy(&positional.stderr)
    );

    project.manifest("script = Scripts/missing.lsl\n");
    let missing = project.run(&[]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("cannot resolve script"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}
