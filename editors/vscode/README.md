# NeoLint for VS Code

This extension starts `neolint-lsp` and provides NeoLint diagnostics, hover,
definition navigation, context-aware completion, and a safe `SEM041` quick fix
for local `.lsl` files.
Completion uses user-defined symbols and the selected Second Life or OpenSim
API catalogue, including function-call snippets when supported by the editor.
The extension does not provide syntax highlighting, compilation, or Viewer
synchronization. Keep the official Second Life VSCode Plugin installed for
Firestorm's external-editor and live-sync workflow; NeoLint runs beside it as
an independent language server.

## Requirements

- VS Code 1.85 or newer.
- A `neolint-lsp` executable available on `PATH`, or an explicit
  `neolint.serverPath` setting.

Build and install the server from the repository root:

```sh
cargo install --path . --bin neolint-lsp --features lsp
```

Build the extension:

```sh
cd editors/vscode
npm install
npm run compile
```

For a local VSIX package, run `npx @vscode/vsce package` from this directory,
then install the resulting `.vsix` through **Extensions: Install from VSIX**.

## Configuration

Configuration is selected independently for each open `.lsl` root. The
most-specific VS Code workspace folder containing the script is selected.
Within that folder, NeoLint discovers `neolint.project`: its `config =`
project-wide layer applies to every root in the folder, while an optional
`script_config = SCRIPT => CONFIG` overlay applies only to the matching listed
script. This also lets a Firestorm temporary copy inherit project defaults
without accidentally inheriting another script's private override. Without a
project manifest, `.neolint.conf` is used directly. Every script remains an
independent LSL compilation unit with its own symbols and analysis state.

Relative `include_dir` paths are resolved relative to the config file that
declares them. An optional `neolint.configPath` setting adds a final config
overlay to every script; relative paths are resolved against that script's
selected workspace folder. For files outside all folders in a multi-root
workspace, use an absolute path for a shared overlay; relative paths have no
unambiguous base and are not applied:

```json
{
  "neolint.serverPath": "/home/user/.cargo/bin/neolint-lsp",
  "neolint.configPath": ".neolint.conf"
}
```

If no config file is found, NeoLint's normal defaults apply. The extension's
`neolint.serverArgs` setting passes additional arguments to the server.

## Firestorm workflow

1. Install the official **Second Life VSCode Plugin** and this NeoLint extension.
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
extension synchronizes `.lsl`, `.inc`, `.h`, `.conf`, and `neolint.project`
files with the server. Each affected root is currently reanalyzed in full;
incremental parsing is not implemented. Diagnostics originating in an included
file are published only to the selected open URI whose buffer supplied the
analysis snapshot.
