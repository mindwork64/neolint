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

Each accepted root analysis records the root, loaded local include files, and
the searched local paths that can affect include resolution. This includes
missing candidates and higher-priority candidates preceding an already selected
include. Local file documents are tracked even when their extension is .inc or
.h; only .lsl files are analyzed as independent roots. Opening, editing, saving
or closing a document schedules the .lsl root itself (when applicable) and the
currently open roots whose last successful dependency set contains it.
Workspace file operations also invalidate roots for matching file or folder
paths. Since a first analysis may not have recorded its dependency set yet, a
file-operation batch also refreshes roots already pending or running in the
worker. Pending work remains coalesced per root and keeps FIFO order. Closing
a root removes its queued task, cancels it only when it is the active root, and
re-analyzes remaining dependents so diagnostics for a no-longer-open include are
withdrawn. The server advertises LSP workspace file operations for local files:
workspace/didCreateFiles, workspace/didRenameFiles, and
workspace/didDeleteFiles. These notifications invalidate roots whose include
search paths can be affected, including roots with a previously missing include.
A newly created include can therefore be picked up before the file is opened as
a text document. textDocument/didOpen remains a fallback for clients that do
not send workspace file-operation notifications. Shutdown signals cancellation
and does not wait for the worker thread.

A completed result is accepted only while the root version, the revisions
of the dependencies discovered by that task, and the workspace file-operation
generation still match the live state. Each create/rename/delete batch advances
a global generation before root selection. Results started before that batch
are discarded and their roots are requeued, including the narrow interval after
the worker finishes but before the LSP thread accepts its result. This generation
is intentionally conservative: an unrelated file operation can also cause an
in-flight result to be recomputed. An unrelated open-document edit or close does
not invalidate a result. A dependency that opens or closes while analysis is running is also detected: the
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
An include must exist as a regular local file before its contents can be
loaded. Include dependency tracking retains every searched candidate up to and
including the selected file. For a missing include, all candidate paths are
retained; for an include resolved in a later directory, missing higher-priority
candidates are retained too, so creating one can correctly shadow the old
selection. Workspace file-operation notifications trigger reanalysis for these
paths even when the file is not open in the editor.

The dependency index is bounded to 4096 distinct paths per analysis. If that
limit is exceeded, the affected root switches to conservative invalidation on
every local file operation, and stale-result validation covers the full open
local-document snapshot rather than an incomplete path set. Symlinked include candidates retain their canonical target, a missing-leaf
identity, and their full lexical path. Parent components remain intact until
existing path prefixes are canonicalized, so a spelling such as
alias/../shared.inc follows filesystem symlink traversal before applying the
parent-directory component. Deleting either a file symlink or a directory
symlink therefore still invalidates dependent roots even though the target
and its contents remain present. Open root documents also retain both their
canonical and lexical path identities; deleting or renaming a symlinked
parent directory therefore invalidates the root itself. Folder notifications
are compared against both canonical and lexical paths so nested includes
beneath a removed symlinked directory are also found.
When multiple open URI aliases point to one canonical
path, the lexicographically first URI supplies the contents and source mapping,
while all aliases remain part of stale-result validation. Resolved include
paths are recorded before their buffer contents are loaded, so an include that
temporarily exceeds the source-size limit remains watched and a later edit can
recover the dependent root. Included-file diagnostics are published only to
the selected open URI whose buffer supplied the snapshot.

## Hover and definition

The server advertises `textDocument/hover` and `textDocument/definition`.
Queries use a symbol index built on the background analysis worker and published
atomically with the corresponding diagnostics snapshot. The index is discarded
when a newer document analysis is scheduled, on document close, and whenever
its version or server-local revision no longer matches the open buffer.

The symbol index is built from the same effective prepared source as analysis,
so declarations in inactive preprocessor branches are excluded and macro-
expanded declarations participate in lookup. Source-map spans are projected
back to the open root buffer before they are exposed to the editor. Hover
resolves user-defined functions, globals, parameters and locals using the AST
lexical scope tree. It also provides signatures for functions/events and
type/value information for built-in constants from the selected target runtime
catalogue (including target-specific OpenSim APIs). Definition navigates to the
declaration span for symbols defined in the current script; runtime built-ins
have hover documentation but no source definition. Identifiers inside comments
or string literals are not treated as symbols. Cursor coordinates use LSP
UTF-16 positions and are converted to source UTF-8 offsets without splitting
Unicode scalar values or CRLF pairs.

The current navigation scope is intentionally limited to the open script.
Cross-file include navigation and workspace-wide symbol indexing are not yet
implemented.

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
