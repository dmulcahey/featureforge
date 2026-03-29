# Per-Task Review Gates Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>` after engineering approval; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-29-per-task-review-gates-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** writing-plans

**Goal:** Enforce per-task review + verification gates before cross-task advancement while preserving final whole-diff review and finish gating.

**Architecture:** Add authoritative task-boundary closure evaluation inside execution runtime state and enforce it at `begin` boundaries. Surface deterministic blocking reasons through workflow/operator status outputs, then align execution skills and tests so runtime and guidance stay contract-consistent.

**Tech Stack:** Rust (`src/execution`, `src/workflow`), Markdown skill templates/docs, Rust integration tests, Node contract tests.

---

## Change Surface
- Runtime execution gating logic in `src/execution/state.rs` and `src/execution/mutate.rs`
- Workflow phase/operator surfaces in `src/workflow/operator.rs` (and status parity where required)
- Execution skill contracts in:
  - `skills/executing-plans/SKILL.md.tmpl`
  - `skills/subagent-driven-development/SKILL.md.tmpl`
- Generated skill docs and instruction-contract tests
- Runtime/phase regression coverage in `tests/plan_execution.rs`, `tests/workflow_runtime.rs`, and shell/contract fixtures as needed

## Preconditions
- Approved spec headers remain:
  - `**Workflow State:** CEO Approved`
  - `**Spec Revision:** 1`
  - `**Last Reviewed By:** plan-ceo-review`
- Spec `## Requirement Index` remains parseable and unchanged in intent
- Work continues only on a non-protected feature branch/worktree

## Existing Capabilities / Built-ins to Reuse
- Existing `gate-review` provenance and serial unit-review receipt validation logic in `src/execution/state.rs`
- Existing runtime strategy checkpoint and cycle-break mechanics in `src/execution/transitions.rs`
- Existing workflow phase derivation and gate surfaces in `src/workflow/operator.rs`
- Existing skill doc generation flow in `node scripts/gen-skill-docs.mjs`

## Known Footguns / Constraints
- Do not weaken fail-closed provenance checks while adding task-boundary gates
- Do not introduce task-order heuristics from transient state; derive from authoritative artifacts
- Keep final whole-diff review and finish gates behaviorally intact
- Skill docs with `.md.tmpl` sources must be regenerated, not hand-edited only

## Requirement Coverage Matrix
- REQ-001 -> Task 2
- REQ-002 -> Task 1, Task 2
- REQ-003 -> Task 1, Task 2
- REQ-004 -> Task 1, Task 2
- REQ-005 -> Task 1, Task 2
- REQ-006 -> Task 3
- REQ-007 -> Task 3, Task 6
- REQ-008 -> Task 3, Task 6
- REQ-009 -> Task 5
- REQ-010 -> Task 5
- REQ-011 -> Task 1, Task 2
- REQ-012 -> Task 4, Task 5, Task 6

## Execution Strategy
- Execute Tasks 1 and 2 serially. Both tasks edit `src/execution/state.rs` and must land together to keep task-closure truth and begin-gate checks consistent.
- Execute Task 3 serially after Tasks 1 and 2. Workflow/operator routing must consume the new task-closure exports and begin-gate behavior from earlier tasks.
- After Task 3, create two isolated worktrees and run Tasks 4 and 5 in parallel:
  - Task 4 owns runtime gate regression tests and phase/status parity tests.
  - Task 5 owns skill-template contract updates, skill doc regeneration, and instruction contract tests.
- Execute Task 6 serially after Tasks 4 and 5. Task 6 is the reintegration and full-gate verification seam across runtime and skill-contract lanes.

## Dependency Diagram
```text
Task 1 -> Task 2
Task 1 -> Task 3
Task 2 -> Task 3
Task 3 -> Task 4
Task 3 -> Task 5
Task 4 -> Task 6
Task 5 -> Task 6
```

## Task 1: Define Authoritative Task-Closure Evaluation

**Spec Coverage:** REQ-002, REQ-003, REQ-004, REQ-005, REQ-011
**Task Outcome:** Runtime can evaluate whether a task is closed using authoritative per-step review provenance plus task-level verification evidence with fail-closed diagnostics.
**Plan Constraints:**
- Reuse existing provenance validators; do not fork duplicate trust logic.
- Keep cycle-break mechanics runtime-owned.
**Open Questions:** none

**Files:**
- Modify: `src/execution/state.rs`
- Modify: `src/execution/transitions.rs`
- Test: `tests/plan_execution.rs`

- [ ] **Step 1: Add failing test for missing task-boundary review/verification closure state**

Run: `cargo test --test plan_execution -- task_boundary_begin_blocked_without_prior_task_closure --exact`
Expected: FAIL with missing behavior assertion.

- [ ] **Step 2: Add helper(s) in `state.rs` to compute prior-task closure state from authoritative artifacts**

Implement pure helper path(s) for:
- prior task selection
- review-closure evaluation
- task-verification receipt evaluation

- [ ] **Step 3: Add/extend reason codes for task-boundary closure failures**

Add deterministic reason-code emission for:
- `prior_task_review_not_green`
- `prior_task_verification_missing`
- `task_cycle_break_active`
- malformed/legacy migration codes for missing verification receipts

- [ ] **Step 4: Wire helper coverage into existing runtime state export path**

Expose enough status data to support `begin` blocking and workflow/operator diagnostics without duplicating parsing logic.

- [ ] **Step 5: Extend tests for closure helpers and reason-code behavior**

Run: `cargo test --test plan_execution -- task_boundary_ --nocapture`
Expected: PASS for new helper and reason-code cases.

- [ ] **Step 6: Run targeted regression for cycle-break compatibility**

Run: `cargo test --test plan_execution -- cycle_break --nocapture`
Expected: PASS; no behavior regression in existing cycle-break tests.

## Task 2: Enforce Task-Closure Gate at Begin Boundary

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-004, REQ-005, REQ-011
**Task Outcome:** `plan execution begin` rejects Task `N+1` when Task `N` is not task-closed and returns deterministic failure class + reason code.
**Plan Constraints:**
- Enforce only on cross-task advancement; same-task resume semantics must remain valid.
- Do not alter approved plan scope or mutation ownership boundaries.
**Open Questions:** none

**Files:**
- Modify: `src/execution/mutate.rs`
- Modify: `src/execution/state.rs`
- Test: `tests/plan_execution.rs`

- [ ] **Step 1: Add failing begin-transition test for cross-task advancement without prior task closure**

Run: `cargo test --test plan_execution -- begin_blocks_cross_task_without_prior_task_closure --exact`
Expected: FAIL with current permissive begin behavior.

- [ ] **Step 2: Add begin-time gate check in `mutate::begin` using Task 1 helper**

Return `ExecutionStateNotReady` plus reason codes when prior task is not closed.

- [ ] **Step 3: Preserve same-task recovery paths**

Verify active/interrupted/blocking semantics still function for same task/step flows.

- [ ] **Step 4: Add legacy in-flight migration-path test cases**

Cover explicit fail-closed behavior when legacy runs are missing new task verification receipts.

- [ ] **Step 5: Run targeted begin/reopen regression suite**

Run: `cargo test --test plan_execution -- begin_ --nocapture`
Expected: PASS for new and existing begin-related behavior.

## Task 3: Surface Deterministic Task-Boundary State in Workflow Phase/Operator

**Spec Coverage:** REQ-006, REQ-007, REQ-008
**Task Outcome:** Workflow/operator phase surfaces report task-boundary blocked states and keep final-review/finish routing intact.
**Plan Constraints:**
- Do not remove or rename existing downstream phase contracts without migration coverage.
- Preserve shell/text/json parity.
**Open Questions:** none

**Files:**
- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/status.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Add failing workflow phase test for task-boundary blocked state**

Run: `cargo test --test workflow_runtime -- workflow_phase_routes_task_boundary_blocked --exact`
Expected: FAIL with current `executing` or downstream-only behavior.

- [ ] **Step 2: Extend operator context derivation to include task-boundary gate diagnostics**

Evaluate task-boundary state before unconditional `executing` advancement.

- [ ] **Step 3: Add/route deterministic phase or next-action text for task-boundary blocks**

Ensure doctor/handoff surfaces carry reason-code guidance.

- [ ] **Step 4: Preserve final review and finish routing behavior**

Keep existing `final_review_pending`, `qa_pending`, `document_release_pending`, and `ready_for_branch_completion` transitions unchanged after all task-boundary gates pass.

- [ ] **Step 5: Run targeted workflow/operator regression tests**

Run: `cargo test --test workflow_runtime -- workflow_phase_ --nocapture`
Expected: PASS including new task-boundary route tests.

## Task 4: Runtime Regression and Contract Coverage (Parallel Lane A)

**Spec Coverage:** REQ-012
**Task Outcome:** Runtime tests explicitly pin per-task gate enforcement, cycle-break interplay, and downstream gate preservation.
**Plan Constraints:**
- This lane owns runtime test files only.
- No skill-doc/template edits in this lane.
**Open Questions:** none

**Files:**
- Modify: `tests/plan_execution.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_runtime_final_review.rs`

- [ ] **Step 1: Create isolated lane worktree for Task 4**

Run: `git worktree add .worktrees/task4-runtime-tests -b codex/task4-runtime-tests`
Expected: new clean worktree created.

- [ ] **Step 2: Add/extend runtime tests for stale/non-independent review provenance and malformed verification receipts**

Keep fixtures deterministic and reason-code explicit, including:
- stale review provenance bindings that no longer match the active checkpoint
- non-independent reviewer provenance
- malformed review-provenance receipts (missing/malformed required headers or invalid binding fields)
- malformed task verification receipt headers/payloads

- [ ] **Step 3: Add regression proving final whole-diff review remains required after task-boundary gating**

Ensure task-level gating does not short-circuit final review requirements.

- [ ] **Step 4: Run lane-targeted runtime tests**

Run: `cargo test --test plan_execution -- task_boundary_ --nocapture`
Run: `cargo test --test workflow_runtime -- task_boundary_ --nocapture`
Expected: PASS.

## Task 5: Skill Contract and Subagent-Consent Updates (Parallel Lane B)

**Spec Coverage:** REQ-009, REQ-010, REQ-012
**Task Outcome:** Execution skills explicitly require per-task review/verification loops and remove per-dispatch user-consent requirement for execution-phase subagents; generated docs and contract tests pass.
**Plan Constraints:**
- This lane owns skill templates/docs and instruction contract tests only.
- Keep wording scoped to execution-phase delegation; do not alter non-execution delegation policy.
**Open Questions:** none

**Files:**
- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Create isolated lane worktree for Task 5**

Run: `git worktree add .worktrees/task5-skill-contracts -b codex/task5-skill-contracts`
Expected: new clean worktree created.

- [ ] **Step 2: Update templates with mandatory per-task review->verification->advance sequencing**

Apply edits in `.tmpl` files only first.

- [ ] **Step 3: Update execution-phase subagent consent language**

State execution-phase runtime-selected subagent dispatch is allowed without per-dispatch user-consent prompts.

- [ ] **Step 4: Regenerate checked-in skill docs from templates**

Run: `node scripts/gen-skill-docs.mjs`
Expected: generated `SKILL.md` files update to match templates.

- [ ] **Step 5: Update instruction-contract expectations and run node contract tests**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: PASS.

## Task 6: Reintegration, Lint Gate, and Plan-Fidelity Readiness

**Spec Coverage:** REQ-007, REQ-008, REQ-012
**Task Outcome:** Reintegration succeeds with full verification (`clippy`, targeted tests, skill-doc contracts), and plan artifact is ready for independent plan-fidelity review receipt.
**Plan Constraints:**
- Resolve lane merge conflicts without changing approved behavior intent.
- Do not skip `plan contract lint` or workflow sync after plan updates.
**Open Questions:** none

**Files:**
- Modify: `src/execution/state.rs`
- Modify: `src/execution/mutate.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/status.rs`
- Modify: `tests/plan_execution.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/runtime_instruction_contracts.rs`

- [ ] **Step 1: Merge Task 4 and Task 5 outputs into integration branch**

Run: `git merge codex/task4-runtime-tests`
Run: `git merge codex/task5-skill-contracts`
Expected: clean merge or resolved conflicts with tests updated accordingly.

- [ ] **Step 2: Run strict lint gate**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS with zero warnings.

- [ ] **Step 3: Run targeted runtime and workflow regressions**

Run: `cargo test --test plan_execution -- --nocapture`
Run: `cargo test --test workflow_runtime -- --nocapture`
Run: `cargo test --test workflow_runtime_final_review -- --nocapture`
Expected: PASS.

- [ ] **Step 4: Run skill-contract verification**

Run: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
Expected: PASS.

- [ ] **Step 5: Run workflow/plan contract lint for this plan+spec pair**

Run: `featureforge plan contract lint --spec docs/featureforge/specs/2026-03-29-per-task-review-gates-design.md --plan docs/featureforge/plans/2026-03-29-per-task-review-gates.md`
Expected: PASS.

- [ ] **Step 6: Prepare and record independent plan-fidelity review artifact**

Run independent review and then:
`featureforge workflow plan-fidelity record --plan docs/featureforge/plans/2026-03-29-per-task-review-gates.md --review-artifact .featureforge/reviews/2026-03-29-per-task-review-gates-plan-fidelity.md`
Expected: runtime-owned pass receipt recorded.

## Evidence Expectations
- Every task completion includes command evidence for the listed verification step(s).
- New reason-code behavior includes explicit assertions in tests (not only snapshot diffs).
- Skill doc changes include regenerated docs and passing skill-doc contract tests.

## Validation Strategy
- Red/green tests for each new gate behavior before and after implementation.
- Preserve downstream late-stage gates with explicit regression assertions.
- Run full strict lint and targeted suites before completion claims.

## Documentation Update Expectations
- Keep this plan current if task/file ownership changes during execution.
- If runtime surface names change during implementation, update related skill text and contract tests in same change.

## Rollout Plan
- Land changes behind normal branch workflow.
- Execute runtime/skill updates in one integrated release to avoid mixed-contract behavior between runtime and guidance.

## Rollback Plan
- Revert per-task `begin` gating and task-boundary phase surfaces while keeping existing final-review and finish gates untouched.
- Revert skill contract sequencing updates if runtime gating must be temporarily disabled.

## Risks and Mitigations
- Risk: Over-constraining execution by blocking legitimate same-task recovery flows.
  - Mitigation: explicit regression coverage for active/interrupted same-task transitions.
- Risk: Contract drift between runtime behavior and skills.
  - Mitigation: template-first edits, regenerated docs, instruction contract tests.
- Risk: Unexpected phase-route regressions in downstream gates.
  - Mitigation: dedicated workflow phase regression tests plus final-review/finish preservation tests.
