# Operator Route Authority

Workflow/operator JSON is the normal-path execution authority for task closure,
review-state repair, and late-stage progression.

Use this rule before executing any routed FeatureForge command:

1. Treat `phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`, `recommended_public_command_template`, `required_inputs`, and `recording_context` as the public route contract.
2. Treat `recommended_command` as display-only compatibility text. Do not parse, split, or execute it.
3. If `recommended_public_command_argv` is present and non-empty, execute that argv vector exactly. The public runtime emits `recommended_public_command_argv[0] == "featureforge"` for executable routes; run it through `$_FEATUREFORGE_BIN` or `_featureforge_exec_public_argv`. Generated shell blocks may provide `_featureforge_exec_public_argv`; that wrapper rebinds argv[0] `featureforge` to `$_FEATUREFORGE_BIN` and fails closed for any other argv[0].
4. If argv is absent and `recommended_public_command_template` is present, bind concrete values by rerunning the same operator query with `workflow operator --plan <approved-plan-path> --input NAME=VALUE --json`; execute only the returned Rust-materialized `recommended_public_command_argv`.
5. If neither executable argv nor a bindable template is present, stop and report the route diagnostic. Do not treat `next_action` alone as executable routing authority.
6. Treat `resume_task` and `resume_step` from `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` as advisory diagnostics. If they disagree with workflow/operator typed argv/template, follow workflow/operator.
7. If execution-start tracking must be recovered after preflight, reconcile or isolate the workspace, rerun workflow/operator JSON, and follow only the typed public argv/template from that operator route before any recovery mutation. If no public argv/template is present, stop and report the route diagnostic. Backfill only factual-only completed steps through public runtime routes returned by workflow/operator; never infer completion from dirty diffs or memory. Resume from the task-boundary review and verification gate before any next-task `begin`.

Reviewed-closure guardrails:

- `task_closure_recording_ready` requires `recording_context.task_number`.
- `release_readiness_recording_ready` and `release_blocker_resolution_required` require `recording_context.branch_closure_id`.
- `final_review_recording_ready` requires `recording_context.branch_closure_id`.
- When `phase_detail=task_closure_recording_ready`, replay is already complete enough for closure refresh; run `close-current-task` and do not reopen the same step again.
- When workflow/operator JSON reports stale or missing closure context, do not invent a repair command. Follow typed argv/template when present; stop and report the route diagnostic when no executable surface exists.
- After `repair-review-state`, follow that command's returned `recommended_public_command_argv` when present before any additional recording command. If argv is absent and a template is present, rerun the same plan-bound workflow/operator query with `--input NAME=VALUE` so Rust materializes argv. If neither exists, stop and report the route diagnostic.
- Keep compatibility/debug-only runtime primitives out of the normal path.
- Hidden compatibility/debug command entrypoints are removed from the public CLI; normal routing must use public commands only.
- In `*_dispatch_required` lanes, request the review and keep rerouting through workflow/operator; do not expand the normal path into low-level dispatch-lineage management.
- Do not manually edit runtime-owned execution records, derived markdown projection artifacts, or `**Execution Note:**` lines to recover runtime state.
- Do not repair runtime progress by editing tracked plan, evidence, review, readiness, QA, or strategy projection files.
- Do not use the internal task-closure recording service boundary directly. Use `close-current-task`.

Late-stage aggregate route coverage:

- Workflow/operator may return `recommended_public_command_argv` or `recommended_public_command_template` with intent `advance_late_stage`; execute only returned argv, using `workflow operator --plan <approved-plan-path> --input NAME=VALUE --json` when a template requires inputs.
- Release-readiness routes bind `result` (`ready` or `blocked`) and `summary_file`.
- Final-review routes bind `reviewer_source`, `reviewer_id`, `result` (`pass` or `fail`), and `summary_file`.
- QA routes bind `result` (`pass` or `fail`) and `summary_file`; do not copy literal command shapes from memory.
- If no typed public argv/template is present, stop and report the route diagnostic instead of looking for low-level runtime primitives.

## Final-Review Recording Route Materializer

Use this route only after a dedicated reviewer has returned a concrete final-review result. Bind concrete values through workflow/operator so Rust-owned template validation and materialization produce the executable argv.

```bash
: "${REVIEWER_SOURCE:?Set REVIEWER_SOURCE to the independent final-review reviewer source before recording.}"
: "${REVIEWER_ID:?Set REVIEWER_ID to the independent final-review reviewer id before recording.}"
: "${REVIEW_RESULT:?Set REVIEW_RESULT=pass|fail from the independent final-review result before recording.}"
: "${SUMMARY_FILE:?Set SUMMARY_FILE to the final-review summary artifact before recording.}"
RECORDING_READY_JSON=$("$_FEATUREFORGE_BIN" workflow operator \
  --plan "$APPROVED_PLAN_PATH" \
  --external-review-result-ready \
  --input "reviewer_source=$REVIEWER_SOURCE" \
  --input "reviewer_id=$REVIEWER_ID" \
  --input "result=$REVIEW_RESULT" \
  --input "summary_file=$SUMMARY_FILE" \
  --json)
```

After this query, execute only the returned `recommended_public_command_argv` exactly. If argv[0] is `featureforge`, run it through `$_FEATUREFORGE_BIN`; generated shell blocks may use `_featureforge_exec_public_argv` when that wrapper is already available. If workflow/operator still returns `recommended_public_command_template` or does not return executable argv, stop and report `RECORDING_READY_JSON`.
