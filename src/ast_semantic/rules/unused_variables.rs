//! SEM024: variables declared but never read.
//!
//! The rule is syntactic: a declaration is used when at least one resolved
//! identifier occurrence reads it. It does not attempt runtime liveness.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::super::scopes::{ScopeTree, SymbolKind};
use super::super::usage::{symbol_key, SymbolKey};

pub(crate) fn check(
    scopes: &ScopeTree,
    used: &HashSet<SymbolKey>,
    reliable: bool,
) -> Vec<Diagnostic> {
    if !reliable {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for scope in scopes.scopes() {
        for symbol in &scope.symbols {
            if !matches!(symbol.kind, SymbolKind::GlobalVariable | SymbolKind::Local) {
                continue;
            }
            if used.contains(&symbol_key(symbol)) {
                continue;
            }
            diagnostics.push(Diagnostic {
                code: "SEM024",
                severity: Severity::Warning,
                kind: DiagnosticKind::Lint,
                message: format!("variable '{}' is declared but never read", symbol.name),
                span: symbol.span,
            });
        }
    }
    diagnostics
}
