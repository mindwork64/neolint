# Changelog

Notable user-facing changes are recorded here. Release archives and SHA-256
checksums are published on the [GitHub Releases page](https://github.com/mindwork64/neolint/releases).

## [1.1.0] - 2026-11-24

### Added

- Independent typed-AST parsing and semantic analysis are now the default
  analysis engine for CLI use.
- AST-only lint checks: `SEM024` for unread variables, `SEM025` for unread
  parameters, and `SEM026` for user functions unreachable from event handlers
  and global initializers.
- Standard LSL and Firestorm preprocessing, include handling, source mapping,
  and expanded preprocessor diagnostics are part of the stable release line.
- JSON diagnostics expose additive optional preprocessor `origin` metadata and
  `related` source locations while preserving the existing `kind` vocabulary
  and JSON v1 fields.
- `--engine compare` runs Legacy and AST and reports semantic differences for
  maintainers.

### Changed

- `ast` is the default engine. Existing configuration files without an
  `engine` setting use AST.
- The stable CI gate now runs debug and release tests, documentation checks,
  Rustdoc, and the Rust 1.75 MSRV test suite.
- Release smoke tests verify both the AST default and the explicit Legacy
  compatibility path.

### Compatibility and migration

- The Legacy analyzer remains available with `--engine legacy` or
  `engine = legacy`; it has not been removed.
- Users who require the historical v1.0.0 plain-LSL behavior should select
  Legacy explicitly. AST can produce additional `SEM024`–`SEM026` warnings,
  has documented semantic differences, and may order diagnostics differently.
- `compare` is intended as a developer diagnostic mode, not as the normal
  production lint engine.
- Neolint remains a CLI tool; this release does not add an IDE integration,
  language server, incremental analysis, or autofix.

### Downloads

- Linux x86_64: `neolint-v1.1.0-x86_64-unknown-linux-gnu.tar.gz`
- Windows x86_64: `neolint-v1.1.0-x86_64-pc-windows-msvc.zip`

Verify each archive with its adjacent `.sha256` file.
