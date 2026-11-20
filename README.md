# neolint

A CLI-only Rust linter for Linden Scripting Language (LSL).

**No GUI. No web UI. No TUI. No IDE frontend.** The product is a command-line utility.

## Status

The repository is being initialized. The first implementation milestone is a source-aware lexer and diagnostic model. Parsing and semantic lint rules will be added incrementally.

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
```

The CLI surface will be finalized alongside the initial lexer/parser implementation.

## License

License selection is pending.
