# neolint CLI benchmark — Phase 8 baseline

- commit `1273c4d325428c9fa95a49c9122bfc99ae5d9e34` (tree `73d77147843f2b6945ddb25bb0cb12b7b9db8cd4`), branch `exp-preprocessor-ast-phase-08-stabilization`
- rustc `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, cargo `cargo 1.99.1 (5f94df478 2026-08-27)`
- host: Linux-7.0.0-43-generic-x86_64-with-glibc2.39 / x86_64 / AMD Ryzen 5 2600X Six-Core Processor / 12 threads
- binary: `/home/eloraam/apps/neolint/target/release/neolint` sha256 `227352d90b1ed1233046e6e7073726c43a354352e94c1e3ac06cc7509843babc` (1675040 bytes)
- warm-up: 3 iterations per row; two independent passes
- RSS: peak VmHWM polled from /proc, 3 untimed samples per row

All figures are **end-to-end CLI** measurements and include process startup.
Plain `--engine legacy` is a direct no-preprocess path; AST always runs an
identity-prepared unit; `--engine compare` runs both analyzers. Ratios
between those modes are not parser-only comparisons.

| Row | Pass | Samples | Median ms | p95 ms | Min ms | Max ms | Exit | RSS KB | Digests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `startup` | 1 | 50 | 1.366 | 1.473 | 1.302 | 1.680 | 0 | 2272 | stable |
| `corpus_small/default` | 1 | 50 | 3.796 | 4.299 | 3.617 | 4.498 | 0 | 3900 | stable |
| `corpus_small/ast` | 1 | 50 | 3.815 | 4.195 | 2.520 | 4.364 | 0 | 3840 | stable |
| `corpus_small/legacy` | 1 | 50 | 3.584 | 3.945 | 3.419 | 4.227 | 0 | 3560 | stable |
| `corpus_small/compare` | 1 | 50 | 4.125 | 4.585 | 2.429 | 5.131 | 0 | 3864 | stable |
| `corpus_medium/default` | 1 | 50 | 2.401 | 4.641 | 2.233 | 5.102 | 0 | 3888 | stable |
| `corpus_medium/ast` | 1 | 50 | 2.247 | 4.267 | 2.159 | 4.559 | 0 | 4068 | stable |
| `corpus_medium/legacy` | 1 | 50 | 3.553 | 3.935 | 2.155 | 3.969 | 0 | 3584 | stable |
| `corpus_medium/compare` | 1 | 50 | 2.447 | 4.613 | 2.365 | 4.977 | 0 | 3864 | stable |
| `corpus_large/default` | 1 | 30 | 4.157 | 4.681 | 2.283 | 4.767 | 0 | 4000 | stable |
| `corpus_large/ast` | 1 | 30 | 4.028 | 4.568 | 2.485 | 4.650 | 0 | 4132 | stable |
| `corpus_large/legacy` | 1 | 30 | 3.752 | 4.158 | 3.037 | 4.273 | 0 | 3528 | stable |
| `corpus_large/compare` | 1 | 30 | 4.467 | 4.912 | 4.374 | 4.986 | 0 | 4088 | stable |
| `corpus_invalid/default` | 1 | 50 | 3.741 | 4.068 | 3.657 | 4.475 | 1 | 3848 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.941 | 4.234 | 3.757 | 4.444 | 1 | 3820 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 3.574 | 3.935 | 3.439 | 4.836 | 1 | 3600 | stable |
| `corpus_invalid/compare` | 1 | 50 | 4.013 | 4.343 | 3.819 | 4.521 | 1 | 3848 | stable |
| `corpus_dir/default` | 1 | 40 | 11.998 | 12.912 | 8.032 | 13.346 | 1 | 4228 | stable |
| `corpus_dir/ast` | 1 | 40 | 12.301 | 13.438 | 8.243 | 13.989 | 1 | 4308 | stable |
| `corpus_dir/legacy` | 1 | 40 | 6.897 | 7.092 | 3.671 | 7.131 | 1 | 4084 | stable |
| `corpus_dir/compare` | 1 | 40 | 13.332 | 15.405 | 8.157 | 16.462 | 1 | 4308 | stable |
| `many_symbols/default` | 1 | 30 | 27.407 | 36.060 | 18.902 | 36.732 | 0 | 16256 | stable |
| `many_symbols/ast` | 1 | 30 | 31.933 | 36.685 | 20.512 | 37.630 | 0 | 16272 | stable |
| `many_symbols/legacy` | 1 | 30 | 11.653 | 16.876 | 9.269 | 18.472 | 0 | 7616 | stable |
| `call_graph/default` | 1 | 30 | 27.922 | 29.722 | 17.187 | 29.998 | 0 | 11808 | stable |
| `call_graph/legacy` | 1 | 30 | 8.766 | 14.771 | 8.135 | 14.801 | 0 | 6584 | stable |
| `nested_scopes/default` | 1 | 30 | 7.001 | 7.644 | 5.729 | 7.679 | 0 | 4840 | stable |
| `nested_scopes/legacy` | 1 | 30 | 4.530 | 5.104 | 4.352 | 5.175 | 0 | 3636 | stable |
| `malformed/default` | 1 | 30 | 4.769 | 7.587 | 3.878 | 7.751 | 1 | 4396 | stable |
| `malformed/legacy` | 1 | 30 | 6.629 | 6.701 | 3.611 | 7.061 | 1 | 3944 | stable |
| `multi_file/default` | 1 | 30 | 7.197 | 7.589 | 5.765 | 7.599 | 0 | 3976 | stable |
| `multi_file/legacy` | 1 | 30 | 4.297 | 4.930 | 2.328 | 5.201 | 0 | 3488 | stable |
| `near_limit/default` | 1 | 8 | 104.487 | 118.003 | 96.414 | 118.003 | 0 | 66884 | stable |
| `near_limit/legacy` | 1 | 8 | 48.096 | 57.159 | 45.905 | 57.159 | 0 | 20620 | stable |
| `diagnostic_flood/default` | 1 | 20 | 83.378 | 94.618 | 70.947 | 108.202 | 0 | 38500 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 43.681 | 52.049 | 42.703 | 55.617 | 0 | 16620 | stable |
| `preprocess/default` | 1 | 30 | 15.909 | 22.619 | 12.534 | 23.266 | 0 | 8268 | stable |
| `preprocess/legacy` | 1 | 30 | 9.862 | 18.253 | 9.507 | 18.968 | 0 | 7524 | stable |
| `stdin/default` | 1 | 30 | 2.048 | 4.014 | 2.008 | 4.030 | 0 | 3888 | stable |
| `stdin/ast` | 1 | 30 | 3.662 | 4.109 | 3.544 | 4.154 | 0 | 3980 | stable |
| `stdin/legacy` | 1 | 30 | 3.373 | 3.588 | 2.204 | 3.892 | 0 | 3480 | stable |
| `startup` | 2 | 50 | 1.400 | 1.475 | 1.076 | 1.650 | 0 | 2136 | stable |
| `corpus_small/default` | 2 | 50 | 4.041 | 4.514 | 3.823 | 5.493 | 0 | 3828 | stable |
| `corpus_small/ast` | 2 | 50 | 3.935 | 4.241 | 3.781 | 4.663 | 0 | 3832 | stable |
| `corpus_small/legacy` | 2 | 50 | 3.669 | 3.988 | 3.439 | 4.043 | 0 | 3476 | stable |
| `corpus_small/compare` | 2 | 50 | 4.041 | 4.513 | 2.501 | 4.598 | 0 | 3852 | stable |
| `corpus_medium/default` | 2 | 50 | 4.075 | 4.461 | 3.005 | 4.546 | 0 | 3920 | stable |
| `corpus_medium/ast` | 2 | 50 | 4.047 | 4.500 | 2.942 | 4.586 | 0 | 3936 | stable |
| `corpus_medium/legacy` | 2 | 50 | 3.604 | 3.923 | 3.494 | 4.195 | 0 | 3460 | stable |
| `corpus_medium/compare` | 2 | 50 | 4.209 | 4.575 | 4.066 | 4.784 | 0 | 3904 | stable |
| `corpus_large/default` | 2 | 30 | 4.114 | 4.367 | 4.013 | 4.386 | 0 | 3952 | stable |
| `corpus_large/ast` | 2 | 30 | 2.506 | 3.063 | 2.318 | 4.752 | 0 | 4128 | stable |
| `corpus_large/legacy` | 2 | 30 | 2.142 | 3.056 | 2.025 | 4.234 | 0 | 3612 | stable |
| `corpus_large/compare` | 2 | 30 | 4.428 | 4.807 | 2.513 | 4.826 | 0 | 4116 | stable |
| `corpus_invalid/default` | 2 | 50 | 3.757 | 3.890 | 3.625 | 3.897 | 1 | 4000 | stable |
| `corpus_invalid/ast` | 2 | 50 | 2.406 | 3.513 | 2.131 | 4.422 | 1 | 3920 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 3.505 | 3.920 | 1.872 | 4.323 | 1 | 3600 | stable |
| `corpus_invalid/compare` | 2 | 50 | 3.852 | 4.027 | 3.051 | 4.273 | 1 | 3852 | stable |
| `corpus_dir/default` | 2 | 40 | 11.773 | 12.430 | 10.867 | 12.745 | 1 | 4212 | stable |
| `corpus_dir/ast` | 2 | 40 | 11.627 | 12.278 | 10.874 | 13.186 | 1 | 4232 | stable |
| `corpus_dir/legacy` | 2 | 40 | 6.832 | 7.193 | 3.677 | 7.352 | 1 | 4048 | stable |
| `corpus_dir/compare` | 2 | 40 | 14.400 | 15.311 | 9.091 | 15.466 | 1 | 4288 | stable |
| `many_symbols/default` | 2 | 30 | 32.560 | 35.046 | 24.860 | 38.033 | 0 | 16316 | stable |
| `many_symbols/ast` | 2 | 30 | 31.227 | 41.185 | 21.615 | 44.059 | 0 | 16292 | stable |
| `many_symbols/legacy` | 2 | 30 | 9.440 | 15.077 | 9.082 | 17.804 | 0 | 7680 | stable |
| `call_graph/default` | 2 | 30 | 18.391 | 23.411 | 15.551 | 26.083 | 0 | 11812 | stable |
| `call_graph/legacy` | 2 | 30 | 8.111 | 8.560 | 7.897 | 8.571 | 0 | 6592 | stable |
| `nested_scopes/default` | 2 | 30 | 7.039 | 7.185 | 5.206 | 7.204 | 0 | 4712 | stable |
| `nested_scopes/legacy` | 2 | 30 | 4.508 | 4.870 | 4.414 | 4.948 | 0 | 3608 | stable |
| `malformed/default` | 2 | 30 | 6.538 | 7.135 | 6.385 | 7.137 | 1 | 4408 | stable |
| `malformed/legacy` | 2 | 30 | 6.255 | 6.733 | 6.097 | 6.745 | 1 | 3976 | stable |
| `multi_file/default` | 2 | 30 | 7.511 | 7.934 | 7.048 | 8.022 | 0 | 3980 | stable |
| `multi_file/legacy` | 2 | 30 | 4.171 | 4.445 | 4.099 | 4.541 | 0 | 3488 | stable |
| `near_limit/default` | 2 | 8 | 103.834 | 111.074 | 101.602 | 111.074 | 0 | 66868 | stable |
| `near_limit/legacy` | 2 | 8 | 45.690 | 46.829 | 45.224 | 46.829 | 0 | 20560 | stable |
| `diagnostic_flood/default` | 2 | 20 | 90.076 | 96.160 | 77.418 | 97.401 | 0 | 38524 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 43.079 | 51.752 | 42.172 | 51.850 | 0 | 16656 | stable |
| `preprocess/default` | 2 | 30 | 12.658 | 13.731 | 11.871 | 18.831 | 0 | 8236 | stable |
| `preprocess/legacy` | 2 | 30 | 9.660 | 18.141 | 9.431 | 18.856 | 0 | 7524 | stable |
| `stdin/default` | 2 | 30 | 3.883 | 3.970 | 2.041 | 4.459 | 0 | 3888 | stable |
| `stdin/ast` | 2 | 30 | 3.644 | 3.764 | 2.053 | 4.263 | 0 | 3932 | stable |
| `stdin/legacy` | 2 | 30 | 3.427 | 3.505 | 3.300 | 3.726 | 0 | 3384 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 27.922 | 18.391 | 51.8 |
| `call_graph/legacy` | 8.766 | 8.111 | 8.1 |
| `corpus_dir/ast` | 12.301 | 11.627 | 5.8 |
| `corpus_dir/compare` | 13.332 | 14.400 | 8.0 |
| `corpus_dir/default` | 11.998 | 11.773 | 1.9 |
| `corpus_dir/legacy` | 6.897 | 6.832 | 0.9 |
| `corpus_invalid/ast` | 3.941 | 2.406 | 63.8 |
| `corpus_invalid/compare` | 4.013 | 3.852 | 4.2 |
| `corpus_invalid/default` | 3.741 | 3.757 | 0.4 |
| `corpus_invalid/legacy` | 3.574 | 3.505 | 1.9 |
| `corpus_large/ast` | 4.028 | 2.506 | 60.8 |
| `corpus_large/compare` | 4.467 | 4.428 | 0.9 |
| `corpus_large/default` | 4.157 | 4.114 | 1.0 |
| `corpus_large/legacy` | 3.752 | 2.142 | 75.1 |
| `corpus_medium/ast` | 2.247 | 4.047 | 80.1 |
| `corpus_medium/compare` | 2.447 | 4.209 | 72.0 |
| `corpus_medium/default` | 2.401 | 4.075 | 69.7 |
| `corpus_medium/legacy` | 3.553 | 3.604 | 1.4 |
| `corpus_small/ast` | 3.815 | 3.935 | 3.2 |
| `corpus_small/compare` | 4.125 | 4.041 | 2.1 |
| `corpus_small/default` | 3.796 | 4.041 | 6.5 |
| `corpus_small/legacy` | 3.584 | 3.669 | 2.4 |
| `diagnostic_flood/default` | 83.378 | 90.076 | 8.0 |
| `diagnostic_flood/legacy` | 43.681 | 43.079 | 1.4 |
| `malformed/default` | 4.769 | 6.538 | 37.1 |
| `malformed/legacy` | 6.629 | 6.255 | 6.0 |
| `many_symbols/ast` | 31.933 | 31.227 | 2.3 |
| `many_symbols/default` | 27.407 | 32.560 | 18.8 |
| `many_symbols/legacy` | 11.653 | 9.440 | 23.4 |
| `multi_file/default` | 7.197 | 7.511 | 4.4 |
| `multi_file/legacy` | 4.297 | 4.171 | 3.0 |
| `near_limit/default` | 104.487 | 103.834 | 0.6 |
| `near_limit/legacy` | 48.096 | 45.690 | 5.3 |
| `nested_scopes/default` | 7.001 | 7.039 | 0.6 |
| `nested_scopes/legacy` | 4.530 | 4.508 | 0.5 |
| `preprocess/default` | 15.909 | 12.658 | 25.7 |
| `preprocess/legacy` | 9.862 | 9.660 | 2.1 |
| `stdin/ast` | 3.662 | 3.644 | 0.5 |
| `stdin/default` | 2.048 | 3.883 | 89.6 |
| `stdin/legacy` | 3.373 | 3.427 | 1.6 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
