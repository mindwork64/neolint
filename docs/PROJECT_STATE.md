# Project state

Compact checkpoint for agent handoffs. Canonical contracts live in the source,
tests and user-facing documentation; update this file at meaningful boundaries.

## Current release line

- **v1.1.0 promotes the AST-first implementation to stable `main`.**
- `ast` is the default CLI and `LintConfig::default()` engine.
- `legacy` remains available through `--engine legacy` or `engine = legacy`.
  It is not deprecated or removed in this release.
- `compare` remains opt-in and is intended for maintainers; it displays Legacy
  diagnostics and reports the semantic comparison on stderr.
- The v1.1.0 release targets remain Linux x86_64 and Windows x86_64.
- Neolint remains a deterministic, offline-by-default CLI tool. No IDE
  integration, language server, incremental analysis or autofix is included.

## Completed implementation

- Phases 1–5 and 6A–6H: SourceManager/SourceMap, LSL and Firestorm
  preprocessing, include handling, macro expansion, typed AST parser, semantic
  engine selection, and AST default.
- Phase 7: AST-native `SEM024` (unread variables), `SEM025` (unread parameters)
  and `SEM026` (unreachable user functions).
- Follow-up semantic coverage: AST-only `SEM027` validates simple assignable
  values in global variable initializers, plus Firestorm switch diagnostics
  `SEM028`–`SEM033` and switch-aware return-flow analysis. The follow-up
  `SEM034`/`SEM035` adds AST-only Mono state-change context checks. A further
  AST-only control-flow slice adds `SEM036` (event value-return errors) and
  `SEM037` (conservative unreachable-statement warnings). The LSL state/list
  follow-up adds `SEM038` (empty states), `SEM039` (nested lists), and `SEM040`
  (list equality/inequality hazards), `SEM041` (bare assignments in `if`
  conditions), `SEM042` (detection-context-only calls), and `SEM043`
  (zero divisors, invalid `llSqrt`/`llPow`/`llAcos`/`llAsin` domains and
  non-positive `llLog`/`llLog10` arguments). `SEM044` warns on populated
  states with no transition path from `default`, and `SEM045` validates known
  operand types for unary, logical, bitwise/shift and increment/decrement operators;
  `SEM046` validates known-incompatible arithmetic and comparison operand
  pairs using the shared LSL type matrix. Legacy and Compare remain unchanged.
- Phase 8: reproducible benchmark harness, resource-limit reconciliation and
  profile-guided fixes.
- Phase 8 follow-ups #108–#111 are integrated and closed:
  - JSON v1 preprocessor `origin` and `related` locations are additive.
  - Boost.Wave `#pragma once` behavior is source-audited and tested.
  - AST/Legacy numeric resource-limit policy parity is regression-tested.
  - CLI help documents implemented preprocessing options.
- Firestorm Lazy Lists are represented structurally in the AST when their independent
  `lazy_lists` gate is enabled: single-index writes, typed extraction for
  integer/float/string/key/vector/rotation, two-index list ranges, one optional
  parenthesis pair, nested index/RHS expressions, and Firestorm's `quaternion`
  alias for rotation extraction. The alias does not extend the seven LSL value
  types; it is accepted only for the Firestorm lazy-list form. Bare reads,
  compound indexed writes and invalid arities fail closed. Parser/semantic
  regression tests are in `tests/ast_firestorm.rs` and
  `tests/ast_semantic_rules.rs`; live Viewer/grid verification remains an
  external manual gate.
- Target profiles separate platform/runtime from preprocessing dialect. Selectors: second-life-mono (default), second-life-lso, second-life-luau, opensim-xengine, opensim-yengine. OpenSim additionally selects a pinned API version: 0.9.2.2/API20, 0.9.3.0/API22, 0.9.3.1/API23, or master-2026-11-25/API24. YEngine defaults to API24; XEngine is restricted to API20.
- Pinned catalogues cover LSL function signatures/overloads, OSSL signatures, event handlers and constants. XEngine API20: 425 LL functions / 425 signatures, 231 OSSL names / 258 signatures, 35 events, 770 constants. YEngine API20/API22/API23/API24: 425/434/457/473 LL names, 425/434/458/474 signatures, 231/231/258/265 OSSL names, 258/258/289/296 OSSL signatures, 36/36/37/37 events, 770/780/815/888 constants.
- XEngine events come from its own lexer/parser event productions; YEngine events come from the event-code enum intersected with handler declarations. SEM047 reports standard SL functions absent from OpenSim; SEM049 reports missing canonical SL events; SEM050 reports missing canonical SL constants.
- OSSL is separate and is not assumed enabled on a region. OsslAllowlist supports strict-empty mode (ossl_allowlist = strict / --strict-ossl) and repeatable expected-enabled functions; SEM048 warns outside that allowlist. Live permissions are not queried.
- The selected catalogue flows through overload/arity/type checks, return-type inference, event/constant checking, call graph, namespace and reachability. Real Viewer/OpenSim in-world execution remains a manual external verification step; repository tests validate pinned snapshots and analyzer behavior, not a running grid.

- Legacy and AST remain independent implementations. Compare fingerprints
  semantic diagnostics `SEM001`–`SEM023`; `SEM024`–`SEM050` are AST-only.

## Architecture

CLI/config target profile + preprocessing dialect → `AnalysisRun` → `AnalysisUnit` → SourceManager / optional preprocessor →
PreparedSource + SourceMap → Legacy, AST or Compare → source-map diagnostic
rendering → suppression and bounded display (`PRE039`).

Plain no-preprocess Legacy retains its historical direct path. AST uses identity
preparation even when preprocessing is disabled. All preprocessed engine modes
share the same prepared source and source map.

## Verified implementation evidence

- Final Phase 8 source and documentation state is recorded in
  [`preprocessor-ast.md`](architecture/preprocessor-ast.md).
- Benchmark methodology and limitations are recorded in
  [`ast-stabilization.md`](architecture/ast-stabilization.md).
- Phase 8 CI recorded 951 passing Rust tests in debug, release and Rust 1.75,
  plus Python metadata/harness and documentation tests. The v1.1.0 promotion
  CI repeats those gates against the release branch and the merge candidate.
- Real-script save-and-lint experiment used a 1,234-line Firestorm script:
  median save+lint 9.328 ms Legacy, 9.916 ms AST and 10.944 ms Compare on a
  Ryzen 5 2600X. This is a fresh-process CLI measurement, not an incremental
  language-server benchmark.

## Release policy and known limitations

- `main` is the stable release branch; feature development continues through
  dedicated branches and pull requests.
- Legacy retirement is not part of v1.1.0 and requires a separate decision.
- Known evidence limits: benchmark timings are single-host/Linux measurements;
  sub-10 ms rows include process-startup and scheduler noise; peak RSS was
  measured only on Linux. `MAX_TYPE_DEPTH` remains a defensive guard that is
  unreachable through the current parser.
- A clean lint result does not guarantee acceptance by the official compiler
  or any simulator.
