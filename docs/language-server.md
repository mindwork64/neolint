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

Each accepted root analysis records the local source files it actually loaded
(root plus resolved `#include` files). Local file documents are tracked even when
their extension is `.inc` or `.h`; only `.lsl` files are analyzed as independent
roots. Opening, editing, saving or closing a document schedules the `.lsl` root
itself (when applicable) and the currently open roots whose last successful
dependency set contains it. The active worker is cancelled
only when its root is among those affected; unrelated roots continue running.
Pending work remains coalesced per root and keeps FIFO order. Closing a root
removes its queued task, cancels it only when it is the active root, and
re-analyzes remaining dependents so diagnostics for a no-longer-open include are
withdrawn. Shutdown signals cancellation and does not wait for the worker thread.

A completed result is accepted only while the root version and the revisions
of the dependencies discovered by that task still match the live document
snapshot. An unrelated open-document edit or close does not invalidate it. A
dependency that opens or closes while analysis is running is also detected: the
task records an empty open-URI revision set for a dependency not open at start,
which no longer matches when a buffer for that path is opened. If analysis fails
before dependency discovery completes,
the last successful dependency set is retained.

The default prepared AST path checks cancellation while scanning source and
expanding directives/macros, during AST parsing, between semantic passes, and
while rendering diagnostics. Cancellation discards the whole in-progress
result: partial diagnostics are never published. The Legacy semantic pass and
a single filesystem operation (for example, reading one include file) and the
Firestorm preprocessing-off identity-map path remain non-preemptible; they finish
the current operation before observing cancellation.
The Compare engine likewise cannot interrupt its Legacy leg while that pass is
running. These limits do not weaken dependency-scoped stale-result validation: a
result is accepted only while the root version and every dependency-path open-URI
revision signature match the task snapshot. Unrelated open documents are ignored.
Every published diagnostic set carries the current version of its own document.

The initial workspace model uses one workspace configuration. When
preprocessing is enabled, configured include directories are searched first and
the workspace root is appended as the final fallback. This is important for
Firestorm, which opens a temporary copy outside the workspace directory. The
server does not yet apply per-script project overlays or multi-root
workspace-specific configuration.

For every analysis task, open local file buffers are captured in an immutable
snapshot. When a resolved include path matches an open local document, the
preprocessor uses that buffer's text instead of reading file contents from
disk. Diagnostics and macro expansion therefore reflect unsaved include edits.
The path must still resolve to an existing regular local file; new files that
do not yet exist on disk are not discoverable as includes. When multiple open
URI aliases point to one canonical path, the lexicographically first URI
supplies the contents and source mapping, while all aliases remain part of
stale-result validation. Resolved local include paths are recorded as
dependencies before their buffer contents are loaded, so an include that
temporarily exceeds the source-size limit remains watched and a later buffer
edit can recover the dependent root. Included-file diagnostics are published
only to the selected open URI whose buffer supplied the snapshot.

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
