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

### FS-06 — `a196` helper-path passes, real CLI path drifts

**Broken pattern:** helper-backed tests pass but compiled CLI behavior differs.

**Expected fixed behavior:** compiled CLI and helper-path behavior match, with compiled CLI treated as authoritative.

**Primary tests:**

- `tests/workflow_shell_smoke.rs`

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

- `tests/workflow_runtime.rs::runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator`
- `tests/plan_execution.rs::runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_preflight`
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

- `tests/workflow_runtime.rs::runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry`
- `tests/plan_execution.rs::runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch`
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
- `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task`

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

- `tests/workflow_runtime.rs::runtime_remediation_fs16_current_positive_task_closure_allows_next_task_begin_even_if_receipts_need_projection_refresh`
- `tests/plan_execution.rs::runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts`

| Scenario | Source | Setup Summary | Expected Fixed Behavior | Probe Command Target (informational unless listed under Task 12 gates) |
|---|---|---|---|---|
| `FS-01` | session `03e9` | late-stage missing-current-closure with drift classification pressure | one consistent route across `operator`/`status`/`doctor`; no `repair already_current` contradiction | covered by prerelease-refresh stale-follow-up regressions; parity-probe budget `<=3` |
| `FS-02` | session `03e9` | late-stage doc/evidence writes around branch-closure baseline | deterministic classification: confined refresh vs true execution reentry vs explicit metadata blocker | parity regression and command-budget guard in compiled-CLI entry routing (`<=2`) |
| `FS-03` | session `a196` | stale prior-task redispatch while later task is active | blocking task target and accepted mutation target match | `<=3` |
| `FS-04` | session `a196` | repair/rebuild path with stale prior dispatch and resume overlays | repair yields one authoritative next action; no wrong blocker survives | `<=3` |
| `FS-05` | session `a196` | unsupported field request on mutation commands | fail before mutation; authoritative digest unchanged | `<=1` |
| `FS-06` | session `a196` | helper/direct path compared to compiled CLI path | compiled CLI remains contract oracle; helper parity enforced | helper-vs-compiled-cli target-mismatch parity lock (`<=2`) |
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
  - `tests/plan_execution.rs::runtime_remediation_fs03_compiled_cli_dispatch_target_acceptance_and_mismatch`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs03_dispatch_target_acceptance_and_mismatch_stay_aligned_between_direct_and_compiled_cli`
- `FS-04`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator`
  - `tests/workflow_runtime.rs::runtime_remediation_fs04_repair_returns_route_consumed_by_operator`
  - `tests/plan_execution.rs::runtime_remediation_fs04_rebuild_evidence_preserves_authoritative_state_digest`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_route_visibility_stays_aligned_between_direct_and_compiled_cli`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift`
- `FS-05`:
  - `tests/plan_execution.rs::record_review_dispatch_task_target_mismatch_fails_before_authoritative_mutation`
  - `tests/plan_execution.rs::record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation`
  - `tests/plan_execution.rs::record_final_review_rejects_unapproved_reviewer_source_before_mutation`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs05_unsupported_field_fails_before_mutation_on_compatibility_aliases`
- `FS-06`:
  - `tests/workflow_shell_smoke.rs::fs06_helper_and_compiled_cli_target_mismatch_stay_in_parity`
- `FS-07`:
  - `tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked`
  - `tests/execution_query.rs::runtime_remediation_fs07_query_surface_parity_for_task_review_dispatch_blocked`
  - `tests/workflow_shell_smoke.rs::fs07_task_review_dispatch_route_parity_in_compiled_cli_surfaces`
- `FS-08`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker`
  - `tests/workflow_runtime.rs::runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs08_stale_blocker_visibility_stays_aligned_between_direct_and_compiled_cli`
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
  - `tests/workflow_runtime.rs::runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator`
  - `tests/plan_execution.rs::runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_preflight`
  - `tests/workflow_shell_smoke.rs::fs12_recovery_path_does_not_require_hidden_preflight_when_run_identity_exists`
- `FS-13`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs13_markdown_note_is_projection_not_authority`
  - `tests/workflow_runtime.rs::runtime_remediation_fs13_hidden_gates_materialize_legacy_open_step_state_when_blocked`
  - `tests/plan_execution.rs::runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state`
  - `tests/workflow_shell_smoke.rs::fs13_normal_recovery_never_requires_manual_plan_note_edit`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs13_authoritative_open_step_state_survives_compiled_cli_round_trip`
- `FS-14`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry`
  - `tests/workflow_runtime.rs::runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task`
  - `tests/plan_execution.rs::runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch`
  - `tests/workflow_shell_smoke.rs::fs14_recovery_to_close_current_task_uses_only_public_intent_commands`
- `FS-15`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target`
  - `tests/workflow_runtime.rs::runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists`
  - `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task`
- `FS-16`:
  - `tests/workflow_runtime.rs::runtime_remediation_fs16_current_positive_task_closure_allows_next_task_begin_even_if_receipts_need_projection_refresh`
  - `tests/plan_execution.rs::runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts`

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
  - Compiled CLI mutation acceptance/mismatch: `tests/plan_execution.rs::runtime_remediation_fs03_compiled_cli_dispatch_target_acceptance_and_mismatch`
  - Compiled CLI task-boundary target coherence: `tests/workflow_shell_smoke.rs::plan_execution_record_review_dispatch_prefers_task_boundary_target_over_interrupted_note_state`
  - Direct-vs-compiled CLI boundary parity: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs03_dispatch_target_acceptance_and_mismatch_stay_aligned_between_direct_and_compiled_cli`
- `FS-04`
  - Shared runtime repair blocker exposure: `tests/workflow_runtime.rs::runtime_remediation_fs04_repair_returns_route_consumed_by_operator`
  - Compiled CLI repair parity: `tests/workflow_runtime.rs::runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator`
  - Authoritative digest invariant: `tests/plan_execution.rs::runtime_remediation_fs04_rebuild_evidence_preserves_authoritative_state_digest`
  - Direct-vs-compiled CLI repair-route parity: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_route_visibility_stays_aligned_between_direct_and_compiled_cli`
  - Direct-vs-compiled CLI external-review-ready flag parity: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift`
- `FS-05`
  - Plan-execution no-mutation invariants: `tests/plan_execution.rs::record_review_dispatch_task_target_mismatch_fails_before_authoritative_mutation`
  - Plan-execution no-mutation invariant for final-review scope task-field rejection: `tests/plan_execution.rs::record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation`
  - Compatibility final-review reviewer-source no-mutation invariant: `tests/plan_execution.rs::record_final_review_rejects_unapproved_reviewer_source_before_mutation`
  - Compatibility alias no-mutation invariant: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs05_unsupported_field_fails_before_mutation_on_compatibility_aliases`
- `FS-06`
  - Helper vs compiled CLI parity lock: `tests/workflow_shell_smoke.rs::fs06_helper_and_compiled_cli_target_mismatch_stay_in_parity`
- `FS-07`
  - Shared runtime blocked-task routing contract: `tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked`
  - Query-surface blocked-task routing parity: `tests/execution_query.rs::runtime_remediation_fs07_query_surface_parity_for_task_review_dispatch_blocked`
  - Compiled CLI route parity across surfaces: `tests/workflow_shell_smoke.rs::fs07_task_review_dispatch_route_parity_in_compiled_cli_surfaces`
- `FS-08`
  - Shared runtime stale-blocker visibility: `tests/workflow_runtime.rs::runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker`
  - Compiled CLI stale-blocker visibility: `tests/workflow_runtime.rs::runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker`
  - Direct-vs-compiled CLI stale-blocker parity: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs08_stale_blocker_visibility_stays_aligned_between_direct_and_compiled_cli`
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
  - Shared authoritative run identity routing: `tests/workflow_runtime.rs::runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator`
  - Plan-execution authoritative run identity for closure recording: `tests/plan_execution.rs::runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_preflight`
  - Compiled CLI recovery without hidden preflight: `tests/workflow_shell_smoke.rs::fs12_recovery_path_does_not_require_hidden_preflight_when_run_identity_exists`
- `FS-13`
  - Shared markdown-note projection boundary: `tests/workflow_runtime.rs::runtime_remediation_fs13_markdown_note_is_projection_not_authority`
  - Shared hidden-gate migration/materialization boundary: `tests/workflow_runtime.rs::runtime_remediation_fs13_hidden_gates_materialize_legacy_open_step_state_when_blocked`
  - Plan-execution authoritative open-step state updates: `tests/plan_execution.rs::runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state`
  - Compiled CLI recovery without manual plan-note edits: `tests/workflow_shell_smoke.rs::fs13_normal_recovery_never_requires_manual_plan_note_edit`
  - Direct-vs-compiled CLI open-step state parity: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs13_authoritative_open_step_state_survives_compiled_cli_round_trip`
- `FS-14`
  - Shared closure-baseline repair routing: `tests/workflow_runtime.rs::runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry`
  - Shared closure-baseline repair-review-state parity: `tests/workflow_runtime.rs::runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task`
  - Plan-execution closure-baseline regeneration without hidden dispatch: `tests/plan_execution.rs::runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch`
  - Compiled CLI public-intent closure repair path: `tests/workflow_shell_smoke.rs::fs14_recovery_to_close_current_task_uses_only_public_intent_commands`
- `FS-15`
  - Shared earliest-stale-boundary precedence: `tests/workflow_runtime.rs::runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target`
  - Shared repair target parity after earlier cleanup: `tests/workflow_runtime.rs::runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists`
  - Direct-vs-compiled CLI stale-boundary target parity: `tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task`
- `FS-16`
  - Shared begin-time closure authority: `tests/workflow_runtime.rs::runtime_remediation_fs16_current_positive_task_closure_allows_next_task_begin_even_if_receipts_need_projection_refresh`
  - Plan-execution begin closure authority independent of receipt projections: `tests/plan_execution.rs::runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts`

- Task 12 command-budget gates (compiled CLI):
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
