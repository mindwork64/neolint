//! Identity preprocessing.
//!
//! Identity mode copies the root bytes unchanged and builds a single `Direct`
//! segment plus a real EOF anchor. It performs no directive, macro, include, or
//! dialect work.

use std::sync::Arc;

use crate::preprocessor::{IncludeGraph, IncludeNode, PreparedSource};
use crate::source::manager::SourceManager;
use crate::source::map::{PreparedSpan, SourceMapBuilder, SourceSpan, SpanError};
use crate::source::OriginKind;

/// Build the identity [`PreparedSource`] for a loaded root.
///
/// The returned text is byte-identical to the root. For non-empty input the map
/// has exactly one `Direct` segment covering `[0, len)`; empty input has no
/// segments and a `(0, 0)` EOF anchor.
pub fn preprocess_identity(
    manager: &SourceManager,
    root: crate::source::SourceId,
) -> Result<PreparedSource, SpanError> {
    let unit = manager.unit();
    if root.unit() != unit {
        return Err(SpanError::ForeignAnalysisUnit);
    }
    let file = manager.file(root).ok_or(SpanError::UnknownSourceId)?;
    let text = Arc::clone(file.shared_text());
    let len = text.len();
    let mut builder = SourceMapBuilder::new(unit, Arc::clone(&text), manager.files().to_vec())?;
    let root_span: SourceSpan = builder.source_span(root, 0, len)?;
    let origin = builder.push_origin(OriginKind::Direct, root_span, None, None, None, None)?;
    if len > 0 {
        builder.push_segment(PreparedSpan::new(unit, 0, len), origin)?;
    }
    let anchor = builder.source_span(root, len, len)?;
    builder.set_eof_anchor(anchor)?;
    let map = builder.build()?;
    let nodes = manager
        .files()
        .iter()
        .map(|file| {
            IncludeNode::new(
                file.id(),
                file.display_name().to_owned(),
                file.canonical_path().cloned(),
            )
        })
        .collect();
    let graph = Arc::new(IncludeGraph::new(nodes, Vec::new()));
    Ok(PreparedSource::new(
        unit,
        text,
        Arc::new(map),
        Vec::new(),
        graph,
    ))
}
