#![forbid(unsafe_code)]

//! Core library for the CLI-only LSL linter.

pub mod diagnostic;
pub mod lexer;

pub use diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
pub use lexer::{lex, Token, TokenKind};
