# QA Recording On Current Reviewed Branch Closures

**Workflow State:** Implementation Target  
**Spec Revision:** 1  
**Last Reviewed By:** clean-context review loop  
**Implementation Target:** Current

## Problem Statement

`qa_pending` is part of the public workflow contract, but QA still lacks a dedicated contract slice comparable to release-readiness and final review.

Without that slice, implementation would have to invent:

- accepted QA result vocabulary
- what QA binds to
- what happens when QA fails
- how QA becomes stale after later edits
- how workflow routes after failed QA

That is not acceptable for an implementation handoff corpus.

## Desired Outcome

QA should be recordable through one public runtime command that records a QA milestone against the current reviewed branch closure and routes failed QA into explicit execution reentry.

After this work:

- QA is a first-class runtime-owned branch milestone
- the record binds to current reviewed branch closure state
- later reviewed branch state makes older QA milestones historical
- post-QA unreviewed edits make QA stale
- failed QA returns explicit execution reentry unless handoff or pivot overrides it

## Decision

Selected approach: add a first-class runtime-owned QA milestone record for the current reviewed branch closure and define its pass/fail routing semantics explicitly.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- `2026-04-02-branch-closure-recording-and-binding.md`
- `2026-04-01-release-readiness-recording-and-binding.md`
- `2026-04-01-final-review-recording-and-binding.md`
- `2026-04-01-execution-repair-and-state-reconcile.md`
- `2026-04-01-workflow-public-phase-contract.md`

## Requirement Index

- [REQ-001][behavior] FeatureForge must provide a public runtime command for recording QA against the current reviewed branch closure.
- [REQ-002][behavior] The authoritative result must be a runtime-owned QA milestone record, not a hand-authored markdown artifact.
- [REQ-003][behavior] The QA record must bind at least current reviewed branch state id, repo, branch, plan path/revision, generated-by identity, result, and summary.
- [REQ-004][behavior] The runtime may emit a human-readable QA artifact as a derived output, but workflow/gates must consume the runtime-owned record model rather than treating markdown as the primary truth surface.
- [REQ-005][behavior] A newer reviewed branch state must not rewrite old QA proof in place; the old milestone becomes historical through closure lineage.
- [REQ-006][behavior] If the workspace moves after a current QA milestone without a new reviewed branch closure, the milestone must become `stale_unreviewed`.
- [REQ-007][behavior] Accepted QA results in the first slice are `pass` and `fail`.
- [REQ-008][behavior] `record-qa --result fail` must return `required_follow_up=execution_reentry` by default and must return `record_handoff` or `record_pivot` instead when an authoritative handoff or pivot override is already in force for that next-safe action.
- [REQ-009][behavior] `qa_pending` routing must remain explicit and must not force implementation to invent negative-path workflow semantics.
- [REQ-010][behavior] Re-running QA recording for the same still-current branch closure and equivalent QA result must be safe and idempotent.
- [REQ-011][verification] Integration tests must prove QA binds to current reviewed branch state, becomes stale or historical appropriately, and routes failed QA into explicit follow-up.
- [REQ-012][behavior] The runtime must derive whether QA is required from one authoritative policy source and must expose that decision to workflow/operator so routing to `qa_pending` versus `ready_for_branch_completion` is deterministic. In the first slice, that authoritative source is normalized approved-plan metadata field `QA Requirement: required|not-required`; normalization trims surrounding whitespace, lowercases ASCII letters, accepts only `required` or `not-required`, and fail-closes on any other token by routing workflow/operator to `phase=pivot_required` with `phase_detail=planning_reentry_required`.
- [REQ-013][behavior] `record-qa` must fail closed unless the same still-current branch closure already has current release-readiness result `ready` and current final-review result `pass`.

## Scope

In scope:

- public QA recording command
- runtime-owned QA records
- pass/fail routing semantics
- stale and historical QA behavior

Out of scope:

- redesigning browser or external QA tooling itself
- weakening finish-gate strictness

## Selected Approach

Add:

- `featureforge plan execution record-qa --plan <path> ...`

`record-qa` must:

1. inspect workflow/operator and review-state truth
2. fail closed unless `qa_pending` is the current valid phase
3. fail closed unless a current reviewed branch closure exists
4. fail closed unless current release-readiness result `ready` and current final-review result `pass` already exist for that same still-current branch closure
5. record one QA milestone against that branch closure
6. return a structured trace plus the resulting QA state

Repo/branch/base-branch bindings must come from the runtime-owned `RepositoryContextResolver`, not from duplicated per-command parsing or hand-authored input.

Chosen routing rule:

- `qa_pending` is emitted only when authoritative approved-plan metadata says `QA Requirement: required` and current branch closure, current release-readiness result `ready`, and current final-review result `pass` already exist for the same branch closure
- when the same `qa_pending` lane lacks an authoritative current-branch test plan or the authoritative QA -> test-plan provenance is missing, blank, malformed, stale, or otherwise invalid, workflow/operator must emit `phase_detail=test_plan_refresh_required`, `next_action=refresh test plan`, and omit `recommended_command` so the branch reroutes through `featureforge:plan-eng-review` before direct `record-qa`
- `ready_for_branch_completion` is emitted directly when that same authoritative policy says `QA Requirement: not-required` and the same current branch closure already has current release-readiness result `ready` and current final-review result `pass`
- if authoritative approved-plan metadata for `QA Requirement` is missing or invalid when workflow/operator must choose between `qa_pending` and `ready_for_branch_completion`, workflow/operator must fail closed to `phase=pivot_required` with `phase_detail=planning_reentry_required` and recommended command `featureforge workflow record-pivot --plan <path> --reason <reason>`
- workflow/operator must consume that policy through the runtime query layer rather than inferring it from prose or external convention
- if current reviewed state is `stale_unreviewed`, workflow/operator must reroute to `phase=executing` with exact next command `repair-review-state`; only that repair flow may later reroute the branch back into late-stage work

## Public Contract

`record-qa` must accept at least:

- `--plan <path>`
- `--result pass|fail`
- `--summary-file <path>`

It must fail closed unless all of these are true:

1. `phase=qa_pending`
2. `review_state_status=clean`
3. `phase_detail=qa_recording_required`
4. a current release-readiness milestone with result `ready` exists for the same still-current branch closure
5. a current final-review milestone with result `pass` exists for the same still-current branch closure
6. authoritative current-branch test-plan provenance is valid, so workflow/operator is not rerouting to `phase_detail=test_plan_refresh_required`

The authoritative QA output contains at least:

- plan path and revision
- repo slug
- branch and base branch where applicable
- current reviewed branch state id
- `branch_closure_id`
- generated-by identity
- result
- summary
- record status

`generated_by_identity` must follow the shared runtime-owned normalization contract from `2026-04-01-supersession-aware-review-identity.md`; in the first slice, QA records use `featureforge/qa`.

## Negative Result Handling

`--result fail` must be operationally defined:

1. the runtime records a failed QA milestone for audit against the current branch closure
2. the failed QA milestone is not treated as current branch-completion truth
3. the command returns:
   - `required_follow_up=execution_reentry` when normal execution repair is the next safe action
   - `required_follow_up=record_handoff` when an authoritative handoff override is already in force
   - `required_follow_up=record_pivot` when an authoritative pivot override is already in force
   - the command must choose among those values by consulting authoritative workflow query field `follow_up_override = none|record_handoff|record_pivot`, not by command-local heuristics
4. workflow/operator must route immediately from the returned `required_follow_up`
   - `execution_reentry` routes immediately to `phase=executing` with `phase_detail=execution_reentry_required`
   - `record_handoff` routes immediately to `phase=handoff_required` with `phase_detail=handoff_recording_required`
   - `record_pivot` routes immediately to `phase=pivot_required` with `phase_detail=planning_reentry_required`
   - structural blocked returns use the workflow contract's structural mapping for `repair_review_state`

## Return Contract

QA recording must return at least:

- `action`: `recorded` | `already_current` | `blocked`
- `branch_closure_id`
- `result`
- `required_follow_up` when follow-up work is required
- `trace[]` or `trace_summary`

For QA recording:

- `action=recorded` is used when a QA milestone was appended, including `--result fail`
- `action=blocked` is reserved for pre-mutation structural failures such as missing branch closure, stale review state, or conflicting same-state rerun inputs
- structural blocked follow-up values are `repair_review_state`
- if the direct command is invoked while branch closure, release-readiness, or final-review prerequisites are missing, the runtime must return the shared out-of-phase response contract defined by `2026-04-01-gate-diagnostics-and-runtime-semantics.md`; workflow/operator then reroutes the branch to the appropriate earlier late-stage phase
- if the direct command is invoked while current reviewed state is `stale_unreviewed`, the runtime must fail closed with `required_follow_up=repair_review_state`
- recorded negative-result follow-up uses `execution_reentry`, `record_handoff`, or `record_pivot`

`already_current` means:

- the same still-current branch closure already has an equivalent recorded QA outcome
- equivalent means same still-current `branch_closure_id`, same `result`, and the same normalized summary content as produced by the shared runtime-owned `SummaryNormalizer`
- no duplicate current QA milestone was appended

If the branch closure is the same but one or more equivalence inputs differ, the runtime must fail closed with `action=blocked`, emit a validation error for conflicting same-state rerun inputs, and append no new milestone.
If branch closure, a current release-readiness result `ready`, or a current final-review result `pass` are missing or stale, `record-qa` must fail closed and workflow/operator must reroute to the earlier authoritative late-stage phase rather than trying to repair QA in place.

## Acceptance Criteria

1. One supported command can record QA against the current reviewed branch closure.
2. Finish gating can reason over that record without requiring markdown as the only primary truth surface.
3. Later reviewed branch state does not require rewriting old QA proof; the older milestone simply becomes historical or stale as appropriate.
4. Unreviewed post-QA branch changes make QA stale until a new reviewed branch state exists.
5. `qa_pending` no longer requires implementation to invent pass/fail routing semantics.
6. Same-state rerun behavior is deterministic and idempotent.

## Test Strategy

- add a CLI-only happy-path test for recording QA against current reviewed branch state
- add a CLI-only negative-path test for `--result fail` that proves runtime returns the authoritative `required_follow_up`
- add a stale-unreviewed branch test after post-QA edits
- add a superseded-QA test when later reviewed branch state replaces the earlier one
- add workflow/operator routing tests for missing or invalid `QA Requirement`, including whitespace/case normalization and fail-closed reroute to `pivot_required`
- keep narrow validator tests only for derived artifact compatibility where needed

## Risks

- leaving QA as a public phase without a dedicated command contract would force implementation to invent part of the workflow contract
- treating QA as a loose artifact instead of a runtime-owned milestone would recreate the same drift problems already seen in release-readiness and final review
