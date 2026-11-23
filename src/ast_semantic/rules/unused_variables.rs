//! SEM024: variables declared but never read.
//!
//! The rule is syntactic: a declaration is used when at least one resolved
//! identifier occurrence reads it. It does not attempt runtime liveness.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::scopes::{ScopeTree, Symbol, SymbolKind};
use super::super::usage::{symbol_key, SymbolKey};

pub(crate) fn check(
    scopes: &ScopeTree,
    used: &HashSet<SymbolKey>,
    reliable: bool,
) -> Vec<Diagnostic> {
    if !reliable {
        return Vec::new();
    }

    let mut candidates: Vec<&Symbol> = scopes
        .scopes()
        .iter()
        .flat_map(|scope| scope.symbols.iter())
        .filter(|symbol| matches!(symbol.kind, SymbolKind::GlobalVariable | SymbolKind::Local))
        .filter(|symbol| !used.contains(&symbol_key(symbol)))
        .collect();
    // Scope creation order is not source order once nested blocks are present.
    candidates.sort_by_key(|symbol| symbol.span.start);

    candidates
        .into_iter()
        .map(|symbol| Diagnostic {
            code: "SEM024",
            severity: Severity::Warning,
            kind: DiagnosticKind::Lint,
            message: format!("variable '{}' is declared but never read", symbol.name),
            span: symbol.span,
        })
        .collect()
}
