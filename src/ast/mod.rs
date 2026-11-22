//! Typed LSL abstract syntax tree and the independent AST parser.

pub mod node;
pub mod parser;
pub mod types;

pub use node::*;
pub use parser::{parse_ast, AST_RECURSION_LIMIT, AST_WORKER_UNAVAILABLE, MAX_AST_RECURSION_DEPTH};
pub use types::{
    AssignmentOperator, BinaryOperator, IncrementOperator, Type, TypeKind, UnaryOperator,
};
