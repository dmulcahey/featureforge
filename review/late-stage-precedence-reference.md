# Late-Stage Precedence Reference

Runtime owns late-stage precedence. Do not maintain a second phase matrix in
this markdown file.

Use these runtime-owned surfaces instead:

- `src/execution/late_stage_precedence.rs` owns the `PRECEDENCE_ROWS` data used
  by execution status assembly and workflow status/operator presentation.
- `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`
  owns the current public phase, next action, recommended skill, typed argv, and
  template route.
- `$_FEATUREFORGE_ROOT/references/operator-route-authority.md` owns detailed
  argv/template binding and route-specific stop rules.

## Command-Boundary Semantics

- Legacy finish-gate compatibility commands are compatibility/debug boundaries,
  not normal-path commands.
- Low-level `record-*` commands are compatibility/debug boundaries and must not
  be required by normal-path guidance.
- When workflow/operator selects a terminal late-stage lane, execute that selected
  typed route or selected handoff lane. Do not use this reference to run a
  memorized chain.
- `requesting-code-review` also supports non-terminal checkpoint/task-boundary
  reviews when runtime reason codes require it, for example
  `prior_task_review_*`.
- Do not infer branch closure, release readiness, final review, QA, or finish
  progression from companion markdown artifacts. Query workflow/operator again
  after each external result or runtime-owned recording command.

## Preemption Notes

- Missing or stale current reviewed truth can preempt the visible late-stage
  lane and route back to runtime-owned repair or branch-closure refresh.
- Missing `Late-Stage Surface` metadata means branch reroute cannot be trusted
  as late-stage-only; runtime must fail closed and surface the blocker.
- Test-plan projection drift is diagnostic-only for QA recording and branch
  completion once runtime-owned branch closure, final-review, and QA record
  truth are current. Runtime-owned corruption, unsafe bindings, or non-regular
  authoritative artifacts still fail closed.
