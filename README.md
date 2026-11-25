<!--
  neolint — repository landing page
  Keep this page focused on orientation and navigation; detailed behavior belongs in docs/.
-->

<p align="center">
  <img src="assets/branding/neolint-logo.png" alt="neolint" width="520">
</p>

<p align="center">
  <strong>A deterministic, offline-by-default linter for Linden Scripting Language (LSL).</strong>
  <br>
  Catch lexical, syntax, preprocessor, and conservative semantic issues before they reach the viewer.
</p>

<p align="center">
  <a href="https://github.com/mindwork64/neolint/releases/latest"><img src="https://img.shields.io/github/v/release/mindwork64/neolint?style=flat-square&label=release" alt="Latest release"></a>
  <a href="https://github.com/mindwork64/neolint/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/mindwork64/neolint/ci.yml?branch=main&style=flat-square&label=CI" alt="CI status"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.75%2B-orange?style=flat-square" alt="Rust 1.75 or newer"></a>
  <a href="https://github.com/mindwork64/neolint/blob/main/LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue?style=flat-square" alt="MIT or Apache 2.0"></a>
  <a href="https://github.com/mindwork64/neolint/releases"><img src="https://img.shields.io/badge/platforms-Linux%20%7C%20Windows-lightgrey?style=flat-square" alt="Linux and Windows"></a>
</p>

<p align="center">
  <a href="#get-started">Get started</a> ·
  <a href="#what-it-checks">What it checks</a> ·
  <a href="#documentation">Documentation</a> ·
  <a href="#for-developers">For developers</a>
</p>

---

## At a glance

Current version: `1.1.0` · [stable release](https://github.com/mindwork64/neolint/releases/tag/v1.1.0)

| | |
|:--|:--|
| **Language** | Linden Scripting Language (LSL) |
| **Implementation** | Rust · no runtime dependencies |
| **Analysis engine** | AST by default · Legacy compatibility mode |
| **Execution** | Local CLI · deterministic · offline during normal lint runs |
| **Release binaries** | Linux x86_64 · Windows x86_64 |
| **Current version** | `1.1.0` · [stable release](https://github.com/mindwork64/neolint/releases/tag/v1.1.0) |

Neolint is a linter, **not an LSL compiler**. A clean report means only that the checks implemented by neolint found no issue; it does not guarantee acceptance or correct behavior in Second Life or another simulator. The official compiler and target runtime remain authoritative.

## Get started

### 1. Download

Choose the archive for your platform from the **[v1.1.0 release](https://github.com/mindwork64/neolint/releases/tag/v1.1.0)**:

| Platform | Archive |
|:--|:--|
| Linux x86_64 | <code>neolint-v1.1.0-x86_64-unknown-linux-gnu.tar.gz</code> |
| Windows x86_64 | <code>neolint-v1.1.0-x86_64-pc-windows-msvc.zip</code> |

Verify the downloaded archive using its adjacent <code>.sha256</code> file. The [installation guide](docs/getting-started.md#verify-a-download) contains platform-specific verification commands and full installation steps.

### 2. Run

~~~sh
# Check one script
neolint script.lsl

# Recursively check a directory
neolint ./scripts/

# Emit machine-readable diagnostics
neolint --format json script.lsl

# Read source from standard input
cat script.lsl | neolint -
~~~

### 3. Tune the result

~~~sh
# Load project-wide settings
neolint --config .neolint.conf ./scripts/

# Disable selected rules
neolint --disable SEM001,SEM004 ./scripts/

# Change a rule's severity
neolint --severity SEM004=warning script.lsl

# Use the historical analysis engine
neolint --engine legacy script.lsl
~~~

See the **[CLI reference](docs/cli-reference.md)** for every option, output format, and exit code, or the **[configuration guide](docs/configuration.md)** to make settings reproducible across a project.

## What it checks

Neolint organizes findings into four diagnostic families:

| Family | Prefix | What it catches |
|:--|:--:|:--|
| Lexical | <code>LEX</code> | Unexpected characters, unterminated strings and comments |
| Syntax | <code>PAR</code>, <code>AST</code> | Invalid declarations, expressions, statements, and parser safety limits |
| Preprocessor | <code>PRE</code> | Invalid directives, macro expansion, includes, conditional blocks, and resource limits |
| Semantic and lint | <code>SEM</code> | Type mismatches, declarations, calls, event signatures, state changes, and selected code-quality issues |

The AST engine is the default in v1.1.0. The previous Legacy engine remains available for compatibility; <code>compare</code> mode is provided for maintainers investigating differences between the two.

**Also included:**
- Standard LSL preprocessing and an opt-in Firestorm preprocessor profile.
- Include directories, macro expansion, source locations, and related diagnostic locations.
- AST-only checks for unused variables and parameters, unreachable functions, global initializers, Firestorm switches, Mono state changes, return/control flow, empty states, nested lists and list comparisons, suspicious `if` assignments, and detection-context misuse (`SEM024`–`SEM042`).
- Human-readable output and structured JSON diagnostics.
- Configurable rule suppression and severity overrides.

The [diagnostic reference](docs/diagnostics.md) lists every code, default severity, examples, and important boundaries. The [language support guide](docs/language-support.md) explains supported syntax and deliberate limitations.

## Documentation

Use this as the map to the project docs. The README stays deliberately concise; each guide owns the details for its subject.

| If you want to… | Start here |
|:--|:--|
| Install neolint and run your first check | **[Getting started](docs/getting-started.md)** |
| Look up command-line flags, JSON output, or exit codes | **[CLI reference](docs/cli-reference.md)** |
| Configure rules, severities, and analysis engine | **[Configuration](docs/configuration.md)** |
| Understand a diagnostic or decide how to fix it | **[Diagnostic reference](docs/diagnostics.md)** |
| Check which LSL constructs are supported | **[Language support](docs/language-support.md)** |
| Use Firestorm preprocessing and syntax extensions | **[Firestorm preprocessor](docs/firestorm-preprocessor.md)** |
| Diagnose installation or invocation problems | **[Troubleshooting](docs/troubleshooting.md)** |
| Review the current implementation and verified status | **[Project state](docs/PROJECT_STATE.md)** |
| Understand AST design and implementation decisions | **[AST stabilization](docs/architecture/ast-stabilization.md)** · **[Preprocessor / AST architecture](docs/architecture/preprocessor-ast.md)** |
| See what changed between versions | **[Changelog](CHANGELOG.md)** |

## Supported platforms

Prebuilt stable release archives are currently provided for:

- **Linux x86_64** — <code>x86_64-unknown-linux-gnu</code>
- **Windows x86_64** — <code>x86_64-pc-windows-msvc</code>

macOS, ARM, 32-bit, and other targets are not part of the current release. Neolint has no GUI, TUI, web interface, IDE integration, language server, or autofix functionality: it is intentionally a focused command-line tool.

## For developers

Build from source with a stable Rust toolchain (minimum supported version: 1.75):

~~~sh
cargo build --release
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
~~~

The CI workflow additionally checks documentation contracts, Rustdoc, release-mode tests, and the Rust 1.75 MSRV. See the [AST stabilization notes](docs/architecture/ast-stabilization.md) and [project state](docs/PROJECT_STATE.md) before changing parser or analysis behavior.

Issues and pull requests are welcome: **[report a bug](https://github.com/mindwork64/neolint/issues/new)** · **[browse open issues](https://github.com/mindwork64/neolint/issues)**.

## License

Neolint's own source code is dual-licensed under **MIT OR Apache-2.0**: choose either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option.

Vendored Linden Research LSL definitions in <code>data/lsl-builtins.txt</code> and <code>data/lsl-deprecated.txt</code> retain their separate upstream MIT license. They are not relicensed by neolint's dual-license choice. See the [third-party notice](LICENSES/LINDEN-RESEARCH-MIT.txt) and [data provenance](data/README.md).

---

<p align="center">
  <sub>Built for people who write LSL and would rather catch mistakes before uploading.</sub>
</p>
