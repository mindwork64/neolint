#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Core library for the CLI-only LSL linter.
//!
//! The crate exposes the lexical, syntactic, and conservative semantic pipeline
//! without constructing a full AST. [`lex`] turns UTF-8 source into tokens and
//! lexical diagnostics; [`parse`] validates the token stream and returns syntax
//! diagnostics plus recovery information; [`analyze`] performs scope, type,
//! control-flow, built-in, and state/event checks.
//!
//! Normal analysis is deterministic and offline. Unknown extension APIs,
//! unresolved expressions, and unverified language behavior are deliberately
//! left unchecked rather than guessed.

pub mod builtins;
pub mod config;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod pipeline;
pub mod preprocessor;
pub mod semantic;
pub mod source;

pub use diagnostic::resolve::{
    LegacyDiagnostic, PreparedDiagnostic, RenderedDiagnostic, UnifiedDiagnosticInput,
};
pub use diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
pub use lexer::{lex, Token, TokenKind};
pub use parser::{parse, parse_legacy};
pub use pipeline::{allocate_run_id, AnalysisRun, RunIdAllocationError, SetupError};
pub use preprocessor::{
    preprocess, preprocess_identity, PreparedSource, PreprocessConfig, PreprocessDiagnostic,
    PreprocessMode, PreprocessOptions, RelatedLocation,
};
pub use semantic::analyze;
pub use source::{
    AnalysisUnit, AnalysisUnitId, LineIndex, MapSegment, OriginId, OriginKind, PreparedSpan,
    ResolvedSpan, RunId, SourceFile, SourceFileKind, SourceId, SourceLoadError, SourceLocation,
    SourceManager, SourceMap, SourceMapBuilder, SourceOrigin, SourceSpan, SpanError,
};
