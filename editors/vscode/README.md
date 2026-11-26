# NeoLint for VS Code

This extension starts `neolint-lsp` and displays NeoLint diagnostics for local
`.lsl` files. It does not provide syntax highlighting, snippets, viewer
synchronization, compilation, or an LSL built-in catalogue. Keep the official
Second Life VSCode Plugin installed for Firestorm's external-editor and
live-sync workflow; NeoLint runs beside it as an independent language server.

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

The initial server supports local `file:` `.lsl` documents. When preprocessing
is enabled, configured include directories are searched first and the VS Code
workspace root is added as the final fallback; this avoids treating Firestorm's
temporary script directory as the project root. Included files are read from
disk by the existing preprocessor. Diagnostics originating in an included file
are published only when that file is also open in VS Code and its buffer matches
the disk content at the time its parent is analyzed. Unsaved include overlays,
`neolint.project` per-script overlays, multiple workspace roots, and incremental
dependency invalidation are not part of this first milestone.
