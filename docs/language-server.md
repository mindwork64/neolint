# VS Code language server

NeoLint provides a small LSP server through the `neolint-lsp` executable. The
server is an adapter over the existing Rust analysis API; it does not contain a
second lexer, parser, semantic engine, or set of lint rules.

## Scope of the first milestone

- JSON-RPC over standard input/output using LSP framing.
- Initialize/shutdown/exit and open/change/save/close document lifecycle.
- Full-text document synchronization with monotonically increasing document
  versions. Older `didChange` versions are ignored.
- Version-tagged `publishDiagnostics` notifications.
- Correct UTF-8 byte-offset to UTF-16 LSP position conversion, including CR,
  LF, CRLF and supplementary Unicode characters.
- Configured engine, target, disabled rules, severity overrides, and preprocessor
  settings through `LintConfig`.
- Workspace `.neolint.conf` discovery or an explicit `neolint.configPath`.

Analysis runs on a single background worker so parsing a large script does
not block the JSON-RPC event loop from receiving edits, close notifications or
shutdown. Pending work is coalesced per root document: a newer snapshot replaces
an older queued snapshot for that root.

An analysis already in progress is not yet cooperatively interruptible inside
the analysis engine. Its result is discarded if any open-document snapshot or
the set of open documents changed while it was running; the latest snapshot is
then queued. Every published diagnostic set carries the current version of its
own document. Thus an older analysis cannot overwrite diagnostics for a newer
root or included-file buffer, although the obsolete CPU work may finish before
the latest task starts.

The initial workspace model uses one workspace configuration. When
preprocessing is enabled, configured include directories are searched first and
the workspace root is appended as the final fallback. This is important for
Firestorm, which opens a temporary copy outside the workspace directory. The
server does not yet apply `neolint.project` per-script overlays, multi-root
workspace-specific configuration, or unsaved buffers for `#include`
dependencies. The preprocessor continues to read include files from disk. A
diagnostic from an included file is published only when that file is open and
its buffer matches the disk text at the time its parent script is analyzed.

## Build and run

```sh
cargo test --all-targets --all-features
cargo install --path . --bin neolint-lsp --features lsp
cd editors/vscode
npm install
npm run compile
```

See [VS Code extension README](https://github.com/mindwork64/neolint/blob/main/editors/vscode/README.md) for setup and the
Firestorm external-editor workflow.
