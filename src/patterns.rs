//! Regex-based scanner for added lines in unified Git diffs.
//!
//! This is deliberately separate from the LSL lexer/parser/semantic pipeline.
//! It is useful for project-specific API and best-practice policies across
//! source languages without changing normal LSL lint behavior.

use fancy_regex::{Regex, RegexBuilder};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// Severity attached to a pattern rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternSeverity {
    /// A blocking policy violation. It only changes the exit code when
    /// `--fail-on-match` is supplied.
    Block,
    /// A non-blocking recommendation.
    Warning,
    /// Informational finding.
    Info,
}

impl PatternSeverity {
    /// Stable lowercase name for human and JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// One validated, compiled pattern rule.
#[derive(Debug)]
struct PatternRule {
    id: String,
    name: String,
    severity: PatternSeverity,
    extensions: Vec<String>,
    regex: Regex,
    message: String,
}

/// Result reported for one added line in a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternFinding {
    /// Repository-relative path from the diff.
    pub file: String,
    /// One-based line number in the new file.
    pub line: usize,
    /// Stable rule identifier.
    pub code: String,
    /// Human-readable rule name.
    pub name: String,
    /// Rule severity.
    pub severity: PatternSeverity,
    /// Advice associated with the matched pattern.
    pub message: String,
    /// Added source line that matched.
    pub source: String,
}

#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default)]
    settings: RuleSettings,
    rules: Vec<RuleDefinition>,
}

#[derive(Debug, Deserialize)]
struct RuleSettings {
    #[serde(default = "default_max_findings")]
    max_findings_per_file: usize,
    #[serde(default)]
    exclude_extensions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RuleDefinition {
    id: String,
    name: String,
    severity: String,
    #[serde(default)]
    extensions: Vec<String>,
    pattern: String,
    message: String,
}

/// Loaded and validated pattern rules.
#[derive(Debug)]
pub struct PatternScanner {
    rules: Vec<PatternRule>,
    max_findings_per_file: usize,
    exclude_extensions: Vec<String>,
}

impl Default for RuleSettings {
    fn default() -> Self {
        Self {
            max_findings_per_file: default_max_findings(),
            exclude_extensions: Vec::new(),
        }
    }
}

fn default_max_findings() -> usize {
    10
}

impl PatternScanner {
    /// Parse and compile a TOML rule file.
    ///
    /// Rule identifiers must be unique uppercase ASCII letters and digits.
    /// Extensions are case-insensitive and must include their leading dot.
    pub fn from_toml(source: &str) -> Result<Self, String> {
        let file: RuleFile = toml::from_str(source)
            .map_err(|error| format!("invalid pattern rules TOML: {error}"))?;

        if file.rules.is_empty() {
            return Err("pattern rules file does not contain any [[rules]] entries".to_owned());
        }
        if file.settings.max_findings_per_file == 0 {
            return Err("settings.max_findings_per_file must be greater than zero".to_owned());
        }

        let exclude_extensions = normalize_extensions(
            &file.settings.exclude_extensions,
            "settings.exclude_extensions",
        )?;
        let mut seen = HashSet::new();
        let mut rules = Vec::with_capacity(file.rules.len());

        for (index, definition) in file.rules.into_iter().enumerate() {
            let item = index + 1;
            validate_rule_id(&definition.id).map_err(|error| format!("rule #{item}: {error}"))?;
            if !seen.insert(definition.id.clone()) {
                return Err(format!(
                    "rule #{item}: duplicate rule id '{}'",
                    definition.id
                ));
            }
            if definition.name.trim().is_empty() {
                return Err(format!("rule {}: name must not be empty", definition.id));
            }
            if definition.message.trim().is_empty() {
                return Err(format!("rule {}: message must not be empty", definition.id));
            }
            if definition.pattern.is_empty() {
                return Err(format!("rule {}: pattern must not be empty", definition.id));
            }

            let severity = match definition.severity.as_str() {
                "block" => PatternSeverity::Block,
                "warning" => PatternSeverity::Warning,
                "info" => PatternSeverity::Info,
                value => {
                    return Err(format!(
                        "rule {}: invalid severity '{value}' (expected block, warning or info)",
                        definition.id
                    ));
                }
            };
            let extensions = normalize_extensions(
                &definition.extensions,
                &format!("rule {} extensions", definition.id),
            )?;
            let regex = RegexBuilder::new(&definition.pattern)
                .backtrack_limit(100_000)
                .build()
                .map_err(|error| format!("rule {} has invalid regex: {error}", definition.id))?;

            rules.push(PatternRule {
                id: definition.id,
                name: definition.name,
                severity,
                extensions,
                regex,
                message: definition.message,
            });
        }

        Ok(Self {
            rules,
            max_findings_per_file: file.settings.max_findings_per_file,
            exclude_extensions,
        })
    }

    /// Scan only added lines in a unified Git diff.
    ///
    /// If `extension_filter` is supplied, it narrows the file extensions that
    /// are considered; it never broadens a rule's own extension allowlist.
    /// A missing rule extension allowlist means all extensions are accepted.
    pub fn scan_unified_diff(
        &self,
        diff: &str,
        extension_filter: Option<&[String]>,
    ) -> Result<Vec<PatternFinding>, String> {
        let filter = extension_filter.map(normalize_filter);
        let mut findings = Vec::new();
        let mut current_file: Option<String> = None;
        let mut new_line = 0usize;
        let mut findings_for_file = 0usize;

        for line in diff.lines() {
            if line.starts_with("diff --git ") {
                current_file = None;
                new_line = 0;
                findings_for_file = 0;
                continue;
            }

            if let Some(header) = line.strip_prefix("+++ ") {
                if header == "/dev/null" {
                    current_file = None;
                    continue;
                }
                let path = parse_diff_path(header)?;
                current_file = Some(path);
                new_line = 0;
                findings_for_file = 0;
                continue;
            }

            if line.starts_with("@@") {
                new_line = parse_new_line_start(line)
                    .ok_or_else(|| format!("malformed unified diff hunk header: '{line}'"))?
                    .saturating_sub(1);
                continue;
            }

            let Some(file) = current_file.as_deref() else {
                continue;
            };

            if let Some(added) = line.strip_prefix('+') {
                new_line += 1;
                if findings_for_file >= self.max_findings_per_file
                    || !self.file_is_in_scope(file, filter.as_deref())
                {
                    continue;
                }

                for rule in &self.rules {
                    if !rule_accepts_extension(rule, file) {
                        continue;
                    }

                    match rule.regex.is_match(added) {
                        Ok(true) => {
                            findings.push(PatternFinding {
                                file: file.to_owned(),
                                line: new_line,
                                code: rule.id.clone(),
                                name: rule.name.clone(),
                                severity: rule.severity,
                                message: rule.message.clone(),
                                source: added.to_owned(),
                            });
                            findings_for_file += 1;
                            if findings_for_file >= self.max_findings_per_file {
                                break;
                            }
                        }
                        Ok(false) => {}
                        Err(error) => {
                            return Err(format!(
                                "regex evaluation failed for rule {} in {file}:{new_line}: {error}",
                                rule.id
                            ));
                        }
                    }
                }
            } else if line.starts_with(' ') {
                new_line += 1;
            }
            // Removed lines and '\\ No newline...' metadata do not advance
            // the line number in the new file.
        }

        Ok(findings)
    }

    fn file_is_in_scope(&self, file: &str, filter: Option<&[String]>) -> bool {
        let extension = path_extension(file);
        if self
            .exclude_extensions
            .iter()
            .any(|excluded| Some(excluded) == extension.as_ref())
        {
            return false;
        }

        match filter {
            Some(extensions) => {
                extension.is_some_and(|value| extensions.iter().any(|allowed| allowed == &value))
            }
            None => true,
        }
    }

    /// Return the number of compiled rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

fn normalize_extensions(values: &[String], field: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let extension = value.trim().to_ascii_lowercase();
        if !extension.starts_with('.') || extension.len() < 2 {
            return Err(format!(
                "{field}: invalid extension '{value}' (expected '.ext')"
            ));
        }
        if !result.contains(&extension) {
            result.push(extension);
        }
    }
    Ok(result)
}

fn normalize_filter(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            if value.starts_with('.') {
                value
            } else {
                format!(".{value}")
            }
        })
        .collect()
}

fn path_extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_ascii_lowercase()))
}

fn rule_accepts_extension(rule: &PatternRule, file: &str) -> bool {
    if rule.extensions.is_empty() {
        return true;
    }
    let Some(extension) = path_extension(file) else {
        return false;
    };
    rule.extensions.iter().any(|value| value == &extension)
}

fn validate_rule_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return Err(format!(
            "invalid rule id '{id}' (use uppercase ASCII letters and digits)"
        ));
    }
    Ok(())
}

fn parse_new_line_start(header: &str) -> Option<usize> {
    let plus = header.find('+')?;
    let digits = header[plus + 1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn parse_diff_path(header: &str) -> Result<String, String> {
    let value = header.trim();
    let value = if value.starts_with('"') {
        unquote_git_path(value)?
    } else {
        value.to_owned()
    };

    let path = value.strip_prefix("b/").unwrap_or(&value);
    if path.is_empty() {
        return Err("unified diff contains an empty destination path".to_owned());
    }
    Ok(path.to_owned())
}

fn unquote_git_path(value: &str) -> Result<String, String> {
    if !value.ends_with('"') || value.len() < 2 {
        return Err(format!("malformed quoted diff path '{value}'"));
    }
    let inner = &value[1..value.len() - 1];
    let mut output = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = chars
            .next()
            .ok_or_else(|| format!("malformed escape in diff path '{value}'"))?;
        match escaped {
            '"' | '\\' => output.push(escaped),
            't' => output.push('\t'),
            'n' => output.push('\n'),
            other => {
                output.push('\\');
                output.push(other);
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"
[settings]
max_findings_per_file = 10

[[rules]]
id = "XDG001"
name = "Hardcoded XDG data directory"
severity = "warning"
extensions = [".js"]
pattern = '''["']/?\.local/share'''
message = "Use the platform data directory helper instead."

[[rules]]
id = "ASYNC001"
name = "Synchronous process spawning"
severity = "block"
extensions = [".js", ".py"]
pattern = '''(GLib\.spawn_command_line_sync|g_spawn_command_line_sync)\s*\('''
message = "Synchronous process spawning may block the main loop."

[[rules]]
id = "LOOK001"
name = "Lookahead test rule"
severity = "info"
extensions = [".js"]
pattern = "foo(?!bar)"
message = "A lookahead expression was evaluated."
"#;

    fn scanner() -> PatternScanner {
        PatternScanner::from_toml(RULES).unwrap()
    }

    #[test]
    fn parses_rules_and_compiles_lookahead_capable_regex() {
        assert_eq!(scanner().rule_count(), 3);
    }

    #[test]
    fn missing_settings_use_safe_default_finding_limit() {
        let source = r#"
[[rules]]
id = "ONE001"
name = "One rule"
severity = "info"
pattern = "token"
message = "Found token."
"#;
        let scanner = PatternScanner::from_toml(source).unwrap();
        assert_eq!(scanner.max_findings_per_file, 10);
    }

    #[test]
    fn supports_negative_lookahead_in_custom_patterns() {
        let diff = concat!(
            "diff --git a/example.js b/example.js\n",
            "--- a/example.js\n+++ b/example.js\n",
            "@@ -0,0 +1,2 @@\n",
            "+foo;\n",
            "+foobar;\n",
        );
        let findings = scanner().scan_unified_diff(diff, None).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "LOOK001");
    }

    #[test]
    fn scans_added_lines_but_ignores_removed_and_context_lines() {
        let diff = concat!(
            "diff --git a/applet.js b/applet.js\n",
            "--- a/applet.js\n",
            "+++ b/applet.js\n",
            "@@ -3,2 +3,3 @@\n",
            " context line with \".local/share\"\n",
            "-old = \".local/share\"\n",
            "+let path = \".local/share\";\n",
            "+GLib.spawn_command_line_sync(\"bad\");\n",
        );
        let findings = scanner().scan_unified_diff(diff, None).unwrap();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].line, 4);
        assert_eq!(findings[0].code, "XDG001");
        assert_eq!(findings[1].line, 5);
        assert_eq!(findings[1].severity, PatternSeverity::Block);
    }

    #[test]
    fn respects_rule_extensions_exclusions_and_cli_filter() {
        let diff = concat!(
            "diff --git a/readme.md b/readme.md\n",
            "--- a/readme.md\n+++ b/readme.md\n",
            "@@ -0,0 +1 @@\n+GLib.spawn_command_line_sync(\"x\");\n",
            "diff --git a/app.js b/app.js\n",
            "--- a/app.js\n+++ b/app.js\n",
            "@@ -0,0 +1 @@\n+GLib.spawn_command_line_sync(\"x\");\n",
        );
        let findings = scanner().scan_unified_diff(diff, None).unwrap();
        assert_eq!(findings.len(), 1);
        let filtered = scanner()
            .scan_unified_diff(diff, Some(&[".py".to_owned()]))
            .unwrap();
        assert!(filtered.is_empty());
    }

    #[test]
    fn enforces_per_file_finding_limit() {
        let diff = concat!(
            "diff --git a/app.js b/app.js\n",
            "--- a/app.js\n+++ b/app.js\n",
            "@@ -0,0 +1,3 @@\n",
            "+let a = \".local/share\";\n",
            "+let b = \".local/share\";\n",
            "+let c = \".local/share\";\n",
        );
        let rules = RULES.replace("max_findings_per_file = 10", "max_findings_per_file = 2");
        let findings = PatternScanner::from_toml(&rules)
            .unwrap()
            .scan_unified_diff(diff, None)
            .unwrap();
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn rejects_duplicate_ids_invalid_severity_and_invalid_regex() {
        let duplicate = RULES.replace("id = \"ASYNC001\"", "id = \"XDG001\"");
        assert!(PatternScanner::from_toml(&duplicate)
            .unwrap_err()
            .contains("duplicate"));

        let severity = RULES.replace("severity = \"warning\"", "severity = \"fatal\"");
        assert!(PatternScanner::from_toml(&severity)
            .unwrap_err()
            .contains("invalid severity"));

        let invalid_regex = r#"
[[rules]]
id = "BAD001"
name = "Bad regular expression"
severity = "warning"
pattern = "("
message = "Must be rejected."
"#;
        assert!(PatternScanner::from_toml(invalid_regex).is_err());
    }

    #[test]
    fn parses_diff_paths_with_spaces() {
        let path = parse_diff_path("b/src/my file.js").unwrap();
        assert_eq!(path, "src/my file.js");
    }

    #[test]
    fn rejects_malformed_hunk_header() {
        assert!(parse_new_line_start("@@ -1,2 +x,3 @@").is_none());
    }
}
