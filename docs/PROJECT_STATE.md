# Project state

Compact checkpoint for agent handoffs. Update at meaningful slice boundaries, not
after every command. Canonical contracts live in the GitHub Issues; this file only
records verified state.

## Where the project is

- **Phase 8 (Issue #107, AST-first stabilization) is implementation-complete on an
  experimental branch and open as PR [#112](https://github.com/mindwork64/neolint/pull/112),
  not merged.**
- Branch: `exp-preprocessor-ast-phase-08-stabilization`.
- Tested PR head: `4a732925bb1d81330066355a4f9105bfffc3c1bb`.
- Integration base: `66936b0e175d51cba8d83377061229869ef64e96` (`exp/preprocessor-ast`).
- Stable `main`: `98556e21e175190c95f2da39fc0e2ba7a8d9544c` — frozen and unchanged.
- AST is the default engine **only on the experimental branch**. Legacy remains
  selectable (`--engine legacy`) and unchanged; Compare remains opt-in and compares
  exactly `SEM001`–`SEM023`; `SEM024`–`SEM026` are AST-only. v1.0.0 and the release
  workflow are untouched.

## Architecture in one paragraph

CLI → `pipeline::AnalysisRun` (one `AnalysisUnit` per input; checked unit/file
budgets) → `SourceManager` + optional preprocessor (identity, standard or
Firestorm dialect) producing a `PreparedSource` with a `SourceMap` → engine
dispatch: `Legacy` (frozen token scanner, direct path), `Ast` (lex → iterative
typed parser with a 256-depth guard on a 16 MiB worker stack → `ast_semantic`
rules over a name-indexed `ScopeTree`, `ExpressionFacts`, `CallSite` and a bounded
call graph), or `Compare` (both analyzers, 23-code differential report) → rendered
diagnostics through the map (prepared source validated once per unit) → capped
display (10,000 per unit and per invocation, terminal `PRE039`).

## Verified gates

Rust sources are unchanged since `0ea4a604d309aa65b83a0793ee7e8ffa9e2de8e6`
(tree `73d77147843f2b6945ddb25bb0cb12b7b9db8cd4`); every commit after it touches
only `bench/`, `scripts/` and docs. CI on the tested head `4a73292` is green:
[45286936785](https://github.com/mindwork64/neolint/actions/runs/45286936785)
(pull_request) and [45286929926](https://github.com/mindwork64/neolint/actions/runs/45286929926)
(push), with formatting, Clippy, debug tests, release tests, rustdoc, MSRV 1.75,
metadata/documentation tests and whitespace steps all successful.

- `cargo test --all-targets --all-features`: **951 passed, 0 failed** (also 951 in
  release and on Rust 1.75).
- `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`: clean.
- `python3 -m unittest discover --start-directory scripts --pattern 'test_*.py'`:
  **19 passed** (canonical metadata extractor plus benchmark-harness unit tests).
- `python3 scripts/test_documentation.py`: **7 passed**.
- `git diff --check`: clean.

## Performance evidence

- Harness: `bench/bench.py` + `bench/workloads.py` + `bench/README.md`, standard
  library only. Evidence document: `docs/architecture/ast-stabilization.md`.
- Both measured binaries were rebuilt from clean worktrees at the recorded commits
  and re-verified inside the harness (`--verify-build`): base `66936b0` →
  `83d9fce6…`, post-fix `1273c4d` → `227352d9…`.
- Captures ran from a clean harness worktree at `a0fea1e` (tree `8264d70e…`,
  `harness_dirty: false`); the recorded harness source digest is `babcf808…`,
  which still matches the current tree.
- Two independent passes per binary, cross-pass signatures consistent, identical
  70-file input manifest (8,764,087 bytes).
- Headline effects: `diagnostic_flood/default` 173.59 → 91.18 ms (**−47.5 %**) and
  `many_symbols/default` 48.97 → 30.12 ms (**−38.5 %**). The near-limit row
  (7.65 MiB, 95.7 % of the 8 MiB root limit) is unchanged within noise
  (486.52 → 496.71 ms), and the Legacy negative control moves 22–36 % on sub-10 ms
  rows, which bounds the noise envelope.
- Peak RSS for the near-limit input is ≈310 MB (≈2.5 KB per statement, roughly
  linear) — a capacity observation, not a violated limit.

## Limits

`docs/architecture/preprocessor-ast.md` §12 maps every limit to its constant,
enforcement point, scope, behaviour on exceed and boundary test. Named constants
`MAX_INCLUDE_DEPTH` and `MAX_ORIGIN_DEPTH` replaced bare literals; N/N+1 tests
cover included-file size, origin-chain depth and invocation unit capacity.

## Known problems and open follow-ups

- #108 preprocessor `DiagnosticKind` and JSON related locations (policy).
- #109 `#pragma once` through the Firestorm inventory include layer.
- #110 duplicated CFG/constant limit constants across AST and Legacy engines.
- #111 `--help` omits `--preprocess`, `--dialect`, `-I`.
- Measured limitations: single-host benchmark evidence, Linux-only RSS,
  sub-10 ms rows dominated by process startup, `MAX_TYPE_DEPTH` unreachable
  through the parser (defensive), pre-existing one-line difference between stable
  `ci.yml` and frozen `main` (Phase 0 commit `d56b392`, deliberately not modified).

## Next action

Review and merge decision for PR #112 is a separate authorization. Any change to
stable `main`, v1.0.0 or Legacy retirement requires its own Issue.
