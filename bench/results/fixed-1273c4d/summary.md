# neolint CLI benchmark — Phase 8 baseline

- harness revision: `a0fea1ea5ce73557a8e925efc34c50528fdd6a5d` (tree `8264d70e02802de0fe57e4d49e2a662c8086eeef`), branch `exp-preprocessor-ast-phase-08-stabilization`, worktree clean
- harness source digest: `babcf808592bf1803849ff1e4313dca8f334f7964d128587230186808347efe3` (over bench/bench.py, bench/workloads.py)
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
| `startup` | 1 | 50 | 1.415 | 1.663 | 0.782 | 1.842 | 0 | 2400 | stable |
| `corpus_small/default` | 1 | 50 | 2.647 | 4.037 | 2.174 | 4.734 | 0 | 3852 | stable |
| `corpus_small/ast` | 1 | 50 | 3.027 | 4.463 | 2.187 | 4.501 | 0 | 3852 | stable |
| `corpus_small/legacy` | 1 | 50 | 2.972 | 4.321 | 2.042 | 4.888 | 0 | 3456 | stable |
| `corpus_small/compare` | 1 | 50 | 4.103 | 4.532 | 2.743 | 4.938 | 0 | 3872 | stable |
| `corpus_medium/default` | 1 | 50 | 3.249 | 5.741 | 2.383 | 6.459 | 0 | 3928 | stable |
| `corpus_medium/ast` | 1 | 50 | 3.286 | 4.326 | 2.621 | 4.419 | 0 | 3904 | stable |
| `corpus_medium/legacy` | 1 | 50 | 3.712 | 5.298 | 2.077 | 6.378 | 0 | 3508 | stable |
| `corpus_medium/compare` | 1 | 50 | 2.633 | 4.631 | 2.340 | 5.455 | 0 | 3936 | stable |
| `corpus_large/default` | 1 | 30 | 4.410 | 4.735 | 4.226 | 4.801 | 0 | 3984 | stable |
| `corpus_large/ast` | 1 | 30 | 4.439 | 4.688 | 4.215 | 4.889 | 0 | 4000 | stable |
| `corpus_large/legacy` | 1 | 30 | 3.947 | 4.182 | 3.824 | 4.399 | 0 | 3484 | stable |
| `corpus_large/compare` | 1 | 30 | 4.565 | 4.947 | 3.345 | 5.061 | 0 | 3996 | stable |
| `corpus_invalid/default` | 1 | 50 | 4.016 | 4.418 | 2.624 | 5.330 | 1 | 3864 | stable |
| `corpus_invalid/ast` | 1 | 50 | 3.578 | 4.369 | 2.463 | 4.809 | 1 | 3832 | stable |
| `corpus_invalid/legacy` | 1 | 50 | 3.683 | 4.295 | 2.343 | 4.699 | 1 | 3496 | stable |
| `corpus_invalid/compare` | 1 | 50 | 4.102 | 4.404 | 2.673 | 4.513 | 1 | 3996 | stable |
| `corpus_dir/default` | 1 | 40 | 8.498 | 12.316 | 7.158 | 12.876 | 1 | 4272 | stable |
| `corpus_dir/ast` | 1 | 40 | 8.647 | 13.025 | 6.775 | 16.777 | 1 | 4276 | stable |
| `corpus_dir/legacy` | 1 | 40 | 4.122 | 7.796 | 3.906 | 8.075 | 1 | 4060 | stable |
| `corpus_dir/compare` | 1 | 40 | 10.166 | 14.744 | 8.050 | 15.174 | 1 | 4300 | stable |
| `many_symbols/default` | 1 | 30 | 29.237 | 34.292 | 23.336 | 36.829 | 0 | 16228 | stable |
| `many_symbols/ast` | 1 | 30 | 31.034 | 35.126 | 19.949 | 38.586 | 0 | 16272 | stable |
| `many_symbols/legacy` | 1 | 30 | 11.717 | 17.487 | 9.838 | 17.917 | 0 | 7664 | stable |
| `call_graph/default` | 1 | 30 | 23.374 | 34.226 | 16.338 | 35.303 | 0 | 11856 | stable |
| `call_graph/legacy` | 1 | 30 | 10.734 | 16.041 | 8.499 | 16.399 | 0 | 6608 | stable |
| `nested_scopes/default` | 1 | 30 | 4.823 | 8.195 | 4.208 | 8.429 | 0 | 4812 | stable |
| `nested_scopes/legacy` | 1 | 30 | 3.141 | 5.364 | 2.730 | 5.663 | 0 | 3668 | stable |
| `malformed/default` | 1 | 30 | 5.993 | 8.377 | 4.447 | 8.935 | 1 | 4388 | stable |
| `malformed/legacy` | 1 | 30 | 5.775 | 7.165 | 3.896 | 7.465 | 1 | 3968 | stable |
| `multi_file/default` | 1 | 30 | 6.203 | 8.891 | 4.957 | 9.033 | 0 | 3980 | stable |
| `multi_file/legacy` | 1 | 30 | 2.999 | 4.868 | 2.644 | 4.967 | 0 | 3576 | stable |
| `near_limit/default` | 1 | 5 | 498.235 | 522.393 | 485.878 | 522.393 | 0 | 309736 | stable |
| `near_limit/legacy` | 1 | 5 | 251.760 | 254.827 | 233.476 | 254.827 | 0 | 85308 | stable |
| `diagnostic_flood/default` | 1 | 20 | 89.626 | 95.329 | 81.242 | 96.200 | 0 | 38528 | stable |
| `diagnostic_flood/legacy` | 1 | 20 | 49.411 | 62.097 | 44.992 | 69.784 | 0 | 16612 | stable |
| `preprocess/default` | 1 | 30 | 13.296 | 22.221 | 12.039 | 24.056 | 0 | 8252 | stable |
| `preprocess/legacy` | 1 | 30 | 9.993 | 18.453 | 9.850 | 18.553 | 0 | 7460 | stable |
| `stdin/default` | 1 | 30 | 3.764 | 4.081 | 2.142 | 4.178 | 0 | 3968 | stable |
| `stdin/ast` | 1 | 30 | 3.826 | 4.247 | 2.640 | 4.566 | 0 | 3748 | stable |
| `stdin/legacy` | 1 | 30 | 2.519 | 3.679 | 2.368 | 3.680 | 0 | 3500 | stable |
| `startup` | 2 | 50 | 1.418 | 1.754 | 0.812 | 1.807 | 0 | 2184 | stable |
| `corpus_small/default` | 2 | 50 | 3.936 | 4.947 | 2.271 | 5.291 | 0 | 3840 | stable |
| `corpus_small/ast` | 2 | 50 | 2.980 | 4.255 | 2.177 | 4.282 | 0 | 3884 | stable |
| `corpus_small/legacy` | 2 | 50 | 3.850 | 4.133 | 2.080 | 4.185 | 0 | 3580 | stable |
| `corpus_small/compare` | 2 | 50 | 4.111 | 4.485 | 3.060 | 4.976 | 0 | 3864 | stable |
| `corpus_medium/default` | 2 | 50 | 4.189 | 4.617 | 3.045 | 5.194 | 0 | 4060 | stable |
| `corpus_medium/ast` | 2 | 50 | 4.180 | 4.558 | 3.098 | 5.604 | 0 | 3980 | stable |
| `corpus_medium/legacy` | 2 | 50 | 3.696 | 3.912 | 2.330 | 4.003 | 0 | 3428 | stable |
| `corpus_medium/compare` | 2 | 50 | 4.303 | 4.678 | 2.804 | 4.727 | 0 | 3904 | stable |
| `corpus_large/default` | 2 | 30 | 3.448 | 4.806 | 3.118 | 5.404 | 0 | 4028 | stable |
| `corpus_large/ast` | 2 | 30 | 3.765 | 4.851 | 2.800 | 4.934 | 0 | 3992 | stable |
| `corpus_large/legacy` | 2 | 30 | 4.050 | 4.629 | 2.881 | 6.762 | 0 | 3540 | stable |
| `corpus_large/compare` | 2 | 30 | 4.819 | 4.921 | 4.500 | 4.932 | 0 | 4112 | stable |
| `corpus_invalid/default` | 2 | 50 | 3.950 | 4.395 | 2.569 | 6.648 | 1 | 3868 | stable |
| `corpus_invalid/ast` | 2 | 50 | 2.269 | 3.478 | 2.129 | 3.793 | 1 | 3820 | stable |
| `corpus_invalid/legacy` | 2 | 50 | 2.180 | 3.942 | 1.982 | 4.138 | 1 | 3596 | stable |
| `corpus_invalid/compare` | 2 | 50 | 4.025 | 4.333 | 2.319 | 4.813 | 1 | 4016 | stable |
| `corpus_dir/default` | 2 | 40 | 11.187 | 12.759 | 7.669 | 12.944 | 1 | 4284 | stable |
| `corpus_dir/ast` | 2 | 40 | 10.917 | 12.334 | 8.141 | 12.504 | 1 | 4240 | stable |
| `corpus_dir/legacy` | 2 | 40 | 7.003 | 7.575 | 3.923 | 7.641 | 1 | 4064 | stable |
| `corpus_dir/compare` | 2 | 40 | 13.499 | 15.148 | 9.067 | 15.525 | 1 | 4296 | stable |
| `many_symbols/default` | 2 | 30 | 33.005 | 41.517 | 19.593 | 42.100 | 0 | 16276 | stable |
| `many_symbols/ast` | 2 | 30 | 23.308 | 33.725 | 19.917 | 36.916 | 0 | 16320 | stable |
| `many_symbols/legacy` | 2 | 30 | 11.858 | 13.559 | 11.573 | 15.533 | 0 | 7568 | stable |
| `call_graph/default` | 2 | 30 | 21.483 | 30.547 | 15.530 | 31.105 | 0 | 11804 | stable |
| `call_graph/legacy` | 2 | 30 | 8.870 | 15.809 | 8.114 | 15.851 | 0 | 6568 | stable |
| `nested_scopes/default` | 2 | 30 | 6.961 | 7.195 | 5.684 | 7.476 | 0 | 4720 | stable |
| `nested_scopes/legacy` | 2 | 30 | 4.631 | 5.860 | 3.266 | 6.526 | 0 | 3644 | stable |
| `malformed/default` | 2 | 30 | 6.742 | 7.228 | 3.946 | 7.961 | 1 | 4444 | stable |
| `malformed/legacy` | 2 | 30 | 6.328 | 6.844 | 6.182 | 6.848 | 1 | 4008 | stable |
| `multi_file/default` | 2 | 30 | 7.478 | 7.929 | 7.196 | 7.938 | 0 | 3960 | stable |
| `multi_file/legacy` | 2 | 30 | 3.003 | 4.915 | 2.350 | 5.503 | 0 | 3520 | stable |
| `near_limit/default` | 2 | 5 | 476.486 | 502.482 | 472.502 | 502.482 | 0 | 309740 | stable |
| `near_limit/legacy` | 2 | 5 | 246.244 | 259.194 | 228.907 | 259.194 | 0 | 85328 | stable |
| `diagnostic_flood/default` | 2 | 20 | 93.044 | 99.730 | 78.834 | 107.253 | 0 | 38528 | stable |
| `diagnostic_flood/legacy` | 2 | 20 | 44.836 | 52.762 | 44.006 | 53.430 | 0 | 16616 | stable |
| `preprocess/default` | 2 | 30 | 13.582 | 22.491 | 12.280 | 26.162 | 0 | 8260 | stable |
| `preprocess/legacy` | 2 | 30 | 10.592 | 19.736 | 10.083 | 20.985 | 0 | 7504 | stable |
| `stdin/default` | 2 | 30 | 3.942 | 4.272 | 2.901 | 4.899 | 0 | 3752 | stable |
| `stdin/ast` | 2 | 30 | 2.443 | 3.990 | 2.161 | 4.109 | 0 | 3852 | stable |
| `stdin/legacy` | 2 | 30 | 3.538 | 3.902 | 2.469 | 3.972 | 0 | 3504 | stable |

## Two-pass variation (median)

| Row | Pass 1 ms | Pass 2 ms | Delta % |
| --- | --- | --- | --- |
| `call_graph/default` | 23.374 | 21.483 | 8.8 |
| `call_graph/legacy` | 10.734 | 8.870 | 21.0 |
| `corpus_dir/ast` | 8.647 | 10.917 | 26.2 |
| `corpus_dir/compare` | 10.166 | 13.499 | 32.8 |
| `corpus_dir/default` | 8.498 | 11.187 | 31.6 |
| `corpus_dir/legacy` | 4.122 | 7.003 | 69.9 |
| `corpus_invalid/ast` | 3.578 | 2.269 | 57.7 |
| `corpus_invalid/compare` | 4.102 | 4.025 | 1.9 |
| `corpus_invalid/default` | 4.016 | 3.950 | 1.6 |
| `corpus_invalid/legacy` | 3.683 | 2.180 | 69.0 |
| `corpus_large/ast` | 4.439 | 3.765 | 17.9 |
| `corpus_large/compare` | 4.565 | 4.819 | 5.6 |
| `corpus_large/default` | 4.410 | 3.448 | 27.9 |
| `corpus_large/legacy` | 3.947 | 4.050 | 2.6 |
| `corpus_medium/ast` | 3.286 | 4.180 | 27.2 |
| `corpus_medium/compare` | 2.633 | 4.303 | 63.4 |
| `corpus_medium/default` | 3.249 | 4.189 | 28.9 |
| `corpus_medium/legacy` | 3.712 | 3.696 | 0.4 |
| `corpus_small/ast` | 3.027 | 2.980 | 1.6 |
| `corpus_small/compare` | 4.103 | 4.111 | 0.2 |
| `corpus_small/default` | 2.647 | 3.936 | 48.7 |
| `corpus_small/legacy` | 2.972 | 3.850 | 29.5 |
| `diagnostic_flood/default` | 89.626 | 93.044 | 3.8 |
| `diagnostic_flood/legacy` | 49.411 | 44.836 | 10.2 |
| `malformed/default` | 5.993 | 6.742 | 12.5 |
| `malformed/legacy` | 5.775 | 6.328 | 9.6 |
| `many_symbols/ast` | 31.034 | 23.308 | 33.1 |
| `many_symbols/default` | 29.237 | 33.005 | 12.9 |
| `many_symbols/legacy` | 11.717 | 11.858 | 1.2 |
| `multi_file/default` | 6.203 | 7.478 | 20.6 |
| `multi_file/legacy` | 2.999 | 3.003 | 0.1 |
| `near_limit/default` | 498.235 | 476.486 | 4.6 |
| `near_limit/legacy` | 251.760 | 246.244 | 2.2 |
| `nested_scopes/default` | 4.823 | 6.961 | 44.3 |
| `nested_scopes/legacy` | 3.141 | 4.631 | 47.4 |
| `preprocess/default` | 13.296 | 13.582 | 2.1 |
| `preprocess/legacy` | 9.993 | 10.592 | 6.0 |
| `stdin/ast` | 3.826 | 2.443 | 56.6 |
| `stdin/default` | 3.764 | 3.942 | 4.7 |
| `stdin/legacy` | 2.519 | 3.538 | 40.4 |

## Notes

- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.
- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.
- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.
- Timing is never a pass/fail threshold in CI.
