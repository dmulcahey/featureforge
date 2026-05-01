use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use gix::bstr::ByteSlice;
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cli::plan_execution::{BeginArgs, CompleteArgs, ReopenArgs, StatusArgs, TransferArgs};
use crate::cli::repo_safety::{RepoSafetyCheckArgs, RepoSafetyIntentArg, RepoSafetyWriteTargetArg};
use crate::contracts::harness::{
    ExecutionTopologyDowngradeRecord, WORKTREE_LEASE_VERSION, WorktreeLease, WorktreeLeaseState,
    read_execution_contract,
};
use crate::contracts::plan::analyze_documents;
use crate::contracts::spec::parse_spec_file;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::authority::{
    ensure_preflight_authoritative_bootstrap,
    ensure_preflight_authoritative_bootstrap_with_existing_authority,
};
pub use crate::execution::context::{
    EvidenceAttempt, EvidenceFormat, ExecutionContext, ExecutionEvidence, FileProof,
    NO_REPO_FILES_MARKER, NoteState, PacketFingerprintInput, PlanStepState,
    compute_packet_fingerprint, current_file_proof, current_file_proof_checked,
    derive_evidence_rel_path, hash_contract_plan, load_execution_context,
    load_execution_context_for_mutation, normalize_source, parse_command_verification_summary,
    render_contract_plan, task_packet_fingerprint,
};
pub(crate) use crate::execution::context::{
    load_execution_context_for_exact_plan, parse_step_line,
};
use crate::execution::current_truth::{
    current_branch_closure_has_tracked_drift as shared_current_branch_closure_has_tracked_drift,
    current_late_stage_branch_bindings as shared_current_late_stage_branch_bindings,
    current_repo_tracked_tree_sha, is_runtime_owned_execution_control_plane_path,
    normalize_summary_content, reviewer_source_is_valid as shared_reviewer_source_is_valid,
};
use crate::execution::event_log::load_reduced_authoritative_state_for_state_path;
use crate::execution::final_review::{
    authoritative_strategy_checkpoint_fingerprint_checked, parse_artifact_document,
    resolve_release_base_branch,
};
use crate::execution::follow_up::{
    FollowUpAliasContext, FollowUpKind, direct_gate_follow_up_from_reason_codes,
    materialized_follow_up_kind_command, missing_branch_closure_gate_follow_up,
    normalize_follow_up_alias,
};
use crate::execution::harness::TopologySelectionContext;
use crate::execution::harness::{
    INITIAL_AUTHORITATIVE_SEQUENCE, LearnedTopologyGuidance, RunIdentitySnapshot,
};
use crate::execution::internal_args::{
    GateContractArgs, GateEvaluatorArgs, GateHandoffArgs, IsolatedAgentsArg, NoteArgs,
    NoteStateArg, RebuildEvidenceArgs, RecommendArgs, RecordContractArgs, RecordEvaluationArgs,
    RecordHandoffArgs, RecordReviewDispatchArgs, ReviewDispatchScopeArg,
};
use crate::execution::leases::authoritative_matching_execution_topology_downgrade_records_checked;
use crate::execution::leases::{
    PreflightWriteAuthorityState, authoritative_state_path,
    load_status_authoritative_overlay_checked, preflight_requires_authoritative_handoff,
    preflight_requires_authoritative_mutation_recovery, preflight_write_authority_state,
    validate_worktree_lease,
};
use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
use crate::execution::query::{ExecutionRoutingState, required_follow_up_from_routing};
pub use crate::execution::read_model::status_from_context;
pub(crate) use crate::execution::read_model::{
    ExactExecutionCommand, ExecutionDerivedTruth, ExecutionReadScope,
    ExecutionReentryCurrentTaskClosureTargets, FinalReviewDispatchAuthority,
    apply_public_read_invariants_to_status, apply_shared_routing_projection_to_read_scope,
    apply_shared_routing_projection_to_read_scope_with_routing,
    branch_closure_record_matches_plan_exemption, closure_baseline_candidate_task,
    compute_status_blocking_records, current_branch_closure_id,
    current_branch_closure_structural_review_state_reason, current_branch_reviewed_state_id,
    current_final_review_dispatch_authority_for_context,
    current_task_review_dispatch_id_for_status, derive_execution_truth_from_authority,
    derive_execution_truth_from_authority_with_gates, document_release_pending_phase_detail,
    execution_reentry_current_task_closure_targets_from_stale_tasks,
    execution_reentry_requires_review_state_repair, finish_review_gate_pass_branch_closure_id,
    has_authoritative_late_stage_progress, is_late_stage_phase, load_execution_read_scope,
    load_execution_read_scope_for_mutation, missing_derived_review_state_fields,
    normalize_optional_overlay_value, parse_harness_phase,
    prerelease_branch_closure_refresh_required, project_persisted_public_repair_targets,
    recommended_execution_source, reopen_exact_execution_command_for_task,
    resolve_exact_execution_command, shared_repair_review_state_reroute_decision,
    stale_current_task_closure_records, status_workspace_state_id,
    task_scope_review_state_repair_reason, task_scope_structural_review_state_reason,
    usable_current_branch_closure_identity,
    usable_current_branch_closure_identity_from_authoritative_state,
    validated_current_branch_closure_identity,
};
pub(crate) use crate::execution::read_model_support::{
    TaskCurrentClosureStatus, active_step, authoritative_unit_review_receipt_path,
    context_all_task_scopes_closed_by_authority, current_review_dispatch_id_from_lineage,
    current_review_dispatch_id_if_still_current, current_task_closure_overlay_restore_required,
    latest_attempt_for_step, latest_attempt_indices_by_step, latest_attempted_step_for_task,
    latest_completed_attempts_by_file, latest_completed_attempts_by_step,
    pre_reducer_earliest_unresolved_stale_task, qa_pending_requires_test_plan_refresh,
    resolve_branch_closure_reviewed_tree_sha, resolve_task_closure_reviewed_tree_sha,
    still_current_task_closure_records,
    still_current_task_closure_records_from_authoritative_state,
    task_boundary_reason_code_from_message, task_closure_baseline_bridge_ready_for_stale_target,
    task_closure_baseline_candidate_can_preempt_stale_target,
    task_closure_baseline_repair_candidate_with_stale_target, task_closure_recording_prerequisites,
    task_closures_are_non_branch_contributing, task_completion_lineage_fingerprint,
    task_current_closure_status,
};
pub use crate::execution::runtime::{ExecutionRuntime, state_dir};
use crate::execution::semantic_identity::{
    normalized_plan_source_for_approved_plan_preflight,
    normalized_plan_source_for_semantic_identity,
};
pub(crate) use crate::execution::status::GateProjectionInputs;
pub use crate::execution::status::{
    GateDiagnostic, GateResult, GateState, PlanExecutionStatus, PublicExecutionCommandContext,
    PublicRecordingContext, PublicRepairTarget, PublicReviewStateTaskClosure, StatusBlockingRecord,
    write_plan_execution_schema,
};
use crate::execution::topology::{
    RecommendOutput, default_preflight_chunking_strategy, default_preflight_evaluator_policy,
    default_preflight_reset_policy, default_preflight_review_stack, recommend_topology,
    tasks_are_independent,
};
use crate::execution::topology::{
    authoritative_run_identity_present, persist_preflight_acceptance,
    preflight_acceptance_for_context,
};
use crate::execution::transitions::{
    AuthoritativeTransitionState, CurrentBrowserQaRecord, claim_step_write_authority,
    load_authoritative_transition_state,
};
use crate::execution::workflow_operator_requery_command;
use crate::git::{
    commit_object_fingerprint, discover_repository,
    is_ancestor_commit as shared_is_ancestor_commit, sha256_hex,
};
use crate::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_authoritative_artifacts_dir,
    normalize_whitespace,
};
use crate::repo_safety::RepoSafetyRuntime;

mod artifact_finish_truth;
mod command_requests;
mod finish_gate;
mod preflight;
mod rebuild_evidence;
mod repo_state;
mod review_gate;
mod runtime_methods;
mod unit_review_truth;
mod worktree_lease_truth;

pub(crate) use artifact_finish_truth::current_test_plan_artifact_path_for_qa_recording;
use artifact_finish_truth::{
    require_current_browser_qa_pass_for_finish, require_current_final_review_pass_for_finish,
    require_current_release_readiness_ready_for_finish,
};
pub use command_requests::{
    BeginRequest, CompleteRequest, NoteRequest, RebuildEvidenceCandidate, RebuildEvidenceRequest,
    ReopenRequest, TransferRequest, TransferRequestMode, normalize_begin_request,
    normalize_complete_request, normalize_note_request, normalize_reopen_request,
    normalize_transfer_request, require_normalized_text,
};
pub use finish_gate::gate_finish_from_context;
use finish_gate::{
    enforce_review_authoritative_late_gate_truth,
    finish_review_gate_checkpoint_matches_current_branch_closure,
};
pub use preflight::{
    ensure_public_begin_preflight_ready, ensure_public_intent_preflight_ready,
    persist_allowed_public_begin_preflight, preflight_from_context,
    public_begin_preflight_persistence_required, public_intent_preflight_persistence_required,
    require_preflight_acceptance, validate_expected_fingerprint,
    validate_public_begin_preflight_allowed, validate_public_intent_preflight_allowed,
};
pub use rebuild_evidence::{
    RebuildEvidenceCounts, RebuildEvidenceFilter, RebuildEvidenceOutput, RebuildEvidenceTarget,
    discover_rebuild_candidates, normalize_rebuild_evidence_request,
    validate_v2_evidence_provenance,
};
pub use repo_state::{current_head_sha, current_tracked_tree_sha};
pub(crate) use repo_state::{
    repo_has_non_runtime_projection_tracked_changes, repo_has_unresolved_index_entries,
    repo_head_detached, repo_safety_preflight_message, repo_safety_preflight_remediation,
    repo_safety_stage,
};
pub use review_gate::gate_review_from_context;
use review_gate::{
    evaluate_pre_checkpoint_finish_gate, gate_result_current_branch_closure_id,
    gate_review_base_result, gate_review_from_context_internal,
    persist_finish_review_gate_pass_checkpoint,
};
pub use runtime_methods::RecordReviewDispatchOutput;
#[cfg(test)]
pub(crate) use runtime_methods::record_review_dispatch_blocked_output_from_gate;
pub(crate) use runtime_methods::{
    current_review_dispatch_id_candidate, ensure_current_review_dispatch_id,
};
use unit_review_truth::{
    UnitReviewReceiptExpectations, approved_unit_contract_fingerprint_for_review,
    enforce_plain_unit_review_truth, enforce_serial_unit_review_truth, is_ancestor_commit,
    load_authoritative_active_contract, reconcile_result_proof_fingerprint_for_review,
    validate_authoritative_unit_review_receipt, validate_authoritative_worktree_lease_fingerprint,
    worktree_lease_execution_context_key,
};
use worktree_lease_truth::{
    current_run_plain_unit_review_receipt_paths, enforce_worktree_lease_binding_truth,
};

#[derive(Debug, Deserialize)]
struct WorktreeLeaseRunIdentityProbe {
    execution_run_id: String,
    source_plan_path: String,
    source_plan_revision: u32,
}

#[derive(Debug, Deserialize)]
struct WorktreeLeaseBindingProbe {
    execution_run_id: String,
    lease_fingerprint: String,
    lease_artifact_path: String,
    #[serde(default)]
    execution_context_key: Option<String>,
    #[serde(default)]
    approved_task_packet_fingerprint: Option<String>,
    #[serde(default)]
    approved_unit_contract_fingerprint: Option<String>,
    #[serde(default)]
    reconcile_result_proof_fingerprint: Option<String>,
    #[serde(default)]
    reviewed_checkpoint_commit_sha: Option<String>,
    #[serde(default)]
    reconcile_result_commit_sha: Option<String>,
    #[serde(default)]
    reconcile_mode: Option<String>,
    #[serde(default)]
    review_receipt_fingerprint: Option<String>,
    #[serde(default)]
    review_receipt_artifact_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorktreeLeaseAuthoritativeContextProbe {
    #[serde(default)]
    run_identity: Option<WorktreeLeaseRunIdentityProbe>,
    #[serde(default)]
    repo_state_baseline_head_sha: Option<String>,
    #[serde(default)]
    repo_state_baseline_worktree_fingerprint: Option<String>,
    active_worktree_lease_fingerprints: Option<Vec<String>>,
    active_worktree_lease_bindings: Option<Vec<WorktreeLeaseBindingProbe>>,
}
