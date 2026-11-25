//! SEM036 event-return hazards and CFG-backed SEM037 unreachable-code findings.

use std::collections::HashSet;

use super::super::flow::{self, FlowAnalysis};
use super::super::scopes::{ScopeId, ScopeTree};

use crate::ast::{Ast, Block, IfStatement, Statement, StatementKind, TopLevel};
use crate::builtins;

use crate::diagnostic::{Diagnostic, DiagnosticKind, Severity};

const MAX_RETURN_HAZARD_WORK: usize = 16_384;
const MAX_RETURN_HAZARD_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy)]
enum BodyKind<'a> {
    Function,
    Event {
        current_state: &'a str,
        known_states: &'a HashSet<String>,
        jump_scope_validated: bool,
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
                ..
            } => target != current_state && known_states.contains(target),
        }
    }

    fn jump_scope_validated(self) -> bool {
        match self {
            Self::Function => true,
            Self::Event {
                jump_scope_validated,
                ..
            } => jump_scope_validated,
        }
    }
}

struct BodyFacts {
    has_jump_or_label: bool,
    has_jump: bool,
    has_error: bool,
    has_unclosed_block: bool,
    terminating_state_changes: HashSet<(usize, usize)>,
}

struct BodyValidation<'a> {
    syntax_diagnostics: &'a [Diagnostic],
    invalid_code_scopes: &'a HashSet<(usize, usize)>,
}

/// Analyze return and reachability hazards in complete function/event bodies.
///
/// SEM036 catches value-returns from event handlers. SEM037 uses the shared CFG
/// to report the first unreachable non-empty statement in each straight-line
/// sequence. The graph accounts for constant conditions, loops, valid jumps,
/// Firestorm switches and event state transitions. Malformed/invalid scopes
/// fail closed; SEM036 retains its older jump/label suppression.
pub(crate) fn check(
    ast: &Ast,
    scopes: &ScopeTree,
    syntax_diagnostics: &[Diagnostic],
    environment_reliable: bool,
    invalid_code_scopes: &HashSet<(usize, usize)>,
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
                let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                if !analyze_body(
                    body,
                    scope,
                    BodyKind::Function,
                    scopes,
                    BodyValidation {
                        syntax_diagnostics,
                        invalid_code_scopes,
                    },
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
                    let scope = scopes.scope_for_block(body).unwrap_or(scopes.root());
                    if !analyze_body(
                        body,
                        scope,
                        BodyKind::Event {
                            current_state,
                            known_states: &known_states,
                            jump_scope_validated: builtins::definitions()
                                .events
                                .contains_key(event.name.as_str()),
                        },
                        scopes,
                        BodyValidation {
                            syntax_diagnostics,
                            invalid_code_scopes,
                        },
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
    scope: ScopeId,
    kind: BodyKind<'_>,
    scopes: &ScopeTree,
    validation: BodyValidation<'_>,
    work: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(facts) = inspect_body(body, kind, work) else {
        return false;
    };

    // Parser damage and invalid jump scopes make the control-flow graph
    // incomplete. Never infer dead code from a recovered or invalid graph.
    if (facts.has_jump && !kind.jump_scope_validated())
        || facts.has_error
        || facts.has_unclosed_block
        || validation
            .invalid_code_scopes
            .contains(&(body.span.start, body.span.end))
        || block_has_syntax_error(body, validation.syntax_diagnostics)
    {
        return true;
    }

    let Some(flow) = flow::analyze_body(body, scope, scopes, &facts.terminating_state_changes)
    else {
        return true;
    };

    let mut body_diagnostics = Vec::new();
    if !report_unreachable_statements(body, &flow, work, &mut body_diagnostics) {
        return false;
    }

    // Preserve SEM036's existing suppression rule for code scopes containing
    // jumps/labels. SEM037 now uses the CFG, which resolves valid jumps and
    // labels instead of suppressing the whole body.
    if !facts.has_jump_or_label
        && scan_sequence(
            &body.statements,
            kind,
            validation.syntax_diagnostics,
            0,
            work,
            &mut body_diagnostics,
        )
        .is_none()
    {
        return false;
    }

    diagnostics.extend(body_diagnostics);
    true
}

fn inspect_body(body: &Block, kind: BodyKind<'_>, work: &mut usize) -> Option<BodyFacts> {
    let mut facts = BodyFacts {
        has_jump_or_label: false,
        has_jump: false,
        has_error: false,
        has_unclosed_block: false,
        terminating_state_changes: HashSet::new(),
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
            StatementKind::Jump(_) => {
                facts.has_jump = true;
                facts.has_jump_or_label = true;
            }
            StatementKind::Label(_) => {
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
            StatementKind::StateChange(change) => {
                if kind.state_change_terminates(&change.state) {
                    facts
                        .terminating_state_changes
                        .insert((statement.span.start, statement.span.end));
                }
            }
            StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::Return(_)
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

fn report_unreachable_statements(
    body: &Block,
    flow: &FlowAnalysis,
    work: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    fn scan_sequence(
        statements: &[Statement],
        flow: &FlowAnalysis,
        work: &mut usize,
        diagnostics: &mut Vec<Diagnostic>,
        depth: usize,
    ) -> bool {
        if depth > MAX_RETURN_HAZARD_DEPTH {
            return false;
        }
        for statement in statements {
            *work = work.saturating_add(1);
            if *work > MAX_RETURN_HAZARD_WORK {
                return false;
            }
            if flow.statement_is_unreachable(statement) {
                if !matches!(statement.kind, StatementKind::Empty) {
                    diagnostics.push(Diagnostic {
                        code: "SEM037",
                        severity: Severity::Warning,
                        kind: DiagnosticKind::Lint,
                        message: "unreachable statement".to_owned(),
                        span: statement.span,
                    });
                    // Report only the first actionable dead statement in this
                    // sequence. Nested statements in its dead subtree would
                    // only create cascading diagnostics.
                    break;
                }
                continue;
            }
            if !scan_nested(statement, flow, work, diagnostics, depth + 1) {
                return false;
            }
        }
        true
    }

    fn scan_nested(
        statement: &Statement,
        flow: &FlowAnalysis,
        work: &mut usize,
        diagnostics: &mut Vec<Diagnostic>,
        depth: usize,
    ) -> bool {
        if depth > MAX_RETURN_HAZARD_DEPTH {
            return false;
        }
        *work = work.saturating_add(1);
        if *work > MAX_RETURN_HAZARD_WORK {
            return false;
        }
        if flow.statement_is_unreachable(statement) {
            if !matches!(statement.kind, StatementKind::Empty) {
                diagnostics.push(Diagnostic {
                    code: "SEM037",
                    severity: Severity::Warning,
                    kind: DiagnosticKind::Lint,
                    message: "unreachable statement".to_owned(),
                    span: statement.span,
                });
            }
            return true;
        }
        match &statement.kind {
            StatementKind::Block(block) => {
                scan_sequence(&block.statements, flow, work, diagnostics, depth + 1)
            }
            StatementKind::If(if_statement) => {
                scan_nested(
                    &if_statement.then_branch,
                    flow,
                    work,
                    diagnostics,
                    depth + 1,
                ) && if let Some(else_branch) = &if_statement.else_branch {
                    scan_nested(else_branch, flow, work, diagnostics, depth + 1)
                } else {
                    true
                }
            }
            StatementKind::For(for_statement) => {
                scan_nested(&for_statement.body, flow, work, diagnostics, depth + 1)
            }
            StatementKind::While(while_statement) => {
                scan_nested(&while_statement.body, flow, work, diagnostics, depth + 1)
            }
            StatementKind::DoWhile(do_while) => {
                scan_nested(&do_while.body, flow, work, diagnostics, depth + 1)
            }
            StatementKind::Switch(switch) => {
                for arm in &switch.arms {
                    if !scan_sequence(&arm.statements, flow, work, diagnostics, depth + 1) {
                        return false;
                    }
                }
                true
            }
            StatementKind::Empty
            | StatementKind::Local(_)
            | StatementKind::Return(_)
            | StatementKind::Jump(_)
            | StatementKind::Label(_)
            | StatementKind::StateChange(_)
            | StatementKind::Expression(_)
            | StatementKind::Break(_)
            | StatementKind::Error(_) => true,
        }
    }

    scan_sequence(&body.statements, flow, work, diagnostics, 0)
}

/// Preserve SEM036's historical source-order scan through bodies without
/// jumps/labels. SEM037 reachability is computed separately by the CFG.
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
    fn cfg_reachability_skips_jumps_in_unvalidated_extension_event_scopes() {
        let source = r#"default { custom_event() { jump nested; { @nested; llOwnerSay("unknown scope"); } } }"#;
        assert!(hazards(source)
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM037"));
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

        let conditional = r#"default { state_entry() { if (llGetOwner() != NULL_KEY) state running; llOwnerSay("may run"); } } state running { state_entry() { } }"#;
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
    fn cfg_resolves_valid_jumps_and_reports_only_the_dead_gap() {
        let source = r#"integer helper() { jump later; llOwnerSay("dead gap"); @later; return 1; } default { state_entry() { } }"#;
        let diagnostics = hazards(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "SEM037");
        assert_eq!(
            &source[diagnostics[0].span.start..diagnostics[0].span.end],
            r#"llOwnerSay("dead gap");"#
        );
    }

    #[test]
    fn invalid_jump_scopes_suppress_cfg_dead_code_inference() {
        let missing = r#"integer helper() { jump nowhere; llOwnerSay("unknown target"); } default { state_entry() { } }"#;
        assert!(hazards(missing)
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM037"));

        let duplicate = r#"integer helper() { jump later; @later; @later; return 1; } default { state_entry() { } }"#;
        assert!(hazards(duplicate)
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM037"));

        let inaccessible = r#"integer helper() { jump nested; { @nested; return 1; } } default { state_entry() { } }"#;
        assert!(hazards(inaccessible)
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM037"));

        let reachable =
            r#"integer helper() { jump later; @later; return 1; } default { state_entry() { } }"#;
        assert!(hazards(reachable).is_empty());
    }
    #[test]
    fn event_return_hazards_remain_suppressed_in_jump_label_bodies() {
        let source = r#"default { state_entry() { jump later; return 1; @later; } }"#;
        let diagnostics = hazards(source);
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "SEM036"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM037"));
    }
    #[test]
    fn traversal_budget_overflow_suppresses_the_whole_rule_family() {
        let mut source = String::from("default { state_entry() { return 1; ");
        source.push_str(&";".repeat(MAX_RETURN_HAZARD_WORK));
        source.push_str(" } }");
        assert!(hazards(&source).is_empty());
    }

    #[test]
    fn exhaustive_switch_arms_that_return_make_following_code_unreachable() {
        let source = r#"integer helper(integer value) { switch (value) { case 1: return 1; default: return 2; } llOwnerSay("dead"); } default { state_entry() { } }"#;
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) =
            parse_ast_with_options(&tokens, AstParseOptions::lsl().with_switches_enabled(true));
        assert!(syntax.is_empty(), "{syntax:?}");
        let diagnostics = crate::ast_semantic::analyze_ast(&ast, &syntax);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "SEM037"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn switch_break_keeps_the_switch_exit_reachable() {
        let source = r#"integer helper(integer value) { switch (value) { case 1: return 1; default: break; } llOwnerSay("live"); return 2; } default { state_entry() { } }"#;
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
    fn jump_label_constant_state_prunes_sem037_branch() {
        let source = r#"
            integer jump_label_preserves_facts_and_prunes_the_other_branch() {
                integer value = 1;
                jump after_write;
                ;
                @after_write;
                if (value == 1) {
                    llOwnerSay("reachable");
                } else {
                    llOwnerSay("unreachable");
                }
                return 1;
            }
            default { state_entry() { } }
        "#;

        let diagnostics = hazards(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "SEM037");
        assert!(
            source[diagnostics[0].span.start..diagnostics[0].span.end]
                .contains("llOwnerSay(\"unreachable\");"),
            "unexpected unreachable span: {:?}",
            &source[diagnostics[0].span.start..diagnostics[0].span.end]
        );
    }

    #[test]
    fn constant_branches_and_loops_drive_cfg_reachability() {
        let source = r#"
            integer false_if() {
                if (FALSE) llOwnerSay("dead");
                return 1;
            }
            integer true_if_return() {
                if (TRUE) return 1;
                llOwnerSay("dead");
            }
            integer false_while() {
                while (FALSE) llOwnerSay("dead");
                return 1;
            }
            integer true_while() {
                while (TRUE) { }
                llOwnerSay("dead");
            }
            integer true_for() {
                for (; TRUE; ) { }
                llOwnerSay("dead");
            }
            integer false_condition_for() {
                for (; FALSE; ) llOwnerSay("dead");
                return 1;
            }
            integer do_returns() {
                do { return 1; } while (FALSE);
                llOwnerSay("dead");
            }
            integer do_forever() {
                do { llOwnerSay("loop"); } while (TRUE);
                llOwnerSay("dead");
            }
            integer unknown_loop(integer condition) {
                while (condition) return 1;
                return 2;
            }
            default { state_entry() { } }
        "#;
        let diagnostics = hazards(source);
        let sem037 = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "SEM037")
            .count();
        assert_eq!(sem037, 8, "{diagnostics:?}");
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
