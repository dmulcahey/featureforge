# Execution Evidence: 2026-03-21-using-superpowers-bypass

**Plan Path:** docs/superpowers/plans/2026-03-21-using-superpowers-bypass.md
**Plan Revision:** 1

## Step Evidence

### Task 1 Step 1
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:38:40Z
**Execution Source:** superpowers:executing-plans
**Claim:** Added red generator and contract assertions for the dedicated using-superpowers bootstrap
**Files:**
- None (no repo file changed)
**Verification:**
- Manual inspection only: Inspected the new test cases in gen-skill-docs.unit.test.mjs and skill-doc-contracts.test.mjs.
**Invalidation Reason:** N/A

### Task 1 Step 2
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:39:03Z
**Execution Source:** superpowers:executing-plans
**Claim:** Ran the Task 1 red test command and confirmed the bootstrap contract currently fails
**Files:**
- None (no repo file changed)
**Verification:**
- `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs` -> failed as expected: missing bypass helper exports and using-superpowers still uses the shared preamble
**Invalidation Reason:** N/A

### Task 1 Step 3
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:41:31Z
**Execution Source:** superpowers:executing-plans
**Claim:** Implemented dedicated using-superpowers shell-line and bypass-gate builders in the skill-doc generator
**Files:**
- scripts/gen-skill-docs.mjs
- tests/codex-runtime/gen-skill-docs.unit.test.mjs
- tests/codex-runtime/skill-doc-contracts.test.mjs
**Verification:**
- Manual inspection only: Inspected the new generator exports and red tests covering the dedicated bootstrap contract.
**Invalidation Reason:** N/A

### Task 1 Step 4
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:41:49Z
**Execution Source:** superpowers:executing-plans
**Claim:** Wired BASE_PREAMBLE rendering so using-superpowers resolves through its dedicated bootstrap path
**Files:**
- scripts/gen-skill-docs.mjs
- skills/using-superpowers/SKILL.md
**Verification:**
- Manual inspection only: Regenerated skills and confirmed the on-disk using-superpowers preamble now derives the session decision path without session markers or contributor state.
**Invalidation Reason:** N/A

### Task 1 Step 5
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:42:05Z
**Execution Source:** superpowers:executing-plans
**Claim:** Re-ran the focused generator and contract tests and confirmed the dedicated using-superpowers bootstrap passes
**Files:**
- scripts/gen-skill-docs.mjs
- skills/using-superpowers/SKILL.md
- tests/codex-runtime/gen-skill-docs.unit.test.mjs
- tests/codex-runtime/skill-doc-contracts.test.mjs
**Verification:**
- `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs` -> passed
**Invalidation Reason:** N/A

### Task 1 Step 6
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:42:41Z
**Execution Source:** superpowers:executing-plans
**Claim:** Committed the Task 1 dedicated bootstrap foundation
**Files:**
- None (no repo file changed)
**Verification:**
- Manual inspection only: Created commit 942be7d with the generator/bootstrap changes and matching execution evidence.
**Invalidation Reason:** N/A

### Task 2 Step 1
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:43:32Z
**Execution Source:** superpowers:executing-plans
**Claim:** Added red runtime-instructions assertions for the using-superpowers bypass gate wording
**Files:**
- tests/codex-runtime/test-runtime-instructions.sh
**Verification:**
- Manual inspection only: Inspected the new using-superpowers runtime-instructions patterns for the opt-out gate, decision path, and malformed-state wording.
**Invalidation Reason:** N/A

### Task 2 Step 2
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:44:12Z
**Execution Source:** superpowers:executing-plans
**Claim:** Ran the runtime-instructions contract check and confirmed the bypass-gate wording is still missing
**Files:**
- None (no repo file changed)
**Verification:**
- `bash tests/codex-runtime/test-runtime-instructions.sh` -> failed as expected: using-superpowers is missing the new bypass-gate wording
**Invalidation Reason:** N/A

### Task 2 Step 3
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:45:05Z
**Execution Source:** superpowers:executing-plans
**Claim:** Updated the using-superpowers template to include the generator-owned bypass gate contract
**Files:**
- scripts/gen-skill-docs.mjs
- skills/using-superpowers/SKILL.md.tmpl
**Verification:**
- Manual inspection only: Confirmed the template now includes the bypass-gate placeholder and the generator helper emits the required wording.
**Invalidation Reason:** N/A

### Task 2 Step 4
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:46:14Z
**Execution Source:** superpowers:executing-plans
**Claim:** Regenerated the using-superpowers skill doc from the updated template and generator
**Files:**
- scripts/gen-skill-docs.mjs
- skills/using-superpowers/SKILL.md
- skills/using-superpowers/SKILL.md.tmpl
**Verification:**
- Manual inspection only: Ran node scripts/gen-skill-docs.mjs and inspected the generated using-superpowers SKILL.md output.
**Invalidation Reason:** N/A

### Task 2 Step 5
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:46:33Z
**Execution Source:** superpowers:executing-plans
**Claim:** Aligned runtime-facing docs with the using-superpowers session bypass gate
**Files:**
- README.md
- docs/README.codex.md
- docs/README.copilot.md
- tests/codex-runtime/skill-doc-contracts.test.mjs
**Verification:**
- Manual inspection only: Updated the README surfaces and the generated-doc contract test to describe the gated entry router instead of unconditional takeover.
**Invalidation Reason:** N/A

### Task 2 Step 6
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-21T15:47:07Z
**Execution Source:** superpowers:executing-plans
**Claim:** Re-ran the freshness and runtime-instructions checks and confirmed the bypass-gate wording passes
**Files:**
- README.md
- docs/README.codex.md
- docs/README.copilot.md
- skills/using-superpowers/SKILL.md
- skills/using-superpowers/SKILL.md.tmpl
- tests/codex-runtime/skill-doc-contracts.test.mjs
- tests/codex-runtime/test-runtime-instructions.sh
**Verification:**
- Manual inspection only: node scripts/gen-skill-docs.mjs --check and bash tests/codex-runtime/test-runtime-instructions.sh both passed.
**Invalidation Reason:** N/A
