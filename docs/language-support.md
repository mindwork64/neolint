# LSL language support

This document records the initial language model used by neolint. It is intentionally conservative: the official compiler remains authoritative, and ambiguous constructs must not be turned into confident diagnostics.

## Source structure

An LSL script contains global declarations and user-defined functions, plus state definitions. Every script must define a `default` state. Other states are introduced with `state name`. A state-change statement targets either a named state or the default state: `state running;` and `state default;` are both valid. Event handlers are declared inside states and contain ordinary statement blocks. Global declarations and user-defined functions must appear before state definitions; a later global declaration is rejected with `PAR009`. The `default` state must appear first and only once; a missing default, including a script with no states, is rejected with `PAR013`; a duplicate default is rejected with `PAR014`. Functions with no return value omit the return type. See [State](https://wiki.secondlife.com/wiki/State) for the state-change grammar. The detailed grammar inventory and test matrix are in [lsl-grammar.md](lsl-grammar.md); compiler context restrictions and the unresolved historical workaround are documented in [state-change-contexts.md](state-change-contexts.md).

## Built-in value types

The LSL Portal lists seven value types:

- `integer`
- `float`
- `string`
- `key`
- `list`
- `vector`
- `rotation`

A user-defined function that returns no value omits its return type entirely; do not write `void` before the function name. This is not an eighth value type. Parameter lists use only the seven LSL value types; a function with no parameters uses empty parentheses `()`, not a C-style `(void)` marker.

## Control flow

The reference includes `if`, `else`, `for`, `while`, `do-while`, `jump`, `return`, and state changes. A `for` initializer and increment are expression lists (comma-separated); the condition is a single required expression, so `for (;;)` is rejected with `PAR006`. Variable declarations in the initializer are rejected with `PAR008`; declare the loop variable in the enclosing block before the loop (for example, `integer i; for (i = 0; i < 10; ++i)`). Parser coverage must be fixture-driven. Empty statements (`;`) are accepted. The C-style ternary conditional operator (`?:`) is not part of LSL; the parser reports a syntax error rather than accepting it. The logical operators `&&` and `||` have equal precedence and are left-associative, matching the LSL operator reference's `1 || 1 && 0` example. The operator table also assigns distinct precedence levels to subtraction and addition: `*`, `/`, and `%` bind most tightly, then `-`, then `+`. LSL supports only the compound assignments `=`, `+=`, `-=`, `*=`, `/=`, and `%=`; the bitwise/shift forms `&=`, `|=`, `^=`, `<<=`, and `>>=` are invalid and rejected by the parser. Assignment operators require a variable on the left. Ordinary variables and vector/rotation variable components (`.x`, `.y`, `.z`, and rotation `.s`) are accepted; literals, binary expressions, and function-call results are rejected with `PAR010`. Prefix and postfix `++`/`--` require a variable reference; component targets are not accepted for increment/decrement and produce `PAR011`. Assignment targets may be variables or components. Function calls require an identifier target; call suffixes on literals, grouped expressions, component accesses, or call results are rejected with `PAR007`.

## Literals and special syntax

The language includes decimal and `0x`/`0X` hexadecimal integer literals, float literals, quoted strings, list literals, and vector/rotation literals using angle-bracket syntax. Hex literals are reduced to 32 bits and interpreted as signed two's-complement; `e`/`E` remain hex digits. A bare `0x` prefix falls back to decimal `0` and an identifier, matching the pinned lexer. Since `<` and `>` are also relational operators, the parser—not the lexer—must resolve their syntactic role. The token scanner applies the same distinction: `<` after a closing parenthesis starts a vector/rotation literal only when that parenthesis closes an explicit type cast, as in `(vector)<1, 2, 3>`; `(left) < right` remains a comparison. The scanner also treats `<` after `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `==`, or `!=` as a literal start, so a direct vector/rotation literal RHS—including one in a `for` increment—stays one expression. A literal at the start of a `return` expression is recognized the same way, so `return <1, 2, 3>;` is inferred as a vector and SEM008 can compare it with the declared function return type. The complete context matrix, including conservative relational cases, is in [vector-literal-contexts.md](vector-literal-contexts.md). Vector and rotation components are accessible only through declared variables: vector variables support `.x`, `.y`, and `.z`; rotation variables support `.x`, `.y`, `.z`, and `.s`. Each component has type `float`. Component access on literals or function return values is invalid in LSL and is rejected with `PAR012`.

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
- `SEM004`: provable argument type mismatch for a known built-in function. Type inference recognizes literals, built-in constants, explicit casts, list/vector/rotation literals, built-in/user-function call return types, variables resolved through the lexical scope model, and supported binary-expression result types.
- `SEM005`: wrong argument count for a known user-defined function.
- `SEM006`: provable argument type mismatch for a known user-defined function, including supported binary-expression result types.
- `SEM007`: duplicate variable declaration in the same lexical scope, including redeclaration of a function parameter. Shadowing a name in a nested block is allowed.
- `SEM008`: explicit return-statement validation in user-defined functions: return-value presence and provable return-expression type compatibility.
- `SEM009`: duplicate jump labels within a single function or event-handler code scope.
- `SEM010`: jump targets that are missing from the current function/event or inaccessible because they are declared in a nested block.
- `SEM011`: detects value-returning functions that may reach the end without returning a value using a token-derived control-flow graph. It models sequential blocks, `if/else`, `for`, `while`, `do-while`, terminal `return` statements, and forward/backward `jump` edges to labels. Constant integer conditions are evaluated with LSL precedence and 32-bit wrapping arithmetic, including canonical integer constants; division/remainder by zero, out-of-range shifts, expressions with unresolved symbols, and expressions beyond the safe size/depth limits remain unknown and allow both outcomes. Both operands of `&&`/`||` are evaluated; no short-circuit simplification is applied. Invalid jump structures suppress cascading SEM011 diagnostics. Unsupported or malformed constructs cause this rule to skip the function rather than guess.
- `SEM012`: warning when a variable reference cannot be resolved to a declaration in the current lexical scope or to a canonical built-in constant. Unknown extension function calls (including their argument ranges) and bodies of unknown extension event handlers are deliberately ignored. This stays a warning because scripting environments may provide additional symbols.
- `SEM013`: warning for calls to the 28 LSL functions marked with the canonical `deprecated` field in the pinned `secondlife/lsl-definitions/lsl_definitions.yaml`. The extractor ignores `slua-deprecated` metadata and omits upstream `use` hints because some of them refer to SLua APIs, not LSL replacements.
- `SEM014`: error for explicit casts whose known source/target pair is unsupported by the official LSL cast matrix. A dedicated source pass checks casts throughout expressions, including local initializers; casts with unknown source types remain unchecked.
- `SEM015`: error when a global or local variable initializer has a provably incompatible type. The rule respects the only supported implicit conversions (`integer` to `float`, and `string` to `key`) and skips unresolved expressions.
- `SEM016`: error when a plain assignment to a declared variable (`name = expression`) has a provably incompatible type. Assignment targets must be simple identifiers; compound assignments are checked by SEM017, and vector/rotation component assignments by SEM018.
- `SEM017`: checks `+=`, `-=`, `*=`, `/=`, and `%=` on declared identifier variables using the official binary-expression type rules. It reports both known unsupported operand combinations and results that cannot be assigned back to the target variable; unknown RHS expressions remain unchecked.
- `SEM018`: validates writes and arithmetic compound assignments to known vector/rotation components. Vector permits `.x`, `.y`, `.z`; rotation permits `.x`, `.y`, `.z`, `.s`. It reports invalid fields, known unsupported compound operand combinations, and values incompatible with `float`; unknown bases and unresolved RHS expressions remain unchecked.
- `SEM019`: validates reads from variables with known types. Vector reads permit `.x`, `.y`, `.z`; rotation reads permit `.x`, `.y`, `.z`, `.s`. Known invalid pairs (for example, `integer count; count.x`) are errors. Component writes remain exclusively SEM018; unresolved or extension-provided base values are skipped.

The scope model handles function and event parameters, global/local declarations, nested block scopes, shadowing, and lexical access checks for jump labels. Type checks are deliberately conservative: unresolved variables and unknown extension functions are not assigned guessed types. SEM012 reports unresolved variable references as warnings; it skips unknown extension function calls including their arguments, and bodies of unknown extension event handlers. SEM008 validates explicit return statements. SEM011 builds a limited token-derived control-flow graph for supported constructs; it does not depend on a full parser AST.

Built-in function, event, and constant metadata is vendored in `data/lsl-builtins.txt` from the official `secondlife/lsl-definitions` generator output. Canonical deprecated LSL function names are vendored separately in `data/lsl-deprecated.txt`, extracted only from the LSL `deprecated` field; SLua-only metadata is excluded. Both snapshots use the same pinned upstream revision and are refreshed together by `scripts/update-lsl-builtins.sh`. They are licensed; ordinary lint runs do not access the network.

Not implemented yet: complete user-symbol resolution across all scripting extensions, broader CFG coverage for uncommon or malformed control-flow constructs and more precise expression analysis, and the broader correctness/style lint rule set. The current scope and type model is intentionally partial and should not be mistaken for a complete compiler symbol table.

Binary-expression inference covers confirmed operand/result combinations: numeric promotion, string/list concatenation, comparisons/logical results, vector dot/cross products, component-wise rotation addition/subtraction, and direction-sensitive vector/rotation operations. Vector addition/subtraction require two vector operands; scalar scaling is supported for vector multiplication/division. Unknown or incompatible combinations remain unresolved. LSL's implicit conversions are intentionally limited to integer-to-float and string-to-key. SEM004 and SEM006 respect those conversions and do not guess the type of unresolved expressions.

A clean result means only that the implemented checks found no issue. It is not a claim that the official LSL compiler would accept the script.
