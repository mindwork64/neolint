# Getting started

Neolint is a linter for Linden Scripting Language (LSL), with a command-line interface and a VS Code language server. It does not replace the official Second Life compiler; a clean lint result does not guarantee compiler or simulator acceptance.

## Supported binary targets

The downloadable release targets are:

- Linux x86_64: `x86_64-unknown-linux-gnu`
- Windows x86_64: `x86_64-pc-windows-msvc`

macOS, ARM, 32-bit Windows/Linux, and other targets are not included in this release.

The v1.1.0 release uses the AST engine by default. To restore the historical v1.0.0 analysis pipeline, pass `--engine legacy` or set `engine = legacy` in the configuration file. `--engine compare` is intended for maintainers investigating differences between the engines.

Stable v1.2.0 is published at <https://github.com/mindwork64/neolint/releases/tag/v1.2.0>. Download the archive for your platform and verify its `.sha256` file before use.

## Install a release (Linux x86_64)

Download the archive matching the current package version and target:

```sh
tar -xzf neolint-v1.2.0-x86_64-unknown-linux-gnu.tar.gz
chmod +x neolint
./neolint --version
```

To put it on `PATH` for the current user:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 neolint "$HOME/.local/bin/neolint"
```

Ensure `$HOME/.local/bin` is on `PATH`, then verify:

```sh
neolint --version
```

## Install a release (Windows x86_64)

In PowerShell, from the directory containing the downloaded archive:

```powershell
Expand-Archive .\neolint-v1.2.0-x86_64-pc-windows-msvc.zip -DestinationPath .
.\neolint.exe --version
```

To add the current directory to `PATH` for the current session:

```powershell
$env:Path += ";$PWD"
neolint --version
```

Persist the directory in the user `PATH` only if it is a stable installation location.

## Verify a download

Download the matching `.sha256` file alongside the archive.

Linux:

```sh
sha256sum -c neolint-v1.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Windows PowerShell:

```powershell
$expected = (Get-Content .\neolint-v1.2.0-x86_64-pc-windows-msvc.zip.sha256).Split()[0]
(Get-FileHash .\neolint-v1.2.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash.ToLower() -eq $expected
```

The checksum file is generated after the archive and refers to the exact uploaded archive.

## First lint run

Create `hello.lsl`:

```lsl
default
{
    state_entry()
    {
        llOwnerSay("Hello");
    }
}
```

Run the linter on one file:

```sh
neolint hello.lsl
```

Run it on a directory (recursive, `.lsl` extension case-insensitive):

```sh
neolint ./scripts/
```

Run with JSON diagnostics:

```sh
neolint --format json hello.lsl
```

Read source from stdin:

```sh
cat hello.lsl | neolint -
```

See [cli-reference.md](cli-reference.md) for every option and [configuration.md](configuration.md) for rule configuration.

The package has no runtime dependencies. Normal linting reads vendored metadata and does not access the network.
