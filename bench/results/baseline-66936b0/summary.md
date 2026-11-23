# neolint CLI benchmark — Phase 8 baseline

- harness revision: `a0fea1ea5ce73557a8e925efc34c50528fdd6a5d` (tree `8264d70e02802de0fe57e4d49e2a662c8086eeef`), branch `exp-preprocessor-ast-phase-08-stabilization`, worktree clean
- harness source digest: `babcf808592bf1803849ff1e4313dca8f334f7964d128587230186808347efe3` (over bench/bench.py, bench/workloads.py)
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
| `startup` | 1 | 50 | 1.291 | 1.677 | 0.794 | 1.725 | 0 | 2532 | stable |
| `corpus_small/default` | 1 | 50 | 2.763 | 4.532 | 2.272 | 5.014 | 0 | 3884 | stable |
| `corpus_small/ast` | 1 | 50 | 3.310 | 4.829 | 2.271 | 6.246 | 0 | 3904 | stable |
| `corpus_small/legacy` | 1 | 50 | 3.692 | 4.696 | 2.161 | 4.975 | 0 | 3712 | stable |
| `corpus_small/compare` | 1 | 50 | 2.867 | 4.699 | 2.387 | 4.872 | 0 | 3856 | stable |
| `corpus_medium/default` | 1 | 50 | 2.765 | 4.060 | 2.475 | 4.859 | 0 | 3900 | stable |
| `corpus_medium/ast` | 1 | 50 | 4.265 | 4.914 | 2.386 | 4.961 | 0 | 3912 | stable |
| `corpus_medium/legacy` | 1 | 50 | 2.748 | 4.172 | 2.000 | 4.544 | 0 | 3592 | stable |
| `corpus_medium/compare` | 1 | 50 | 3.744 | 5.065 | 2.681 | 5.734 | 0 | 3860 | stable |
| `corpus_large/default` | 1 | 30 | 3.038 | 5.060 | 2.636 | 5.088 | 0 | 3928 | stable |
| `corpus_large/ast` | 1 | 30 | 3.472 | 6.238 | 2.694 | 6.399 | 0 | 4100 | stable |
| `corpus_large/legacy` | 1 | 30 | 2.350 | 4.624 | 2.049 | 4.812 | 0 | 3752 | stable |
| `corpus_large/compare` | 1 | 30 | 2.958 | 5.522 | 2.501 | 5.535 | 0 | 3936 | stable |
| `corpus_invalid/default` | 1 | 50 | 3.217 | 4.340 | 2.334 | 5.136 | 1 | 3900 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.969 | 4.605 | 2.290 | 4.904 | 1 | 3864 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 2.486 | 4.299 | 2.048 | 4.536 | 1 | 3708 | stable |
| `corpus_invalid/compare` | 1 | 50 | 2.833 | 4.667 | 2.265 | 4.737 | 1 | 3948 | stable |
| `corpus_dir/default` | 1 | 40 | 9.445 | 14.426 | 7.544 | 15.956 | 1 | 4300 | stable |
| `corpus_dir/ast` | 1 | 40 | 9.534 | 12.319 | 7.227 | 12.941 | 1 | 4240 | stable |
| `corpus_dir/legacy` | 1 | 40 | 7.171 | 7.970 | 4.089 | 8.203 | 1 | 4104 | stable |
| `corpus_dir/compare` | 1 | 40 | 9.843 | 14.827 | 8.331 | 16.414 | 1 | 4296 | stable |
| `many_symbols/default` | 1 | 30 | 50.102 | 59.511 | 36.247 | 61.902 | 0 | 15972 | stable |
| `many_symbols/ast` | 1 | 30 | 43.734 | 58.137 | 35.884 | 58.332 | 0 | 15920 | stable |
| `many_symbols/legacy` | 1 | 30 | 10.027 | 18.160 | 9.718 | 18.284 | 0 | 7816 | stable |
| `call_graph/default` | 1 | 30 | 22.188 | 30.578 | 16.307 | 31.480 | 0 | 11512 | stable |
| `call_graph/legacy` | 1 | 30 | 11.404 | 15.769 | 8.363 | 17.905 | 0 | 6660 | stable |
| `nested_scopes/default` | 1 | 30 | 5.386 | 7.533 | 4.150 | 7.812 | 0 | 4792 | stable |
| `nested_scopes/legacy` | 1 | 30 | 4.974 | 5.452 | 2.997 | 5.455 | 0 | 3776 | stable |
| `malformed/default` | 1 | 30 | 7.137 | 8.004 | 4.365 | 8.005 | 1 | 4380 | stable |
| `malformed/legacy` | 1 | 30 | 4.044 | 7.215 | 3.623 | 7.231 | 1 | 4136 | stable |
| `multi_file/default` | 1 | 30 | 7.306 | 8.276 | 5.266 | 8.465 | 0 | 3944 | stable |
| `multi_file/legacy` | 1 | 30 | 3.407 | 4.980 | 2.404 | 5.026 | 0 | 3560 | stable |
| `near_limit/default` | 1 | 5 | 488.176 | 493.694 | 469.316 | 493.694 | 0 | 309748 | stable |
| `near_limit/legacy` | 1 | 5 | 240.765 | 256.574 | 232.105 | 256.574 | 0 | 85356 | stable |
| `diagnostic_flood/default` | 1 | 20 | 179.782 | 189.801 | 163.277 | 189.936 | 0 | 38504 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 49.220 | 52.988 | 46.000 | 54.735 | 0 | 16768 | stable |
| `preprocess/default` | 1 | 30 | 15.210 | 23.643 | 12.210 | 24.345 | 0 | 8188 | stable |
| `preprocess/legacy` | 1 | 30 | 11.466 | 18.846 | 10.133 | 21.256 | 0 | 7536 | stable |
| `stdin/default` | 1 | 30 | 2.640 | 4.558 | 2.266 | 4.561 | 0 | 3816 | stable |
| `stdin/ast` | 1 | 30 | 3.048 | 4.740 | 2.371 | 4.755 | 0 | 3924 | stable |
| `stdin/legacy` | 1 | 30 | 3.857 | 4.204 | 2.123 | 4.933 | 0 | 3588 | stable |
| `startup` | 2 | 50 | 1.168 | 1.736 | 0.774 | 1.826 | 0 | 2368 | stable |
| `corpus_small/default` | 2 | 50 | 3.039 | 4.799 | 2.152 | 5.277 | 0 | 3836 | stable |
| `corpus_small/ast` | 2 | 50 | 2.649 | 4.518 | 2.181 | 4.605 | 0 | 3984 | stable |
| `corpus_small/legacy` | 2 | 50 | 3.202 | 4.196 | 2.123 | 4.420 | 0 | 3652 | stable |
| `corpus_small/compare` | 2 | 50 | 4.223 | 5.068 | 2.491 | 5.316 | 0 | 3860 | stable |
| `corpus_medium/default` | 2 | 50 | 3.310 | 4.850 | 2.320 | 6.169 | 0 | 4020 | stable |
| `corpus_medium/ast` | 2 | 50 | 3.143 | 4.754 | 2.537 | 4.863 | 0 | 3888 | stable |
| `corpus_medium/legacy` | 2 | 50 | 2.476 | 4.381 | 2.068 | 4.515 | 0 | 3604 | stable |
| `corpus_medium/compare` | 2 | 50 | 2.825 | 5.279 | 2.507 | 7.134 | 0 | 3920 | stable |
| `corpus_large/default` | 2 | 30 | 2.909 | 4.494 | 2.414 | 4.816 | 0 | 4136 | stable |
| `corpus_large/ast` | 2 | 30 | 4.515 | 6.035 | 2.608 | 6.126 | 0 | 3952 | stable |
| `corpus_large/legacy` | 2 | 30 | 2.525 | 4.396 | 2.199 | 4.506 | 0 | 3800 | stable |
| `corpus_large/compare` | 2 | 30 | 4.710 | 5.927 | 2.934 | 5.952 | 0 | 3992 | stable |
| `corpus_invalid/default` | 2 | 50 | 3.364 | 4.557 | 2.242 | 4.875 | 1 | 3856 | stable |
| `corpus_invalid/ast` | 2 | 50 | 3.685 | 4.793 | 2.447 | 5.164 | 1 | 3832 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 3.793 | 4.260 | 1.992 | 4.526 | 1 | 3560 | stable |
| `corpus_invalid/compare` | 2 | 50 | 3.190 | 4.881 | 2.350 | 5.604 | 1 | 3856 | stable |
| `corpus_dir/default` | 2 | 40 | 8.767 | 12.878 | 7.595 | 14.626 | 1 | 4256 | stable |
| `corpus_dir/ast` | 2 | 40 | 8.735 | 12.675 | 6.628 | 13.182 | 1 | 4264 | stable |
| `corpus_dir/legacy` | 2 | 40 | 4.839 | 8.745 | 3.936 | 13.715 | 1 | 4200 | stable |
| `corpus_dir/compare` | 2 | 40 | 9.910 | 14.645 | 8.651 | 15.437 | 1 | 4280 | stable |
| `many_symbols/default` | 2 | 30 | 46.425 | 58.502 | 35.825 | 58.789 | 0 | 15940 | stable |
| `many_symbols/ast` | 2 | 30 | 47.586 | 60.540 | 35.353 | 61.699 | 0 | 15924 | stable |
| `many_symbols/legacy` | 2 | 30 | 11.852 | 18.394 | 9.660 | 18.594 | 0 | 7788 | stable |
| `call_graph/default` | 2 | 30 | 25.187 | 31.296 | 16.213 | 37.593 | 0 | 11500 | stable |
| `call_graph/legacy` | 2 | 30 | 11.000 | 16.056 | 8.237 | 19.946 | 0 | 6648 | stable |
| `nested_scopes/default` | 2 | 30 | 7.314 | 8.569 | 4.366 | 8.724 | 0 | 4700 | stable |
| `nested_scopes/legacy` | 2 | 30 | 4.728 | 5.955 | 2.942 | 6.179 | 0 | 3840 | stable |
| `malformed/default` | 2 | 30 | 7.114 | 7.736 | 4.479 | 8.173 | 1 | 4464 | stable |
| `malformed/legacy` | 2 | 30 | 6.465 | 7.382 | 3.664 | 7.430 | 1 | 4168 | stable |
| `multi_file/default` | 2 | 30 | 7.368 | 8.354 | 5.026 | 8.621 | 0 | 3928 | stable |
| `multi_file/legacy` | 2 | 30 | 4.389 | 4.882 | 2.698 | 7.103 | 0 | 3688 | stable |
| `near_limit/default` | 2 | 5 | 484.861 | 508.329 | 478.357 | 508.329 | 0 | 309748 | stable |
| `near_limit/legacy` | 2 | 5 | 239.591 | 246.435 | 227.896 | 246.435 | 0 | 85464 | stable |
| `diagnostic_flood/default` | 2 | 20 | 172.193 | 188.615 | 157.943 | 193.648 | 0 | 38524 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 50.464 | 61.481 | 43.835 | 64.025 | 0 | 16764 | stable |
| `preprocess/default` | 2 | 30 | 14.619 | 23.022 | 11.869 | 23.565 | 0 | 8136 | stable |
| `preprocess/legacy` | 2 | 30 | 12.155 | 18.561 | 10.008 | 18.598 | 0 | 7588 | stable |
| `stdin/default` | 2 | 30 | 4.067 | 4.471 | 2.378 | 4.505 | 0 | 3800 | stable |
| `stdin/ast` | 2 | 30 | 3.361 | 4.496 | 2.619 | 4.513 | 0 | 3812 | stable |
| `stdin/legacy` | 2 | 30 | 2.210 | 4.009 | 1.900 | 4.513 | 0 | 3584 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 22.188 | 25.187 | 13.5 |
| `call_graph/legacy` | 11.404 | 11.000 | 3.7 |
| `corpus_dir/ast` | 9.534 | 8.735 | 9.1 |
| `corpus_dir/compare` | 9.843 | 9.910 | 0.7 |
| `corpus_dir/default` | 9.445 | 8.767 | 7.7 |
| `corpus_dir/legacy` | 7.171 | 4.839 | 48.2 |
| `corpus_invalid/ast` | 3.969 | 3.685 | 7.7 |
| `corpus_invalid/compare` | 2.833 | 3.190 | 12.6 |
| `corpus_invalid/default` | 3.217 | 3.364 | 4.5 |
| `corpus_invalid/legacy` | 2.486 | 3.793 | 52.6 |
| `corpus_large/ast` | 3.472 | 4.515 | 30.0 |
| `corpus_large/compare` | 2.958 | 4.710 | 59.2 |
| `corpus_large/default` | 3.038 | 2.909 | 4.4 |
| `corpus_large/legacy` | 2.350 | 2.525 | 7.4 |
| `corpus_medium/ast` | 4.265 | 3.143 | 35.7 |
| `corpus_medium/compare` | 3.744 | 2.825 | 32.5 |
| `corpus_medium/default` | 2.765 | 3.310 | 19.7 |
| `corpus_medium/legacy` | 2.748 | 2.476 | 11.0 |
| `corpus_small/ast` | 3.310 | 2.649 | 24.9 |
| `corpus_small/compare` | 2.867 | 4.223 | 47.3 |
| `corpus_small/default` | 2.763 | 3.039 | 10.0 |
| `corpus_small/legacy` | 3.692 | 3.202 | 15.3 |
| `diagnostic_flood/default` | 179.782 | 172.193 | 4.4 |
| `diagnostic_flood/legacy` | 49.220 | 50.464 | 2.5 |
| `malformed/default` | 7.137 | 7.114 | 0.3 |
| `malformed/legacy` | 4.044 | 6.465 | 59.9 |
| `many_symbols/ast` | 43.734 | 47.586 | 8.8 |
| `many_symbols/default` | 50.102 | 46.425 | 7.9 |
| `many_symbols/legacy` | 10.027 | 11.852 | 18.2 |
| `multi_file/default` | 7.306 | 7.368 | 0.9 |
| `multi_file/legacy` | 3.407 | 4.389 | 28.8 |
| `near_limit/default` | 488.176 | 484.861 | 0.7 |
| `near_limit/legacy` | 240.765 | 239.591 | 0.5 |
| `nested_scopes/default` | 5.386 | 7.314 | 35.8 |
| `nested_scopes/legacy` | 4.974 | 4.728 | 5.2 |
| `preprocess/default` | 15.210 | 14.619 | 4.0 |
| `preprocess/legacy` | 11.466 | 12.155 | 6.0 |
| `stdin/ast` | 3.048 | 3.361 | 10.3 |
| `stdin/default` | 2.640 | 4.067 | 54.1 |
| `stdin/legacy` | 3.857 | 2.210 | 74.5 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
