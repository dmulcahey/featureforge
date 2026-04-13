# Late-Stage Precedence Reference

This reference is runtime-owned and must stay aligned with:

- `src/workflow/late_stage_precedence.rs` (`PRECEDENCE_ROWS`)
- `src/workflow/operator.rs` (phase -> next action -> recommended skill routing)

Use this table when skill/docs wording needs an explicit late-stage routing source.

| Release Gate | Review Gate | QA Gate | Phase | Next Action (Public Contract) | Recommended Skill | Reason Family |
| --- | --- | --- | --- | --- | --- | --- |
| blocked | blocked | blocked | `document_release_pending` | derived from phase_detail: advance late stage (branch-closure refresh lane); resolve release blocker | `featureforge:document-release` | `release_readiness` |
| blocked | blocked | ready | `document_release_pending` | derived from phase_detail: advance late stage (branch-closure refresh lane); resolve release blocker | `featureforge:document-release` | `release_readiness` |
| blocked | ready | blocked | `document_release_pending` | derived from phase_detail: advance late stage (branch-closure refresh lane); resolve release blocker | `featureforge:document-release` | `release_readiness` |
| blocked | ready | ready | `document_release_pending` | derived from phase_detail: advance late stage (branch-closure refresh lane); resolve release blocker | `featureforge:document-release` | `release_readiness` |
| ready | blocked | blocked | `final_review_pending` | derived from phase_detail: request final review; wait for external review result; advance late stage | `featureforge:requesting-code-review` | `final_review_freshness` |
| ready | blocked | ready | `final_review_pending` | derived from phase_detail: request final review; wait for external review result; advance late stage | `featureforge:requesting-code-review` | `final_review_freshness` |
| ready | ready | blocked | `qa_pending` | derived from phase_detail: run QA; refresh test plan | `featureforge:qa-only` | `qa_freshness` |
| ready | ready | ready | `ready_for_branch_completion` | derived from phase_detail: finish branch | `featureforge:finishing-a-development-branch` | `all_fresh` |

## Command-Boundary Semantics

- `gate-review` and `gate-finish` are compatibility/debug boundaries, not normal-path commands.
- low-level `record-*` commands are compatibility/debug boundaries and must not be required by normal-path guidance.
- For workflow-routed terminal sequencing, run `document-release` before terminal `requesting-code-review`.
- `requesting-code-review` also supports non-terminal checkpoint/task-boundary reviews when runtime reason codes require it (for example `prior_task_review_*`).
- `document_release_pending` keeps two distinct public next actions by `phase_detail`: `advance late stage` and `resolve release blocker`.
- `final_review_pending` keeps three distinct public next actions by `phase_detail`: `request final review`, `wait for external review result`, and `advance late stage`.
- `qa_pending` keeps two distinct public next actions by `phase_detail`: `run QA` and `refresh test plan`.
- `ready_for_branch_completion` keeps one public next action: `finish branch`.

## Notes

- `review_state_status=missing_current_closure` preempts the normal late-stage table and reroutes back to `document_release_pending` with the branch-closure refresh lane of `advance late stage`; late-stage work must not remain in `final_review_pending` or `qa_pending` once the current branch closure is gone.
- `review_state_status=stale_unreviewed` preempts the normal late-stage table and reroutes first to `executing` with `repair review state / reenter execution` so runtime-owned repair can re-establish current reviewed truth before any more late-stage recording.
- If the approved plan does not declare `Late-Stage Surface` metadata, branch reroute cannot be classified as trusted late-stage-only; runtime must fail closed to execution reentry and surface that blocker explicitly.
- `qa_pending` routing can be preempted by helper-owned test-plan refresh requirements (`featureforge:plan-eng-review`) when workflow/operator sees gate-finish context for stale, missing, malformed, or provenance-invalid current-branch test-plan artifacts; the user sees this directly as `phase_detail=test_plan_refresh_required` before invoking `gate-finish`.
- If runtime guards detect malformed or unknown late-stage inputs, helper outputs fail closed.
