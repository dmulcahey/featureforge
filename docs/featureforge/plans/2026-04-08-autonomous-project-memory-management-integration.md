# Autonomous Project Memory Management Integration Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>` after engineering approval; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 2
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-04-08-autonomous-project-memory-management-integration-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review
**QA Requirement:** not-required

**Goal:** Implement autonomous, non-blocking project-memory behavior across FeatureForge skills with `featureforge:project-memory` as the sole writer and contract tests that enforce ownership, authority, and review-independence boundaries.

**Architecture:** Deliver this as a contract-first documentation-and-tests slice. Each task updates a bounded skill cluster and corresponding tests, regenerates checked-in skill docs, and proves behavior through targeted suites before proceeding. Keep runtime command surfaces untouched and encode policy through template text plus deterministic contract assertions.

**Tech Stack:** Markdown skill templates (`skills/*/SKILL.md.tmpl`), generated skill docs (`skills/*/SKILL.md`), Node contract tests (`tests/codex-runtime/*.mjs`), Rust instruction/routing contract tests (`tests/runtime_instruction_contracts.rs`, `tests/using_featureforge_skill.rs`)

---

## Plan Contract

This plan defines implementation order, task ownership, and validation for the approved autonomous project-memory integration spec. If this plan and the approved spec diverge, the approved spec wins and this plan must be updated in the same change.

## Existing Capabilities / Built-ins to Reuse

- `tests/codex-runtime/skill-doc-contracts.test.mjs` already enforces cross-skill wording/contract invariants and is the primary enforcement surface for this spec.
- `tests/runtime_instruction_contracts.rs` already validates AGENTS/skill authority boundaries and late-stage workflow ordering language.
- `tests/using_featureforge_skill.rs` already validates explicit project-memory routing in `using-featureforge`.
- `scripts/gen-skill-docs.mjs` already regenerates checked-in `skills/*/SKILL.md` from `.tmpl` sources.
- Existing project-memory references under `skills/project-memory/` (authority boundaries + examples) already carry reject-vocabulary and file-intent semantics.

## Known Footguns / Constraints

- Generated skill docs must be regenerated whenever `.tmpl` files change.
- This scope is skill-library and contract-test only. Do not introduce runtime stages, helpers, or gate commands.
- `docs/project_notes/*` remains supportive memory; policy text must not invert authority order.
- `AGENTS.md` is instruction authority and must remain explicit-only for memory updates.
- `tests/codex-runtime/skill-doc-contracts.test.mjs` is a hotspot touched by multiple tasks; sequence changes to avoid parallel merge conflicts and flaky intermediate states.

## Cross-Task Invariants

- Use `featureforge:test-driven-development` discipline per task: add/update failing assertions first, then implement minimal text changes to make them pass.
- Keep autonomous memory non-blocking in every affected skill.
- Keep `featureforge:project-memory` as the only repo-visible memory writer.
- Preserve review-skill independence (no autonomous project-memory consult injection in approval/final-review skills).
- Run `node scripts/gen-skill-docs.mjs` after any template change before evaluating contract test outcomes.

## Change Surface

- Project-memory authority and examples:
  - `skills/project-memory/SKILL.md.tmpl`
  - `skills/project-memory/SKILL.md`
  - `skills/project-memory/authority-boundaries.md`
  - `skills/project-memory/examples.md`
- Read-side consult and execution-capture skill templates:
  - `skills/brainstorming/SKILL.md.tmpl`, `skills/brainstorming/SKILL.md`
  - `skills/writing-plans/SKILL.md.tmpl`, `skills/writing-plans/SKILL.md`
  - `skills/systematic-debugging/SKILL.md.tmpl`, `skills/systematic-debugging/SKILL.md`
  - `skills/executing-plans/SKILL.md.tmpl`, `skills/executing-plans/SKILL.md`
  - `skills/subagent-driven-development/SKILL.md.tmpl`, `skills/subagent-driven-development/SKILL.md`
- Late-stage ownership and routing:
  - `skills/document-release/SKILL.md.tmpl`, `skills/document-release/SKILL.md`
  - `skills/finishing-a-development-branch/SKILL.md.tmpl`, `skills/finishing-a-development-branch/SKILL.md`
  - `skills/using-featureforge/SKILL.md.tmpl`, `skills/using-featureforge/SKILL.md`
  - `AGENTS.md`
- Contract tests:
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `tests/codex-runtime/project-memory-content.test.mjs`
  - `tests/runtime_instruction_contracts.rs`
  - `tests/using_featureforge_skill.rs`

## Preconditions

- Approved source spec exists at `docs/featureforge/specs/2026-04-08-autonomous-project-memory-management-integration-design.md` with:
  - `**Workflow State:** CEO Approved`
  - `**Spec Revision:** 1`
  - `**Last Reviewed By:** plan-ceo-review`
  - parseable `## Requirement Index`
- `node` is available for skill-doc generation and codex-runtime tests.
- Rust test tooling is available for targeted contract suites.

## Evidence Expectations

- Diff shows template-source updates for all affected skills plus regenerated `SKILL.md` outputs.
- Diff shows project-memory boundary text narrowed to `docs/project_notes/*` autonomous scope with explicit `AGENTS.md` carveout.
- Diff shows contract tests encoding allowlist ownership (`document-release` + debugging exception), deterministic idempotency/atomicity, and structured sweep-outcome reporting.
- Diff shows no new runtime stage/helper command additions.

## Validation Strategy

- Task-level targeted suites run after each task.
- Final required matrix (from approved spec REQ-026):
  - `node scripts/gen-skill-docs.mjs`
  - `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
  - targeted memory-integration suites touched by this work (`tests/codex-runtime/project-memory-content.test.mjs`, plus affected Rust contract suites)

## Documentation Update Expectations

- Skill docs must reflect the new ownership model and workflow timing (default sweep in `document-release`, no finish-stage writes).
- `AGENTS.md` project-memory section remains consult-first and explicit-writer guidance only.
- No historical specs/plans/docs are updated in this slice unless needed for direct contract alignment.

## Rollout Plan

1. Land contract assertions and project-memory boundary updates.
2. Land consult/ownership/routing skill updates in ordered slices.
3. Regenerate docs and close contract suites.
4. Run final validation matrix and hand off to `featureforge:plan-fidelity-review`.

## Rollback Plan

- Revert the full policy slice if contract suites reveal regressions that cannot be resolved without scope expansion.
- Do not partially keep new assertions while reverting core skill text (or vice versa); keep test and skill policy surfaces coherent.

## Risks and Mitigations

- Risk: wording drift across templates and generated docs.
  - Mitigation: regenerate after each template edit and run contract tests per task.
- Risk: accidental memory gating language.
  - Mitigation: explicit negative assertions for blocker phrasing in contract tests.
- Risk: ownership drift reintroduces autonomous writers outside allowlist.
  - Mitigation: allowlist assertions in contract tests plus explicit skill-boundary language.
- Risk: false review contamination via memory consults in review skills.
  - Mitigation: explicit no-auto-consult assertions for review skills.

## Execution Strategy

- Execute Task 1 serially. It establishes project-memory authority wording that downstream tasks and assertions depend on.
- Execute Task 2 serially after Task 1. It updates the same contract-test hotspot (`tests/codex-runtime/skill-doc-contracts.test.mjs`) and generated skill surfaces touched in Task 1.
- Execute Task 3 serially after Task 2. It revises late-stage ownership wording in shared generated skill outputs and reuses the same contract-test hotspot.
- Execute Task 4 serially after Task 3. It updates shared routing/authority surfaces (`skills/using-featureforge/SKILL.md`, `AGENTS.md`, and Rust contract tests) that must reconcile against prior late-stage changes.
- Execute Task 5 serially after Task 4. It revisits `skills/project-memory/examples.md` from Task 1 and finalizes shared contract assertions after all routing/ownership wording is stable.
- Execute Task 6 serially after Task 5. It is the deterministic final validation gate over the fully integrated branch state.

## Dependency Diagram

```text
Task 1 -> Task 2
Task 2 -> Task 3
Task 3 -> Task 4
Task 4 -> Task 5
Task 5 -> Task 6
```

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-002 -> Task 1
- REQ-003 -> Task 1
- REQ-004 -> Task 2
- REQ-005 -> Task 2
- REQ-006 -> Task 2
- REQ-007 -> Task 2
- REQ-008 -> Task 4
- REQ-009 -> Task 2
- REQ-010 -> Task 2
- REQ-011 -> Task 2
- REQ-012 -> Task 2
- REQ-013 -> Task 3
- REQ-014 -> Task 3
- REQ-015 -> Task 3
- REQ-016 -> Task 3
- REQ-017 -> Task 3
- REQ-018 -> Task 4
- REQ-019 -> Task 1, Task 3
- REQ-020 -> Task 1, Task 2, Task 3, Task 4, Task 5
- REQ-021 -> Task 1, Task 4, Task 5
- REQ-022 -> Task 1, Task 3
- REQ-023 -> Task 3
- REQ-024 -> Task 1, Task 3
- REQ-025 -> Task 3
- REQ-026 -> Task 6
- REQ-027 -> Task 3

## Task 1: Narrow Project-Memory Authority and Boundary Contracts

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-019, REQ-022, REQ-024  
**Task Outcome:** Project-memory skill docs explicitly enforce sole-writer authority, `docs/project_notes/*` autonomous scope, explicit-only `AGENTS.md` edits, and autonomous-owner allowlist semantics.  
**Plan Constraints:**
- Keep reject vocabulary centralized in project-memory boundary references.
- Preserve non-gating language.
- Keep AGENTS edits explicit-only.
**Open Questions:** none

**Files:**
- Modify: `skills/project-memory/SKILL.md.tmpl`
- Modify: `skills/project-memory/SKILL.md`
- Modify: `skills/project-memory/authority-boundaries.md`
- Modify: `skills/project-memory/examples.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add red assertions in `tests/codex-runtime/skill-doc-contracts.test.mjs` for narrowed autonomous write scope, explicit-only AGENTS edits, and allowlisted autonomous owners.**
- [ ] **Step 2: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm failures target project-memory boundary wording.**
- [ ] **Step 3: Update project-memory template and boundary/example docs to satisfy assertions (including document-release default owner + debugging exception references).**
- [ ] **Step 4: Regenerate skill docs via `node scripts/gen-skill-docs.mjs` and confirm `skills/project-memory/SKILL.md` reflects template changes.**
- [ ] **Step 5: Re-run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm green for Task 1 assertions.**
- [ ] **Step 6: Commit Task 1 scope.**

## Task 2: Add Consult-Only and Capture-First Rules to Planning/Debug/Execution Skills

**Spec Coverage:** REQ-004, REQ-005, REQ-006, REQ-007, REQ-009, REQ-010, REQ-011, REQ-012  
**Task Outcome:** Brainstorming/writing-plans/systematic-debugging consult rules and executing-plans/subagent capture-first rules are explicit, non-blocking, and authority-safe.  
**Plan Constraints:**
- Debugging write-through stays narrow and threshold-gated.
- Execution loops must not directly edit `docs/project_notes/*`.
- Missing/stale memory files must not block planning/debugging flows.
**Open Questions:** none

**Files:**
- Modify: `skills/brainstorming/SKILL.md.tmpl`
- Modify: `skills/brainstorming/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/systematic-debugging/SKILL.md.tmpl`
- Modify: `skills/systematic-debugging/SKILL.md`
- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add red assertions for optional consult language, approved-artifact precedence, strict debugging threshold, and execution capture-first/no-direct-memory-write language.**
- [ ] **Step 2: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm failures for the targeted skill set.**
- [ ] **Step 3: Patch the five skill templates with required wording and threshold semantics.**
- [ ] **Step 4: Regenerate checked-in skill docs with `node scripts/gen-skill-docs.mjs`.**
- [ ] **Step 5: Re-run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm Task 2 assertions are green.**
- [ ] **Step 6: Commit Task 2 scope.**

## Task 3: Encode Late-Stage Memory Ownership, Atomicity, and Outcome Reporting

**Spec Coverage:** REQ-013, REQ-014, REQ-015, REQ-016, REQ-017, REQ-019, REQ-022, REQ-023, REQ-024, REQ-025, REQ-027  
**Task Outcome:** Document-release owns default non-blocking sweep with deterministic merge/atomicity/reporting semantics, and finishing-a-development-branch explicitly remains outside memory ownership.  
**Plan Constraints:**
- Preserve non-blocking semantics across all sweep outcomes.
- Enforce source-filtering to authoritative/stable branch artifacts only; explicitly reject unsourced chat/transient narrative as sweep input.
- Require structured reporting for rejection and no-op paths.
- Do not introduce finish-stage memory writes.
**Open Questions:** none

**Files:**
- Modify: `skills/document-release/SKILL.md.tmpl`
- Modify: `skills/document-release/SKILL.md`
- Modify: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- Modify: `skills/finishing-a-development-branch/SKILL.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/runtime_instruction_contracts.rs`

- [ ] **Step 1: Add red assertions for document-release default ownership, zero-or-one sweep behavior, deterministic collision handling, source-filter enforcement (`authoritative/stable artifacts only`, `no unsourced chat narrative`), atomic multi-file semantics, and structured outcome reporting (including no-op passes).**
- [ ] **Step 2: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and `cargo nextest run --test runtime_instruction_contracts` to confirm red state for Task 3 deltas.**
- [ ] **Step 3: Update document-release and finishing templates to implement the approved ownership/reporting/atomicity model.**
- [ ] **Step 4: Regenerate skill docs with `node scripts/gen-skill-docs.mjs`.**
- [ ] **Step 5: Re-run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and `cargo nextest run --test runtime_instruction_contracts` to confirm green.**
- [ ] **Step 6: Commit Task 3 scope.**

## Task 4: Align Router, Review Independence, and AGENTS Guidance

**Spec Coverage:** REQ-008, REQ-018, REQ-021  
**Task Outcome:** Entry-router wording clarifies workflow-owned autonomous follow-up, AGENTS guidance aligns with explicit-only ownership, and review-skill surfaces remain memory-independent by default.  
**Plan Constraints:**
- `using-featureforge` must keep explicit project-memory routing only.
- Review skills must not gain autonomous project-memory consult hooks.
- AGENTS edits must preserve instruction-authority posture.
**Open Questions:** none

**Files:**
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `AGENTS.md`
- Modify: `tests/using_featureforge_skill.rs`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/using_featureforge_skill.rs`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add red assertions for router clarifier language and no-default-stack guarantee, plus explicit no-auto-memory-consult expectations for review skills.**
- [ ] **Step 2: Run targeted suites (`node --test tests/codex-runtime/skill-doc-contracts.test.mjs`, `cargo nextest run --test using_featureforge_skill --test runtime_instruction_contracts`) and confirm red assertions fire.**
- [ ] **Step 3: Patch router template + AGENTS wording (and any additional skill text only if required to satisfy independence assertions).**
- [ ] **Step 4: Regenerate skill docs with `node scripts/gen-skill-docs.mjs`.**
- [ ] **Step 5: Re-run targeted suites and confirm green.**
- [ ] **Step 6: Commit Task 4 scope.**

## Task 5: Final Contract Consolidation and Example Coverage

**Spec Coverage:** REQ-020, REQ-021  
**Task Outcome:** Contract tests fully enforce the new model, including updated project-memory example scenarios and wording boundaries, with no remaining stale assertions.  
**Plan Constraints:**
- Keep assertions focused on behavior contract, not incidental prose formatting.
- Ensure project-memory examples cover document-release sweep, no-delta skip, debugging write-through threshold, and tracker-drift rejection.
**Open Questions:** none

**Files:**
- Modify: `skills/project-memory/examples.md`
- Modify: `tests/codex-runtime/project-memory-content.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/project-memory-content.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add/adjust red assertions for the new examples and tightened non-gating ownership language.**
- [ ] **Step 2: Run `node --test tests/codex-runtime/project-memory-content.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm red state.**
- [ ] **Step 3: Patch examples and contract wording until assertions map exactly to approved-spec requirements.**
- [ ] **Step 4: Re-run `node --test tests/codex-runtime/project-memory-content.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm green.**
- [ ] **Step 5: Commit Task 5 scope.**

## Task 6: Run Required Validation Matrix and Prepare Review Handoff

**Spec Coverage:** REQ-026  
**Task Outcome:** Required validation matrix passes on the candidate `HEAD`, and the branch is ready for plan-fidelity review handoff with deterministic evidence.  
**Plan Constraints:**
- Do not claim completion without green matrix commands.
- Keep verification tied to the current candidate `HEAD`.
**Open Questions:** none

**Files:**
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/project-memory-content.test.mjs`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/using_featureforge_skill.rs`

- [ ] **Step 1: Run `node scripts/gen-skill-docs.mjs` and ensure generated docs are up to date.**
- [ ] **Step 2: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`.**
- [ ] **Step 3: Run `node --test tests/codex-runtime/project-memory-content.test.mjs`.**
- [ ] **Step 4: Run `cargo nextest run --test runtime_instruction_contracts --test using_featureforge_skill`.**
- [ ] **Step 5: Run plan-level sanity checks (`cargo clippy --all-targets --all-features -- -D warnings` only if Rust files changed in this implementation; otherwise document why it was skipped).**
- [ ] **Step 6: Record verification outputs in execution notes/handoff and commit final integration updates.**
