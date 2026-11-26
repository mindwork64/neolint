# Multi-script projects

NeoLint can analyze a declared set of LSL scripts through one project manifest.
This is an orchestration layer: **every script remains an independent LSL
compilation unit**. Project membership does not create shared globals,
functions, states, or symbols.

## Create a manifest

Create a UTF-8 file named neolint.project:

~~~ini
# Project-wide defaults
config = .neolint.conf

# Scripts are analyzed in declaration order
script = Scripts/controller.lsl
script = Scripts/worker.lsl
script = Scripts/telemetry.lsl

# Optional config overlay for one listed script
script_config = Scripts/worker.lsl => config/worker.conf
~~~

Run the project:

~~~sh
neolint --project neolint.project
neolint --project neolint.project --format json
~~~

The --project option cannot be combined with positional file paths,
directories, or stdin. Use one manifest per invocation. An explicit manifest
path is required; NeoLint does not discover a project file automatically.

## Manifest format

The format is line-oriented, not TOML, YAML, or JSON. Blank lines and text
after # are comments. Every non-empty line is key = value.

| Key | Meaning |
| --- | --- |
| config = PATH | Optional project-wide config file. At most one. |
| script = PATH | Required, repeatable script entry. |
| script_config = SCRIPT => CONFIG | Optional per-script config overlay. The script must also appear in a script entry. |

Paths in the manifest are relative to the directory containing the manifest.
Absolute paths are also accepted. Referenced scripts and config files must
already exist and be regular files. Scripts must have a .lsl extension
(case-insensitive). Duplicate script paths are rejected after canonical path
resolution, so aliases such as Scripts/a.lsl and ./Scripts/a.lsl cannot
accidentally analyze the same file twice. Each script can have at most one
script_config entry. A script configuration may be declared before or after
its script entry.

The order of script declarations is the project order. It determines analysis
and diagnostic output order. NeoLint does not recursively scan directories
referenced by the manifest: list every root script explicitly.

## Configuration layers and precedence

Each script receives its own effective configuration. Layers are applied in
this order:

1. NeoLint defaults (ast, Second Life Mono, no preprocessing).
2. Project-wide config.
3. The script's optional script_config.
4. The invocation-wide --config FILE overlay, when supplied with --project.
5. Command-line flags such as --engine, --target, --disable, --severity,
   --preprocess, --dialect, and include-directory options.

Later layers override earlier scalar settings when they explicitly specify
them. severity.RULE from a later layer replaces the setting for that rule.
Disabled rules are additive: a later config cannot re-enable a rule disabled
by an earlier layer. OSSL function entries are additive unless a later config
explicitly contains `ossl_allowlist = strict`, which resets inherited entries
to an explicitly empty list before applying any `ossl_function` entries in that
same file. A target-layer change alone does not remove inherited OSSL entries;
incompatible combinations are rejected during effective-config validation. An
include_dir list from a later layer replaces the previous list only when that
layer contains at least one include_dir setting. Command-line include directories
replace the effective list when any -I/--include-dir option is present.

--config is an overlay for every script in project mode, not a replacement
for the project's configuration chain. Relative include_dir paths inside a
project, per-script, or CLI config file are resolved relative to the directory
containing that particular config file. Include paths are not checked when an
individual config file is read: existence and directory type are checked only
after all config layers and CLI overrides have been applied. A stale inherited
include list can therefore be replaced by a valid later list without failing
early. This makes project runs independent of the shell's current working
directory for manifest-owned settings.

Validation is performed against every script's final effective configuration
before analysis begins. An invalid target/engine pairing or missing include
directory stops the project invocation with exit code 2; NeoLint does not
silently ignore settings that would be invalid for an individual script.

## Compilation and include boundaries

Every listed script gets an independent AnalysisUnit, source manager, parser,
symbol table, semantic analysis and diagnostic stream. When preprocessing is
enabled for that script, it also gets an independent preprocessor invocation.
For example, two scripts may each declare an integer named shared and a
function named helper() without a cross-script duplicate declaration. A call in one
script cannot resolve a function declared only in another script.

Includes remain part of the including script's own preprocessing unit. The
including file's directory is searched first for quoted includes, followed by
that script's configured include directories. No declarations from another
root script are implicitly imported. If two scripts include files with the
same name from different directories, each include is resolved using that
script's own configured search path.

A project may intentionally mix analysis engines, target profiles, and
preprocessing settings across scripts. For example, a legacy script can use
engine = legacy while the rest of the project uses the default AST engine. A
script with preprocessing disabled uses its own plain source; a script with
preprocessing enabled expands only its own include tree.

## Output and exit status

Human output uses each root script's manifest path as its display name. Include
diagnostics retain the include source location. --format json keeps the
existing JSON v1 top-level array and diagnostic object schema: the file
member identifies the script or included file. Clean scripts do not add
placeholder JSON objects. Diagnostics remain in manifest order and in each
unit's existing deterministic analysis order.

All project entries use the shared unit runner, including a script configured
for Legacy without preprocessing. Consequently each project script is subject
to the existing per-unit and invocation-wide diagnostic caps. This differs from
ordinary positional CLI invocations, where plain Legacy without preprocessing
retains its historical uncapped direct path. Input, manifest, configuration,
source preparation and file-read errors use exit code 2.
Remaining error-severity diagnostics use exit code 1. If one or more scripts
use engine = compare, their comparison report is emitted on stderr and an
unmatched comparison uses exit code 3 (unless an input failure requires 2).

## Example layout

~~~text
project/
├── neolint.project
├── .neolint.conf
├── config/
│   └── worker.conf
├── Scripts/
│   ├── controller.lsl
│   ├── worker.lsl
│   └── telemetry.lsl
└── includes/
    └── shared.lsl
~~~

neolint.project:

~~~ini
config = .neolint.conf
script = Scripts/controller.lsl
script = Scripts/worker.lsl
script = Scripts/telemetry.lsl
script_config = Scripts/worker.lsl => config/worker.conf
~~~

.neolint.conf:

~~~ini
engine = ast
target = second-life-mono
preprocess = true
include_dir = includes
severity.SEM024 = warning
~~~

config/worker.conf:

~~~ini
disable = SEM024
~~~

The worker's SEM024 findings are suppressed, but this does not change
diagnostics from the controller or telemetry scripts.
