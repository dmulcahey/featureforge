# Runtime Remediation Regression Inventory

This fixture index tracks the single-shot runtime-remediation regression scenarios.

Each scenario is represented by at least one compiled-CLI coverage test and, where useful,
an additional lower-level runtime/shared-truth test.

| Scenario | Source | Setup Summary | Expected Fixed Behavior | Probe Command Target |
|---|---|---|---|---|
| `FS-01` | session `03e9` | late-stage missing-current-closure with drift classification pressure | one consistent route across `operator`/`status`/`doctor`; no `repair already_current` contradiction | covered by prerelease-refresh stale-follow-up regressions; parity-probe budget `<=3` |
| `FS-02` | session `03e9` | late-stage doc/evidence writes around branch-closure baseline | deterministic classification: confined refresh vs true execution reentry vs explicit metadata blocker | parity regression and command-budget guard in compiled-CLI entry routing |
| `FS-03` | session `a196` | stale prior-task redispatch while later task is active | blocking task target and accepted mutation target match | `<=2` |
| `FS-04` | session `a196` | repair/rebuild path with stale prior dispatch and resume overlays | repair yields one authoritative next action; no wrong blocker survives | `<=2` |
| `FS-05` | session `a196` | unsupported field request on mutation commands | fail before mutation; authoritative digest unchanged | `<=1` |
| `FS-06` | session `a196` | helper/direct path compared to compiled CLI path | compiled CLI remains contract oracle; helper parity enforced | helper-vs-compiled-cli target-mismatch parity lock |
| `FS-07` | session `b83b` | status reports dispatch-required while operator advertises begin/reentry | all surfaces share same routing decision fields | covered by task-boundary dispatch-blocked routing regressions; parity-probe budget `<=3` |
| `FS-08` | session `b83b` | resume overlays plus stale prior-task closure | stale prerequisite remains visible; resume does not hide blocker | `<=1` |
| `FS-09` | session `b83b` | repair clears one stale condition and should expose next blocker | repair returns post-repair blocker directly | `<=2` |
| `FS-10` | PR `#34` bug class | stale persisted follow-up conflicts with live current truth | stale follow-up ignored/cleared; live closure truth wins | `<=1` |
| `FS-11` | late-stage precedence | release-facing drift after review or direct review-first attempt | document release precedence enforced before final review, then QA | `<=3` |
| `FS-12` | cross-session diagnosis | derived receipt/projection missing with authoritative records intact | normal closure/late-stage routing remains valid; projection regenerates without truth mutation | `<=2` |

## Coverage Map

- `FS-01`: `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs`
- `FS-02`: `tests/workflow_runtime_final_review.rs`, `tests/workflow_entry_shell_smoke.rs`
- `FS-03`: `tests/workflow_runtime.rs`, `tests/plan_execution.rs`
- `FS-04`: `tests/workflow_runtime.rs`, `tests/plan_execution.rs`
- `FS-05`: `tests/plan_execution.rs`, `tests/contracts_execution_runtime_boundaries.rs`
- `FS-06`: `tests/workflow_shell_smoke.rs`
- `FS-07`: `tests/workflow_runtime.rs`, `tests/workflow_shell_smoke.rs`
- `FS-08`: `tests/workflow_runtime.rs`, `tests/plan_execution.rs`
- `FS-09`: `tests/workflow_runtime.rs`, `tests/workflow_entry_shell_smoke.rs`
- `FS-10`: `tests/plan_execution.rs`, `tests/workflow_shell_smoke.rs`
- `FS-11`: `tests/workflow_runtime_final_review.rs`, `tests/plan_execution_final_review.rs`
- `FS-12`: `tests/plan_execution.rs`, `tests/plan_execution_final_review.rs`
- Task 12 command-budget gates (compiled CLI):
  - `task_close_happy_path_runtime_management_budget_is_capped` (`tests/workflow_shell_smoke.rs`) `<=3`
  - `reentry_recovery_runtime_management_budget_is_capped` (`tests/workflow_shell_smoke.rs`) `<=2`
  - `stale_release_refresh_runtime_management_budget_is_capped_before_new_review_step` (`tests/workflow_shell_smoke.rs`) `<=3`
- runtime-doc/skill-contract integration references:
  - `tests/runtime_instruction_contracts.rs`
  - `tests/using_featureforge_skill.rs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `tests/codex-runtime/workflow-fixtures.test.mjs`
