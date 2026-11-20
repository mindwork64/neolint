## Summary

Describe the behavior or documentation change and the reason for it.

## Verification

- [ ] Focused tests cover the changed behavior.
- [ ] Positive and negative cases are covered where applicable.
- [ ] Required Python metadata tests pass:
  `python3 -m unittest discover --start-directory scripts --pattern 'test_*.py'`
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test --all-targets --all-features` passes.
- [ ] `git diff --check` passes (manual check unless explicitly added to CI).

## Language and compatibility evidence

- [ ] LSL grammar/type claims are backed by an authoritative reference or explicitly marked uncertain.
- [ ] Unknown simulator/environment extensions remain conservative.
- [ ] Normal lint execution remains deterministic and offline.
- [ ] No unsupported C/Java language assumptions were introduced.

## Documentation synchronization

- [ ] Relevant repository docs are updated.
- [ ] Relevant GitHub Wiki pages are updated in the Wiki repository, or this PR explains why no Wiki change is needed.
- [ ] Relevant Linear issues reflect the shipped scope, acceptance criteria and remaining work.
- [ ] Parent issues remain open while required subtasks are incomplete.
- [ ] After merge, record the PR number, merge commit SHA and CI run in Linear; update Wiki and milestone progress.

## Scope control

- [ ] No unrelated refactoring or dependency was added.
- [ ] Known limitations and unresolved cases are stated.
- [ ] Release/CLI/diagnostic compatibility impact is documented.

## DeepSeek token-efficiency checklist (when DeepSeek contributed)

- [ ] The prompt limited inspection to relevant files and line ranges.
- [ ] No complete large files (especially `src/semantic.rs`) or full CI logs were dumped.
- [ ] Existing helpers, fixtures and test utilities were reused.
- [ ] Focused tests ran before the full CI suite.
- [ ] The report lists exact changed files, commands/results, links and unresolved items without repeating repository context.
