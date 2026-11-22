//! Confirmed, deterministic predefined macros.
//!
//! Phase 3 provides only `__FILE__` and `__LINE__`. Dynamic Boost.Wave-style
//! values (`__DATE__`, `__TIME__`, `__TIMESTAMP__`, `__COUNTER__`, ...) are
//! intentionally unsupported because they would depend on wall-clock or process
//! state.

use std::collections::HashMap;
use std::sync::Arc;

use crate::preprocessor::token::{PieceKind, RawSpan};
use crate::source::file::{SourceFile, SourceId};

/// File-name predefined macro.
pub const FILE_MACRO: &str = "__FILE__";
/// Line-number predefined macro.
pub const LINE_MACRO: &str = "__LINE__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuiltinExpansion {
    pub(crate) text: String,
    pub(crate) kind: PieceKind,
}

pub(crate) fn expand(
    name: &str,
    span: RawSpan,
    files: &HashMap<SourceId, Arc<SourceFile>>,
) -> Option<BuiltinExpansion> {
    match name {
        FILE_MACRO => {
            let file = files.get(&span.source)?;
            let escaped = file
                .display_name()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            Some(BuiltinExpansion {
                text: format!("\"{escaped}\""),
                kind: PieceKind::String,
            })
        }
        LINE_MACRO => {
            let file = files.get(&span.source)?;
            let line = file.line_index().location(span.start).ok()?.line;
            Some(BuiltinExpansion {
                text: line.to_string(),
                kind: PieceKind::Number,
            })
        }
        _ => None,
    }
}
