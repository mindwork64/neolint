# Configuration

Neolint uses a small line-oriented configuration format. It is not TOML, YAML, or JSON.

```ini
# Ignore rules that are not useful for this script
disable = SEM001, SEM003

# Keep the diagnostic but make it non-failing
severity.SEM004 = warning

# Select the analysis engine; legacy is the default
engine = legacy
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
| `engine = legacy\|ast\|compare` | Select the analysis engine; `legacy` is the default. |

Rule codes must contain only uppercase ASCII letters and digits. Unknown settings, malformed lines, invalid severity names, invalid rule codes, and invalid engine values cause a configuration error and exit code 2. A configuration file without an `engine` line stays valid and selects `legacy`.

Multiple `severity.*` lines, multiple `disable` lines, and multiple `include_dir` lines are supported. Disabled-rule filtering and severity overrides are applied only to displayed diagnostics, before CLI overrides. Include directories preserve file order; quoted includes search the including file's directory first. `#pragma once` is a neolint convenience, not a verified Firestorm guarantee.

## Precedence

1. Default diagnostic severity and the default `engine = legacy`.
2. Configuration-file `disable`, `severity.*`, `preprocess`, `dialect`, `include_dir`, and `engine`.
3. Command-line `--disable`, `--severity`, `--dialect`, `--preprocess`, `--engine`, and `-I`/`--include-dir`, applied afterward.

The command line wins for the same rule. A non-empty CLI include-directory list replaces the configured list; CLI `--dialect` overrides the configured dialect; CLI `--engine` overrides the configured engine. Rule disabling and severity overrides affect only the displayed diagnostic stream: a disabled rule does not appear in that stream, and a severity of `warning` or `info` does not cause exit code 1 unless changed to `error`. In `--engine compare` mode the parity report is computed from the raw pre-configuration streams, so it can still report a disabled rule and select exit code 3; severity overrides do not change the report's raw severities.

## Examples

```sh
neolint --config .neolint.conf hello.lsl
neolint --disable SEM001,SEM004 --config .neolint.conf hello.lsl
neolint --severity SEM012=error hello.lsl
neolint --preprocess -I ./includes hello.lsl
neolint --dialect firestorm hello.lsl
neolint --engine ast hello.lsl
```

Invalid configuration:

```ini
severity.SEM004 = fatal
```

This is rejected with exit code 2 and a message identifying the line.
