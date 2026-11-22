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
15. **No implicit coordinate conversions.** `Span -> PreparedSpan` is an
    explicit checked call; `PreparedSpan -> SourceSpan` is only through the
    source map; there is no `Span -> SourceSpan` conversion.
16. **Different diagnostics carry different coordinate kinds.**
    Preprocessor diagnostics carry original `SourceSpan`s and bypass the map;
    engine diagnostics carry prepared `Span`s and are resolved through it.

---

## 1. Current pipeline (A)

```
CLI arguments
  → LintConfig (config file, --disable, --severity)
  → input collection (files, directories, stdin)
  → for each collected input (each input is an independent analysis unit):
      read whole file as UTF-8 String
      → lex(&source)            -> Vec<Token>, Vec<Diagnostic> (LEX)
      → parse(&tokens)          -> Vec<Diagnostic> (PAR)
      → analyze_with_syntax_diagnostics(&tokens, &syntax_diagnostics)
                                -> Vec<Diagnostic> (SEM)
      → config.apply(&mut diagnostics)
  → human / JSON rendering (results from all inputs)
  → exit code 0 | 1 | 2
```

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
  CLI/config  |   Source Manager  |   one AnalysisUnit per root input
              +---------+---------+
                        | LoadedSource: SourceId + text + kind (per unit)
                        v
              +-------------------+
              |   Preprocessor    |  (identity in Phase 1; core in Phases 2–3)
              +---------+---------+
                        | PreparedSource { text, SourceMap, PreprocessDiagnostics }
                        v
              +-------------------+
              |      Lexer        |  (unchanged tokenization rules)
              +---------+---------+
                        | Token stream (PreparedSpan + Eof)
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

Diagnostics enter the unified layer through two different coordinate paths:
preprocessor findings arrive with a direct original `SourceSpan` (the directive
may no longer exist in the prepared buffer), while lexer/parser/semantic
findings arrive with the frozen prepared `Span` and are explicitly bridged and
resolved through the unit's `SourceMap` (§7.1.1).

---

## 3. Component responsibilities (C)

### AnalysisUnit (`src/source/`, orchestrated by `src/pipeline.rs`)

- One AnalysisUnit is the complete independent analysis of **one root source**:
  one root file or one stdin stream.
- It owns:
  - its own `SourceId` namespace (root gets the first ID);
  - its own macro table;
  - its own conditional stack;
  - its own `SourceMap`;
  - its own include graph and include stack;
  - its own diagnostics and its own prepared buffer.
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
- Produces exactly one prepared text buffer per unit.
- Produces the unit's `SourceMap`, mapping `PreparedSpan` ranges to original
  code.
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
- Provides the explicit, checked bridge
  `prepared_span(Span) -> Result<PreparedSpan, SpanError>` and exposes
  `prepared_len`, `text`, and `map`. It has no implicit conversion to or from
  `Span`/`SourceSpan`.
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
  - `Legacy(Diagnostic)` — plain-LSL pass-through with the frozen `Span`.
- Rejects malformed coordinates with `SpanError` handling (§7.3.1): one internal
  diagnostic, affected finding dropped, run continues, no panic and no EOF
  substitution.
- Merges diagnostics from both engines and from all units in a deterministic
  order.
- Applies `LintConfig` exactly once after merging.
- Produces `RenderedDiagnostic` values carrying the frozen `Diagnostic`, the
  original `SourceId`, and related locations.
- Renders human and JSON output; owns the exit-code decision.

### CLI / configuration (`src/main.rs`, `src/config.rs`)

- Creates one AnalysisUnit per collected input.
- Chooses the dialect profile and the engine per unit.
- Keeps plain LSL as the default with preprocessing disabled.
- Maps CLI/config failures to exit code 2 and lint findings to 0/1.
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

- `SourceId` is local to the AnalysisUnit. IDs from different units must never be
  compared, mixed, or persisted.
- The root receives the first ID in the unit (conventionally `0`).
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

Input: a `LoadedSource` (a unit-local `SourceId`, the file text, and its display
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

Input: exactly one `PreparedSource`. Output: one token stream.

Invariants:

- The prepared text is valid UTF-8.
- The prepared text is produced deterministically from the same inputs.
- Removing directives must not merge adjacent tokens in a way that changes
  their meaning; the implementation inserts safe separation where needed.
- The prepared buffer may be empty. The lexer then emits only `Eof` at byte
  offset 0; the bridge yields the zero-length `PreparedSpan(0, 0)`.
- For non-empty buffers, the lexer's `Eof` position equals the prepared text
  length (byte offset `len`); the bridge yields the zero-length
  `PreparedSpan(len, len)`.

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
- The map exposes `Result`-based lookups. Well-formed ranges always resolve;
  malformed ranges and map holes return `SpanError` and are never repaired by
  pointing at the anchor (§7.3.1).
- The EOF anchor is valid by construction (`SourceMap::source_span` was used to
  build it), so `eof_anchor()` itself is infallible.

### 4.6 Lexer → Engines

Input: `&[Token]` ending in `Eof`. Output: diagnostics.

Invariants:

- Token order is the prepared text order.
- Every token span lies inside the prepared buffer and on UTF-8 boundaries.
- The `Eof` token has a zero-length prepared span at the end of the buffer; it is
  the anchor for parser diagnostics such as `PAR013`.
- Neither engine may assume that byte offsets are monotonic *in the original
  files*; monotonicity holds only in the prepared buffer.
- Engines receive tokens and produce the frozen `Span` values, which address the
  prepared buffer. They do not construct `PreparedSpan` themselves; the unified
  layer bridges each diagnostic with
  `PreparedSource::prepared_span(span)` and then resolves it (§7.1.1).
- Zero-length spans (including EOF spans) are legal and resolvable.

### 4.7 Preprocessor and Engines → Unified Diagnostics

Input: `UnifiedDiagnosticInput` values:

- `Preprocess(PreprocessDiagnostic)` — original `SourceSpan`, no map lookup;
- `Prepared(PreparedDiagnostic)` — frozen `Span` in prepared coordinates;
- `Legacy(Diagnostic)` — frozen `Span` in original coordinates (plain-LSL path).

Output: `RenderedDiagnostic` values grouped by original file and by unit.

Invariants:

- Every rendered diagnostic has a real file, line, column, and byte range in that
  file, including diagnostics produced at `Eof` and diagnostics in units whose
  prepared buffer is empty.
- A `PreprocessDiagnostic` never passes through `resolve_span`; its primary and
  related spans are already original and go directly through
  `SourceMap::location`/`render_span`.
- A `PreparedDiagnostic` is bridged explicitly
  (`PreparedSource::prepared_span`) and then resolved; a bridge or resolve error
  is handled as an internal error (§7.3.1), never as EOF.
- When preprocessing is disabled, the `Legacy` path is passed through unchanged
  (same `Span`, line, column, order), preserving baseline output.
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
// The exact Ast type is defined in Phase 4; this signature is normative.
pub fn parse_ast(tokens: &[Token]) -> (Ast, Vec<Diagnostic>);
```

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

New preprocessing, bridge, and diagnostic entry points are **additive**:

```rust
// conceptual, finalized in Phase 1/3; one call per AnalysisUnit
pub fn prepare(unit: &mut AnalysisUnit, config: &PreprocessConfig) -> PreparedSource;
pub fn analyze_prepared(prepared: &PreparedSource, options: &EngineOptions) -> RunDiagnostics;

impl PreparedSource {
    pub fn prepared_span(&self, span: Span) -> Result<PreparedSpan, SpanError>;
}

impl SourceMap {
    pub fn resolve_span(&self, prepared: PreparedSpan) -> Result<ResolvedSpan, SpanError>;
    pub fn resolve_point(&self, offset: usize) -> Result<SourceSpan, SpanError>;
    pub fn location(&self, span: SourceSpan) -> Result<SourceLocation, SpanError>;
    pub fn render_span(&self, span: SourceSpan) -> Result<Span, SpanError>;
}

pub enum UnifiedDiagnosticInput { Preprocess(PreprocessDiagnostic), Prepared(PreparedDiagnostic), Legacy(Diagnostic) }
```

Contract:

- `lex`, `parse`, `analyze`, `Diagnostic`, `Token`, and `Span` keep their exact
  signatures and field meanings. No existing function parameter or return type
  changes to `PreparedSpan`.
- The only `Span -> PreparedSpan` path is the checked adapter; the only
  `PreparedSpan -> SourceSpan` path is `SourceMap::resolve_span`/`resolve_point`.
- `Span -> SourceSpan` does not exist as a conversion.
- `UnifiedDiagnosticInput` is a new additive type; `Diagnostic` is not changed.

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
   skipped), de-duplicate displayed paths, enforce one stdin. The result is an
   ordered list of root inputs.
4. **Unit loop.** For each root input, in order, run the per-unit lifecycle
   below. A failure in one unit is recorded and does not abort the remaining
   units (matching today's per-file behavior).
5. **Merging.** Concatenate unit diagnostics in input order, then apply the
   deterministic ordering policy (§11).
6. **Configuration.** Apply disabled rules and severity overrides once to the
   merged set.
7. **Rendering.** Human output or JSON; exit code 0/1. Input/config failures
   return 2 as today.
8. **Teardown.** Drop all units; no global mutable state survives. Built-in
   metadata stays cached in its existing `OnceLock`.

**Per AnalysisUnit:**

1. **Unit creation.** Allocate the unit-local `SourceId` namespace, macro table,
   conditional stack, include graph, and `SourceMap`.
2. **Root load.** Read the root as UTF-8 and register it as the unit's first
   source. Unreadable/invalid root is an input error for this unit (exit 2
   contribution) and the unit ends.
3. **Preparation.** Preprocess the root:
   - identity mode: copy text unchanged, build a trivial map and EOF anchor;
   - Firestorm mode: scan directives, expand macros, resolve includes in textual
     order, build the full map.
   Preprocessor problems become `PreprocessDiagnostic` values with original
   `SourceSpan`s. Failures inside preparation are diagnostics, not Rust errors,
   except for limits that make further analysis unsafe (still diagnostics; the
   unit continues with a bounded, truncated prepared stream).
4. **Lexing.** Tokenize the prepared text. Lexical diagnostics are the frozen
   `Diagnostic` shape with a prepared `Span`. An empty prepared buffer yields
   only `Eof` at prepared offset 0.
5. **Engine selection.**
   - `legacy` (default in Phases 1–5): `parse_legacy` →
     `analyze_with_syntax_diagnostics`.
   - `ast` (Phase 4+): `parse_ast` → AST semantic analyzer.
   - `compare` (development/CI only, Phase 6): both, then differential report.
6. **Input assembly.** Collect:
   - preprocessor findings as `UnifiedDiagnosticInput::Preprocess`;
   - lexer/parser/semantic findings as
     `UnifiedDiagnosticInput::Prepared`, carrying their frozen `Span`;
   - plain-LSL findings as `UnifiedDiagnosticInput::Legacy` only when no
     preprocessor/map is active.
7. **Resolution.**
   - `Preprocess`: primary/related `SourceSpan` → `SourceLocation` directly
     (no `resolve_span`).
   - `Prepared`: `PreparedSource::prepared_span(span)` → `resolve_span` →
     `SourceSpan` → `SourceLocation`, then `render_span` into the frozen
     `Diagnostic.span`.
   - `Legacy`: pass through.
   Zero-length spans at `Eof` use the map's EOF anchor (§7.3).
   A `SpanError` follows §7.3.1: internal diagnostic, drop the affected finding,
   continue; never panic and never substitute the anchor.
8. **Unit merge.** Apply the engine-specific deduplication policy and sort by
   `(source load order, start offset, code)`. Legacy-only single-file output
   keeps its current order.

An empty source file is a valid unit: the prepared buffer is empty, the lexer
emits only `Eof`, the legacy parser reports `PAR013`, and the `Eof` diagnostic
resolves to the root file at offset 0 (line 1, column 1). Conditional
compilation that removes all user code behaves the same way, except that the EOF
anchor comes from the last directive/anchor location (§7.3).

---

## 6. Analysis units and multiple source files (F)

### 6.1 Definition

An **AnalysisUnit** is the complete, independent analysis of one root source:

- one root file, or one stdin stream;
- its own `SourceId` namespace (root first);
- its own macro table;
- its own conditional stack;
- its own `SourceMap`;
- its own include graph and include stack;
- its own diagnostics;
- its own prepared buffer and token stream.

One CLI invocation may create any number of AnalysisUnits: one per collected
file, directory entry, or stdin input. A unit is **not** a CLI invocation-wide
namespace.

### 6.2 Lifecycle and isolation

- The unit is created when its root starts loading and is dropped after its
  diagnostics have been resolved and merged into the CLI-wide result.
- All preprocessing state is unit-local. `#define`, `#undef`, conditional-stack
  entries, include guards, and `#pragma once` state never cross unit boundaries.
- Reason: the existing CLI contract analyzes each collected file on its own.
  Independent roots are independent scripts; macro state leaking between them
  would make output depend on file order, change results when a directory gains a
  file, and break the baseline compatibility tests.
- `SourceId` values are unit-local and must not be compared across units.
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

- Unit results are concatenated in the deterministic input order produced by
  directory collection (sorted paths, de-duplicated display names).
- Each rendered diagnostic keeps its unit's resolved file and location.
- Configuration filtering and severity overrides are applied once to the merged
  list.
- Exit code: `2` if any input/config failure occurred, otherwise `1` if any
  remaining error-severity diagnostic exists, otherwise `0`. This matches
  today's behavior.
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
/// Dense identifier of one original file inside one AnalysisUnit.
/// It is never assigned to the prepared buffer.
pub struct SourceId(u32);

/// Original files only. The prepared buffer is not a file kind.
pub enum SourceFileKind { Root, Stdin, Included }

pub struct SourceFile {
    pub id: SourceId,
    pub display_name: String,
    pub canonical_path: Option<PathBuf>,
    pub kind: SourceFileKind,
    pub text: Arc<str>,
    // line index is built lazily and cached
}

/// A range in the Prepared Buffer. Offsets are zero-based UTF-8 bytes.
/// Fields are private: the checked constructors below are the only way to
/// create one, so an inverted, out-of-bounds, or non-boundary prepared range is
/// unrepresentable through the public API.
pub struct PreparedSpan {
    start: usize,
    end: usize,
}

impl PreparedSpan {
    /// Checked bridge from the frozen public `Span` to prepared coordinates.
    /// `Span.line`/`Span.column` are display metadata and are not used here;
    /// only the byte offsets are validated. There is deliberately no
    /// `From<Span>`/`Into<PreparedSpan>` implementation.
    pub fn try_from_lexer(span: Span, prepared_text: &str) -> Result<Self, SpanError>;

    /// Checked constructor used by the pipeline for generated ranges.
    pub fn new(start: usize, end: usize, prepared_text: &str) -> Result<Self, SpanError>;

    pub fn start(&self) -> usize;
    pub fn end(&self) -> usize;
}

/// A range in an original file. Fields are private so that a span cannot be
/// forged without validation; `SourceMap::source_span` is the checked
/// constructor.
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

/// One link in a provenance chain. Every span here is an original-file span.
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
    pub prepared: PreparedSpan,
    pub origin: OriginId,
}

/// The result of resolving one prepared range.
pub struct ResolvedSpan {
    pub primary: SourceSpan,
    pub related: Vec<SourceSpan>,
}

/// Every way a coordinate can be malformed. All variants are errors, not
/// fallbacks: in particular `MapInvariant` and `UnmappedPreparedRange` must
/// never silently resolve to the EOF anchor.
pub enum SpanError {
    /// `start > end` in either coordinate namespace.
    InvertedRange,
    /// A prepared or original offset is past the end of its text.
    OutOfBounds,
    /// An offset splits a UTF-8 scalar value.
    NotCharBoundary,
    /// A `SourceSpan` names a `SourceId` not registered in this unit.
    UnknownSourceId,
    /// A prepared range is not covered by any map segment (builder bug).
    UnmappedPreparedRange,
    /// Segments overlap, are unsorted, or violate another map invariant.
    MapInvariant,
}

pub struct SourceMap {
    prepared_text: Arc<str>,     // validation only; never a SourceFile
    files: Vec<SourceFile>,
    segments: Vec<MapSegment>,   // sorted, non-overlapping, cover [0, prepared_len)
    eof_anchor: SourceSpan,      // always a real original-file position
    origins: Vec<SourceOrigin>,  // arena; chains are acyclic
}

impl SourceMap {
    /// Resolve a prepared range. Every well-formed range with
    /// `end <= prepared_len` is guaranteed to return `Ok`, including EOF and
    /// zero-length ranges and ranges crossing segment boundaries. Malformed
    /// ranges and map holes return `Err`; they never fall back to the EOF
    /// anchor.
    pub fn resolve_span(&self, prepared: PreparedSpan) -> Result<ResolvedSpan, SpanError>;

    /// Resolve a single prepared byte offset. `offset == prepared_len`
    /// resolves to the EOF anchor. Offsets past the end, or offsets that split
    /// a UTF-8 scalar, return `Err`.
    pub fn resolve_point(&self, offset: usize) -> Result<SourceSpan, SpanError>;

    /// Turn an original-file span into line/column coordinates. Unknown source
    /// ids and out-of-file ranges return `Err`.
    pub fn location(&self, span: SourceSpan) -> Result<SourceLocation, SpanError>;

    /// Build the frozen public `Span` for a resolved original range, so the
    /// existing `Diagnostic` shape can be rendered without changing it.
    pub fn render_span(&self, span: SourceSpan) -> Result<Span, SpanError>;

    /// Checked constructor for original spans produced by the pipeline.
    pub fn source_span(
        &self,
        source: SourceId,
        start: usize,
        end: usize,
    ) -> Result<SourceSpan, SpanError>;

    /// The anchor used for empty buffers and end-of-input diagnostics. It is
    /// valid by construction and never fails.
    pub fn eof_anchor(&self) -> SourceSpan;
}

/// The single prepared buffer handed to the lexer, with its explicit bridge to
/// prepared coordinates. `text` and `map` share the same `Arc<str>`.
pub struct PreparedSource {
    text: Arc<str>,                      // shared with the map for validation
    pub preprocess_diagnostics: Vec<PreprocessDiagnostic>,
    map: Arc<SourceMap>,
}

impl PreparedSource {
    /// The checked Span -> PreparedSpan adapter. This is the only public
    /// conversion between the frozen legacy `Span` and prepared coordinates.
    pub fn prepared_span(&self, span: Span) -> Result<PreparedSpan, SpanError>;
    pub fn prepared_len(&self) -> usize;
    pub fn map(&self) -> &SourceMap;
    pub fn text(&self) -> &str;
}

/// One secondary location attached to a rendered or preprocessor diagnostic.
pub struct RelatedLocation {
    pub span: SourceSpan,     // always an original file
    pub label: String,
}

/// Preprocessor diagnostics are born in original coordinates. They never pass
/// through `resolve_span`.
pub struct PreprocessDiagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub primary: SourceSpan,             // always an original file
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

/// Everything the unified layer accepts.
pub enum UnifiedDiagnosticInput {
    /// Preprocessor finding, already in original coordinates.
    Preprocess(PreprocessDiagnostic),
    /// Lexer/parser/semantic finding in prepared coordinates.
    Prepared(PreparedDiagnostic),
    /// Plain-LSL compatibility finding; `Diagnostic.span` is already original.
    Legacy(Diagnostic),
}

/// The rendered form: the frozen `Diagnostic` plus file identity and notes.
pub struct RenderedDiagnostic {
    pub diagnostic: Diagnostic,          // unchanged public shape
    pub source: SourceId,                // original file for CLI grouping
    pub related: Vec<RelatedLocation>,
}

impl RenderedDiagnostic {
    /// Direct insertion; no prepared-buffer resolution is involved.
    pub fn from_preprocess(
        diagnostic: PreprocessDiagnostic,
        map: &SourceMap,
    ) -> Result<Self, SpanError>;

    /// Explicit bridge then SourceMap resolution.
    pub fn from_prepared(
        diagnostic: PreparedDiagnostic,
        prepared: &PreparedSource,
        map: &SourceMap,
    ) -> Result<Self, SpanError>;

    /// No-op pass-through for the plain-LSL compatibility path.
    pub fn from_legacy(diagnostic: Diagnostic, source: SourceId) -> Self;
}
```

`PreparedSpan` and `SourceSpan` are distinct types with no implicit conversion
between them. A compiled-in mix-up is therefore a type error, not a diagnostic
that silently points at the wrong file. `SourceSpan::source()` always returns a
`SourceId` of a `SourceFile` registered in this unit; the prepared buffer has no
`SourceId` at all. `PreparedSpan` fields are private, so only the checked bridge
can create prepared coordinates.

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
2. `Span -> PreparedSpan` is explicit: `PreparedSource::prepared_span(span)` or
   `PreparedSpan::try_from_lexer(span, prepared_text)`. Both return
   `Result<PreparedSpan, SpanError>` and validate inversion, `prepared_len`, and
   UTF-8 boundaries. There is no `From<Span>`/`Into<PreparedSpan>`.
3. `Span.line`/`Span.column` are display metadata and are ignored by the bridge.
   Line/column for a rendered diagnostic are recomputed from the resolved
   original `SourceSpan` (`SourceMap::render_span`).
4. `PreparedSpan -> SourceSpan` happens **only** through
   `SourceMap::resolve_span`/`resolve_point`.
5. `SourceSpan` can never be constructed from prepared bytes: its constructor
   is `SourceMap::source_span(source, start, end)`, and it requires a real
   `SourceId`.
6. `SourceLocation` is computed only from a `SourceSpan` via
   `SourceMap::location`.
7. AST nodes store prepared coordinates (`Span` or `PreparedSpan`), never a
   `SourceSpan` for generated text. They are resolved through the same
   `SourceMap` at rendering time.
8. In plain-LSL compatibility mode there is no map: `UnifiedDiagnosticInput::
   Legacy` passes `Diagnostic.span` through unchanged, preserving baseline
   output.

These are conceptual definitions. Phase 1 fixes the exact visibility, derives,
constructors, and capacities; the semantics below are binding.

### 7.2 Coordinate namespaces and offsets

- `SourceId` is unit-local. It must never be persisted or compared across units
  or runs.
- Every offset is a zero-based UTF-8 byte offset. Spans are half-open
  (`start..end`) and must lie on UTF-8 character boundaries.
- `PreparedSpan` addresses bytes **only** in the prepared buffer. It has no
  `SourceId` and cannot be rendered directly.
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
  errors or constructor errors, not silent off-by-file diagnostics.

### 7.3 Empty input, EOF, and segment boundaries

These cases are normative; Phase 1 must test each of them.

**Empty source file.** The `SourceFile` has `len() == 0`. The map has no
segments and an `eof_anchor` that is a zero-length span at offset 0 of the root
file. The lexer emits only
`Eof` at byte offset 0, which the bridge turns into `PreparedSpan(0, 0)`.
`resolve_span(PreparedSpan(0, 0))` returns
`Ok(ResolvedSpan { primary: eof_anchor, related: [] })`. `PAR013` therefore
resolves to the root file at line 1, column 1, exactly like today's legacy
output.

**Empty prepared buffer from non-empty source.** Conditional compilation can
remove all user code. The prepared buffer may be empty even though the root file
is not. Segments then cover the empty range `[0, 0)` (vacuously), and
`eof_anchor` is a real position derived from the last processed directive (the
end of the last direct/anchor segment), or the root start if no anchor exists.
EOF diagnostics still resolve to a real file and a real line/column.

**EOF token and EOF diagnostics.** The lexer's `Eof` token always has byte
offset `prepared_len`; the bridge yields
`PreparedSpan(prepared_len, prepared_len)`. Parser diagnostics at end of input
(for example `PAR013`, an incomplete block, or a missing closing brace) use the
same zero-length prepared span. Resolution rule:

- `resolve_point(prepared_len)` returns `Ok(eof_anchor)`.
- `resolve_span(PreparedSpan(n, n))` with `n == prepared_len` returns
  `Ok(anchor)` with no related locations.
- A zero-length prepared span that is **not** at the end resolves as a point
  (below).
- Only well-formed EOF ranges take this path. A malformed range is `Err`, never
  the anchor.

**Point resolution and segment boundaries.** Segments are half-open
`[start, end)`. For `resolve_point(offset)`:

1. If `offset > prepared_len`, return `Err(SpanError::OutOfBounds)`.
2. If `offset` is not a UTF-8 character boundary in the prepared text, return
   `Err(SpanError::NotCharBoundary)`.
3. If `offset == prepared_len`, return `Ok(eof_anchor)`.
4. Otherwise, return the origin of the unique segment with
   `prepared.start <= offset < prepared.end`. An offset exactly at a boundary
   belongs to the segment that **starts** at that offset (right-biased). This
   makes boundary resolution deterministic.
5. If no such segment exists, return `Err(SpanError::UnmappedPreparedRange)` or
   `Err(SpanError::MapInvariant)`. Never fall back to the EOF anchor and never
   panic.

For a zero-length `PreparedSpan(n, n)` with `n < prepared_len`, resolution uses
`resolve_point(n)` and returns `Ok(primary = resolve_point(n)?, related = [])`.

**Span crossing several segments.** A prepared span may cross any number of
segment boundaries (a macro argument followed by macro body, then by direct
code). `resolve_span` must:

1. Split the prepared range at every segment boundary into 1..N contiguous
   prepared runs. A zero-length span yields one point run.
2. Resolve every run through its segment origin to an original `SourceSpan`
   (walking the origin chain to the nearest real user span for `Generated`,
   `TokenPaste`, and `Stringify` links).
3. Choose `primary` by origin priority: `Direct` > `MacroArgument` >
   `MacroDefinition` > `Include` > `TokenPaste`/`Stringify` > `Generated`.
   Ties are broken by the leftmost prepared run. This ensures a diagnostic that
   starts in pasted text but continues into direct code still points at the
   user-written code when one exists.
4. Put the remaining distinct spans into `related`, ordered left-to-right by
   their prepared run and deduplicated by `(source, start, end)`.
5. Never return a prepared coordinate or a synthetic file. If every run is
   generated, every origin still points at a real anchor, so `primary` remains a
   real user location.
6. If a segment's origin chain is broken, or the segment table has a hole or an
   overlap, return `Err(SpanError::MapInvariant)`. A malformed map is never
   repaired by pointing at the EOF anchor.

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
| `start > end` in a prepared range | `SpanError::InvertedRange` | `PreparedSpan` constructor, `resolve_span` |
| `start > end` in an original range | `SpanError::InvertedRange` | `SourceMap::source_span`, `location` |
| `end > prepared_len` | `SpanError::OutOfBounds` | `PreparedSpan` constructor, `resolve_span` |
| `end > source_file.len()` | `SpanError::OutOfBounds` | `SourceMap::source_span`, `location` |
| prepared offset splits UTF-8 | `SpanError::NotCharBoundary` | `PreparedSpan` bridge / `resolve_point` |
| original offset splits UTF-8 | `SpanError::NotCharBoundary` | `SourceMap::source_span`, `location` |
| `SourceId` not registered in the unit | `SpanError::UnknownSourceId` | `location`, `source_span` |
| prepared range has no segment | `SpanError::UnmappedPreparedRange` | `resolve_span`, `resolve_point` |
| segments overlap, are unsorted, or an origin chain is broken | `SpanError::MapInvariant` | `resolve_span`, `resolve_point` |

Categories:

- **Correct EOF and zero-length ranges are not errors.** `PreparedSpan(len,
  len)` and `PreparedSpan(n, n)` for `n < len` resolve to `Ok`. `SourceSpan`
  zero-length points are valid original positions.
- **Malformed coordinates are internal errors.** They indicate a builder,
  adapter, or engine bug. Debug builds `debug_assert!` on the invariant; release
  builds propagate `Err`. The unified layer records one internal-invariant
  diagnostic (code and family fixed in Phase 1 and documented), drops only the
  affected finding, and continues. It never panics and never substitutes the
  EOF anchor.
- **User preprocessor errors are diagnostics, not `SpanError`s.** An unknown
  directive, bad `#if` expression, missing include, or recursive macro keeps a
  valid `SourceSpan` and becomes a `PreprocessDiagnostic`. It is not an invalid
  coordinate and does not take the `Err` path.

The public API is `Result`-based rather than `Option`-based on purpose: a
caller must not be able to `unwrap` a missing location on untrusted input.
`SpanError` implements `Display` and `std::error::Error` so the CLI can log it
without a dependency.

### 7.4 Lines and columns

- Line indices are built per file from the file's own bytes. `\n` increments the
  line; `\r` is ignored for line counting, matching the lexer's current
  behavior.
- Columns count Unicode scalar values from the beginning of the line, starting
  at 1, matching the lexer's current implementation.
- The Source Manager owns each file's line index.
  `SourceMap::location(SourceSpan)` consults that index on demand; byte offsets
  remain the canonical identity and line/column are derived data. Invalid
  source spans return `Err(SpanError)`, never a guessed position. Preprocessor
  diagnostics use this path directly, without any prepared-buffer lookup.
- The lexer's `Span.line`/`Span.column` describe the prepared buffer. They are
  used for internal ordering and debugging; rendered diagnostics use the
  original file location via `SourceMap::render_span`.

### 7.5 Mapping model

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

### 7.6 Multiple related locations and unified input

`RelatedLocation` is defined once in §7.1. A finding keeps its primary location
separate from its notes.

Input variants and their resolution paths:

| Variant | Primary coordinates | Resolution |
| --- | --- | --- |
| `Preprocess(PreprocessDiagnostic)` | `SourceSpan` in an original file | `SourceMap::location` / `render_span` directly; **never** `resolve_span` |
| `Prepared(PreparedDiagnostic)` | `Span` in the prepared buffer | `PreparedSource::prepared_span` → `resolve_span` → `render_span` |
| `Legacy(Diagnostic)` | original coordinates, plain-LSL path | pass-through, no map |

Rules:

- `PreprocessDiagnostic.primary` is already an original `SourceSpan` because the
  directive, macro definition, or include site may have been removed from the
  prepared buffer. Resolving it through prepared coordinates would be
  impossible or wrong.
- `PreparedDiagnostic.span` is exactly the frozen `Span` produced by the lexer
  or an engine over the prepared buffer. It is bridged explicitly and then
  resolved. A diagnostic that crosses several segments follows the split and
  priority rules in §7.3.
- `Legacy` is only valid when preprocessing is disabled and the unit has no
  map; its `span` is passed through unchanged, preserving baseline output.
- `related` is ordered from most to least relevant: macro definition, include
  site, outer invocations. Each related span is resolved by the same rule as its
  parent variant (direct `location` for `Preprocess`, prepare/resolve for
  `Prepared`), deduplicated by `(source, start, end)`, and never replaces the
  primary.
- The rendered output is a `RenderedDiagnostic` in which `diagnostic.span` is
  the frozen `Span` shape in original coordinates and `source` is the original
  file for CLI grouping. The existing `Diagnostic` type is not changed.
- JSON keeps the existing object shape for the primary location. Related
  locations are added as an optional `related` array in Phase 3 with a
  documented schema note; existing fields never change meaning.
- An `Err` from bridging or resolution follows §7.3.1: internal diagnostic,
  affected finding dropped, run continues, never a panic or EOF fallback.

### 7.6.1 Worked examples: where each primary location comes from

| Case | Kind | Primary location | Related locations | Resolution path |
| --- | --- | --- | --- | --- |
| Unknown directive, e.g. `#frobnicate` | `PreprocessDiagnostic` | The directive range in the file that contains it | none | Direct `SourceSpan`; no map lookup |
| Malformed `#if` expression, e.g. `#if (1 +` | `PreprocessDiagnostic` | The expression range after `if`; if the expression is empty, a zero-length point at the directive end | The opening directive range | Direct `SourceSpan` |
| Unterminated conditional block | `PreprocessDiagnostic` | The opening `#if`/`#ifdef`/`#ifndef` directive range (the actionable place) | A zero-length point at end of the containing file | Direct `SourceSpan` |
| Missing include, e.g. `#include "missing.lsl"` | `PreprocessDiagnostic` | The path spelling range inside the directive (`missing.lsl`) | The full directive range | Direct `SourceSpan` |
| Recursive macro, e.g. `#define A A` then `A` | `PreprocessDiagnostic` | The invocation that recursed (the `A` occurrence) | The macro definition body; outer invocation if nested | Direct `SourceSpan` |
| EOF diagnostic after all code was removed by `#if 0 ... #endif` | Engine `PAR013` | The map's EOF anchor: a real zero-length position at the end of the last directive/anchor (or root start) | none | `PreparedSpan(len, len)` → `resolve_span` → `eof_anchor` |

Why the paths differ:

- Preprocessor findings are attached to source text that may no longer exist in
  the prepared buffer. Their `SourceSpan` is recorded while scanning the
  original file, so there is nothing to resolve and no prepared range to use.
- The EOF case is an engine finding: the parser only sees the prepared buffer,
  produces a zero-length prepared span at its end, and the map translates it to
  a real file position. This is the only way `PAR013` can be rendered when
  conditional compilation removed everything.
- If the directive appears inside an include, the primary `SourceId` is that
  included file, not the root; the include site becomes a related location.

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
- Resolution is total for every well-formed range in
  `[0, prepared_len]`, including empty buffers, EOF, and zero-length ranges.
  Malformed ranges return `SpanError`, are handled once at the unified layer,
  and never fall back to the EOF anchor.
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
| Span bridge / map | Inverted, out-of-bounds, non-UTF-8, unknown `SourceId`, map hole, broken segments | `SpanError`; internal-invariant diagnostic; only the affected finding is dropped; no panic, no EOF fallback | 1 |
| Lexer | Unterminated string/comment, unsupported character | `LEX` diagnostic with prepared `Span`, tokens recovered | 1 |
| Parser | Syntax error | `PAR` diagnostic with prepared `Span`, bounded recovery | 1 |
| Semantic | Rule violation | `SEM` diagnostic with prepared `Span` | 1 |
| Any stage | Internal invariant violation | `SpanError` plus §7.3.1 handling (drop the affected finding, emit an internal diagnostic); never a fallback location, `panic!`, `unwrap`, or unchecked slicing on user input | 1 |
| Both engines | `compare` mode disagreement | Development/CI failure only; never a user-facing panic | n/a |

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
  - **malformed internal coordinates** — `SpanError`, internal diagnostic,
    affected finding dropped, run continues;
  - **user preprocessor errors** — `PreprocessDiagnostic` with a valid original
    `SourceSpan`; they never take the `SpanError` path.
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
    the bridge to `PreparedSpan` is explicit and checked. Existing callers of
    `lex`/`parse` never see `PreparedSpan` unless they opt into the new API.
11. **New diagnostic types are additive.** `PreprocessDiagnostic`,
    `PreparedDiagnostic`, `UnifiedDiagnosticInput`, `RenderedDiagnostic`, and
    `SpanError` are new exports. `Diagnostic` is not modified and remains the
    rendered single-location type.

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
- Sorting of diagnostics uses a total, stable key: `(unit input order, source
  load order inside the unit, start offset, end offset, code, message)`.
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
| Source files per AnalysisUnit | 256 | Source Manager |
| Include depth | 64 | Preprocessor include stack |
| Macro expansion depth | 64 | Preprocessor expander |
| Expanded bytes/tokens per unit | 8 MiB / 1,000,000 tokens | Preprocessor |
| Origin chain depth | 64 | Source Map |
| Preprocessor diagnostics | 1,000 | Preprocessor |
| Total diagnostics | existing behavior; cap in Phase 3 | Unified layer |
| AST recursion depth | 256 nodes (with explicit guard) | AST parser |
| Control-flow analysis | existing legacy caps (tokens/nodes/edges/depth) | AST semantic analyzer |

Requirements:

- Exceeding a limit produces a single, specific `PRE`/`AST` diagnostic and stops
  the affected expansion; the rest of the unit continues.
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
   real file and a valid offset; identity mapping round-trips byte offsets; line
   and column match the lexer's semantics, including CRLF and multi-byte
   characters; `PreparedSpan`/`SourceSpan` separation is enforced by the type
   system. Mandatory cases: empty source file, empty prepared buffer,
   `Eof`/`PAR013` resolution, zero-length spans, points exactly on segment
   boundaries, and spans crossing several segments.
2a. **Span bridge tests**: `PreparedSource::prepared_span` and
   `PreparedSpan::try_from_lexer` accept every lexer span in a valid prepared
   buffer and preserve byte offsets; they reject inverted ranges, ranges past
   `prepared_len`, and offsets inside a multi-byte UTF-8 scalar with the
   documented `SpanError`; `Span.line`/`column` are ignored by the bridge. The
   API has no `From`/`Into` impl (confirmed by review and, where practical, a
   compile-fail fixture without adding a dependency).
2b. **Invalid-coordinate tests**: the §7.3.1 matrix is exercised directly:
   inverted prepared/original ranges, out-of-bounds prepared/original ranges,
   non-boundary offsets, unknown `SourceId`, unmapped prepared ranges, and
   deliberately corrupted segment tables. Every case returns the expected
   `SpanError`, never panics, and never returns the EOF anchor. Valid EOF,
   zero-length, and empty-buffer cases return `Ok`.
3. **AnalysisUnit isolation tests**: two roots in one CLI invocation must not
   share macros, conditional-stack state, include guards, `SourceId` namespaces,
   or diagnostics; changing one file's `#define` must not change the other
   file's output; order of units must not change results.
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
   temporary directories. Unified diagnostics tests must feed all three
   `UnifiedDiagnosticInput` variants through one merge and assert: identical
   legacy output for the `Legacy` path, direct resolution for `Preprocess`, and
   bridge+resolve for `Prepared`, including mixed ordering and deduplication.
7. **Golden corpus**: existing `corpus/{small,medium,large,invalid}` plus a new
   preprocessor corpus. Plain-LSL golden results must not change.
8. **Differential Legacy↔AST harness** (Phase 4+): compare normalized
   diagnostics as a multiset of `(code, severity, source, start, end)`, plus a
   per-code report. The harness calls the public `parse_legacy`/`parse_ast` and
   both semantic entry points independently; neither engine is a wrapper around
   the other. Order is compared only through the documented deterministic
   policy.
9. **Fallback/fuzz-style tests without dependencies**: deterministic generators
   over token/directive alphabets, bounded by iteration counts, run in normal
   `cargo test`.

### Requirements

- Every new diagnostic code has at least one positive and one boundary test.
- Every phase adds tests for its resource limits and malformed-input behavior.
- No test uses the network, sleeps, or depends on host locale/time zone.
- Tests must pass on MSRV 1.75; Phase 1+ should add an MSRV check step to the
  experimental workflow (separate from the required quality steps).
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
3. **Shadow comparison.** In `compare` mode, run both engines on the same
   prepared tokens, resolve both results, and compare normalized multisets.
   Differences are reported as a development artifact, never as user findings.
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
6. **Ordering policy.** Unified output is sorted deterministically, so rule
   migration does not reshuffle user output. Legacy-only mode keeps the current
   pass order to avoid breaking existing golden tests.
7. **No duplicate findings.** When both engines run for a user-visible mode, the
   merge layer must deduplicate by `(code, primary span)`. Duplicate detection is
   part of the Phase 6 acceptance criteria.
8. **Feature gating.** AST engine selection is a CLI/config option in the
   experimental branch. Default stays legacy until Phase 6 stabilization.
9. **Rollback.** The legacy parser and semantic analyzer remain compiled,
   public, and selectable through the end of Phase 6; removal requires a
   separate decision and a major-version boundary. Legacy differential testing
   must remain possible until that decision.
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

### Phase 1 — Source Manager, Source Map, Identity Preprocessor

- **Goal:** make multi-file provenance and line resolution real without changing
  analysis.
- **Functionality:**
  - `SourceId`, `SourceFile`, `PreparedSpan`, `SourceSpan`, `SourceLocation`,
    `SourceOrigin`, `SourceMap` with `Result`-based `resolve_span`,
    `resolve_point`, `location`, `render_span`, `source_span`, and
    `eof_anchor`; `SpanError` with the §7.3.1 variants;
  - `PreparedSource::prepared_span` and `PreparedSpan::try_from_lexer` as the
    only `Span -> PreparedSpan` bridge;
  - additive diagnostic types (`PreprocessDiagnostic`, `PreparedDiagnostic`,
    `UnifiedDiagnosticInput`, `RenderedDiagnostic`) and their resolution rules,
    even though real `PRE` findings start in Phase 2;
  - `AnalysisUnit` with unit-local `SourceId` namespace, map, diagnostics, and
    preprocessor state placeholders (macro table/conditional stack are empty
    until Phase 2);
  - per-file line index with lexer-compatible columns;
  - Source Manager with root/stdin loading and deterministic per-unit IDs;
  - identity preprocessor producing a byte-identical prepared buffer, a single
    identity segment, and an EOF anchor;
  - `PreparedSource` and the unified input/rendered diagnostic API (no CLI
    change yet).
- **Modules:** `src/source/{mod.rs,file.rs,line_index.rs,map.rs,manager.rs,unit.rs}`,
  `src/preprocessor/{mod.rs,identity.rs,config.rs,diagnostics.rs}`,
  `src/diagnostic/resolve.rs`, `src/pipeline.rs` (unit orchestration).
- **Dependencies:** Phase 0/0.1/0.2 contracts.
- **Acceptance criteria:**
  - identity prepared text equals input bytes for all fixtures;
  - map covers `[0, prepared_len)`, segments sorted and non-overlapping;
  - `resolve_span`/`resolve_point` return `Ok` for every well-formed range,
    including empty buffers, EOF (`prepared_len`), zero-length ranges, boundary
    offsets, and cross-segment ranges; malformed ranges return the documented
    `SpanError` and never fall back to the EOF anchor;
  - the `Span -> PreparedSpan` bridge rejects inverted, out-of-bounds, and
    non-UTF-8-boundary spans; there is no implicit conversion;
  - `UnifiedDiagnosticInput` accepts a synthetic `PreprocessDiagnostic` with an
    original `SourceSpan`, a `PreparedDiagnostic` bridged through the map, and a
    `Legacy(Diagnostic)` passed through unchanged;
  - `PAR013` on an empty source resolves to root `(0, 0)`, line 1, column 1;
  - a `PreprocessDiagnostic` never calls `resolve_span`; its primary is the
    original `SourceSpan` (verified with the §7.6.1 example shapes);
  - every offset resolves to the same line/column as the current lexer for
    ASCII, UTF-8, CRLF, and empty files;
  - creating two AnalysisUnits in one run keeps their `SourceId` namespaces and
    diagnostics isolated;
  - no public legacy API change; `parse_legacy` exists as a thin explicit
    wrapper while `parse` keeps its exact behavior;
  - no new dependencies; MSRV 1.75.
- **Tests:** unit tests for map construction/lookup, identity round-trip,
  UTF-8/CRLF columns, empty source, empty buffer, EOF/PAR013, zero-length and
  cross-segment spans, boundary points, multi-unit isolation, the full
  malformed-coordinate matrix from §7.3.1, the `Span` bridge, and mixed
  `UnifiedDiagnosticInput` handling (must not panic).
- **Done when:** the full suite passes and identity/EOF/unit-isolation paths are
  covered by contract tests.

### Phase 2 — Preprocessor core

- **Goal:** C-like conditional compilation and object/function macros producing
  mapped prepared text.
- **Functionality:** `#define`, `#undef`, `#if`, `#ifdef`, `#ifndef`, `#elif`,
  `#else`, `#endif`; object-like and function-like macros; macro invocation
  with arguments; integer expression evaluator with defined precedence and
  overflow behavior; `PreprocessDiagnostic` for every `PRE` finding with direct
  original `SourceSpan`s; expansion limits; source map segments for invocations,
  definitions, and arguments.
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

### Phase 3 — Includes, advanced macros, Firestorm built-ins, CLI integration

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

### Phase 4 — AST parser

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

### Phase 5 — Firestorm extensions

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
    resolve.rs            Span bridge use, UnifiedDiagnosticInput -> RenderedDiagnostic
  lexer.rs                tokenizer only; no directive knowledge
  parser.rs               legacy token parser: parse_legacy + frozen parse alias
  semantic.rs             legacy semantic analyzer (frozen behavior)
  builtins.rs             vendored LSL metadata (unchanged)
  config.rs               lint configuration (extended additively later)

  source/
    mod.rs                re-exports
    file.rs               SourceId, SourceFile, kinds, text ownership
    line_index.rs         byte offset -> line/column (lexer-compatible)
    map.rs                PreparedSpan, SourceSpan, SourceOrigin, SourceMap,
                          SpanError, segments, EOF anchor, checked resolve_*
    manager.rs            per-unit loading, path policy, cycles, limits
    unit.rs               AnalysisUnit: per-root namespace, map, diagnostics

  preprocessor/
    mod.rs                prepare(), PreprocessConfig, PreparedSource,
                          prepared_span Span bridge
    identity.rs           Phase 1 identity pass-through
    directive.rs          Phase 2 raw-text directive scanner
    macros.rs             macro table, definitions, expansion
    expr.rs               #if expression evaluator
    include.rs            Phase 3 include expansion
    builtins.rs           Phase 3 predefined macros / profile built-ins
    diagnostics.rs        PreprocessDiagnostic constructors, PRE codes

  ast/
    mod.rs                AST root and re-exports
    node.rs               node types with spans
    parser.rs             AST parser and recovery: independent parse_ast
    ext.rs (Phase 5)      Firestorm-only nodes

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
  immutable `Arc`. `SourceId` values never cross unit boundaries.
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
  `Prepared`, and pass-through for `Legacy`. It is the only place that builds
  `RenderedDiagnostic`.
- **`Span` bridge** lives with `PreparedSource` (it needs the prepared text for
  UTF-8 validation) and returns `PreparedSpan`; no other module may convert
  `Span` to prepared coordinates.

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
  (`PreparedSource::prepared_span`), valid only for spans produced from the
  prepared buffer.
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
- **AnalysisUnit:** the independent analysis of one root file or stdin stream;
  owns its `SourceId` namespace, macro table, conditional stack, include graph,
  `SourceMap`, and diagnostics.
- **Prepared source / prepared buffer:** the single UTF-8 text produced by the
  preprocessor and consumed by the lexer. It is not a `SourceFile`.
- **Prepared span (`PreparedSpan`):** a byte range in the prepared buffer,
  with no `SourceId`.
- **Source span (`SourceSpan`):** a byte range in an original root/include file,
  always with a real `SourceId`.
- **EOF anchor:** a real original-file `SourceSpan` used for empty prepared
  buffers and zero-length spans at the end of input.
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
