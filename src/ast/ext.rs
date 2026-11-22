//! Firestorm-specific AST nodes and parse options.

use crate::diagnostic::Span;

use super::node::{ErrorNode, Expression, Statement};

/// Independent feature gates for the AST parser.
///
/// `parse_ast` uses the default (both disabled), so the compatibility API stays
/// ordinary LSL. Firestorm callers pass the effective flags from
/// `PreparedSource` through [`parse_ast_with_options`](super::parse_ast_with_options).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AstParseOptions {
    switches: bool,
    lazy_lists: bool,
}

impl AstParseOptions {
    /// Both Firestorm extensions disabled (ordinary LSL).
    pub const fn lsl() -> Self {
        Self {
            switches: false,
            lazy_lists: false,
        }
    }

    /// Whether Firestorm `switch`/`case`/`default`/`break` parsing is enabled.
    pub const fn switches_enabled(self) -> bool {
        self.switches
    }

    /// Whether Firestorm lazy-list index expressions are enabled.
    pub const fn lazy_lists_enabled(self) -> bool {
        self.lazy_lists
    }

    /// Enable or disable Firestorm switches.
    pub const fn with_switches_enabled(mut self, enabled: bool) -> Self {
        self.switches = enabled;
        self
    }

    /// Enable or disable Firestorm lazy lists.
    pub const fn with_lazy_lists_enabled(mut self, enabled: bool) -> Self {
        self.lazy_lists = enabled;
        self
    }

    /// Build options from effective extension flags.
    pub const fn from_flags(switches: bool, lazy_lists: bool) -> Self {
        Self {
            switches,
            lazy_lists,
        }
    }
}

/// Firestorm `switch` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchStatement {
    /// Full switch span.
    pub span: Span,
    /// Switch expression.
    pub condition: Expression,
    /// Case/default arms in source order, preserving fallthrough and duplicates.
    pub arms: Vec<SwitchArm>,
}

/// One `case` or `default` arm of a switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchArm {
    /// Span from the label through the last arm statement.
    pub span: Span,
    /// Arm label.
    pub label: SwitchLabel,
    /// Statements belonging to this arm, in source order.
    pub statements: Vec<Statement>,
}

/// Firestorm switch arm label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchLabel {
    /// `case expression`.
    Case(Expression),
    /// `default`.
    Default,
    /// Explicit malformed-case placeholder.
    Error(ErrorNode),
}

/// Contextual Firestorm `break;` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakStatement {
    /// Statement span.
    pub span: Span,
}

/// Firestorm lazy-list index expression.
///
/// One index represents `base[index]`; a second index represents the
/// `first,last` range form accepted by `(list)base[first,last]` in Firestorm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LazyIndexExpression {
    /// Full expression span from base through `]`.
    pub span: Span,
    /// Indexed base expression.
    pub base: Box<Expression>,
    /// One or more index expressions.
    pub indices: Vec<Expression>,
}
