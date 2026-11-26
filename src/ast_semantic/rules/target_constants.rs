//! SEM050: canonical Second Life constants absent from the selected target API.

use std::collections::{HashMap, HashSet};

use crate::ast::ExpressionKind;
use crate::builtins::ConstantDefinition;
use crate::diagnostic::Diagnostic;
use crate::target::{TargetPlatform, TargetProfile};

use super::super::expressions::ExpressionSite;
use super::super::scopes::ScopeTree;
use super::semantic_error;

/// Report references to canonical SL constants that the selected OpenSim API does not expose.
///
/// A variable with the same spelling shadows the built-in constant and is not
/// reported. Parser errors and unreliable source preprocessing suppress the rule.
pub(crate) fn check(
    sites: &[ExpressionSite<'_>],
    scopes: &ScopeTree,
    canonical_constants: &HashMap<String, ConstantDefinition>,
    target_constants: &HashMap<String, ConstantDefinition>,
    target: TargetProfile,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if target.platform() != TargetPlatform::OpenSim
        || !environment_reliable
        || !syntax_diagnostics.is_empty()
    {
        return Vec::new();
    }

    let version = target
        .opensim_api_version()
        .expect("OpenSim target always has an API version");
    let mut reported = HashSet::new();
    let mut diagnostics = Vec::new();
    for site in sites {
        let ExpressionKind::Identifier(identifier) = &site.expression.kind else {
            continue;
        };
        if !canonical_constants.contains_key(identifier.name.as_str())
            || target_constants.contains_key(identifier.name.as_str())
            || scopes
                .lookup_value_at(site.scope, &identifier.name, identifier.span.start)
                .is_some()
            || !reported.insert((identifier.span.start, identifier.span.end))
        {
            continue;
        }
        diagnostics.push(semantic_error(
            "SEM050",
            format!(
                "constant '{}' is not defined by OpenSim {} (OS_APIVERSION {})",
                identifier.name,
                version.as_str(),
                version.api_version()
            ),
            identifier.span,
        ));
    }
    diagnostics
}
