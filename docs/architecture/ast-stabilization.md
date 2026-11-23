# AST-first stabilization evidence (Phase 8)

Status: evidence package for GitHub Issue #107. This document answers, with
reproducible measurements and verifiable binary provenance, whether the AST-first
experimental implementation is stable enough to support a *separate* future
decision about stable `main` / v1.0.0. It does not authorize that decision, does
not change `main`, and does not remove the Legacy engine.

**Verdict: QUALIFIED WITH FOLLOW-UPS.** Every required gate is green on the PR
head, the documented resource limits are reconciled with their enforcement
points, both measured binaries are built from verified clean worktrees, the
harness and raw samples are committed and reproducible, and two measured
bottlenecks were fixed with large, repeatable gains. Four documented limitations
remain and are tracked as separate Issues (#108–#111); none is a correctness
defect in the analyzed paths.

## 1. What is measured

All figures are **end-to-end CLI** measurements: a real subprocess invocation of
`target/release/neolint`, timed with `time.perf_counter_ns()`, including process
startup. Three warm-up iterations precede the measured samples.

Path caveats that must accompany every ratio below:

- Plain `--engine legacy` runs a direct no-preprocess path.
- AST always runs an identity-prepared unit (`prepare` → lex → parse → rules →
  map-based rendering), so a Legacy/AST ratio is **not** a parser-only comparison.
- `--engine compare` runs both analyzers by design and is not a single-engine
  baseline.
- Rows below ~5 ms are dominated by process startup and scheduler noise. The
  Legacy rows act as a negative control: their code is identical in both binaries,
  so their spread measures session noise (observed up to ±26 % on sub-10 ms rows).
  Claims are only made where the effect clearly exceeds that envelope.

## 2. Provenance

Harness source revision and measured binary source revision are recorded
separately. The harness revision is whatever commit produced `bench/bench.py`;
the measured binary revision is passed explicitly and verified by SHA-256.

| | Baseline | Post-fix |
| --- | --- | --- |
| Binary source commit | `66936b0e175d51cba8d83377061229869ef64e96` | `1273c4d325428c9fa95a49c9122bfc99ae5d9e34` |
| Binary source tree | `87d810459064f438fe61c0a632fa83642ea68971` | `73d77147843f2b6945ddb25bb0cb12b7b9db8cd4` |
| Binary SHA-256 | `83d9fce60666773fc73318a70a163b147c428eacd155eb89ece85486295f8ac8` | `227352d90b1ed1233046e6e7073726c43a354352e94c1e3ac06cc7509843babc` |
| Binary bytes | 1,659,648 | 1,675,040 |
| Build worktree | `target/wt-base`, clean, `HEAD = 66936b0` | `target/wt-fixed`, clean, `HEAD = 1273c4d` |
| Verification | `--expect-binary-sha256` matched; `binary_source_verified: true` | same |

Both binaries were rebuilt from fresh `git worktree` checkouts of the exact
commits above with `cargo build --release --bin neolint` on the recorded
toolchain, and every build worktree reported `git status --porcelain` empty. The
post-fix SHA-256 is byte-identical to the binary built from the primary working
tree, which also demonstrates that the build is path-independent here.

Each `bench/results/*/meta.json` records `harness_commit`, `harness_tree`,
`harness_branch`, `binary_source_commit`, `binary_source_tree`,
`binary_source_verified`, `binary_sha256` and `binary_sha256_expected`. The
harness refuses to measure when the expected digest does not match
(`--expect-binary-sha256`); the earlier run that recorded a misleading
`commit`/`branch` pair was discarded and is not referenced anywhere.

## 3. Method and reproduction

Harness: `bench/bench.py` with `bench/workloads.py` (Python 3 standard library
only, no new dependencies, no network).

```bash
# One-time: build the two measured binaries from clean worktrees
git worktree add target/wt-base 66936b0e175d51cba8d83377061229869ef64e96
( cd target/wt-base && cargo build --release --bin neolint )
git worktree add target/wt-fixed 1273c4d325428c9fa95a49c9122bfc99ae5d9e34
( cd target/wt-fixed && cargo build --release --bin neolint )

# Baseline (two independent passes)
python3 bench/bench.py --passes 2 --skip-build \
  --binary target/wt-base/target/release/neolint \
  --binary-source-commit 66936b0e175d51cba8d83377061229869ef64e96 \
  --expect-binary-sha256 83d9fce60666773fc73318a70a163b147c428eacd155eb89ece85486295f8ac8 \
  --out bench/results/baseline-66936b0

# Post-fix (two independent passes)
python3 bench/bench.py --passes 2 --skip-build \
  --binary target/wt-fixed/target/release/neolint \
  --binary-source-commit 1273c4d325428c9fa95a49c9122bfc99ae5d9e34 \
  --expect-binary-sha256 227352d90b1ed1233046e6e7073726c43a354352e94c1e3ac06cc7509843babc \
  --out bench/results/fixed-1273c4d
```

To rebuild from source instead of a worktree, drop `--skip-build --binary` and
pass `--binary-source-commit`/`--expect-binary-sha256` for the record.

Every sample records exit status and SHA-256 of stdout and stderr. A row whose
exit status or digests vary is a hard failure (`exit 4`), never averaged. Both
committed runs report stable digests and a single exit code per row.

Peak RSS is the kernel's `VmHWM` for the child, polled untimed from
`/proc/<pid>/status` in three separate runs per row. `ru_maxrss` from `os.wait4`
was rejected after measurement showed it reports the harness footprint
(≈26 MB for every row) because a forked child inherits the parent before `exec`.
On hosts without `/proc` the report says `unavailable`; no value is estimated.

### Raw measurements

Uncompressed `raw.jsonl` is regenerated and git-ignored. Compressed per-sample
data is committed next to each summary:

| Run | File | Bytes | SHA-256 |
| --- | --- | --- | --- |
| Baseline | `bench/results/baseline-66936b0/raw.jsonl.gz` | 27,995 | `40adcb5891868a836824439972cd4fa355bf2c570c8fc20f3455ccb0ab7570b6` |
| Post-fix | `bench/results/fixed-1273c4d/raw.jsonl.gz` | 28,274 | `43d7dd1bf5af26b0a99f1f8fac64597dae09e24ad727b32657e9de1de37229f9` |

Each file holds one JSON object per sample: `pass`, `row`, `sample`, `ns`,
`exit`, both digests, both byte counts. `gunzip -c … | head` is enough to verify.
Nothing in this document depends on GitHub Actions artifacts.

## 4. Environment

| Item | Value |
| --- | --- |
| Harness revision at measurement time | recorded per run in `meta.json` (`harness_commit`) |
| Rust | rustc 1.99.1, cargo 1.99.1; MSRV toolchain 1.75.0 also installed |
| OS / kernel | Linux 7.0.0-43-generic, x86_64 |
| CPU | AMD Ryzen 5 2600X Six-Core (12 threads) |
| Memory | 15.5 GiB |
| Python | 3.12.3 |
| Profile | release, default features |

Measurements are from a single host. No cross-platform or ARM claim is made.

## 5. Fixtures

Every benchmark input file is hashed recursively with repository-relative keys:
**70 files, 8,764,087 bytes**, and the same aggregate input digest
`2b6d4f958b34b4706f949df4e09634a832009d3531bf19659085bd5911ca8807` for both
runs (computed as SHA-256 over `path\0sha256\n` in sorted path order). The full
per-file manifest lives in `meta.json` under `inputs`, with `bytes` and `sha256`
for each entry.

Coverage of that manifest: all 30 checked-in corpus files (29 fixtures plus
`corpus/manifest.txt`), all 8 preprocessing include files, all 24 generated
multi-file units, and the eight generated single-file workloads.

| Generated input | Bytes | SHA-256 | Purpose |
| --- | --- | --- | --- |
| `many_symbols.lsl` | 113,822 | `70a8f3886edc24d8…` | 2,000 declarations × 2,000 reads, no diagnostics |
| `call_graph.lsl` | 66,431 | `6555db41e45c1e80…` | 800 reachable + 700 unreachable functions, 400-wide fan-out |
| `nested_scopes.lsl` | 36,949 | `d2b6eb3060d9b9b8…` | nesting depth 40, 5 locals per level |
| `preprocess_root.lsl` | 16,148 | `6809da09c436623e…` | 400 generated helpers over 8 includes |
| `near_limit.lsl` | **8,026,842** | `84e03b23704f131c1f20e57fff75790f88de37b7c33d7f6c69015cfbdc3a3c76` | 120,000 statements, 91 % of the 8 MiB root limit |
| `malformed.lsl` | 14,932 | `73c2886339f2205c…` | 500 malformed initializers |
| `diagnostic_flood.lsl` | 448,932 | `1d478c97c7d80b43…` | 10,000 undeclared references at the cap |
| `stdin.lsl` | 114 | `4a237b239d17422a…` | stdin preparation path |

**Near-limit fixture.** The previous 25,000-statement fixture was only ~1.6 MiB,
which did not exercise the documented 8 MiB root-source limit. It is now 120,000
statements: 8,026,842 bytes, 91 % of `MAX_ROOT_SOURCE_BYTES` (8,388,608) and
therefore under the limit, syntactically valid, and diagnostic-free. Because each
invocation now costs ~0.5 s, that row uses **5** samples per pass instead of 30–50
(two passes, both binaries, 10 samples per binary; the reduction is recorded as
a note in `meta.json` and in the summary table).

## 6. Results

Base binary versus post-fix binary, identical fixtures, same host, two passes
combined. `n` is the combined sample count; `p95` is nearest-rank over the
combined samples.

| Row | n | Base median | Base p95 | Head median | Head p95 | Δ median |
| --- | --- | --- | --- | --- | --- | --- |
| `startup` (`--version`) | 100 | 1.04 | 1.76 | 1.16 | 1.62 | +11.6 % |
| `corpus_dir/default` | 80 | 11.75 | 13.18 | 9.79 | 13.22 | −16.7 % |
| `corpus_dir/ast` | 80 | 10.04 | 12.71 | 8.92 | 13.20 | −11.1 % |
| `corpus_dir/legacy` | 80 | 6.92 | 7.64 | 7.03 | 8.06 | +1.7 % |
| `corpus_dir/compare` | 80 | 13.46 | 14.78 | 9.83 | 15.32 | −27.0 % |
| `many_symbols/default` | 60 | 47.34 | 61.91 | 28.69 | 35.85 | **−39.4 %** |
| `many_symbols/ast` | 60 | 46.98 | 57.02 | 29.59 | 35.38 | **−37.0 %** |
| `many_symbols/legacy` | 60 | 11.53 | 17.70 | 10.47 | 17.74 | −9.2 % |
| `call_graph/default` | 60 | 21.82 | 29.65 | 21.44 | 29.62 | −1.7 % |
| `call_graph/legacy` | 60 | 12.08 | 16.02 | 10.49 | 15.77 | −13.2 % |
| `near_limit/default` | 10 | 483.22 | 502.98 | 477.76 | 505.60 | −1.1 % |
| `near_limit/legacy` | 10 | 234.76 | 257.42 | 234.70 | 243.23 | −0.0 % |
| `diagnostic_flood/default` | 40 | 167.84 | 183.39 | 83.20 | 99.67 | **−50.4 %** |
| `diagnostic_flood/legacy` | 40 | 50.62 | 55.18 | 49.74 | 58.97 | −1.7 % |
| `preprocess/default` | 60 | 19.27 | 22.56 | 13.11 | 21.61 | −32.0 % |
| `preprocess/legacy` | 60 | 10.61 | 18.59 | 10.89 | 17.73 | +2.6 % |
| `malformed/default` | 60 | 7.21 | 8.38 | 5.57 | 7.67 | −22.8 % |
| `malformed/legacy` | 60 | 6.33 | 7.44 | 5.08 | 6.95 | −19.7 % |
| `multi_file/default` | 60 | 6.50 | 8.25 | 7.42 | 8.08 | +14.1 % |
| `multi_file/legacy` | 60 | 4.37 | 5.41 | 4.34 | 5.09 | −0.7 % |
| `nested_scopes/default` | 60 | 7.42 | 8.16 | 6.11 | 7.78 | −17.7 % |
| `nested_scopes/legacy` | 60 | 4.73 | 5.44 | 3.50 | 5.31 | −26.0 % |
| `stdin/default` | 60 | 3.76 | 4.32 | 2.98 | 4.70 | −20.7 % |

**Reading the noise.** The Legacy rows cannot change between these two binaries,
yet they move by up to 26 % (`nested_scopes/legacy`) and `malformed/legacy` by
19.7 %, because each binary was measured in its own sequential session. That is
the empirical noise envelope for sub-10 ms rows. Only the two rows with effects
far outside it — `diagnostic_flood/default` (−50.4 %) and `many_symbols/default`
(−39.4 %) — are claimed as improvements, and both replicate across three
independent sessions. Rows whose movement stays inside the envelope are reported
as measurements, not as gains.

Peak RSS (post-fix run, median of three untimed samples per row): `--version`
≈2.3 MB; corpus directory ≈4.2 MB; `multi_file` ≈7 MB; `preprocess` ≈8 MB;
`call_graph` ≈12 MB; `many_symbols` ≈16 MB; `diagnostic_flood` ≈39 MB;
`near_limit` ≈67 MB. Corpus-scale runs stay in the low single-digit megabytes.

Two-pass variation (same binary, pass 1 vs pass 2): rows below 5 ms varied by
20–60 %, rows between 5 and 20 ms by roughly 5–20 %, and the expensive rows
(`many_symbols`, `near_limit`, `diagnostic_flood`) by 1–9 %. No absolute
regression threshold is encoded anywhere, and timing is never a CI criterion.

## 7. Profile-guided fixes

No sampling profiler was available: `perf` is blocked by
`kernel.perf_event_paranoid=4` on the measurement host and `valgrind` is not
installed. Attribution therefore used stage-level timing (lex / parse / analyze /
render) through a temporary local harness that was deleted afterwards, plus
scaling experiments and A/B runs of the two verified binaries on identical
fixtures.

### 7.1 Prepared-source validation hoisted out of the per-diagnostic path

`RenderedDiagnostic::from_prepared` compared the entire prepared buffer against
the source map's copy on **every** diagnostic. The check is required, but calling
it per diagnostic made rendering scale with `diagnostics × prepared_bytes`.
Validation now runs once per unit; the public `from_prepared` still validates and
an internal constructor is used only after that check.

Measured with the two verified binaries, median of 7 CLI runs each, identical
fixtures:

| Flood fixture | Bytes | Baseline | Post-fix | Gain |
| --- | --- | --- | --- | --- |
| 1,000 findings | 35,920 | 12.53 ms | 12.82 ms | −2.3 % (below noise) |
| 2,500 findings | 91,420 | 23.66 ms | 23.83 ms | −0.7 % (below noise) |
| 5,000 findings | 183,920 | 62.53 ms | 43.58 ms | 30.3 % |
| 10,000 findings | 368,920 | 152.94 ms | 76.95 ms | 49.7 % |

The gain grows with the prepared buffer exactly as the `diagnostics × bytes`
mechanism predicts, and is zero when the buffer is small — which is why the
effect is invisible on small inputs and only shows on diagnostic-heavy ones. The
harness row `diagnostic_flood/default` (−50.4 %) agrees.

### 7.2 Name-indexed scope lookup

`ScopeTree::lookup_value_at` walked the scope chain and linearly scanned every
symbol of the current scope per identifier use — `O(D × U)` within one scope.
Each scope now carries a private index (`spelling → symbol indices` ordered by
declaration start); resolution is one hash lookup plus a binary search. The index
is derived from `scopes`, built and sorted deterministically, and never iterated
for output.

Measured with the same two verified binaries, median of 7 CLI runs, generated
sources where every declaration is read (no diagnostics):

| Declarations / reads | Bytes | Baseline | Post-fix | Gain |
| --- | --- | --- | --- | --- |
| 500 / 500 | 23,810 | 12.74 ms | 10.61 ms | 16.7 % |
| 1,000 / 1,000 | 47,810 | 27.05 ms | 14.13 ms | 47.7 % |
| 2,000 / 2,000 | 97,810 | 44.11 ms | 23.70 ms | 46.3 % |
| 4,000 / 4,000 | 197,810 | 136.43 ms | 52.74 ms | 61.3 % |

These are end-to-end CLI numbers, so they include parsing and process startup;
the analysis-only effect is larger. Growth is now close to linear instead of
quadratic. The harness row `many_symbols/default` improves 47.34 → 28.69 ms
(−39.4 %).

Equivalence is proven by a test that compares the indexed resolution against a
reference implementation of the old linear scan for every scope, every binding
spelling and every source offset across five shadowing-heavy fixtures.

## 8. Resource limits

Every limit is mapped to its constant, value, enforcement point, scope,
behaviour on exceed and boundary test in
[`preprocessor-ast.md` §12](preprocessor-ast.md). Summary:

| Area | Limits | Boundary coverage |
| --- | --- | --- |
| Source manager | root 8 MiB, included 4 MiB, 256 files/unit, 65,536 units/invocation | root size, included size (new), file count, unit capacity (new) at N and N+1 |
| Preprocessor | include depth 64, macro depth 64, conditional depth 64, expression depth 128, 8 MiB / 1,000,000 expanded bytes/tokens, 1,000 diagnostics | `PRE006`, `PRE019`–`PRE022`, `PRE034`/`PRE035`, `PRE013` cases |
| Diagnostics | 10,000 rendered per unit (after rule suppression), 10,000 per invocation | `cap_suppression` suite, `global_cli_diagnostic_cap_truncates_all_later_files_and_keeps_one_pre039` |
| AST parser | recursion depth 256, 16 MiB worker stack | `AST001` cases in `tests/ast_parser.rs` and `tests/ast_firestorm.rs`, `AST002` worker-failure test |
| AST semantics | CFG 4,096/8,192/128/16,384, constant 128/64, type depth 256, call graph 4,096/8,192/16,384 | Phase 6C/6D suites, call-graph budget tests |
| Legacy semantics | CFG 16,384/4,096/8,192/128, constant 128/64 | Legacy semantic tests |
| Compare | 100,000 fingerprints/engine, 64 details/rule/direction | `compare_budget_enforcement`, detail-sample test |

Changes made in this phase: `MAX_INCLUDE_DEPTH` and `MAX_ORIGIN_DEPTH` are new
names for former bare `64` literals with unchanged values and semantics; missing
N/N+1 tests were added for included-file size, origin-chain depth and invocation
unit capacity.

`MAX_TYPE_DEPTH` is documented as defence in depth: the parser's 256-deep
recursion guard rejects deeper input first (verified: 250 nested parentheses parse
cleanly, 260 produce `AST001`), so the type walker cannot be reached with a
deeper source-level expression today.

Hostile-input checks: bounded reads take at most `limit + 1` bytes; capacity and
unit checks run before the corresponding allocation; call-graph counters are
checked before every node, edge and work step; the compare budget is counted
before fingerprints are materialised; a suppressed rule cannot bypass a
diagnostic cap (the cap is applied after suppression).

## 9. Finding classification

| Finding | Classification | Action |
| --- | --- | --- |
| §12 described included-file size as a future feature | Documentation drift | Fixed |
| §12's "total diagnostics — cap in Phase 3" row was obsolete | Documentation drift | Fixed: two real caps documented |
| Call-graph, type-depth and constant-evaluation limits absent from §12 | Documentation drift | Fixed |
| Include-depth and origin-chain limits were bare `64` literals | Maintainability gap | Named constants, unchanged values |
| No boundary test for included-file size, exact origin-chain depth, invocation unit capacity | Test-coverage gap | N/N+1 tests added |
| Per-diagnostic whole-buffer validation in rendering | Measured bottleneck | Fixed (−30…−50 % on large floods) |
| Linear scope symbol scan | Measured bottleneck | Fixed (16–61 % end-to-end at scale) |
| Benchmark metadata mixed harness revision with binary revision | Evidence defect | Fixed: separate fields, verified SHA-256 |
| Include/multi-file inputs were not hashed | Evidence defect | Fixed: 70-file recursive manifest |
| Near-limit fixture was 1.6 MiB, not near the 8 MiB limit | Evidence defect | Fixed: 8,026,842 bytes, 91 % of the limit |
| `MAX_TYPE_DEPTH` unreachable through the parser | Known limitation (defensive) | Documented |
| Duplicated CFG constant sets in AST and Legacy engines | Known limitation | Documented, follow-up #110 |
| PRE diagnostics use `DiagnosticKind::Syntax` | Known limitation / policy question | Follow-up #108 |
| Related locations absent from JSON | Known limitation | Follow-up #108 |
| `#pragma once` not proven through the Firestorm inventory layer | Known limitation | Follow-up #109 |
| `--help` omits `--preprocess`, `--dialect`, `-I` | Documentation drift (discoverability) | Follow-up #111 |
| Iterative traversals, call-graph bounds, compare budget pre-check, multi-unit isolation, diagnostic caps | No issue found | Evidence in §8 and the Phase 7 record |

Pre-existing process note: stable `.github/workflows/ci.yml` differs from frozen
`main` by one line (`pull_request: branches: [main]`) introduced by Phase 0
commit `d56b392`. It is recorded here for accuracy and was **not** modified by
this phase; `release.yml` is unchanged.

## 10. Limitations and accepted deviations

- Single host, single OS/architecture; no ARM, macOS or Windows measurement.
- Peak RSS is only available on Linux via `/proc`; other hosts report
  `unavailable`.
- Sequential rather than interleaved A/B sessions bound the noise at roughly
  ±26 % for sub-10 ms rows, measured with the Legacy negative control. Only
  effects well outside that envelope are claimed.
- The checked-in corpus is small (the whole directory runs in ~7–12 ms), so real
  corpus rows are dominated by process startup; synthetic workloads carry the
  performance evidence.
- No sampling profiler was available on the host; attribution used stage timing,
  scaling experiments and A/B binaries rather than flame graphs.
- `many_symbols`, `call_graph`, `near_limit` and `diagnostic_flood` are synthetic
  worst cases, not representative scripts; improvements there are not claims
  about typical user files.
- The AST engine still does more total work than Legacy by design (identity
  preparation, typed parsing, scope model, source-map rendering). No attempt was
  made to close that gap, and none of these measurements justifies removing
  Legacy.
- Exit codes and output digests were stable across all samples and both passes
  for every row in both runs.

## 11. Follow-up Issues

- #108 — preprocessor `DiagnosticKind` and JSON related locations (policy).
- #109 — prove `#pragma once` through the Firestorm inventory include layer.
- #110 — consolidate duplicated CFG and constant-evaluation limit constants.
- #111 — `--help` omits the implemented `--preprocess`, `--dialect` and `-I`.

## 12. Roadmap

- [x] Phase 0 — experimental branch, CI, architecture contracts.
- [x] Phase 1 — Source Manager, Source Map, identity preprocessor.
- [x] Phase 2 — preprocessor core.
- [x] Phase 3 — includes, advanced macros, Firestorm built-ins and CLI integration.
- [x] Phase 4 — typed AST parser and recovery.
- [x] Phase 5 — Firestorm AST extensions.
- [x] Phase 6A–6F — AST implementations for SEM001–SEM023.
- [x] Phase 6G — engine selection, bounded Compare, CLI wiring.
- [x] Phase 6H — AST default on the experimental integration branch; Legacy retained and stable `main` left unchanged.
- [x] Phase 7 — AST-native SEM024–SEM026, documentation and regression tests.
- [x] Phase 8 (this Issue) — reproducible benchmark facility and verified baseline, resource-limit reconciliation, two profile-guided fixes, evidence package and follow-ups #108–#111.
- [ ] Separate decision on stable `main` / v1.0.0 migration.
- [ ] Separate decision on future Legacy retirement, if justified.
