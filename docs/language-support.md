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
enabled. AST is the default CLI engine on the experimental Phase 6H branch, so
these forms are accepted by the default CLI when the gates are on; explicit
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
error and is not converted into an isolated block. `default` is optional,
duplicate cases/defaults are retained as separate AST arms, and nested switches
and case blocks are supported. `break;` is contextual: inside a switch it is a
break statement (including inside nested blocks); outside a switch it remains an
ordinary identifier expression.

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
default on the experimental Phase 6H branch and passes those effective gates;
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

All three rules fail closed on malformed input: if the same unit has an
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

## Important limitations

Neolint is not a complete LSL compiler or a complete symbol database for every scripting environment. It does not guarantee that a script will compile or behave correctly in Second Life or another simulator. Always use the official compiler and the target runtime to verify scripts that matter.

For command options and input handling, see the [command-line reference](cli-reference.md). For common errors, see [Troubleshooting](troubleshooting.md).
