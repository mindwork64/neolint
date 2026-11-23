# Firestorm preprocessor profile

Neolint's `--dialect firestorm` profile enables full preprocessing plus the
documented Firestorm preprocessing behaviors that are safe and deterministic to
reproduce. It does **not** enable switch or lazy-list syntax; those remain Phase 5.

## `//fspreprocessor off`

This is a script-wide kill switch, not a regional directive. Neolint performs a
raw substring search over the entire original root text. If the literal
`//fspreprocessor off` appears anywhere, all preprocessing is disabled and the
original bytes are passed to the ordinary LSL pipeline.

| Occurrence | Behavior |
| --- | --- |
| Marker absent | normal preprocessing |
| Marker at the start | whole file unprocessed |
| Marker in the middle | whole file unprocessed, including code before it |
| Marker at the end | whole file unprocessed |
| Marker inside a string literal | whole file unprocessed |
| Marker inside a block comment | whole file unprocessed |

Neolint emits one informational `PRE037` diagnostic at the exact marker range.
It never contributes to exit code 1. The marker has no effect in the standard
LSL preprocessor profile.

## Predefined macros

Only statically known built-ins are provided:

| Macro | Expansion |
| --- | --- |
| `__FILE__` | The owning `SourceFile` display name, escaped as an LSL string. |
| `__LINE__` | One-based line of the invocation in the owning source file. |

Values are computed from the source map's `SourceFile`/`LineIndex`, including
inside included files and macro replacement rescans. The following dynamic
Boost.Wave-style values are intentionally **not** fabricated: `__DATE__`,
`__TIME__`, `__TIMESTAMP__`, `__COUNTER__`, `__CLOCK__`. They remain ordinary
unresolved identifiers.

## Includes and pragma once

Firestorm include resolution is filesystem-independent in the viewer, so neolint
documents its own deterministic local policy rather than claiming end-to-end
Firestorm parity. Quoted includes search the including file's directory first,
then configured include directories; angle includes search only configured
include directories. URL/FTP/UNC paths are rejected. See
[cli-reference.md](cli-reference.md) and the architecture document for details.

`#pragma once` is a documented neolint convenience. Boost.Wave supports it, but
Firestorm's inventory-based include hook has not been proven end-to-end, so this
is not part of the guaranteed Firestorm compatibility suite.

### `#pragma once` through the Firestorm include layer

The upstream path is reconstructed from pinned sources rather than observed in a
running viewer:

- **Boost.Wave 1.90.0**, tag `boost-1.90.0`, commit
  `d7aa5a369d8f2faf48c8ea16ba041dfabbf91363`:
  `util/cpp_iterator.hpp:415` records `ctx.get_current_filename()` when the pragma
  is seen, `util/interpret_pragma.hpp:149` does the same from the pragma
  interpreter, `util/cpp_include_paths.hpp:188` stores the key, and
  `cpp_iterator.hpp:1598` suppresses an include when
  `ctx.has_pragma_once(native_path_str)` matches — where `native_path_str` is the
  path produced by `locate_include_file`. `cpp_context.hpp:407-412` defines both
  accessors. The pragma-once code is line-identical to the host's Boost 1.83, so
  the contract is stable across those versions.
- **Firestorm**, `FirestormViewer/phoenix-firestorm` commit
  `7e44160d001b719d17ee93816425c570974b9a8b`, `indra/newview/fslslpreproc.cpp`:
  `trace_include_files::found_include_directive` (`:559`) resolves the include
  spelling through the inventory (`:563`); when the asset is not in the run's
  cache it starts an async fetch (`:599`) and returns `true` (`:610`), which makes
  Boost.Wave skip the file for that pass. The callback writes the decoded asset to
  `<LL_PATH_CACHE>/lslpreproc/<name>` (`:698`, `:743`), restarts preprocessing
  (`:754`), and the same directory is registered as an include path (`:1312`).

The resulting trace is: Wave asks `found_include_directive` → inventory lookup →
async fetch and defer → cache write under the item name → restart → Wave resolves
the include from `<cache>/lslpreproc/` → `#pragma once` records the path Wave
opened → a later include of the same spelling resolves to the same path and is
suppressed. Because Firestorm caches each inventory item under its own name in a
single directory, both the recorded and the looked-up keys are that same path
string, so `#pragma once` is effective across repeated includes of an item.

**What this does and does not prove.** It is a source-level reconstruction plus an
isolated reproduction of neolint's own policy; no Firestorm viewer was built or
executed, so viewer integration remains unverified.

neolint keys the guard on the canonicalised resolved file
(`src/preprocessor/directive.rs:477`, `:623`), not on the path string Wave
opened. For ordinary files the two agree. The documented deviations are:

- *Spelling variants.* `x.lsl` and `./x.lsl` canonicalise to one file, so neolint
  suppresses the second include; Boost.Wave keys on the path string and would
  treat them as distinct. neolint is stricter here by design.
- *Resolution model.* neolint resolves includes on the filesystem (including
  file's directory first, then configured directories, angle includes only in
  configured directories); Firestorm resolves through inventory names. neolint's
  policy is deterministic and local, not viewer parity.
- *Restarts.* Firestorm restarts preprocessing after each cache miss, so the
  pragma-once set is per preprocessing run; neolint preprocesses a unit once, so
  the set is per unit. Within a unit the suppression result is the same.

The neolint Firestorm profile is covered by
`firestorm_profile_pragma_once_suppresses_repeated_includes_in_a_cache_layout`
(cache-style directory, nested and diamond includes, repeated include) and
`firestorm_profile_pragma_once_covers_spelling_variants_through_canonical_paths`
(deviation above).

## Switch and lazy-list syntax

Phase 3's preprocessor does not rewrite `switch`/`case`/`default`/`break` or
lazy-list indexing. Phase 5 adds independent effective gates and a separate AST
representation of the original source constructs in `parse_ast_with_options`.
The CLI runs the AST engine by default on the experimental Phase 6H branch, so
these constructs are parsed when the gates are on; explicit `--engine legacy`
runs the Legacy engine, which does not parse them. The gates are
driven by `PreprocessSettings` and the active `USE_SWITCHES`/`USE_LAZY_LISTS`
macros, with `//fspreprocessor off` taking priority.

## Not implemented

Dynamic built-ins, GNU comma-swallowing, `__VA_OPT__`, source-to-source switch or
lazy-list rewriting, Firestorm-specific file enumeration, viewer inventory, and
viewer optimizer/compression transforms are outside Phase 5.
