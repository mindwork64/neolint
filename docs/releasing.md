# Publishing a NeoLint release

Release publication is deliberately separated from ordinary CI. Pushing a
version tag builds and validates release assets but does not publish a GitHub
Release automatically.

## Prepare a version

1. Start from an up-to-date `main` branch and prepare the release through a
   dedicated pull request.
2. Update the Rust package version in `Cargo.toml`; let Cargo update
   `Cargo.lock`.
3. Update the VS Code extension version independently in
   `editors/vscode/package.json` and the root package entry in
   `editors/vscode/package-lock.json`.
4. Move the completed `Unreleased` entries into a dated version section in
   `CHANGELOG.md`.
5. Check the README, installation guide, and extension instructions for version
   and asset-name consistency.
6. Merge the reviewed release-preparation PR only after CI passes and Codex
   review is clear.

## Build and validate the assets

Create and push the matching version tag only after the preparation PR is
merged and the release owner has explicitly authorized publication:

```sh
git switch main
git fetch origin
git reset --hard origin/main
git tag vX.Y.Z
git push origin vX.Y.Z
```

The tag-triggered workflow builds and validates:

- Linux x86_64 archive containing `neolint`, `neolint-lsp`, project licenses,
  generated Rust dependency notices, and separate vendored-data notices.
- Windows x86_64 archive with the equivalent files.
- VS Code extension VSIX, including the production `vscode-languageclient`
  dependency and extension license files.
- SHA-256 checksums for all three distributable packages.

The validation job checks the tag/package version match, checksum integrity,
archive contents, and VSIX runtime contents. A green run means the assets are
ready; they are not yet attached to a public release.

## Publish the GitHub Release

After the tag-triggered workflow succeeds:

1. Open **Actions → Release artifacts → Run workflow**.
2. Select the same `vX.Y.Z` tag as the workflow ref.
3. Set `publish` to `true` and start the workflow.
4. Wait for the build and validation jobs to pass again. The publish job then
   extracts the matching release section from `CHANGELOG.md` and creates the
   GitHub Release with all archives, checksums, and the VSIX.

The publish job refuses to overwrite an existing release. If a published
release needs a correction, prepare a new version instead of replacing its
assets in place.

## Scope

Marketplace publication is not configured. The VS Code extension is
distributed as a VSIX attached to the GitHub Release. macOS and ARM binaries
are not part of the current release matrix.
