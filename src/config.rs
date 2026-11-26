//! Dependency-free rule configuration for the CLI.
//!
//! Configuration is a small line-oriented `key = value` format. It supports
//! disabling rule codes, overriding their severity, preprocessing settings, and
//! analysis-engine selection; command-line overrides are applied afterward by
//! the binary. Parsing and validation return human readable errors so the CLI
//! can exit with code 2 without panicking.
use crate::diagnostic::{Diagnostic, Severity};
use crate::engine::{AnalysisEngine, DisplaySuppression};
use crate::preprocessor::PreprocessDialect;
use crate::target::{OpenSimApiVersion, OsslAllowlist, TargetProfile};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Rule configuration for disabling rules and overriding their severity.

#[derive(Debug, Clone, Default)]
pub struct LintConfig {
    disabled_rules: HashSet<String>,
    severity_overrides: HashMap<String, Severity>,
    preprocess: bool,
    preprocess_explicit: bool,
    dialect: PreprocessDialect,
    dialect_explicit: bool,
    include_dirs: Vec<PathBuf>,
    include_dirs_explicit: bool,
    engine: AnalysisEngine,
    engine_explicit: bool,
    target: TargetProfile,
    target_explicit: bool,
    ossl_allowlist: OsslAllowlist,
    ossl_allowlist_reset_explicit: bool,
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
    /// `disable = RULE[, RULE...]`, `severity.RULE = error|warning|info`,
    /// `preprocess = true|false`, `dialect = lsl|firestorm`,
    /// `include_dir = PATH`, `engine = legacy|ast|compare`, and `target = PROFILE`. A configuration
    /// without an `engine` setting selects [`AnalysisEngine::Ast`].
    ///
    /// # Errors
    ///
    /// Returns a line-numbered error for malformed settings, invalid rule
    /// codes, invalid severity names, or invalid engine selectors.
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut config = Self::default();
        let mut target_version_override: Option<(usize, OpenSimApiVersion)> = None;

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
                config.preprocess_explicit = true;
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
                config.dialect_explicit = true;
                config.dialect = match value {
                    "lsl" => PreprocessDialect::Lsl,
                    "firestorm" => {
                        config.preprocess = true;
                        config.preprocess_explicit = true;
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
                config.include_dirs_explicit = true;
                config.include_dirs.push(PathBuf::from(value));
                continue;
            }

            if key == "engine" {
                config.engine_explicit = true;
                config.engine = AnalysisEngine::parse(value).ok_or_else(|| {
                    format!(
                        "line {line_number}: invalid engine value '{value}' (expected legacy, ast or compare)"
                    )
                })?;
                continue;
            }

            if key == "target" {
                config.target = TargetProfile::parse(value).ok_or_else(|| {
                    format!(
                        "line {line_number}: invalid target '{value}' (expected second-life-mono, second-life-lso, second-life-luau, opensim-xengine or opensim-yengine)"
                    )
                })?;
                config.target_explicit = true;
                continue;
            }

            if key == "target_version" {
                let version = OpenSimApiVersion::parse(value).ok_or_else(|| {
                    format!(
                        "line {line_number}: invalid target_version '{value}' (expected 0.9.2.2, 0.9.3.0, 0.9.3.1 or master-2026-11-25)"
                    )
                })?;
                target_version_override = Some((line_number, version));
                continue;
            }

            if key == "ossl_allowlist" {
                if value != "strict" {
                    return Err(format!(
                        "line {line_number}: invalid ossl_allowlist value '{value}' (expected strict)"
                    ));
                }
                config.enable_ossl_allowlist();
                config.ossl_allowlist_reset_explicit = true;
                continue;
            }

            if key == "ossl_function" {
                config
                    .add_ossl_function(value)
                    .map_err(|error| format!("line {line_number}: {error}"))?;
                continue;
            }

            return Err(format!("line {line_number}: unknown setting '{key}'"));
        }

        if let Some((line_number, version)) = target_version_override {
            config.target = config
                .target
                .with_opensim_api_version(version)
                .ok_or_else(|| {
                    format!(
                        "line {line_number}: target_version {} is not compatible with target {}",
                        version.as_str(),
                        config.target.as_str()
                    )
                })?;
            config.target_explicit = true;
        }

        Ok(config)
    }

    /// Apply explicitly configured values from another config on top of this one.
    ///
    /// Disabled rules are additive, severity overrides replace matching entries,
    /// and scalar settings replace the base only when the overlay explicitly
    /// contains that setting. OSSL function entries extend the inherited
    /// allowlist unless the overlay explicitly sets `ossl_allowlist = strict`,
    /// which resets inherited entries before applying functions from that file.
    /// This is used for project defaults, per-script overrides, and invocation-wide
    /// CLI config overlays.
    pub fn apply_overrides_from(&mut self, overrides: &Self) {
        self.disabled_rules
            .extend(overrides.disabled_rules.iter().cloned());
        self.severity_overrides.extend(
            overrides
                .severity_overrides
                .iter()
                .map(|(rule, severity)| (rule.clone(), *severity)),
        );
        if overrides.preprocess_explicit {
            self.preprocess = overrides.preprocess;
            self.preprocess_explicit = true;
        }
        if overrides.dialect_explicit {
            self.dialect = overrides.dialect;
            self.dialect_explicit = true;
        }
        if overrides.include_dirs_explicit {
            self.include_dirs = overrides.include_dirs.clone();
            self.include_dirs_explicit = true;
        }
        if overrides.engine_explicit {
            self.engine = overrides.engine;
            self.engine_explicit = true;
        }
        if overrides.target_explicit {
            self.target = overrides.target;
            self.target_explicit = true;
        }
        if overrides.ossl_allowlist_reset_explicit {
            // `ossl_allowlist = strict` resets inherited entries. Functions
            // declared in the same overlay remain present regardless of their
            // order relative to the strict setting.
            self.ossl_allowlist = overrides.ossl_allowlist.clone();
            self.ossl_allowlist_reset_explicit = true;
        } else if overrides.ossl_allowlist.is_explicit() {
            // `ossl_function` entries without a strict reset extend the
            // inherited allowlist.
            self.ossl_allowlist.enable();
            for function in overrides.ossl_allowlist.functions() {
                self.ossl_allowlist.add(function.clone());
            }
        }
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

    /// Configured target platform/runtime profile.
    pub fn target(&self) -> TargetProfile {
        self.target
    }

    /// Whether a target was explicitly selected in the config or CLI.
    pub fn target_explicit(&self) -> bool {
        self.target_explicit
    }

    /// Override the target profile (CLI override).
    pub fn set_target(&mut self, target: TargetProfile) {
        self.target = target;
        self.target_explicit = true;
    }

    /// Override the OpenSim API snapshot version (CLI override).
    pub fn set_target_version(&mut self, version: OpenSimApiVersion) -> Result<(), String> {
        self.target = self
            .target
            .with_opensim_api_version(version)
            .ok_or_else(|| {
                format!(
                    "target version {} is incompatible with target {}",
                    version.as_str(),
                    self.target.as_str()
                )
            })?;
        self.target_explicit = true;
        Ok(())
    }

    /// OSSL functions that the project expects to be enabled in its OpenSim region.
    pub fn ossl_allowlist(&self) -> &OsslAllowlist {
        &self.ossl_allowlist
    }

    /// Explicitly enable the OSSL allowlist, even if it is empty.
    pub fn enable_ossl_allowlist(&mut self) {
        self.ossl_allowlist.enable();
    }

    /// Add one OSSL function name to the project allowlist.
    ///
    /// This is a project expectation, not a query of a simulator's live security configuration.
    pub fn add_ossl_function(&mut self, function: &str) -> Result<(), String> {
        if !function.starts_with("os")
            || function.len() <= 2
            || !function
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(format!(
                "invalid OSSL function '{function}' (expected an os-prefixed identifier)"
            ));
        }
        self.ossl_allowlist.add(function.to_owned());
        Ok(())
    }

    /// Validate target-dependent configuration after CLI overrides have been applied.
    pub fn validate_target_settings(&self) -> Result<(), String> {
        if !self.ossl_allowlist.is_explicit() {
            return Ok(());
        }
        if !self.target.is_opensim() {
            return Err(
                "ossl_function entries require an OpenSim target (opensim-xengine or opensim-yengine)"
                    .to_owned(),
            );
        }
        let catalogue = crate::builtins::ossl_functions_for_target(self.target)
            .expect("OpenSim targets have an OSSL interface catalogue");
        let mut functions: Vec<_> = self.ossl_allowlist.functions().iter().collect();
        functions.sort();
        for function in functions {
            if !catalogue.contains_key(function) {
                return Err(format!(
                    "OSSL function '{function}' is not present in the selected {} interface",
                    self.target.runtime().as_str()
                ));
            }
        }
        Ok(())
    }

    /// Configured analysis engine, defaulting to [`AnalysisEngine::Ast`].
    ///
    /// A configuration without an `engine` setting selects AST.
    /// [`AnalysisEngine::Legacy`] must be selected explicitly to restore the
    /// historical Legacy pipeline.
    pub fn engine(&self) -> AnalysisEngine {
        self.engine
    }

    /// Enable or disable preprocessing (CLI override).
    pub fn set_preprocess(&mut self, enabled: bool) {
        self.preprocess = enabled;
        self.preprocess_explicit = true;
    }

    /// Override the dialect (CLI override).
    pub fn set_dialect(&mut self, dialect: PreprocessDialect) {
        self.dialect = dialect;
        self.dialect_explicit = true;
        if dialect == PreprocessDialect::Firestorm {
            self.preprocess = true;
            self.preprocess_explicit = true;
        }
    }

    /// Replace the include-directory list (CLI override).
    pub fn set_include_dirs(&mut self, dirs: Vec<PathBuf>) {
        self.include_dirs = dirs;
        self.include_dirs_explicit = true;
    }

    /// Override the analysis engine (CLI override).
    pub fn set_engine(&mut self, engine: AnalysisEngine) {
        self.engine = engine;
        self.engine_explicit = true;
    }

    /// Apply disabled-rule filtering and severity overrides to one diagnostic.
    ///
    /// Returns `false` when the rule is disabled. Otherwise updates the
    /// diagnostic's effective severity and returns `true`.
    pub fn apply_to_diagnostic(&self, diagnostic: &mut Diagnostic) -> bool {
        if self.disabled_rules.contains(diagnostic.code) {
            return false;
        }
        if let Some(severity) = self.severity_overrides.get(diagnostic.code) {
            diagnostic.severity = *severity;
        }
        true
    }

    /// Apply disabled-rule filtering and severity overrides in place.
    ///
    /// Disabled diagnostics are removed; remaining diagnostics are updated in
    /// their original order.
    pub fn apply(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.retain_mut(|diagnostic| self.apply_to_diagnostic(diagnostic));
    }

    /// Disabled rule codes as display suppression.
    ///
    /// The set combines config-file `disable =` values with command-line
    /// `--disable` values. Severity overrides are deliberately excluded because
    /// a downgraded rule still produces diagnostics and still counts toward the
    /// rendered-diagnostic cap.
    pub fn display_suppression(&self) -> DisplaySuppression {
        DisplaySuppression::new(self.disabled_rules.iter().cloned())
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
    fn explicit_config_overlay_replaces_scalars_and_adds_rule_overrides() {
        let mut base = LintConfig::parse(
            "disable = SEM001\nseverity.SEM004 = info\nengine = legacy\npreprocess = true\ndialect = firestorm\ninclude_dir = base-inc\n",
        )
        .unwrap();
        let overlay = LintConfig::parse(
            "disable = SEM002\nseverity.SEM004 = warning\nengine = ast\npreprocess = false\ninclude_dir = script-inc\n",
        )
        .unwrap();

        base.apply_overrides_from(&overlay);

        assert_eq!(base.engine(), AnalysisEngine::Ast);
        assert!(!base.preprocess_enabled());
        assert_eq!(base.dialect(), PreprocessDialect::Firestorm);
        assert_eq!(base.include_dirs(), &[PathBuf::from("script-inc")]);
        let mut first = diagnostic("SEM001");
        let mut second = diagnostic("SEM002");
        let mut severity = diagnostic("SEM004");
        assert!(!base.apply_to_diagnostic(&mut first));
        assert!(!base.apply_to_diagnostic(&mut second));
        assert!(base.apply_to_diagnostic(&mut severity));
        assert_eq!(severity.severity, Severity::Warning);
    }

    #[test]
    fn firestorm_dialect_overlay_also_enables_preprocessing() {
        let mut base = LintConfig::parse("preprocess = false\n").unwrap();
        let overlay = LintConfig::parse("dialect = firestorm\n").unwrap();
        base.apply_overrides_from(&overlay);
        assert!(base.preprocess_enabled());
        assert_eq!(base.dialect(), PreprocessDialect::Firestorm);
    }

    #[test]
    fn config_overlay_inherits_unspecified_values() {
        let mut base = LintConfig::parse(
            "engine = legacy\ntarget = opensim-yengine\nossl_allowlist = strict\nossl_function = osGetAgentIP\n",
        )
        .unwrap();
        let overlay = LintConfig::parse("disable = SEM001\n").unwrap();
        base.apply_overrides_from(&overlay);

        assert_eq!(base.engine(), AnalysisEngine::Legacy);
        assert_eq!(base.target().as_str(), "opensim-yengine");
        assert!(base.ossl_allowlist().is_explicit());
        assert!(base.ossl_allowlist().allows("osGetAgentIP"));

        let additive = LintConfig::parse("ossl_function = osTeleportAgent\n").unwrap();
        base.apply_overrides_from(&additive);
        assert!(base.ossl_allowlist().allows("osGetAgentIP"));
        assert!(base.ossl_allowlist().allows("osTeleportAgent"));

        let reset = LintConfig::parse("ossl_allowlist = strict\n").unwrap();
        base.apply_overrides_from(&reset);
        assert!(base.ossl_allowlist().is_explicit());
        assert!(!base.ossl_allowlist().allows("osGetAgentIP"));
        assert!(!base.ossl_allowlist().allows("osTeleportAgent"));
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
    fn applies_one_diagnostic_and_reports_when_rule_is_disabled() {
        let config = LintConfig::parse("disable = SEM001\nseverity.SEM002 = warning\n").unwrap();
        let mut disabled = diagnostic("SEM001");
        let mut downgraded = diagnostic("SEM002");

        assert!(!config.apply_to_diagnostic(&mut disabled));
        assert!(config.apply_to_diagnostic(&mut downgraded));
        assert_eq!(downgraded.severity, Severity::Warning);
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
    fn display_suppression_contains_disabled_rules_but_not_severity_overrides() {
        let mut config = LintConfig::parse("disable = SEM012\nseverity.SEM013 = info\n").unwrap();
        config.disable(vec!["SEM014".to_owned()]).unwrap();

        let suppression = config.display_suppression();
        assert!(!suppression.is_empty());
        assert!(suppression.is_suppressed("SEM012"));
        assert!(suppression.is_suppressed("SEM014"));
        assert!(!suppression.is_suppressed("SEM013"));
        assert!(!suppression.is_suppressed("SEM001"));
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

#[cfg(test)]
mod engine_config_tests {
    use super::*;

    #[test]
    fn default_engine_is_ast() {
        assert_eq!(LintConfig::default().engine(), AnalysisEngine::Ast);
        assert_eq!(
            LintConfig::parse("# no engine setting\n").unwrap().engine(),
            AnalysisEngine::Ast
        );
    }

    #[test]
    fn parses_each_engine_selector() {
        for (value, expected) in [
            ("legacy", AnalysisEngine::Legacy),
            ("ast", AnalysisEngine::Ast),
            ("compare", AnalysisEngine::Compare),
        ] {
            let config = LintConfig::parse(&format!("engine = {value}\n")).unwrap();
            assert_eq!(config.engine(), expected, "{value}");
        }
    }

    #[test]
    fn rejects_invalid_and_empty_engine_values_with_line_numbers() {
        let invalid = LintConfig::parse("disable = SEM001\nengine = faster\n").unwrap_err();
        assert_eq!(
            invalid,
            "line 2: invalid engine value 'faster' (expected legacy, ast or compare)"
        );

        let empty = LintConfig::parse("engine =\n").unwrap_err();
        assert_eq!(
            empty,
            "line 1: invalid engine value '' (expected legacy, ast or compare)"
        );
    }

    #[test]
    fn old_config_without_engine_parses_as_ast() {
        let config = LintConfig::parse("disable = SEM001\nseverity.SEM004 = warning\n").unwrap();
        assert_eq!(config.engine(), AnalysisEngine::Ast);
    }

    #[test]
    fn set_engine_overrides_the_configured_value() {
        let mut config = LintConfig::parse("engine = ast\n").unwrap();
        assert_eq!(config.engine(), AnalysisEngine::Ast);
        config.set_engine(AnalysisEngine::Compare);
        assert_eq!(config.engine(), AnalysisEngine::Compare);
    }
}

#[cfg(test)]
mod target_config_tests {
    use super::*;

    #[test]
    fn default_target_is_second_life_mono_and_is_not_explicit() {
        let config = LintConfig::default();
        assert_eq!(config.target(), TargetProfile::default());
        assert!(!config.target_explicit());
    }

    #[test]
    fn parses_target_profile_independently_from_dialect() {
        let config = LintConfig::parse("dialect = firestorm\ntarget = opensim-yengine\n").unwrap();
        assert_eq!(config.dialect(), PreprocessDialect::Firestorm);
        assert_eq!(config.target().as_str(), "opensim-yengine");
        assert!(config.target_explicit());
    }

    #[test]
    fn command_line_target_override_is_explicit() {
        let mut config = LintConfig::default();
        config.set_target(TargetProfile::parse("second-life-luau").unwrap());
        assert_eq!(config.target().as_str(), "second-life-luau");
        assert!(config.target_explicit());
    }

    #[test]
    fn parses_target_version_independently_of_setting_order() {
        let config =
            LintConfig::parse("target_version = 0.9.3.1\ntarget = opensim-yengine").unwrap();
        assert_eq!(
            config.target().opensim_api_version(),
            Some(OpenSimApiVersion::V0_9_3_1)
        );
    }

    #[test]
    fn rejects_target_versions_for_incompatible_profiles() {
        assert!(LintConfig::parse("target = opensim-xengine\ntarget_version = 0.9.3.0").is_err());
        assert!(LintConfig::parse("target = second-life-mono\ntarget_version = 0.9.2.2").is_err());
        assert!(LintConfig::parse("target_version = latest").is_err());
    }

    #[test]
    fn rejects_unknown_target_names() {
        assert!(LintConfig::parse("target = mono").is_err());
        assert!(LintConfig::parse("target = firestorm").is_err());
        assert!(LintConfig::parse("target = opensim-mono").is_err());
    }
}

#[cfg(test)]
mod ossl_config_tests {
    use super::*;

    #[test]
    fn parses_repeatable_ossl_allowlist_entries() {
        let config = LintConfig::parse(
            "target = opensim-yengine\nossl_function = osGetAgentIP\nossl_function = osTeleportAgent\n",
        )
        .unwrap();
        assert!(config.ossl_allowlist().is_explicit());
        assert!(config.ossl_allowlist().allows("osGetAgentIP"));
        assert!(config.ossl_allowlist().allows("osTeleportAgent"));
        assert!(config.validate_target_settings().is_ok());
    }

    #[test]
    fn rejects_ossl_allowlist_on_second_life_targets() {
        let config = LintConfig::parse("ossl_function = osGetAgentIP").unwrap();
        assert!(config.validate_target_settings().is_err());
    }

    #[test]
    fn rejects_ossl_names_not_in_the_selected_runtime_interface() {
        let config =
            LintConfig::parse("target = opensim-xengine\nossl_function = osAESDecrypt").unwrap();
        assert!(config.validate_target_settings().is_err());
    }

    #[test]
    fn supports_explicit_empty_strict_ossl_allowlist() {
        let config =
            LintConfig::parse("target = opensim-yengine\nossl_allowlist = strict").unwrap();
        assert!(config.ossl_allowlist().is_explicit());
        assert!(config.ossl_allowlist().functions().is_empty());
        assert!(config.validate_target_settings().is_ok());
    }

    #[test]
    fn rejects_malformed_ossl_names() {
        for value in ["", "llOwnerSay", "os-foo", "osFoo bar", "osé"] {
            assert!(LintConfig::parse(&format!("ossl_function = {value}")).is_err());
        }
    }
}

#[cfg(test)]
mod target_diagnostic_config_tests {
    use super::*;

    #[test]
    fn target_api_diagnostics_accept_standard_disable_and_severity_configuration() {
        let config = LintConfig::parse(
            "disable = SEM049,SEM050\nseverity.SEM049 = warning\nseverity.SEM050 = error\n",
        )
        .expect("target diagnostics are valid configurable rule codes");
        let _ = config;
    }
}
