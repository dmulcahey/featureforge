# Final-Review Recording On Current Reviewed Branch Closures

**Workflow State:** Implementation Target  
**Spec Revision:** 3  
**Last Reviewed By:** clean-context review loop
**Implementation Target:** Current

## Problem Statement

Final review is currently modeled as a strict pair of markdown artifacts that must keep matching current repo, branch, base branch, plan, and head truth.

That is too fragile for the real architectural question.

The real authoritative event is:

- an independent reviewer passed the current reviewed branch closure state

Everything else should be derived from that.

## Desired Outcome

Final review should be recordable through one public runtime command that records an independent final-review milestone against the current reviewed branch closure and optionally emits the human-readable public and dedicated reviewer artifacts.

After this work:

- the authoritative truth is a runtime-owned final-review record
- the record binds to current reviewed branch closure state
- older final-review milestones become historical when later reviewed branch state supersedes their bound branch closure
- human-readable review artifacts become derived outputs
- agents can progress the normal final-review stage through one preferred aggregate late-stage command instead of hand-assembling the standard terminal sequence

## Decision

Selected approach: add a first-class runtime-owned final-review milestone record bound to the current reviewed branch closure.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- `2026-04-02-branch-closure-recording-and-binding.md`
- `2026-04-01-release-readiness-recording-and-binding.md`
- `2026-04-01-execution-repair-and-state-reconcile.md`
- `2026-04-01-workflow-public-phase-contract.md`

## Requirement Index

- [REQ-001][behavior] FeatureForge must provide a public runtime command for recording independent final review against the current reviewed branch closure.
- [REQ-002][behavior] The authoritative result must be a runtime-owned final-review record, not a pair of hand-authored markdown artifacts.
- [REQ-003][behavior] The final-review record must bind at least current reviewed branch state id, review independence provenance, reviewer source, reviewer id, result, repo, branch, base branch, and plan path/revision.
- [REQ-004][behavior] The runtime may emit public and dedicated human-readable final-review artifacts as derived outputs, but gates must not depend on those artifacts as the only primary truth surface.
- [REQ-005][behavior] A newer reviewed branch state must not cause old final-review proof to be rewritten in place; the old milestone becomes historical through closure lineage.
- [REQ-006][behavior] If the workspace moves after a current final-review milestone without a new reviewed branch closure, the milestone must become stale.
- [REQ-007][behavior] Release-readiness remains a separate branch milestone and must not be silently synthesized by final-review recording.
- [REQ-008][verification] Integration tests must prove that final review binds to current reviewed branch state and that later reviewed state supersedes earlier final-review milestones cleanly.
- [REQ-009][verification] Negative tests must still prove rejection of contradictory or malformed derived reviewer artifacts where those artifacts remain supported.
- [REQ-010][behavior] `advance-late-stage` must be the preferred aggregate agent-facing command for normal final-review progression once a current reviewed branch closure and a valid final-review dispatch already exist, with `record-final-review` retained as the lower-level stage primitive.
- [REQ-011][behavior] A public `record-branch-closure` primitive must exist and workflow/repair surfaces must route operators to it explicitly when final-review recording needs a current reviewed branch closure and one is missing.
- [REQ-012][behavior] Re-running final-review recording for the same still-current branch closure, dispatch lineage, and equivalent reviewer outcome must be safe and idempotent.

## Scope

In scope:

- public final-review recording command
- runtime-owned final-review records
- independent reviewer provenance capture
- derived public and dedicated reviewer artifacts
- superseded and stale final-review milestone behavior

Out of scope:

- changing dedicated-independent review policy
- weakening finish-gate strictness
- changing release-readiness policy itself

## Selected Approach

Add:

- `featureforge plan execution record-branch-closure --plan <path>`
- `featureforge plan execution record-final-review --plan <path> ...`

Add preferred aggregate late-stage surface:

- `featureforge plan execution advance-late-stage --plan <path> ...`

The operator supplies reviewer result input. The runtime supplies:

- current reviewed branch state identity
- repo/branch/base-branch binding
- plan binding
- milestone storage
- optional derived artifact generation

`advance-late-stage` should:

1. inspect workflow/operator and review-state truth
2. fail closed unless `final_review_pending` is the current valid late stage
3. fail closed unless a current reviewed branch closure already exists
4. ensure required dispatch-lineage preconditions are satisfied through supported services
5. delegate to `record-final-review`
6. return a structured trace naming the stage primitive it invoked and the resulting milestone state

## Public Contract

The authoritative output is a final-review record containing at least:

- source plan path and revision
- repo slug
- branch and base branch
- current reviewed branch state id
- reviewer provenance
- reviewer source
- reviewer id
- result
- summary
- record status

Any public or dedicated markdown artifacts generated from it are derivative.

Chosen output policy:

1. the canonical derived human-readable outputs remain markdown for compatibility with the current workflow surface
2. review summary content lives in the authoritative final-review record
3. public and dedicated markdown artifacts are generated from the record, not hand-authored as authority
4. in the first slice, final-review records do not populate `generated_by_identity`; reviewer identity and independence are expressed through `reviewer_provenance`

Chosen command policy:

1. `record-final-review` is the stable primitive for the stage-specific recording service
2. if branch closure or a current release-readiness result `ready` are missing, workflow/operator must reroute back to `document_release_pending`; `record-branch-closure` is then the explicit prerequisite command for returning to late-stage branch flow
3. if current reviewed state is `stale_unreviewed`, workflow/operator must reroute to `phase=executing` with exact next command `repair-review-state`; only that repair flow may later reroute the branch back to `document_release_pending`
4. `record-review-dispatch` remains a separate explicit mutation surface when final independent review is being requested
5. `advance-late-stage` is the preferred agent-facing command when workflow/operator already routes the branch to `final_review_pending`, current branch closure exists, and final-review dispatch already exists
6. final-review examples must pass the originating `--dispatch-id <dispatch-id>` explicitly

Chosen result and reviewer-source policy:

1. accepted final-review results are `pass` and `fail`
2. accepted `reviewer_source` values in the first slice are `fresh-context-subagent`, `cross-model`, and `human-independent-reviewer`
3. those accepted `reviewer_source` values are the complete authoritative independence-policy classes for the first slice; the runtime must fail closed on any other category rather than inventing a second independence-policy surface
4. `reviewer_provenance` for final review is exactly the pair `{ reviewer_source, reviewer_id }`
5. the runtime must reject unknown reviewer-source vocabulary at record time
6. repo/branch/base-branch bindings must come from the runtime-owned `RepositoryContextResolver`, not from duplicated per-command parsing or hand-authored input

Determinism rule:

- workflow/query surfaces derive final-review pending substates from dispatch truth, not from a hidden staged reviewer-result object
- workflow/query surfaces must not emit `final_review_pending` when branch closure or a current release-readiness result `ready` are missing; that case reroutes back to `document_release_pending`
- workflow/query surfaces must not emit `final_review_pending` when current reviewed state is `stale_unreviewed`; that case reroutes to `executing` with exact next command `repair-review-state`

## Aggregate Command Contract

`advance-late-stage` must accept at least, for the final-review path:

- `--plan <path>`
- `--dispatch-id <dispatch-id>`
- `--reviewer-source fresh-context-subagent|cross-model|human-independent-reviewer`
- `--reviewer-id <reviewer-id>`
- `--result pass|fail`
- `--summary-file <path>`

It must fail closed unless all of these are true:

1. `phase=final_review_pending`
2. `review_state_status` is `clean`
3. a valid explicit final-review dispatch record exists for the supplied `dispatch_id`
4. a current release-readiness milestone with result `ready` exists for the same still-current branch closure the final-review record will bind to
5. when workflow/operator is queried with `--external-review-result-ready`, the corresponding routed substate is `final_review_recording_ready`

The aggregate command validates those underlying truth conditions directly. It does not require the caller to persist a prior workflow/operator response, but the public routing surface must expose `final_review_recording_ready` when those same conditions are queried through workflow/operator with `--external-review-result-ready`.

`recording_context.branch_closure_id` in `final_review_recording_ready` is diagnostic and authoritative binding context, not a second required aggregate input. The aggregate command continues to accept `--dispatch-id` only. At mutation time it must resolve the still-current branch closure through authoritative query state, validate the same-closure prerequisites required for final review, and delegate using that resolved closure. The emitted branch-closure id exists for transparency and primitive fallback to `record-final-review`; it is not a caller-supplied cross-check token.

`final_review_outcome_pending` is an external-input wait state, not a hidden runtime latch. While the independent reviewer has not yet returned a result to the caller, workflow/operator omits `recommended_command`. When the caller reruns workflow/operator with `--external-review-result-ready`, the public routed substate becomes `final_review_recording_ready` and the exact next mutation command becomes this final-review path of `advance-late-stage` for the same dispatch lineage.

It must not record final review when `review_state_status` is `stale_unreviewed` or `missing_current_closure`.
It must also fail closed before mutation if release-readiness-only arguments or result vocabulary are supplied while `phase=final_review_pending`.

## Primitive Contract

`record-final-review` must accept at least:

- `--plan <path>`
- `--branch-closure-id <closure-id>`
- `--dispatch-id <dispatch-id>`
- `--reviewer-source fresh-context-subagent|cross-model|human-independent-reviewer`
- `--reviewer-id <reviewer-id>`
- `--result pass|fail`
- `--summary-file <path>`

`record-final-review` must not synthesize a branch closure or dispatch lineage implicitly. Workflow/operator or repair surfaces may direct the operator through those prerequisites first, but the primitive records the milestone only against explicit prerequisite ids.
`record-final-review` must also fail closed unless the supplied `branch_closure_id` is still current, the supplied `dispatch_id` is still the current valid final-review dispatch for that same still-current branch closure, and that branch closure has a current release-readiness milestone with result `ready`.

## Return Contract

Final-review recording must return at least:

- `action`: `recorded` | `already_current` | `blocked`
- `branch_closure_id`
- `dispatch_id`
- `result`
- `required_follow_up` when follow-up work is required

When `advance-late-stage` takes the final-review path, it must return the same primitive contract plus:

- `stage_path=final_review`
- `delegated_primitive=record-final-review`
- `trace[]` or `trace_summary` describing the validations and delegated primitive call it executed

For final-review recording:

- `action=recorded` is used when a final-review milestone was appended, including `--result fail`
- `action=blocked` is reserved for pre-mutation structural failures such as missing branch closure, missing dispatch, stale review state, or conflicting same-state rerun inputs
- structural blocked follow-up values are `record_review_dispatch` and `repair_review_state`
- if the direct command is invoked while branch closure or release-readiness prerequisites are missing, the runtime must return the shared out-of-phase response contract defined by `2026-04-01-gate-diagnostics-and-runtime-semantics.md`; workflow/operator then reroutes the branch back to `document_release_pending`
- if the direct command is invoked while current reviewed state is `stale_unreviewed`, the runtime must fail closed with `required_follow_up=repair_review_state`
- recorded negative-result follow-up uses `execution_reentry`, `record_handoff`, or `record_pivot`

`already_current` means:

- the same still-current branch closure already has an equivalent recorded final-review outcome bound to the same still-current dispatch lineage
- equivalent means same still-current `branch_closure_id`, same still-current `dispatch_id`, same `reviewer_source`, same `reviewer_id`, same `result`, and the same normalized summary content as produced by the shared runtime-owned `SummaryNormalizer`
- no duplicate current final-review milestone was appended

If the branch closure and dispatch are the same but one or more equivalence inputs differ, the runtime must fail closed with `action=blocked`, emit a validation error for conflicting same-state rerun inputs, and append no new milestone.

## Negative Result Handling

`--result fail` must be operationally defined:

1. the runtime records a failed final-review milestone for audit against the current branch closure
2. the failed milestone is not treated as current branch-completion truth
3. the command returns `action=recorded`
4. the command returns:
   - `required_follow_up=execution_reentry` when normal execution repair is the next safe action
   - `required_follow_up=record_handoff` when an authoritative handoff override is already in force
   - `required_follow_up=record_pivot` when an authoritative pivot override is already in force
   - the command must choose among those values by consulting authoritative workflow query field `follow_up_override = none|record_handoff|record_pivot`, not by command-local heuristics
5. workflow/operator must route immediately from the returned `required_follow_up`
   - structural blocked returns use the workflow contract's structural mappings for `repair_review_state` and `record_review_dispatch`
   - `execution_reentry` routes immediately to `phase=executing` with `phase_detail=execution_reentry_required`
   - `record_handoff` routes immediately to `phase=handoff_required` with `phase_detail=handoff_recording_required`
   - `record_pivot` routes immediately to `phase=pivot_required` with `phase_detail=planning_reentry_required`

Chosen lineage policy:

1. final-review milestones inherit supersession and historical state through the closure record they bind to
2. no separate milestone-level `superseded_by` edge is required in the first implementation slice
3. query layers may denormalize that relationship for convenience, but the authority source remains closure lineage

## Acceptance Criteria

1. One supported command can record a passing independent final review against the current reviewed branch closure.
2. Finish gating can reason over that record without requiring hand-authored markdown as the only primary truth surface.
3. Later reviewed branch state does not require rewriting old final-review proof; the older milestone simply becomes historical or stale as appropriate.
4. Unreviewed post-review branch changes make final review stale until a new reviewed branch state is recorded and independently reviewed.
5. Derived artifacts, when emitted, are validator-compatible by construction.
6. `record-branch-closure` is the documented next command when branch closure is missing.
7. `advance-late-stage` is the documented first-choice agent-facing command for the normal final-review path once branch closure, a current release-readiness result `ready`, and dispatch already exist.
8. Same-state rerun behavior is deterministic and idempotent.

## Concrete Examples

### Example 1: Independent Review Against Current Branch Closure

Scenario:

- branch closure is current
- release-readiness is current
- an independent reviewer assesses the current reviewed branch state and passes it

Expected result:

- `advance-late-stage` validates current branch-closure truth and current release-readiness truth, then delegates to `record-final-review` and records one current final-review milestone bound to that branch closure
- generated public and dedicated markdown artifacts, if requested, match the authoritative record by construction

### Example 2: Unreviewed Fix After Final Review

Scenario:

- final review was recorded against reviewed state `F1`
- a later fix lands in response to new feedback, producing workspace state `F2`

Expected result:

- the older final-review milestone becomes `stale_unreviewed`
- the runtime does not ask the operator to repair old markdown fingerprints
- a new reviewed branch closure plus independent final review is required for `F2`

### Example 3: Later Reviewed Replacement After Final Review

Scenario:

- final review was recorded against reviewed state `F1`
- later reviewed work records a new current reviewed branch closure `F2`
- independent final review is then recorded for `F2`

Expected result:

- the older final-review milestone for `F1` becomes historical through closure lineage
- the newer final-review milestone for `F2` becomes current
- the runtime does not confuse historical supersession with stale-unreviewed drift

## Test Strategy

- add a CLI-only happy-path test for recording final review against current reviewed branch state
- add a CLI-only negative-path test for `--result fail` that proves runtime returns the authoritative `required_follow_up`
- add a CLI-only `advance-late-stage` test for `final_review_pending`
- add a stale-unreviewed branch test after post-review edits
- add a superseded-final-review test when later reviewed branch state replaces the earlier one
- keep narrow validator tests only for derived artifact compatibility and independence rules where needed

## Risks

- keeping markdown artifacts as the only primary truth surface would preserve the current mismatch loops
- dropping machine-bound reviewed branch state identity would make independent review impossible to bind meaningfully
