# Diagnostic reference

This page describes the diagnostic codes emitted by neolint. Severity and kind are defaults; configuration may disable a code or change its severity.

## Reading diagnostics

Human output has the form:

```text
path:line:column: severity[code]: message
```

Lines and columns are one-based. JSON additionally exposes zero-based UTF-8 byte offsets (`start`, `end`) and end positions. See [cli-reference.md](cli-reference.md#json-output).

## Lexical diagnostics

All lexical diagnostics have default severity **error** and kind **lexical**.

| Code | Meaning | Limitation |
| --- | --- | --- |
| `LEX000` | Unexpected character outside comments/strings. | Identifiers are ASCII-only; Unicode outside strings/comments is rejected. |
| `LEX001` | Unterminated string literal. | The diagnostic points at the opening string. |
| `LEX002` | Unterminated block comment. | The diagnostic points at the opening `/*`. |

Examples: `integer café;`, `"unterminated`, `/* unterminated`.

## Parser/syntax diagnostics

All parser diagnostics have default severity **error** and kind **syntax**.

| Code | Meaning | Typical example |
| --- | --- | --- |
| `PAR001` | Expected a global declaration or state definition. | `void f() {}` |
| `PAR003` | Expected a parameter type. | `integer f(void) {}` |
| `PAR004` | Expected an event handler inside a state. | `default { integer x; }` |
| `PAR005` | Expected an expression after a binary operator. | `integer x = 1 + ;` |
| `PAR006` | Expected an expression; also used for an empty `for` condition. | `if () {}`, `for (;;) ;` |
| `PAR007` | Required token missing or unexpected. | Missing `;`, `)`, `}`, or a malformed event header. |
| `PAR008` | Typed declaration in a `for` initializer. | `for (integer i = 0; ...)` |
| `PAR009` | Global declaration/function after state definitions. | `default {} integer x;` |
| `PAR010` | Invalid assignment target. | `1 = 2;` |
| `PAR011` | Invalid increment/decrement target. | `1++;`, `vector.x++` |
| `PAR012` | Component access on a non-variable. | `<1,2,3>.x`, `llGetPos().x` |
| `PAR013` | Missing, late, or out-of-order default state. | empty file, `state x {}` before `default` |
| `PAR014` | Duplicate default state. | `default {} default {}` |

For malformed input, neolint attempts to continue far enough to report additional independent problems. Recovery is best-effort: unbalanced outer braces can affect which surrounding construct receives a diagnostic.
### AST parser diagnostics

The independent `parse_ast` parser mirrors the `PARxxx` syntax contract on the
same prepared token stream. Its only additional code is the explicit recursion
guard:

| Code | Meaning | Notes |
| --- | --- | --- |
| `AST001` | AST recursion depth exceeds 256 recursive constructs. | Error severity, syntax kind. Emitted once per parse invocation; the over-deep branch is skipped and later independent constructs are still parsed when delimiters permit. |
| `AST002` | The bounded AST parser worker stack could not be created or terminated safely. | Error severity, syntax kind. The parser returns an empty AST and never retries the recursive parser on the caller's stack. |

`AST001` is intentionally AST-only. The legacy parser has no equivalent guard;
all ordinary syntax findings remain `PARxxx` and are compared by the differential
harness.

## Preprocessor diagnostics

All preprocessor diagnostics have kind **syntax**. `PRE037` is informational;
the rest default to **error**. Their `primary` and `related` locations are original-file
`SourceSpan`s in the diagnostic's own `AnalysisUnit`; they never pass through
the prepared-span map. Related locations identify the relevant macro
definition, previous definition, outer invocation, conditional opening, or EOF.

| Code | Meaning | Related/notes |
| --- | --- | --- |
| `PRE001` | Unknown preprocessor directive. | Primary is the directive line. |
| `PRE002` | Malformed or empty directive, including unexpected tokens after `#else`/`#endif`. | Primary is the directive line; conditional recovery still matches the frame. |
| `PRE003` | Invalid `#define` (missing name, malformed parameter list, duplicate parameter). | Primary is the definition name/line. |
| `PRE004` | Invalid `#undef` (missing, extra, or non-identifier operand). | Primary is the directive/operand. |
| `PRE005` | Conflicting macro redefinition. | Related points at the previous definition; the old definition remains active. |
| `PRE006` | Invalid `#if`/`#elif` expression syntax. | Primary is the expression/token; related is the conditional opening. |
| `PRE007` | Missing or invalid `#ifdef`/`#ifndef` identifier. | Related is the conditional opening. |
| `PRE008` | Duplicate `#else`. | Primary is the second `#else`. |
| `PRE009` | `#elif` after `#else`. | Primary is the misplaced `#elif`. |
| `PRE010` | `#elif` without a matching `#if`. | Primary is the `#elif`. |
| `PRE011` | `#else` without a matching `#if`. | Primary is the `#else`. |
| `PRE012` | `#endif` without a matching `#if`. | Primary is the `#endif`. |
| `PRE013` | Unterminated conditional block at EOF. | Primary is the opening directive; related is the EOF point. |
| `PRE014` | Wrong number of arguments for a function-like macro. | Related is the macro definition; the invocation is left unexpanded. |
| `PRE015` | Unterminated function-like macro invocation. | Related is the macro definition; the invocation is left unexpanded. |
| `PRE017` | Direct macro recursion. | Primary is the recursive invocation; related includes the definition and outer invocations. |
| `PRE018` | Indirect macro recursion. | Primary is the recursive invocation; related includes the definition and outer invocations. |
| `PRE019` | Macro expansion depth exceeds 64. | Primary is the invocation that would exceed the limit. |
| `PRE020` | Expanded-output byte budget (8 MiB) exceeded. | The affected expansion stops; the rest of the unit continues within the budget. |
| `PRE021` | Expanded-output token budget (1,000,000) exceeded. | The affected expansion stops; the rest of the unit continues within the budget. |
| `PRE022` | Preprocessor diagnostic limit (1,000) reached. | This is the final PRE diagnostic; later findings are suppressed. |
| `PRE025` | Conditional nesting depth exceeds 64. | The over-deep frame is treated as inactive and still matched by `#endif`. |
| `PRE026` | Division or remainder by zero in an evaluated expression. | The expression evaluates as false; unselected short-circuit branches are not diagnosed. |
| `PRE027` | Checked `i64` arithmetic, literal, negation, division, or shift overflow. | The expression evaluates as false. |
| `PRE028` | Invalid shift: negative left operand, negative count, or count at least 64 in an evaluated expression. | The expression evaluates as false. |
| `PRE029` | Malformed decimal/octal/hexadecimal integer literal. | The literal is treated as zero so parsing can continue. |
| `PRE030` | Missing `#if`/`#elif` expression. | Primary is a zero-length point at the directive end; related is the conditional opening. |
| `PRE031` | Invalid `#include` syntax or forbidden remote/network include path. | Primary is the path spelling; related is the full directive. |
| `PRE032` | Included file not found or unreadable (including invalid UTF-8/non-regular file). | Primary is the path spelling; related is the full directive. |
| `PRE033` | Include cycle on the active include stack. | Primary is the include path; related is the previous entry and the directive. |
| `PRE034` | Include depth exceeds 64. | Primary is the include path; related is the include-stack context. |
| `PRE035` | Included source exceeds 4 MiB or the unit's 256-file limit. | Primary is the path spelling; related is the directive. |
| `PRE036` | Unknown `#pragma` (other than `#pragma once`). | Primary is the pragma spelling/directive. |
| `PRE037` | Firestorm `//fspreprocessor off` kill switch was found. | Severity **info**; primary is the exact marker range; never affects exit code. |
| `PRE038` | Invalid `#`/`##` use, operands, or token-paste result. | Primary is the invocation; the affected expansion is left bounded. |
| `PRE039` | Total rendered diagnostics limit (10,000) reached for an AnalysisUnit or a unit-based CLI invocation. | Terminal error diagnostic; later findings are suppressed. A CLI overflow removes unit-level PRE039 terminals and appends exactly one terminal PRE039. |

`PRE016` remains intentionally unused; malformed invocation classes are covered by
`PRE014` and `PRE015`.

`PRE039` is emitted only by unit-based invocations: the default no-preprocess AST
path (identity preparation), explicit `--engine ast`/`--engine compare`, and
full preprocessing (`--preprocess` or `--dialect firestorm`) for every engine.
Only explicit `--engine legacy` without preprocessing uses the uncapped direct
plain-Legacy path and never emits `PRE039`.

## Semantic and lint diagnostics

All `SEM` diagnostics have kind **semantic** except `SEM013` and `SEM023`, which are **lint**. `SEM012`, `SEM013` and `SEM023` default to **warning**; the rest default to **error**.

| Code | Meaning | Default | Important boundary |
| --- | --- | --- | --- |
| `SEM001` | Duplicate top-level variable/function declaration. | error | First declaration location is included where available. |
| `SEM002` | Wrong argument count for a known built-in function. | error | Unknown extension functions are not checked. |
| `SEM003` | Wrong parameter count/type for a known event handler. | error | Known canonical events only. |
| `SEM004` | Provable built-in argument type mismatch. | error | Unresolved expressions are skipped, not guessed. |
| `SEM005` | Wrong argument count for a user-defined function. | error | — |
| `SEM006` | Provable argument type mismatch for a user-defined function. | error | Unresolved expressions are skipped. |
| `SEM007` | Duplicate variable declaration in one lexical scope. | error | Nested shadowing is allowed. |
| `SEM008` | Invalid explicit return presence/type. | error | Only explicit returns are checked. |
| `SEM009` | Duplicate jump label in one function/event scope. | error | Labels in different code scopes are independent. |
| `SEM010` | Undefined or inaccessible jump target. | error | Jumping into a nested block is rejected. |
| `SEM011` | Value-returning function may reach the end without returning. | error | Conservative analysis; complex or malformed control flow may not be checked. |
| `SEM012` | Unresolved variable reference. | warning / semantic | Unknown extension calls and unknown extension event bodies are skipped. Warnings do not fail unless escalated. |
| `SEM013` | Call to a canonical deprecated LSL function. | warning / lint | SLua-only metadata and replacement hints are excluded. |
| `SEM014` | Unsupported explicit cast. | error | Only known source/target type pairs are checked. |
| `SEM015` | Incompatible global/local initializer type. | error | `integer`→`float` and `string`→`key` are accepted; unresolved values are skipped. |
| `SEM016` | Incompatible plain assignment type. | error | Compound assignments are SEM017; component writes are SEM018. |
| `SEM017` | Invalid arithmetic compound-assignment operand/result. | error | Unresolved RHS values are skipped. |
| `SEM018` | Invalid vector/rotation component write. | error | Unknown bases and unresolved values are skipped. |
| `SEM019` | Invalid vector/rotation component read. | error | Component writes belong to SEM018. |
| `SEM020` | Duplicate canonical event handler within one state. | error | Unknown extension event names are ignored. |
| `SEM021` | Duplicate named state or state/global/function/built-in namespace collision. | error | God-only built-ins are excluded; `default` duplication remains PAR014. |
| `SEM022` | Undefined state-change target in a known event handler. | error | Function state changes and unknown extension event bodies are not checked. |
| `SEM023` | Standalone comparison result is discarded. | warning / lint | Only comparison expression statements are checked; calls, assignments, increment/decrement, used comparisons and ambiguous/syntax-error cases are skipped. |

## Illustrative minimal examples

```lsl
// SEM001
integer value;
integer value;
default { state_entry() { } }
```

```lsl
// SEM002: llOwnerSay expects one argument
default { state_entry() { llOwnerSay(); } }
```

```lsl
// SEM004: integer where string is required
default { state_entry() { llOwnerSay(42); } }
```

```lsl
// SEM011: missing return
integer f(integer value) { if (value > 0) return 1; }
default { state_entry() { } }
```

```lsl
// SEM012: unresolved variable warning
default { state_entry() { llOwnerSay(missing_name); } }
```

```lsl
// SEM020: duplicate event handler
default
{
    state_entry() { }
    state_entry() { }
}
```

```lsl
// SEM021: duplicate named state
default { }
state running { }
state running { }
```

```lsl
// SEM022: undefined state target
default { state_entry() { state missing; } }
```

```lsl
// SEM023: comparison result discarded; assignment was likely intended
default { state_entry() { integer RStat; RStat == FALSE; } }
```

```lsl
// SEM003: wrong event parameter count
default { touch_start() { } }
```

```lsl
// SEM005: wrong user-function argument count
integer add(integer value) { return value; }
default { state_entry() { add(); } }
```

```lsl
// SEM006: provable user-function argument type mismatch
integer add(integer value) { return value; }
default { state_entry() { add("text"); } }
```

```lsl
// SEM007: duplicate local in one scope
default { state_entry() { integer value; integer value; } }
```

```lsl
// SEM008: incompatible return expression
integer f() { return "text"; }
default { }
```

```lsl
// SEM009: duplicate jump label
integer f() { @again; @again; return 1; }
default { }
```

```lsl
// SEM010: undefined jump target
integer f() { jump missing; return 1; }
default { }
```

```lsl
// SEM013: deprecated canonical function
default { state_entry() { llSoundPreload("sound"); } }
```

```lsl
// SEM014: unsupported explicit cast
default { state_entry() { vector position; integer value = (integer)position; } }
```

```lsl
// SEM015: incompatible initializer
default { state_entry() { integer value = 1.5; } }
```

```lsl
// SEM016: incompatible plain assignment
default { state_entry() { integer value; value = 1.5; } }
```

```lsl
// SEM017: unsupported compound assignment operands
default { state_entry() { integer value; value += "text"; } }
```

```lsl
// SEM018: invalid component write
default { state_entry() { integer value; value.x = 1.0; } }
```

```lsl
// SEM019: invalid component read
default { state_entry() { integer value; float result = value.x; } }
```

## Conservative boundaries

- Unknown simulator extensions and extension APIs are not treated as known LSL definitions.
- Unknown function calls and their arguments are not reported as definite errors.
- Unknown extension event bodies are excluded from relevant semantic checks.
- Unresolved types and values are not assigned guessed types.
- Some complex or malformed control-flow cases are not analyzed.
- SEM023 intentionally skips comparisons containing calls or side effects and statements with syntax errors.
- Parser recovery does not promise full reconstruction after unbalanced outer braces.
- A clean lint result does not guarantee acceptance by the official compiler or any simulator.

## Related references

- [Language support](language-support.md)
- [CLI reference](cli-reference.md)
- [Troubleshooting](troubleshooting.md)
