# Project state

Compact checkpoint for agent handoffs. Update at meaningful slice boundaries, not
after every command. Canonical contracts live in the GitHub Issues; this file only
records verified state.

## Where the project is

- Phase 8 (Issue #107, AST-first stabilization) is **implementation-complete on an
  experimental branch, PR open, not merged**.
- AST is the default engine **only on `exp/preprocessor-ast`** and its feature
  branches. Stable `main` is frozen at
  `98556e21e175190c95f2da39fc0e2ba7a8d9544c` and is unchanged.
- Legacy remains selectable (`--engine legacy`) and unchanged; Compare remains
  opt-in and compares exactly `SEM001`–`SEM023`; `SEM024`–`SEM026` are AST-only.
- v1.0.0 and the release workflow are untouched.

## Architecture in one paragraph

CLI → `pipeline::AnalysisRun` (one `AnalysisUnit` per input; checked unit/file
budgets) → `SourceManager` + optional preprocessor (identity, standard or
Firestorm dialect) producing a `PreparedSource` with a `SourceMap` → engine
dispatch: `Legacy` (frozen token scanner, direct path), `Ast` (lex → iterative
typed parser with a 256-depth guard on a 16 MiB worker stack → `ast_semantic`
rules over `ScopeTree`/`ExpressionFacts`/`CallSite`/call graph), or `Compare`
(both analyzers, 23-code differential report) → rendered diagnostics through the
map (validated once per unit) → capped display (10,000 per unit and per
invocation, terminal `PRE039`).

## Verified gates (last run)

Recorded for the exact tested revisions; re-run only when source or tests change.

- `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo test --all-targets --all-features`: 951 passed, 0 failed.
- `cargo test --release --all-targets --all-features`: same count.
- `cargo +1.75.0 test --all-targets --all-features`: same count (MSRV 1.75).
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`: clean.
- Python metadata/documentation gates: 10 + 7 tests OK.
- `git diff --check`: clean.

## Performance evidence

- Harness: `bench/bench.py` (+ `bench/workloads.py`, `bench/README.md`), stdlib
  only. Baseline and post-fix runs live in `bench/results/`.
- Evidence document: `docs/architecture/ast-stabilization.md`.
- Two measured fixes: per-diagnostic whole-buffer validation hoisted to per-unit
  (flood −49 % end-to-end), and name-indexed scope lookup (4,000-declaration
  analysis 86 ms → 7.8 ms). No regression beyond the stated noise floor.

## Limits

`docs/architecture/preprocessor-ast.md` §12 maps every limit to its constant,
enforcement point, scope, behaviour on exceed and boundary test. Named constants
`MAX_INCLUDE_DEPTH` and `MAX_ORIGIN_DEPTH` replaced bare literals; new N/N+1 tests
cover included-file size, origin-chain depth and invocation unit capacity.

## Known problems and open follow-ups

- #108 preprocessor `DiagnosticKind` and JSON related locations (policy).
- #109 `#pragma once` through the Firestorm inventory include layer.
- #110 duplicated CFG/constant limit constants across AST and Legacy engines.
- #111 `--help` omits `--preprocess`, `--dialect`, `-I`.
- Measured limitations: single-host benchmark evidence, Linux-only RSS,
  sub-5 ms rows dominated by process startup, `MAX_TYPE_DEPTH` unreachable through
  the parser (defensive), pre-existing one-line difference between stable `ci.yml`
  and frozen `main` (Phase 0 commit `d56b392`, deliberately not modified).

## Next action

Review and merge decision for the Phase 8 PR is a separate authorization. Any
change to stable `main`, v1.0.0 or Legacy retirement requires its own Issue.
