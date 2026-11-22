//! Typed syntax tree nodes. Every node carries a prepared `Span`.

use crate::diagnostic::Span;

use super::types::{AssignmentOperator, BinaryOperator, IncrementOperator, Type, UnaryOperator};

/// Root of one parsed LSL file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ast {
    /// Top-level declarations and states in source order.
    pub items: Vec<TopLevel>,
    /// Range from the first through the last non-EOF token.
    pub span: Span,
}

/// A top-level declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopLevel {
    /// Global variable declaration.
    Global(GlobalVariableDeclaration),
    /// User-defined function.
    Function(FunctionDeclaration),
    /// State declaration.
    State(StateDeclaration),
    /// Explicit syntax-error placeholder.
    Error(ErrorNode),
}

/// Global variable declaration with one or more declarators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalVariableDeclaration {
    /// Full declaration span.
    pub span: Span,
    /// Declared type.
    pub ty: Type,
    /// Individual variables.
    pub declarators: Vec<VariableDeclarator>,
}

/// One variable name and optional initializer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableDeclarator {
    /// Span from name through initializer.
    pub span: Span,
    /// Variable name.
    pub name: String,
    /// Name token span.
    pub name_span: Span,
    /// Optional initializer.
    pub initializer: Option<Expression>,
    /// Explicit malformed initializer, when `=` was present but parsing failed.
    pub initializer_error: Option<ErrorNode>,
}

/// User-defined function declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDeclaration {
    /// Full declaration span.
    pub span: Span,
    /// Optional return type; `None` for an untyped function.
    pub return_type: Option<Type>,
    /// Function name.
    pub name: String,
    /// Function name span.
    pub name_span: Span,
    /// Formal parameters.
    pub parameters: Vec<Parameter>,
    /// Function body; `None` when the opening block delimiter is missing.
    pub body: Option<Block>,
}

/// Function or event parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    /// Span from type through name.
    pub span: Span,
    /// Parameter type.
    pub ty: Type,
    /// Parameter name.
    pub name: String,
    /// Name span.
    pub name_span: Span,
}

/// State declaration, including `default`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDeclaration {
    /// Full state span.
    pub span: Span,
    /// State name; `default` is represented by `is_default`.
    pub name: String,
    /// State name/token span.
    pub name_span: Span,
    /// Whether the state name was missing.
    pub name_missing: bool,
    /// Whether this is the default state.
    pub is_default: bool,
    /// Whether the opening state-body brace was missing.
    pub body_missing: bool,
    /// Event handlers in source order.
    pub events: Vec<EventHandler>,
}

/// One event handler inside a state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventHandler {
    /// Full handler span.
    pub span: Span,
    /// Event name.
    pub name: String,
    /// Event name span.
    pub name_span: Span,
    /// Event parameters.
    pub parameters: Vec<Parameter>,
    /// Handler body; `None` when the opening block delimiter is missing.
    pub body: Option<Block>,
}

/// Braced statement block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Span from `{` through `}`.
    pub span: Span,
    /// Statements in source order.
    pub statements: Vec<Statement>,
}

/// One statement with a common span and typed kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// Full statement span.
    pub span: Span,
    /// Statement category.
    pub kind: StatementKind,
}

/// Typed statement categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementKind {
    /// `;`
    Empty,
    /// Nested block.
    Block(Block),
    /// Local variable declaration.
    Local(LocalVariableDeclaration),
    /// `if`/`else`.
    If(IfStatement),
    /// `for` loop.
    For(ForStatement),
    /// `while` loop.
    While(WhileStatement),
    /// `do`/`while` loop.
    DoWhile(DoWhileStatement),
    /// `return` statement.
    Return(ReturnStatement),
    /// `jump` statement.
    Jump(JumpStatement),
    /// `state` change.
    StateChange(StateChangeStatement),
    /// `@label;`.
    Label(LabelStatement),
    /// Expression statement.
    Expression(ExpressionStatement),
    /// Recovered malformed statement.
    Error(ErrorNode),
}

/// Local variable declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVariableDeclaration {
    /// Declaration span.
    pub span: Span,
    /// Declared type.
    pub ty: Type,
    /// Declarators.
    pub declarators: Vec<VariableDeclarator>,
}

/// `if` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStatement {
    /// Full statement span.
    pub span: Span,
    /// Condition.
    pub condition: Expression,
    /// Then branch.
    pub then_branch: Box<Statement>,
    /// Optional else branch.
    pub else_branch: Option<Box<Statement>>,
}

/// `for` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForStatement {
    /// Full statement span.
    pub span: Span,
    /// Initializer expressions.
    pub initializer: Vec<Expression>,
    /// Required condition.
    pub condition: Expression,
    /// Increment expressions.
    pub increment: Vec<Expression>,
    /// Loop body.
    pub body: Box<Statement>,
}

/// `while` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhileStatement {
    /// Full statement span.
    pub span: Span,
    /// Condition.
    pub condition: Expression,
    /// Loop body.
    pub body: Box<Statement>,
}

/// `do`/`while` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoWhileStatement {
    /// Full statement span.
    pub span: Span,
    /// Loop body.
    pub body: Box<Statement>,
    /// Trailing condition.
    pub condition: Expression,
}

/// `return` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnStatement {
    /// Statement span.
    pub span: Span,
    /// Optional returned expression.
    pub value: Option<Expression>,
}

/// `jump label;` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpStatement {
    /// Statement span.
    pub span: Span,
    /// Target label.
    pub label: String,
    /// Label span.
    pub label_span: Span,
}

/// `state name;` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChangeStatement {
    /// Statement span.
    pub span: Span,
    /// Target state name.
    pub state: String,
    /// Name span.
    pub state_span: Span,
}

/// `@label;` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelStatement {
    /// Statement span.
    pub span: Span,
    /// Label name.
    pub name: String,
    /// Name span.
    pub name_span: Span,
}

/// Expression statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionStatement {
    /// Statement span.
    pub span: Span,
    /// Expression.
    pub expression: Expression,
}

/// Expression with a common span and typed kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expression {
    /// Full expression span.
    pub span: Span,
    /// Expression category.
    pub kind: ExpressionKind,
}

/// Typed expression categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionKind {
    /// Variable/function name.
    Identifier(Identifier),
    /// Integer literal.
    Integer(IntegerLiteral),
    /// Floating-point literal.
    Float(FloatLiteral),
    /// String literal.
    String(StringLiteral),
    /// Parenthesized expression.
    Parenthesized(Box<Expression>),
    /// List literal.
    List(ListExpression),
    /// Vector literal.
    Vector(VectorExpression),
    /// Rotation literal.
    Rotation(RotationExpression),
    /// Function call.
    Call(CallExpression),
    /// Vector/rotation component access.
    Component(ComponentAccessExpression),
    /// Unary operation.
    Unary(UnaryExpression),
    /// Binary operation.
    Binary(BinaryExpression),
    /// Assignment or compound assignment.
    Assignment(AssignmentExpression),
    /// Explicit type cast.
    Cast(CastExpression),
    /// Prefix or postfix increment/decrement.
    Increment(IncrementExpression),
    /// Recovered malformed expression.
    Error(ErrorNode),
}

/// Identifier expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    /// Identifier name.
    pub name: String,
    /// Identifier span.
    pub span: Span,
}

/// Integer literal; `text` preserves the source spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegerLiteral {
    /// Source spelling.
    pub text: String,
    /// Literal span.
    pub span: Span,
}

/// Floating-point literal; `text` preserves the source spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatLiteral {
    /// Source spelling.
    pub text: String,
    /// Literal span.
    pub span: Span,
}

/// String literal; `text` preserves quotes and escapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLiteral {
    /// Source spelling.
    pub text: String,
    /// Literal span.
    pub span: Span,
}

/// List literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListExpression {
    /// List span.
    pub span: Span,
    /// Elements.
    pub elements: Vec<Expression>,
}

/// Vector literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorExpression {
    /// Literal span.
    pub span: Span,
    /// Components.
    pub components: Vec<Expression>,
}

/// Rotation literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationExpression {
    /// Literal span.
    pub span: Span,
    /// Components.
    pub components: Vec<Expression>,
}

/// Function call expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallExpression {
    /// Full call span.
    pub span: Span,
    /// Callee expression (an identifier for valid LSL).
    pub callee: Box<Expression>,
    /// Arguments.
    pub arguments: Vec<Expression>,
}

/// One vector/rotation component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    /// `.x`
    X,
    /// `.y`
    Y,
    /// `.z`
    Z,
    /// `.s`
    S,
    /// Unrecognized component spelling, preserved in `component_name`.
    Unknown,
}

/// Component access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentAccessExpression {
    /// Full access span.
    pub span: Span,
    /// Base expression.
    pub base: Box<Expression>,
    /// Selected component, or `Unknown` for an invalid spelling.
    pub component: Component,
    /// Original component spelling, including invalid names.
    pub component_name: String,
    /// Component name span.
    pub component_span: Span,
}

/// Unary expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaryExpression {
    /// Full expression span.
    pub span: Span,
    /// Operator.
    pub operator: UnaryOperator,
    /// Operator token span.
    pub operator_span: Span,
    /// Operand.
    pub operand: Box<Expression>,
}

/// Binary expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryExpression {
    /// Full expression span.
    pub span: Span,
    /// Left operand.
    pub left: Box<Expression>,
    /// Operator.
    pub operator: BinaryOperator,
    /// Operator token span.
    pub operator_span: Span,
    /// Right operand.
    pub right: Box<Expression>,
}

/// Assignment expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentExpression {
    /// Full expression span.
    pub span: Span,
    /// Assignment target.
    pub target: Box<Expression>,
    /// Assignment operator.
    pub operator: AssignmentOperator,
    /// Operator token span.
    pub operator_span: Span,
    /// Assigned value.
    pub value: Box<Expression>,
}

/// Explicit cast expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastExpression {
    /// Full expression span.
    pub span: Span,
    /// Target type.
    pub ty: Type,
    /// Cast operand.
    pub operand: Box<Expression>,
}

/// Prefix/postfix increment or decrement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementExpression {
    /// Full expression span.
    pub span: Span,
    /// Operator.
    pub operator: IncrementOperator,
    /// Operator token span.
    pub operator_span: Span,
    /// Whether the operator appears before the operand.
    pub prefix: bool,
    /// Operand.
    pub operand: Box<Expression>,
}

/// Explicit error placeholder with a source range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorNode {
    /// Range associated with the recovered error.
    pub span: Span,
    /// Stable error code when one was emitted.
    pub code: Option<&'static str>,
}
