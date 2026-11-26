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

An edit that changes the open-document snapshot signals cancellation to the
currently running analysis. The interrupted root is queued again against the
newest snapshot, and pending roots are refreshed without changing their FIFO
order. Closing a document cancels an active snapshot as well; the closed root is
removed while still-open roots are rescheduled. Shutdown signals cancellation
and does not wait for the worker thread.

The default prepared AST path checks cancellation while scanning source and
expanding directives/macros, during AST parsing, between semantic passes, and
while rendering diagnostics. Cancellation discards the whole in-progress
result: partial diagnostics are never published. The Legacy semantic pass and
a single filesystem operation (for example, reading one include file) and the
Firestorm preprocessing-off identity-map path remain non-preemptible; they finish
the current operation before observing cancellation.
The Compare engine likewise cannot interrupt its Legacy leg while that pass is
running. These limits do not weaken stale-result validation: a result is still
accepted only while every open-document revision and the open-document set
match the task snapshot. Every published diagnostic set carries the current
version of its own document.

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
