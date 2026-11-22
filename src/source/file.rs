//! Identity types and original source files.
//!
//! Every coordinate in the experimental pipeline carries the identity of the
//! [`AnalysisUnit`](crate::source::AnalysisUnit) that owns it. The unit id is
//! scoped by a process-local [`RunId`], so coordinates from independently
//! active invocations can never be exchanged even when their unit indexes and
//! byte offsets coincide.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::source::line_index::LineIndex;

/// Process-local identity of one analysis invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(u64);

impl RunId {
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw run value. It is internal identity only and must never be
    /// emitted in diagnostics or used as an output-ordering key.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "run-{}", self.0)
    }
}

/// Identity of one AnalysisUnit inside one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnalysisUnitId {
    run: RunId,
    index: u32,
}

impl AnalysisUnitId {
    pub(crate) fn new(run: RunId, index: u32) -> Self {
        Self { run, index }
    }

    /// Run that owns this unit.
    pub fn run(self) -> RunId {
        self.run
    }

    /// Deterministic unit index inside the run (`0..N-1`).
    pub fn index(self) -> u32 {
        self.index
    }
}

/// Identity of one original source file inside one AnalysisUnit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId {
    unit: AnalysisUnitId,
    file: u32,
}

impl SourceId {
    pub(crate) fn new(unit: AnalysisUnitId, file: u32) -> Self {
        Self { unit, file }
    }

    /// Unit that owns the file.
    pub fn unit(self) -> AnalysisUnitId {
        self.unit
    }

    /// Unit-local file index. Index 0 is the root.
    pub fn file(self) -> u32 {
        self.file
    }
}

/// Category of an original source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFileKind {
    /// Root source loaded from a regular local file.
    Root,
    /// Root source supplied through a reader or text input.
    Stdin,
    /// Included file. Reserved for the include phase; Phase 1 never loads one.
    Included,
}

/// One immutable original source file.
///
/// The file owns its text and lazily builds a [`LineIndex`]. The same `Arc` is
/// shared with [`SourceMap`](crate::source::SourceMap); the prepared buffer is
/// never stored as a `SourceFile`.
#[derive(Debug)]
pub struct SourceFile {
    pub(crate) id: SourceId,
    pub(crate) display_name: String,
    pub(crate) canonical_path: Option<PathBuf>,
    pub(crate) kind: SourceFileKind,
    pub(crate) text: Arc<str>,
    line_index: OnceLock<LineIndex>,
}

impl SourceFile {
    pub(crate) fn new(
        id: SourceId,
        display_name: String,
        canonical_path: Option<PathBuf>,
        kind: SourceFileKind,
        text: Arc<str>,
    ) -> Self {
        Self {
            id,
            display_name,
            canonical_path,
            kind,
            text,
            line_index: OnceLock::new(),
        }
    }

    /// Identity of this file.
    pub fn id(&self) -> SourceId {
        self.id
    }

    /// Display spelling supplied by the user (or the stdin display name).
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Canonical path, when the source came from a filesystem file.
    pub fn canonical_path(&self) -> Option<&PathBuf> {
        self.canonical_path.as_ref()
    }

    /// Category of this source.
    pub fn kind(&self) -> SourceFileKind {
        self.kind
    }

    /// Immutable source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Shared immutable source text.
    pub fn shared_text(&self) -> &Arc<str> {
        &self.text
    }

    /// Source length in UTF-8 bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the source is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Lazily built line index for this file.
    pub fn line_index(&self) -> &LineIndex {
        self.line_index
            .get_or_init(|| LineIndex::new(Arc::clone(&self.text)))
    }
}
