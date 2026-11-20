# LSL language support

This document records the initial language model used by neolint. It is intentionally conservative: the official compiler remains authoritative, and ambiguous constructs must not be turned into confident diagnostics.

## Source structure

An LSL script contains global declarations and user-defined functions, plus state definitions. Every script has a `default` state. Other states are introduced with `state name`. Event handlers are declared inside states and contain ordinary statement blocks. User-defined functions must appear before state definitions; functions with no return value omit the return type.

## Built-in value types

The LSL Portal lists seven value types:

- `integer`
- `float`
- `string`
- `key`
- `list`
- `vector`
- `rotation`

A user-defined function that returns no value omits its return type entirely; do not write `void` before the function name. This is not an eighth value type.

## Control flow

The reference includes `if`, `else`, `for`, `while`, `do-while`, `jump`, `return`, and state changes. A `for` initializer is an expression; variable declarations in the initializer are rejected with `PAR008`. Declare the loop variable in the enclosing block before the loop (for example, `integer i; for (i = 0; i < 10; ++i)`). Parser coverage must be fixture-driven.

## Literals and special syntax

The language includes integer and float literals, quoted strings, list literals, and vector/rotation literals using angle-bracket syntax. Since `<` and `>` are also relational operators, the parser—not the lexer—must resolve their syntactic role.

## Comments

Both line comments (`//`) and block comments (`/* ... */`) are accepted.

## Canonical library metadata

Built-in functions, constants, and related metadata should come from the official `secondlife/lsl-definitions` repository. Its `lsl_definitions.yaml` is the LSL source; `slua_definitions.yaml` describes a different language and must not be merged into the LSL symbol table.

## Lexer implementation notes

The lexer preserves UTF-8 byte offsets and one-based line/column positions. Unicode is accepted in comments and string contents. Identifiers are currently ASCII-only; unsupported non-ASCII characters outside comments/strings receive a lexical diagnostic until verified against the official language implementation.

## CLI configuration and inputs

The CLI accepts explicit files, stdin (`-`), and directories. Directories are traversed recursively for `.lsl` files, sorted deterministically, with symlinks skipped. Explicit file paths are accepted regardless of extension.

A config file can disable rules or override severity:

```ini
disable = SEM001, SEM003
severity.SEM004 = warning
```

Command-line `--disable` and `--severity RULE=LEVEL` settings override the file settings. Severity levels: `error`, `warning`, `info`.

## Current implementation and limitations

The current CLI performs lexical analysis, syntax validation, and the following conservative semantic checks:

- `SEM001`: duplicate top-level variable/function declarations.
- `SEM002`: wrong argument count for a known built-in LSL function.
- `SEM003`: incorrect parameter count/type for a known LSL event handler.
- `SEM004`: provable argument type mismatch for a known built-in function. Type inference recognizes literals, built-in constants, explicit casts, list/vector/rotation literals, built-in call return types, and variables resolved through the lexical scope model.
- `SEM005`: wrong argument count for a known user-defined function.
- `SEM006`: provable argument type mismatch for a known user-defined function.
- `SEM007`: duplicate variable declaration in the same lexical scope, including redeclaration of a function parameter. Shadowing a name in a nested block is allowed.

The scope model handles function and event parameters, global/local declarations, nested block scopes, and shadowing. Type checks are deliberately conservative: unresolved variables and unknown extension functions are not assigned guessed types and are not reported as undefined symbols.

Built-in function, event, and constant metadata is vendored in `data/lsl-builtins.txt` from the official `secondlife/lsl-definitions` generator output. It is pinned and licensed; ordinary lint runs do not access the network.

Not implemented yet: complete user-symbol resolution, undefined-symbol diagnostics, function return-statement/type validation, deprecation diagnostics, and the broader correctness/style lint rule set. The current scope and type model is intentionally partial and should not be mistaken for a complete compiler symbol table.

LSL's implicit conversions are intentionally limited to integer-to-float and string-to-key. SEM004 and SEM006 respect those conversions and do not guess the type of unresolved expressions.

A clean result means only that the implemented checks found no issue. It is not a claim that the official LSL compiler would accept the script.
