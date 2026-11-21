# Pattern scanner

The optional `scan-diff` subcommand is a language-independent regex policy scanner. It is separate from neolint's LSL lexer, parser and semantic checks: it does not alter normal `neolint file.lsl` behavior.

The scanner reads a unified Git diff and checks **added lines only**. Context lines are used to maintain new-file line numbers; removed lines are ignored. It can be used in local workflows or CI without asking neolint to understand the syntax of every language in a repository.

## Quick start

Scan the changes in the current branch against the base branch:

```sh
git diff --no-ext-diff --unified=0 origin/main...HEAD | neolint scan-diff
```

Scan staged changes:

```sh
git diff --cached --no-ext-diff --unified=0 | neolint scan-diff
```

Fail CI when any pattern matches:

```sh
git diff --no-ext-diff --unified=0 origin/main...HEAD \
  | neolint scan-diff --fail-on-match
```

The built-in rules are embedded in the executable. The scan is deterministic and offline; it does not query GitHub, Cinnamon Spices, or any network service.

## Command line

```text
neolint scan-diff [OPTIONS] [PATCH|-]
```

When `PATCH` is omitted or `-`, the scanner reads the unified diff from stdin. A patch file may be supplied as the single positional argument.

| Option | Meaning |
| --- | --- |
| `--rules FILE` | Load a custom TOML rule file instead of the built-in rules. |
| `--extensions LIST` | Narrow the scan to comma-separated extensions, for example `.js,.py`. This cannot broaden a rule's own extension list. |
| `--format human` | Human-readable findings (default). |
| `--format json` | JSON array suitable for CI tooling. |
| `--fail-on-match` | Return exit code 1 when any finding is reported. |
| `-h`, `--help` | Show subcommand help. |

### JSON output

The output is a JSON array. Each finding has `file`, `line`, `code`, `name`, `severity`, `message`, and `source` fields. A clean scan emits `[]`.

```json
[{"file":"applet.js","line":12,"code":"XDG001","name":"Hardcoded XDG data directory","severity":"warning","message":"Use the platform data directory helper.","source":"const path = '.local/share';"}]
```

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Scan completed. Findings are advisory by default. |
| `1` | At least one finding was reported and `--fail-on-match` was supplied. |
| `2` | Invalid arguments, unreadable patch/rule file, invalid rule configuration, or malformed diff hunk. |

Rule severities are descriptive classifications. Even `block` findings do not change the exit code unless `--fail-on-match` is enabled. This mirrors the safe default of the original review-bot workflow: teams can introduce checks before deciding to make them mandatory.

## Built-in rules

The initial built-in profile focuses on Cinnamon/GLib/GIO projects:

| Rule | Severity | Purpose |
| --- | --- | --- |
| `XDG001` | warning | Hardcoded `.local/share` data directory. |
| `XDG002` | warning | Hardcoded `.config` configuration directory. |
| `XDG003` | warning | Hardcoded `.cache` cache directory. |
| `XDG004` | warning | Hardcoded `.local/state` state directory. |
| `ASYNC001` | warning | Synchronous GLib process spawning. |
| `ASYNC002` | warning | Synchronous GLib process execution. |
| `ASYNC003` | warning | Synchronous GIO file-content reads. |
| `ASYNC004` | warning | Synchronous GIO child enumeration. |
| `ASYNC005` | warning | Synchronous Cinnamon UTF-8 file reads. |
| `ASYNC006` | info | Synchronous GLib file metadata checks. |

Rules declare their own extension allowlists. The built-in set excludes translation files, Markdown, SVG and desktop-entry files, and reports at most 10 findings per changed file.

The scanner implementation is generic; the built-in profile is intentionally a starter set, not a complete mirror of the Linux Mint Cinnamon review bot. Additional checks can be supplied in a project-owned TOML file.

## Custom rule files

A custom rule file replaces the built-in rules. It uses TOML with one `[[rules]]` table per rule:

```toml
[settings]
max_findings_per_file = 10
exclude_extensions = [".md", ".po"]

[[rules]]
id = "SEC001"
name = "Avoid shell execution with an untrusted value"
severity = "block"
extensions = [".py"]
pattern = '''subprocess\.run\([^)]*shell\s*=\s*True'''
message = "Review shell execution carefully; pass an argument array when possible."
```

Fields:

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Unique code containing uppercase ASCII letters and digits. |
| `name` | yes | Short display name. |
| `severity` | yes | `block`, `warning`, or `info`. |
| `pattern` | yes | Regular expression applied independently to each added line. |
| `message` | yes | Advice shown when the expression matches. |
| `extensions` | no | Allowed extensions such as `[".js", ".py"]`; omitted or empty means all extensions. |

The optional `[settings]` table accepts:

- `max_findings_per_file`: maximum number of findings reported per changed file (default 10; must be greater than zero).
- `exclude_extensions`: file extensions ignored by every rule.

The engine uses a Rust regex implementation with look-around support. Regular expressions are case-sensitive unless the pattern says otherwise. Matching is line-oriented; multi-line rules are not supported. Pattern evaluation has a backtracking limit to avoid unbounded work from pathological expressions.

## Scope and limitations

- The scanner operates on unified diffs, not complete source trees. Use `git diff` to choose the changes that should be reviewed.
- It only evaluates added lines. A finding is not proof of a defect; regex rules can produce false positives and should be reviewed.
- It does not parse or type-check JavaScript, Python, C, Vala, or other languages.
- File renames are associated with the destination path from the diff.
- Binary changes and deleted files have no added source lines and are not scanned.
- The default rule profile does not automatically track the latest Cinnamon API or version-specific deprecations. Update the profile deliberately as project policy evolves.
- The custom rule file is project-maintained policy; it is not the same format as `.neolint.conf`, which controls LSL diagnostic severity and disabling.

The rule engine and starter profile were developed for neolint with the public Cinnamon Spices pattern-review workflow as a design reference. Neolint does not copy or execute that workflow's GitHub Action.
