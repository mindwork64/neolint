# Bundled LSL metadata

Neolint includes bundled definitions for canonical LSL functions, event handlers, constants, and deprecated functions. These definitions are based on the official [Second Life LSL definitions](https://github.com/secondlife/lsl-definitions) project.

The bundled metadata is used to provide built-in signature checks and deprecation warnings. Normal linting uses the copy included with neolint and does not require a network connection.

## OpenSimulator interface snapshots

The OpenSim target catalogues are generated from the public C# API interface declarations. They contain API names, not implementation bodies. Exact source revisions are pinned here so the snapshots can be regenerated and audited:

| Target | Source revision | LSL API names | OSSL API names | Key-to-string aliases |
| --- | --- | ---: | ---: | ---: |
| XEngine (historical) | OpenSim tag `0.9.2.2Dev`, commit `22e9f41502bfe1e437263215a3abaa2d1cb4eea1` | 425 | 231 | 31 |
| YEngine | OpenSim `master`, commit `1e1e033d383a179f73404f021f8ca1af7e53e2de` | 473 | 265 | 32 |

Source paths in the OpenSim repository:

- `OpenSim/Region/ScriptEngine/Shared/Api/Interface/ILSL_Api.cs`
- `OpenSim/Region/ScriptEngine/Shared/Api/Interface/IOSSL_Api.cs`
- `OpenSim/Region/ScriptEngine/Shared/LSL_Types.cs` (OpenSim key-to-String conversion semantics)

The generated `opensim-*-ll-functions.txt` and `opensim-*-ossl-functions.txt` files are sorted name lists. The `opensim-*-ll-key-string-params.txt` files record interface positions where OpenSim declares a C# `string` parameter but the canonical Second Life LSL signature uses `key`. OpenSim's LSL key wrapper provides an implicit conversion to `System.String`; the analyzer applies this target-specific compatibility only to OpenSim. YEngine's three-argument `llLinkPlaySound` overload is represented separately in the call checker.

To regenerate, obtain the four interface files from the pinned revisions, place each runtime's `ILSL_Api.cs` and `IOSSL_Api.cs` in a separate directory, then run:

```sh
python3 scripts/generate_opensim_catalogues.py \
  --xengine-dir /path/to/xengine-interface \
  --yengine-dir /path/to/yengine-interface
```

An OSSL interface entry does not mean a region has enabled that function. OpenSim region security settings, threat levels and permissions control actual availability. Neolint therefore uses an explicit project-side allowlist rather than treating the interface list as a deployment permission list.

## License and attribution

The upstream Second Life definitions are distributed under the MIT License. The applicable notice is included in [`LICENSES/LINDEN-RESEARCH-MIT.txt`](../LICENSES/LINDEN-RESEARCH-MIT.txt). This separate license applies to the bundled Second Life data; neolint's own source code is available under MIT OR Apache-2.0. OpenSim snapshots contain only names and narrowly scoped parameter-alias metadata derived from the cited interfaces; consult the upstream OpenSim repository for its source licensing terms.
