# Runtime Remediation Regression Inventory

This fixture index tracks the single-shot runtime-remediation regression scenarios at
scenario/file granularity. It is intentionally a compact coverage map, not a
second audit report or function-level traceability matrix.

## Scenario Coverage Matrix

| Scenario | Regression Class | Expected Contract | Primary Coverage Surfaces |
|---|---|---|---|
| `FS-01` | Contradictory late-stage reroute | Operator, status, doctor, branch-closure mutation, and repair agree on one route. | `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-02` | Late-stage doc/evidence churn loop | Late-stage drift is classified deterministically without re-staling execution. | `tests/workflow_runtime_final_review.rs`, `tests/workflow_entry_shell_smoke.rs` |
| `FS-03` | Prior-task redispatch target mismatch | Blocking target and accepted mutation target come from the same shared decision. | `tests/workflow_runtime.rs`, `tests/plan_execution.rs`, `tests/internal_plan_execution.rs` |
| `FS-04` | Repair/rebuild leaves wrong blocker | Repair returns the authoritative post-repair blocker and legal next command. | `tests/workflow_runtime.rs`, `tests/plan_execution.rs`, `tests/contracts_execution_runtime_boundaries.rs` |
| `FS-05` | Unsupported field mutates before rejection | Invalid input fails before authoritative state changes. | `tests/plan_execution.rs`, `tests/contracts_execution_runtime_boundaries.rs` |
| `FS-06` | Helper/direct path masks shipped CLI behavior | Compiled CLI remains the public oracle; internal compatibility stays quarantined. | `tests/public_cli_flow_contracts.rs`, `tests/internal_workflow_shell_smoke.rs` |
| `FS-07` | Status truthful, operator stale | Status and operator share the same route decision fields. | `tests/execution_query.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-08` | Resume overlay hides stale prerequisite | Earliest stale prior-task closure remains visible and wins. | `tests/workflow_runtime.rs`, `tests/contracts_execution_runtime_boundaries.rs` |
| `FS-09` | Repair clears one layer but hides next blocker | Repair exposes the next blocker immediately after cleanup. | `tests/workflow_runtime.rs`, `tests/workflow_entry_shell_smoke.rs` |
| `FS-10` | Stale follow-up overrides live truth | Live current truth wins over stale persisted follow-up. | `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-11` | Later begin route rejected by earlier blocker | Operator, repair, and begin target the same blocker or all agree begin is legal. | `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs`, `tests/public_replay_churn.rs` |
| `FS-12` | Authoritative run exists without current preflight | Begin, operator, and close-current-task use authoritative run identity without hidden preflight. | `tests/public_replay_churn.rs`, `tests/internal_workflow_runtime.rs`, `tests/internal_plan_execution.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-13` | Later parked interruption masks earlier stale boundary | Earliest unresolved stale boundary wins; no manual note edit is required. | `tests/public_replay_churn.rs`, `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-14` | Missing current closure baseline routes to replay | Routing surfaces `task_closure_recording_ready`; `close-current-task` refreshes closure/projections. | `tests/public_replay_churn.rs`, `tests/internal_workflow_runtime.rs`, `tests/internal_plan_execution.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-15` | False later reopen target | Earliest unresolved stale boundary is selected every time. | `tests/public_replay_churn.rs`, `tests/workflow_runtime.rs`, `tests/internal_contracts_execution_runtime_boundaries.rs` |
| `FS-16` | Receipt/projection drift blocks next begin | Current positive task closure remains begin-time authority. | `tests/public_replay_churn.rs`, `tests/internal_plan_execution.rs` |
| `FS-17` | Truthful replay does not converge to closure recording | Replay converges through `task_closure_recording_ready` and `close-current-task`. | `tests/public_replay_churn.rs`, `tests/workflow_runtime.rs`, `tests/internal_plan_execution.rs` |
| `FS-18` | Cycle-break remains globally sticky | Cycle-break is task-scoped and clears after the bound task is truthfully reclosed. | `tests/workflow_runtime.rs`, `tests/internal_plan_execution.rs` |
| `FS-19` | Superseded stale history keeps routing | Superseded stale history is ignored for unresolved-stale targeting. | `tests/workflow_runtime.rs`, `tests/contracts_execution_runtime_boundaries.rs` |
| `FS-20` | Runtime-owned plan/evidence churn unwinds closure chain | Upstream task closure and late-stage chain stay current when filtered drift is empty. | `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-21` | Resume advisory preempts earlier closure bridge | Resume hints are hidden when the legal next route is earlier `close-current-task`. | `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs` |
| `FS-22` | Repair destructively clears lineage before bridge | Repair stays bridge-first and non-destructive when a closure bridge exists. | `tests/public_replay_churn.rs`, `tests/workflow_runtime.rs`, `tests/internal_plan_execution.rs` |

## Coverage Map

This map stays at scenario/file granularity on purpose. Individual test function
names live in the tests themselves so ordinary test renames do not churn this
inventory.

| Coverage Surface | Scenarios |
|---|---|
| `tests/workflow_runtime.rs` | `FS-01`, `FS-03`, `FS-07`, `FS-08`, `FS-09`, `FS-10`, `FS-11`, `FS-13`, `FS-15`, `FS-17`, `FS-18`, `FS-19`, `FS-20`, `FS-21`, `FS-22` |
| `tests/workflow_runtime_final_review.rs` | `FS-02` |
| `tests/workflow_shell_smoke.rs` | `FS-01`, `FS-07`, `FS-10`, `FS-11`, `FS-12`, `FS-13`, `FS-14`, `FS-20`, `FS-21`, public workflow-command mapping |
| `tests/workflow_entry_shell_smoke.rs` | `FS-02`, `FS-09` |
| `tests/plan_execution.rs` and `tests/internal_plan_execution.rs` | `FS-03`, `FS-04`, `FS-05`, `FS-12`, `FS-13`, `FS-14`, `FS-16`, `FS-17`, `FS-18`, `FS-22` |
| `tests/contracts_execution_runtime_boundaries.rs` and `tests/internal_contracts_execution_runtime_boundaries.rs` | `FS-03`, `FS-04`, `FS-05`, `FS-08`, `FS-13`, `FS-15`, `FS-19` |
| `tests/execution_query.rs` | `FS-07` |
| `tests/public_replay_churn.rs` | Public compiled-CLI replay coverage for `FS-11`, `FS-12`, `FS-13`, `FS-14`, `FS-15`, `FS-16`, `FS-17`, `FS-22`; current churn replay coverage for projection loss/current closure, recommended-route loop detection, stale/superseded dispatch lineage, explicit projection materialization, targetless stale reconcile diagnostics, and cycle-break cleanup |
| `tests/public_cli_flow_contracts.rs` | Public-flow scanner coverage for `FS-06` helper quarantine, hidden-helper leakage, and compiled-CLI boundary drift |
| `tests/internal_workflow_runtime.rs` and `tests/internal_workflow_shell_smoke.rs` | Internal compatibility coverage for `FS-03`, `FS-04`, `FS-06`, `FS-08`, `FS-12`, `FS-13`, `FS-14` |
| `tests/bootstrap_smoke.rs` | Public workflow-command mapping |
| `tests/runtime_instruction_plan_review_contracts.rs`, `tests/runtime_instruction_contracts.rs`, `tests/using_featureforge_skill.rs`, `tests/codex-runtime/*.test.mjs` | Runtime-doc and skill-contract integration |

### Command-Budget Coverage

Compiled public-flow coverage keeps explicit runtime-management command budgets
for `FS-11`, `FS-17`, `FS-20`, task-close happy/internal-dispatch paths, and
stale release-refresh progression. The exact test function names are
intentionally not part of this fixture index.
