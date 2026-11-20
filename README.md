# neolint

A CLI-only Rust linter for Linden Scripting Language (LSL).

**No GUI. No web UI. No TUI. No IDE frontend.** The product is a command-line utility.

## Status

The current implementation provides a source-aware lexer, syntax validation, a vendored database of official LSL functions/events/constants, and a console CLI with twelve conservative semantic checks (SEM001–SEM012). It supports scope-aware variable resolution, user-function call checks, recursive directory scanning, and rule configuration.

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
- `SEM004`: provable argument type mismatch for a known built-in function, including checks against variables resolved in the current lexical scope.
- `SEM005`: wrong argument count for a known user-defined function.
- `SEM006`: provable argument type mismatch for a known user-defined function.
- `SEM007`: duplicate local declaration in the same lexical scope, including redeclaration of a parameter; shadowing in a nested block is allowed.
- `SEM008`: validates explicit return statements in user-defined functions, including return-value presence and provable return-expression type compatibility.
- `SEM009`: duplicate jump labels within one function or event handler.
- `SEM010`: jump targets that are undefined or located inside an inaccessible nested block.
- `SEM011`: value-returning functions that may reach the end without returning a value. The analysis handles sequential blocks and `if/else`, treats `for/while` as potentially executing zero times, and does not speculate about functions containing `jump` labels.
- `SEM012`: warning for a variable reference that cannot be resolved to a declaration in the current lexical scope or a canonical built-in constant. Unknown extension function calls (including their argument ranges) and bodies of unknown extension event handlers are deliberately ignored; the rule is a warning because environments may provide extra symbols.

The parser accepts LSL user-defined functions with a value return type or no return type; do not write `void` for a function that returns no value. A no-parameter function uses empty parentheses, not `(void)`. It enforces that user-defined functions precede state definitions (`PAR009`) and reports `PAR008` when a variable declaration is used directly in a `for` initializer; declare the loop variable before the loop.

SEM011 is deliberately conservative: functions containing jump statements or labels are skipped until their control-flow edges are modeled; the rule does not yet build a full control-flow graph. `SEM012` warnings do not make the CLI exit with code 1 by default; they can be disabled or assigned another severity through the normal configuration and CLI options.

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
