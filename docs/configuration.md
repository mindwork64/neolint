# Configuration

Neolint uses a small line-oriented configuration format. It is not TOML, YAML, or JSON.

```ini
# Ignore rules that are not useful for this script
disable = SEM001, SEM003

# Keep the diagnostic but make it non-failing
severity.SEM004 = warning
```

Blank lines and text after `#` are ignored. Leading and trailing whitespace is ignored.

## Settings

| Setting | Value |
| --- | --- |
| `disable = RULE[, RULE...]` | Rule codes to remove from output. |
| `severity.RULE = error\|warning\|info` | Override a rule's severity. |
| `preprocess = true\|false` | Enable/disable full preprocessing. |
| `dialect = lsl\|firestorm` | Select the preprocessing dialect; `firestorm` enables preprocessing. |
| `include_dir = PATH` | Add an include search directory (repeatable). |

Rule codes must contain only uppercase ASCII letters and digits. Unknown settings, malformed lines, invalid severity names, and invalid rule codes cause a configuration error and exit code 2.

Multiple `severity.*` lines, multiple `disable` lines, and multiple `include_dir` lines are supported. The configuration is applied to diagnostics before CLI overrides. Include directories preserve file order; quoted includes search the including file's directory first. `#pragma once` is a neolint convenience, not a verified Firestorm guarantee.

## Precedence

1. Default diagnostic severity.
2. Configuration-file `disable` and `severity.*`.
3. Command-line `--disable`, `--severity`, `--dialect`, `--preprocess`, and `-I`/`--include-dir`, applied afterward.

The command line wins for the same rule. A non-empty CLI include-directory list replaces the configured list; CLI `--dialect` overrides the configured dialect. A disabled rule cannot produce output or affect the exit code, even if it also has a severity override. A severity of `warning` or `info` does not cause exit code 1 unless changed to `error`.

## Examples

```sh
neolint --config .neolint.conf hello.lsl
neolint --disable SEM001,SEM004 --config .neolint.conf hello.lsl
neolint --severity SEM012=error hello.lsl
neolint --preprocess -I ./includes hello.lsl
neolint --dialect firestorm hello.lsl
```

Invalid configuration:

```ini
severity.SEM004 = fatal
```

This is rejected with exit code 2 and a message identifying the line.
