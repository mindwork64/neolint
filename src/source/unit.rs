//! Opaque AnalysisUnit handle.
//!
//! A unit owns its original sources and either prepared mode (one
//! `PreparedSource`/`SourceMap` pair) or plain-LSL Legacy mode (neither).

use crate::diagnostic::resolve::{LegacyDiagnostic, PreparedDiagnostic, RenderedDiagnostic};
use crate::diagnostic::{Diagnostic, Severity};
use crate::engine::compare::compare_semantic_diagnostics;
use crate::engine::wiring::{
    analyze_ast as analyze_ast_semantics, analyze_legacy as analyze_legacy_semantics, AstSyntax,
    LegacySyntax,
};
use crate::engine::{
    AnalysisEngine, DisplaySuppression, EngineAnalysisError, EngineAnalysisOptions,
    UnitEngineAnalysis,
};
use crate::preprocessor::PreparedSource;
use crate::semantic::analyze_with_syntax_diagnostics;
use crate::source::file::{AnalysisUnitId, SourceFileKind, SourceId};
use crate::source::manager::SourceManager;
use crate::source::map::SpanError;
use std::sync::Arc;

/// One independent analysis of a single root source.
#[derive(Debug)]
pub struct AnalysisUnit {
    id: AnalysisUnitId,
    manager: SourceManager,
    prepared: Option<PreparedSource>,
}

impl AnalysisUnit {
    pub(crate) fn new_legacy(id: AnalysisUnitId, manager: SourceManager) -> Self {
        Self {
            id,
            manager,
            prepared: None,
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

    /// Run the existing lexer/parser/semantic pipeline over this unit and
    /// resolve its diagnostics.
    ///
    /// Legacy units use the no-map path and keep frozen spans untouched.
    /// Prepared units bridge prepared spans through the prepared source and map.
    /// Preprocessor diagnostics, when present, are converted first.
    pub fn analyze(&self) -> Result<Vec<RenderedDiagnostic>, SpanError> {
        let suppression = DisplaySuppression::default();
        if let Some(prepared) = &self.prepared {
            return Ok(self
                .analyze_prepared(prepared, AnalysisEngine::Legacy, false, &suppression)?
                .into_diagnostics());
        }
        let (rendered, _tokens) = self.analyze_plain(false, &suppression)?;
        Ok(rendered)
    }

    /// Run an engine-aware analysis with default options.
    ///
    /// `Ast` and `Compare` require a prepared source; a plain no-map unit is
    /// rejected before any analysis work, leaving the unit untouched.
    pub fn analyze_with_engine(
        &self,
        engine: AnalysisEngine,
    ) -> Result<UnitEngineAnalysis, EngineAnalysisError> {
        self.analyze_with_engine_options(engine, &EngineAnalysisOptions::default())
    }

    /// Run an engine-aware analysis with explicit options.
    pub fn analyze_with_engine_options(
        &self,
        engine: AnalysisEngine,
        options: &EngineAnalysisOptions,
    ) -> Result<UnitEngineAnalysis, EngineAnalysisError> {
        self.analyze_with_engine_options_suppressed(engine, options, &DisplaySuppression::default())
    }

    /// Run an engine-aware analysis with explicit options and display suppression.
    ///
    /// Suppressed diagnostics are dropped before the rendered-diagnostic cap is
    /// evaluated, so a disabled rule cannot cause the terminal `PRE039`
    /// overflow diagnostic merely by exceeding the cap. The frozen
    /// [`AnalysisUnit::analyze`] and [`AnalysisUnit::analyze_with_engine_options`]
    /// entry points apply no suppression and keep their existing cap behavior.
    pub fn analyze_with_engine_options_suppressed(
        &self,
        engine: AnalysisEngine,
        options: &EngineAnalysisOptions,
        suppression: &DisplaySuppression,
    ) -> Result<UnitEngineAnalysis, EngineAnalysisError> {
        if let Some(prepared) = &self.prepared {
            return Ok(self.analyze_prepared(
                prepared,
                engine,
                options.collect_tokens,
                suppression,
            )?);
        }
        if engine.requires_prepared_source() {
            return Err(EngineAnalysisError::PreparedSourceRequired { unit: self.id });
        }
        let (diagnostics, tokens) = self.analyze_plain(options.collect_tokens, suppression)?;
        Ok(UnitEngineAnalysis::new(diagnostics, None, tokens))
    }

    /// Plain no-map Legacy path, shared by `analyze` and the engine API.
    fn analyze_plain(
        &self,
        collect_tokens: bool,
        suppression: &DisplaySuppression,
    ) -> Result<(Vec<RenderedDiagnostic>, Vec<crate::lexer::Token>), SpanError> {
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
        if !suppression.is_empty() {
            rendered.retain(|diagnostic| !suppression.is_suppressed(diagnostic.diagnostic().code));
        }
        let limit = crate::preprocessor::diagnostics::MAX_TOTAL_DIAGNOSTICS;
        if rendered.len() > limit {
            rendered.truncate(limit - 1);
            let index = crate::source::line_index::LineIndex::new(Arc::from(text));
            let position =
                index
                    .location(text.len())
                    .unwrap_or(crate::source::line_index::SourcePosition {
                        offset: text.len(),
                        line: 1,
                        column: 1,
                    });
            let diagnostic = Diagnostic {
                code: "PRE039",
                severity: crate::diagnostic::Severity::Error,
                kind: crate::diagnostic::DiagnosticKind::Syntax,
                message: format!("total diagnostic limit ({limit}) reached"),
                span: crate::diagnostic::Span {
                    start: text.len(),
                    end: text.len(),
                    line: position.line,
                    column: position.column,
                    end_line: position.line,
                    end_column: position.column,
                },
            };
            let tagged = self.legacy_diagnostic(diagnostic)?;
            rendered.push(RenderedDiagnostic::from_legacy(tagged, self)?);
        }
        let tokens = if collect_tokens { tokens } else { Vec::new() };
        Ok((rendered, tokens))
    }

    /// Prepared path, shared by `analyze` and the engine API.
    ///
    /// The prepared text is lexed exactly once and the same immutable token
    /// slice is handed to every parser/semantic stage. Parser diagnostics are
    /// kept engine-local through the private `LegacySyntax`/`AstSyntax`
    /// wrappers. The AST semantic stream is never appended to a Compare display
    /// stream; display keeps the frozen Legacy stream.
    fn analyze_prepared(
        &self,
        prepared: &PreparedSource,
        engine: AnalysisEngine,
        collect_tokens: bool,
        suppression: &DisplaySuppression,
    ) -> Result<UnitEngineAnalysis, SpanError> {
        let mut rendered = Vec::new();
        for diagnostic in prepared.preprocess_diagnostics() {
            rendered.push(RenderedDiagnostic::from_preprocess(
                diagnostic.clone(),
                prepared,
            )?);
        }
        let text = prepared.text();
        let (tokens, lexical) = crate::lex(text);
        // Error-severity preprocessing may have dropped source before this text
        // was produced, and error-severity lexical diagnostics mean parts of it
        // were not tokenized. Either makes the Phase 7 read/use and reachability
        // rules fail closed. Informational/warning notices do not.
        let environment_reliable = !prepared
            .preprocess_diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
            && !lexical
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error);
        let options = crate::ast::AstParseOptions::from_flags(
            prepared.switches_enabled(),
            prepared.lazy_lists_enabled(),
        );
        let mut comparison = None;
        let display: Vec<Diagnostic> = match engine {
            AnalysisEngine::Legacy => {
                let legacy_syntax = LegacySyntax::parse(&tokens);
                let legacy_semantic = analyze_legacy_semantics(&tokens, &legacy_syntax);
                let mut display = lexical;
                display.extend(legacy_syntax.into_diagnostics());
                display.extend(legacy_semantic);
                display
            }
            AnalysisEngine::Ast => {
                let (ast, ast_syntax) = AstSyntax::parse(&tokens, options);
                let ast_semantic = analyze_ast_semantics(&ast, &ast_syntax, environment_reliable);
                let mut display = lexical;
                display.extend(ast_syntax.into_diagnostics());
                display.extend(ast_semantic);
                display
            }
            AnalysisEngine::Compare => {
                let legacy_syntax = LegacySyntax::parse(&tokens);
                let legacy_semantic = analyze_legacy_semantics(&tokens, &legacy_syntax);
                let (ast, ast_syntax) = AstSyntax::parse(&tokens, options);
                let ast_semantic = analyze_ast_semantics(&ast, &ast_syntax, environment_reliable);
                comparison = Some(compare_semantic_diagnostics(
                    &legacy_semantic,
                    &ast_semantic,
                ));
                let mut display = lexical;
                display.extend(legacy_syntax.into_diagnostics());
                display.extend(legacy_semantic);
                display
            }
        };
        // Validate the prepared source once per unit: the check compares the whole
        // prepared buffer, so doing it per diagnostic made rendering quadratic in
        // the number of diagnostics.
        RenderedDiagnostic::validate(prepared)?;
        for diagnostic in display {
            rendered.push(RenderedDiagnostic::from_prepared_validated(
                PreparedDiagnostic::new(
                    diagnostic.code,
                    diagnostic.severity,
                    diagnostic.kind,
                    diagnostic.message,
                    diagnostic.span,
                ),
                prepared,
            )?);
        }
        if suppression.is_empty() {
            crate::preprocessor::diagnostics::cap_rendered(prepared, &mut rendered)?;
        } else {
            crate::preprocessor::diagnostics::cap_rendered_with(
                prepared,
                &mut rendered,
                |diagnostic| !suppression.is_suppressed(diagnostic.diagnostic().code),
            )?;
        }
        let tokens = if collect_tokens { tokens } else { Vec::new() };
        Ok(UnitEngineAnalysis::new(rendered, comparison, tokens))
    }

    /// Tag a frozen legacy diagnostic with this unit.
    ///
    /// The returned value always carries this unit's id; callers cannot attach
    /// it to another unit. Prepared units return `NotLegacyUnit`.
    pub(crate) fn legacy_diagnostic(
        &self,
        diagnostic: Diagnostic,
    ) -> Result<LegacyDiagnostic, SpanError> {
        if self.legacy_root().is_none() {
            return Err(SpanError::NotLegacyUnit);
        }
        Ok(LegacyDiagnostic::new(diagnostic, self.id))
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::ast_semantic::analyze_ast as run_ast_semantic;
    use crate::pipeline::AnalysisRun;

    fn prepared_unit(source: &str) -> (AnalysisRun, usize) {
        let mut run = AnalysisRun::new().unwrap();
        let index = run.add_prepared_text("root.lsl", source).unwrap();
        (run, index)
    }

    /// The AST engine display is exactly `lexical ++ ast_syntax ++ ast_semantic`,
    /// wired to the AST parser's own findings.
    #[test]
    fn ast_engine_display_matches_independently_wired_oracle() {
        let source =
            "integer g;\ninteger g;\ndefault { state_entry() { integer x = ; llSay(0); } }";
        let (run, index) = prepared_unit(source);
        let unit = run.unit(index).unwrap();
        let prepared = unit.prepared_source().unwrap();
        let analysis = unit.analyze_with_engine(AnalysisEngine::Ast).unwrap();

        let (tokens, lexical) = crate::lex(prepared.text());
        let (ast, ast_syntax) = crate::ast::parse_ast_with_options(
            &tokens,
            crate::ast::AstParseOptions::from_flags(
                prepared.switches_enabled(),
                prepared.lazy_lists_enabled(),
            ),
        );
        let ast_semantic = run_ast_semantic(&ast, &ast_syntax);
        let expected: Vec<Diagnostic> = lexical
            .into_iter()
            .chain(ast_syntax)
            .chain(ast_semantic)
            .collect();
        assert_eq!(analysis.diagnostics().len(), expected.len());
        for (rendered, expected) in analysis.diagnostics().iter().zip(expected.iter()) {
            assert_eq!(rendered.diagnostic(), expected);
            assert_eq!(rendered.source(), unit.root().unwrap());
        }
        assert!(analysis.comparison().is_none());
    }

    /// Compare reports exactly the comparator result over the two raw streams and
    /// displays only the Legacy stream.
    #[test]
    fn compare_engine_report_matches_raw_stream_oracle() {
        let source =
            "integer g;\ninteger g;\ndefault { state_entry() { integer x = ; llSay(0); } }";
        let (run, index) = prepared_unit(source);
        let unit = run.unit(index).unwrap();
        let prepared = unit.prepared_source().unwrap();
        let analysis = unit.analyze_with_engine(AnalysisEngine::Compare).unwrap();

        let (tokens, lexical) = crate::lex(prepared.text());
        let legacy_syntax = crate::parse_legacy(&tokens);
        let legacy_semantic =
            crate::semantic::analyze_with_syntax_diagnostics(&tokens, &legacy_syntax);
        let (ast, ast_syntax) = crate::ast::parse_ast_with_options(
            &tokens,
            crate::ast::AstParseOptions::from_flags(
                prepared.switches_enabled(),
                prepared.lazy_lists_enabled(),
            ),
        );
        let ast_semantic = run_ast_semantic(&ast, &ast_syntax);
        let oracle = compare_semantic_diagnostics(&legacy_semantic, &ast_semantic);
        assert_eq!(analysis.comparison(), Some(&oracle));

        let expected: Vec<Diagnostic> = lexical
            .into_iter()
            .chain(legacy_syntax)
            .chain(legacy_semantic)
            .collect();
        assert_eq!(analysis.diagnostics().len(), expected.len());
        for (rendered, expected) in analysis.diagnostics().iter().zip(expected.iter()) {
            assert_eq!(rendered.diagnostic(), expected);
        }
    }

    /// The refactored `analyze()` keeps the frozen Legacy diagnostic stream
    /// identical to `analyze_with_engine(Legacy)`.
    #[test]
    fn legacy_engine_display_is_unchanged_by_engine_refactor() {
        let source =
            "integer g;\ninteger g;\ndefault { state_entry() { integer x = ; llSay(0); } }";
        let (run, index) = prepared_unit(source);
        let unit = run.unit(index).unwrap();
        let plain = unit.analyze().unwrap();
        let legacy = unit.analyze_with_engine(AnalysisEngine::Legacy).unwrap();
        assert_eq!(plain.len(), legacy.diagnostics().len());
        for (plain, legacy) in plain.iter().zip(legacy.diagnostics().iter()) {
            assert_eq!(plain.diagnostic(), legacy.diagnostic());
            assert_eq!(plain.source(), legacy.source());
            assert_eq!(plain.related(), legacy.related());
        }
        assert!(legacy.comparison().is_none());
    }
}
