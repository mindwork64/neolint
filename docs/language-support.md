# LSL language support

Neolint checks common LSL syntax and a defined set of semantic rules. Its analysis is intentionally conservative: the official Second Life compiler remains authoritative, and uncertain language behavior is not treated as a definite error.

## Script structure

An LSL script contains global declarations and user-defined functions followed by state definitions. Every script must have exactly one `default` state, and it must be the first state. Additional states use the `state name` form. Event handlers are declared inside states.

Global declarations and user-defined functions must appear before state definitions. A function that returns no value omits its return type; LSL does not use `void` as a function return type or parameter-list marker.

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

### State changes and Mono compatibility

A `state target;` statement is valid in an event handler. A top-level
user-defined (global) function is a different context: the Mono compiler
rejects ordinary state changes from these functions with the diagnostic
`Global functions can't change state`. Neolint's AST engine reports
`SEM034` for these placements, whether or not the function is currently
reachable from an event handler.

The Second Life Wiki documents one historical compiler bugfeature:
`if (condition) state target;` with the state change as the direct, unbraced
then-branch and no `else`. The same page records that Rand Linden had previously
said this workaround was no longer valid. A 2026 Mono/LSL-Luau compatibility
report demonstrates different execution behavior for this exact pattern.
Neolint therefore reports this narrow shape as `SEM035`, a warning—not as a
supported workaround or portable LSL. Do not use it in new code; perform state
transitions in event handlers. Braced branches, `if/else`, and loop bodies do
not qualify and receive `SEM034`.

These are AST-only diagnostics. Legacy output remains frozen, and Compare still
compares only `SEM001`–`SEM023`. The rule skips malformed state-change
statements, incomplete function bodies, unreliable lexical/preprocessor units,
and inputs exceeding its bounded traversal budget. The official references are
the [LSL State documentation](https://wiki.secondlife.com/wiki/State) and the
[2026 Mono/LSL-Luau state-change compatibility report](https://feedback.secondlife.com/slua-alpha/p/state-changes-discrepancy-in-lsl-luau-vs-mono).

### Return statements and unreachable code

A bare `return;` is valid in a function or event handler. A value-return such as
`return 1;` inside an event handler is not valid LSL behavior: the compiler may
accept it, but the official reference warns that executing it crashes the
script. Neolint reports this as `SEM036` for canonical and extension event
handlers. Use a value-return only from a user-defined function with a matching
return type.

`SEM037` warns when Neolint can prove that the first following statement is
unreachable after an unconditional `return`, after an `if/else` whose two
branches both terminate, or after an event handler changes to a different,
known state. A transition to the current state is not a terminator because LSL
performs no state-change side effects in that case. Conditional one-sided
returns/transitions, loops and switch dispatch are not treated as guaranteed
terminators. Bodies containing `jump`/labels or malformed syntax are skipped to
avoid false positives. This is a bounded, conservative check rather than a full
statement-reachability proof.

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

## Important limitations

Neolint is not a complete LSL compiler or a complete symbol database for every scripting environment. It does not guarantee that a script will compile or behave correctly in Second Life or another simulator. Always use the official compiler and the target runtime to verify scripts that matter.

For command options and input handling, see the [command-line reference](cli-reference.md). For common errors, see [Troubleshooting](troubleshooting.md).
