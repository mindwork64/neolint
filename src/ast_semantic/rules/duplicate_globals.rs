//! SEM001: duplicate top-level global/function declarations.

use std::collections::HashMap;

use crate::ast::{Ast, TopLevel};
use crate::diagnostic::Diagnostic;

use super::semantic_error;

pub(crate) fn check(ast: &Ast) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut globals: HashMap<&str, crate::diagnostic::Span> = HashMap::new();
    for item in &ast.items {
        let (name, span) = match item {
            TopLevel::Global(global) => {
                // Legacy SEM001 registers only the first declarator of a global
                // declaration; later declarators are visible to ScopeModel but
                // are not part of the SEM001 duplicate scan.
                let Some(declarator) = global.declarators.first() else {
                    continue;
                };
                if declarator.name_error.is_some() || declarator.name.is_empty() {
                    continue;
                }
                (&declarator.name, declarator.name_span)
            }
            TopLevel::Function(function) => {
                if function.name.is_empty() {
                    continue;
                }
                (&function.name, function.name_span)
            }
            TopLevel::State(_) | TopLevel::Error(_) => continue,
        };
        if let Some(previous) = globals.get(name.as_str()) {
            diagnostics.push(semantic_error(
                "SEM001",
                format!(
                    "duplicate global declaration '{}'; first declared at {}:{}",
                    name, previous.line, previous.column
                ),
                span,
            ));
        } else {
            globals.insert(name.as_str(), span);
        }
    }
    diagnostics
}
