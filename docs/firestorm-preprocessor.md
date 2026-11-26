# Firestorm preprocessor profile

Neolint's `--dialect firestorm` profile enables full preprocessing plus the
documented Firestorm preprocessing behaviors that are safe and deterministic to
reproduce. Selecting the dialect does **not** turn on switch or lazy-list parsing
by itself: the AST parser accepts those forms only when their independent
Firestorm gates are active, typically through `USE_SWITCHES` and
`USE_LAZY_LISTS`. See [Language support](language-support.md#firestorm-extensions-opt-in).

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

`#pragma once` remains a documented neolint convenience, not part of the
guaranteed Firestorm compatibility suite. The evidence below is separated by how
it was obtained.

#### 1. Observed in neolint tests

neolint keys the guard on the canonicalised resolved file
(`src/preprocessor/directive.rs:477`, `:623`). Two regressions pin the neolint
Firestorm profile (cache-style `<dir>/lslpreproc/` include directory, Firestorm
preprocessing settings):

- `firestorm_profile_pragma_once_suppresses_repeated_includes_in_a_cache_layout`
  — nested, diamond and repeated includes of a `#pragma once` header produce one
  body.
- `firestorm_profile_pragma_once_suppresses_dot_segment_alias` — `once.lsl` and
  `./once.lsl` share one guard because neolint canonicalises the resolved file.
  This is neolint's own filesystem policy; it is not a Firestorm inventory test.

#### 2. Observed in a standalone Boost.Wave 1.90.0 probe

`tools/compat/boost-wave-1.90-pragma-once/` builds the real Boost.Wave 1.90.0
implementation and models Firestorm's inventory/cache hook (exact item-name
lookup, cache-miss deferral, cache write under the item name, restart, then
Wave's ordinary filesystem lookup). Recorded output, source and header hashes are
in
[`RESULTS.md`](../tools/compat/boost-wave-1.90-pragma-once/RESULTS.md) and
[`captured-output.txt`](../tools/compat/boost-wave-1.90-pragma-once/captured-output.txt).

The recorded run executed all eleven scenarios (11 passed, 0 skipped, 0 failed).
The hook writes the cache file synchronously, whereas Firestorm writes it from an
asynchronous asset callback; the probe models the include/cache shape, not its
timing, in-flight duplicate requests or cache invalidation. The dot-segment
scenario deliberately uses a warm, pre-populated cache so it does not depend on
that synchronous write.

| Scenario | Result |
| --- | --- |
| one include of a `#pragma once` header | body once |
| same header without the pragma, twice | body twice |
| direct repeat, nested and diamond includes | body once |
| re-entry after returning to the parent | body once |
| two distinct `#pragma once` headers | each body once |
| cache miss: pass 1 defers, pass 2 opens | pragma-once checked on the cached pass |
| `name.lsl` then `./name.lsl` (warm cache) | inventory stage reports a miss for `./name.lsl`; Wave resolves it to the same cached file and suppresses the second body |
| `./name.lsl` never cached | inventory miss and no file → Wave bad-include error |
| symlink alias of one physical file | body twice |

The Wave headers used for the probe are byte-identical to the ones in the pinned
Firestorm dependency (`secondlife/3p-boost` `v1.90.0-c7a9feb`, linux64 sha1
`6b93e323…`), so the library code matches the pinned dependency; the inventory
and cache layer is a stub, not the viewer.

#### 3. Reconstructed from pinned Firestorm source

- **Boost.Wave 1.90.0**, tag `boost-1.90.0`, commit
  `d7aa5a369d8f2faf48c8ea16ba041dfabbf91363`:
  `util/cpp_iterator.hpp:415` and `util/interpret_pragma.hpp:149` record
  `ctx.get_current_filename()` when the pragma is seen;
  `util/cpp_include_paths.hpp:188` stores the key; `cpp_iterator.hpp:1598`
  suppresses an include when `has_pragma_once(native_path_str)` matches.
  `find_include_file` normalises the successful path with
  `normalize(currpath).string()` (`cpp_include_paths.hpp:339`, `:375`), where
  `normalize` is `lexically_normal().make_preferred()`
  (`util/filesystem_compatibility.hpp:69-76`).
- **Firestorm**, `FirestormViewer/phoenix-firestorm` commit
  `7e44160d001b719d17ee93816425c570974b9a8b`, `indra/newview/fslslpreproc.cpp`:
  `ScriptMatches` compares `item->getName() == mName` and requires
  `AT_LSL_TEXT` (`:57-75`); `findInventoryByName` uses it (`:77-90`);
  `trace_include_files::found_include_directive` (`:559`) resolves the spelling
  through the inventory (`:563`), starts an async fetch on a cache miss (`:599`)
  and returns `true` (`:610`); the callback writes
  `<LL_PATH_CACHE>/lslpreproc/<name>` (`:698`, `:743`) and restarts (`:754`); the
  context is created with the item name (`:1306`) and the cache directory is
  added as an include path (`:1312`).

Two stages must not be conflated: the **Firestorm inventory hook** matches the
include spelling against inventory item names exactly, while **Boost.Wave's
filesystem locator** decides which file is opened and normalises that path
lexically. A `./` spelling can miss the inventory stage and still resolve to the
same file at the Wave stage, as the probe shows.

Documented differences from neolint's policy:

- *Path identity.* neolint canonicalises the resolved file, so symlink and
  physical aliases share one guard. Wave only normalises lexically, so a symlink
  alias is a different key (probe scenario 10). Dot segments are equivalent in
  both because Wave normalises them.
- *Resolution model.* neolint resolves includes on the filesystem (including
  file's directory first, then configured directories; angle includes only in
  configured directories); Firestorm resolves through exact inventory names
  before falling back to Wave's filesystem search. neolint's policy is
  deterministic and local, not viewer parity.
- *Restarts.* Firestorm restarts preprocessing after each cache miss, so the
  pragma-once set is per preprocessing run; neolint preprocesses a unit once, so
  the set is per unit. Within a unit the suppression result is the same.

#### 4. Still unverified

The standalone probe does not execute the Firestorm viewer. It does not model
the inventory service, the asynchronous asset backend, its timing or in-flight
duplicate requests, the UI, or Firestorm's exact cache directory lifetime.
Viewer-level `#pragma once` behaviour therefore remains unverified. Issue #109
was closed after the scoped neolint tests, Boost.Wave probe and pinned-source
reconstruction were completed; that resolution does not claim live Viewer
integration.

## Switch and lazy-list syntax

Phase 3's preprocessor does not rewrite `switch`/`case`/`default`/`break` or
lazy-list indexing. Phase 5 adds independent effective gates and a separate AST
representation of the original source constructs in `parse_ast_with_options`.
The CLI runs the AST engine by default, so these constructs are parsed when the
gates are on; explicit `--engine legacy`
runs the Legacy engine, which does not parse them. The gates are
driven by `PreprocessSettings` and the active `USE_SWITCHES`/`USE_LAZY_LISTS`
macros, with `//fspreprocessor off` taking priority.

### Lazy-list forms confirmed by the pinned source

At Firestorm commit `7e44160d001b719d17ee93816425c570974b9a8b`,
`reformat_lazy_lists()` in `indra/newview/fslslpreproc.cpp` performs two
separate transformations:

- `identifier[index] = rhs` becomes `identifier = lazy_list_set(identifier,
  index, [rhs])`. The matcher excludes `==`, balances nested brackets in the
  index and nested parentheses in the RHS, skips comments and string literals,
  and accepts a semicolon or closing parenthesis as the RHS boundary.
- Typed reads map `integer`, `float`, `string`, `key`, `vector`, `rotation` and
  `list` to their corresponding `llList2*` functions. `quaternion` is an
  additional spelling for `llList2Rot`, not an LSL value type. One optional pair
  of parentheses around the indexed reference is preserved.

The source-level regex can rewrite malformed arities into calls that the LSL
compiler subsequently rejects. NeoLint models valid forms, not every textual
match of that regex: scalar extractions have one index; list extraction accepts
a two-index range only; the one-index list spelling is rejected because the
regex lowers it to `llList2List(src, index)`, while the
[canonical function signature](https://wiki.secondlife.com/wiki/LlList2List)
requires `src, start, end`. More than two indices, bare reads, compound
writes and non-identifier bases are diagnosed. This is intentionally stricter than merely
matching the regex and avoids treating invalid generated calls as valid syntax.

NeoLint keeps these constructs in the AST and does not emulate Firestorm's
source rewriting. The regex patterns were also checked with a one-off C++ probe
compiled against the locally pinned Boost.Regex 1.90 implementation. That probe
is exploratory evidence, not a checked-in test or CI gate; the committed Rust
tests verify NeoLint's parser, feature gates, recovery and source-map behavior.

A separate user-run smoke test in Firestorm-Releasex64 6.6.17.70368 (Mono)
confirmed the supported Lazy List forms in a logged-in Viewer and live region:
the preprocessor output contained the expected `lazy_list_set` and `llList2*`
rewrites, and the script executed with the expected values (42, 2.5, the
expected string/key/vector/rotation values and a range length of 3). The
one-index `(list)items[index]` form became a two-argument `llList2List` call
and was rejected by the simulator compiler. This smoke test covers Lazy Lists;
it does not verify the separate `#pragma once` inventory/cache integration.

## Not implemented

Dynamic built-ins, GNU comma-swallowing, `__VA_OPT__`, source-to-source switch or
lazy-list rewriting, Firestorm-specific file enumeration, viewer inventory, and
viewer optimizer/compression transforms are outside Phase 5.
