# Release engineering

## Supported targets

Initial binary releases target:

| Platform | Rust target | Archive |
| --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `neolint-v<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `neolint-v<version>-x86_64-pc-windows-msvc.zip` |

macOS, ARM, 32-bit, and other targets are not part of the initial release matrix.

## Workflow

The release workflow is [.github/workflows/release.yml](../.github/workflows/release.yml). It is separate from normal PR CI.

It runs:

- automatically for tags matching `v*`;
- manually via `workflow_dispatch`, which builds and tests both platforms without publishing a release.

Each target runs on its native GitHub-hosted runner:

- Ubuntu for Linux x86_64;
- Windows for Windows x86_64.

The workflow builds optimized binaries with the portable Rust stable toolchain and the exact target triple. There is currently no committed `Cargo.lock`, so `--locked` is not used; introduce it only together with a committed lockfile.

## Version and tag check

The package version comes from `cargo metadata`. Before publishing, the workflow requires:

```text
tag == "v" + Cargo.toml package version
```

A mismatch fails the publish job. The workflow does not change the package version.

## Smoke tests

The packaged release binary must pass, on its native runner:

- `--help` exit 0;
- `--version` exact `neolint <version>`;
- a valid corpus fixture exit 0;
- an invalid fixture exit 1 with `SEM020`;
- `--format json` output parseable as JSON with all documented fields.

Linux runs [`scripts/release-smoke.sh`](../scripts/release-smoke.sh); Windows runs [`scripts/release-smoke.ps1`](../scripts/release-smoke.ps1). Compilation alone is not accepted as runtime evidence.

## Packaging and checksums

Linux:

- copy the release binary into a staging directory as `neolint`;
- create the `.tar.gz`;
- generate `<archive>.sha256` with `sha256sum`.

Windows:

- copy `neolint.exe` into a staging directory;
- create the `.zip` with `Compress-Archive`;
- generate `<archive>.sha256` with `Get-FileHash -Algorithm SHA256`.

Checksums are generated after the archive, from the exact file uploaded. Both archives and both checksum files are uploaded as workflow artifacts.

## Publishing

Only the publish job receives `contents: write`; build jobs use `contents: read`.

For a tag push:

1. Both build jobs must succeed.
2. Artifacts are downloaded and expected archive/checksum names are verified.
3. The tag/version check must pass.
4. The workflow refuses to proceed if the GitHub Release already exists, avoiding silent overwrite of assets.
5. `gh release create` attaches the archives and checksum files using the workflow `GITHUB_TOKEN`.

No personal token, repository secret, crates.io publication, or package-manager publication is used.

## Manual dispatch

After the workflow exists on the default branch, maintainers can run **Actions → Release artifacts → Run workflow** on a branch. Manual dispatch builds and smoke-tests both targets, downloads both uploaded artifacts with `actions/download-artifact@v8`, and validates both SHA-256 checksums and archive contents. It does not publish a release, because publish requires a `v*` tag push.

Matrix validation was completed before merge in workflow dispatch [run #6](https://github.com/mindwork64/neolint/actions/runs/44857913971) on head `6c2473126d0db745ea3a5fd79175d1ff566ffebc`: Linux, Windows and the artifact-validation job all passed, and publishing was skipped. After merge, maintainers can dispatch the workflow again on `main` before a public release if a fresh validation is required; manual dispatch never publishes.

## Stable release checklist

- [x] Issue #54 license decision is complete (MIT OR Apache-2.0).
- [x] Issues #55 and #56 are closed by merged work.
- [x] Issue #58 final acceptance is complete.
- [ ] All required CI checks pass on the release commit.
- [ ] Manual release workflow dispatch passes on Linux and Windows.
- [ ] The tag exactly matches the Cargo package version.
- [ ] Checksums are published and independently verified.
- [ ] No release blocker remains.

Do not publish v1.0.0 while any blocker remains or product-owner acceptance is missing.
