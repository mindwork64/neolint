# Configuration

Neolint uses a small line-oriented configuration format. It is not TOML, YAML, or JSON.

```ini
# Ignore rules that are not useful for this script
disable = SEM001, SEM003

# Keep the diagnostic but make it non-failing
severity.SEM004 = warning

# AST is the default in v1.1.0 and later.
# Set this to legacy only when preserving the v1.0.0 engine is required.
engine = ast
target = second-life-mono
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
| `engine = legacy\|ast\|compare` | Select the analysis engine; `ast` is the default, and `legacy` restores the v1.0.0 analysis pipeline. |
| `target = PROFILE` | Select a platform/runtime profile; defaults to `second-life-mono`. Independent of the preprocessing dialect. |
| `ossl_allowlist = strict` | Enable OSSL checking even with an empty allowlist; all known OSSL calls warn unless added below. Requires an OpenSim target. |
| `ossl_function = NAME` | Add an OSSL function to the project's expected-enabled allowlist (repeatable; requires an OpenSim target). |

Rule codes must contain only uppercase ASCII letters and digits. Unknown settings, malformed lines, invalid severity names, invalid rule codes, invalid engine values, invalid target profiles, malformed OSSL names, and OSSL names absent from the selected runtime interface cause a configuration error and exit code 2. A configuration file without an `engine` line stays valid and selects `ast`; add `engine = legacy` to restore the v1.0.0 Legacy pipeline.

Multiple `severity.*` lines, multiple `disable` lines, and multiple `include_dir` lines are supported. Disabled-rule filtering and severity overrides are applied only to displayed diagnostics, before CLI overrides. Include directories preserve file order; quoted includes search the including file's directory first. `#pragma once` is a neolint convenience, not a verified Firestorm guarantee.

## Precedence

1. Default diagnostic severity and the default `engine = ast`.
2. Configuration-file `disable`, `severity.*`, `preprocess`, `dialect`, `include_dir`, `engine`, and `target`.
3. Command-line `--disable`, `--severity`, `--dialect`, `--target`, `--preprocess`, `--engine`, and `-I`/`--include-dir`, applied afterward.

The command line wins for the same rule. A non-empty CLI include-directory list replaces the configured list; CLI `--dialect` overrides the configured dialect; CLI `--target` overrides the configured target; CLI `--strict-ossl` enables an empty or populated OSSL allowlist; CLI `--ossl-function` entries are added to the configured OSSL allowlist; CLI `--engine` overrides the configured engine. Rule disabling and severity overrides affect only the displayed diagnostic stream: a disabled rule does not appear in that stream, and a severity of `warning` or `info` does not cause exit code 1 unless changed to `error`. In `--engine compare` mode the parity report is computed from the raw pre-configuration streams, so it can still report a disabled rule and select exit code 3; severity overrides do not change the report's raw severities.


## Target platform and runtime profiles

The target profile is separate from `dialect`. The dialect controls source
preprocessing and Firestorm syntax gates; the target selects the intended
simulator platform and script runtime.

Supported selectors:

| Selector | Platform | Runtime |
| --- | --- | --- |
| `second-life-mono` | Second Life | Mono (default) |
| `second-life-lso` | Second Life | Legacy LSO |
| `second-life-luau` | Second Life | LSL compiled for Luau |
| `opensim-xengine` | OpenSimulator | Historical XEngine (OpenSim versions before 0.9.3.0) |
| `opensim-yengine` | OpenSimulator | YEngine (OpenSim 0.9.3.0+) |

Use `--target PROFILE` or `target = PROFILE`. For example:

```sh
neolint --target opensim-yengine script.lsl
neolint --dialect firestorm --target second-life-mono script.lsl
neolint --target opensim-yengine --ossl-function osGetAgentIP script.lsl
```

Firestorm is not a runtime. Combining `--dialect firestorm` with an OpenSim
target does not mean that OpenSim implements every Firestorm extension; it
only selects the source preprocessor dialect independently of the selected
runtime. The current target-aware rule slice is deliberately narrow:
`SEM034`/`SEM035` are emitted only for the verified Second Life Mono profile.
Other targets fail closed for those diagnostics until their behavior is
independently verified. Their silence is not proof that the corresponding
construct is accepted by the target compiler.

The frozen Legacy engine is not target-profile aware. Selecting a `target`
with `--engine legacy` is rejected rather than silently ignored. AST is the
default; Compare passes the target to its AST analyzer while retaining the
existing Legacy comparison stream.

The AST analyzer uses pinned, runtime-specific OpenSim LSL function-name
snapshots. It recognizes 425 XEngine and 473 YEngine ll* API names; SEM047
reports a call to a canonical Second Life LSL function absent from the selected
OpenSim interface. Common signatures reuse canonical Second Life metadata,
with OpenSim key/string parameter aliases and the YEngine llLinkPlaySound
three-argument overload represented explicitly. OpenSim event and constant
catalogues are not yet runtime-specific, and interface presence does not prove
that every function has been verified in a running region.

OSSL is separate from standard LSL. The pinned interfaces contain 231 XEngine
and 265 YEngine OSSL function names, but this is not a promise that a region
enables them. Region threat levels, security configuration and permissions
control actual availability. By default NeoLint makes no OSSL availability
assumption. Set ossl_allowlist = strict (or pass --strict-ossl) to enable
checking. Repeated ossl_function = NAME entries (or --ossl-function NAME)
form a project-side allowlist of functions expected to be enabled. When that
list is explicit, SEM048 warns about known OSSL calls omitted from it. The
allowlist does not query or prove the live region configuration.

Second Life Mono, LSO and LSL-on-Luau currently share the canonical Linden Lab
function catalogue; full per-runtime event/API signature validation is not
claimed. The selected target is an analysis target, not a live compiler or
simulator connection.

Second Life's `second-life-luau` selector means **LSL compiled for Luau**; it
does not select the separate SLua language. The editor exposes the LSL-on-Luau
compiler target only on regions that advertise Lua script support. OpenSim
0.9.3.0 removed XEngine as part of its .NET 8 transition; `opensim-xengine`
exists only to analyze legacy deployments.

References: [Second Life compiler targets](https://lsl.dev/fundamentals/script-editor/),
[OpenSimulator scripting documentation](https://opensimulator.dev/wiki/Scripting_Documentation),
[Second Life State reference](https://wiki.secondlife.com/wiki/State).

## Engine migration (v1.1.0)

Starting with v1.1.0, AST is the default engine for CLI invocations and for
`LintConfig::default()`. Configuration files without an `engine` setting now
select AST. This is a behavioral default change even though the Rust
signatures are unchanged.

- Add `engine = legacy` to a configuration file, or pass `--engine legacy`, to
  restore the historical v1.0.0 Legacy pipeline.
- AST adds the AST-only lint rules `SEM024` (unused variables), `SEM025`
  (unused parameters), and `SEM026` (unreachable user functions). Their default
  severity is warning. AST also reports `SEM027` for non-simple global
  initializers and `SEM028`–`SEM029` for Firestorm switch errors. The Firestorm
  switch lint diagnostics `SEM030`–`SEM033` are warnings and appear only when
  the switches feature gate is enabled. `SEM034` rejects global-function state
  changes that Mono does not support; `SEM035` warns on the historical
  if-without-else compatibility workaround. `SEM036` rejects value returns
  from events, and `SEM037` warns on unreachable statements proven by the
  bounded control-flow graph, including constant branches, loop exits, jumps
  and Firestorm switch paths. `SEM038` rejects empty
  states, `SEM039` rejects elements with list type inside a list, and `SEM040`
  warns when list equality/inequality compares lengths rather than contents.
  `SEM041` warns on a bare plain assignment in an `if` condition; an extra pair
  of parentheses marks the assignment as intentional. `SEM042` warns when
  detection-context functions can be reached from events that do not provide
  the required detection data. `SEM043` warns on statically provable math-domain hazards, including zero
  divisors, invalid `llSqrt`/`llPow`/`llAcos`/`llAsin` arguments and non-positive
  `llLog`/`llLog10` arguments; `SEM044` warns when a populated named state
  has no transition path from `default`; `SEM045` reports known-invalid operator
  operand types; `SEM046` rejects known-incompatible arithmetic and comparison
  operand pairs.
- AST and Legacy may report intentional, documented differences and may order
  findings differently. The migration does not remove Legacy.
- `--engine compare` is a maintainer-oriented diagnostic mode: it runs both
  analyzers, displays Legacy diagnostics and reports the compared semantic
  differences on stderr.
- The default no-preprocess AST run is unit-based and capped at 10,000 rendered
  diagnostics per unit plus the invocation-wide limit; overflow appends a
  terminal `PRE039` and exits 1. Explicit Legacy without preprocessing remains
  uncapped.
- Unit-based runs of every engine, including `--engine legacy --preprocess`,
  apply display suppression before the rendered-diagnostic cap, so disabling a
  rule can avoid a terminal `PRE039`.
- The stdin label follows the preparation path, not the engine: unit-based runs
  (default AST, explicit `--engine ast`/`--engine compare`, and any preprocessed
  run including `--engine legacy --preprocess`) render `<stdin>`; only the
  direct plain-Legacy path (preprocessing off) renders `-`.
- Firestorm `switch` statements are accepted by AST when the switches gate is
  enabled; explicit Legacy rejects them.
- An event or declaration identifier that collides with a builtin name is a
  Legacy-only `SEM002` error; `default { llSay() { } }` is an error under Legacy
  and clean under AST.
- Without preprocessing, a raw `#define` reports `LEX000` under both engines;
  Legacy also emits `SEM012` for `define` and the macro name, while AST does not.
- Diagnostic ordering follows each engine's pass order, so AST and Legacy can
  order the same findings differently. Neither stream is globally sorted.

The SEM002 and `#define` differences are accepted AST/Legacy deltas analyzed in
the [Phase 6H implementation record](architecture/preprocessor-ast.md#phase-6h-implementation-record).

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
