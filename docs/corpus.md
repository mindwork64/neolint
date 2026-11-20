# LSL regression corpus

## Purpose

The corpus exercises neolint against complete, project-authored LSL scripts rather than isolated unit-test fragments. It checks parser-level validity, semantic-negative diagnostics, and recursive directory processing deterministically and offline. `syntax` and `category` are independent: a syntax-valid fixture may still be a semantic-negative example.

The corpus does **not** claim to be an official LSL compiler suite. Expected results describe the current merged implementation, not every behavior of Second Life or another simulator.

## Layout

```text
corpus/
  manifest.txt
  small/      focused scripts for individual language areas
  medium/     multi-function and multi-state scripts
  large/      broader state/event/function integration and smoke cases
  invalid/    malformed programs with explicit diagnostic expectations
tests/corpus.rs
```

## Snapshot

* 29 fixtures total.
* 9 small, 6 medium, 3 large, 11 invalid.
* 29 original project-authored fixtures.
* 0 third-party fixtures.

Fixtures marked `diagnostics=none` are expected to be clean. Syntax-valid semantic-negative fixtures may intentionally emit SEM diagnostics. The eleven `category=invalid` fixtures cover:

* `PAR006` — empty `for` condition;
* `PAR007` — unclosed state;
* `PAR012` — component access on a non-variable literal;
* `SEM001` — duplicate global declaration;
* `SEM004` — known built-in argument type mismatches;
* `SEM007` — duplicate local declaration;
* `SEM008` — incompatible explicit return expression;
* `SEM009` — duplicate jump label;
* `SEM010` — undefined jump targets;
* `SEM011` — reachable function fallthrough;
* `SEM016` — incompatible plain assignment;
* `SEM018` — invalid vector component write;
* `SEM019` — invalid vector/integer component read;
* `SEM020` — duplicate canonical event handler;
* `SEM021` — duplicate named state or state/global/function namespace collision;
* `SEM022` — undefined state-change target.

## Language coverage

The valid scripts cover default and named states, event handlers, timer/dialog/listen/sensor/inventory/object patterns, user functions, global/local variables, nested scopes and shadowing, lists, strings, keys, vectors, rotations, arithmetic/logical/comparison/bitwise expressions, casts, compound and component assignments, `if`/`else`, `for`/`while`/`do-while`, jump labels, built-in and user-function calls, hex literals, and unknown-extension boundaries.

The invalid fixtures exercise representative parser, symbol, type, component and CFG failures. They intentionally do not attempt to enumerate every possible diagnostic.

## Manifest

`corpus/manifest.txt` is a compact pipe-delimited manifest. Each non-comment line has ten fields:

```text
id|category|path|origin|license|syntax|diagnostics|exit|limitations|purpose
```

* `category` is `small`, `medium`, `large` or `invalid`; `invalid` denotes a deliberately negative example, not a size.
* `syntax` is parser-level `valid` or `invalid`; it is independent of expected semantic diagnostics. A syntax-valid fixture may still intentionally emit SEM diagnostics.
* `diagnostics` is `none` or comma-separated `CODE:COUNT:SEVERITY` entries.
* `exit` is the expected CLI exit code (`0` or `1`).
* `origin` and `license` are mandatory. The current corpus uses `original` and `project-authored`.

`tests/corpus.rs` validates unique IDs, paths, required metadata, syntax/diagnostic consistency, missing or unlisted `.lsl` files, and exact diagnostic counts/severity through real CLI invocations.

## Running

Run the corpus tests only:

```sh
cargo test --test corpus
```

Run the full project suite:

```sh
cargo test --all-targets --all-features
```

The directory test invokes the actual `neolint` binary recursively and compares the aggregate manifest result. It also requires every fixture path to appear in output. The runner enforces a 10-second per-fixture timeout and a 20-second recursive-directory timeout; this is a completion smoke test, not a microbenchmark.

## Licensing and provenance

All fixtures were written specifically for neolint and are marked `original` / `project-authored`. No third-party script is included. The project's own source license is still pending; no third-party material may be added until its redistribution terms and compatibility with the eventual project license are reviewed and recorded in the manifest and adjacent notices.

## Adding a fixture

1. Add the `.lsl` file under the appropriate corpus directory.
2. Add exactly one manifest row with a unique ID, category, provenance, license, syntax classification, expected diagnostics, exit code, limitations and purpose.
3. Keep expected results deterministic and limited to implemented behavior.
4. Run `cargo test --test corpus` and inspect any mismatch by fixture ID.
5. Run the full required CI before opening a PR.

Do not add network fetching, generated wall-clock assertions, or third-party material with unclear redistribution rights.
