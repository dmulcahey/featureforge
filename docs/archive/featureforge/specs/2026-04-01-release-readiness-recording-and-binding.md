# Release-Readiness Recording On Current Reviewed Branch Closures

**Workflow State:** Implementation Target  
**Spec Revision:** 3  
**Last Reviewed By:** clean-context review loop
**Implementation Target:** Current

## Problem Statement

Release-readiness is currently treated as a strict late-stage artifact that operators must effectively author by hand and that the runtime later validates against current plan, repo, branch, base branch, and `HEAD`.

Under the supersession-aware model, that is the wrong abstraction boundary.

Release-readiness should not be "a markdown file that hopefully still matches current branch truth." It should be a runtime-owned milestone recorded against the **current reviewed branch closure**.

## Desired Outcome

Release-readiness should be recordable through one public runtime command that records the milestone against the current reviewed branch state and optionally emits a human-readable artifact.

After this work:

- the authoritative truth is a runtime-owned release-readiness record
- the record binds to current reviewed branch closure state
- older release-readiness milestones can become historical when later reviewed branch state supersedes their bound branch closure
- human-readable release-readiness artifacts become derived outputs
- agents can use one preferred aggregate late-stage command instead of hand-assembling the normal terminal-stage recording sequence

## Decision

Selected approach: add a first-class runtime-owned release-readiness milestone record for the current reviewed branch closure.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- `2026-04-02-branch-closure-recording-and-binding.md`
- `2026-04-01-execution-repair-and-state-reconcile.md`
- `2026-04-01-workflow-public-phase-contract.md`

## Requirement Index

- [REQ-001][behavior] FeatureForge must provide a public runtime command for recording release-readiness against the current reviewed branch closure.
- [REQ-002][behavior] The authoritative result must be a runtime-owned release-readiness record, not a hand-authored markdown artifact.
- [REQ-003][behavior] The record must bind at least plan path, plan revision, repo, branch, base branch, current reviewed branch state id, result, and generator identity.
- [REQ-004][behavior] The runtime may emit a human-readable release-readiness artifact as a derived output, but `gate-finish` must consume the runtime-owned record model rather than treating the markdown artifact as the only primary truth surface.
- [REQ-005][behavior] Recording release-readiness for a newer reviewed branch state must not rewrite older milestones in place; older milestones become historical through closure lineage.
- [REQ-006][behavior] If the workspace moves after a current release-readiness milestone without a new reviewed branch closure, the milestone must become stale rather than silently refreshed.
- [REQ-007][verification] Integration tests must prove that release-readiness binds to current reviewed branch state and becomes stale or historical appropriately under later changes.
- [REQ-008][verification] Negative tests must still prove rejection of contradictory or malformed derived artifacts where those artifacts remain supported.
- [REQ-009][behavior] `advance-late-stage` must be the preferred aggregate agent-facing command for normal release-readiness progression once a current reviewed branch closure already exists, with `record-release-readiness` retained as the lower-level stage primitive.
- [REQ-010][behavior] A public `record-branch-closure` primitive must exist and workflow/repair surfaces must route operators to it explicitly when release-readiness needs a current reviewed branch closure and one is missing.
- [REQ-011][behavior] Re-running release-readiness recording for the same still-current branch closure and equivalent release-readiness result must be safe and idempotent.

## Scope

In scope:

- public release-readiness recording command
- runtime-owned release-readiness records
- derived human-readable artifacts
- current reviewed branch state binding
- stale and historical milestone behavior

Out of scope:

- changing final-review policy itself
- weakening finish-gate strictness
- changing document editing behavior in `document-release`

## Selected Approach

Add:

- `featureforge plan execution record-branch-closure --plan <path>`
- `featureforge plan execution record-release-readiness --plan <path> ...`

Add preferred aggregate late-stage surface:

- `featureforge plan execution advance-late-stage --plan <path> ...`

The operator supplies release summary content and result input. The runtime supplies:

- current reviewed branch state identity
- plan/revision binding
- repo/branch/base-branch binding
- milestone storage
- optional artifact generation

Repo/branch/base-branch bindings must come from the runtime-owned `RepositoryContextResolver`, not from duplicated per-command parsing or hand-authored input.
`generated_by_identity` must follow the shared runtime-owned normalization contract from `2026-04-01-supersession-aware-review-identity.md`; in the first slice, release-readiness records use `featureforge/release-readiness`.

`advance-late-stage` should:

1. inspect workflow/operator and review-state truth
2. fail closed unless `document_release_pending` is the current valid late stage
3. fail closed unless a current reviewed branch closure already exists
4. delegate to `record-release-readiness`
5. return a structured trace naming the stage primitive it invoked and the resulting milestone state

## Public Contract

The authoritative output is a release-readiness record containing at least:

- source plan path and revision
- repo slug
- branch and base branch
- current reviewed branch state id
- result
- release summary
- generated-by identity
- record status

Any markdown artifact generated from it is derivative.

Chosen output policy:

1. the canonical derived human-readable output remains markdown for compatibility with the current workflow surface
2. the release summary lives in the authoritative record
3. markdown is generated from that record rather than being separately authored truth

Chosen command policy:

1. `record-release-readiness` is the stable primitive for the stage-specific recording service
2. `record-branch-closure` is the explicit prerequisite command when workflow/operator says the branch closure is missing
3. `advance-late-stage` is the preferred agent-facing command when workflow/operator already routes the branch to `document_release_pending` and current reviewed branch closure exists
4. skills and process guides must teach the explicit prerequisite step plus the aggregate command for the normal terminal path
5. release-readiness examples must pass an explicit `--result ready|blocked` value; the result must not be implicit

Chosen lineage policy:

1. release-readiness milestones inherit historical state through the branch-closure lineage they bind to
2. no separate milestone-level `superseded` status is required
3. post-recording unreviewed branch movement marks the milestone `stale_unreviewed` until a new reviewed branch closure exists
4. at most one release-readiness milestone may be query-current for a given still-current branch closure at a time
5. when a newer release-readiness milestone is recorded for the same still-current branch closure, the previously query-current release-readiness milestone for that same closure becomes `historical`
6. only a current release-readiness milestone with result `ready` satisfies the downstream prerequisite for final-review routing; mere milestone existence does not

## Aggregate Command Contract

`advance-late-stage` must accept at least, for the release-readiness path:

- `--plan <path>`
- `--result ready|blocked`
- `--summary-file <path>`

It must fail closed unless all of these are true:

1. `phase=document_release_pending`
2. `review_state_status` is `clean`
3. `phase_detail` is `release_readiness_recording_ready` or `release_blocker_resolution_required`

It must not record release-readiness when `review_state_status` is `stale_unreviewed` or `missing_current_closure`.
It must also fail closed before mutation if final-review-only arguments or result vocabulary are supplied while `phase=document_release_pending`.

`recording_context.branch_closure_id` must be exposed by workflow/operator when `phase_detail=release_readiness_recording_ready` or `phase_detail=release_blocker_resolution_required`. That id exists for authoritative binding context, transparency, and primitive fallback to `record-release-readiness` even though the aggregate release-readiness path still takes only `--plan`, `--result`, and `--summary-file`.

## Primitive Contract

`record-release-readiness` must accept at least:

- `--plan <path>`
- `--branch-closure-id <closure-id>`
- `--result ready|blocked`
- `--summary-file <path>`

`record-release-readiness` must not synthesize or infer a branch closure implicitly. Workflow/operator or repair surfaces may direct the operator to `record-branch-closure` first, but the primitive records the milestone only against an explicit branch-closure id.
`record-release-readiness` must also fail closed unless the supplied `branch_closure_id` is still current for the branch and still matches the reviewed branch state the runtime trusts at mutation time.

## Return Contract

Release-readiness recording must return at least:

- `action`: `recorded` | `already_current` | `blocked`
- `branch_closure_id`
- `result`
- `required_follow_up` when follow-up work is required

When `advance-late-stage` takes the release-readiness path, it must return the same primitive contract plus:

- `stage_path=release_readiness`
- `delegated_primitive=record-release-readiness`
- `dispatch_id=null`
- `trace[]` or `trace_summary` describing the validations and delegated primitive call it executed

For release-readiness recording:

- `action=recorded` is used when a release-readiness milestone was appended, including `--result blocked`
- `action=blocked` is reserved for pre-mutation structural failures such as missing branch closure, stale review state, or conflicting same-state rerun inputs
- structural blocked follow-up values are `record_branch_closure` and `repair_review_state`
- if the direct command is invoked out of phase and the exact next safe step is not deterministically one of those blocked follow-ups, the runtime must return the shared out-of-phase response contract defined by `2026-04-01-gate-diagnostics-and-runtime-semantics.md`
- recorded negative-result follow-up uses `resolve_release_blocker`

`already_current` means:

- the same still-current branch closure already has an equivalent recorded release-readiness outcome
- equivalent means same still-current `branch_closure_id`, same `result`, and the same normalized summary content as produced by the shared runtime-owned `SummaryNormalizer`
- no duplicate current release-readiness milestone was appended

If the branch closure is the same but one or more equivalence inputs differ, the runtime must:

- append a new release-readiness milestone when the current release-readiness result for that same branch closure is `blocked` and the rerun represents blocker-resolution progression on that branch closure
- otherwise fail closed with `action=blocked`, emit a validation error for conflicting same-state rerun inputs, and append no new milestone

Chosen current-state selection rule:

- `current_release_readiness_state` is the newest non-stale release-readiness milestone bound to the still-current branch closure
- when blocker-resolution progression records a newer release-readiness milestone on that same still-current branch closure, the older milestone becomes `historical` for query purposes even though it remains auditable
- status, routing, and finish gates must read only that single selected current release-readiness milestone

## Negative Result Handling

`--result blocked` must be operationally defined:

1. the runtime records a release-readiness milestone with result `blocked` against the current branch closure for audit
2. the command returns `action=recorded`
3. the command returns `required_follow_up=resolve_release_blocker`
4. the workflow remains in `document_release_pending`
5. `phase_detail` becomes `release_blocker_resolution_required`
6. `next_action` becomes `resolve release blocker`
7. `recommended_command` remains the same exact `advance-late-stage` release-readiness command once the blocker is resolved and an updated summary is ready
8. if resolving the blocker changes repo-tracked content, the reviewed branch state becomes `stale_unreviewed`, workflow must route to execution reentry or review-state repair, and a new current branch closure must exist before release-readiness can be recorded again
9. if resolving the blocker does not change repo-tracked content, rerunning `advance-late-stage --result ready ...` or `advance-late-stage --result blocked ...` against the same still-current branch closure is allowed and records a newer release-readiness milestone for that branch closure
10. when that newer same-closure milestone is recorded, the older same-closure release-readiness milestone becomes `historical` and the newer milestone becomes the only query-current release-readiness result for that branch closure

## Acceptance Criteria

1. One supported command can record release-readiness against the current reviewed branch closure.
2. Finish gating can reason over that record without requiring hand-authored markdown as the only primary truth surface.
3. Later reviewed branch state does not require rewriting old release-readiness proof; the older milestone simply becomes historical or stale as appropriate.
4. Unreviewed post-milestone branch changes make release-readiness stale until a new reviewed branch state exists.
5. `document-release` can reference this command as the supported recording path.
6. `record-branch-closure` is the documented next command when branch closure is missing.
7. `advance-late-stage` is the documented first-choice agent-facing command for the normal release-readiness path once branch closure is current.
8. Same-state rerun behavior is deterministic and idempotent.

## Concrete Examples

### Example 1: Normal Late-Stage Flow

Scenario:

- branch closure is current
- release notes and documentation updates are complete
- `advance-late-stage` is invoked while the workflow phase is `document_release_pending`
- the command includes `--result ready`

Expected result:

- `advance-late-stage` validates the current reviewed branch closure, delegates to `record-release-readiness`, and records one current release-readiness milestone bound to that branch state
- optional generated markdown artifact for human consumption
- no hand-authored release-readiness file required for authority

### Example 2: Doc Fix After Release-Readiness

Scenario:

- release-readiness was recorded against reviewed state `R1`
- a follow-up doc fix or changelog adjustment lands, producing workspace state `R2`

Expected result:

- the earlier release-readiness milestone becomes `stale_unreviewed`
- the runtime does not silently refresh the old record
- a new reviewed branch closure must exist before a fresh release-readiness milestone is recorded

### Example 3: Blocked Release-Readiness Fixed By Repo Changes

Scenario:

- release-readiness was recorded as `blocked`
- resolving the blocker required edits to repo-tracked release notes or other branch content

Expected result:

- workflow first routes to execution reentry or `repair-review-state`
- the prior blocked release-readiness milestone is no longer reusable as current branch truth
- the operator records a new branch closure
- `advance-late-stage --result ready ...` runs only after the new current reviewed branch closure exists

## Test Strategy

- add a CLI-only happy-path test for recording release-readiness against current reviewed branch state
- add a CLI-only negative-path test for `--result blocked` that proves workflow remains `document_release_pending`
- add a CLI-only `advance-late-stage` test for `document_release_pending`
- add a stale-unreviewed branch test after post-milestone edits
- add a superseded-milestone test when later reviewed branch state replaces the earlier one
- keep narrow validator tests only for derived artifact compatibility where needed

## Risks

- keeping markdown as the only primary truth surface would preserve the current late-stage churn
- recording release-readiness against raw workspace state instead of current reviewed branch state would make the milestone too weak to trust
