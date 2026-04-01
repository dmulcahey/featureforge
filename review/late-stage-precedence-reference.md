# Late-Stage Precedence Reference

This reference is runtime-owned and must stay aligned with:

- `src/workflow/late_stage_precedence.rs` (`PRECEDENCE_ROWS`)
- `src/workflow/operator.rs` (phase -> next action -> recommended skill routing)

Use this table when skill/docs wording needs an explicit late-stage routing source.

| Release Gate | Review Gate | QA Gate | Phase | Next Action | Recommended Skill | Reason Family |
| --- | --- | --- | --- | --- | --- | --- |
| blocked | blocked | blocked | `document_release_pending` | `run_document_release` | `featureforge:document-release` | `release_readiness` |
| blocked | blocked | ready | `document_release_pending` | `run_document_release` | `featureforge:document-release` | `release_readiness` |
| blocked | ready | blocked | `document_release_pending` | `run_document_release` | `featureforge:document-release` | `release_readiness` |
| blocked | ready | ready | `document_release_pending` | `run_document_release` | `featureforge:document-release` | `release_readiness` |
| ready | blocked | blocked | `final_review_pending` | `request_code_review` | `featureforge:requesting-code-review` | `final_review_freshness` |
| ready | blocked | ready | `final_review_pending` | `request_code_review` | `featureforge:requesting-code-review` | `final_review_freshness` |
| ready | ready | blocked | `qa_pending` | `run_qa_only` | `featureforge:qa-only` | `qa_freshness` |
| ready | ready | ready | `ready_for_branch_completion` | `finish_branch` | `featureforge:finishing-a-development-branch` | `all_fresh` |

## Command-Boundary Semantics

- `gate-review` is read-only state evaluation.
- `gate-review-dispatch` is the dispatch-proof minting boundary.
- For workflow-routed terminal sequencing, run `document-release` before terminal `requesting-code-review`.
- `requesting-code-review` also supports non-terminal checkpoint/task-boundary reviews when runtime reason codes require it (for example `prior_task_review_*`).

## Notes

- `qa_pending` routing can be preempted by helper-owned test-plan refresh requirements (`featureforge:plan-eng-review`) when `gate-finish` reports stale or missing current-branch test-plan artifacts.
- If runtime guards detect malformed or unknown late-stage inputs, helper outputs fail closed.
