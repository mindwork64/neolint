//! SEM036/SEM037: event return-value hazards and conservative unreachable code.

use std::collections::HashSet;

use crate::ast::{Ast, Block, IfStatement, Statement, StatementKind, TopLevel};

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

const MAX_RETURN_HAZARD_WORK: usize = 16_384;
const MAX_RETURN_HAZARD_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy)]
enum BodyKind<'a> {
    Function,
    Event {
        current_state: &'a str,
        known_states: &'a HashSet<String>,
    },
}

impl BodyKind<'_> {
    fn is_event(self) -> bool {
        matches!(self, Self::Event { .. })
    }

    fn state_change_terminates(self, target: &str) -> bool {
        match self {
            Self::Function => false,
            Self::Event {
                current_state,
                known_states,
            } => target != current_state && known_states.contains(target),
        }
    }
}

struct BodyFacts {
    has_jump_or_label: bool,
    has_error: bool,
    has_unclosed_block: bool,
}

/// Analyze return and reachability hazards in complete function/event bodies.
///
/// SEM036 catches a value-return from an event handler, which can compile but
/// crash at runtime. SEM037 is deliberately narrower: it reports only the
/// first unreachable statement in a straight-line block after a guaranteed
/// return or a transition to a different, known state inside an event. Any
/// jump/label or malformed structure suppresses both rules for the owning body.
pub(crate) fn check(
    ast: &Ast,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
) -> Vec<Diagnostic> {
    if !environment_reliable {
        return Vec::new();
    }

    let mut known_states = HashSet::new();
    let mut duplicate_states = HashSet::new();
    for item in &ast.items {
        let TopLevel::State(state) = item else {
            continue;
        };
        if state.body_missing || !state.body_closed || state.name_missing {
            continue;
        }
        let name = if state.is_default {
            "default".to_owned()
        } else {
            state.name.clone()
        };
        if !known_states.insert(name.clone()) {
            duplicate_states.insert(name);
        }
    }
    // A duplicated name is already invalid (SEM021); don't use it as a proof
    // that a state-change statement ends its event handler.
    known_states.retain(|name| !duplicate_states.contains(name));
    let mut diagnostics = Vec::new();
    let mut work = 0usize;

    for item in &ast.items {
        match item {
            TopLevel::Function(function) => {
                let Some(body) = &function.body else {
                    continue;
                };
                if function.name.is_empty() || !body.closed {
                    continue;
                }
                if !analyze_body(
                    body,
                    BodyKind::Function,
                    syntax_diagnostics,
                    &mut work,
                    &mut diagnostics,
                ) {
                    return Vec::new();
                }
            }
            TopLevel::State(state) => {
                if state.body_missing || !state.body_closed {
                    continue;
                }
                for event in &state.events {
                    let Some(body) = &event.body else {
                        continue;
                    };
                    if !body.closed {
                        continue;
                    }
                    let current_state = if state.is_default {
                        "default"
                    } else {
                        state.name.as_str()
                    };
                    if !analyze_body(
                        body,
                        BodyKind::Event {
                            current_state,
                            known_states: &known_states,
                        },
                        syntax_diagnostics,
                        &mut work,
                        &mut diagnostics,
                    ) {
                        return Vec::new();
                    }
                }
            }
            TopLevel::Global(_) | TopLevel::Error(_) => {}
        }
    }

    diagnostics
}

fn analyze_body(
    body: &Block,
    kind: BodyKind<'_>,
    syntax_diagnostics: &[Diagnostic],
    work: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(facts) = inspect_body(body, work) else {
        return false;
    };

    // Parser damage suppresses both rules for this body. A return expression
    // outside the malformed range is not enough context to trust the recovered
    // event/function structure as a whole.
    // Jumps and labels can make a textually dead tail reachable. This first
    // slice skips the entire code scope instead of approximating jump targets.
    if facts.has_jump_or_label
        || facts.has_error
        || facts.has_unclosed_block
        || block_has_syntax_error(body, syntax_diagnostics)
    {
        return true;
    }

    scan_sequence(
        &body.statements,
        kind,
        syntax_diagnostics,
        0,
        work,
        diagnostics,
    )
    .is_some()
}

fn inspect_body(body: &Block, work: &mut usize) -> Option<BodyFacts> {
    let mut facts = BodyFacts {
        has_jump_or_label: false,
        has_error: false,
        has_unclosed_block: false,
    };
    let mut pending: Vec<(&Statement, usize)> = body
        .statements
        .iter()
        .rev()
        .map(|statement| (statement, 1))
        .collect();

    while let Some((statement, depth)) = pending.pop() {
        *work = work.checked_add(1)?;
        if *work > MAX_RETURN_HAZARD_WORK || depth > MAX_RETURN_HAZARD_DEPTH {
            return None;
        }

        match &statement.kind {
            StatementKind::Jump(_) | StatementKind::Label(_) => {
                facts.has_jump_or_label = true;
            }
            StatementKind::Error(_) => facts.has_error = true,
            StatementKind::Block(block) => {
                if !block.closed {
                    facts.has_unclosed_block = true;
                }
                pending.extend(
                    block
                        .statements
                        .iter()
                        .rev()
                        .map(|child| (child, depth + 1)),
                );
            }
            StatementKind::If(if_statement) => {
                pending.push((&if_statement.then_branch, depth + 1));
                if let Some(else_branch) = &if_statement.else_branch {
                    pending.push((else_branch, depth + 1));
                }
            }
            StatementKind::For(for_statement) => {
                pending.push((&for_statement.body, depth + 1));
            }
            StatementKind::While(while_statement) => {
                pending.push((&while_statement.body, depth + 1));
            }
            StatementKind::DoWhile(do_while) => {
                pending.push((&do_while.body, depth + 1));
            }
            StatementKind::Switch(switch) => {
                for arm in switch.arms.iter().rev() {
                    pending.extend(arm.statements.iter().rev().map(|child| (child, depth + 1)));
                }
            }
            StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::Return(_)
            | StatementKind::StateChange(_)
            | StatementKind::Expression(_)
            | StatementKind::Break(_) => {}
        }
    }

    Some(facts)
}

fn scan_sequence(
    statements: &[Statement],
    kind: BodyKind<'_>,
    syntax_diagnostics: &[Diagnostic],
    depth: usize,
    work: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<bool> {
    if depth > MAX_RETURN_HAZARD_DEPTH {
        return None;
    }

    let mut terminated = false;
    for statement in statements {
        *work = work.checked_add(1)?;
        if *work > MAX_RETURN_HAZARD_WORK {
            return None;
        }

        if terminated {
            if !matches!(statement.kind, StatementKind::Empty) {
                diagnostics.push(Diagnostic {
                    code: "SEM037",
                    severity: Severity::Warning,
                    kind: DiagnosticKind::Lint,
                    message: "unreachable statement after an unconditional return or state change"
                        .to_owned(),
                    span: statement.span,
                });
                // One primary finding per straight-line sequence; don't emit
                // a cascade for every statement in the dead tail.
                break;
            }
            continue;
        }

        let statement_terminates = scan_statement(
            statement,
            kind,
            syntax_diagnostics,
            depth + 1,
            work,
            diagnostics,
        )?;
        if statement_terminates {
            terminated = true;
        }
    }

    Some(terminated)
}

/// Inspect nested bodies and determine whether this statement always exits its
/// enclosing function/event. Loops and switches are intentionally not treated
/// as guaranteed termination constructs.
fn scan_statement(
    statement: &Statement,
    kind: BodyKind<'_>,
    syntax_diagnostics: &[Diagnostic],
    depth: usize,
    work: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<bool> {
    if depth > MAX_RETURN_HAZARD_DEPTH {
        return None;
    }

    match &statement.kind {
        StatementKind::Return(return_statement) => {
            if kind.is_event() {
                if let Some(value) = &return_statement.value {
                    if !statement_has_syntax_error(statement, syntax_diagnostics) {
                        diagnostics.push(event_return_diagnostic(value.span));
                    }
                }
            }
            Some(true)
        }
        StatementKind::StateChange(change) => Some(kind.state_change_terminates(&change.state)),
        StatementKind::Block(block) => {
            if !block.closed {
                return Some(false);
            }
            scan_sequence(
                &block.statements,
                kind,
                syntax_diagnostics,
                depth + 1,
                work,
                diagnostics,
            )
        }
        StatementKind::If(IfStatement {
            then_branch,
            else_branch,
            ..
        }) => {
            let then_terminates = scan_statement(
                then_branch,
                kind,
                syntax_diagnostics,
                depth + 1,
                work,
                diagnostics,
            )?;
            let Some(else_branch) = else_branch else {
                return Some(false);
            };
            let else_terminates = scan_statement(
                else_branch,
                kind,
                syntax_diagnostics,
                depth + 1,
                work,
                diagnostics,
            )?;
            Some(then_terminates && else_terminates)
        }
        StatementKind::For(for_statement) => {
            let _ = scan_statement(
                &for_statement.body,
                kind,
                syntax_diagnostics,
                depth + 1,
                work,
                diagnostics,
            )?;
            Some(false)
        }
        StatementKind::While(while_statement) => {
            let _ = scan_statement(
                &while_statement.body,
                kind,
                syntax_diagnostics,
                depth + 1,
                work,
                diagnostics,
            )?;
            Some(false)
        }
        StatementKind::DoWhile(do_while) => {
            let _ = scan_statement(
                &do_while.body,
                kind,
                syntax_diagnostics,
                depth + 1,
                work,
                diagnostics,
            )?;
            Some(false)
        }
        StatementKind::Switch(switch) => {
            for arm in &switch.arms {
                let _ = scan_sequence(
                    &arm.statements,
                    kind,
                    syntax_diagnostics,
                    depth + 1,
                    work,
                    diagnostics,
                )?;
            }
            Some(false)
        }
        StatementKind::Empty
        | StatementKind::Local(_)
        | StatementKind::Jump(_)
        | StatementKind::Label(_)
        | StatementKind::Expression(_)
        | StatementKind::Break(_)
        | StatementKind::Error(_) => Some(false),
    }
}

fn event_return_diagnostic(span: crate::diagnostic::Span) -> Diagnostic {
    Diagnostic {
        code: "SEM036",
        severity: Severity::Error,
        kind: DiagnosticKind::Semantic,
        message: "event handlers cannot return a value; use 'return;' to exit the event".to_owned(),
        span,
    }
}

fn statement_has_syntax_error(statement: &Statement, diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.span.start >= statement.span.start
            && diagnostic.span.start <= statement.span.end.saturating_add(1)
    })
}

fn block_has_syntax_error(body: &Block, syntax_diagnostics: &[Diagnostic]) -> bool {
    syntax_diagnostics.iter().any(|diagnostic| {
        diagnostic.span.start >= body.span.start && diagnostic.span.start <= body.span.end
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{parse_ast_with_options, AstParseOptions};
    use crate::diagnostic::{DiagnosticKind, Severity};
    use crate::lex;

    fn analyze(source: &str, reliable: bool) -> Vec<Diagnostic> {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
        crate::ast_semantic::analyze_ast_with_environment(&ast, &syntax, reliable)
    }

    fn hazards(source: &str) -> Vec<Diagnostic> {
        analyze(source, true)
            .into_iter()
            .filter(|diagnostic| matches!(diagnostic.code, "SEM036" | "SEM037"))
            .collect()
    }

    #[test]
    fn event_value_return_is_an_error_but_bare_return_is_valid() {
        let source = r#"default { state_entry() { return 1; } }"#;
        let diagnostics = hazards(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "SEM036");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Semantic);
        assert_eq!(
            &source[diagnostics[0].span.start..diagnostics[0].span.end],
            "1"
        );
    }

    #[test]
    fn unknown_extension_event_handlers_are_also_event_contexts() {
        let diagnostics = hazards(r#"default { custom_event() { return "bad"; } }"#);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "SEM036");
    }

    #[test]
    fn value_returns_in_user_functions_are_not_event_return_errors() {
        assert!(hazards("integer helper() { return 1; } default { state_entry() { } }").is_empty());
    }

    #[test]
    fn direct_code_after_return_is_reported_once_per_sequence() {
        let source = r#"integer helper() { return 1; llOwnerSay("dead"); llOwnerSay("also dead"); } default { state_entry() { } }"#;
        let diagnostics = hazards(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "SEM037");
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Lint);
        assert_eq!(
            &source[diagnostics[0].span.start..diagnostics[0].span.end],
            r#"llOwnerSay("dead");"#
        );
    }

    #[test]
    fn both_returning_if_branches_make_the_following_statement_unreachable() {
        let source = r#"integer helper(integer condition) { if (condition) return 1; else return 2; llOwnerSay("dead"); } default { state_entry() { } }"#;
        let diagnostics = hazards(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "SEM037");
    }

    #[test]
    fn one_returning_if_branch_does_not_make_the_following_statement_dead() {
        let source = r#"integer helper(integer condition) { if (condition) return 1; llOwnerSay("live"); return 2; } default { state_entry() { } }"#;
        assert!(hazards(source).is_empty());
    }

    #[test]
    fn event_state_change_terminates_the_event_but_conditional_transition_does_not() {
        let direct = r#"default { state_entry() { state running; llOwnerSay("dead"); } } state running { state_entry() { } }"#;
        let direct_diagnostics = hazards(direct);
        assert_eq!(direct_diagnostics.len(), 1, "{direct_diagnostics:?}");
        assert_eq!(direct_diagnostics[0].code, "SEM037");

        let conditional = r#"default { state_entry() { if (TRUE) state running; llOwnerSay("may run"); } } state running { state_entry() { } }"#;
        assert!(hazards(conditional).is_empty());
    }

    #[test]
    fn same_state_and_unknown_state_targets_do_not_terminate_an_event() {
        let same_default =
            r#"default { state_entry() { state default; llOwnerSay("still reachable"); } }"#;
        assert!(hazards(same_default).is_empty());

        let same_named = r#"default { } state running { state_entry() { state running; llOwnerSay("still reachable"); } }"#;
        assert!(hazards(same_named).is_empty());

        let unknown = r#"default { state_entry() { state missing; llOwnerSay("not a proven transition"); } }"#;
        assert!(hazards(unknown).is_empty());
    }

    #[test]
    fn duplicate_state_names_do_not_prove_a_valid_transition() {
        let source = r#"default { } state running { state_entry() { state other; llOwnerSay("not proven"); } } state other { state_entry() { } } state other { state_entry() { } }"#;
        assert!(hazards(source).is_empty());
    }

    #[test]
    fn global_function_state_changes_are_not_treated_as_valid_termination() {
        let source = r#"helper() { state running; llOwnerSay("not assumed dead"); } default { state_entry() { } } state running { state_entry() { } }"#;
        assert!(hazards(source).is_empty());
    }

    #[test]
    fn jumps_and_labels_suppress_dead_code_inference() {
        let source = r#"integer helper() { return 1; @later; llOwnerSay("reachable by label"); jump later; } default { state_entry() { } }"#;
        assert!(hazards(source)
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM037"));
    }

    #[test]
    fn event_return_hazards_are_suppressed_in_jump_label_bodies() {
        let source = r#"default { state_entry() { return 1; @later; jump later; } }"#;
        assert!(hazards(source).is_empty());
    }

    #[test]
    fn traversal_budget_overflow_suppresses_the_whole_rule_family() {
        let mut source = String::from("default { state_entry() { return 1; ");
        source.push_str(&";".repeat(MAX_RETURN_HAZARD_WORK));
        source.push_str(" } }");
        assert!(hazards(&source).is_empty());
    }

    #[test]
    fn switch_dispatch_is_not_assumed_to_terminate() {
        let source = r#"integer helper(integer value) { switch (value) { case 1: return 1; default: return 2; } llOwnerSay("conservative"); return 3; } default { state_entry() { } }"#;
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) =
            parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
        assert!(syntax.is_empty(), "{syntax:?}");
        let diagnostics = crate::ast_semantic::analyze_ast(&ast, &syntax);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "SEM037"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn malformed_bodies_and_unreliable_environments_fail_closed() {
        let malformed = r#"default { state_entry() { return 1; llOwnerSay( ; } }"#;
        let (tokens, lexical) = lex(malformed);
        assert!(lexical.is_empty());
        let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
        assert!(!syntax.is_empty());
        let diagnostics = crate::ast_semantic::analyze_ast(&ast, &syntax);
        assert!(diagnostics
            .iter()
            .all(|item| !matches!(item.code, "SEM036" | "SEM037")));

        let source = r#"default { state_entry() { return 1; } }"#;
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty());
        let (ast, syntax) = parse_ast_with_options(&tokens, AstParseOptions::lsl());
        let diagnostics = crate::ast_semantic::analyze_ast_with_environment(&ast, &syntax, false);
        assert!(diagnostics
            .iter()
            .all(|item| !matches!(item.code, "SEM036" | "SEM037")));
    }
}
