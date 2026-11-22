//! Dependency-free rule configuration for the CLI.
//!
//! Configuration is a small line-oriented `key = value` format. It supports
//! disabling rule codes and overriding their severity; command-line overrides
//! are applied afterward by the binary. Parsing and validation return human
//! readable errors so the CLI can exit with code 2 without panicking.
use crate::diagnostic::{Diagnostic, Severity};
use crate::preprocessor::PreprocessDialect;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Rule configuration for disabling rules and overriding their severity.

#[derive(Debug, Clone, Default)]
pub struct LintConfig {
    disabled_rules: HashSet<String>,
    severity_overrides: HashMap<String, Severity>,
    preprocess: bool,
    dialect: PreprocessDialect,
    include_dirs: Vec<PathBuf>,
}

impl LintConfig {
    /// Parse a small, dependency-free configuration format:
    ///
    /// disable = SEM001, SEM003
    /// severity.SEM004 = warning
    ///
    /// Blank lines and lines beginning with '#' are ignored.
    /// Parse the line-oriented configuration format.
    ///
    /// Blank lines and comments are ignored. Supported settings are
    /// `disable = RULE[, RULE...]` and `severity.RULE = error|warning|info`.
    ///
    /// # Errors
    ///
    /// Returns a line-numbered error for malformed settings, invalid rule
    /// codes, or invalid severity names.
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut config = Self::default();

        for (line_index, raw_line) in source.lines().enumerate() {
            let line_number = line_index + 1;
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("line {line_number}: expected 'key = value'"));
            };

            let key = key.trim();
            let value = value.trim();

            if key == "disable" {
                for rule in value
                    .split(',')
                    .map(str::trim)
                    .filter(|rule| !rule.is_empty())
                {
                    validate_rule_code(rule)
                        .map_err(|error| format!("line {line_number}: {error}"))?;
                    config.disabled_rules.insert(rule.to_owned());
                }
                continue;
            }

            if let Some(rule) = key.strip_prefix("severity.") {
                let rule = rule.trim();
                validate_rule_code(rule).map_err(|error| format!("line {line_number}: {error}"))?;
                let severity = parse_severity(value)
                    .ok_or_else(|| format!(
                        "line {line_number}: invalid severity '{value}' (expected error, warning or info)"
                    ))?;
                config.severity_overrides.insert(rule.to_owned(), severity);
                continue;
            }

            if key == "preprocess" {
                config.preprocess = match value {
                    "true" => true,
                    "false" => false,
                    _ => {
                        return Err(format!(
                            "line {line_number}: invalid preprocess value '{value}' (expected true or false)"
                        ))
                    }
                };
                continue;
            }

            if key == "dialect" {
                config.dialect = match value {
                    "lsl" => PreprocessDialect::Lsl,
                    "firestorm" => {
                        config.preprocess = true;
                        PreprocessDialect::Firestorm
                    }
                    _ => {
                        return Err(format!(
                        "line {line_number}: invalid dialect '{value}' (expected lsl or firestorm)"
                    ))
                    }
                };
                continue;
            }

            if key == "include_dir" {
                if value.is_empty() {
                    return Err(format!("line {line_number}: include_dir requires a path"));
                }
                config.include_dirs.push(PathBuf::from(value));
                continue;
            }

            return Err(format!("line {line_number}: unknown setting '{key}'"));
        }

        Ok(config)
    }

    /// Disable each supplied rule code.
    ///
    /// # Errors
    ///
    /// Returns an error when a code is empty or contains characters other than
    /// uppercase ASCII letters and digits.
    pub fn disable(&mut self, rules: impl IntoIterator<Item = String>) -> Result<(), String> {
        for rule in rules {
            validate_rule_code(&rule)?;
            self.disabled_rules.insert(rule);
        }
        Ok(())
    }

    /// Override the severity for one rule code.
    ///
    /// # Errors
    ///
    /// Returns an error when the rule code is syntactically invalid.
    pub fn set_severity(&mut self, rule: &str, severity: Severity) -> Result<(), String> {
        validate_rule_code(rule)?;
        self.severity_overrides.insert(rule.to_owned(), severity);
        Ok(())
    }

    /// Whether preprocessing was requested by the configuration file.
    pub fn preprocess_enabled(&self) -> bool {
        self.preprocess
    }

    /// Configured preprocessing dialect.
    pub fn dialect(&self) -> PreprocessDialect {
        self.dialect
    }

    /// Configured include directories, in file order.
    pub fn include_dirs(&self) -> &[PathBuf] {
        &self.include_dirs
    }

    /// Enable or disable preprocessing (CLI override).
    pub fn set_preprocess(&mut self, enabled: bool) {
        self.preprocess = enabled;
    }

    /// Override the dialect (CLI override).
    pub fn set_dialect(&mut self, dialect: PreprocessDialect) {
        self.dialect = dialect;
        if dialect == PreprocessDialect::Firestorm {
            self.preprocess = true;
        }
    }

    /// Replace the include-directory list (CLI override).
    pub fn set_include_dirs(&mut self, dirs: Vec<PathBuf>) {
        self.include_dirs = dirs;
    }

    /// Apply disabled-rule filtering and severity overrides in place.
    ///
    /// Disabled diagnostics are removed; remaining diagnostics are updated in
    /// their original order.
    pub fn apply(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.retain(|diagnostic| !self.disabled_rules.contains(diagnostic.code));

        for diagnostic in diagnostics {
            if let Some(severity) = self.severity_overrides.get(diagnostic.code) {
                diagnostic.severity = *severity;
            }
        }
    }
}

fn validate_rule_code(rule: &str) -> Result<(), String> {
    if rule.is_empty()
        || !rule
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return Err(format!(
            "invalid rule code '{rule}' (use uppercase letters and digits)"
        ));
    }
    Ok(())
}

fn parse_severity(value: &str) -> Option<Severity> {
    match value {
        "error" => Some(Severity::Error),
        "warning" => Some(Severity::Warning),
        "info" => Some(Severity::Info),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{DiagnosticKind, Span};

    fn diagnostic(code: &'static str) -> Diagnostic {
        Diagnostic {
            code,
            severity: Severity::Error,
            kind: DiagnosticKind::Semantic,
            message: "test diagnostic".to_owned(),
            span: Span {
                start: 0,
                end: 1,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 2,
            },
        }
    }

    #[test]
    fn parses_disabled_rules_and_severity_overrides() {
        let config = LintConfig::parse(
            "# project defaults\ndisable = SEM001, SEM003\nseverity.SEM004 = warning\n",
        )
        .unwrap();

        let mut diagnostics = vec![
            diagnostic("SEM001"),
            diagnostic("SEM002"),
            diagnostic("SEM003"),
            diagnostic("SEM004"),
        ];
        config.apply(&mut diagnostics);

        assert_eq!(
            diagnostics.iter().map(|item| item.code).collect::<Vec<_>>(),
            vec!["SEM002", "SEM004"]
        );
        assert_eq!(diagnostics[1].severity, Severity::Warning);
    }

    #[test]
    fn command_line_settings_can_be_applied_after_config() {
        let mut config = LintConfig::parse("severity.SEM004 = warning").unwrap();
        config.set_severity("SEM004", Severity::Info).unwrap();
        config.disable(vec!["SEM002".to_owned()]).unwrap();

        let mut diagnostics = vec![diagnostic("SEM002"), diagnostic("SEM004")];
        config.apply(&mut diagnostics);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Info);
    }

    #[test]
    fn rejects_unknown_settings_and_invalid_severities() {
        assert!(LintConfig::parse("enabled = false").is_err());
        assert!(LintConfig::parse("severity.SEM001 = fatal").is_err());
    }

    #[test]
    fn rejects_malformed_rule_codes() {
        assert!(LintConfig::parse("disable = sem001").is_err());
        assert!(LintConfig::parse("severity.SEM-001 = warning").is_err());
    }

    #[test]
    fn accepts_empty_configuration() {
        assert!(LintConfig::parse("# no local overrides\n\n").is_ok());
    }
}

#[cfg(test)]
mod preprocess_config_tests {
    use super::*;

    #[test]
    fn parses_preprocess_dialect_and_repeated_include_dirs() {
        let config = LintConfig::parse(
            "preprocess = true\ndialect = firestorm\ninclude_dir = ./a\ninclude_dir = ./b\n",
        )
        .unwrap();
        assert!(config.preprocess_enabled());
        assert_eq!(config.dialect(), PreprocessDialect::Firestorm);
        assert_eq!(
            config.include_dirs(),
            &[PathBuf::from("./a"), PathBuf::from("./b")]
        );
    }

    #[test]
    fn firestorm_dialect_enables_preprocessing() {
        let config = LintConfig::parse("dialect = firestorm").unwrap();
        assert!(config.preprocess_enabled());
        assert_eq!(config.dialect(), PreprocessDialect::Firestorm);
    }

    #[test]
    fn rejects_invalid_preprocess_settings() {
        assert!(LintConfig::parse("preprocess = maybe").is_err());
        assert!(LintConfig::parse("dialect = lua").is_err());
        assert!(LintConfig::parse("include_dir =").is_err());
    }
}
