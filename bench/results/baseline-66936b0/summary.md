# neolint CLI benchmark — Phase 8 baseline

- commit `1273c4d325428c9fa95a49c9122bfc99ae5d9e34` (tree `73d77147843f2b6945ddb25bb0cb12b7b9db8cd4`), branch `exp-preprocessor-ast-phase-08-stabilization`
- rustc `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, cargo `cargo 1.99.1 (5f94df478 2026-08-27)`
- host: Linux-7.0.0-43-generic-x86_64-with-glibc2.39 / x86_64 / AMD Ryzen 5 2600X Six-Core Processor / 12 threads
- binary: `/home/eloraam/apps/neolint/target/base-wt/target/release/neolint` sha256 `83d9fce60666773fc73318a70a163b147c428eacd155eb89ece85486295f8ac8` (1659648 bytes)
- warm-up: 3 iterations per row; two independent passes
- RSS: peak VmHWM polled from /proc, 3 untimed samples per row

All figures are **end-to-end CLI** measurements and include process startup.
Plain `--engine legacy` is a direct no-preprocess path; AST always runs an
identity-prepared unit; `--engine compare` runs both analyzers. Ratios
between those modes are not parser-only comparisons.

| Row | Pass | Samples | Median ms | p95 ms | Min ms | Max ms | Exit | RSS KB | Digests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `startup` | 1 | 50 | 0.969 | 1.631 | 0.790 | 1.772 | 0 | 2340 | stable |
| `corpus_small/default` | 1 | 50 | 2.623 | 4.335 | 2.201 | 6.044 | 0 | 3828 | stable |
| `corpus_small/ast` | 1 | 50 | 2.366 | 4.346 | 2.103 | 4.709 | 0 | 3828 | stable |
| `corpus_small/legacy` | 1 | 50 | 3.188 | 3.962 | 1.901 | 4.072 | 0 | 3664 | stable |
| `corpus_small/compare` | 1 | 50 | 4.302 | 4.862 | 2.306 | 5.446 | 0 | 3860 | stable |
| `corpus_medium/default` | 1 | 50 | 4.244 | 4.702 | 2.490 | 5.083 | 0 | 3888 | stable |
| `corpus_medium/ast` | 1 | 50 | 4.110 | 4.585 | 2.844 | 4.968 | 0 | 3856 | stable |
| `corpus_medium/legacy` | 1 | 50 | 2.942 | 3.481 | 2.029 | 3.537 | 0 | 3588 | stable |
| `corpus_medium/compare` | 1 | 50 | 2.436 | 2.773 | 2.291 | 4.105 | 0 | 3924 | stable |
| `corpus_large/default` | 1 | 30 | 3.257 | 4.490 | 2.405 | 4.500 | 0 | 3996 | stable |
| `corpus_large/ast` | 1 | 30 | 2.985 | 4.471 | 2.599 | 4.482 | 0 | 3892 | stable |
| `corpus_large/legacy` | 1 | 30 | 3.868 | 4.117 | 3.727 | 4.130 | 0 | 3532 | stable |
| `corpus_large/compare` | 1 | 30 | 4.499 | 4.940 | 4.414 | 5.068 | 0 | 3936 | stable |
| `corpus_invalid/default` | 1 | 50 | 3.853 | 4.651 | 2.072 | 5.609 | 1 | 3880 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.955 | 4.283 | 2.072 | 4.922 | 1 | 3800 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 3.563 | 3.945 | 3.434 | 4.211 | 1 | 3584 | stable |
| `corpus_invalid/compare` | 1 | 50 | 3.946 | 4.440 | 3.762 | 4.664 | 1 | 3820 | stable |
| `corpus_dir/default` | 1 | 40 | 11.544 | 12.117 | 7.168 | 12.489 | 1 | 4240 | stable |
| `corpus_dir/ast` | 1 | 40 | 12.124 | 13.062 | 8.824 | 13.235 | 1 | 4204 | stable |
| `corpus_dir/legacy` | 1 | 40 | 5.075 | 7.351 | 4.670 | 7.859 | 1 | 4244 | stable |
| `corpus_dir/compare` | 1 | 40 | 13.920 | 15.455 | 8.506 | 15.966 | 1 | 4276 | stable |
| `many_symbols/default` | 1 | 30 | 47.024 | 59.198 | 38.129 | 61.727 | 0 | 15920 | stable |
| `many_symbols/ast` | 1 | 30 | 48.027 | 59.653 | 33.658 | 60.234 | 0 | 15920 | stable |
| `many_symbols/legacy` | 1 | 30 | 10.275 | 17.900 | 9.195 | 18.005 | 0 | 7676 | stable |
| `call_graph/default` | 1 | 30 | 19.780 | 29.427 | 16.103 | 29.625 | 0 | 11488 | stable |
| `call_graph/legacy` | 1 | 30 | 14.399 | 15.537 | 8.108 | 15.896 | 0 | 6544 | stable |
| `nested_scopes/default` | 1 | 30 | 7.094 | 8.664 | 5.145 | 9.967 | 0 | 4796 | stable |
| `nested_scopes/legacy` | 1 | 30 | 2.481 | 5.022 | 2.426 | 5.154 | 0 | 3776 | stable |
| `malformed/default` | 1 | 30 | 7.037 | 7.667 | 4.038 | 7.874 | 1 | 4432 | stable |
| `malformed/legacy` | 1 | 30 | 6.358 | 6.818 | 6.146 | 6.917 | 1 | 4148 | stable |
| `multi_file/default` | 1 | 30 | 6.359 | 7.961 | 4.346 | 8.461 | 0 | 3916 | stable |
| `multi_file/legacy` | 1 | 30 | 4.193 | 4.317 | 2.881 | 4.386 | 0 | 3664 | stable |
| `near_limit/default` | 1 | 8 | 106.658 | 110.784 | 99.899 | 110.784 | 0 | 66844 | stable |
| `near_limit/legacy` | 1 | 8 | 50.413 | 56.072 | 46.053 | 56.072 | 0 | 20652 | stable |
| `diagnostic_flood/default` | 1 | 20 | 168.064 | 175.463 | 153.741 | 178.277 | 0 | 38476 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 45.259 | 50.936 | 42.489 | 53.124 | 0 | 16780 | stable |
| `preprocess/default` | 1 | 30 | 13.148 | 21.589 | 11.807 | 21.653 | 0 | 8112 | stable |
| `preprocess/legacy` | 1 | 30 | 10.005 | 18.205 | 9.579 | 18.490 | 0 | 7564 | stable |
| `stdin/default` | 1 | 30 | 3.842 | 4.390 | 2.379 | 4.506 | 0 | 3824 | stable |
| `stdin/ast` | 1 | 30 | 2.274 | 4.173 | 2.026 | 4.499 | 0 | 3876 | stable |
| `stdin/legacy` | 1 | 30 | 3.372 | 3.979 | 1.844 | 4.229 | 0 | 3712 | stable |
| `startup` | 2 | 50 | 1.407 | 1.607 | 0.871 | 1.736 | 0 | 2368 | stable |
| `corpus_small/default` | 2 | 50 | 3.871 | 4.005 | 3.719 | 4.057 | 0 | 3800 | stable |
| `corpus_small/ast` | 2 | 50 | 2.885 | 4.343 | 2.183 | 4.420 | 0 | 3800 | stable |
| `corpus_small/legacy` | 2 | 50 | 3.549 | 3.862 | 1.948 | 4.007 | 0 | 3596 | stable |
| `corpus_small/compare` | 2 | 50 | 3.994 | 4.665 | 3.896 | 4.802 | 0 | 3828 | stable |
| `corpus_medium/default` | 2 | 50 | 3.901 | 4.422 | 2.368 | 4.507 | 0 | 3820 | stable |
| `corpus_medium/ast` | 2 | 50 | 4.045 | 4.922 | 2.373 | 5.116 | 0 | 3916 | stable |
| `corpus_medium/legacy` | 2 | 50 | 3.618 | 4.228 | 2.042 | 4.316 | 0 | 3576 | stable |
| `corpus_medium/compare` | 2 | 50 | 4.040 | 4.727 | 2.276 | 4.762 | 0 | 4068 | stable |
| `corpus_large/default` | 2 | 30 | 2.704 | 4.992 | 2.321 | 5.070 | 0 | 3904 | stable |
| `corpus_large/ast` | 2 | 30 | 4.021 | 5.246 | 2.605 | 5.291 | 0 | 3936 | stable |
| `corpus_large/legacy` | 2 | 30 | 2.546 | 4.603 | 2.106 | 4.697 | 0 | 3640 | stable |
| `corpus_large/compare` | 2 | 30 | 2.846 | 4.925 | 2.458 | 4.955 | 0 | 3968 | stable |
| `corpus_invalid/default` | 2 | 50 | 3.879 | 4.233 | 3.752 | 4.414 | 1 | 3804 | stable |
| `corpus_invalid/ast` | 2 | 50 | 3.862 | 4.212 | 3.728 | 4.293 | 1 | 3832 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 3.559 | 3.927 | 3.462 | 4.064 | 1 | 3564 | stable |
| `corpus_invalid/compare` | 2 | 50 | 3.939 | 4.493 | 2.182 | 5.177 | 1 | 3864 | stable |
| `corpus_dir/default` | 2 | 40 | 8.190 | 13.401 | 6.608 | 14.342 | 1 | 4240 | stable |
| `corpus_dir/ast` | 2 | 40 | 7.604 | 11.431 | 6.592 | 11.791 | 1 | 4232 | stable |
| `corpus_dir/legacy` | 2 | 40 | 7.067 | 8.338 | 3.830 | 8.595 | 1 | 4180 | stable |
| `corpus_dir/compare` | 2 | 40 | 14.455 | 15.645 | 10.384 | 16.137 | 1 | 4288 | stable |
| `many_symbols/default` | 2 | 30 | 55.300 | 59.298 | 34.454 | 63.427 | 0 | 15872 | stable |
| `many_symbols/ast` | 2 | 30 | 53.923 | 58.065 | 34.302 | 59.471 | 0 | 15916 | stable |
| `many_symbols/legacy` | 2 | 30 | 9.523 | 17.631 | 9.166 | 17.834 | 0 | 7660 | stable |
| `call_graph/default` | 2 | 30 | 28.789 | 29.381 | 22.162 | 29.490 | 0 | 11480 | stable |
| `call_graph/legacy` | 2 | 30 | 14.459 | 16.121 | 8.186 | 16.122 | 0 | 6540 | stable |
| `nested_scopes/default` | 2 | 30 | 7.019 | 7.775 | 4.670 | 7.925 | 0 | 4800 | stable |
| `nested_scopes/legacy` | 2 | 30 | 3.288 | 4.791 | 3.109 | 4.825 | 0 | 3792 | stable |
| `malformed/default` | 2 | 30 | 7.139 | 7.547 | 5.387 | 7.611 | 1 | 4404 | stable |
| `malformed/legacy` | 2 | 30 | 6.258 | 6.687 | 6.149 | 6.871 | 1 | 4184 | stable |
| `multi_file/default` | 2 | 30 | 7.273 | 7.643 | 5.570 | 7.997 | 0 | 3892 | stable |
| `multi_file/legacy` | 2 | 30 | 4.288 | 4.500 | 3.029 | 4.596 | 0 | 3704 | stable |
| `near_limit/default` | 2 | 8 | 108.008 | 117.287 | 99.805 | 117.287 | 0 | 66808 | stable |
| `near_limit/legacy` | 2 | 8 | 49.438 | 54.567 | 45.366 | 54.567 | 0 | 20652 | stable |
| `diagnostic_flood/default` | 2 | 20 | 166.108 | 178.683 | 156.610 | 182.594 | 0 | 38488 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 49.918 | 51.875 | 42.426 | 52.158 | 0 | 16632 | stable |
| `preprocess/default` | 2 | 30 | 14.275 | 22.247 | 11.838 | 23.685 | 0 | 8184 | stable |
| `preprocess/legacy` | 2 | 30 | 12.132 | 17.770 | 9.498 | 17.943 | 0 | 7588 | stable |
| `stdin/default` | 2 | 30 | 2.599 | 4.037 | 2.322 | 4.193 | 0 | 3796 | stable |
| `stdin/ast` | 2 | 30 | 2.604 | 3.017 | 2.465 | 3.830 | 0 | 3892 | stable |
| `stdin/legacy` | 2 | 30 | 3.563 | 4.081 | 3.411 | 4.126 | 0 | 3736 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 19.780 | 28.789 | 45.5 |
| `call_graph/legacy` | 14.399 | 14.459 | 0.4 |
| `corpus_dir/ast` | 12.124 | 7.604 | 59.4 |
| `corpus_dir/compare` | 13.920 | 14.455 | 3.8 |
| `corpus_dir/default` | 11.544 | 8.190 | 41.0 |
| `corpus_dir/legacy` | 5.075 | 7.067 | 39.2 |
| `corpus_invalid/ast` | 3.955 | 3.862 | 2.4 |
| `corpus_invalid/compare` | 3.946 | 3.939 | 0.2 |
| `corpus_invalid/default` | 3.853 | 3.879 | 0.7 |
| `corpus_invalid/legacy` | 3.563 | 3.559 | 0.1 |
| `corpus_large/ast` | 2.985 | 4.021 | 34.7 |
| `corpus_large/compare` | 4.499 | 2.846 | 58.1 |
| `corpus_large/default` | 3.257 | 2.704 | 20.4 |
| `corpus_large/legacy` | 3.868 | 2.546 | 51.9 |
| `corpus_medium/ast` | 4.110 | 4.045 | 1.6 |
| `corpus_medium/compare` | 2.436 | 4.040 | 65.8 |
| `corpus_medium/default` | 4.244 | 3.901 | 8.8 |
| `corpus_medium/legacy` | 2.942 | 3.618 | 23.0 |
| `corpus_small/ast` | 2.366 | 2.885 | 21.9 |
| `corpus_small/compare` | 4.302 | 3.994 | 7.7 |
| `corpus_small/default` | 2.623 | 3.871 | 47.6 |
| `corpus_small/legacy` | 3.188 | 3.549 | 11.3 |
| `diagnostic_flood/default` | 168.064 | 166.108 | 1.2 |
| `diagnostic_flood/legacy` | 45.259 | 49.918 | 10.3 |
| `malformed/default` | 7.037 | 7.139 | 1.5 |
| `malformed/legacy` | 6.358 | 6.258 | 1.6 |
| `many_symbols/ast` | 48.027 | 53.923 | 12.3 |
| `many_symbols/default` | 47.024 | 55.300 | 17.6 |
| `many_symbols/legacy` | 10.275 | 9.523 | 7.9 |
| `multi_file/default` | 6.359 | 7.273 | 14.4 |
| `multi_file/legacy` | 4.193 | 4.288 | 2.3 |
| `near_limit/default` | 106.658 | 108.008 | 1.3 |
| `near_limit/legacy` | 50.413 | 49.438 | 2.0 |
| `nested_scopes/default` | 7.094 | 7.019 | 1.1 |
| `nested_scopes/legacy` | 2.481 | 3.288 | 32.5 |
| `preprocess/default` | 13.148 | 14.275 | 8.6 |
| `preprocess/legacy` | 10.005 | 12.132 | 21.3 |
| `stdin/ast` | 2.274 | 2.604 | 14.5 |
| `stdin/default` | 3.842 | 2.599 | 47.9 |
| `stdin/legacy` | 3.372 | 3.563 | 5.7 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
