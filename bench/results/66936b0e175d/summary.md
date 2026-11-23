# neolint CLI benchmark — Phase 8 baseline

- commit `66936b0e175d51cba8d83377061229869ef64e96` (tree `87d810459064f438fe61c0a632fa83642ea68971`), branch `exp-preprocessor-ast-phase-08-stabilization`
- rustc `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, cargo `cargo 1.99.1 (5f94df478 2026-08-27)`
- host: Linux-7.0.0-43-generic-x86_64-with-glibc2.39 / x86_64 / AMD Ryzen 5 2600X Six-Core Processor / 12 threads
- binary: `target/release/neolint` sha256 `83d9fce60666773fc73318a70a163b147c428eacd155eb89ece85486295f8ac8` (1659648 bytes)
- warm-up: 3 iterations per row; two independent passes
- RSS: peak VmHWM polled from /proc, 3 untimed samples per row

All figures are **end-to-end CLI** measurements and include process startup.
Plain `--engine legacy` is a direct no-preprocess path; AST always runs an
identity-prepared unit; `--engine compare` runs both analyzers. Ratios
between those modes are not parser-only comparisons.

| Row | Pass | Samples | Median ms | p95 ms | Min ms | Max ms | Exit | RSS KB | Digests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `startup` | 1 | 50 | 1.015 | 1.663 | 0.821 | 1.868 | 0 | 2040 | stable |
| `corpus_small/default` | 1 | 50 | 4.051 | 4.271 | 2.651 | 4.680 | 0 | 3836 | stable |
| `corpus_small/ast` | 1 | 50 | 4.128 | 4.647 | 2.413 | 4.892 | 0 | 3788 | stable |
| `corpus_small/legacy` | 1 | 50 | 2.505 | 4.838 | 2.133 | 5.153 | 0 | 3568 | stable |
| `corpus_small/compare` | 1 | 50 | 4.554 | 4.949 | 2.425 | 5.091 | 0 | 4032 | stable |
| `corpus_medium/default` | 1 | 50 | 4.647 | 5.204 | 2.731 | 5.264 | 0 | 3856 | stable |
| `corpus_medium/ast` | 1 | 50 | 3.153 | 5.109 | 2.523 | 5.246 | 0 | 3992 | stable |
| `corpus_medium/legacy` | 1 | 50 | 4.248 | 4.734 | 2.162 | 5.133 | 0 | 3492 | stable |
| `corpus_medium/compare` | 1 | 50 | 3.066 | 5.251 | 2.415 | 5.969 | 0 | 3904 | stable |
| `corpus_large/default` | 1 | 30 | 4.610 | 4.925 | 2.886 | 4.992 | 0 | 3956 | stable |
| `corpus_large/ast` | 1 | 30 | 4.783 | 5.530 | 2.875 | 6.111 | 0 | 3908 | stable |
| `corpus_large/legacy` | 1 | 30 | 2.826 | 4.252 | 2.215 | 6.505 | 0 | 3608 | stable |
| `corpus_large/compare` | 1 | 30 | 3.998 | 5.036 | 2.770 | 5.139 | 0 | 4104 | stable |
| `corpus_invalid/default` | 1 | 50 | 3.083 | 4.789 | 2.247 | 5.006 | 1 | 3836 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.975 | 4.979 | 2.392 | 5.103 | 1 | 3752 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 4.118 | 4.839 | 2.312 | 5.852 | 1 | 3516 | stable |
| `corpus_invalid/compare` | 1 | 50 | 4.508 | 5.076 | 2.703 | 5.286 | 1 | 3828 | stable |
| `corpus_dir/default` | 1 | 40 | 10.323 | 14.727 | 8.116 | 15.275 | 1 | 4156 | stable |
| `corpus_dir/ast` | 1 | 40 | 8.598 | 11.863 | 7.664 | 13.113 | 1 | 4216 | stable |
| `corpus_dir/legacy` | 1 | 40 | 7.252 | 8.213 | 4.163 | 9.185 | 1 | 4064 | stable |
| `corpus_dir/compare` | 1 | 40 | 11.389 | 14.883 | 8.629 | 16.473 | 1 | 4360 | stable |
| `many_symbols/default` | 1 | 30 | 50.220 | 61.329 | 39.517 | 64.814 | 0 | 14548 | stable |
| `many_symbols/ast` | 1 | 30 | 49.583 | 59.859 | 38.490 | 61.169 | 0 | 14600 | stable |
| `many_symbols/legacy` | 1 | 30 | 15.485 | 17.246 | 9.125 | 17.516 | 0 | 7132 | stable |
| `call_graph/default` | 1 | 30 | 21.632 | 32.128 | 17.131 | 32.385 | 0 | 11480 | stable |
| `call_graph/legacy` | 1 | 30 | 10.902 | 18.199 | 8.499 | 20.538 | 0 | 6492 | stable |
| `nested_scopes/default` | 1 | 30 | 6.370 | 8.638 | 4.419 | 8.891 | 0 | 4680 | stable |
| `nested_scopes/legacy` | 1 | 30 | 5.196 | 5.669 | 2.916 | 5.981 | 0 | 3772 | stable |
| `malformed/default` | 1 | 30 | 7.936 | 8.305 | 4.762 | 8.807 | 1 | 4396 | stable |
| `malformed/legacy` | 1 | 30 | 5.596 | 8.967 | 4.041 | 9.319 | 1 | 4088 | stable |
| `multi_file/default` | 1 | 30 | 5.969 | 8.522 | 5.008 | 8.999 | 0 | 3948 | stable |
| `multi_file/legacy` | 1 | 30 | 4.433 | 5.205 | 2.448 | 5.274 | 0 | 3692 | stable |
| `near_limit/default` | 1 | 8 | 109.710 | 121.494 | 104.946 | 121.494 | 0 | 66816 | stable |
| `near_limit/legacy` | 1 | 8 | 58.198 | 69.388 | 49.312 | 69.388 | 0 | 20552 | stable |
| `diagnostic_flood/default` | 1 | 20 | 169.752 | 180.968 | 159.924 | 182.307 | 0 | 38476 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 48.241 | 54.641 | 45.502 | 57.403 | 0 | 16648 | stable |
| `preprocess/default` | 1 | 30 | 16.338 | 22.435 | 12.926 | 24.167 | 0 | 8172 | stable |
| `preprocess/legacy` | 1 | 30 | 11.414 | 19.650 | 10.398 | 19.783 | 0 | 7452 | stable |
| `stdin/default` | 1 | 30 | 4.149 | 4.426 | 2.943 | 4.518 | 0 | 3740 | stable |
| `stdin/ast` | 1 | 30 | 3.080 | 4.783 | 2.549 | 5.175 | 0 | 3832 | stable |
| `stdin/legacy` | 1 | 30 | 3.976 | 4.256 | 2.649 | 4.396 | 0 | 3496 | stable |
| `startup` | 2 | 50 | 1.545 | 1.765 | 1.403 | 1.892 | 0 | 2408 | stable |
| `corpus_small/default` | 2 | 50 | 4.264 | 4.772 | 2.282 | 6.117 | 0 | 3844 | stable |
| `corpus_small/ast` | 2 | 50 | 3.822 | 4.941 | 2.564 | 6.078 | 0 | 3792 | stable |
| `corpus_small/legacy` | 2 | 50 | 4.170 | 4.573 | 2.137 | 5.522 | 0 | 3460 | stable |
| `corpus_small/compare` | 2 | 50 | 4.479 | 4.914 | 2.713 | 5.523 | 0 | 3856 | stable |
| `corpus_medium/default` | 2 | 50 | 4.317 | 4.989 | 2.764 | 5.901 | 0 | 3844 | stable |
| `corpus_medium/ast` | 2 | 50 | 4.535 | 5.085 | 2.755 | 6.190 | 0 | 3824 | stable |
| `corpus_medium/legacy` | 2 | 50 | 4.056 | 4.593 | 2.444 | 5.641 | 0 | 3444 | stable |
| `corpus_medium/compare` | 2 | 50 | 3.474 | 4.819 | 2.874 | 5.729 | 0 | 4068 | stable |
| `corpus_large/default` | 2 | 30 | 4.078 | 5.044 | 2.789 | 6.805 | 0 | 3924 | stable |
| `corpus_large/ast` | 2 | 30 | 2.916 | 5.193 | 2.557 | 5.268 | 0 | 3988 | stable |
| `corpus_large/legacy` | 2 | 30 | 2.987 | 4.532 | 2.266 | 4.574 | 0 | 3528 | stable |
| `corpus_large/compare` | 2 | 30 | 3.099 | 5.620 | 2.761 | 5.986 | 0 | 3976 | stable |
| `corpus_invalid/default` | 2 | 50 | 2.580 | 4.568 | 2.252 | 5.470 | 1 | 3884 | stable |
| `corpus_invalid/ast` | 2 | 50 | 2.589 | 4.860 | 2.239 | 5.600 | 1 | 3912 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 2.376 | 4.213 | 2.016 | 4.658 | 1 | 3604 | stable |
| `corpus_invalid/compare` | 2 | 50 | 2.766 | 4.661 | 2.308 | 5.495 | 1 | 4000 | stable |
| `corpus_dir/default` | 2 | 40 | 8.748 | 12.526 | 7.210 | 13.898 | 1 | 4180 | stable |
| `corpus_dir/ast` | 2 | 40 | 7.687 | 10.127 | 6.650 | 12.509 | 1 | 4184 | stable |
| `corpus_dir/legacy` | 2 | 40 | 4.601 | 8.127 | 4.032 | 8.575 | 1 | 4036 | stable |
| `corpus_dir/compare` | 2 | 40 | 12.891 | 14.690 | 9.479 | 15.510 | 1 | 4312 | stable |
| `many_symbols/default` | 2 | 30 | 50.633 | 58.997 | 38.263 | 59.890 | 0 | 14572 | stable |
| `many_symbols/ast` | 2 | 30 | 49.314 | 61.255 | 36.842 | 62.676 | 0 | 14548 | stable |
| `many_symbols/legacy` | 2 | 30 | 13.198 | 17.456 | 8.906 | 17.558 | 0 | 7196 | stable |
| `call_graph/default` | 2 | 30 | 28.522 | 32.874 | 16.223 | 33.887 | 0 | 11468 | stable |
| `call_graph/legacy` | 2 | 30 | 12.007 | 17.705 | 8.481 | 19.686 | 0 | 6512 | stable |
| `nested_scopes/default` | 2 | 30 | 6.175 | 7.934 | 4.300 | 7.939 | 0 | 4652 | stable |
| `nested_scopes/legacy` | 2 | 30 | 4.867 | 7.127 | 2.942 | 7.262 | 0 | 3632 | stable |
| `malformed/default` | 2 | 30 | 7.324 | 8.169 | 4.914 | 8.283 | 1 | 4416 | stable |
| `malformed/legacy` | 2 | 30 | 6.579 | 7.306 | 4.647 | 8.072 | 1 | 4084 | stable |
| `multi_file/default` | 2 | 30 | 7.702 | 8.611 | 5.156 | 8.661 | 0 | 3968 | stable |
| `multi_file/legacy` | 2 | 30 | 3.928 | 5.011 | 2.602 | 5.165 | 0 | 3620 | stable |
| `near_limit/default` | 2 | 8 | 111.687 | 123.763 | 104.219 | 123.763 | 0 | 66812 | stable |
| `near_limit/legacy` | 2 | 8 | 53.207 | 59.556 | 51.064 | 59.556 | 0 | 20648 | stable |
| `diagnostic_flood/default` | 2 | 20 | 180.921 | 189.858 | 163.132 | 193.482 | 0 | 38480 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 49.255 | 56.415 | 45.132 | 56.441 | 0 | 16608 | stable |
| `preprocess/default` | 2 | 30 | 16.519 | 23.636 | 12.354 | 23.951 | 0 | 8136 | stable |
| `preprocess/legacy` | 2 | 30 | 11.525 | 19.243 | 10.190 | 19.529 | 0 | 7500 | stable |
| `stdin/default` | 2 | 30 | 4.144 | 4.805 | 2.231 | 5.610 | 0 | 3748 | stable |
| `stdin/ast` | 2 | 30 | 3.642 | 4.986 | 2.190 | 5.449 | 0 | 3732 | stable |
| `stdin/legacy` | 2 | 30 | 3.858 | 4.346 | 2.040 | 4.450 | 0 | 3656 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 21.632 | 28.522 | 31.8 |
| `call_graph/legacy` | 10.902 | 12.007 | 10.1 |
| `corpus_dir/ast` | 8.598 | 7.687 | 11.8 |
| `corpus_dir/compare` | 11.389 | 12.891 | 13.2 |
| `corpus_dir/default` | 10.323 | 8.748 | 18.0 |
| `corpus_dir/legacy` | 7.252 | 4.601 | 57.6 |
| `corpus_invalid/ast` | 3.975 | 2.589 | 53.6 |
| `corpus_invalid/compare` | 4.508 | 2.766 | 63.0 |
| `corpus_invalid/default` | 3.083 | 2.580 | 19.5 |
| `corpus_invalid/legacy` | 4.118 | 2.376 | 73.3 |
| `corpus_large/ast` | 4.783 | 2.916 | 64.0 |
| `corpus_large/compare` | 3.998 | 3.099 | 29.0 |
| `corpus_large/default` | 4.610 | 4.078 | 13.0 |
| `corpus_large/legacy` | 2.826 | 2.987 | 5.7 |
| `corpus_medium/ast` | 3.153 | 4.535 | 43.8 |
| `corpus_medium/compare` | 3.066 | 3.474 | 13.3 |
| `corpus_medium/default` | 4.647 | 4.317 | 7.6 |
| `corpus_medium/legacy` | 4.248 | 4.056 | 4.7 |
| `corpus_small/ast` | 4.128 | 3.822 | 8.0 |
| `corpus_small/compare` | 4.554 | 4.479 | 1.7 |
| `corpus_small/default` | 4.051 | 4.264 | 5.2 |
| `corpus_small/legacy` | 2.505 | 4.170 | 66.5 |
| `diagnostic_flood/default` | 169.752 | 180.921 | 6.6 |
| `diagnostic_flood/legacy` | 48.241 | 49.255 | 2.1 |
| `malformed/default` | 7.936 | 7.324 | 8.4 |
| `malformed/legacy` | 5.596 | 6.579 | 17.6 |
| `many_symbols/ast` | 49.583 | 49.314 | 0.5 |
| `many_symbols/default` | 50.220 | 50.633 | 0.8 |
| `many_symbols/legacy` | 15.485 | 13.198 | 17.3 |
| `multi_file/default` | 5.969 | 7.702 | 29.0 |
| `multi_file/legacy` | 4.433 | 3.928 | 12.9 |
| `near_limit/default` | 109.710 | 111.687 | 1.8 |
| `near_limit/legacy` | 58.198 | 53.207 | 9.4 |
| `nested_scopes/default` | 6.370 | 6.175 | 3.2 |
| `nested_scopes/legacy` | 5.196 | 4.867 | 6.7 |
| `preprocess/default` | 16.338 | 16.519 | 1.1 |
| `preprocess/legacy` | 11.414 | 11.525 | 1.0 |
| `stdin/ast` | 3.080 | 3.642 | 18.3 |
| `stdin/default` | 4.149 | 4.144 | 0.1 |
| `stdin/legacy` | 3.976 | 3.858 | 3.1 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
