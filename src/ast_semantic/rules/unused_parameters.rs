//! SEM025: parameters declared but never read.
//!
//! The rule is syntactic, like SEM024: a parameter is used when at least one
//! resolved identifier occurrence reads it. The scope tree registers parameters
//! only for user functions and canonical events, so unknown extension-event
//! parameters are structurally excluded rather than special-cased here.

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
            if symbol.kind != SymbolKind::Parameter {
                continue;
            }
            if used.contains(&symbol_key(symbol)) {
                continue;
            }
            diagnostics.push(Diagnostic {
                code: "SEM025",
                severity: Severity::Warning,
                kind: DiagnosticKind::Lint,
                message: format!("parameter '{}' is never read", symbol.name),
                span: symbol.span,
            });
        }
    }
    diagnostics
}
