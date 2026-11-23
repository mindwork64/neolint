# CLI benchmark harness (Phase 8)

Dependency-free Python 3 harness for end-to-end `neolint` CLI measurements.
It answers the Phase 8 question "how does the AST-default CLI perform" without
adding a crate, a benchmark framework or a timing assertion to CI.

## Usage

```bash
# Build once, then measure two independent passes
python3 bench/bench.py --passes 2

# Measure a binary built from a specific commit and verify its digest
python3 bench/bench.py --passes 2 --skip-build \
  --binary target/wt-base/target/release/neolint \
  --binary-source-commit <commit> --expect-binary-sha256 <sha256>

# Fast smoke check of the harness itself (never a timing threshold)
python3 bench/bench.py --quick --self-test --skip-build
```

Results are written to `bench/results/<label>/{summary.md,meta.json,raw.jsonl,raw.jsonl.gz}`.
`summary.md`, `meta.json` and the compressed `raw.jsonl.gz` are committed;
uncompressed `raw.jsonl` is regenerated on demand and ignored by git.

`meta.json` separates the **harness** revision (`harness_commit`/`harness_tree`/
`harness_branch`) from the **measured binary** revision
(`binary_source_commit`/`binary_source_tree`) and records the binary SHA-256.
Passing `--expect-binary-sha256` makes the harness refuse to measure a binary that
does not match, and sets `binary_source_verified`. Every input file is hashed
recursively under `inputs` with repository-relative keys and byte sizes.

## What is measured

`time.perf_counter_ns()` around a real subprocess invocation of
`target/release/neolint`, including process startup. Three warm-up iterations
precede the measured samples. Every sample records the exit status and the
SHA-256 of stdout and stderr; a row is marked **unstable** if any of those
differ, which is a correctness/determinism failure and is never averaged away.

Peak RSS is the kernel's `VmHWM` for the child, polled untimed from
`/proc/<pid>/status` in three separate runs per row. `ru_maxrss` from
`os.wait4` was rejected: a forked child inherits the harness footprint before
`exec`, so every row reported the same ~26 MB. Where `/proc` is unavailable the
report says `unavailable` and no value is invented.

## Workloads

Checked-in corpus: one small, medium, large and invalid fixture, plus the whole
corpus directory. Deterministic synthetic fixtures are generated into
`target/bench-fixtures/` from explicit counts (no randomness, no network, no
corpus-manifest changes):

| Fixture | Parameters | Purpose |
| --- | --- | --- |
| `many_symbols` | 2,000 declarations, 2,000 uses | one-scope symbol lookup worst case |
| `call_graph` | 800 reachable + 700 unreachable functions, 400-wide fan-out | call-graph construction and reachability |
| `nested_scopes` | depth 40, 5 locals per level | scope chain walk |
| `preprocess` | 400 generated helpers over 8 includes | macro/include expansion |
| `near_limit` | 120,000 statements (7.65 MiB, 91 % of the 8 MiB root limit) | large valid input near a documented limit |
| `malformed` | 500 malformed initializers | recovery path |
| `diagnostic_flood` | 10,000 undeclared references | rendered-diagnostic cap |
| `multi_file` | 24 units in one directory | multi-unit aggregation |
| `stdin` | one small source | stdin preparation path |

Modes: default (AST), explicit `--engine ast`, explicit `--engine legacy`,
`--engine compare`, and `--preprocess` where the fixture needs preprocessing.

## Fair-comparison caveats

- Every figure is an **end-to-end CLI** measurement and includes process startup.
  A `--version` row is recorded so the startup share is visible.
- Plain `--engine legacy` is a direct no-preprocess path. AST always runs an
  identity-prepared unit. Their ratio is **not** a parser-only comparison.
- `--engine compare` runs both analyzers by design and is not a single-engine
  baseline.
- Sub-5 ms rows are dominated by scheduling noise. Observed two-pass variation is
  roughly 20-50 % there and around 1-10 % for the rows above ~20 ms. Compare only
  expensive workloads, or medians of several passes, when evaluating a change.
- Timing is never a pass/fail threshold in CI.
