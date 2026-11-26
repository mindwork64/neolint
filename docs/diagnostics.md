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

All preprocessor diagnostics keep kind **syntax** for compatibility. In JSON v1
preprocessor-origin PRE diagnostics additionally carry
`"origin":"preprocessor"`; the CLI-generated `PRE039` aggregate-cap marker
does not. This distinguishes actual preprocessor findings without extending
the existing `kind` values. `PRE037` is
informational; the rest default to **error**. Their `primary` and `related`
locations are original-file `SourceSpan`s in the diagnostic's own
`AnalysisUnit`; they never pass through the prepared-span map. Related
locations identify the relevant macro definition, previous definition, outer
invocation, conditional opening, or EOF, and are exposed in the JSON `related`
array with their file, label, byte range and line/column positions.

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

All SEM diagnostics have kind semantic except SEM013, SEM023, SEM024,
SEM025, SEM026, SEM030–SEM033, SEM035, SEM037, SEM040–SEM044, and SEM048,
which are lint. SEM012, SEM013, SEM023, SEM024–SEM026, SEM030–SEM033,
SEM035, SEM037, SEM040–SEM044, and SEM048 default to warning. SEM047,
SEM049, and SEM050 are AST-only semantic errors. The frozen Legacy/Compare
parity range remains SEM001–SEM023.


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
| `SEM011` | Value-returning function may reach the end without returning. | error | AST-only CFG folds bounded finite integer/float expressions and propagates definitely known local integer/float values with a forward worklist to a fixed point. Safe scalar increments, arithmetic compound assignments, single nested writes and direct assignment/increment condition results are tracked when provable; ambiguous multiple writes invalidate only touched locals. Constant Firestorm switch dispatch uses scalar/composite equality (including string/key, vector/rotation and list-length equality); local flow facts can select numeric cases. Source-order fallthrough is preserved. Unknown/unsupported/over-budget labels, incompatible SEM029 case types, and multiple SEM028 defaults keep all possible arms reachable. Per-environment facts, bounded expression scans, worklist iterations, edge visits, switch dispatch and cumulative fact processing are bounded; budget exhaustion suppresses conclusions for that body. Mutable globals/parameters and call results remain unknown. |
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
| `SEM024` | Variable is declared but never read. | warning / lint | Syntactic read/use analysis, not runtime liveness. Plain assignment targets and call callees are not reads; compound assignments, increments/decrements and component reads are. Emits nothing when the unit has AST parser diagnostics, recovered function signatures, error nodes, missing/unclosed function/event/state bodies, or error-severity preprocessor/lexical diagnostics. |
| `SEM025` | Parameter is declared but never read. | warning / lint | Syntactic read/use analysis over user-function and canonical event parameters. Unknown extension-event parameters and malformed/recovered parameters are excluded. Emits nothing when the unit has AST parser diagnostics, recovered function signatures, error nodes, missing/unclosed function/event/state bodies, or error-severity preprocessor/lexical diagnostics. |
| `SEM026` | User function is never called from an event handler or global initializer. | warning / lint | Reachability over matched direct identifier calls only; built-in callees, indirect calls and calls without a closing parenthesis create no edges. A function called only from another unreachable function is still reported. Prototypes, recovered signatures and duplicate declarations after the first definition are never nodes. Suppressed by the same fail-closed conditions as SEM024/SEM025 (AST parser diagnostics, recovered signatures, error nodes, missing/unclosed bodies, or error-severity preprocessor/lexical diagnostics) and by budget overflow. |
| `SEM027` | Global initializer is not a simple assignable value. | error / semantic | AST-only. Checks global declarations, not locals. Accepts literals, visible global variables, built-in constants, numeric vector/rotation components and flat list values. Rejects operators, calls, parenthesized expressions, nested list literals and non-numeric vector/rotation components. Malformed/Error expressions, an unreliable lexical/preprocessor environment, and resource-limit exhaustion are skipped. |
| `SEM028` | A Firestorm switch has more than one `default` label. | error / semantic | AST-only and emitted only when the Firestorm switches gate is enabled. Reports the second `default`; malformed switch bodies are skipped. |
| `SEM029` | A statically typed `case` expression cannot be compared with the switch expression. | error / semantic | AST-only. Uses the existing LSL equality-operator type matrix; unknown types are not guessed. |
| `SEM030` | Two constant `case` expressions have the same value. | warning / lint | AST-only. Constant-folds bounded scalar expressions and recognizes built-in constants; simple vector, rotation, and list literals can also be compared. Unknown/dynamic expressions are skipped. Numeric integer/float values are compared using LSL float coercion. |
| `SEM031` | A Firestorm switch has no `default` label. | warning / lint | AST-only. `default` is optional syntax; this is a completeness warning, not a parser error. |
| `SEM032` | The switch expression is a compile-time constant. | warning / lint | AST-only. Reports once on the switch expression; evaluation is bounded and conservative. |
| `SEM033` | The switch expression has list type. | warning / lint | AST-only. Firestorm switch lowering compares the list value; LSL list equality compares list lengths rather than element contents. |
| `SEM034` | A state change occurs inside a global user-defined function. | error / semantic | AST-only, Second Life Mono target only. The narrow direct `if`-without-`else` form is reported as SEM035 instead. Other target profiles fail closed until their behavior is verified. Malformed statements, unclosed function bodies, unreliable environments and budget overflow are skipped. |
| `SEM035` | A state change relies on the historical Mono `if`-without-`else` workaround. | warning / lint | AST-only, Second Life Mono target only. Matches only a direct unbraced state-change then-branch with no `else`; this is an unsupported compatibility behavior, not portable LSL. Other targets fail closed pending verification. |
| `SEM036` | An event handler returns a value. | error / semantic | AST-only. The compiler may accept this, but the official LSL reference says it crashes at runtime. Applies to canonical and extension event handlers; malformed bodies and unreliable units are skipped. For compatibility, bodies containing jump/label control flow are conservatively skipped for this check. |
| `SEM037` | A statement is unreachable according to the control-flow graph. | warning / lint | AST-only. Uses the bounded shared CFG, including constant conditions, loops, valid jump edges, and Firestorm switch dispatch using supported scalar/composite equality (integer/float, string/key, vector/rotation and list-length equality). Numeric local flow facts can also select cases. Source-order fallthrough/default and break to the innermost switch are preserved; unknown/unsupported/over-budget labels, incompatible SEM029 types and multiple SEM028 defaults preserve all arms. Reports the first unreachable non-empty statement in each straight-line sequence. Invalid jump scopes, parser errors, malformed nodes and budget exhaustion suppress the affected body. |
| `SEM038` | A state has no event handlers. | error / semantic | AST-only. Checks complete `default` and named states; malformed states and states overlapped by parser errors are skipped. |
| `SEM039` | A list element has list type. | error / semantic | AST-only. Checks nested literals, visible list variables, list-returning calls and other expressions whose type is provably `list`. Unknown types and malformed expressions are skipped. |
| `SEM040` | Equality/inequality compares two lists. | warning / lint | AST-only. Reports only when both operand types are known to be `list`. `==` compares lengths only; `!=` returns their length difference. |
| `SEM041` | Bare plain assignment in an `if` condition. | warning / lint | AST-only. Matches a top-level unparenthesized `=` assignment; an extra pair of parentheses marks intentional assignment. Compound assignments and assignments nested inside larger expressions are skipped. |
| `SEM042` | Detection-context-only function is reachable from an incompatible event. | warning / lint | AST-only. Propagates known event contexts through direct user-function calls. `llDetectedGrab` and `llDetectedTouch*` are touch-only, `llDetectedLinkNumber` is restricted to touch/collision events, `llDetectedDamage` requires a damage event, and `llAdjustDamage` requires `on_damage`. Unknown extension events and unreferenced helpers are not guessed; parser errors, unreliable environments and graph-budget overflow suppress the rule. |
| `SEM043` | A math operation has a statically provable domain error. | warning / lint | AST-only. Checks `/`, `%`, `/=` and `%=` for known zero divisors; `llSqrt` with a negative argument; `llPow` with a negative base and known non-integer exponent; and `llAcos`/`llAsin` with known arguments outside `[-1.0, 1.0]`; `llLog`/`llLog10` with known non-positive arguments are also reported because LSL silently returns `0.0` instead of a logarithm. Only valid built-in calls with known numeric arguments are evaluated; user-function shadowing, mutable variables, unknown values and malformed calls are skipped. Invalid inverse-trigonometric results are `NaN` in Mono and trigger a Math Error in legacy LSO. Parser errors, unreliable environments and work/depth budget exhaustion suppress the rule. |
| `SEM044` | A populated named state has no transition path from `default`. | warning / lint | AST-only. Builds a bounded over-approximation from state-change statements in event handlers. Conditional and otherwise unreachable transitions count as possible edges; unknown targets, malformed/duplicate states, parser errors, unreliable environments and budget exhaustion are handled conservatively (4,096 states, 32,768 traversal units and 128 nested statement levels maximum). Empty states remain covered by SEM038. |
| `SEM045` | An operator operand is incompatible or an increment target is not writable. | error / semantic | AST-only. Checks unary `-`, `!`, `~`, logical/bitwise/shift operators, and `++`/`--` on known variables. Unknown operand types are skipped; parser errors, unreliable lexical/preprocessor environments and more than 65,536 expression sites suppress the rule. Built-in constants cannot be incremented or decremented. Unary plus is intentionally outside the rule. |
| `SEM046` | A known binary arithmetic/comparison operator has incompatible operand types. | error / semantic | AST-only. Validates `+`, `-`, `*`, `/`, `%`, equality and relational operators against the shared LSL type-result matrix. Unknown operand types are skipped; parser errors, unreliable lexical/preprocessor environments and more than 65,536 expression sites suppress the rule. List equality remains covered by SEM040 when both operands are lists. |
| `SEM047` | A canonical Second Life LSL function is absent from the selected OpenSim runtime interface. | error / semantic | AST-only target-aware check for opensim-xengine and opensim-yengine. Uses pinned ILSL_Api.cs function-name snapshots; does not infer that an unknown/OSSL call is invalid. User-defined functions shadowing a canonical name are not reported. This checks interface presence, not live region behavior or all API semantic differences. |
| `SEM048` | A known OSSL call is not listed in the project's explicit OpenSim OSSL allowlist. | warning / lint | AST-only. Emitted only for an OpenSim target when ossl_allowlist = strict / --strict-ossl is selected or at least one ossl_function / --ossl-function entry is configured. The allowlist describes what the project expects to be enabled; it does not query region security settings. No list means no OSSL availability assumption. |
| `SEM049` | A canonical Second Life LSL event handler is absent from the selected OpenSim compiler/runtime. | error / semantic | AST-only target-aware check. XEngine uses its own lexer/parser event productions; YEngine uses the event-code enum intersected with handler declarations. Only complete event handlers in reliable input are considered. |
| `SEM050` | A canonical Second Life LSL constant is absent from the selected OpenSim API snapshot. | error / semantic | AST-only target-aware check. Uses the selected version's constants table; local/global variables shadowing the same name are respected. Parser errors and unreliable preprocessing suppress the rule. |

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
// SEM024: local variable is never read
default { state_entry() { integer unused; } }
```

```lsl
// SEM025: parameter is never read
integer unusedParameter(integer value) { return 1; }
default { state_entry() { llOwnerSay((string)unusedParameter(1)); } }
```

```lsl
// SEM026: function is never called from any event or global initializer
unusedFunction() { }
default { state_entry() { } }
```

```lsl
// SEM027: a global initializer cannot call a runtime function
float sampled = llGetTime();
default { state_entry() { } }
```

```lsl
// SEM034: global user-defined functions cannot change state on Mono
change_state() { state running; }
default { state_entry() { } }
state running { state_entry() { } }
```

```lsl
// SEM035: historical Mono compiler bugfeature; not guaranteed valid
// and not portable to LSL-Luau
change_state(integer should_change) { if (should_change) state running; }
default { state_entry() { } }
state running { state_entry() { } }
```

```lsl
// SEM036: event handlers cannot return values
// This can compile but crashes when the return executes.
default { state_entry() { return 1; } }
```

```lsl
// SEM037: the CFG proves this following statement is unreachable
integer value() { return 1; llOwnerSay("never executed"); }
default { state_entry() { } }
```

```lsl
// SEM041: likely assignment/comparison typo
key target;
default { state_entry() { if (target = llGetOwner()) { } } }
```

```lsl
// SEM042: detection data is not available in state_entry
default { state_entry() { llOwnerSay((string)llDetectedKey(0)); } }
```

```lsl
// SEM043: zero divisors, invalid math domains and silent log fallbacks
integer divide(integer value) { return value / (3 - 3); }
default {
    state_entry() {
        integer value = 10;
        value %= FALSE;
        float root = llSqrt(-1.0); // NaN in Mono; Math Error in LSO
        float power = llPow(-2.0, 0.5); // NaN in Mono; Math Error in LSO
        float acos = llAcos(1.5); // NaN in Mono; Math Error in LSO
        float asin = llAsin(-1.5); // NaN in Mono; Math Error in LSO
        float logarithm = llLog(0.0); // LSL returns 0.0
        float logarithm10 = llLog10(-1.0); // LSL returns 0.0
    }
}
```

```lsl
// SEM044: this populated state has no transition path from default
default { state_entry() { } }
state abandoned { state_entry() { llOwnerSay("not reachable from default"); } }
```

```lsl
// SEM045: known-invalid operator operand types
default {
    state_entry() {
        float amount = 1.0;
        string label = "bad";
        integer invalid_not = !amount;
        integer invalid_shift = 1 << label;
        label++;
    }
}
```

```lsl
// SEM046: arithmetic operands must have a supported type combination
default { state_entry() { string text = "x"; integer invalid = text - 1; } }
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
- SEM024 is a syntactic read/use analysis, not runtime liveness: a read counts even when reachable only from a function that later call-graph analysis may prove is never invoked. SEM024 fails closed and emits nothing when the same unit has an error-severity preprocessor or lexical diagnostic, or when syntax errors, recovered function signatures, error nodes, missing bodies or unclosed function/event/state bodies make reads unreliable.
- SEM025 uses the same syntactic read/use model and the same fail-closed gate. Because the scope model registers parameters only for user functions and canonical events, unknown extension-event parameters are excluded structurally rather than by a second policy.
- SEM026 is reachability, not dead-code elimination: roots are matched direct identifier calls in event bodies and global initializers, and edges are matched direct identifier calls in user-function bodies. Built-in callees, indirect calls and calls without a real closing parenthesis never create edges; first-definition duplicate handling follows SEM001. A function called only from another unreachable function is still reported, because this is reachability, not a claim that no call sites exist. The graph is bounded and any node/edge/work overflow emits no SEM026 rather than reporting a partial result; error-severity preprocessor or lexical diagnostics suppress it through the same fail-closed gate as SEM024/SEM025.
- SEM027 is an AST-only semantic error for global initializers that are definitely not simple values. It accepts scalar literals, visible global-variable references, built-in constants, vectors/rotations made from simple numeric components, and flat lists made from simple values. It does not constant-fold operators or function calls. Parenthesized forms and nested list literals are rejected to match the simple-assignable grammar; malformed expressions and inputs that exceed the bounded 128-node/64-depth check, or units with error-severity preprocessor/lexical diagnostics, are skipped.
- SEM028–SEM033 are Firestorm-switch-only AST diagnostics. They are active
  only when the switch feature gate is enabled. The CFG evaluates constant
  switch values and case labels with the same scalar/composite equality rules
  used by those diagnostics: integer/float coercion, string/key equality,
  vector/rotation component equality and list-length equality. Numeric
  switch values can additionally use definite local data-flow facts.
  Source-order fallthrough remains intact. If the value or any label is
  unknown, unsupported or over budget, all possible arms are retained.
  Incompatible SEM029 case types and multiple SEM028 defaults also disable
  dispatch pruning. A known value with no matching case selects default or
  preserves an unmatched path when there is no default. Break targets the
  innermost switch even through loop nesting;
  a nested switch owns its own break target.
- SEM036 reports a value-return from an event handler because the official Second Life Wiki documents that the compiler can accept it even though executing it crashes the script. `return;` remains valid. Parser-damaged bodies and unreliable lexical/preprocessor units are skipped.
- SEM037 uses the shared bounded CFG to report the first unreachable
  non-empty statement in each straight-line sequence, including inside nested
  blocks, conditional branches, loop bodies and switch arms. The forward
  worklist propagates definitely known local numeric values through reachable
  joins, loop back-edges and valid jump/label edges. Safe increments and
  arithmetic compound assignments update facts; ambiguous multiple writes
  invalidate only potentially written locals. Assignment/increment results can
  determine conditions when known. Mutable globals and parameters stay unknown.
  Per-environment facts, side-effect/error-expression scans, worklist
  iterations, CFG edge visits, switch dispatch and cumulative fact processing
  are bounded. Oversized expressions fail closed before recursive flow
  analysis. CFG joins intersect incoming must-states in place instead of
  cloning whole maps for each edge. Constant switch dispatch uses the Firestorm evaluator's integer/float,
  string/key, vector/rotation and list-length equality semantics. Numeric local
  facts can additionally select a case; unknown or unsupported values keep all
  possible arms reachable. Source-order fallthrough and default/no-match rules
  are preserved. Break exits the innermost switch, even through nested loops;
  nested switches shadow the
  enclosing target. Invalid jump scopes, duplicate/missing labels, syntax
  errors, error nodes, unclosed blocks and budget exhaustion suppress SEM037
  for the affected body. SEM036 keeps its conservative jump/label suppression.
- SEM041 follows the documented LSLint convention for the common `=`/`==` typo: only a bare plain assignment at the top level of an `if` condition is warned. Wrapping the assignment in an additional pair of parentheses, as in `if ((target = llGetOwner()))`, explicitly marks it as intentional. Compound assignments, assignments inside larger expressions, and other control-flow conditions are outside this rule. Parser-damaged conditions and unreliable preprocessor/lexical environments are skipped.
- SEM042 propagates known event contexts through matched direct user-function calls. General `llDetected*` functions require a detection event; `llDetectedGrab` and `llDetectedTouch*` are touch-only; `llDetectedLinkNumber` is restricted to touch/collision events; `llDetectedDamage` is limited to `on_damage`/`final_damage`; and `llAdjustDamage` is `on_damage`-only. Unknown extension events contribute no guessed context, unreferenced functions are ignored, and the check fails closed for parser errors, unreliable environments or call-graph budget overflow.
- SEM043 reports valid numeric `/`, integer `%`, `/=`, or `%=` operations with a provably zero divisor. It also reports `llSqrt` with a known negative argument, `llPow` with a known negative base plus a known non-integer exponent, and `llAcos`/`llAsin` with known arguments outside `[-1.0, 1.0]`. `llLog`/`llLog10` with known non-positive arguments are reported separately because LSL returns `0.0` instead of a logarithm. The bounded evaluator handles literals, unshadowed numeric built-in constants, parentheses, unary numeric operators, numeric casts and side-effect-free constant arithmetic. It does not evaluate mutable variables or user-defined function calls; built-in shadowing, invalid signatures and malformed calls are respected. Exponents outside the signed-integer range or with non-finite values remain unknown. Vector `%` remains a cross product. Imaginary results are `NaN` in Mono and a Math Error in legacy LSO. Any parser error, unreliable lexical/preprocessor environment or evaluator budget/depth exhaustion suppresses the rule.
- SEM044 builds a conservative state-transition graph starting at `default` and warns on populated named states with no path from that root. Every state-change statement in a valid event body is treated as a possible edge, even when nested in a conditional or textually unreachable code; this favors false negatives over false positives. Transitions inside global functions are not used because those are separately diagnosed by SEM034/SEM035. Empty states are left to SEM038. The rule skips duplicate/malformed state sets, parser errors, unreliable environments and budget/depth exhaustion. This is structural reachability from `default`, not a model of a script restored from an inventory snapshot in a previously active state.
- SEM045 validates only operator/type combinations with confirmed LSL constraints: unary `-` accepts integer/float/vector/rotation, `!` and `~` require integer, logical/bitwise/shift operators require two integers, and increment/decrement require a known integer/float variable. Unknown values are not guessed. Built-in constants are not writable. Unary plus is intentionally excluded. Any parser error, unreliable lexical/preprocessor environment or more than 65,536 expression sites suppresses the rule.
- SEM046 validates ordinary arithmetic and comparison operators only when both operand types are known. It reuses the shared LSL result-type matrix, so accepted vector/rotation arithmetic, string/list concatenation, mixed numeric operations and implicit string/key equality conversions stay aligned with expression inference. Unknown values are not guessed; parser errors, unreliable input or expression-site budget exhaustion suppress the rule.
- Parser recovery does not promise full reconstruction after unbalanced outer braces.
- A clean lint result does not guarantee acceptance by the official compiler or any simulator.

## Related references

- [Language support](language-support.md)
- [CLI reference](cli-reference.md)
- [Troubleshooting](troubleshooting.md)
