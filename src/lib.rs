#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Core library for the CLI-only LSL linter.
//!
//! The crate exposes the established lexical, syntactic, and conservative
//! semantic pipeline alongside an independent typed AST parser. [`lex`] turns
//! UTF-8 source into tokens and lexical diagnostics; [`parse`] validates the token
//! stream and returns Legacy syntax diagnostics plus recovery information;
//! [`parse_ast`] builds a typed syntax tree without replacing the Legacy path;
//! [`analyze`] performs scope, type, control-flow, built-in, and state/event checks.
//!
//! Normal analysis is deterministic and offline. Unknown extension APIs,
//! unresolved expressions, and unverified language behavior are deliberately
//! left unchecked rather than guessed.

pub mod ast;
pub mod ast_semantic;
pub mod builtins;
pub mod config;
pub mod diagnostic;
pub mod engine;
pub mod lexer;
pub mod parser;
pub mod pipeline;
pub mod preprocessor;
pub mod semantic;
pub mod source;
pub mod target;

pub use ast::{parse_ast, parse_ast_with_options, Ast, AstParseOptions};
pub use ast_semantic::{
    analyze_ast, analyze_ast_with_environment, analyze_ast_with_target,
    analyze_ast_with_target_options,
};
pub use diagnostic::resolve::{
    LegacyDiagnostic, PreparedDiagnostic, RenderedDiagnostic, UnifiedDiagnosticInput,
};
pub use diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
pub use engine::{
    compare_semantic_diagnostics, AnalysisEngine, DiagnosticFingerprint, DisplaySuppression,
    EngineAnalysisError, EngineAnalysisOptions, EngineComparisonReport, RuleComparison,
    RunEngineAnalysis, UnitComparison, UnitEngineAnalysis, MAX_COMPARE_DETAILS_PER_RULE,
    MAX_COMPARE_FINGERPRINTS_PER_ENGINE, SEM_CODES,
};
pub use lexer::{lex, Token, TokenKind};
pub use parser::{parse, parse_legacy};
pub use pipeline::{allocate_run_id, AnalysisRun, RunIdAllocationError, SetupError};
pub use preprocessor::{
    preprocess, preprocess_identity, preprocess_with_includes, preprocess_with_settings,
    PreparedSource, PreprocessConfig, PreprocessDiagnostic, PreprocessDialect, PreprocessMode,
    PreprocessOptions, PreprocessSettings, RelatedLocation,
};
pub use semantic::analyze;
pub use source::{
    AnalysisUnit, AnalysisUnitId, LineIndex, MapSegment, OriginId, OriginKind, PreparedSpan,
    ResolvedSpan, RunId, SourceFile, SourceFileKind, SourceId, SourceLoadError, SourceLocation,
    SourceManager, SourceMap, SourceMapBuilder, SourceOrigin, SourceSpan, SpanError,
};

pub use target::{OpenSimApiVersion, OsslAllowlist, ScriptRuntime, TargetPlatform, TargetProfile};
