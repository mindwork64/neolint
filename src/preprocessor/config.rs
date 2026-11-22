//! Preprocessor configuration.
//!
//! Identity mode is the default and preserves the Phase 1 byte-identical
//! behavior. Full mode enables the directive and macro engine; include
//! directories and the dialect profile are explicit opt-ins.

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
    pub fn full() -> PreprocessOptions {
        PreprocessOptions::full()
    }
}

/// Explicit preprocessing settings passed to the Phase 3 entry points.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreprocessOptions {
    mode: PreprocessMode,
    dialect: PreprocessDialect,
    include_dirs: Vec<PathBuf>,
}

impl PreprocessOptions {
    /// Identity options.
    pub fn identity() -> Self {
        Self::default()
    }

    /// Full LSL preprocessing with no include directories.
    pub fn full() -> Self {
        Self {
            mode: PreprocessMode::Full,
            dialect: PreprocessDialect::Lsl,
            include_dirs: Vec::new(),
        }
    }

    /// Full Firestorm profile preprocessing.
    pub fn firestorm() -> Self {
        Self {
            mode: PreprocessMode::Full,
            dialect: PreprocessDialect::Firestorm,
            include_dirs: Vec::new(),
        }
    }

    /// Selected mode.
    pub const fn mode(&self) -> PreprocessMode {
        self.mode
    }

    /// Selected dialect.
    pub const fn dialect(&self) -> PreprocessDialect {
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
    pub const fn with_dialect(mut self, dialect: PreprocessDialect) -> Self {
        self.dialect = dialect;
        self
    }

    /// Enable full preprocessing while retaining the selected dialect.
    pub const fn with_full(mut self) -> Self {
        self.mode = PreprocessMode::Full;
        self
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
        Self {
            mode,
            ..Self::identity()
        }
    }
}

impl From<PreprocessDialect> for PreprocessOptions {
    fn from(dialect: PreprocessDialect) -> Self {
        Self::firestorm().with_dialect(dialect)
    }
}
