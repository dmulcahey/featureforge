# Prompt-Surface Reduction and Skill-Doc Compaction Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>` after engineering approval; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-30-prompt-surface-reduction-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

**Goal:** Reduce top-level generated skill-doc prompt surface while preserving fail-closed operational contract behavior.

**Architecture:** Keep top-level `SKILL.md` documents operational and compact, move non-load-bearing depth into companion references, and enforce the split with deterministic generation and contract/budget tests. Use shared generator compaction primitives first, then apply targeted template reductions in bounded file-ownership lanes.

**Tech Stack:** Node.js (`scripts/gen-skill-docs.mjs`), Markdown templates, existing `tests/codex-runtime/*` node test suites.

---

## Preconditions

- Approved source spec exists and remains `CEO Approved` at `docs/featureforge/specs/2026-03-30-prompt-surface-reduction-design.md`.
- Node test harness is available locally (`node --test`).
- Implementation follows `featureforge:test-driven-development` per task and closes with `featureforge:verification-before-completion` evidence.

## Existing Capabilities / Built-ins to Reuse

- `scripts/gen-skill-docs.mjs` already controls shared generation behavior.
- `tests/codex-runtime/skill-doc-contracts.test.mjs` already verifies skill-doc contract surfaces.
- `tests/codex-runtime/skill-doc-generation.test.mjs` already verifies generation behavior.
- Existing `REFERENCE.md` pattern in repo docs can be reused for skill-companion docs.

## Known Footguns / Constraints

- Do not move mandatory helper commands, stop conditions, or approval law out of top-level `SKILL.md`.
- Do not introduce new runtime dependencies for generation/runtime flows.
- Do not hand-edit generated `skills/*/SKILL.md` without corresponding `.tmpl`/generator updates and regeneration.
- Budget reductions are invalid if behavioral contract tests regress.

## Change Surface

- Generator: `scripts/gen-skill-docs.mjs`
- Target templates: `skills/plan-ceo-review/SKILL.md.tmpl`, `skills/plan-eng-review/SKILL.md.tmpl`, `skills/finishing-a-development-branch/SKILL.md.tmpl`, `skills/subagent-driven-development/SKILL.md.tmpl`, `skills/requesting-code-review/SKILL.md.tmpl`
- Generated outputs: corresponding `skills/*/SKILL.md`
- Companion refs: targeted `skills/*/REFERENCE.md` and optional `skills/*/CHECKLIST.md`
- Tests: `tests/codex-runtime/skill-doc-contracts.test.mjs`, `tests/codex-runtime/skill-doc-generation.test.mjs`, plus a new budget-focused test file

## Requirement Coverage Matrix

- REQ-001 -> Task 4, Task 5, Task 6
- REQ-002 -> Task 2, Task 4, Task 5, Task 6
- REQ-003 -> Task 4, Task 5, Task 6
- REQ-004 -> Task 2, Task 6
- REQ-005 -> Task 3
- REQ-006 -> Task 3
- REQ-007 -> Task 2, Task 3, Task 7
- REQ-008 -> Task 2
- REQ-009 -> Task 4, Task 5, Task 6, Task 7
- REQ-010 -> Task 1, Task 7
- REQ-011 -> Task 3
- REQ-012 -> Task 7

## Execution Strategy

- After baseline kickoff, create two worktrees and run Tasks 1 and 2 in parallel:
  - Task 1 owns budget harness and baseline size assertions.
  - Task 2 owns shared generator compaction primitives and contract updates.
- Execute Task 3 serially after Tasks 1 and 2. It edits shared generator/pointer behavior after both baseline and shared-compaction slices are in place.
- Execute Task 4 serially after Task 3. It introduces `plan-ceo-review` compaction while updating shared contract-test hotspots.
- Execute Task 5 serially after Task 4. It updates the same shared contract-test hotspots while compacting `plan-eng-review`.
- Execute Task 6 serially after Task 5. It continues shared contract-test hotspot updates while compacting the remaining high-volume skills.
- Execute Task 7 serially after Tasks 1, 4, 5, and 6. It is the reintegration, full-regeneration, evidence, and hotspot-ordering gate for shared verification files.

## Dependency Diagram

```text
Task 1 -> Task 3
Task 2 -> Task 3
Task 3 -> Task 4
Task 4 -> Task 5
Task 5 -> Task 6
Task 1 -> Task 7
Task 4 -> Task 7
Task 5 -> Task 7
Task 6 -> Task 7
```

## Task 1: Baseline Budget Harness

**Spec Coverage:** REQ-010
**Task Outcome:** The test suite enforces fail-closed targeted-skill and aggregate prompt-surface budget thresholds.
**Plan Constraints:**
- Budget checks must be deterministic, reviewable in CI output, and include an aggregate fail-closed threshold.
- Baseline values must be derived from generated artifacts, not hardcoded guesses.
**Open Questions:** none

**Files:**
- Create: `tests/codex-runtime/skill-doc-budgets.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-generation.test.mjs`
- Test: `tests/codex-runtime/skill-doc-budgets.test.mjs`

- [ ] **Step 1: Write failing budget tests for targeted and aggregate thresholds**

Add tests that assert baseline capture and fail-closed thresholds for high-priority skills plus aggregate generated `skills/*/SKILL.md` size.

- [ ] **Step 2: Run budget tests and confirm initial failure**

Run: `node --test tests/codex-runtime/skill-doc-budgets.test.mjs`
Expected: FAIL because budget helpers and thresholds are not yet wired.

- [ ] **Step 3: Implement minimal budget helpers in test code**

Add file-reading/line-count helpers and stable assertions for target files and aggregate generated skill-doc size.

- [ ] **Step 4: Re-run budget tests and confirm pass**

Run: `node --test tests/codex-runtime/skill-doc-budgets.test.mjs`
Expected: PASS with explicit counts in output.

- [ ] **Step 5: Commit Task 1**

Run: `git add tests/codex-runtime/skill-doc-budgets.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs && git commit -m "test: add skill-doc budget harness"`

## Task 2: Generator Shared-Block Compaction

**Spec Coverage:** REQ-002, REQ-004, REQ-007, REQ-008
**Task Outcome:** Shared generated sections are compacted without removing mandatory operational law.
**Plan Constraints:**
- Keep command-level and fail-closed language intact.
- Do not add new runtime dependencies.
**Open Questions:** none

**Files:**
- Modify: `scripts/gen-skill-docs.mjs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add failing contract checks for compact shared wording guarantees**

Extend contract tests to assert required law remains while redundant long-form blocks shrink.

- [ ] **Step 2: Run contract test file and capture failures**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: FAIL on new compactness/contract assertions.

- [ ] **Step 3: Implement generator compaction primitives**

Refactor shared-block emitters in `scripts/gen-skill-docs.mjs` with concise templates preserving required phrases.

- [ ] **Step 4: Re-run contract tests and confirm pass**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit Task 2**

Run: `git add scripts/gen-skill-docs.mjs tests/codex-runtime/skill-doc-contracts.test.mjs && git commit -m "refactor: compact shared skill-doc generator sections"`

## Task 3: Companion Pointer Contract

**Spec Coverage:** REQ-005, REQ-006, REQ-007, REQ-011
**Task Outcome:** Generator emits concise companion-reference pointers only when companion files exist, with fail-closed tests for presence/absence and pointer concision.
**Plan Constraints:**
- Pointer text must stay short and non-boilerplate.
- Missing companions must not break top-level safety or generation.
**Open Questions:** none

**Files:**
- Modify: `scripts/gen-skill-docs.mjs`
- Modify: `tests/codex-runtime/skill-doc-generation.test.mjs`
- Test: `tests/codex-runtime/skill-doc-generation.test.mjs`

- [ ] **Step 1: Add failing generation tests for pointer presence/absence and concision behavior**

Define assertions for pointer emitted only with existing companion files and failing checks for non-concise pointer blocks.

- [ ] **Step 2: Run generation tests and confirm failure**

Run: `node --test tests/codex-runtime/skill-doc-generation.test.mjs`
Expected: FAIL on new pointer behavior assertions.

- [ ] **Step 3: Implement pointer logic in generator**

Add concise conditional pointer emission and normalization rules (canonical short pointer block) based on companion file existence.

- [ ] **Step 4: Re-run generation tests and confirm pass**

Run: `node --test tests/codex-runtime/skill-doc-generation.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit Task 3**

Run: `git add scripts/gen-skill-docs.mjs tests/codex-runtime/skill-doc-generation.test.mjs && git commit -m "feat: add companion pointer generation contract"`

## Task 4: Compaction Lane - plan-ceo-review

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-009
**Task Outcome:** `plan-ceo-review` top-level docs retain load-bearing law while moving deep rationale/examples into companion references.
**Plan Constraints:**
- Keep workflow-critical sections executable in top-level doc.
- Target at least one-third line reduction from current baseline.
**Open Questions:** none

**Files:**
- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Create: `skills/plan-ceo-review/REFERENCE.md`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Write failing contract assertions specific to plan-ceo-review retained law**

Add checks that required sections/commands remain in top-level output.

- [ ] **Step 2: Run targeted contract tests and confirm failure**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: FAIL on new plan-ceo-review assertions.

- [ ] **Step 3: Compact template and extract reference content**

Edit `.tmpl`, author `REFERENCE.md`, regenerate docs with `node scripts/gen-skill-docs.mjs`.

- [ ] **Step 4: Re-run contracts and verify line-budget delta**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-budgets.test.mjs`
Expected: PASS and measurable line reduction.

- [ ] **Step 5: Commit Task 4**

Run: `git add skills/plan-ceo-review/SKILL.md.tmpl skills/plan-ceo-review/REFERENCE.md skills/plan-ceo-review/SKILL.md tests/codex-runtime/skill-doc-contracts.test.mjs && git commit -m "docs: compact plan-ceo-review skill surface"`

## Task 5: Compaction Lane - plan-eng-review

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-009
**Task Outcome:** `plan-eng-review` top-level docs retain approval/topology law while moving extended examples into companion references.
**Plan Constraints:**
- Preserve approval-header and topology contract instructions in top-level output.
- Target at least one-third line reduction from current baseline.
**Open Questions:** none

**Files:**
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Create: `skills/plan-eng-review/REFERENCE.md`
- Modify: `skills/plan-eng-review/SKILL.md`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Write failing contract assertions specific to plan-eng-review retained law**

Add checks for required approval/topology clauses in top-level output.

- [ ] **Step 2: Run targeted contract tests and confirm failure**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: FAIL on new plan-eng-review assertions.

- [ ] **Step 3: Compact template and extract reference content**

Edit `.tmpl`, add `REFERENCE.md`, and regenerate docs.

- [ ] **Step 4: Re-run contracts and verify budget pass**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-budgets.test.mjs`
Expected: PASS with required line reduction.

- [ ] **Step 5: Commit Task 5**

Run: `git add skills/plan-eng-review/SKILL.md.tmpl skills/plan-eng-review/REFERENCE.md skills/plan-eng-review/SKILL.md tests/codex-runtime/skill-doc-contracts.test.mjs && git commit -m "docs: compact plan-eng-review skill surface"`

## Task 6: Compaction Lane - Remaining High-Volume Skills

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-004, REQ-009
**Task Outcome:** The remaining three priority skills are compacted with companion references while preserving required execution/review law.
**Plan Constraints:**
- Keep each skill's load-bearing sequencing/routing constraints top-level.
- Keep shared contract-test hotspot edits consistent with the serial Task 4 -> Task 5 -> Task 6 sequence.
**Open Questions:** none

**Files:**
- Modify: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Create: `skills/finishing-a-development-branch/REFERENCE.md`
- Create: `skills/subagent-driven-development/REFERENCE.md`
- Create: `skills/requesting-code-review/REFERENCE.md`
- Modify: `skills/finishing-a-development-branch/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `skills/requesting-code-review/SKILL.md`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add failing contract assertions for the three targeted skills**

Add skill-specific contract checks for retained top-level operational law.

- [ ] **Step 2: Run targeted contract tests and confirm failure**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: FAIL on new assertions.

- [ ] **Step 3: Compact templates, add references, regenerate docs**

Edit three `.tmpl` files, add `REFERENCE.md` companions, run generator.

- [ ] **Step 4: Re-run contract and budget tests**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-budgets.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit Task 6**

Run: `git add skills/finishing-a-development-branch/SKILL.md.tmpl skills/subagent-driven-development/SKILL.md.tmpl skills/requesting-code-review/SKILL.md.tmpl skills/finishing-a-development-branch/REFERENCE.md skills/subagent-driven-development/REFERENCE.md skills/requesting-code-review/REFERENCE.md skills/finishing-a-development-branch/SKILL.md skills/subagent-driven-development/SKILL.md skills/requesting-code-review/SKILL.md tests/codex-runtime/skill-doc-contracts.test.mjs && git commit -m "docs: compact remaining high-volume skill docs"`

## Task 7: Reintegration, Full Verification, and Evidence

**Spec Coverage:** REQ-007, REQ-009, REQ-010, REQ-012
**Task Outcome:** All compaction lanes are integrated, regenerated, verified, and documented with before/after evidence.
**Plan Constraints:**
- Final verification must include generator + contract + generation + budget suites.
- Evidence must include explicit line-count deltas for targeted skills and aggregate total.
**Open Questions:** none

**Files:**
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-generation.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-budgets.test.mjs`
- Create: `docs/featureforge/execution-evidence/2026-03-30-prompt-surface-reduction-r1-evidence.md`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Regenerate all targeted skill docs from current templates**

Run: `node scripts/gen-skill-docs.mjs`
Expected: Generation completes with no errors.

- [ ] **Step 2: Run full relevant verification suite**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs tests/codex-runtime/skill-doc-budgets.test.mjs`
Expected: PASS, including aggregate budget assertions.

- [ ] **Step 3: Capture before/after line-count evidence**

Run: `wc -l skills/plan-ceo-review/SKILL.md skills/plan-eng-review/SKILL.md skills/finishing-a-development-branch/SKILL.md skills/subagent-driven-development/SKILL.md skills/requesting-code-review/SKILL.md` and `find skills -name SKILL.md -type f | sort | xargs wc -l | tail -n 1`.

- [ ] **Step 4: Write execution evidence artifact with requirement traceability**

Include verification commands, outputs, and REQ coverage outcomes in evidence markdown.

- [ ] **Step 5: Commit Task 7**

Run: `git add docs/featureforge/execution-evidence/2026-03-30-prompt-surface-reduction-r1-evidence.md skills tests/codex-runtime scripts/gen-skill-docs.mjs && git commit -m "feat: deliver prompt-surface reduction compaction plan"`

## Evidence Expectations

- Before/after line counts for each priority skill.
- Aggregate `skills/*/SKILL.md` line-count delta.
- Verification command transcript with pass/fail outcomes.
- Explicit mapping from executed tasks back to REQ IDs.

## Validation Strategy

- Targeted TDD cycle per task (failing test -> minimal implementation -> passing test).
- Full regeneration and contract suite at reintegration.
- Budget suite as a required gate, not an informational report.

## Documentation Update Expectations

- Add companion `REFERENCE.md` documents for extracted long-form guidance.
- Keep top-level `SKILL.md` concise and operational.
- Reflect budget and verification evidence in execution-evidence artifact.

## Rollout Plan

1. Run Tasks 1 and 2 in parallel, then execute Task 3 serially as the shared-generator seam.
2. Execute Tasks 4-6 serially to keep shared contract-test hotspot edits ordered.
3. Complete Task 7 reintegration gates.
4. Submit for `featureforge:plan-eng-review` after plan-fidelity pass.

## Rollback Plan

1. Revert compaction commits that cause contract regressions.
2. Regenerate from pre-change templates.
3. Re-run contract/generation/budget tests until baseline passes.

## Risks and Mitigations

- Risk: accidental removal of safety-critical clauses.
  - Mitigation: contract tests for each targeted skill before and after compaction.
- Risk: non-deterministic generation causing noisy diffs.
  - Mitigation: generation test coverage and deterministic render ordering assertions.
- Risk: over-serialization or file-ownership conflict during execution.
  - Mitigation: keep only Tasks 1-2 parallel, then serialize shared-hotspot tasks (4-6) and close with a final serial reintegration gate.

## Engineering Review Summary

**Review Status:** clear
**Reviewed At:** 2026-03-30T13:01:17Z
**Review Mode:** small_change
**Reviewed Plan Revision:** 1
**Critical Gaps:** 0
**Browser QA Required:** no
**Test Plan Artifact:** `/Users/davidmulcahey/.featureforge/projects/dmulcahey-featureforge/davidmulcahey-current-test-plan-20260330-090117.md`
**Outside Voice:** skipped
