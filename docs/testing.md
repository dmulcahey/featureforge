# Testing Superpowers

This repository has three primary automated validation surfaces plus opt-in or change-specific eval gates:

- `tests/codex-runtime/*.test.mjs` for deterministic generated-skill, template, and fixture contracts
- `tests/codex-runtime/` for install docs, generated skill preambles, helper binaries, and upgrade/migration behavior
- `tests/brainstorm-server/` for the brainstorming visual companion server

## Recommended Validation Order

Run these commands from the repository root:

```bash
npm --prefix runtime/core-helpers run build:check
node scripts/gen-agent-docs.mjs --check
node scripts/gen-skill-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
node tests/codex-runtime/run-shell-tests.mjs
bash tests/brainstorm-server/test-launch-wrappers.sh
node --test tests/brainstorm-server/server.test.js tests/brainstorm-server/ws-protocol.test.js
```

## What Each Suite Covers

### `tests/codex-runtime/*.test.mjs`

- Generated `skills/*/SKILL.md` presence, frontmatter, generated-header, and placeholder coverage
- Semantic preamble contracts for base and review skills
- Unit coverage for `scripts/gen-skill-docs.mjs` pure helper behavior
- Workflow-fixture regression coverage for the sequencing contract

### `tests/codex-runtime/`

- Generated `skills/*/SKILL.md` freshness plus runtime-facing install and workflow contract checks
- Generated reviewer-agent artifact freshness for Codex and GitHub Copilot
- Checked-in bundled runtime artifact freshness for `runtime/core-helpers/dist/*.cjs`
- Canonical retained shell-suite execution through `node tests/codex-runtime/run-shell-tests.mjs`, which discovers the durable `test-*.sh` files, runs them in parallel, and reports them in stable lexical order
- Runtime helper contracts for staged install/update, config, plan execution, update checks, migration delegation, and upgrade flow
- Supported public workflow CLI contracts for read-only status, next-step, artifact, explain, and failure output
- Workflow-status helper contracts for branch-scoped workflow manifests and conservative stage routing
- PowerShell wrapper behavior, including Git Bash selection and Windows path handling
- Install documentation and supported runtime references
- Required support files such as `VERSION`, `review/TODOS-format.md`, `review/checklist.md`, the shared QA assets, and `superpowers-upgrade/SKILL.md`
- Dedicated workflow-artifact fixtures under `tests/codex-runtime/fixtures/workflow-artifacts/` for sequencing-contract coverage without coupling tests to repository-root docs

### `tests/brainstorm-server/`

- WebSocket protocol behavior for the brainstorming visual companion
- HTTP server behavior and frame-serving expectations
- Shell and PowerShell launch-wrapper smoke coverage

## When To Run What

- Editing any `SKILL.md.tmpl`, runtime helper, or install/readme doc: run `node --test tests/codex-runtime/*.test.mjs` plus `node tests/codex-runtime/run-shell-tests.mjs`
- Editing files under `runtime/core-helpers/`: run `npm --prefix runtime/core-helpers run build:check` before the deterministic Node tests and `node tests/codex-runtime/run-shell-tests.mjs`
- Editing brainstorming server files under `skills/brainstorming/scripts/`: run `bash tests/brainstorm-server/test-launch-wrappers.sh` and `node --test tests/brainstorm-server/server.test.js tests/brainstorm-server/ws-protocol.test.js`
- Editing both runtime and brainstorming-server files: run both suites

## Evals And Change-Specific Gates

- `tests/evals/*.eval.mjs` remains an opt-in quality tier for the Node-driven prompt-behavior checks that still use `.eval.mjs`
- `tests/evals/using-superpowers-routing.orchestrator.md` is the authoritative Item 1 routing gate and drives the repo-versioned scenario, runner, and judge markdown artifacts plus local per-scenario evidence bundles under `~/.superpowers/projects/<slug>/...`
  This gate is agent-executed and does not run through `node --test` or the Node OpenAI-judge helper path. It is not part of the default deterministic validation order, but it is a required change-specific gate for Item 1 routing-safety work.
- See `tests/evals/README.md` for the Node-based eval environment variables and for routing-eval logging behavior

## Notes

- `test-runtime-instructions.sh` is the contract gate for supported install and runtime documentation
- `test-workflow-enhancements.sh` covers the imported review, QA, and document-release workflow contracts
- `test-workflow-sequencing.sh` covers artifact-state routing, stage gates, and the optional worktree policy using checked-in workflow fixtures in `tests/codex-runtime/fixtures/workflow-artifacts/`
- `tests/codex-runtime/*.test.mjs` covers the deterministic generated-skill and fixture assertions that do not need shell execution
- `node tests/codex-runtime/run-shell-tests.mjs` is the canonical retained-shell-suite entrypoint and runs the durable `test-*.sh` files in parallel with stable lexical reporting
- `npm --prefix runtime/core-helpers run build:check` is the freshness gate for the checked-in bundled helper artifacts
- `test-powershell-wrapper-bash-resolution.sh` covers shared PowerShell wrapper bash selection and override behavior
- `test-superpowers-install-runtime.sh` covers staged install/update preflight, swap, compatibility-link refresh, and missing-next-step reporting
- `test-superpowers-install-runtime-pwsh.sh` covers the PowerShell staged-install wrapper and refresh of already-present copied Windows agent files
- `test-superpowers-plan-execution.sh` covers the execution helper state machine, evidence canonicalization, rollback behavior, and malformed evidence rejection
- `test-superpowers-workflow.sh` covers the supported public workflow inspection CLI, including read-only state rendering, missing-expected-path handling, manifest diagnostics, and non-mutation guarantees
- `test-superpowers-workflow-status.sh` covers the internal workflow-state helper, including bootstrap, summary-mode parity, repo-identity recovery, malformed-artifact diagnostics, branch isolation, fallback refresh behavior, and conservative write-conflict handling
- `test-superpowers-update-check.sh` covers semver comparison, snooze handling, and just-upgraded markers
- `test-superpowers-upgrade-skill.sh` covers install-root resolution and direct upgrade-flow version resolution
- `test-superpowers-slug.sh` covers the shared slug helper, including missing-remote fallback, detached HEAD handling, and shell-safe escaped output
- `test-launch-wrappers.sh` covers the brainstorm launcher wrappers for Bash and PowerShell, including documented `C:\...` project paths
- `tests/brainstorm-server/server.test.js` and `tests/brainstorm-server/ws-protocol.test.js` cover the brainstorming server's HTTP behavior and websocket protocol semantics
