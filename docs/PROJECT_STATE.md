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
  `SEM028`–`SEM033` and switch-aware return-flow analysis; Legacy and Compare
  remain unchanged.
- Phase 8: reproducible benchmark harness, resource-limit reconciliation and
  profile-guided fixes.
- Phase 8 follow-ups #108–#111 are integrated and closed:
  - JSON v1 preprocessor `origin` and `related` locations are additive.
  - Boost.Wave `#pragma once` behavior is source-audited and tested.
  - AST/Legacy numeric resource-limit policy parity is regression-tested.
  - CLI help documents implemented preprocessing options.
- Legacy and AST remain independent implementations. Compare fingerprints
  semantic diagnostics `SEM001`–`SEM023`; `SEM024`–`SEM033` are AST-only.

## Architecture

CLI → `AnalysisRun` → `AnalysisUnit` → SourceManager / optional preprocessor →
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
