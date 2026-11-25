# SEM rule ownership matrix

Phases 6A–6F migrate all existing SEM001–SEM023 diagnostics to the AST engine.
No SEM rule remains Legacy-only in the current matrix.

## Engine selection and compare scope (Phase 6G)

- `--engine` / `engine = legacy|ast|compare` selects the analyzer. AST is the
  default engine; Legacy is the explicit v1.0.0 compatibility engine, selected
  with `--engine legacy` or `engine = legacy`.
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

## AST-only global initializer validation (SEM027)

- `ast_semantic::rules::global_initializers` checks only top-level global
  declarator initializers. It does not change SEM015's global/local type checks.
- The rule accepts scalar literals, visible global-variable references,
  canonical built-in constants, vectors/rotations with simple numeric
  components, and flat lists of simple values. It does not evaluate operators
  or calls; parenthesized forms and nested list literals are rejected.
- Malformed/Error expressions, unreliable lexical/preprocessor environments,
  and expressions exceeding the 128-node or 64-level budget are skipped rather
  than diagnosed.
- SEM027 has no Legacy counterpart. It is emitted by the AST default and
  explicit AST engine only; Legacy and the SEM001–SEM023 Compare contract are
  unchanged.

## Firestorm switch semantics (SEM028–SEM033)

- `ast_semantic::rules::firestorm_switches` checks only typed AST switch nodes;
  it therefore runs only when the Firestorm switches feature gate was enabled.
- The rule reports duplicate `default` labels and provably incompatible case
  types as semantic errors. It warns on duplicate constant case values, a
  missing `default`, constant switch expressions, and list comparisons.
- Duplicate detection and constant evaluation are bounded. Dynamic or unknown
  case expressions are not guessed; malformed switches and switches overlapped
  by parser errors are skipped. An unreliable lexical/preprocessor environment
  suppresses the rule.
- These diagnostics are AST-only and remain outside the SEM001–SEM023 Compare
  parity set. Legacy remains unchanged.
- The flow graph evaluates constant Firestorm switch values and case labels
  using the same equality rules as switch diagnostics: numeric values, strings,
  keys, vectors, rotations and list-length equality. Flow-sensitive local
  integer/float facts are also used for numeric switch values. Matching entries
  are selected while source-order fallthrough, the optional no-match path and
  break to the innermost switch exit are preserved. Unknown values or
  unsupported case expressions retain every possible arm instead of guessing.
  Nested switches own their break targets, including when nested inside loops.

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
- Constant conditions use a bounded AST-only numeric evaluator and a local
  scalar data-flow environment. It supports 32-bit integer wrapping/division/shift
  rules, finite f32 arithmetic and comparisons, canonical integer/float constants,
  integer-to-float casts and integer logical operators when both operands are
  known. Definitely assigned local integer/float values propagate through a
  forward CFG worklist to a fixed point across branches, loops and valid
  jump/label edges. Safe scalar increments and arithmetic compound assignments
  update known values. A single nested write is tracked through its containing
  expression; multiple writes with unspecified relative evaluation order
  invalidate only the potentially written local facts. Direct assignment and
  increment results can determine an enclosing condition when their values are
  known. Mutable globals/parameters and call results remain unknown. LSL
  evaluates both operands of logical AND/OR; no short-circuit is assumed.
  Non-finite results, unsafe float-to-integer conversions and unsupported
  mixed-operand operators remain unknown. Local facts are capped at 4,096 per
  environment; side-effect/error-expression scans, CFG edges, switch dispatch
  work, worklist iterations and cumulative fact-processing work are bounded.
  Oversized expressions are treated as unknown before recursive flow reasoning.
  Incompatible SEM029 case labels and duplicate SEM028 defaults keep all switch
  arms reachable; partial case-label evaluation that exhausts its aggregate
  budget also falls back to every arm. Exceeding any analysis budget suppresses
  conclusions for the affected body.
  Empty for-loop conditions and malformed conditions retain their distinct
  semantics.
- This is a deliberate post-migration AST enhancement. Legacy remains frozen;
  Compare may therefore report intentional SEM011 differences when the AST
  proves a floating-point condition that the Legacy integer evaluator leaves
  unknown.
- Firestorm switches are analyzed by the shared CFG. Constant values prune
  impossible case dispatches using the same scalar/composite equality semantics
  as SEM028–SEM033, while flow-sensitive local numeric facts can also select a
  case. Source-order fallthrough remains intact; unknown values or unsupported
  labels keep all possible arms reachable. Malformed/recovered control-flow
  forms and budget exhaustion still suppress SEM011 for the affected function.
  Constant dispatch is disabled for semantically invalid switches (SEM028 or
  incompatible SEM029 labels); an unknown or over-budget label set also keeps
  all arms reachable rather than inferring a partial dispatch.

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

## Phase 7 AST-only rules (SEM024–SEM026)

Phase 7 adds `SEM024` (unused variable), `SEM025` (unused parameter), and
`SEM026` (unreachable function) as new AST-native rules. They are **outside**
the SEM001–SEM023 differential parity contract: the matrix above records the
migration of the 23 pre-existing Legacy diagnostics, and these three rules have
no Legacy counterpart to migrate. Legacy remains frozen and emits none of them.

- The raw comparator still covers exactly `SEM001`–`SEM023`. The new codes are
  never counted as parity mismatches, never enter the Legacy display stream, and
  never appear in the Compare report.
- `--engine ast` and the AST default emit them. Explicit `--engine legacy` emits
  none of them, and `--engine compare` stays the SEM001–SEM023 parity diagnostic
  mode.
- Their boundaries are independent of the parity matrix. `SEM024`/`SEM025` are
  syntactic read/use analyses over `(ScopeId, name-span start)` declaration
  identity, not runtime liveness. `SEM025` covers only user-function and
  canonical-event parameters. `SEM026` is iterative function reachability from
  event handlers and global initializers, not statement reachability or
  whole-program runtime reachability.
- All three fail closed on malformed units. `SEM026` also suppresses the whole
  rule when its call-graph node, edge, or work budget overflows, rather than
  reporting a partial reachability result.


## Mono global-function state-change context (SEM034/SEM035)

- `ast_semantic::rules::state_transitions` scans complete top-level
  user-defined function bodies, not event-handler bodies. This is lexical
  context validation, not call-graph reachability: an unused helper is still a
  global function and is checked even if no event can reach it.
- `SEM034` is an error for a state change in a global function. The one
  historical Mono compatibility shape recognized separately is an unbraced
  state-change statement used as the direct then-branch of an `if` with no
  `else`; it receives `SEM035` warning because it is unsupported and not
  portable behavior. An `if/else`, a block body, and loop bodies do not qualify.
- Event-handler state changes remain valid and continue to use SEM022 for
  undefined targets. SEM022's Legacy/AST parity contract is unchanged; it still
  does not inspect ordinary function bodies.
- The rule fails closed on unreliable preprocessing/lexing and suppresses
  individual findings when a syntax diagnostic touches the statement. Unclosed
  function bodies are skipped. The traversal is capped at 16,384 statements;
  overflow suppresses the whole rule rather than emitting partial findings.
- Both codes are AST-only, do not enter the SEM001–SEM023 comparator, and do
  not change Legacy behavior.


## Return and unreachable-code hazards (SEM036/SEM037)

- `ast_semantic::rules::return_hazards` checks complete top-level function and
  event-handler bodies. `SEM036` reports a value-return from an event handler,
  including unknown extension events. `return;` remains valid; function value
  returns remain governed by SEM008.
- SEM037 uses the shared bounded CFG to report the first unreachable
  non-empty statement in each straight-line sequence. It covers nested blocks,
  conditional branches, loop bodies, switch arms and statements following
  guaranteed termination. The CFG prunes statically known conditions using
  definitely assigned local numeric values. A forward worklist merges facts at
  each reachable join and iterates loop back-edges and valid jump/label edges
  to a fixed point. Safe increments and arithmetic compound assignments update
  facts; ambiguous multiple writes invalidate only touched locals.
  Right-associative assignment chains are evaluated from the inner assignment
  outward; compound assignments with effectful right operands remain unknown
  when read/write ordering cannot be established. Assignment and increment
  results can also determine conditions.
- The CFG models zero-iteration and non-terminating for/while/do-while paths,
  valid jump edges, constant switch dispatch, source-order fallthrough,
  default, and break to the innermost switch exit. Firestorm string/key/vector/
  rotation constants and list-length equality use the same comparison semantics
  as SEM028–SEM033; unknown/unsupported switch values retain all possible arms.
  A nested switch shadows the surrounding
  switch break target, while a break inside a loop nested in a switch still
  exits that switch. A switch whose feasible arms all terminate has no
  reachable exit; a reachable break or unmatched path keeps the exit reachable.
- Per-environment facts, side-effect/error-expression traversals, switch
  dispatch, CFG edge visits, worklist iterations and cumulative fact-processing
  work are bounded. CFG joins intersect incoming must-states in place, without
  cloning both maps and allocating a third map for each edge. A disabled input
  disables the merged environment; a budget overflow never yields a partial
  reachability result.
  Invalid jump scopes, duplicate/missing labels, unvalidated jumps in unknown
  extension events, parser errors, error nodes, unclosed blocks and budget
  exhaustion suppress SEM037 for the affected body.
- SEM036 retains its earlier conservative suppression in bodies containing
  jumps/labels. Both diagnostics are AST-only, outside SEM001–SEM023 Compare,
  and do not alter Legacy output.


## State and list semantics (SEM038–SEM040)

- SEM038 requires each complete state declaration to contain at least one
  event handler. It skips incomplete state bodies, malformed state ranges and
  unreliable lexical/preprocessor environments. Canonical and extension event
  handlers both satisfy the requirement.
- SEM039 checks each list element with the AST scope/type resolver. It reports
  nested list literals, visible variables of type list, list-returning calls and
  other expressions with a provable list result. Unknown types and malformed
  expressions are skipped. The traversal is capped at 65,536 work items and
  fails closed on overflow.
- SEM040 warns on == / != when both operands are provably lists. Its message
  distinguishes length-only equality from the length-difference result of
  inequality. It reports at the operator and skips malformed or syntax-damaged
  expressions.
- All three rules are AST-only and remain outside the SEM001–SEM023 Compare parity contract. Legacy output is unchanged.


## AST-only operator and LSL hazard rules (SEM041–SEM046)

- SEM041 checks only a bare top-level plain assignment used as an `if`
  condition. An extra pair of grouping parentheses marks an intentional
  assignment. Compound assignments and assignments nested in larger
  expressions are outside the rule.
- SEM042 propagates known event contexts through matched direct calls to
  user-defined functions. Detection-only built-ins are checked against their
  documented event contexts; unknown extension events and unreachable helper
  functions do not contribute guessed contexts.
- SEM043 reports type-valid division and integer remainder when a bounded,
  side-effect-free evaluator proves the divisor is zero. It also checks
  `llSqrt`, `llPow`, `llAcos` and `llAsin` for statically imaginary results;
  inverse sine/cosine arguments must be within `[-1.0, 1.0]`. It warns when
  `llLog`/`llLog10` receive known non-positive arguments because LSL silently
  returns `0.0` instead of a logarithm. Mutable variables and user-defined
  calls are not evaluated, built-in shadowing is respected, and vector `%`
  remains a cross product. Mono returns `NaN` for imaginary results; legacy LSO
  triggers a Math Error for those cases.
- SEM044 constructs a bounded over-approximation of state transitions from
  event-handler bodies, rooted at `default`. Conditional and textually
  unreachable state changes count as possible edges. Global-function state
  changes remain under SEM034/SEM035, and empty states remain under SEM038.
- SEM045 validates only confirmed operator/type constraints: unary `-` accepts
  integer/float/vector/rotation, `!` and `~` require integer, logical,
  bitwise and shift operators require two integer operands,
  and `++`/`--` require a known integer/float variable. Built-in constants are
  not writable. Unary plus is intentionally outside the rule. Unknown
  expression types are skipped rather than guessed.
- SEM046 validates only ordinary arithmetic and comparison operands. It
  requires both operand types to be known and reuses
  `types::infer_binary_result_type`, avoiding a second copy of the LSL operator
  matrix. Unknown operands are skipped. The check is capped at 65,536 collected
  expression sites and fails closed on parser errors or unreliable lexical/
  preprocessor input.
- SEM041–SEM046 are AST-only and remain outside the SEM001–SEM023 differential
  Compare contract. They do not change Legacy output. Rules that depend on
  complete lexical/preprocessor and parser input fail closed when that input
  is unreliable; their individual resource limits are documented alongside
  each rule.
