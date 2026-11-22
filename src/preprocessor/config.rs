//! Preprocessor configuration.
//!
//! [`PreprocessOptions`] is the compact Phase 2 mode selector. Phase 3
//! extensions (dialect and include directories) live in [`PreprocessSettings`]
//! so the Phase 2 `Copy`/`const` contract stays intact.

use std::path::PathBuf;

/// Selects the preprocessing implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreprocessMode {
    /// Byte-identical copy with a single direct map segment.
    #[default]
    Identity,
    /// Directive and macro preprocessing.
    Full,
}

/// Dialect profile for full preprocessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreprocessDialect {
    /// Standard LSL preprocessing.
    #[default]
    Lsl,
    /// Firestorm-compatible preprocessing profile.
    Firestorm,
}

/// Identity configuration retained from Phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreprocessConfig;

impl PreprocessConfig {
    /// Identity configuration.
    pub const fn identity() -> Self {
        Self
    }

    /// Full LSL directive and macro preprocessing.
    pub const fn full() -> PreprocessOptions {
        PreprocessOptions::full()
    }
}

/// Compact Phase 2 preprocessing mode.
///
/// This type remains `Copy` and its constructors/accessor remain `const`.
/// Phase 3 options are carried separately by [`PreprocessSettings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreprocessOptions {
    mode: PreprocessMode,
}

impl PreprocessOptions {
    /// Identity options.
    pub const fn identity() -> Self {
        Self {
            mode: PreprocessMode::Identity,
        }
    }

    /// Full LSL preprocessing.
    pub const fn full() -> Self {
        Self {
            mode: PreprocessMode::Full,
        }
    }

    /// Selected mode.
    pub const fn mode(self) -> PreprocessMode {
        self.mode
    }
}

/// Phase 3 preprocessing settings: the Phase 2 mode plus dialect and includes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreprocessSettings {
    options: PreprocessOptions,
    dialect: PreprocessDialect,
    include_dirs: Vec<PathBuf>,
    switches_enabled: bool,
    lazy_lists_enabled: bool,
}

impl PreprocessSettings {
    /// Identity settings.
    pub const fn identity() -> Self {
        Self {
            options: PreprocessOptions::identity(),
            dialect: PreprocessDialect::Lsl,
            include_dirs: Vec::new(),
            switches_enabled: false,
            lazy_lists_enabled: false,
        }
    }

    /// Full LSL preprocessing with no include directories.
    pub const fn full() -> Self {
        Self {
            options: PreprocessOptions::full(),
            dialect: PreprocessDialect::Lsl,
            include_dirs: Vec::new(),
            switches_enabled: false,
            lazy_lists_enabled: false,
        }
    }

    /// Full Firestorm profile preprocessing.
    pub const fn firestorm() -> Self {
        Self {
            options: PreprocessOptions::full(),
            dialect: PreprocessDialect::Firestorm,
            include_dirs: Vec::new(),
            switches_enabled: false,
            lazy_lists_enabled: false,
        }
    }

    /// Phase 2 mode selector.
    pub fn options(&self) -> PreprocessOptions {
        self.options
    }

    /// Selected mode.
    pub fn mode(&self) -> PreprocessMode {
        self.options.mode()
    }

    /// Selected dialect.
    pub fn dialect(&self) -> PreprocessDialect {
        self.dialect
    }

    /// Include directories in caller-supplied order.
    pub fn include_dirs(&self) -> &[PathBuf] {
        &self.include_dirs
    }

    /// Replace the include-directory list.
    pub fn with_include_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.include_dirs = dirs;
        self
    }

    /// Replace the dialect profile.
    pub fn with_dialect(mut self, dialect: PreprocessDialect) -> Self {
        self.dialect = dialect;
        self
    }

    /// Whether Firestorm switch parsing was requested.
    pub fn switches_enabled(&self) -> bool {
        self.switches_enabled
    }

    /// Whether Firestorm lazy-list parsing was requested.
    pub fn lazy_lists_enabled(&self) -> bool {
        self.lazy_lists_enabled
    }

    /// Enable or disable Firestorm switches.
    pub fn with_switches_enabled(mut self, enabled: bool) -> Self {
        self.switches_enabled = enabled;
        self
    }

    /// Enable or disable Firestorm lazy lists.
    pub fn with_lazy_lists_enabled(mut self, enabled: bool) -> Self {
        self.lazy_lists_enabled = enabled;
        self
    }

    /// Enable full preprocessing while retaining the selected dialect.
    pub fn with_full(mut self) -> Self {
        self.options = PreprocessOptions::full();
        self
    }
}

impl From<PreprocessOptions> for PreprocessSettings {
    fn from(options: PreprocessOptions) -> Self {
        Self {
            options,
            dialect: PreprocessDialect::Lsl,
            include_dirs: Vec::new(),
            switches_enabled: false,
            lazy_lists_enabled: false,
        }
    }
}

impl From<PreprocessConfig> for PreprocessOptions {
    fn from(_: PreprocessConfig) -> Self {
        Self::identity()
    }
}

impl From<&PreprocessConfig> for PreprocessOptions {
    fn from(_: &PreprocessConfig) -> Self {
        Self::identity()
    }
}

impl From<PreprocessMode> for PreprocessOptions {
    fn from(mode: PreprocessMode) -> Self {
        Self { mode }
    }
}

impl From<PreprocessDialect> for PreprocessSettings {
    fn from(dialect: PreprocessDialect) -> Self {
        PreprocessSettings::firestorm().with_dialect(dialect)
    }
}

impl From<PreprocessConfig> for PreprocessSettings {
    fn from(config: PreprocessConfig) -> Self {
        PreprocessOptions::from(config).into()
    }
}

impl From<&PreprocessConfig> for PreprocessSettings {
    fn from(config: &PreprocessConfig) -> Self {
        PreprocessOptions::from(config).into()
    }
}

impl From<PreprocessMode> for PreprocessSettings {
    fn from(mode: PreprocessMode) -> Self {
        PreprocessOptions::from(mode).into()
    }
}

#[cfg(test)]
mod phase2_api_compatibility_tests {
    use super::*;

    fn require_copy<T: Copy>() {}

    const IDENTITY: PreprocessOptions = PreprocessOptions::identity();
    const FULL: PreprocessOptions = PreprocessOptions::full();
    const FULL_MODE: PreprocessMode = PreprocessOptions::full().mode();
    const CONFIG_FULL: PreprocessOptions = PreprocessConfig::full();
    const FIRESTORM_SETTINGS: PreprocessSettings = PreprocessSettings::firestorm();

    #[test]
    fn phase2_options_remain_copy_and_const_constructible() {
        require_copy::<PreprocessOptions>();
        assert_eq!(IDENTITY.mode(), PreprocessMode::Identity);
        assert_eq!(FULL.mode(), PreprocessMode::Full);
        assert_eq!(FULL_MODE, PreprocessMode::Full);
        assert_eq!(CONFIG_FULL.mode(), PreprocessMode::Full);
        assert_eq!(FIRESTORM_SETTINGS.mode(), PreprocessMode::Full);
        assert_eq!(FIRESTORM_SETTINGS.dialect(), PreprocessDialect::Firestorm);
    }

    #[test]
    fn phase2_conversions_still_compile() {
        let from_mode: PreprocessOptions = PreprocessMode::Full.into();
        let from_config: PreprocessOptions = PreprocessConfig.into();
        let from_ref: PreprocessOptions = (&PreprocessConfig).into();
        assert_eq!(from_mode.mode(), PreprocessMode::Full);
        assert_eq!(from_config.mode(), PreprocessMode::Identity);
        assert_eq!(from_ref.mode(), PreprocessMode::Identity);
        let settings: PreprocessSettings = from_mode.into();
        assert_eq!(settings.mode(), PreprocessMode::Full);
    }

    #[test]
    fn phase3_settings_carry_dialect_and_include_directories() {
        let settings = PreprocessSettings::full()
            .with_dialect(PreprocessDialect::Firestorm)
            .with_include_dirs(vec![PathBuf::from("./includes")]);
        assert_eq!(settings.mode(), PreprocessMode::Full);
        assert_eq!(settings.dialect(), PreprocessDialect::Firestorm);
        assert_eq!(settings.include_dirs(), &[PathBuf::from("./includes")]);
        assert_eq!(
            PreprocessSettings::identity().dialect(),
            PreprocessDialect::Lsl
        );
        let firestorm: PreprocessSettings = PreprocessDialect::Firestorm.into();
        assert_eq!(firestorm.mode(), PreprocessMode::Full);
        assert_eq!(firestorm.dialect(), PreprocessDialect::Firestorm);
        let lsl: PreprocessSettings = PreprocessDialect::Lsl.into();
        assert_eq!(lsl.mode(), PreprocessMode::Full);
        assert_eq!(lsl.dialect(), PreprocessDialect::Lsl);
    }
}

#[cfg(test)]
mod phase5_feature_gate_tests {
    use super::*;

    #[test]
    fn switches_and_lazy_lists_are_independent_and_default_off() {
        let defaults = PreprocessSettings::firestorm();
        assert!(!defaults.switches_enabled());
        assert!(!defaults.lazy_lists_enabled());

        let switches = defaults.clone().with_switches_enabled(true);
        assert!(switches.switches_enabled());
        assert!(!switches.lazy_lists_enabled());

        let lazy_lists = defaults.clone().with_lazy_lists_enabled(true);
        assert!(!lazy_lists.switches_enabled());
        assert!(lazy_lists.lazy_lists_enabled());

        let both = defaults
            .with_switches_enabled(true)
            .with_lazy_lists_enabled(true);
        assert!(both.switches_enabled());
        assert!(both.lazy_lists_enabled());
    }
}
