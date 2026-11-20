# neolint

A CLI-only Rust linter for Linden Scripting Language (LSL).

**No GUI. No web UI. No TUI. No IDE frontend.** The product is a command-line utility.

## Status

The current implementation provides a source-aware lexer, a syntax parser, a vendored database of official LSL functions/events/constants, and a console CLI with three conservative semantic checks (SEM001–SEM003). Broader semantic analysis and lint rules are being added incrementally.

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
- Conservative semantic checks for duplicate globals/functions, built-in call arity, and known event handler signatures.
- The canonical LSL definitions are data input; SLua definitions are not part of LSL.

## Command line

```sh
neolint script.lsl
neolint --format json script.lsl
cat script.lsl | neolint -
neolint --tokens script.lsl
```

The current build reports lexical and syntax errors, duplicate top-level declarations (SEM001), built-in function argument-count mismatches (SEM002), and known event handler signature mismatches (SEM003). A clean result does not yet mean full semantic validation or the wider lint rule set has passed.

## License

License selection is pending.
