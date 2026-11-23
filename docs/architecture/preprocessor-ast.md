# Preprocessor and AST Architecture

Status: **Phases 1–5 and 6A–6F implemented on the experimental branch**;
this document remains the normative contract for the remaining Phase 6 slices.
Baseline architecture: `98556e21e175190c95f2da39fc0e2ba7a8d9544c`.
Scope: source ownership, identity/full preprocessing, includes, advanced macros,
the Firestorm preprocessing profile, unified diagnostics, CLI integration, and the
independent typed AST parser and Firestorm switch/lazy-list AST parsing are
implemented. All existing SEM001–SEM023 diagnostics now have independent AST
implementations with differential parity; engine selection/integration remains
future work.
Audience: maintainers and contributors working on Phase 6.

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
3. **In prepared mode, both engines receive the same `PreparedSource`/token
   stream.** No engine preprocesses on its own. Plain-LSL Legacy mode is the
   explicit no-map/no-PreparedSource compatibility path.
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
15. **No implicit coordinate conversions.** `Span -> PreparedSpan` is an
    explicit checked call; `PreparedSpan -> SourceSpan` is only through the
    source map; there is no `Span -> SourceSpan` conversion.
16. **Different diagnostics carry different coordinate kinds.**
    Preprocessor diagnostics carry original `SourceSpan`s and bypass the map;
    engine diagnostics carry prepared `Span`s and are resolved through it.
17. **Every coordinate carries owning-unit identity.** `SourceId` embeds
    `AnalysisUnitId`; `PreparedSpan` carries it. A map rejects foreign
    coordinates with `ForeignAnalysisUnit`/`ForeignPreparedSource` before any
    file or segment lookup.
18. **There is exactly one public `Span` adapter per PreparedSource.** It
    assigns identity and validates the range; it cannot recover provenance that
    `Span` does not contain.
19. **Malformed coordinate handling is panic-free in every build profile.**
    The path returns `Result::Err`; no `debug_assert!`/`assert!` is used on it.
20. **The Legacy path is unit-tagged and root-only.** `LegacyDiagnostic` has no
    public constructor; `from_legacy` takes an `AnalysisUnit` handle, checks the
    tag, and uses `legacy_root()` rather than a caller-supplied `SourceId`.
21. **Every origin span is owned and validated.** `SourceOrigin` has no public
    constructor; the map builder rejects foreign spans before insertion, and
    traversal revalidates. Foreign origin data is an explicit error, never a
    local note.
22. **Run identity is part of unit identity.** `AnalysisUnitId` includes a
    process-local `RunId`, so coordinates cannot be exchanged between
    independently active invocations even when unit index and offsets match.

---

## 1. Current pipeline (A)

```
CLI arguments (--engine, --preprocess, --dialect, -I, ...)
  → LintConfig (config file, --disable, --severity, engine selection)
  → input collection (files, directories, stdin)
  → choose the unit preparation path:
      no --preprocess, engine = legacy:
          the existing direct plain loop (uncapped run-level, unchanged)
      no --preprocess, engine = ast | compare:
          identity preparation (byte-identical prepared source + map)
      --preprocess (any engine):
          full preprocessing with dialect and include directories
  → for each collected input (each input is an independent analysis unit):
      construct AnalysisUnit
      → one lex of the plain or prepared text
      → selected engine:
          legacy: parse_legacy → analyze_with_syntax_diagnostics
          ast:    parse_ast    → ast_semantic::analyze_ast
          compare: both, display the Legacy stream, build the report
      → config.apply(&mut diagnostics) per source group
  → human / JSON rendering (results from all inputs)
  → Compare report on stderr only (compare mode)
  → exit code 0 | 1 | 2 | 3
```

The CLI keeps three preparation/execution paths. The plain-Legacy path is the
pre-Phase-6 CLI loop verbatim and is not capped at run level. `ast` and `compare`
without preprocessing use identity preparation so the engine API always has a
prepared source and source map; identity preparation copies bytes and does not
expand macros or includes. Any engine with `--preprocess` uses the full
preprocessor and dialect. `compare` never merges the AST stream into stdout:
stdout keeps the Legacy display schema, and the deterministic report goes to
stderr.

A single `AnalysisUnit` engine call lexes the prepared text exactly once; the
same token slice feeds both analyzers and (when requested) `--tokens`. The old
second `lex(prepared.text())` call in the CLI token path is gone.

The current CLI already analyzes every collected input independently: one file
read, one token stream, one diagnostic set per input. The new architecture names
that unit of work **AnalysisUnit** (§6) and keeps it independent, even though
one CLI invocation may contain many such units.

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
- `LintConfig` in `src/config.rs` supports `disable`, `severity.RULE`,
  `preprocess`, `dialect`, `include_dir`, and `engine = legacy|ast|compare`;
  rule codes must match `[A-Z0-9]+` and the default engine is `legacy`.
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
  CLI/config  |   Source Manager  |   one AnalysisUnit per root input,
              |                   |   RunId + deterministic unit index
              +---------+---------+
                        | LoadedSource: unit-scoped SourceId + text + kind
                        | (plain-LSL Legacy mode bypasses the next two stages)
                        v
              +-------------------+
              |   Preprocessor    |  (identity in Phase 1; core in Phases 2–3)
              +---------+---------+
                        | PreparedSource { text, SourceMap, PreprocessDiagnostics }
                        v
              +-------------------+
              |      Lexer        |  (unchanged tokenization rules)
              +---------+---------+
                        | Token stream (prepared Span + Eof)
           +------------+-------------+
           v                          v
  +------------------+       +------------------+
  |  Legacy Engine   |       |    AST Engine    |
  | parse_legacy     |       | parse_ast        |
  | + legacy SEM     |       | + AST SEM        |
  +--------+---------+       +--------+---------+
           |                          |
           +-------------+------------+
                         v
              +-------------------+
              | Unified Diagnostics|  resolve PreparedSpans through SourceMap,
              | + config filtering |  merge, deduplicate policy, render
              +---------+---------+
                         v
                 per-unit rendered results, then CLI-wide
                 human / JSON output and exit code
```

The loop is: **prepare → lex → parse (legacy or AST) → resolve → merge → render**
inside each AnalysisUnit. The AST engine and the legacy engine are alternatives
at the top of the funnel but share everything before the funnel. No engine may
preprocess on its own. Units never share mutable preprocessor state or
diagnostic IDs; they may share immutable file contents (§6.3).

Diagnostics enter the unified layer through three paths: preprocessor findings
arrive with a direct original `SourceSpan` (the directive may no longer exist in
the prepared buffer); lexer/parser/semantic findings arrive with the frozen
prepared `Span` and are explicitly bridged and resolved through the unit's
`SourceMap` (§7.1.1); plain-LSL findings arrive as a unit-tagged
`LegacyDiagnostic` and never touch a map.

---

## 3. Component responsibilities (C)

### AnalysisUnit (`src/source/`, orchestrated by `src/pipeline.rs`)

- One AnalysisUnit is the complete independent analysis of **one root source**:
  one root file or one stdin stream.
- The CLI assigns each unit an `AnalysisUnitId { RunId, index }`; the index is
  deterministic in collected-input order (`0..N-1`) and `RunId` is unique to the
  invocation. Neither is persisted or appears in output.
- The unit is an opaque handle constructed by the pipeline. It exposes its id
  and, only for a plain-LSL single-root unit, `legacy_root()`.
- It owns:
  - its own `SourceId` namespace, where `SourceId` includes the unit id and a
    local file index (root gets local file index 0);
  - its own `SourceManager` and root source;
  - its own macro table, conditional stack, include graph, and include stack;
  - its own diagnostics;
  - an optional prepared pipeline: `PreparedSource` and its `SourceMap` are
    created together when preprocessing/prepared-coordinate analysis is active.
    Plain-LSL Legacy mode has neither and renders against `legacy_root()`.
- It is created by the CLI/pipeline for each collected input, lives until its
  rendered results are produced, and is then dropped. No state survives.
- It never reads mutable state from another unit. Only immutable file
  contents may be shared through the optional cache (§6.3).
- One CLI invocation may create many units; unit results are concatenated in
  input order for output (§6.5).

### Source Manager (`src/source/`)

- Assigns stable `SourceId` values inside one AnalysisUnit.
- Loads the root source (file or stdin) and include files on request.
- Owns file text, display names, and line indices.
- Enforces path policy (local files only, no network, no directory includes).
- Detects include cycles through the unit's active include stack.
- Enforces file-size and file-count limits per unit.
- Never tokenizes and never interprets LSL or preprocessor syntax.

### Preprocessor (`src/preprocessor/`)

- Scans raw text for directives, comments, and string/character boundaries.
- Maintains the macro table, conditional stack, and include expansion **for one
  AnalysisUnit**.
- When selected for a unit, produces exactly one prepared text buffer and
  its paired `SourceMap`, mapping `PreparedSpan` ranges to original code. The
  plain-LSL Legacy no-map path does not invoke the preprocessor.
- Emits `PRE` diagnostics for directive and include problems.
- Does not know about tokens, AST nodes, parser recovery, or semantic rules.
- Does not perform semantic type checking or built-in argument checks.

### PreparedSource (`src/preprocessor/`)

- The single text buffer handed to the lexer for one unit.
- Carries `Arc<SourceMap>` and `Vec<PreprocessDiagnostic>`.
- Is immutable after construction.
- In identity mode, `text` is byte-identical to the input file.
- Is **not** a `SourceFile` and has no `SourceId`. Its bytes are addressed only
  by `PreparedSpan` (§7.1).
- Carries its `AnalysisUnitId`. At most one `PreparedSource` exists per unit:
  it is present with its `SourceMap` in prepared/preprocessor mode and absent in
  the plain-LSL Legacy no-map path. The unit id is the prepared-source identity.
- Provides the **single** public, checked bridge
  `prepared_span(Span) -> Result<PreparedSpan, SpanError>` (stamping the unit)
  plus `eof_span() -> PreparedSpan`, and exposes `unit`, `prepared_len`, `text`,
  and `map`. There is no public `PreparedSpan` constructor and no implicit
  conversion to or from `Span`/`SourceSpan`.
- The adapter cannot recover provenance from `Span`: `Span` has no source
  identity. It validates the range and assigns identity from this source's
  context; correctness relies on engines being invoked only with the
  PreparedSource they lexed.
- Its preprocess diagnostics are already in original coordinates and are **not**
  resolved through the source map.

### Lexer (`src/lexer.rs`)

- Unchanged tokenization: keywords, identifiers, literals, operators, `Eof`.
- Produces `Span` values **relative to the prepared buffer**.
- Does not see directives (they are removed or rewritten by the preprocessor)
  and does not need to know that a map exists.
- Continues to discard comments and whitespace, and to reject `#` outside
  directives exactly as today when no preprocessor is active.

### Legacy Engine (`src/parser.rs`, `src/semantic.rs`)

- Exposes the independently callable `parse_legacy(tokens)` and keeps the legacy
  semantic analyzer.
- Consumes the same `Vec<Token>` as the AST engine.
- Keeps its current diagnostic codes, severities, spans, and pass order in
  legacy mode.
- Is not modified to understand macros or includes. It sees only the prepared,
  expanded token stream.
- In the plain-LSL path, its frozen `Diagnostic` values are tagged as
  `LegacyDiagnostic` by the owning unit pipeline; callers cannot attach them to
  another unit's source.
- **Does not evolve into the AST parser.** It stays compiled and runnable
  through the end of the migration (§14).

### AST Engine (`src/ast/`, `src/ast_semantic/`)

- Exposes the independently callable `parse_ast(tokens)` returning an AST plus
  diagnostics.
- Builds typed nodes with `Span`/`PreparedSpan` provenance preserved.
- Parses the same prepared token stream with equivalent recovery behavior.
- Performs semantic analysis on the AST, gradually taking over SEM rules.
- Never runs include resolution or macro expansion itself.
- May reuse grammar helper functions with the legacy parser only when both
  engines remain independently runnable and their results remain comparable
  (§4.8).

### Unified Diagnostics (new layer, introduced with CLI integration)

- Accepts `UnifiedDiagnosticInput`:
  - `Preprocess(PreprocessDiagnostic)` — already original `SourceSpan`, inserted
    directly via `SourceMap::location`/`render_span`;
  - `Prepared(PreparedDiagnostic)` — prepared `Span`, bridged explicitly and
    resolved through `SourceMap`;
  - `Legacy(LegacyDiagnostic)` — plain-LSL pass-through with the frozen
    `Diagnostic.span`, tagged with the producing unit and rendered against that
    unit's root only.
- Rejects malformed **and foreign** coordinates with `SpanError` handling
  (§7.3.1): unit ownership is checked before geometry or rendering, and the
  error is fail-fast within the unit. No partial unit result is accepted, other
  units are not merged into it, and there is no panic, no cross-unit file
  fallback, and no EOF substitution.
- Selects the displayed engine's stream: Legacy or AST. Compare keeps the Legacy
  displayed stream and emits a separate deterministic report. There is no
  cross-engine display merge.
- Applies `LintConfig` per source group before rendering.
- Produces `RenderedDiagnostic` values carrying the frozen `Diagnostic`, the
  original `SourceId`, and related locations.
- Renders human and JSON output; owns the exit-code decision.

### CLI / configuration (`src/main.rs`, `src/config.rs`)

- Creates one AnalysisUnit per collected input.
- Chooses the dialect profile and the analysis engine (`legacy` default, `ast`,
  or `compare`) from configuration plus `--engine` overrides.
- Chooses the preparation path: plain Legacy (no prepared source), identity
  preparation for `ast`/`compare` without preprocessing, or full preprocessing
  when requested.
- Keeps plain LSL as the default with preprocessing disabled.
- Maps CLI/config failures to exit code 2; lint findings to 0/1; an unmatched
  Compare report to 3 (after any 2); a matching report follows the displayed
  Legacy stream.
- Emits the Compare report on stderr only; stdout keeps the Legacy schema.
- Validates all new options without panicking.

---

## 4. Contracts between components (D)

All contracts below are normative. Exact Rust signatures are finalized in
Phase 1, but the semantics must not drift.

### 4.1 CLI → AnalysisUnit

Input: one collected root path, a stdin marker, or nothing for that unit.
Output: an AnalysisUnit or an `InputError` that maps to exit code 2.

Invariants:

- The CLI creates one AnalysisUnit per collected input, in the deterministic
  input order produced by directory collection.
- Units are constructed and completed one at a time or in parallel, but their
  results are always merged in input order.
- A unit's root is read as UTF-8; invalid UTF-8 is an input error (exit 2).
- A UTF-8 BOM is **not** silently stripped (today it is a lexical error; this
  behavior is preserved unless a later phase changes it explicitly).
- stdin (`-`) may appear at most once per CLI invocation. It is one unit, and
  the stream is read once for that unit.
- A failure in one unit does not corrupt or mutate another unit.

### 4.2 AnalysisUnit → Source Manager

Input: the unit's root. Output: the unit's `SourceId` namespace and loaded file
contents.

Invariants:

- `SourceId` embeds the owning `AnalysisUnitId` plus a unit-local file index.
  Two units may both have file index 0; those `SourceId` values are different
  and must never be compared or mixed. IDs are never persisted.
- The unit id is assigned deterministically by the CLI in input order
  (`0..N-1`). The root receives local file index 0.
- Any `SourceSpan` or `RelatedLocation` crossing a unit boundary is rejected by
  the target `SourceMap` with `ForeignAnalysisUnit`; matching local indexes are
  never accepted.
- The macro table, conditional stack, include graph, `SourceMap`, and
  diagnostics are single-unit state. They are never shared between units.
- Rationale for strict isolation: independent root files are independent LSL
  programs. A `#define`, an `#undef`, a conditional-stack entry, or an include
  guard from one root must not change the analysis of another root. Leaking
  macro state would make results depend on input order and would break the
  existing contract that every collected file is analyzed on its own.
- Immutable file contents (and optionally their line indices) **may** be cached
  across units by canonical path to avoid repeated disk reads. Such a cache is
  content-only: it never carries `SourceId`, macro tables, conditional stacks, or
  diagnostics. Cache hits and misses must not affect output.

### 4.3 Source Manager → Preprocessor

Input: a `LoadedSource` (a unit-scoped `SourceId`, the file text, and its display
name); include requests carry the requesting `SourceId` and a path/spelling.
Output: a resolved include or a `PRE` diagnostic.

Invariants:

- Include resolution is relative to the including file's directory (and to
  explicit `-I` roots when introduced); it is never relative to the process CWD
  by accident.
- Network paths and URLs are rejected as source-level diagnostics.
- Canonical paths are used for cycle detection and de-duplication, but the
  user-facing spelling is preserved for messages.
- Include depth and total file count are bounded per unit (§12).

### 4.4 Preprocessor → Lexer

Input: exactly one `PreparedSource` in the prepared pipeline. Output: one
prepared token stream. The separate Legacy no-map path lexes the root directly.

Invariants:

- The prepared text is valid UTF-8.
- The prepared text is produced deterministically from the same inputs.
- Removing directives must not merge adjacent tokens in a way that changes
  their meaning; the implementation inserts safe separation where needed.
- The prepared buffer may be empty. The lexer then emits only `Eof` at byte
  offset 0; the bridge yields the zero-length `PreparedSpan` at offset 0.
- For non-empty buffers, the lexer's `Eof` position equals the prepared text
  length (byte offset `len`); the bridge yields the zero-length
  `PreparedSpan` at offset `len`.

### 4.5 Preprocessor → Source Map

Every prepared byte has exactly one origin segment, addressed by `PreparedSpan`:

- Segments are sorted by prepared start, are non-overlapping, and cover the
  half-open range `[0, prepared_len)`. Bytes with no direct user counterpart
  (for example inserted separators) still have a segment, with origin
  `Generated` pointing at the nearest enclosing real user location; there are
  no unmapped holes.
- A segment maps to at least one `SourceOrigin`; the origin chain is acyclic.
- The map records an **EOF anchor** `SourceSpan` in a real original file. The
  anchor is the target for valid zero-length prepared spans at `prepared_len`
  and for an empty prepared buffer. It is never a synthetic file, never a
  prepared-buffer coordinate, and never a fallback for malformed ranges.
- Direct (unmodified) input has a single identity segment per file plus the EOF
  anchor.
- The map never stores prepared bytes as a `SourceFile`; `SourceSpan` is valid
  only for original files (§7).
- The map is bound to one `AnalysisUnitId`; all files, segments, origins, and
  the EOF anchor carry that unit. `resolve_span`/`resolve_point` reject a
  `PreparedSpan` from another unit with `ForeignPreparedSource`;
  `location`/`render_span`/`source_span` reject a foreign `SourceSpan` with
  `ForeignAnalysisUnit`.
- The map exposes `Result`-based lookups. Well-formed ranges of its own unit
  always resolve; malformed ranges and map holes return `SpanError` and are
  never repaired by pointing at the anchor (§7.3.1).
- The EOF anchor is valid by construction (`SourceMap::source_span` was used to
  build it), belongs to the map's unit, and `eof_anchor()` is infallible.

### 4.6 Lexer → Engines

Input: `&[Token]` ending in `Eof`. Output: diagnostics.

Invariants:

- Token order is the prepared text order.
- Every token span lies inside the prepared buffer and on UTF-8 boundaries.
- The `Eof` token has byte offset `prepared_len`; `PreparedSource::eof_span()`
  yields the unit-stamped zero-length `PreparedSpan` used as the anchor for
  parser diagnostics such as `PAR013`.
- Neither engine may assume that byte offsets are monotonic *in the original
  files*; monotonicity holds only in the prepared buffer.
- Engines receive tokens and produce the frozen `Span` values, which address the
  prepared buffer. They do not construct `PreparedSpan` themselves; the unified
  layer bridges each diagnostic with
  `PreparedSource::prepared_span(span)`, stamping the source's unit, then
  resolves it through that unit's map (§7.1.1).
- Zero-length spans (including EOF spans) are legal and resolvable; use
  `PreparedSource::eof_span()` for the end-of-buffer point.

### 4.7 Preprocessor and Engines → Unified Diagnostics

Input: `UnifiedDiagnosticInput` values:

- `Preprocess(PreprocessDiagnostic)` — original `SourceSpan`, no map lookup;
- `Prepared(PreparedDiagnostic)` — frozen `Span` in prepared coordinates;
- `Legacy(LegacyDiagnostic)` — frozen `Diagnostic.span` plus the producing
  unit tag (plain-LSL path).

Output: `RenderedDiagnostic` values grouped by original file and by unit.

Invariants:

- Every rendered diagnostic has a real file, line, column, and byte range in that
  file, including diagnostics produced at `Eof` and diagnostics in units whose
  prepared buffer is empty.
- A `PreprocessDiagnostic` never passes through `resolve_span`; its primary and
  related spans are already original and go directly through
  `SourceMap::location`/`render_span`. Every one of them must belong to the
  map's unit, or conversion fails with `ForeignAnalysisUnit`.
- A `PreparedDiagnostic` is bridged explicitly
  (`PreparedSource::prepared_span`) and then resolved. The `PreparedSource` and
  `SourceMap` must have the same unit; otherwise `ForeignPreparedSource` is
  returned before bridging. A bridge or resolve error is handled as an internal
  error (§7.3.1), never as EOF.
- When preprocessing is disabled, `from_legacy` verifies
  `legacy.unit() == unit.id()` (`ForeignAnalysisUnit` otherwise) and takes the
  source only from `unit.legacy_root()` (`NotLegacyUnit` if absent). The span is
  passed through unchanged (same `Span`, line, column, order), preserving
  baseline output, and no map or prepared-buffer operation is involved.
- Secondary locations (macro definition, include chain) never replace the
  primary location.
- Config filtering runs once, after merging, in deterministic unit order.

### 4.8 Public library API

The existing public items are frozen for the transition period:

```rust
pub fn lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>);
pub fn parse(tokens: &[Token]) -> Vec<Diagnostic>; // frozen alias of parse_legacy
pub fn analyze(tokens: &[Token]) -> Vec<Diagnostic>;
pub fn analyze_with_syntax_diagnostics(tokens: &[Token], syntax: &[Diagnostic]) -> Vec<Diagnostic>;
pub use diagnostic::{Diagnostic, DiagnosticKind, Severity, Span};
```

The two parsers are explicitly independent and independently runnable:

```rust
// Legacy parser: token-scanning validation; keeps today's behavior and codes.
pub fn parse_legacy(tokens: &[Token]) -> Vec<Diagnostic>;

// AST parser: builds a typed tree and returns its own diagnostics.
pub fn parse_ast(tokens: &[Token]) -> (Ast, Vec<Diagnostic>);

// Phase 5: independent Firestorm switch/lazy-list gates.
pub fn parse_ast_with_options(
    tokens: &[Token],
    options: AstParseOptions,
) -> (Ast, Vec<Diagnostic>);
```

`parse_ast` is exactly `parse_ast_with_options(tokens, AstParseOptions::lsl())`.
The effective gates are read from `PreparedSource::switches_enabled()` and
`PreparedSource::lazy_lists_enabled()` and passed as plain booleans; the AST
module does not import preprocessor internals.

Contract:

- `parse(tokens)` keeps its exact current signature and behavior and is
  implemented in terms of `parse_legacy` during the transition.
- `parse_legacy` and `parse_ast` are both public, both accept the same prepared
  token stream, and neither is implemented by calling the other for its complete
  result.
- Sharing low-level grammar helpers (type checks, precedence tables, delimiter
  matching) is allowed only while both engines stay independently runnable and
  their outputs stay directly comparable. A shared helper must not encode
  engine-specific recovery or diagnostic policy.
- `parse_ast` must be callable in a build where the legacy semantic analyzer is
  not invoked at all, and vice versa.

New preprocessing, bridge, and diagnostic entry points are **additive**. The
older conceptual `analyze_prepared(prepared, &EngineOptions) -> RunDiagnostics`
sketch below is superseded by the implemented Phase 6G engine API:

```rust
// Actual engine selection and analysis surface (Phase 6G).
pub enum AnalysisEngine {
    Legacy,
    Ast,
    Compare,
}

impl AnalysisEngine {
    pub fn parse(value: &str) -> Option<Self>;
    pub const fn as_str(self) -> &'static str;
    pub const fn requires_prepared_source(self) -> bool;
}

pub struct EngineAnalysisOptions {
    pub collect_tokens: bool,
}

pub enum EngineAnalysisError {
    Span(SpanError),
    PreparedSourceRequired { unit: AnalysisUnitId },
}

impl AnalysisUnit {
    pub fn analyze_with_engine(
        &self,
        engine: AnalysisEngine,
    ) -> Result<UnitEngineAnalysis, EngineAnalysisError>;
    pub fn analyze_with_engine_options(
        &self,
        engine: AnalysisEngine,
        options: &EngineAnalysisOptions,
    ) -> Result<UnitEngineAnalysis, EngineAnalysisError>;
}

impl AnalysisRun {
    pub fn analyze_with_engine(
        &self,
        engine: AnalysisEngine,
    ) -> Result<RunEngineAnalysis, EngineAnalysisError>;
    pub fn analyze_with_engine_options(
        &self,
        engine: AnalysisEngine,
        options: &EngineAnalysisOptions,
    ) -> Result<RunEngineAnalysis, EngineAnalysisError>;
}

// UnitEngineAnalysis exposes the displayed diagnostics, an optional comparison,
// and the single-lex token vector. RunEngineAnalysis exposes dense-order
// diagnostics, comparisons, tokens(unit_index), and all_matched().
pub struct UnitEngineAnalysis { /* diagnostics, comparison, tokens */ }
pub struct RunEngineAnalysis { /* diagnostics, comparisons, per-unit tokens */ }

pub struct EngineComparisonReport {
    pub matched: bool,
    pub comparison_complete: bool,
    pub details_truncated: bool,
    pub legacy_count: usize,
    pub ast_count: usize,
    pub by_rule: Vec<RuleComparison>,
}

// RuleComparison carries exact counts plus bounded missing/extra fingerprint
// samples. DiagnosticFingerprint carries code, severity, kind, message, start,
// end, line, column, end_line, and end_column.
pub struct RuleComparison { /* exact counts and bounded samples */ }
pub struct DiagnosticFingerprint { /* full fingerprint fields */ }

impl PreparedSource {
    pub fn prepared_span(&self, span: Span) -> Result<PreparedSpan, SpanError>;
    pub fn eof_span(&self) -> PreparedSpan;
    pub fn unit(&self) -> AnalysisUnitId;
}

impl SourceMap {
    pub fn unit(&self) -> AnalysisUnitId;
    pub fn resolve_span(&self, prepared: PreparedSpan) -> Result<ResolvedSpan, SpanError>;
    pub fn resolve_point(&self, point: PreparedSpan) -> Result<SourceSpan, SpanError>;
    pub fn location(&self, span: SourceSpan) -> Result<SourceLocation, SpanError>;
    pub fn render_span(&self, span: SourceSpan) -> Result<Span, SpanError>;
    pub fn source_span(&self, source: SourceId, start: usize, end: usize)
        -> Result<SourceSpan, SpanError>;
    pub fn eof_anchor(&self) -> SourceSpan;
    pub fn origin(&self, id: OriginId) -> Result<&SourceOrigin, SpanError>;
}

impl AnalysisUnit {
    pub fn id(&self) -> AnalysisUnitId;
    pub fn legacy_root(&self) -> Option<SourceId>;
}

impl RenderedDiagnostic {
    pub fn from_legacy(
        diagnostic: LegacyDiagnostic,
        unit: &AnalysisUnit,
    ) -> Result<Self, SpanError>;
}

pub enum UnifiedDiagnosticInput {
    Preprocess(PreprocessDiagnostic),
    Prepared(PreparedDiagnostic),
    Legacy(LegacyDiagnostic),
}
```

Contract:

- `lex`, `parse`, `analyze`, `Diagnostic`, `Token`, and `Span` keep their exact
  signatures and field meanings. No existing function parameter or return type
  changes to `PreparedSpan`.
- The only `Span -> PreparedSpan` path is
  `PreparedSource::prepared_span`; there is no public `PreparedSpan`
  constructor. The only `PreparedSpan -> SourceSpan` path is
  `SourceMap::resolve_span`/`resolve_point`, which first verify unit ownership.
- `Span -> SourceSpan` does not exist as a conversion.
- `SourceSpan`/`PreparedSpan`/`OriginId` from another unit or run are never
  accepted, even when their offsets and unit indexes coincide.
- `UnifiedDiagnosticInput` is a new additive type; `Diagnostic` is not changed.
  `LegacyDiagnostic` pairs the frozen diagnostic with its producing unit and has
  no public constructor, so the Legacy `SourceId` cannot be selected by callers.
- There is no public constructor for `PreparedSpan`, `SourceOrigin`,
  `OriginId`, `LegacyDiagnostic`, or `RenderedDiagnostic`. They are created only
  by `PreparedSource`, the map builder, the unit pipeline, or the conversion
  functions; no public path bypasses the ownership checks.

---

## 5. Lifecycle of one analysis (E)

The CLI lifecycle contains an outer loop over units and an inner lifecycle per
unit.

**CLI-wide:**

1. **Argument parsing.** Parse CLI options; reject unknown combinations with
   exit code 2. No file is touched yet.
2. **Configuration.** Load `LintConfig`; validate rule codes; resolve dialect
   profile and engine selection; reject unsupported combinations (for example
   Firestorm extensions without a Firestorm dialect).
3. **Input collection.** Expand directories deterministically (sorted, symlinks
   skipped), de-duplicate displayed paths, enforce one stdin. The invocation
   allocates a process-local `RunId` (synchronized checked monotonic counter). The result is
   an ordered list of root inputs; each receives an `AnalysisUnitId` whose index
   is `0..N-1` and whose run is this `RunId`. Neither value is user-facing.
4. **Unit loop.** For each root input, in order, run the per-unit lifecycle
   below. A failure in one unit is recorded and does not abort the remaining
   units (matching today's per-file behavior).
5. **Merging.** Collect unit results in input/unit order. The displayed Legacy
   stream is not globally sorted; Compare reports are collected in the same
   dense unit order.
6. **Configuration.** Apply disabled rules and severity overrides per source
   group before rendering; no global re-sort is performed.
7. **Rendering.** Human output or JSON; exit code 0/1, or 3 for an unmatched
   Compare report. Input/config failures return 2 as today.
8. **Teardown.** Drop all units; no per-analysis mutable state survives.
   The process-wide synchronized `RunId` allocator and built-in metadata cache
   remain for the lifetime of the process; neither contributes to user output.

**Per AnalysisUnit:**

1. **Unit creation.** Create the opaque `AnalysisUnit` handle with its assigned
   `AnalysisUnitId`; allocate the `SourceId` namespace (ids include the unit),
   macro table, conditional stack, and include graph. Only the pipeline
   constructs the handle. A `SourceMap`/`PreparedSource` pair is created only
   if this unit enters the prepared/preprocessor path; Legacy mode creates
   neither.
2. **Root load.** Read the root as UTF-8 and register it as local file index 0,
   producing a unit-owned `SourceId`. Unreadable/invalid root is an input error
   for this unit (exit 2 contribution) and the unit ends.
3. **Preparation.** Select the coordinate path:
   - plain-LSL Legacy mode (preprocessing disabled): keep the loaded root as-is;
     create no `PreparedSource` and no `SourceMap`;
   - identity/prepared mode: copy the root text byte-for-byte into one
     `PreparedSource`, build the identity map and EOF anchor;
   - full Phase 2 mode: scan directives, expand macros, and build the full map;
   - later Firestorm mode: add include resolution and the remaining extensions
     in textual order.
   Origin records are inserted only through the map builder, which validates
   `span`/`invocation`/`definition` against the unit and rejects foreign spans
   before they enter the arena (§7.5.1). Preprocessor problems become
   `PreprocessDiagnostic` values with original `SourceSpan`s. Failures inside
   preparation are diagnostics, not Rust errors, except for limits that make
   further analysis unsafe (still diagnostics; the unit continues with a bounded
   prepared stream).
4. **Lexing.** Tokenize the root text directly in Legacy mode, or the
   `PreparedSource` text in prepared mode. Legacy findings retain original
   coordinates; prepared-path lexer/engine findings use prepared coordinates.
   An empty input yields only `Eof` at byte offset 0 in either path.
5. **Engine selection (implemented in Phase 6G).**
   - `legacy` (default): `parse_legacy` → `analyze_with_syntax_diagnostics`; in
     the plain-LSL path the unit tags each returned `Diagnostic` as a
     `LegacyDiagnostic`.
   - `ast`: `parse_ast` → AST semantic analyzer; the displayed stream is the AST
     lexical/parser/semantic stream.
   - `compare`: runs both raw semantic streams, displays the Legacy stream only,
     and attaches an `EngineComparisonReport`. The AST semantic stream is never
     appended to the displayed Legacy stream.
   - The prepared text is lexed exactly once per unit; both analyzers and the
     `--tokens` output consume that single token slice.
6. **Input assembly.** Collect:
   - preprocessor findings as `UnifiedDiagnosticInput::Preprocess`;
   - lexer/parser/semantic findings as
     `UnifiedDiagnosticInput::Prepared`, carrying their frozen `Span`;
   - plain-LSL findings as `UnifiedDiagnosticInput::Legacy(LegacyDiagnostic)`
     only when no preprocessor/map is active; the tag is assigned by this unit.
7. **Resolution.**
   - `Preprocess`: primary/related `SourceSpan` → `SourceLocation` directly
     (no `resolve_span`).
   - `Prepared`: verify `prepared.unit() == map.unit()`; bridge with
     `PreparedSource::prepared_span(span)` → `resolve_span` → `SourceSpan` →
     `SourceLocation`, then `render_span` into the frozen `Diagnostic.span`.
   - `Legacy`: `RenderedDiagnostic::from_legacy(legacy, unit)` verifies the tag
     and obtains the source only from `unit.legacy_root()`; `Diagnostic.span` is
     copied unchanged and no map is consulted.
   A zero-length prepared span at `Eof` is obtained from
   `PreparedSource::eof_span()` and resolved through `resolve_point`; it uses the
   same unit's map EOF anchor (§7.3). Legacy EOF diagnostics bypass this path
   and preserve their original `Diagnostic.span` unchanged.
   A `SpanError` is fail-fast within the unit (constraint E): the engine returns
   `EngineAnalysisError::Span`, the CLI reports `neolint: cannot analyze unit N`
   and exits 2. It never panics, never substitutes the anchor, and never accepts
   a partially analyzed unit.
8. **Unit merge.** The displayed Legacy stream is **not** globally sorted; it
   preserves the existing per-unit, per-source, pass order. Only the Compare
   report is deterministically ordered: dense unit index, `SEM001`..=`SEM023`
   rule order, then the fixed fingerprint order. `RunId` is excluded from
   ordering. There is no cross-engine deduplication merge step: Compare displays
   the Legacy stream only.

An empty source file is a valid unit in either path. The lexer emits only
`Eof` and the legacy parser reports `PAR013`. In plain-LSL Legacy mode the
original `Diagnostic.span` is passed through unchanged; in prepared mode the
zero-length `PreparedSpan` resolves through the map to the root at offset 0
(line 1, column 1). In later conditional-preprocessor mode, removing all code
from a non-empty root uses the map's real EOF anchor from the last directive or
root start (§7.3).

---

## 6. Analysis units and multiple source files (F)

### 6.1 Definition

An **AnalysisUnit** is the complete, independent analysis of one root source:

- one root file, or one stdin stream;
- one `AnalysisUnitId` = `{ RunId, index }`, where the index is deterministic
  within the invocation and `RunId` is unique to that invocation;
- its own `SourceId` namespace, where ids embed the unit id and a local file
  index (root gets index 0);
- its own `SourceManager` and root source;
- its own macro table, conditional stack, include graph, and include stack;
- either no prepared pipeline (plain-LSL Legacy no-map mode), or exactly one
  `PreparedSource`/`SourceMap` pair. Every `PreparedSpan` in that pair carries
  the unit id;
- its own diagnostics;
- a prepared buffer/token stream only when the prepared pipeline is active.

One CLI invocation may create any number of AnalysisUnits: one per collected
file, directory entry, or stdin input. A unit is **not** a CLI invocation-wide
namespace. A `SourceId`, `SourceSpan`, `PreparedSpan`, `RelatedLocation`, or
EOF anchor from one unit is rejected by another unit's map with
`ForeignAnalysisUnit` or `ForeignPreparedSource`; no same-index fallback exists.

### 6.2 Lifecycle and isolation

- The unit is created when its root starts loading and is dropped after its
  diagnostics have been resolved and merged into the CLI-wide result.
- The unit handle is opaque and pipeline-constructed. It exposes its id and, for
  a plain-LSL single-root Legacy unit only, `legacy_root()`; there is no public
  constructor and no way to attach a different unit's root. A prepared unit has
  its `PreparedSource`/`SourceMap` pair instead and does not expose `legacy_root()`.
- All preprocessing state is unit-local. `#define`, `#undef`, conditional-stack
  entries, include guards, and `#pragma once` state never cross unit boundaries.
- Reason: the existing CLI contract analyzes each collected file on its own.
  Independent roots are independent scripts; macro state leaking between them
  would make output depend on file order, change results when a directory gains a
  file, and break the baseline compatibility tests.
- `SourceId` embeds the unit id; two units may both have local file index 0,
  but those are different ids. Coordinates are only meaningful together with
  their unit, and cross-unit use is an explicit error.
- Unit indexes are only comparable within one `RunId`. Coordinates can escape
  as plain values, so a second independently active invocation in the same
  process gets a different `RunId`; its maps reject the first invocation's
  coordinates instead of matching index 0. This is enforced by the runtime
  checks above, not by Rust lifetimes.
- `RunId` allocation is process-wide and concurrency-safe. A single
  `std::sync::Mutex<u64>` serializes allocation across threads. The allocator
  reads the current value, computes the successor with `checked_add(1)`, stores
  the successor, and returns the previous value. A poisoned mutex returns
  `RunIdAllocationError::AllocatorPoisoned`; counter exhaustion returns
  `RunIdAllocationError::CounterExhausted`. Neither condition panics, wraps, or
  reuses an earlier ID. The failure is reported as an input/setup error (CLI
  exit code 2) before any AnalysisUnit is created. The mutex is not held while
  files are read or analysis runs.
- Units may be analyzed sequentially or in parallel. If parallel, merging is
  still performed in input order, so the observable result is deterministic.

### 6.3 Shared immutable file cache

- Immutable file contents may be cached across units, keyed by canonical path
  (optionally plus a content digest), to avoid repeated reads.
- A cache entry contains **only** text (and optionally its immutable line
  index). It never contains a `SourceId`, macro table, conditional stack, origin,
  include edge, or diagnostic.
- A cache hit must produce exactly the same output as a cold read.
- The cache is an optimization, not an architectural component; it must be
  possible to disable it without changing behavior. Phase 1 may omit it.

### 6.4 stdin

- `-` may be supplied at most once per CLI invocation and forms exactly one unit.
- The stdin bytes are read once, recorded under a display name such as `<stdin>`,
  and treated as the unit's root.
- stdin cannot be `#include`d by other units.
- Includes inside a stdin unit resolve relative to the process working directory
  only if a later phase explicitly defines that policy; the default is to reject
  relative includes from stdin unless an include root is supplied.

### 6.5 Merging results from several units

- Unit results are collected in the deterministic input order produced by
  directory collection (sorted paths, de-duplicated display names).
- Each rendered diagnostic keeps its unit's resolved file and location.
- Configuration filtering and severity overrides are applied per source group to
  the displayed stream. The Compare parity report is separate: it is computed
  from raw pre-configuration streams and is emitted on stderr in dense unit order.
- Exit code: `2` if any input/config failure occurred; otherwise `3` if a Compare
  report is unmatched; otherwise `1` if any remaining displayed error-severity
  diagnostic exists; otherwise `0`.
- There is no global merge, sort, or cross-engine deduplication step; the
  displayed stream keeps the existing per-unit, per-source, pass order.
- Diagnostics from one unit can never be attributed to another unit's file; the
  unit's `SourceMap` is the only resolver for its diagnostics.

### 6.6 Includes inside one unit

- `SourceId` order is load order: root first, then includes in the order they are
  first entered (depth-first, textual order). This order is the unit's diagnostic
  grouping order.
- The prepared buffer is **not** a `SourceFile` and has no `SourceId`; its bytes
  are addressed by `PreparedSpan` (§7).
- Include resolution:
  - textual position matters: an include is expanded at its directive site;
  - the same file included twice is expanded twice unless an include guard or
    `#pragma once` suppresses it; the Source Manager still loads it once and
    reuses the text (one `SourceId`, many expansions);
  - cycles are detected on the unit's active include stack using canonical paths
    and produce a `PRE` error at the `#include` site; the include is not entered;
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
use std::sync::Mutex;

/// Process-local identity of one analysis invocation.
pub struct RunId(u64);

/// Explicit failure to allocate a process-unique RunId.
pub enum RunIdAllocationError {
    CounterExhausted,
    AllocatorPoisoned,
}

/// Process-wide allocator. The mutex serializes concurrent callers; checked
/// increment prevents wraparound or identifier reuse. No unwrap/expect is used.
struct RunIdAllocator {
    next: Mutex<u64>,
}

impl RunIdAllocator {
    fn allocate(&self) -> Result<RunId, RunIdAllocationError> {
        let mut next = self
            .next
            .lock()
            .map_err(|_| RunIdAllocationError::AllocatorPoisoned)?;
        let allocated = *next;
        let successor = allocated
            .checked_add(1)
            .ok_or(RunIdAllocationError::CounterExhausted)?;
        *next = successor;
        Ok(RunId(allocated))
    }
}

static RUN_ID_ALLOCATOR: RunIdAllocator = RunIdAllocator {
    next: Mutex::new(0),
};

fn allocate_run_id() -> Result<RunId, RunIdAllocationError> {
    RUN_ID_ALLOCATOR.allocate()
}

/// Identity of one AnalysisUnit: run identity plus a deterministic unit index.
/// The index is assigned 0..N-1 in collected-input order. The whole id is
/// run-local, is not persisted, and never appears in human or JSON output.
pub struct AnalysisUnitId {
    run: RunId,
    index: u32,
}

impl AnalysisUnitId {
    pub fn run(&self) -> RunId;
    pub fn index(&self) -> u32;
}

/// Opaque handle to one AnalysisUnit. Constructed only by the pipeline; there
/// is no public constructor, so a caller cannot fabricate a unit-to-source
/// association.
pub struct AnalysisUnit {
    // private: id, source manager/root, mode, preprocessor state,
    // diagnostics, and prepared: Option<PreparedSource>
}

impl AnalysisUnit {
    pub fn id(&self) -> AnalysisUnitId;

    /// Returns the root `SourceId` only when this unit is a plain-LSL,
    /// single-root, no-map unit. Returns `None` for a preprocessed unit and
    /// for a unit without a successfully loaded root. There is no way to obtain
    /// an included file's id through this method.
    pub fn legacy_root(&self) -> Option<SourceId>;
}

/// Identity of one original file. The unit is part of the identity, so the same
/// local file index in two units is a different SourceId.
pub struct SourceId {
    unit: AnalysisUnitId,
    file: u32,
}

impl SourceId {
    pub fn unit(&self) -> AnalysisUnitId;
    pub fn file(&self) -> u32;
}

/// Original files only. The prepared buffer is not a file kind.
pub enum SourceFileKind { Root, Stdin, Included }

pub struct SourceFile {
    pub id: SourceId,          // carries AnalysisUnitId + local file index
    pub display_name: String,
    pub canonical_path: Option<PathBuf>,
    pub kind: SourceFileKind,
    pub text: Arc<str>,
    // line index is built lazily and cached
}

/// A range in the Prepared Buffer. Offsets are zero-based UTF-8 bytes.
/// Fields are private. When an AnalysisUnit has a prepared pipeline, it has
/// exactly one PreparedSource, so the unit id is the prepared-source identity;
/// a span from another unit is rejected by the owning SourceMap. Legacy units
/// do not construct this type.
pub struct PreparedSpan {
    unit: AnalysisUnitId,
    start: usize,
    end: usize,
}

impl PreparedSpan {
    // No public constructor. The only public adapter is
    // PreparedSource::prepared_span, which stamps the correct unit id.
    pub fn unit(&self) -> AnalysisUnitId;
    pub fn start(&self) -> usize;
    pub fn end(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

/// A range in an original file. Fields are private; `SourceMap::source_span` is
/// the only checked constructor and always stamps the map's unit.
pub struct SourceSpan {
    source: SourceId,
    start: usize,
    end: usize,
}

impl SourceSpan {
    pub fn source(&self) -> SourceId;
    pub fn start(&self) -> usize;
    pub fn end(&self) -> usize;
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

/// Map-local handle of one origin in the arena.
pub struct OriginId {
    unit: AnalysisUnitId,
    index: u32,
}

impl OriginId {
    pub fn unit(&self) -> AnalysisUnitId;
}

/// One link in a provenance chain. Fields are private: only the owning
/// `SourceMap` builder can construct an origin, and it validates every span
/// against the map's unit before insertion.
pub struct SourceOrigin {
    kind: OriginKind,
    span: SourceSpan,              // primary location for this link
    invocation: Option<SourceSpan>,
    definition: Option<SourceSpan>,
    parent: Option<OriginId>,      // same-unit chain towards the root invocation
    label: Option<&'static str>,
}

impl SourceOrigin {
    pub fn kind(&self) -> OriginKind;
    pub fn span(&self) -> SourceSpan;
    pub fn invocation(&self) -> Option<SourceSpan>;
    pub fn definition(&self) -> Option<SourceSpan>;
    pub fn parent(&self) -> Option<OriginId>;
    pub fn label(&self) -> Option<&'static str>;
}

/// A mapped range in the prepared buffer.
pub struct MapSegment {
    pub prepared: PreparedSpan,
    pub origin: OriginId,
}

/// The result of resolving one prepared range.
pub struct ResolvedSpan {
    pub primary: SourceSpan,
    pub related: Vec<SourceSpan>,
}

/// Every coordinate, ownership, or legacy-conversion failure. All variants are
/// errors, not fallbacks: in particular the foreign-identity, map-invariant,
/// unmapped, and legacy-mismatch variants must never silently resolve to the
/// EOF anchor or a local file.
pub enum SpanError {
    /// `start > end` in either coordinate namespace.
    InvertedRange,
    /// A prepared or original offset is past the end of its text.
    OutOfBounds,
    /// An offset splits a UTF-8 scalar value.
    NotCharBoundary,
    /// A `SourceSpan` comes from another AnalysisUnit.
    ForeignAnalysisUnit,
    /// A `PreparedSpan` comes from another PreparedSource/AnalysisUnit.
    ForeignPreparedSource,
    /// `resolve_point` received a non-empty prepared span.
    NotAPoint,
    /// The unit is not a plain-LSL root unit, so the Legacy path cannot render.
    NotLegacyUnit,
    /// A `SourceSpan` names a `SourceId` not registered in this unit.
    UnknownSourceId,
    /// An `OriginId` index is absent from this unit's origin arena.
    UnknownOriginId,
    /// A prepared range is not covered by any map segment (builder bug).
    UnmappedPreparedRange,
    /// Segments overlap, are unsorted, or violate another map invariant.
    MapInvariant,
}

pub struct SourceMap {
    unit: AnalysisUnitId,
    prepared_text: Arc<str>,     // validation only; never a SourceFile
    files: Vec<SourceFile>,      // all ids in this vec carry `unit`
    segments: Vec<MapSegment>,   // sorted, non-overlapping, cover [0, prepared_len)
    eof_anchor: SourceSpan,      // carries `unit`
    origins: Vec<SourceOrigin>,  // arena; chains are acyclic
}

impl SourceMap {
    pub fn unit(&self) -> AnalysisUnitId;

    /// Resolve a prepared range. Every well-formed range of this unit's
    /// PreparedSource is guaranteed to return `Ok`, including EOF and
    /// zero-length ranges and ranges crossing segment boundaries. A span from
    /// another unit returns `ForeignPreparedSource`. Malformed ranges and map
    /// holes return `Err`; they never fall back to the EOF anchor.
    pub fn resolve_span(&self, prepared: PreparedSpan) -> Result<ResolvedSpan, SpanError>;

    /// Resolve a zero-length prepared point of this unit. `point.start ==
    /// point.end` is required; `resolve_point` returns `NotAPoint` otherwise.
    /// The point at `prepared_len` resolves to the EOF anchor. Out-of-bounds
    /// points, non-UTF-8 boundaries, and foreign units return `Err`.
    pub fn resolve_point(&self, point: PreparedSpan) -> Result<SourceSpan, SpanError>;

    /// Turn an original-file span into line/column coordinates. A span from
    /// another AnalysisUnit returns `ForeignAnalysisUnit` before any file
    /// lookup. Unknown file indexes and out-of-file ranges return `Err`.
    pub fn location(&self, span: SourceSpan) -> Result<SourceLocation, SpanError>;

    /// Build the frozen public `Span` for a resolved original range of this
    /// unit, so the existing `Diagnostic` shape can be rendered unchanged.
    pub fn render_span(&self, span: SourceSpan) -> Result<Span, SpanError>;

    /// Checked constructor for original spans of this unit. A foreign
    /// `SourceId` returns `ForeignAnalysisUnit`; the result always carries this
    /// map's unit id.
    pub fn source_span(
        &self,
        source: SourceId,
        start: usize,
        end: usize,
    ) -> Result<SourceSpan, SpanError>;

    /// The anchor used for empty buffers and end-of-input diagnostics. It
    /// belongs to this unit and is valid by construction.
    pub fn eof_anchor(&self) -> SourceSpan;

    /// Resolve an origin handle from this unit's arena. A foreign-unit handle
    /// returns `ForeignAnalysisUnit`; a missing index returns
    /// `UnknownOriginId`. Every span in the returned origin has already been
    /// unit-validated at construction and is revalidated defensively.
    pub fn origin(&self, id: OriginId) -> Result<&SourceOrigin, SpanError>;
}

/// The single prepared buffer handed to the lexer for one AnalysisUnit.
pub struct PreparedSource {
    unit: AnalysisUnitId,                // one per prepared AnalysisUnit
    text: Arc<str>,                      // shared with the map for validation
    pub preprocess_diagnostics: Vec<PreprocessDiagnostic>,
    map: Arc<SourceMap>,                 // same unit by construction
}

impl PreparedSource {
    /// The only public Span -> PreparedSpan adapter. It stamps this source's
    /// unit id and validates inverted/out-of-bounds/non-UTF-8 ranges.
    ///
    /// A `Span` carries no source identity, so this adapter cannot prove that
    /// the span was produced by *this* PreparedSource. It is a checked
    /// adaptation of a trusted caller-provided span, not provenance recovery.
    /// Engines must only be given the PreparedSource they lexed.
    pub fn prepared_span(&self, span: Span) -> Result<PreparedSpan, SpanError>;

    /// A zero-length prepared span at the end of this buffer, carrying this
    /// source's unit id. This is the input for `SourceMap::resolve_point`.
    pub fn eof_span(&self) -> PreparedSpan;

    pub fn unit(&self) -> AnalysisUnitId;
    pub fn prepared_len(&self) -> usize;
    pub fn map(&self) -> &SourceMap;
    pub fn text(&self) -> &str;
}

/// One secondary location attached to a rendered or preprocessor diagnostic.
/// Every contained span must belong to the same AnalysisUnit as the primary;
/// conversion validates this and returns `ForeignAnalysisUnit` otherwise.
pub struct RelatedLocation {
    pub span: SourceSpan,
    pub label: String,
}

/// Preprocessor diagnostics are born in original coordinates. They never pass
/// through `resolve_span`.
pub struct PreprocessDiagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub primary: SourceSpan,             // always this unit's original file
    pub related: Vec<RelatedLocation>,   // macro def/invocation, include site
}

/// Engine diagnostics are born in prepared coordinates. `span` is exactly the
/// frozen `Token.span`/`Diagnostic.span` shape produced from `PreparedSource`.
pub struct PreparedDiagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Span,                      // prepared coordinates
    pub related: Vec<PreparedSpan>,      // additional prepared ranges
}

/// A frozen legacy `Diagnostic` tagged with the unit that produced it.
/// Fields are private and there is no public constructor: only the unit's
/// legacy engine/pipeline can create one. `Diagnostic.span` is untouched.
pub struct LegacyDiagnostic {
    diagnostic: Diagnostic,
    unit: AnalysisUnitId,
}

impl LegacyDiagnostic {
    // pub(crate) fn new(diagnostic: Diagnostic, unit: AnalysisUnitId) -> Self;
    pub fn diagnostic(&self) -> &Diagnostic;
    pub fn unit(&self) -> AnalysisUnitId;
}

/// Everything the unified layer accepts.
pub enum UnifiedDiagnosticInput {
    /// Preprocessor finding, already in original coordinates.
    Preprocess(PreprocessDiagnostic),
    /// Lexer/parser/semantic finding in prepared coordinates.
    Prepared(PreparedDiagnostic),
    /// Plain-LSL compatibility finding; tagged with its producing unit.
    Legacy(LegacyDiagnostic),
}

/// The rendered form: the frozen `Diagnostic` plus file identity and notes.
/// Fields are private; the constructors below are the only ways to build one,
/// so a foreign span cannot bypass the ownership checks.
pub struct RenderedDiagnostic {
    diagnostic: Diagnostic,          // unchanged public shape
    source: SourceId,                // original file, unit included
    related: Vec<RelatedLocation>,
}

impl RenderedDiagnostic {
    pub fn diagnostic(&self) -> &Diagnostic;
    pub fn source(&self) -> SourceId;
    pub fn related(&self) -> &[RelatedLocation];

    /// Direct insertion; primary and every related span must belong to the
    /// unit of `prepared`, whose map is used. No prepared-buffer resolution is
    /// involved.
    pub fn from_preprocess(
        diagnostic: PreprocessDiagnostic,
        prepared: &PreparedSource,
    ) -> Result<Self, SpanError>;

    /// Explicit bridge then SourceMap resolution. The map is obtained from the
    /// `PreparedSource`; callers cannot supply a different map for this unit.
    pub fn from_prepared(
        diagnostic: PreparedDiagnostic,
        prepared: &PreparedSource,
    ) -> Result<Self, SpanError>;

    /// Pass-through for the plain-LSL compatibility path. The source is
    /// obtained exclusively from `unit.legacy_root()`; the caller cannot supply
    /// a `SourceId`, so another unit's or an included file's id cannot be used.
    /// `legacy.unit()` must equal `unit.id()` (`ForeignAnalysisUnit`
    /// otherwise), and the unit must expose a legacy root (`NotLegacyUnit`
    /// otherwise). No source map is involved and `Diagnostic.span` is copied
    /// unchanged, preserving line/column/order and baseline output.
    pub fn from_legacy(
        diagnostic: LegacyDiagnostic,
        unit: &AnalysisUnit,
    ) -> Result<Self, SpanError>;
}
```

`PreparedSpan` and `SourceSpan` are distinct types with no implicit conversion
between them, and both carry ownership identity. A compiled-in mix-up is
therefore a type error at best and an explicit `SpanError` at worst, never a
silent diagnostic in another file. `SourceSpan::source()` returns a `SourceId`
that contains the owning `AnalysisUnitId`; the prepared buffer has no
`SourceId`, but `PreparedSpan` carries the unit and is rejected by any other
unit's map. Fields are private, so only the checked constructors can create
coordinates.

### 7.1.1 The `Span` bridge (frozen legacy type)

The public types stay exactly as they are:

```rust
pub struct Token { pub kind: TokenKind, pub lexeme: String, pub span: Span }
pub struct Diagnostic { pub code: &'static str, pub severity: Severity,
                        pub kind: DiagnosticKind, pub message: String,
                        pub span: Span }
pub fn lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>);
```

`Span` is a byte range plus line/column metadata, and it **has no coordinate
tag**. Its meaning depends on the text that produced it:

| Producer | `Span` coordinates |
| --- | --- |
| `lex` on plain LSL source (legacy path) | original root file |
| `lex` on `PreparedSource` (preprocessed path) | prepared buffer |
| lexer/parser/semantic diagnostics on `PreparedSource` | prepared buffer |

Rules for the bridge:

1. In the preprocessed path, `Token.span` and engine `Diagnostic.span` address
   the prepared buffer. They are never assumed to be original coordinates.
2. There is exactly **one** public adapter:
   `PreparedSource::prepared_span(span) -> Result<PreparedSpan, SpanError>`.
   It validates inversion, `prepared_len`, and UTF-8 boundaries, and stamps the
   owning `AnalysisUnitId`. There is no public `PreparedSpan::new`,
   `try_from_lexer`, `From<Span>`, or `Into<PreparedSpan>`.
3. The adapter cannot recover provenance from a `Span`, because `Span` contains
   no source identity. It is a **checked adaptation** of a span the caller
   asserts came from this prepared buffer. A `Span` from a different source
   with a fitting range cannot be detected at this step; the contract is that
   engines are only ever handed the `PreparedSource` they lexed. Identity is
   assigned by `PreparedSource`, not verified against the `Span`.
4. `Span.line`/`Span.column` are display metadata and are ignored by the bridge.
   Line/column for a rendered diagnostic are recomputed from the resolved
   original `SourceSpan` (`SourceMap::render_span`).
5. `PreparedSpan -> SourceSpan` happens **only** through
   `SourceMap::resolve_span`/`resolve_point`. The map first checks that the
   span's unit is its own unit and returns `ForeignPreparedSource` otherwise;
   matching `start`/`end` alone is never sufficient.
6. `SourceSpan` can never be constructed from prepared bytes: its constructor
   is `SourceMap::source_span(source, start, end)`, which returns
   `ForeignAnalysisUnit` for another unit's `SourceId` and otherwise stamps the
   map's unit.
7. `SourceLocation` is computed only from a `SourceSpan` via
   `SourceMap::location`; a foreign-unit span returns `ForeignAnalysisUnit`
   before any file lookup.
8. AST nodes store prepared coordinates (`Span` or `PreparedSpan`), never a
   `SourceSpan` for generated text. They are resolved through the same
   `SourceMap` at rendering time.
9. In plain-LSL compatibility mode there is no map:
   `UnifiedDiagnosticInput::Legacy(LegacyDiagnostic)` verifies its producing
   unit and passes the enclosed `Diagnostic.span` through unchanged, preserving
   baseline output. The source is obtained from that unit's `legacy_root()`;
   no caller-supplied `SourceId` or prepared-span conversion is involved.

These are conceptual definitions. Phase 1 fixes the exact visibility, derives,
constructors, and capacities; the semantics below are binding.

### 7.2 Coordinate namespaces and offsets

- `AnalysisUnitId` is `{ RunId, index }`: the CLI assigns the index
  deterministically as `0..N-1` in collected-input order, and every invocation
  gets a distinct process-local `RunId`. The id is not persisted and does not
  appear in output or ordering.
- `SourceId` is unit-scoped and contains `{ AnalysisUnitId, file: u32 }`. Two
  units may both have local file index 0, but those are different `SourceId`
  values. A raw file index must never be compared across units or stored
  without its unit.
- **Lifetime boundary across invocations.** Coordinates are plain `Copy` values
  and can escape the invocation that produced them, so lifetimes alone do not
  prevent reuse. `RunId` is therefore part of every `AnalysisUnitId`, and every
  map checks the full id. A coordinate from invocation 1 cannot be accepted by
  invocation 2's map even when both use unit index 0 and identical offsets; the
  check fails with `ForeignAnalysisUnit`/`ForeignPreparedSource`. This is a
  runtime ownership check, not a compile-time guarantee. `RunId` is allocated
  process-locally (mutex-protected checked monotonic counter) and never affects output.
- Every offset is a zero-based UTF-8 byte offset. Spans are half-open
  (`start..end`) and must lie on UTF-8 character boundaries.
- `PreparedSpan` addresses bytes **only** in the prepared buffer of a specific
  `AnalysisUnitId`. It has no `SourceId` (the buffer is not a file) but it does
  carry the unit, so a map can reject a span from another unit.
- `SourceSpan` addresses bytes **only** in an original `SourceFile`. It is the
  only span type that can be converted to `SourceLocation`.
- A prepared buffer is not a `SourceFile`; it has no display name in `files` and
  no `SourceId`.
- Zero-length spans are legal in both namespaces. A `PreparedSpan` with
  `start == end == n` denotes a point in the prepared buffer; a `SourceSpan` for
  source `s` with `start == end == n` denotes a point in a real file. Point
  semantics are used for EOF, inserted text, and "declared here" locations.
- `SourceSpan` must always name a real original file. There is no synthetic or
  "builtin" source id in this model; generated text inherits the nearest real
  anchor.
- `Span` is the frozen legacy type and is **not** a third namespace. It is
  interpreted as prepared coordinates only when it came from lexing a
  `PreparedSource`; in the plain-LSL path it is original coordinates. The
  bridge in §7.1.1 is explicit and checked, and no `From`/`Into` conversion
  exists between `Span` and either span type.
- `PreparedSpan` and `SourceSpan` fields are private; checked constructors are
  the only way to build them. This makes invalid coordinate mixes compile-time
  errors or explicit `SpanError` values, not silent off-by-file diagnostics.
- Ownership is checked before geometry. `SourceMap::location`/`source_span`
  return `ForeignAnalysisUnit` for another unit's `SourceId`;
  `resolve_span`/`resolve_point` return `ForeignPreparedSource` for another
  unit's `PreparedSpan`. A coincidentally equal file index or equal
  `start`/`end` never permits acceptance.

### 7.3 Empty input, EOF, and segment boundaries

These cases are normative; Phase 1 must test each of them.

**Empty source file.** The `SourceFile` has `len() == 0`. The map has no
segments and an `eof_anchor` that is a zero-length span at offset 0 of the root
file, carrying the unit id. The lexer emits only `Eof` at byte offset 0;
`PreparedSource::eof_span()` (or `prepared_span` on that `Eof` span) yields a
zero-length `PreparedSpan` stamped with this unit.
`resolve_span(...)` returns `Ok(ResolvedSpan { primary: eof_anchor, related:
[] })`. `PAR013` therefore resolves to the root file at line 1, column 1,
exactly like today's legacy output.

**Empty prepared buffer from non-empty source.** Conditional compilation can
remove all user code. The prepared buffer may be empty even though the root file
is not. Segments then cover the empty range `[0, 0)` (vacuously), and
`eof_anchor` is a real position derived from the last processed directive (the
end of the last direct/anchor segment), or the root start if no anchor exists.
EOF diagnostics still resolve to a real file and a real line/column.

**EOF token and EOF diagnostics.** The lexer's `Eof` token always has byte
offset `prepared_len`; `PreparedSource::eof_span()` yields the zero-length
`PreparedSpan` at that offset stamped with this unit. Parser diagnostics at end
of input (for example `PAR013`, an incomplete block, or a missing closing brace)
use the same zero-length prepared span. Resolution rule:

- `resolve_point(source.eof_span())` returns `Ok(eof_anchor)` for the same unit.
- `resolve_span(source.eof_span())` returns `Ok(anchor)` with no related
  locations.
- A zero-length prepared span that is **not** at the end resolves as a point
  (below).
- Only well-formed EOF ranges take this path. A malformed range is `Err`, never
  the anchor.

**Point resolution and segment boundaries.** Segments are half-open
`[start, end)`. `resolve_point(point: PreparedSpan)` accepts only a zero-length
span. Resolution order:

1. If `point.unit() != self.unit()`, return
   `Err(SpanError::ForeignPreparedSource)`.
2. If `point.start() != point.end()`, return `Err(SpanError::NotAPoint)`.
3. If `point.start() > prepared_len`, return `Err(SpanError::OutOfBounds)`.
4. If the offset is not a UTF-8 character boundary in the prepared text, return
   `Err(SpanError::NotCharBoundary)`.
5. If `point.start() == prepared_len`, return `Ok(eof_anchor)`.
6. Otherwise, return the origin of the unique segment with
   `segment.prepared.start <= offset < segment.prepared.end`. An offset exactly
   at a boundary belongs to the segment that **starts** at that offset
   (right-biased). This makes boundary resolution deterministic.
7. If no such segment exists, return `Err(SpanError::UnmappedPreparedRange)` or
   `Err(SpanError::MapInvariant)`. Never fall back to the EOF anchor and never
   panic.

For a zero-length `PreparedSpan` with `start == end < prepared_len`,
`resolve_span` returns `Ok(primary = resolve_point(point)?, related = [])`.

**Span crossing several segments.** A prepared span may cross any number of
segment boundaries (a macro argument followed by macro body, then by direct
code). `resolve_span` must:

0. Reject a span whose unit is not this map's unit with
   `Err(SpanError::ForeignPreparedSource)` before any geometry check.
1. Split the prepared range at every segment boundary into 1..N contiguous
   prepared runs. A zero-length span yields one point run.
2. Resolve every run through its **entire** origin chain, after validating
   every chain node:
   - a user-visible immediate origin (`Direct`, `Include`, `MacroInvocation`,
     `MacroArgument`, `MacroDefinition`) maps by relative offsets only when the
     segment and origin ranges have equal length; otherwise its `span` anchor is
     used, without inventing an offset correspondence;
   - `Generated` walks outward to the nearest user-visible origin;
   - `TokenPaste`/`Stringify` use their invocation site when present, otherwise
     continue outward;
   - if no user-visible node exists, the nearest validated anchor is used.
3. Collect related spans from **all** chain levels: ancestor spans and every
   `invocation`/`definition` on the chain, plus the other prepared runs. The
   immediate origin's own span is not duplicated when it is the primary.
4. For point resolution, apply the same chain walk: relative mapping only for a
   user-visible immediate origin with equal range lengths, otherwise the
   corresponding user anchor point. `prepared_len` still resolves to the EOF
   anchor.
5. Choose `primary` by the resolved run priority: `Direct` > `MacroArgument` >
   `MacroDefinition` > `Include` > `TokenPaste`/`Stringify` > `Generated`.
   `MacroInvocation` shares the `MacroArgument` user-code tier (it is not used
   as a segment origin in Phase 1). Ties are broken by the leftmost prepared
   run. This ensures a diagnostic that starts in pasted text but continues into
   direct code still points at the user-written code when one exists.
6. Put the remaining distinct spans into `related`, ordered by prepared run and
   chain level, deduplicated by `(source, start, end)`.
7. Never return a prepared coordinate or a synthetic file. If every run is
   generated, every origin still points at a real anchor, so `primary` remains a
   real user location.
8. If a segment's origin chain is broken or cyclic, or the segment table has a
   hole or an overlap, return `Err(SpanError::MapInvariant)`; a foreign chain
   node/span returns the corresponding `ForeignAnalysisUnit`. A malformed chain
   is never partially resolved or repaired by pointing at the EOF anchor.

Consequences:

- A diagnostic on a token produced by a macro argument points at the argument
  text; the macro definition and invocation become related locations.
- A diagnostic on a token manufactured by `##` points at the invocation site as
  the primary location and never claims that the pasted spelling exists in a
  file.
- A diagnostic whose range spans two files (through an include boundary) reports
  the primary file plus the other file as a related location.

### 7.3.1 Invalid ranges and coordinates

Invalid coordinates never panic, never silently resolve to the EOF anchor, and
never become a user-facing location in the wrong file. They are `Err` values.

| Invalid input | Error | Detection point |
| --- | --- | --- |
| `start > end` in a prepared range | `SpanError::InvertedRange` | `PreparedSource::prepared_span`, `resolve_span` |
| `start > end` in an original range | `SpanError::InvertedRange` | `SourceMap::source_span`, `location` |
| `end > prepared_len` | `SpanError::OutOfBounds` | `PreparedSource::prepared_span`, `resolve_span`, `resolve_point` |
| `end > source_file.len()` | `SpanError::OutOfBounds` | `SourceMap::source_span`, `location` |
| prepared offset splits UTF-8 | `SpanError::NotCharBoundary` | `prepared_span`, `resolve_point` |
| original offset splits UTF-8 | `SpanError::NotCharBoundary` | `SourceMap::source_span`, `location` |
| `SourceSpan` from another AnalysisUnit | `SpanError::ForeignAnalysisUnit` | `location`, `source_span`, `render_span`, related conversion |
| `PreparedSpan` from another unit/source | `SpanError::ForeignPreparedSource` | `resolve_span`, `resolve_point`, `from_prepared` |
| `resolve_point` given a non-empty span | `SpanError::NotAPoint` | `resolve_point` |
| `SourceId` not registered in the unit | `SpanError::UnknownSourceId` | `location`, `source_span` |
| `OriginId` from another unit/run | `SpanError::ForeignAnalysisUnit` | `origin`, chain traversal |
| `OriginId` index absent from the arena | `SpanError::UnknownOriginId` | `origin`, chain traversal |
| Legacy diagnostic tag/mode mismatch | `SpanError::ForeignAnalysisUnit` / `NotLegacyUnit` | `from_legacy` |
| prepared range has no segment | `SpanError::UnmappedPreparedRange` | `resolve_span`, `resolve_point` |
| segments overlap, are unsorted, or an origin chain is broken | `SpanError::MapInvariant` | `resolve_span`, `resolve_point` |

Categories:

- **Correct EOF and zero-length ranges are not errors.** A zero-length
  `PreparedSpan` at `prepared_len` and at `n < prepared_len` resolves to `Ok`.
  `SourceSpan` zero-length points are valid original positions.
- **Malformed coordinates always return `Err`.** Behavior is identical in debug
  and release builds: there are **no** `debug_assert!`/`assert!` calls in the
  `SourceMap` resolution path, the `Span` adapter, or the diagnostic conversion
  path. Public APIs never panic on malformed coordinates, and malformed
  coordinates never become the EOF anchor.
- **Foreign identity is its own error class.** A span from another
  AnalysisUnit/PreparedSource is rejected with `ForeignAnalysisUnit` or
  `ForeignPreparedSource` before geometry or file lookup. Matching local file
  indexes or matching `start`/`end` are irrelevant; no fallback file is used.
- **These malformed/foreign cases are internal or API-misuse errors.** The
  unified layer propagates the error fail-fast within the unit: no partial unit
  result is accepted, the CLI reports `neolint: cannot analyze unit N`, and the
  process exits 2. Other units are not analyzed as part of a partial result.
- **User preprocessor errors are diagnostics, not `SpanError`s.** An unknown
  directive, bad `#if` expression, missing include, or recursive macro keeps a
  valid `SourceSpan` of its own unit and becomes a `PreprocessDiagnostic`. It is
  not an invalid coordinate and does not take the `Err` path.
- **Assertions, if any, must be unreachable from public input.** Optional
  internal assertions are allowed only in pipeline constructors that cannot be
  called with user-controlled coordinates (for example, asserting a freshly
  built segment table is sorted). They must not be used to detect malformed
  spans; the error path is always `Result`. The preferred design has none in the
  resolution path.

The public API is `Result`-based rather than `Option`-based on purpose: a
caller must not be able to `unwrap` a missing location on untrusted input.
`SpanError` implements `Display` and `std::error::Error` so the CLI can log it
without a dependency.

### 7.4 Lines and columns

- Line indices are built per file from the file's own bytes and match the
  existing lexer exactly: `\n` increments the line; `\r` also increments the
  line and consumes an immediately following `\n` as the same logical newline.
  A lone `\r` is therefore a line break, not an ignored byte.
- Columns count Unicode scalar values from the beginning of the line, starting
  at 1, matching the lexer's current implementation.
- The Source Manager owns each file's line index.
  `SourceMap::location(SourceSpan)` first checks that the span's
  `AnalysisUnitId` equals the map's unit (`ForeignAnalysisUnit` otherwise), then
  consults that index on demand; byte offsets remain the canonical identity and
  line/column are derived data. Invalid source spans return `Err(SpanError)`,
  never a guessed position. Preprocessor diagnostics use this path directly,
  without any prepared-buffer lookup.
- The lexer's `Span.line`/`Span.column` describe the prepared buffer. They are
  used for internal ordering and debugging; rendered diagnostics use the
  original file location via `SourceMap::render_span`.

### 7.5 Mapping model

- The prepared buffer is covered by ordered, non-overlapping segments.
- Every segment has exactly one **primary origin** (`MapSegment.origin`) and an
  optional chain of **secondary origins** reachable through
  `SourceOrigin::parent()`, `invocation()`, and `definition()`.
- The primary origin answers "where should the user look to fix this?".
  Secondary origins answer "how did this code get here?" and are rendered as
  notes, never as the diagnostic's main location.
- Direct code: one segment covering the copied bytes, origin `Direct`.
- Include: the included file's copied bytes carry origin `Include` with the
  include directive site recorded as the invocation and the included file range
  as the span. The include directive site is the secondary origin, so a finding
  inside an include can also point back at the `#include` line.
- Macro invocation: tokens produced by a macro body map to the body range with
  origin `MacroDefinition`, whose `invocation()` points at the call site.
- Macro argument substitution: the argument's own bytes map to the argument
  range with origin `MacroArgument`, whose `invocation()` points at the macro
  call and whose `parent()` chain continues to the definition.
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

### 7.5.1 Origin arena ownership and validation

- `SourceOrigin` has no public constructor and its fields are private. Only the
  owning `SourceMap` builder can insert an origin into the arena; there is no
  public API that accepts a caller-built origin.
- Before insertion, the builder validates `span`, `invocation`, and `definition`
  with the same unit check used by `source_span`. A foreign-unit span aborts the
  insertion with `SpanError::ForeignAnalysisUnit`; the origin is not added and
  no local file is substituted.
- `parent` must be an `OriginId` from the same map and must already exist.
  A foreign-unit id returns `ForeignAnalysisUnit`; a missing same-unit index
  returns `UnknownOriginId`; a cycle or a broken chain returns `MapInvariant`.
- `OriginId` carries its `AnalysisUnitId`; `SourceMap::origin` checks it before
  indexing. Segmentation (`MapSegment.origin`) and every chain walk revalidate
  the unit of each visited origin and of its spans defensively. A corrupted
  arena (only constructible in tests) produces an error, never a note or primary
  location in a local file.
- Consequently, no foreign `SourceSpan` can enter the origin arena, and no
  nested `parent` chain can launder one into a local diagnostic or note. There
  is no silent drop and no fallback location.

### 7.6 Multiple related locations and unified input

`RelatedLocation` is defined once in §7.1. A finding keeps its primary location
separate from its notes.

Input variants and their resolution paths:

| Variant | Primary coordinates | Resolution |
| --- | --- | --- |
| `Preprocess(PreprocessDiagnostic)` | `SourceSpan` in an original file | `SourceMap::location` / `render_span` directly; **never** `resolve_span` |
| `Prepared(PreparedDiagnostic)` | `Span` in the prepared buffer | `PreparedSource::prepared_span` → `resolve_span` → `render_span` |
| `Legacy(LegacyDiagnostic)` | original coordinates, tagged with its unit | tag check + `unit.legacy_root()`, pass-through, no map |

Rules:

- `PreprocessDiagnostic.primary` is already an original `SourceSpan` because the
  directive, macro definition, or include site may have been removed from the
  prepared buffer. Resolving it through prepared coordinates would be
  impossible or wrong.
- `RenderedDiagnostic::from_preprocess` obtains the map from the
  `PreparedSource` and verifies the pair's unit, prepared length, and prepared
  text. Primary and
  related spans must belong to that unit; otherwise it returns
  `ForeignAnalysisUnit` before rendering and never falls back to a same-index
  file of the local unit.
- `PreparedDiagnostic.span` is exactly the frozen `Span` produced by the lexer
  or an engine over the prepared buffer. It is bridged explicitly and then
  resolved. A diagnostic that crosses several segments follows the split and
  priority rules in §7.3.
- `RenderedDiagnostic::from_prepared` obtains the map from the
  `PreparedSource` and verifies the pair's unit, prepared length, and prepared
  text before the `Span` bridge. There is no caller-supplied map parameter, so a same-unit map
  built for a different prepared buffer cannot be substituted. Every related
  span from the primary resolution and from each explicit related prepared range
  is validated, deduplicated, and excluded when it equals the primary.
- `Legacy` carries a `LegacyDiagnostic` created only by the producing unit's
  legacy engine. `RenderedDiagnostic::from_legacy` requires
  `legacy.unit() == unit.id()` (`ForeignAnalysisUnit` otherwise) and obtains the
  source exclusively from `unit.legacy_root()`. If the unit is preprocessed or
  has no loaded root, it returns `NotLegacyUnit`; an included-file `SourceId`
  cannot be supplied because the constructor takes no `SourceId` at all.
  `Diagnostic.span` is copied unchanged, so line, column, order, and baseline
  output are preserved, and no prepared-buffer mapping operation is involved.
- `related` is ordered from most to least relevant: macro definition, include
  site, outer invocations. Each related span is resolved by the same rule as its
  parent variant (direct `location` for `Preprocess`, prepare/resolve for
  `Prepared`), must belong to the same unit, is deduplicated by
  `(source, start, end)`, and never replaces the primary.
- The rendered output is a `RenderedDiagnostic` in which `diagnostic.span` is
  the frozen `Span` shape in original coordinates and `source` is the original
  file (with its `AnalysisUnitId`) for CLI grouping. The existing `Diagnostic`
  type is not changed.
- JSON keeps the existing object shape for the primary location. Related
  locations are added as an optional `related` array in Phase 3 with a
  documented schema note; existing fields never change meaning.
- An `Err` from bridging, foreign-identity checking, or resolution follows
  §7.3.1: fail-fast within the unit (`EngineAnalysisError::Span`); the CLI
  reports `neolint: cannot analyze unit N` and exits 2. There is no partial
  result, panic, or EOF fallback.

### 7.6.1 Worked examples: where each primary location comes from

| Case | Kind | Primary location | Related locations | Resolution path |
| --- | --- | --- | --- | --- |
| Unknown directive, e.g. `#frobnicate` | `PreprocessDiagnostic` | The directive range in the file that contains it | none | Direct `SourceSpan`; no map lookup |
| Malformed `#if` expression, e.g. `#if (1 +` | `PreprocessDiagnostic` | The expression range after `if`; if the expression is empty, a zero-length point at the directive end | The opening directive range | Direct `SourceSpan` |
| Unterminated conditional block | `PreprocessDiagnostic` | The opening `#if`/`#ifdef`/`#ifndef` directive range (the actionable place) | A zero-length point at end of the containing file | Direct `SourceSpan` |
| Missing include, e.g. `#include "missing.lsl"` | `PreprocessDiagnostic` | The path spelling range inside the directive (`missing.lsl`) | The full directive range | Direct `SourceSpan` |
| Recursive macro, e.g. `#define A A` then `A` | `PreprocessDiagnostic` | The invocation that recursed (the `A` occurrence) | The macro definition body; outer invocation if nested | Direct `SourceSpan` |
| EOF diagnostic after all code was removed by `#if 0 ... #endif` | Engine `PAR013` | The map's EOF anchor: a real zero-length position at the end of the last directive/anchor (or root start) | none | `PreparedSource::eof_span()` → `resolve_span` → `eof_anchor` |

Why the paths differ:

- Preprocessor findings are attached to source text that may no longer exist in
  the prepared buffer. Their `SourceSpan` is recorded while scanning the
  original file, so there is nothing to resolve and no prepared range to use.
- The EOF case is an engine finding: the parser only sees the prepared buffer,
  produces a zero-length prepared span at its end, and the map translates it to
  a real file position. This is the only way `PAR013` can be rendered when
  conditional compilation removed everything.
- If the directive appears inside an include, the primary `SourceId` is that
  included file, not the root; the include site becomes a related location. The
  `AnalysisUnitId` is still the unit under analysis, so the span is valid for
  that unit's map.
- Every span in these examples belongs to exactly one unit. A `PreprocessDiagnostic`
  built with another unit's span is rejected with `ForeignAnalysisUnit`; an
  engine diagnostic resolved against another unit's map is rejected with
  `ForeignPreparedSource`.

### 7.7 Why not a line map

Line mapping cannot represent: several macro arguments on one line, a macro
expanding to zero or many lines, token paste, includes that overlay multiple
files, or AST nodes that span a macro boundary. Range-based segments with
explicit chains are required, and they are usable both for tokens (range
lookup) and for future AST nodes (span lookup).

### 7.8 Lookup and performance

- Segment lookup is a binary search over prepared ranges: `O(log n)`.
- Point resolution at a segment boundary is one comparison away from the binary
  search result; `offset == prepared_len` short-circuits to the EOF anchor.
- A span crossing `k` segments costs `O(log n + k)`. `k` is bounded by the
  number of macro/include transitions inside one span, not by file size.
- Origin chains are an arena; walking is `O(depth)` with a hard depth cap.
- Each file's line index is built once per unit (`O(file bytes)`), not per
  diagnostic.
- The map is immutable and shared with `Arc`; there is no interior mutability in
  the analysis path.
- Resolution is total for every well-formed range of the map's own unit in
  `[0, prepared_len]`, including empty buffers, EOF, and zero-length ranges.
  Foreign, malformed, and unmapped ranges return `SpanError`, are handled once
  at the unified layer, and never fall back to the EOF anchor.
- `resolve_*` returns `Result`, not `Option`: callers cannot `unwrap` a missing
  location on user input. `eof_anchor()` is the only infallible resolver and is
  valid by construction.

---

## 8. Error handling per stage (H)

| Stage | Failure | Behavior | Exit code |
| --- | --- | --- | --- |
| CLI | Unknown option, bad `--severity`, bad `--format` | Message on stderr, no analysis | 2 |
| Config | Malformed setting, invalid rule code, unknown key | Message on stderr | 2 |
| Input collection | Path metadata failure, empty directory | Message on stderr | 2 |
| Root read | Missing file, permission, invalid UTF-8 | Message on stderr | 2 |
| Source Manager | Include missing/unreadable/invalid UTF-8/not a file | `PreprocessDiagnostic` with a direct original `SourceSpan` at the include site; analysis continues | 1 |
| Preprocessor | Unknown directive, malformed macro, unterminated conditional, bad `#if` expression, path rejection | `PreprocessDiagnostic`, best-effort recovery; the construct is skipped | 1 |
| Preprocessor | Include cycle, expansion depth/size limit, diagnostic limit | `PreprocessDiagnostic` once, expansion stopped at the limit | 1 |
| Span bridge / map | Inverted, out-of-bounds, non-UTF-8, foreign unit/source/run, unknown `SourceId`/`OriginId`, non-point, map hole, origin cycle, broken segments | `SpanError` propagated as `EngineAnalysisError::Span`; fail-fast within the unit; no panic, no EOF fallback, no partial unit result | 2 |
| Legacy conversion | Tagged unit differs from the target unit, or the unit is not a plain-LSL root | `ForeignAnalysisUnit` / `NotLegacyUnit`; fail-fast within the unit; no map, no fallback file | 2 |
| Lexer | Unterminated string/comment, unsupported character | `LEX` diagnostic with prepared `Span`, tokens recovered | 1 |
| Parser | Syntax error | `PAR` diagnostic with prepared `Span`, bounded recovery | 1 |
| Semantic | Rule violation | `SEM` diagnostic with prepared `Span` | 1 |
| Any stage | Internal invariant violation | `SpanError`; fail-fast within the unit; the CLI reports `neolint: cannot analyze unit N`; never a fallback location, `panic!`, `unwrap`, or unchecked slicing on user input | 2 |
| Both engines | `compare` mode disagreement | Reported on stderr as an unmatched unit; never a user-facing panic | 3 |

Rules:

- User input is untrusted data. Indexing user-controlled offsets must go through
  checked accessors; arithmetic on offsets uses checked or saturating operations.
- `PreprocessDiagnostic` uses the existing `DiagnosticKind::Syntax` and the
  `PRE` code family. A dedicated `DiagnosticKind` is deliberately deferred:
  adding a value to that public enum changes the documented JSON `kind` domain
  and needs its own compatibility decision (Phase 3, recorded here when
  decided).
- Keep three failure categories separate:
  - **valid EOF/zero-length** — normal `Ok` resolution, including `PAR013` on an
    empty prepared buffer;
  - **malformed, foreign, or legacy-mismatched input** — `SpanError`
    (`ForeignAnalysisUnit`, `ForeignPreparedSource`, `NotLegacyUnit` included),
    fail-fast within the unit: the engine returns the error, the CLI reports
    `neolint: cannot analyze unit N`, and the process exits 2. A partially
    analyzed unit is never accepted or printed. Same behavior in debug and
    release: no assertions and no panic on this path. A foreign origin span/id is
    never silently dropped and never resolved to a local file;
  - **user preprocessor errors** — `PreprocessDiagnostic` with a valid original
    `SourceSpan` of its own unit; they never take the `SpanError` path.
- Every new diagnostic code must be added to `docs/diagnostics.md`. The
  documentation test scans `src/lexer.rs`, `src/parser.rs`, `src/semantic.rs`,
  and every Rust file under `src/preprocessor/`; Phase 4 must extend it to the
  AST modules so new codes cannot go undocumented.
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
   period. New functionality is additive. `parse_legacy` and `parse_ast` are
   added as independent entry points; `parse` stays behaviorally identical to
   `parse_legacy`.
3. **Diagnostics are unchanged in plain LSL.** Same codes, severities, kinds,
   messages, spans, order, human output, JSON output, and exit codes. The
   existing 350 tests and the corpus manifest are the gate.
   In this mode `Diagnostic.span` is an original-coordinate `Span` and travels
   through `UnifiedDiagnosticInput::Legacy` untouched.
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
   then both engines are supported, public, and selectable, including for
   differential testing.
10. **`Span` keeps its dual interpretation.** Its type and fields do not change;
    the bridge to `PreparedSpan` is explicit and checked, and only reachable
    through a `PreparedSource` (the adapter stamps that source's unit).
    Existing callers of `lex`/`parse` never see `PreparedSpan` unless they opt
    into the new API.
11. **Identity-bearing types are additive.** `RunId`, `AnalysisUnitId`, the
    expanded `SourceId`, `PreparedSpan`/`OriginId` ownership, and the
    foreign-identity errors are new APIs. They are not part of the frozen legacy
    surface and do not change `Span`, `Token`, `Diagnostic`, or any existing
    function signature. `RunId` is process-local and never affects output.
12. **New diagnostic types are additive.** `LegacyDiagnostic`,
    `PreprocessDiagnostic`, `PreparedDiagnostic`, `UnifiedDiagnosticInput`,
    `RenderedDiagnostic`, and `SpanError` are new exports. `Diagnostic` is not
    modified and remains the rendered single-location type.
13. **The Legacy constructor cannot choose a source.** `from_legacy` takes a
    unit-tagged `LegacyDiagnostic` and an `AnalysisUnit` handle, never a
    `SourceId`; it uses the unit's root only and copies `Diagnostic.span`
    unchanged. Baseline output and ordering are unaffected.

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

### 10.2 `//fspreprocessor off` (global switch, not a section)

The marker is a **script-wide kill switch** in Firestorm, not a regional
directive. Verified against
[`fslslpreproc.cpp`](https://github.com/FirestormViewer/phoenix-firestorm/blob/4643f4acb7a50624cd30acbf6ae4b5157fa207e6/indra/newview/fslslpreproc.cpp#L1145-L1159)
at commit `4643f4acb7a50624cd30acbf6ae4b5157fa207e6`:

```cpp
std::string rinput = input;
bool preprocessor_enabled = true;
{
    std::string::size_type location_index = rinput.find("//fspreprocessor off");
    if (location_index != std::string::npos)
    {
        ... display_message("... disabled by script marker" ...);
        preprocessor_enabled = false;
    }
}
```

Behavior that neolint must reproduce:

- The check is a raw substring search over the entire original script text, not
  a parsed comment or directive scan.
- If the literal `//fspreprocessor off` occurs **anywhere** in the file — at the
  start, in the middle, at the end, inside a string literal, or inside a block
  comment — the whole script is processed with preprocessing disabled.
- Only the first occurrence matters; there is no re-enable marker and no
  region/section semantics.
- When disabled, Firestorm does not run Boost.Wave, does not expand includes or
  macros, and skips its post-processing transforms (lazy lists, switch rewriting,
  optimizer, compression), all of which are guarded by `preprocessor_enabled`
  (`fslslpreproc.cpp` around the `if (preprocessor_enabled)` and
  `if (preprocessor_enabled && !errored)` blocks). The original text is then
  compiled as ordinary LSL.
- Firestorm displays an informational message whose line number is the count of
  newlines before the marker. neolint mirrors this as an informational
  `PreprocessDiagnostic` whose primary `SourceSpan` is the marker range in the
  original file; it never changes the exit code.
- Because this is an area where Firestorm's behavior surprises users, the
  implementation must be literal: do not "improve" it into section handling, and
  do not ignore occurrences inside strings or comments.

Required fixture tests (Phase 3):

| Fixture | Expected behavior |
| --- | --- |
| marker absent | normal preprocessing |
| marker at the very start | whole file unprocessed |
| marker in the middle | whole file unprocessed, including code before it |
| marker at the end | whole file unprocessed |
| marker inside a string literal | whole file unprocessed (raw substring match) |
| marker inside a block comment | whole file unprocessed (raw substring match) |

This functionality belongs to the **preprocessor** and is implemented in
Phase 3, not Phase 5.

### 10.3 Include guards and `#pragma once`

Include guards and `#pragma once` are different features and get different
classifications.

- **Classic include guards** (`#ifndef X` / `#define X` / `#endif`) are ordinary
  standard preprocessing. They are fully supported and tested as guaranteed
  preprocessor behavior.
- **`#pragma once`** is a **compatible convenience of neolint**, not a
  guaranteed Firestorm feature:
  - Firestorm's `fslslpreproc.cpp` contains no `#pragma` handling of its own and
    no `#pragma once` code path.
  - Firestorm's pinned Boost.Wave is **1.90** (`autobuild.xml`, `boost`
    dependency), and Boost.Wave 1.90 supports `#pragma once` when
    `BOOST_WAVE_SUPPORT_PRAGMA_ONCE` is enabled, which is the default in
    `wave_config.hpp`. Firestorm does not define or override that macro.
  - However, Firestorm resolves includes through its own inventory-based
    `trace_include_files` hook, and there is no Firestorm end-to-end test or
    documentation proving `#pragma once` behavior through that layer. Boost.Wave
    keys pragma-once state by the filename it sees, which Firestorm supplies from
    inventory names rather than filesystem paths.
  - Therefore neolint implements `#pragma once` as a documented, compatible
    addition. It is **not** part of the guaranteed Firestorm compatibility suite
    and must be tested separately from verified Firestorm behavior.
  - If end-to-end Firestorm evidence becomes available, this classification may
    be upgraded by updating this document and the compatibility tests.
- `#pragma once` identity is per canonical path within one AnalysisUnit, and the
  effect never crosses unit boundaries. This rule applies whether or not the
  canonical path came from the filesystem or from an include lookup.

### 10.4 Built-ins and predefined macros

- Only documented, statically known predefined macros may be provided (for
  example file/line style macros defined by the chosen profile). Their values
  are computed from the source map, never invented.
- No Firestorm built-in may receive a fabricated constant value. Dynamic or
  environment-dependent built-ins stay unresolved and are treated
  conservatively by semantic analysis.
- The macro table is per-AnalysisUnit and never global.

### 10.5 Standard LSL safety

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
- Sorting of diagnostics uses a total, stable key: `(unit index, source load
  order inside the unit, start offset, end offset, code, message)`. `RunId` is
  excluded: it is process-local and must never affect output or ordering.
- `AnalysisUnitId.index` assignment uses collected-input order only. It never
  depends on map iteration, filesystem enumeration, or parallelism. `RunId` is
  allocated per invocation by a process-wide `Mutex<u64>` allocator with a
  checked increment. The mutex serializes concurrent invocations; poisoned
  allocator state and counter exhaustion are explicit setup errors, never
  wraparound or reuse. The allocator lock is not held during analysis.
- New tests must avoid time, randomness, and network; temporary directories
  follow the existing `temporary_directory()` pattern in the integration tests.

---

## 12. Resource limits (L)

Limits are mandatory because preprocessor input is untrusted. The values below
are implemented in `src/source/manager.rs` and `src/preprocessor/` (Phase 2).

| Limit | Implemented value | Enforcement point |
| --- | --- | --- |
| Root file size | 8 MiB (`MAX_ROOT_SOURCE_BYTES`) | Source Manager |
| Root AnalysisUnits per CLI invocation | 65,536 (`MAX_ANALYSIS_UNITS_PER_INVOCATION`) | Pipeline |
| Included file size | 4 MiB (`MAX_INCLUDED_SOURCE_BYTES`, future include phase) | Source Manager |
| Source files per AnalysisUnit | 256 (`MAX_SOURCE_FILES_PER_UNIT`) | Source Manager |
| Include depth | 64 | Preprocessor include stack |
| Macro expansion depth | 64 | Preprocessor expander |
| Conditional nesting depth | 64 | Preprocessor conditional stack |
| Expression nesting depth | 128 | Preprocessor expression parser |
| Expanded bytes/tokens per unit | 8 MiB / 1,000,000 tokens | Preprocessor output budget |
| Origin chain depth | 64 | Source Map |
| Preprocessor diagnostics | 1,000 | Preprocessor diagnostic sink |
| Total rendered diagnostics per unit | 10,000 | Unified analysis result |
| Total diagnostics | existing behavior; cap in Phase 3 | Unified layer |
| AST recursion depth | 256 recursive constructs (explicit AST001 guard) | AST parser |
| Control-flow analysis | existing legacy caps (tokens/nodes/edges/depth) | AST semantic analyzer |
| Compare fingerprints per engine per unit | 100,000 (`MAX_COMPARE_FINGERPRINTS_PER_ENGINE`) | Engine comparator |
| Compare detail samples | 64 per rule per direction (`MAX_COMPARE_DETAILS_PER_RULE`) | Engine comparator |

Requirements:

- Exceeding a limit produces a single, specific `PRE`/`AST` diagnostic and stops
  the affected expansion; the rest of the unit continues.
- Compare totals stay exact under both compare limits. Exceeding the
  100,000-fingerprint per-engine budget sets `comparison_complete = false` and
  forces `matched = false`, while per-rule counts and missing/extra totals remain
  exact. Exceeding the 64-sample detail cap (more than 64 missing/extra
  fingerprints in one rule/direction) sets `details_truncated = true`; that is a
  display bound, not an incomplete comparison.
- Limits are checked before allocation where possible.
- `AnalysisUnitId` and `RunId` assignment uses checked arithmetic. Exceeding
  the unit/file-index/RunId capacity is an input error (exit 2), never a wrap,
  re-use, or `panic`. `RunId` is process-local and never emitted.
- Phase 1 tests cover root size boundary/+1, source count capacity, and unit
  count arithmetic. Phase 2 tests cover conditional depth 64/65, macro depth
  64/65, expansion growth, and the diagnostic cap; include limits remain Phase 3.
- No limit may depend on machine memory size.

---

## 13. Testing strategy (M)

### Layers

1. **Unit tests** co-located with modules (`#[cfg(test)] mod tests`), covering
   each public function and each invariant (spans, UTF-8 boundaries, segment
   ordering, chain construction, directive scanning edge cases).
2. **Contract tests** for the source map: every prepared range resolves to a
   real file and a valid offset; identity mapping round-trips byte offsets; line
   and column match the lexer's semantics, including CRLF and multi-byte
   characters; `PreparedSpan`/`SourceSpan` separation is enforced by the type
   system. Mandatory cases: empty source file, empty prepared buffer,
   `Eof`/`PAR013` resolution, zero-length spans, points exactly on segment
   boundaries, and spans crossing several segments.
2a. **Span bridge tests**: `PreparedSource::prepared_span` is the only bridge.
   It accepts every lexer span in a valid prepared buffer, preserves byte
   offsets, stamps the source's `AnalysisUnitId`, and rejects inverted ranges,
   ranges past `prepared_len`, and offsets inside a multi-byte UTF-8 scalar with
   the documented `SpanError`. `Span.line`/`column` are ignored. There is no
   public `PreparedSpan` constructor and no `From`/`Into` impl (confirmed by
   review and, where practical, a compile-fail fixture without adding a
   dependency).
2b. **Invalid-coordinate tests**: the §7.3.1 matrix is exercised directly:
   inverted prepared/original ranges, out-of-bounds prepared/original ranges,
   non-boundary offsets, foreign unit/source, unknown `SourceId`, non-point
   input to `resolve_point`, unmapped prepared ranges, and deliberately
   corrupted segment tables. Every case returns the expected `SpanError`, never
   panics, and never returns the EOF anchor. Valid EOF, zero-length, and
   empty-buffer cases return `Ok`.
2c. **Unit-identity isolation suite (Phase 1, mandatory; TEST A–Q)**:
   - **TEST A:** two AnalysisUnits whose roots both have local file index 0.
     Passing a `SourceSpan` from A to B's `SourceMap::location`/`render_span`
     returns `ForeignAnalysisUnit`; no B coordinate is produced.
   - **TEST B:** two `PreparedSource`s with equal text length. Passing a
     `PreparedSpan` from A to B's `resolve_span` returns
     `ForeignPreparedSource`; equal `start`/`end` is not accepted.
   - **TEST C:** A's `eof_anchor()` passed to B's `location` returns
     `ForeignAnalysisUnit`; A's `eof_span()` passed to B's `resolve_point`
     returns `ForeignPreparedSource`.
   - **TEST D:** an invalid range in a debug build returns `SpanError` and does
     not panic.
   - **TEST E:** the same invalid-range tests in a release build produce
     identical `Err` variants and messages; no behavior depends on
     `debug_assertions`.
   - **TEST F:** valid EOF and zero-length spans resolve to `Ok` in both debug
     and release builds.
   - **TEST G:** a `RelatedLocation` whose `SourceSpan` belongs to another unit
     causes `RenderedDiagnostic::from_preprocess` to return
     `ForeignAnalysisUnit`; the diagnostic is not attributed to a local file.
   - **TEST H:** a `LegacyDiagnostic` tagged with unit B, passed with unit A's
     handle, returns `ForeignAnalysisUnit`; no A root is attached and the
     diagnostic is not rendered.
   - **TEST I:** the Legacy path has no `SourceId` parameter. A plain-LSL
     Legacy `AnalysisUnit` has no `PreparedSource`/`SourceMap`, and its
     `legacy_root()` is its root source, never an included file
     (`SourceFileKind::Included` cannot be obtained as a legacy root). A
     prepared unit has exactly one `PreparedSource`/`SourceMap` pair,
     `legacy_root() == None`, and `from_legacy` returns `NotLegacyUnit`.
   - **TEST J:** a valid Legacy conversion from unit A returns A's root and
     preserves `Diagnostic.span` byte-for-byte, including line, column,
     `end_line`, `end_column`, code, message, severity, and diagnostic order;
     baseline output is unchanged.
   - **TEST K:** an origin with a foreign `span` is rejected at insertion with
     `ForeignAnalysisUnit`; it is absent from the arena and no note or primary
     location is emitted from it.
   - **TEST L:** the same for a foreign `invocation`.
   - **TEST M:** the same for a foreign `definition`.
   - **TEST N:** a foreign span reachable only through a nested `parent` chain
     (corrupted arena, test-only) causes traversal (`origin`, `resolve_span`) to
     return `ForeignAnalysisUnit`; no foreign location is emitted as a local
     primary or note, and no local file is substituted.
   - **TEST O:** two independently active analysis invocations in one process,
     both with unit index 0. A coordinate from invocation 1 passed to
     invocation 2's map returns `ForeignAnalysisUnit`/`ForeignPreparedSource`
     because `RunId` differs; equal offsets and indexes are not accepted.
   - **TEST P:** start two analysis invocations on separate threads and release
     them from a shared `Barrier` (no sleeps or timing assumptions). Each
     concurrently allocates its `RunId`, creates a unit with index 0 and
     same-length source/prepared buffers, and returns its map and coordinates.
     Assert that the `RunId`s differ and that each map rejects the other run's
     `SourceSpan` and `PreparedSpan` with the corresponding foreign-identity
     errors. Repeat enough concurrent allocations to exercise the allocator
     under contention; all IDs must be unique. No wrap, reuse, panic, or
     allocator failure may be hidden by the test.
   - **TEST Q:** use isolated `RunIdAllocator` instances so global test state is
     not corrupted. Set one counter to `u64::MAX` and verify `allocate()` returns
     `CounterExhausted` repeatedly without wrapping or issuing an ID. Poison a
     separate test allocator's mutex from a worker thread, join that worker,
     and verify the next allocation returns `AllocatorPoisoned` without panic.
     These failures must not modify or poison the process-wide allocator.
   These tests run as `cargo test` **and** `cargo test --release` in Phase 1 CI;
   assertions must hold identically in both profiles.
3. **AnalysisUnit isolation tests**: two roots in one CLI invocation must not
   share macros, conditional-stack state, include guards, `SourceId` namespaces,
   `PreparedSpan` identities, or diagnostics; changing one file's `#define`
   must not change the other file's output; order of units must not change
   results. Cross-unit coordinates must fail with the identity errors above.
4. **Preprocessor tests** (Phases 2–3): one fixture per directive and per macro
   form, plus malformed and adversarial inputs (unterminated conditionals,
   recursive macros, paste bombs, include cycles, missing includes, path
   traversal). The `//fspreprocessor off` fixture matrix from §10.2 is required,
   including marker occurrences inside strings and block comments. The six
   `PreprocessDiagnostic` scenarios in §7.6.1 are mandatory: each test asserts
   the exact primary `SourceSpan` (file, start, end) and the related locations,
   and verifies that the primary is **not** produced by `resolve_span`.
5. **`#pragma once` tests** are a separate suite from verified Firestorm
   compatibility tests (§10.3). They may assert neolint behavior, but the test
   names and documentation must not claim Firestorm parity.
6. **Integration tests** via the public API and the binary
   (`env!("CARGO_BIN_EXE_neolint")`), in the existing `tests/` style, using
   temporary directories. Integration coverage asserts unchanged plain-Legacy
   output, identity-prepared `ast`/`compare` without preprocessing, full
   preprocessing for every engine, and stderr Compare report separation and
   unit order. There is no cross-engine deduplication merge step: Compare
   displays only the Legacy stream.
7. **Golden corpus and pre-change CLI oracle**: existing
   `corpus/{small,medium,large,invalid}` plus a new preprocessor corpus.
   Plain-LSL golden results must not change. `tests/legacy_golden.rs` holds six
   byte-exact stdout/stderr fixtures captured from the unmodified base
   `4dd01ca16539e6944a6714f133e9d2636adb6a95`; both the default invocation and
   `--engine legacy` must reproduce them. That pre-change golden baseline is the
   compatibility oracle for CLI wiring. Compare detail truncation (the bounded
   64-sample display) is a distinct state from an incomplete comparison (the
   100,000-fingerprint budget); tests must assert both independently.
8. **Differential Legacy↔AST harness** (Phase 4+): compare exact 10-field
   fingerprints as a multiset of `(code, severity, kind, message, start, end,
   line, column, end_line, end_column)`, plus a per-code report. The harness
   calls the public `parse_legacy`/`parse_ast` and both semantic entry points
   independently; neither engine is a wrapper around the other. Order is
   compared only through the documented deterministic policy.
9. **Fallback/fuzz-style tests without dependencies**: deterministic generators
   over token/directive alphabets, bounded by iteration counts, run in normal
   `cargo test`.

### Requirements

- Every new diagnostic code has at least one positive and one boundary test.
- Every phase adds tests for its resource limits and malformed-input behavior.
- Phase 1 implements the TEST A–Q identity suite and runs it under both
  `cargo test` and `cargo test --release`; results are identical, with no panic
  in either profile.
- Phase 1 also covers root size boundary/+1, source count capacity, deterministic
  ids, line-index/lexer parity, identity round-trip, EOF/zero-length, origin
  corruption, Legacy ownership, and concurrent `RunId` allocation.
- No test uses the network, sleeps, or depends on host locale/time zone.
- Tests must pass on MSRV 1.75; the experimental workflow installs Rust 1.75.0
  and runs `cargo +1.75.0 test --all-targets --all-features`.
- CI: `experimental-ci.yml` runs on pushes to `exp/preprocessor-ast` and to flat
  `exp-preprocessor-ast-phase-*` branches, and on PRs targeting
  `exp/preprocessor-ast`. It has no `workflow_dispatch`, because the file is not
  on the default branch and manual dispatch is not an active trigger in this
  isolation model. `ci.yml` runs only for `main` pushes and PRs targeting
  `main`.

---

## 14. Legacy → AST migration strategy (N)

Migration is incremental and evidence-driven. The two parsers are separate
implementations, not stages of the same code.

1. **Parser independence.** `parse_legacy` and `parse_ast` are both public and
   independently callable. `parse_ast` must not be implemented by calling
   `parse_legacy` for its full result, and `parse_legacy` must not depend on AST
   types. Shared grammar helpers are allowed only when they contain no
   engine-specific diagnostic or recovery policy.
2. **Rule ownership matrix.** For every `SEMxxx` (and `PARxxx` where the AST
   parser owns recovery), record: legacy owner, AST owner, parity status,
   known differences, and the test that proves parity. The matrix lives next to
   the AST semantic code and is updated in the same commit as behavior changes.
3. **Shadow comparison (implemented as Compare mode).** Both engines run from
   the same single-lex prepared token slice, and their raw semantic streams are
   compared as exact multisets. The report is emitted on stderr only; the
   displayed stream is the Legacy stream. Differences are a development artifact,
   not merged into user findings, and an unmatched report exits 3.
4. **Parser parity criteria.** Before an AST parser change may replace legacy
   parsing for a construct, all of the following must hold on the full corpus
   (`small`, `medium`, `large`, `invalid`) and the grammar inventory tests:
   - accept/reject decisions match;
   - `(code, severity, normalized span)` multisets match, or every difference is
     a documented, tested fix;
   - `Eof`/empty-input diagnostics (`PAR013` and incomplete-construct cases)
     match;
   - recovery produces the same number of independent findings per malformed
     construct;
   - no new panic on malformed or deeply nested input.
   The differential report is attached to the change. Full rule-by-rule parity
   is the Phase 6 gate; construct-level parser parity is the Phase 4 gate.
5. **Parity thresholds for rules.** A rule may move to the AST engine only when:
   - its corpus and unit-test results match legacy exactly, or
   - every difference is documented as an intentional fix and covered by a test.
6. **Ordering policy.** The displayed Legacy stream is not globally sorted; it
   keeps the existing per-unit, per-source, pass order. Only the Compare report
   is deterministically ordered. Rule migration therefore does not reshuffle
   user output, and Legacy-only mode keeps the current pass order.
7. **No duplicate findings.** Compare has no cross-engine deduplication merge
   step because it displays the Legacy stream only; the AST semantic stream is
   never appended to stdout. Legacy and AST modes each display exactly one
   engine's stream.
8. **Feature gating (status: implemented).** Engine selection is available
   through `--engine` and the `engine` configuration key in the experimental
   branch. The default remains `legacy` until a separate Phase 6 stabilization
   decision; `ast` and `compare` are opt-in.
9. **Rollback (status).** The legacy parser and semantic analyzer remain
   compiled, public, and selectable (`--engine legacy` or `engine = legacy`);
   removal requires a separate decision and a major-version boundary. Legacy
   differential testing remains possible.
10. **Documentation.** Every behavioral difference introduced by AST rules is
    added to `docs/diagnostics.md` with an example and a boundary note.

---

## 15. Phase plan (O)

### Phase 0 — Experimental branch, CI, architecture contracts

- **Goal:** create a safe, isolated development contour and freeze contracts.
- **Deliverables:** `exp/preprocessor-ast` branch; `experimental-ci.yml`;
  `main`-scoped filters in `ci.yml`; this document.
- **Dependencies:** baseline commit only.
- **Acceptance criteria:**
  - branch starts exactly at the baseline;
  - `main` is untouched;
  - experimental CI triggers for the integration branch, flat
    `exp-preprocessor-ast-phase-*` branches, and PRs targeting the integration
    branch;
  - stable CI triggers only for `main` pushes and PRs;
  - full existing test suite, fmt, clippy, and rustdoc pass;
  - no runtime/source behavior changes outside CI and docs.
- **Tests:** the existing 350 tests; workflow validation on push and on a PR.
- **Done when:** the branch is pushed, both workflows show the expected
  triggers, and the report records SHAs and run URLs.

#### Phase 0.1 — Contract corrections (completed by this revision)

- **Goal:** remove ambiguities and factual errors from the Phase 0 contracts
  before any runtime component exists.
- **Corrections:**
  - split `PreparedSpan` (prepared buffer only) from `SourceSpan` (original
    files only); the prepared buffer is no longer a `SourceFile` and has no
    `SourceId`;
  - define EOF, empty-buffer, zero-length, boundary, and cross-segment
    resolution, including `PAR013` on an empty prepared buffer;
  - introduce the AnalysisUnit and define multi-root CLI behavior, isolation,
    optional shared immutable file cache, stdin, and result merging;
  - make `parse_legacy` and `parse_ast` explicit, independent public entry
    points, with `parse` frozen as an alias of the legacy behavior;
  - correct `//fspreprocessor off` to Firestorm's global, raw-substring
    semantics and move it to Phase 3;
  - classify `#pragma once` as a compatible neolint convenience, separate from
    verified Firestorm behavior;
  - replace the impossible `exp/preprocessor-ast/**` CI trigger with the flat
    `exp-preprocessor-ast-phase-*` pattern and remove `workflow_dispatch`.
- **Dependencies:** Phase 0.
- **Acceptance criteria:** all ten criteria in the Phase 0.1 task statement,
  no runtime code changes, and the full CI gate passes.

#### Phase 0.2 — Span bridge and diagnostic contract closure (this revision)

- **Goal:** close the remaining source-location contract gaps before Phase 1.
- **Corrections:**
  - define the explicit, checked `Span -> PreparedSpan` bridge and the
    `PreparedSpan -> SourceSpan` resolution path, with no implicit conversions;
  - define `PreprocessDiagnostic` (original coordinates), `PreparedDiagnostic`
    (prepared coordinates), `UnifiedDiagnosticInput`, `RenderedDiagnostic`, and
    `SpanError`;
  - specify invalid-coordinate behavior (`start > end`, out-of-bounds,
    non-UTF-8 boundaries, unknown `SourceId`) as explicit `Result` errors with no
    panic and no EOF fallback;
  - add worked examples for directive, `#if`, conditional, include, recursive
    macro, and EOF-after-removal diagnostics.
- **Acceptance criteria:** all criteria in the Phase 0.2 task statement, no
  runtime code changes, full CI gate passes.

#### Phase 0.3 — Coordinate namespace isolation and panic-free contract (this revision)

- **Goal:** make cross-unit/cross-source coordinate mixing impossible to accept
  silently and remove assertions from the malformed-coordinate path.
- **Corrections:**
  - add `AnalysisUnitId` and make `SourceId` contain `{ unit, file }`;
  - stamp `PreparedSpan` with the owning unit and reject foreign spans in
    `resolve_span`/`resolve_point` with `ForeignPreparedSource`;
  - reject foreign `SourceSpan`/`RelatedLocation`/EOF anchors with
    `ForeignAnalysisUnit`;
  - require point resolution to take a zero-length `PreparedSpan`, add
    `PreparedSource::eof_span`, and return `NotAPoint` otherwise;
  - reduce the `Span` bridge to the single public
    `PreparedSource::prepared_span` adapter and document that it assigns
    identity but cannot recover provenance from `Span`;
  - remove `debug_assert!` from the malformed-coordinate path; behavior is
    identical in debug and release and always `Result::Err`.
- **Review follow-up (same phase):**
  - make `RunId` allocation process-wide and concurrency-safe with a mutex,
    checked increment, and explicit poisoned/exhausted errors; add barrier-based
    concurrent allocation coverage and isolated failure-path tests (TEST P/Q);
  - make the Legacy path unit-tagged: `LegacyDiagnostic` is created only by the
    producing unit, and `from_legacy` takes an `AnalysisUnit` handle, obtains the
    root through `legacy_root()`, and never accepts a caller-supplied
    `SourceId`;
  - validate every `SourceOrigin` span at arena insertion and on traversal, with
    map-local `OriginId`s and explicit errors for foreign, unknown, or cyclic
    chains;
  - add `RunId` to `AnalysisUnitId` so independently active invocations in one
    process cannot exchange coordinates that happen to share unit index and
    offsets.
- **Acceptance criteria:** all criteria in the Phase 0.3 task statement, TEST A–Q
  recorded in the Phase 1 plan, no runtime code changes, full CI gate passes.

### Phase 1 — Source Manager, Source Map, Identity Preprocessor

- **Status:** implemented on `exp-preprocessor-ast-phase-01-source-foundation`
  (documentation-only Phase 0 contracts are merged into the integration branch).
- **Goal:** make multi-file provenance and line resolution real without changing
  analysis.
- **Functionality:**
  - `RunId`, `AnalysisUnitId { run, index }`, `SourceId { unit, file }`,
    `SourceFile`, `PreparedSpan` (unit-stamped), `SourceSpan`,
    `SourceLocation`, checked/private `SourceOrigin`, map-local `OriginId`,
    `SourceMap` with `Result`-based `resolve_span`, `resolve_point` (point span
    input), `location`, `render_span`, `source_span`, `eof_anchor`, `unit`, and
    `origin`; `SpanError` with the §7.3.1 variants, including
    `ForeignAnalysisUnit`, `ForeignPreparedSource`, `NotLegacyUnit`,
    `NotAPoint`, and `UnknownOriginId`;
  - `PreparedSource::prepared_span` as the **only** public `Span ->
    PreparedSpan` adapter, plus `PreparedSource::eof_span`; no public
    `PreparedSpan` constructor;
  - additive diagnostic types (`LegacyDiagnostic`, `PreprocessDiagnostic`,
    `PreparedDiagnostic`, `UnifiedDiagnosticInput`, `RenderedDiagnostic`) and
    their resolution rules, even though real `PRE` findings start in Phase 2;
  - `AnalysisUnit` opaque handle with a `RunId`-scoped `AnalysisUnitId`,
    `legacy_root()` for plain-LSL no-map units, a unit-scoped `SourceId`
    namespace, diagnostics and preprocessor state placeholders; prepared units
    own exactly one `PreparedSource`/`SourceMap` pair, while Legacy units own
    neither;
  - origin insertion/traversal ownership checks in the `SourceMap` builder, with
    no public `SourceOrigin` constructor;
  - per-file line index with lexer-compatible columns;
  - Source Manager with root/stdin loading and deterministic per-unit IDs;
  - identity preprocessor producing a byte-identical prepared buffer, a single
    identity segment, and an EOF anchor;
  - `PreparedSource` and the unified input/rendered diagnostic API (no CLI
    change yet).
- **Modules:** `src/source/{mod.rs,file.rs,line_index.rs,map.rs,manager.rs,unit.rs}`,
  `src/preprocessor/{mod.rs,identity.rs,config.rs,diagnostics.rs}`,
  `src/diagnostic/resolve.rs`, `src/pipeline.rs` (unit orchestration).
- **Dependencies:** Phase 0/0.1/0.2/0.3 contracts.
- **Acceptance criteria:**
  - plain-LSL Legacy units contain no `PreparedSource` or `SourceMap`; prepared
    units contain exactly one matching pair, and the two modes cannot be mixed;
  - identity prepared text equals input bytes for all fixtures;
  - map covers `[0, prepared_len)`, segments sorted and non-overlapping;
  - `resolve_span`/`resolve_point` return `Ok` for every well-formed range of
    their own unit, including empty buffers, EOF (`prepared_len`), zero-length
    ranges, boundary offsets, and cross-segment ranges; malformed ranges return
    the documented `SpanError` and never fall back to the EOF anchor;
  - a `SourceSpan` from another unit returns `ForeignAnalysisUnit`; a
    `PreparedSpan` from another unit/source returns `ForeignPreparedSource`;
    equal local file indexes or equal offsets never permit acceptance;
  - the single `PreparedSource::prepared_span` bridge rejects inverted,
    out-of-bounds, and non-UTF-8-boundary spans, stamps the unit, and is the
    only public constructor path for `PreparedSpan`;
  - the full TEST A–Q suite from §13 passes identically under `cargo test` and
    `cargo test --release`; no resolution-path assertion or panic exists in
    either profile;
  - `UnifiedDiagnosticInput` accepts a synthetic `PreprocessDiagnostic` with an
    original `SourceSpan`, a `PreparedDiagnostic` bridged through the map, and a
    `Legacy(LegacyDiagnostic)` verified against the target unit and rendered
    against its root;
  - `from_legacy` cannot be given a `SourceId`: a foreign tag returns
    `ForeignAnalysisUnit`, a non-legacy/preprocessed unit returns
    `NotLegacyUnit`, and a valid conversion preserves `Diagnostic.span` and
    baseline output exactly;
  - every `SourceOrigin` field is unit-validated at insertion; foreign
    `span`/`invocation`/`definition`, foreign/unknown `OriginId`, and a foreign
    span reached through a nested parent chain return `SpanError` and never emit
    a local location;
  - `PAR013` on an empty source resolves to root `(0, 0)`, line 1, column 1;
  - a `PreprocessDiagnostic` never calls `resolve_span`; its primary is the
    original `SourceSpan` (verified with the §7.6.1 example shapes);
  - every offset resolves to the same line/column as the current lexer for
    ASCII, UTF-8, LF, lone CR, CRLF, and empty files;
  - creating two AnalysisUnits in one run assigns different
    `AnalysisUnitId`s, keeps their `SourceId`/`PreparedSpan` identities and
    diagnostics isolated, and rejects every cross-unit coordinate;
  - two independent runs in one process get different `RunId`s, so a coordinate
    from one run is rejected by the other's map even at equal unit index and
    offsets; concurrent allocation is serialized and verified with a barrier;
  - `RunId` counter exhaustion and mutex poisoning return explicit setup errors,
    never panic, wrap, or reuse an identifier;
  - no public legacy API change; `parse_legacy` exists as a thin explicit
    wrapper while `parse` keeps its exact behavior;
  - no new dependencies; MSRV 1.75.
- **Tests:** unit tests for map construction/lookup, identity round-trip,
  UTF-8/CRLF columns, empty source, empty buffer, EOF/PAR013, zero-length and
  cross-segment spans, boundary points, multi-unit/multi-run isolation,
  origin-arena corruption, Legacy tag/root ownership, concurrent RunId
  allocation, TEST A–Q, the full
  malformed-coordinate matrix from §7.3.1, the single `Span` adapter, and mixed
  `UnifiedDiagnosticInput` handling. Run in both debug and release profiles;
  must not panic in either.
- **Done when:** the full suite passes and identity/EOF/unit-isolation paths are
  covered by contract tests.

#### Phase 1 implemented surface

- Modules: `src/source/{mod,file,line_index,map,manager,unit}.rs`,
  `src/preprocessor/{mod,identity,config,diagnostics}.rs`,
  `src/diagnostic/resolve.rs`, `src/pipeline.rs`; `src/lib.rs` re-exports the
  additive API. `src/lexer.rs`, `src/parser.rs`, `src/semantic.rs`, and
  `src/diagnostic.rs`'s frozen types are unchanged except for the additive
  `pub mod resolve;` declaration and `parse_legacy`/`parse` wiring.
- Identity: `RunIdAllocator` uses `Mutex<u64>` and cannot panic, wrap, or reuse;
  `AnalysisRun::new()` allocates before creating units; unit indexes are
  deterministic `0..N-1`.
- Sources: `SourceManager` loads one root from a regular file or supplied
  reader/text, preserves display spelling, stores an optional canonical path,
  bounds reads to 8 MiB, rejects invalid UTF-8, non-regular roots, and remote
  UNC/URL spellings.
- Lines: `LineIndex` matches lexer semantics for LF, lone CR, CRLF, UTF-8
  columns, EOF, and trailing newlines; it rejects offsets inside a UTF-8 scalar.
- Maps: `SourceMapBuilder` validates files, origins, segments, EOF anchor, and
  origin chains; `SourceMap` exposes `resolve_span`, `resolve_point`,
  `location`, `render_span`, `source_span`, `eof_anchor`, `origin`, and `unit`.
  The identity map has one `Direct` segment (non-empty) or no segment (empty)
  plus a real root EOF anchor.
- Diagnostics: `LegacyDiagnostic` is pipeline-tagged; `from_legacy` takes only
  `&AnalysisUnit` and uses `legacy_root()`. `PreprocessDiagnostic` is converted
  directly; `PreparedDiagnostic` is bridged and resolved. `RenderedDiagnostic`
  private fields and constructors enforce ownership.
- Library orchestration: `AnalysisRun::new/add_legacy_file/add_legacy_text/
  add_legacy_reader/add_prepared_file/add_prepared_text/add_prepared_reader/
  analyze` and `AnalysisUnit::analyze` run the existing lexer, `parse_legacy`,
  and semantic analyzer and return rendered diagnostics. Legacy units bypass the
  map; prepared units bridge and resolve. The CLI does not call these entry
  points in Phase 1, so existing CLI output is unchanged.
- `SourceMapBuilder` rejects zero-length segments, holes, overlaps, foreign
  spans/ids, missing parents, chains deeper than 64, and non-zero-length EOF
  anchors. `SourceMap` revalidates origin chains during traversal.
- Errors: `SpanError` is the stable internal resolution error. Malformed or
  foreign coordinates return `Err`, are never silently dropped, and are never
  replaced by the EOF anchor or a local file.
- CI: debug tests, release tests, rustdoc, Clippy, Python metadata tests, and an
  explicit `cargo +1.75.0 test --all-targets --all-features` MSRV step.
- Deferred: directives, macros, includes, Firestorm switches, Lazy Lists, AST,
  AST semantics, and CLI behavior. `SourceFileKind::Included` exists but nothing
  loads it in Phase 1.

#### Phase 1 review fixes

- Resolution walks the complete parent chain: user-visible origins map by
  relative offsets only when segment and origin lengths match; `Generated`
  walks to the nearest user origin; `TokenPaste`/`Stringify` prefer invocation.
  Ancestor spans and all `invocation`/`definition` locations become related.
- `RenderedDiagnostic::from_prepared` carries `ResolvedSpan.related()` plus the
  related results of every explicit prepared related range, validates them,
  deduplicates, and excludes the primary.
- `from_prepared` and `from_preprocess` take no map argument; both use the
  `PreparedSource`'s map and verify unit, prepared length, and prepared text, so
  same-unit foreign maps cannot be supplied. `legacy_diagnostic` is
  crate-internal; the unused empty `AnalysisUnit::diagnostics()` accessor was
  removed.

### Phase 2 — Preprocessor core (implemented)

- **Goal:** C-like conditional compilation and object/function macros producing
  mapped prepared text.
- **Functionality:** `#define`, `#undef`, `#if`, `#ifdef`, `#ifndef`, `#elif`,
  `#else`, `#endif`; object-like and function-like macros; macro invocation
  with arguments; integer expression evaluator with defined precedence and
  overflow behavior; `PreprocessDiagnostic` for every `PRE` finding with direct
  original `SourceSpan`s of the same unit and `RelatedLocation`s of the same
  unit; expansion limits; source map segments for invocations, definitions, and
  arguments.
- **Modules:** `src/preprocessor/{directive.rs,macros.rs,expr.rs,expand.rs,
  diagnostics.rs}`.
- **Dependencies:** Phase 1 (source map, prepared source).
- **Acceptance criteria:**
  - directive semantics match the documented subset (including `defined`,
    nesting, and `#elif` after a taken branch);
  - malformed directives recover and continue;
  - recursion and size bombs stop at limits with one diagnostic;
  - every generated token resolves to a primary origin;
  - every `PRE` diagnostic has a valid original `SourceSpan` and correct related
    locations for the §7.6.1 cases (unknown directive, bad `#if`, unterminated
    conditional, recursive macro);
  - plain LSL without directives is byte-identical (identity behavior preserved).
- **Tests:** per-directive fixtures, expression table tests, macro expansion
  fixtures with expected origin chains, malformed/adversarial inputs, limits,
  and exact `PreprocessDiagnostic` primary/related assertions.
- **Done when:** directive and macro suites pass, map invariants hold, and the
  legacy suite is unchanged.

#### Phase 2 implementation record

**Status:** implemented in `src/preprocessor/{config,directive,macros,expand,
expr,token,diagnostics}.rs`. Phase 1 types and the identity pass-through are
unchanged.

**Entry point.** `preprocess(manager, root, options)` dispatches on the compact
`Copy` `PreprocessOptions::mode()`. `PreprocessConfig` remains a unit value and
converts to identity options, so existing callers compile unchanged;
`PreprocessConfig::full()` and `PreprocessOptions::full()` opt into Phase 2.
`Identity` calls `preprocess_identity` unchanged and `Default` remains identity,
so plain-LSL callers do not opt in accidentally. Phase 3 extensions are carried
by `PreprocessSettings { options, dialect, include_dirs }`; the read-only
`preprocess_with_settings` and mutable `preprocess_with_includes` entry points
accept it. `AnalysisRun::add_prepared_*` still creates identity units; the
Phase 3 `add_preprocessed_*` methods use the extended settings.

**Scanner and directives.** The scanner works on original bytes, not regular
expressions or the LSL lexer. It tracks strings, `//` comments, `/* ... */`
comments, physical line starts, whitespace, and backslash-newline splicing.
Line splicing applies to ordinary active code as well as directive text: the
backslash-newline bytes are removed before tokenization, so a `#` later on the
same logical line is never a directive. Directives are recognized only when `#`
is the first token of a logical line after whitespace/comments; a `#` inside a
string, comment, or ordinary code is not a directive. LF, CRLF, and lone CR are
preserved (except the spliced newline). A directive consumes its logical line;
one original line terminator is retained as direct output so neighbouring
tokens are not glued together. Active ordinary code is accumulated across
physical lines and tokenized as one stream, so function-like calls, arguments,
comments, and strings may span lines. The conditional stack stores
parent-active, current-active, branch-taken, `#else`-seen, and opening span, and
handles nested inactive blocks structurally. `#define`/`#undef` in inactive
blocks do not change the table. `#elif` after a taken branch or after `#else`
is not evaluated. Unknown directives and Phase 3 directives (`#include`,
`#pragma`) are diagnosed rather than silently accepted.

**Macros.** Object-like and function-like macros are supported. A function-like
definition requires `(` immediately after the name; `#define F (x)` is
object-like. Calls allow whitespace/comments between the name and `(`. Argument
parsing is parenthesis-depth aware, preserves nested commas, trims argument-edge
whitespace/comments, pre-expands arguments, substitutes parameters, and rescans
the replacement. Identical redefinitions are accepted; conflicting ones keep the
old definition and emit `PRE005`. `#undef` of an absent name is a no-op.
Recursive expansions stop at the offending invocation, emit one diagnostic per
invocation site, and leave the invocation text in place while scanning continues.

**Expression grammar.** `defined IDENT` and `defined(IDENT)` are resolved before
macro expansion; the operand is not macro-expanded. Integers are decimal,
leading-zero octal, or `0x` hexadecimal. The fixed model is signed 64-bit.
Undefined identifiers evaluate to zero. Precedence, from lowest to highest:

```text
?:  ||  &&  |  ^  &  == !=  < <= > >=  << >>  + -  * / %  unary + - ! ~  ()
```

Binary operators are left-associative; `?:` is right-associative. `&&`, `||`,
and `?:` short-circuit and do not evaluate unselected operands. Arithmetic,
unary negation, shifts, and integer literals use checked `i64` operations;
division/remainder by zero, `INT64_MIN / -1`, out-of-range shifts, and overflow
are diagnostics. No wrapping arithmetic or profile-dependent behavior is used.

**Source map provenance.** Every prepared byte belongs to a segment. Unchanged
active text is `Direct`. Replacement tokens are `MacroDefinition` origins whose
span is the definition fragment and whose invocation/definition fields point to
the call and `#define`. Argument text is `MacroArgument` with the exact argument
span, call, and definition. Nested calls link through the parent chain rather
than replacing it with one final span. Inserted separators are `Generated`
origins anchored at a real source point. The driver emits output and provisional
provenance first, then materializes origins parent-first and appends segments in
prepared order. Empty output has no segments and an EOF anchor at the end of the
last consumed directive, or at offset zero when there is none; non-empty output
anchors at the root end.

**Recovery and limits.** Malformed directives recover at the next logical line;
malformed/unselected conditional expressions evaluate to false without
suppressing structural errors. The engine enforces macro depth 64, conditional
depth 64, 8 MiB/1,000,000 expanded bytes/tokens, expression depth 128, and
1,000 PRE diagnostics. Exceeding a limit stops the affected expansion, emits one
specific diagnostic, and continues safely. Macro table and conditional stack are
per-run, iteration uses deterministic containers, and no network/clock/random
state is consulted.

**Tests.** Unit modules cover tokenization/continuations, every directive,
conditional errors and inactive blocks, object/function macros, zero/multiple
arguments, nested parentheses and calls, pre-expansion, recursion, redefinition,
`#undef`, expression precedence/associativity/short-circuit/overflow/shifts/
malformed input, provenance and every-byte map coverage, UTF-8/CRLF, depth and
diagnostic limits, and exact diagnostic primary/related locations. The full
Phase 1 suite remains unchanged.

### Phase 3 — Includes, advanced macros, Firestorm built-ins, CLI integration (implemented)

- **Goal:** complete the preprocessor and connect it to the legacy pipeline
  behind an explicit opt-in.
- **Functionality:** nested includes, classic include guards, `#` and `##`,
  variadic macros, the global `//fspreprocessor off` kill switch (§10.2),
  predefined macros/built-ins (documented only), include graph and cycle
  detection, CLI/config options, unified rendering of all three
  `UnifiedDiagnosticInput` variants, exit-code policy, JSON note extension if
  approved.
- **Conditional compatibility addition:** `#pragma once` is implemented as a
  neolint convenience classified separately from verified Firestorm behavior
  (§10.3), with its own tests.
- **Modules:** `src/preprocessor/{include.rs,builtins.rs}`, `src/pipeline.rs`
  (or equivalent orchestration), `src/main.rs`, `src/config.rs`,
  `src/diagnostic.rs` (only if an additive note type is approved).
- **Dependencies:** Phase 2.
- **Acceptance criteria:**
  - include cycles, missing files, remote paths, and depth limits produce
    `PreprocessDiagnostic`s without panics; a missing include's primary location
    is the path spelling in the including file, with the directive as related;
  - preprocessor findings bypass `resolve_span`; engine findings are bridged and
    resolved; plain-LSL findings pass through unchanged;
  - every rendered diagnostic and note belongs to one unit; cross-unit spans
    are rejected by the identity checks, never rendered against a local file;
  - the same include included twice behaves per documented guard rules;
  - `#pragma once` behavior is covered by its own suite and is not presented as
    verified Firestorm compatibility;
  - the `//fspreprocessor off` fixture matrix passes, including marker
    occurrences inside strings and block comments (whole file disabled);
  - `#`/`##` origins never claim a literal user file range;
  - plain LSL mode output is byte-identical to baseline;
  - enabling the preprocessor without directives yields the same diagnostics as
    legacy;
  - CLI/config errors still exit 2; preprocessing findings exit 1 through the
    normal severity path.
- **Tests:** include-tree fixtures in temporary directories, cycle/limit tests,
  macro operator tests, the six marker fixtures from §10.2, the separate
  `#pragma once` suite, CLI integration tests, mixed-input rendering tests
  (preprocess + prepared + legacy), source-map rendering tests, golden plain-LSL
  comparison.
- **Done when:** the preprocessor is usable end-to-end behind a flag, documented,
  and covered by integration tests.

#### Phase 3 implementation record

**Status:** implemented in `src/preprocessor/{include,builtins,macros,expand,
directive}.rs`, `src/source/{manager,file,unit}.rs`, `src/pipeline.rs`,
`src/diagnostic/resolve.rs`, `src/config.rs`, and `src/main.rs`.

**Includes.** `SourceManager::load_included_file` performs bounded 4 MiB reads,
regular-file/UTF-8 validation, canonical-path de-duplication, source-count checks,
and deterministic `SourceId` registration. A file is loaded once and reused for
repeated textual expansion. Quoted includes search the including file's display
directory first and then include directories in caller order; angle includes
search only include directories. Absolute local paths are accepted; URL, FTP,
UNC, and other remote spellings are rejected before filesystem access. Stdin has
no relative directory, so quoted includes from stdin require an explicit include
directory. Include cycles are detected by canonical path on the active stack;
depth 64 is allowed and 65 is rejected. Classic include guards use the shared
macro table. `#pragma once` is a per-unit canonical-path convenience; unknown
pragmas produce `PRE036`. The include graph records nodes in load order and
textual edges in expansion order. Included direct and macro origins point at the
included `SourceId`, include-site invocation, and parent include context.

**Advanced macros.** Function-like replacement lists support `#`, `##`, and C
variadic parameters ending in `...` with `__VA_ARGS__`. Stringification uses the
raw (unexpanded) argument, normalizes whitespace/comments, escapes `\` and `"`,
and preserves an empty argument as `""`. Token pasting uses raw operands,
supports chains and empty operands, joins only the adjacent boundary token of
each sequence while preserving the argument's remaining tokens, validates that
the result is exactly one preprocessing token, and diagnoses invalid pastes with
`PRE038`. Pasted and
stringified bytes use `OriginKind::TokenPaste`/`Stringify` with the invocation as
primary and definition/invocation/argument parent context as related. Variadic
fixed arguments are validated; `__VA_ARGS__` substitutes the remaining arguments
with generated commas. GNU comma-swallowing and `__VA_OPT__` are not implemented.

**Firestorm profile and built-ins.** In `Firestorm` dialect, a raw substring search
of the original root for `//fspreprocessor off` disables all preprocessing for
the whole script and returns the identity-prepared text plus one informational
`PRE037` diagnostic at the exact marker range. Occurrences in strings or block
comments count; standard LSL mode ignores the marker. `__FILE__` and `__LINE__`
are the only predefined macros. They are computed from the owning `SourceFile`
and `LineIndex`, including inside includes and macro replacement rescans; dynamic
values such as `__DATE__`, `__TIME__`, `__COUNTER__`, and `__TIMESTAMP__` remain
unresolved identifiers. Switch and lazy-list syntax remain Phase 5.

**CLI/diagnostics.** `--preprocess`, `--dialect`, and repeated `-I`/`--include-dir`
are supported; the config file accepts `preprocess`, `dialect`, and repeated
`include_dir`. The compact Phase 2 `PreprocessOptions` stays `Copy` and `const`;
Phase 3 options live in `PreprocessSettings`. CLI dialect/include settings override config. Plain invocation
without preprocessing flags keeps the frozen legacy pipeline. Prepared units use
one `PreparedSource`/`SourceMap`, convert preprocess diagnostics directly and
engine diagnostics through the map, group rendered diagnostics by source load
order, and preserve existing human/JSON schema and exit code semantics. A
per-unit total-diagnostic cap of 10,000 appends one terminal `PRE039`; the CLI
applies the same 10,000 limit across all `FileResult`s, truncates every excess
result and appends exactly one invocation-terminal `PRE039`.

**Tests.** Permanent suites cover quoted/angle/nested/duplicate includes, guards,
pragma once, cycles, depth 64/65, missing/malformed/remote paths, Unicode/spaces,
stdin policy, size/count limits, include graph order, included-file diagnostics
and every-byte map coverage, stringification, token pasting, variadics,
provenance, Firestorm kill-switch fixtures, built-ins, CLI/config flags and exit
codes, and unchanged plain-LSL behavior.

### Phase 4 — AST parser (implemented)

- **Goal:** build a typed AST over the same prepared token stream without
  changing diagnostic behavior.
- **Functionality:** AST types for declarations, functions, states, events,
  statements, expressions, types, and literals; spans preserved on every node;
  recovery equivalent to legacy; explicit recursion depth guard; public
  `parse_ast(tokens) -> (Ast, Vec<Diagnostic>)`.
- **Modules:** `src/ast/{mod.rs,node.rs,types.rs,parser.rs}`.
- **Dependencies:** Phases 1–3 (prepared stream and source map), with the legacy
  parser as the behavioral reference and the frozen `parse_legacy` entry point.
- **Acceptance criteria:**
  - `parse_ast` is independently callable and does not delegate its full result
    to `parse_legacy`; `parse_legacy`/`parse` remain unchanged and public;
  - accept/reject decisions match legacy on the valid and invalid corpus;
  - diagnostic codes and spans match legacy or differences are documented;
  - the parser parity criteria in §14.4 pass on the full corpus and grammar
    inventory;
  - malformed and deeply nested inputs never panic;
  - AST nodes retain prepared spans, and every node can be resolved through the
    source map.
- **Tests:** grammar inventory parity, corpus valid/invalid parity, recovery
  tests, depth/limit tests, AST shape tests for representative programs, and a
  differential harness that invokes both parsers directly.
- **Done when:** the AST parser parses the whole corpus with parity, the
  differential harness reports only documented deltas, and both parsers are
  independently runnable.

#### Phase 4 implementation record

**Status:** implemented in `src/ast/{mod,node,types,parser}.rs`; exported through
`neolint::ast` and `parse_ast`. The legacy parser is unchanged.

**Public API.** `parse_ast(tokens: &[Token]) -> (Ast, Vec<Diagnostic>)` is
independently callable on the same prepared stream as `parse_legacy`; it never
calls the legacy parser or semantic analyzer. `Ast` contains typed `TopLevel`
declarations (`Global`, `Function`, `State`, explicit `Error`), typed statements
(empty, block, local declaration, if/else, for, while, do/while, return, jump,
state change, label, expression, error), and typed expressions (identifier,
integer/float/string literals, parenthesized, list, vector, rotation, call,
component access, unary, binary, assignment, cast, prefix/postfix increment,
error). Every node carries a prepared `Span`; composite spans run from their
first through their last token. Operator enums preserve LSL's non-C precedence
table, including equal `&&`/`||` precedence and subtraction binding above
addition.

**Recovery.** The AST parser deliberately mirrors the legacy delimiter-aware
synchronization at top level, state/event, parameters, statement, condition,
`for` clause, and argument boundaries. Failed constructs become explicit
`ErrorNode`s rather than fabricated valid nodes. Event/function recovery keeps
later events, functions, and states; failed statements keep later independent
statements. Every loop either consumes a token or exits its recovery branch.

**Recursion guard.** `MAX_AST_RECURSION_DEPTH` is 256. Expression, prefix,
statement, and block recursion is counted before entering the next recursive
construct. Exceeding it emits exactly one `AST001` (error/syntax), skips the
affected branch with a bounded balanced-delimiter skip, and continues with later
input. Parsing runs on a scoped 16 MiB worker stack so the explicit guard is
reached safely even in unoptimized builds; no state is shared between parses. If
that worker cannot be created or terminates unexpectedly, `parse_ast` returns an
empty AST plus exactly one `AST002` diagnostic and never retries the recursive
parser on the caller's stack.

**Known differences.** For ordinary grammar and malformed inputs, the AST parser
reproduces legacy `PARxxx` code, severity, kind, message and span as a multiset
on the grammar inventory and full `corpus/{small,medium,large,invalid}`. The only
intentional syntax diagnostics are `AST001` for input nesting that legacy does
not explicitly guard and `AST002` for the safe worker resource-failure path. AST recovery nodes are internal tree structure and are not
user-visible diagnostics. No AST semantic analysis, SEM migration, switch/lazy
syntax, pipeline selection, or CLI behavior is part of Phase 4.

**Tests.** Dedicated suites cover AST shape for every declaration/statement/
expression category, exact spans including UTF-8 and EOF, grammar-inventory and
full-corpus differential parity, recovery after each malformed construct group,
deterministic prefix/token-soup robustness, empty/non-Eof token streams, depth
boundaries 255/256/257, extreme expression nesting, and deeply nested blocks.

### Phase 5 — Firestorm extensions (implemented)

- **Goal:** add Firestorm-only syntax behind explicit flags.
- **Functionality:** `switch`/`case`/`default`/`break`, lazy lists,
  `USE_SWITCHES`, `USE_LAZY_LISTS`, dialect profile plumbing, AST nodes for the
  new constructs. The global `//fspreprocessor off` switch is **not** part of
  this phase: it is preprocessor behavior implemented in Phase 3 (§10.2).
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
  lazy-list operators, and interaction tests with a script already disabled by
  the Phase 3 `//fspreprocessor off` marker.
- **Done when:** the dialect matrix is fully covered and the standard mode
  remains unchanged.

#### Phase 5 implementation record

**Status:** implemented in `src/ast/ext.rs`, `src/ast/parser.rs`,
`src/preprocessor/config.rs`, `src/preprocessor/directive.rs`, and
`PreparedSource`. `main` and the CLI remain on the Legacy engine.

**Independent gates.** `PreprocessSettings` carries `switches_enabled` and
`lazy_lists_enabled`, both false by default, with independent builders. The
effective gates are:

```text
switches  = Firestorm dialect && (setting || USE_SWITCHES latched)
lazy list = Firestorm dialect && (setting || USE_LAZY_LISTS latched)
```

LSL mode forces both off. After each emitted preprocessing token, the
preprocessor checks whether the corresponding `USE_*` macro is currently
defined. The first such observation latches that feature, including definitions
from includes; inactive conditional branches, comments and strings do not
trigger it. A later `#undef` does not clear an already latched discovery.
`//fspreprocessor off` returns identity text with both gates
off, overriding settings and macros.

**AST API.** `PreparedSource::switches_enabled()`/`lazy_lists_enabled()` expose
effective gates. `AstParseOptions` and
`parse_ast_with_options(tokens, options)` consume them without importing the
preprocessor. `parse_ast(tokens)` remains the compatibility API and is exactly
`AstParseOptions::lsl()` (both extensions disabled).

**Switch.** Firestorm `switch`/`case`/`default`/`break` are contextual,
not new lexer keywords. `SwitchStatement` preserves the condition and ordered
`SwitchArm`s; `SwitchLabel` distinguishes case expressions, default and explicit
error labels. Duplicate cases/defaults remain distinct arms, missing default is
not fabricated, and fallthrough is represented by statement order. `break;` is a
`StatementKind::Break` only inside an enclosing switch, including nested blocks;
outside a switch it remains ordinary LSL identifier syntax. Case bodies support
both `case x:` and `case x { ... }` forms. Malformed labels, missing
conditions/brackets/blocks and unexpected tokens recover into Error nodes while
later statements remain parseable.

**Lazy lists.** `LazyIndexExpression` stores the base and one or more index
expressions with exact spans. Firestorm-confirmed forms are represented without
rewriting source:

- single-index assignment: `identifier[index] = rhs`;
- typed extraction: `(integer|float|string|key|vector|rotation)identifier[index]`;
- list extraction: `(list)identifier[index]` and `(list)identifier[first,last]`;
- the optional fully parenthesized typed-reference form.

Firestorm does not confirm bare reads, compound indexed assignment, deletion,
more than two indices, or non-identifier bases; the parser diagnoses the
unsupported structural forms it can prove locally. `quaternion` is not added as
a type: the frozen lexer/type model has no quaternion token, and Phase 5 does not
invent one. No source-to-source replacement with `lazy_list_set`/`llList2*` is
performed.

**Spans and safety.** All extension nodes carry prepared `Span`s and pass the
same worker-stack, AST001/AST002, recovery and SourceMap bridge contracts as
Phase 4. Nested switches and nested lazy indices are depth-guarded at 256.

**Tests.** `tests/ast_firestorm.rs` covers the gate matrix, USE_* latch, includes,
inactive branches/comments/strings, kill switch, switch ordering/fallthrough/
default/break/nesting/case blocks/spans/duplicates/malformed recovery, lazy
access/assignment/casts/indices/spans/malformed recovery and deep nesting.
`tests/ast_bridge.rs` resolves every new node span through identity, macro and
include prepared streams. Existing Legacy/grammar/corpus tests remain unchanged.

### Phase 6 — AST semantic analyzer and migration (Phase 6A–6F implemented)

#### Phase 6A implementation record

**Status:** implemented independently in `src/ast_semantic/`; not connected to
`AnalysisRun`, the CLI, pipeline, preprocessor or SourceMap. The Legacy analyzer
remains compiled, unchanged and authoritative.

**API.** `analyze_ast(&Ast, &[Diagnostic]) -> Vec<Diagnostic>` runs the migrated
structural rules over the typed AST. `src/ast_semantic/scopes.rs` exposes the
AST-derived `ScopeTree`, `Scope`, `ScopeId` and `Symbol` model for later type and
name-resolution work. No token stream is accepted or rescanned.

**Scopes.** The root scope holds global variables and functions. Each function
and canonical-event body gets a scope, with parameters registered before locals.
Each `StatementKind::Block` creates a child scope; unbraced control bodies use
the enclosing scope. Firestorm switch bodies create a child scope, and block-form
case arms create another child. Missing names/bodies and Error nodes are skipped.
Traversal uses an explicit work stack rather than recursive AST visitation.

**Migrated rules.** SEM001, SEM007, SEM020 and SEM021 are implemented in focused
files under `src/ast_semantic/rules/`. They preserve the Legacy code, severity,
kind, message and primary/previous spans. `src/ast_semantic/rule-ownership.md`
contains the complete SEM001–SEM023 ownership matrix and parity status.

**Parity.** `tests/ast_semantic_parity.rs` lexes once, runs Legacy parse+semantic
and AST parse+semantic independently, filters those four codes, and compares the
full normalized multiset (code, severity, kind, message, all byte and line/column
coordinates) over every `.lsl` corpus fixture in deterministic order. Focused
tests cover cross-kind globals, parameter/local collisions, shadowing, canonical
and unknown events, state/builtin/god-only collisions, malformed recovery and
nested switch scopes. No intentional diagnostic deltas exist in this slice.

**Boundary.** No CLI/pipeline engine selection, no compare mode, no new SEM codes
or severity changes, no AST semantic rules beyond these four, and no changes to
Legacy parser/analyzer behavior. SEM002–SEM006, SEM008–SEM019, SEM022 and SEM023
remain Legacy-only.

#### Phase 6B implementation record

**Status:** implemented independently in `src/ast_semantic/{types.rs,calls.rs}`
and focused rule modules; still not connected to `AnalysisRun`, CLI, pipeline,
preprocessor or SourceMap. Legacy remains the only user-visible semantic engine.

**Expression typing.** `infer_expression_type` operates on typed AST expressions
and returns `Option<String>`, conservatively `None` when no type is provable. It
covers literals, list/vector/rotation literals, visible identifiers, built-in
constants, explicit casts, component reads, unary/binary operators, and known
built-in/user call returns. Identifier resolution uses declaration spans so
use-before-declaration and nested shadowing follow Legacy source-order rules.
Function declarations are not treated as values.

**Signatures.** User signatures come from the first AST function with a body;
built-in function/event signatures and constants come from the existing vendored
`builtins::definitions()` data. No duplicate catalogue was added. Untyped user
functions have the Legacy `void` return type.

**Rules.** SEM002/SEM004 (built-in arity/type), SEM003 (canonical event parameter
count/type), and SEM005/SEM006 (user arity/type) are implemented in focused rule
modules. `src/ast_semantic/calls.rs` visits every call exactly once across all
expression-bearing fields, including switch conditions/labels/arms and lazy-list
indices. A user function sharing a built-in name receives both checks, preserving
Legacy multiplicity.

**Recovery/suppression.** `CallExpression` stores opening/closing parenthesis
spans and `declared_argument_count`/`arguments_complete` so syntax diagnostics
inside an argument list suppress SEM002–SEM006 like Legacy and malformed arity is
still counted; recovery stops at a top-level semicolon. `EventHandler` stores
`declared_parameter_count` and declared parameter type kinds, preserving
malformed/nameless parameter positions for SEM003. Malformed top-level function
headers with `)` plus a body contribute a recoverable signature through
`Ast::recovered_functions` without changing parser diagnostics, so user-call
checks and SEM021 namespace precedence retain the Legacy first definition. Unknown extension events remain
unchecked. Switch case expressions use the switch body scope, matching Legacy
local visibility from earlier arms.
SEM001 also mirrors Legacy's top-level-call-in-global-initializer quirk.

**Parity.** `tests/ast_semantic_parity.rs` compares full normalized multisets for
the nine migrated codes over every corpus fixture. Focused tests assert exact
messages/spans for SEM002–SEM006, every expression-bearing call position,
built-in/user name collisions, first-definition behavior, implicit conversions,
syntax suppression, Firestorm switch/lazy traversal and malformed events. The
type resolver has unit tests for the complete Legacy binary result matrix,
cast pairs, components, source-order lookup and call returns. No intentional
diagnostic deltas exist in this slice.

**Boundary at the end of 6B.** SEM008–SEM019, SEM022 and SEM023 remained
Legacy-only. No CLI/pipeline selection, new SEM codes, severity changes or Legacy
behavior changes.

#### Phase 6C implementation record

**Status:** implemented independently in `src/ast_semantic/flow.rs`; still not
connected to `AnalysisRun`, CLI, pipeline, preprocessor or SourceMap. Legacy
remains the only user-visible semantic engine.

**Flow model.** Function bodies and canonical event bodies are independent
code scopes for labels/jumps. Missing/unclosed bodies and unknown extension
events are skipped. `ScopeTree::is_ancestor` supplies same/ancestor/descendant
semantics without byte-offset heuristics. Traversal is source ordered and
iterative for nested blocks; unbraced control-flow bodies reuse their enclosing
scope.

**SEM008.** Explicit returns in top-level functions use the Phase 6B typer and
first-definition user signatures. Untyped functions reject values, typed
functions reject missing values, and known incompatible types use the existing
implicit-conversion matrix. `ReturnStatement::keyword_span` preserves the exact
Legacy no-value primary span. A malformed non-empty return expression retains a
recovered `ReturnStatement` with an Error expression at its first token; labels
and jumps inside that range are retained as recovered AST metadata for
SEM009/SEM010. Events are not checked.

**SEM009/SEM010.** Labels are collected before jumps, so forward references are
valid. Duplicates keep the first declaration for lookup; jumps resolve against
the function/event namespace and require the label scope to be the same as or an
ancestor of the jump scope.

**SEM011.** A bounded AST CFG models sequential statements/blocks, if/else,
for/while/do-while, returns, labels, forward/backward jumps and empty `for`
conditions (`ForStatement::condition_missing`). Nodes, edges, work and depth are
capped; edges are deduplicated; unsupported or incomplete graphs skip the whole
function. A function with SEM009/SEM010 findings is suppressed. Unsupported
Firestorm switch/break and Error/recovery flow forms are skipped conservatively.

**Conditions.** An AST-only evaluator proves the Legacy integer constant cases
with 32-bit wrapping, division/remainder/shift guards, unary/binary operators,
canonical integer constants, immutable-name checks against the lexical scope and
the simple float-literal/constant truthiness case. Unknown conditions keep both
path outcomes; calls, mutations, unsupported syntax and excessive depth/work stay
unknown.

**Parity.** The differential harness now compares full normalized records for the
13 migrated codes SEM001–SEM011, SEM020 and SEM021 over every corpus fixture.
Focused tests cover exact messages/spans, duplicate/first-definition signatures,
return expressions, label/jump ancestry, constant conditions, loops, budget
boundaries, invalid-jump suppression, malformed recovery and Firestorm
switch/break. No intentional diagnostic deltas exist in this slice.

**Boundary at the end of 6C.** SEM012–SEM019, SEM022 and SEM023 remained
Legacy-only. No new SEM codes, severity changes, CLI/pipeline selection or Legacy
behavior changes.

#### Phase 6D implementation record

**Status:** implemented independently in `src/ast_semantic/expressions.rs` and
focused rule modules; still not connected to `AnalysisRun`, CLI, pipeline,
preprocessor or SourceMap. Legacy remains the only user-visible semantic engine.

**Shared traversal.** One AST-only facts pass provides every expression with its
`ScopeId` plus every declarator initializer. Scope propagation matches Phase 6A:
root globals, function/event body scopes, nested blocks, switch scopes and
unbraced control-flow bodies. Unknown extension event bodies are traversed.
Phase 6B call collection now filters the same facts, preserving call order and
multiplicity.

**Migrated rules.** SEM014 reuses the Phase 6B cast matrix and target type span.
SEM015 checks each global/local declarator initializer independently with
source-order visibility. SEM016/SEM017 accept the same conservative direct
identifier lvalues as Legacy and reuse the binary result-type matrix. SEM018
handles grouped vector/rotation component writes for plain and arithmetic
compound operators. SEM019 marks recognized component writes first and reports
only invalid reads, so writes never double-report.

**Conservatism.** Unknown/unresolved types, malformed/Error expressions and
Legacy-ignored grouped identifier lvalues remain unchecked. No token scanning,
source slicing or duplicate type matrices were introduced.

**Parity.** The differential harness now compares full normalized records for the
19 migrated codes SEM001–SEM011, SEM014–SEM021 over every corpus fixture. Focused
tests cover exact messages/spans, the cast matrix, comma declarators, nested
control-flow/switch/unknown-event initializers, plain/compound assignments,
grouped component lvalues, component read/write suppression, unknown bases and
cross-rule multiplicity. No intentional diagnostic deltas exist in this slice.

**Boundary at the end of 6D.** SEM012, SEM013, SEM022 and SEM023 remained
Legacy-only. No new SEM codes, severity changes, CLI/pipeline selection or Legacy
behavior changes.

#### Phase 6E implementation record

**Status:** implemented independently in `src/ast_semantic/rules/`; still not
connected to `AnalysisRun`, CLI, pipeline, preprocessor or SourceMap. Legacy
remains the only user-visible semantic engine.

**SEM012.** Source-ordered expression facts and `ScopeTree::lookup_value_at`
resolve visible variables/parameters; canonical constants are known; direct call
callees are excluded. Unknown-call argument ranges suppress only with a real
matched `)`, including nested expressions and nested unknown calls. Unknown
extension event bodies suppress only when state/event bodies are structurally
closed. Malformed/unmatched calls therefore retain eligible identifiers.

**SEM013.** Actual call sites are checked against the existing canonical
`definitions().deprecated_functions` metadata. User functions shadow/reuse of a
deprecated name suppress the warning; declarations are not calls. A real matched
`)` is required, but syntax errors inside matched arguments are not suppression
and `arguments_complete` is irrelevant. SEM013 is independent of SEM012's
unknown-call/event boundaries. Candidates are source-sorted by callee span.

**AST metadata.** `CallExpression::matched_close` distinguishes real closing
parentheses from malformed-call recovery. Malformed unmatched calls retain
eligible identifier expressions so SEM012 sees them while no suppression range
is created; malformed event parameter identifiers are retained as recovered
references without becoming declared parameters. The metadata is additive and
parser diagnostics are unchanged.

**Parity.** The differential harness compares full normalized records for all 21
migrated codes SEM001–SEM021 over every corpus fixture. Focused exact tests cover
warning severity/kind/spans, canonical deprecated metadata (without hardcoding
the list), unknown-call/event suppression, malformed unmatched calls, nested
calls, source-order/multiplicity and cross-rule independence. No intentional
diagnostic deltas exist in this slice.

**Boundary at the end of 6E.** SEM022 and SEM023 remained Legacy-only. No new
SEM codes, severity changes, CLI/pipeline selection or Legacy behavior changes.

#### Phase 6F implementation record

**Status:** implemented independently in `src/ast_semantic/rules/` and the shared
statement walker; still not connected to `AnalysisRun`, CLI, pipeline, compare
mode, preprocessor or SourceMap. Legacy remains the only user-visible semantic
engine.

**SEM022.** Known states come only from complete AST state declarations
(`body_closed`, non-missing, non-empty names), including `default`. State changes
are eligible only inside complete state bodies and complete canonical event
bodies; ordinary functions, unknown extension events, incomplete expressions and
unmatched bodies are excluded. Later states are known before checking, and the
Legacy `state default;` keyword exception is preserved.

**SEM023.** A shared iterative statement walker visits function and all event
bodies, nested blocks, unbraced if/else/loop bodies and switch arms. Only a
statement-owned expression whose parenthesized root is one of the six comparison
operators is a candidate. Control conditions, initializers, returns and other
used expressions are not candidates. Statement-local syntax diagnostics, Error
descendants, calls, assignments and increments suppress the warning; the span is
the exact operator token.

**Parity.** The differential harness now compares full normalized records for all
23 existing codes SEM001–SEM023 over every corpus fixture. Focused exact tests
cover both rules' messages/spans/severity/kind, state-target completeness and
event boundaries, all comparison operators, nested/unbraced control flow,
unknown events, syntax/side-effect suppression, Firestorm switch arms and
cross-rule coexistence. No intentional diagnostic deltas exist in this slice.

**Boundary.** All existing SEM rules are AST-migrated; engine selection and
CLI/pipeline integration remain intentionally out of scope.

#### Phase 6G implementation record

**Status:** implemented and wired on the experimental branch. Slice 1
(`4f13716bbd49696ab68984cee86943e7c15f3e74`) added the engine API; this slice
wires configuration, the CLI, output, and documentation on top of integration
base `4dd01ca16539e6944a6714f133e9d2636adb6a95`. The default engine remains
`legacy`.

**Engine selection.** `LintConfig` accepts `engine = legacy|ast|compare`, with
`legacy` as the default and line-numbered errors for invalid/empty values. The
CLI accepts `--engine VALUE` and `--engine=VALUE`, rejects missing/empty/unknown
values with exit 2, and applies the CLI value after configuration load. No
`--ast`/`--compare` aliases exist.

**Preparation and dispatch.** Three paths are preserved: the plain no-preprocess
Legacy direct loop is unchanged and remains uncapped at run level; `ast` and
`compare` without preprocessing use byte-identical identity preparation; any
engine with preprocessing uses the full preprocessor and dialect. The single
unit-based runner keeps unit order, stdin handling, include-graph grouping,
per-source `config.apply`, and the invocation-wide `PRE039` cap. AST/Compare
preflight every unit before analysis and fail fast with exit 2 if a prepared
source is missing. Engine analysis errors map to
`neolint: cannot analyze unit N` and exit 2.

**Tokens and compare output.** Each prepared unit is lexed once; the same token
slice feeds both analyzers and `--tokens`, so the second
`lex(prepared.text())` call is gone. Compare displays the Legacy stream only on
stdout, preserving the existing human/JSON schema. A deterministic report is
written to stderr: compact human lines or one JSON object
`{"engine":"compare","units":[…],"matched":…}`. Per-rule counts are exact,
missing/extra fingerprints are bounded at 64 per rule per direction, and the
100,000-fingerprint budget marks the comparison incomplete. Exit code 3 signals
an unmatched Compare report after any exit-2 failure; a matched report follows
the displayed Legacy stream (0/1). The run-level `all_matched()` signal requires
both `matched` and `comparison_complete` on every unit, so an inconsistent
incomplete report cannot select a successful exit.

**Tests and compatibility.** `tests/legacy_golden.rs` runs the same six
pre-change fixtures captured from base `4dd01ca…` both by default and with
`--engine legacy`. `tests/cli.rs` covers both flag forms, config selection and
CLI override, invalid/missing/empty values, identity preparation without
preprocessing, preprocessing/includes/Firestorm/stdin, stderr report separation,
JSON stdout compatibility, exit 0/1/2/3 precedence, multi-file and directory
report order, and single-lex token parity. Slice 2b adds config-before-report,
AST/Compare invocation-cap, help, legacy-override, prepared-token, and
identity-byte coverage, plus incomplete-vs-truncated, every-code and absolute-limit
engine tests. One exit-3 fixture is deliberately
corrupted input (`while (i < n( {` plus an unterminated non-returning function
and an event named `llSay`); that SEM002 divergence is recovery behavior on
invalid input, not a parity defect. Divergence also occurs on accepted input: a
declaration identifier that collides with a builtin name (for example
`default { llSay() { } }`) yields a Legacy-only SEM002, because Legacy scans the
event header as a builtin call while AST only inspects expression call sites.
The differential corpus does not contain that collision, so the rule matrix's
`exact` status remains corpus-relative; Compare reports the genuine delta.

**Validation evidence (final working-tree run).**

- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --all-targets --all-features -- -D warnings` → exit 0.
- `cargo test --all-targets --all-features` → exit 0, 788 passed, 0 failed.
- `cargo test --release --all-targets --all-features` → exit 0, 788 passed.
- `cargo +1.75.0 test --all-targets --all-features` → exit 0, 788 passed.
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` → exit 0.
- `python3 -m unittest discover --start-directory scripts --pattern 'test_*.py'`
  → 9 tests OK.
- `python3 scripts/test_documentation.py` → 6 tests OK.
- `git diff --check` → exit 0.

**Boundary.** No AST parser/semantic rule, engine, source, pipeline, lexer,
diagnostic, or preprocessor module changed in this slice. Parser/lexer parity is
still not claimed; Compare covers SEM001–SEM023 only.

- **Goal:** migrate SEM rules to the AST engine, add conservative AST-only rules,
  and stabilize.
- **Functionality:** scope/type/flow analysis over the AST; rule ownership
  matrix; differential Legacy↔AST harness; engine selection with a Legacy-only
  displayed stream in compare (no dedup merge); new conservative rules with tests
  and docs; performance validation.
- **Modules:** `src/ast_semantic/{mod.rs,scopes.rs,types.rs,flow.rs,rules/…}`,
  unified diagnostics layer.
- **Dependencies:** Phases 4–5.
- **Acceptance criteria:**
  - every migrated rule has parity evidence or a documented, tested delta;
  - no duplicate findings in user-visible output;
  - deterministic ordering and stable output across runs;
  - performance within documented bounds on the corpus;
  - the legacy parser (`parse_legacy`) and legacy semantic analyzer remain
    public, compiled, and selectable until stabilization is signed off, so
    differential testing stays possible.
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
  diagnostic.rs           Diagnostic/Span/Severity/DiagnosticKind (shared, frozen)
  diagnostic/
    resolve.rs            Span bridge use, LegacyDiagnostic tag/root checks,
                          UnifiedDiagnosticInput -> RenderedDiagnostic
  lexer.rs                tokenizer only; no directive knowledge
  parser.rs               legacy token parser: parse_legacy + frozen parse alias
  semantic.rs             legacy semantic analyzer (frozen behavior)
  builtins.rs             vendored LSL metadata (unchanged)
  config.rs               lint configuration (extended additively later)

  source/
    mod.rs                re-exports
    file.rs               SourceId { unit, file }, SourceFile, kinds, text
    line_index.rs         byte offset -> line/column (lexer-compatible)
    map.rs                unit-bound SourceMap, PreparedSpan, SourceSpan,
                          private SourceOrigin, OriginId, SpanError, segments,
                          validated origin arena, EOF anchor, checked resolve_*
                          with foreign-identity rejection
    manager.rs            per-unit loading, path policy, cycles, limits
    unit.rs               RunId/AnalysisUnitId assignment, opaque AnalysisUnit
                          handle with legacy_root(), source namespace, optional
                          PreparedSource/SourceMap pair, diagnostics

  preprocessor/
    mod.rs                preprocess(), PreprocessConfig/Options, PreparedSource with
                          unit id, prepared_span (only bridge), eof_span
    identity.rs           Phase 1 identity pass-through
    directive.rs          Phase 2 raw-text directive scanner
    macros.rs             macro table, definitions, expansion
    expr.rs               #if expression evaluator
    include.rs            include graph, path parsing/resolution
    builtins.rs           confirmed __FILE__/__LINE__ predefined macros
    diagnostics.rs        PreprocessDiagnostic constructors, PRE codes

  ast/
    mod.rs                AST root and re-exports
    node.rs               node types with spans
    parser.rs             AST parser and recovery: independent parse_ast
    ext.rs                Firestorm-only AST nodes/options

  ast_semantic/
    mod.rs                Phase 6 analyzer entry point
    scopes.rs
    types.rs
    flow.rs
    rules/                per-rule modules, ownership matrix

  pipeline.rs             per-unit orchestration shared by CLI/tests
```

Rules for the structure:

- **Lexer** keeps tokenization, literals, operators, comments, `Span`
  construction, and `Eof`. It must never learn directives, macro tables, or
  include logic. `#` handling stays as today unless the preprocessor has already
  removed the text.
- **Preprocessor** owns everything between raw bytes and prepared bytes:
  directive detection, conditional evaluation, macro expansion, include
  resolution, stringification, pasting, profile built-ins, and the map.
- **Source Map** lives in `src/source/map.rs` and is owned by an
  **AnalysisUnit**, not by the CLI invocation, the preprocessor, or the lexer.
  The preprocessor is its only writer during construction; consumers get an
  immutable `Arc`. Only the map builder inserts origins, after unit validation;
  `SourceId`/`OriginId` values never cross unit or run boundaries.
- **AST types** live in `src/ast/`; AST semantic rules live in
  `src/ast_semantic/`. The AST must not import preprocessor internals; it only
  sees tokens and spans.
- **Both parser and semantic engine pairs** are reachable through one pipeline
  entry point that selects `Legacy`, `Ast`, or `Compare`; they share
  `Vec<Token>` and the unit's `SourceMap`. `parse_legacy` and `parse_ast` are
  separate public functions; no engine calls the other for its full result.
- **Diagnostics** stay in `src/diagnostic.rs` (frozen). The additive
  `diagnostic/resolve.rs` module owns the `UnifiedDiagnosticInput` conversion:
  direct location for `Preprocess`, explicit bridge + `SourceMap` for
  `Prepared`, and tag/root check + pass-through for `Legacy`. It is the only
  place that builds `RenderedDiagnostic`; no other module may mint a
  `LegacyDiagnostic` or attach a `SourceId` to one.
- **`Span` bridge** lives with `PreparedSource` (it needs the prepared text for
  UTF-8 validation) and returns a unit-stamped `PreparedSpan`; no other module
  may convert `Span` to prepared coordinates.
- **Unit identity** is created in `src/source/unit.rs` and embedded in every
  `SourceId`/`PreparedSpan`/`SourceSpan`/`OriginId`. `RunId` and unit index are
  generated only there. `SourceMap` checks ownership before geometry; no other
  module may mint ids, spans, origins, or the opaque `AnalysisUnit` handle.

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
11. EOF and empty-buffer resolution are easy to get subtly wrong (offset 0,
    synthetic anchors, `PAR013`). The Phase 1 contract tests in §7.3 are
    mandatory, not optional.
12. Per-unit isolation is a correctness property, not an optimization. A shared
    macro table or a shared `SourceId` namespace would make output depend on
    input order and break the existing per-file contract.
13. Maintaining two parsers duplicates grammar knowledge. Shared helpers are
    allowed only within the independence rules in §4.8 and §14; parity must be
    proven by the differential harness, not assumed.
14. `//fspreprocessor off` is a raw substring match, so it can trigger inside a
    string or comment. This is intentional Firestorm-compatible behavior and
    must not be "fixed" into comment-aware matching.
15. `#pragma once` is supported by the pinned Boost.Wave but not proven
    end-to-end through Firestorm's inventory include layer; keep it in the
    separate compatibility bucket until evidence exists.
16. `Span` has no namespace tag. In the preprocessed path every engine `Span`
    is prepared; in plain-LSL mode it is original. Any code that treats an
    engine span as original after preprocessing is a silent mislocation bug.
    The checked bridge is mandatory, and the `Legacy` variant is the only
    pass-through.
17. Do not turn a `SpanError` into the EOF anchor for convenience. That would
    satisfy "renders something" while hiding builder/engine bugs; §7.3.1
    requires the opposite.
18. `PreprocessDiagnostic` must capture its `SourceSpan` while scanning the
    original bytes. Reconstructing it later from the prepared buffer is
    impossible when a directive or macro definition was removed.
19. Unit-local ids are not enough: without the embedded `AnalysisUnitId`, a
    `SourceId(0)` from A would be indistinguishable from `SourceId(0)` from B and
    could silently render B's file. The unit must be part of `SourceId` and of
    `PreparedSpan`, and checked before geometry.
20. `Span` carries no provenance. The adapter can validate a range and assign
    the current source's identity, but it cannot prove the span came from that
    source. Do not claim such verification; enforce the calling convention that
    engines receive only their own `PreparedSource`.
21. `debug_assert!` is not an acceptable malformed-input policy. Debug and
    release must return the same `Result`; assertions in the resolution path
    would make behavior build-dependent and violate panic-free handling.
22. The frozen `Diagnostic` has no unit identity. `LegacyDiagnostic` supplies
    the tag, and `from_legacy` uses only the unit handle's root; the raw
    diagnostic's provenance still cannot be proven if a caller bypasses the
    wrapper. The wrapper has no public constructor to make that bypass hard.
23. The origin arena is a trusted structure only if every insertion is
    validated. Keep `SourceOrigin` fields private, make `OriginId` map-local,
    and revalidate on traversal; a test-only corrupted arena must still fail
    closed rather than emit a local note.
24. `SourceSpan`/`PreparedSpan` are `Copy` values and can escape their run, so
    lifetime branding alone is insufficient. `RunId` is required to reject
    cross-invocation coordinates at runtime; it is process-local and must be
    excluded from output and ordering.

Open questions to resolve during implementation (with an update to this
document):

- Should `PRE` diagnostics get a dedicated `DiagnosticKind`, and how does that
  affect the documented JSON `kind` values?
- Are related locations exposed in JSON in Phase 3, and under which schema
  versioning rule?
- Exact CLI spelling for dialect/preprocessor/extension flags.
- Whether the prepared buffer is one concatenated string per AnalysisUnit
  (current plan) or one per file (rejected for now: a single buffer keeps token
  spans unambiguous). It is always per unit, never per CLI invocation.
- MSRV verification belongs in the experimental workflow as an additional step;
  it is not part of the required Phase 0 quality gate.

---

## 18. Glossary

- **Span bridge:** the explicit, checked `Span -> PreparedSpan` conversion
  (`PreparedSource::prepared_span`). It validates the range and assigns the
  source's unit; it cannot recover provenance that `Span` does not carry.
- **PreparedDiagnostic:** lexer/parser/semantic finding carrying a frozen `Span`
  in prepared coordinates.
- **PreprocessDiagnostic:** preprocessor finding carrying a direct original
  `SourceSpan`; never resolved through the map.
- **UnifiedDiagnosticInput:** the additive enum accepted by the unified layer
  (`Preprocess`, `Prepared`, `Legacy`).
- **RenderedDiagnostic:** resolved output holding the frozen `Diagnostic`, the
  original `SourceId`, and related locations.
- **SpanError:** explicit error for malformed/invalid coordinates; never a
  silent fallback.
- **RunId:** process-local identity of one invocation, allocated by a
  process-wide mutex-protected checked monotonic counter; concurrent callers
  receive unique IDs. Poisoning or exhaustion is an explicit setup error; the
  ID is not user-facing and is not part of output/order.
- **AnalysisUnitId:** `{ RunId, index }`; the index is assigned deterministically
  in input order within one run. Coordinates from different runs never compare
  equal.
- **LegacyDiagnostic:** frozen `Diagnostic` plus the producing unit tag; created
  only by that unit's legacy pipeline and accepted only with that unit's handle.
- **legacy_root:** the root `SourceId` exposed by a plain-LSL, no-map
  `AnalysisUnit`; `None` for preprocessed units.
- **OriginId:** map-local handle to one `SourceOrigin`; carries the owning unit
  and is rejected by another map.
- **AnalysisUnit:** the independent analysis of one root file or stdin stream;
  owns its `AnalysisUnitId`, `SourceId` namespace, source manager, and
  diagnostics. Prepared/preprocessor mode has one `PreparedSource`/`SourceMap`
  pair; plain-LSL Legacy mode has neither and uses `legacy_root()`.
- **ForeignAnalysisUnit:** `SpanError` for a `SourceSpan`/`RelatedLocation`/
  EOF anchor/origin field/origin id used against another unit's (or another
  run's) map.
- **ForeignPreparedSource:** `SpanError` for a `PreparedSpan` used against
  another unit's `SourceMap`.
- **EOF span:** the unit-stamped zero-length `PreparedSpan` returned by
  `PreparedSource::eof_span`, used for EOF point resolution.
- **Prepared source / prepared buffer:** the single UTF-8 text produced by the
  preprocessor and consumed by the lexer. It is not a `SourceFile`.
- **Prepared span (`PreparedSpan`):** a byte range in one unit's prepared
  buffer, carrying the owning `AnalysisUnitId` but no `SourceId`.
- **Source span (`SourceSpan`):** a byte range in an original root/include file,
  with a `SourceId` that embeds the owning `AnalysisUnitId`.
- **EOF anchor:** a unit-owned, real original-file `SourceSpan` used for empty
  prepared buffers and zero-length spans at the end of input.
- **Origin:** one link describing where prepared bytes came from.
- **Origin chain:** the ordered list of origins from prepared bytes back to the
  root file.
- **Direct code:** bytes copied unchanged from a file.
- **Identity mode:** preprocessing that copies input bytes unchanged; the
  Phase 1 default and the legacy compatibility anchor.
- **Dialect profile:** the explicit set of language/preprocessor/extension
  behaviors for a unit (`Lsl`, `Firestorm` plus flags).
- **Legacy engine:** the frozen token-based `parse_legacy` parser and legacy
  semantic analyzer; remains public through the migration.
- **AST engine:** the independent `parse_ast` parser and AST semantic analyzer.
- **Parity:** agreement between legacy and AST engines on a defined diagnostic
  multiset, or a documented and tested intentional difference.
- **Unified diagnostics:** the layer that resolves, merges, filters, orders, and
  renders findings from either engine and from all units.

---

## 19. References

### Current codebase

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
- Compatibility evidence: `tests/`, `corpus/manifest.txt`,
  `tests/grammar_inventory.rs`.

### External behavior evidence

- Firestorm preprocessor source:
  [`indra/newview/fslslpreproc.cpp`](https://github.com/FirestormViewer/phoenix-firestorm/blob/4643f4acb7a50624cd30acbf6ae4b5157fa207e6/indra/newview/fslslpreproc.cpp)
  at commit `4643f4acb7a50624cd30acbf6ae4b5157fa207e6`. Marker check at lines
  1145–1159; preprocessing/extension guards around the `preprocessor_enabled`
  branches.
- Firestorm Boost pin: `autobuild.xml` (`boost` dependency) at the same commit
  references Boost 1.90.
- Boost.Wave 1.90 `#pragma once` support:
  `include/boost/wave/util/interpret_pragma.hpp` and
  `include/boost/wave/wave_config.hpp` (`BOOST_WAVE_SUPPORT_PRAGMA_ONCE`
  defaults to 1) at tag `boost-1.90.0`.
