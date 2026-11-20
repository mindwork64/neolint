# neolint

A CLI-only Rust linter for Linden Scripting Language (LSL).

**No GUI. No web UI. No TUI. No IDE frontend.** The product is a command-line utility.

## Status

The current implementation provides a source-aware lexer, syntax validation, a vendored database of official LSL functions/events/constants, and a console CLI with four conservative semantic checks (SEM001–SEM004). The CLI supports recursive directory scanning and rule configuration.

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

The current build reports lexical and syntax errors, duplicate top-level declarations (SEM001), built-in function argument-count mismatches (SEM002), known event handler signature mismatches (SEM003), and provable built-in argument type mismatches (SEM004). A clean result does not yet mean full semantic validation or the wider lint rule set has passed.

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
