# SEM rule ownership matrix

Phase 6 is incremental. Phase 6A migrates only the four structural rules below;
all other rules remain owned by the Legacy token analyzer and do not run in the
AST engine yet.

| Rule | Legacy owner | AST owner | Status | Parity | Known differences | Proof |
| --- | --- | --- | --- | --- | --- | --- |
| SEM001 | `semantic::analyze_with_syntax_diagnostics` top-level scan | `ast_semantic::rules::duplicate_globals` | migrated in 6A | exact | none | `tests/ast_semantic_parity.rs::sem001_exact_messages_and_spans`, full corpus |
| SEM002 | `analyze_builtin_calls` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM003 | `analyze_event_signatures` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM004 | `analyze_builtin_calls` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM005 | `analyze_user_function_calls` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
| SEM006 | `analyze_user_function_calls` | not implemented yet | Legacy-only | n/a | none in 6A | existing Legacy tests |
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
