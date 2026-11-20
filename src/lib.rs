#![forbid(unsafe_code)]

//! Core library for the CLI-only LSL linter.

pub mod builtins;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod semantic;

pub use diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
pub use lexer::{lex, Token, TokenKind};
pub use parser::parse;
pub use semantic::analyze;
