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
| `--tokens` | Print lexer tokens instead of the normal human diagnostic output. Development aid. |

Options may be repeated where meaningful. `--disable` and `--severity` values accumulate. Configuration-file settings are applied first; command-line `--disable` and `--severity` are applied afterward and therefore take precedence for the same rule.

`--tokens` cannot be combined with `--format json`; that combination exits with code 2.

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

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | No error-severity diagnostics. Warnings and info diagnostics do not fail the run. |
| `1` | At least one error-severity diagnostic remains after configuration. |
| `2` | Command-line, input, configuration, or file-read error. |

Configured severity changes affect exit status. For example, `--severity SEM012=error` makes a warning-producing run exit 1.

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
```

See [configuration.md](configuration.md) and [diagnostics.md](diagnostics.md).
