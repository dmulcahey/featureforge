# Runtime Safety Thirty-Ninth Audit Remediation

## Workflow State

Engineering remediation plan for the thirty-ninth runtime-safety audit loop. This plan is active only until the ordered tasks below are implemented, fully verified, and independently reviewed. It does not make another audit loop part of the plan artifact; any later audit is controlled by explicit user/session direction or by new evidence such as material route/prompt changes, failed validation, or unresolved review findings.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow/project skills. Do not allow reviewers or implementation subagents to spawn additional subagents. Before each full test cycle, verify no existing `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, `cargo clippy`, or Codex-runtime Node validation process is running.

If a full nextest run exceeds 4-5 minutes, run `cargo clean`, rerun once, and remediate if the regression repeats. If a full test suite run exceeds 10 minutes, stop immediately, run `cargo clean`, rerun, and enter performance remediation if the regression is repeatable.

## Goal

Resolve the actionable thirty-ninth audit findings by reducing process and prompt-surface churn without weakening runtime safety:

- completed audit plans are archived rather than left active as standing loop authority
- route-owning skills keep mandatory top-level execution law while delegating detailed task-closure route mechanics to the canonical route reference
- static boundary tests have an explicit growth policy that favors public/import-boundary contracts over private-topology scanners

## Architecture

Preserve the current runtime architecture:

```text
CLI args
  -> command module
  -> transition guard
  -> event append
  -> reducer
  -> read model
  -> route decision
  -> workflow operator presentation
```

This plan must not alter public route semantics, runtime authority, receipt/projection authority, or late-stage progression. The work is documentation, prompt-surface, generated-doc, and policy-test maintenance only.

## Change Surface

- `docs/featureforge/archive/runtime-safety-audit-history/README.md`
- move completed `docs/featureforge/plans/2026-05-16-runtime-safety-thirty-eighth-audit-remediation.md` to `docs/featureforge/archive/runtime-safety-audit-history/plans/`
- `references/operator-route-authority.md`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- generated `skills/executing-plans/SKILL.md`
- generated `skills/subagent-driven-development/SKILL.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `docs/testing.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

## Preconditions

- The thirty-eighth remediation tasks are implemented, fully validated, and independently reviewed.
- The thirty-ninth audit report is the source finding set for this plan.
- Do not use FeatureForge runtime/workflow/project skills.
- Generated skill docs must be regenerated from templates, not hand-edited.
- Historical archived material is evidence only, not active workflow authority.

## Known Footguns / Constraints

- Do not remove mandatory top-level route law from route-owning skills.
- Do not treat display-only recommendation text as executable authority.
- Do not reintroduce hidden/debug commands or low-level recorder routes.
- Do not make companion references route-selection authority; they provide binding detail after workflow/operator selects a route.
- Do not weaken prompt-budget, generated-doc, source-archive, or reviewer-recursion tests.
- Do not add new static scanner layers for private topology. Prefer deleting duplicated logic, import-direction checks, public route projection tests, or public-output tests.
- Do not edit archived historical plans beyond moving the completed active plan into the archive.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| REQ-001 Completed audit-loop plans are archived and no active plan encodes a standing self-perpetuating audit loop | Task 1 |
| REQ-002 Route-owning skills retain mandatory task-boundary actionability but delegate detailed task-closure route mechanics to the canonical reference | Task 2 |
| REQ-003 Boundary-test growth policy discourages private-topology scanner churn and preserves high-signal modularity checks | Task 3 |

## Task 1: Archive Completed Audit Loop Plan

**Spec Coverage:** REQ-001

**Goal:** Remove the completed thirty-eighth plan from active planning surfaces and update the archive index so active docs no longer encode a standing audit-loop trigger.

**Context:** The signal/noise audit found `docs/featureforge/plans/2026-05-16-runtime-safety-thirty-eighth-audit-remediation.md` still made the next deep audit part of that plan's completion rule. The loop did run, and keeping that completed plan active makes the repo artifact itself a source of process churn.

**Constraints:**

- Preserve the completed plan append-only by moving it into the runtime-safety audit archive.
- Do not rewrite the historical plan content.
- The new active plan must not include a "run another audit loop" completion condition.
- Update archive counts and recent-plan list coherently.

**Done when:**

- `docs/featureforge/plans/2026-05-16-runtime-safety-thirty-eighth-audit-remediation.md` no longer exists under active plans.
- The same file exists under `docs/featureforge/archive/runtime-safety-audit-history/plans/`.
- `docs/featureforge/archive/runtime-safety-audit-history/README.md` names this thirty-ninth plan as the current active runtime-safety plan.
- No active plan text says a plan remains active until another deep audit loop runs.

**Files:**

- `docs/featureforge/archive/runtime-safety-audit-history/README.md`
- `docs/featureforge/archive/runtime-safety-audit-history/plans/2026-05-16-runtime-safety-thirty-eighth-audit-remediation.md`
- `docs/featureforge/plans/2026-05-16-runtime-safety-thirty-ninth-audit-remediation.md`

**Detailed Implementation Steps:**

1. Move the thirty-eighth remediation plan from `docs/featureforge/plans/` to `docs/featureforge/archive/runtime-safety-audit-history/plans/`.
2. Update the archive index current active plan path to the thirty-ninth plan.
3. Increment archived remediation plan and audit report counts.
4. Add the thirty-eighth plan to the recent superseded active plans list.
5. Search active plans for the retired self-perpetuating audit-loop wording; confirm only historical archived plans contain that old process law.

**Validation Expectations:**

- A recursive search for the retired deep-audit completion phrase and final-audit-loop heading exits with no matches under `docs/featureforge/plans`.
- `node --test tests/codex-runtime/*.test.mjs`
- Full strict Clippy and full no-fail-fast nextest before task review.

## Task 2: Compact Task-Closure Route Law In Route-Owning Skills

**Spec Coverage:** REQ-002

**Goal:** Keep route-owning skills actionable while removing duplicated detailed task-closure route mechanics that are already owned by `references/operator-route-authority.md`.

**Context:** The thirty-ninth audit found `executing-plans` and `subagent-driven-development` still repeat `task_closure_recording_ready`, `task_review_dispatch_required`, dispatch-lane, and `--external-review-result-ready` mechanics inline. The canonical reference already owns detailed route law. The skills should keep the mandatory task-boundary order and tell agents to query workflow/operator JSON and follow typed argv/template/reference; they should not duplicate the route table.

**Constraints:**

- Edit `.tmpl` files and regenerate `SKILL.md`; do not hand-edit generated skills.
- Preserve mandatory task-boundary safety:
  - dedicated-independent task review before next task
  - verification inputs collected for `close-current-task`
  - no Task `N+1` before Task `N` has current positive closure
  - workflow/operator JSON is the route owner
  - `recommended_public_command_argv`/template only; stop if absent
- Preserve the warning that `--external-review-result-ready` is for actual external review results, not verification-only completion.
- Keep detailed `task_closure_recording_ready`, dispatch-lane, and closure replay details in `references/operator-route-authority.md`.
- Update Codex-runtime skill-doc tests to assert the compact contract instead of detailed inline route wording.

**Done when:**

- `skills/executing-plans/SKILL.md.tmpl` no longer names `task_closure_recording_ready` or `task_review_dispatch_required`.
- `skills/subagent-driven-development/SKILL.md.tmpl` no longer names `task_closure_recording_ready` or `task_review_dispatch_required`.
- Both generated skills still state the mandatory task-boundary order and typed workflow/operator route rule.
- `references/operator-route-authority.md` still contains the detailed task-closure route mechanics.
- Codex-runtime tests prove mandatory law remains top-level and detailed route mechanics remain in the canonical reference.
- Prompt budget remains enforced.

**Files:**

- `references/operator-route-authority.md`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- generated `skills/executing-plans/SKILL.md`
- generated `skills/subagent-driven-development/SKILL.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/skill-doc-budgets.json` only if generation changes counts

**Detailed Implementation Steps:**

1. Confirm `references/operator-route-authority.md` includes the detailed task-closure route law for `task_closure_recording_ready`, `close-current-task`, dispatch-required lanes, and external-review-ready materialization.
2. In `executing-plans`, replace detailed task-closure route bullets with a compact rule:
   - after review is green, run verification and collect close inputs
   - after the external review result is in hand, rerun workflow/operator JSON and follow the Installed Control Plane plus canonical route reference
   - do not start Task `N+1` until `close-current-task` succeeds and current positive closure exists
3. In `subagent-driven-development`, make the same compaction.
4. Regenerate skills with `node scripts/gen-skill-docs.mjs`.
5. Update `tests/codex-runtime/skill-doc-contracts.test.mjs` so it rejects detailed route tokens in generated route-owning skills while requiring them in `references/operator-route-authority.md`.
6. Run the Codex-runtime skill-doc contract suite and budget checks.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- Full strict Clippy and full no-fail-fast nextest before task review.

## Task 3: Document Boundary-Test Growth Policy

**Spec Coverage:** REQ-003

**Goal:** Make the intended boundary-test signal explicit so future work does not respond to every modularization concern by adding private-topology scanner assertions.

**Context:** `tests/runtime_module_boundaries.rs` is high-value where it protects import direction, public route projection, typed command authority, and concrete historical failure classes. The audit flagged risk that it can become a brittle private-topology policy layer if future tasks add line-count or helper-name assertions without a failure-class rationale.

**Constraints:**

- Do not remove existing guards in this task unless an obviously obsolete assertion is found.
- Do not add new scanner assertions.
- The policy should guide future changes toward deleting duplicate logic and testing public/import-boundary behavior.
- Keep `advance_late_stage` large-module visibility from the thirty-eighth remediation.

**Done when:**

- `docs/testing.md` states that new runtime-boundary scanner assertions require a concrete audited failure class or public/import-boundary contract.
- `docs/featureforge/reference/execution-runtime-module-boundaries.md` clarifies that large-module documentation is visibility debt, not safety proof.
- The reference encourages import-direction, public route projection, and externally visible behavior checks over private helper topology.

**Files:**

- `docs/testing.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

**Detailed Implementation Steps:**

1. Add a concise boundary-test growth policy to the runtime testing section of `docs/testing.md`.
2. Update the execution-runtime module boundary reference to state that large-module threshold documentation is not proof of semantic safety.
3. Add guidance that new private-topology assertions should be avoided unless they protect an audited failure class or a stable boundary API.
4. Run docs/prompt Node contracts to confirm no active-doc route or prompt traps were introduced.

**Validation Expectations:**

- `node --test tests/codex-runtime/*.test.mjs`
- Full strict Clippy and full no-fail-fast nextest before task review.
