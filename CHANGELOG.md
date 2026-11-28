# Changelog

Notable user-facing changes are recorded here. Release archives and SHA-256
checksums are published on the [GitHub Releases page](https://github.com/mindwork64/neolint/releases).

## [Unreleased]

## [1.3.0] - 2026-11-28

### Added

- Optional multi-script project manifests (`--project`) with deterministic
  script ordering, project defaults, per-script configuration overlays, and
  independent preprocessing and analysis units.
- The optional `neolint-lsp` executable, backed by the same Rust analysis
  engine as the CLI. The server provides LSP diagnostics, full-text document
  synchronization, safe shutdown, cancellation-aware analysis, and
  dependency-scoped invalidation for local includes and configuration files.
- A VS Code extension with live diagnostics, hover, definition navigation,
  context-aware completion (including target API catalogues), and a safe
  `SEM041` quick fix that marks an assignment condition as intentional.
- Multi-root workspace support and per-script configuration selection for
  `neolint.project`, `.neolint.conf`, and the optional extension overlay.
- LSLint-inspired AST diagnostics for constant conditions, empty `if`
  statements, transitions to the current state, multiple LSO jump sites, and
  unsafe `print()` usage.
- A Second Life God Mode-only function guard with explicit `--god-mode` opt-in.

### Changed

- Release archives now include both the CLI and LSP executables for Linux
  x86_64 and Windows x86_64, together with project licenses, generated
  notices for the locked Rust dependency graph, and separate notices for
  vendored Linden Research and OpenSimulator metadata.
- The release workflow packages the VS Code extension as a VSIX and publishes
  SHA-256 checksums for all three distributable packages.
- LSP initialization now returns the standard nested
  `InitializeResult.capabilities` object and identifies the server/version in
  `serverInfo`.
- Release smoke tests exercise an actual LSP JSON-RPC initialize/shutdown
  exchange and validate the VSIX runtime dependency contents.

### Compatibility and boundaries

- AST remains the default engine. The Legacy engine remains available through
  `--engine legacy`; Compare remains an opt-in maintainer mode.
- The LSP server analyzes each open root script independently. Hover,
  definition and completion are scoped to the current script; cross-file
  navigation and workspace-wide symbol indexing are not included.
- Each affected root is currently reanalyzed in full. Incremental parsing and
  AST reuse are not implemented.
- NeoLint remains a linter, not a compiler. A clean lint result does not
  guarantee acceptance by the official compiler or a simulator.
- The VS Code extension is distributed as a downloadable VSIX on GitHub
  Releases; Marketplace publication is not part of this release.

### Verification

- Rust debug/release tests, the Rust 1.75 MSRV suite, Clippy, Rustdoc,
  documentation contracts, VS Code type-check/build, VSIX content validation,
  and platform-specific CLI/LSP release smoke tests are required release gates.
- Live Firestorm external-editor integration remains a low-priority manual
  follow-up (issue #132), not a release blocker.

## [1.2.0] - 2026-11-26

### Added

- Expanded control-flow and data-flow analysis: CFG-based reachability,
  fixed-point propagation of known local constants, branch- and switch-aware
  analysis, and more precise handling of side effects and unreachable code.
- Extended `SEM043` mathematical-domain checks for operations that can produce
  invalid or non-real results when their input domain is known.
- Runtime/target profiles for Second Life Mono, LSO and Luau, plus OpenSim
  XEngine/YEngine catalogues pinned to API20, API22, API23 and API24 snapshots.
- Target-aware diagnostics `SEM047`–`SEM050` for unavailable APIs, explicit
  OSSL allowlists, events and constants. OSSL permissions are not inferred from
  the selected target.
- Firestorm Lazy List AST support for indexed writes, typed scalar reads,
  two-index list ranges and the Firestorm-only `quaternion` alias for rotation.
  Supported forms are feature-gated and preserve original source locations.

### Changed

- Semantic analysis now uses richer call-graph, side-effect,
  constant-propagation and control-flow facts while keeping unknown values
  conservative and resource limits bounded.
- Runtime targets are selected independently from preprocessing dialects.
  OpenSim API snapshots are explicit and reproducible; live region permissions
  are not queried.

### Compatibility and boundaries

- AST remains the default engine. The Legacy engine remains available through
  `--engine legacy`; Compare remains an opt-in maintainer mode.
- Firestorm switches and lazy lists remain opt-in through their independent
  syntax gates.
- List extraction accepts a two-index range, `(list)items[first,last]`.
  The one-index spelling is rejected because Firestorm rewrites it into an
  invalid two-argument `llList2List` call.
- Neolint remains a deterministic CLI linter, not a compiler. A clean lint
  result does not guarantee acceptance by a simulator.

### Verification

- 1,186 Rust tests passed in Debug, Release and Rust 1.75.0 configurations.
- Clippy, Rustdoc, catalogue, documentation, benchmark-harness, site-content
  and Astro production-build checks passed.
- Firestorm-Releasex64 6.6.17.70368: supported Lazy List forms compiled and
  executed in-world with the expected runtime output; the malformed one-index
  list extraction failed compilation as expected.

### Downloads

- Linux x86_64: `neolint-v1.2.0-x86_64-unknown-linux-gnu.tar.gz`
- Windows x86_64: `neolint-v1.2.0-x86_64-pc-windows-msvc.zip`

Verify each archive with its adjacent `.sha256` file.


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
