# neolint CLI benchmark — Phase 8 baseline

- harness revision: `0ea4a604d309aa65b83a0793ee7e8ffa9e2de8e6` (tree `33f97eee79f211b1c7cb06a0d977e41b8eb1ce46`), branch `exp-preprocessor-ast-phase-08-stabilization`
- measured binary: built from `66936b0e175d51cba8d83377061229869ef64e96` (tree `87d810459064f438fe61c0a632fa83642ea68971`), sha256 `83d9fce60666773fc73318a70a163b147c428eacd155eb89ece85486295f8ac8`, verified against the expected SHA-256
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
| `startup` | 1 | 50 | 1.003 | 1.769 | 0.760 | 1.902 | 0 | 2220 | stable |
| `corpus_small/default` | 1 | 50 | 2.936 | 4.611 | 2.440 | 4.714 | 0 | 3824 | stable |
| `corpus_small/ast` | 1 | 50 | 2.940 | 4.975 | 2.498 | 6.234 | 0 | 3768 | stable |
| `corpus_small/legacy` | 1 | 50 | 2.477 | 3.189 | 2.222 | 4.200 | 0 | 3600 | stable |
| `corpus_small/compare` | 1 | 50 | 4.192 | 5.616 | 2.563 | 8.166 | 0 | 4036 | stable |
| `corpus_medium/default` | 1 | 50 | 2.475 | 2.786 | 2.227 | 2.964 | 0 | 3844 | stable |
| `corpus_medium/ast` | 1 | 50 | 2.583 | 4.459 | 2.286 | 4.541 | 0 | 3828 | stable |
| `corpus_medium/legacy` | 1 | 50 | 2.720 | 4.966 | 2.010 | 5.138 | 0 | 3636 | stable |
| `corpus_medium/compare` | 1 | 50 | 2.709 | 4.715 | 2.312 | 4.856 | 0 | 3904 | stable |
| `corpus_large/default` | 1 | 30 | 2.780 | 4.180 | 2.419 | 5.899 | 0 | 4004 | stable |
| `corpus_large/ast` | 1 | 30 | 2.660 | 3.195 | 2.373 | 3.621 | 0 | 3924 | stable |
| `corpus_large/legacy` | 1 | 30 | 3.817 | 4.482 | 2.103 | 5.207 | 0 | 3584 | stable |
| `corpus_large/compare` | 1 | 30 | 4.506 | 4.928 | 4.349 | 4.959 | 0 | 3984 | stable |
| `corpus_invalid/default` | 1 | 50 | 3.925 | 4.243 | 2.802 | 4.315 | 1 | 3888 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.896 | 4.332 | 3.790 | 4.623 | 1 | 3776 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 3.645 | 4.110 | 3.477 | 4.367 | 1 | 3496 | stable |
| `corpus_invalid/compare` | 1 | 50 | 3.992 | 4.199 | 3.070 | 4.657 | 1 | 3916 | stable |
| `corpus_dir/default` | 1 | 40 | 11.399 | 12.519 | 9.306 | 14.982 | 1 | 4168 | stable |
| `corpus_dir/ast` | 1 | 40 | 8.679 | 13.155 | 7.613 | 13.528 | 1 | 4176 | stable |
| `corpus_dir/legacy` | 1 | 40 | 5.915 | 7.191 | 3.993 | 7.643 | 1 | 4128 | stable |
| `corpus_dir/compare` | 1 | 40 | 12.316 | 14.699 | 9.342 | 16.741 | 1 | 4280 | stable |
| `many_symbols/default` | 1 | 30 | 47.435 | 61.537 | 37.132 | 61.908 | 0 | 15872 | stable |
| `many_symbols/ast` | 1 | 30 | 47.305 | 58.707 | 34.380 | 61.539 | 0 | 15872 | stable |
| `many_symbols/legacy` | 1 | 30 | 11.745 | 17.805 | 9.439 | 18.160 | 0 | 7580 | stable |
| `call_graph/default` | 1 | 30 | 22.171 | 31.870 | 16.474 | 34.225 | 0 | 11460 | stable |
| `call_graph/legacy` | 1 | 30 | 12.084 | 15.603 | 8.268 | 16.505 | 0 | 6520 | stable |
| `nested_scopes/default` | 1 | 30 | 7.226 | 8.370 | 4.387 | 8.778 | 0 | 4640 | stable |
| `nested_scopes/legacy` | 1 | 30 | 4.716 | 5.519 | 2.582 | 5.667 | 0 | 3732 | stable |
| `malformed/default` | 1 | 30 | 7.282 | 7.965 | 4.087 | 8.378 | 1 | 4408 | stable |
| `malformed/legacy` | 1 | 30 | 4.140 | 7.554 | 3.740 | 7.585 | 1 | 4056 | stable |
| `multi_file/default` | 1 | 30 | 5.893 | 8.254 | 4.739 | 8.548 | 0 | 3872 | stable |
| `multi_file/legacy` | 1 | 30 | 4.538 | 5.489 | 2.720 | 5.540 | 0 | 3600 | stable |
| `near_limit/default` | 1 | 5 | 476.031 | 502.976 | 459.817 | 502.976 | 0 | 309692 | stable |
| `near_limit/legacy` | 1 | 5 | 229.978 | 242.692 | 223.667 | 242.692 | 0 | 85344 | stable |
| `diagnostic_flood/default` | 1 | 20 | 167.648 | 176.741 | 155.722 | 179.305 | 0 | 38452 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 49.928 | 53.980 | 43.089 | 55.182 | 0 | 16632 | stable |
| `preprocess/default` | 1 | 30 | 20.562 | 21.998 | 11.824 | 22.681 | 0 | 8140 | stable |
| `preprocess/legacy` | 1 | 30 | 10.027 | 17.907 | 9.705 | 18.609 | 0 | 7536 | stable |
| `stdin/default` | 1 | 30 | 3.841 | 4.085 | 2.632 | 4.428 | 0 | 3736 | stable |
| `stdin/ast` | 1 | 30 | 3.899 | 4.309 | 3.677 | 4.873 | 0 | 3732 | stable |
| `stdin/legacy` | 1 | 30 | 3.577 | 3.955 | 3.442 | 4.060 | 0 | 3472 | stable |
| `startup` | 2 | 50 | 1.168 | 1.708 | 0.803 | 1.778 | 0 | 2200 | stable |
| `corpus_small/default` | 2 | 50 | 2.861 | 4.109 | 2.672 | 4.291 | 0 | 3808 | stable |
| `corpus_small/ast` | 2 | 50 | 3.965 | 4.407 | 2.693 | 4.930 | 0 | 3796 | stable |
| `corpus_small/legacy` | 2 | 50 | 3.638 | 4.135 | 3.479 | 4.341 | 0 | 3532 | stable |
| `corpus_small/compare` | 2 | 50 | 4.088 | 4.309 | 3.936 | 4.386 | 0 | 3964 | stable |
| `corpus_medium/default` | 2 | 50 | 4.007 | 4.266 | 2.827 | 4.308 | 0 | 3828 | stable |
| `corpus_medium/ast` | 2 | 50 | 4.045 | 4.312 | 2.881 | 4.547 | 0 | 3828 | stable |
| `corpus_medium/legacy` | 2 | 50 | 3.686 | 3.986 | 2.755 | 4.298 | 0 | 3464 | stable |
| `corpus_medium/compare` | 2 | 50 | 4.219 | 4.465 | 4.066 | 4.595 | 0 | 4060 | stable |
| `corpus_large/default` | 2 | 30 | 4.253 | 4.494 | 4.020 | 4.641 | 0 | 3908 | stable |
| `corpus_large/ast` | 2 | 30 | 4.251 | 4.433 | 4.121 | 4.653 | 0 | 3884 | stable |
| `corpus_large/legacy` | 2 | 30 | 4.042 | 4.267 | 3.758 | 4.810 | 0 | 3560 | stable |
| `corpus_large/compare` | 2 | 30 | 4.486 | 4.626 | 3.583 | 4.648 | 0 | 3972 | stable |
| `corpus_invalid/default` | 2 | 50 | 2.962 | 4.246 | 2.264 | 4.266 | 1 | 3772 | stable |
| `corpus_invalid/ast` | 2 | 50 | 2.261 | 4.317 | 2.107 | 4.513 | 1 | 3816 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 3.697 | 3.998 | 2.137 | 4.910 | 1 | 3608 | stable |
| `corpus_invalid/compare` | 2 | 50 | 4.207 | 4.611 | 3.887 | 4.879 | 1 | 3864 | stable |
| `corpus_dir/default` | 2 | 40 | 12.114 | 13.744 | 9.429 | 14.344 | 1 | 4200 | stable |
| `corpus_dir/ast` | 2 | 40 | 11.619 | 12.319 | 8.497 | 12.707 | 1 | 4224 | stable |
| `corpus_dir/legacy` | 2 | 40 | 6.954 | 7.832 | 6.801 | 8.117 | 1 | 4040 | stable |
| `corpus_dir/compare` | 2 | 40 | 13.614 | 14.874 | 10.366 | 15.157 | 1 | 4272 | stable |
| `many_symbols/default` | 2 | 30 | 46.335 | 66.817 | 33.842 | 66.984 | 0 | 15780 | stable |
| `many_symbols/ast` | 2 | 30 | 46.683 | 57.020 | 36.277 | 60.415 | 0 | 15880 | stable |
| `many_symbols/legacy` | 2 | 30 | 10.816 | 17.572 | 9.496 | 18.044 | 0 | 7620 | stable |
| `call_graph/default` | 2 | 30 | 21.202 | 29.654 | 19.684 | 31.978 | 0 | 11476 | stable |
| `call_graph/legacy` | 2 | 30 | 12.306 | 16.193 | 8.344 | 16.651 | 0 | 6500 | stable |
| `nested_scopes/default` | 2 | 30 | 7.429 | 8.157 | 4.773 | 8.203 | 0 | 4624 | stable |
| `nested_scopes/legacy` | 2 | 30 | 4.730 | 5.441 | 2.669 | 5.540 | 0 | 3692 | stable |
| `malformed/default` | 2 | 30 | 7.070 | 8.480 | 4.609 | 9.038 | 1 | 4400 | stable |
| `malformed/legacy` | 2 | 30 | 6.456 | 7.444 | 4.082 | 7.633 | 1 | 4072 | stable |
| `multi_file/default` | 2 | 30 | 7.458 | 8.252 | 5.390 | 8.792 | 0 | 3904 | stable |
| `multi_file/legacy` | 2 | 30 | 3.882 | 4.731 | 2.553 | 4.758 | 0 | 3584 | stable |
| `near_limit/default` | 2 | 5 | 488.595 | 496.593 | 479.129 | 496.593 | 0 | 309696 | stable |
| `near_limit/legacy` | 2 | 5 | 241.484 | 257.425 | 231.909 | 257.425 | 0 | 85356 | stable |
| `diagnostic_flood/default` | 2 | 20 | 168.351 | 183.832 | 152.128 | 189.257 | 0 | 38452 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 51.141 | 56.103 | 42.817 | 56.842 | 0 | 16620 | stable |
| `preprocess/default` | 2 | 30 | 17.282 | 22.818 | 12.800 | 23.515 | 0 | 8176 | stable |
| `preprocess/legacy` | 2 | 30 | 13.426 | 18.686 | 9.843 | 18.696 | 0 | 7508 | stable |
| `stdin/default` | 2 | 30 | 2.441 | 4.335 | 2.091 | 4.339 | 0 | 3812 | stable |
| `stdin/ast` | 2 | 30 | 2.467 | 4.396 | 2.152 | 4.470 | 0 | 3768 | stable |
| `stdin/legacy` | 2 | 30 | 2.427 | 4.176 | 1.993 | 4.343 | 0 | 3476 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 22.171 | 21.202 | 4.6 |
| `call_graph/legacy` | 12.084 | 12.306 | 1.8 |
| `corpus_dir/ast` | 8.679 | 11.619 | 33.9 |
| `corpus_dir/compare` | 12.316 | 13.614 | 10.5 |
| `corpus_dir/default` | 11.399 | 12.114 | 6.3 |
| `corpus_dir/legacy` | 5.915 | 6.954 | 17.6 |
| `corpus_invalid/ast` | 3.896 | 2.261 | 72.3 |
| `corpus_invalid/compare` | 3.992 | 4.207 | 5.4 |
| `corpus_invalid/default` | 3.925 | 2.962 | 32.5 |
| `corpus_invalid/legacy` | 3.645 | 3.697 | 1.4 |
| `corpus_large/ast` | 2.660 | 4.251 | 59.9 |
| `corpus_large/compare` | 4.506 | 4.486 | 0.5 |
| `corpus_large/default` | 2.780 | 4.253 | 53.0 |
| `corpus_large/legacy` | 3.817 | 4.042 | 5.9 |
| `corpus_medium/ast` | 2.583 | 4.045 | 56.6 |
| `corpus_medium/compare` | 2.709 | 4.219 | 55.7 |
| `corpus_medium/default` | 2.475 | 4.007 | 61.9 |
| `corpus_medium/legacy` | 2.720 | 3.686 | 35.5 |
| `corpus_small/ast` | 2.940 | 3.965 | 34.9 |
| `corpus_small/compare` | 4.192 | 4.088 | 2.5 |
| `corpus_small/default` | 2.936 | 2.861 | 2.6 |
| `corpus_small/legacy` | 2.477 | 3.638 | 46.9 |
| `diagnostic_flood/default` | 167.648 | 168.351 | 0.4 |
| `diagnostic_flood/legacy` | 49.928 | 51.141 | 2.4 |
| `malformed/default` | 7.282 | 7.070 | 3.0 |
| `malformed/legacy` | 4.140 | 6.456 | 56.0 |
| `many_symbols/ast` | 47.305 | 46.683 | 1.3 |
| `many_symbols/default` | 47.435 | 46.335 | 2.4 |
| `many_symbols/legacy` | 11.745 | 10.816 | 8.6 |
| `multi_file/default` | 5.893 | 7.458 | 26.5 |
| `multi_file/legacy` | 4.538 | 3.882 | 16.9 |
| `near_limit/default` | 476.031 | 488.595 | 2.6 |
| `near_limit/legacy` | 229.978 | 241.484 | 5.0 |
| `nested_scopes/default` | 7.226 | 7.429 | 2.8 |
| `nested_scopes/legacy` | 4.716 | 4.730 | 0.3 |
| `preprocess/default` | 20.562 | 17.282 | 19.0 |
| `preprocess/legacy` | 10.027 | 13.426 | 33.9 |
| `stdin/ast` | 3.899 | 2.467 | 58.1 |
| `stdin/default` | 3.841 | 2.441 | 57.3 |
| `stdin/legacy` | 3.577 | 2.427 | 47.4 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
