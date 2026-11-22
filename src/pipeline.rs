//! Library-level orchestration for Phase 1.
//!
//! [`AnalysisRun`] allocates a process-unique `RunId`, creates independent
//! [`AnalysisUnit`]s in deterministic order, and offers the two mutually
//! exclusive modes: plain-LSL Legacy (no prepared source or map) and prepared
//! identity (exactly one prepared source/map pair).

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use crate::diagnostic::resolve::RenderedDiagnostic;
use crate::preprocessor::config::PreprocessOptions;
use crate::preprocessor::identity::preprocess_identity;
use crate::preprocessor::preprocess_with_includes;
use crate::source::file::{AnalysisUnitId, RunId};
use crate::source::manager::{SourceLoadError, SourceManager, MAX_ANALYSIS_UNITS_PER_INVOCATION};
use crate::source::map::SpanError;
use crate::source::unit::AnalysisUnit;

/// Failure to allocate a process-unique [`RunId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunIdAllocationError {
    /// The `u64` counter is exhausted; no identifier may be reused.
    CounterExhausted,
    /// The allocator mutex was poisoned by a panicking thread.
    AllocatorPoisoned,
}

impl fmt::Display for RunIdAllocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CounterExhausted => formatter.write_str("run id counter is exhausted"),
            Self::AllocatorPoisoned => formatter.write_str("run id allocator is poisoned"),
        }
    }
}

impl std::error::Error for RunIdAllocationError {}

#[derive(Debug)]
struct RunIdAllocator {
    next: Mutex<u64>,
}

impl RunIdAllocator {
    #[cfg(test)]
    fn new(next: u64) -> Self {
        Self {
            next: Mutex::new(next),
        }
    }

    fn allocate(&self) -> Result<RunId, RunIdAllocationError> {
        let mut next = self
            .next
            .lock()
            .map_err(|_| RunIdAllocationError::AllocatorPoisoned)?;
        let allocated = *next;
        let successor = allocated
            .checked_add(1)
            .ok_or(RunIdAllocationError::CounterExhausted)?;
        *next = successor;
        Ok(RunId::new(allocated))
    }
}

static RUN_ID_ALLOCATOR: RunIdAllocator = RunIdAllocator {
    next: Mutex::new(0),
};

/// Allocate a process-unique `RunId`.
pub fn allocate_run_id() -> Result<RunId, RunIdAllocationError> {
    RUN_ID_ALLOCATOR.allocate()
}

/// Failure to set up an analysis unit.
#[derive(Debug)]
pub enum SetupError {
    /// Run id allocation failed before the unit was created.
    RunId(RunIdAllocationError),
    /// Root source loading failed.
    Source(SourceLoadError),
    /// Identity preprocessing failed.
    Map(SpanError),
    /// The invocation reached its unit limit.
    TooManyUnits {
        /// Configured limit.
        limit: usize,
    },
}

impl fmt::Display for SetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RunId(error) => write!(formatter, "{error}"),
            Self::Source(error) => write!(formatter, "{error}"),
            Self::Map(error) => write!(formatter, "identity preprocessing failed: {error}"),
            Self::TooManyUnits { limit } => {
                write!(formatter, "invocation exceeds {limit} analysis units")
            }
        }
    }
}

impl std::error::Error for SetupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RunId(error) => Some(error),
            Self::Source(error) => Some(error),
            Self::Map(error) => Some(error),
            Self::TooManyUnits { .. } => None,
        }
    }
}

impl From<SourceLoadError> for SetupError {
    fn from(error: SourceLoadError) -> Self {
        Self::Source(error)
    }
}

impl From<SpanError> for SetupError {
    fn from(error: SpanError) -> Self {
        Self::Map(error)
    }
}

/// One library-level analysis invocation.
#[derive(Debug)]
pub struct AnalysisRun {
    run: RunId,
    units: Vec<AnalysisUnit>,
}

impl AnalysisRun {
    /// Allocate a run id before creating any unit.
    pub fn new() -> Result<Self, RunIdAllocationError> {
        Ok(Self {
            run: allocate_run_id()?,
            units: Vec::new(),
        })
    }

    /// Run identity.
    pub fn run_id(&self) -> RunId {
        self.run
    }

    /// Units in deterministic input order.
    pub fn units(&self) -> &[AnalysisUnit] {
        &self.units
    }

    /// Unit by index.
    pub fn unit(&self, index: usize) -> Option<&AnalysisUnit> {
        self.units.get(index)
    }

    /// Add a plain-LSL Legacy unit from a regular local file.
    pub fn add_legacy_file(&mut self, path: impl AsRef<Path>) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        manager.load_root_file(path)?;
        self.units.push(AnalysisUnit::new_legacy(unit_id, manager));
        Ok(index)
    }

    /// Add a plain-LSL Legacy unit from supplied text.
    pub fn add_legacy_text(&mut self, display_name: &str, text: &str) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        manager.load_root_text(display_name, text)?;
        self.units.push(AnalysisUnit::new_legacy(unit_id, manager));
        Ok(index)
    }

    /// Add a plain-LSL Legacy unit from a supplied reader.
    pub fn add_legacy_reader(
        &mut self,
        display_name: &str,
        reader: impl Read,
    ) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        manager.load_root_reader(display_name, reader)?;
        self.units.push(AnalysisUnit::new_legacy(unit_id, manager));
        Ok(index)
    }

    /// Add an identity-prepared unit from a regular local file.
    pub fn add_prepared_file(&mut self, path: impl AsRef<Path>) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        let root = manager.load_root_file(path)?;
        let prepared = preprocess_identity(&manager, root)?;
        self.units
            .push(AnalysisUnit::new_prepared(unit_id, manager, prepared));
        Ok(index)
    }

    /// Add an identity-prepared unit from supplied text.
    pub fn add_prepared_text(
        &mut self,
        display_name: &str,
        text: &str,
    ) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        let root = manager.load_root_text(display_name, text)?;
        let prepared = preprocess_identity(&manager, root)?;
        self.units
            .push(AnalysisUnit::new_prepared(unit_id, manager, prepared));
        Ok(index)
    }

    /// Add an identity-prepared unit from a supplied reader.
    pub fn add_prepared_reader(
        &mut self,
        display_name: &str,
        reader: impl Read,
    ) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        let root = manager.load_root_reader(display_name, reader)?;
        let prepared = preprocess_identity(&manager, root)?;
        self.units
            .push(AnalysisUnit::new_prepared(unit_id, manager, prepared));
        Ok(index)
    }

    /// Add a fully preprocessed unit from a regular local file.
    pub fn add_preprocessed_file(
        &mut self,
        path: impl AsRef<Path>,
        options: PreprocessOptions,
    ) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        let root = manager.load_root_file(path)?;
        let prepared = preprocess_with_includes(&mut manager, root, options)?;
        self.units
            .push(AnalysisUnit::new_prepared(unit_id, manager, prepared));
        Ok(index)
    }

    /// Add a fully preprocessed unit from supplied text (for example stdin).
    pub fn add_preprocessed_text(
        &mut self,
        display_name: &str,
        text: &str,
        options: PreprocessOptions,
    ) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        let root = manager.load_root_text(display_name, text)?;
        let prepared = preprocess_with_includes(&mut manager, root, options)?;
        self.units
            .push(AnalysisUnit::new_prepared(unit_id, manager, prepared));
        Ok(index)
    }

    /// Add a fully preprocessed unit from a supplied reader.
    pub fn add_preprocessed_reader(
        &mut self,
        display_name: &str,
        reader: impl Read,
        options: PreprocessOptions,
    ) -> Result<usize, SetupError> {
        let (index, unit_id, mut manager) = self.begin_unit()?;
        let root = manager.load_root_reader(display_name, reader)?;
        let prepared = preprocess_with_includes(&mut manager, root, options)?;
        self.units
            .push(AnalysisUnit::new_prepared(unit_id, manager, prepared));
        Ok(index)
    }

    /// Analyze every unit independently and concatenate rendered diagnostics in
    /// unit order. This does not change CLI behavior; the CLI does not call it.
    pub fn analyze(&self) -> Result<Vec<RenderedDiagnostic>, SpanError> {
        let mut rendered = Vec::new();
        for unit in &self.units {
            rendered.extend(unit.analyze()?);
        }
        Ok(rendered)
    }

    fn begin_unit(&self) -> Result<(usize, AnalysisUnitId, SourceManager), SetupError> {
        let index = self.units.len();
        if index >= MAX_ANALYSIS_UNITS_PER_INVOCATION {
            return Err(SetupError::TooManyUnits {
                limit: MAX_ANALYSIS_UNITS_PER_INVOCATION,
            });
        }
        let index_u32 = u32::try_from(index).map_err(|_| SetupError::TooManyUnits {
            limit: MAX_ANALYSIS_UNITS_PER_INVOCATION,
        })?;
        let unit_id = AnalysisUnitId::new(self.run, index_u32);
        Ok((index, unit_id, SourceManager::new(unit_id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Span;
    use std::collections::HashSet;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn span(start: usize, end: usize) -> Span {
        Span {
            start,
            end,
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        }
    }

    #[test]
    fn unit_indexes_are_deterministic_within_a_run() {
        let mut run = AnalysisRun::new().unwrap();
        let first = run.add_legacy_text("a.lsl", "default {}").unwrap();
        let second = run.add_prepared_text("b.lsl", "default {}").unwrap();
        assert_eq!(first, 0);
        assert_eq!(second, 1);
        assert_eq!(run.unit(first).unwrap().id().index(), 0);
        assert_eq!(run.unit(second).unwrap().id().index(), 1);
        assert!(run.unit(first).unwrap().legacy_root().is_some());
        assert!(run.unit(second).unwrap().legacy_root().is_none());
    }

    #[test]
    fn independent_runs_reject_cross_run_coordinates() {
        let mut first = AnalysisRun::new().unwrap();
        let mut second = AnalysisRun::new().unwrap();
        let first_index = first.add_prepared_text("a.lsl", "abc").unwrap();
        let second_index = second.add_prepared_text("b.lsl", "abc").unwrap();
        let first_unit = first.unit(first_index).unwrap();
        let second_unit = second.unit(second_index).unwrap();
        assert_ne!(first.run_id(), second.run_id());
        assert_eq!(first_unit.id().index(), second_unit.id().index());

        let first_map = first_unit.source_map().unwrap();
        let second_map = second_unit.source_map().unwrap();
        let first_span = first_map
            .source_span(first_unit.root().unwrap(), 0, 1)
            .unwrap();
        let first_prepared = first_unit
            .prepared_source()
            .unwrap()
            .prepared_span(span(0, 1))
            .unwrap();
        assert_eq!(
            second_map.location(first_span),
            Err(SpanError::ForeignAnalysisUnit)
        );
        assert_eq!(
            second_map.resolve_span(first_prepared),
            Err(SpanError::ForeignPreparedSource)
        );

        // The reverse direction is symmetric.
        let second_span = second_map
            .source_span(second_unit.root().unwrap(), 0, 1)
            .unwrap();
        assert_eq!(
            first_map.location(second_span),
            Err(SpanError::ForeignAnalysisUnit)
        );
    }

    #[test]
    fn concurrent_runs_allocate_unique_ids_and_reject_foreign_coordinates() {
        const THREADS: usize = 8;
        let barrier = Arc::new(Barrier::new(THREADS));
        let mut handles = Vec::new();
        for index in 0..THREADS {
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                let mut run = AnalysisRun::new().unwrap();
                let unit_index = run
                    .add_prepared_text(&format!("{index}.lsl"), "abc")
                    .unwrap();
                let unit = run.unit(unit_index).unwrap();
                let map = Arc::clone(unit.prepared_source().unwrap().shared_map());
                let root = unit.root().unwrap();
                let source_span = map.source_span(root, 0, 1).unwrap();
                let prepared_span = unit
                    .prepared_source()
                    .unwrap()
                    .prepared_span(span(0, 1))
                    .unwrap();
                (run.run_id(), map, source_span, prepared_span)
            }));
        }
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.join().unwrap());
        }
        let unique: HashSet<_> = results.iter().map(|result| result.0).collect();
        assert_eq!(unique.len(), THREADS);
        for (index, (_, map, source_span, prepared_span)) in results.iter().enumerate() {
            for (other_index, (_, other_map, _, _)) in results.iter().enumerate() {
                if index == other_index {
                    continue;
                }
                assert_eq!(
                    other_map.resolve_span(*prepared_span),
                    Err(SpanError::ForeignPreparedSource)
                );
                assert_eq!(
                    other_map.location(*source_span),
                    Err(SpanError::ForeignAnalysisUnit)
                );
                assert!(map.unit() != other_map.unit());
            }
        }
    }

    #[test]
    fn legacy_analysis_matches_frozen_pipeline_exactly() {
        let text = "default { state_entry() { integer x = ; } }";
        let mut run = AnalysisRun::new().unwrap();
        let index = run.add_legacy_text("root.lsl", text).unwrap();
        let rendered = run.unit(index).unwrap().analyze().unwrap();

        let (tokens, lexical) = crate::lex(text);
        let syntax = crate::parse_legacy(&tokens);
        let semantic = crate::semantic::analyze_with_syntax_diagnostics(&tokens, &syntax);
        let expected: Vec<_> = lexical.into_iter().chain(syntax).chain(semantic).collect();
        assert_eq!(rendered.len(), expected.len());
        for (rendered, expected) in rendered.iter().zip(expected.iter()) {
            assert_eq!(rendered.diagnostic(), expected);
            assert_eq!(rendered.source(), run.unit(index).unwrap().root().unwrap());
            assert!(rendered.related().is_empty());
        }
    }

    #[test]
    fn prepared_identity_analysis_preserves_baseline_spans() {
        let text = "default { state_entry() { integer x = ; } }";
        let mut run = AnalysisRun::new().unwrap();
        let index = run.add_prepared_text("root.lsl", text).unwrap();
        let rendered = run.unit(index).unwrap().analyze().unwrap();
        let (tokens, lexical) = crate::lex(text);
        let syntax = crate::parse_legacy(&tokens);
        let semantic = crate::semantic::analyze_with_syntax_diagnostics(&tokens, &syntax);
        let expected: Vec<_> = lexical.into_iter().chain(syntax).chain(semantic).collect();
        assert_eq!(rendered.len(), expected.len());
        for (rendered, expected) in rendered.iter().zip(expected.iter()) {
            assert_eq!(rendered.diagnostic(), expected);
        }
        assert!(run.analyze().is_ok());
        assert!(run.unit(index).unwrap().prepared_source().is_some());
        assert!(run.unit(index).unwrap().legacy_root().is_none());
    }

    #[test]
    fn isolated_allocator_exhaustion_does_not_wrap_or_reuse() {
        let allocator = RunIdAllocator::new(u64::MAX);
        assert_eq!(
            allocator.allocate().unwrap_err(),
            RunIdAllocationError::CounterExhausted
        );
        assert_eq!(
            allocator.allocate().unwrap_err(),
            RunIdAllocationError::CounterExhausted
        );
    }

    #[test]
    fn isolated_allocator_poisoning_returns_explicit_error() {
        let allocator = Arc::new(RunIdAllocator::new(0));
        let worker_allocator = Arc::clone(&allocator);
        let worker = thread::spawn(move || {
            let _guard = worker_allocator.next.lock().unwrap();
            panic!("poison isolated allocator");
        });
        assert!(worker.join().is_err());
        assert_eq!(
            allocator.allocate().unwrap_err(),
            RunIdAllocationError::AllocatorPoisoned
        );
        // The process-wide allocator is independent and still usable.
        assert!(allocate_run_id().is_ok());
        assert!(allocator.allocate().is_err());
    }
}

#[cfg(test)]
mod phase3_total_diagnostic_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessOptions;
    use crate::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;

    #[test]
    fn total_rendered_diagnostics_are_capped_with_one_terminal_diagnostic() {
        let mut source = String::new();
        for _ in 0..MAX_TOTAL_DIAGNOSTICS + 100 {
            source.push_str("integer duplicate;\n");
        }
        let mut run = AnalysisRun::new().unwrap();
        let index = run
            .add_preprocessed_text("root.lsl", &source, PreprocessOptions::full())
            .unwrap();
        let rendered = run.unit(index).unwrap().analyze().unwrap();
        assert_eq!(rendered.len(), MAX_TOTAL_DIAGNOSTICS);
        assert_eq!(rendered.last().unwrap().diagnostic().code, "PRE039");
    }
}
