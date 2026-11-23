# neolint CLI benchmark — Phase 8 baseline

- harness revision: `37731ffa0f53a8ecf04fd57ffb9f33a426d8ced7` (tree `6b8b84fc2dfc476d7d174ddd579759ad1ab2a458`), branch `exp-preprocessor-ast-phase-08-stabilization`
- measured binary: built from `1273c4d325428c9fa95a49c9122bfc99ae5d9e34` (tree `73d77147843f2b6945ddb25bb0cb12b7b9db8cd4`), sha256 `227352d90b1ed1233046e6e7073726c43a354352e94c1e3ac06cc7509843babc`, digest matches the expected value; a fresh build of the same commit reproduced the same digest
- benchmark inputs: 70 files, 8764087 bytes (per-file hashes in `meta.json`)
- rustc `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, cargo `cargo 1.99.1 (5f94df478 2026-08-27)`
- host: Linux-7.0.0-43-generic-x86_64-with-glibc2.39 / x86_64 / AMD Ryzen 5 2600X Six-Core Processor / 12 threads
- binary path: `/home/eloraam/apps/neolint/target/release/neolint` (1675040 bytes)
- warm-up: 3 iterations per row; two independent passes
- RSS: peak VmHWM polled from /proc, 3 untimed samples per row

All figures are **end-to-end CLI** measurements and include process startup.
Plain `--engine legacy` is a direct no-preprocess path; AST always runs an
identity-prepared unit; `--engine compare` runs both analyzers. Ratios
between those modes are not parser-only comparisons.

| Row | Pass | Samples | Median ms | p95 ms | Min ms | Max ms | Exit | RSS KB | Digests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `startup` | 1 | 50 | 1.359 | 1.494 | 0.789 | 1.632 | 0 | 1944 | stable |
| `corpus_small/default` | 1 | 50 | 3.961 | 4.287 | 2.511 | 4.501 | 0 | 3928 | stable |
| `corpus_small/ast` | 1 | 50 | 3.871 | 4.205 | 3.027 | 4.951 | 0 | 3876 | stable |
| `corpus_small/legacy` | 1 | 50 | 3.507 | 3.768 | 2.579 | 3.787 | 0 | 3456 | stable |
| `corpus_small/compare` | 1 | 50 | 4.001 | 4.258 | 2.296 | 4.456 | 0 | 3844 | stable |
| `corpus_medium/default` | 1 | 50 | 4.104 | 4.381 | 2.424 | 4.730 | 0 | 3900 | stable |
| `corpus_medium/ast` | 1 | 50 | 3.987 | 4.352 | 3.859 | 4.822 | 0 | 3924 | stable |
| `corpus_medium/legacy` | 1 | 50 | 3.639 | 4.183 | 2.773 | 4.251 | 0 | 3580 | stable |
| `corpus_medium/compare` | 1 | 50 | 4.244 | 4.666 | 4.083 | 4.746 | 0 | 3912 | stable |
| `corpus_large/default` | 1 | 30 | 4.153 | 4.519 | 4.018 | 4.611 | 0 | 3996 | stable |
| `corpus_large/ast` | 1 | 30 | 4.271 | 4.649 | 4.095 | 4.776 | 0 | 4068 | stable |
| `corpus_large/legacy` | 1 | 30 | 3.770 | 4.208 | 2.670 | 4.300 | 0 | 3564 | stable |
| `corpus_large/compare` | 1 | 30 | 4.476 | 4.977 | 3.382 | 5.056 | 0 | 3976 | stable |
| `corpus_invalid/default` | 1 | 50 | 3.955 | 4.441 | 2.417 | 4.840 | 1 | 3908 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.941 | 4.233 | 3.819 | 4.291 | 1 | 3844 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 3.522 | 3.878 | 2.480 | 3.960 | 1 | 3568 | stable |
| `corpus_invalid/compare` | 1 | 50 | 4.096 | 4.533 | 3.894 | 4.700 | 1 | 3968 | stable |
| `corpus_dir/default` | 1 | 40 | 12.225 | 13.249 | 7.632 | 13.464 | 1 | 4240 | stable |
| `corpus_dir/ast` | 1 | 40 | 11.319 | 12.647 | 8.159 | 12.718 | 1 | 4288 | stable |
| `corpus_dir/legacy` | 1 | 40 | 6.967 | 7.549 | 4.035 | 8.264 | 1 | 4092 | stable |
| `corpus_dir/compare` | 1 | 40 | 14.401 | 15.522 | 10.915 | 15.759 | 1 | 4288 | stable |
| `many_symbols/default` | 1 | 30 | 33.262 | 35.392 | 19.353 | 36.179 | 0 | 16228 | stable |
| `many_symbols/ast` | 1 | 30 | 33.843 | 35.055 | 19.308 | 35.195 | 0 | 16296 | stable |
| `many_symbols/legacy` | 1 | 30 | 9.897 | 17.243 | 9.206 | 17.454 | 0 | 7664 | stable |
| `call_graph/default` | 1 | 30 | 25.554 | 29.304 | 18.694 | 29.528 | 0 | 11832 | stable |
| `call_graph/legacy` | 1 | 30 | 10.014 | 15.669 | 7.957 | 15.772 | 0 | 6568 | stable |
| `nested_scopes/default` | 1 | 30 | 6.497 | 7.453 | 4.952 | 8.234 | 0 | 4772 | stable |
| `nested_scopes/legacy` | 1 | 30 | 4.694 | 5.158 | 2.715 | 5.187 | 0 | 3676 | stable |
| `malformed/default` | 1 | 30 | 6.781 | 7.309 | 4.877 | 7.391 | 1 | 4448 | stable |
| `malformed/legacy` | 1 | 30 | 6.352 | 6.775 | 6.159 | 6.803 | 1 | 4036 | stable |
| `multi_file/default` | 1 | 30 | 7.691 | 8.227 | 6.848 | 8.427 | 0 | 4036 | stable |
| `multi_file/legacy` | 1 | 30 | 4.233 | 4.537 | 4.011 | 4.720 | 0 | 3568 | stable |
| `near_limit/default` | 1 | 5 | 456.207 | 464.564 | 448.954 | 464.564 | 0 | 309744 | stable |
| `near_limit/legacy` | 1 | 5 | 217.220 | 225.596 | 212.597 | 225.596 | 0 | 85324 | stable |
| `diagnostic_flood/default` | 1 | 20 | 87.380 | 97.013 | 77.923 | 100.557 | 0 | 38480 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 48.742 | 52.010 | 43.141 | 52.357 | 0 | 16660 | stable |
| `preprocess/default` | 1 | 30 | 14.805 | 22.538 | 11.967 | 22.690 | 0 | 8264 | stable |
| `preprocess/legacy` | 1 | 30 | 11.942 | 18.032 | 9.545 | 18.369 | 0 | 7568 | stable |
| `stdin/default` | 1 | 30 | 3.785 | 4.266 | 2.027 | 4.341 | 0 | 3992 | stable |
| `stdin/ast` | 1 | 30 | 3.766 | 4.284 | 2.999 | 4.340 | 0 | 3824 | stable |
| `stdin/legacy` | 1 | 30 | 3.527 | 3.878 | 3.383 | 3.896 | 0 | 3468 | stable |
| `startup` | 2 | 50 | 1.425 | 1.578 | 1.360 | 1.703 | 0 | 2088 | stable |
| `corpus_small/default` | 2 | 50 | 3.909 | 4.440 | 3.013 | 4.883 | 0 | 3852 | stable |
| `corpus_small/ast` | 2 | 50 | 3.920 | 4.171 | 3.742 | 4.282 | 0 | 3896 | stable |
| `corpus_small/legacy` | 2 | 50 | 3.570 | 3.856 | 3.444 | 3.988 | 0 | 3472 | stable |
| `corpus_small/compare` | 2 | 50 | 4.216 | 4.621 | 3.917 | 4.802 | 0 | 3988 | stable |
| `corpus_medium/default` | 2 | 50 | 4.052 | 4.447 | 3.873 | 4.470 | 0 | 4076 | stable |
| `corpus_medium/ast` | 2 | 50 | 3.921 | 4.325 | 3.813 | 4.608 | 0 | 3904 | stable |
| `corpus_medium/legacy` | 2 | 50 | 3.619 | 3.904 | 2.268 | 4.454 | 0 | 3460 | stable |
| `corpus_medium/compare` | 2 | 50 | 4.179 | 4.491 | 4.013 | 4.592 | 0 | 3912 | stable |
| `corpus_large/default` | 2 | 30 | 4.267 | 4.660 | 2.935 | 4.750 | 0 | 4044 | stable |
| `corpus_large/ast` | 2 | 30 | 4.152 | 4.524 | 4.024 | 4.560 | 0 | 3960 | stable |
| `corpus_large/legacy` | 2 | 30 | 3.875 | 4.064 | 2.439 | 4.152 | 0 | 3688 | stable |
| `corpus_large/compare` | 2 | 30 | 4.496 | 4.931 | 2.904 | 5.751 | 0 | 4000 | stable |
| `corpus_invalid/default` | 2 | 50 | 2.179 | 2.597 | 2.078 | 2.643 | 1 | 4004 | stable |
| `corpus_invalid/ast` | 2 | 50 | 2.125 | 3.888 | 2.058 | 4.006 | 1 | 3936 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 3.460 | 3.552 | 2.345 | 3.666 | 1 | 3612 | stable |
| `corpus_invalid/compare` | 2 | 50 | 3.927 | 4.217 | 3.863 | 4.317 | 1 | 3852 | stable |
| `corpus_dir/default` | 2 | 40 | 12.118 | 12.470 | 11.441 | 12.579 | 1 | 4252 | stable |
| `corpus_dir/ast` | 2 | 40 | 11.996 | 12.616 | 11.409 | 13.078 | 1 | 4168 | stable |
| `corpus_dir/legacy` | 2 | 40 | 6.797 | 7.105 | 6.691 | 7.190 | 1 | 4152 | stable |
| `corpus_dir/compare` | 2 | 40 | 13.960 | 14.386 | 10.419 | 14.592 | 1 | 4320 | stable |
| `many_symbols/default` | 2 | 30 | 27.878 | 35.270 | 26.979 | 35.627 | 0 | 16272 | stable |
| `many_symbols/ast` | 2 | 30 | 33.188 | 34.903 | 21.405 | 35.164 | 0 | 16264 | stable |
| `many_symbols/legacy` | 2 | 30 | 11.687 | 17.074 | 9.139 | 17.191 | 0 | 7664 | stable |
| `call_graph/default` | 2 | 30 | 27.861 | 28.627 | 21.506 | 28.638 | 0 | 11804 | stable |
| `call_graph/legacy` | 2 | 30 | 9.988 | 10.812 | 8.039 | 10.854 | 0 | 6596 | stable |
| `nested_scopes/default` | 2 | 30 | 3.915 | 4.633 | 3.799 | 4.645 | 0 | 4728 | stable |
| `nested_scopes/legacy` | 2 | 30 | 2.454 | 2.654 | 2.407 | 2.662 | 0 | 3684 | stable |
| `malformed/default` | 2 | 30 | 3.750 | 4.167 | 3.669 | 7.230 | 1 | 4396 | stable |
| `malformed/legacy` | 2 | 30 | 3.384 | 3.707 | 3.290 | 6.835 | 1 | 4048 | stable |
| `multi_file/default` | 2 | 30 | 6.077 | 7.861 | 4.244 | 8.717 | 0 | 4024 | stable |
| `multi_file/legacy` | 2 | 30 | 4.423 | 4.673 | 2.682 | 4.757 | 0 | 3584 | stable |
| `near_limit/default` | 2 | 5 | 473.109 | 489.633 | 464.744 | 489.633 | 0 | 309748 | stable |
| `near_limit/legacy` | 2 | 5 | 223.262 | 236.719 | 221.042 | 236.719 | 0 | 85364 | stable |
| `diagnostic_flood/default` | 2 | 20 | 86.072 | 95.164 | 77.695 | 99.388 | 0 | 38500 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 43.930 | 52.229 | 43.147 | 53.021 | 0 | 16644 | stable |
| `preprocess/default` | 2 | 30 | 16.066 | 22.160 | 13.091 | 22.562 | 0 | 8280 | stable |
| `preprocess/legacy` | 2 | 30 | 9.827 | 15.520 | 9.684 | 17.860 | 0 | 7568 | stable |
| `stdin/default` | 2 | 30 | 2.723 | 4.269 | 2.306 | 4.313 | 0 | 3852 | stable |
| `stdin/ast` | 2 | 30 | 3.941 | 4.153 | 2.814 | 4.195 | 0 | 3872 | stable |
| `stdin/legacy` | 2 | 30 | 3.503 | 3.848 | 3.394 | 3.941 | 0 | 3464 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 25.554 | 27.861 | 9.0 |
| `call_graph/legacy` | 10.014 | 9.988 | 0.3 |
| `corpus_dir/ast` | 11.319 | 11.996 | 6.0 |
| `corpus_dir/compare` | 14.401 | 13.960 | 3.2 |
| `corpus_dir/default` | 12.225 | 12.118 | 0.9 |
| `corpus_dir/legacy` | 6.967 | 6.797 | 2.5 |
| `corpus_invalid/ast` | 3.941 | 2.125 | 85.5 |
| `corpus_invalid/compare` | 4.096 | 3.927 | 4.3 |
| `corpus_invalid/default` | 3.955 | 2.179 | 81.5 |
| `corpus_invalid/legacy` | 3.522 | 3.460 | 1.8 |
| `corpus_large/ast` | 4.271 | 4.152 | 2.9 |
| `corpus_large/compare` | 4.476 | 4.496 | 0.4 |
| `corpus_large/default` | 4.153 | 4.267 | 2.7 |
| `corpus_large/legacy` | 3.770 | 3.875 | 2.8 |
| `corpus_medium/ast` | 3.987 | 3.921 | 1.7 |
| `corpus_medium/compare` | 4.244 | 4.179 | 1.5 |
| `corpus_medium/default` | 4.104 | 4.052 | 1.3 |
| `corpus_medium/legacy` | 3.639 | 3.619 | 0.5 |
| `corpus_small/ast` | 3.871 | 3.920 | 1.3 |
| `corpus_small/compare` | 4.001 | 4.216 | 5.4 |
| `corpus_small/default` | 3.961 | 3.909 | 1.3 |
| `corpus_small/legacy` | 3.507 | 3.570 | 1.8 |
| `diagnostic_flood/default` | 87.380 | 86.072 | 1.5 |
| `diagnostic_flood/legacy` | 48.742 | 43.930 | 11.0 |
| `malformed/default` | 6.781 | 3.750 | 80.8 |
| `malformed/legacy` | 6.352 | 3.384 | 87.7 |
| `many_symbols/ast` | 33.843 | 33.188 | 2.0 |
| `many_symbols/default` | 33.262 | 27.878 | 19.3 |
| `many_symbols/legacy` | 9.897 | 11.687 | 18.1 |
| `multi_file/default` | 7.691 | 6.077 | 26.6 |
| `multi_file/legacy` | 4.233 | 4.423 | 4.5 |
| `near_limit/default` | 456.207 | 473.109 | 3.7 |
| `near_limit/legacy` | 217.220 | 223.262 | 2.8 |
| `nested_scopes/default` | 6.497 | 3.915 | 65.9 |
| `nested_scopes/legacy` | 4.694 | 2.454 | 91.3 |
| `preprocess/default` | 14.805 | 16.066 | 8.5 |
| `preprocess/legacy` | 11.942 | 9.827 | 21.5 |
| `stdin/ast` | 3.766 | 3.941 | 4.7 |
| `stdin/default` | 3.785 | 2.723 | 39.0 |
| `stdin/legacy` | 3.527 | 3.503 | 0.7 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
