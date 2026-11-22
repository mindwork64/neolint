# Preprocessor and AST Architecture

Status: **architecture contract for the experimental `exp/preprocessor-ast` branch**.
Baseline: `98556e21e175190c95f2da39fc0e2ba7a8d9544c`.
Scope: Phase 0. No runtime behavior changes are included in this document.
Audience: maintainers and contributors who will implement Phases 1–6.

This document is the normative source of context for the preprocessor and AST
work. It records the current pipeline, the target pipeline, the contracts between
stages, the provenance model, the error and resource policies, the compatibility
rules, the module plan, and the acceptance criteria for every phase. If an
implementation decision conflicts with this document, the document must be
updated first (or the conflict must be recorded as an explicit open question).

---

## 0. Goals and non-goals

### Goals

1. Add a Firestorm-compatible preprocessor that runs **before** the existing lexer.
2. Add an AST parser and an AST semantic analyzer that can gradually replace the
   token-scanning legacy engines.
3. Keep both engines on the **same prepared token stream** so their results are
   directly comparable.
4. Preserve exact legacy behavior in the default (plain LSL) mode.
5. Make every diagnostic traceable to real user code through a source map.
6. Keep the project dependency-free, offline, deterministic, and panic-free on
   malformed input, with MSRV 1.75.

### Non-goals for Phase 0

- No macro expansion, include handling, runtime source map, AST types, switch,
  lazy lists, or new SEM rules.
- No change to the existing lexer/parser/semantic behavior.
- No change to the release workflow or the stable `main` branch.
- No new dependencies.

### 0.1 Fixed decisions

These decisions are binding for all phases. Changing one requires an explicit
revision of this document.

1. **The preprocessor is independent of the AST.** It never imports AST types
   and never performs semantic analysis.
2. **The preprocessor runs before the existing lexer.** There is no second
   tokenization model for directives.
3. **Both engines receive the same `PreparedSource`/token stream.** No engine
   preprocesses on its own.
4. **The legacy API is preserved for the transition period.** `lex`, `parse`,
   `analyze`, `analyze_with_syntax_diagnostics`, `Diagnostic`, `Span`, `Token`,
   and `TokenKind` keep their signatures and meanings.
5. **Plain LSL is the default mode.** Preprocessing is off unless explicitly
   enabled.
6. **The Firestorm preprocessor is explicitly enabled.** It is never implied by
   the input file name, extension, or content.
7. **Firestorm extensions are independently gated.** Preprocessing does not
   enable `switch`, lazy lists, `USE_SWITCHES`, or `USE_LAZY_LISTS`.
8. **No network includes.** Remote paths are rejected as source errors.
9. **No fabricated values for dynamic Firestorm built-ins.** Unknown values stay
   unresolved and are handled conservatively.
10. **Determinism is mandatory.** Same inputs produce byte-identical output,
    including diagnostic order.
11. **Cycle, recursion, and expansion-size protection is mandatory.** Include
    cycles, recursive macros, and generated-size bombs are detected and bounded.
12. **Malformed user code must never panic.** Internal invariant failures degrade
    to diagnostics or documented bounded fallbacks.
13. **MSRV remains 1.75.** New code must not use newer standard-library APIs
    without an explicit decision and a CI check.
14. **No new dependencies without a technical justification and separate
    approval.** The default crate stays dependency-free.

---

## 1. Current pipeline (A)

```
CLI arguments
  → LintConfig (config file, --disable, --severity)
  → input collection (files, directories, stdin)
  → read whole file as UTF-8 String
  → lex(&source)            -> Vec<Token>, Vec<Diagnostic> (LEX)
  → parse(&tokens)          -> Vec<Diagnostic> (PAR)
  → analyze_with_syntax_diagnostics(&tokens, &syntax_diagnostics)
                            -> Vec<Diagnostic> (SEM)
  → config.apply(&mut diagnostics)
  → human / JSON rendering
  → exit code 0 | 1 | 2
```

Current stage contracts, as implemented at the baseline:

- `lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>)` in `src/lexer.rs`.
  - The token stream always ends with `TokenKind::Eof`.
  - `Token { kind, lexeme: String, span: Span }`.
  - `Span` stores zero-based UTF-8 byte offsets (`start`, `end`) plus one-based
    `line`, `column`, `end_line`, `end_column`.
  - Line breaks are recognized by `\n`; `\r` is whitespace. Columns count
    Unicode scalar values, not bytes and not display cells.
  - Whitespace and comments (`//`, `/* ... */`) are discarded.
  - There is no `#` handling: `#` produces `LEX000` "unexpected character".
- `parse(tokens: &[Token]) -> Vec<Diagnostic>` in `src/parser.rs`.
  - Hand-written recursive descent over token indices; **no AST**.
  - Requires the `Eof` sentinel; recovery uses delimiter-aware synchronizers
    (`synchronize_top_level`, `synchronize_statement`, `synchronize_arguments`,
    and related helpers).
  - Emits `PARxxx` diagnostics with `DiagnosticKind::Syntax`.
- `analyze_with_syntax_diagnostics(tokens, syntax_diagnostics)` in
  `src/semantic.rs`.
  - Multiple independent token scans in a fixed pass order.
  - Diagnostics are **not deduplicated**; output order equals pass order.
  - Built-in call and user-call arity/type checks are suppressed only for
    argument lists that already contain a syntax error.
- `Diagnostic { code: &'static str, severity, kind, message, span }` in
  `src/diagnostic.rs`.
  - There is no source/file identity in a diagnostic. The CLI renders the file
    path from the input loop.
- `LintConfig` in `src/config.rs` supports only `disable = ...` and
  `severity.RULE = ...`; rule codes must match `[A-Z0-9]+`.
- Built-in metadata is vendored (`data/lsl-builtins.txt`,
  `data/lsl-deprecated.txt`) and loaded through `include_str!` + `OnceLock`;
  there is no network access.
- The existing test suite (350 tests) plus `corpus/manifest.txt` is the
  compatibility contract.

Consequences that shape the new architecture:

1. Any transformation of source text before lexing shifts `Span` coordinates.
   The legacy path must therefore stay byte-identical when preprocessing is off.
2. `Diagnostic` cannot express a file, an include stack, or a macro chain. The
   resolution layer must map prepared-code coordinates back to a real file
   **before** diagnostics are rendered.
3. The parser and semantic analyzer depend on token order and on `TokenKind`
   values, not on source text. A preprocessed stream is acceptable to both as
   long as it is a valid, ordered `Vec<Token>` ending in `Eof`.

---

## 2. Target pipeline (B)

```
              +-------------------+
  CLI/config  |   Source Manager  |
              +---------+---------+
                        | LoadedSource: SourceId + text + kind
                        v
              +-------------------+
              |   Preprocessor    |  (identity in Phase 1; core in Phases 2–3)
              +---------+---------+
                        | PreparedSource { text, SourceMap, preprocessor diagnostics }
                        v
              +-------------------+
              |      Lexer        |  (unchanged tokenization rules)
              +---------+---------+
                        | Token stream (prepared-buffer Spans + Eof)
           +------------+-------------+
           v                          v
  +------------------+       +------------------+
  |  Legacy Engine   |       |    AST Engine    |
  | parser+semantic  |       | AST parser+SEM   |
  +--------+---------+       +--------+---------+
           |                          |
           +-------------+------------+
                         v
              +-------------------+
              | Unified Diagnostics|  resolve spans through SourceMap,
              | + config filtering |  merge, deduplicate policy, render
              +---------+---------+
                        v
                 human / JSON / exit code
```

The AST engine and the legacy engine are alternatives at the top of the funnel
but share everything before the funnel. No engine may preprocess on its own.

---

## 3. Component responsibilities (C)

### Source Manager (`src/source/`)

- Assigns stable `SourceId` values for one analysis run.
- Loads the root source (file or stdin) and include files on request.
- Owns file text, display names, and line indices.
- Enforces path policy (local files only, no network, no directory includes).
- Detects include cycles through the active include stack.
- Enforces file-size and file-count limits.
- Never tokenizes and never interprets LSL or preprocessor syntax.

### Preprocessor (`src/preprocessor/`)

- Scans raw text for directives, comments, and string/character boundaries.
- Maintains the macro table, conditional stack, and include expansion.
- Produces exactly one prepared text buffer per root analysis.
- Produces the `SourceMap` that maps prepared ranges to original code.
- Emits `PRE` diagnostics for directive and include problems.
- Does not know about tokens, AST nodes, parser recovery, or semantic rules.
- Does not perform semantic type checking or built-in argument checks.

### PreparedSource (`src/preprocessor/`)

- The single text buffer handed to the lexer.
- Carries `Arc<SourceMap>` and preprocessor diagnostics.
- Is immutable after construction.
- In identity mode, `text` is byte-identical to the input file.

### Lexer (`src/lexer.rs`)

- Unchanged tokenization: keywords, identifiers, literals, operators, `Eof`.
- Produces `Span` values **relative to the prepared buffer**.
- Does not see directives (they are removed or rewritten by the preprocessor)
  and does not need to know that a map exists.
- Continues to discard comments and whitespace, and to reject `#` outside
  directives exactly as today when no preprocessor is active.

### Legacy Engine (`src/parser.rs`, `src/semantic.rs`)

- Consumes the same `Vec<Token>` as the AST engine.
- Keeps its current diagnostic codes, severities, spans, and pass order in
  legacy mode.
- Is not modified to understand macros or includes. It sees only the prepared,
  expanded token stream.

### AST Engine (`src/ast/`, `src/ast_semantic/`)

- Builds typed nodes with `Span` (and later origin identifiers) preserved.
- Parses the same prepared token stream with equivalent recovery behavior.
- Performs semantic analysis on the AST, gradually taking over SEM rules.
- Never runs include resolution or macro expansion itself.

### Unified Diagnostics (new layer, introduced with CLI integration)

- Resolves prepared-buffer spans through `SourceMap` to file/line/column.
- Merges diagnostics from both engines in a deterministic order.
- Applies `LintConfig` exactly once.
- Renders human and JSON output; owns the exit-code decision.
- Keeps the primary rendered location in the existing `Diagnostic` shape and
  carries secondary locations separately (see §7).

### CLI / configuration (`src/main.rs`, `src/config.rs`)

- Chooses the dialect profile and the engine.
- Keeps plain LSL as the default with preprocessing disabled.
- Maps CLI/config failures to exit code 2 and lint findings to 0/1.
- Validates all new options without panicking.

---

## 4. Contracts between components (D)

All contracts below are normative. Exact Rust signatures are finalized in
Phase 1, but the semantics must not drift.

### 4.1 CLI → Source Manager

Input: root path, stdin marker, or nothing. Output: `SourceId` for a loaded root
plus its text, or an `InputError` that maps to exit code 2.

Invariants:

- The root is read as UTF-8; invalid UTF-8 is an input error (exit 2).
- A UTF-8 BOM is **not** silently stripped (today it is a lexical error; this
  behavior is preserved unless a later phase changes it explicitly).
- stdin is read at most once per run.
- `SourceId` values are dense and assigned in load order.

### 4.2 Source Manager → Preprocessor

Input: a `LoadedSource` (a `SourceId`, the file text, and its display name);
include requests carry the requesting `SourceId` and a path/spelling. Output: a
resolved include or a `PRE` diagnostic.

Invariants:

- Include resolution is relative to the including file's directory (and to
  explicit `-I` roots when introduced); it is never relative to the process CWD
  by accident.
- Network paths and URLs are rejected as source-level diagnostics.
- Canonical paths are used for cycle detection and de-duplication, but the
  user-facing spelling is preserved for messages.
- Include depth and total file count are bounded (§12).

### 4.3 Preprocessor → Lexer

Input: exactly one `PreparedSource`. Output: one token stream.

Invariants:

- The prepared text is valid UTF-8.
- The prepared text is produced deterministically from the same inputs.
- Removing directives must not merge adjacent tokens in a way that changes
  their meaning; the implementation inserts safe separation where needed.
- The lexer's `Eof` position equals the prepared text length.

### 4.4 Preprocessor → Source Map

Every generated byte range in the prepared buffer has an origin segment:

- Segments are sorted by generated start, are non-overlapping, and cover the
  whole prepared buffer. Bytes with no direct user counterpart (for example
  inserted separators) still have a segment, with origin `Generated` pointing at
  the nearest enclosing user location; there are no unmapped holes.
- A segment maps to at least one `SourceOrigin`; the origin chain is acyclic.
- Direct (unmodified) input has a single identity segment per file.

### 4.5 Lexer → Engines

Input: `&[Token]` ending in `Eof`. Output: diagnostics.

Invariants:

- Token order is the prepared text order.
- Every token span lies inside the prepared buffer and on UTF-8 boundaries.
- Neither engine may assume that byte offsets are monotonic *in the original
  files*; monotonicity holds only in the prepared buffer.
- Diagnostics use prepared-buffer spans; the unified layer resolves them.

### 4.6 Engines → Unified Diagnostics

Input: diagnostics with prepared-buffer spans (AST/legacy) or already-original
spans (legacy compatibility path). Output: rendered diagnostics grouped by the
original file.

Invariants:

- Every rendered diagnostic has a real file, line, column, and byte range in that
  file.
- Secondary locations (macro definition, include chain) never replace the
  primary location.
- Config filtering runs once, after merging.

### 4.7 Public library API

The existing public items are frozen for the transition period:

```rust
pub fn lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>);
pub fn parse(tokens: &[Token]) -> Vec<Diagnostic>;
pub fn analyze(tokens: &[Token]) -> Vec<Diagnostic>;
pub fn analyze_with_syntax_diagnostics(tokens: &[Token], syntax: &[Diagnostic]) -> Vec<Diagnostic>;
pub use diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
```

New entry points are **additive**:

```rust
// conceptual, finalized in Phase 1/3
pub fn prepare(source: LoadedSource, config: &PreprocessConfig) -> PreparedSource;
pub fn analyze_prepared(prepared: &PreparedSource, options: &EngineOptions) -> RunDiagnostics;
```

`lex`, `parse`, and `analyze` remain the legacy path and must not acquire
preprocessing side effects.

---

## 5. Lifecycle of one analysis (E)

1. **Argument parsing.** Parse CLI options; reject unknown combinations with
   exit code 2. No file is touched yet.
2. **Configuration.** Load `LintConfig`; validate rule codes; resolve dialect
   profile and engine selection; reject unsupported combinations (for example
   Firestorm extensions without a Firestorm dialect).
3. **Input collection.** Expand directories deterministically (sorted, symlinks
   skipped), de-duplicate displayed paths, enforce one stdin.
4. **Root load.** Read the root as UTF-8; register it in the Source Manager.
5. **Preparation.** Preprocess the root:
   - identity mode: copy text unchanged, build a trivial map;
   - Firestorm mode: scan directives, expand macros, resolve includes in textual
     order, emit `PRE` diagnostics, build the full map.
   Failures inside preparation become diagnostics, not Rust errors, except for
   limits that make further analysis unsafe (still diagnostics; the run
   continues with a bounded, truncated prepared stream).
6. **Lexing.** Tokenize the prepared text. Lexical diagnostics are recorded with
   prepared spans.
7. **Engine selection.**
   - `legacy` (default in Phase 1–5): `parse` → `analyze_with_syntax_diagnostics`.
   - `ast` (Phase 4+): AST parser → AST semantic analyzer.
   - `compare` (development/CI only, Phase 6): both, then differential report.
8. **Resolution.** Map each diagnostic's prepared span to `SourceSpan`, then to
   `SourceLocation` (file, offset, line, column). Attach secondary locations
   from the origin chain where available.
9. **Merging.** Apply the engine-specific deduplication policy, then sort by
   `(source load order, start offset, code)`. Legacy single-file runs keep their
   current order.
10. **Configuration.** Apply disabled rules and severity overrides once.
11. **Rendering.** Human output or JSON; exit code 0/1. Input/config failures
    return 2 as today.
12. **Teardown.** Drop the run; no global state survives. Built-in metadata stays
    cached in its existing `OnceLock`.

Empty input: the preprocessor produces an empty prepared buffer, the lexer
produces only `Eof`, and the legacy parser reports `PAR013` exactly as today.

---

## 6. Multiple source files (F)

- A run has exactly one **root** file (or stdin). Every other file is an include.
- `SourceId` order is load order: root first, then includes in the order they are
  first entered (depth-first, textual order). This order is the diagnostic
  grouping order.
- `SourceFile` records: `id`, display name, optional canonical path, kind
  (`Root`, `Included`, `Stdin`, `Prepared`), and whether it is synthetic.
- The **prepared buffer is itself a `SourceFile`** of kind `Prepared`. Prepared
  spans are offsets into it; original spans are offsets into root/include files.
  Keeping the prepared buffer as a pseudo-file makes token spans unambiguous and
  lets the `SourceMap` be a range map instead of a per-token table.
- Include resolution:
  - textual position matters: an include is expanded at its directive site;
  - the same file included twice is expanded twice unless a guard or
    `#pragma once` suppresses it; the Source Manager still loads it once and
    reuses the text (one `SourceId`, many expansions);
  - cycles are detected on the active include stack using canonical paths and
    produce a `PRE` error at the `#include` site; the include is not entered;
  - unresolved includes produce one `PRE` error and no tokens.
- Diagnostics are grouped per originating file for rendering. A diagnostic whose
  origin chain crosses files keeps its primary location in the file where the
  user wrote the code and lists secondary locations for the include chain.
- CLI display paths: the root keeps the path exactly as supplied by the user;
  includes are displayed relative to the including file when possible, otherwise
  as resolved, never as absolute paths unless the user supplied one.
- Determinism: include expansion follows textual order only. Filesystem
  enumeration order is never used for includes.

---

## 7. Provenance and source mapping (G)

The source map is a **range map with origin chains**, not a line-to-line table.

### 7.1 Conceptual types

```rust
/// Dense identifier of one loaded source file within a run.
pub struct SourceId(u32);

pub enum SourceFileKind { Root, Stdin, Included, Prepared }

pub struct SourceFile {
    pub id: SourceId,
    pub display_name: String,
    pub canonical_path: Option<PathBuf>,
    pub kind: SourceFileKind,
    pub text: Arc<str>,
    // line index is built lazily and cached
}

/// A range in an original file. Offsets are zero-based UTF-8 bytes.
pub struct SourceSpan {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

/// A resolved human-readable position in an original file.
pub struct SourceLocation {
    pub source: SourceId,
    pub offset: usize,
    pub line: u32,   // 1-based
    pub column: u32, // 1-based Unicode scalar count, matching the lexer
}

pub enum OriginKind {
    Direct,          // unmodified code copied from a file
    Include,         // code entered through #include
    MacroInvocation, // the macro call site
    MacroDefinition, // the macro body
    MacroArgument,   // an argument substituted into the body
    TokenPaste,      // result of ##
    Stringify,       // result of #
    Generated,       // text with no direct user counterpart
}

/// One link in a provenance chain.
pub struct SourceOrigin {
    pub kind: OriginKind,
    pub span: SourceSpan,          // primary location for this link
    pub invocation: Option<SourceSpan>,
    pub definition: Option<SourceSpan>,
    pub parent: Option<OriginId>,  // chain towards the root invocation
    pub label: Option<&'static str>,
}

/// A mapped range in the prepared buffer.
pub struct MapSegment {
    pub generated_start: usize,
    pub generated_end: usize,
    pub origin: OriginId,
}

pub struct SourceMap {
    files: Vec<SourceFile>,
    segments: Vec<MapSegment>, // sorted, non-overlapping
    origins: Vec<SourceOrigin>, // arena; chains are acyclic
}
```

These are conceptual definitions. Phase 1 fixes the exact visibility, derives,
constructors, and capacities; the semantics below are binding.

### 7.2 Identification and offsets

- `SourceId` is run-local. It must never be persisted or compared across runs.
- Every offset is a zero-based UTF-8 byte offset. Spans are half-open
  (`start..end`) and must lie on UTF-8 character boundaries.
- `SourceSpan` never refers to the prepared buffer. The prepared buffer is
  addressed by `generated_start..generated_end` in `MapSegment` and by `Span` in
  tokens.
- A `SourceSpan` with `start == end` is a point (used for "inserted here"
  origins) and is allowed.

### 7.3 Lines and columns

- Line indices are built per file from the file's own bytes. `\n` increments the
  line; `\r` is ignored for line counting, matching the lexer's current
  behavior.
- Columns count Unicode scalar values from the beginning of the line, starting
  at 1, matching the lexer's current implementation.
- `SourceLocation` is computed from `SourceSpan.start` on demand and cached in
  the file's line index. Byte offsets remain the canonical identity; line and
  column are derived data.
- The lexer's `Span.line`/`Span.column` describe the prepared buffer. They are
  used for internal ordering and debugging; rendered diagnostics use the
  original file location.

### 7.4 Mapping model

- The prepared buffer is covered by ordered, non-overlapping segments.
- Every segment has exactly one **primary origin** (`MapSegment.origin`) and an
  optional chain of **secondary origins** reachable through
  `SourceOrigin.parent`, `invocation`, and `definition`.
- The primary origin answers "where should the user look to fix this?".
  Secondary origins answer "how did this code get here?" and are rendered as
  notes, never as the diagnostic's main location.
- Direct code: one segment covering the copied bytes, origin `Direct`.
- Include: the included file's copied bytes carry origin `Include` with the
  include directive site recorded as the invocation and the included file range
  as the span. The include directive site is the secondary origin, so a finding
  inside an include can also point back at the `#include` line.
- Macro invocation: tokens produced by a macro body map to the body range with
  origin `MacroDefinition`, whose `invocation` points at the call site.
- Macro argument substitution: the argument's own bytes map to the argument
  range with origin `MacroArgument`, whose `invocation` points at the macro call
  and whose `parent` chain continues to the definition.
- Nested expansion: the chain is body-of-outer → invocation-of-inner →
  body-of-inner, bounded by the expansion depth limit. The chain is walked
  outwards for "note:" rendering; the primary location is the innermost
  user-written span, preferring an argument location over a definition location.
- Token paste and stringification: the resulting bytes have no exact original
  counterpart. Their primary origin is the invocation site; the definition and
  the operand ranges are recorded as secondary links with kind
  `TokenPaste`/`Stringify`. The renderer must not claim that the pasted text
  exists literally in a file.
- Generated text (for example inserted separators): origin `Generated` with the
  nearest enclosing user location, so diagnostics never point at offset 0 by
  accident.

### 7.5 Multiple related locations

A diagnostic carries:

```rust
pub struct RelatedLocation {
    pub span: SourceSpan,
    pub label: String,
}

pub struct MappedDiagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub primary: SourceSpan,
    pub related: Vec<RelatedLocation>,
}
```

- `primary` is the location the user should fix.
- `related` is ordered from most to least relevant: macro definition, include
  site, outer invocations.
- The existing `Diagnostic` remains the rendered single-location form. The
  unified layer converts `MappedDiagnostic` into one `Diagnostic` per primary
  location and renders `related` as additional note lines in human output.
- JSON keeps the existing object shape for the primary location. If related
  locations are exposed in JSON, they are added as an optional `related` array
  in Phase 3 together with a documented schema note; existing fields never
  change meaning.
- Related locations are resolved to the same `file`/line/column form and are
  deduplicated if several links point at the same span.

### 7.6 Why not a line map

Line mapping cannot represent: several macro arguments on one line, a macro
expanding to zero or many lines, token paste, includes that overlay multiple
files, or AST nodes that span a macro boundary. Range-based segments with
explicit chains are required, and they are usable both for tokens (range
lookup) and for future AST nodes (span lookup).

### 7.7 Lookup and performance

- Segment lookup is a binary search over sorted generated ranges: `O(log n)`.
- Origin chains are an arena; walking is `O(depth)` with a hard depth cap.
- Each file's line index is built once per run (`O(file bytes)`), not per
  diagnostic.
- The map is immutable and shared with `Arc`; there is no interior mutability in
  the analysis path.

---

## 8. Error handling per stage (H)

| Stage | Failure | Behavior | Exit code |
| --- | --- | --- | --- |
| CLI | Unknown option, bad `--severity`, bad `--format` | Message on stderr, no analysis | 2 |
| Config | Malformed setting, invalid rule code, unknown key | Message on stderr | 2 |
| Input collection | Path metadata failure, empty directory | Message on stderr | 2 |
| Root read | Missing file, permission, invalid UTF-8 | Message on stderr | 2 |
| Source Manager | Include missing/unreadable/invalid UTF-8/not a file | `PRE` error diagnostic at the include site, analysis continues | 1 |
| Preprocessor | Unknown directive, malformed macro, unterminated conditional, bad `#if` expression, path rejection | `PRE` error diagnostic, best-effort recovery; the construct is skipped | 1 |
| Preprocessor | Include cycle, expansion depth/size limit, diagnostic limit | `PRE` error diagnostic once, expansion stopped at the limit | 1 |
| Lexer | Unterminated string/comment, unsupported character | `LEX` diagnostic, tokens recovered | 1 |
| Parser | Syntax error | `PAR` diagnostic, bounded recovery | 1 |
| Semantic | Rule violation | `SEM` diagnostic | 1 |
| Any stage | Internal invariant violation | Must degrade to a diagnostic or a bounded, documented fallback; never `panic!`, `unwrap`, or unchecked slicing on user input | 1 |
| Both engines | `compare` mode disagreement | Development/CI failure only; never a user-facing panic | n/a |

Rules:

- User input is untrusted data. Indexing user-controlled offsets must go through
  checked accessors; arithmetic on offsets uses checked or saturating operations.
- `PRE` diagnostics are `DiagnosticKind::Lexical`? **No** — introduce a dedicated
  kind only if the documentation and JSON schema are extended. Until then `PRE`
  diagnostics use `DiagnosticKind::Syntax` with a distinct code family and are
  documented as preprocessing diagnostics. The final kind decision is part of
  Phase 3 CLI integration and must be recorded here.
- `PRE` diagnostics use the existing `DiagnosticKind::Syntax` and the `PRE`
  code family. A dedicated `DiagnosticKind` is deliberately deferred: adding a
  value to that public enum changes the documented JSON `kind` domain and needs
  its own compatibility decision (Phase 3, recorded here when decided).
- Every new diagnostic code must be added to `docs/diagnostics.md`. The
  documentation test currently scans only `src/lexer.rs`, `src/parser.rs`, and
  `src/semantic.rs`; Phase 2 must extend its source list to the preprocessor
  module (and Phase 4 to the AST modules) so new codes cannot go undocumented.
- Preprocessor diagnostics that prevent meaningful analysis (for example a
  missing include) do not abort the run: the rest of the file is still analyzed
  and independent findings are still reported.

---

## 9. Backward compatibility (I)

1. **Default mode is unchanged.** Without an explicit preprocessor flag, the
   pipeline is `lex → parse → analyze` with the same input text. No identity
   preprocessing runs implicitly in a way that could alter bytes.
2. **Public API is frozen.** `lex`, `parse`, `analyze`,
   `analyze_with_syntax_diagnostics`, `Diagnostic`, `Span`, `Token`, and
   `TokenKind` keep their signatures and field meanings for the transition
   period. New functionality is additive.
3. **Diagnostics are unchanged in plain LSL.** Same codes, severities, kinds,
   messages, spans, order, human output, JSON output, and exit codes. The
   existing 350 tests and the corpus manifest are the gate.
4. **`--tokens` output** continues to print the tokens of the prepared stream
   (which equals the input text in legacy mode).
5. **Configuration** gains new keys only in an additive way. Unknown keys remain
   an error (exit 2). Existing config files keep working unchanged.
6. **JSON** retains existing fields and meanings. Any extension is additive and
   documented; no field is renamed or repurposed.
7. **Firestorm is opt-in.** Both the preprocessor and the extension syntax have
   separate explicit switches. Enabling the preprocessor must not enable
   `switch`, lazy lists, `USE_SWITCHES`, or `USE_LAZY_LISTS`.
8. **Compatibility gate.** Before any commit that touches the legacy path, run
   the full test suite, the corpus suite, `fmt`, `clippy -D warnings`, and
   `cargo doc -D warnings`. Phase 1 adds a byte-identity test for the identity
   preprocessor; Phase 3 adds an end-to-end golden test that plain LSL with the
   preprocessor enabled but no directives produces the same diagnostics as
   legacy.
9. **Deprecation** of legacy APIs happens only after Phase 6 parity gates; until
   then both engines are supported and selectable.

---

## 10. LSL and Firestorm modes (J)

### 10.1 Dialects

| Dialect | Preprocessor | Switch/lazy lists | Default |
| --- | --- | --- | --- |
| `Lsl` | off | off | yes |
| `Lsl` + explicit preprocess | on | off | no |
| `Firestorm` | on | opt-in per extension flag | no |

- Dialect selection is explicit (`--dialect lsl|firestorm` or equivalent,
  finalized in Phase 3) and rejected when combined with incompatible options.
- Preprocessing and Firestorm extensions are independent gates. A user may want
  `#define` without `switch`, and the architecture must allow that.
- Firestorm mode must not silently accept standard-LSL-invalid constructs that
  are not documented Firestorm behavior.

### 10.2 `//fspreprocessor off`

- The directive disables preprocessing for a region. It requires raw comment
  access, so it is handled by the preprocessor scanner before the lexer.
- When preprocessing is off, the region is copied verbatim and lexed as normal
  LSL; `#` there follows normal lexical rules (and is an error in plain LSL).
- The region boundaries are recorded in the source map as direct code.

### 10.3 Built-ins and predefined macros

- Only documented, statically known predefined macros may be provided (for
  example file/line style macros defined by the chosen profile). Their values
  are computed from the source map, never invented.
- No Firestorm built-in may receive a fabricated constant value. Dynamic or
  environment-dependent built-ins stay unresolved and are treated
  conservatively by semantic analysis.
- The macro table is per-run and never global.

### 10.4 Standard LSL safety

- No directive is recognized in `Lsl` mode.
- `switch`, `case`, `default` as a statement, lazy-list operators, and related
  syntax stay syntax errors in `Lsl` mode, with legacy codes and messages.
- Corpus files and `tests/grammar_inventory.rs` encode the current boundaries;
  Firestorm-only tests must live in separate fixtures that are never run in
  `Lsl` mode.

---

## 11. Determinism and offline requirements (K)

- No network access at any stage, including include resolution and built-in
  metadata. Paths that look remote (`http://`, `https://`, `//host/share`) are
  rejected as source errors.
- No reliance on wall-clock time, random numbers, environment variables, process
  ID, or filesystem enumeration order.
- Same inputs (source bytes, CLI options, config, include file contents) must
  produce byte-identical output, including diagnostic order.
- Ordered containers (`Vec`, `BTreeMap`) are used wherever iteration affects
  output. `HashMap` is allowed only for lookup-only tables (like the existing
  built-in index).
- `SourceId` assignment follows deterministic load order.
- Include de-duplication uses canonical paths only for identity; display uses the
  spelling from the source.
- Built-in metadata remains vendored and versioned in the repository.
- Sorting of diagnostics uses a total, stable key: `(load order of source,
  start offset, end offset, code, message)`.
- New tests must avoid time, randomness, and network; temporary directories
  follow the existing `temporary_directory()` pattern in the integration tests.

---

## 12. Resource limits (L)

Limits are mandatory because preprocessor input is untrusted. Values are
provisional; Phase 1/2 must fix them as constants and add tests.

| Limit | Provisional value | Enforcement point |
| --- | --- | --- |
| Root file size | existing practical limits; explicit cap in Phase 1 | Source Manager |
| Included file size | 4 MiB | Source Manager |
| Source files per run | 256 | Source Manager |
| Include depth | 64 | Preprocessor include stack |
| Macro expansion depth | 64 | Preprocessor expander |
| Expanded bytes/tokens per run | 8 MiB / 1,000,000 tokens | Preprocessor |
| Origin chain depth | 64 | Source Map |
| Preprocessor diagnostics | 1,000 | Preprocessor |
| Total diagnostics | existing behavior; cap in Phase 3 | Unified layer |
| AST recursion depth | 256 nodes (with explicit guard) | AST parser |
| Control-flow analysis | existing legacy caps (tokens/nodes/edges/depth) | AST semantic analyzer |

Requirements:

- Exceeding a limit produces a single, specific `PRE`/`AST` diagnostic and stops
  the affected expansion; the rest of the run continues.
- Limits are checked before allocation where possible.
- Limit tests must be part of Phase 2/3/4 acceptance.
- No limit may depend on machine memory size.

---

## 13. Testing strategy (M)

### Layers

1. **Unit tests** co-located with modules (`#[cfg(test)] mod tests`), covering
   each public function and each invariant (spans, UTF-8 boundaries, segment
   ordering, chain construction, directive scanning edge cases).
2. **Contract tests** for the source map: every prepared range resolves to a
   file and a valid offset; identity mapping round-trips byte offsets; line and
   column match the lexer's semantics, including CRLF and multi-byte characters.
3. **Preprocessor tests** (Phases 2–3): one fixture per directive and per macro
   form, plus malformed and adversarial inputs (unterminated conditionals,
   recursive macros, paste bombs, include cycles, missing includes, path
   traversal).
4. **Integration tests** via the public API and the binary
   (`env!("CARGO_BIN_EXE_neolint")`), in the existing `tests/` style, using
   temporary directories.
5. **Golden corpus**: existing `corpus/{small,medium,large,invalid}` plus a new
   preprocessor corpus. Plain-LSL golden results must not change.
6. **Differential Legacy↔AST harness** (Phase 4+): compare normalized
   diagnostics as a multiset of `(code, severity, source, start, end)`, plus a
   per-code report. Order is compared only through the documented deterministic
   policy.
7. **Fallback/fuzz-style tests without dependencies**: deterministic generators
   over token/directive alphabets, bounded by iteration counts, run in normal
   `cargo test`.

### Requirements

- Every new diagnostic code has at least one positive and one boundary test.
- Every phase adds tests for its resource limits and malformed-input behavior.
- No test uses the network, sleeps, or depends on host locale/time zone.
- Tests must pass on MSRV 1.75; Phase 1+ should add an MSRV check step to the
  experimental workflow (separate from the required quality steps).
- CI: `experimental-ci.yml` runs the same quality gate as `ci.yml` on the
  experimental branch and on PRs targeting it; `ci.yml` runs only for `main`
  pushes and PRs targeting `main`.

---

## 14. Legacy → AST migration strategy (N)

Migration is incremental and evidence-driven.

1. **Rule ownership matrix.** For every `SEMxxx` (and `PARxxx` where the AST
   parser owns recovery), record: legacy owner, AST owner, parity status,
   known differences, and the test that proves parity. The matrix lives next to
   the AST semantic code and is updated in the same commit as behavior changes.
2. **Shadow comparison.** In `compare` mode, run both engines on the same
   prepared tokens, resolve both results, and compare normalized multisets.
   Differences are reported as a development artifact, never as user findings.
3. **Parity thresholds.** A rule may move to the AST engine only when:
   - its corpus and unit-test results match legacy exactly, or
   - every difference is documented as an intentional fix and covered by a test.
4. **Ordering policy.** Unified output is sorted deterministically, so rule
   migration does not reshuffle user output. Legacy-only mode keeps the current
   pass order to avoid breaking existing golden tests.
5. **No duplicate findings.** When both engines run for a user-visible mode, the
   merge layer must deduplicate by `(code, primary span)`. Duplicate detection is
   part of the Phase 6 acceptance criteria.
6. **Feature gating.** AST engine selection is a CLI/config option in the
   experimental branch. Default stays legacy until Phase 6 stabilization.
7. **Rollback.** The legacy engine remains compiled and selectable through the
   end of Phase 6; removal requires a separate decision and a major-version
   boundary.
8. **Documentation.** Every behavioral difference introduced by AST rules is
   added to `docs/diagnostics.md` with an example and a boundary note.

---

## 15. Phase plan (O)

### Phase 0 — Experimental branch, CI, architecture contracts (this phase)

- **Goal:** create a safe, isolated development contour and freeze contracts.
- **Deliverables:** `exp/preprocessor-ast` branch; `experimental-ci.yml`;
  `main`-scoped filters in `ci.yml`; this document.
- **Dependencies:** baseline commit only.
- **Acceptance criteria:**
  - branch starts exactly at the baseline;
  - `main` is untouched;
  - experimental CI triggers only for the experimental branch and its PRs;
  - stable CI triggers only for `main` pushes and PRs;
  - full existing test suite, fmt, clippy, and rustdoc pass;
  - no runtime/source behavior changes outside CI and docs.
- **Tests:** the existing 350 tests; workflow validation on push and on a PR.
- **Done when:** the branch is pushed, both workflows show the expected
  triggers, and the report records SHAs and run URLs.

### Phase 1 — Source Manager, Source Map, Identity Preprocessor

- **Goal:** make multi-file provenance and line resolution real without changing
  analysis.
- **Functionality:**
  - `SourceId`, `SourceFile`, `SourceSpan`, `SourceLocation`, `SourceOrigin`,
    `SourceMap`;
  - per-file line index with lexer-compatible columns;
  - Source Manager with root/stdin loading and deterministic IDs;
  - identity preprocessor producing a byte-identical prepared buffer and a
    single identity segment;
  - `PreparedSource` and a resolved-diagnostic API (no CLI change yet).
- **Modules:** `src/source/{mod.rs,file.rs,line_index.rs,map.rs,manager.rs}`,
  `src/preprocessor/{mod.rs,identity.rs,config.rs}`.
- **Dependencies:** Phase 0.
- **Acceptance criteria:**
  - identity prepared text equals input bytes for all fixtures;
  - map covers the buffer, segments sorted and non-overlapping;
  - every offset resolves to the same line/column as the current lexer for
    ASCII, UTF-8, CRLF, and empty files;
  - no public legacy API change;
  - no new dependencies; MSRV 1.75.
- **Tests:** unit tests for map construction/lookup, identity round-trip,
  UTF-8/CRLF columns, empty input, and malformed offsets (must not panic).
- **Done when:** the full suite passes and the identity path is covered by
  contract tests.

### Phase 2 — Preprocessor core

- **Goal:** C-like conditional compilation and object/function macros producing
  mapped prepared text.
- **Functionality:** `#define`, `#undef`, `#if`, `#ifdef`, `#ifndef`, `#elif`,
  `#else`, `#endif`; object-like and function-like macros; macro invocation
  with arguments; integer expression evaluator with defined precedence and
  overflow behavior; `PRE` diagnostics; expansion limits; source map segments
  for invocations, definitions, and arguments.
- **Modules:** `src/preprocessor/{directive.rs,macros.rs,expr.rs,expand.rs,
  diagnostics.rs}`.
- **Dependencies:** Phase 1 (source map, prepared source).
- **Acceptance criteria:**
  - directive semantics match the documented subset (including `defined`,
    nesting, and `#elif` after a taken branch);
  - malformed directives recover and continue;
  - recursion and size bombs stop at limits with one diagnostic;
  - every generated token resolves to a primary origin;
  - plain LSL without directives is byte-identical (identity behavior preserved).
- **Tests:** per-directive fixtures, expression table tests, macro expansion
  fixtures with expected origin chains, malformed/adversarial inputs, limits.
- **Done when:** directive and macro suites pass, map invariants hold, and the
  legacy suite is unchanged.

### Phase 3 — Includes, advanced macros, Firestorm built-ins, CLI integration

- **Goal:** complete the preprocessor and connect it to the legacy pipeline
  behind an explicit opt-in.
- **Functionality:** nested includes, include guards, `#pragma once`, `#` and
  `##`, variadic macros, predefined macros/built-ins (documented only), include
  graph and cycle detection, CLI/config options, diagnostic rendering through
  the source map, exit-code policy, JSON note extension if approved.
- **Modules:** `src/preprocessor/{include.rs,builtins.rs}`, `src/pipeline.rs`
  (or equivalent orchestration), `src/main.rs`, `src/config.rs`,
  `src/diagnostic.rs` (only if an additive note type is approved).
- **Dependencies:** Phase 2.
- **Acceptance criteria:**
  - include cycles, missing files, remote paths, and depth limits produce
    diagnostics without panics;
  - the same include included twice behaves per documented guard rules;
  - `#`/`##` origins never claim a literal user file range;
  - plain LSL mode output is byte-identical to baseline;
  - enabling the preprocessor without directives yields the same diagnostics as
    legacy;
  - CLI/config errors still exit 2; preprocessing findings exit 1 through the
    normal severity path.
- **Tests:** include-tree fixtures in temporary directories, cycle/limit tests,
  macro operator tests, CLI integration tests, source-map rendering tests,
  golden plain-LSL comparison.
- **Done when:** the preprocessor is usable end-to-end behind a flag, documented,
  and covered by integration tests.

### Phase 4 — AST parser

- **Goal:** build a typed AST over the same prepared token stream without
  changing diagnostic behavior.
- **Functionality:** AST types for declarations, functions, states, events,
  statements, expressions, types, and literals; spans preserved on every node;
  recovery equivalent to legacy; explicit recursion depth guard; parser API.
- **Modules:** `src/ast/{mod.rs,node.rs,types.rs,parser.rs}`.
- **Dependencies:** Phases 1–3 (prepared stream and source map), legacy parser as
  the behavioral reference.
- **Acceptance criteria:**
  - accept/reject decisions match legacy on the valid and invalid corpus;
  - diagnostic codes and spans match legacy or differences are documented;
  - malformed and deeply nested inputs never panic;
  - AST nodes retain prepared spans, and every node can be resolved through the
    source map.
- **Tests:** grammar inventory parity, corpus valid/invalid parity, recovery
  tests, depth/limit tests, AST shape tests for representative programs.
- **Done when:** the AST parser parses the whole corpus with parity and the
  differential harness reports only documented deltas.

### Phase 5 — Firestorm extensions

- **Goal:** add Firestorm-only syntax behind explicit flags.
- **Functionality:** `switch`/`case`/`default`/`break`, lazy lists,
  `USE_SWITCHES`, `USE_LAZY_LISTS`, `//fspreprocessor off` interactions, dialect
  profile plumbing, AST nodes for the new constructs.
- **Modules:** `src/ast/ext/` (or `src/ast/firestorm.rs`), preprocessor dialect
  profile updates.
- **Dependencies:** Phase 4 (AST) and Phase 3 (dialect/profile plumbing).
- **Acceptance criteria:**
  - all new syntax is rejected in `Lsl` mode with legacy-compatible errors;
  - accepted in Firestorm mode only when the relevant flag is on;
  - no fabricated built-in values;
  - source map works for extension constructs;
  - documentation lists supported and unsupported Firestorm behavior.
- **Tests:** positive/negative dialect matrix, switch fallthrough/break,
  lazy-list operators, preprocessing-off regions, interaction tests.
- **Done when:** the dialect matrix is fully covered and the standard mode
  remains unchanged.

### Phase 6 — AST semantic analyzer and migration

- **Goal:** migrate SEM rules to the AST engine, add conservative AST-only rules,
  and stabilize.
- **Functionality:** scope/type/flow analysis over the AST; rule ownership
  matrix; differential Legacy↔AST harness; unified output with deduplication;
  new conservative rules with tests and docs; performance validation.
- **Modules:** `src/ast_semantic/{mod.rs,scopes.rs,types.rs,flow.rs,rules/…}`,
  unified diagnostics layer.
- **Dependencies:** Phases 4–5.
- **Acceptance criteria:**
  - every migrated rule has parity evidence or a documented, tested delta;
  - no duplicate findings in user-visible output;
  - deterministic ordering and stable output across runs;
  - performance within documented bounds on the corpus;
  - legacy engine still selectable until stabilization is signed off.
- **Tests:** per-rule parity tests, differential corpus runs, dedup/ordering
  tests, new-rule positive/negative tests, regression corpus.
- **Done when:** the AST engine is the default in the experimental branch, the
  parity matrix is complete, and the migration report is accepted.

---

## 16. Module structure (proposed, not created in Phase 0)

```
src/
  main.rs                 CLI; option parsing; output; exit codes
  lib.rs                  public API surface (legacy re-exports stay stable)
  diagnostic.rs           Diagnostic/Span/Severity/DiagnosticKind (shared)
  lexer.rs                tokenizer only; no directive knowledge
  parser.rs               legacy token parser (frozen behavior)
  semantic.rs             legacy semantic analyzer (frozen behavior)
  builtins.rs             vendored LSL metadata (unchanged)
  config.rs               lint configuration (extended additively later)

  source/
    mod.rs                re-exports
    file.rs               SourceId, SourceFile, kinds, text ownership
    line_index.rs         byte offset -> line/column (lexer-compatible)
    map.rs                SourceSpan, SourceOrigin, SourceMap, segments
    manager.rs            loading roots/includes, path policy, cycles, limits

  preprocessor/
    mod.rs                prepare(), PreprocessConfig, PreparedSource
    identity.rs           Phase 1 identity pass-through
    directive.rs          Phase 2 raw-text directive scanner
    macros.rs             macro table, definitions, expansion
    expr.rs               #if expression evaluator
    include.rs            Phase 3 include expansion
    builtins.rs           Phase 3 predefined macros / profile built-ins
    diagnostics.rs        PRE code allocation and constructors

  ast/
    mod.rs                AST root and re-exports
    node.rs               node types with spans
    parser.rs             AST parser and recovery
    ext.rs (Phase 5)      Firestorm-only nodes

  ast_semantic/
    mod.rs                Phase 6 analyzer entry point
    scopes.rs
    types.rs
    flow.rs
    rules/                per-rule modules, ownership matrix

  pipeline.rs             (optional) stage orchestration shared by CLI/tests
```

Rules for the structure:

- **Lexer** keeps tokenization, literals, operators, comments, `Span`
  construction, and `Eof`. It must never learn directives, macro tables, or
  include logic. `#` handling stays as today unless the preprocessor has already
  removed the text.
- **Preprocessor** owns everything between raw bytes and prepared bytes:
  directive detection, conditional evaluation, macro expansion, include
  resolution, stringification, pasting, profile built-ins, and the map.
- **Source Map** lives in `src/source/map.rs` and is owned by the run, not by
  the preprocessor or the lexer. The preprocessor is its only writer during
  construction; consumers get an immutable `Arc`.
- **AST types** live in `src/ast/`; AST semantic rules live in
  `src/ast_semantic/`. The AST must not import preprocessor internals; it only
  sees tokens and spans.
- **Both semantic engines** are reachable through one pipeline entry point that
  selects `Legacy`, `Ast`, or `Compare`; they share `Vec<Token>` and the
  `SourceMap`. No engine calls the other.
- **Diagnostics** stay in `src/diagnostic.rs`; the unified resolution layer is a
  thin module (for example `src/diagnostic/resolve.rs`) added when CLI
  integration starts, keeping the core type stable.

Modules are created only when their phase starts. Phase 0 does not create empty
placeholder modules.

---

## 17. Risks and open questions

Risks tracked from the baseline analysis:

1. `Span` line/column are computed from the prepared text; all rendering must
   resolve origin locations. Tests assert exact positions, so identity mode must
   be byte-identical.
2. Macro-expanded offsets are non-monotonic in the original files. Only prepared
   order is guaranteed; anything that assumes source order must be audited.
3. Legacy diagnostic order equals pass order. Unified output must keep legacy
   order for plain mode and define a stable order for multi-file/dual-engine
   mode.
4. Parser recovery is index-based and expects `Eof`; the preprocessor must never
   produce a stream that violates these invariants.
5. Error suppression across parser/semantic is coupled; the AST engine needs an
   equivalent policy or documented deltas.
6. Adding a public field to `Diagnostic` or `Token` is a breaking change; prefer
   additive resolution types and keep the core types frozen.
7. CI runs stable Rust; MSRV 1.75 is not enforced today. New code must avoid
   post-1.75 std APIs, and an MSRV check should be added when the experimental
   workflow is extended.
8. The documentation test only scans three source files; new diagnostic families
   must extend it, or codes can ship undocumented.
9. Preprocessor scope can grow without bound; the phase plan and limits are the
   guardrails.
10. Firestorm behavior must be documented, not guessed; unknown built-ins stay
    unresolved.

Open questions to resolve during implementation (with an update to this
document):

- Should `PRE` diagnostics get a dedicated `DiagnosticKind`, and how does that
  affect the documented JSON `kind` values?
- Are related locations exposed in JSON in Phase 3, and under which schema
  versioning rule?
- Exact CLI spelling for dialect/preprocessor/extension flags.
- Whether `#pragma once` identity is per canonical path or per loaded
  `SourceId` (they coincide today; the contract must state it).
- Whether the prepared buffer is one concatenated string per run (current plan)
  or one per file (rejected for now: a single buffer keeps token spans
  unambiguous).
- MSRV verification belongs in the experimental workflow as an additional step;
  it is not part of the required Phase 0 quality gate.

---

## 18. Glossary

- **Prepared source / prepared buffer:** the single UTF-8 text produced by the
  preprocessor and consumed by the lexer.
- **Generated range:** a byte range in the prepared buffer.
- **Source span:** a byte range in an original root/include file.
- **Origin:** one link describing where prepared bytes came from.
- **Origin chain:** the ordered list of origins from generated bytes back to the
  root file.
- **Direct code:** bytes copied unchanged from a file.
- **Identity mode:** preprocessing that copies input bytes unchanged; the
  Phase 1 default and the legacy compatibility anchor.
- **Dialect profile:** the explicit set of language/preprocessor/extension
  behaviors for a run (`Lsl`, `Firestorm` plus flags).
- **Legacy engine:** the current token-based parser and semantic analyzer.
- **AST engine:** the future AST parser and AST semantic analyzer.
- **Unified diagnostics:** the layer that resolves, merges, filters, orders, and
  renders findings from either engine.

---

## 19. References in the current codebase

- Pipeline and CLI exit codes: `src/main.rs`.
- Token model and lexing rules: `src/lexer.rs` (`Token`, `TokenKind`, `lex`).
- Legacy parser and recovery: `src/parser.rs` (`Parser`, `parse`).
- Legacy semantic passes and suppression: `src/semantic.rs`
  (`analyze_with_syntax_diagnostics`).
- Diagnostic model: `src/diagnostic.rs` (`Span`, `Diagnostic`).
- Configuration format: `src/config.rs`.
- Vendored metadata: `src/builtins.rs`, `data/`.
- Stable quality gate: `.github/workflows/ci.yml`.
- Experimental quality gate: `.github/workflows/experimental-ci.yml`.
- Compatibility evidence: `tests/`, `corpus/manifest.txt`, `tests/grammar_inventory.rs`.
