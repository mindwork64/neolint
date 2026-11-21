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

Identifiers are currently ASCII-only. Non-ASCII characters are accepted inside comments and string contents, but not in identifiers.

## Built-in functions and events

Built-in functions, event-handler signatures, and constants are based on the canonical LSL definitions published by [Second Life](https://github.com/secondlife/lsl-definitions). Neolint also warns about calls to canonical LSL functions marked as deprecated.

This metadata does not include every simulator-specific extension or third-party scripting API. Such APIs may be available in a particular environment but are not assumed to be part of canonical LSL.

## Semantic checks

In addition to lexical and syntax diagnostics, neolint checks selected declarations, function calls, event signatures, variable references, return statements, jump labels, types, vector/rotation components, duplicate event handlers and states, and state-change targets.

The complete list of diagnostic codes, default severities, examples, and rule boundaries is in the [diagnostic reference](diagnostics.md).

Analysis is limited to cases that neolint can determine with reasonable confidence. Unknown extension calls and unknown extension event bodies are excluded from relevant semantic checks. Unresolved expressions are not assigned guessed types, and some complex control-flow cases may remain unchecked.

## Important limitations

Neolint is not a complete LSL compiler or a complete symbol database for every scripting environment. It does not guarantee that a script will compile or behave correctly in Second Life or another simulator. Always use the official compiler and the target runtime to verify scripts that matter.

For command options and input handling, see the [command-line reference](cli-reference.md). For common errors, see [Troubleshooting](troubleshooting.md).
