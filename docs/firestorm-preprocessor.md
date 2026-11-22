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

## Not implemented

Dynamic built-ins, GNU comma-swallowing, `__VA_OPT__`, Firestorm switch/lazy-list
syntax, Firestorm-specific file enumeration, viewer inventory, and viewer
optimizer/compression transforms are outside Phase 3.
