# AST-default golden provenance (Phase 6H, Issue #103, Slice 0)

These goldens are the **AST engine baseline** captured from the unmodified base
binary. They are deliberately stored under `tests/golden/ast/` and are separate
from the Legacy goldens in `tests/golden/<case>/`.

## Observed capture state

| Fact | Value |
| --- | --- |
| Base commit (`git rev-parse HEAD`) | `ab49154dee10e6e52629bf95c869bf9b33798764` |
| Branch | `exp-preprocessor-ast-phase-06h-ast-default` |
| Base tree (`git rev-parse HEAD^{tree}`) | `5d6764a62cf70e7ac75ecee0b1b383898074dec7` |
| `rustc --version` | `rustc 1.99.1 (58cf8f9c3 2026-10-13)` |
| `cargo --version` | `cargo 1.99.1 (5f94df478 2026-08-27)` |
| Build command | `cargo build --bin neolint` (run from the repository root) |
| Built binary | `target/debug/neolint` |
| Binary SHA-256 | `70ac5d2453d10708a0786c36416887d406eccd4da0f6a1b1c766ab37509e47d6` |

`git status --porcelain --untracked-files=all` was empty at capture time. It
was checked immediately before the build and again immediately after it. The
captures themselves wrote only into the ignored `target/` scratch area, so the
worktree stayed clean while the goldens were produced. The verified scratch
results were then copied verbatim into `tests/golden/ast/`.

## Exact capture commands

Run from the repository root, with the base binary and a scratch output tree:

```sh
BIN="$PWD/target/debug/neolint"
OUT="$PWD/target/ast-golden-capture-1"
mkdir -p "$OUT/plain" "$OUT/preprocess_include" "$OUT/firestorm" \
         "$OUT/stdin" "$OUT/directory" "$OUT/tokens_preprocess"

(cd tests/golden/plain && "$BIN" --engine ast plain.lsl >"$OUT/plain/stdout.txt" 2>"$OUT/plain/stderr.txt"); printf '%s\n' $? >"$OUT/plain/exit.txt"

(cd tests/golden/preprocess_include && "$BIN" --engine ast --preprocess -I inc root.lsl >"$OUT/preprocess_include/stdout.txt" 2>"$OUT/preprocess_include/stderr.txt"); printf '%s\n' $? >"$OUT/preprocess_include/exit.txt"

(cd tests/golden/firestorm && "$BIN" --engine ast --dialect firestorm firestorm.lsl >"$OUT/firestorm/stdout.txt" 2>"$OUT/firestorm/stderr.txt"); printf '%s\n' $? >"$OUT/firestorm/exit.txt"

(cd tests/golden/stdin && "$BIN" --engine ast - <input.lsl >"$OUT/stdin/stdout.txt" 2>"$OUT/stdin/stderr.txt"); printf '%s\n' $? >"$OUT/stdin/exit.txt"

(cd tests/golden/directory && "$BIN" --engine ast tree >"$OUT/directory/stdout.txt" 2>"$OUT/directory/stderr.txt"); printf '%s\n' $? >"$OUT/directory/exit.txt"

(cd tests/golden/tokens_preprocess && "$BIN" --engine ast --tokens --preprocess -I inc root.lsl >"$OUT/tokens_preprocess/stdout.txt" 2>"$OUT/tokens_preprocess/stderr.txt"); printf '%s\n' $? >"$OUT/tokens_preprocess/exit.txt"
```

Each `exit.txt` therefore contains the observed process status as a decimal
integer plus a newline, not merely console text. All six observed values are 0
or 1.

## Observed exit statuses

| Case | Working directory | Exit |
| --- | --- | --- |
| `plain` | `tests/golden/plain` | 1 |
| `preprocess_include` | `tests/golden/preprocess_include` | 1 |
| `firestorm` | `tests/golden/firestorm` | 0 |
| `stdin` | `tests/golden/stdin` | 0 |
| `directory` | `tests/golden/directory` | 1 |
| `tokens_preprocess` | `tests/golden/tokens_preprocess` | 0 |

## Input fixture SHA-256

| Fixture | SHA-256 |
| --- | --- |
| `tests/golden/plain/plain.lsl` | `74ea388359ddf4e2a8e2010742721246947aa311ecaf41bbacbebac5b8a7bd7d` |
| `tests/golden/preprocess_include/root.lsl` | `dca8bf02691de8a3b4d648982f2351c379d517a47656589ba51ec7fa9f9d111f` |
| `tests/golden/preprocess_include/inc/defs.lsl` | `064067e1e43e361ec3698364bf4819905500513ba42ba5096f73aede3e1d5ebe` |
| `tests/golden/firestorm/firestorm.lsl` | `f621b0e3efdc8750e738895c727ae8b0ceb7cd69bb7db146a56062c83e1111e9` |
| `tests/golden/stdin/input.lsl` | `11ea01de61bf8580698dddd913684cc0fba968bb3d53197f3a6e8ce1d3fda51d` |
| `tests/golden/directory/tree/a.lsl` | `11ea01de61bf8580698dddd913684cc0fba968bb3d53197f3a6e8ce1d3fda51d` |
| `tests/golden/directory/tree/nested/b.lsl` | `d917cb90c14087ff51da5a61a686b7eaf598fe72d79d2b72ccc28926b1c1f8f4` |
| `tests/golden/tokens_preprocess/root.lsl` | `dca8bf02691de8a3b4d648982f2351c379d517a47656589ba51ec7fa9f9d111f` |
| `tests/golden/tokens_preprocess/inc/defs.lsl` | `360b3c90d4d1e7f293cd24f037e12ec6b36a7682fc31458190d869445d48c66a` |

## Captured artifact SHA-256

| Artifact | SHA-256 |
| --- | --- |
| `plain/stdout.txt` | `66976e4fb535c14a92cacbfd6c812073e8747e401edfb78c99afe2831ff9a52c` |
| `preprocess_include/stdout.txt` | `31bc1969b67f2b4d6db519f1ccd5524a8811b95bf611af6908cf4ad7354363ac` |
| `firestorm/stdout.txt` | `d8cb113eb17a942a1b115686d59cb8b3525e83b06efb14e5ce26ab00a8c156e2` |
| `stdin/stdout.txt` | `f2fe6dd25a06dfd860e3830625bd54a40ec11e7e0c77f1f6ce9b02ae52a3fe56` |
| `directory/stdout.txt` | `0032e2c4bc5263c57c32e6781f6efb5985194bb31c6746e862d1f30ceca01027` |
| `tokens_preprocess/stdout.txt` | `8d6ac0495225443762d371d76f4264b4e1d9e733585b29eb3e4e759e8e48ec59` |
| every `stderr.txt` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (empty) |
| `exit.txt`, status 0 | `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa` (`0\n`) |
| `exit.txt`, status 1 | `4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865` (`1\n`) |

## Expected distinguishing behavior

- `firestorm`: the AST engine accepts `switch` in the Firestorm dialect and
  exits 0 with `firestorm.lsl: no diagnostics`. The separate Legacy golden for
  the same fixture still records the pre-change PAR/SEM diagnostics and exit 1;
  it was not modified by this capture.
- `stdin`: the AST path labels the source `<stdin>`; the Legacy golden labels it
  `-`. The AST golden was captured with `<stdin>` verbatim.
- `plain`, `preprocess_include`, `directory`, and `tokens_preprocess` are
  byte-identical to their Legacy goldens in both streams, but are captured here
  as independent AST artifacts.

## Reproducibility and immutability

The six captures were executed twice, into independent scratch trees
(`target/ast-golden-capture-1` and `target/ast-golden-capture-2`). `diff -r`
reported no differences, and the installed `tests/golden/ast/` tree was compared
against the verified scratch tree with `diff -r` and matched exactly.

These goldens were captured from the unmodified base binary **before any
`src/**` edit**. They must never be regenerated after this commit: they are the
independent AST-default baseline that later Phase 6H slices must satisfy, and
regenerating them from a modified binary would destroy their provenance value.
