//! Opaque AnalysisUnit handle.
//!
//! A unit owns its original sources and either prepared mode (one
//! `PreparedSource`/`SourceMap` pair) or plain-LSL Legacy mode (neither).

use crate::diagnostic::resolve::{LegacyDiagnostic, PreparedDiagnostic, RenderedDiagnostic};
use crate::diagnostic::Diagnostic;
use crate::preprocessor::PreparedSource;
use crate::semantic::analyze_with_syntax_diagnostics;
use crate::source::file::{AnalysisUnitId, SourceFileKind, SourceId};
use crate::source::manager::SourceManager;
use crate::source::map::SpanError;

/// One independent analysis of a single root source.
#[derive(Debug)]
pub struct AnalysisUnit {
    id: AnalysisUnitId,
    manager: SourceManager,
    prepared: Option<PreparedSource>,
    diagnostics: Vec<Diagnostic>,
}

impl AnalysisUnit {
    pub(crate) fn new_legacy(id: AnalysisUnitId, manager: SourceManager) -> Self {
        Self {
            id,
            manager,
            prepared: None,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn new_prepared(
        id: AnalysisUnitId,
        manager: SourceManager,
        prepared: PreparedSource,
    ) -> Self {
        Self {
            id,
            manager,
            prepared: Some(prepared),
            diagnostics: Vec::new(),
        }
    }

    /// Unit identity.
    pub fn id(&self) -> AnalysisUnitId {
        self.id
    }

    /// Original source manager for this unit.
    pub fn manager(&self) -> &SourceManager {
        &self.manager
    }

    /// Root source identity, without the Legacy-mode filter.
    pub fn root(&self) -> Option<SourceId> {
        self.manager.root()
    }

    /// Root source for the plain-LSL Legacy path.
    ///
    /// Returns `Some` only for a unit with no prepared pipeline and a root file
    /// that is not an include. This is the only source accepted by
    /// [`RenderedDiagnostic::from_legacy`](crate::diagnostic::resolve::RenderedDiagnostic::from_legacy).
    pub fn legacy_root(&self) -> Option<SourceId> {
        if self.prepared.is_some() {
            return None;
        }
        let root = self.manager.root()?;
        let file = self.manager.file(root)?;
        if file.kind() == SourceFileKind::Included {
            return None;
        }
        Some(root)
    }

    /// Prepared pipeline, if this unit uses it.
    pub fn prepared_source(&self) -> Option<&PreparedSource> {
        self.prepared.as_ref()
    }

    /// Source map, if this unit uses the prepared pipeline.
    pub fn source_map(&self) -> Option<&crate::source::map::SourceMap> {
        self.prepared.as_ref().map(PreparedSource::map)
    }

    /// Unit-local frozen diagnostics collected by the pipeline.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Run the existing lexer/parser/semantic pipeline over this unit and
    /// resolve its diagnostics.
    ///
    /// Legacy units use the no-map path and keep frozen spans untouched.
    /// Prepared units bridge prepared spans through the prepared source and map.
    /// Preprocessor diagnostics, when present, are converted first.
    pub fn analyze(&self) -> Result<Vec<RenderedDiagnostic>, SpanError> {
        if let Some(prepared) = &self.prepared {
            let map = prepared.map();
            let mut rendered = Vec::new();
            for diagnostic in prepared.preprocess_diagnostics() {
                rendered.push(RenderedDiagnostic::from_preprocess(
                    diagnostic.clone(),
                    map,
                )?);
            }
            let text = prepared.text();
            let (tokens, lexical) = crate::lex(text);
            let syntax = crate::parse_legacy(&tokens);
            let semantic = analyze_with_syntax_diagnostics(&tokens, &syntax);
            for diagnostic in lexical.into_iter().chain(syntax).chain(semantic) {
                rendered.push(RenderedDiagnostic::from_prepared(
                    PreparedDiagnostic::new(
                        diagnostic.code,
                        diagnostic.severity,
                        diagnostic.kind,
                        diagnostic.message,
                        diagnostic.span,
                    ),
                    prepared,
                    map,
                )?);
            }
            return Ok(rendered);
        }

        let root = self.legacy_root().ok_or(SpanError::NotLegacyUnit)?;
        let text = self.manager.text(root).ok_or(SpanError::UnknownSourceId)?;
        let (tokens, lexical) = crate::lex(text);
        let syntax = crate::parse_legacy(&tokens);
        let semantic = analyze_with_syntax_diagnostics(&tokens, &syntax);
        let mut rendered = Vec::new();
        for diagnostic in lexical.into_iter().chain(syntax).chain(semantic) {
            let tagged = self.legacy_diagnostic(diagnostic)?;
            rendered.push(RenderedDiagnostic::from_legacy(tagged, self)?);
        }
        Ok(rendered)
    }

    /// Tag a frozen legacy diagnostic with this unit.
    ///
    /// The returned value always carries this unit's id; callers cannot attach
    /// it to another unit. Prepared units return `NotLegacyUnit`.
    pub fn legacy_diagnostic(&self, diagnostic: Diagnostic) -> Result<LegacyDiagnostic, SpanError> {
        if self.legacy_root().is_none() {
            return Err(SpanError::NotLegacyUnit);
        }
        Ok(LegacyDiagnostic::new(diagnostic, self.id))
    }
}
