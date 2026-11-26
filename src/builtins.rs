//! Vendored LSL built-in signature metadata and lookup.
//!
//! `data/lsl-builtins.txt` is the generated signature snapshot and
//! `data/lsl-deprecated.txt` contains canonically deprecated function names.
//! The snapshots are parsed once into a process-wide [`LslDefinitions`] value.
//! Normal linting reads only these checked-in files and performs no network I/O.
//! See `data/README.md` for provenance and the separate upstream license.
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::target::TargetProfile;

const BUILTIN_SOURCE: &str = include_str!("../data/lsl-builtins.txt");
const DEPRECATED_SOURCE: &str = include_str!("../data/lsl-deprecated.txt");
const OPENSIM_XENGINE_API20_FUNCTIONS: &str =
    include_str!("../data/opensim/xengine-api20/functions.txt");
const OPENSIM_XENGINE_API20_OSSL: &str =
    include_str!("../data/opensim/xengine-api20/ossl-functions.txt");
const OPENSIM_XENGINE_API20_EVENTS: &str = include_str!("../data/opensim/xengine-api20/events.txt");
const OPENSIM_XENGINE_API20_CONSTANTS: &str =
    include_str!("../data/opensim/xengine-api20/constants.txt");

const OPENSIM_YENGINE_API20_FUNCTIONS: &str =
    include_str!("../data/opensim/yengine-api20/functions.txt");
const OPENSIM_YENGINE_API20_OSSL: &str =
    include_str!("../data/opensim/yengine-api20/ossl-functions.txt");
const OPENSIM_YENGINE_API20_EVENTS: &str = include_str!("../data/opensim/yengine-api20/events.txt");
const OPENSIM_YENGINE_API20_CONSTANTS: &str =
    include_str!("../data/opensim/yengine-api20/constants.txt");

const OPENSIM_YENGINE_API22_FUNCTIONS: &str =
    include_str!("../data/opensim/yengine-api22/functions.txt");
const OPENSIM_YENGINE_API22_OSSL: &str =
    include_str!("../data/opensim/yengine-api22/ossl-functions.txt");
const OPENSIM_YENGINE_API22_EVENTS: &str = include_str!("../data/opensim/yengine-api22/events.txt");
const OPENSIM_YENGINE_API22_CONSTANTS: &str =
    include_str!("../data/opensim/yengine-api22/constants.txt");

const OPENSIM_YENGINE_API23_FUNCTIONS: &str =
    include_str!("../data/opensim/yengine-api23/functions.txt");
const OPENSIM_YENGINE_API23_OSSL: &str =
    include_str!("../data/opensim/yengine-api23/ossl-functions.txt");
const OPENSIM_YENGINE_API23_EVENTS: &str = include_str!("../data/opensim/yengine-api23/events.txt");
const OPENSIM_YENGINE_API23_CONSTANTS: &str =
    include_str!("../data/opensim/yengine-api23/constants.txt");

const OPENSIM_YENGINE_API24_FUNCTIONS: &str =
    include_str!("../data/opensim/yengine-api24/functions.txt");
const OPENSIM_YENGINE_API24_OSSL: &str =
    include_str!("../data/opensim/yengine-api24/ossl-functions.txt");
const OPENSIM_YENGINE_API24_EVENTS: &str = include_str!("../data/opensim/yengine-api24/events.txt");
const OPENSIM_YENGINE_API24_CONSTANTS: &str =
    include_str!("../data/opensim/yengine-api24/constants.txt");

/// One parameter of a canonical built-in function or event signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    /// LSL type name (`integer`, `string`, etc.).
    pub type_name: String,
    /// Parameter name from the vendored metadata.
    pub name: String,
}

/// Canonical signature for a built-in function or event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// Canonical function/event name.
    pub name: String,
    /// Return type, or `event` for an event signature.
    pub return_type: String,
    /// Ordered parameter list.
    pub parameters: Vec<Parameter>,
}

/// Canonical built-in constant definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantDefinition {
    /// Constant name.
    pub name: String,
    /// Declared LSL type.
    pub type_name: String,
    /// Canonical literal value text.
    pub value: String,
}

/// Read-only index of vendored LSL definitions.
#[derive(Debug, Default)]
pub struct LslDefinitions {
    /// Built-in function signatures keyed by canonical name.
    pub functions: HashMap<String, Signature>,
    /// Event signatures keyed by canonical name.
    pub events: HashMap<String, Signature>,
    /// Built-in constants keyed by name.
    pub constants: HashMap<String, ConstantDefinition>,
    /// Canonical deprecated function names.
    pub deprecated_functions: HashSet<String>,
}

/// Function catalogue with every supported overload retained.
pub type FunctionCatalog = HashMap<String, Vec<Signature>>;

/// Runtime-specific API metadata selected by a TargetProfile.
///
/// functions, events and constants describe the standard LSL API.
/// ossl_functions is separate because OSSL availability is controlled by
/// target region security settings, not only by the simulator build.
#[derive(Debug, Default)]
pub struct TargetApiCatalog {
    /// Standard LSL function signatures, including overloads.
    pub functions: FunctionCatalog,
    /// Event handler signatures accepted by the target compiler.
    pub events: HashMap<String, Signature>,
    /// Constants exposed by the target compiler/runtime.
    pub constants: HashMap<String, ConstantDefinition>,
    /// OSSL function signatures exposed by this OpenSim source snapshot.
    pub ossl_functions: FunctionCatalog,
    /// All callable built-ins (standard LSL plus OSSL, where available).
    pub callable_functions: FunctionCatalog,
}

static DEFINITIONS: OnceLock<LslDefinitions> = OnceLock::new();
static SECOND_LIFE_CATALOG: OnceLock<TargetApiCatalog> = OnceLock::new();
static OPENSIM_XENGINE_API20: OnceLock<TargetApiCatalog> = OnceLock::new();
static OPENSIM_YENGINE_API20: OnceLock<TargetApiCatalog> = OnceLock::new();
static OPENSIM_YENGINE_API22: OnceLock<TargetApiCatalog> = OnceLock::new();
static OPENSIM_YENGINE_API23: OnceLock<TargetApiCatalog> = OnceLock::new();
static OPENSIM_YENGINE_API24: OnceLock<TargetApiCatalog> = OnceLock::new();

/// Return the built-in LSL definitions vendored in this build.
///
/// The data is generated by the official secondlife/lsl-definitions project.
/// It is parsed once and then shared read-only for the process lifetime.
pub fn definitions() -> &'static LslDefinitions {
    DEFINITIONS.get_or_init(|| parse_definitions(BUILTIN_SOURCE))
}

/// Return all semantic API metadata for the selected platform/runtime/version.
pub fn catalog_for_target(target: TargetProfile) -> &'static TargetApiCatalog {
    use crate::target::{OpenSimApiVersion, ScriptRuntime};

    match target.runtime() {
        ScriptRuntime::Mono | ScriptRuntime::Lso | ScriptRuntime::Luau => {
            SECOND_LIFE_CATALOG.get_or_init(canonical_catalog)
        }
        ScriptRuntime::XEngine => match target.opensim_api_version() {
            Some(OpenSimApiVersion::V0_9_2_2) => OPENSIM_XENGINE_API20.get_or_init(|| {
                parse_target_catalog(
                    OPENSIM_XENGINE_API20_FUNCTIONS,
                    OPENSIM_XENGINE_API20_EVENTS,
                    OPENSIM_XENGINE_API20_CONSTANTS,
                    OPENSIM_XENGINE_API20_OSSL,
                )
            }),
            _ => unreachable!("TargetProfile prevents unsupported XEngine API versions"),
        },
        ScriptRuntime::YEngine => match target.opensim_api_version() {
            Some(OpenSimApiVersion::V0_9_2_2) => OPENSIM_YENGINE_API20.get_or_init(|| {
                parse_target_catalog(
                    OPENSIM_YENGINE_API20_FUNCTIONS,
                    OPENSIM_YENGINE_API20_EVENTS,
                    OPENSIM_YENGINE_API20_CONSTANTS,
                    OPENSIM_YENGINE_API20_OSSL,
                )
            }),
            Some(OpenSimApiVersion::V0_9_3_0) => OPENSIM_YENGINE_API22.get_or_init(|| {
                parse_target_catalog(
                    OPENSIM_YENGINE_API22_FUNCTIONS,
                    OPENSIM_YENGINE_API22_EVENTS,
                    OPENSIM_YENGINE_API22_CONSTANTS,
                    OPENSIM_YENGINE_API22_OSSL,
                )
            }),
            Some(OpenSimApiVersion::V0_9_3_1) => OPENSIM_YENGINE_API23.get_or_init(|| {
                parse_target_catalog(
                    OPENSIM_YENGINE_API23_FUNCTIONS,
                    OPENSIM_YENGINE_API23_EVENTS,
                    OPENSIM_YENGINE_API23_CONSTANTS,
                    OPENSIM_YENGINE_API23_OSSL,
                )
            }),
            Some(OpenSimApiVersion::Master2026_11_25) => OPENSIM_YENGINE_API24.get_or_init(|| {
                parse_target_catalog(
                    OPENSIM_YENGINE_API24_FUNCTIONS,
                    OPENSIM_YENGINE_API24_EVENTS,
                    OPENSIM_YENGINE_API24_CONSTANTS,
                    OPENSIM_YENGINE_API24_OSSL,
                )
            }),
            None => unreachable!("OpenSim TargetProfile always stores an API version"),
        },
    }
}

/// Return all standard LSL function signatures for a target.
pub fn functions_for_target(target: TargetProfile) -> &'static FunctionCatalog {
    &catalog_for_target(target).functions
}

/// Return every callable function in the selected runtime, including OSSL.
pub fn callable_functions_for_target(target: TargetProfile) -> &'static FunctionCatalog {
    &catalog_for_target(target).callable_functions
}

/// Return known OSSL overloads for an OpenSim target.
pub fn ossl_functions_for_target(target: TargetProfile) -> Option<&'static FunctionCatalog> {
    target
        .is_opensim()
        .then(|| &catalog_for_target(target).ossl_functions)
}

fn canonical_catalog() -> TargetApiCatalog {
    let definitions = definitions();
    TargetApiCatalog {
        functions: definitions
            .functions
            .iter()
            .map(|(name, signature)| (name.clone(), vec![signature.clone()]))
            .collect(),
        events: definitions.events.clone(),
        constants: definitions.constants.clone(),
        ossl_functions: HashMap::new(),
        callable_functions: definitions
            .functions
            .iter()
            .map(|(name, signature)| (name.clone(), vec![signature.clone()]))
            .collect(),
    }
}

fn parse_target_catalog(
    functions: &str,
    events: &str,
    constants: &str,
    ossl_functions: &str,
) -> TargetApiCatalog {
    let functions = parse_function_catalog(functions);
    let ossl_functions = parse_function_catalog(ossl_functions);
    let mut callable_functions = functions.clone();
    for (name, signatures) in &ossl_functions {
        callable_functions
            .entry(name.clone())
            .or_default()
            .extend(signatures.iter().cloned());
    }
    for signatures in callable_functions.values_mut() {
        signatures.sort_by(|left, right| {
            left.parameters
                .iter()
                .map(|parameter| parameter.type_name.as_str())
                .cmp(
                    right
                        .parameters
                        .iter()
                        .map(|parameter| parameter.type_name.as_str()),
                )
                .then_with(|| left.return_type.cmp(&right.return_type))
        });
        signatures.dedup();
    }
    TargetApiCatalog {
        functions,
        events: parse_event_catalog(events),
        constants: parse_constant_catalog(constants),
        ossl_functions,
        callable_functions,
    }
}

fn parse_function_catalog(source: &str) -> FunctionCatalog {
    let mut catalog: FunctionCatalog = HashMap::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let Some(signature) = parse_signature_line(line, false) else {
            continue;
        };
        catalog
            .entry(signature.name.clone())
            .or_default()
            .push(signature);
    }
    for signatures in catalog.values_mut() {
        signatures.sort_by(|left, right| {
            left.parameters
                .iter()
                .map(|parameter| parameter.type_name.as_str())
                .cmp(
                    right
                        .parameters
                        .iter()
                        .map(|parameter| parameter.type_name.as_str()),
                )
                .then_with(|| left.return_type.cmp(&right.return_type))
        });
        signatures.dedup();
    }
    catalog
}

fn parse_event_catalog(source: &str) -> HashMap<String, Signature> {
    let mut catalog = HashMap::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if let Some(signature) = parse_signature_line(line, true) {
            catalog.insert(signature.name.clone(), signature);
        }
    }
    catalog
}

fn parse_constant_catalog(source: &str) -> HashMap<String, ConstantDefinition> {
    let mut catalog = HashMap::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if let Some(constant) = parse_constant(line) {
            catalog.insert(constant.name.clone(), constant);
        }
    }
    catalog
}

fn parse_signature_line(line: &str, event_only: bool) -> Option<Signature> {
    let (declaration, is_event) = if let Some(declaration) = line.strip_prefix("event ") {
        (declaration, true)
    } else {
        (line, false)
    };
    if event_only != is_event {
        return None;
    }
    let open = declaration.find('(')?;
    let close = declaration.rfind(')')?;
    if close < open {
        return None;
    }
    let head = declaration[..open].split_whitespace().collect::<Vec<_>>();
    let (return_type, name) = if is_event {
        ("event", *head.first()?)
    } else {
        (*head.first()?, *head.get(1)?)
    };
    Some(Signature {
        name: name.to_owned(),
        return_type: return_type.to_owned(),
        parameters: parse_parameters(declaration[open + 1..close].trim()),
    })
}

fn parse_definitions(source: &str) -> LslDefinitions {
    let mut definitions = LslDefinitions::default();

    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        if let Some(constant) = parse_constant(line) {
            definitions
                .constants
                .insert(constant.name.clone(), constant);
            continue;
        }

        let Some(open_paren) = line.find('(') else {
            continue;
        };
        let Some(close_paren) = line.rfind(')') else {
            continue;
        };
        if close_paren < open_paren {
            continue;
        }

        let declaration = line[..open_paren].trim();
        let arguments = line[open_paren + 1..close_paren].trim();
        let (is_event, return_type, name) =
            if let Some(event_declaration) = declaration.strip_prefix("event ") {
                let mut parts = event_declaration.split_whitespace();
                let Some(name) = parts.next() else {
                    continue;
                };
                (true, "event".to_owned(), name.to_owned())
            } else {
                let mut parts = declaration.split_whitespace();
                let (Some(return_type), Some(name)) = (parts.next(), parts.next()) else {
                    continue;
                };
                (false, return_type.to_owned(), name.to_owned())
            };

        let signature = Signature {
            name: name.clone(),
            return_type,
            parameters: parse_parameters(arguments),
        };

        if is_event {
            definitions.events.insert(name, signature);
        } else {
            definitions.functions.insert(name, signature);
        }
    }

    definitions.deprecated_functions = parse_deprecated_functions(DEPRECATED_SOURCE);
    definitions
}

fn parse_deprecated_functions(source: &str) -> HashSet<String> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn parse_constant(line: &str) -> Option<ConstantDefinition> {
    let declaration = line.strip_prefix("const ")?;
    let (left, value) = declaration.split_once('=')?;
    let mut parts = left.split_whitespace();
    let type_name = parts.next()?.to_owned();
    let name = parts.next()?.to_owned();

    Some(ConstantDefinition {
        name,
        type_name,
        value: value.trim().to_owned(),
    })
}

fn parse_parameters(source: &str) -> Vec<Parameter> {
    if source.is_empty() {
        return Vec::new();
    }

    source
        .split(',')
        .filter_map(|parameter| {
            let mut parts = parameter.split_whitespace();
            Some(Parameter {
                type_name: parts.next()?.to_owned(),
                name: parts.next()?.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::OpenSimApiVersion;

    #[test]
    fn parses_canonical_function_signatures() {
        let definitions = definitions();
        let owner_say = definitions.functions.get("llOwnerSay").unwrap();

        assert_eq!(owner_say.return_type, "void");
        assert_eq!(
            owner_say.parameters,
            vec![Parameter {
                type_name: "string".to_owned(),
                name: "msg".to_owned(),
            }]
        );

        let get_pos = definitions.functions.get("llGetPos").unwrap();
        assert_eq!(get_pos.return_type, "vector");
        assert!(get_pos.parameters.is_empty());
    }

    #[test]
    fn parses_event_signatures_and_constants() {
        let definitions = definitions();
        let listen = definitions.events.get("listen").unwrap();

        assert_eq!(listen.parameters.len(), 4);
        assert_eq!(listen.parameters[0].type_name, "integer");
        assert_eq!(listen.parameters[2].type_name, "key");

        let true_constant = definitions.constants.get("TRUE").unwrap();
        assert_eq!(true_constant.type_name, "integer");
        assert_eq!(true_constant.value, "1");
    }

    #[test]
    fn pinned_snapshot_has_expected_definition_counts() {
        let definitions = definitions();
        assert_eq!(definitions.functions.len(), 529);
        assert_eq!(definitions.events.len(), 43);
        assert_eq!(definitions.constants.len(), 1082);
    }

    #[test]
    fn parses_only_canonical_lsl_deprecation_metadata() {
        let definitions = definitions();
        assert_eq!(definitions.deprecated_functions.len(), 28);
        assert!(definitions.deprecated_functions.contains("llCloud"));
        assert!(definitions.deprecated_functions.contains("llSetPrimURL"));
        assert!(definitions
            .deprecated_functions
            .iter()
            .all(|name| definitions.functions.contains_key(name)));
        assert!(!definitions.deprecated_functions.contains("ll.GetPos"));
    }

    #[test]
    fn versioned_target_catalogues_match_pinned_opensim_interfaces() {
        let xengine = catalog_for_target(TargetProfile::parse("opensim-xengine").unwrap());
        let yengine24 = catalog_for_target(TargetProfile::parse("opensim-yengine").unwrap());
        assert_eq!(xengine.functions.len(), 425);
        assert_eq!(xengine.events.len(), 35);
        assert_eq!(xengine.constants.len(), 770);
        assert_eq!(xengine.ossl_functions.len(), 231);
        assert_eq!(yengine24.functions.len(), 473);
        assert_eq!(yengine24.events.len(), 37);
        assert_eq!(yengine24.constants.len(), 888);
        assert_eq!(yengine24.ossl_functions.len(), 265);
        assert_eq!(yengine24.callable_functions.len(), 738);
        assert!(yengine24.callable_functions.contains_key("llOwnerSay"));
        assert!(yengine24.callable_functions.contains_key("osGetAgentIP"));
        assert!(xengine.functions.contains_key("llOwnerSay"));
        assert!(!xengine.functions.contains_key("llGetCameraFOV"));
        assert!(yengine24.functions.contains_key("llGetCameraFOV"));
        assert!(yengine24.events.contains_key("linkset_data"));
        assert!(!yengine24.events.contains_key("region_cross"));
        assert!(!xengine.events.contains_key("path_update"));
        assert!(!xengine.events.contains_key("linkset_data"));
        assert_eq!(xengine.constants["OS_APIVERSION"].value, "20");
        assert_eq!(yengine24.constants["OS_APIVERSION"].value, "24");
    }

    #[test]
    fn api20_api22_api23_and_api24_snapshots_are_not_conflated() {
        let profile = TargetProfile::parse("opensim-yengine").unwrap();
        let api20 = catalog_for_target(
            profile
                .with_opensim_api_version(OpenSimApiVersion::V0_9_2_2)
                .unwrap(),
        );
        let api22 = catalog_for_target(
            profile
                .with_opensim_api_version(OpenSimApiVersion::V0_9_3_0)
                .unwrap(),
        );
        let api23 = catalog_for_target(
            profile
                .with_opensim_api_version(OpenSimApiVersion::V0_9_3_1)
                .unwrap(),
        );
        let api24 = catalog_for_target(profile);
        assert_eq!(
            (
                api20.functions.len(),
                api20.constants.len(),
                api20.events.len()
            ),
            (425, 770, 36)
        );
        assert_eq!(
            (
                api22.functions.len(),
                api22.constants.len(),
                api22.events.len()
            ),
            (434, 780, 36)
        );
        assert_eq!(
            (
                api23.functions.len(),
                api23.constants.len(),
                api23.events.len()
            ),
            (457, 815, 37)
        );
        assert_eq!(
            (
                api24.functions.len(),
                api24.constants.len(),
                api24.events.len()
            ),
            (473, 888, 37)
        );
        assert!(!api20.events.contains_key("linkset_data"));
        assert!(!api22.events.contains_key("linkset_data"));
        assert!(api23.events.contains_key("linkset_data"));
        assert!(api24.events.contains_key("linkset_data"));
    }

    #[test]
    fn overload_signatures_are_retained_for_lsl_and_ossl() {
        let yengine = catalog_for_target(TargetProfile::parse("opensim-yengine").unwrap());
        assert_eq!(yengine.functions["llLinkPlaySound"].len(), 2);
        assert_eq!(yengine.ossl_functions["osApproxEquals"].len(), 6);
        assert_eq!(yengine.ossl_functions["osSlerp"].len(), 2);
        assert_eq!(yengine.ossl_functions["osTeleportAgent"].len(), 3);
    }

    #[test]
    fn target_catalogue_constants_include_opensim_extensions_and_values() {
        let yengine = catalog_for_target(TargetProfile::parse("opensim-yengine").unwrap());
        assert!(yengine.constants.contains_key("OS_APIVERSION"));
        assert_eq!(yengine.constants["OS_APIVERSION"].value, "24");
    }

    #[test]
    fn second_life_profiles_keep_the_full_canonical_function_catalogue() {
        let canonical = definitions().functions.len();
        for target in ["second-life-mono", "second-life-lso", "second-life-luau"] {
            assert_eq!(
                functions_for_target(TargetProfile::parse(target).unwrap()).len(),
                canonical
            );
        }
    }

    #[test]
    fn ossl_catalogues_are_distinct_from_standard_lsl_and_runtime_specific() {
        let xengine =
            ossl_functions_for_target(TargetProfile::parse("opensim-xengine").unwrap()).unwrap();
        let yengine =
            ossl_functions_for_target(TargetProfile::parse("opensim-yengine").unwrap()).unwrap();
        assert_eq!(xengine.len(), 231);
        assert_eq!(yengine.len(), 265);
        assert!(xengine.contains_key("osGetAgentIP"));
        assert!(yengine.contains_key("osGetAgentIP"));
        assert!(!xengine.contains_key("osAESDecrypt"));
        assert!(yengine.contains_key("osAESDecrypt"));
        assert!(!xengine.contains_key("osDefinitelyNotAFunction"));
        assert!(ossl_functions_for_target(TargetProfile::default()).is_none());
    }

    #[test]
    fn does_not_mix_slua_signatures_into_lsl() {
        let definitions = definitions();
        assert!(!definitions.functions.contains_key("ll.GetPos"));
        assert!(!definitions.functions.contains_key("ll_print"));
    }
}
