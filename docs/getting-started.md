# Getting started

NeoLint is a linter for Linden Scripting Language (LSL), with a command-line
interface and an optional VS Code language server. It does not replace the
official Second Life compiler; a clean lint result does not guarantee compiler
or simulator acceptance.

This guide describes the planned v1.3.0 distribution. Its release assets are
not available until the release-preparation PR is merged and the v1.3.0 tag is
published. The currently published v1.2.0 package contains the CLI only.

## Supported binary targets

The v1.3.0 release provides prebuilt CLI and language-server binaries for:

- Linux x86_64: `x86_64-unknown-linux-gnu`
- Windows x86_64: `x86_64-pc-windows-msvc`

macOS, ARM, 32-bit Windows/Linux, and other targets are not included.

## Install the binaries (Linux x86_64)

Download
`neolint-v1.3.0-x86_64-unknown-linux-gnu.tar.gz` and its adjacent
`.sha256` file from the
[v1.3.0 GitHub release](https://github.com/mindwork64/neolint/releases/tag/v1.3.0).
Verify the archive before extracting it:

```sh
sha256sum -c neolint-v1.3.0-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf neolint-v1.3.0-x86_64-unknown-linux-gnu.tar.gz
```

Install both executables for the current user:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 neolint neolint-lsp "$HOME/.local/bin/"
```

Ensure `$HOME/.local/bin` is on `PATH`, then verify the CLI:

```sh
neolint --version
```

## Install the binaries (Windows x86_64)

Download
`neolint-v1.3.0-x86_64-pc-windows-msvc.zip` and its `.sha256` file from
the [v1.3.0 GitHub release](https://github.com/mindwork64/neolint/releases/tag/v1.3.0).

Verify the SHA-256 checksum in PowerShell:

```powershell
$expected = (Get-Content .\neolint-v1.3.0-x86_64-pc-windows-msvc.zip.sha256).Split()[0]
$actual = (Get-FileHash .\neolint-v1.3.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "SHA-256 mismatch" }
```

Extract the archive and place both `neolint.exe` and `neolint-lsp.exe` in
a stable directory on the user `PATH`. Verify the CLI:

```powershell
neolint.exe --version
```

## Install the VS Code extension

The extension requires VS Code 1.85 or newer and the `neolint-lsp` executable
installed above.

1. Download `neolint-vscode-0.2.0.vsix` and its adjacent
   `.sha256` checksum from the
   [v1.3.0 GitHub release](https://github.com/mindwork64/neolint/releases/tag/v1.3.0).
2. Verify the VSIX checksum using the same platform-specific SHA-256 tool.
3. In VS Code, run **Extensions: Install from VSIX…** and select the downloaded
   file. Alternatively, run `code --install-extension neolint-vscode-0.2.0.vsix`.

The extension starts `neolint-lsp` from `PATH` by default. When the executable
is installed elsewhere, set `neolint.serverPath` to its absolute path in VS
Code settings. The VSIX contains its required Node.js runtime dependency; Node
is not required separately on the end-user machine.

For live Firestorm editing, install the official Second Life VSCode Plugin as
well. It remains responsible for Viewer connectivity and synchronization;
NeoLint runs alongside it and provides independent diagnostics and navigation.
See the [VS Code extension guide](../editors/vscode/README.md) for workspace
configuration and integration boundaries.

## Verify the release archive

The archive checksums are generated after packaging and refer to the exact
uploaded archive. The Linux and Windows packages include both executables, the MIT and
Apache project licenses, a generated third-party notices file for the locked
Rust dependency graph, the Linden Research MIT notice, the OpenSimulator
notice, and data provenance documentation.

Linux:

```sh
sha256sum -c neolint-v1.3.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Windows PowerShell:

```powershell
$expected = (Get-Content .\neolint-v1.3.0-x86_64-pc-windows-msvc.zip.sha256).Split()[0]
$actual = (Get-FileHash .\neolint-v1.3.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "SHA-256 mismatch" }
```

## First lint run

Create `hello.lsl`:

```lsl
default
{
    state_entry()
    {
        llOwnerSay("Hello");
    }
}
```

Run the linter on one file:

```sh
neolint hello.lsl
```

Run it on a directory (recursive, `.lsl` extension case-insensitive):

```sh
neolint ./scripts/
```

Run with JSON diagnostics:

```sh
neolint --format json hello.lsl
```

Read source from stdin:

```sh
cat hello.lsl | neolint -
```

The AST engine is the default. To use the historical analysis pipeline, pass
`--engine legacy` or set `engine = legacy` in the configuration file.
`--engine compare` is intended for maintainers.

See the [CLI reference](cli-reference.md) for every option and the
[configuration guide](configuration.md) for rule configuration. Normal linting
uses vendored metadata and does not access the network.
