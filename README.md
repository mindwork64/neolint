# neolint

A CLI-only Rust linter for Linden Scripting Language (LSL).

**No GUI. No web UI. No TUI. No IDE frontend.** The product is a command-line utility.

## Status

The current implementation provides a source-aware lexer, syntax validation, a vendored database of official LSL functions/events/constants, and a console CLI with nineteen conservative semantic checks (SEM001–SEM019). It supports scope-aware variable resolution, user-function call checks, recursive directory scanning, and rule configuration.

## References

- [LSL Portal](https://wiki.secondlife.com/wiki/LSL_Portal)
- [Official LSL Reference](https://create.secondlife.com/script/lsl-reference/)
- [Canonical LSL definitions](https://github.com/secondlife/lsl-definitions)

## Design

- Rust stable.
- Offline and deterministic during normal linting.
- Stable diagnostic codes and source locations.
- Syntax, semantic, and lint diagnostics remain distinct.
- Vendored official LSL function, event, and constant definitions; normal lint runs offline.
- Conservative semantic checks for duplicate globals/functions, built-in call arity/types, and known event handler signatures.
- The canonical LSL definitions are data input; SLua definitions are not part of LSL.

## Command line

```sh
neolint script.lsl
neolint --format json script.lsl
neolint ./scripts/
neolint --config .neolint.conf ./scripts/
neolint --disable SEM001,SEM004 ./scripts/
neolint --severity SEM004=warning script.lsl
cat script.lsl | neolint -
neolint --tokens script.lsl
```

The current build reports lexical and syntax errors, plus these semantic checks:

- `SEM001`: duplicate top-level variable/function declarations.
- `SEM002`: wrong argument count for a known built-in LSL function.
- `SEM003`: incorrect parameter count/type for a known LSL event handler.
- `SEM004`: provable argument type mismatch for a known built-in function, including variables resolved in the current lexical scope and supported binary-expression result types.
- `SEM005`: wrong argument count for a known user-defined function.
- `SEM006`: provable argument type mismatch for a known user-defined function, including supported binary-expression result types.
- `SEM007`: duplicate local declaration in the same lexical scope, including redeclaration of a parameter; shadowing in a nested block is allowed.
- `SEM008`: validates explicit return statements in user-defined functions, including return-value presence and provable return-expression type compatibility.
- `SEM009`: duplicate jump labels within one function or event handler.
- `SEM010`: jump targets that are undefined or located inside an inaccessible nested block.
- `SEM011`: value-returning functions that may reach the end without returning a value. A token-derived control-flow graph models sequential blocks, `if/else`, `for`, `while`, `do-while`, `return`, and forward/backward `jump` edges. Constant integer conditions are evaluated with LSL precedence and 32-bit wrapping arithmetic, including canonical integer constants; division/remainder by zero, out-of-range shifts, expressions with unresolved symbols, and expressions beyond the safe size/depth limits remain unknown and allow both outcomes. Both operands of `&&`/`||` are evaluated; no short-circuit simplification is applied. Invalid jump structures suppress cascading SEM011 diagnostics.
- `SEM012`: warning for a variable reference that cannot be resolved to a declaration in the current lexical scope or a canonical built-in constant. Unknown extension function calls (including their argument ranges) and bodies of unknown extension event handlers are deliberately ignored; the rule is a warning because environments may provide extra symbols.
- `SEM013`: warning when calling one of the 28 LSL functions marked `deprecated` in the pinned official `lsl_definitions.yaml` snapshot. SLua-only deprecation fields are excluded, and no SLua replacement hints are presented as LSL APIs.
- `SEM014`: error for explicit casts from known source types to unsupported LSL targets, scanned throughout expressions including local initializers; unknown source types are left unchecked.
- `SEM015`: error for a global or local variable initializer whose known expression type is incompatible with the declared variable type; `integer` to `float` and `string` to `key` implicit conversions are accepted.
- `SEM016`: error for a plain assignment to a declared variable when the known expression type is incompatible with the variable type. Compound assignments are checked by SEM017; vector/rotation component assignments are checked by SEM018.
- `SEM017`: type-checks the result of `+=`, `-=`, `*=`, `/=`, and `%=`; reports known unsupported operand combinations and incompatible results, while unresolved right-hand values are skipped.
- `SEM018`: validates vector/rotation component writes, checks component existence and `float` compatibility, and reports known unsupported compound operand pairs while remaining conservative for unknown expressions.
- `SEM019`: reports reads of invalid components on variables with known types (for example, `integer count; count.x`, `vector position; position.s`, or `rotation orientation; orientation.w`). Valid vector `.x/.y/.z` and rotation `.x/.y/.z/.s` reads remain `float`; component writes stay with SEM018 and unknown extension values are skipped.

The parser accepts LSL user-defined functions with a value return type or no return type; do not write `void` for a function that returns no value. A no-parameter function uses empty parentheses, not `(void)`. It enforces that global declarations and user-defined functions precede state definitions (`PAR009`), that a `default` state is mandatory, first and unique (`PAR013`/`PAR014`), and reports `PAR008` when a variable declaration is used directly in a `for` initializer; declare the loop variable before the loop. Empty statements (`;`) are accepted, and `for` initializer/increment clauses accept comma-separated expression lists while the condition remains required (`PAR006`). Ternary conditional expressions (`?:`) are not part of LSL and are rejected by the parser. The `&&` and `||` operators share one precedence level and are left-associative; C-style precedence is not applied. The detailed grammar inventory and test matrix are in [docs/lsl-grammar.md](docs/lsl-grammar.md).

SEM011 uses a conservative token-derived control-flow graph rather than a full parser AST. Unsupported or malformed control-flow constructs are skipped when the graph cannot be built safely. `SEM012` warnings do not make the CLI exit with code 1 by default; they can be disabled or assigned another severity through the normal configuration and CLI options.

Binary expression inference is deliberately conservative. It covers supported numeric operations and promotions, string/list concatenation, comparisons/logical results, and documented vector/rotation combinations. Unknown or incompatible operand pairs are left unresolved rather than assigned a guessed type.

`SEM013` is a lint warning by default, so it does not make the CLI exit with code 1. Use `--disable SEM013` or `--severity SEM013=error` to change that behavior. Canonical deprecation metadata lives in `data/lsl-deprecated.txt` and is refreshed together with the builtin prototype snapshot by `scripts/update-lsl-builtins.sh`.

A clean result does not mean full semantic validation or complete lint-rule coverage has passed.

## Configuration

A configuration file is a small text file:

```ini
# Disable rules that do not fit this codebase
disable = SEM001, SEM003

# Downgrade a rule without hiding its diagnostics
severity.SEM004 = warning
```

Use `--config FILE` to load it. Command-line `--disable` and `--severity RULE=LEVEL` settings are applied after the config file and therefore take precedence for the same rule. Severity levels are `error`, `warning`, and `info`.

When a directory is passed, neolint recursively scans it for `.lsl` files (case-insensitive extension), in deterministic path order. Explicit file paths are checked as supplied, regardless of extension. Symbolic links encountered during recursive scanning are skipped.

## License

License selection is pending.
