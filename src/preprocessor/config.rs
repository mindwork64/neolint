//! Preprocessor configuration.
//!
//! Identity mode is the default and preserves the Phase 1 byte-identical
//! behavior. Full mode enables the Phase 2 directive and macro engine.

/// Selects the preprocessing implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreprocessMode {
    /// Byte-identical copy with a single direct map segment.
    #[default]
    Identity,
    /// Directive and macro preprocessing.
    Full,
}

/// Identity configuration retained from Phase 1.
///
/// `PreprocessConfig` is still a unit value, so existing `PreprocessConfig`
/// expressions and `Default` continue to compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreprocessConfig;

impl PreprocessConfig {
    /// Identity configuration.
    pub const fn identity() -> Self {
        Self
    }

    /// Full Phase 2 directive and macro preprocessing.
    pub const fn full() -> PreprocessOptions {
        PreprocessOptions::full()
    }
}

/// Explicit preprocessing mode passed to [`crate::preprocessor::preprocess`].
///
/// `PreprocessConfig` converts into identity options, preserving existing
/// callers. New callers can use [`PreprocessOptions::full`] or
/// [`PreprocessConfig::full`].
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

    /// Full Phase 2 directive and macro options.
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
