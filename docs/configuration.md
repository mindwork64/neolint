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

Rule codes must contain only uppercase ASCII letters and digits. Unknown settings, malformed lines, invalid severity names, and invalid rule codes cause a configuration error and exit code 2.

Multiple `severity.*` lines and multiple `disable` lines are supported. The configuration is applied to diagnostics before CLI overrides.

## Precedence

1. Default diagnostic severity.
2. Configuration-file `disable` and `severity.*`.
3. Command-line `--disable` and `--severity`, applied afterward.

The command line wins for the same rule. A disabled rule cannot produce output or affect the exit code, even if it also has a severity override. A severity of `warning` or `info` does not cause exit code 1 unless changed to `error`.

## Examples

```sh
neolint --config .neolint.conf hello.lsl
neolint --disable SEM001,SEM004 --config .neolint.conf hello.lsl
neolint --severity SEM012=error hello.lsl
```

Invalid configuration:

```ini
severity.SEM004 = fatal
```

This is rejected with exit code 2 and a message identifying the line.


The separate `neolint scan-diff` subcommand has its own TOML regex-rule file; it does not use `.neolint.conf`. See [Pattern scanner](pattern-scanner.md) for its schema and built-in policy profile.
