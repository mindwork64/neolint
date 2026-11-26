use neolint::{
    AnalysisEngine, AnalysisRun, DisplaySuppression, EngineAnalysisOptions, OpenSimApiVersion,
    OsslAllowlist, ScriptRuntime, TargetPlatform, TargetProfile,
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
fn versioned_events_and_constants_produce_target_specific_diagnostics() {
    let yengine = TargetProfile::parse("opensim-yengine").unwrap();
    let api20 = yengine
        .with_opensim_api_version(OpenSimApiVersion::V0_9_2_2)
        .unwrap();
    let api22 = yengine
        .with_opensim_api_version(OpenSimApiVersion::V0_9_3_0)
        .unwrap();
    let api23 = yengine
        .with_opensim_api_version(OpenSimApiVersion::V0_9_3_1)
        .unwrap();
    let api24 = yengine;

    let linkset_data = r#"
default {
    linkset_data(integer action, string name, string value) { }
}
"#;
    for target in [api20, api22] {
        let results = codes(linkset_data, target, &OsslAllowlist::default());
        assert!(
            has_code(&results, "SEM049"),
            "{}: {results:?}",
            target.as_str()
        );
    }
    for target in [api23, api24] {
        let results = codes(linkset_data, target, &OsslAllowlist::default());
        assert!(
            !has_code(&results, "SEM049"),
            "{}: {results:?}",
            target.as_str()
        );
        assert!(
            !has_code(&results, "SEM003"),
            "{}: {results:?}",
            target.as_str()
        );
    }

    let handler_parameter_scope = r#"
default {
    linkset_data(integer action, string name, string value) {
        integer local = action;
    }
}
"#;
    let api23_scope = codes(handler_parameter_scope, api23, &OsslAllowlist::default());
    assert!(!has_code(&api23_scope, "SEM012"), "{api23_scope:?}");
    assert!(!has_code(&api23_scope, "SEM049"), "{api23_scope:?}");

    // A canonical SL event absent from every pinned OpenSim API snapshot.
    let experience_event = r#"
default {
    experience_permissions(key agent_id) { }
}
"#;
    for target in [api20, api22, api23, api24] {
        let results = codes(experience_event, target, &OsslAllowlist::default());
        assert!(
            has_code(&results, "SEM049"),
            "{}: {results:?}",
            target.as_str()
        );
    }

    // AGENT_AUTOMATED is a canonical SL constant but is not declared by the
    // pinned OpenSim constants API snapshots. It must not fall through to a
    // second, generic unresolved-name warning.
    let missing_constant = r#"
default {
    state_entry() {
        integer flags = AGENT_AUTOMATED;
    }
}
"#;
    for target in [api20, api22, api23, api24] {
        let results = codes(missing_constant, target, &OsslAllowlist::default());
        assert!(
            has_code(&results, "SEM050"),
            "{}: {results:?}",
            target.as_str()
        );
        assert!(
            !has_code(&results, "SEM012"),
            "{}: {results:?}",
            target.as_str()
        );
    }
    let second_life = codes(
        missing_constant,
        TargetProfile::default(),
        &OsslAllowlist::default(),
    );
    assert!(!has_code(&second_life, "SEM050"), "{second_life:?}");
}

#[test]
fn xengine_event_surface_uses_its_own_parser_not_the_yengine_handler_interface() {
    let source = r#"
default {
    path_update(integer type, list data) { }
    linkset_data(integer action, string name, string value) { }
}
"#;
    let xengine = codes(
        source,
        TargetProfile::parse("opensim-xengine").unwrap(),
        &OsslAllowlist::default(),
    );
    assert_eq!(
        xengine
            .iter()
            .filter(|code| code.as_str() == "SEM049")
            .count(),
        2,
        "{xengine:?}"
    );
}

#[test]
fn ossl_overloads_participate_in_arity_type_checks_and_return_type_inference() {
    let valid = r#"
default {
    state_entry() {
        integer float_match = osApproxEquals(1.0, 2.0, 0.01);
        integer vector_match = osApproxEquals(<0,0,0>, <1,1,1>);
        rotation rotation_result = osSlerp(ZERO_ROTATION, ZERO_ROTATION, 0.5);
        vector vector_result = osSlerp(ZERO_VECTOR, <1,1,1>, 0.5);
        osTeleportAgent(llGetOwner(), ZERO_VECTOR, ZERO_VECTOR);
    }
}
"#;
    let valid_results = codes(
        valid,
        TargetProfile::parse("opensim-yengine").unwrap(),
        &OsslAllowlist::default(),
    );
    for code in ["SEM002", "SEM004", "SEM027", "SEM046"] {
        assert!(!has_code(&valid_results, code), "{code}: {valid_results:?}");
    }

    let invalid = r#"
default {
    state_entry() {
        osApproxEquals(1, <1,1,1>, 0.01, 0.02);
        osApproxEquals("not-a-number", <1,1,1>);
    }
}
"#;
    let invalid_results = codes(
        invalid,
        TargetProfile::parse("opensim-yengine").unwrap(),
        &OsslAllowlist::default(),
    );
    assert!(has_code(&invalid_results, "SEM002"), "{invalid_results:?}");
    assert!(has_code(&invalid_results, "SEM004"), "{invalid_results:?}");
}

#[test]
fn target_event_and_constant_checks_fail_closed_for_recovered_input() {
    let target = TargetProfile::parse("opensim-yengine").unwrap();
    let mut allowlist = OsslAllowlist::default();
    allowlist.enable();
    let malformed = r#"
default {
    linkset_data(integer action, string name, string value { }
    state_entry() {
        integer flags = AGENT_AUTOMATED + ;
        osGetAgentIP(,);
    }
}
"#;
    let results = codes(malformed, target, &allowlist);
    assert!(!has_code(&results, "SEM049"), "{results:?}");
    assert!(!has_code(&results, "SEM050"), "{results:?}");
    assert!(!has_code(&results, "SEM048"), "{results:?}");
}

#[test]
fn cfg_constant_folding_uses_the_selected_opensim_constant_values() {
    let source = r#"
integer helper() {
    if (OS_APIVERSION == 24) {
        return 1;
    }
}
default { state_entry() { } }
"#;
    let api20 = TargetProfile::parse("opensim-yengine")
        .unwrap()
        .with_opensim_api_version(OpenSimApiVersion::V0_9_2_2)
        .unwrap();
    let api24 = TargetProfile::parse("opensim-yengine").unwrap();
    let api20_results = codes(source, api20, &OsslAllowlist::default());
    let api24_results = codes(source, api24, &OsslAllowlist::default());
    assert!(has_code(&api20_results, "SEM011"), "{api20_results:?}");
    assert!(!has_code(&api24_results, "SEM011"), "{api24_results:?}");
}

#[test]
fn target_constant_shadowing_does_not_report_a_missing_builtin() {
    let source = r#"
default {
    state_entry() {
        integer AGENT_AUTOMATED = 7;
        integer flags = AGENT_AUTOMATED;
    }
}
"#;
    let results = codes(
        source,
        TargetProfile::parse("opensim-yengine").unwrap(),
        &OsslAllowlist::default(),
    );
    assert!(!has_code(&results, "SEM050"), "{results:?}");
    assert!(!has_code(&results, "SEM012"), "{results:?}");
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
