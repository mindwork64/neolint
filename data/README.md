# Vendored LSL definitions

## Built-in prototypes

The file `lsl-builtins.txt` is copied from the official generated builtin prototype list:

- Upstream: https://github.com/secondlife/lsl-definitions
- Source path: `generated/builtins.txt`
- Pinned upstream revision: `eb8354a3984ea31123ea3f69259ed99b9edb3f05`
- Upstream blob SHA: `2c5735c774e88d9e6c93e15c92bd5b9ec9b432aa`
- Snapshot date: 2026-11-20

It contains prototypes for LSL functions, event handlers, and constants.

## Deprecation metadata

The file `lsl-deprecated.txt` lists functions whose canonical LSL definition has a `deprecated` property:

- Upstream source: `lsl_definitions.yaml`
- Pinned upstream revision: `eb8354a3984ea31123ea3f69259ed99b9edb3f05`
- Extractor: `scripts/extract-lsl-deprecated.py`

The extractor intentionally ignores `slua-deprecated`. It also does not export the upstream `use` hints because they may name SLua APIs and must not be presented as LSL replacements.

## License and updates

The upstream repository is distributed under the MIT License. See `LICENSES/LINDEN-RESEARCH-MIT.txt` and retain this attribution when redistributing the snapshots.

Normal neolint execution reads checked-in snapshots and does not access the network. Updating both files is an explicit maintainer action:

```sh
bash scripts/update-lsl-builtins.sh
```

An optional 40-character upstream commit SHA can be passed to update both snapshots from the same revision.
