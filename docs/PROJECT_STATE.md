# Project state

Compact checkpoint for agent handoffs. Canonical contracts live in the source,
tests and user-facing documentation; update this file at meaningful boundaries.

## Current release line

- **v1.3.0 release preparation** builds on the v1.2.0 AST-first engine and
  adds the multi-script project model, the LSP server, and the VS Code package.
  The release tag and GitHub Release are intentionally created only after the
  release-preparation PR is merged and approved.
- `ast` is the default CLI and `LintConfig::default()` engine.
- `legacy` remains available through `--engine legacy` or `engine = legacy`.
  It is not deprecated or removed.
- `compare` remains opt-in and is intended for maintainers; it displays Legacy
  diagnostics and reports the semantic comparison on stderr.
- The planned v1.3.0 binary targets are Linux x86_64 and Windows x86_64.
  Each platform archive contains both `neolint` and `neolint-lsp`.
- The VS Code extension is packaged as a VSIX and attached to the GitHub
  Release. Marketplace publication is not configured.
- Normal CLI and LSP analysis is local and offline-by-default; the language
  server is an adapter over the same Rust analysis API, not a second analyzer.

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
  pairs using the shared LSL type matrix. The LSLint compatibility follow-up adds
  `SEM053` (transition to current state), `SEM054` (multiple LSO jump sites),
  `SEM055` (empty if), `SEM056` (constant condition, including guarded
  local/global constant propagation), `SEM057` (`print()`), and
  `SEM058` (God Mode-only calls, guarded by explicit `--god-mode`). These are
  AST-only diagnostics; the Legacy engine remains frozen.
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
  `tests/ast_semantic_rules.rs`. A real Firestorm-Releasex64 6.6.17.70368
  (Mono) compile/runtime smoke test confirmed the supported forms in a live
  region; the malformed one-index list extraction failed compilation as
  expected. This test does not verify the separate `#pragma once` inventory
  integration.
- Target profiles separate platform/runtime from preprocessing dialect. Selectors: second-life-mono (default), second-life-lso, second-life-luau, opensim-xengine, opensim-yengine. OpenSim additionally selects a pinned API version: 0.9.2.2/API20, 0.9.3.0/API22, 0.9.3.1/API23, or master-2026-11-25/API24. YEngine defaults to API24; XEngine is restricted to API20.
- Pinned catalogues cover LSL function signatures/overloads, OSSL signatures, event handlers and constants. XEngine API20: 425 LL functions / 425 signatures, 231 OSSL names / 258 signatures, 35 events, 770 constants. YEngine API20/API22/API23/API24: 425/434/457/473 LL names, 425/434/458/474 signatures, 231/231/258/265 OSSL names, 258/258/289/296 OSSL signatures, 36/36/37/37 events, 770/780/815/888 constants.
- XEngine events come from its own lexer/parser event productions; YEngine events come from the event-code enum intersected with handler declarations. SEM047 reports standard SL functions absent from OpenSim; SEM049 reports missing canonical SL events; SEM050 reports missing canonical SL constants.
- OSSL is separate and is not assumed enabled on a region. OsslAllowlist supports strict-empty mode (ossl_allowlist = strict / --strict-ossl) and repeatable expected-enabled functions; SEM048 warns outside that allowlist. Live permissions are not queried.
- The selected catalogue flows through overload/arity/type checks, return-type inference, event/constant checking, call graph, namespace and reachability. The Second Life/OpenSim target-profile catalogues are pinned snapshots and are not claimed to have been exercised against every live runtime; OSSL permissions are not queried. The Lazy List smoke test above is a separate, explicitly verified Firestorm case.

- Legacy and AST remain independent implementations. Compare fingerprints
  semantic diagnostics `SEM001`–`SEM023`; `SEM024`–`SEM058` are AST-only.

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
- The v1.2.0 line recorded its full Debug, Release and Rust 1.75.0 test
  matrix, Clippy, Rustdoc, Python catalogue/documentation checks and Astro
  production build. The v1.3.0 release-preparation PR adds explicit LSP
  initialize/shutdown smoke tests, VSIX content validation, and tagged
  Linux/Windows archive plus checksum validation. Use the PR's CI results as
  the authoritative verification for this release candidate.
- Real-script save-and-lint experiment used a 1,234-line Firestorm script:
  median save+lint 9.328 ms Legacy, 9.916 ms AST and 10.944 ms Compare on a
  Ryzen 5 2600X. This is a fresh-process CLI measurement, not an incremental
  language-server benchmark.

## Multi-script project model

The manifest-driven CLI project mode is integrated on `main`. Every listed
root script receives its own AnalysisUnit, preprocessor, parser, symbol table,
semantic state and diagnostic stream. Project defaults, per-script config
overlays, invocation-wide config overlays and CLI options are merged
deterministically. Includes remain scoped to their own root script; no
project-wide symbol table or cross-script name resolution is created. Script
and diagnostic order follows the manifest. See [project-model.md](project-model.md).

## Language server and VS Code client

- `neolint-lsp` is an optional Cargo binary enabled by the `lsp` feature.
  The CLI's default build does not pull in the LSP dependency set.
- The server provides standard LSP diagnostics, full-text synchronization,
  initialization/shutdown, cancellation-aware background analysis,
  dependency-scoped invalidation, multi-root workspace configuration, hover,
  definition, completion and the safe SEM041 quick fix.
- Each open `.lsl` root has an independent analysis/configuration snapshot.
  Hover, definition and completion are scoped to that current root; there is no
  cross-file include navigation or workspace-wide symbol index.
- Affected roots are reanalyzed in full. Incremental parsing/AST reuse is not
  implemented.
- The VS Code extension is in `editors/vscode`. It includes the production
  `vscode-languageclient` runtime dependency in its VSIX. The release package
  is installed from VSIX; Marketplace publishing is not configured.
- The LSP initialize response follows LSP 3.17 `InitializeResult` shape:
  capabilities are nested under `result.capabilities`, and `serverInfo`
  reports the server name and compiled package version.

## Release policy and known limitations

- `main` is the stable release branch; changes enter it through dedicated
  branches and pull requests.
- v1.3.0 is the release-preparation target. Do not create the tag or GitHub
  Release until the preparation PR is merged and the release is explicitly
  authorized. The operational sequence is documented in [releasing.md](releasing.md).
- Legacy retirement is not part of v1.3.0 and requires a separate decision.
- Known evidence limits: benchmark timings are single-host/Linux measurements;
  sub-10 ms rows include process-startup and scheduler noise; peak RSS was
  measured only on Linux. `MAX_TYPE_DEPTH` remains a defensive guard that is
  unreachable through the current parser. Firestorm `#pragma once` inventory/
  asset integration remains unverified even though the separate Lazy List
  compile/runtime smoke test passed.
- A clean lint result does not guarantee acceptance by the official compiler
  or any simulator.
