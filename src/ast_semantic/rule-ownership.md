# SEM rule ownership matrix

Phase 6 is incremental. Phase 6A migrated the structural rules and Phase 6B
migrated the call/event signature rules below; the remaining rules stay owned by
the Legacy token analyzer and do not run in the AST engine yet.

| Rule | Legacy owner | AST owner | Status | Parity | Known differences | Proof |
| --- | --- | --- | --- | --- | --- | --- |
| SEM001 | `semantic::analyze_with_syntax_diagnostics` top-level scan | `ast_semantic::rules::duplicate_globals` | migrated in 6A | exact | none | `tests/ast_semantic_parity.rs::sem001_exact_messages_and_spans`, full corpus |
| SEM002 | `analyze_builtin_calls` | `ast_semantic::rules::builtin_calls` | migrated in 6B | exact | none | `sem002_and_sem004_exact_messages_and_spans`, full corpus |
| SEM003 | `analyze_event_signatures` | `ast_semantic::rules::event_signatures` | migrated in 6B | exact | none | `sem003_exact_arity_and_type_messages_and_spans`, full corpus |
| SEM004 | `analyze_builtin_calls` | `ast_semantic::rules::builtin_calls` | migrated in 6B | exact | none | `sem002_and_sem004_exact_messages_and_spans`, full corpus |
| SEM005 | `analyze_user_function_calls` | `ast_semantic::rules::user_calls` | migrated in 6B | exact | none | `sem005_and_sem006_exact_messages_and_spans`, full corpus |
| SEM006 | `analyze_user_function_calls` | `ast_semantic::rules::user_calls` | migrated in 6B | exact | none | `sem005_and_sem006_exact_messages_and_spans`, full corpus |
| SEM007 | `semantic::ScopeModel::add_variable` | `ast_semantic::rules::duplicate_locals` | migrated in 6A | exact | none | `sem007_exact_messages_spans_and_shadowing`, full corpus |
| SEM008 | `analyze_function_returns` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM009 | `analyze_jump_labels` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM010 | `analyze_jump_labels` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM011 | `analyze_missing_return_paths` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM012 | `analyze_unresolved_variables` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM013 | `analyze_deprecated_function_calls` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM014 | `analyze_explicit_casts` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM015 | `analyze_variable_initializers` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM016 | `analyze_variable_assignments` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM017 | `analyze_compound_assignments` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM018 | `analyze_component_assignments` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM019 | `analyze_component_reads` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM020 | `analyze_state_topology` | `ast_semantic::rules::duplicate_events` | migrated in 6A | exact | none | `sem020_exact_message_and_scope_boundary`, full corpus |
| SEM021 | `analyze_state_topology` | `ast_semantic::rules::state_namespace` | migrated in 6A | exact | none | `sem021_exact_duplicate_and_namespace_collisions`, full corpus |
| SEM022 | `analyze_state_topology` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM023 | `analyze_discarded_comparison_expressions` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |

## AST scope semantics

- Root scope: all global variable declarators and user-defined functions.
- Function body: one scope; parameters are registered before locals.
- Canonical event handler body: one scope; parameters are registered before locals.
  Unknown extension event parameters are not registered, matching the Legacy
  conservative extension boundary; local declarations in those bodies are still
  scanned.
- Each `StatementKind::Block` creates a child scope.
- A Firestorm switch body creates a child scope; block-form case arms create
  another child via their `Block` statement.
- Unbraced `if`/loop bodies and switch arms use their enclosing scope.
- Missing names/bodies/recovery nodes are skipped, never fabricated.

## Legacy parity notes

- SEM001 intentionally mirrors the Legacy scan, which registers only the first
  declarator of each top-level declaration. ScopeModel still records every
  declarator for SEM021 namespace collision checks.
- SEM020/SEM021 skip states/events whose body braces were not structurally
  closed, matching the Legacy brace-matching oracle.
- SEM021 registers a user function only when `FunctionDeclaration::body` is
  present, matching Legacy `collect_user_functions` requiring `{` after the
  parameter list.
- Malformed event parameter lists preserve a recoverable `EventHandler` with its
  brace-complete body for AST scopes/SEM020/SEM007. Valid `type + identifier`
  parameters across the full bounded parameter range are recovered, matching
  Legacy `register_parameters`. Syntax findings inside the skipped body are
  suppressed, and suppressed EOF errors do not set the parser's EOF latch, so
  Legacy parser diagnostic parity remains unchanged.

## Phase 6B expression typer and call signatures

- `src/ast_semantic/types.rs` infers a type only when the typed AST proves it:
  literals, list/vector/rotation literals, visible identifiers, constants,
  casts, components, unary/binary operators, and known built-in/user calls.
  Unknown expressions return `None`; no default type is guessed.
- Identifier lookup uses `ScopeTree::lookup_value_at` and filters bindings whose
  declaration span is after the use, preserving Legacy source-order visibility
  and nested shadowing. Function declarations are not values.
- User signatures come from the first AST function declaration with a body;
  built-in signatures and constants come from the existing vendored
  `builtins::definitions()` metadata. No second catalogue exists.
- `src/ast_semantic/calls.rs` visits every call exactly once across globals,
  declarations, control flow, switch conditions/case labels/arms, lazy indices,
  calls, lists and vector/rotation literals.
- SEM002/SEM004 and SEM005/SEM006 independently check the same call name, so a
  user function shadowing a built-in keeps Legacy multiplicity.
- SEM003 uses AST `declared_parameter_count`, including malformed parameter
  positions, plus recovered parameter types; unknown extension events are
  ignored.
- A syntax diagnostic inside a call's parentheses suppresses call arity/type
  findings, matching Legacy. The AST stores the opening/closing parenthesis spans
  for this bounded range.
- Malformed argument lists recover to the bounded closing parenthesis and store
  `declared_argument_count` plus `arguments_complete`; arity still matches
  Legacy even when expression recovery failed.
- Legacy SEM001's top-level-call quirk in global initializers is modeled too:
  identifiers followed by `(` at top level participate in the duplicate scan.
- Firestorm switch/lazy AST forms are traversed without panics and without
  inventing types; the Legacy analyzer remains the reference oracle.
