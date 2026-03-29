# Per-Task Review Gates in Execution Workflow

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Problem Statement
The execution workflow currently enforces review and finish gates only after all plan steps are complete. This allows execution to start Task N+1 immediately after Task N implementation steps complete, even when Task N has not passed an independent review loop and follow-on verification checkpoint.

That behavior weakens plan execution quality control because remediation can be deferred too late, cross-task risk can compound, and reviewers lose a clean per-task checkpoint.

## Desired Outcome
Enforce a strict task-boundary contract:
1. Implement all steps for the current task.
2. Run dedicated independent review for that task in a fresh-context subagent.
3. If review fails, reopen/remediate/re-review until green.
4. Use runtime cycle tracking and auto cycle-break when review/remediation churn reaches 3 cycles for the same task.
5. After review is green, run verification-before-completion for task-scoped verification evidence.
6. Only then allow execution to advance to the next task.
7. Keep the existing final whole-diff review gate before branch completion.

## Scope
- Runtime enforcement of per-task review + verification gate before cross-task advancement.
- Runtime/operator phase and diagnostics updates to surface task-boundary gate state.
- Execution skill contract updates (`executing-plans`, `subagent-driven-development`, related docs) to align with runtime enforcement.
- Explicit subagent dispatch policy change: execution-time review/implementation subagents are allowed without per-dispatch user consent once execution is in progress.
- Preserve existing final review gate as an additional downstream whole-diff checkpoint.

## Out of Scope
- Replacing final whole-diff review with task-only review.
- Changing approved plan/spec scope during remediation.
- Weakening existing authoritative artifact, provenance, or trust-boundary checks.

## Selected Approach (Option A)
Keep final review and add mandatory per-task green gates.

Why this approach:
- Catches defects earlier at each task boundary.
- Preserves cross-task/system-wide quality check at the end.
- Reuses existing runtime cycle tracking (`review_remediation`, `cycle_break`) instead of introducing parallel churn logic.

## Workflow Contract (Target Behavior)

### Task Lifecycle

```
(task N steps active)
  -> all task N steps complete
  -> task N review pending (independent fresh subagent)
  -> [pass] task N verification pending (verification-before-completion)
  -> [pass] task N ready/closed
  -> task N+1 may begin

  -> [fail review] reopen task N remediation
  -> review rerun (cycle count++)
  -> cycle 3 => cycle_break strategy state
```

### Hard Rule
Starting the first step of Task N+1 is blocked while Task N is not closed under:
- review green
- verification complete

## Runtime Changes
1. Add task-boundary readiness evaluation in execution runtime state.
- Compute the most recently active/completed task.
- Determine whether that task has satisfied:
  - required independent review receipt/provenance
  - verification-before-completion checkpoint for task closure

2. Enforce gate in `begin` transition.
- Reject `plan execution begin --task <next-task>` when prior task is not task-closed.
- Return structured failure (`ExecutionStateNotReady`) with explicit reason codes (for example `prior_task_review_not_green`, `prior_task_verification_missing`).

3. Keep cycle tracking runtime-owned and automatic.
- Continue using review-dispatch + reopen cycle accounting.
- Preserve auto `cycle_break` transition at cycle 3 per task.
- Do not require human replanning loopback for cycle-break entry.

4. Expose task gate state through workflow operator/phase surfaces.
- Add/route a task-level pending phase (or equivalent deterministic diagnostics) before `executing` advances to later tasks.
- Ensure shell/text/json parity for this surface.

5. Preserve final whole-diff review and finish gates.
- Existing `final_review_pending` behavior remains required after all tasks are task-closed.

## Skill/Contract Changes
1. `skills/executing-plans/SKILL.md(.tmpl)`
- Replace final-only sequencing with per-task loop:
  - complete task steps
  - run independent task review (fresh subagent)
  - remediate/re-review until green
  - run verification-before-completion
  - then advance
- Keep Step 3 final review gate for whole diff.

2. `skills/subagent-driven-development/SKILL.md(.tmpl)`
- Align with enforced per-task review/verification gate.
- Clarify the runtime-owned cycle-break path at task boundaries.

3. Subagent consent policy text in execution-facing skills.
- Remove per-dispatch user-consent requirement for subagent use during approved execution flows.
- State that approved execution stage authorizes runtime-selected subagent dispatch.

## Independent Review Requirements
Per-task review must be:
- dedicated-independent
- fresh context (not inherited implementation session history)
- traceable to task packet/task checkpoint artifacts
- pass/fail explicit

Task closure is blocked on missing, stale, or non-independent review provenance.

## Verification Requirements
After review green and before task closure:
- run verification-before-completion workflow for task-scoped checks
- require fresh command evidence (no inferred pass)
- block next-task start on missing/failed verification

## Error Handling and Edge Cases
- If review artifacts are unreadable/malformed: fail closed on task closure.
- If a review fails and remediation reopens work: prior green state is invalidated.
- If cycle-break is active: execution remains in remediation strategy until runtime-owned conditions allow continuation.
- If execution restarts from persisted state: task-boundary gate must be recomputed from authoritative state, not transient session assumptions.

## Observability
- Add reason-code coverage for task-boundary blocks.
- Emit phase/next-action diagnostics that clearly indicate review-pending vs verification-pending vs remediation/cycle-break.
- Preserve strategy checkpoint fingerprint traceability through per-task review receipts.

## Test Plan (Acceptance)
1. Runtime blocks `begin` on Task N+1 when Task N review is missing.
2. Runtime blocks `begin` on Task N+1 when Task N verification is missing.
3. Runtime allows `begin` on Task N+1 only after Task N review green + verification complete.
4. Three review/remediation cycles on same task auto-enter `cycle_break`.
5. Operator phase/next-action surfaces new task-boundary gate state deterministically.
6. Existing final review and finish gates still execute after all task-boundary gates pass.
7. Skill-doc contract tests pin updated per-task sequencing and subagent consent behavior.

## Risks and Mitigations
- Risk: duplicated review logic between task-level and final-level flows.
  - Mitigation: centralize gate checks in runtime helpers and reuse provenance validators.
- Risk: accidental weakening of authoritative artifact checks while adding task gate.
  - Mitigation: fail-closed defaults and targeted regression tests around stale/malformed artifacts.
- Risk: execution friction from stricter gating.
  - Mitigation: clear diagnostics and next-action guidance for remediation loop.

## Rollout and Rollback
- Rollout via feature branch with targeted runtime + skill + contract tests.
- If regressions appear, rollback by reverting per-task begin-block logic and related skill changes while preserving existing final review gate behavior.
