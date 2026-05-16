use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use gix::bstr::ByteSlice;
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::Serialize;

use crate::cli::plan_execution::{BeginArgs, CompleteArgs, ReopenArgs, StatusArgs, TransferArgs};
use crate::cli::repo_safety::{RepoSafetyCheckArgs, RepoSafetyIntentArg, RepoSafetyWriteTargetArg};
use crate::contracts::harness::{
    ExecutionContract, ExecutionTopologyDowngradeRecord, WORKTREE_LEASE_VERSION, WorktreeLease,
    WorktreeLeaseState, parse_contract_task_step_scope, read_execution_contract,
};
use crate::contracts::plan::analyze_documents;
use crate::contracts::spec::parse_spec_file;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::authority::{
    ensure_preflight_authoritative_bootstrap,
    ensure_preflight_authoritative_bootstrap_with_existing_authority,
};
pub(crate) use crate::execution::closure_dispatch::{
    current_review_dispatch_id_candidate, current_review_dispatch_id_from_lineage,
    current_review_dispatch_id_if_still_current,
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
pub(crate) use crate::execution::current_closure_projection::{
    TaskCurrentClosureStatus, current_task_closure_overlay_restore_required,
    still_current_task_closure_records_from_authoritative_state, task_current_closure_status,
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
    normalize_follow_up_alias,
};
use crate::execution::harness::{
    INITIAL_AUTHORITATIVE_SEQUENCE, LearnedTopologyGuidance, RunIdentitySnapshot,
    TopologySelectionContext, WorktreeLeaseBindingSnapshot, WorktreeLeaseReleaseRecord,
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
    ExecutionReadScope, ExecutionReentryCurrentTaskClosureTargets,
    apply_public_read_invariants_to_read_scope, apply_shared_routing_projection_to_read_scope,
    apply_shared_routing_projection_to_read_scope_with_routing,
    branch_closure_record_matches_plan_exemption,
    current_branch_closure_structural_review_state_reason,
    current_branch_gate_bindings_from_authoritative_state,
    execution_reentry_current_task_closure_targets_from_inputs,
    execution_reentry_requires_review_state_repair,
    execution_reentry_requires_review_state_repair_with_authority, load_execution_read_scope,
    load_execution_read_scope_for_mutation, missing_derived_review_state_fields,
    normalize_optional_overlay_value, recommended_execution_source,
    shared_repair_review_state_reroute_decision, status_workspace_state_id,
    task_scope_review_state_repair_reason, task_scope_structural_review_state_reason,
    usable_current_branch_closure_identity_from_authoritative_state,
    validated_current_branch_closure_identity,
    validated_current_branch_closure_identity_from_authoritative_state,
};
pub use crate::execution::runtime::{ExecutionRuntime, state_dir};
use crate::execution::semantic_identity::{
    normalized_plan_source_for_approved_plan_preflight,
    normalized_plan_source_for_semantic_identity,
};
pub(crate) use crate::execution::stale_target_projection::closure_baseline_candidate_task;
pub(crate) use crate::execution::status::GateProjectionInputs;
pub use crate::execution::status::{
    GateDiagnostic, GateResult, GateState, PlanExecutionStatus, PublicExecutionCommandContext,
    PublicRecordingContext, PublicRepairTarget, PublicReviewStateTaskClosure, StatusBlockingRecord,
    write_plan_execution_schema,
};
pub(crate) type AuthoritativeTransitionStateRef<'a> =
    Result<Option<&'a AuthoritativeTransitionState>, &'a JsonFailure>;
pub(crate) use crate::execution::status_support::{
    CurrentTaskClosureBranchRouteFacts, active_step, authoritative_completed_steps_for_context,
    current_task_closure_branch_route_facts_from_status, latest_attempt_for_step,
    latest_attempt_indices_by_step, latest_attempted_step_for_task,
    latest_completed_attempts_by_file, latest_completed_attempts_by_step,
    resolve_branch_closure_reviewed_tree_sha, resolve_task_closure_reviewed_tree_sha,
    task_boundary_reason_code_from_message,
    task_closure_baseline_candidate_can_preempt_stale_target,
    task_closure_baseline_repair_candidate_with_stale_target_and_authority,
    task_completion_lineage_fingerprint, task_latest_attempts_are_completed,
};
pub(super) use crate::execution::status_support::{
    PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT, WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND,
    public_typed_operator_route_contract,
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
pub(crate) use worktree_lease_truth::{
    releasable_terminal_worktree_lease_fingerprints_for_task_closure,
    worktree_lease_public_gate_reason_code,
};

pub(super) const PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION: &str = concat!(
    "The runtime proof metadata is stale or invalid. Run `featureforge workflow operator --plan <approved-plan-path> --json`; ",
    public_typed_operator_route_contract!(),
    "; do not manually edit internal proof artifacts."
);
pub(super) const PUBLIC_WORKFLOW_OPERATOR_REMEDIATION: &str = concat!(
    "The execution proof metadata is stale or invalid. Run `featureforge workflow operator --plan <approved-plan-path> --json`; ",
    public_typed_operator_route_contract!(),
    "; do not manually edit proof artifacts."
);
pub(super) const PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION: &str = concat!(
    "The completed execution unit is missing current independent review proof metadata. Run `featureforge workflow operator --plan <approved-plan-path> --json`; ",
    public_typed_operator_route_contract!(),
    "; do not record internal proof artifacts directly."
);
pub(super) const PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION: &str = concat!(
    "The late-stage proof metadata is stale or invalid. Run `featureforge workflow operator --plan <approved-plan-path> --json`; ",
    public_typed_operator_route_contract!(),
    "."
);

pub(super) fn public_workflow_operator_remediation_for_plan(plan_rel: &str) -> String {
    format!(
        "The execution proof metadata is stale or invalid. Run `{WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND}` for `{plan_rel}`; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}; do not manually edit proof artifacts."
    )
}

pub(super) fn public_typed_operator_route_remediation(context: &str) -> String {
    format!(
        "{context} Run `featureforge workflow operator --plan <approved-plan-path> --json`; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}."
    )
}

pub(super) fn public_typed_operator_route_remediation_for_plan(
    context: &str,
    plan_rel: &str,
) -> String {
    format!(
        "{context} Run `{WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND}` for `{plan_rel}`; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}."
    )
}

pub(super) fn step_completed_by_authoritative_truth(
    step: &PlanStepState,
    authoritative_completed_steps: Option<&BTreeSet<(u32, u32)>>,
) -> bool {
    authoritative_completed_steps.map_or(step.checked, |completed_steps| {
        completed_steps.contains(&(step.task_number, step.step_number))
    })
}

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
pub(crate) use finish_gate::gate_finish_from_context_with_authoritative_state;
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
    validate_v2_evidence_provenance, validate_v2_evidence_provenance_for_completed_steps,
    warn_v2_evidence_provenance,
};
pub use repo_state::{current_head_sha, current_tracked_tree_sha};
pub(crate) use repo_state::{
    repo_has_non_runtime_projection_tracked_changes, repo_has_unresolved_index_entries,
    repo_head_detached, repo_safety_preflight_message, repo_safety_preflight_remediation,
    repo_safety_stage,
};
pub use review_gate::gate_review_from_context;
use review_gate::{
    evaluate_pre_checkpoint_finish_gate, gate_review_base_result,
    gate_review_from_context_internal, verify_completed_step_evidence_projection,
};
pub(crate) use review_gate::{
    gate_review_from_context_with_authoritative_state,
    persist_finish_review_gate_pass_checkpoint_for_command_with_authoritative_state,
};
pub use runtime_methods::RecordReviewDispatchOutput;
#[cfg(test)]
pub(crate) use runtime_methods::record_review_dispatch_blocked_output_from_gate;
use unit_review_truth::{
    UnitReviewProofAuthority, UnitReviewReceiptExpectations,
    approved_unit_contract_fingerprint_for_review, classify_unit_review_proof_authority,
    enforce_serial_unit_review_truth, is_ancestor_commit, load_authoritative_active_contract,
    reconcile_result_proof_fingerprint_for_review, validate_authoritative_unit_review_receipt,
    validate_authoritative_worktree_lease_fingerprint,
    warn_plain_unit_review_receipts_diagnostic_only, worktree_lease_execution_context_key,
};
use worktree_lease_truth::{
    current_run_plain_unit_review_receipt_paths, enforce_worktree_lease_binding_truth,
};
