# Core Helper Runtime Modernization Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use `superpowers:subagent-driven-development` when isolated-agent workflows are available in the current platform/session; otherwise use `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 2
**Execution Mode:** superpowers:subagent-driven-development
**Source Spec:** `docs/superpowers/specs/2026-03-19-core-helper-runtime-modernization-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

**Goal:** Replace the shell-first implementations of `superpowers-config`, `superpowers-workflow-status`, and `superpowers-plan-execution` with a bundled TypeScript/Node runtime while preserving the documented/tested CLI contract and introducing staged install/update flows for the new runtime generation.

**Architecture:** Add a dedicated `runtime/core-helpers/` workspace that owns TypeScript source, lockfile, bundling, and checked-in runtime artifacts. Keep the shipped `bin/` command names, but convert those three targeted commands into thin launchers for the bundled runtime with a stable fail-closed wrapper contract. Add a staged install/update helper that validates Node 20 and the bundled runtime before swapping the shared install into place, then update docs, inline upgrade guidance, and tests to treat the bundled runtime and staged helper as first-class release surfaces. `bin/superpowers-install-runtime` becomes the canonical owner of staging, preflight, swap, and repair of update-sensitive install artifacts that already exist; `bin/superpowers-migrate-install` remains a compatibility entrypoint that delegates into that staged flow.

**Tech Stack:** Node 20 LTS, TypeScript, bundled checked-in JavaScript runtime artifacts, POSIX shell launchers, PowerShell wrappers, `npm` in the dedicated runtime subdirectory, `node --test`, existing `tests/codex-runtime/` shell suites, runtime docs, release notes

---

## What Already Exists

- `bin/superpowers-config`, `bin/superpowers-workflow-status`, and `bin/superpowers-plan-execution` are the current Bash implementations and already define the external command surface that must be preserved.
- `bin/superpowers-config.ps1`, `bin/superpowers-workflow-status.ps1`, and `bin/superpowers-plan-execution.ps1` already provide the Windows wrapper entrypoints, but they currently depend on `bin/superpowers-pwsh-common.ps1` to locate and call Bash.
- `bin/superpowers-migrate-install` and `bin/superpowers-migrate-install.ps1` already handle install-layout migration into `~/.superpowers/install`, so they are the natural place to hook runtime-generation migration behavior.
- `superpowers-upgrade/SKILL.md` and `tests/codex-runtime/test-superpowers-upgrade-skill.sh` already define the inline `UPGRADE_AVAILABLE` handoff, so the staged helper must become their upgrade path too.
- `tests/codex-runtime/test-superpowers-config.sh`, `tests/codex-runtime/test-superpowers-workflow-status.sh`, `tests/codex-runtime/test-superpowers-plan-execution.sh`, and `tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh` already pin large parts of the behavioral contract that the migrated helpers must keep.
- `tests/codex-runtime/test-runtime-instructions.sh`, `docs/testing.md`, `README.md`, `.codex/INSTALL.md`, `.copilot/INSTALL.md`, `docs/README.codex.md`, and `docs/README.copilot.md` already define the runtime/install contract and must be updated rather than bypassed.
- `scripts/gen-skill-docs.mjs` and `scripts/gen-agent-docs.mjs` already establish the repo’s pattern of checked-in generated artifacts with freshness checks.

## Planned File Structure

- Create: `runtime/core-helpers/package.json`
  Dedicated runtime workspace manifest with build/check scripts.
- Create: `runtime/core-helpers/package-lock.json`
  Checked-in lockfile for reproducible bundle generation.
- Create: `runtime/core-helpers/tsconfig.json`
  TypeScript compile contract for the dedicated runtime workspace.
- Create: `runtime/core-helpers/scripts/build-runtime.mjs`
  Deterministic bundle generator and freshness checker for checked-in runtime artifacts.
- Create: `runtime/core-helpers/src/core/errors.ts`
  Shared typed failure definitions and neutral metadata.
- Create: `runtime/core-helpers/src/core/config.ts`
  Ported config behavior.
- Create: `runtime/core-helpers/src/core/workflow-status.ts`
  Ported workflow helper behavior.
- Create: `runtime/core-helpers/src/core/plan-execution.ts`
  Ported execution helper behavior.
- Create: `runtime/core-helpers/src/platform/filesystem.ts`
  Atomic writes, temp paths, and file helpers.
- Create: `runtime/core-helpers/src/platform/paths.ts`
  Repo-relative path normalization and path helpers.
- Create: `runtime/core-helpers/src/platform/process.ts`
  Process/runtime helpers used by all CLI entrypoints.
- Create: `runtime/core-helpers/src/cli/superpowers-config.ts`
- Create: `runtime/core-helpers/src/cli/superpowers-workflow-status.ts`
- Create: `runtime/core-helpers/src/cli/superpowers-plan-execution.ts`
- Create checked-in bundle outputs:
  - `runtime/core-helpers/dist/superpowers-config.cjs`
  - `runtime/core-helpers/dist/superpowers-workflow-status.cjs`
  - `runtime/core-helpers/dist/superpowers-plan-execution.cjs`
- Create: `bin/superpowers-runtime-common.sh`
- Create: `bin/superpowers-runtime-common.ps1`
  Shared shell-side Node/runtime launch helper for the migrated commands.
- Create: `bin/superpowers-install-runtime`
  Staged install/update helper for the migrated runtime generation.
- Create: `bin/superpowers-install-runtime.ps1`
  PowerShell entrypoint for staged install/update.
- Modify:
  - `bin/superpowers-config`
  - `bin/superpowers-config.ps1`
  - `bin/superpowers-workflow-status`
  - `bin/superpowers-workflow-status.ps1`
  - `bin/superpowers-plan-execution`
  - `bin/superpowers-plan-execution.ps1`
  - `bin/superpowers-migrate-install`
  - `bin/superpowers-migrate-install.ps1`
  - `superpowers-upgrade/SKILL.md`
- Create tests:
  - `tests/codex-runtime/runtime-build-contract.test.mjs`
  - `tests/codex-runtime/test-core-helper-runtime-launch.sh`
  - `tests/codex-runtime/test-superpowers-install-runtime.sh`
  - `tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh`
  - `tests/codex-runtime/config-core.test.mjs`
  - `tests/codex-runtime/config-cli.test.mjs`
  - `tests/codex-runtime/workflow-status-core.test.mjs`
  - `tests/codex-runtime/workflow-status-cli.test.mjs`
  - `tests/codex-runtime/plan-execution-core.test.mjs`
  - `tests/codex-runtime/plan-execution-cli.test.mjs`
- Modify tests:
  - `tests/codex-runtime/test-superpowers-config.sh`
  - `tests/codex-runtime/test-superpowers-workflow-status.sh`
  - `tests/codex-runtime/test-superpowers-plan-execution.sh`
  - `tests/codex-runtime/test-superpowers-workflow.sh`
  - `tests/codex-runtime/test-workflow-sequencing.sh`
  - `tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
  - `tests/codex-runtime/test-superpowers-migrate-install.sh`
  - `tests/codex-runtime/test-superpowers-upgrade-skill.sh`
  - `tests/codex-runtime/test-runtime-instructions.sh`
- Modify docs:
  - `README.md`
  - `.codex/INSTALL.md`
  - `.copilot/INSTALL.md`
  - `docs/README.codex.md`
  - `docs/README.copilot.md`
  - `docs/testing.md`
  - `RELEASE-NOTES.md`

## Not In Scope

- Rewriting `bin/superpowers-update-check`, `bin/superpowers-workflow`, or other non-targeted helpers into the new runtime in this change.
- Changing the documented/tested public contract of the targeted commands beyond the explicitly approved runtime/install failure additions.
- Adding a checked-in runtime fingerprint manifest as part of wrapper-time validation.
- Reworking the brainstorming visual companion or other optional Node-backed UX helpers.
- Introducing a new package-manager-based install contract for normal users.

## Implementation Notes

- Use `superpowers:test-driven-development` for each task: add red coverage first, verify failure, implement the minimum change, rerun the targeted suite, then commit.
- Keep the runtime workspace isolated inside `runtime/core-helpers/`; do not add a root-level `package.json` or `tsconfig.json`.
- Prefer a minimal runtime dependency set even though third-party packages are allowed. Every new runtime dependency must be justified, locked, bundled, and explicitly reviewed.
- Treat the checked-in bundled artifacts as a generated compatibility surface. `runtime/core-helpers/scripts/build-runtime.mjs --check` must fail if source, lockfile state, or checked-in bundles drift.
- Treat `runtime/core-helpers/src/**` plus `runtime/core-helpers/scripts/build-runtime.mjs` as the only editable runtime sources. Do not hand-edit `runtime/core-helpers/dist/*.cjs`; those files should change only via `npm --prefix runtime/core-helpers run build`.
- Keep `superpowers-config` behavior narrow. Even though it writes `config.yaml`, this migration should preserve the current line-oriented key/value contract rather than turning the helper into a general YAML parser/rewriter.
- Keep `runtime/core-helpers/src/platform/*` generic. Filesystem/path/process helpers may be shared there, but helper-specific parsing, workflow semantics, execution-state transitions, and file-format logic must stay in `core/config`, `core/workflow-status`, or `core/plan-execution`.
- Keep `runtime/core-helpers/src/core/errors.ts` presentation-neutral. It should define typed failures and neutral metadata only; CLI adapters own exit-code mapping, stderr text, and JSON/human-readable rendering.
- Make Node-native tests the primary behavioral coverage for the migrated runtime modules and selected CLI adapters. Keep the shell and PowerShell suites as thinner public contract, wrapper, and install-surface coverage rather than the only place core behavior is verified.
- Do not add silent runtime fallback behavior in wrappers. Missing Node, unsupported Node, missing bundles, and invalid bundles must fail closed with the agreed machine-readable failure classes and remediation text.
- Treat `bin/superpowers-install-runtime` as the canonical operational path for fresh installs, manual updates, and inline `UPGRADE_AVAILABLE` guidance once the migrated runtime ships.
- Keep `bin/superpowers-migrate-install` as a compatibility shim only. It should delegate into `bin/superpowers-install-runtime` rather than owning a second install state machine.
- Let `bin/superpowers-install-runtime` repair only deterministic update-sensitive artifacts, such as legacy compatibility links and already-present copied Windows agent files. Do not make it provision first-time Codex/Copilot discovery links.
- Keep staged install preflight narrow: verify Node 20 and the presence of the expected shipped bundle artifacts, but do not execute the bundles during install. Invalid-bundle detection remains primarily a release/test responsibility, with wrapper fail-closed behavior as the runtime backstop.
- For `superpowers-workflow-status` and `superpowers-plan-execution`, add temporary side-by-side equivalence suites that compare the legacy shell helper to the new bundled runtime on representative fixtures before wrapper replacement. Remove those temporary suites in the coordinated cutover task so legacy behavior evidence does not become a permanent second contract surface.
- Make the shared shell and PowerShell launcher helpers derive the install root from their own script location and resolve bundled entrypoints relative to that root. Installed commands must not depend on the caller's current working directory.
- Preserve the current manifest-backed fast path for `superpowers-workflow-status status` when `--refresh` is not requested and the manifest is still valid. Full repo artifact discovery should stay off that hot path unless refresh or recovery is required.
- Keep the migration helper-to-helper in implementation order, but do not cut over publicly until all three targeted commands and the staged install/update path pass together.

## Diagrams

### Runtime Launch Flow

```text
bin/superpowers-<helper>
   |
   +--> shell/pwsh runtime preflight + install-root resolution
            |
            +--> Node 20 missing/old --------> fail closed with stable failure_class
            |
            +--> bundled entry missing ------> fail closed with stable failure_class
            |
            +--> launch bundled runtime -----> <install-root>/runtime/core-helpers/dist/<helper>.cjs
                                                 |
                                                 +--> typed core module
                                                 +--> stdout/stderr/exit contract
```

### Staged Install / Update Flow

```text
install/update command
   |
   +--> validate Node 20
   |      |
   |      +--> fail -> report requirement + cleanup stage + keep current install
   |
   +--> prepare staged checkout/runtime
   |
   +--> validate expected bundled runtime artifacts are present
   |      |
   |      +--> fail -> cleanup stage + keep current install
   |
   +--> swap staged install into ~/.superpowers/install
   |
   +--> repair existing compatibility links and copied agent files
   |
   +--> print remaining first-time setup steps when discovery links do not exist
```

## Failure Modes To Preserve

| Codepath | Failure to prevent | Guardrail |
| --- | --- | --- |
| wrapper launch | missing/unsupported Node yields ambiguous shell failure | stable wrapper failure classes + launch tests |
| bundled runtime | stale or missing bundle ships unnoticed | runtime build check + release verification |
| config helper | config list/get/set semantics drift | existing smoke suite + wrapper launch suite |
| workflow helper | routing, `expect`, or `sync` semantics drift | existing workflow-status/workflow-sequencing suites + public CLI suite |
| execution helper | plan mutation semantics drift or corrupt artifacts | existing plan-execution suite + staged preflight coverage |
| install/update | failed upgrade leaves broken partial install in place | staged helper tests + migration-helper tests |
| upgrade guidance | `UPGRADE_AVAILABLE` still routes users to raw `git pull` | upgrade-skill tests + staged helper docs |

## Task 1: Scaffold The Dedicated Runtime Workspace And Build Contract

**Files:**
- Create: `runtime/core-helpers/package.json`
- Create: `runtime/core-helpers/package-lock.json`
- Create: `runtime/core-helpers/tsconfig.json`
- Create: `runtime/core-helpers/scripts/build-runtime.mjs`
- Create: `runtime/core-helpers/src/core/errors.ts`
- Create: `runtime/core-helpers/src/platform/filesystem.ts`
- Create: `runtime/core-helpers/src/platform/paths.ts`
- Create: `runtime/core-helpers/src/platform/process.ts`
- Create: `runtime/core-helpers/src/cli/superpowers-config.ts`
- Create: `runtime/core-helpers/src/cli/superpowers-workflow-status.ts`
- Create: `runtime/core-helpers/src/cli/superpowers-plan-execution.ts`
- Create: `runtime/core-helpers/dist/superpowers-config.cjs`
- Create: `runtime/core-helpers/dist/superpowers-workflow-status.cjs`
- Create: `runtime/core-helpers/dist/superpowers-plan-execution.cjs`
- Create: `tests/codex-runtime/runtime-build-contract.test.mjs`
- Modify: `tests/codex-runtime/test-runtime-instructions.sh`
- Test: `node --test tests/codex-runtime/runtime-build-contract.test.mjs`
- Test: `npm --prefix runtime/core-helpers run build:check`

- [x] **Step 1: Write the failing runtime workspace contract test**
```js
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

test('runtime workspace files exist', () => {
  for (const path of [
    'runtime/core-helpers/package.json',
    'runtime/core-helpers/package-lock.json',
    'runtime/core-helpers/tsconfig.json',
    'runtime/core-helpers/scripts/build-runtime.mjs',
  ]) {
    assert.equal(fs.existsSync(path), true, `${path} should exist`);
  }
});
```

- [x] **Step 2: Run the red build-contract test**
Run: `node --test tests/codex-runtime/runtime-build-contract.test.mjs`
Expected: FAIL with missing-file assertions for the new runtime workspace.

- [x] **Step 3: Add the dedicated runtime workspace files**
```json
{
  "name": "superpowers-core-helpers-runtime",
  "private": true,
  "type": "module",
  "engines": { "node": ">=20 <21 || >=22" },
  "scripts": {
    "build": "node scripts/build-runtime.mjs",
    "build:check": "node scripts/build-runtime.mjs --check"
  }
}
```

- [x] **Step 4: Add compileable placeholder entrypoints and bundle generation**
```ts
export function main(argv: string[]): number {
  throw new Error(`Not implemented: ${argv[1] ?? 'helper'}`);
}
```

- [x] **Step 5: Generate the checked-in lockfile and dist placeholders**
Run: `npm --prefix runtime/core-helpers install`
Expected: PASS and create/update `runtime/core-helpers/package-lock.json`.

Run: `npm --prefix runtime/core-helpers run build`
Expected: PASS and write the checked-in `dist/*.cjs` placeholder bundles.

- [x] **Step 6: Add the runtime workspace to runtime validation**
```bash
# Add to tests/codex-runtime/test-runtime-instructions.sh validation inventory:
"runtime/core-helpers/package.json"
"runtime/core-helpers/package-lock.json"
"runtime/core-helpers/tsconfig.json"
"runtime/core-helpers/scripts/build-runtime.mjs"
"runtime/core-helpers/dist/superpowers-config.cjs"
"runtime/core-helpers/dist/superpowers-workflow-status.cjs"
"runtime/core-helpers/dist/superpowers-plan-execution.cjs"
```

- [x] **Step 7: Re-run the targeted green checks**
Run: `node --test tests/codex-runtime/runtime-build-contract.test.mjs`
Expected: PASS

Run: `npm --prefix runtime/core-helpers run build:check`
Expected: PASS with no bundle drift reported.

- [x] **Step 8: Commit the runtime workspace scaffold**
```bash
git add runtime/core-helpers tests/codex-runtime/runtime-build-contract.test.mjs tests/codex-runtime/test-runtime-instructions.sh
git commit -m "build: scaffold core helper runtime workspace"
```

## Task 2: Add The Staged Install / Update Helper And Migrate Runtime Docs

**Files:**
- Create: `bin/superpowers-install-runtime`
- Create: `bin/superpowers-install-runtime.ps1`
- Modify: `bin/superpowers-migrate-install`
- Modify: `bin/superpowers-migrate-install.ps1`
- Modify: `superpowers-upgrade/SKILL.md`
- Create: `tests/codex-runtime/test-superpowers-install-runtime.sh`
- Create: `tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh`
- Modify: `tests/codex-runtime/test-superpowers-migrate-install.sh`
- Modify: `tests/codex-runtime/test-superpowers-upgrade-skill.sh`
- Modify: `.codex/INSTALL.md`
- Modify: `.copilot/INSTALL.md`
- Modify: `README.md`
- Modify: `docs/README.codex.md`
- Modify: `docs/README.copilot.md`
- Modify: `docs/testing.md`
- Test: `bash tests/codex-runtime/test-superpowers-install-runtime.sh`
- Test: `bash tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh`
- Test: `bash tests/codex-runtime/test-superpowers-migrate-install.sh`
- Test: `bash tests/codex-runtime/test-superpowers-upgrade-skill.sh`
- Test: `bash tests/codex-runtime/test-runtime-instructions.sh`

- [x] **Step 1: Add failing staged-install tests**
```bash
# Cover:
# - missing Node -> no swap + cleanup of stage + explicit failure
# - unsupported Node -> no swap + explicit failure
# - missing required bundle in staged checkout -> no swap + cleanup
# - successful preflight -> swap staged install into place
# - existing compatibility links / copied Windows agent files are repaired after swap
# - missing first-time discovery links remain manual and are surfaced as next steps
# - invalid bundle behavior remains covered by release verification and wrapper failure tests
# - PowerShell install/update entrypoints delegate correctly when pwsh is available
# - PowerShell install/update entrypoints refresh already-present copied agent files on Windows
# - migrate-install delegates to install-runtime instead of owning a parallel flow
# - inline upgrade skill routes upgrade/install actions through superpowers-install-runtime
```

- [x] **Step 2: Run the red install/migrate suites**
Run: `bash tests/codex-runtime/test-superpowers-install-runtime.sh`
Expected: FAIL because the staged helper does not exist yet.

Run: `bash tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh`
Expected: FAIL because the PowerShell staged install/update entrypoints do not exist yet.

Run: `bash tests/codex-runtime/test-superpowers-migrate-install.sh`
Expected: FAIL once it expects delegation into the staged runtime helper.

Run: `bash tests/codex-runtime/test-superpowers-upgrade-skill.sh`
Expected: FAIL once it expects inline upgrade guidance to use the staged runtime helper instead of direct `git pull`.

- [x] **Step 3: Implement the staged helper**
```bash
# staged flow:
# 1. clone or refresh stage
# 2. verify node version
# 3. verify required bundled runtime artifacts exist
# 4. swap into ~/.superpowers/install
# 5. repair existing compatibility links and copied Windows agent files
# 6. expose migrate-install only as a compatibility delegator into this flow
# 7. print any remaining first-time discovery-link setup steps
```

- [x] **Step 4: Hook migration and docs into the staged helper**
```bash
# Fresh install, manual update docs, and the inline upgrade skill should point to
# the staged helper instead of raw `git pull`.
```

- [x] **Step 5: Rerun the install/runtime docs suites**
Run: `bash tests/codex-runtime/test-superpowers-install-runtime.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-migrate-install.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-upgrade-skill.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-runtime-instructions.sh`
Expected: PASS

- [x] **Step 6: Commit the staged install/update slice**
```bash
git add bin/superpowers-install-runtime bin/superpowers-install-runtime.ps1 bin/superpowers-migrate-install bin/superpowers-migrate-install.ps1 superpowers-upgrade/SKILL.md .codex/INSTALL.md .copilot/INSTALL.md README.md docs/README.codex.md docs/README.copilot.md docs/testing.md tests/codex-runtime/test-superpowers-install-runtime.sh tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh tests/codex-runtime/test-superpowers-migrate-install.sh tests/codex-runtime/test-superpowers-upgrade-skill.sh tests/codex-runtime/test-runtime-instructions.sh
git commit -m "feat: add staged runtime install flow"
```

## Task 3: Port `superpowers-config` And Establish The Shared Wrapper Failure Contract

**Files:**
- Create: `bin/superpowers-runtime-common.sh`
- Create: `bin/superpowers-runtime-common.ps1`
- Create: `runtime/core-helpers/src/core/config.ts`
- Modify: `runtime/core-helpers/src/core/errors.ts`
- Modify: `runtime/core-helpers/src/platform/filesystem.ts`
- Modify: `runtime/core-helpers/src/platform/process.ts`
- Modify: `runtime/core-helpers/src/cli/superpowers-config.ts`
- Modify: `runtime/core-helpers/dist/superpowers-config.cjs`
- Create: `tests/codex-runtime/config-core.test.mjs`
- Create: `tests/codex-runtime/config-cli.test.mjs`
- Create: `tests/codex-runtime/test-core-helper-runtime-launch.sh`
- Modify: `tests/codex-runtime/test-superpowers-config.sh`
- Modify: `tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
- Modify: `bin/superpowers-config`
- Modify: `bin/superpowers-config.ps1`
- Test: `node --test tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs`
- Test: `bash tests/codex-runtime/test-core-helper-runtime-launch.sh`
- Test: `bash tests/codex-runtime/test-superpowers-config.sh`
- Test: `bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`

- [x] **Step 1: Add failing Node-native config tests and launch-contract coverage**
```bash
# Cover:
# - core/config read, write, and list behavior directly under node --test
# - CLI adapter exit codes and output shaping for get/set/list usage errors
# - missing node -> RuntimeDependencyMissing
# - unsupported node version -> RuntimeDependencyVersionUnsupported
# - missing dist entry -> RuntimeArtifactMissing
# - invalid dist entry -> RuntimeArtifactInvalid
# - bundle path resolution is anchored to the install root, not the caller cwd
# - PowerShell wrapper launches Node directly for migrated helpers
# - config duplicate-key reads keep "last matching key wins"
# - config writes replace matching lines or append when absent
# - config writes preserve unrelated non-target lines
# - config get preserves the current whitespace-trimmed read behavior
```

- [x] **Step 2: Run the red launch/config tests**
Run: `node --test tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs`
Expected: FAIL because the config core and CLI adapter do not exist yet.

Run: `bash tests/codex-runtime/test-core-helper-runtime-launch.sh`
Expected: FAIL because the shared runtime launcher does not exist yet.

Run: `bash tests/codex-runtime/test-superpowers-config.sh`
Expected: FAIL once the test expects the Node-backed wrapper contract.

- [x] **Step 3: Add the shared shell and PowerShell launch helpers**
```bash
emit_runtime_failure() {
  printf '{"failure_class":"%s","message":"%s"}\n' "$1" "$2" >&2
}

superpowers_install_root() {
  cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1
  pwd -P
}
```

```powershell
function Get-SuperpowersInstallRoot {
  return (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}

function Get-SuperpowersNodePath {
  $node = Get-Command node -ErrorAction SilentlyContinue
  if (-not $node) { throw 'RuntimeDependencyMissing' }
  return $node.Source
}
```

Keep this logic in the new `bin/superpowers-runtime-common.ps1` file so the existing `bin/superpowers-pwsh-common.ps1` remains Bash-focused for non-migrated wrappers.

- [x] **Step 4: Port the config core and CLI entrypoint**
```ts
export function getConfigValue(configText: string, key: string): string {
  // preserve current "last matching key wins" behavior
}
```

```bash
# Preserve the existing narrow config semantics:
# - treat config.yaml as a simple top-level key/value file
# - keep "last matching key wins" reads
# - replace or append matching lines on write
# - do not introduce YAML normalization, comment preservation promises,
#   or nested-structure editing behavior in this migration
```

- [x] **Step 5: Replace the shipped config wrappers with Node launchers**
```bash
#!/usr/bin/env bash
set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/superpowers-runtime-common.sh"
superpowers_run_node_runtime "$(superpowers_install_root)/runtime/core-helpers/dist/superpowers-config.cjs" "$@"
```

- [x] **Step 6: Rebuild and rerun the targeted config suites**
Run: `npm --prefix runtime/core-helpers run build`
Expected: PASS and refresh `runtime/core-helpers/dist/superpowers-config.cjs`.

Run: `node --test tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs`
Expected: PASS

Run: `bash tests/codex-runtime/test-core-helper-runtime-launch.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-config.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
Expected: PASS with the targeted migrated wrappers no longer depending on Bash.

- [ ] **Step 7: Commit the config slice**
```bash
git add bin/superpowers-runtime-common.sh bin/superpowers-runtime-common.ps1 bin/superpowers-config bin/superpowers-config.ps1 runtime/core-helpers tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs tests/codex-runtime/test-core-helper-runtime-launch.sh tests/codex-runtime/test-superpowers-config.sh tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh
git commit -m "feat: port superpowers-config to bundled runtime"
```

## Task 4: Port `superpowers-workflow-status` Without Changing Workflow Semantics

**Files:**
- Create: `tests/codex-runtime/test-superpowers-workflow-status-equivalence.sh`
- Create: `tests/codex-runtime/workflow-status-core.test.mjs`
- Create: `tests/codex-runtime/workflow-status-cli.test.mjs`
- Modify: `runtime/core-helpers/src/core/workflow-status.ts`
- Modify: `runtime/core-helpers/src/platform/filesystem.ts`
- Modify: `runtime/core-helpers/src/platform/paths.ts`
- Modify: `runtime/core-helpers/src/cli/superpowers-workflow-status.ts`
- Modify: `runtime/core-helpers/dist/superpowers-workflow-status.cjs`
- Modify: `bin/superpowers-workflow-status`
- Modify: `bin/superpowers-workflow-status.ps1`
- Modify: `tests/codex-runtime/test-superpowers-workflow-status.sh`
- Modify: `tests/codex-runtime/test-superpowers-workflow.sh`
- Modify: `tests/codex-runtime/test-workflow-sequencing.sh`
- Test: `node --test tests/codex-runtime/workflow-status-core.test.mjs tests/codex-runtime/workflow-status-cli.test.mjs`
- Test: `bash tests/codex-runtime/test-superpowers-workflow-status-equivalence.sh`
- Test: `bash tests/codex-runtime/test-superpowers-workflow-status.sh`
- Test: `bash tests/codex-runtime/test-superpowers-workflow.sh`
- Test: `bash tests/codex-runtime/test-workflow-sequencing.sh`

- [ ] **Step 1: Extend workflow-status tests with Node-native coverage, explicit parity expectations, and a temporary equivalence harness**
```bash
# Keep assertions for:
# - core/workflow-status routing and manifest logic directly under node --test
# - CLI adapter exit codes and structured output for status/expect/sync paths
# - non-`--refresh` status preserves the manifest-backed fast path when the manifest is valid
# - status / expect / sync / resolve behavior
# - reason/note fields
# - manifest recovery / branch identity
# - missing/corrupt manifest cases
# - legacy shell helper and direct Node CLI match on representative fixture cases
```

- [ ] **Step 2: Run the red workflow-status suites**
Run: `node --test tests/codex-runtime/workflow-status-core.test.mjs tests/codex-runtime/workflow-status-cli.test.mjs`
Expected: FAIL because the workflow-status core and CLI adapter do not yet preserve the helper contract.

Run: `bash tests/codex-runtime/test-superpowers-workflow-status-equivalence.sh`
Expected: FAIL because the direct Node CLI does not yet match the legacy shell helper on the chosen fixture cases.

Run: `bash tests/codex-runtime/test-superpowers-workflow-status.sh`
Expected: FAIL because the Node-backed implementation does not yet preserve the full helper contract.

Run: `bash tests/codex-runtime/test-superpowers-workflow.sh`
Expected: FAIL if the public workflow CLI no longer matches the helper behavior.

- [ ] **Step 3: Port the workflow core with typed state and failure outputs**
```ts
export type WorkflowStatusResult =
  | { kind: 'resolved'; status: string; nextSkill: string; specPath: string; planPath: string; reason?: string }
  | { kind: 'runtime_failure'; failureClass: string; message: string };

// Preserve the current performance shape:
// - status without --refresh should return from a valid manifest when possible
// - full repo/spec/plan discovery should only happen on refresh or recovery paths
```

- [ ] **Step 4: Run temporary old-vs-new equivalence checks before wrapper replacement**
Run: `bash tests/codex-runtime/test-superpowers-workflow-status-equivalence.sh`
Expected: PASS with the legacy shell helper and direct Node CLI producing matching representative outputs on the chosen fixtures.

- [ ] **Step 5: Replace the shipped workflow-status wrappers with bundled runtime launch**
```bash
superpowers_run_node_runtime "$(superpowers_install_root)/runtime/core-helpers/dist/superpowers-workflow-status.cjs" "$@"
```

- [ ] **Step 6: Rebuild and rerun the workflow suites**
Run: `npm --prefix runtime/core-helpers run build`
Expected: PASS and refresh `runtime/core-helpers/dist/superpowers-workflow-status.cjs`.

Run: `node --test tests/codex-runtime/workflow-status-core.test.mjs tests/codex-runtime/workflow-status-cli.test.mjs`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-workflow-status.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-workflow.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-workflow-sequencing.sh`
Expected: PASS

- [ ] **Step 7: Commit the workflow-status slice**
```bash
git add bin/superpowers-workflow-status bin/superpowers-workflow-status.ps1 runtime/core-helpers tests/codex-runtime/workflow-status-core.test.mjs tests/codex-runtime/workflow-status-cli.test.mjs tests/codex-runtime/test-superpowers-workflow-status-equivalence.sh tests/codex-runtime/test-superpowers-workflow-status.sh tests/codex-runtime/test-superpowers-workflow.sh tests/codex-runtime/test-workflow-sequencing.sh
git commit -m "feat: port workflow-status to bundled runtime"
```

## Task 5: Port `superpowers-plan-execution` And Preserve Execution-State Semantics

**Files:**
- Create: `tests/codex-runtime/test-superpowers-plan-execution-equivalence.sh`
- Create: `tests/codex-runtime/plan-execution-core.test.mjs`
- Create: `tests/codex-runtime/plan-execution-cli.test.mjs`
- Modify: `runtime/core-helpers/src/core/plan-execution.ts`
- Modify: `runtime/core-helpers/src/platform/filesystem.ts`
- Modify: `runtime/core-helpers/src/platform/paths.ts`
- Modify: `runtime/core-helpers/src/cli/superpowers-plan-execution.ts`
- Modify: `runtime/core-helpers/dist/superpowers-plan-execution.cjs`
- Modify: `bin/superpowers-plan-execution`
- Modify: `bin/superpowers-plan-execution.ps1`
- Modify: `tests/codex-runtime/test-superpowers-plan-execution.sh`
- Test: `node --test tests/codex-runtime/plan-execution-core.test.mjs tests/codex-runtime/plan-execution-cli.test.mjs`
- Test: `bash tests/codex-runtime/test-superpowers-plan-execution-equivalence.sh`
- Test: `bash tests/codex-runtime/test-superpowers-plan-execution.sh`

- [ ] **Step 1: Tighten red execution-helper Node-native coverage, parity coverage, and add a temporary equivalence harness**
```bash
# Preserve:
# - core/plan-execution state transitions directly under node --test
# - CLI adapter exit codes and machine-readable failure rendering for execution commands
# - status / recommend / begin / transfer / complete / note / reopen
# - exact failure classes for malformed state
# - evidence-path handling
# - atomic rewrite / backup behavior
# - legacy shell helper and direct Node CLI match on representative success/failure fixtures
```

- [ ] **Step 2: Run the red execution suite**
Run: `node --test tests/codex-runtime/plan-execution-core.test.mjs tests/codex-runtime/plan-execution-cli.test.mjs`
Expected: FAIL because the execution core and CLI adapter do not yet preserve the helper contract.

Run: `bash tests/codex-runtime/test-superpowers-plan-execution-equivalence.sh`
Expected: FAIL because the direct Node CLI does not yet match the legacy shell helper on the chosen fixture cases.

Run: `bash tests/codex-runtime/test-superpowers-plan-execution.sh`
Expected: FAIL because the Node-backed implementation does not yet preserve execution semantics.

- [ ] **Step 3: Port the execution state machine with explicit mutation helpers**
```ts
export function applyExecutionTransition(planText: string, args: BeginArgs | CompleteArgs | NoteArgs): TransitionResult {
  // preserve current checked-step, note, and evidence semantics
}
```

- [ ] **Step 4: Run temporary old-vs-new equivalence checks before wrapper replacement**
Run: `bash tests/codex-runtime/test-superpowers-plan-execution-equivalence.sh`
Expected: PASS with the legacy shell helper and direct Node CLI producing matching representative success/failure behavior on the chosen fixtures.

- [ ] **Step 5: Replace the shipped execution wrappers with bundled runtime launch**
```bash
superpowers_run_node_runtime "$(superpowers_install_root)/runtime/core-helpers/dist/superpowers-plan-execution.cjs" "$@"
```

- [ ] **Step 6: Rebuild and rerun the execution suite**
Run: `npm --prefix runtime/core-helpers run build`
Expected: PASS and refresh `runtime/core-helpers/dist/superpowers-plan-execution.cjs`.

Run: `node --test tests/codex-runtime/plan-execution-core.test.mjs tests/codex-runtime/plan-execution-cli.test.mjs`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-plan-execution.sh`
Expected: PASS

- [ ] **Step 7: Commit the execution slice**
```bash
git add bin/superpowers-plan-execution bin/superpowers-plan-execution.ps1 runtime/core-helpers tests/codex-runtime/plan-execution-core.test.mjs tests/codex-runtime/plan-execution-cli.test.mjs tests/codex-runtime/test-superpowers-plan-execution-equivalence.sh tests/codex-runtime/test-superpowers-plan-execution.sh
git commit -m "feat: port plan execution to bundled runtime"
```

## Task 6: Remove Legacy Helper Bodies, Refresh Release Surface, And Run Full Verification

**Files:**
- Modify: `bin/superpowers-config`
- Modify: `bin/superpowers-workflow-status`
- Modify: `bin/superpowers-plan-execution`
- Delete: `tests/codex-runtime/test-superpowers-workflow-status-equivalence.sh`
- Delete: `tests/codex-runtime/test-superpowers-plan-execution-equivalence.sh`
- Modify: `tests/codex-runtime/test-runtime-instructions.sh`
- Modify: `docs/testing.md`
- Modify: `RELEASE-NOTES.md`
- Modify: `runtime/core-helpers/dist/superpowers-config.cjs`
- Modify: `runtime/core-helpers/dist/superpowers-workflow-status.cjs`
- Modify: `runtime/core-helpers/dist/superpowers-plan-execution.cjs`
- Test: `npm --prefix runtime/core-helpers run build:check`
- Test: `node --test tests/codex-runtime/runtime-build-contract.test.mjs`
- Test: `node --test tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs`
- Test: `node --test tests/codex-runtime/workflow-status-core.test.mjs tests/codex-runtime/workflow-status-cli.test.mjs`
- Test: `node --test tests/codex-runtime/plan-execution-core.test.mjs tests/codex-runtime/plan-execution-cli.test.mjs`
- Test: `bash tests/codex-runtime/test-core-helper-runtime-launch.sh`
- Test: `bash tests/codex-runtime/test-superpowers-config.sh`
- Test: `bash tests/codex-runtime/test-superpowers-workflow-status.sh`
- Test: `bash tests/codex-runtime/test-superpowers-workflow.sh`
- Test: `bash tests/codex-runtime/test-superpowers-plan-execution.sh`
- Test: `bash tests/codex-runtime/test-superpowers-install-runtime.sh`
- Test: `bash tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh`
- Test: `bash tests/codex-runtime/test-superpowers-migrate-install.sh`
- Test: `bash tests/codex-runtime/test-superpowers-upgrade-skill.sh`
- Test: `bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
- Test: `bash tests/codex-runtime/test-workflow-sequencing.sh`
- Test: `bash tests/codex-runtime/test-runtime-instructions.sh`

- [ ] **Step 1: Remove any remaining live legacy helper logic from the targeted shipped wrappers and delete the temporary equivalence harnesses**
```bash
# After this step the targeted `bin/` files should be launchers only,
# not dormant copies of the old Bash implementations, and the temporary
# old-vs-new equivalence tests should be gone.
```

- [ ] **Step 2: Update release/testing docs for the bundled runtime generation**
```bash
# Add the runtime build/freshness check to docs/testing.md:
npm --prefix runtime/core-helpers run build:check
```

- [ ] **Step 3: Refresh the bundled runtime artifacts one last time**
Run: `npm --prefix runtime/core-helpers run build`
Expected: PASS and refresh all checked-in `dist/*.cjs` artifacts.

- [ ] **Step 4: Run the full verification matrix**
Run: `npm --prefix runtime/core-helpers run build:check`
Expected: PASS

Run: `node --test tests/codex-runtime/runtime-build-contract.test.mjs`
Expected: PASS

Run: `node --test tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs`
Expected: PASS

Run: `node --test tests/codex-runtime/workflow-status-core.test.mjs tests/codex-runtime/workflow-status-cli.test.mjs`
Expected: PASS

Run: `node --test tests/codex-runtime/plan-execution-core.test.mjs tests/codex-runtime/plan-execution-cli.test.mjs`
Expected: PASS

Run: `bash tests/codex-runtime/test-core-helper-runtime-launch.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-config.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-workflow-status.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-workflow.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-plan-execution.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-install-runtime.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-migrate-install.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-superpowers-upgrade-skill.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-workflow-sequencing.sh`
Expected: PASS

Run: `bash tests/codex-runtime/test-runtime-instructions.sh`
Expected: PASS

- [ ] **Step 5: Commit the coordinated cutover release surface**
```bash
git add bin runtime/core-helpers tests/codex-runtime README.md .codex/INSTALL.md .copilot/INSTALL.md docs/README.codex.md docs/README.copilot.md docs/testing.md RELEASE-NOTES.md
git commit -m "feat: ship bundled core helper runtime"
```
