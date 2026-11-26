//! Deterministic manifest for a multi-script NeoLint project.
//!
//! A project lists independent root scripts. It never combines their ASTs,
//! symbol tables, preprocessors, or semantic state.
//!
//! Manifest format (paths are relative to the manifest directory):
//!
//! ```text
//! config = .neolint.conf
//! script = Scripts/controller.lsl
//! script = Scripts/worker.lsl
//! script_config = Scripts/controller.lsl => .neolint/controller.conf
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A script entry in a ProjectManifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScript {
    path: PathBuf,
    display_path: PathBuf,
    config: Option<PathBuf>,
}

impl ProjectScript {
    /// Canonical path to the script file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Path spelling as written in the manifest, used in project reports.
    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    /// Optional per-script config file, resolved relative to the manifest directory.
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }
}

/// A deterministic list of independent LSL scripts and their configuration files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectManifest {
    path: PathBuf,
    root: PathBuf,
    default_config: Option<PathBuf>,
    scripts: Vec<ProjectScript>,
}

impl ProjectManifest {
    /// Load and validate a project manifest from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|error| {
            format!("cannot read project manifest '{}': {error}", path.display())
        })?;
        Self::parse(path, &source)
    }

    /// Parse a manifest while validating every referenced file.
    ///
    /// Relative paths are resolved from the manifest directory. Scripts are
    /// kept in declaration order; duplicate canonical script paths are rejected.
    pub fn parse(path: impl AsRef<Path>, source: &str) -> Result<Self, String> {
        let absolute_path = absolute_lexical(path.as_ref())?;
        let parent = absolute_path.parent().unwrap_or_else(|| Path::new("."));
        let root = fs::canonicalize(parent).map_err(|error| {
            format!(
                "cannot resolve project directory '{}': {error}",
                parent.display()
            )
        })?;
        let manifest_name = absolute_path.file_name().ok_or_else(|| {
            format!(
                "project manifest path '{}' has no file name",
                absolute_path.display()
            )
        })?;
        // Keep parse() usable with an in-memory manifest whose file has not
        // been written yet, while still normalizing its parent directory.
        let path = root.join(manifest_name);

        let mut default_config: Option<(PathBuf, usize)> = None;
        let mut script_entries: Vec<(PathBuf, PathBuf, usize)> = Vec::new();
        let mut script_configs: Vec<(PathBuf, PathBuf, usize)> = Vec::new();

        for (index, raw_line) in source.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("line {line_number}: expected 'key = value'"));
            };
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                return Err(format!("line {line_number}: '{key}' requires a value"));
            }

            match key {
                "config" => {
                    if default_config.is_some() {
                        return Err(format!("line {line_number}: duplicate project config"));
                    }
                    let resolved = resolve_file(&root, value, "project config")
                        .map_err(|error| format!("line {line_number}: {error}"))?;
                    default_config = Some((resolved, line_number));
                }
                "script" => {
                    let display = PathBuf::from(value);
                    let resolved = resolve_file(&root, value, "script")
                        .map_err(|error| format!("line {line_number}: {error}"))?;
                    if !has_lsl_extension(&resolved) {
                        return Err(format!(
                            "line {line_number}: script '{}' must have a .lsl extension",
                            value
                        ));
                    }
                    script_entries.push((resolved, display, line_number));
                }
                "script_config" => {
                    let Some((script, config)) = value.split_once("=>") else {
                        return Err(format!(
                            "line {line_number}: script_config must be 'SCRIPT => CONFIG'"
                        ));
                    };
                    let script = script.trim();
                    let config = config.trim();
                    if script.is_empty() || config.is_empty() {
                        return Err(format!(
                            "line {line_number}: script_config requires both script and config paths"
                        ));
                    }
                    let script_path = resolve_file(&root, script, "script")
                        .map_err(|error| format!("line {line_number}: {error}"))?;
                    let config_path = resolve_file(&root, config, "script config")
                        .map_err(|error| format!("line {line_number}: {error}"))?;
                    script_configs.push((script_path, config_path, line_number));
                }
                _ => {
                    return Err(format!(
                        "line {line_number}: unknown project setting '{key}'"
                    ))
                }
            }
        }

        if script_entries.is_empty() {
            return Err("project manifest contains no script entries".to_owned());
        }

        let mut seen_scripts = HashSet::new();
        for (script, _, line_number) in &script_entries {
            if !seen_scripts.insert(script.clone()) {
                return Err(format!(
                    "line {line_number}: duplicate script '{}' (same canonical file)",
                    script.display()
                ));
            }
        }

        let mut configs_by_script = HashMap::new();
        for (script, config, line_number) in script_configs {
            if !seen_scripts.contains(&script) {
                return Err(format!(
                    "line {line_number}: script_config refers to a script not listed in this project: '{}'",
                    script.display()
                ));
            }
            if configs_by_script.insert(script.clone(), config).is_some() {
                return Err(format!(
                    "line {line_number}: duplicate script_config for '{}'",
                    script.display()
                ));
            }
        }

        let scripts = script_entries
            .into_iter()
            .map(|(path, display_path, _)| ProjectScript {
                config: configs_by_script.remove(&path),
                path,
                display_path,
            })
            .collect();

        Ok(Self {
            path,
            root,
            default_config: default_config.map(|(path, _)| path),
            scripts,
        })
    }

    /// Absolute manifest path with its parent directory resolved.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Canonical project directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Optional project-wide config file.
    pub fn default_config(&self) -> Option<&Path> {
        self.default_config.as_deref()
    }

    /// Scripts in manifest declaration order.
    pub fn scripts(&self) -> &[ProjectScript] {
        &self.scripts
    }
}

fn absolute_lexical(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| format!("cannot resolve current directory: {error}"))
    }
}

fn resolve_file(root: &Path, value: &str, description: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    let candidate = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let resolved = fs::canonicalize(&candidate).map_err(|error| {
        format!(
            "cannot resolve {description} '{}': {error}",
            candidate.display()
        )
    })?;
    let metadata = fs::metadata(&resolved).map_err(|error| {
        format!(
            "cannot inspect {description} '{}': {error}",
            resolved.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "{description} '{}' is not a regular file",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn has_lsl_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lsl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("neolint-project-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, contents).unwrap();
            path
        }

        fn manifest_path(&self) -> PathBuf {
            self.0.join("neolint.project")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn loads_scripts_in_manifest_order_with_default_and_per_script_configs() {
        let temp = TempDir::new();
        temp.file(".neolint.conf", "engine = ast\n");
        temp.file("config/worker.conf", "disable = SEM001\n");
        temp.file("Scripts/controller.lsl", "default {}");
        temp.file("Scripts/worker.lsl", "default {}");

        let manifest = ProjectManifest::parse(
            temp.manifest_path(),
            "config = .neolint.conf\nscript = Scripts/worker.lsl\nscript = Scripts/controller.lsl\nscript_config = Scripts/worker.lsl => config/worker.conf\n",
        )
        .unwrap();

        assert_eq!(manifest.scripts().len(), 2);
        assert_eq!(
            manifest.scripts()[0].display_path(),
            Path::new("Scripts/worker.lsl")
        );
        assert_eq!(
            manifest.scripts()[0].config(),
            Some(
                fs::canonicalize(temp.0.join("config/worker.conf"))
                    .unwrap()
                    .as_path()
            )
        );
        assert_eq!(
            manifest.scripts()[1].display_path(),
            Path::new("Scripts/controller.lsl")
        );
        assert!(manifest.scripts()[1].config().is_none());
        assert!(manifest.default_config().is_some());
    }

    #[test]
    fn rejects_duplicate_scripts_even_when_paths_use_different_spellings() {
        let temp = TempDir::new();
        temp.file("a.lsl", "default {}");
        let error =
            ProjectManifest::parse(temp.manifest_path(), "script = a.lsl\nscript = ./a.lsl\n")
                .unwrap_err();
        assert!(error.contains("duplicate script"), "{error}");
    }

    #[test]
    fn rejects_script_config_for_unlisted_script() {
        let temp = TempDir::new();
        temp.file("a.lsl", "default {}");
        temp.file("b.lsl", "default {}");
        temp.file("b.conf", "disable = SEM001");
        let error = ProjectManifest::parse(
            temp.manifest_path(),
            "script = a.lsl\nscript_config = b.lsl => b.conf\n",
        )
        .unwrap_err();
        assert!(error.contains("not listed"), "{error}");
    }

    #[test]
    fn rejects_multiple_configs_for_the_same_script() {
        let temp = TempDir::new();
        temp.file("a.lsl", "default {}");
        temp.file("one.conf", "disable = SEM001\n");
        temp.file("two.conf", "disable = SEM002\n");
        let error = ProjectManifest::parse(
            temp.manifest_path(),
            "script = a.lsl\nscript_config = a.lsl => one.conf\nscript_config = ./a.lsl => two.conf\n",
        )
        .unwrap_err();
        assert!(error.contains("duplicate script_config"), "{error}");
    }

    #[test]
    fn rejects_non_lsl_files_and_empty_projects() {
        let temp = TempDir::new();
        temp.file("readme.txt", "not a script");
        let non_lsl =
            ProjectManifest::parse(temp.manifest_path(), "script = readme.txt\n").unwrap_err();
        assert!(non_lsl.contains(".lsl"), "{non_lsl}");
        let empty = ProjectManifest::parse(temp.manifest_path(), "# no scripts\n").unwrap_err();
        assert!(empty.contains("no script"), "{empty}");
    }

    #[test]
    fn rejects_malformed_and_unknown_settings() {
        let temp = TempDir::new();
        temp.file("a.lsl", "default {}");
        let malformed =
            ProjectManifest::parse(temp.manifest_path(), "script_config = a.lsl config.conf\n")
                .unwrap_err();
        assert!(malformed.contains("SCRIPT => CONFIG"), "{malformed}");
        let unknown =
            ProjectManifest::parse(temp.manifest_path(), "script = a.lsl\nunknown = value\n")
                .unwrap_err();
        assert!(unknown.contains("unknown project setting"), "{unknown}");
    }
}
