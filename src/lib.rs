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
pub mod semantic;

pub use diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
pub use lexer::{lex, Token, TokenKind};
pub use parser::parse;
pub use semantic::analyze;
