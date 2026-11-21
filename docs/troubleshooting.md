# Troubleshooting

## `neolint: command not found`

The executable is not on `PATH`.

- Linux: place `neolint` in a directory such as `$HOME/.local/bin` and ensure that directory is on `PATH`.
- Windows PowerShell: add the containing directory to `PATH`, then open a new shell:

```powershell
$env:Path += ";C:\path\to\neolint"
neolint --version
```

## Wrong platform archive

Use Linux x86_64 on 64-bit glibc Linux and Windows x86_64 on 64-bit Windows. macOS, ARM, 32-bit targets, and other platforms are not part of the initial binary release.

## Windows extraction or execution issues

Use PowerShell `Expand-Archive`. If execution is blocked by policy, inspect the archive and executable manually; do not assume an installer exists. Neolint does not ship an installer wizard.

## Configuration errors

A configuration file is line-oriented `key = value`, not TOML/YAML/JSON. A malformed line, unknown setting, invalid severity, or invalid rule code produces exit code 2. See [configuration.md](configuration.md).

## Unsupported option

Unknown options produce exit code 2 and a message. `--tokens` cannot be combined with `--format json`.

## Stdin problems

`-` reads stdin and may appear only once. Without piped input, the command waits for stdin. Use `cat script.lsl | neolint -` on Linux or `Get-Content script.lsl | neolint -` on Windows.

## Exit code 1 but output says warning

Warnings and info diagnostics do not fail by default. Exit code 1 means at least one remaining diagnostic has severity `error`. Check `--severity` and configuration overrides.

## Exit code 2

Exit code 2 indicates a command-line, configuration, input, or file-read problem, not a lint finding. Read stderr for the specific path or option.

## Unexpected language diagnostics

Neolint is conservative and intentionally incomplete. See [language-support.md](language-support.md) and [diagnostics.md](diagnostics.md). A clean result is not proof of compiler acceptance.

## Directory scanning did not find a file

Directories are scanned recursively for `.lsl` files only, case-insensitively. Symlinks encountered during recursive traversal are skipped. Pass an explicit file path to lint a file with another extension.
