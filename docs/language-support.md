# LSL language support

Neolint checks common LSL syntax and a defined set of semantic rules. Its analysis is intentionally conservative: the official Second Life compiler remains authoritative, and uncertain language behavior is not treated as a definite error.

## Script structure

An LSL script contains global declarations and user-defined functions followed by state definitions. Every script must have exactly one `default` state, and it must be the first state. Additional states use the `state name` form. Event handlers are declared inside states.

Global declarations and user-defined functions must appear before state definitions. A function that returns no value omits its return type; LSL does not use `void` as a function return type or parameter-list marker.

Every state must contain at least one event handler. Neolint's AST engine
reports `SEM038` for a complete but empty `default` or named state. A state with
malformed event syntax is left to the parser diagnostics rather than receiving
a cascading empty-state error.

A state-change statement can target a named state or the default state:

```lsl
state running;
state default;
```

See the official [LSL State reference](https://wiki.secondlife.com/wiki/State).

## Value types

LSL has seven built-in value types:

- `integer`
- `float`
- `string`
- `key`
- `list`
- `vector`
- `rotation`

User-defined functions can return one of these types or omit a return type when they return no value. Function parameters use the same seven types.

## Statements and expressions

Neolint recognizes blocks, empty statements, local declarations, expression statements, assignments, function calls, `if`/`else`, `for`, `while`, `do-while`, `return`, `jump` labels, and state changes.

### State changes and runtime profiles

A `state target;` statement is valid in an event handler. A top-level
user-defined (global) function is a different context: the Second Life Mono
compiler rejects ordinary state changes from these functions with the
diagnostic `Global functions can't change state`. The AST engine reports
`SEM034` for these placements when the selected target is `second-life-mono`.

The Second Life Wiki documents one historical compiler bugfeature:
`if (condition) state target;` with the state change as the direct, unbraced
then-branch and no `else`. The same page records that this workaround was
described as no longer valid. Neolint reports this narrow shape as `SEM035`,
a warning—not as a supported workaround or portable LSL. Do not use it in new
code; perform state transitions in event handlers.

These two diagnostics are currently enabled only for `second-life-mono`.
They are deliberately suppressed for LSO, LSL-on-Luau, OpenSim XEngine and
OpenSim YEngine until the exact compiler/runtime behavior is verified for each
target. No warning on those targets must not be read as proof that a global
function state change is valid there. Legacy output remains frozen, and
Compare retains its existing `SEM001`–`SEM023` parity contract.

The selected target is independent of `--dialect`. Firestorm controls the
preprocessor dialect and its opt-in parser gates; it is not a runtime. The
supported initial target selectors are `second-life-mono` (default),
`second-life-lso`, `second-life-luau`, `opensim-xengine`, and
`opensim-yengine`.

second-life-luau means LSL compiled to Luau, not the separate SLua language.
OpenSim XEngine is a historical target: OpenSimulator removed it in 0.9.3.0
and uses YEngine in that release line.

The AST analyzer uses versioned OpenSim API snapshots generated from the
upstream compiler/runtime sources and pinned in data/README.md. Each snapshot
includes standard LSL function overloads, OSSL signatures, event handlers and
constants. XEngine uses its own lexer/parser event productions; YEngine uses
its event-code enum intersected with the handler interface.

The snapshots currently cover:
- XEngine API20 (OpenSim 0.9.2.2): 425 standard functions, 258 OSSL
  signatures, 35 events and 770 constants.
- YEngine API20/API22/API23/API24: 425/434/457/473 standard functions,
  258/258/289/296 OSSL signatures, 36/36/37/37 events, and
  770/780/815/888 constants.

Use target_version to select an OpenSim API version. YEngine defaults to the
pinned API24 snapshot; XEngine is available only for API20. The data includes
overloads and language-level type mappings. SEM047 reports canonical SL
functions absent from the selected OpenSim API. SEM049 reports canonical
SL event handlers absent from the selected compiler. SEM050 reports
canonical SL constants absent from the selected constants table. Function
overloads are resolved using arity and known argument types; ambiguous
expression return types remain unknown.

OSSL availability is not inferred from the interface. A function present in
the simulator build may still be disabled on a particular region by security
configuration, threat level, or permission. Enable an explicit project
allowlist with ossl_allowlist = strict / --strict-ossl; repeated
ossl_function = osName / --ossl-function osName entries describe the
functions expected to be enabled. SEM048 warns when a known OSSL call is
outside that list. The rule does not inspect live region permissions.

The selected API catalogue also drives built-in call checks, return-type
inference, event checks, constants, call-graph analysis, state namespace and
reachability. Second Life Mono, LSO and LSL-on-Luau currently share the
canonical Linden Lab API metadata; runtime-specific compiler behavior is
only enabled where independently confirmed. Static catalogue tests pass, but
no connected Second Life Viewer/OpenSim simulator was available for in-world
execution verification.

The rule skips malformed state-change statements, incomplete function bodies,
unreliable lexical/preprocessor units, and inputs exceeding its bounded
traversal budget. References: the
[LSL State documentation](https://wiki.secondlife.com/wiki/State),
[Second Life compiler targets](https://lsl.dev/fundamentals/script-editor/),
[OpenSimulator scripting documentation](https://opensimulator.dev/wiki/Scripting_Documentation),
and the [Mono/LSL-Luau state-change compatibility report](https://feedback.secondlife.com/slua-alpha/p/state-changes-discrepancy-in-lsl-luau-vs-mono).

### Return statements and unreachable code

A bare `return;` is valid in a function or event handler. A value-return such as
`return 1;` inside an event handler is not valid LSL behavior: the compiler may
accept it, but the official reference warns that executing it crashes the
script. Neolint reports this as `SEM036` for canonical and extension event
handlers. Use a value-return only from a user-defined function with a matching
return type.

SEM037 uses Neolint's bounded control-flow graph to report unreachable
statements. In addition to literal and built-in constants, it propagates
definitely known local numeric values from declarations and assignments.
At every reachable CFG join, a value is retained only when all incoming paths
agree; a branch that returns does not constrain the surviving path. A bounded
forward worklist iterates through loop back-edges and valid jump/label edges
to a fixed point. Safe increments and arithmetic compound assignments update
facts; one nested write is tracked through its containing expression, while
ambiguous multiple writes invalidate only touched locals. Assignment and
increment results can determine conditions when known. Mutable globals and
parameters are never assumed constant. Per-environment fact storage, side
effect/error-expression scans, worklist iterations, CFG edge visits, switch
dispatch and total fact-processing work are bounded; oversized expressions or
budget exhaustion suppress conclusions for the affected body. CFG joins
intersect incoming must-states in place to avoid cloning whole fact maps for
each edge.

The CFG also reasons about loop exit paths, valid jump/label edges, and
Firestorm constant switch dispatch. It uses scalar/composite equality for
integer/float, string/key, vector/rotation and list-length values; numeric local
facts can also select cases. When the switch value and every case label are
known, only matching cases are entered and source-order fallthrough is
preserved; a known unmatched value selects default or the no-match exit.
Unknown values, unsupported/over-budget labels, incompatible case types and
multiple default labels retain all possible arms. This prevents invalid
switch syntax/types from becoming false reachability proofs. Break always
targets the innermost switch, even when nested
inside a loop; a nested switch owns its own break target. This allows Neolint
to find dead statements after a switch whose feasible arms all terminate, while
preserving the exit for reachable breaks and unmatched cases.

A transition to a different, unique known state terminates an event handler.
A transition to the current state has no state-change side effect, and unknown
targets are not treated as valid transitions. Invalid jump scopes, malformed
syntax, error nodes, unclosed blocks and analysis-budget exhaustion suppress
the affected body. The check reports only the first unreachable non-empty
statement in each straight-line sequence; it is not a general optimizer or
runtime model.

References: [Second Life Wiki — Return](https://wiki.secondlife.com/wiki/Return),
[Second Life Wiki — State](https://wiki.secondlife.com/wiki/State), and the
[LSL Wiki compiler error list](https://www.lslwiki.digiworldz.com/lslwiki/wakka.php?wakka=errors).

The `for` condition is required. Its initializer and increment clauses may be empty and may contain comma-separated expressions. A variable declaration is not allowed in the initializer; declare the loop variable before the loop:

```lsl
integer i;
for (i = 0; i < 10; ++i)
{
    // loop body
}
```

The C-style ternary operator (`?:`) and bitwise/shift compound assignments (`&=`, `|=`, `^=`, `<<=`, `>>=`) are not part of the supported LSL syntax.

Assignment targets must be variables or supported vector/rotation components. Prefix and postfix `++`/`--` apply to variables, not components.

A plain assignment at the top level of an `if` condition is often a typo for an equality test:

```lsl
if (target = llGetOwner()) // likely meant: target == llGetOwner()
{
    // ...
}
```

The AST engine reports this as the warning `SEM041`. To make an intentional
assignment explicit and silence the warning, put an extra pair of parentheses
around the assignment: `if ((target = llGetOwner()))`. The rule is deliberately
narrow: it checks only bare plain assignments in `if` conditions, not compound
assignments or assignments nested inside larger expressions. This follows the
[Second Life Wiki guidance for the `=`/`==` mistake](https://wiki.secondlife.com/wiki/If#The_=/==_Mistakes).

### Detection functions and event context

The `llDetected*` family reads data supplied by detection events. General
functions such as `llDetectedKey`, `llDetectedName` and `llDetectedPos` are
meaningful in `collision`, `collision_start`, `collision_end`, `sensor`,
`touch`, `touch_start`, `touch_end`, `on_damage` and `final_damage` handlers,
or in user-defined functions called from those handlers. Calling them from an
unrelated event silently produces unusable data.

Some functions have narrower contracts: `llDetectedGrab` and every
`llDetectedTouch*` function are touch-only; `llDetectedLinkNumber` requires a
touch or collision event context; `llDetectedDamage` is available in
`on_damage` and `final_damage`; and `llAdjustDamage` is available only in
`on_damage`.

The AST engine reports an incompatible reachable call as warning `SEM042`.
It propagates contexts through direct user-function calls, so a helper used
only from a touch/collision/sensor/damage handler is accepted, while a helper
also called from `state_entry` is warned. Unknown extension events do not
supply an assumed context, and unreferenced helpers are not diagnosed. Parser
errors, unreliable lexical/preprocessor input and graph-budget exhaustion
suppress the rule rather than producing partial findings.

References: [Second Life Wiki — llDetectedKey](https://wiki.secondlife.com/wiki/LlDetectedKey),
[llDetectedGrab](https://wiki.secondlife.com/wiki/LlDetectedGrab),
[llDetectedLinkNumber](https://wiki.secondlife.com/wiki/LlDetectedLinkNumber),
[llDetectedTouchFace](https://wiki.secondlife.com/wiki/LlDetectedTouchFace),
[llDetectedDamage](https://wiki.secondlife.com/wiki/LlDetectedDamage),
[llAdjustDamage](https://wiki.secondlife.com/wiki/LlAdjustDamage),
[On damage](https://wiki.secondlife.com/wiki/On_damage), and
[Final damage](https://wiki.secondlife.com/wiki/Final_damage).

### Statically provable math-domain hazards

LSL integer/float division and integer remainder by zero cause a runtime
`Math Error`; this includes `/=`, `%=` and vector division by a scalar zero.
The AST engine reports warning `SEM043` when a bounded constant evaluator can
prove the divisor is zero. It also checks `llSqrt`, `llPow`, `llAcos` and `llAsin`:
a negative square-root argument, a negative power base with a non-integer
exponent, or an `llAcos`/`llAsin` argument outside `[-1.0, 1.0]` produces an
imaginary result. The documented runtime difference is important: Mono returns
`NaN`, while legacy LSO triggers a `Math Error`. The logarithm built-ins
`llLog` and `llLog10` are also checked: for arguments `<= 0.0`, LSL returns
`0.0` instead of a logarithm, which can silently corrupt a calculation.

```lsl
integer quotient = 10 / (3 - 3); // SEM043
integer remainder = 10 % FALSE; // SEM043
float negative_zero = 1.0 / -0.0; // SEM043
float invalid_root = llSqrt(-1.0); // SEM043
float invalid_power = llPow(-2.0, 0.5); // SEM043
float invalid_acos = llAcos(1.5); // SEM043
float invalid_asin = llAsin(-1.5); // SEM043
float invalid_log = llLog(0.0); // SEM043: LSL returns 0.0
float invalid_log10 = llLog10(-1.0); // SEM043: LSL returns 0.0
```

The evaluator understands literals, unshadowed numeric built-in constants,
parentheses, unary numeric operators, numeric casts and side-effect-free
constant arithmetic. Mutable variables and user-defined function calls are
never guessed. `llPow` is warned on only when both arguments are known and the
exponent is provably non-integral; out-of-range or non-finite exponent values
are left unknown. Built-in name shadowing and invalid/malformed calls are
respected. Vector `%` cross products are not mistaken for integer remainders.
Evaluation is bounded and fails closed on parser errors or unreliable
lexical/preprocessor input.

References: [LSL Errors — Math Error](https://wiki.secondlife.com/wiki/LSL_Errors),
[llSqrt](https://wiki.secondlife.com/wiki/LlSqrt), [llPow](https://wiki.secondlife.com/wiki/LlPow),
[LSL Operators — modulus](https://wiki.secondlife.com/wiki/LSL_Operators), and
[LSL 101 — Integers](https://wiki.secondlife.com/wiki/LSL_101/LSL_in_Focus:_Integers).

### State reachability

LSL starts in `default` when a script is compiled, reset or loaded, and a `state`
statement transfers execution to another declared state. The AST engine reports
warning `SEM044` when a populated named state has no transition path from
`default`. It builds a bounded graph from transitions in event-handler bodies.
All syntactically present transitions count as possible edges, including
conditional branches; this intentionally avoids claiming that a state is dead
when the condition is not statically understood. Empty states remain the
responsibility of `SEM038`.

This is structural reachability from `default`, not a model of scripts resumed
from inventory in a previously active state. Parser errors, incomplete or
duplicate states, unreliable lexical/preprocessor input and graph-budget
exhaustion suppress the rule.

References: [Second Life Wiki — State](https://wiki.secondlife.com/wiki/State),
[Getting Started in LSL — States](https://wiki.secondlife.com/wiki/Getting_Started_In_LSL).

### Lists and comparisons

LSL lists cannot contain other lists. This includes a nested list literal, a
variable whose type is `list`, a function returning `list`, or a list-valued
expression used as an element. Neolint reports `SEM039` when it can prove that
an element has list type; unresolved extension expressions are not guessed.

The `==` and `!=` operators do not compare list contents. Equality compares
lengths only, while inequality returns the difference between the two lengths
rather than a normalized boolean. Neolint reports `SEM040` when both operands
are known to have list type. Use an explicit element-by-element comparison
when content equality is intended, or compare lengths directly when that is
the intended operation.

References: [Second Life Wiki — LSL List](https://wiki.secondlife.com/wiki/LSL_List)
and [Second Life Wiki — ListCompare](https://wiki.secondlife.com/wiki/ListCompare).

## Literals, components, and comments

Neolint recognizes decimal and hexadecimal integer literals, floating-point literals, quoted strings, list literals, and vector/rotation literals written with angle brackets. Hexadecimal values through `0xFFFFFFFF` use signed 32-bit interpretation. Line comments (`//`) and block comments (`/* ... */`) are accepted.

Vector variables support `.x`, `.y`, and `.z`. Rotation variables support `.x`, `.y`, `.z`, and `.s`. Component values are floats. Component access is supported on declared variables, not directly on literals or function-call results.

Identifiers are currently ASCII-only. Non-ASCII characters are accepted inside comments and string contents, but not in identifiers. Identifier matching is case-sensitive, exactly as in the Legacy engine: `Value` and `value` are distinct bindings.

## Firestorm extensions (opt-in)

Firestorm's LSL preprocessor supports several syntax extensions. Neolint parses
them only in the independent AST engine and only when the corresponding gate is
enabled. AST is the default CLI engine, so these forms are accepted by the
default CLI when the gates are on; explicit
Legacy (`--engine legacy` or `engine = legacy`) rejects them, as does plain LSL
parsing. The preprocessor dialect alone does not enable either extension.

### Switches

With `switches` enabled, the AST parser accepts Firestorm `switch` statements:

```lsl
switch (value)
{
    case 1:
        first();
    case 2:
        second();
        break;
    default:
        third();
}
```

Case order and fallthrough are preserved. Missing `break` is therefore not an
error and is not converted into an isolated block. `default` is optional; a
missing label produces the AST-only `SEM031` completeness warning. Duplicate
constant cases (`SEM030`), repeated `default` labels (`SEM028`), incompatible
case types (`SEM029`), constant switch expressions (`SEM032`), and list switch
comparisons (`SEM033`) receive semantic diagnostics when they can be determined
reliably. Unknown or malformed expressions are not guessed. Nested switches
and case blocks are supported. `break;` is contextual: inside a switch it exits
the innermost switch, including when nested in another block or control-flow
statement; outside a switch it remains an ordinary identifier expression.

The AST control-flow graph models case dispatch, fallthrough, `default`, and
`break`. Value-returning functions that contain a switch can therefore receive
`SEM011` when an unmatched path or a `break` can reach the function end, rather
than having their return analysis disabled for the entire function.

### Lazy lists

With `lazy_lists` enabled, the AST parser represents these Firestorm-confirmed
forms:

```lsl
myList[index] = value;          // single-index assignment
(integer)myList[index];         // typed extraction
(string)myList[index];
(list)myList[index];
(list)myList[first,last];       // list range extraction
```

Supported typed extractions are integer, float, string, key, vector, rotation,
and list. The typed reference may be fully parenthesized. The parser preserves
LHS/RHS/index structure and spans; it does not rewrite source to
`lazy_list_set` or `llList2*`. Bare reads without a typed cast, compound indexed
assignment, deletion, more than two indices, and non-identifier bases are not
part of confirmed Firestorm behavior and are not promised here. `quaternion` is
not added to the type system.

### Enabling gates

The preprocessing API exposes independent `switches` and `lazy_lists` settings,
and Firestorm's `USE_SWITCHES`/`USE_LAZY_LISTS` macros are detected after
emitted preprocessing tokens from the real active macro state (including
includes); once observed, a later `#undef` does not disable the extension. The
`//fspreprocessor off` kill switch disables preprocessing and both extensions.
`PreparedSource` exposes the effective gates; pass them to
`AstParseOptions`/`parse_ast_with_options`. The CLI runs the AST engine by
default and passes those effective gates;
explicit `--engine legacy` runs the Legacy engine, which does not parse these
extensions.

## Built-in functions and events

Built-in functions, event-handler signatures, and constants are based on the canonical LSL definitions published by [Second Life](https://github.com/secondlife/lsl-definitions). Neolint also warns about calls to canonical LSL functions marked as deprecated.

This metadata does not include every simulator-specific extension or third-party scripting API. Such APIs may be available in a particular environment but are not assumed to be part of canonical LSL.

## Semantic checks

In addition to lexical and syntax diagnostics, neolint checks selected declarations, function calls, event signatures, variable references, return statements, jump labels, types, vector/rotation components, duplicate event handlers and states, and state-change targets.

The complete list of diagnostic codes, default severities, examples, and rule boundaries is in the [diagnostic reference](diagnostics.md).

Analysis is limited to cases that neolint can determine with reasonable confidence. Unknown extension calls and unknown extension event bodies are excluded from relevant semantic checks. Unresolved expressions are not assigned guessed types, and some complex control-flow cases may remain unchecked.

## Unused and dead-code analysis

Neolint's unused/dead-code analysis is available only in the AST engine: the
default CLI run and explicit `--engine ast`. Explicit Legacy (`--engine legacy`
or `engine = legacy`) emits none of these diagnostics, and they are not part of
the Compare parity report. All three are warnings by default with the `lint`
kind, and they participate in `--disable`, `disable`, and `severity.CODE=...`
like the other warnings.

- `SEM024` reports a global or local variable that is never read.
- `SEM025` reports a parameter that is never read.
- `SEM026` reports a user-defined function that no script entry point reaches.

The analysis is syntactic. A declaration is used when a resolved identifier
occurrence reads its value. Plain assignment targets (including plain component
writes) and direct call callees are not reads; compound assignment targets
(including compound component writes), `++`/`--`, and component reads are.
Declaration identity is the owning lexical scope plus the declaration name span,
so shadowed declarations with the same spelling remain distinct and a read of a
different binding never marks a declaration used.

`SEM024` and `SEM025` do not perform runtime liveness or CFG-based dead-store
elimination. A read counts wherever it appears in syntactically valid source,
including inside a function that `SEM026` separately reports as unreachable.
`SEM025` covers parameters of user-defined functions and canonical event
handlers; parameters of unknown extension events are outside the scope model and
are therefore never reported.

`SEM026` is function reachability, not statement reachability or whole-program
runtime reachability. Roots are direct user-function calls in event handlers of
any state (including extension events) and in global initializers. Edges are
direct identifier calls in user-function bodies, and only calls with a matched
closing parenthesis create edges. Reachability is iterative and cycle-safe.
Prototypes, recovered signatures, incomplete bodies and duplicate declarations
after the first definition are never candidates. Functions reached only through
another unreachable function remain unreachable.

All three rules fail closed on unreliable units: if the same unit has an
error-severity preprocessor or lexical diagnostic, or if parser diagnostics,
recovered function signatures, error nodes, missing or unclosed
function/event/state bodies, or other unreliable structure make the read/use set
or call graph untrustworthy, they emit nothing. Error-severity preprocessing can
mean source was dropped before parsing, and an error-severity lexical diagnostic
means part of the text was not tokenized, so neither case is safe to analyze
optimistically. Informational and warning preprocessor notices do not suppress.
`SEM026` also suppresses the whole rule when its bounded graph limits (4,096
nodes, 8,192 edges, 16,384 work steps) are exceeded, rather than reporting a
partial result. The exact messages, spans, defaults, and boundaries are in the
[diagnostic reference](diagnostics.md).

## Global variable initializers

Global variables may be initialized from a simple value rather than a runtime
expression. The AST engine reports `SEM027` when it can prove that a global
initializer is not such a value. Accepted forms include scalar literals,
visible global-variable references, built-in constants, vectors/rotations whose
components are simple numeric values, and flat lists of simple values.

Arithmetic and logical operators, function calls, casts, parenthesized forms,
nested list literals, and non-numeric vector/rotation components are not treated
as simple global values. Local-variable initializers are unaffected. This is a
static syntactic check, not constant folding and not a guarantee that the target
viewer/compiler will accept every script; see the [diagnostic reference](diagnostics.md)
for its conservative boundaries.

## Operator operand types

The AST engine reports semantic error `SEM045` when it can prove that an
operator receives a type outside the supported LSL operand set. The rule checks
logical `&&`/`||`, bitwise `&`/`|`/`^` and shifts `<<`/`>>` as integer-only
operators; unary `!` and `~` also require an integer. Unary negation accepts
integer, float, vector and rotation values. Increment and decrement require a
known integer or float variable, and built-in constants cannot be modified.

For example, the following contains three independent `SEM045` findings:

```lsl
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

The rule deliberately skips unknown expression types instead of guessing. It
also fails closed for parser errors, unreliable lexical/preprocessor input, or
more than 65,536 collected expression sites. These diagnostics are AST-only:
Legacy output and the SEM001–SEM023 Compare parity set are unchanged. Unary
plus is not currently covered by SEM045.

References: [Second Life Wiki — LSL Operators](https://wiki.secondlife.com/wiki/LSL_Operators), [LSL 101 — Integers](https://wiki.secondlife.com/wiki/LSL_101/LSL_in_Focus:_Integers), and [LslPlus operator type validation](https://hdiff.luite.com/diff/LslPlus/0.2.0/0.3.0).

## Binary arithmetic and comparison operand types

The AST engine reports semantic error `SEM046` when both operands have known
types but an ordinary arithmetic or comparison operator has no valid result
under the shared LSL type matrix. It checks `+`, `-`, `*`, `/`, `%`,
`==`, `!=`, `<`, `<=`, `>` and `>=`.

The matrix preserves LSL-specific combinations: integer/float arithmetic,
string and list concatenation with `+`, vector addition/subtraction/dot and
cross products, rotation addition/subtraction/multiplication/division, and
vector-scalar or vector-rotation operations. Equality accepts same-type values
and documented implicit conversions. Relational comparisons require numeric
operands. List equality remains accepted by the language and is separately
flagged by `SEM040` because it compares list lengths rather than contents.

For example, the following operation is rejected because subtraction does not
accept a string and an integer:

```lsl
default {
    state_entry() {
        string label = "distance";
        integer invalid = label - 1; // SEM046
    }
}
```

The check reuses the same type-result matrix used by expression inference. It
skips an operation if either operand type is unknown, and fails closed for
parser errors, unreliable lexical/preprocessor input, or more than 65,536
collected expression sites. Logical, bitwise and shift operators remain under
`SEM045`. The rule is AST-only; Legacy and the SEM001–SEM023 Compare parity
set are unchanged.

References: [Second Life Wiki — LSL Operators](https://wiki.secondlife.com/wiki/LSL_Operators) and [Linden Scripting Language Guide, §3.2–3.3](https://static-secondlife-com.s3.amazonaws.com/downloads/textures/guides/LSLGuide.pdf).

## Important limitations

Neolint is not a complete LSL compiler or a complete symbol database for every scripting environment. It does not guarantee that a script will compile or behave correctly in Second Life or another simulator. Always use the official compiler and the target runtime to verify scripts that matter.

For command options and input handling, see the [command-line reference](cli-reference.md). For common errors, see [Troubleshooting](troubleshooting.md).
