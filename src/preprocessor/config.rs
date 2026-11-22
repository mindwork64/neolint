//! Preprocessor configuration.
//!
//! Phase 1 implements only the identity mode. Directives, macros, includes,
//! dialects, and extension switches belong to later phases.

/// Configuration for the identity preprocessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreprocessConfig;

impl PreprocessConfig {
    /// Identity configuration.
    pub const fn identity() -> Self {
        Self
    }
}
