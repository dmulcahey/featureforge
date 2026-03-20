# Execution Evidence: 2026-03-19-core-helper-runtime-modernization

**Plan Path:** docs/superpowers/plans/2026-03-19-core-helper-runtime-modernization.md
**Plan Revision:** 2

## Step Evidence

### Task 1 Step 1
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:22:43Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Added the failing runtime workspace contract scaffold.
**Files:**
- tests/codex-runtime/runtime-build-contract.test.mjs
**Verification:**
- Manual inspection only: Added node:test coverage that asserts the new runtime workspace files exist.
**Invalidation Reason:** N/A

### Task 1 Step 2
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:22:47Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Ran the red runtime workspace contract test and confirmed the expected missing-file failure before scaffolding existed.
**Files:**
- None (no repo file changed)
**Verification:**
- Manual inspection only: Before adding the workspace files, node --test tests/codex-runtime/runtime-build-contract.test.mjs failed on missing runtime/core-helpers/package.json.
**Invalidation Reason:** N/A

### Task 1 Step 3
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:22:50Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Added the dedicated runtime workspace manifest, TypeScript config, build script, and base shared runtime modules.
**Files:**
- runtime/core-helpers/package.json
- runtime/core-helpers/scripts/build-runtime.mjs
- runtime/core-helpers/src/core/errors.ts
- runtime/core-helpers/src/platform/filesystem.ts
- runtime/core-helpers/src/platform/paths.ts
- runtime/core-helpers/src/platform/process.ts
- runtime/core-helpers/tsconfig.json
**Verification:**
- Manual inspection only: Added the isolated runtime/core-helpers workspace with Node 20 engine constraints, build scripts, and shared placeholder runtime modules.
**Invalidation Reason:** N/A

### Task 1 Step 4
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:22:55Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Added compileable placeholder CLI entrypoints for the three migrated helpers and wired deterministic bundle generation.
**Files:**
- runtime/core-helpers/src/cli/superpowers-config.ts
- runtime/core-helpers/src/cli/superpowers-plan-execution.ts
- runtime/core-helpers/src/cli/superpowers-workflow-status.ts
**Verification:**
- Manual inspection only: Added placeholder entrypoints that compile and fail closed with not-implemented messages while the runtime build script bundles them to dist/*.cjs.
**Invalidation Reason:** N/A

### Task 1 Step 5
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:23:00Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Generated the runtime lockfile and checked-in placeholder dist bundles.
**Files:**
- runtime/core-helpers/dist/superpowers-config.cjs
- runtime/core-helpers/dist/superpowers-plan-execution.cjs
- runtime/core-helpers/dist/superpowers-workflow-status.cjs
- runtime/core-helpers/package-lock.json
**Verification:**
- Manual inspection only: Ran npm --prefix runtime/core-helpers install and npm --prefix runtime/core-helpers run build to generate the lockfile and placeholder bundles.
**Invalidation Reason:** N/A

### Task 1 Step 6
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:23:06Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Added the runtime workspace artifacts to the runtime validation inventory.
**Files:**
- tests/codex-runtime/test-runtime-instructions.sh
**Verification:**
- Manual inspection only: Updated the runtime FILES inventory so validation now requires the runtime workspace manifest, build script, and checked-in dist artifacts.
**Invalidation Reason:** N/A

### Task 1 Step 7
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:23:24Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Re-ran the Task 1 green checks and confirmed the runtime workspace scaffold is fresh.
**Files:**
- None (no repo file changed)
**Verification:**
- `node --test tests/codex-runtime/runtime-build-contract.test.mjs && npm --prefix runtime/core-helpers run build:check` -> PASS
**Invalidation Reason:** N/A

### Task 1 Step 8
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:24:45Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Committed the runtime workspace scaffold in afbb4d9.
**Files:**
- docs/superpowers/execution-evidence/2026-03-19-core-helper-runtime-modernization-r2-evidence.md
- docs/superpowers/plans/2026-03-19-core-helper-runtime-modernization.md
- runtime/core-helpers/dist/superpowers-config.cjs
- runtime/core-helpers/dist/superpowers-plan-execution.cjs
- runtime/core-helpers/dist/superpowers-workflow-status.cjs
- runtime/core-helpers/package-lock.json
- runtime/core-helpers/package.json
- runtime/core-helpers/scripts/build-runtime.mjs
- runtime/core-helpers/src/cli/superpowers-config.ts
- runtime/core-helpers/src/cli/superpowers-plan-execution.ts
- runtime/core-helpers/src/cli/superpowers-workflow-status.ts
- runtime/core-helpers/src/core/errors.ts
- runtime/core-helpers/src/platform/filesystem.ts
- runtime/core-helpers/src/platform/paths.ts
- runtime/core-helpers/src/platform/process.ts
- runtime/core-helpers/tsconfig.json
- tests/codex-runtime/runtime-build-contract.test.mjs
- tests/codex-runtime/test-runtime-instructions.sh
**Verification:**
- Manual inspection only: Committed the Task 1 scaffold slice as afbb4d9 after the targeted runtime-build-contract and build:check verifications passed.
**Invalidation Reason:** N/A

### Task 2 Step 1
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:29:38Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Added failing staged-install, migrate-install delegation, and upgrade-skill regression coverage.
**Files:**
- tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh
- tests/codex-runtime/test-superpowers-install-runtime.sh
- tests/codex-runtime/test-superpowers-migrate-install.sh
- tests/codex-runtime/test-superpowers-upgrade-skill.sh
**Verification:**
- Manual inspection only: Added new staged install shell and PowerShell regression suites plus stricter migrate-install and upgrade-skill coverage that now require the staged runtime helper contract.
**Invalidation Reason:** N/A

### Task 2 Step 2
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:30:15Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Ran the staged-install red suites and confirmed the missing helper and upgrade-path failures.
**Files:**
- None (no repo file changed)
**Verification:**
- `bash tests/codex-runtime/test-superpowers-install-runtime.sh && bash tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh && bash tests/codex-runtime/test-superpowers-migrate-install.sh && bash tests/codex-runtime/test-superpowers-upgrade-skill.sh` -> FAIL: staged install helper entrypoints do not exist, migrate-install still owns install logic, and the upgrade skill still points at raw git pull.
**Invalidation Reason:** N/A

### Task 2 Step 3
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:42:10Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Implemented the staged install/update helper, PowerShell entrypoint, and migrate-install compatibility delegation.
**Files:**
- bin/superpowers-install-runtime
- bin/superpowers-install-runtime.ps1
- bin/superpowers-migrate-install
- bin/superpowers-migrate-install.ps1
- tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh
- tests/codex-runtime/test-superpowers-install-runtime.sh
- tests/codex-runtime/test-superpowers-migrate-install.sh
**Verification:**
- Manual inspection only: Added Node 20 preflight, staged clone and swap, bundled-runtime validation, existing-link repair, already-present copied-agent refresh, and a compatibility shim from migrate-install into the new staged helper.
**Invalidation Reason:** N/A

### Task 2 Step 4
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:42:17Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Routed upgrade guidance, install docs, and runtime-surface validation through superpowers-install-runtime.
**Files:**
- .codex/INSTALL.md
- .copilot/INSTALL.md
- README.md
- docs/README.codex.md
- docs/README.copilot.md
- docs/testing.md
- superpowers-upgrade/SKILL.md
- tests/codex-runtime/test-runtime-instructions.sh
- tests/codex-runtime/test-superpowers-upgrade-skill.sh
**Verification:**
- Manual inspection only: Updated the supported install and update docs to make superpowers-install-runtime the canonical path, kept migrate-install as a compatibility shim, and tightened the runtime contract tests around the new helper.
**Invalidation Reason:** N/A

### Task 2 Step 5
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:42:25Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Re-ran the staged install, migrate, upgrade-skill, and runtime-instructions suites until they all passed.
**Files:**
- None (no repo file changed)
**Verification:**
- `bash tests/codex-runtime/test-superpowers-install-runtime.sh && bash tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh && bash tests/codex-runtime/test-superpowers-migrate-install.sh && bash tests/codex-runtime/test-superpowers-upgrade-skill.sh && bash tests/codex-runtime/test-runtime-instructions.sh` -> PASS
**Invalidation Reason:** N/A

### Task 2 Step 6
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:43:28Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Committed the staged install/update slice in 46d6527.
**Files:**
- .codex/INSTALL.md
- .copilot/INSTALL.md
- README.md
- bin/superpowers-install-runtime
- bin/superpowers-install-runtime.ps1
- bin/superpowers-migrate-install
- bin/superpowers-migrate-install.ps1
- docs/README.codex.md
- docs/README.copilot.md
- docs/superpowers/execution-evidence/2026-03-19-core-helper-runtime-modernization-r2-evidence.md
- docs/superpowers/plans/2026-03-19-core-helper-runtime-modernization.md
- docs/testing.md
- superpowers-upgrade/SKILL.md
- tests/codex-runtime/test-runtime-instructions.sh
- tests/codex-runtime/test-superpowers-install-runtime-pwsh.sh
- tests/codex-runtime/test-superpowers-install-runtime.sh
- tests/codex-runtime/test-superpowers-migrate-install.sh
- tests/codex-runtime/test-superpowers-upgrade-skill.sh
**Verification:**
- Manual inspection only: Committed the Task 2 staged runtime install flow in 46d6527 after the staged install, PowerShell wrapper, migration delegation, upgrade-skill, and runtime-instructions suites all passed.
**Invalidation Reason:** N/A

### Task 3 Step 1
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:48:08Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Added failing config core, CLI, shared-launch, and wrapper regression coverage.
**Files:**
- tests/codex-runtime/config-cli.test.mjs
- tests/codex-runtime/config-core.test.mjs
- tests/codex-runtime/test-core-helper-runtime-launch.sh
- tests/codex-runtime/test-superpowers-config.sh
**Verification:**
- Manual inspection only: Added node:test config core and CLI suites plus shared-launch and wrapper smoke coverage that now require the Node-backed config runtime.
**Invalidation Reason:** N/A

### Task 3 Step 2
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:48:18Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Ran the config and runtime-launch red suites and confirmed the missing config runtime failures.
**Files:**
- None (no repo file changed)
**Verification:**
- `node --test tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs && bash tests/codex-runtime/test-core-helper-runtime-launch.sh && bash tests/codex-runtime/test-superpowers-config.sh` -> FAIL: runtime/core-helpers/src/core/config.ts is missing, the shared runtime launch helpers do not exist, and the config wrapper is still Bash-backed.
**Invalidation Reason:** N/A

### Task 3 Step 3
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:56:13Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Added shared shell and PowerShell runtime launch helpers for migrated commands.
**Files:**
- bin/superpowers-runtime-common.ps1
- bin/superpowers-runtime-common.sh
- tests/codex-runtime/test-core-helper-runtime-launch.sh
**Verification:**
- Manual inspection only: Added install-root-relative runtime launch helpers that fail closed on missing or unsupported Node, missing bundles, and invalid bundles while keeping PowerShell launch logic separate from the Bash-oriented helper.
**Invalidation Reason:** N/A

### Task 3 Step 4
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:56:24Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Ported the config core and CLI entrypoint to the bundled runtime while preserving the narrow line-oriented config contract.
**Files:**
- runtime/core-helpers/src/cli/superpowers-config.ts
- runtime/core-helpers/src/core/config.ts
- runtime/core-helpers/src/core/errors.ts
- runtime/core-helpers/src/platform/filesystem.ts
- tests/codex-runtime/config-cli.test.mjs
- tests/codex-runtime/config-core.test.mjs
**Verification:**
- Manual inspection only: Added the pure config module plus CLI adapter with last-match wins reads, replace-or-append writes, preserved unrelated lines, and explicit usage failures.
**Invalidation Reason:** N/A

### Task 3 Step 5
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:56:35Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Replaced the shipped config wrappers with Node-backed launchers and updated PowerShell coverage for the migrated helper.
**Files:**
- bin/superpowers-config
- bin/superpowers-config.ps1
- tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh
- tests/codex-runtime/test-superpowers-config.sh
**Verification:**
- Manual inspection only: Switched the config entrypoints to the shared runtime launchers and extended the PowerShell wrapper regression suite so the migrated config helper must bypass Git Bash and launch Node directly.
**Invalidation Reason:** N/A

### Task 3 Step 6
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-20T02:56:46Z
**Execution Source:** superpowers:subagent-driven-development
**Claim:** Rebuilt the bundled config runtime and reran the targeted config and wrapper suites until they all passed.
**Files:**
- None (no repo file changed)
**Verification:**
- `npm --prefix runtime/core-helpers run build && node --test tests/codex-runtime/config-core.test.mjs tests/codex-runtime/config-cli.test.mjs && bash tests/codex-runtime/test-core-helper-runtime-launch.sh && bash tests/codex-runtime/test-superpowers-config.sh && bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh && npm --prefix runtime/core-helpers run build:check` -> PASS
**Invalidation Reason:** N/A
