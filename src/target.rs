//! Explicit target platform and script-runtime profiles.
//!
//! This is independent from preprocessing dialects such as Firestorm.

use std::collections::HashSet;

/// Platform on which an LSL script is intended to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetPlatform {
    /// Linden Lab Second Life grid.
    SecondLife,
    /// OpenSimulator grids.
    OpenSim,
}

/// Script runtime/compiler target exposed by a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptRuntime {
    /// Second Life Mono runtime.
    Mono,
    /// Original Second Life LSO bytecode runtime.
    Lso,
    /// Second Life's LSL-to-Luau compiler target.
    Luau,
    /// Historical OpenSimulator XEngine runtime, removed in OpenSim 0.9.3.0.
    XEngine,
    /// OpenSimulator YEngine runtime.
    YEngine,
}

impl ScriptRuntime {
    /// Stable runtime name used in diagnostics and documentation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::Lso => "LSO",
            Self::Luau => "Luau",
            Self::XEngine => "XEngine",
            Self::YEngine => "YEngine",
        }
    }
}

/// A validated platform/runtime combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetProfile {
    platform: TargetPlatform,
    runtime: ScriptRuntime,
}

impl Default for TargetProfile {
    fn default() -> Self {
        Self {
            platform: TargetPlatform::SecondLife,
            runtime: ScriptRuntime::Mono,
        }
    }
}

impl TargetProfile {
    /// Construct a profile if the runtime belongs to the selected platform.
    pub const fn new(platform: TargetPlatform, runtime: ScriptRuntime) -> Option<Self> {
        let compatible = matches!(
            (platform, runtime),
            (TargetPlatform::SecondLife, ScriptRuntime::Mono)
                | (TargetPlatform::SecondLife, ScriptRuntime::Lso)
                | (TargetPlatform::SecondLife, ScriptRuntime::Luau)
                | (TargetPlatform::OpenSim, ScriptRuntime::XEngine)
                | (TargetPlatform::OpenSim, ScriptRuntime::YEngine)
        );
        if compatible {
            Some(Self { platform, runtime })
        } else {
            None
        }
    }

    /// Parse a stable CLI/config selector.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "second-life-mono" => Self::new(TargetPlatform::SecondLife, ScriptRuntime::Mono),
            "second-life-lso" => Self::new(TargetPlatform::SecondLife, ScriptRuntime::Lso),
            "second-life-luau" => Self::new(TargetPlatform::SecondLife, ScriptRuntime::Luau),
            "opensim-xengine" => Self::new(TargetPlatform::OpenSim, ScriptRuntime::XEngine),
            "opensim-yengine" => Self::new(TargetPlatform::OpenSim, ScriptRuntime::YEngine),
            _ => None,
        }
    }

    /// Selected platform.
    pub const fn platform(self) -> TargetPlatform {
        self.platform
    }

    /// Selected runtime.
    pub const fn runtime(self) -> ScriptRuntime {
        self.runtime
    }

    /// Whether this target belongs to OpenSimulator.
    pub const fn is_opensim(self) -> bool {
        matches!(self.platform, TargetPlatform::OpenSim)
    }

    /// Stable lowercase CLI/config selector.
    pub fn as_str(self) -> &'static str {
        match (self.platform, self.runtime) {
            (TargetPlatform::SecondLife, ScriptRuntime::Mono) => "second-life-mono",
            (TargetPlatform::SecondLife, ScriptRuntime::Lso) => "second-life-lso",
            (TargetPlatform::SecondLife, ScriptRuntime::Luau) => "second-life-luau",
            (TargetPlatform::OpenSim, ScriptRuntime::XEngine) => "opensim-xengine",
            (TargetPlatform::OpenSim, ScriptRuntime::YEngine) => "opensim-yengine",
            _ => unreachable!("TargetProfile stores only compatible pairs"),
        }
    }

    /// Whether verified Mono-specific state-change diagnostics apply.
    ///
    /// Other runtimes remain unchecked until their behavior is confirmed.
    pub const fn supports_mono_state_change_checks(self) -> bool {
        matches!(
            (self.platform, self.runtime),
            (TargetPlatform::SecondLife, ScriptRuntime::Mono)
        )
    }
}

/// Explicit project-side list of OSSL functions expected to be enabled by a region.
///
/// OpenSim's OSSL interface is not a promise that each function is enabled.
/// Region security configuration and threat levels decide actual availability.
/// This allowlist records the functions that the project expects its target
/// region to expose; it is intentionally empty unless configured by the user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsslAllowlist {
    explicit: bool,
    functions: HashSet<String>,
}

impl OsslAllowlist {
    /// Enable checking with an initially empty allowlist.
    ///
    /// In strict mode, every known OSSL call is outside the allowlist until
    /// explicitly added. Calling this method does not clear existing entries.
    pub fn enable(&mut self) {
        self.explicit = true;
    }

    /// Mark the allowlist explicit and add one function name.
    pub fn add(&mut self, function: impl Into<String>) {
        self.explicit = true;
        self.functions.insert(function.into());
    }

    /// Whether OSSL checking was explicitly enabled, including an empty allowlist.
    pub const fn is_explicit(&self) -> bool {
        self.explicit
    }

    /// Function names that the project expects to be enabled on its region.
    pub fn functions(&self) -> &HashSet<String> {
        &self.functions
    }

    /// Whether the configured project allowlist includes one function.
    pub fn allows(&self, function: &str) -> bool {
        self.functions.contains(function)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preserves_second_life_mono() {
        let target = TargetProfile::default();
        assert_eq!(target.platform(), TargetPlatform::SecondLife);
        assert_eq!(target.runtime(), ScriptRuntime::Mono);
        assert_eq!(target.as_str(), "second-life-mono");
        assert!(target.supports_mono_state_change_checks());
    }

    #[test]
    fn all_profiles_round_trip() {
        for selector in [
            "second-life-mono",
            "second-life-lso",
            "second-life-luau",
            "opensim-xengine",
            "opensim-yengine",
        ] {
            let target = TargetProfile::parse(selector).unwrap();
            assert_eq!(target.as_str(), selector);
            assert_eq!(TargetProfile::parse(target.as_str()), Some(target));
        }
    }

    #[test]
    fn invalid_platform_runtime_pairs_are_rejected() {
        assert!(TargetProfile::new(TargetPlatform::SecondLife, ScriptRuntime::XEngine).is_none());
        assert!(TargetProfile::new(TargetPlatform::OpenSim, ScriptRuntime::Mono).is_none());
        assert!(TargetProfile::new(TargetPlatform::OpenSim, ScriptRuntime::Luau).is_none());
        for invalid in [
            "",
            "lsl",
            "firestorm",
            "mono",
            "luau",
            "opensim",
            "Second-Life-Mono",
        ] {
            assert_eq!(TargetProfile::parse(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn ossl_allowlist_can_be_explicitly_empty_or_populated() {
        let mut allowlist = OsslAllowlist::default();
        assert!(!allowlist.is_explicit());
        assert!(allowlist.functions().is_empty());
        allowlist.add("osGetAgentIP");
        allowlist.add("osTeleportAgent");
        assert!(allowlist.is_explicit());
        assert!(allowlist.allows("osGetAgentIP"));
        assert!(!allowlist.allows("osGetAgentOwner"));
        assert_eq!(allowlist.functions().len(), 2);
        let mut strict_empty = OsslAllowlist::default();
        strict_empty.enable();
        assert!(strict_empty.is_explicit());
        assert!(strict_empty.functions().is_empty());
    }

    #[test]
    fn only_second_life_mono_uses_mono_specific_checks() {
        for selector in [
            "second-life-lso",
            "second-life-luau",
            "opensim-xengine",
            "opensim-yengine",
        ] {
            assert!(!TargetProfile::parse(selector)
                .unwrap()
                .supports_mono_state_change_checks());
        }
    }
}
