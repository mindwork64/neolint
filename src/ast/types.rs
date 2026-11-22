//! Type and operator vocabulary for the typed LSL AST.

use crate::diagnostic::Span;

/// One of the seven LSL value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// 32-bit signed integer.
    Integer,
    /// Floating-point value.
    Float,
    /// String value.
    String,
    /// Key value.
    Key,
    /// Heterogeneous list.
    List,
    /// Three-component vector.
    Vector,
    /// Four-component rotation.
    Rotation,
}

/// A source-written LSL type with its exact span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Type {
    /// Type category.
    pub kind: TypeKind,
    /// Source range of the type keyword.
    pub span: Span,
}

/// Prefix and unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    /// Unary plus.
    Plus,
    /// Unary minus.
    Minus,
    /// Logical negation.
    Bang,
    /// Bitwise complement.
    Tilde,
}

/// Binary operators in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    /// Logical OR.
    OrOr,
    /// Logical AND.
    AndAnd,
    /// Bitwise OR.
    BitOr,
    /// Bitwise XOR.
    BitXor,
    /// Bitwise AND.
    BitAnd,
    /// Equality.
    Equal,
    /// Inequality.
    NotEqual,
    /// Less than.
    Less,
    /// Less than or equal.
    LessEqual,
    /// Greater than.
    Greater,
    /// Greater than or equal.
    GreaterEqual,
    /// Left shift.
    ShiftLeft,
    /// Right shift.
    ShiftRight,
    /// Addition.
    Add,
    /// Subtraction.
    Subtract,
    /// Multiplication.
    Multiply,
    /// Division.
    Divide,
    /// Remainder.
    Remainder,
}

/// Assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOperator {
    /// `=`
    Assign,
    /// `+=`
    AddAssign,
    /// `-=`
    SubtractAssign,
    /// `*=`
    MultiplyAssign,
    /// `/=`
    DivideAssign,
    /// `%=`
    RemainderAssign,
}

/// Prefix or postfix increment/decrement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncrementOperator {
    /// `++`
    Increment,
    /// `--`
    Decrement,
}
