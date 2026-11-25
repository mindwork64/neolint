use neolint::{
    AnalysisEngine, AnalysisRun, DisplaySuppression, EngineAnalysisOptions, OsslAllowlist,
    ScriptRuntime, TargetPlatform, TargetProfile,
};

fn codes(source: &str, target: TargetProfile, allowlist: &OsslAllowlist) -> Vec<String> {
    let mut run = AnalysisRun::new().unwrap();
    let index = run.add_prepared_text("target-profile.lsl", source).unwrap();
    let unit = run.unit(index).unwrap();
    let analysis = unit
        .analyze_with_target_options_suppressed(
            AnalysisEngine::Ast,
            &EngineAnalysisOptions::default(),
            target,
            allowlist,
            &DisplaySuppression::default(),
        )
        .unwrap();
    analysis
        .diagnostics()
        .iter()
        .map(|item| item.diagnostic().code.to_owned())
        .collect()
}

fn has_code(codes: &[String], code: &str) -> bool {
    codes.iter().any(|item| item == code)
}

#[test]
fn target_profile_selectors_are_platform_runtime_pairs() {
    let mono = TargetProfile::default();
    assert_eq!(mono.platform(), TargetPlatform::SecondLife);
    assert_eq!(mono.runtime(), ScriptRuntime::Mono);
    assert_eq!(mono.as_str(), "second-life-mono");

    assert_eq!(
        TargetProfile::new(TargetPlatform::SecondLife, ScriptRuntime::YEngine),
        None
    );
    assert_eq!(
        TargetProfile::new(TargetPlatform::OpenSim, ScriptRuntime::Luau),
        None
    );
    for selector in [
        "second-life-lso",
        "second-life-luau",
        "opensim-xengine",
        "opensim-yengine",
    ] {
        let target = TargetProfile::parse(selector).unwrap();
        assert_eq!(TargetProfile::parse(target.as_str()), Some(target));
    }
}

#[test]
fn opensim_csharp_key_to_string_api_parameters_accept_lsl_key_values() {
    let source = r#"
default {
    state_entry() {
        llListen(0, "name", llGetOwner(), "");
    }
}
"#;
    let yengine = codes(
        source,
        TargetProfile::parse("opensim-yengine").unwrap(),
        &OsslAllowlist::default(),
    );
    assert!(!has_code(&yengine, "SEM004"), "{yengine:?}");
    let mono = codes(source, TargetProfile::default(), &OsslAllowlist::default());
    assert!(!has_code(&mono, "SEM004"), "{mono:?}");
}

#[test]
fn opensim_profiles_report_second_life_functions_missing_from_the_runtime() {
    let source = r#"
helper() {
    llGetCameraFOV();
    llGetExperienceDetails(NULL_KEY);
    llTransferOwnership(NULL_KEY, 0, []);
    llOwnerSay("supported");
}
default { state_entry() { helper(); } }
"#;
    let no_ossl = OsslAllowlist::default();
    let xengine = codes(
        source,
        TargetProfile::parse("opensim-xengine").unwrap(),
        &no_ossl,
    );
    assert!(has_code(&xengine, "SEM047"), "{xengine:?}");

    let yengine = codes(
        source,
        TargetProfile::parse("opensim-yengine").unwrap(),
        &no_ossl,
    );
    assert!(has_code(&yengine, "SEM047"), "{yengine:?}");

    // The same source is a canonical Second Life API surface; SEM047 is
    // strictly an OpenSim compatibility diagnostic.
    let mono = codes(source, TargetProfile::default(), &no_ossl);
    assert!(!has_code(&mono, "SEM047"), "{mono:?}");
}

#[test]
fn expression_type_inference_uses_the_selected_runtime_builtin_catalogue() {
    let source = r#"
default {
    state_entry() {
        float value = llGetCameraFOV() + ZERO_VECTOR;
    }
}
"#;
    let xengine = codes(
        source,
        TargetProfile::parse("opensim-xengine").unwrap(),
        &OsslAllowlist::default(),
    );
    assert!(has_code(&xengine, "SEM047"), "{xengine:?}");
    assert!(!has_code(&xengine, "SEM046"), "{xengine:?}");

    let mono = codes(source, TargetProfile::default(), &OsslAllowlist::default());
    assert!(has_code(&mono, "SEM046"), "{mono:?}");
    assert!(!has_code(&mono, "SEM047"), "{mono:?}");
}

#[test]
fn yengine_only_lsl_functions_and_overloads_are_target_aware() {
    let three_argument_call = r#"
default {
    state_entry() {
        llGetCameraFOV();
        llLinkPlaySound(LINK_THIS, "bell", 1.0);
    }
}
"#;
    let yengine = codes(
        three_argument_call,
        TargetProfile::parse("opensim-yengine").unwrap(),
        &OsslAllowlist::default(),
    );
    assert!(!has_code(&yengine, "SEM047"), "{yengine:?}");
    assert!(!has_code(&yengine, "SEM002"), "{yengine:?}");

    let xengine = codes(
        three_argument_call,
        TargetProfile::parse("opensim-xengine").unwrap(),
        &OsslAllowlist::default(),
    );
    assert!(has_code(&xengine, "SEM047"), "{xengine:?}");

    let mono = codes(
        three_argument_call,
        TargetProfile::default(),
        &OsslAllowlist::default(),
    );
    assert!(has_code(&mono, "SEM002"), "{mono:?}");
}

#[test]
fn ossl_checks_require_an_explicit_allowlist_and_never_guess_region_security() {
    let source = r#"
default {
    state_entry() {
        osGetAgentIP(llGetOwner());
        osTeleportAgent(llGetOwner(), ZERO_VECTOR, ZERO_ROTATION);
        osDefinitelyNotAnOpenSimFunction();
    }
}
"#;
    let target = TargetProfile::parse("opensim-yengine").unwrap();
    let empty = OsslAllowlist::default();
    let without_allowlist = codes(source, target, &empty);
    assert!(
        !has_code(&without_allowlist, "SEM048"),
        "{without_allowlist:?}"
    );

    let mut strict_empty = OsslAllowlist::default();
    strict_empty.enable();
    let strict_empty_results = codes(source, target, &strict_empty);
    let strict_warnings = strict_empty_results
        .iter()
        .filter(|code| code.as_str() == "SEM048")
        .count();
    assert_eq!(strict_warnings, 2, "{strict_empty_results:?}");

    let mut allowlist = OsslAllowlist::default();
    allowlist.add("osGetAgentIP");
    let one_allowed = codes(source, target, &allowlist);
    assert!(has_code(&one_allowed, "SEM048"), "{one_allowed:?}");
    let sem048_count = one_allowed
        .iter()
        .filter(|code| code.as_str() == "SEM048")
        .count();
    assert_eq!(sem048_count, 1, "{one_allowed:?}");

    allowlist.add("osTeleportAgent");
    let both_allowed = codes(source, target, &allowlist);
    assert!(!has_code(&both_allowed, "SEM048"), "{both_allowed:?}");
}

#[test]
fn target_api_diagnostics_fail_closed_for_malformed_or_unreliable_calls() {
    let source = r#"
default {
    state_entry() {
        llGetExperienceDetails(NULL_KEY, );
        osGetAgentIP(llGetOwner(, ));
    }
}
"#;
    let mut allowlist = OsslAllowlist::default();
    allowlist.add("osGetAgentIP");
    let target = TargetProfile::parse("opensim-yengine").unwrap();
    let results = codes(source, target, &allowlist);
    assert!(!has_code(&results, "SEM047"), "{results:?}");
    assert!(!has_code(&results, "SEM048"), "{results:?}");
}

#[test]
fn ossl_allowlist_validates_against_the_selected_runtime_snapshot() {
    use neolint::config::LintConfig;

    let config = LintConfig::parse(
        "target = opensim-yengine\nossl_function = osGetAgentIP\nossl_function = osTeleportAgent\n",
    )
    .unwrap();
    assert!(config.validate_target_settings().is_ok());

    let old_runtime =
        LintConfig::parse("target = opensim-xengine\nossl_function = osAESDecrypt\n").unwrap();
    assert!(old_runtime.validate_target_settings().is_err());

    let wrong_platform = LintConfig::parse("ossl_function = osGetAgentIP").unwrap();
    assert!(wrong_platform.validate_target_settings().is_err());
}
