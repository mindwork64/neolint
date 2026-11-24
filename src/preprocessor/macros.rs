//! Macro definitions and the per-run macro table.

use std::collections::BTreeMap;

use crate::preprocessor::token::{Piece, PieceKind, RawSpan};

/// One object-like or function-like macro definition.
#[derive(Debug, Clone)]
pub(crate) struct MacroDefinition {
    pub(crate) name: String,
    pub(crate) params: Option<Vec<String>>,
    pub(crate) variadic: bool,
    pub(crate) replacement: Vec<Piece>,
    pub(crate) definition_span: RawSpan,
}

impl MacroDefinition {
    pub(crate) fn is_function_like(&self) -> bool {
        self.params.is_some()
    }

    fn same_signature(&self, other: &Self) -> bool {
        if self.params != other.params || self.variadic != other.variadic {
            return false;
        }
        token_signature(&self.replacement) == token_signature(&other.replacement)
    }
}

fn token_signature(pieces: &[Piece]) -> Vec<&str> {
    pieces
        .iter()
        .filter(|piece| !matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment))
        .map(|piece| piece.text.as_str())
        .collect()
}

/// Result of a `#define`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefineOutcome {
    Defined,
    Identical,
    Conflict,
}

/// Per-preprocessing-run macro table.
#[derive(Debug, Default)]
pub(crate) struct MacroTable {
    macros: BTreeMap<String, MacroDefinition>,
}

impl MacroTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&MacroDefinition> {
        self.macros.get(name)
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.macros.contains_key(name)
    }

    pub(crate) fn define(&mut self, definition: MacroDefinition) -> DefineOutcome {
        if let Some(existing) = self.macros.get(&definition.name) {
            if existing.same_signature(&definition) {
                return DefineOutcome::Identical;
            }
            return DefineOutcome::Conflict;
        }
        self.macros.insert(definition.name.clone(), definition);
        DefineOutcome::Defined
    }

    pub(crate) fn undef(&mut self, name: &str) -> bool {
        self.macros.remove(name).is_some()
    }
}
