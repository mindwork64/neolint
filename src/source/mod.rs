//! Original-source ownership, line indexing, and checked source mapping.
//!
//! All coordinates are bound to an [`AnalysisUnitId`]. The prepared buffer is
//! not a `SourceFile`; [`PreparedSpan`] and [`SourceSpan`] are distinct types.

pub mod file;
pub mod line_index;
pub mod manager;
pub mod map;
pub mod unit;

pub use file::{AnalysisUnitId, RunId, SourceFile, SourceFileKind, SourceId};
pub use line_index::{LineIndex, LineIndexError, SourcePosition};
pub use manager::{
    SourceLoadError, SourceManager, MAX_ANALYSIS_UNITS_PER_INVOCATION, MAX_INCLUDED_SOURCE_BYTES,
    MAX_ROOT_SOURCE_BYTES, MAX_SOURCE_FILES_PER_UNIT,
};
pub use map::{
    MapSegment, OriginId, OriginKind, PreparedSpan, ResolvedSpan, SourceLocation, SourceMap,
    SourceMapBuilder, SourceOrigin, SourceSpan, SpanError,
};
pub use unit::AnalysisUnit;
