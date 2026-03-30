# First-Class Plan-Fidelity-Review Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>` after engineering approval; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-30-first-class-plan-fidelity-review-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** writing-plans

**Goal:** Make `featureforge:plan-fidelity-review` a first-class public workflow stage with correct routing, explicit independent-review guidance, and deterministic contract coverage.

**Architecture:** Keep runtime-owned fidelity receipt/schema validation intact while adding a first-class public skill surface and routing ownership. Update status/operator/session-entry and skill/docs surfaces together so runtime truth, workflow guidance, and tests stay in parity. Land test-first changes for routing and instruction contracts, then regenerate skill docs and verify both Rust and Node contract suites.

**Tech Stack:** Rust (`src/workflow`, `src/session_entry`), markdown skill templates/docs, Rust integration tests, Node skill-doc contract tests.

---

## Change Surface
- New public skill surface:
  - `skills/plan-fidelity-review/SKILL.md.tmpl`
  - `skills/plan-fidelity-review/reviewer-prompt.md`
  - generated `skills/plan-fidelity-review/SKILL.md`
- Routing/runtime surfaces:
  - `src/workflow/status.rs`
  - `src/workflow/operator.rs`
  - `src/session_entry/mod.rs`
- Existing skill templates/docs:
  - `skills/using-featureforge/SKILL.md.tmpl`
  - `skills/writing-plans/SKILL.md.tmpl`
  - `skills/plan-eng-review/SKILL.md.tmpl`
  - generated `SKILL.md` counterparts
- Public docs:
  - `README.md`
  - `docs/README.codex.md`
  - `docs/README.copilot.md`
- Tests:
  - `tests/runtime_instruction_plan_review_contracts.rs`
  - `tests/runtime_instruction_contracts.rs`
  - `tests/workflow_runtime.rs`
  - `tests/session_config_slug.rs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`

## Preconditions
- Approved spec headers must remain:
  - `**Workflow State:** CEO Approved`
  - `**Spec Revision:** 1`
  - `**Last Reviewed By:** plan-ceo-review`
- Spec `## Requirement Index` must remain parseable and unchanged in intent.
- Work must stay fail-closed for trust-boundary behavior.
- Generated skill docs must be regenerated from `.tmpl` sources.

## Existing Capabilities / Built-ins to Reuse
- `src/contracts/plan.rs` fidelity-gate reason code and receipt validation primitives.
- `src/execution/topology.rs` review artifact parse/validation for `plan-fidelity-review` stage and provenance fields.
- `src/workflow/status.rs` gate evaluation plumbing and route diagnostics scaffolding.
- Existing `workflow plan-fidelity record` CLI command and runtime-owned receipt persistence.

## Known Footguns / Constraints
- Do not invent a second fidelity artifact schema.
- Do not weaken independence checks (`fresh-context-subagent` remains canonical public provenance).
- Do not leave docs/skills showing a pipeline different from runtime routing.
- Keep route distinctions explicit:
  - plan authoring defects -> `featureforge:writing-plans`
  - fidelity receipt state defects -> `featureforge:plan-fidelity-review`

## Requirement Coverage Matrix
- REQ-001 -> Task 2, Task 6
- REQ-002 -> Task 3, Task 6
- REQ-003 -> Task 2, Task 6
- REQ-004 -> Task 2, Task 4, Task 6
- REQ-005 -> Task 3, Task 6
- REQ-006 -> Task 4, Task 6
- REQ-007 -> Task 3, Task 4, Task 6
- REQ-008 -> Task 2, Task 3, Task 6
- REQ-009 -> Task 2, Task 6
- REQ-010 -> Task 5
- REQ-011 -> Task 1, Task 4, Task 5, Task 6, Task 7

## Decision Alignment Matrix
- DEC-001 -> Task 2, Task 6
- DEC-002 -> Task 2, Task 4, Task 6
- DEC-003 -> Task 2, Task 7
- DEC-004 -> Task 3, Task 4, Task 6

## Execution Strategy
- Execute Task 1 serially first. It establishes failing contract expectations (`featureforge:test-driven-development`) and anchors the red/green baseline.
- Execute Task 2 serially after Task 1. It creates the new first-class skill surface and reviewer prompt that downstream docs/routing tasks depend on.
- Execute Task 3 serially after Task 2. It updates shared workflow docs and skill handoff templates that must stay aligned with the Task 2 stage contract.
- After Task 3, create two isolated worktrees and run Tasks 4 and 5 in parallel:
  - Task 4 owns workflow routing and operator remediation for draft plans blocked only on fidelity receipt state.
  - Task 5 owns session-entry explicit skill recognition and its session-key contract tests.
- Execute Task 6 serially after Tasks 4 and 5 merge. It is the integration seam for regenerated skill docs and cross-surface contract assertions.
- Execute Task 7 last as strict verification and fidelity-gate readiness proof (`featureforge:verification-before-completion`).

## Dependency Diagram
```text
Task 1 -> Task 2
Task 2 -> Task 3
Task 3 -> Task 4
Task 3 -> Task 5
Task 4 -> Task 6
Task 5 -> Task 6
Task 6 -> Task 7
```

## Fidelity Routing State Diagram
```text
approved spec + draft plan
        |
        v
evaluate_plan_fidelity_gate
        |
        +-- pass --------------------------------> next_skill=featureforge:plan-eng-review
        |
        +-- missing/stale/malformed/non-pass ----> next_skill=featureforge:plan-fidelity-review

separate branch:
plan absent/stale/malformed/contract-invalid ----> next_skill=featureforge:writing-plans
```

## Task 1: Pin Failing Contracts for First-Class Stage Routing

**Spec Coverage:** REQ-011
**Task Outcome:** Tests fail for the current missing first-class stage surface and mismatched routing/remediation text, creating a red baseline before implementation.
**Plan Constraints:**
- Follow `featureforge:test-driven-development` red-first behavior.
- Add deterministic assertions (no snapshot-only expectations).
**Open Questions:** none

**Files:**
- Modify: `tests/runtime_instruction_plan_review_contracts.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/runtime_instruction_contracts.rs`

- [ ] **Step 1: Add failing runtime-instruction contract assertions for first-class `plan-fidelity-review` stage expectations**
Run: `cargo test --test runtime_instruction_plan_review_contracts -- --nocapture`
Expected: FAIL on missing/newly tightened assertions.

- [ ] **Step 2: Add failing workflow route expectation for draft plan with non-pass fidelity gate to return `featureforge:plan-fidelity-review`**
Run: `cargo test --test workflow_runtime -- --nocapture`
Expected: FAIL on current `featureforge:writing-plans` route for receipt-state-only blockers.
Include explicit red tests for each REQ-006 receipt-state class: missing, stale, malformed, non-pass, and non-independent.

- [ ] **Step 3: Add failing instruction-contract assertions for handoff language in `writing-plans`, `plan-eng-review`, and `using-featureforge`**
Run: `cargo test --test runtime_instruction_contracts -- --nocapture`
Expected: FAIL on outdated wording and stage ownership.

## Task 2: Create First-Class `plan-fidelity-review` Skill Surface

**Spec Coverage:** REQ-001, REQ-003, REQ-004, REQ-008, REQ-009
**Task Outcome:** Repository contains a first-class public skill directory and reviewer prompt that codifies independent fresh-context fidelity review and runtime receipt recording.
**Plan Constraints:**
- Keep the stage verification-only; no plan rewriting ownership.
- Keep canonical reviewer source as `fresh-context-subagent` in first-class workflow guidance.
- Preserve runtime compatibility acceptance for `cross-model` provenance at low-level validators while forbidding first-class recommendation of `cross-model` in the new skill/prompt text.
**Open Questions:** none

**Files:**
- Create: `skills/plan-fidelity-review/SKILL.md.tmpl`
- Create: `skills/plan-fidelity-review/reviewer-prompt.md`
- Modify: `tests/runtime_instruction_plan_review_contracts.rs`

- [ ] **Step 1: Write template contract for `skills/plan-fidelity-review/SKILL.md.tmpl` with trigger, gate, artifact, and terminal handoff semantics**
Include explicit independent reviewer requirement and `workflow plan-fidelity record` command.

- [ ] **Step 2: Author `skills/plan-fidelity-review/reviewer-prompt.md` to enforce requirement-index and execution-topology fidelity checks**
Require parseable output fields used by runtime validation (`Review Stage`, `Review Verdict`, `Reviewer Source`, `Distinct From Stages`, `Verified Surfaces`, `Verified Requirement IDs`).

- [ ] **Step 3: Add or update contract assertions proving the new first-class skill directory and fresh-context provenance language**
Run: `cargo test --test runtime_instruction_plan_review_contracts -- --nocapture`
Expected: PASS for newly introduced skill-surface checks.

## Task 3: Align Public Docs and Skill Handoffs to First-Class Stage

**Spec Coverage:** REQ-002, REQ-005, REQ-007, REQ-008
**Task Outcome:** Public workflow sequence and handoff/remediation language consistently show `plan-fidelity-review` between writing and engineering review.
**Plan Constraints:**
- Edit `.tmpl` sources for generated skills; regenerate checked-in `SKILL.md` outputs.
- Keep distinctions between plan-authoring defects and fidelity-receipt defects explicit.
**Open Questions:** none

**Files:**
- Modify: `README.md`
- Modify: `docs/README.codex.md`
- Modify: `docs/README.copilot.md`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`

- [ ] **Step 1: Update workflow pipeline text in root and platform docs to include `plan-fidelity-review` stage explicitly**
Run: `rg -n "brainstorming ->|plan-fidelity-review|plan-eng-review" README.md docs/README.codex.md docs/README.copilot.md`
Expected: every canonical sequence includes `writing-plans -> plan-fidelity-review -> plan-eng-review`.

- [ ] **Step 2: Update `using-featureforge` routing text for draft-plan receipt-state blockers to route to `featureforge:plan-fidelity-review`**
Keep `writing-plans` route only for plan-authoring invalidity cases.

- [ ] **Step 3: Update `writing-plans` handoff text to invoke `featureforge:plan-fidelity-review` as first-class stage owner**
Keep explicit prohibition on jumping directly to `plan-eng-review` before pass receipt.

- [ ] **Step 4: Update `plan-eng-review` remediation text so missing/stale/malformed/non-pass/non-independent receipts route to `featureforge:plan-fidelity-review`**
Retain fail-closed prerequisite before engineering review starts.

## Task 4: Route Draft Plan Fidelity Gate Failures to First-Class Stage (Parallel Lane A)

**Spec Coverage:** REQ-004, REQ-006, REQ-007, REQ-011
**Task Outcome:** Runtime route and diagnostics send receipt-state-only blockers to `featureforge:plan-fidelity-review` while preserving writing-plans for true authoring defects.
**Plan Constraints:**
- Preserve existing reason-code vocabulary and fail-closed semantics.
- Do not alter receipt schema or acceptance rules.
- Keep DEC-002 compatibility boundary explicit: runtime may accept `cross-model`, but first-class route guidance must continue recommending `fresh-context-subagent`.
**Open Questions:** none

**Files:**
- Modify: `src/workflow/status.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Create isolated worktree for lane A**
Run: `git worktree add .worktrees/task4-fidelity-routing -b codex/task4-fidelity-routing`
Expected: clean lane worktree created.

- [ ] **Step 2: Update draft-plan branch in `status.rs` so failed fidelity gates route to `featureforge:plan-fidelity-review` when plan candidate is otherwise current**
Keep stale/invalid plan-content cases routed to `featureforge:writing-plans`.

- [ ] **Step 3: Update plan-fidelity remediation text and any operator guidance strings to reference `featureforge:plan-fidelity-review` for receipt-state blockers**
Run: `cargo test --test workflow_runtime -- --nocapture`
Expected: PASS with route and diagnostics parity.
Assert `next_skill=featureforge:plan-fidelity-review` for missing, stale, malformed, non-pass, and non-independent receipt states.

## Task 5: Add Session-Entry Recognition for Explicit Plan-Fidelity Requests (Parallel Lane B)

**Spec Coverage:** REQ-010, REQ-011
**Task Outcome:** Explicit user invocation of `plan-fidelity-review` is treated as FeatureForge re-entry intent.
**Plan Constraints:**
- Keep existing session-entry gate ownership and fail-closed behavior unchanged.
- Modify only recognition list/contract tests needed for this stage.
**Open Questions:** none

**Files:**
- Modify: `src/session_entry/mod.rs`
- Modify: `tests/session_config_slug.rs`

- [ ] **Step 1: Create isolated worktree for lane B**
Run: `git worktree add .worktrees/task5-session-entry-recognition -b codex/task5-session-entry-recognition`
Expected: clean lane worktree created.

- [ ] **Step 2: Add `plan-fidelity-review` to `FEATUREFORGE_SKILLS` explicit-skill recognition list**
Ensure no regressions to existing skill recognition behavior.

- [ ] **Step 3: Add/adjust session-entry test that verifies explicit skill-name re-entry for `plan-fidelity-review`**
Run: `cargo test --test session_config_slug -- --nocapture`
Expected: PASS including new explicit-skill case.

## Task 6: Regenerate Skill Docs and Lock Cross-Surface Contracts

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-004, REQ-005, REQ-006, REQ-007, REQ-008, REQ-009, REQ-011
**Task Outcome:** Generated skill docs, Rust instruction contracts, Node skill-doc contracts, and runtime routing tests all pass with the first-class stage semantics.
**Plan Constraints:**
- Regenerate docs from templates only (`node scripts/gen-skill-docs.mjs`).
- Keep assertions deterministic across Rust and Node suites.
- Keep DEC-002 dual contract under test: runtime-level compatibility for `cross-model` remains valid, while first-class docs/prompts must not recommend it.
**Open Questions:** none

**Files:**
- Modify: `skills/plan-fidelity-review/SKILL.md`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/runtime_instruction_plan_review_contracts.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Merge parallel lanes and resolve conflicts without changing approved behavior intent**
Run: `git merge codex/task4-fidelity-routing`
Run: `git merge codex/task5-session-entry-recognition`
Expected: clean merge or intentional conflict resolution.

- [ ] **Step 2: Regenerate checked-in skill docs**
Run: `node scripts/gen-skill-docs.mjs`
Expected: generated `SKILL.md` outputs updated to match templates.

- [ ] **Step 3: Run Rust instruction and routing contracts**
Run: `cargo test --test runtime_instruction_plan_review_contracts -- --nocapture`
Run: `cargo test --test runtime_instruction_contracts -- --nocapture`
Run: `cargo test --test workflow_runtime -- --nocapture`
Expected: PASS.
Include assertions that `cross-model` remains validator-compatible where contractually allowed and that first-class stage guidance still recommends `fresh-context-subagent`.

- [ ] **Step 4: Run Node skill-doc contract suite**
Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: PASS.

## Task 7: Final Verification, Plan Lint, and Plan-Fidelity Handoff Readiness

**Spec Coverage:** REQ-011
**Task Outcome:** Change set is warning-clean, target suites pass, and this plan artifact is lint-clean and synced for the dedicated `plan-fidelity-review` gate.
**Plan Constraints:**
- Use `featureforge:verification-before-completion` semantics; no completion claims without command evidence.
- Do not invoke `featureforge:plan-eng-review` directly from writing-plans output.
**Open Questions:** none

**Files:**
- Modify: `docs/featureforge/plans/2026-03-30-first-class-plan-fidelity-review.md`

- [ ] **Step 1: Run strict lint gate**
Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS with zero warnings.

- [ ] **Step 2: Run targeted Rust suites for this slice**
Run: `cargo test --test runtime_instruction_plan_review_contracts`
Run: `cargo test --test runtime_instruction_contracts`
Run: `cargo test --test workflow_runtime`
Run: `cargo test --test session_config_slug`
Expected: PASS.

- [ ] **Step 3: Run plan contract lint for approved spec + draft plan pair**
Run: `featureforge plan contract lint --spec docs/featureforge/specs/2026-03-30-first-class-plan-fidelity-review-design.md --plan docs/featureforge/plans/2026-03-30-first-class-plan-fidelity-review.md`
Expected: PASS.

- [ ] **Step 4: Sync plan artifact in workflow state**
Run: `featureforge workflow sync --artifact plan --path docs/featureforge/plans/2026-03-30-first-class-plan-fidelity-review.md`
Expected: workflow status references this plan path (or deterministic ambiguity diagnostics if pre-existing state conflicts remain).

- [ ] **Step 5: Invoke `featureforge:plan-fidelity-review` as the owning stage for independent review artifact generation and receipt recording**
Run: `featureforge workflow status --refresh`
Expected: route points to `featureforge:plan-fidelity-review` for this draft plan until a matching pass receipt exists.
If a direct command smoke check is needed, treat it as stage-owned output verification only:
Run: `featureforge workflow plan-fidelity record --plan docs/featureforge/plans/2026-03-30-first-class-plan-fidelity-review.md --review-artifact .featureforge/reviews/2026-03-30-first-class-plan-fidelity-review-plan-fidelity.md`
Expected: receipt records only when the independent stage artifact satisfies runtime validation.

## Evidence Expectations
- Every route change includes reason-code assertions and next-skill assertions.
- Every skill-template change includes regenerated `SKILL.md` artifacts.
- Session-entry recognition changes include explicit skill-name re-entry coverage.

## Validation Strategy
- Red tests first, then minimal implementation until green.
- Keep topology/routing and doc-contract suites in parity.
- Validate both Rust contract tests and Node skill-doc tests before completion.

## Coverage Graph
- First-class stage directory and generated skill doc exist -> automated (`runtime_instruction_plan_review_contracts`, `skill-doc-contracts.test.mjs`).
- Canonical docs pipeline includes `plan-fidelity-review` between plan-writing and engineering-review -> automated (`runtime_instruction_contracts`, Node skill-doc contracts where applicable).
- Draft plan with receipt-state-only blocker routes to `featureforge:plan-fidelity-review` -> automated (`workflow_runtime`).
- Receipt-state routing variants (missing/stale/malformed/non-pass/non-independent) all resolve to `featureforge:plan-fidelity-review` with deterministic reason-code parity -> automated (`workflow_runtime`, `contracts_spec_plan` where parser-level invalidity applies).
- Plan authoring defects still route to `featureforge:writing-plans` -> automated (`workflow_runtime`).
- `plan-eng-review` still fail-closes without matching pass receipt but remediation points to `plan-fidelity-review` -> automated (`runtime_instruction_plan_review_contracts`, `runtime_instruction_contracts`).
- Session-entry explicit skill recognition includes `plan-fidelity-review` -> automated (`session_config_slug`).
- DEC-002 dual contract (runtime compatibility acceptance for `cross-model` plus first-class non-recommendation) -> automated (`runtime_instruction_plan_review_contracts`, `runtime_instruction_contracts`, `skill-doc-contracts.test.mjs`).

## Documentation Update Expectations
- Keep workflow sequences consistent across root and platform docs.
- Keep generated skill docs in sync with template source edits in the same change.

## Rollout Plan
- Land skill/doc/routing/test updates in one integrated merge to avoid mixed-contract runtime/doc states.
- Prefer targeted suites first, then strict lint and broader regression commands.

## Rollback Plan
- Revert first-class stage routing changes in `status.rs`/`operator.rs` and session-entry recognition updates together.
- Revert corresponding skill/doc/template and contract-test updates in the same rollback change to maintain parity.

## Risks and Mitigations
- Risk: Route oscillation between writing-plans and plan-fidelity-review.
  - Mitigation: explicit branch conditions and regression tests for both route families.
- Risk: Guidance/runtime drift after template edits.
  - Mitigation: template-first edits plus regeneration and Node/Rust contract tests.
- Risk: Independence semantics diluted in reviewer prompt.
  - Mitigation: assert `fresh-context-subagent` and `Distinct From Stages` contract fields in tests.
