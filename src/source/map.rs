//! Checked source mapping between the prepared buffer and original files.
//!
//! The map owns the origin arena and segment table. All coordinate types are
//! bound to an [`AnalysisUnitId`]; foreign coordinates are rejected before any
//! geometry, file lookup, or indexing.

use std::fmt;
use std::sync::Arc;

use crate::diagnostic::Span;
use crate::source::file::{AnalysisUnitId, SourceFile, SourceId};
use crate::source::line_index::LineIndexError;

/// Maximum supported origin-parent chain length.
pub(crate) const MAX_ORIGIN_DEPTH: usize = 64;

/// A range in the prepared buffer. Fields are private; only
/// [`PreparedSource`](crate::preprocessor::PreparedSource) creates one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreparedSpan {
    pub(crate) unit: AnalysisUnitId,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl PreparedSpan {
    pub(crate) fn new(unit: AnalysisUnitId, start: usize, end: usize) -> Self {
        Self { unit, start, end }
    }

    /// Unit that owns the prepared buffer.
    pub fn unit(self) -> AnalysisUnitId {
        self.unit
    }

    /// Zero-based prepared start offset.
    pub fn start(self) -> usize {
        self.start
    }

    /// Zero-based prepared end offset.
    pub fn end(self) -> usize {
        self.end
    }

    /// Whether the range is empty.
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A range in an original source file. Fields are private; the map builder is
/// the only creator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub(crate) source: SourceId,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl SourceSpan {
    pub(crate) fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }

    /// Original file identity.
    pub fn source(self) -> SourceId {
        self.source
    }

    /// Zero-based original start offset.
    pub fn start(self) -> usize {
        self.start
    }

    /// Zero-based original end offset.
    pub fn end(self) -> usize {
        self.end
    }

    /// Whether the range is a point.
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A resolved position in an original file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    source: SourceId,
    offset: usize,
    line: usize,
    column: usize,
}

impl SourceLocation {
    /// Original file identity.
    pub fn source(self) -> SourceId {
        self.source
    }

    /// Zero-based UTF-8 byte offset.
    pub fn offset(self) -> usize {
        self.offset
    }

    /// One-based line.
    pub fn line(self) -> usize {
        self.line
    }

    /// One-based Unicode scalar column.
    pub fn column(self) -> usize {
        self.column
    }
}

/// How prepared bytes were produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginKind {
    /// Unmodified code copied from a file.
    Direct,
    /// Code entered through an include directive.
    Include,
    /// The macro invocation site.
    MacroInvocation,
    /// The macro definition body.
    MacroDefinition,
    /// An argument substituted into a macro body.
    MacroArgument,
    /// A token produced by `##`.
    TokenPaste,
    /// A token produced by `#`.
    Stringify,
    /// Generated text with no direct user counterpart.
    Generated,
}

impl OriginKind {
    fn priority(self) -> u8 {
        match self {
            Self::Direct => 0,
            // The documented priority groups argument and invocation locations
            // in the same user-code tier; ties are broken left-to-right.
            Self::MacroInvocation | Self::MacroArgument => 1,
            Self::MacroDefinition => 2,
            Self::Include => 3,
            Self::TokenPaste => 4,
            Self::Stringify => 5,
            Self::Generated => 6,
        }
    }
}

/// Map-local handle of one origin in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OriginId {
    pub(crate) unit: AnalysisUnitId,
    pub(crate) index: u32,
}

impl OriginId {
    pub(crate) fn new(unit: AnalysisUnitId, index: u32) -> Self {
        Self { unit, index }
    }

    /// Unit that owns the origin arena.
    pub fn unit(self) -> AnalysisUnitId {
        self.unit
    }
}

/// One link in a provenance chain. Fields are private; only the map builder
/// inserts origins, after validating every span against the map's unit.
#[derive(Debug, Clone)]
pub struct SourceOrigin {
    kind: OriginKind,
    span: SourceSpan,
    invocation: Option<SourceSpan>,
    definition: Option<SourceSpan>,
    parent: Option<OriginId>,
    label: Option<&'static str>,
}

impl SourceOrigin {
    pub(crate) fn new(
        kind: OriginKind,
        span: SourceSpan,
        invocation: Option<SourceSpan>,
        definition: Option<SourceSpan>,
        parent: Option<OriginId>,
        label: Option<&'static str>,
    ) -> Self {
        Self {
            kind,
            span,
            invocation,
            definition,
            parent,
            label,
        }
    }

    /// Provenance category.
    pub fn kind(&self) -> OriginKind {
        self.kind
    }

    /// Primary original location.
    pub fn span(&self) -> SourceSpan {
        self.span
    }

    /// Macro/include invocation location.
    pub fn invocation(&self) -> Option<SourceSpan> {
        self.invocation
    }

    /// Macro definition location.
    pub fn definition(&self) -> Option<SourceSpan> {
        self.definition
    }

    /// Parent origin in the same arena.
    pub fn parent(&self) -> Option<OriginId> {
        self.parent
    }

    /// Optional human-readable label.
    pub fn label(&self) -> Option<&'static str> {
        self.label
    }
}

/// One mapped prepared range.
#[derive(Debug, Clone)]
pub struct MapSegment {
    prepared: PreparedSpan,
    origin: OriginId,
}

impl MapSegment {
    /// Prepared range covered by this segment.
    pub fn prepared(&self) -> PreparedSpan {
        self.prepared
    }

    /// Origin of the covered prepared range.
    pub fn origin(&self) -> OriginId {
        self.origin
    }
}

/// Result of resolving one prepared range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpan {
    primary: SourceSpan,
    related: Vec<SourceSpan>,
}

impl ResolvedSpan {
    /// Primary location to fix.
    pub fn primary(&self) -> SourceSpan {
        self.primary
    }

    /// Secondary locations, deduplicated in deterministic order.
    pub fn related(&self) -> &[SourceSpan] {
        &self.related
    }
}

/// Every coordinate, ownership, or legacy-conversion failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanError {
    /// `start > end`.
    InvertedRange,
    /// A range exceeds its source or the prepared buffer.
    OutOfBounds,
    /// An offset splits a UTF-8 scalar value.
    NotCharBoundary,
    /// A coordinate belongs to another AnalysisUnit.
    ForeignAnalysisUnit,
    /// A prepared coordinate belongs to another PreparedSource/unit.
    ForeignPreparedSource,
    /// `resolve_point` received a non-empty prepared span.
    NotAPoint,
    /// The unit is not a plain-LSL root unit.
    NotLegacyUnit,
    /// A `SourceId` is not registered in this unit.
    UnknownSourceId,
    /// An `OriginId` index is missing from this unit's arena.
    UnknownOriginId,
    /// A prepared range has no segment.
    UnmappedPreparedRange,
    /// Segments overlap, are unsorted, or a chain is broken.
    MapInvariant,
}

impl fmt::Display for SpanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvertedRange => "range start is after range end",
            Self::OutOfBounds => "coordinate is outside its source",
            Self::NotCharBoundary => "coordinate splits a UTF-8 scalar",
            Self::ForeignAnalysisUnit => "coordinate belongs to another analysis unit",
            Self::ForeignPreparedSource => "prepared coordinate belongs to another source",
            Self::NotAPoint => "point resolution requires a zero-length prepared span",
            Self::NotLegacyUnit => "unit is not a plain-LSL root unit",
            Self::UnknownSourceId => "source id is not registered in this unit",
            Self::UnknownOriginId => "origin id is not registered in this arena",
            Self::UnmappedPreparedRange => "prepared range has no origin segment",
            Self::MapInvariant => "source map invariant is broken",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SpanError {}

/// Unit-bound source map.
#[derive(Debug)]
pub struct SourceMap {
    unit: AnalysisUnitId,
    prepared_text: Arc<str>,
    files: Vec<Arc<SourceFile>>,
    segments: Vec<MapSegment>,
    eof_anchor: SourceSpan,
    origins: Vec<SourceOrigin>,
}

impl SourceMap {
    /// Unit that owns this map.
    pub fn unit(&self) -> AnalysisUnitId {
        self.unit
    }

    /// Length of the prepared buffer.
    pub fn prepared_len(&self) -> usize {
        self.prepared_text.len()
    }

    /// Prepared text this map was built for.
    pub fn prepared_text(&self) -> &str {
        &self.prepared_text
    }

    /// Original file registered for `id`, if it belongs to this unit.
    pub fn file(&self, id: SourceId) -> Option<&SourceFile> {
        if id.unit() != self.unit {
            return None;
        }
        self.find_file(id.file()).map(|file| file.as_ref())
    }

    /// Resolve a prepared range only when it maps one-to-one to contiguous
    /// bytes copied directly from the requested source file. Macro expansions,
    /// includes, generated text and ranges crossing removed source gaps return
    /// `Ok(None)`. This is intended for editor edits that must not guess.
    #[cfg(feature = "lsp")]
    pub(crate) fn resolve_direct_span(
        &self,
        prepared: PreparedSpan,
        source: SourceId,
    ) -> Result<Option<SourceSpan>, SpanError> {
        if prepared.unit != self.unit {
            return Err(SpanError::ForeignPreparedSource);
        }
        if source.unit() != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        if prepared.start > prepared.end {
            return Err(SpanError::InvertedRange);
        }
        if prepared.end > self.prepared_text.len() {
            return Err(SpanError::OutOfBounds);
        }
        if !self.prepared_text.is_char_boundary(prepared.start)
            || !self.prepared_text.is_char_boundary(prepared.end)
        {
            return Err(SpanError::NotCharBoundary);
        }
        if prepared.start == prepared.end {
            return Ok(None);
        }

        let first = self
            .segments
            .partition_point(|segment| segment.prepared.end <= prepared.start);
        let mut cursor = prepared.start;
        let mut mapped_start = None;
        let mut mapped_end = 0usize;

        for segment in &self.segments[first..] {
            if cursor >= prepared.end {
                break;
            }
            if segment.prepared.start > cursor {
                return Ok(None);
            }
            let run_start = cursor.max(segment.prepared.start);
            let run_end = prepared.end.min(segment.prepared.end);
            if run_start >= run_end {
                continue;
            }

            let origin = self.origin(segment.origin)?;
            if origin.kind != OriginKind::Direct
                || origin.span.source() != source
                || origin.parent.is_some()
                || origin.invocation.is_some()
                || origin.definition.is_some()
            {
                return Ok(None);
            }
            let prepared_len = segment.prepared.end - segment.prepared.start;
            let source_len = origin.span.end() - origin.span.start();
            if prepared_len != source_len {
                return Ok(None);
            }

            let run_source_start = origin.span.start() + (run_start - segment.prepared.start);
            let run_source_end = run_source_start + (run_end - run_start);
            let source_file = self.file(source).ok_or(SpanError::UnknownSourceId)?;
            let Some(source_text) = source_file.text().get(run_source_start..run_source_end) else {
                return Ok(None);
            };
            if self.prepared_text.get(run_start..run_end) != Some(source_text) {
                return Ok(None);
            }
            if mapped_start.is_some() {
                if mapped_end != run_source_start {
                    return Ok(None);
                }
            } else {
                mapped_start = Some(run_source_start);
            }
            mapped_end = run_source_end;
            cursor = run_end;
        }

        if cursor != prepared.end {
            return Ok(None);
        }
        let Some(start) = mapped_start else {
            return Ok(None);
        };
        let span = SourceSpan::new(source, start, mapped_end);
        self.validate_span_owned(span)?;
        Ok(Some(span))
    }

    /// Resolve a prepared range to original locations.
    pub fn resolve_span(&self, prepared: PreparedSpan) -> Result<ResolvedSpan, SpanError> {
        if prepared.unit != self.unit {
            return Err(SpanError::ForeignPreparedSource);
        }
        let start = prepared.start;
        let end = prepared.end;
        if start > end {
            return Err(SpanError::InvertedRange);
        }
        if end > self.prepared_text.len() {
            return Err(SpanError::OutOfBounds);
        }
        if !self.prepared_text.is_char_boundary(start) || !self.prepared_text.is_char_boundary(end)
        {
            return Err(SpanError::NotCharBoundary);
        }
        if start == end {
            let primary = self.resolve_point(prepared)?;
            return Ok(ResolvedSpan {
                primary,
                related: Vec::new(),
            });
        }

        let mut cursor = start;
        let mut candidates: Vec<(u8, usize, SourceSpan)> = Vec::new();
        let mut related: Vec<SourceSpan> = Vec::new();
        let first = self
            .segments
            .partition_point(|segment| segment.prepared.end <= start);
        for segment in &self.segments[first..] {
            if cursor >= end {
                break;
            }
            if segment.prepared.start > cursor {
                return Err(SpanError::UnmappedPreparedRange);
            }
            if segment.prepared.start >= end {
                break;
            }
            let run_start = cursor.max(segment.prepared.start);
            let run_end = end.min(segment.prepared.end);
            if run_start >= run_end {
                continue;
            }
            let (priority, mapped, chain_related) =
                self.resolve_run_origin(segment, segment.origin, run_start, run_end)?;
            candidates.push((priority, run_start, mapped));
            for span in chain_related {
                push_unique(&mut related, span);
            }
            cursor = run_end;
        }
        if cursor < end {
            return Err(SpanError::UnmappedPreparedRange);
        }
        candidates.sort_by_key(|(priority, run_start, _)| (*priority, *run_start));
        let Some((_, _, primary)) = candidates.first().copied() else {
            return Err(SpanError::UnmappedPreparedRange);
        };
        for (_, _, mapped) in &candidates {
            push_unique(&mut related, *mapped);
        }
        let mut deduplicated = Vec::new();
        for span in related {
            if span != primary {
                push_unique(&mut deduplicated, span);
            }
        }
        Ok(ResolvedSpan {
            primary,
            related: deduplicated,
        })
    }

    /// Resolve a zero-length prepared point.
    pub fn resolve_point(&self, point: PreparedSpan) -> Result<SourceSpan, SpanError> {
        if point.unit != self.unit {
            return Err(SpanError::ForeignPreparedSource);
        }
        if point.start != point.end {
            return Err(SpanError::NotAPoint);
        }
        let offset = point.start;
        if offset > self.prepared_text.len() {
            return Err(SpanError::OutOfBounds);
        }
        if !self.prepared_text.is_char_boundary(offset) {
            return Err(SpanError::NotCharBoundary);
        }
        if offset == self.prepared_text.len() {
            return Ok(self.eof_anchor);
        }
        let index = self
            .segments
            .partition_point(|segment| segment.prepared.end <= offset);
        let Some(segment) = self.segments.get(index) else {
            return Err(SpanError::UnmappedPreparedRange);
        };
        if segment.prepared.start > offset || offset >= segment.prepared.end {
            return Err(SpanError::UnmappedPreparedRange);
        }
        self.resolve_point_origin(segment, segment.origin, offset)
    }

    /// Resolve an original file span to line/column.
    pub fn location(&self, span: SourceSpan) -> Result<SourceLocation, SpanError> {
        if span.source.unit() != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        let file = self
            .find_file(span.source.file())
            .ok_or(SpanError::UnknownSourceId)?;
        self.validate_source_span_in(file, span)?;
        let position = file
            .line_index()
            .location(span.start)
            .map_err(LineIndexError::into_span_error)?;
        Ok(SourceLocation {
            source: span.source,
            offset: span.start,
            line: position.line,
            column: position.column,
        })
    }

    /// Rebuild the frozen [`Span`] shape from an original range.
    pub fn render_span(&self, span: SourceSpan) -> Result<Span, SpanError> {
        if span.source.unit() != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        let file = self
            .find_file(span.source.file())
            .ok_or(SpanError::UnknownSourceId)?;
        self.validate_source_span_in(file, span)?;
        let start = file
            .line_index()
            .location(span.start)
            .map_err(LineIndexError::into_span_error)?;
        let end = file
            .line_index()
            .location(span.end)
            .map_err(LineIndexError::into_span_error)?;
        Ok(Span {
            start: span.start,
            end: span.end,
            line: start.line,
            column: start.column,
            end_line: end.line,
            end_column: end.column,
        })
    }

    /// Checked constructor for an original span of this unit.
    pub fn source_span(
        &self,
        source: SourceId,
        start: usize,
        end: usize,
    ) -> Result<SourceSpan, SpanError> {
        if source.unit() != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        let file = self
            .find_file(source.file())
            .ok_or(SpanError::UnknownSourceId)?;
        let span = SourceSpan::new(source, start, end);
        self.validate_source_span_in(file, span)?;
        Ok(span)
    }

    /// End-of-input anchor.
    pub fn eof_anchor(&self) -> SourceSpan {
        self.eof_anchor
    }

    /// Resolve an origin handle of this unit's arena.
    pub fn origin(&self, id: OriginId) -> Result<&SourceOrigin, SpanError> {
        if id.unit != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        self.validate_origin_chain(id)?;
        self.origins
            .get(id.index as usize)
            .ok_or(SpanError::UnknownOriginId)
    }

    fn find_file(&self, file_index: u32) -> Option<&Arc<SourceFile>> {
        let index = self
            .files
            .binary_search_by_key(&file_index, |file| file.id.file())
            .ok()?;
        self.files.get(index)
    }

    fn validate_source_span_in(
        &self,
        file: &SourceFile,
        span: SourceSpan,
    ) -> Result<(), SpanError> {
        if span.source != file.id() {
            return Err(SpanError::UnknownSourceId);
        }
        if span.start > span.end {
            return Err(SpanError::InvertedRange);
        }
        if span.end > file.len() {
            return Err(SpanError::OutOfBounds);
        }
        if !file.text().is_char_boundary(span.start) || !file.text().is_char_boundary(span.end) {
            return Err(SpanError::NotCharBoundary);
        }
        Ok(())
    }

    fn validate_origin_chain(&self, id: OriginId) -> Result<(), SpanError> {
        let mut current = Some(id);
        let mut depth = 0;
        while let Some(origin_id) = current {
            if origin_id.unit != self.unit {
                return Err(SpanError::ForeignAnalysisUnit);
            }
            let origin = self
                .origins
                .get(origin_id.index as usize)
                .ok_or(SpanError::UnknownOriginId)?;
            self.validate_origin_spans(origin)?;
            depth += 1;
            if depth > 64 {
                return Err(SpanError::MapInvariant);
            }
            current = origin.parent;
        }
        Ok(())
    }

    fn validate_origin_spans(&self, origin: &SourceOrigin) -> Result<(), SpanError> {
        self.validate_span_owned(origin.span)?;
        if let Some(span) = origin.invocation {
            self.validate_span_owned(span)?;
        }
        if let Some(span) = origin.definition {
            self.validate_span_owned(span)?;
        }
        if let Some(parent) = origin.parent {
            if parent.unit != self.unit {
                return Err(SpanError::ForeignAnalysisUnit);
            }
            if parent.index as usize >= self.origins.len() {
                return Err(SpanError::UnknownOriginId);
            }
        }
        Ok(())
    }

    fn validate_span_owned(&self, span: SourceSpan) -> Result<(), SpanError> {
        if span.source.unit() != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        let file = self
            .find_file(span.source.file())
            .ok_or(SpanError::UnknownSourceId)?;
        self.validate_source_span_in(file, span)
    }

    fn origin_chain(&self, id: OriginId) -> Result<Vec<OriginId>, SpanError> {
        let mut chain = Vec::new();
        let mut current = Some(id);
        while let Some(origin_id) = current {
            if origin_id.unit != self.unit {
                return Err(SpanError::ForeignAnalysisUnit);
            }
            let origin = self
                .origins
                .get(origin_id.index as usize)
                .ok_or(SpanError::UnknownOriginId)?;
            self.validate_origin_spans(origin)?;
            chain.push(origin_id);
            if chain.len() > MAX_ORIGIN_DEPTH {
                return Err(SpanError::MapInvariant);
            }
            current = origin.parent;
        }
        Ok(chain)
    }

    fn resolve_run_origin(
        &self,
        segment: &MapSegment,
        origin_id: OriginId,
        run_start: usize,
        run_end: usize,
    ) -> Result<(u8, SourceSpan, Vec<SourceSpan>), SpanError> {
        let chain = self.origin_chain(origin_id)?;
        let mut primary: Option<(usize, u8, SourceSpan)> = None;
        for (index, id) in chain.iter().enumerate() {
            let origin = self
                .origins
                .get(id.index as usize)
                .ok_or(SpanError::UnknownOriginId)?;
            match origin.kind {
                OriginKind::Generated => continue,
                OriginKind::TokenPaste | OriginKind::Stringify => {
                    if let Some(invocation) = origin.invocation {
                        self.validate_span_owned(invocation)?;
                        primary = Some((index, OriginKind::MacroInvocation.priority(), invocation));
                        break;
                    }
                }
                _ => {
                    let mapped = if index == 0 {
                        self.map_user_run(segment, origin, run_start, run_end, true)?
                    } else {
                        self.validate_span_owned(origin.span)?;
                        origin.span
                    };
                    primary = Some((index, origin.kind.priority(), mapped));
                    break;
                }
            }
        }
        let (primary_index, priority, mapped) = match primary {
            Some(value) => value,
            None => {
                let origin = self
                    .origins
                    .get(origin_id.index as usize)
                    .ok_or(SpanError::UnknownOriginId)?;
                self.validate_span_owned(origin.span)?;
                (0, origin.kind.priority(), origin.span)
            }
        };
        let mut related = Vec::new();
        for (index, id) in chain.iter().enumerate() {
            let origin = self
                .origins
                .get(id.index as usize)
                .ok_or(SpanError::UnknownOriginId)?;
            if index != primary_index {
                push_unique(&mut related, origin.span);
            }
            if let Some(span) = origin.invocation {
                push_unique(&mut related, span);
            }
            if let Some(span) = origin.definition {
                push_unique(&mut related, span);
            }
        }
        Ok((priority, mapped, related))
    }

    fn resolve_point_origin(
        &self,
        segment: &MapSegment,
        origin_id: OriginId,
        offset: usize,
    ) -> Result<SourceSpan, SpanError> {
        let chain = self.origin_chain(origin_id)?;
        for (index, id) in chain.iter().enumerate() {
            let origin = self
                .origins
                .get(id.index as usize)
                .ok_or(SpanError::UnknownOriginId)?;
            match origin.kind {
                OriginKind::Generated => continue,
                OriginKind::TokenPaste | OriginKind::Stringify => {
                    if let Some(invocation) = origin.invocation {
                        self.validate_span_owned(invocation)?;
                        let span =
                            SourceSpan::new(invocation.source, invocation.start, invocation.start);
                        self.validate_span_owned(span)?;
                        return Ok(span);
                    }
                }
                _ => {
                    let span = if index == 0 {
                        self.map_user_point(segment, origin, offset, true)?
                    } else {
                        self.validate_span_owned(origin.span)?;
                        SourceSpan::new(origin.span.source, origin.span.start, origin.span.start)
                    };
                    self.validate_span_owned(span)?;
                    return Ok(span);
                }
            }
        }
        let origin = self
            .origins
            .get(origin_id.index as usize)
            .ok_or(SpanError::UnknownOriginId)?;
        self.validate_span_owned(origin.span)?;
        Ok(SourceSpan::new(
            origin.span.source,
            origin.span.start,
            origin.span.start,
        ))
    }

    fn map_user_run(
        &self,
        segment: &MapSegment,
        origin: &SourceOrigin,
        run_start: usize,
        run_end: usize,
        allow_relative: bool,
    ) -> Result<SourceSpan, SpanError> {
        let prepared_len = segment.prepared.end - segment.prepared.start;
        let original_len = origin.span.end - origin.span.start;
        if allow_relative && prepared_len == original_len {
            let delta = run_start - segment.prepared.start;
            let start = origin.span.start + delta;
            let end = start + (run_end - run_start);
            let span = SourceSpan::new(origin.span.source, start, end);
            self.validate_span_owned(span)?;
            Ok(span)
        } else {
            self.validate_span_owned(origin.span)?;
            Ok(origin.span)
        }
    }

    fn map_user_point(
        &self,
        segment: &MapSegment,
        origin: &SourceOrigin,
        offset: usize,
        allow_relative: bool,
    ) -> Result<SourceSpan, SpanError> {
        let prepared_len = segment.prepared.end - segment.prepared.start;
        let original_len = origin.span.end - origin.span.start;
        let start = if allow_relative && prepared_len == original_len {
            origin.span.start + (offset - segment.prepared.start)
        } else {
            origin.span.start
        };
        let span = SourceSpan::new(origin.span.source, start, start);
        self.validate_span_owned(span)?;
        Ok(span)
    }
}

fn push_unique(spans: &mut Vec<SourceSpan>, span: SourceSpan) {
    if !spans.contains(&span) {
        spans.push(span);
    }
}

trait IntoSpanError {
    fn into_span_error(self) -> SpanError;
}

impl IntoSpanError for LineIndexError {
    fn into_span_error(self) -> SpanError {
        match self {
            LineIndexError::OutOfBounds => SpanError::OutOfBounds,
            LineIndexError::NotCharBoundary => SpanError::NotCharBoundary,
            LineIndexError::LineOutOfBounds | LineIndexError::ColumnOutOfBounds => {
                SpanError::MapInvariant
            }
        }
    }
}

/// Builder for a unit-bound [`SourceMap`].
#[derive(Debug)]
pub struct SourceMapBuilder {
    unit: AnalysisUnitId,
    prepared_text: Arc<str>,
    files: Vec<Arc<SourceFile>>,
    segments: Vec<MapSegment>,
    origins: Vec<SourceOrigin>,
    eof_anchor: Option<SourceSpan>,
}

impl SourceMapBuilder {
    /// Create a builder for one unit and its loaded original files.
    pub fn new(
        unit: AnalysisUnitId,
        prepared_text: Arc<str>,
        files: Vec<Arc<SourceFile>>,
    ) -> Result<Self, SpanError> {
        let mut files = files;
        files.sort_by_key(|file| file.id.file());
        for (index, file) in files.iter().enumerate() {
            if file.id.unit() != unit {
                return Err(SpanError::ForeignAnalysisUnit);
            }
            if file.id.file() as usize != index {
                return Err(SpanError::MapInvariant);
            }
        }
        Ok(Self {
            unit,
            prepared_text,
            files,
            segments: Vec::new(),
            origins: Vec::new(),
            eof_anchor: None,
        })
    }

    /// Prepared buffer length.
    pub fn prepared_len(&self) -> usize {
        self.prepared_text.len()
    }

    /// Checked constructor for an original span of this unit.
    pub fn source_span(
        &self,
        source: SourceId,
        start: usize,
        end: usize,
    ) -> Result<SourceSpan, SpanError> {
        if source.unit() != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        let file = self
            .find_file(source.file())
            .ok_or(SpanError::UnknownSourceId)?;
        let span = SourceSpan::new(source, start, end);
        self.validate_source_span_in(file, span)?;
        Ok(span)
    }

    /// Insert a validated origin and return its map-local handle.
    pub fn push_origin(
        &mut self,
        kind: OriginKind,
        span: SourceSpan,
        invocation: Option<SourceSpan>,
        definition: Option<SourceSpan>,
        parent: Option<OriginId>,
        label: Option<&'static str>,
    ) -> Result<OriginId, SpanError> {
        self.validate_span(span)?;
        if let Some(span) = invocation {
            self.validate_span(span)?;
        }
        if let Some(span) = definition {
            self.validate_span(span)?;
        }
        if let Some(parent) = parent {
            if parent.unit != self.unit {
                return Err(SpanError::ForeignAnalysisUnit);
            }
            if parent.index as usize >= self.origins.len() {
                return Err(SpanError::UnknownOriginId);
            }
            if self.origin_depth(parent) >= MAX_ORIGIN_DEPTH {
                return Err(SpanError::MapInvariant);
            }
        }
        let index = u32::try_from(self.origins.len()).map_err(|_| SpanError::MapInvariant)?;
        let origin = SourceOrigin::new(kind, span, invocation, definition, parent, label);
        self.origins.push(origin);
        Ok(OriginId::new(self.unit, index))
    }

    /// Append a prepared segment. Segments must be appended in order.
    pub fn push_segment(
        &mut self,
        prepared: PreparedSpan,
        origin: OriginId,
    ) -> Result<(), SpanError> {
        if prepared.unit != self.unit {
            return Err(SpanError::ForeignPreparedSource);
        }
        if prepared.start > prepared.end {
            return Err(SpanError::InvertedRange);
        }
        if prepared.start == prepared.end {
            return Err(SpanError::MapInvariant);
        }
        if prepared.end > self.prepared_text.len() {
            return Err(SpanError::OutOfBounds);
        }
        if !self.prepared_text.is_char_boundary(prepared.start)
            || !self.prepared_text.is_char_boundary(prepared.end)
        {
            return Err(SpanError::NotCharBoundary);
        }
        if origin.unit != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        if origin.index as usize >= self.origins.len() {
            return Err(SpanError::UnknownOriginId);
        }
        if let Some(last) = self.segments.last() {
            if prepared.start < last.prepared.end {
                return Err(SpanError::MapInvariant);
            }
        }
        self.segments.push(MapSegment { prepared, origin });
        Ok(())
    }

    /// Set the zero-length end-of-input anchor.
    pub fn set_eof_anchor(&mut self, anchor: SourceSpan) -> Result<(), SpanError> {
        self.validate_span(anchor)?;
        if anchor.start != anchor.end {
            return Err(SpanError::MapInvariant);
        }
        self.eof_anchor = Some(anchor);
        Ok(())
    }

    /// Validate and build the immutable map.
    pub fn build(self) -> Result<SourceMap, SpanError> {
        let anchor = self.eof_anchor.ok_or(SpanError::MapInvariant)?;
        self.validate_span(anchor)?;
        if anchor.start != anchor.end {
            return Err(SpanError::MapInvariant);
        }
        if self.prepared_text.is_empty() {
            if !self.segments.is_empty() {
                return Err(SpanError::MapInvariant);
            }
        } else {
            let Some(first) = self.segments.first() else {
                return Err(SpanError::UnmappedPreparedRange);
            };
            if first.prepared.start != 0 {
                return Err(SpanError::UnmappedPreparedRange);
            }
            let mut cursor = 0;
            for segment in &self.segments {
                if segment.prepared.start != cursor {
                    return Err(SpanError::MapInvariant);
                }
                cursor = segment.prepared.end;
            }
            if cursor != self.prepared_text.len() {
                return Err(SpanError::UnmappedPreparedRange);
            }
        }
        for segment in &self.segments {
            if segment.origin.unit != self.unit {
                return Err(SpanError::ForeignAnalysisUnit);
            }
            if segment.origin.index as usize >= self.origins.len() {
                return Err(SpanError::UnknownOriginId);
            }
        }
        for origin in &self.origins {
            self.validate_span(origin.span)?;
            if let Some(span) = origin.invocation {
                self.validate_span(span)?;
            }
            if let Some(span) = origin.definition {
                self.validate_span(span)?;
            }
            if let Some(parent) = origin.parent {
                if parent.unit != self.unit {
                    return Err(SpanError::ForeignAnalysisUnit);
                }
                if parent.index as usize >= self.origins.len() {
                    return Err(SpanError::UnknownOriginId);
                }
            }
        }
        Ok(SourceMap {
            unit: self.unit,
            prepared_text: self.prepared_text,
            files: self.files,
            segments: self.segments,
            eof_anchor: anchor,
            origins: self.origins,
        })
    }

    fn find_file(&self, file_index: u32) -> Option<&Arc<SourceFile>> {
        let index = self
            .files
            .binary_search_by_key(&file_index, |file| file.id.file())
            .ok()?;
        self.files.get(index)
    }

    fn validate_span(&self, span: SourceSpan) -> Result<(), SpanError> {
        if span.source.unit() != self.unit {
            return Err(SpanError::ForeignAnalysisUnit);
        }
        let file = self
            .find_file(span.source.file())
            .ok_or(SpanError::UnknownSourceId)?;
        self.validate_source_span_in(file, span)
    }

    fn validate_source_span_in(
        &self,
        file: &SourceFile,
        span: SourceSpan,
    ) -> Result<(), SpanError> {
        if span.source != file.id() {
            return Err(SpanError::UnknownSourceId);
        }
        if span.start > span.end {
            return Err(SpanError::InvertedRange);
        }
        if span.end > file.len() {
            return Err(SpanError::OutOfBounds);
        }
        if !file.text().is_char_boundary(span.start) || !file.text().is_char_boundary(span.end) {
            return Err(SpanError::NotCharBoundary);
        }
        Ok(())
    }

    fn origin_depth(&self, mut id: OriginId) -> usize {
        let mut depth = 0;
        loop {
            depth += 1;
            if depth > MAX_ORIGIN_DEPTH + 1 {
                return depth;
            }
            let Some(origin) = self.origins.get(id.index as usize) else {
                return depth;
            };
            match origin.parent {
                Some(parent) if parent.unit == self.unit => id = parent,
                _ => return depth,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::file::RunId;
    use crate::source::manager::SourceManager;
    use std::sync::Arc;

    fn unit(index: u32) -> AnalysisUnitId {
        AnalysisUnitId::new(RunId::new(1), index)
    }

    fn fixture(text: &str, index: u32) -> (AnalysisUnitId, SourceId, Vec<Arc<SourceFile>>) {
        let unit = unit(index);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        (unit, root, manager.files().to_vec())
    }

    fn identity(text: &str, index: u32) -> (SourceMap, SourceId, AnalysisUnitId) {
        let (unit, root, files) = fixture(text, index);
        let mut builder = SourceMapBuilder::new(unit, Arc::from(text), files).unwrap();
        let root_span = builder.source_span(root, 0, text.len()).unwrap();
        let origin = builder
            .push_origin(OriginKind::Direct, root_span, None, None, None, None)
            .unwrap();
        if !text.is_empty() {
            builder
                .push_segment(PreparedSpan::new(unit, 0, text.len()), origin)
                .unwrap();
        }
        let anchor = builder.source_span(root, text.len(), text.len()).unwrap();
        builder.set_eof_anchor(anchor).unwrap();
        (builder.build().unwrap(), root, unit)
    }

    #[test]
    fn identity_round_trips_ranges_points_and_eof() {
        let text = "abc\ndef";
        let (map, root, unit) = identity(text, 0);
        let span = map.source_span(root, 1, 4).unwrap();
        let resolved = map.resolve_span(PreparedSpan::new(unit, 1, 4)).unwrap();
        assert_eq!(resolved.primary(), span);
        assert!(resolved.related().is_empty());
        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 2, 2)).unwrap(),
            map.source_span(root, 2, 2).unwrap()
        );
        let eof = map
            .resolve_point(PreparedSpan::new(unit, text.len(), text.len()))
            .unwrap();
        assert_eq!(eof, map.eof_anchor());
        let rendered = map.render_span(span).unwrap();
        assert_eq!((rendered.start, rendered.line, rendered.column), (1, 1, 2));
        assert_eq!((rendered.end_line, rendered.end_column), (2, 1));
    }

    #[test]
    fn empty_identity_has_no_segments_and_resolves_eof() {
        let (map, root, unit) = identity("", 0);
        assert_eq!(map.prepared_len(), 0);
        let point = PreparedSpan::new(unit, 0, 0);
        assert_eq!(map.resolve_point(point).unwrap(), map.eof_anchor());
        assert_eq!(map.eof_anchor(), map.source_span(root, 0, 0).unwrap());
        assert_eq!(map.resolve_span(point).unwrap().primary(), map.eof_anchor());
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn resolve_direct_span_accepts_only_contiguous_one_to_one_root_text() {
        let (map, root, unit) = identity("if (value = 1) {}", 0);
        let prepared = PreparedSpan::new(unit, 4, 13);
        assert_eq!(
            map.resolve_direct_span(prepared, root).unwrap(),
            Some(map.source_span(root, 4, 13).unwrap())
        );
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn resolve_direct_span_rejects_non_direct_and_gapped_ranges() {
        let text = Arc::<str>::from("abcdefgh");
        let (unit, root, files) = fixture("abcdefgh", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::clone(&text), files).unwrap();
        let direct = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 2).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let macro_definition = builder
            .push_origin(
                OriginKind::MacroDefinition,
                builder.source_span(root, 2, 4).unwrap(),
                Some(builder.source_span(root, 0, 0).unwrap()),
                Some(builder.source_span(root, 2, 4).unwrap()),
                None,
                Some("macro body"),
            )
            .unwrap();
        let included = builder
            .push_origin(
                OriginKind::Include,
                builder.source_span(root, 4, 6).unwrap(),
                Some(builder.source_span(root, 4, 4).unwrap()),
                None,
                None,
                Some("included text"),
            )
            .unwrap();
        let generated = builder
            .push_origin(
                OriginKind::Generated,
                builder.source_span(root, 6, 8).unwrap(),
                None,
                None,
                None,
                Some("generated text"),
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 2), direct)
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 2, 4), macro_definition)
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 4, 6), included)
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 6, 8), generated)
            .unwrap();
        builder
            .set_eof_anchor(builder.source_span(root, 8, 8).unwrap())
            .unwrap();
        let map = builder.build().unwrap();

        assert_eq!(
            map.resolve_direct_span(PreparedSpan::new(unit, 0, 2), root)
                .unwrap(),
            Some(map.source_span(root, 0, 2).unwrap())
        );
        assert!(map
            .resolve_direct_span(PreparedSpan::new(unit, 0, 4), root)
            .unwrap()
            .is_none());
        for (start, end) in [(2, 4), (4, 6), (6, 8)] {
            assert!(map
                .resolve_direct_span(PreparedSpan::new(unit, start, end), root)
                .unwrap()
                .is_none());
        }

        // Both pieces are direct one-to-one mappings, but a removed source
        // directive separates them in the root buffer. Do not span the gap.
        let (gap_unit, gap_root, gap_files) = fixture("abcdef", 2);
        let mut gap_builder =
            SourceMapBuilder::new(gap_unit, Arc::from("abcd"), gap_files).unwrap();
        let left = gap_builder
            .push_origin(
                OriginKind::Direct,
                gap_builder.source_span(gap_root, 0, 2).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let right = gap_builder
            .push_origin(
                OriginKind::Direct,
                gap_builder.source_span(gap_root, 4, 6).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        gap_builder
            .push_segment(PreparedSpan::new(gap_unit, 0, 2), left)
            .unwrap();
        gap_builder
            .push_segment(PreparedSpan::new(gap_unit, 2, 4), right)
            .unwrap();
        gap_builder
            .set_eof_anchor(gap_builder.source_span(gap_root, 6, 6).unwrap())
            .unwrap();
        let gap_map = gap_builder.build().unwrap();
        assert!(gap_map
            .resolve_direct_span(PreparedSpan::new(gap_unit, 0, 4), gap_root)
            .unwrap()
            .is_none());

        // A forged or malformed direct map with equal-length but different
        // bytes is not sufficient proof for an editor text edit.
        let (mismatch_unit, mismatch_root, mismatch_files) = fixture("abcd", 3);
        let mut mismatch_builder =
            SourceMapBuilder::new(mismatch_unit, Arc::from("ABcd"), mismatch_files).unwrap();
        let mismatch_origin = mismatch_builder
            .push_origin(
                OriginKind::Direct,
                mismatch_builder.source_span(mismatch_root, 0, 4).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        mismatch_builder
            .push_segment(PreparedSpan::new(mismatch_unit, 0, 4), mismatch_origin)
            .unwrap();
        mismatch_builder
            .set_eof_anchor(mismatch_builder.source_span(mismatch_root, 4, 4).unwrap())
            .unwrap();
        let mismatch_map = mismatch_builder.build().unwrap();
        assert!(mismatch_map
            .resolve_direct_span(PreparedSpan::new(mismatch_unit, 0, 4), mismatch_root)
            .unwrap()
            .is_none());
    }

    #[test]
    fn boundary_points_are_right_biased_and_cross_segment_spans_are_split() {
        let (unit, root, files) = fixture("abcd", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files.clone()).unwrap();
        let first_origin = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 2, 4).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let second_origin = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 2).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 2), first_origin)
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 2, 4), second_origin)
            .unwrap();
        let anchor = builder.source_span(root, 4, 4).unwrap();
        builder.set_eof_anchor(anchor).unwrap();
        let map = builder.build().unwrap();

        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 2, 2)).unwrap(),
            map.source_span(root, 0, 0).unwrap()
        );
        let resolved = map.resolve_span(PreparedSpan::new(unit, 1, 3)).unwrap();
        assert_eq!(resolved.primary(), map.source_span(root, 3, 4).unwrap());
        assert_eq!(resolved.related(), &[map.source_span(root, 0, 1).unwrap()]);
    }

    #[test]
    fn origin_priority_prefers_direct_code_and_deduplicates_related() {
        let (unit, root, files) = fixture("abcd", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files).unwrap();
        let generated = builder
            .push_origin(
                OriginKind::Generated,
                builder.source_span(root, 0, 2).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let direct = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 2, 4).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 2), generated)
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 2, 4), direct)
            .unwrap();
        let anchor = builder.source_span(root, 4, 4).unwrap();
        builder.set_eof_anchor(anchor).unwrap();
        let map = builder.build().unwrap();
        let resolved = map.resolve_span(PreparedSpan::new(unit, 0, 4)).unwrap();
        assert_eq!(resolved.primary(), map.source_span(root, 2, 4).unwrap());
        assert_eq!(resolved.related(), &[map.source_span(root, 0, 2).unwrap()]);
    }

    #[test]
    fn nested_origin_chain_resolves_primary_and_all_related_locations() {
        let (unit, root, files) = fixture("abcdef", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files).unwrap();
        let origin1 = builder
            .push_origin(
                OriginKind::MacroDefinition,
                builder.source_span(root, 2, 3).unwrap(),
                Some(builder.source_span(root, 0, 2).unwrap()),
                Some(builder.source_span(root, 1, 3).unwrap()),
                None,
                None,
            )
            .unwrap();
        let origin2 = builder
            .push_origin(
                OriginKind::Generated,
                builder.source_span(root, 4, 5).unwrap(),
                None,
                None,
                Some(origin1),
                None,
            )
            .unwrap();
        let origin3 = builder
            .push_origin(
                OriginKind::MacroDefinition,
                builder.source_span(root, 3, 4).unwrap(),
                Some(builder.source_span(root, 0, 1).unwrap()),
                Some(builder.source_span(root, 1, 4).unwrap()),
                Some(origin2),
                None,
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 4), origin3)
            .unwrap();
        builder
            .set_eof_anchor(builder.source_span(root, 6, 6).unwrap())
            .unwrap();
        let map = builder.build().unwrap();

        let resolved = map.resolve_span(PreparedSpan::new(unit, 0, 4)).unwrap();
        assert_eq!(resolved.primary(), map.source_span(root, 3, 4).unwrap());
        for expected in [
            map.source_span(root, 0, 1).unwrap(),
            map.source_span(root, 1, 4).unwrap(),
            map.source_span(root, 4, 5).unwrap(),
            map.source_span(root, 2, 3).unwrap(),
            map.source_span(root, 0, 2).unwrap(),
            map.source_span(root, 1, 3).unwrap(),
        ] {
            assert!(
                resolved.related().contains(&expected),
                "missing related {expected:?}: {resolved:?}"
            );
        }
        assert_eq!(resolved.related().len(), 6);
        assert!(!resolved.related().contains(&resolved.primary()));

        // A point is anchored to the user origin, not relative-mapped through
        // a longer prepared segment.
        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 1, 1)).unwrap(),
            map.source_span(root, 3, 3).unwrap()
        );
        // EOF still uses the map anchor.
        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 4, 4)).unwrap(),
            map.source_span(root, 6, 6).unwrap()
        );

        // A purely generated immediate origin resolves through its parent.
        let (unit, root, files) = fixture("abcdef", 1);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files).unwrap();
        let parent = builder
            .push_origin(
                OriginKind::MacroDefinition,
                builder.source_span(root, 2, 3).unwrap(),
                Some(builder.source_span(root, 0, 2).unwrap()),
                Some(builder.source_span(root, 1, 3).unwrap()),
                None,
                None,
            )
            .unwrap();
        let generated = builder
            .push_origin(
                OriginKind::Generated,
                builder.source_span(root, 4, 5).unwrap(),
                None,
                None,
                Some(parent),
                None,
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 4), generated)
            .unwrap();
        builder
            .set_eof_anchor(builder.source_span(root, 6, 6).unwrap())
            .unwrap();
        let map = builder.build().unwrap();
        let resolved = map.resolve_span(PreparedSpan::new(unit, 0, 4)).unwrap();
        assert_eq!(resolved.primary(), map.source_span(root, 2, 3).unwrap());
        assert!(resolved
            .related()
            .contains(&map.source_span(root, 4, 5).unwrap()));
        assert!(resolved
            .related()
            .contains(&map.source_span(root, 0, 2).unwrap()));
        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 2, 2)).unwrap(),
            map.source_span(root, 2, 2).unwrap()
        );
    }

    #[test]
    fn rejects_holes_overlaps_and_foreign_segments() {
        let (unit, root, files) = fixture("abcd", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files.clone()).unwrap();
        let origin = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 2).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 2), origin)
            .unwrap();
        assert_eq!(
            builder.push_segment(PreparedSpan::new(unit, 1, 3), origin),
            Err(SpanError::MapInvariant)
        );
        builder
            .push_segment(PreparedSpan::new(unit, 3, 4), origin)
            .unwrap();
        let anchor = builder.source_span(root, 4, 4).unwrap();
        builder.set_eof_anchor(anchor).unwrap();
        assert!(matches!(builder.build(), Err(SpanError::MapInvariant)));

        let (other_unit, _, _) = fixture("abcd", 1);
        let (unit, root, files) = fixture("abcd", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files).unwrap();
        let origin = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 4).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let foreign = PreparedSpan::new(other_unit, 0, 4);
        assert_eq!(
            builder.push_segment(foreign, origin),
            Err(SpanError::ForeignPreparedSource)
        );
    }

    #[test]
    fn rejects_inverted_out_of_bounds_and_non_boundary_prepared_ranges() {
        let (map, _, unit) = identity("é", 0);
        assert_eq!(
            map.resolve_span(PreparedSpan::new(unit, 2, 1)),
            Err(SpanError::InvertedRange)
        );
        assert_eq!(
            map.resolve_span(PreparedSpan::new(unit, 0, 3)),
            Err(SpanError::OutOfBounds)
        );
        assert_eq!(
            map.resolve_span(PreparedSpan::new(unit, 1, 2)),
            Err(SpanError::NotCharBoundary)
        );
        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 1, 1)),
            Err(SpanError::NotCharBoundary)
        );
        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 0, 2)),
            Err(SpanError::NotAPoint)
        );
        assert_eq!(
            map.resolve_point(PreparedSpan::new(unit, 3, 3)),
            Err(SpanError::OutOfBounds)
        );
    }

    #[test]
    fn rejects_foreign_and_unknown_source_coordinates() {
        let (first, root, first_unit) = identity("abcd", 0);
        let (second, _, _) = identity("abcd", 1);
        let foreign = second
            .source_span(second.files.first().unwrap().id(), 0, 1)
            .unwrap();
        assert_eq!(first.location(foreign), Err(SpanError::ForeignAnalysisUnit));
        assert_eq!(
            second.location(first.source_span(root, 0, 1).unwrap()),
            Err(SpanError::ForeignAnalysisUnit)
        );
        assert_eq!(
            first.resolve_span(PreparedSpan::new(second.unit(), 0, 1)),
            Err(SpanError::ForeignPreparedSource)
        );
        assert_eq!(
            first.resolve_point(PreparedSpan::new(second.unit(), 0, 0)),
            Err(SpanError::ForeignPreparedSource)
        );
        assert_eq!(
            second.location(first.eof_anchor()),
            Err(SpanError::ForeignAnalysisUnit)
        );
        assert_eq!(
            first.resolve_point(PreparedSpan::new(
                second.unit(),
                second.prepared_len(),
                second.prepared_len()
            )),
            Err(SpanError::ForeignPreparedSource)
        );
        assert_eq!(
            first.source_span(SourceId::new(first_unit, 99), 0, 1),
            Err(SpanError::UnknownSourceId)
        );
        let unknown = SourceSpan::new(SourceId::new(first_unit, 99), 0, 1);
        assert_eq!(first.location(unknown), Err(SpanError::UnknownSourceId));
        assert_eq!(first.source_span(root, 0, 5), Err(SpanError::OutOfBounds));
        assert_eq!(first.source_span(root, 2, 1), Err(SpanError::InvertedRange));
        assert!(matches!(
            first.origin(OriginId::new(second.unit(), 0)),
            Err(SpanError::ForeignAnalysisUnit)
        ));
        assert!(matches!(
            first.origin(OriginId::new(first_unit, 999)),
            Err(SpanError::UnknownOriginId)
        ));
    }

    #[test]
    fn rejects_original_offsets_inside_utf8_scalars() {
        let (map, root, _) = identity("é", 0);
        assert_eq!(map.source_span(root, 1, 2), Err(SpanError::NotCharBoundary));
        assert_eq!(map.source_span(root, 0, 1), Err(SpanError::NotCharBoundary));
    }

    #[test]
    fn rejects_foreign_origin_spans_and_unknown_or_foreign_origin_ids() {
        let (unit, root, files) = fixture("abcd", 0);
        let (other_unit, other_root, _) = fixture("abcd", 1);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files).unwrap();
        let foreign_span = SourceSpan::new(other_root, 0, 1);
        assert_eq!(
            builder.push_origin(OriginKind::Direct, foreign_span, None, None, None, None),
            Err(SpanError::ForeignAnalysisUnit)
        );
        assert_eq!(
            builder.push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 1).unwrap(),
                Some(foreign_span),
                None,
                None,
                None
            ),
            Err(SpanError::ForeignAnalysisUnit)
        );
        assert_eq!(
            builder.push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 1).unwrap(),
                None,
                Some(foreign_span),
                None,
                None
            ),
            Err(SpanError::ForeignAnalysisUnit)
        );
        let origin = builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 1).unwrap(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            builder.push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 1).unwrap(),
                None,
                None,
                Some(OriginId::new(unit, 99)),
                None
            ),
            Err(SpanError::UnknownOriginId)
        );
        assert_eq!(
            builder.push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 1).unwrap(),
                None,
                None,
                Some(OriginId::new(other_unit, 0)),
                None
            ),
            Err(SpanError::ForeignAnalysisUnit)
        );
        assert!(builder
            .push_origin(
                OriginKind::Direct,
                builder.source_span(root, 0, 1).unwrap(),
                None,
                None,
                Some(origin),
                None
            )
            .is_ok());
    }

    #[test]
    fn origin_chain_accepts_the_documented_depth_and_rejects_one_more() {
        let (unit, root, files) = fixture("abcd", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files.clone()).unwrap();
        let span = builder.source_span(root, 0, 1).unwrap();
        let mut parent = None;
        for _ in 0..MAX_ORIGIN_DEPTH {
            parent = Some(
                builder
                    .push_origin(OriginKind::Direct, span, None, None, parent, None)
                    .expect("origins up to the documented depth must be accepted"),
            );
        }
        let error = builder
            .push_origin(OriginKind::Direct, span, None, None, parent, None)
            .expect_err("one origin beyond the documented depth must fail closed");
        assert_eq!(error, SpanError::MapInvariant);
    }

    #[test]
    fn enforces_origin_depth_and_detects_cycles() {
        let (unit, root, files) = fixture("abcd", 0);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("abcd"), files.clone()).unwrap();
        let span = builder.source_span(root, 0, 1).unwrap();
        let first = builder
            .push_origin(OriginKind::Direct, span, None, None, None, None)
            .unwrap();
        let mut parent = first;
        let mut depth_error = false;
        for _ in 0..80 {
            match builder.push_origin(OriginKind::Direct, span, None, None, Some(parent), None) {
                Ok(origin) => parent = origin,
                Err(error) => {
                    assert_eq!(error, SpanError::MapInvariant);
                    depth_error = true;
                    break;
                }
            }
        }
        assert!(depth_error);

        // Corrupt a built map to prove chain validation fails closed.
        let (map, root, unit) = identity("abcd", 2);
        let mut map = map;
        let segment_origin = map.segments[0].origin;
        let parent = OriginId::new(unit, 1);
        map.origins.push(SourceOrigin::new(
            OriginKind::Direct,
            map.source_span(root, 0, 1).unwrap(),
            None,
            None,
            Some(segment_origin),
            None,
        ));
        map.segments[0].origin = parent;
        map.origins[segment_origin.index as usize].parent = Some(parent);
        assert_eq!(
            map.resolve_span(PreparedSpan::new(unit, 0, 1)),
            Err(SpanError::MapInvariant)
        );
    }

    #[test]
    fn nested_parent_chain_rejects_foreign_spans() {
        let (unit, root, files) = fixture("ab", 0);
        let (_other_unit, other_root, _) = fixture("ab", 1);
        let mut builder = SourceMapBuilder::new(unit, Arc::from("ab"), files).unwrap();
        let valid_span = builder.source_span(root, 0, 2).unwrap();
        let first = builder
            .push_origin(OriginKind::Direct, valid_span, None, None, None, None)
            .unwrap();
        let second = builder
            .push_origin(
                OriginKind::Direct,
                valid_span,
                None,
                None,
                Some(first),
                None,
            )
            .unwrap();
        let anchor = builder.source_span(root, 2, 2).unwrap();
        builder.set_eof_anchor(anchor).unwrap();
        builder
            .push_segment(PreparedSpan::new(unit, 0, 2), second)
            .unwrap();
        let mut map = builder.build().unwrap();
        map.origins[first.index as usize].span = SourceSpan::new(other_root, 0, 2);
        assert!(matches!(
            map.origin(second),
            Err(SpanError::ForeignAnalysisUnit)
        ));
        assert_eq!(
            map.resolve_span(PreparedSpan::new(unit, 0, 2)),
            Err(SpanError::ForeignAnalysisUnit)
        );
    }

    #[test]
    fn origin_priority_and_related_order_are_deterministic() {
        let (map, root, unit) = identity("abcd", 0);
        let span = map.source_span(root, 0, 4).unwrap();
        let resolved = map.resolve_span(PreparedSpan::new(unit, 0, 4)).unwrap();
        assert_eq!(resolved.primary(), span);
        assert!(resolved.related().is_empty());
    }
}
