# NeoLint for VS Code

This extension starts `neolint-lsp` and provides NeoLint diagnostics, hover,
definition navigation, and context-aware completion for local `.lsl` files.
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

The server loads `.neolint.conf` from the first VS Code workspace folder when
present. Relative `include_dir` paths are resolved relative to that config
file, matching the CLI's config-file behavior. A custom file can be selected
with `neolint.configPath` in VS Code settings:

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

The server supports local `file:` `.lsl` documents. When preprocessing is
enabled, configured include directories are searched first and the VS Code
workspace root is added as the final fallback; this avoids treating Firestorm's
temporary script directory as the project root. Open include buffers are used
as unsaved overlays, and include changes invalidate only the open roots that
depend on them. Each affected root is currently reanalyzed in full; incremental
parsing is not implemented. Per-script `neolint.project` overlays and
multi-root workspace-specific configuration remain future work. Diagnostics
originating in an included file are published only to the selected open URI
whose buffer supplied the analysis snapshot.
