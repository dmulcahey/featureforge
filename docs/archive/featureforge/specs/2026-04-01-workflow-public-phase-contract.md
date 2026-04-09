# Workflow Public Phase Contract

**Workflow State:** Implementation Target  
**Spec Revision:** 4  
**Last Reviewed By:** clean-context review loop
**Implementation Target:** Current

## Problem Statement

Under the supersession-aware model, the workflow layer can no longer hide review-state problems behind generic late-stage phases.

Operators need a clear public answer to:

- am I still executing
- do I need to record task closure
- did later changes supersede earlier reviewed work
- did unreviewed changes make current reviewed state stale
- am I ready for release-readiness or final review

If the workflow surface keeps reporting generic phases while the real issue is stale review state, the operator will still end up in confusing loops.

## Desired Outcome

FeatureForge should have one authoritative public workflow contract that reflects current reviewed closure truth and tells the operator the next correct action.

## Decision

Selected approach: align the public phase and routing contract to the supersession-aware state model.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- `2026-04-01-gate-diagnostics-and-runtime-semantics.md`

## Requirement Index

- [REQ-001][behavior] FeatureForge must define one canonical public phase vocabulary aligned to current reviewed closure truth.
- [REQ-002][behavior] The public contract must expose a distinct review-state repair or execution-reentry signal when current reviewed closures are stale or missing.
- [REQ-003][behavior] The public contract must clearly distinguish “superseded historical closure” from “stale current closure that needs a new review.”
- [REQ-004][behavior] Late-stage precedence must remain explicit across release-readiness, final review, QA, and branch completion under the new reviewed branch closure model.
- [REQ-005][behavior] Public routing outputs must present one coherent recommendation contract instead of split semantics across `next_skill`, `next_action`, and `recommended_skill`.
- [REQ-006][behavior] Workflow routing must consume the stable public review-state query interface rather than reconstructing closure semantics from unrelated execution internals.
- [REQ-007][behavior] Workflow outputs must expose the exact preferred runtime command string or command template the agent is expected to run next.
- [REQ-008][verification] Runtime outputs, generated docs, and contract tests must use the same public phase vocabulary and the same routing meanings.

## Scope

In scope:

- public phase inventory
- review-state repair / execution-reentry signaling
- routing recommendation contract
- late-stage precedence under the new closure model

Out of scope:

- deep execution-engine redesign unrelated to the public contract

## Public Phases

These names should be treated as frozen for implementation unless a later spec revision changes them intentionally.

## Public CLI Contract

The authoritative workflow query surface is:

- `featureforge workflow operator --plan <path>`
- `featureforge workflow operator --plan <path> --external-review-result-ready` as an explicit query-time hint that the caller already has the external review result in hand for the currently pending task-scope or final-review dispatch

The future implementation target requires explicit `--plan <path>` for this public command. Planless invocation is not part of the normative contract for the supersession-aware model.

The public `phase` should cover these operator moments:

- `executing`
- `task_closure_pending`
- `document_release_pending`
- `final_review_pending`
- `qa_pending`
- `ready_for_branch_completion`
- `handoff_required`
- `pivot_required`

The public API shape should also include a separate review-state field:

- `review_state_status`

And one explicit phase-substate field:

- `phase_detail`

And one explicit execution-command derivation field for `phase=executing`:

- `execution_command_context`

And one explicit next-action field:

- `next_action`

Allowed `review_state_status` values:

- `clean`
- `stale_unreviewed`
- `missing_current_closure`

Allowed `phase_detail` values:

- `execution_in_progress`
- `execution_reentry_required`
- `task_review_dispatch_required`
- `task_review_result_pending`
- `task_closure_recording_ready`
- `branch_closure_recording_required_for_release_readiness`
- `release_readiness_recording_ready`
- `release_blocker_resolution_required`
- `final_review_dispatch_required`
- `final_review_outcome_pending`
- `final_review_recording_ready`
- `qa_recording_required`
- `test_plan_refresh_required`
- `finish_review_gate_ready`
- `finish_completion_gate_ready`
- `handoff_recording_required`
- `planning_reentry_required`

This is the chosen shape:

- `phase` answers where the workflow is
- `phase_detail` answers which substate inside the phase is active
- `review_state_status` answers whether current reviewed state is usable
- `qa_requirement` answers whether approved-plan metadata requires QA for the current branch
- `follow_up_override` answers whether negative-result commands must emit `record_handoff` or `record_pivot` instead of `execution_reentry`
- `finish_review_gate_pass_branch_closure_id` answers whether `gate-review` already passed for the still-current branch closure
- `recording_context` answers which runtime-known identifiers are required to execute the current `recommended_command` without relying on cached prior mutation output
- `execution_command_context` answers which execution step kind and scope the runtime used to derive the exact execution `recommended_command`
- `next_action` answers what the operator should do next
- `recommended_command` answers the exact singular runtime command string or template the agent should run next

Authoritative-source rule:

- `qa_requirement` is derived from normalized approved-plan metadata field `QA Requirement: required|not-required`
- normalization is owned by the shared `PolicyMetadataNormalizer`, which trims surrounding whitespace, lowercases ASCII letters, accepts only `required` or `not-required`, and fail-closes on any other token
- `follow_up_override` is derived from authoritative workflow decision state, not from command-local inference
- `finish_review_gate_pass_branch_closure_id` is derived from a runtime-owned `gate-review` pass checkpoint bound to the still-current branch closure
- `recording_context` is derived from authoritative current dispatch and closure query state, not from cached prior CLI output
- `recording_context` is omitted entirely when the current `recommended_command` needs no extra runtime-known identifiers and no documented primitive-fallback or transparency ids need to be surfaced; it must never be `null` or an empty object
- `recording_context.task_number` and `recording_context.dispatch_id` must be present when `phase_detail=task_closure_recording_ready`
- `recording_context.branch_closure_id` must be present when `phase_detail=release_readiness_recording_ready` or `phase_detail=release_blocker_resolution_required`
- `recording_context.dispatch_id` and `recording_context.branch_closure_id` must be present when `phase_detail=final_review_recording_ready`
- in `release_readiness_recording_ready` and `release_blocker_resolution_required`, `recording_context.branch_closure_id` is emitted for authoritative binding context, transparency, and primitive fallback even though the aggregate release-readiness command still takes only `--plan`, `--result`, and `--summary-file`
- in `final_review_recording_ready`, `recording_context.branch_closure_id` is emitted for authoritative binding context, transparency, and primitive fallback even though the aggregate final-review command still takes only `--dispatch-id`
- `execution_command_context` is omitted entirely outside `phase=executing`
- when `phase=executing` and `recommended_command` is a step-oriented execution command, `execution_command_context` must be present and must carry at least `command_kind = begin|complete|reopen`, `task_number`, and `step_id` when the addressed command kind operates on one concrete step
- when `phase=executing` and `recommended_command=featureforge plan execution repair-review-state --plan <path>`, `execution_command_context` must be omitted because the immediate routed command is repair, not a step mutation
- `note` remains an auxiliary execution command available to callers between authoritative routed commands, but it must not appear as workflow/operator `recommended_command` in the first slice
- one shared `ExecutionCommandResolver` owns derivation of the exact execution `recommended_command` from `execution_command_context`
- `ExecutionCommandResolver` must map authoritative execution state to exactly one routed `command_kind = begin|complete|reopen` when review state is clean; if no single execution-step command can be derived unambiguously, workflow/operator must fail closed rather than emit a bundle or prose-only execution recommendation
- if `qa_requirement` is missing or invalid at a point where workflow/operator must decide between `qa_pending` and `ready_for_branch_completion`, workflow/operator must fail closed to `phase=pivot_required` with `phase_detail=planning_reentry_required`
- one runtime-owned `FollowUpOverrideResolver` owns derivation and clearing of `follow_up_override`
- `follow_up_override=record_handoff` is emitted only while authoritative workflow state requires negative-result routing to handoff
- `follow_up_override=record_pivot` is emitted only while authoritative workflow state requires negative-result routing to planning reentry
- if both raw handoff and pivot conditions exist at the same time, workflow/operator must expose `follow_up_override=record_pivot`
- `follow_up_override` clears automatically after the corresponding handoff or pivot recording succeeds, or when authoritative workflow state reevaluates to `none`

`recommended_command` is singular and deterministic:

- it must name exactly one runtime command string or command template
- it must not be a conjunction, disjunction, or prose bundle
- multi-step flows are represented by phase transitions and `phase_detail`, not by compound command strings
- when `next_action` includes non-runtime work such as resolving a blocker, `recommended_command` names the next runtime command to run after that prerequisite is satisfied
- `recommended_command` may be omitted only when `next_action=wait for external review result` or `next_action=refresh test plan`, because no runtime mutation is actionable until the external reviewer returns or a refreshed test plan exists
- `task_review_result_pending` and `final_review_outcome_pending` are external-input wait states. While the external reviewer result is not yet available to the caller, `recommended_command` remains omitted.
- `test_plan_refresh_required` is a plan-eng-review reroute state. While the current test-plan artifact is missing, malformed, stale, or otherwise fails authoritative provenance checks, `recommended_command` remains omitted and the operator is expected to refresh the plan through that workflow lane before direct QA recording can resume.
- when the caller reruns workflow/operator with `--external-review-result-ready`, the runtime must expose the matching recording-ready substate instead of inventing hidden stored reviewer-result state: `task_closure_recording_ready` for task scope and `final_review_recording_ready` for final-review scope

Allowed `next_action` families:

- `continue execution`
- `execution reentry required`
- `dispatch review`
- `wait for external review result`
- `close current task`
- `repair review state / reenter execution`
- `record branch closure`
- `advance late stage`
- `resolve release blocker`
- `dispatch final review`
- `run QA`
- `refresh test plan`
- `run finish review gate`
- `run finish completion gate`
- `hand off`
- `pivot / return to planning`

Historical supersession should be exposed through closure summaries and details, not as a separate `review_state_status` value. If the current reviewed state is healthy, the status remains `clean` even when older closures were superseded.

`review_state_repair_required` should not be a top-level phase. It should be represented by `review_state_status` plus the corresponding `next_action`.

Chosen representation detail:

- `task_closure_pending` is a real top-level phase because it represents a distinct operator milestone, not just an execution detail

`phase_detail` is phase-scoped, not a second status enum. Invalid combinations must fail closed in routing tests.

When a phase contains more than one valid substate, the phase-level table below describes only the phase meaning. The exact `next_action` and `recommended_command` must be derived from the active `phase_detail` and must match one row in the routing matrix.

## Phase Semantics

| phase | trigger semantics | expected next_action | recommended_command |
| --- | --- | --- | --- |
| `executing` | active implementation work is still the primary flow | derived from current execution substate | exact step-oriented execution command or repair command derived by `ExecutionCommandResolver` from current execution state |
| `task_closure_pending` | execution for the current task is done enough that the task-closure flow should run next | derived from `phase_detail` | exact task-closure command derived from `phase_detail` |
| `document_release_pending` | current reviewed branch state is ready for release documentation and release-readiness recording | derived from `phase_detail` | exact release-stage command derived from `phase_detail` |
| `final_review_pending` | current reviewed branch closure and a current release-readiness result `ready` are already in place and final independent review is the next late-stage milestone | derived from `phase_detail` | exact final-review-stage command derived from `phase_detail` |
| `qa_pending` | QA policy requires QA and the current reviewed branch closure, current release-readiness result `ready`, and current final-review result `pass` are already in place for the same branch closure | derived from `phase_detail` | exact QA-stage command derived from `phase_detail` |
| `ready_for_branch_completion` | the branch satisfies late-stage readiness and can move through the final finish gates before merge/cleanup flow | derived from `phase_detail` | exact finish-gate command derived from `phase_detail` |
| `handoff_required` | the workflow requires transfer to another owner or lane before more implementation continues locally | hand off | `featureforge plan execution transfer --plan <path> --scope <task|branch> --to <owner> --reason <reason>` |
| `pivot_required` | the workflow has determined that the current implementation direction should not continue without a plan/spec/strategy change | pivot / return to planning | `featureforge workflow record-pivot --plan <path> --reason <reason>` |

## Phase-Detail Trigger Rules

The routing layer must derive `phase_detail` deterministically from authoritative query outputs.

| phase_detail | trigger semantics |
| --- | --- |
| `execution_in_progress` | current actionable work remains in ordinary execution and no closer milestone is pending |
| `execution_reentry_required` | the next safe action is renewed execution work before any more late-stage progression, including after failed task review, failed task verification, failed final review, or failed QA unless handoff or pivot overrides it |
| `task_review_dispatch_required` | the current task is complete enough for review, but no valid task-scope review-dispatch record exists yet |
| `task_review_result_pending` | a valid task-scope review-dispatch record exists and workflow/operator is waiting for an external review result before task closure can be recorded |
| `task_closure_recording_ready` | the workflow is in `task_closure_pending`, a valid task-scope review-dispatch record exists, and the caller supplied `--external-review-result-ready` to indicate the external review result is already available for task-closure recording |
| `branch_closure_recording_required_for_release_readiness` | the workflow is in `document_release_pending`, no current reviewed branch closure exists, and the next required recording command is `record-branch-closure` |
| `release_readiness_recording_ready` | the workflow is in `document_release_pending` and release-readiness can be recorded against an existing current reviewed branch closure |
| `release_blocker_resolution_required` | the workflow is in `document_release_pending` and the latest release-readiness result for the current branch closure is `blocked` |
| `final_review_dispatch_required` | the workflow is in `final_review_pending`, the current reviewed branch closure and a current release-readiness result `ready` already exist for the same branch closure, and no valid final-review dispatch exists yet |
| `final_review_outcome_pending` | the workflow is in `final_review_pending`, the current reviewed branch closure and a current release-readiness result `ready` already exist for the same branch closure, a valid final-review dispatch exists, and workflow/operator is waiting for the external reviewer result before final-review recording can run |
| `final_review_recording_ready` | the workflow is in `final_review_pending`, a valid final-review dispatch exists, and the caller supplied `--external-review-result-ready` to indicate the independent reviewer result is already available for final-review recording |
| `qa_recording_required` | the workflow is in `qa_pending`, authoritative QA policy says QA is required for the current branch, and the current reviewed branch closure, current release-readiness result `ready`, and current final-review result `pass` all already exist for that same branch closure |
| `test_plan_refresh_required` | the workflow is in `qa_pending`, authoritative QA policy says QA is required, but the current test-plan artifact is missing, malformed, stale, or fails authoritative provenance/generator validation so QA must reroute through test-plan refresh before direct recording can continue |
| `finish_review_gate_ready` | late-stage milestones are satisfied and the next finish command is `gate-review` |
| `finish_completion_gate_ready` | `gate-review` already passed for the same current branch closure surfaced by workflow/operator, as proven by `finish_review_gate_pass_branch_closure_id`, and the next finish command is `gate-finish` |
| `handoff_recording_required` | a transfer to another owner or lane is the next safe action and has not been recorded yet |
| `planning_reentry_required` | the authoritative workflow state requires strategy or planning reentry before more execution or late-stage recording |

## Exceptional-Phase Trigger Rules

`handoff_required` must be emitted when at least one of these is true:

1. the current actionable task or lane is assigned to a different owner than the current operator context
2. a prior workflow decision or transfer checkpoint requires another owner before more local execution continues
3. the runtime determines the next safe action is a transfer rather than continued implementation, review, or late-stage recording

`pivot_required` must be emitted when at least one of these is true:

1. the approved plan/spec no longer matches the required implementation direction
2. an authoritative planning or review outcome requires strategy change before more execution continues
3. the runtime determines that continued local execution would proceed against an invalid or obsolete contract

## Exceptional Command Contracts

`featureforge plan execution transfer --plan <path> --scope <task|branch> --to <owner> --reason <reason>` must:

1. record a runtime-owned handoff decision/checkpoint for the addressed task or branch scope
2. return at least `action: recorded|already_current|blocked`, the addressed scope, and a machine-readable record id or trace summary
3. clear `follow_up_override=record_handoff` only when the recorded handoff satisfies the current authoritative workflow decision that required handoff
4. fail closed without clearing `follow_up_override` when invoked out of phase or when the requested handoff does not satisfy the current authoritative workflow decision

`featureforge workflow record-pivot --plan <path> --reason <reason>` must:

1. record a runtime-owned pivot decision/checkpoint for the current workstream
2. return at least `action: recorded|already_current|blocked` and a machine-readable record id or trace summary
3. clear `follow_up_override=record_pivot` only when the recorded pivot satisfies the current authoritative workflow decision that required planning reentry
4. fail closed without clearing `follow_up_override` when invoked out of phase or when the recorded pivot does not satisfy the current authoritative workflow decision

Determinism rule:

- `task_review_result_pending` is derived from dispatch state only. The runtime must not require a hidden staged reviewer-result object to expose this phase detail.
- `task_closure_recording_ready` is emitted only when the caller supplies `--external-review-result-ready` while the underlying dispatch state is otherwise `task_review_result_pending`.
- `final_review_outcome_pending` is derived from dispatch state only. The runtime must not require a hidden staged reviewer-result object to expose this phase detail.
- `final_review_recording_ready` is emitted only when the caller supplies `--external-review-result-ready` while the underlying dispatch state is otherwise `final_review_outcome_pending`.
- `final_review_pending` must not be emitted unless current branch closure and a current release-readiness result `ready` already exist for the same branch closure.
- `qa_pending` must not be emitted unless current branch closure, a current release-readiness result `ready`, and a current final-review result `pass` already exist for the same branch closure and authoritative QA policy says QA is required.
- if authoritative `qa_requirement` is missing or invalid when late-stage routing needs a QA decision, workflow/operator must route to `phase=pivot_required` with `phase_detail=planning_reentry_required`, `next_action=pivot / return to planning`, and `recommended_command=featureforge workflow record-pivot --plan <path> --reason <reason>`
- if review-state repair or late-stage drift leaves branch closure missing while `review_state_status=missing_current_closure` after release-readiness, final review, or QA previously existed, workflow/operator must reroute back to `phase=document_release_pending` with `phase_detail=branch_closure_recording_required_for_release_readiness`; it must not remain in `final_review_pending` or `qa_pending`
- failed task review or failed task verification route immediately to `phase=executing` with `phase_detail=execution_reentry_required` when the authoritative command result returned `required_follow_up=execution_reentry`
- failed final review or failed QA route immediately to `phase=executing` with `phase_detail=execution_reentry_required` when the authoritative command result returned `required_follow_up=execution_reentry`
- structural blocked returns with `required_follow_up=repair_review_state` route to `phase=executing` with `phase_detail=execution_reentry_required`; `recommended_command` becomes `featureforge plan execution repair-review-state --plan <path>`
- structural blocked returns with `required_follow_up=record_branch_closure` route to `phase=document_release_pending` with `phase_detail=branch_closure_recording_required_for_release_readiness`; `recommended_command` becomes `featureforge plan execution record-branch-closure --plan <path>`
- structural blocked returns with `required_follow_up=record_review_dispatch` route back to the scope-appropriate dispatch checkpoint: `phase=task_closure_pending` with `phase_detail=task_review_dispatch_required` for task scope, or `phase=final_review_pending` with `phase_detail=final_review_dispatch_required` for final-review scope
- recorded release-readiness results with `required_follow_up=resolve_release_blocker` route to `phase=document_release_pending` with `phase_detail=release_blocker_resolution_required`; `recommended_command` remains `featureforge plan execution advance-late-stage --plan <path> --result ready|blocked --summary-file <path>`
- failed task review, failed task verification, failed final review, or failed QA route immediately to `phase=handoff_required` with `phase_detail=handoff_recording_required` when the authoritative command result returned `required_follow_up=record_handoff`
- failed task review, failed task verification, failed final review, or failed QA route immediately to `phase=pivot_required` with `phase_detail=planning_reentry_required` when the authoritative command result returned `required_follow_up=record_pivot`
- stale release-readiness, stale final review, or stale QA caused by later repo-tracked edits routes first to `phase=executing` with `phase_detail=execution_reentry_required` and `review_state_status=stale_unreviewed` so `repair-review-state` can determine whether the exact next safe step is true execution reentry or a reroute back to `document_release_pending` for branch-closure recording
- stale task-scope review state before closure routes first to `phase=executing` with `phase_detail=execution_reentry_required` and `review_state_status=stale_unreviewed`; once execution stabilizes again, workflow/operator must return to `phase=task_closure_pending` with `phase_detail=task_review_dispatch_required` so a fresh task review is dispatched before closure

## Selected Approach

The public contract should make current reviewed closure truth authoritative.

That means:

- if the workspace moved past the current reviewed state, the public contract should say repair/review-state reentry is required
- if earlier closures are merely superseded by later reviewed work, that should not be reported like an error
- if final review is pending but current reviewed branch state is stale, the operator should not have to infer that they really need execution reentry
- workflow/operator code should translate the public review-state query model rather than duplicating closure or milestone logic

## Example Routing Matrix

| phase | phase_detail | review_state_status | expected interpretation | next_action shape | recommended_command |
| --- | --- | --- | --- | --- | --- |
| `executing` | `execution_in_progress` | `clean` | normal execution continues | continue execution | exact `begin`, `complete`, or `reopen` command derived by `ExecutionCommandResolver` from `execution_command_context` |
| `executing` | `execution_reentry_required` | `clean` | task review, task verification, final review, or QA failed and execution work must resume before late-stage progression can continue | execution reentry required | exact `begin`, `complete`, or `reopen` command derived by `ExecutionCommandResolver` from `execution_command_context` |
| `executing` | `execution_reentry_required` | `stale_unreviewed` | current reviewed branch or task state went stale after task review, final review, release-readiness, or QA because repo-tracked work moved forward | repair review state / reenter execution | `featureforge plan execution repair-review-state --plan <path>` |
| `task_closure_pending` | `task_review_dispatch_required` | `clean` | task closure flow has started but review has not been dispatched yet | dispatch review | `featureforge plan execution record-review-dispatch --plan <path> --scope task --task <n>` |
| `task_closure_pending` | `task_review_result_pending` | `clean` | task review dispatch exists and workflow/operator is waiting for the external review result before closure can be recorded | wait for external review result | omitted until reviewer result is available |
| `task_closure_pending` | `task_closure_recording_ready` | `clean` | the caller has the external task-review result in hand and task closure can now be recorded against the existing dispatch lineage | close current task | `featureforge plan execution close-current-task --plan <path> --task <n> --dispatch-id <id> --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]` |
| `document_release_pending` | `branch_closure_recording_required_for_release_readiness` | `missing_current_closure` | release-readiness is next, but current reviewed branch closure must be recorded first | record branch closure | `featureforge plan execution record-branch-closure --plan <path>` |
| `document_release_pending` | `release_readiness_recording_ready` | `clean` | release-readiness can be recorded now | advance late stage | `featureforge plan execution advance-late-stage --plan <path> --result ready|blocked --summary-file <path>` |
| `document_release_pending` | `release_blocker_resolution_required` | `clean` | a blocked release-readiness result exists for the current branch closure and must be resolved before progression; once resolved, the same release-readiness command is rerun against that branch closure | resolve release blocker | `featureforge plan execution advance-late-stage --plan <path> --result ready|blocked --summary-file <path>` |
| `final_review_pending` | `final_review_dispatch_required` | `clean` | branch is ready to request final independent review | dispatch final review | `featureforge plan execution record-review-dispatch --plan <path> --scope final-review` |
| `final_review_pending` | `final_review_outcome_pending` | `clean` | final-review dispatch exists and workflow/operator is waiting for the external reviewer result before final-review recording can run | wait for external review result | omitted until reviewer result is available |
| `final_review_pending` | `final_review_recording_ready` | `clean` | the caller has the independent reviewer result in hand and final-review recording can now run against the existing dispatch lineage | advance late stage | `featureforge plan execution advance-late-stage --plan <path> --dispatch-id <id> --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>` |
| `qa_pending` | `qa_recording_required` | `clean` | QA evidence must be recorded only after current branch closure, current release-readiness result `ready`, and current final-review result `pass` all exist for that same branch closure | run QA | `featureforge plan execution record-qa --plan <path> --result pass|fail --summary-file <path>` |
| `qa_pending` | `test_plan_refresh_required` | `clean` | QA policy requires QA, but the current authoritative test-plan artifact must be refreshed before direct QA recording can continue | refresh test plan | omitted until the refreshed plan is recorded through the plan-eng-review lane |
| `pivot_required` | `planning_reentry_required` | `clean` | approved-plan QA policy metadata is missing or invalid, so late-stage routing cannot safely decide between QA and branch completion | pivot / return to planning | `featureforge workflow record-pivot --plan <path> --reason <reason>` |
| `ready_for_branch_completion` | `finish_review_gate_ready` | `clean` | current state is healthy and the next finish command is `gate-review` | run finish review gate | `featureforge plan execution gate-review --plan <path>` |
| `ready_for_branch_completion` | `finish_completion_gate_ready` | `clean` | `gate-review` already passed for the same current branch closure and the next finish command is `gate-finish` | run finish completion gate | `featureforge plan execution gate-finish --plan <path>` |
| `handoff_required` | `handoff_recording_required` | `clean` | review state is usable but ownership or lane must change | hand off | `featureforge plan execution transfer --plan <path> --scope <task|branch> --to <owner> --reason <reason>` |
| `pivot_required` | `planning_reentry_required` | `clean` | review state may be fine, but the workstream cannot proceed without replanning | pivot / return to planning | `featureforge workflow record-pivot --plan <path> --reason <reason>` |

## Acceptance Criteria

1. Public workflow output can distinguish stale review-state repair from normal late-stage waiting.
2. Operators are not told to re-review or repair merely because an earlier closure is historical and superseded.
3. Routing outputs present one coherent next-action contract.
4. Generated docs and contract tests use the same phase names and meanings as runtime.
5. Late-stage sequencing remains clear under the reviewed-branch-closure model.
6. Workflow routing remains testable from public review-state fixtures instead of oversized execution-state construction.
7. Workflow/operator tells the agent the exact preferred command string or command template to run next.

## Test Strategy

- add routing tests where `review_state_status=stale_unreviewed` blocks otherwise normal phases
- add routing tests showing superseded closures do not trigger false repair phases
- add routing tests for late-stage precedence after release-readiness and final review milestones
- add routing tests for `handoff_required` and `pivot_required` trigger semantics
- add contract tests asserting one routing recommendation contract only
- add routing tests for `recommended_command` values

## Risks

- keeping stale review-state hidden behind generic late-stage phases will preserve operator confusion
- adding new phase names without updating generated docs and tests will recreate the current drift
