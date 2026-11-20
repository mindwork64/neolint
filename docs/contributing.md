# Contributing and release workflow

## Pull requests

Use the pull request template at `.github/PULL_REQUEST_TEMPLATE.md`.

Before requesting review:
- Inspect the final diff for accidental or unrelated changes.
- Add focused positive and negative tests for changed behavior.
- Verify language/semantic claims against authoritative LSL references. Historical compiler evidence must be labelled as such.
- Preserve conservative handling of unknown simulator/environment extensions.
- Keep normal lint runs deterministic and offline.
- Run the full validation set:

```sh
python3 -m unittest discover --start-directory scripts --pattern 'test_*.py'
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

The current GitHub Actions workflow runs the first four commands. `git diff --check` is a separate manual check unless the workflow is explicitly changed to run it.

## After every merged PR

Documentation synchronization is part of acceptance, not optional cleanup.

1. Confirm the PR is merged and CI passed on its final head.
2. Record the PR number, merge commit SHA, CI run URL and concise changed-file summary.
3. Update the GitHub Wiki pages affected by the change. Wiki pages live in the separate `mindwork64/neolint.wiki` Git repository; edit the relevant Markdown page(s) and commit the changes there. Do not substitute a repository `docs/` file or a Linear document for an actual Wiki update.
4. Update the affected Linear issue(s): shipped behavior, acceptance criteria, PR/commit/CI links and remaining work.
5. Update parent issue status only when all required child issues are complete.
6. Update Linear milestone/project progress if the merge changes scope or completion.
7. Ensure no Wiki or Linear text claims that branch-only, unmerged code has shipped.

## DeepSeek API token-efficiency requirements

Every implementation prompt must include explicit token-saving instructions:

- Inspect only the relevant files and targeted line ranges; do not scan or dump the whole repository.
- Never print entire large files, especially `src/semantic.rs`; request only the functions/ranges needed.
- Reuse existing helpers, fixtures, and test utilities instead of duplicating logic.
- Avoid unrelated refactoring, repeated analysis passes, and unnecessary dependencies.
- Make bounded, logically grouped changes; inspect the diff before making follow-up edits.
- Run focused tests first and the complete required CI once after the change is ready.
- Do not paste full CI logs into the prompt/report. Provide command, pass/fail summary and run link; include only relevant excerpts for failures.
- Do not repeat repository background that is already included in the task.
- Report exact changed files, tests executed, commit/PR links and unresolved limitations concisely.
- Never claim a test, manual check, merge or Wiki update that was not actually performed.

## Product and release boundaries

Neolint v1.0 is a CLI-only Rust LSL linter. GUI/TUI/web UI, IDE/LSP, automatic fixes, a full AST, crates.io publication and a complete catalog of simulator-specific extensions are outside v1.0 unless separately approved.

A clean lint result is not a guarantee that the official Second Life compiler or a particular simulator runtime will accept the script.
