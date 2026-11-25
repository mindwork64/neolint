//! SEM034/SEM035: state changes inside global user-defined functions.

use std::collections::HashSet;

use crate::ast::{Ast, Block, IfStatement, Statement, StatementKind, TopLevel};
use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

use super::semantic_error;

const MAX_STATE_CONTEXT_WORK: usize = 16_384;

/// Check state-change statements in top-level user-defined functions.
///
/// Mono rejects ordinary state changes in global functions. A historical
/// compiler bugfeature used a state change as the direct then-branch of an if
/// statement without an else. That narrow shape is retained as SEM035 warning,
/// not accepted as a portable or currently supported language feature.
pub(crate) fn check(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let mut work = 0usize;

    for item in &ast.items {
        let TopLevel::Function(function) = item else {
            continue;
        };
        if function.name.is_empty() {
            continue;
        }
        let Some(body) = &function.body else {
            continue;
        };
        if !body.closed {
            continue;
        }

        let mut allowed_compatibility_spans = HashSet::new();
        let mut statements = Vec::new();
        if !collect_statements(
            body,
            syntax_diagnostics,
            &mut work,
            &mut statements,
            &mut allowed_compatibility_spans,
        ) {
            // Never report from a partial traversal.
            return Vec::new();
        }

        for statement in statements {
            let StatementKind::StateChange(change) = &statement.kind else {
                continue;
            };
            if change.state.is_empty()
                || has_syntax_error(statement.span.start, statement.span.end, syntax_diagnostics)
            {
                continue;
            }

            if allowed_compatibility_spans.contains(&statement.span.start) {
                diagnostics.push(Diagnostic {
                    code: "SEM035",
                    severity: Severity::Warning,
                    kind: DiagnosticKind::Lint,
                    message: "state change in a global function relies on the unsupported Mono if-without-else workaround; move it into an event handler for portable behavior".to_owned(),
                    span: change.state_span,
                });
            } else {
                diagnostics.push(semantic_error(
                    "SEM034",
                    "global functions cannot change script state on Mono; move the state change into an event handler".to_owned(),
                    change.state_span,
                ));
            }
        }
    }

    diagnostics
}

/// Collect statements in source order, classifying only the direct then-branch
/// of an if-without-else as the historical Mono compatibility form.
fn collect_statements<'a>(
    body: &'a Block,
    syntax_diagnostics: &[Diagnostic],
    work: &mut usize,
    statements: &mut Vec<&'a Statement>,
    compatibility_spans: &mut HashSet<usize>,
) -> bool {
    let mut pending: Vec<&Statement> = body.statements.iter().rev().collect();

    while let Some(statement) = pending.pop() {
        *work = work.saturating_add(1);
        if *work > MAX_STATE_CONTEXT_WORK {
            return false;
        }
        statements.push(statement);

        match &statement.kind {
            StatementKind::Block(block) => {
                pending.extend(block.statements.iter().rev());
            }
            StatementKind::If(IfStatement {
                then_branch,
                else_branch,
                span,
                ..
            }) => {
                if else_branch.is_none()
                    && matches!(then_branch.kind, StatementKind::StateChange(_))
                    && !has_syntax_error(span.start, span.end, syntax_diagnostics)
                {
                    compatibility_spans.insert(then_branch.span.start);
                }
                if let Some(else_branch) = else_branch {
                    pending.push(else_branch);
                }
                pending.push(then_branch);
            }
            StatementKind::For(for_statement) => pending.push(&for_statement.body),
            StatementKind::While(while_statement) => pending.push(&while_statement.body),
            StatementKind::DoWhile(do_while) => pending.push(&do_while.body),
            StatementKind::Switch(switch) => {
                for arm in switch.arms.iter().rev() {
                    pending.extend(arm.statements.iter().rev());
                }
            }
            StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::Return(_)
            | StatementKind::Jump(_)
            | StatementKind::StateChange(_)
            | StatementKind::Label(_)
            | StatementKind::Expression(_)
            | StatementKind::Break(_)
            | StatementKind::Error(_) => {}
        }
    }

    true
}

fn has_syntax_error(start: usize, end: usize, diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.span.start >= start && diagnostic.span.start <= end.saturating_add(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{parse_ast_with_options, AstParseOptions};
    use crate::lex;

    fn analyze(source: &str, reliable: bool) -> Vec<Diagnostic> {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
        crate::ast_semantic::analyze_ast_with_environment(&ast, &syntax, reliable)
    }

    fn codes(source: &str) -> Vec<&'static str> {
        analyze(source, true)
            .into_iter()
            .filter(|diagnostic| matches!(diagnostic.code, "SEM034" | "SEM035"))
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn direct_function_state_change_is_an_error() {
        assert_eq!(
            codes("helper() { state running; } default { state_entry() { } } state running { state_entry() { } }"),
            vec!["SEM034"]
        );
    }

    #[test]
    fn simple_if_without_else_is_the_historical_mono_warning() {
        assert_eq!(
            codes("helper(integer condition) { if (condition) state running; } default { state_entry() { } } state running { state_entry() { } }"),
            vec!["SEM035"]
        );
    }

    #[test]
    fn block_if_and_if_else_are_not_the_historical_exception() {
        assert_eq!(
            codes("helper(integer condition) { if (condition) { state running; } if (condition) state default; else ; } default { state_entry() { } } state running { state_entry() { } }"),
            vec!["SEM034", "SEM034"]
        );
    }

    #[test]
    fn nested_blocks_and_loop_bodies_are_checked() {
        assert_eq!(
            codes("helper(integer condition) { { state running; } while (condition) state running; for (; condition; ) state running; } default { state_entry() { } } state running { state_entry() { } }"),
            vec!["SEM034", "SEM034", "SEM034"]
        );
    }

    #[test]
    fn a_function_called_by_an_event_is_still_a_global_function() {
        assert_eq!(
            codes("helper() { state running; } default { state_entry() { helper(); } } state running { state_entry() { } }"),
            vec!["SEM034"]
        );
    }

    #[test]
    fn state_changes_in_event_handlers_are_not_reported() {
        assert!(codes("default { state_entry() { state running; } } state running { state_entry() { state default; } }").is_empty());
    }

    #[test]
    fn syntax_errors_in_a_state_change_suppress_that_finding() {
        let source = "helper() { state running } default { state_entry() { } } state running { state_entry() { } }";
        let diagnostics = analyze(source, true);
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM034" && diagnostic.code != "SEM035"));
    }

    #[test]
    fn traversal_budget_overflow_suppresses_the_whole_rule() {
        let mut source = String::from("helper() { state default; ");
        source.push_str(&";".repeat(MAX_STATE_CONTEXT_WORK));
        source.push_str(" } default { state_entry() { } }");
        assert!(codes(&source).is_empty());
    }

    #[test]
    fn unclosed_functions_and_unreliable_environments_fail_closed() {
        assert!(codes("helper() { state running; default { state_entry() { } } state running { state_entry() { } }").is_empty());
        assert!(analyze("helper() { state running; } default { state_entry() { } } state running { state_entry() { } }", false)
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "SEM034" | "SEM035")));
    }
}
