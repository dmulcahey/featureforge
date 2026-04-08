# Late-Stage Precedence Reference

This reference is runtime-owned and must stay aligned with:

- `src/workflow/late_stage_precedence.rs` (`PRECEDENCE_ROWS`)
- `src/workflow/operator.rs` (phase -> next action -> recommended skill routing)

Use this table when skill/docs wording needs an explicit late-stage routing source.

| Release Gate | Review Gate | QA Gate | Phase | Next Action | Recommended Skill | Reason Family |
| --- | --- | --- | --- | --- | --- | --- |
| blocked | blocked | blocked | `document_release_pending` | `advance_late_stage` | `featureforge:document-release` | `release_readiness` |
| blocked | blocked | ready | `document_release_pending` | `advance_late_stage` | `featureforge:document-release` | `release_readiness` |
| blocked | ready | blocked | `document_release_pending` | `advance_late_stage` | `featureforge:document-release` | `release_readiness` |
| blocked | ready | ready | `document_release_pending` | `advance_late_stage` | `featureforge:document-release` | `release_readiness` |
| ready | blocked | blocked | `final_review_pending` | `dispatch_final_review` | `featureforge:requesting-code-review` | `final_review_freshness` |
| ready | blocked | ready | `final_review_pending` | `dispatch_final_review` | `featureforge:requesting-code-review` | `final_review_freshness` |
| ready | ready | blocked | `qa_pending` | `run_qa` | `featureforge:qa-only` | `qa_freshness` |
| ready | ready | ready | `ready_for_branch_completion` | `run_finish_review_gate` | `featureforge:finishing-a-development-branch` | `all_fresh` |

## Command-Boundary Semantics

- `gate-review` is the first finish gate and may record or refresh the current branch-closure checkpoint.
- `record-review-dispatch` is the dispatch-proof minting boundary.
- For workflow-routed terminal sequencing, run `document-release` before terminal `requesting-code-review`.
- `requesting-code-review` also supports non-terminal checkpoint/task-boundary reviews when runtime reason codes require it (for example `prior_task_review_*`).

## Notes

- `review_state_status=missing_current_closure` preempts the normal late-stage table and reroutes back to `document_release_pending` with `record branch closure`; late-stage work must not remain in `final_review_pending` or `qa_pending` once the current branch closure is gone.
- `review_state_status=stale_unreviewed` preempts the normal late-stage table and reroutes first to `executing` with `repair review state / reenter execution` so runtime-owned repair can re-establish current reviewed truth before any more late-stage recording.
- `qa_pending` routing can be preempted by helper-owned test-plan refresh requirements (`featureforge:plan-eng-review`) when workflow/operator sees gate-finish context for stale, missing, malformed, or provenance-invalid current-branch test-plan artifacts; the user sees this directly as `phase_detail=test_plan_refresh_required` before invoking `gate-finish`.
- If runtime guards detect malformed or unknown late-stage inputs, helper outputs fail closed.
