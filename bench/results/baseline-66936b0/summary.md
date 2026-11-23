# neolint CLI benchmark — Phase 8 baseline

- harness revision: `37731ffa0f53a8ecf04fd57ffb9f33a426d8ced7` (tree `6b8b84fc2dfc476d7d174ddd579759ad1ab2a458`), branch `exp-preprocessor-ast-phase-08-stabilization`
- measured binary: built from `66936b0e175d51cba8d83377061229869ef64e96` (tree `87d810459064f438fe61c0a632fa83642ea68971`), sha256 `83d9fce60666773fc73318a70a163b147c428eacd155eb89ece85486295f8ac8`, digest matches the expected value; a fresh build of the same commit reproduced the same digest
- benchmark inputs: 70 files, 8764087 bytes (per-file hashes in `meta.json`)
- rustc `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, cargo `cargo 1.99.1 (5f94df478 2026-08-27)`
- host: Linux-7.0.0-43-generic-x86_64-with-glibc2.39 / x86_64 / AMD Ryzen 5 2600X Six-Core Processor / 12 threads
- binary path: `/home/eloraam/apps/neolint/target/wt-base/target/release/neolint` (1659648 bytes)
- warm-up: 3 iterations per row; two independent passes
- RSS: peak VmHWM polled from /proc, 3 untimed samples per row

All figures are **end-to-end CLI** measurements and include process startup.
Plain `--engine legacy` is a direct no-preprocess path; AST always runs an
identity-prepared unit; `--engine compare` runs both analyzers. Ratios
between those modes are not parser-only comparisons.

| Row | Pass | Samples | Median ms | p95 ms | Min ms | Max ms | Exit | RSS KB | Digests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `startup` | 1 | 50 | 1.382 | 1.610 | 0.949 | 1.650 | 0 | 2148 | stable |
| `corpus_small/default` | 1 | 50 | 3.829 | 4.125 | 2.448 | 4.295 | 0 | 3744 | stable |
| `corpus_small/ast` | 1 | 50 | 3.744 | 3.861 | 3.675 | 4.138 | 0 | 3696 | stable |
| `corpus_small/legacy` | 1 | 50 | 3.553 | 3.893 | 3.388 | 4.032 | 0 | 3452 | stable |
| `corpus_small/compare` | 1 | 50 | 4.112 | 4.686 | 3.887 | 6.314 | 0 | 3984 | stable |
| `corpus_medium/default` | 1 | 50 | 3.966 | 4.629 | 2.187 | 4.970 | 0 | 3792 | stable |
| `corpus_medium/ast` | 1 | 50 | 3.880 | 4.365 | 2.543 | 4.922 | 0 | 3792 | stable |
| `corpus_medium/legacy` | 1 | 50 | 3.641 | 3.952 | 3.485 | 3.995 | 0 | 3460 | stable |
| `corpus_medium/compare` | 1 | 50 | 4.117 | 4.596 | 2.909 | 4.670 | 0 | 3868 | stable |
| `corpus_large/default` | 1 | 30 | 4.159 | 4.494 | 4.045 | 4.752 | 0 | 3912 | stable |
| `corpus_large/ast` | 1 | 30 | 4.235 | 4.603 | 4.042 | 4.912 | 0 | 3964 | stable |
| `corpus_large/legacy` | 1 | 30 | 3.726 | 4.006 | 2.803 | 4.009 | 0 | 3496 | stable |
| `corpus_large/compare` | 1 | 30 | 4.574 | 4.870 | 3.485 | 4.872 | 0 | 3940 | stable |
| `corpus_invalid/default` | 1 | 50 | 3.822 | 4.136 | 2.149 | 4.231 | 1 | 3824 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.793 | 4.226 | 2.984 | 4.681 | 1 | 3880 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 3.496 | 3.770 | 2.748 | 3.852 | 1 | 3700 | stable |
| `corpus_invalid/compare` | 1 | 50 | 3.941 | 4.143 | 3.857 | 4.320 | 1 | 3868 | stable |
| `corpus_dir/default` | 1 | 40 | 12.028 | 12.468 | 8.948 | 13.384 | 1 | 4188 | stable |
| `corpus_dir/ast` | 1 | 40 | 11.626 | 12.125 | 10.368 | 13.044 | 1 | 4216 | stable |
| `corpus_dir/legacy` | 1 | 40 | 6.829 | 7.055 | 6.708 | 7.148 | 1 | 4044 | stable |
| `corpus_dir/compare` | 1 | 40 | 14.047 | 14.740 | 11.348 | 15.230 | 1 | 4272 | stable |
| `many_symbols/default` | 1 | 30 | 51.997 | 59.119 | 33.387 | 59.632 | 0 | 15800 | stable |
| `many_symbols/ast` | 1 | 30 | 45.813 | 59.503 | 34.493 | 59.997 | 0 | 15780 | stable |
| `many_symbols/legacy` | 1 | 30 | 9.322 | 17.261 | 9.216 | 17.741 | 0 | 7692 | stable |
| `call_graph/default` | 1 | 30 | 28.827 | 30.029 | 16.160 | 30.090 | 0 | 11488 | stable |
| `call_graph/legacy` | 1 | 30 | 14.349 | 15.173 | 9.941 | 15.335 | 0 | 6516 | stable |
| `nested_scopes/default` | 1 | 30 | 6.667 | 7.807 | 5.206 | 8.325 | 0 | 4572 | stable |
| `nested_scopes/legacy` | 1 | 30 | 4.551 | 5.014 | 4.395 | 5.074 | 0 | 3668 | stable |
| `malformed/default` | 1 | 30 | 6.941 | 7.199 | 6.836 | 7.310 | 1 | 4336 | stable |
| `malformed/legacy` | 1 | 30 | 6.239 | 6.532 | 6.151 | 6.626 | 1 | 4084 | stable |
| `multi_file/default` | 1 | 30 | 7.391 | 7.842 | 5.535 | 8.021 | 0 | 3908 | stable |
| `multi_file/legacy` | 1 | 30 | 4.240 | 4.590 | 2.843 | 4.708 | 0 | 3600 | stable |
| `near_limit/default` | 1 | 5 | 457.090 | 469.575 | 455.111 | 469.575 | 0 | 309604 | stable |
| `near_limit/legacy` | 1 | 5 | 221.395 | 230.122 | 214.510 | 230.122 | 0 | 85344 | stable |
| `diagnostic_flood/default` | 1 | 20 | 161.343 | 184.349 | 149.852 | 188.909 | 0 | 38384 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 43.265 | 50.404 | 42.139 | 52.212 | 0 | 16624 | stable |
| `preprocess/default` | 1 | 30 | 21.191 | 21.994 | 14.314 | 22.612 | 0 | 8104 | stable |
| `preprocess/legacy` | 1 | 30 | 12.081 | 18.137 | 9.460 | 18.599 | 0 | 7564 | stable |
| `stdin/default` | 1 | 30 | 2.069 | 3.954 | 2.018 | 4.126 | 0 | 3732 | stable |
| `stdin/ast` | 1 | 30 | 3.924 | 4.137 | 2.169 | 4.139 | 0 | 3668 | stable |
| `stdin/legacy` | 1 | 30 | 1.881 | 3.741 | 1.850 | 3.842 | 0 | 3524 | stable |
| `startup` | 2 | 50 | 1.324 | 1.521 | 0.817 | 1.710 | 0 | 2152 | stable |
| `corpus_small/default` | 2 | 50 | 2.335 | 4.094 | 2.121 | 4.249 | 0 | 3784 | stable |
| `corpus_small/ast` | 2 | 50 | 3.844 | 3.918 | 3.673 | 4.074 | 0 | 3668 | stable |
| `corpus_small/legacy` | 2 | 50 | 2.124 | 3.906 | 1.869 | 4.211 | 0 | 3492 | stable |
| `corpus_small/compare` | 2 | 50 | 2.288 | 4.202 | 2.159 | 4.277 | 0 | 3924 | stable |
| `corpus_medium/default` | 2 | 50 | 3.913 | 4.481 | 2.816 | 4.722 | 0 | 3804 | stable |
| `corpus_medium/ast` | 2 | 50 | 4.015 | 4.509 | 2.595 | 4.614 | 0 | 3820 | stable |
| `corpus_medium/legacy` | 2 | 50 | 3.636 | 3.943 | 3.519 | 4.314 | 0 | 3488 | stable |
| `corpus_medium/compare` | 2 | 50 | 4.125 | 4.238 | 4.039 | 4.764 | 0 | 3872 | stable |
| `corpus_large/default` | 2 | 30 | 4.165 | 4.750 | 3.593 | 4.856 | 0 | 3912 | stable |
| `corpus_large/ast` | 2 | 30 | 4.107 | 4.349 | 3.965 | 4.517 | 0 | 3972 | stable |
| `corpus_large/legacy` | 2 | 30 | 3.712 | 3.947 | 2.413 | 4.202 | 0 | 3560 | stable |
| `corpus_large/compare` | 2 | 30 | 4.351 | 4.715 | 4.231 | 4.720 | 0 | 4092 | stable |
| `corpus_invalid/default` | 2 | 50 | 3.747 | 4.267 | 2.668 | 4.278 | 1 | 3900 | stable |
| `corpus_invalid/ast` | 2 | 50 | 3.721 | 3.825 | 3.636 | 4.380 | 1 | 3784 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 3.523 | 3.790 | 3.443 | 4.354 | 1 | 3484 | stable |
| `corpus_invalid/compare` | 2 | 50 | 3.956 | 4.265 | 3.858 | 4.511 | 1 | 3900 | stable |
| `corpus_dir/default` | 2 | 40 | 11.731 | 13.081 | 10.708 | 13.941 | 1 | 4156 | stable |
| `corpus_dir/ast` | 2 | 40 | 11.791 | 12.236 | 8.801 | 13.465 | 1 | 4168 | stable |
| `corpus_dir/legacy` | 2 | 40 | 6.822 | 6.889 | 6.718 | 6.990 | 1 | 4136 | stable |
| `corpus_dir/compare` | 2 | 40 | 13.799 | 14.483 | 12.807 | 14.869 | 1 | 4292 | stable |
| `many_symbols/default` | 2 | 30 | 50.417 | 60.468 | 34.329 | 62.063 | 0 | 15800 | stable |
| `many_symbols/ast` | 2 | 30 | 56.791 | 61.566 | 36.671 | 62.715 | 0 | 15804 | stable |
| `many_symbols/legacy` | 2 | 30 | 9.229 | 17.277 | 9.071 | 18.457 | 0 | 7676 | stable |
| `call_graph/default` | 2 | 30 | 19.845 | 29.539 | 16.015 | 29.733 | 0 | 11460 | stable |
| `call_graph/legacy` | 2 | 30 | 14.365 | 15.148 | 14.256 | 15.170 | 0 | 6512 | stable |
| `nested_scopes/default` | 2 | 30 | 6.764 | 7.621 | 5.537 | 7.798 | 0 | 4724 | stable |
| `nested_scopes/legacy` | 2 | 30 | 4.423 | 4.978 | 2.829 | 5.159 | 0 | 3668 | stable |
| `malformed/default` | 2 | 30 | 6.575 | 6.943 | 6.460 | 7.466 | 1 | 4304 | stable |
| `malformed/legacy` | 2 | 30 | 6.106 | 6.284 | 6.016 | 6.487 | 1 | 4084 | stable |
| `multi_file/default` | 2 | 30 | 7.328 | 7.753 | 5.442 | 7.882 | 0 | 3900 | stable |
| `multi_file/legacy` | 2 | 30 | 4.143 | 4.583 | 4.049 | 4.723 | 0 | 3608 | stable |
| `near_limit/default` | 2 | 5 | 456.293 | 470.917 | 453.731 | 470.917 | 0 | 309620 | stable |
| `near_limit/legacy` | 2 | 5 | 214.543 | 217.535 | 212.584 | 217.535 | 0 | 85344 | stable |
| `diagnostic_flood/default` | 2 | 20 | 155.880 | 171.477 | 148.859 | 177.843 | 0 | 38420 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 45.638 | 51.742 | 42.452 | 52.599 | 0 | 16624 | stable |
| `preprocess/default` | 2 | 30 | 15.161 | 22.745 | 11.806 | 23.028 | 0 | 8116 | stable |
| `preprocess/legacy` | 2 | 30 | 10.343 | 17.928 | 9.523 | 17.937 | 0 | 7472 | stable |
| `stdin/default` | 2 | 30 | 3.061 | 4.137 | 2.021 | 4.165 | 0 | 3688 | stable |
| `stdin/ast` | 2 | 30 | 3.710 | 4.136 | 2.339 | 4.368 | 0 | 3756 | stable |
| `stdin/legacy` | 2 | 30 | 3.426 | 3.713 | 2.162 | 4.026 | 0 | 3500 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 28.827 | 19.845 | 45.3 |
| `call_graph/legacy` | 14.349 | 14.365 | 0.1 |
| `corpus_dir/ast` | 11.626 | 11.791 | 1.4 |
| `corpus_dir/compare` | 14.047 | 13.799 | 1.8 |
| `corpus_dir/default` | 12.028 | 11.731 | 2.5 |
| `corpus_dir/legacy` | 6.829 | 6.822 | 0.1 |
| `corpus_invalid/ast` | 3.793 | 3.721 | 1.9 |
| `corpus_invalid/compare` | 3.941 | 3.956 | 0.4 |
| `corpus_invalid/default` | 3.822 | 3.747 | 2.0 |
| `corpus_invalid/legacy` | 3.496 | 3.523 | 0.8 |
| `corpus_large/ast` | 4.235 | 4.107 | 3.1 |
| `corpus_large/compare` | 4.574 | 4.351 | 5.1 |
| `corpus_large/default` | 4.159 | 4.165 | 0.1 |
| `corpus_large/legacy` | 3.726 | 3.712 | 0.4 |
| `corpus_medium/ast` | 3.880 | 4.015 | 3.5 |
| `corpus_medium/compare` | 4.117 | 4.125 | 0.2 |
| `corpus_medium/default` | 3.966 | 3.913 | 1.4 |
| `corpus_medium/legacy` | 3.641 | 3.636 | 0.1 |
| `corpus_small/ast` | 3.744 | 3.844 | 2.7 |
| `corpus_small/compare` | 4.112 | 2.288 | 79.7 |
| `corpus_small/default` | 3.829 | 2.335 | 64.0 |
| `corpus_small/legacy` | 3.553 | 2.124 | 67.2 |
| `diagnostic_flood/default` | 161.343 | 155.880 | 3.5 |
| `diagnostic_flood/legacy` | 43.265 | 45.638 | 5.5 |
| `malformed/default` | 6.941 | 6.575 | 5.6 |
| `malformed/legacy` | 6.239 | 6.106 | 2.2 |
| `many_symbols/ast` | 45.813 | 56.791 | 24.0 |
| `many_symbols/default` | 51.997 | 50.417 | 3.1 |
| `many_symbols/legacy` | 9.322 | 9.229 | 1.0 |
| `multi_file/default` | 7.391 | 7.328 | 0.9 |
| `multi_file/legacy` | 4.240 | 4.143 | 2.3 |
| `near_limit/default` | 457.090 | 456.293 | 0.2 |
| `near_limit/legacy` | 221.395 | 214.543 | 3.2 |
| `nested_scopes/default` | 6.667 | 6.764 | 1.4 |
| `nested_scopes/legacy` | 4.551 | 4.423 | 2.9 |
| `preprocess/default` | 21.191 | 15.161 | 39.8 |
| `preprocess/legacy` | 12.081 | 10.343 | 16.8 |
| `stdin/ast` | 3.924 | 3.710 | 5.8 |
| `stdin/default` | 2.069 | 3.061 | 47.9 |
| `stdin/legacy` | 1.881 | 3.426 | 82.2 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
