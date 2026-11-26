# LSL and simulator API metadata

NeoLint vendors the canonical Second Life LSL metadata and pinned OpenSimulator
API snapshots. The generated text files are parsed once per process and kept
immutable. Linting does not require network access.

## Canonical Second Life language metadata

The source is the official
[secondlife/lsl-definitions](https://github.com/secondlife/lsl-definitions)
repository, file lsl_definitions.yaml, pinned at:

- Commit: eb8354a3984ea31123ea3f69259ed99b9edb3f05
- Snapshot date: 2026-11-17
- 529 standard LSL functions
- 43 event handlers
- 1,082 constants

data/lsl-builtins.txt is the normalized bundled representation. Mono, LSO,
and LSL compiled for Luau currently share this language-level API catalogue.
Runtime-specific semantic restrictions are applied separately; this is not a
claim that the three compilers behave identically in every edge case.

## OpenSimulator versioned API catalogues

OpenSim profiles are pinned to upstream source commits, not floating branch
names. The catalogue identity is the public OS_APIVERSION value plus the
source revision recorded here.

| Catalogue | OpenSim source | API version | Runtime | LSL functions / signatures | OSSL functions / signatures | Events | Constants |
| --- | --- | ---: | --- | ---: | ---: | ---: | ---: |
| xengine-api20 | tag 0.9.2.2Dev, commit 22e9f41502bfe1e437263215a3abaa2d1cb4eea1 | 20 | XEngine | 425 / 425 | 231 / 258 | 35 | 770 |
| yengine-api20 | tag 0.9.2.2Dev, commit 22e9f41502bfe1e437263215a3abaa2d1cb4eea1 | 20 | YEngine | 425 / 425 | 231 / 258 | 36 | 770 |
| yengine-api22 | tag 0.9.3.0Dev, commit 8569d47c76193f4b6784f7e1f45e5575ab603b0f | 22 | YEngine | 434 / 434 | 231 / 258 | 36 | 780 |
| yengine-api23 | tag 0.9.3.1Dev, commit 24cbfb284a07d0257d6d8ef3879ca59363c64a5e | 23 | YEngine | 457 / 458 | 258 / 289 | 37 | 815 |
| yengine-api24 | master snapshot 2026-11-25, commit a0afffdb088b8dcadc598d152daccbf23dfa650c | 24 | YEngine | 473 / 474 | 265 / 296 | 37 | 888 |

Each catalogue directory contains:

- functions.txt: standard ll* function signatures. Every overload is retained as a separate signature.
- ossl-functions.txt: os* signatures, also retaining all overloads.
- events.txt: accepted event handler signatures for this compiler/runtime.
- constants.txt: public constants, language types, and source values.

XEngine events are derived from that version's own lsl.lexer.cs event tokens
and lsl.parser.cs productions. This source exposes 35 event handlers and
does not include path_update or linkset_data.

YEngine events are derived from MMRScriptEventCode.cs intersected with
MMRIEventHandlers.cs. The region_cross method is not an accepted event-code
entry and is intentionally excluded. linkset_data appears in API23 and
API24, but not API20 or API22.

### Type and overload handling

The generator maps OpenSim C# wrappers to LSL types. For parameters whose C#
interface spelling uses string while the canonical LSL declaration uses key,
the generator preserves the language-level key type; the analyzer also models
OpenSim's key/string conversion in both directions. OpenSim function overloads
are selected by argument count and inferred argument types. If overload return
types cannot be distinguished safely, expression type inference remains
unknown instead of guessing.

The canonical Second Life function catalogue and OpenSim's runtime catalogue
remain separate. SEM047 compares standard SL ll* names against the selected
OpenSim interface. OSSL is not silently folded into standard LSL.

### Target selectors

- opensim-xengine selects XEngine API20 (0.9.2.2); later XEngine API snapshots are not offered.
- opensim-yengine defaults to the pinned API24 snapshot. Use target_version = 0.9.2.2, 0.9.3.0, 0.9.3.1, or master-2026-11-25 to select API20, API22, API23, or API24 respectively.
- XEngine was removed in OpenSim 0.9.3.0; YEngine is the supported runtime from that release line onward.

### Regenerating the snapshots

The generator uses only the Python standard library. Prepare one source
directory per catalogue, containing the C# files listed below.

Common to all snapshots:

- ILSL_Api.cs
- IOSSL_Api.cs
- LSL_Constants.cs

For XEngine API20, also provide lsl.lexer.cs and lsl.parser.cs from
OpenSim/Region/ScriptEngine/Shared/CodeTools/.

For each YEngine snapshot, also provide MMRScriptEventCode.cs and
MMRIEventHandlers.cs from OpenSim/Region/ScriptEngine/YEngine/.

Run:

    python3 scripts/generate_opensim_catalogues.py \
      --profile xengine-api20=/path/to/xengine-api20 \
      --profile yengine-api20=/path/to/yengine-api20 \
      --profile yengine-api22=/path/to/yengine-api22 \
      --profile yengine-api23=/path/to/yengine-api23 \
      --profile yengine-api24=/path/to/yengine-api24

The generator fails on unsupported C# types, unparsed event tokens, or
event declarations without canonical signatures. It does not silently drop
an overload or invent a type mapping.

### OSSL availability is not inferred

The OSSL interface is a list of functions implemented by the simulator build,
not a list of functions enabled on a particular region. Region security
configuration, threat levels, and permissions control actual availability.
NeoLint therefore checks OSSL usage only against an explicit project-side
allowlist (ossl_allowlist = strict / --strict-ossl, plus repeated
ossl_function / --ossl-function entries). It does not query or claim to
know the live region's settings.

## License and attribution

The canonical Second Life definitions are distributed under the MIT License;
the applicable notice is in LICENSES/LINDEN-RESEARCH-MIT.txt.

OpenSimulator interface metadata is derived from the project's source
interfaces, which carry the OpenSimulator license and notice. The notice is
included in LICENSES/OPENSIM-NOTICE.txt. This applies to the generated
OpenSim catalogue data and is separate from NeoLint's own MIT OR Apache-2.0
source-code licensing.
