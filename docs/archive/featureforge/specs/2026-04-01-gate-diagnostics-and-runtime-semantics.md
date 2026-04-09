# Gate Diagnostics And Runtime Semantics

**Workflow State:** Implementation Target  
**Spec Revision:** 4  
**Last Reviewed By:** clean-context review loop
**Implementation Target:** Current

## Problem Statement

The old runtime vocabulary is centered on stale evidence, packet fingerprints, and mutable receipt matching.

The supersession-aware model changes the important questions:

- what is the workspace state now
- what is the current reviewed state
- which closures are current
- which closures were superseded by later reviewed work
- which closures are stale because unreviewed changes landed afterward

If the gates and status surfaces do not expose those answers clearly, the new model will still feel opaque.

## Desired Outcome

Operators and agents should be able to read status and gate failures without guessing whether they are blocked by:

- missing current closure
- superseded prior closure
- stale-unreviewed current closure
- missing milestone against current reviewed branch state
- malformed derived artifact

## Decision

Selected approach: make current reviewed state and effective current closure state explicit in status and diagnostics.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- `2026-04-01-execution-task-closure-command-surface.md`
- `2026-04-02-branch-closure-recording-and-binding.md`

## Requirement Index

- [REQ-001][behavior] Status output must distinguish workspace state from current reviewed state.
- [REQ-002][behavior] Status output must expose effective current closure state, including current, superseded, and stale-unreviewed counts or details.
- [REQ-003][behavior] `gate-review` and `gate-finish` must reason over the same effective current closure model and late-stage truth semantics.
- [REQ-004][behavior] Gate failures must expose structured expected-versus-observed payloads for current reviewed state, milestone binding, and stale-unreviewed conditions.
- [REQ-005][behavior] Public diagnostics must clearly distinguish “superseded by later reviewed work” from “stale because unreviewed changes landed.”
- [REQ-006][behavior] Public validation and bypass flows must make their control-flow decision before validating irrelevant inputs.
- [REQ-007][behavior] Public state names such as `current_branch_reviewed_state_id` and `workspace_state_id` must replace ambiguous freshness names such as `latest_*` where those names imply real currentness.
- [REQ-008][behavior] Gate and status surfaces must consume a stable review-state query/read-model interface rather than raw stores, markdown artifacts, or ad hoc state scans.
- [REQ-009][behavior] Workflow-facing status outputs must expose the exact preferred aggregate command or lower-level fallback command string or command template the operator is expected to run next.
- [REQ-010][behavior] Status output must expose a first-class `phase_detail` or equivalent substate field whenever routing depends on task/final-review dispatch or recording readiness.
- [REQ-011][verification] Tests must prove that status and gate output expose current reviewed state, superseded state, stale-unreviewed state, and milestone mismatches clearly.
- [REQ-012][verification] Tests must prove `gate-review` and `gate-finish` remain semantically aligned under the new closure model.

## Scope

In scope:

- status field naming
- gate diagnostic payloads
- current/superseded/stale-unreviewed vocabulary
- late-gate truth parity
- control-flow clarity

Out of scope:

- redesigning workflow phases directly
- changing review policy itself

## Selected Approach

Expose at least these public concepts:

- `workspace_state_id`
- `current_branch_reviewed_state_id`
- `current_task_closures`
- `superseded_closures_summary`
- `stale_unreviewed_closures`
- `current_branch_closure_id`
- `current_release_readiness_state`
- `current_final_review_state`
- `current_qa_state`
- `qa_requirement`
- `follow_up_override`
- `finish_review_gate_pass_branch_closure_id`
- `recommended_command`

Gate diagnostics should also expose structured fields such as:

- `expected_reviewed_state_id`
- `observed_workspace_state_id`
- `superseded_by`
- `stale_reason`
- `required_next_record`

Identity rule:

- `workspace_state_id`, `observed_workspace_state_id`, and `expected_reviewed_state_id` use the same typed identity family as `reviewed_state_id`
- repo-writing workflows derive observed workspace identity from normalized repo-tracked content only; untracked files do not change those fields

Status and gates should be read-only consumers of the authoritative review-state query layer.

## Default Status Shape

The default status surface should be concrete enough to implement consistently:

```text
workspace_state_id
current_branch_reviewed_state_id
current_branch_closure_id
harness_phase
phase_detail
review_state_status
recording_context
current_task_closures
superseded_closures_summary
stale_unreviewed_closures
current_release_readiness_state
current_final_review_state
current_qa_state
qa_requirement
follow_up_override
finish_review_gate_pass_branch_closure_id
blocking_records[]
next_action
recommended_command
```

Frozen status enums:

- `review_state_status`: `clean` | `stale_unreviewed` | `missing_current_closure`
- `phase_detail`: `execution_in_progress` | `execution_reentry_required` | `task_review_dispatch_required` | `task_review_result_pending` | `task_closure_recording_ready` | `branch_closure_recording_required_for_release_readiness` | `release_readiness_recording_ready` | `release_blocker_resolution_required` | `final_review_dispatch_required` | `final_review_outcome_pending` | `final_review_recording_ready` | `qa_recording_required` | `test_plan_refresh_required` | `finish_review_gate_ready` | `finish_completion_gate_ready` | `handoff_recording_required` | `planning_reentry_required`
- `next_action`: `continue execution` | `execution reentry required` | `dispatch review` | `wait for external review result` | `close current task` | `repair review state / reenter execution` | `record branch closure` | `advance late stage` | `resolve release blocker` | `dispatch final review` | `run QA` | `refresh test plan` | `run finish review gate` | `run finish completion gate` | `hand off` | `pivot / return to planning`

Default summarization policy:

1. `current_task_closures` is summarized by default.
2. `superseded` closures are summarized as counts plus representative ids by default.
3. Fully enumerated historical superseded closures are available only in expanded or verbose output.
4. `stale_unreviewed` blockers are always surfaced explicitly in default output.
5. `recommended_command` must point to exactly one runtime command string or command template whenever the next runtime mutation or inspection command is actionable. It must never be a conjunction or disjunction of multiple commands.
6. `recommended_command` may be omitted only for external-review wait substates where `next_action=wait for external review result` and no runtime mutation is actionable until external review input arrives, or for `phase_detail=test_plan_refresh_required` where the current-branch test-plan artifact must be refreshed through `featureforge:plan-eng-review` before direct QA recording becomes actionable.
7. when present, `recommended_command` must point to a single concrete or template form of `close-current-task`, `repair-review-state`, `record-review-dispatch`, `record-branch-closure`, `advance-late-stage`, `record-qa`, `gate-review`, `gate-finish`, `transfer`, `featureforge workflow operator --plan <path>`, `featureforge workflow record-pivot`, or the exact step-oriented execution command that the operator is expected to run next.
8. `phase_detail` must carry dispatch-versus-recording readiness when `phase` alone is insufficient.
9. `current_task_closures` entries must each carry their own `reviewed_state_id` and effective reviewed-surface summary; there is no singular task-wide current reviewed state outside that collection.
10. when `phase_detail=task_closure_recording_ready` or `phase_detail=final_review_recording_ready`, `recording_context` must expose the runtime-known identifiers required to execute the exact `recommended_command` without depending on cached prior mutation output.
11. `recording_context` is omitted entirely when the current `recommended_command` needs no extra runtime-known identifiers and no documented primitive-fallback or transparency ids need to be surfaced; it must never be `null` or an empty object.
12. when `phase_detail=release_readiness_recording_ready` or `phase_detail=release_blocker_resolution_required`, `recording_context.branch_closure_id` must be exposed for authoritative binding context, transparency, and primitive fallback to `record-release-readiness` even though the aggregate release-readiness command still takes only `--plan`, `--result`, and `--summary-file`.
13. in `final_review_recording_ready`, `recording_context.branch_closure_id` is exposed for authoritative binding context, transparency, and primitive fallback even though the aggregate final-review command still takes only `--dispatch-id`.
14. when `phase=executing` and the routed `recommended_command` is a step-oriented execution command, the runtime must expose `execution_command_context` with a deterministic `command_kind = begin|complete|reopen`; `note` remains auxiliary and must not be surfaced as the singular routed command.
15. the supporting `plan execution status` surface exposes the harness-owned phase field as `harness_phase`; `phase` is reserved for the authoritative `workflow operator` route contract.

`blocking_records[]` must be a structured summary of active public blockers, with each element carrying at least:

- `code`
- `scope_type`
- `scope_key`
- `record_type`
- `record_id` when a concrete record exists
- `review_state_status`
- `required_follow_up` when one is already deterministically known
- `message`

`blocking_records[]` population rules:

1. one element must exist for each active public blocker that independently prevents the current routed recording, repair, gate, or finish step
2. when `review_state_status=stale_unreviewed`, `blocking_records[]` must include at least one entry for each currently surfaced stale task or branch closure that blocks progression
3. when a required current record is missing, `blocking_records[]` must include one entry naming the missing `record_type` and the scope it blocks, even if `record_id` is null
4. when `follow_up_override` has already forced the next safe negative-result path, the blocking element must carry the resulting `required_follow_up`
5. external-review wait substates do not populate `blocking_records[]` solely because the reviewer has not returned yet; those are wait states, not runtime structural blockers
6. `blocking_records[]` is a projection summary, not a second authority model; every element must be derivable from current closure, milestone, and workflow decision truth

## Required Follow-Up Vocabulary

When a mutating command returns `required_follow_up`, the value must be chosen from this frozen vocabulary:

- `execution_reentry`
- `repair_review_state`
- `record_review_dispatch`
- `record_branch_closure`
- `resolve_release_blocker`
- `record_handoff`
- `record_pivot`

Commands may omit `required_follow_up` only when no immediate follow-up is required, or when the direct command failed because the call was out of phase and the exact next command must be re-derived through `featureforge workflow operator --plan <path>`.

## Shared Out-Of-Phase Response Contract

When a direct mutating or gating command is invoked out of phase and the runtime requires workflow/operator to re-derive the next safe step, the command must return one shared machine-readable blocked contract:

- `action=blocked`
- `code=out_of_phase_requery_required`
- `required_follow_up` omitted
- `recommended_command=featureforge workflow operator --plan <path>`
- `rederive_via_workflow_operator=true`

The command may add human-readable diagnostic detail, but it must not invent command-specific out-of-phase payload shapes.

Commands with command-specific result envelopes may retain their command-specific action fields, but when they return the shared out-of-phase contract those command-specific fields must either be omitted or set to their blocked value. They must not contradict the top-level `action=blocked`.

Chosen query-model rule:

- milestone state remains a separate query field family, not an embedded subtype of closure records in public status output

That means the public read model should expose closure state and milestone state side by side rather than forcing consumers to infer milestone state through closure payload nesting.

Chosen current-milestone selection rule:

- `current_release_readiness_state`, `current_final_review_state`, and `current_qa_state` must each identify one selected query-current milestone at most
- selection is always relative to the still-current branch closure
- if multiple auditable milestones of the same type exist on that same still-current branch closure, query surfaces must expose only the newest eligible current milestone and mark the older same-closure milestones `historical`

Chosen finish-gate checkpoint rule:

- `gate-review` pass must record or refresh one runtime-owned gate checkpoint bound to the still-current branch closure it validated
- status/query surfaces must expose that checkpoint as `finish_review_gate_pass_branch_closure_id`
- `finish_completion_gate_ready` is true only when `finish_review_gate_pass_branch_closure_id` equals the still-current `current_branch_closure_id`
- `gate-review` is therefore not a pure query. It is a bounded evaluation command that may mutate only the runtime-owned finish-gate checkpoint for the branch closure it just validated; it must not mint dispatch lineage or any task/branch/release/final-review/QA milestone.

## Finish-Gate Command Contracts

`gate-review` must accept at least:

- `--plan <path>`

`gate-review` must fail closed unless all of these are true:

1. `phase=ready_for_branch_completion`
2. `phase_detail=finish_review_gate_ready`
3. `review_state_status` is `clean`
4. a still-current branch closure exists for the branch being finished

`gate-review` must return at least:

- `action`: `passed` | `blocked`
- `current_branch_closure_id`
- `finish_review_gate_pass_branch_closure_id`
- `trace[]` or `trace_summary`

On `action=passed`, `gate-review` must record or refresh the finish-gate checkpoint for `current_branch_closure_id`.
On `action=blocked`, `gate-review` must not mutate that checkpoint.
If the direct command is invoked out of phase and the exact next safe step is not deterministically derivable from public review-state truth, `gate-review` must return the shared out-of-phase response contract defined above.

`gate-finish` must accept at least:

- `--plan <path>`

`gate-finish` must fail closed unless all of these are true:

1. `phase=ready_for_branch_completion`
2. `phase_detail=finish_completion_gate_ready`
3. `review_state_status` is `clean`
4. `finish_review_gate_pass_branch_closure_id` equals the still-current `current_branch_closure_id`

`gate-finish` must return at least:

- `action`: `passed` | `blocked`
- `current_branch_closure_id`
- `finish_review_gate_pass_branch_closure_id`
- `trace[]` or `trace_summary`

On `action=blocked`, `gate-finish` must not fabricate or refresh a review-gate checkpoint.
If the direct command is invoked out of phase and the exact next safe step is not deterministically derivable from public review-state truth, `gate-finish` must return the shared out-of-phase response contract defined above.

Chosen negative-result override rule:

- status/query surfaces must expose `follow_up_override = none | record_handoff | record_pivot`
- `follow_up_override` is derived from authoritative workflow decision state, not from command-local heuristics
- one runtime-owned `FollowUpOverrideResolver` must own derivation, precedence, and clearing of `follow_up_override`
- `follow_up_override=record_handoff` is set only when authoritative workflow state has already decided the next safe negative-result follow-up is handoff rather than execution reentry
- `follow_up_override=record_pivot` is set only when authoritative workflow state has already decided the next safe negative-result follow-up is planning reentry rather than execution reentry
- if both raw handoff and pivot conditions are present simultaneously, `FollowUpOverrideResolver` must prefer `record_pivot`
- `follow_up_override` must clear automatically once the corresponding handoff or pivot recording is completed successfully, or once authoritative workflow state reevaluates to `none`
- task-close, final-review, and QA negative-result recording commands must consult `follow_up_override` before choosing `required_follow_up`

## Example Status Matrix

| workspace vs reviewed | closure state | expected public reading |
| --- | --- | --- |
| same | `current` | current reviewed state is usable |
| newer workspace | `stale_unreviewed` | new review or re-closure required |
| same for current closure, older historical overlap exists | `clean` plus superseded closure summary | no defect; older work was legitimately replaced |
| missing current closure | `missing_current_closure` | operator must record or repair closure state before advancing |

## Acceptance Criteria

1. Status output makes it obvious when the workspace moved past the current reviewed state.
2. Status output makes it obvious when an older closure is simply superseded rather than broken.
3. Gate failures clearly distinguish stale-unreviewed state from malformed records.
4. `gate-review` and `gate-finish` cannot disagree about effective current closure truth.
5. Public field names no longer imply freshness they do not actually represent.
6. Status and gate policy remain testable without exercising unrelated artifact-rendering code.
7. Status output names the exact preferred command string or command template the agent should run next.
8. Status output exposes the substate the routing layer needs instead of forcing clients to infer it from prose.

## Test Strategy

- add status tests for workspace versus current reviewed state
- add diagnostic tests for stale-unreviewed closures
- add diagnostic tests for superseded closures
- add parity tests for `gate-review` and `gate-finish`
- add control-flow tests proving bypass and non-mutating validation do not fail on irrelevant inputs first
- add status tests for `recommended_command` mapping to aggregate commands
- add status tests for `phase_detail` on task-closure, document-release, and final-review phases

## Risks

- carrying forward the old freshness vocabulary will make the new model harder to understand than it needs to be
- exposing only reason codes without state payloads will preserve trial-and-error repair loops
