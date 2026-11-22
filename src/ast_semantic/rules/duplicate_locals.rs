//! SEM007: duplicate variable declaration in one lexical scope.

use std::collections::HashMap;

use crate::diagnostic::Diagnostic;

use super::super::scopes::ScopeTree;
use super::semantic_error;

pub(crate) fn check(scopes: &ScopeTree) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for scope in scopes
        .scopes()
        .iter()
        .skip(scopes.root().index().saturating_add(1))
    {
        let mut seen: HashMap<&str, crate::diagnostic::Span> = HashMap::new();
        for symbol in &scope.symbols {
            if let Some(previous) = seen.insert(symbol.name.as_str(), symbol.span) {
                diagnostics.push(semantic_error(
                    "SEM007",
                    format!(
                        "duplicate variable '{}' in the same scope; first declared at {}:{}",
                        symbol.name, previous.line, previous.column
                    ),
                    symbol.span,
                ));
            }
        }
    }
    diagnostics
}
