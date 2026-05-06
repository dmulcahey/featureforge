# Runtime Remediation Regression Inventory

This fixture index tracks the single-shot runtime-remediation regression scenarios.

Each scenario is represented by at least one compiled-CLI coverage test and, where useful,
an additional lower-level runtime/shared-truth test.

## Detailed Failure Shapes (Mandatory)

### FS-01 — `03e9` contradictory late-stage reroute

**Broken pattern:**

- `workflow operator` says branch closure / document release path
- branch-closure mutation says repair is required
- `repair-review-state` says `already_current`

**Expected fixed behavior:** one authoritative answer across operator, branch-closure mutation, and repair.

**Primary tests:**

- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`

### FS-02 — `03e9` Task 5 ↔ Task 6 late-stage loop caused by repo-owned plan / evidence changes

**Broken pattern:** late-stage writes re-stale execution and loop between late-stage refresh and execution reentry.

**Expected fixed behavior:** deterministic classification of late-stage drift versus true execution drift.

**Primary tests:**

- `tests/workflow_runtime_final_review.rs`
- `tests/workflow_entry_shell_smoke.rs`

### FS-03 — `a196` prior-task redispatch contradiction

**Broken pattern:** begin requires prior-task redispatch, but dispatch recorder rejects that target and only permits a different one.

**Expected fixed behavior:** one shared target and one accepted mutation path.

**Primary tests:**

- `tests/workflow_runtime.rs`
- `tests/plan_execution.rs`

### FS-04 — `a196` rebuild / repair mutated truth but still exposed the wrong blocker

**Broken pattern:** repair mutates state and still leaves the wrong route visible.

**Expected fixed behavior:** repair returns one authoritative blocker and the next command actually honors it.

**Primary tests:**

- `tests/workflow_runtime.rs`
- `tests/plan_execution.rs`
- `tests/contracts_execution_runtime_boundaries.rs`

### FS-05 — `a196` unsupported field mutates state before rejection

**Broken pattern:** unsupported-field CLI paths mutate authoritative state before returning an error.

**Expected fixed behavior:** invalid input fails before any mutation.

**Primary tests:**

- `tests/plan_execution.rs`
- `tests/contracts_execution_runtime_boundaries.rs`

### FS-06 — `a196` hidden-helper path masks shipped CLI behavior

**Broken pattern:** helper-backed tests pass but compiled CLI behavior differs; helper-backed tests passed even though the shipped CLI boundary behaved differently.

**Expected fixed behavior:** compiled CLI coverage is the public contract; hidden-helper behavior is quarantined in explicitly internal compatibility tests.

**Primary tests:**

- `tests/internal_workflow_shell_smoke.rs`

### FS-07 — `b83b` status truthful, operator stale

**Broken pattern:** status points to the right blocker, operator still recommends execution reentry / begin.

**Expected fixed behavior:** status and operator agree because they use the same next-action engine.

**Primary tests:**

- `tests/execution_query.rs`
- `tests/workflow_shell_smoke.rs`

### FS-08 — `b83b` resumed execution hides stale prior-task closure

**Broken pattern:** later resume overlays suppress the real stale prerequisite.

**Expected fixed behavior:** stale prior-task closure stays visible and wins.

**Primary tests:**

- `tests/workflow_runtime.rs`

### FS-09 — `b83b` repair clears stale closure, but next blocker stays hidden

**Broken pattern:** repair removes one stale layer but fails to surface the next blocker and still says begin.

**Expected fixed behavior:** repair immediately surfaces the next blocker after its own cleanup.

**Primary tests:**

- `tests/workflow_runtime.rs`
- `tests/workflow_entry_shell_smoke.rs`

### FS-10 — PR #34 stale follow-up overrides live truth

**Broken pattern:** persisted repair follow-up remains stuck on execution reentry or branch refresh after live truth is already current.

**Expected fixed behavior:** live truth wins and stale follow-up is ignored or cleared.

**Primary tests:**

- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`

### FS-11 — Operator recommends `Begin Task 3`, but `begin` rejects on older Task 2 blockers

**Broken pattern:**

- operator / status surfaces `begin --task 3 --step 6`
- actual `begin` rejects because Task 2 dispatch / receipt / run-id blockers still exist

**Fixture setup:**

- rebased consumer-style fixture with forward reentry overlay pointing at Task 3
- stale Task 2 boundary still present
- current code path reproduces the contradiction

**Expected fixed behavior:**

- operator, repair, and begin all surface the same Task 2 blocker or all agree that Task 3 is legal
- no contradiction remains

**Tests to add:**

- `tests/workflow_runtime.rs::runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine`
- `tests/workflow_shell_smoke.rs::fs11_operator_and_begin_target_parity_after_rebase_resume`

### FS-12 — Authoritative run exists, but preflight acceptance is missing or stale

**Broken pattern:**

- execution already exists in authoritative transition state
- preflight acceptance is missing or stale
- begin / close-current-task / operator still lose `execution_run_id`

**Fixture setup:**

- authoritative state contains `run_identity.execution_run_id`
- preflight acceptance missing or intentionally stale

**Expected fixed behavior:**

- operator, begin, and close-current-task still work from authoritative run identity
- hidden `preflight` is unnecessary

**Tests to add:**

- `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator`
- `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_preflight`
- `tests/workflow_shell_smoke.rs::fs12_recovery_path_does_not_require_hidden_preflight_when_run_identity_exists`

### FS-13 — Later parked interrupted note masks an earlier stale repair boundary

**Broken pattern:**

- Task 3 interrupted note remains parked
- Task 2 or Task 1 is the real earliest stale boundary
- runtime refuses the earlier reopen or keeps routing to the later interruption

**Fixture setup:**

- authoritative open-step state or legacy markdown note on a later task
- stale earlier task closure on an earlier task

**Expected fixed behavior:**

- earliest stale boundary wins
- no manual plan note edit required

**Tests to add:**

- `tests/workflow_runtime.rs::runtime_remediation_fs13_markdown_note_is_projection_not_authority`
- `tests/workflow_shell_smoke.rs::fs13_normal_recovery_never_requires_manual_plan_note_edit`

### FS-14 — Missing current task-closure baseline routes to `close-current-task`, not generic execution replay

**Broken pattern:**

- task execution is already complete on the current reviewed state
- only current closure / current-run receipt baseline is missing
- runtime still routes to generic execution reentry or hidden rebuild helpers

**Fixture setup:**

- completed task with no current task closure baseline
- valid review and verification inputs ready
- no earlier stale boundary

**Expected fixed behavior:**

- operator / repair surface `task_closure_recording_ready`
- `close-current-task` rebuilds the current closure baseline and its projections

**Tests to add:**

- `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry`
- `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch`
- `tests/workflow_shell_smoke.rs::fs14_recovery_to_close_current_task_uses_only_public_intent_commands`

### FS-15 — False later reopen target after earlier repair

**Broken pattern:**

- after fixing Task 1, operator falsely routes to Task 6 even though Task 2 is the true next repair target

**Fixture setup:**

- stale tasks 2 and 6 present after earlier repair cleanup
- current code reproduces later-target preference

**Expected fixed behavior:**

- earliest unresolved stale boundary is selected every time

**Tests to add:**

- `tests/workflow_runtime.rs::runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target`
- `tests/internal_contracts_execution_runtime_boundaries.rs::internal_only_compatibility_runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task`

### FS-16 — Current positive task closure allows next-task begin even if receipt projections later drift

**Broken pattern:**

- prior task already has a current positive closure
- begin still fails because run-scoped receipt files are missing / stale / regenerated later

**Fixture setup:**

- current positive task closure present
- remove or stale receipt projections without changing the reviewed state that closure binds to

**Expected fixed behavior:**

- next-task begin is still allowed
- receipt regeneration is a projection / closure-refresh concern, not a begin-time blocker

**Tests to add:**

- `tests/public_replay_churn.rs::public_replay_fs16_current_closure_allows_next_begin_after_projection_drift`
- `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts`

### FS-17 — Truthful replay must converge to closure recording

**Broken pattern:**

- task replay already completed truthfully
- stale-unreviewed boundary remains
- runtime falls back to generic execution reentry instead of closure recording

**Expected fixed behavior:**

- `phase_detail=task_closure_recording_ready`
- recommended command routes to `close-current-task --task 1`
- no second reopen for the same step

**Primary tests:**

- `tests/workflow_runtime.rs::fs17_stale_unreviewed_truthful_replay_promotes_to_task_closure_recording_ready`
- `tests/internal_plan_execution.rs::internal_only_compatibility_fs17_close_current_task_converges_after_truthful_replay_without_second_reopen`

### FS-18 — Cycle-break is task-scoped and clears after bound task reclose

**Broken pattern:**

- cycle-break stays effectively global
- downstream begin remains blocked after the bound task is truthfully reclosed

**Expected fixed behavior:**

- cycle-break binding is task-scoped
- bound cycle-break clears automatically after fresh current closure on that task
- next task begin unblocks without reopening the repaired task again

**Primary tests:**

- `tests/workflow_runtime.rs::fs18_cycle_break_binding_is_task_scoped_not_global`
- `tests/internal_plan_execution.rs::internal_only_compatibility_fs18_begin_unblocks_next_task_after_cycle_break_task_reclosed`

### FS-19 — Superseded stale history must stop routing

**Broken pattern:**

- stale historical task closure remains unresolved even after newer current closure supersedes it

**Expected fixed behavior:**

- superseded stale historical closure no longer participates in unresolved-stale targeting

**Primary tests:**

- `tests/workflow_runtime.rs::fs19_superseded_stale_historical_task_closure_is_not_an_unresolved_stale_boundary`
- `tests/contracts_execution_runtime_boundaries.rs::fs19_compiled_cli_ignores_superseded_stale_history_when_selecting_blocking_task`

### FS-20 — Runtime-owned plan/evidence churn must not unwind upstream closure truth or late-stage chain

**Broken pattern:**

- reopening downstream stale work mutates approved plan / execution-evidence artifacts
- upstream current task closure and late-stage chain get re-staled or nulled

**Expected fixed behavior:**

- upstream current task closure remains current
- branch closure remains current when filtered drift is empty
- late-stage chain is not unwound by runtime-owned plan/evidence-only churn

**Primary tests:**

- `tests/workflow_runtime.rs::fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_stale_current_task_closure`
- `tests/workflow_runtime.rs::fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_null_current_branch_closure`
- `tests/workflow_runtime.rs::fs20_branch_closure_remains_current_when_only_runtime_owned_plan_and_execution_evidence_paths_changed`
- `tests/workflow_shell_smoke.rs::fs20_reopening_downstream_stale_task_does_not_unwind_upstream_current_closure_when_only_plan_and_evidence_change`
- `tests/workflow_shell_smoke.rs::fs20_late_stage_chain_is_not_unwound_by_runtime_owned_plan_and_execution_evidence_churn`

### FS-21 — Resume advisory hints must be suppressed when preempted by earlier closure bridge

**Broken pattern:**

- `resume_task` / `resume_step` remain visible while earlier closure bridge is the legal next move

**Expected fixed behavior:**

- `resume_task` / `resume_step` are hidden when preempted
- status/operator/repair all agree on `close-current-task --task 1`

**Primary tests:**

- `tests/workflow_runtime.rs::fs21_operator_status_and_exact_command_all_agree_on_close_current_task_when_bridge_is_ready`
- `tests/workflow_shell_smoke.rs::fs21_resume_task_is_suppressed_when_earlier_closure_bridge_preempts_it`

### FS-22 — Repair is bridge-first and non-destructive when closure bridge exists

**Broken pattern:**

- `repair-review-state` clears dispatch lineage or task-scope state before honoring available closure bridge

**Expected fixed behavior:**

- repair routes directly to `task_closure_recording_ready`
- no destructive dispatch/task-scope cleanup in bridge case

**Primary tests:**

- `tests/workflow_runtime.rs::fs22_repair_review_state_prefers_non_destructive_closure_bridge_over_reentry_cleanup`
- `tests/internal_plan_execution.rs::internal_only_compatibility_fs22_repair_review_state_does_not_clear_dispatch_lineage_when_close_current_task_bridge_exists`

| Scenario | Source | Setup Summary | Expected Fixed Behavior | Probe Command Target (informational unless listed under Task 12 gates) |
|---|---|---|---|---|
| `FS-01` | session `03e9` | late-stage missing-current-closure with drift classification pressure | one consistent route across `operator`/`status`/`doctor`; no `repair already_current` contradiction | covered by prerelease-refresh stale-follow-up regressions; parity-probe budget `<=3` |
| `FS-02` | session `03e9` | late-stage doc/evidence writes around branch-closure baseline | deterministic classification: confined refresh vs true execution reentry vs explicit metadata blocker | parity regression and command-budget guard in compiled-CLI entry routing (`<=2`) |
| `FS-03` | session `a196` | stale prior-task redispatch while later task is active | blocking task target and accepted mutation target match | `<=3` |
| `FS-04` | session `a196` | repair/rebuild path with stale prior dispatch and resume overlays | repair yields one authoritative next action; no wrong blocker survives | `<=3` |
| `FS-05` | session `a196` | unsupported field request on mutation commands | fail before mutation; authoritative digest unchanged | `<=1` |
| `FS-06` | session `a196` | helper/direct path compared to compiled CLI path | compiled CLI remains contract oracle; helper semantics stay quarantined internally | helper cutover boundary lock (`<=2`) |
| `FS-07` | session `b83b` | status reports dispatch-required while operator advertises begin/reentry | all surfaces share same routing decision fields | covered by task-boundary dispatch-blocked routing regressions; parity-probe budget `<=3` |
| `FS-08` | session `b83b` | resume overlays plus stale prior-task closure | stale prerequisite remains visible; resume does not hide blocker | `<=1` |
| `FS-09` | session `b83b` | repair clears one stale condition and should expose next blocker | repair returns post-repair blocker directly | `<=3` |
| `FS-10` | PR `#34` bug class | stale persisted follow-up conflicts with live current truth | stale follow-up ignored/cleared; live closure truth wins | `<=1` |
| `FS-11` | stuck-session replay | operator/status recommend `begin` on a later task while begin-time legality still blocks on an earlier task boundary | operator, repair, and begin share one next-action decision and target the same blocker | `<=3` |
| `FS-12` | stuck-session replay | authoritative execution run exists while preflight acceptance is missing/stale | begin/operator/close-current-task use authoritative run identity without hidden preflight | `<=3` |
| `FS-13` | stuck-session replay | later interrupted/open-step marker masks an earlier stale task boundary | authoritative open-step state is projection-only control state; earliest stale boundary still wins | `<=3` |
| `FS-14` | stuck-session replay | task execution is current but current task-closure baseline/projections are missing | routing surfaces `task_closure_recording_ready` and closure repair runs through `close-current-task` | `<=2` |
| `FS-15` | stuck-session replay | after earlier repair, routing falsely targets a later stale task | stale-boundary targeting always selects earliest unresolved stale task | `<=2` |
| `FS-16` | stuck-session replay | prior task has current positive closure but receipt projections drift or are regenerated later | next-task begin remains legal; projection refresh stays a closure/projection concern | `<=2` |
| `FS-17` | churn convergence replay | truthful replay finished but runtime still falls back to execution reentry | replay converges through `task_closure_recording_ready` -> `close-current-task` with no second reopen | `<=2` |
| `FS-18` | churn convergence replay | cycle-break treated as global sticky blocker | cycle-break stays task-scoped, clears after bound task reclose, and next-task begin unblocks | `<=3` |
| `FS-19` | churn convergence replay | superseded stale history still selected as unresolved stale blocker | superseded stale history is ignored for stale-boundary routing | `<=2` |
| `FS-20` | churn convergence replay | runtime-owned plan/evidence-only churn unwinds upstream closure and late-stage chain | upstream closure and late-stage chain stay current when filtered drift is empty | `<=2` |
| `FS-21` | churn convergence replay | resume advisory hints remain visible while earlier closure bridge is authoritative | resume hints are suppressed; all surfaces route to close-current-task on the earlier task | `<=2` |
| `FS-22` | churn convergence replay | repair performs destructive cleanup despite available closure bridge | repair stays non-destructive and routes bridge-first to closure recording | `<=2` |

## Coverage Map

- `FS-01`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs01_shared_route_parity_for_missing_current_closure`
  - `tests/workflow_shell_smoke.rs::plan_execution_record_release_readiness_primitive_uses_shared_routing_when_stale`
  - `tests/workflow_shell_smoke.rs::runtime_remediation_fs01_compiled_cli_repair_and_branch_closure_do_not_disagree`
- `FS-02`:
  - `tests/workflow_runtime_final_review.rs::fs02_late_stage_drift_routes_consistently_across_operator_and_status`
  - `tests/workflow_entry_shell_smoke.rs::fs02_entry_route_surfaces_share_parity_and_budget`
- `FS-03`:
  - `tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs03_compiled_cli_dispatch_target_acceptance_and_mismatch`
  - `tests/internal_workflow_shell_smoke.rs::internal_only_compatibility_plan_execution_record_review_dispatch_prefers_task_boundary_target_over_interrupted_note_state`
  - `tests/internal_contracts_execution_runtime_boundaries.rs::internal_only_compatibility_runtime_remediation_fs03_internal_dispatch_target_acceptance_and_mismatch_preserve_mutation_contract`
- `FS-04`:
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator`
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs04_repair_returns_route_consumed_by_operator`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs04_rebuild_evidence_preserves_authoritative_state_digest`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_route_visibility_is_compiled_cli_contract`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift`
- `FS-05`:
  - `tests/internal_plan_execution.rs::internal_only_compatibility_record_review_dispatch_task_target_mismatch_fails_before_authoritative_mutation`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_record_final_review_rejects_unapproved_reviewer_source_before_mutation`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs05_unsupported_field_fails_before_mutation_on_compatibility_aliases`
- `FS-06`:
  - `tests/internal_workflow_shell_smoke.rs::internal_only_fs06_hidden_dispatch_target_mismatch_keeps_helper_semantics_and_cli_cutover_boundary`
- `FS-07`:
  - `tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked`
  - `tests/execution_query.rs::runtime_remediation_fs07_query_surface_parity_for_task_review_dispatch_blocked`
  - `tests/workflow_shell_smoke.rs::fs07_task_review_dispatch_route_parity_in_compiled_cli_surfaces`
- `FS-08`:
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker`
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs08_stale_blocker_visibility_is_compiled_cli_contract`
- `FS-09`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs09_repair_exposes_next_blocker_immediately`
  - `tests/workflow_entry_shell_smoke.rs::fs09_repair_surfaces_post_repair_next_blocker_in_entry_cli`
- `FS-10`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs10_stale_follow_up_is_ignored_when_truth_is_current`
  - `tests/workflow_shell_smoke.rs::prerelease_branch_closure_refresh_ignores_stale_execution_reentry_follow_up`
- `FS-11`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine`
  - `tests/workflow_runtime.rs::runtime_remediation_fs11_repair_returns_same_action_as_operator_and_begin`
  - `tests/workflow_shell_smoke.rs::fs11_operator_and_begin_target_parity_after_rebase_resume`
  - `tests/workflow_shell_smoke.rs::fs11_repair_output_matches_following_public_command_without_hidden_helper`
  - `tests/workflow_shell_smoke.rs::fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers`
- `FS-12`:
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_preflight`
  - `tests/workflow_shell_smoke.rs::fs12_recovery_path_does_not_require_hidden_preflight_when_run_identity_exists`
- `FS-13`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs13_markdown_note_is_projection_not_authority`
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs13_hidden_gates_do_not_materialize_legacy_open_step_state_when_blocked`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state`
  - `tests/workflow_shell_smoke.rs::fs13_normal_recovery_never_requires_manual_plan_note_edit`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs13_authoritative_open_step_state_survives_compiled_cli_round_trip`
- `FS-14`:
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry`
  - `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch`
  - `tests/workflow_shell_smoke.rs::fs14_recovery_to_close_current_task_uses_only_public_intent_commands`
- `FS-15`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target`
  - `tests/workflow_runtime.rs::runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists`
  - `tests/internal_contracts_execution_runtime_boundaries.rs::internal_only_compatibility_runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task`
- `FS-16`:
  - `tests/public_replay_churn.rs::public_replay_fs16_current_closure_allows_next_begin_after_projection_drift`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts`
- `FS-17`:
  - `tests/workflow_runtime.rs::fs17_stale_unreviewed_truthful_replay_promotes_to_task_closure_recording_ready`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_fs17_close_current_task_converges_after_truthful_replay_without_second_reopen`
- `FS-18`:
  - `tests/workflow_runtime.rs::fs18_cycle_break_binding_is_task_scoped_not_global`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_fs18_begin_unblocks_next_task_after_cycle_break_task_reclosed`
- `FS-19`:
  - `tests/workflow_runtime.rs::fs19_superseded_stale_historical_task_closure_is_not_an_unresolved_stale_boundary`
  - `tests/contracts_execution_runtime_boundaries.rs::fs19_compiled_cli_ignores_superseded_stale_history_when_selecting_blocking_task`
- `FS-20`:
  - `tests/workflow_runtime.rs::fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_stale_current_task_closure`
  - `tests/workflow_runtime.rs::fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_null_current_branch_closure`
  - `tests/workflow_runtime.rs::fs20_branch_closure_remains_current_when_only_runtime_owned_plan_and_execution_evidence_paths_changed`
  - `tests/workflow_shell_smoke.rs::fs20_reopening_downstream_stale_task_does_not_unwind_upstream_current_closure_when_only_plan_and_evidence_change`
  - `tests/workflow_shell_smoke.rs::fs20_late_stage_chain_is_not_unwound_by_runtime_owned_plan_and_execution_evidence_churn`
- `FS-21`:
  - `tests/workflow_runtime.rs::fs21_operator_status_and_exact_command_all_agree_on_close_current_task_when_bridge_is_ready`
  - `tests/workflow_shell_smoke.rs::fs21_resume_task_is_suppressed_when_earlier_closure_bridge_preempts_it`
- `FS-22`:
  - `tests/workflow_runtime.rs::fs22_repair_review_state_prefers_non_destructive_closure_bridge_over_reentry_cleanup`
  - `tests/internal_plan_execution.rs::internal_only_compatibility_fs22_repair_review_state_does_not_clear_dispatch_lineage_when_close_current_task_bridge_exists`

## Function-Level Traceability

- `FS-01`
  - Shared runtime: `tests/workflow_runtime.rs::runtime_remediation_fs01_shared_route_parity_for_missing_current_closure`
  - Compiled CLI parity: `tests/workflow_shell_smoke.rs::compiled_cli_route_parity_probe_for_late_stage_refresh_fixture`
  - Compiled CLI stale reroute parity guard: `tests/workflow_shell_smoke.rs::plan_execution_record_release_readiness_primitive_uses_shared_routing_when_stale`
  - Compiled CLI repair/branch-closure consistency: `tests/workflow_shell_smoke.rs::runtime_remediation_fs01_compiled_cli_repair_and_branch_closure_do_not_disagree`
- `FS-02`
  - Shared runtime: `tests/workflow_runtime_final_review.rs::fs02_late_stage_drift_routes_consistently_across_operator_and_status`
  - Compiled CLI parity: `tests/workflow_entry_shell_smoke.rs::fs02_entry_route_surfaces_share_parity_and_budget`
- `FS-03`
  - Shared runtime routing: `tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked`
  - Compiled CLI mutation acceptance/mismatch: `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs03_compiled_cli_dispatch_target_acceptance_and_mismatch`
  - Compiled CLI task-boundary target coherence: `tests/internal_workflow_shell_smoke.rs::internal_only_compatibility_plan_execution_record_review_dispatch_prefers_task_boundary_target_over_interrupted_note_state`
  - Internal compatibility dispatch-target mutation contract: `tests/internal_contracts_execution_runtime_boundaries.rs::internal_only_compatibility_runtime_remediation_fs03_internal_dispatch_target_acceptance_and_mismatch_preserve_mutation_contract`
- `FS-04`
  - Shared runtime repair blocker exposure: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs04_repair_returns_route_consumed_by_operator`
  - Compiled CLI repair parity: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator`
  - Authoritative digest invariant: `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs04_rebuild_evidence_preserves_authoritative_state_digest`
  - Compiled CLI repair-route contract: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_route_visibility_is_compiled_cli_contract`
  - Compiled CLI external-review-ready flag contract: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift`
- `FS-05`
  - Plan-execution no-mutation invariants: `tests/internal_plan_execution.rs::internal_only_compatibility_record_review_dispatch_task_target_mismatch_fails_before_authoritative_mutation`
  - Plan-execution no-mutation invariant for final-review scope task-field rejection: `tests/internal_plan_execution.rs::internal_only_compatibility_record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation`
  - Compatibility final-review reviewer-source no-mutation invariant: `tests/internal_plan_execution.rs::internal_only_compatibility_record_final_review_rejects_unapproved_reviewer_source_before_mutation`
  - Compatibility alias no-mutation invariant: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs05_unsupported_field_fails_before_mutation_on_compatibility_aliases`
- `FS-06`
  - Internal helper semantics and CLI cutover boundary: `tests/internal_workflow_shell_smoke.rs::internal_only_fs06_hidden_dispatch_target_mismatch_keeps_helper_semantics_and_cli_cutover_boundary`
- `FS-07`
  - Shared runtime blocked-task routing contract: `tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked`
  - Query-surface blocked-task routing parity: `tests/execution_query.rs::runtime_remediation_fs07_query_surface_parity_for_task_review_dispatch_blocked`
  - Compiled CLI route parity across surfaces: `tests/workflow_shell_smoke.rs::fs07_task_review_dispatch_route_parity_in_compiled_cli_surfaces`
- `FS-08`
  - Shared runtime stale-blocker visibility: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker`
  - Compiled CLI stale-blocker visibility: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker`
  - Compiled CLI stale-blocker contract: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs08_stale_blocker_visibility_is_compiled_cli_contract`
- `FS-09`
  - Shared runtime post-repair blocker exposure: `tests/workflow_runtime.rs::runtime_remediation_fs09_repair_exposes_next_blocker_immediately`
  - Compiled CLI post-repair blocker exposure: `tests/workflow_entry_shell_smoke.rs::fs09_repair_surfaces_post_repair_next_blocker_in_entry_cli`
- `FS-10`
  - Shared runtime stale-follow-up suppression: `tests/workflow_runtime.rs::runtime_remediation_fs10_stale_follow_up_is_ignored_when_truth_is_current`
  - Compiled CLI stale-follow-up suppression: `tests/workflow_shell_smoke.rs::prerelease_branch_closure_refresh_ignores_stale_execution_reentry_follow_up`
- `FS-11`
  - Shared next-action parity: `tests/workflow_runtime.rs::runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine`
  - Shared repair/operator alignment: `tests/workflow_runtime.rs::runtime_remediation_fs11_repair_returns_same_action_as_operator_and_begin`
  - Compiled CLI operator/begin parity: `tests/workflow_shell_smoke.rs::fs11_operator_and_begin_target_parity_after_rebase_resume`
  - Compiled CLI repair follow-up parity: `tests/workflow_shell_smoke.rs::fs11_repair_output_matches_following_public_command_without_hidden_helper`
  - Compiled CLI rebase/resume budget cap: `tests/workflow_shell_smoke.rs::fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers`
- `FS-12`
  - Shared authoritative run identity routing: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator`
  - Plan-execution authoritative run identity for closure recording: `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_preflight`
  - Compiled CLI recovery without hidden preflight: `tests/workflow_shell_smoke.rs::fs12_recovery_path_does_not_require_hidden_preflight_when_run_identity_exists`
- `FS-13`
  - Shared markdown-note projection boundary: `tests/workflow_runtime.rs::runtime_remediation_fs13_markdown_note_is_projection_not_authority`
  - Internal hidden-gate non-materialization boundary: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs13_hidden_gates_do_not_materialize_legacy_open_step_state_when_blocked`
  - Plan-execution authoritative open-step state updates: `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state`
  - Compiled CLI recovery without manual plan-note edits: `tests/workflow_shell_smoke.rs::fs13_normal_recovery_never_requires_manual_plan_note_edit`
  - Compiled CLI open-step state contract: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs13_authoritative_open_step_state_survives_compiled_cli_round_trip`
- `FS-14`
  - Shared closure-baseline repair routing: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry`
  - Shared closure-baseline repair-review-state parity: `tests/internal_workflow_runtime.rs::internal_only_compatibility_runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task`
  - Plan-execution closure-baseline regeneration without hidden dispatch: `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch`
  - Compiled CLI public-intent closure repair path: `tests/workflow_shell_smoke.rs::fs14_recovery_to_close_current_task_uses_only_public_intent_commands`
- `FS-15`
  - Shared earliest-stale-boundary precedence: `tests/workflow_runtime.rs::runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target`
  - Shared repair target parity after earlier cleanup: `tests/workflow_runtime.rs::runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists`
  - Compiled CLI stale-boundary target contract: `tests/internal_contracts_execution_runtime_boundaries.rs::internal_only_compatibility_runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task`
- `FS-16`
  - Public replay begin-time closure authority: `tests/public_replay_churn.rs::public_replay_fs16_current_closure_allows_next_begin_after_projection_drift`
  - Plan-execution begin closure authority independent of receipt projections: `tests/internal_plan_execution.rs::internal_only_compatibility_runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts`
- `FS-17`
  - Shared truthful-replay bridge convergence: `tests/workflow_runtime.rs::fs17_stale_unreviewed_truthful_replay_promotes_to_task_closure_recording_ready`
  - Plan-execution truthful-replay close-current-task budget: `tests/internal_plan_execution.rs::internal_only_compatibility_fs17_close_current_task_converges_after_truthful_replay_without_second_reopen`
- `FS-18`
  - Shared cycle-break task-scoping: `tests/workflow_runtime.rs::fs18_cycle_break_binding_is_task_scoped_not_global`
  - Plan-execution cycle-break clear and begin unblock: `tests/internal_plan_execution.rs::internal_only_compatibility_fs18_begin_unblocks_next_task_after_cycle_break_task_reclosed`
- `FS-19`
  - Shared superseded-stale-history suppression: `tests/workflow_runtime.rs::fs19_superseded_stale_historical_task_closure_is_not_an_unresolved_stale_boundary`
  - Compiled CLI stale-history suppression contract: `tests/contracts_execution_runtime_boundaries.rs::fs19_compiled_cli_ignores_superseded_stale_history_when_selecting_blocking_task`
- `FS-20`
  - Shared task-closure freshness under runtime-owned churn: `tests/workflow_runtime.rs::fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_stale_current_task_closure`
  - Shared branch-closure freshness under runtime-owned churn: `tests/workflow_runtime.rs::fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_null_current_branch_closure`
  - Shared filtered drift no-unwind guard: `tests/workflow_runtime.rs::fs20_branch_closure_remains_current_when_only_runtime_owned_plan_and_execution_evidence_paths_changed`
  - Compiled CLI downstream-reopen churn guard: `tests/workflow_shell_smoke.rs::fs20_reopening_downstream_stale_task_does_not_unwind_upstream_current_closure_when_only_plan_and_evidence_change`
  - Compiled CLI late-stage chain no-unwind guard: `tests/workflow_shell_smoke.rs::fs20_late_stage_chain_is_not_unwound_by_runtime_owned_plan_and_execution_evidence_churn`
- `FS-21`
  - Shared route parity for bridge-ready close-current-task preemption: `tests/workflow_runtime.rs::fs21_operator_status_and_exact_command_all_agree_on_close_current_task_when_bridge_is_ready`
  - Compiled CLI resume-hint suppression when preempted: `tests/workflow_shell_smoke.rs::fs21_resume_task_is_suppressed_when_earlier_closure_bridge_preempts_it`
- `FS-22`
  - Shared bridge-first non-destructive repair routing: `tests/workflow_runtime.rs::fs22_repair_review_state_prefers_non_destructive_closure_bridge_over_reentry_cleanup`
  - Plan-execution dispatch-lineage preservation under bridge-first repair: `tests/internal_plan_execution.rs::internal_only_compatibility_fs22_repair_review_state_does_not_clear_dispatch_lineage_when_close_current_task_bridge_exists`

- Task 12 command-budget gates (compiled CLI):
  - `fs17_close_current_task_converges_after_truthful_replay_without_second_reopen` (`tests/plan_execution.rs`) `<=2`
  - `fs20_reopening_downstream_stale_task_does_not_unwind_upstream_current_closure_when_only_plan_and_evidence_change` (`tests/workflow_shell_smoke.rs`) upstream closure-refresh route count `<=1`
  - `task_close_happy_path_runtime_management_budget_is_capped` (`tests/workflow_shell_smoke.rs`) `<=2`
  - `task_close_internal_dispatch_runtime_management_budget_is_capped` (`tests/workflow_shell_smoke.rs`) `<=2`
  - `fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers` (`tests/workflow_shell_smoke.rs`) `<=3`
  - `stale_release_refresh_runtime_management_budget_is_capped_before_new_review_step` (`tests/workflow_shell_smoke.rs`) `<=3`
- Public workflow-command mapping coverage:
  - `tests/bootstrap_smoke.rs::workflow_help_surface_hides_compatibility_only_commands`
  - `tests/workflow_shell_smoke.rs::workflow_help_outside_repo_mentions_the_public_surfaces`
- runtime-doc/skill-contract integration references:
  - `tests/runtime_instruction_plan_review_contracts.rs`
  - `tests/runtime_instruction_contracts.rs`
  - `tests/using_featureforge_skill.rs`
  - `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `tests/codex-runtime/skill-doc-generation.test.mjs`
  - `tests/codex-runtime/workflow-fixtures.test.mjs`
