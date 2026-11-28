# NeoLint for VS Code

NeoLint starts the `neolint-lsp` executable and provides live LSL diagnostics,
hover, definition navigation, context-aware completion, and a safe
`SEM041` quick fix for local `.lsl` files. Completion uses user-defined
symbols and the selected Second Life or OpenSim API catalogue, including
function-call snippets when supported by the editor.

The extension does not provide syntax highlighting, compilation, cross-file
navigation, or Viewer synchronization. Keep the official Second Life VSCode
Plugin installed for Firestorm's external-editor and live-sync workflow;
NeoLint runs beside it as an independent language server.

## Install the release

Requirements:

- VS Code 1.85 or newer.
- The `neolint-lsp` executable from the matching NeoLint release archive,
  available on `PATH` or selected through `neolint.serverPath`.

The v1.3.0 release archive contains both `neolint` and `neolint-lsp` for
Linux x86_64 and Windows x86_64. Follow the
[Getting started guide](../../docs/getting-started.md) to install the binaries
and verify their checksums.

Download `neolint-vscode-0.2.0.vsix` from the same
[GitHub release](https://github.com/mindwork64/neolint/releases/tag/v1.3.0),
verify its adjacent SHA-256 file, then install it in VS Code using
**Extensions: Install from VSIX…** or:

```sh
code --install-extension neolint-vscode-0.2.0.vsix
```

The VSIX includes the production `vscode-languageclient` dependency. Node.js
is not required on the machine where the extension is installed.

## Build from source

From the repository root:

```sh
cargo build --release --features lsp --bin neolint-lsp
```

Then build and package the extension:

```sh
cd editors/vscode
npm ci
npm run check
npm run package
```

This creates `neolint-vscode-0.2.0.vsix` in this directory.

## Configuration

Configuration is selected independently for each open `.lsl` root. The
most-specific VS Code workspace folder containing the script is selected.
Within that folder, NeoLint discovers `neolint.project`: its `config =`
project-wide layer applies to every root in the folder, while an optional
`script_config = SCRIPT => CONFIG` overlay applies only to the matching listed
script. This also lets a Firestorm temporary copy inherit project defaults
without accidentally inheriting another script's private override. Without a
project manifest, `.neolint.conf` is used directly.

Relative `include_dir` paths are resolved relative to the config file that
declares them. An optional `neolint.configPath` setting adds a final config
overlay to every script; relative paths are resolved against that script's
selected workspace folder. For files outside all folders in a multi-root
workspace, use an absolute path for a shared overlay:

```json
{
  "neolint.serverPath": "/home/user/.cargo/bin/neolint-lsp",
  "neolint.configPath": ".neolint.conf"
}
```

If no config file is found, NeoLint's normal defaults apply. The
`neolint.serverArgs` setting passes additional arguments to the server.

## Firestorm workflow

1. Install the official **Second Life VSCode Plugin** and this NeoLint
   extension.
2. Configure Firestorm to use VS Code as the external script editor and enable
   VS Code Tight Integration if that is part of your normal workflow.
3. Open the project folder in VS Code before launching a script from Firestorm
   when workspace-level `.neolint.conf` or include paths are required.
4. Open the script from Firestorm. The official plugin retains responsibility
   for the Viewer WebSocket session and live synchronization; NeoLint analyses
   the `.lsl` file VS Code opens.

The server supports local `file:` `.lsl` documents and dynamic VS Code
workspace-folder changes. In a single-folder workspace, an external Firestorm
temporary script copy retains the historical fallback to that folder. In a
multi-root workspace, scripts outside every workspace folder are intentionally
not guessed to belong to the first project and receive workspace-independent
defaults (an absolute `neolint.configPath` overlay still applies).

When preprocessing is enabled for a selected workspace, configured include
directories are searched first and that workspace root is added as the final
fallback. Open include buffers are used as unsaved overlays, and include
changes invalidate only the open roots that depend on them. Configuration
files and the project manifest are tracked as dependencies too, so editing
their open buffers refreshes affected roots using the unsaved contents. The
extension synchronizes `.lsl`, `.inc`, `.h`, `.conf`, and
`neolint.project` files with the server. Each affected root is currently
reanalyzed in full; incremental parsing is not implemented. Diagnostics
originating in an included file are published only to the selected open URI
whose buffer supplied the analysis snapshot.

## Distribution scope

The VSIX is published as a GitHub Release asset and is installed manually.
Marketplace publication is not configured. The extension version is
maintained independently of the Rust CLI version; VSIX `0.2.0` is packaged
with NeoLint CLI/LSP release `1.3.0`.
