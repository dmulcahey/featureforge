pub(super) use std::collections::{BTreeMap, BTreeSet};
pub(super) use std::fs;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::time::Instant;

pub(super) use jiff::Timestamp;
pub(super) use serde::Serialize;
pub(super) use sha2::{Digest, Sha256};

pub(super) use crate::cli::plan_execution::{
    AdvanceLateStageArgs, AdvanceLateStageResultArg, BeginArgs, CloseCurrentTaskArgs, CompleteArgs,
    MaterializeProjectionScopeArg, MaterializeProjectionsArgs, ReopenArgs, ReviewOutcomeArg,
    StatusArgs, TransferArgs, VerificationOutcomeArg,
};
pub(super) use crate::diagnostics::{FailureClass, JsonFailure};
pub(super) use crate::execution::closure_dispatch::{
    TaskDispatchReviewedStateStatus, ensure_final_review_dispatch_id_matches,
    ensure_task_dispatch_id_matches, task_dispatch_reviewed_state_status,
};
pub(super) use crate::execution::closure_dispatch_mutation::{
    ensure_current_review_dispatch_id, ensure_current_review_dispatch_id_for_command,
};
pub(super) use crate::execution::command_eligibility::{
    PublicAdvanceLateStageMode, PublicCommand, PublicCommandInputRequirement, PublicMutationKind,
    PublicMutationRequest, PublicTransferMode, blocked_follow_up_for_operator,
    close_current_task_required_follow_up, late_stage_required_follow_up,
    negative_result_follow_up, operator_requires_review_state_repair,
    public_command_recommendation_surfaces, recommended_public_command_argv,
    recommended_public_command_display, release_readiness_required_follow_up,
    require_public_mutation, required_inputs_for_public_command,
};
pub(super) use crate::execution::command_model::{
    branch_closure_record_matches_plan_exemption, load_execution_read_scope_for_mutation,
    public_status_from_context_with_shared_routing,
    public_status_from_supplied_context_with_shared_routing, require_prior_task_closure_for_begin,
    still_current_task_closure_records, structural_current_task_closure_failures,
    task_closure_negative_result_blocks_current_reviewed_state,
    task_completion_lineage_fingerprint, usable_current_branch_closure_identity,
};
pub(super) use crate::execution::context::{
    current_file_proof, load_execution_context_for_rebuild,
};
pub(super) use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason, branch_closure_rerecording_assessment,
    branch_source_task_closure_ids as shared_branch_source_task_closure_ids,
    final_review_dispatch_still_current as shared_final_review_dispatch_still_current,
    finish_requires_test_plan_refresh as shared_finish_requires_test_plan_refresh,
    handoff_decision_scope as shared_handoff_decision_scope,
    is_runtime_owned_execution_control_plane_path,
    negative_result_requires_execution_reentry as shared_negative_result_requires_execution_reentry,
    normalize_summary_content,
    render_late_stage_surface_only_branch_surface as late_stage_surface_only_branch_surface,
    reviewer_source_is_valid as shared_reviewer_source_is_valid, summary_hash,
    task_closure_contributes_to_branch_surface as shared_task_closure_contributes_to_branch_surface,
};
#[cfg(test)]
pub(super) use crate::execution::current_truth::{
    normalized_late_stage_surface, path_matches_late_stage_surface,
};
pub(super) use crate::execution::final_review::authoritative_strategy_checkpoint_fingerprint_checked;
pub(super) use crate::execution::follow_up::RepairFollowUpKind;
pub(super) use crate::execution::internal_args::{
    NoteArgs, RebuildEvidenceArgs, RecordBranchClosureArgs, RecordFinalReviewArgs, RecordQaArgs,
    RecordReleaseReadinessArgs, ReviewDispatchScopeArg,
};
pub(super) use crate::execution::invariants::{
    InvariantEnforcementMode, check_runtime_status_invariants,
};
pub(super) use crate::execution::leases::{
    StatusAuthoritativeOverlay,
    authoritative_matching_execution_topology_downgrade_records_checked,
    load_status_authoritative_overlay_checked,
};
pub(super) use crate::execution::next_action::repair_review_state_public_command;
pub(super) use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
pub(super) use crate::execution::projection_renderer::{
    BranchClosureProjectionInput, FinalReviewProjectionInput, QaProjectionInput,
    RenderedExecutionProjections, publish_authoritative_artifact, render_branch_closure_artifact,
    render_execution_projections, render_final_review_artifacts, render_qa_artifact,
    render_release_readiness_artifact,
};
pub(super) use crate::execution::public_command_types::RecommendedPublicCommandArgv;
pub(super) use crate::execution::query::ExecutionRoutingState;
pub(super) use crate::execution::recording::{
    BranchClosureWrite, BrowserQaWrite, CurrentTaskClosureWrite, FinalReviewWrite,
    NegativeTaskClosureWrite, ReleaseReadinessWrite,
    current_task_closure_postconditions_would_mutate, record_browser_qa,
    record_current_branch_closure, record_current_task_closure,
    record_final_review as persist_final_review_record, record_negative_task_closure,
    record_release_readiness as persist_release_readiness_record,
    resolve_current_task_closure_postconditions_for_current_workspace,
};
pub(super) use crate::execution::reducer::RuntimeState;
pub(super) use crate::execution::router::project_runtime_routing_state_with_reduced_state;
pub(super) use crate::execution::semantic_identity::{
    branch_definition_identity_for_context, semantic_paths_changed_between_raw_trees,
    semantic_workspace_snapshot, task_definition_identity_for_task,
};
pub(super) use crate::execution::stale_target_projection::RuntimeGateSnapshot;
pub(super) use crate::execution::state::{
    EvidenceAttempt, ExecutionContext, ExecutionEvidence, ExecutionRuntime, FileProof,
    NO_REPO_FILES_MARKER, PlanExecutionStatus, RebuildEvidenceCandidate, RebuildEvidenceCounts,
    RebuildEvidenceFilter, RebuildEvidenceOutput, RebuildEvidenceTarget, current_head_sha,
    current_review_dispatch_id_candidate, current_test_plan_artifact_path_for_qa_recording,
    discover_rebuild_candidates, ensure_public_intent_preflight_ready, gate_finish_from_context,
    gate_review_from_context, load_execution_context_for_exact_plan,
    load_execution_context_for_mutation, normalize_begin_request, normalize_complete_request,
    normalize_note_request, normalize_rebuild_evidence_request, normalize_reopen_request,
    normalize_source, normalize_transfer_request, persist_allowed_public_begin_preflight,
    persist_finish_review_gate_pass_checkpoint_for_command,
    public_intent_preflight_persistence_required, require_normalized_text,
    require_preflight_acceptance, task_packet_fingerprint, validate_expected_fingerprint,
};
pub(super) use crate::execution::transitions::{
    AuthoritativeTransitionState, BranchClosureRecord, CurrentTaskClosureRecord,
    OpenStepStateRecord, StepCommand, claim_step_write_authority, enforce_active_contract_scope,
    enforce_authoritative_phase, load_authoritative_transition_state,
    load_or_initialize_authoritative_transition_state,
};
pub(super) use crate::git::{commit_object_fingerprint, discover_repository};
pub(super) use crate::paths::{
    harness_authoritative_artifact_path, normalize_repo_relative_path,
    write_atomic as write_atomic_file,
};

mod branch_closure_truth;
mod late_stage_reruns;
mod mutation_guards;
mod operator_outputs;
mod outputs;
mod path_persistence;
mod rebuild_support;
mod summary_inputs;

pub(super) use branch_closure_truth::*;
pub(super) use late_stage_reruns::*;
pub(super) use mutation_guards::*;
pub(crate) use operator_outputs::*;
pub(super) use outputs::*;
pub use outputs::{
    AdvanceLateStageOutput, CloseCurrentTaskOutput, MaterializeProjectionsOutput,
    RecordBranchClosureOutput, RecordQaOutput,
};
pub(super) use path_persistence::*;
pub(super) use rebuild_support::*;
pub(super) use summary_inputs::*;

#[cfg(test)]
mod unit_tests;
