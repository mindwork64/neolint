# neolint

A deterministic, offline-by-default CLI linter for Linden Scripting Language (LSL), written in Rust.

Neolint finds lexical, syntax, and conservative semantic problems before code reaches the official Second Life compiler. **It is not an LSL compiler and does not guarantee acceptance by Second Life or any simulator.** A clean result only means the implemented checks found no issue.

## Status

- Current package version: `1.0.0` (release candidate; not yet published).
- Implemented diagnostics: `LEX000`–`LEX002`, parser `PAR001`–`PAR014` (no `PAR002`), and `SEM001`–`SEM022`.
- CLI-only: no GUI, TUI, web UI, IDE integration, language server, or autofix.
- Normal lint runs are deterministic and offline.
- No stable v1.0.0 release has been published.
- Dual-licensed under **MIT OR Apache-2.0**.

## Supported release targets

The initial downloadable binary targets are Linux x86_64 and Windows x86_64. macOS, ARM, 32-bit, and other targets are not part of the initial binary release.

Release archives are produced by the release workflow when a version tag is published. No release assets should be assumed to exist before that happens. See [Getting started](docs/getting-started.md) for exact archive names, installation, checksums, and build-from-source instructions.

## Quick start

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

Exit codes: `0` no error-severity diagnostics, `1` at least one error, `2` CLI/input/config error. Warnings and info diagnostics do not fail unless configured as errors.

Build from source with stable Rust:

```sh
cargo build --release
./target/release/neolint --version
```

## Documentation

- [Getting started and installation](docs/getting-started.md)
- [CLI reference](docs/cli-reference.md)
- [Configuration](docs/configuration.md)
- [Complete diagnostic reference](docs/diagnostics.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Language support](docs/language-support.md)
- [Grammar inventory and parser recovery](docs/lsl-grammar.md)
- [Regression corpus](docs/corpus.md)
- [Release engineering](docs/releasing.md)
- [Contributing](docs/contributing.md)

## Scope and limits

The analyzer uses a hand-written token-based parser and conservative symbol/type/control-flow analysis. It intentionally avoids speculative diagnostics: unknown extension APIs, unresolved values, and unverified language behavior are not treated as definite errors. Unknown extension event bodies are excluded from relevant semantic checks.

The diagnostic families are documented in [docs/diagnostics.md](docs/diagnostics.md), including conservative boundaries for `SEM011` control flow, `SEM012` unresolved symbols, canonical built-in metadata, and state/event rules `SEM020`–`SEM022`.

## License

Neolint's own software source is available under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option.

Vendored Linden Research LSL definitions in `data/lsl-builtins.txt`, `data/lsl-deprecated.txt`, and the upstream notice at [`LICENSES/LINDEN-RESEARCH-MIT.txt`](LICENSES/LINDEN-RESEARCH-MIT.txt) retain their separate upstream MIT license. They are not relicensed by neolint's MIT OR Apache-2.0 choice. See [`data/README.md`](data/README.md) for provenance and attribution.
