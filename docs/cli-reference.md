# CLI reference

```text
neolint [OPTIONS] <FILE|DIRECTORY...|->
```

At least one input is required. With no input, neolint prints help and exits with code 2.

## Options

| Option | Meaning |
| --- | --- |
| `-h`, `--help` | Print usage and exit successfully. |
| `-V`, `--version` | Print `neolint <package-version>` and exit successfully. |
| `--format human` | Human-readable diagnostics; the default. |
| `--format json` | Emit a JSON array of diagnostic objects. |
| `--config FILE` | Load rule settings from a configuration file. |
| `--disable RULES` | Disable comma-separated rule codes. |
| `--severity RULE=LEVEL` | Override severity for a rule; `LEVEL` is `error`, `warning`, or `info`. |
| `--tokens` | Print lexer tokens before human diagnostics. Cannot be combined with `--format json`. In preprocessed mode these are tokens of the prepared root buffer. |
| `--preprocess` | Enable the standard LSL preprocessor. |
| `--dialect lsl\|firestorm` | Select the preprocessing dialect. `firestorm` enables preprocessing. |
| `--engine ENGINE` | Select the analysis engine: `legacy` (default), `ast`, or `compare`. |
| `-I DIR`, `--include-dir DIR` | Add an include search directory (repeatable, order preserved). |
| `--include-dir=DIR`, `--dialect=DIALECT`, `--engine=ENGINE` | Equals forms of the corresponding options. |

Options may be repeated where meaningful. `--disable` and `--severity` values accumulate. Configuration-file settings are applied first; command-line `--disable` and `--severity` are applied afterward and therefore take precedence for the same rule. CLI `--dialect` overrides the configured dialect, CLI `--preprocess` enables preprocessing, CLI `--engine` overrides the configured engine, and a non-empty CLI include-directory list replaces the configured list.

Without preprocessing flags, plain-LSL behavior is unchanged. Include directories require `--preprocess` or `--dialect firestorm`; otherwise the CLI exits with code 2. `--tokens` in prepared mode shows tokens from the prepared root buffer, not the included files separately.

`--tokens` cannot be combined with `--format json`; that combination exits with code 2.

## Engines

| Engine | Behavior |
| --- | --- |
| `legacy` | Default. The frozen Legacy lexer/parser/semantic pipeline. Output is unchanged from previous releases. |
| `ast` | The independent typed-AST parser and AST semantic analyzer. |
| `compare` | Runs both analyzers, displays the Legacy stream, and writes a semantic comparison report to stderr. |

Without `--preprocess`, `ast` and `compare` use identity preparation: the source bytes are copied unchanged into a prepared buffer so the engine API has a source map. Identity preparation is not preprocessing; macros and includes are not expanded. With `--preprocess` (or `--dialect firestorm`), every engine uses the full preprocessor and dialect.

In `compare` mode, stdout keeps the existing human/JSON diagnostic schema and contains only the Legacy display stream. The AST semantic stream is not merged into user output. The comparison report always goes to stderr, one deterministic section per unit in unit order, and includes per-rule counts plus bounded exact missing/extra fingerprints. With `--format json`, stdout remains the existing diagnostics array. On a successful run stderr receives a single JSON object: `{"engine":"compare","units":[…],"matched":true|false}`. Consumers must gate on the exit status: per-unit prepare or analyze failures print `neolint: …` error lines before the report, and the report is absent only on early fatal CLI/config/input failures or when no comparison ran. Stderr is therefore not guaranteed to be pure JSON.

A bounded detail list and an incomplete comparison are different states. Detail truncation (`details_truncated = true`) occurs only when a direction has more than 64 missing/extra findings; the report then shows the first 64 while totals stay exact. An incomplete comparison means an engine produced more than 100,000 `SEM001`–`SEM023` fingerprints, so the comparator performs no multiset comparison at all: per-rule `legacy_count`/`ast_count` stay exact, but `missing_total`/`extra_total` are unknown and render as `?` in human output and `null` in JSON (never `0`). Such a unit sets `comparison_complete = false`, can never be reported as matched, and exits 3 (unless an input failure already requires exit 2).

```sh
neolint --engine compare --format json script.lsl
```

## Preprocessing and configuration

```text
preprocess = true
dialect = firestorm
include_dir = ./includes
```

- `preprocess` is `true` or `false`.
- `dialect` is `lsl` or `firestorm`; `firestorm` enables preprocessing.
- `include_dir` may appear repeatedly and preserves file order.

Quoted includes search the including file's directory first, then the include directories in order; angle includes search only the include directories. URL, `http(s)`, `ftp`, and UNC/network paths are rejected. `#pragma once` is a documented neolint convenience, not a guaranteed Firestorm capability. `//fspreprocessor off` is a Firestorm-only script-wide kill switch and emits an informational `PRE037`; it does not affect the exit code.

## Inputs

- One or more explicit file paths are accepted as supplied, regardless of extension.
- A directory is scanned recursively for files whose extension is `.lsl`, case-insensitively.
- Directory traversal is deterministic: entries are processed in sorted name order.
- Symbolic links encountered during recursive directory traversal are skipped.
- Duplicate paths are de-duplicated by their displayed path.
- `-` reads stdin and may be specified only once. Do not combine multiple stdin entries.
- Inputs that fail path/metadata access cause exit code 2 before linting. If a later file cannot be read, already collected results are still printed, and the final exit code is 2.

## Human output

Clean file:

```text
hello.lsl: no diagnostics
```

Diagnostic:

```text
hello.lsl:3:9: error[SEM004]: llOwnerSay argument 1 has type integer, expected string
```

The prefix is `path:line:column: severity[code]: message`. Lines and columns are one-based.

## JSON output

The top-level value is always an array. A clean run produces `[]`. Each diagnostic object has these fields:

| Field | Meaning |
| --- | --- |
| `file` | Input path as displayed by the CLI. |
| `code` | Stable diagnostic code, such as `SEM004`. |
| `severity` | `error`, `warning`, or `info`. |
| `kind` | `lexical`, `syntax`, `semantic`, or `lint`. |
| `message` | Human-readable diagnostic text. |
| `line` | One-based start line. |
| `column` | One-based start column. |
| `end_line` | One-based end line. |
| `end_column` | One-based end column. |
| `start` | Zero-based UTF-8 byte offset of the span start. |
| `end` | Zero-based UTF-8 byte offset of the span end. |

Example:

```sh
neolint --format json corpus/invalid/duplicate_event.lsl
```

```json
[{"file":"corpus/invalid/duplicate_event.lsl","code":"SEM020","severity":"error","kind":"semantic","message":"duplicate event handler 'state_entry' in state 'default'; first declared at 3:5","line":8,"column":5,"end_line":8,"end_column":16,"start":74,"end":85}]
```

`start`/`end` are byte offsets, not display columns. Columns are one-based character positions as reported by the lexer.

The default plain no-preprocess Legacy path is uncapped: it does not apply the 10,000-diagnostic invocation limit and never appends `PRE039`. Unit-based paths (`--preprocess` or `--dialect firestorm`, both enabling preprocessing, and identity-prepared `--engine ast`/`--engine compare`) apply the per-unit 10,000 terminal cap and then the invocation-wide 10,000 cap. On invocation overflow the CLI removes any per-unit terminal `PRE039`, truncates later results, and appends exactly one terminal `PRE039` (error severity; exit code 1 unless an input error already requires 2, or an unmatched Compare report returns 3).

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | No error-severity diagnostics. Warnings and info diagnostics do not fail the run. |
| `1` | At least one error-severity diagnostic remains after configuration. |
| `2` | Command-line, input, configuration, or file-read error. |
| `3` | Compare mode only: at least one unit's semantic comparison is unmatched. |

Configured severity changes affect exit status. For example, `--severity SEM012=error` makes a warning-producing run exit 1. Exit code `3` takes precedence over `1` when a Compare report is unmatched, but never over `2`: input, configuration, file-read, and preparation failures still exit 2. A matching Compare report with displayed Legacy error diagnostics exits 1, and a matched report with no errors exits 0.

## Examples

```sh
neolint hello.lsl
neolint --format json hello.lsl
neolint one.lsl two.lsl
neolint ./scripts/
neolint --config .neolint.conf ./scripts/
neolint --disable SEM001,SEM004 ./scripts/
neolint --severity SEM004=warning hello.lsl
cat hello.lsl | neolint -
neolint --tokens hello.lsl
neolint --preprocess -I ./includes script.lsl
neolint --dialect firestorm script.lsl
neolint --engine ast script.lsl
neolint --engine=compare --format json script.lsl
```

See [configuration.md](configuration.md), [diagnostics.md](diagnostics.md), and [firestorm-preprocessor.md](firestorm-preprocessor.md).
