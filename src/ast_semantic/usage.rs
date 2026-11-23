//! Shared syntactic identifier-use model for unused-declaration rules.
//!
//! SEM024 and the later SEM025 are syntactic read/use analyses, not runtime
//! liveness analyses. A read counts even when it is unreachable at runtime, for
//! example inside a function that SEM026 may later prove is never called.
//!
//! Declaration identity is [`SymbolKey`]: the owning [`ScopeId`] plus the
//! declaration name-span start. Keys are valid only for the immutable scope tree
//! of one analysis unit and must never be persisted or compared across units.

use std::collections::HashSet;

use crate::ast::{Ast, Block, ExpressionKind, StatementKind, TopLevel};
use crate::diagnostic::Diagnostic;

use super::expressions::{ExpressionFacts, IdentifierUse, UseContext};
use super::scopes::{ScopeId, ScopeTree, Symbol};
use super::statements;

/// Stable identity of one declaration within a single [`ScopeTree`].
pub(crate) type SymbolKey = (usize, usize);

/// Key for a declaration symbol.
pub(crate) fn symbol_key(symbol: &Symbol) -> SymbolKey {
    key_for(symbol.scope, symbol.span.start)
}

/// Key for a declaration scope and name-span start.
pub(crate) fn key_for(scope: ScopeId, span_start: usize) -> SymbolKey {
    (scope.index(), span_start)
}

/// Resolve identifier uses and return the declarations that are read.
///
/// Only [`UseContext::Read`] and [`UseContext::ReadWrite`] count as reads.
/// Plain assignment targets and call callees are excluded. Unresolved names are
/// ignored here because SEM012 owns them.
pub(crate) fn used_symbols(uses: &[IdentifierUse<'_>], scopes: &ScopeTree) -> HashSet<SymbolKey> {
    let mut used = HashSet::new();
    for usage in uses {
        if !matches!(usage.context, UseContext::Read | UseContext::ReadWrite) {
            continue;
        }
        if let Some(symbol) = scopes.lookup_value_at(
            usage.scope,
            &usage.identifier.name,
            usage.identifier.span.start,
        ) {
            used.insert(symbol_key(symbol));
        }
    }
    used
}

/// Whether the unit is complete enough for optimistic read/use analysis.
///
/// A hidden read could live in any malformed region, so the rules fail closed:
/// when reliability cannot be established they emit nothing. This is the
/// documented conservative boundary for SEM024/SEM025. `facts` must be the
/// shared [`crate::ast_semantic::expressions::collect_facts`] output for `ast`.
pub(crate) fn unit_is_reliable(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    facts: &ExpressionFacts<'_>,
) -> bool {
    if !syntax_diagnostics.is_empty() || !ast.recovered_functions.is_empty() {
        return false;
    }
    if ast
        .items
        .iter()
        .any(|item| matches!(item, TopLevel::Error(_)))
    {
        return false;
    }

    if facts
        .expressions
        .iter()
        .any(|site| matches!(site.expression.kind, ExpressionKind::Error(_)))
    {
        return false;
    }

    for item in &ast.items {
        match item {
            TopLevel::Function(function) => {
                let Some(body) = &function.body else {
                    // A missing body could contain the only read of any binding.
                    return false;
                };
                if !body.closed || block_has_error_statement(body) {
                    return false;
                }
            }
            TopLevel::State(state) => {
                if state.body_missing || !state.body_closed {
                    return false;
                }
                for event in &state.events {
                    let Some(body) = &event.body else {
                        return false;
                    };
                    if !body.closed || block_has_error_statement(body) {
                        return false;
                    }
                }
            }
            TopLevel::Global(_) | TopLevel::Error(_) => {}
        }
    }

    true
}

fn block_has_error_statement(body: &Block) -> bool {
    let mut found = false;
    statements::visit_statements(body, |statement| {
        if matches!(statement.kind, StatementKind::Error(_)) {
            found = true;
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{parse_ast, ErrorNode, Expression, RecoveredFunction, Statement};
    use crate::ast_semantic::expressions::collect_facts;
    use crate::diagnostic::{DiagnosticKind, Severity};
    use crate::lex;

    fn parsed(source: &str) -> (Ast, Vec<Diagnostic>) {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        parse_ast(&tokens)
    }

    fn clean_ast() -> Ast {
        let (ast, syntax) = parsed(
            "integer g;\ninteger f(integer p) { integer l; l = g; return l; }\ndefault { state_entry() { g = f(g); } }\n",
        );
        assert!(syntax.is_empty(), "{syntax:?}");
        ast
    }

    fn unit_facts(ast: &Ast) -> ExpressionFacts<'_> {
        let scopes = ScopeTree::build(ast);
        collect_facts(ast, &scopes)
    }

    fn reliable(ast: &Ast) -> bool {
        let facts = unit_facts(ast);
        unit_is_reliable(ast, &[], &facts)
    }

    fn error_node(ast: &Ast) -> ErrorNode {
        ErrorNode {
            span: ast.span,
            code: None,
        }
    }

    #[test]
    fn clean_complete_unit_is_reliable() {
        assert!(reliable(&clean_ast()));
    }

    #[test]
    fn syntax_diagnostics_fail_closed() {
        let ast = clean_ast();
        let diagnostic = Diagnostic {
            code: "PAR006",
            severity: Severity::Error,
            kind: DiagnosticKind::Syntax,
            message: "sentinel".to_owned(),
            span: ast.span,
        };
        let facts = unit_facts(&ast);
        assert!(!unit_is_reliable(&ast, &[diagnostic], &facts));
    }

    #[test]
    fn recovered_function_fails_closed() {
        let mut ast = clean_ast();
        ast.recovered_functions.push(RecoveredFunction {
            name: "recovered".to_owned(),
            name_span: ast.span,
            return_type: "integer".to_owned(),
            parameter_types: Vec::new(),
        });
        assert!(!reliable(&ast));
    }

    #[test]
    fn top_level_error_fails_closed() {
        let mut ast = clean_ast();
        let error = error_node(&ast);
        ast.items.push(TopLevel::Error(error));
        assert!(!reliable(&ast));
    }

    #[test]
    fn statement_error_fails_closed() {
        let mut ast = clean_ast();
        let span = ast.span;
        let Some(TopLevel::Function(function)) = ast
            .items
            .iter_mut()
            .find(|item| matches!(item, TopLevel::Function(_)))
        else {
            panic!("expected function");
        };
        let Some(body) = function.body.as_mut() else {
            panic!("expected function body");
        };
        body.statements.push(Statement {
            span,
            kind: StatementKind::Error(ErrorNode { span, code: None }),
        });
        assert!(!reliable(&ast));
    }

    #[test]
    fn expression_error_fails_closed() {
        let mut ast = clean_ast();
        let span = ast.span;
        let Some(TopLevel::Global(global)) = ast.items.get_mut(0) else {
            panic!("expected leading global");
        };
        let Some(declarator) = global.declarators.first_mut() else {
            panic!("expected global declarator");
        };
        declarator.initializer = Some(Expression {
            span,
            kind: ExpressionKind::Error(ErrorNode { span, code: None }),
        });
        assert!(!reliable(&ast));
    }

    #[test]
    fn missing_function_body_fails_closed() {
        let mut ast = clean_ast();
        let Some(TopLevel::Function(function)) = ast
            .items
            .iter_mut()
            .find(|item| matches!(item, TopLevel::Function(_)))
        else {
            panic!("expected function");
        };
        function.body = None;
        assert!(!reliable(&ast));
    }

    #[test]
    fn unclosed_function_body_fails_closed() {
        let mut ast = clean_ast();
        let Some(TopLevel::Function(function)) = ast
            .items
            .iter_mut()
            .find(|item| matches!(item, TopLevel::Function(_)))
        else {
            panic!("expected function");
        };
        let Some(body) = function.body.as_mut() else {
            panic!("expected function body");
        };
        body.closed = false;
        assert!(!reliable(&ast));
    }

    #[test]
    fn missing_state_body_fails_closed() {
        let mut ast = clean_ast();
        let Some(TopLevel::State(state)) = ast
            .items
            .iter_mut()
            .find(|item| matches!(item, TopLevel::State(_)))
        else {
            panic!("expected state");
        };
        state.body_missing = true;
        assert!(!reliable(&ast));
    }

    #[test]
    fn unclosed_state_body_fails_closed() {
        let mut ast = clean_ast();
        let Some(TopLevel::State(state)) = ast
            .items
            .iter_mut()
            .find(|item| matches!(item, TopLevel::State(_)))
        else {
            panic!("expected state");
        };
        state.body_closed = false;
        assert!(!reliable(&ast));
    }

    #[test]
    fn unclosed_event_body_fails_closed() {
        let mut ast = clean_ast();
        let Some(TopLevel::State(state)) = ast
            .items
            .iter_mut()
            .find(|item| matches!(item, TopLevel::State(_)))
        else {
            panic!("expected state");
        };
        let Some(event) = state.events.first_mut() else {
            panic!("expected event");
        };
        let Some(body) = event.body.as_mut() else {
            panic!("expected event body");
        };
        body.closed = false;
        assert!(!reliable(&ast));
    }
}
