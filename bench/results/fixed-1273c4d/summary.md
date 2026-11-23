# neolint CLI benchmark — Phase 8 baseline

- harness revision: `0ea4a604d309aa65b83a0793ee7e8ffa9e2de8e6` (tree `33f97eee79f211b1c7cb06a0d977e41b8eb1ce46`), branch `exp-preprocessor-ast-phase-08-stabilization`
- measured binary: built from `1273c4d325428c9fa95a49c9122bfc99ae5d9e34` (tree `73d77147843f2b6945ddb25bb0cb12b7b9db8cd4`), sha256 `227352d90b1ed1233046e6e7073726c43a354352e94c1e3ac06cc7509843babc`, verified against the expected SHA-256
- benchmark inputs: 70 files, 8764087 bytes (per-file hashes in `meta.json`)
- rustc `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, cargo `cargo 1.99.1 (5f94df478 2026-08-27)`
- host: Linux-7.0.0-43-generic-x86_64-with-glibc2.39 / x86_64 / AMD Ryzen 5 2600X Six-Core Processor / 12 threads
- binary path: `/home/eloraam/apps/neolint/target/wt-fixed/target/release/neolint` (1675040 bytes)
- warm-up: 3 iterations per row; two independent passes
- RSS: peak VmHWM polled from /proc, 3 untimed samples per row

All figures are **end-to-end CLI** measurements and include process startup.
Plain `--engine legacy` is a direct no-preprocess path; AST always runs an
identity-prepared unit; `--engine compare` runs both analyzers. Ratios
between those modes are not parser-only comparisons.

| Row | Pass | Samples | Median ms | p95 ms | Min ms | Max ms | Exit | RSS KB | Digests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `startup` | 1 | 50 | 1.473 | 1.636 | 0.841 | 1.648 | 0 | 2404 | stable |
| `corpus_small/default` | 1 | 50 | 3.680 | 4.584 | 2.464 | 4.714 | 0 | 3796 | stable |
| `corpus_small/ast` | 1 | 50 | 2.862 | 4.524 | 2.283 | 4.926 | 0 | 3864 | stable |
| `corpus_small/legacy` | 1 | 50 | 3.583 | 3.955 | 2.609 | 4.129 | 0 | 3452 | stable |
| `corpus_small/compare` | 1 | 50 | 4.118 | 4.325 | 3.906 | 4.463 | 0 | 3888 | stable |
| `corpus_medium/default` | 1 | 50 | 4.227 | 4.715 | 2.449 | 4.964 | 0 | 3888 | stable |
| `corpus_medium/ast` | 1 | 50 | 3.339 | 5.792 | 2.287 | 7.004 | 0 | 3992 | stable |
| `corpus_medium/legacy` | 1 | 50 | 2.319 | 4.391 | 1.994 | 4.671 | 0 | 3456 | stable |
| `corpus_medium/compare` | 1 | 50 | 3.535 | 5.034 | 2.475 | 5.111 | 0 | 4048 | stable |
| `corpus_large/default` | 1 | 30 | 2.694 | 3.237 | 2.369 | 3.554 | 0 | 3984 | stable |
| `corpus_large/ast` | 1 | 30 | 2.818 | 4.366 | 2.369 | 4.554 | 0 | 3984 | stable |
| `corpus_large/legacy` | 1 | 30 | 2.373 | 4.311 | 2.100 | 4.457 | 0 | 3576 | stable |
| `corpus_large/compare` | 1 | 30 | 3.171 | 5.085 | 2.574 | 5.120 | 0 | 3980 | stable |
| `corpus_invalid/default` | 1 | 50 | 2.653 | 4.473 | 2.234 | 4.648 | 1 | 3832 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.254 | 4.659 | 2.324 | 5.069 | 1 | 3852 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 3.495 | 4.368 | 2.043 | 4.555 | 1 | 3632 | stable |
| `corpus_invalid/compare` | 1 | 50 | 4.054 | 4.817 | 2.401 | 5.223 | 1 | 3956 | stable |
| `corpus_dir/default` | 1 | 40 | 9.181 | 13.238 | 7.798 | 15.781 | 1 | 4296 | stable |
| `corpus_dir/ast` | 1 | 40 | 7.795 | 9.792 | 6.604 | 12.012 | 1 | 4284 | stable |
| `corpus_dir/legacy` | 1 | 40 | 5.967 | 8.092 | 4.072 | 8.251 | 1 | 4044 | stable |
| `corpus_dir/compare` | 1 | 40 | 9.257 | 14.500 | 8.441 | 15.089 | 1 | 4292 | stable |
| `many_symbols/default` | 1 | 30 | 29.236 | 35.813 | 19.426 | 36.152 | 0 | 16280 | stable |
| `many_symbols/ast` | 1 | 30 | 29.428 | 36.499 | 19.326 | 36.679 | 0 | 16256 | stable |
| `many_symbols/legacy` | 1 | 30 | 10.121 | 18.438 | 9.451 | 19.663 | 0 | 7576 | stable |
| `call_graph/default` | 1 | 30 | 16.923 | 26.324 | 15.609 | 26.816 | 0 | 11788 | stable |
| `call_graph/legacy` | 1 | 30 | 9.776 | 13.726 | 8.185 | 14.860 | 0 | 6524 | stable |
| `nested_scopes/default` | 1 | 30 | 4.805 | 7.615 | 4.615 | 9.207 | 0 | 4868 | stable |
| `nested_scopes/legacy` | 1 | 30 | 3.197 | 4.743 | 3.108 | 4.810 | 0 | 3612 | stable |
| `malformed/default` | 1 | 30 | 4.627 | 8.341 | 4.056 | 9.314 | 1 | 4396 | stable |
| `malformed/legacy` | 1 | 30 | 4.581 | 7.027 | 3.576 | 7.240 | 1 | 4036 | stable |
| `multi_file/default` | 1 | 30 | 5.928 | 8.181 | 4.617 | 8.181 | 0 | 3992 | stable |
| `multi_file/legacy` | 1 | 30 | 4.662 | 5.274 | 2.484 | 5.688 | 0 | 3660 | stable |
| `near_limit/default` | 1 | 5 | 478.131 | 505.600 | 464.042 | 505.600 | 0 | 309756 | stable |
| `near_limit/legacy` | 1 | 5 | 235.212 | 242.444 | 231.647 | 242.444 | 0 | 85252 | stable |
| `diagnostic_flood/default` | 1 | 20 | 80.228 | 97.749 | 75.578 | 99.261 | 0 | 38520 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 49.825 | 54.300 | 43.715 | 55.023 | 0 | 16576 | stable |
| `preprocess/default` | 1 | 30 | 13.175 | 17.371 | 11.876 | 21.648 | 0 | 8284 | stable |
| `preprocess/legacy` | 1 | 30 | 10.715 | 18.626 | 9.819 | 19.092 | 0 | 7464 | stable |
| `stdin/default` | 1 | 30 | 2.431 | 4.475 | 2.149 | 4.936 | 0 | 3844 | stable |
| `stdin/ast` | 1 | 30 | 2.472 | 4.420 | 2.162 | 4.488 | 0 | 3868 | stable |
| `stdin/legacy` | 1 | 30 | 2.502 | 2.948 | 1.975 | 4.588 | 0 | 3476 | stable |
| `startup` | 2 | 50 | 1.058 | 1.299 | 0.822 | 2.219 | 0 | 2184 | stable |
| `corpus_small/default` | 2 | 50 | 2.887 | 4.259 | 2.379 | 4.464 | 0 | 3828 | stable |
| `corpus_small/ast` | 2 | 50 | 3.798 | 4.608 | 2.193 | 5.620 | 0 | 3760 | stable |
| `corpus_small/legacy` | 2 | 50 | 3.655 | 4.416 | 2.468 | 4.592 | 0 | 3432 | stable |
| `corpus_small/compare` | 2 | 50 | 2.959 | 4.292 | 2.319 | 4.322 | 0 | 4056 | stable |
| `corpus_medium/default` | 2 | 50 | 2.968 | 4.336 | 2.543 | 4.507 | 0 | 3928 | stable |
| `corpus_medium/ast` | 2 | 50 | 4.016 | 4.628 | 2.503 | 6.675 | 0 | 3928 | stable |
| `corpus_medium/legacy` | 2 | 50 | 3.420 | 4.475 | 2.069 | 4.631 | 0 | 3424 | stable |
| `corpus_medium/compare` | 2 | 50 | 4.310 | 4.931 | 2.598 | 5.046 | 0 | 3896 | stable |
| `corpus_large/default` | 2 | 30 | 3.599 | 5.517 | 2.545 | 6.073 | 0 | 3960 | stable |
| `corpus_large/ast` | 2 | 30 | 3.071 | 5.358 | 2.586 | 5.775 | 0 | 4068 | stable |
| `corpus_large/legacy` | 2 | 30 | 3.563 | 4.488 | 2.251 | 4.636 | 0 | 3528 | stable |
| `corpus_large/compare` | 2 | 30 | 4.731 | 5.196 | 2.746 | 6.720 | 0 | 3984 | stable |
| `corpus_invalid/default` | 2 | 50 | 4.016 | 4.808 | 2.576 | 6.290 | 1 | 3924 | stable |
| `corpus_invalid/ast` | 2 | 50 | 2.618 | 4.127 | 2.200 | 4.562 | 1 | 3984 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 2.122 | 3.688 | 1.982 | 3.776 | 1 | 3512 | stable |
| `corpus_invalid/compare` | 2 | 50 | 4.189 | 5.169 | 2.509 | 5.638 | 1 | 3816 | stable |
| `corpus_dir/default` | 2 | 40 | 10.237 | 12.850 | 7.727 | 13.255 | 1 | 4300 | stable |
| `corpus_dir/ast` | 2 | 40 | 12.024 | 14.028 | 8.275 | 14.354 | 1 | 4300 | stable |
| `corpus_dir/legacy` | 2 | 40 | 7.121 | 7.520 | 4.054 | 8.652 | 1 | 4072 | stable |
| `corpus_dir/compare` | 2 | 40 | 13.529 | 15.810 | 8.771 | 16.197 | 1 | 4284 | stable |
| `many_symbols/default` | 2 | 30 | 28.396 | 36.014 | 19.605 | 36.351 | 0 | 16256 | stable |
| `many_symbols/ast` | 2 | 30 | 29.593 | 35.256 | 19.583 | 35.381 | 0 | 16220 | stable |
| `many_symbols/legacy` | 2 | 30 | 10.882 | 17.744 | 9.459 | 17.929 | 0 | 7608 | stable |
| `call_graph/default` | 2 | 30 | 24.870 | 30.971 | 18.762 | 32.723 | 0 | 11844 | stable |
| `call_graph/legacy` | 2 | 30 | 12.153 | 16.084 | 8.277 | 16.325 | 0 | 6556 | stable |
| `nested_scopes/default` | 2 | 30 | 6.904 | 7.889 | 4.429 | 8.637 | 0 | 4724 | stable |
| `nested_scopes/legacy` | 2 | 30 | 4.596 | 5.867 | 2.502 | 6.296 | 0 | 3604 | stable |
| `malformed/default` | 2 | 30 | 6.811 | 7.237 | 4.914 | 7.546 | 1 | 4424 | stable |
| `malformed/legacy` | 2 | 30 | 6.292 | 6.596 | 4.382 | 8.817 | 1 | 4016 | stable |
| `multi_file/default` | 2 | 30 | 7.449 | 7.977 | 5.732 | 9.343 | 0 | 3980 | stable |
| `multi_file/legacy` | 2 | 30 | 4.295 | 4.679 | 2.851 | 4.813 | 0 | 3588 | stable |
| `near_limit/default` | 2 | 5 | 476.574 | 481.755 | 468.141 | 481.755 | 0 | 309752 | stable |
| `near_limit/legacy` | 2 | 5 | 225.691 | 243.231 | 219.401 | 243.231 | 0 | 85268 | stable |
| `diagnostic_flood/default` | 2 | 20 | 88.838 | 103.272 | 77.744 | 104.327 | 0 | 38488 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 49.291 | 70.322 | 44.039 | 77.999 | 0 | 16624 | stable |
| `preprocess/default` | 2 | 30 | 13.051 | 21.665 | 12.053 | 21.688 | 0 | 8260 | stable |
| `preprocess/legacy` | 2 | 30 | 10.952 | 16.202 | 9.718 | 17.733 | 0 | 7500 | stable |
| `stdin/default` | 2 | 30 | 3.722 | 4.737 | 2.411 | 4.756 | 0 | 3892 | stable |
| `stdin/ast` | 2 | 30 | 3.997 | 4.682 | 2.482 | 4.686 | 0 | 3800 | stable |
| `stdin/legacy` | 2 | 30 | 3.505 | 3.690 | 3.106 | 3.703 | 0 | 3480 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 16.923 | 24.870 | 47.0 |
| `call_graph/legacy` | 9.776 | 12.153 | 24.3 |
| `corpus_dir/ast` | 7.795 | 12.024 | 54.2 |
| `corpus_dir/compare` | 9.257 | 13.529 | 46.1 |
| `corpus_dir/default` | 9.181 | 10.237 | 11.5 |
| `corpus_dir/legacy` | 5.967 | 7.121 | 19.3 |
| `corpus_invalid/ast` | 3.254 | 2.618 | 24.3 |
| `corpus_invalid/compare` | 4.054 | 4.189 | 3.3 |
| `corpus_invalid/default` | 2.653 | 4.016 | 51.4 |
| `corpus_invalid/legacy` | 3.495 | 2.122 | 64.7 |
| `corpus_large/ast` | 2.818 | 3.071 | 9.0 |
| `corpus_large/compare` | 3.171 | 4.731 | 49.2 |
| `corpus_large/default` | 2.694 | 3.599 | 33.6 |
| `corpus_large/legacy` | 2.373 | 3.563 | 50.1 |
| `corpus_medium/ast` | 3.339 | 4.016 | 20.3 |
| `corpus_medium/compare` | 3.535 | 4.310 | 21.9 |
| `corpus_medium/default` | 4.227 | 2.968 | 42.4 |
| `corpus_medium/legacy` | 2.319 | 3.420 | 47.5 |
| `corpus_small/ast` | 2.862 | 3.798 | 32.7 |
| `corpus_small/compare` | 4.118 | 2.959 | 39.2 |
| `corpus_small/default` | 3.680 | 2.887 | 27.4 |
| `corpus_small/legacy` | 3.583 | 3.655 | 2.0 |
| `diagnostic_flood/default` | 80.228 | 88.838 | 10.7 |
| `diagnostic_flood/legacy` | 49.825 | 49.291 | 1.1 |
| `malformed/default` | 4.627 | 6.811 | 47.2 |
| `malformed/legacy` | 4.581 | 6.292 | 37.4 |
| `many_symbols/ast` | 29.428 | 29.593 | 0.6 |
| `many_symbols/default` | 29.236 | 28.396 | 3.0 |
| `many_symbols/legacy` | 10.121 | 10.882 | 7.5 |
| `multi_file/default` | 5.928 | 7.449 | 25.7 |
| `multi_file/legacy` | 4.662 | 4.295 | 8.6 |
| `near_limit/default` | 478.131 | 476.574 | 0.3 |
| `near_limit/legacy` | 235.212 | 225.691 | 4.2 |
| `nested_scopes/default` | 4.805 | 6.904 | 43.7 |
| `nested_scopes/legacy` | 3.197 | 4.596 | 43.8 |
| `preprocess/default` | 13.175 | 13.051 | 1.0 |
| `preprocess/legacy` | 10.715 | 10.952 | 2.2 |
| `stdin/ast` | 2.472 | 3.997 | 61.7 |
| `stdin/default` | 2.431 | 3.722 | 53.1 |
| `stdin/legacy` | 2.502 | 3.505 | 40.1 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
