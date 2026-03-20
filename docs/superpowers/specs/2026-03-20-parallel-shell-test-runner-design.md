# Parallel Shell Test Runner

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Summary

Superpowers should make the durable `tests/codex-runtime/*.sh` suite parallel-safe by construction and give that suite one canonical parallel runner. The runner should discover every retained `test-*.sh` directly under `tests/codex-runtime/`, sort the list deterministically, launch the tests in parallel every time, and report results in that same stable order.

This design targets the long-lived shell suite that remains valuable after the bundled core-helper runtime migration completes. It does not spend effort on temporary migration harnesses that are expected to be removed as part of the current modernization plan.

## Problem

The current shell-heavy test surface has two related problems:

1. Some durable shell tests are not safe to run concurrently because they mutate shared state in the checked-in repo.
2. The supported validation flow is still described as a manual list of `bash tests/...` commands, which makes parallel execution a convention instead of an enforced contract.

The current known contention point is `tests/codex-runtime/test-core-helper-runtime-launch.sh`, which mutates `runtime/core-helpers/dist/superpowers-config.cjs` in the working checkout to simulate missing and invalid runtime bundles. That behavior races with other tests that exercise the same shipped runtime bundle.

## Goals

- Make every retained `tests/codex-runtime/test-*.sh` test safe to run concurrently with every other retained shell test.
- Define one canonical shell-suite entrypoint that always runs the retained shell tests in parallel.
- Keep runner behavior deterministic: stable membership, stable ordering, stable summary output, stable failure reporting.
- Preserve the ability to run any individual shell test directly during local debugging.
- Keep the shell suite focused on durable wrapper, runtime, install, documentation, and integration contracts.

## Non-Goals

- Do not preserve temporary migration-equivalence shell harnesses that are already scheduled for removal.
- Do not replace the existing `.test.mjs` coverage for core-helper logic with shell tests.
- Do not introduce a second source of truth for shell-suite membership through a manifest file.
- Do not add fallback serialization or grouping to the default shell runner. The retained suite must be parallel-safe as a whole.

## Scope Boundary

The retained shell-suite membership rule is:

- Every durable `test-*.sh` directly under `tests/codex-runtime/` is part of the canonical shell suite.
- The runner auto-discovers those files, sorts them lexically, and executes all of them in parallel.
- Helper scripts, scratch scripts, and temporary migration harnesses must not remain in that directory under a `test-*.sh` name unless they are real suite members.

This keeps directory membership as the single source of truth and avoids manifest drift.

## Design

### 1. Isolation Contract For Retained Shell Tests

Every retained shell test must follow this rule:

- Tests may read from the real repo root.
- Tests may write only to test-local temp directories, temp install roots, temp repos, temp state directories, or temp homes created by that test.
- No retained shell test may mutate checked-in runtime bundles, checked-in docs, or other shared files under the repo root in a way that affects other tests.

Applied examples:

- `SUPERPOWERS_STATE_DIR` must be test-local.
- `HOME` must be test-local when install/update behavior is under test.
- Runtime-artifact corruption or deletion scenarios must operate against a copied temp install tree, not the working checkout.

### 2. Refactoring Shared-State Tests

Any retained shell test that currently mutates repo-root runtime artifacts must be rewritten to use an isolated temp install copy.

The first required change is:

- `tests/codex-runtime/test-core-helper-runtime-launch.sh` must stop renaming or corrupting `runtime/core-helpers/dist/superpowers-config.cjs` in the working checkout.
- Instead, it should construct or copy a temp install root, run the relevant wrapper from that temp install root, and simulate missing or invalid bundle states there.

This preserves the current coverage while removing cross-test interference.

### 3. Canonical Parallel Runner

Add a small Node-based runner under `tests/codex-runtime/` that becomes the canonical entrypoint for the retained shell suite.

Runner contract:

- Discover every `test-*.sh` directly under `tests/codex-runtime/`.
- Sort the file list lexically.
- Launch every discovered shell test in parallel.
- Capture each child process's stdout, stderr, exit code, and elapsed time independently.
- Print the final report in lexical order, not completion order.
- Exit non-zero if any shell test fails.

The runner should optimize for deterministic diagnostics instead of streaming interleaved live output.

### 4. Deterministic Reporting

The runner should provide:

- stable suite membership based on directory contents
- stable execution order for summary and failure blocks
- isolated captured output per test
- a concise final summary with passed, failed, and total counts

This keeps the suite parallel without making debugging noisy or order-dependent.

### 5. Documentation And Contract Updates

`docs/testing.md` should stop documenting the durable shell suite as a hand-maintained list of individual `bash tests/...` commands. Instead it should point to the canonical runner as the supported shell-suite entrypoint.

`tests/codex-runtime/test-runtime-instructions.sh` should enforce the new contract by checking that:

- docs reference the canonical shell runner
- temporary migration harnesses that should be gone are absent
- retained runtime references still match the supported validation flow

## Data Flow

### Parallel Shell Suite Execution

```text
docs/testing.md
      |
      v
canonical shell runner (Node)
      |
      +--> discover tests/codex-runtime/test-*.sh
      |
      +--> sort lexically
      |
      +--> spawn all shell tests in parallel
      |         |
      |         +--> test-local temp dirs / temp homes / temp installs
      |         +--> captured stdout/stderr/exit code
      |
      +--> collect all results
      |
      +--> print deterministic report in lexical order
      |
      +--> non-zero exit if any test failed
```

### Isolated Runtime-Bundle Failure Simulation

```text
repo root bundle (read-only for retained tests)
      |
      +--> copied into temp install root
                |
                +--> test mutates copied bundle only
                |
                +--> wrapper executes from temp install root
                |
                +--> failure/assertion remains local to that test
```

## Error Handling

The runner itself should fail explicitly for:

- no discovered shell tests
- unreadable or non-executable test files
- child-process spawn failures
- shell-test non-zero exits

Failures should identify the specific test file and preserve its captured output block.

## Testing Strategy

Implementation should include:

- a red/green change for the shared-state offender, proving runtime-bundle mutation no longer touches the working checkout
- deterministic tests for the runner's lexical discovery and stable reporting order
- repeated successful runner invocations to prove stable aggregate behavior
- full `tests/codex-runtime` Node plus shell validation after the new runner lands

## Rollout

1. Refactor retained shared-state shell tests to use isolated temp install roots.
2. Add the canonical Node shell runner.
3. Update `docs/testing.md` to point to the runner.
4. Update runtime-instruction tests to enforce the new runner contract.
5. Run the full codex-runtime suite through the new path.

## Risks

- If any retained shell test still writes to repo-root shared state, the new runner will surface intermittent failures under parallel load.
- If directory hygiene drifts and ad hoc helper scripts are left under `tests/codex-runtime/` with a `test-*.sh` name, discovery will treat them as supported suite members.
- If the runner streams child output live instead of buffering per test, parallel execution will become harder to debug.

## Open Questions Resolved In This Design

- Canonical runner language: Node, not Bash-only orchestration.
- Membership source of truth: directory membership, not a manifest.
- Scheduling model: all retained shell tests run in parallel by default.
- Scope target: durable retained shell tests only, not temporary migration harnesses.
