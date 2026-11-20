# LSL language support

This document records the initial language model used by neolint. It is intentionally conservative: the official compiler remains authoritative, and ambiguous constructs must not be turned into confident diagnostics.

## Source structure

An LSL script contains global declarations and user-defined functions, plus state definitions. Every script has a `default` state. Other states are introduced with `state name`. Event handlers are declared inside states and contain ordinary statement blocks.

## Built-in value types

The LSL Portal lists seven value types:

- `integer`
- `float`
- `string`
- `key`
- `list`
- `vector`
- `rotation`

`void` is a function return type, not an eighth value type.

## Control flow

The reference includes `if`, `else`, `for`, `while`, `do-while`, `jump`, `return`, and state changes. Parser coverage must be fixture-driven.

## Literals and special syntax

The language includes integer and float literals, quoted strings, list literals, and vector/rotation literals using angle-bracket syntax. Since `<` and `>` are also relational operators, the parser—not the lexer—must resolve their syntactic role.

## Comments

Both line comments (`//`) and block comments (`/* ... */`) are accepted.

## Canonical library metadata

Built-in functions, constants, and related metadata should come from the official `secondlife/lsl-definitions` repository. Its `lsl_definitions.yaml` is the LSL source; `slua_definitions.yaml` describes a different language and must not be merged into the LSL symbol table.

## Lexer implementation notes

The lexer preserves UTF-8 byte offsets and one-based line/column positions. Unicode is accepted in comments and string contents. Identifiers are currently ASCII-only; unsupported non-ASCII characters outside comments/strings receive a lexical diagnostic until verified against the official language implementation.

## Current implementation and limitations

The current CLI performs lexical analysis, syntax validation, and these conservative semantic checks:

- `SEM001`: duplicate top-level variable/function declarations.
- `SEM002`: wrong argument count for a known built-in LSL function.

Built-in function, event, and constant metadata is vendored in `data/lsl-builtins.txt` from the official `secondlife/lsl-definitions` generator output. It is pinned and licensed; ordinary lint runs do not access the network.

Not implemented yet: full user symbol resolution, argument and return type compatibility, event handler signature validation, deprecation diagnostics, and the broader correctness/style lint rule set. Unknown functions are intentionally left alone unless they are present in the built-in database.

A clean result means only that the implemented checks found no issue. It is not a claim that the official LSL compiler would accept the script.
