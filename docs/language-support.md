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

## Current limitations

The initial release implements lexical analysis only. It does not yet parse scripts, validate event signatures, resolve names, check types, or apply lint rules. A clean lexer result is not a claim that the whole script is valid LSL.
