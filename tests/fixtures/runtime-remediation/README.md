# Runtime Remediation Regression Inventory

This fixture index tracks the single-shot runtime-remediation regression scenarios.

Each scenario is represented by at least one compiled-CLI coverage test and, where useful,
an additional lower-level runtime/shared-truth test.

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
| `FS-11` | late-stage precedence | release-facing drift after review or direct review-first attempt | document release precedence enforced before final review, then QA | `<=3` |
| `FS-12` | cross-session diagnosis | derived receipt/projection missing with authoritative records intact | normal closure/late-stage routing remains valid; projection regenerates without truth mutation | `<=3` |
| `FS-13` | final-review deviation disposition | failed final review with recorded deviations and independent deviation adjudication | deviation disposition remains independently validated and fail-before-mutation compatibility guards hold | `<=2` |

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
  - `tests/workflow_runtime_final_review.rs::fs11_document_release_precedes_final_review_after_release_truth_stales`
  - `tests/plan_execution_final_review.rs::fs11_status_routes_release_readiness_before_final_review_when_release_state_stales`
  - `tests/plan_execution_final_review.rs::fs11_gate_finish_rejects_final_review_release_binding_mismatch`
- `FS-12`:
  - `tests/plan_execution.rs::rebuild_evidence_noop_regenerates_reviewer_projection_when_reviewer_projection_is_missing`
  - `tests/plan_execution.rs::rebuild_evidence_noop_regenerates_final_review_projection_when_reviewer_projection_is_tampered`
  - `tests/plan_execution_final_review.rs::fs12_missing_final_review_projection_regenerates_without_truth_mutation`
- `FS-13`:
  - `tests/workflow_shell_smoke.rs::plan_execution_advance_late_stage_final_review_keeps_deviation_verdict_independent_when_review_fails`
  - `tests/plan_execution_final_review.rs::dedicated_final_review_receipt_accepts_failed_result_with_independent_deviation_pass`
  - `tests/plan_execution_final_review.rs::dedicated_final_review_receipt_rejects_failed_result_with_failed_deviation_verdict`

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
  - Shared runtime precedence: `tests/workflow_runtime_final_review.rs::fs11_document_release_precedes_final_review_after_release_truth_stales`
  - Compiled CLI precedence: `tests/plan_execution_final_review.rs::fs11_status_routes_release_readiness_before_final_review_when_release_state_stales`
  - Finish-gate release-binding mismatch rejection: `tests/plan_execution_final_review.rs::fs11_gate_finish_rejects_final_review_release_binding_mismatch`
- `FS-12`
  - Plan-execution projection regeneration invariants: `tests/plan_execution.rs::rebuild_evidence_noop_regenerates_reviewer_projection_when_reviewer_projection_is_missing`
  - Plan-execution projection regeneration invariants (tampered final-review projection): `tests/plan_execution.rs::rebuild_evidence_noop_regenerates_final_review_projection_when_reviewer_projection_is_tampered`
  - Final-review projection regeneration invariants: `tests/plan_execution_final_review.rs::fs12_missing_final_review_projection_regenerates_without_truth_mutation`
- `FS-13`
  - Compiled CLI final-review deviation disposition independence: `tests/workflow_shell_smoke.rs::plan_execution_advance_late_stage_final_review_keeps_deviation_verdict_independent_when_review_fails`
  - Final-review receipt acceptance for failed result with independent deviation pass: `tests/plan_execution_final_review.rs::dedicated_final_review_receipt_accepts_failed_result_with_independent_deviation_pass`
  - Final-review receipt rejection for failed deviation verdict: `tests/plan_execution_final_review.rs::dedicated_final_review_receipt_rejects_failed_result_with_failed_deviation_verdict`

- Task 12 command-budget gates (compiled CLI):
  - `task_close_happy_path_runtime_management_budget_is_capped` (`tests/workflow_shell_smoke.rs`) `<=3`
  - `task_close_internal_dispatch_runtime_management_budget_is_capped` (`tests/workflow_shell_smoke.rs`) `<=2`
  - `reentry_recovery_runtime_management_budget_is_capped` (`tests/workflow_shell_smoke.rs`) `<=2`
  - `stale_release_refresh_runtime_management_budget_is_capped_before_new_review_step` (`tests/workflow_shell_smoke.rs`) `<=3`
- Public workflow-command mapping coverage:
  - `tests/bootstrap_smoke.rs::workflow_help_surface_hides_compatibility_only_commands`
  - `tests/workflow_shell_smoke.rs::workflow_help_outside_repo_mentions_the_public_surfaces`
- runtime-doc/skill-contract integration references:
  - `tests/runtime_instruction_contracts.rs`
  - `tests/using_featureforge_skill.rs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `tests/codex-runtime/workflow-fixtures.test.mjs`
