# neolint

A CLI-only Rust linter for Linden Scripting Language (LSL).

**No GUI. No web UI. No TUI. No IDE frontend.** The product is a command-line utility.

## Status

The first implementation milestone provides a source-aware lexer, a syntax parser, and a console CLI. Semantic analysis and lint rules are being added incrementally.

## References

- [LSL Portal](https://wiki.secondlife.com/wiki/LSL_Portal)
- [Official LSL Reference](https://create.secondlife.com/script/lsl-reference/)
- [Canonical LSL definitions](https://github.com/secondlife/lsl-definitions)

## Design

- Rust stable.
- Offline and deterministic during normal linting.
- Stable diagnostic codes and source locations.
- Syntax, semantic, and lint diagnostics remain distinct.
- The canonical LSL definitions are data input; SLua definitions are not part of LSL.

## Planned command line

```sh
neolint script.lsl
neolint --format json script.lsl
cat script.lsl | neolint -
neolint --tokens script.lsl
```

The current build reports lexical and syntax errors. A clean result does not yet mean semantic validation or lint rules have passed.

## License

License selection is pending.
