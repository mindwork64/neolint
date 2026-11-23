# SEM rule ownership matrix

Phases 6A–6F migrate all existing SEM001–SEM023 diagnostics to the AST engine.
No SEM rule remains Legacy-only in the current matrix.

## Engine selection and compare scope (Phase 6G)

- `--engine` / `engine = legacy|ast|compare` selects the analyzer. AST is the
  Phase 6H default on the experimental branch; Legacy is the explicit rollback
  engine, selected with `--engine legacy` or `engine = legacy`.
- Compare reports cover only the raw `SEM001`–`SEM023` semantic streams. Parser
  and lexer parity is not claimed. Parser/lexer recovery divergence on invalid
  input is reported rather than treated as a parity defect; the SEM002 delta
  below is a separate builtin-call-detection difference that also occurs on
  accepted input.
- The Compare display stream is Legacy-only; the AST semantic stream is never
  appended to user output. The per-rule matrix below is unchanged.
- Known SEM002 delta surfaced by Compare: a declaration identifier that collides
  with a builtin name (for example an event named `llSay`) yields a Legacy-only
  `SEM002`, because Legacy token-scans the event header as a builtin call while
  AST only inspects expression call sites. The per-rule matrix's `exact` status
  refers to the differential corpus, and its `Known differences` cell is
  corpus-relative too: SEM002's `none` means "no difference inside the
  differential corpus", not "no difference in any input". Compare therefore
  reports a genuine delta on such input.

| Rule | Legacy owner | AST owner | Status | Parity | Known differences | Proof |
| --- | --- | --- | --- | --- | --- | --- |
| SEM001 | `semantic::analyze_with_syntax_diagnostics` top-level scan | `ast_semantic::rules::duplicate_globals` | migrated in 6A | exact | none | `tests/ast_semantic_parity.rs::sem001_exact_messages_and_spans`, full corpus |
| SEM002 | `analyze_builtin_calls` | `ast_semantic::rules::builtin_calls` | migrated in 6B | exact | none | `sem002_and_sem004_exact_messages_and_spans`, full corpus |
| SEM003 | `analyze_event_signatures` | `ast_semantic::rules::event_signatures` | migrated in 6B | exact | none | `sem003_exact_arity_and_type_messages_and_spans`, full corpus |
| SEM004 | `analyze_builtin_calls` | `ast_semantic::rules::builtin_calls` | migrated in 6B | exact | none | `sem002_and_sem004_exact_messages_and_spans`, full corpus |
| SEM005 | `analyze_user_function_calls` | `ast_semantic::rules::user_calls` | migrated in 6B | exact | none | `sem005_and_sem006_exact_messages_and_spans`, full corpus |
| SEM006 | `analyze_user_function_calls` | `ast_semantic::rules::user_calls` | migrated in 6B | exact | none | `sem005_and_sem006_exact_messages_and_spans`, full corpus |
| SEM007 | `semantic::ScopeModel::add_variable` | `ast_semantic::rules::duplicate_locals` | migrated in 6A | exact | none | `sem007_exact_messages_spans_and_shadowing`, full corpus |
| SEM008 | `analyze_function_returns` | `ast_semantic::flow::check_returns` | migrated in 6C | exact | none | `sem008_exact_return_messages_and_spans`, full corpus |
| SEM009 | `analyze_jump_labels` | `ast_semantic::flow::check_jumps` | migrated in 6C | exact | none | `sem009_and_sem010_exact_messages_and_spans`, full corpus |
| SEM010 | `analyze_jump_labels` | `ast_semantic::flow::check_jumps` | migrated in 6C | exact | none | `sem009_and_sem010_exact_messages_and_spans`, full corpus |
| SEM011 | `analyze_missing_return_paths` | `ast_semantic::flow::check_missing_returns` | migrated in 6C | exact | none | `sem011_exact_message_and_span`, full corpus |
| SEM012 | `analyze_unresolved_variables` | `ast_semantic::rules::unresolved_variables` | migrated in 6E | exact | none | `sem012_exact_warning_and_suppression_boundaries`, full corpus |
| SEM013 | `analyze_deprecated_function_calls` | `ast_semantic::rules::deprecated_calls` | migrated in 6E | exact | none | `sem013_exact_warning_and_canonical_metadata`, full corpus |
| SEM014 | `analyze_explicit_casts` | `ast_semantic::rules::explicit_casts` | migrated in 6D | exact | none | `sem014_exact_cast_message_and_target_span`, full corpus |
| SEM015 | `analyze_variable_initializers` | `ast_semantic::rules::variable_initializers` | migrated in 6D | exact | none | `sem015_exact_initializer_message_and_name_span`, full corpus |
| SEM016 | `analyze_variable_assignments` | `ast_semantic::rules::assignments` | migrated in 6D | exact | none | `sem016_exact_assignment_message_and_target_span`, full corpus |
| SEM017 | `analyze_compound_assignments` | `ast_semantic::rules::assignments` | migrated in 6D | exact | none | `sem017_exact_messages_and_target_span`, full corpus |
| SEM018 | `analyze_component_assignments` | `ast_semantic::rules::components` | migrated in 6D | exact | none | `sem018_exact_messages_and_component_span`, full corpus |
| SEM019 | `analyze_component_reads` | `ast_semantic::rules::components` | migrated in 6D | exact | none | `sem019_exact_read_message_and_component_span`, full corpus |
| SEM020 | `analyze_state_topology` | `ast_semantic::rules::duplicate_events` | migrated in 6A | exact | none | `sem020_exact_message_and_scope_boundary`, full corpus |
| SEM021 | `analyze_state_topology` | `ast_semantic::rules::state_namespace` | migrated in 6A | exact | none | `sem021_exact_duplicate_and_namespace_collisions`, full corpus |
| SEM022 | `analyze_state_topology` | `ast_semantic::rules::state_targets` | migrated in 6F | exact | none | `sem022_exact_message_and_target_span`, full corpus |
| SEM023 | `analyze_discarded_comparison_expressions` | `ast_semantic::rules::discarded_comparisons` | migrated in 6F | exact | none | `sem023_exact_warning_operator_span`, full corpus |

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
  Legacy even when expression recovery failed. Recovery stops at a top-level
  semicolon so a following call is not swallowed.
- Event nodes retain declared parameter type kinds independently of optional
  parameter names; SEM003 uses that list when arity matches.
- Malformed top-level function headers with `)` + body contribute a recoverable
  signature through `Ast::recovered_functions`, without changing parser
  diagnostics; first-definition selection stays source ordered and recovered
  names participate in SEM021 namespace precedence before built-ins.
- Switch case expressions are traversed in the switch body scope, so locals from
  earlier arms resolve exactly as Legacy ScopeModel sees them.
- Legacy SEM001's top-level-call quirk in global initializers is modeled too:
  identifiers followed by `(` at top level participate in the duplicate scan.
- Firestorm switch/lazy AST forms are traversed without panics and without
  inventing types; the Legacy analyzer remains the reference oracle.

## Phase 6C return and control-flow analysis

- `src/ast_semantic/flow.rs` consumes typed AST nodes and `ScopeTree` only; it
  never accepts or rescans tokens.
- Function bodies and canonical event bodies form independent label/jump code
  scopes. Missing/unclosed bodies and unknown extension events are skipped.
  `ScopeTree::is_ancestor` provides same/ancestor/descendant visibility.
- SEM008 uses the Phase 6B expression typer and first-definition user signatures;
  `ReturnStatement::keyword_span` supplies the exact no-value primary span.
  A malformed non-empty return expression retains a recovered `ReturnStatement`
  with an Error expression at its first token, matching Legacy token scans.
  Labels and jumps inside that recovered range are retained as AST metadata for
  SEM009/SEM010 visibility without rescanning tokens in `flow.rs`.
- SEM009/SEM010 collect labels before resolving jumps, preserving forward jumps,
  first-declaration lookup and nested-block accessibility.
- SEM011 uses a bounded AST CFG with explicit node/edge/work/depth budgets,
  deduplicated edges, fail-closed skip on unsupported/partial graphs and
  suppression when the same function has SEM009/SEM010 findings.
- Constant conditions use an AST-only 32-bit evaluator with the Legacy
  wrapping/division/shift rules, canonical constants, mutable-shadow checks and
  float truthiness. `ForStatement::condition_missing` preserves genuinely empty
  `for (;;)`, while `condition_malformed` marks a non-empty parse failure as an
  unknown condition rather than an infinite loop.
- Firestorm switch/break and Error/recovery control-flow forms conservatively
  suppress SEM011 for the affected function.

## Phase 6D expression validation and assignment semantics

- `src/ast_semantic/expressions.rs` is the shared AST-only traversal for every
  expression and declarator initializer, with the same scope propagation as
  Phase 6A/6B call collection. `calls.rs` now filters this shared fact set, so
  Phase 6B call order/multiplicity is unchanged.
- SEM014 reuses the Phase 6B cast matrix and target-type span.
- SEM015 checks every global/local declarator initializer independently using
  source-order `lookup_value_at` visibility and the existing implicit conversion
  rules.
- SEM016/SEM017 accept only direct ordinary identifier lvalues (matching
  Legacy's conservative boundary) and reuse the binary result-type matrix.
- SEM018 supports direct identifier component targets with arbitrary grouping
  parentheses, plain and all five arithmetic compound operators, the Legacy
  component validity matrix and exact component spans.
- SEM019 marks all recognized component write targets first, then reports
  invalid component reads; a write is never double-reported as a read.
- Unknown/unresolved types and malformed/Error expressions remain conservative.

## Phase 6E symbol warnings and deprecated calls

- SEM012 consumes source-ordered `ExpressionSite`s. Visible variables/parameters
  resolve through `ScopeTree::lookup_value_at`; canonical constants are known;
  direct call callees are not variable references.
- Unknown-call argument ranges suppress SEM012 only when the AST has a real
  matched closing parenthesis. Identifiers in malformed/unmatched calls remain
  eligible, matching Legacy's lack of a suppression range.
- Unknown extension event bodies suppress SEM012 only when the state and event
  bodies are structurally closed. Canonical event bodies are analyzed normally.
- SEM013 is independent: it uses only `definitions().deprecated_functions`,
  suppresses user-function shadowing, requires a real matched close, ignores
  syntax errors inside matched arguments and is unaffected by SEM012's unknown
  event/call boundaries. Candidates are sorted by callee source offset.
- `CallExpression::matched_close` is additive parser metadata distinguishing a
  real `)` from malformed-call recovery; no parser diagnostics changed.
  Malformed unmatched calls retain eligible identifier expressions for SEM012
  without creating an argument suppression range. Malformed event parameter
  identifiers are likewise retained as recovered references, without treating
  them as declared parameters.

## Phase 6F state-target and discarded-comparison rules

- SEM022 builds its known-state set only from complete AST state declarations,
  then scans state changes only inside complete canonical event bodies. Unknown
  extension events, ordinary functions, incomplete states/events and `state
  default;` are outside the rule. Targets are checked against all states before
  reporting, so later declarations are valid.
- SEM023 checks only statement-owned expression roots that reduce through
  parentheses to a comparison. A shared iterative statement walker covers
  functions, canonical/unknown event bodies, nested blocks, unbraced control
  bodies and switch arms, while control conditions are not candidates.
- SEM023 suppresses statement-local syntax diagnostics, Error descendants, and
  any nested call, assignment or increment side effect. Its primary span is the
  exact comparison operator span.
