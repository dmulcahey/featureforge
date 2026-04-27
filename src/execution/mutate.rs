use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cli::plan_execution::{
    AdvanceLateStageArgs, AdvanceLateStageResultArg, BeginArgs, CloseCurrentTaskArgs, CompleteArgs,
    MaterializeProjectionScopeArg, MaterializeProjectionsArgs, NoteArgs, RebuildEvidenceArgs,
    RecordBranchClosureArgs, RecordFinalReviewArgs, RecordQaArgs, RecordReleaseReadinessArgs,
    ReopenArgs, ReviewDispatchScopeArg, ReviewOutcomeArg, StatusArgs, TransferArgs,
    VerificationOutcomeArg,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::authority::write_authoritative_unit_review_receipt_artifact;
use crate::execution::command_eligibility::{
    PublicMutationKind, PublicMutationRequest, PublicTransferMode, blocked_follow_up_for_operator,
    close_current_task_required_follow_up, decide_public_mutation, late_stage_required_follow_up,
    negative_result_follow_up, operator_requires_review_state_repair,
    release_readiness_required_follow_up, require_public_mutation,
};
use crate::execution::current_truth::{
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
use crate::execution::current_truth::{
    normalized_late_stage_surface, path_matches_late_stage_surface,
};
use crate::execution::final_review::authoritative_strategy_checkpoint_fingerprint_checked;
use crate::execution::handoff::{
    WorkflowTransferRecordIdentity, WorkflowTransferRecordInput,
    current_workflow_transfer_record_path, latest_matching_workflow_transfer_request_record,
    write_workflow_transfer_record,
};
use crate::execution::invariants::{InvariantEnforcementMode, check_runtime_status_invariants};
use crate::execution::leases::{
    StatusAuthoritativeOverlay,
    authoritative_matching_execution_topology_downgrade_records_checked,
    load_status_authoritative_overlay_checked,
};
use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
use crate::execution::projection_renderer::{
    BranchClosureProjectionInput, FinalReviewProjectionInput, ProjectionWriteMode,
    QaProjectionInput, RenderedExecutionProjections, materialize_late_stage_projection_artifacts,
    normal_projection_write_mode, publish_authoritative_artifact,
    regenerate_projection_artifacts_from_authoritative_state, render_branch_closure_artifact,
    render_execution_projections, render_final_review_artifacts, render_qa_artifact,
    render_release_readiness_artifact, timestamp_slug, write_execution_projection_read_models,
    write_project_artifact,
};
use crate::execution::query::ExecutionRoutingState;
use crate::execution::recording::{
    BranchClosureWrite, BrowserQaWrite, CurrentTaskClosureWrite, FinalReviewWrite,
    NegativeTaskClosureWrite, ReleaseReadinessWrite,
    current_task_closure_postconditions_would_mutate, record_browser_qa,
    record_current_branch_closure, record_current_task_closure,
    record_final_review as persist_final_review_record, record_negative_task_closure,
    record_release_readiness as persist_release_readiness_record,
    resolve_current_task_closure_postconditions,
};
use crate::execution::reducer::{RuntimeGateSnapshot, RuntimeState};
use crate::execution::router::project_runtime_routing_state_with_reduced_state;
use crate::execution::semantic_identity::{
    branch_definition_identity_for_context, semantic_paths_changed_between_raw_trees,
    semantic_workspace_snapshot, task_definition_identity_for_task,
};
use crate::execution::state::{
    EvidenceAttempt, ExecutionContext, ExecutionEvidence, ExecutionRuntime, FileProof,
    NO_REPO_FILES_MARKER, PlanExecutionStatus, RebuildEvidenceCandidate, RebuildEvidenceCounts,
    RebuildEvidenceFilter, RebuildEvidenceOutput, RebuildEvidenceTarget,
    branch_closure_record_matches_plan_exemption, current_file_proof, current_head_sha,
    current_review_dispatch_id_candidate, current_test_plan_artifact_path_for_qa_recording,
    discover_rebuild_candidates, ensure_current_review_dispatch_id,
    load_execution_context_for_exact_plan, load_execution_context_for_mutation,
    load_execution_context_for_rebuild, load_execution_read_scope_for_mutation,
    normalize_begin_request, normalize_complete_request, normalize_note_request,
    normalize_rebuild_evidence_request, normalize_reopen_request, normalize_source,
    normalize_transfer_request, require_normalized_text, require_preflight_acceptance,
    require_prior_task_closure_for_begin, status_from_context_with_shared_routing,
    still_current_task_closure_records, structural_current_task_closure_failures,
    task_closure_baseline_repair_candidate,
    task_closure_negative_result_blocks_current_reviewed_state,
    task_completion_lineage_fingerprint, task_packet_fingerprint,
    usable_current_branch_closure_identity, validate_expected_fingerprint,
};
use crate::execution::transitions::{
    AuthoritativeTransitionState, BranchClosureRecord, CurrentTaskClosureRecord,
    OpenStepStateRecord, StepCommand, claim_step_write_authority, enforce_active_contract_scope,
    enforce_authoritative_phase, load_authoritative_transition_state,
    load_or_initialize_authoritative_transition_state,
};
use crate::execution::workflow_operator_requery_command;
use crate::git::{commit_object_fingerprint, discover_repository};
use crate::paths::{
    harness_authoritative_artifact_path, normalize_repo_relative_path,
    write_atomic as write_atomic_file,
};

#[derive(Debug, Clone, Serialize)]
pub struct CloseCurrentTaskOutput {
    pub action: String,
    pub task_number: u32,
    pub dispatch_validation_action: String,
    pub closure_action: String,
    pub task_closure_status: String,
    pub superseded_task_closure_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closure_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authoritative_next_action: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterializeProjectionsOutput {
    pub action: String,
    pub projection_mode: String,
    pub written_paths: Vec<String>,
    pub runtime_truth_changed: bool,
    pub trace_summary: String,
}

fn close_current_task_already_current_output(
    task_number: u32,
    closure_record_id: String,
    trace_summary: &str,
    mut reason_codes: Vec<String>,
) -> CloseCurrentTaskOutput {
    reason_codes.sort();
    reason_codes.dedup();
    CloseCurrentTaskOutput {
        action: String::from("already_current"),
        task_number,
        dispatch_validation_action: String::from("validated"),
        closure_action: String::from("already_current"),
        task_closure_status: String::from("current"),
        superseded_task_closure_ids: Vec::new(),
        closure_record_id: Some(closure_record_id),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        required_follow_up: None,
        blocking_scope: None,
        blocking_task: None,
        blocking_reason_codes: reason_codes,
        authoritative_next_action: None,
        trace_summary: String::from(trace_summary),
    }
}

fn resolve_already_current_task_closure_postconditions(
    authoritative_state: &mut AuthoritativeTransitionState,
    task_number: u32,
    closure_record_id: &str,
    reviewed_state_id: &str,
) -> Result<Vec<String>, JsonFailure> {
    if resolve_current_task_closure_postconditions(
        authoritative_state,
        task_number,
        closure_record_id,
        reviewed_state_id,
    )? {
        authoritative_state
            .persist_if_dirty_with_failpoint_and_command(None, "close_current_task")?;
        return Ok(vec![String::from(
            "current_task_closure_postconditions_resolved",
        )]);
    }
    Ok(Vec::new())
}

fn current_positive_closure_matches_incoming_results(
    current_record: &CurrentTaskClosureRecord,
    review_result: &str,
    verification_result: &str,
) -> bool {
    current_record.review_result == "pass"
        && current_record.verification_result == "pass"
        && review_result == "pass"
        && verification_result == "pass"
        && current_record
            .closure_status
            .as_deref()
            .is_none_or(|status| status == "current")
}

fn task_closure_negative_result_blocks_reviewed_state(
    authoritative_state: &AuthoritativeTransitionState,
    task: u32,
    reviewed_state_id: &str,
) -> bool {
    authoritative_state
        .task_closure_negative_result(task)
        .is_some_and(|negative_record| {
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_record
                    .semantic_reviewed_state_id
                    .as_deref()
                    .unwrap_or(negative_record.reviewed_state_id.as_str()),
                Some(reviewed_state_id),
            )
        })
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordBranchClosureOutput {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    pub superseded_branch_closure_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvanceLateStageOutput {
    pub action: String,
    pub stage_path: String,
    pub delegated_primitive: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordQaOutput {
    pub action: String,
    pub branch_closure_id: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransferOutput {
    pub action: String,
    pub scope: String,
    pub to: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    pub trace_summary: String,
}

struct CurrentFinalReviewAuthorityCheck<'a> {
    branch_closure_id: &'a str,
    dispatch_id: &'a str,
    reviewer_source: &'a str,
    reviewer_id: &'a str,
    result: &'a str,
    normalized_summary_hash: &'a str,
}

struct EquivalentFinalReviewRerunParams<'a> {
    stage_path: &'a str,
    delegated_primitive: &'a str,
    dispatch_id: &'a str,
    reviewer_source: &'a str,
    reviewer_id: &'a str,
    result: &'a str,
    summary_file: &'a Path,
    required_follow_up: Option<String>,
}

struct ResolvedFinalReviewEvidence {
    base_branch: String,
    deviations_required: bool,
}

struct BlockedCloseCurrentTaskOutputContext<'a> {
    task_number: u32,
    dispatch_validation_action: &'a str,
    task_closure_status: &'a str,
    closure_record_id: Option<String>,
    code: Option<String>,
    recommended_command: Option<String>,
    rederive_via_workflow_operator: Option<bool>,
    required_follow_up: Option<String>,
    trace_summary: &'a str,
}

fn consume_execution_reentry_repair_follow_up(
    authoritative_state: Option<&mut AuthoritativeTransitionState>,
) -> Result<bool, JsonFailure> {
    let Some(authoritative_state) = authoritative_state else {
        return Ok(false);
    };
    if authoritative_state
        .review_state_repair_follow_up_record()
        .is_none_or(|record| record.kind.public_token() != "execution_reentry")
    {
        return Ok(false);
    }
    authoritative_state.set_review_state_repair_follow_up(None)?;
    authoritative_state.set_harness_phase_executing()?;
    Ok(true)
}

fn current_open_step_authoritative_sequence(context: &ExecutionContext) -> u64 {
    load_status_authoritative_overlay_checked(context)
        .ok()
        .flatten()
        .and_then(|overlay| {
            overlay
                .latest_authoritative_sequence
                .or(overlay.authoritative_sequence)
        })
        .unwrap_or(1)
}

fn open_step_state_record(
    context: &ExecutionContext,
    task: u32,
    step: u32,
    note_state: crate::execution::state::NoteState,
    note_summary: &str,
) -> OpenStepStateRecord {
    OpenStepStateRecord {
        task,
        step,
        note_state: note_state.as_str().to_owned(),
        note_summary: truncate_summary(note_summary),
        execution_mode: Some(context.plan_document.execution_mode.clone()),
        repo_root: Some(context.runtime.repo_root.to_string_lossy().into_owned()),
        source_plan_path: context.plan_rel.clone(),
        source_plan_revision: context.plan_document.plan_revision,
        authoritative_sequence: current_open_step_authoritative_sequence(context),
    }
}

fn begin_failure_class_from_blocking_reason_codes(reason_codes: &[String]) -> FailureClass {
    if reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_current_closure_reviewed_state_malformed")
    {
        return FailureClass::MalformedExecutionState;
    }
    if reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "current_task_closure_overlay_restore_required"
                | "prior_task_current_closure_missing"
                | "prior_task_current_closure_stale"
                | "prior_task_current_closure_invalid"
                | "prior_task_review_dispatch_missing"
                | "prior_task_review_dispatch_stale"
                | "prior_task_review_not_green"
                | "prior_task_verification_missing"
                | "prior_task_verification_missing_legacy"
                | "task_review_not_independent"
                | "task_review_receipt_malformed"
                | "task_verification_receipt_malformed"
                | "task_cycle_break_active"
        )
    }) {
        return FailureClass::ExecutionStateNotReady;
    }
    FailureClass::InvalidStepTransition
}

fn begin_failure_class_from_status(status: &PlanExecutionStatus) -> FailureClass {
    let reason_codes = begin_failure_reason_codes(status);
    begin_failure_class_from_blocking_reason_codes(&reason_codes)
}

fn begin_failure_reason_codes(status: &PlanExecutionStatus) -> Vec<String> {
    let mut reason_codes = status.blocking_reason_codes.clone();
    for reason_code in &status.reason_codes {
        if !reason_codes.iter().any(|existing| existing == reason_code) {
            reason_codes.push(reason_code.clone());
        }
    }
    reason_codes
}

pub fn begin(
    runtime: &ExecutionRuntime,
    args: &BeginArgs,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let request = normalize_begin_request(args);
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, &args.plan)?;
    validate_expected_fingerprint(&context, &request.expect_execution_fingerprint)?;
    require_preflight_acceptance(&context)?;
    let mut authoritative_state =
        Some(load_or_initialize_authoritative_transition_state(&context)?);
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Begin)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Begin,
        request.task,
        request.step,
    )?;
    let begin_status = status_from_context_with_shared_routing(runtime, &context, false)?;

    let step_index = step_index(&context, request.task, request.step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested task/step does not exist in the approved plan.",
        )
    })?;
    match context.plan_document.execution_mode.as_str() {
        "none" => match request.execution_mode.as_deref() {
            Some("featureforge:executing-plans" | "featureforge:subagent-driven-development") => {
                context.plan_document.execution_mode = request.execution_mode.unwrap();
            }
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::InvalidExecutionMode,
                    "The first begin for a plan revision must supply a valid execution mode.",
                ));
            }
        },
        existing_mode => {
            if request
                .execution_mode
                .as_deref()
                .is_some_and(|candidate| candidate != existing_mode)
            {
                return Err(JsonFailure::new(
                    FailureClass::InvalidExecutionMode,
                    "begin may not change the persisted execution mode.",
                ));
            }
        }
    }

    if let Some(active) = context
        .steps
        .iter()
        .find(|step| step.note_state == Some(crate::execution::state::NoteState::Active))
    {
        if active.task_number == request.task && active.step_number == request.step {
            let would_consume_execution_reentry_follow_up =
                authoritative_state.as_ref().is_some_and(|state| {
                    state
                        .review_state_repair_follow_up_record()
                        .is_some_and(|record| record.kind.public_token() == "execution_reentry")
                });
            if would_consume_execution_reentry_follow_up {
                require_public_mutation(
                    &begin_status,
                    PublicMutationRequest {
                        kind: PublicMutationKind::Begin,
                        task: Some(request.task),
                        step: Some(request.step),
                        transfer_mode: None,
                        transfer_scope: None,
                        command_name: "begin",
                    },
                    begin_failure_class_from_status(&begin_status),
                )?;
            }
            let consumed_execution_reentry_follow_up =
                consume_execution_reentry_repair_follow_up(authoritative_state.as_mut())?;
            if consumed_execution_reentry_follow_up
                && let Some(authoritative_state) = authoritative_state.as_mut()
            {
                authoritative_state.persist_if_dirty_with_failpoint_and_command(None, "begin")?;
                let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
                return status_with_shared_routing_or_context(runtime, &args.plan, &reloaded);
            }
            return status_with_shared_routing_or_context(runtime, &args.plan, &context);
        }
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "A different step is already active.",
        ));
    }

    require_public_mutation(
        &begin_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Begin,
            task: Some(request.task),
            step: Some(request.step),
            transfer_mode: None,
            transfer_scope: None,
            command_name: "begin",
        },
        begin_failure_class_from_status(&begin_status),
    )?;
    if context.steps[step_index].checked {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "begin may not target a completed step.",
        ));
    }
    require_prior_task_closure_for_begin(&context, request.task)?;
    let blocked_step = context
        .steps
        .iter()
        .find(|step| step.note_state == Some(crate::execution::state::NoteState::Blocked));
    let resuming_blocked_same_step = blocked_step.is_some_and(|blocked| {
        blocked.task_number == request.task && blocked.step_number == request.step
    });
    if blocked_step.is_some() && !resuming_blocked_same_step {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "begin may not bypass existing blocked work.",
        ));
    }
    if let Some(interrupted_step_index) = context
        .steps
        .iter()
        .position(|step| step.note_state == Some(crate::execution::state::NoteState::Interrupted))
    {
        let interrupted_task = context.steps[interrupted_step_index].task_number;
        let interrupted_step = context.steps[interrupted_step_index].step_number;
        if interrupted_task != request.task || interrupted_step != request.step {
            let interrupted_is_earlier = interrupted_task < request.task
                || (interrupted_task == request.task && interrupted_step < request.step);
            if interrupted_is_earlier {
                return Err(JsonFailure::new(
                    FailureClass::InvalidStepTransition,
                    "Interrupted work must resume on the same step.",
                ));
            }
            // A later parked marker cannot mask an earlier stale-boundary reopen/begin target.
            context.steps[interrupted_step_index].note_state = None;
            context.steps[interrupted_step_index].note_summary.clear();
        }
    }

    let projection_write_mode = normal_projection_write_mode()?;
    context.steps[step_index].note_state = Some(crate::execution::state::NoteState::Active);
    context.steps[step_index].note_summary = truncate_summary(&require_normalized_text(
        &context.steps[step_index].title,
        FailureClass::InvalidCommandInput,
        "Execution note summaries may not be blank after whitespace normalization.",
    )?);
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.record_open_step_state(open_step_state_record(
            &context,
            request.task,
            request.step,
            crate::execution::state::NoteState::Active,
            &context.steps[step_index].note_summary,
        ))?;
        authoritative_state.ensure_initial_dispatch_strategy_checkpoint(
            &context,
            &context.plan_document.execution_mode,
        )?;
        consume_execution_reentry_repair_follow_up(Some(authoritative_state))?;
    }

    let rendered = render_execution_projections(&context);
    record_execution_projection_fingerprints(authoritative_state.as_mut(), &rendered)?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            "begin",
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "begin_after_plan_write_before_authoritative_state_publish",
        )?;
    }
    write_execution_projection_read_models(&context, &rendered, projection_write_mode)?;
    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}

pub fn complete(
    runtime: &ExecutionRuntime,
    args: &CompleteArgs,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let request = normalize_complete_request(args)?;
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, &args.plan)?;
    validate_expected_fingerprint(&context, &request.expect_execution_fingerprint)?;
    normalize_or_seed_source(&request.source, &mut context.plan_document.execution_mode)?;
    let mut authoritative_state =
        Some(load_or_initialize_authoritative_transition_state(&context)?);
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Complete)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Complete,
        request.task,
        request.step,
    )?;
    let complete_status = status_from_context_with_shared_routing(runtime, &context, false)?;
    require_public_mutation(
        &complete_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Complete,
            task: Some(request.task),
            step: Some(request.step),
            transfer_mode: None,
            transfer_scope: None,
            command_name: "complete",
        },
        FailureClass::ExecutionStateNotReady,
    )?;
    let provenance = authoritative_state
        .as_ref()
        .map(|state| state.evidence_provenance())
        .unwrap_or_default();

    let step_index = step_index(&context, request.task, request.step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested task/step does not exist in the approved plan.",
        )
    })?;
    if context.steps[step_index].note_state != Some(crate::execution::state::NoteState::Active) {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "complete may target only the current active step.",
        ));
    }
    if context.steps[step_index].checked {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "complete may not directly refresh an already checked step.",
        ));
    }

    let files = if request.files.is_empty() {
        default_files_for_task(&context, request.task)
    } else {
        canonicalize_files(&request.files)?
    };
    let files = canonicalize_repo_visible_paths(&context.runtime.repo_root, &files)?;
    let file_proofs = files
        .iter()
        .map(|path| FileProof {
            path: path.clone(),
            proof: current_file_proof(&context.runtime.repo_root, path),
        })
        .collect::<Vec<_>>();

    context.steps[step_index].checked = true;
    context.steps[step_index].note_state = None;
    context.steps[step_index].note_summary.clear();
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.clear_open_step_state()?;
    }

    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let packet_fingerprint = task_packet_fingerprint(
        &context,
        &source_spec_fingerprint,
        request.task,
        request.step,
    )
    .ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "complete could not determine semantic task contract identity for task {}.",
                request.task
            ),
        )
    })?;
    let projection_write_mode = normal_projection_write_mode()?;
    let recorded_at = Timestamp::now().to_string();
    let head_sha = context.current_head_sha()?;
    let new_attempt = EvidenceAttempt {
        task_number: request.task,
        step_number: request.step,
        attempt_number: next_attempt_number(&context.evidence, request.task, request.step),
        status: String::from("Completed"),
        recorded_at,
        execution_source: request.source.clone(),
        claim: request.claim,
        files: files.clone(),
        file_proofs,
        verify_command: request.verify_command,
        verification_summary: request.verification_summary,
        invalidation_reason: String::from("N/A"),
        packet_fingerprint: Some(packet_fingerprint),
        head_sha: Some(head_sha.clone()),
        base_sha: Some(head_sha),
        source_contract_path: provenance.source_contract_path,
        source_contract_fingerprint: provenance.source_contract_fingerprint,
        source_evaluation_report_fingerprint: provenance.source_evaluation_report_fingerprint,
        evaluator_verdict: provenance.evaluator_verdict,
        failing_criterion_ids: provenance.failing_criterion_ids,
        source_handoff_fingerprint: provenance.source_handoff_fingerprint,
        repo_state_baseline_head_sha: provenance.repo_state_baseline_head_sha,
        repo_state_baseline_worktree_fingerprint: provenance
            .repo_state_baseline_worktree_fingerprint,
    };

    context.evidence.attempts.push(new_attempt);
    context.evidence.format = crate::execution::state::EvidenceFormat::V2;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.record_execution_evidence_attempts(&context)?;
    }

    let rendered = render_execution_projections(&context);
    record_execution_projection_fingerprints(authoritative_state.as_mut(), &rendered)?;
    let _ = consume_execution_reentry_repair_follow_up(authoritative_state.as_mut())?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_step_hint_and_rollback(
            authoritative_state,
            "complete",
            Some((request.task, request.step)),
            AuthoritativePersistRollback {
                plan_path: &context.plan_abs,
                original_plan: &context.plan_source,
                evidence_path: &context.evidence_abs,
                failpoint: "complete_after_plan_and_evidence_write_before_authoritative_state_publish",
            },
        )?;
    }
    maybe_trigger_failpoint("complete_after_plan_write")?;
    write_execution_projection_read_models(&context, &rendered, projection_write_mode)?;
    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}

pub fn note(
    runtime: &ExecutionRuntime,
    args: &NoteArgs,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let request = normalize_note_request(args)?;
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, &args.plan)?;
    validate_expected_fingerprint(&context, &request.expect_execution_fingerprint)?;
    let mut authoritative_state =
        Some(load_or_initialize_authoritative_transition_state(&context)?);
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Note)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Note,
        request.task,
        request.step,
    )?;

    let step_index = step_index(&context, request.task, request.step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested task/step does not exist in the approved plan.",
        )
    })?;
    if context.steps[step_index].note_state != Some(crate::execution::state::NoteState::Active) {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "note may target only the current active step.",
        ));
    }
    if context.steps[step_index].checked {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "note may not target a completed step.",
        ));
    }

    let projection_write_mode = normal_projection_write_mode()?;
    context.steps[step_index].note_state = Some(request.state);
    context.steps[step_index].note_summary = request.message;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.record_open_step_state(open_step_state_record(
            &context,
            request.task,
            request.step,
            request.state,
            &context.steps[step_index].note_summary,
        ))?;
        authoritative_state.apply_note_reset_policy(request.state)?;
    }

    let rendered = render_execution_projections(&context);
    record_execution_projection_fingerprints(authoritative_state.as_mut(), &rendered)?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            "note",
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "note_after_plan_write_before_authoritative_state_publish",
        )?;
    }
    write_execution_projection_read_models(&context, &rendered, projection_write_mode)?;
    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}

pub fn reopen(
    runtime: &ExecutionRuntime,
    args: &ReopenArgs,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let request = normalize_reopen_request(args)?;
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, &args.plan)?;
    validate_expected_fingerprint(&context, &request.expect_execution_fingerprint)?;
    normalize_or_seed_source(&request.source, &mut context.plan_document.execution_mode)?;
    let mut authoritative_state =
        Some(load_or_initialize_authoritative_transition_state(&context)?);
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Reopen)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Reopen,
        request.task,
        request.step,
    )?;
    let step_index = step_index(&context, request.task, request.step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested task/step does not exist in the approved plan.",
        )
    })?;
    let reopen_status = status_from_context_with_shared_routing(runtime, &context, false)?;
    require_public_mutation(
        &reopen_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Reopen,
            task: Some(request.task),
            step: Some(request.step),
            transfer_mode: None,
            transfer_scope: None,
            command_name: "reopen",
        },
        FailureClass::ExecutionStateNotReady,
    )?;
    if let Some(existing_interrupted_index) = context
        .steps
        .iter()
        .position(|step| step.note_state == Some(crate::execution::state::NoteState::Interrupted))
    {
        let existing_task = context.steps[existing_interrupted_index].task_number;
        let existing_step = context.steps[existing_interrupted_index].step_number;
        if existing_task == request.task && existing_step == request.step {
            return status_with_shared_routing_or_context(runtime, &args.plan, &context);
        }
        let existing_is_earlier = existing_task < request.task
            || (existing_task == request.task && existing_step < request.step);
        if existing_is_earlier {
            return Err(JsonFailure::new(
                FailureClass::InvalidStepTransition,
                "reopen may not create a second parked interrupted step while one already exists.",
            ));
        }
        // Clear a later parked marker before reopening an earlier stale-boundary target.
        context.steps[existing_interrupted_index].note_state = None;
        context.steps[existing_interrupted_index]
            .note_summary
            .clear();
    }
    let authoritative_task_closure_current = authoritative_state
        .as_ref()
        .and_then(|state| state.current_task_closure_result(request.task))
        .is_some();
    if !context.steps[step_index].checked && !authoritative_task_closure_current {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "reopen may target only a completed step.",
        ));
    }

    let projection_write_mode = normal_projection_write_mode()?;
    invalidate_latest_completed_attempt(&mut context, request.task, request.step, &request.reason)?;
    context.steps[step_index].checked = false;
    context.steps[step_index].note_state = Some(crate::execution::state::NoteState::Interrupted);
    context.steps[step_index].note_summary = truncate_summary(&request.reason);
    context.evidence.format = crate::execution::state::EvidenceFormat::V2;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.record_open_step_state(open_step_state_record(
            &context,
            request.task,
            request.step,
            crate::execution::state::NoteState::Interrupted,
            &context.steps[step_index].note_summary,
        ))?;
        authoritative_state
            .clear_current_task_closure_results_for_execution_reentry([request.task])?;
        authoritative_state.stale_reopen_provenance()?;
        authoritative_state.record_reopen_strategy_checkpoint(
            &context,
            &context.plan_document.execution_mode,
            request.task,
            request.step,
            &request.reason,
        )?;
        if authoritative_state.strategy_checkpoint_kind() != Some("cycle_break") {
            authoritative_state.set_harness_phase_executing()?;
        }
        authoritative_state.record_execution_evidence_attempts(&context)?;
        consume_execution_reentry_repair_follow_up(Some(authoritative_state))?;
    }

    let rendered = render_execution_projections(&context);
    record_execution_projection_fingerprints(authoritative_state.as_mut(), &rendered)?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            "reopen",
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "reopen_after_plan_and_evidence_write_before_authoritative_state_publish",
        )?;
    }
    maybe_trigger_failpoint("reopen_after_plan_write")?;
    write_execution_projection_read_models(&context, &rendered, projection_write_mode)?;

    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}

fn normalize_or_seed_source(source: &str, execution_mode: &mut String) -> Result<(), JsonFailure> {
    match source {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::InvalidExecutionMode,
                "Execution source must be one of the supported execution modes.",
            ));
        }
    }
    if execution_mode == "none" {
        *execution_mode = source.to_owned();
        return Ok(());
    }
    normalize_source(source, execution_mode)
}

pub fn transfer(runtime: &ExecutionRuntime, args: &TransferArgs) -> Result<Value, JsonFailure> {
    let request = normalize_transfer_request(args)?;
    match request.mode {
        crate::execution::state::TransferRequestMode::RepairStep {
            repair_task,
            repair_step,
            source,
            expect_execution_fingerprint,
        } => serde_json::to_value(transfer_repair_step(
            runtime,
            &args.plan,
            repair_task,
            repair_step,
            &source,
            &request.reason,
            &expect_execution_fingerprint,
        )?)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!("Could not serialize transfer legacy output: {error}"),
            )
        }),
        crate::execution::state::TransferRequestMode::WorkflowHandoff { scope, to } => {
            serde_json::to_value(record_workflow_transfer(
                runtime,
                &args.plan,
                &scope,
                &to,
                &request.reason,
            )?)
            .map_err(|error| {
                JsonFailure::new(
                    FailureClass::PartialAuthoritativeMutation,
                    format!("Could not serialize transfer output: {error}"),
                )
            })
        }
    }
}

fn transfer_repair_step(
    runtime: &ExecutionRuntime,
    plan: &Path,
    repair_task: u32,
    repair_step: u32,
    source: &str,
    reason: &str,
    expect_execution_fingerprint: &str,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, plan)?;
    validate_expected_fingerprint(&context, expect_execution_fingerprint)?;
    normalize_source(source, &context.plan_document.execution_mode)?;
    let transfer_status = status_from_context_with_shared_routing(runtime, &context, false)?;
    require_public_mutation(
        &transfer_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Transfer,
            task: Some(repair_task),
            step: Some(repair_step),
            transfer_mode: Some(PublicTransferMode::RepairStep),
            transfer_scope: None,
            command_name: "transfer",
        },
        FailureClass::ExecutionStateNotReady,
    )?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Transfer)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Transfer,
        repair_task,
        repair_step,
    )?;

    let active_index = context
        .steps
        .iter()
        .position(|step| step.note_state == Some(crate::execution::state::NoteState::Active))
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidStepTransition,
                "transfer requires a current active step.",
            )
        })?;
    if context
        .steps
        .iter()
        .any(|step| step.note_state == Some(crate::execution::state::NoteState::Interrupted))
    {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "transfer may not create a second parked interrupted step while one already exists.",
        ));
    }

    let repair_index = step_index(&context, repair_task, repair_step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested repair task/step does not exist in the approved plan.",
        )
    })?;
    if !context.steps[repair_index].checked {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "transfer may target only a completed repair step.",
        ));
    }

    let projection_write_mode = normal_projection_write_mode()?;
    invalidate_latest_completed_attempt(&mut context, repair_task, repair_step, reason)?;
    context.steps[repair_index].checked = false;
    context.steps[repair_index].note_state = None;
    context.steps[repair_index].note_summary.clear();
    context.steps[active_index].note_state = Some(crate::execution::state::NoteState::Interrupted);
    context.steps[active_index].note_summary = truncate_summary(&format!(
        "Parked for repair of Task {repair_task} Step {repair_step}"
    ));
    context.evidence.format = crate::execution::state::EvidenceFormat::V2;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.record_open_step_state(open_step_state_record(
            &context,
            context.steps[active_index].task_number,
            context.steps[active_index].step_number,
            crate::execution::state::NoteState::Interrupted,
            &context.steps[active_index].note_summary,
        ))?;
    }

    let rendered = render_execution_projections(&context);
    record_execution_projection_fingerprints(authoritative_state.as_mut(), &rendered)?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            "transfer",
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "transfer_after_plan_and_evidence_write_before_authoritative_state_publish",
        )?;
    }
    maybe_trigger_failpoint("transfer_after_plan_write")?;
    write_execution_projection_read_models(&context, &rendered, projection_write_mode)?;

    let reloaded = load_execution_context_for_mutation(runtime, plan)?;
    status_with_shared_routing_or_context(runtime, plan, &reloaded)
}

fn status_with_shared_routing_or_context(
    runtime: &ExecutionRuntime,
    plan: &Path,
    fallback_context: &ExecutionContext,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let unsanitized_post_status =
        status_from_context_with_shared_routing(runtime, fallback_context, false).ok();
    if let Some(status) = unsanitized_post_status.as_ref() {
        enforce_post_mutation_shared_status_invariants(status)?;
    }
    let args = StatusArgs {
        plan: plan.to_path_buf(),
        external_review_result_ready: false,
    };
    match runtime.status(&args) {
        Ok(status) => {
            if unsanitized_post_status.is_none() {
                enforce_post_mutation_shared_status_invariants(&status)?;
            }
            enforce_post_mutation_semantic_workspace_invariant(
                fallback_context,
                unsanitized_post_status.as_ref(),
                &status,
            )?;
            Ok(status)
        }
        Err(error) => {
            let legacy_pre_harness_failure = error.error_class
                == FailureClass::MalformedExecutionState.as_str()
                && error
                    .message
                    .contains("Legacy pre-harness execution evidence is no longer accepted");
            let malformed_preflight_seed_failure = error.error_class
                == FailureClass::MalformedExecutionState.as_str()
                && error
                    .message
                    .contains("Persisted execution preflight acceptance");
            let exact_command_derivation_failure = error.error_class
                == FailureClass::MalformedExecutionState.as_str()
                && error
                    .message
                    .contains("could not derive the exact execution command");
            if legacy_pre_harness_failure
                || (malformed_preflight_seed_failure
                    && load_authoritative_transition_state(fallback_context)?
                        .as_ref()
                        .and_then(|state| state.execution_run_id_opt())
                        .is_some())
                || exact_command_derivation_failure
            {
                let fallback_status =
                    status_from_context_with_shared_routing(runtime, fallback_context, false)?;
                enforce_post_mutation_status_invariants(
                    fallback_context,
                    &fallback_status,
                    unsanitized_post_status.as_ref(),
                )?;
                return Ok(fallback_status);
            }
            Err(error)
        }
    }
}

fn enforce_post_mutation_status_invariants(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    baseline_status: Option<&PlanExecutionStatus>,
) -> Result<(), JsonFailure> {
    enforce_post_mutation_shared_status_invariants(status)?;
    enforce_post_mutation_semantic_workspace_invariant(context, baseline_status, status)
}

fn enforce_post_mutation_shared_status_invariants(
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    let injected_status =
        if std::env::var("FEATUREFORGE_PLAN_EXECUTION_POST_MUTATION_INVARIANT_TEST_INJECTION")
            .is_ok()
        {
            let mut injected_status = status.clone();
            crate::execution::invariants::inject_post_mutation_invariant_test_violation(
                &mut injected_status,
            );
            Some(injected_status)
        } else {
            None
        };
    let status = injected_status.as_ref().unwrap_or(status);
    let violations =
        check_runtime_status_invariants(status, InvariantEnforcementMode::PostMutation);
    if !violations.is_empty() {
        let details = violations
            .iter()
            .map(|violation| format!("{}: {}", violation.code, violation.detail))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Post-mutation invariant violated: {details}"),
        ));
    }
    Ok(())
}

fn enforce_post_mutation_semantic_workspace_invariant(
    context: &ExecutionContext,
    baseline_status: Option<&PlanExecutionStatus>,
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    if let Some(baseline_status) = baseline_status {
        let semantic_workspace_changed =
            baseline_status.semantic_workspace_tree_id != status.semantic_workspace_tree_id;
        if semantic_workspace_changed
            && semantic_changed_paths_between_statuses(context, baseline_status, status)?.is_empty()
        {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Post-mutation invariant violated: semantic workspace identity changed without semantic repo-content changes.",
            ));
        }
    }
    Ok(())
}

fn semantic_changed_paths_between_statuses(
    context: &ExecutionContext,
    baseline_status: &PlanExecutionStatus,
    status: &PlanExecutionStatus,
) -> Result<Vec<String>, JsonFailure> {
    let Some(baseline_raw_tree) = raw_workspace_tree_sha(baseline_status) else {
        return Ok(Vec::new());
    };
    let Some(current_raw_tree) = raw_workspace_tree_sha(status) else {
        return Ok(Vec::new());
    };
    semantic_paths_changed_between_raw_trees(context, baseline_raw_tree, current_raw_tree)
}

fn raw_workspace_tree_sha(status: &PlanExecutionStatus) -> Option<&str> {
    status
        .raw_workspace_tree_id
        .as_deref()
        .and_then(|value| value.strip_prefix("git_tree:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn record_workflow_transfer(
    runtime: &ExecutionRuntime,
    plan: &Path,
    scope: &str,
    to: &str,
    reason: &str,
) -> Result<TransferOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context_for_mutation(runtime, plan)?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "transfer requires authoritative harness state.",
        ));
    };
    let status = status_with_shared_routing_or_context(runtime, plan, &context)?;
    require_public_mutation(
        &status,
        PublicMutationRequest {
            kind: PublicMutationKind::Transfer,
            task: None,
            step: None,
            transfer_mode: Some(PublicTransferMode::WorkflowHandoff),
            transfer_scope: Some(scope.to_owned()),
            command_name: "transfer",
        },
        FailureClass::ExecutionStateNotReady,
    )?;
    let operator = current_workflow_operator(runtime, plan, false)?;
    let head_sha = current_head_sha(&runtime.repo_root)?;
    let decision_scope = shared_handoff_decision_scope(
        status.active_task,
        status.blocking_task,
        status.resume_task,
        status.handoff_required,
        Some(status.harness_phase),
    );
    let identity = WorkflowTransferRecordIdentity {
        repo_slug: &runtime.repo_slug,
        safe_branch: &runtime.safe_branch,
        plan_path: &context.plan_rel,
        branch_name: &runtime.branch_name,
        head_sha: &head_sha,
        decision_reason_codes: &status.reason_codes,
        decision_scope,
    };
    let input = WorkflowTransferRecordInput { scope, to, reason };
    let operator_routes_handoff = operator.phase_detail == "handoff_recording_required"
        && matches!(operator.phase.as_str(), "handoff_required" | "executing");
    if !operator_routes_handoff {
        return Ok(TransferOutput {
            action: String::from("blocked"),
            scope: scope.to_owned(),
            to: to.to_owned(),
            reason: reason.to_owned(),
            record_path: None,
            code: Some(String::from("out_of_phase_requery_required")),
            recommended_command: Some(recommended_operator_command(plan, false)),
            rederive_via_workflow_operator: Some(true),
            trace_summary: String::from(
                "transfer failed closed because workflow/operator does not currently route to handoff recording.",
            ),
        });
    }
    if decision_scope.is_some_and(|expected_scope| scope != expected_scope) {
        return Ok(TransferOutput {
            action: String::from("blocked"),
            scope: scope.to_owned(),
            to: to.to_owned(),
            reason: reason.to_owned(),
            record_path: None,
            code: None,
            recommended_command: decision_scope.map(|expected_scope| {
                format!(
                    "featureforge plan execution transfer --plan {} --scope {expected_scope} --to <owner> --reason <reason>",
                    context.plan_rel
                )
            }),
            rederive_via_workflow_operator: None,
            trace_summary: String::from(
                "transfer failed closed because the requested scope does not satisfy the current handoff decision scope.",
            ),
        });
    }

    if let Some(existing_record) =
        latest_matching_workflow_transfer_request_record(&runtime.state_dir, identity, input)
    {
        let existing_source = fs::read_to_string(&existing_record).map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not read existing workflow transfer record {}: {error}",
                    existing_record.display()
                ),
            )
        })?;
        let existing_fingerprint = sha256_hex(existing_source.as_bytes());
        authoritative_state.record_runtime_handoff_checkpoint(
            &existing_record.display().to_string(),
            &existing_fingerprint,
        )?;
        authoritative_state.persist_if_dirty_with_failpoint_and_command(None, "transfer")?;
        return Ok(TransferOutput {
            action: String::from("already_current"),
            scope: scope.to_owned(),
            to: to.to_owned(),
            reason: reason.to_owned(),
            record_path: Some(existing_record.display().to_string()),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            trace_summary: String::from(
                "The current handoff decision already has an equivalent recorded workflow transfer checkpoint.",
            ),
        });
    }

    if let Some(record_path) = current_workflow_transfer_record_path(&runtime.state_dir, identity) {
        return Ok(TransferOutput {
            action: String::from("blocked"),
            scope: scope.to_owned(),
            to: to.to_owned(),
            reason: reason.to_owned(),
            record_path: Some(record_path.display().to_string()),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            trace_summary: String::from(
                "A different workflow transfer checkpoint is already current for this handoff decision.",
            ),
        });
    }

    let (record_path, record_fingerprint) =
        write_workflow_transfer_record(&runtime.state_dir, identity, input)?;
    authoritative_state.record_runtime_handoff_checkpoint(
        &record_path.display().to_string(),
        &record_fingerprint,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint_and_command(None, "transfer")?;

    Ok(TransferOutput {
        action: String::from("recorded"),
        scope: scope.to_owned(),
        to: to.to_owned(),
        reason: reason.to_owned(),
        record_path: Some(record_path.display().to_string()),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        trace_summary: String::from(
            "Recorded a runtime-owned workflow transfer checkpoint and cleared the current handoff override.",
        ),
    })
}

fn require_close_current_task_public_mutation(
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<(), JsonFailure> {
    require_public_mutation(
        status,
        PublicMutationRequest {
            kind: PublicMutationKind::CloseCurrentTask,
            task: Some(task),
            step: None,
            transfer_mode: None,
            transfer_scope: None,
            command_name: "close-current-task",
        },
        FailureClass::ExecutionStateNotReady,
    )
}

fn close_current_task_public_mutation_allowed(status: &PlanExecutionStatus, task: u32) -> bool {
    decide_public_mutation(
        status,
        &PublicMutationRequest {
            kind: PublicMutationKind::CloseCurrentTask,
            task: Some(task),
            step: None,
            transfer_mode: None,
            transfer_scope: None,
            command_name: "close-current-task",
        },
    )
    .allowed
}

pub fn close_current_task(
    runtime: &ExecutionRuntime,
    args: &CloseCurrentTaskArgs,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    let initial_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let authoritative_execution_run_id = load_authoritative_transition_state(&initial_context)?
        .as_ref()
        .and_then(|state| state.execution_run_id_opt());
    let status = status_with_shared_routing_or_context(runtime, &args.plan, &initial_context)?;
    let execution_run_id = authoritative_execution_run_id
        .or_else(|| status.execution_run_id.as_ref().map(|value| value.0.clone()))
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires an active execution run identity from authoritative transition state or preflight seed state.",
            )
        })?;
    let verification_result = args.verification_result.as_str();
    let initial_reviewed_state_id = current_task_reviewed_state_id(&initial_context, args.task)?;
    let initial_raw_reviewed_state_id =
        current_task_raw_reviewed_state_id(&initial_context, args.task)?;
    let initial_closure_record_id = current_task_closure_record_id(&initial_context, args.task)?;
    let candidate_dispatch_id = current_review_dispatch_id_candidate(
        &initial_context,
        ReviewDispatchScopeArg::Task,
        Some(args.task),
        args.dispatch_id.as_deref(),
    )?;
    let closure_baseline_repair_candidate =
        task_closure_baseline_repair_candidate(&initial_context, &status, args.task)?;
    let projection_refresh_only_candidate = closure_baseline_repair_candidate
        .as_ref()
        .is_some_and(|candidate| candidate.projection_refresh_only);
    if let Some(dispatch_id) = candidate_dispatch_id.as_deref() {
        ensure_task_dispatch_id_matches(&initial_context, args.task, dispatch_id)?;
        match task_dispatch_reviewed_state_status(
            &initial_context,
            args.task,
            &initial_reviewed_state_id,
            &initial_raw_reviewed_state_id,
        )? {
            TaskDispatchReviewedStateStatus::Current => {}
            TaskDispatchReviewedStateStatus::MissingReviewedStateBinding => {
                return Ok(blocked_close_current_task_output(
                    BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "blocked",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: Some(String::from("request_external_review")),
                        trace_summary: "close-current-task failed closed because the current task review dispatch lineage does not bind a current reviewed state.",
                    },
                ));
            }
            TaskDispatchReviewedStateStatus::StaleReviewedState => {
                return Ok(blocked_close_current_task_output(
                    BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "blocked",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: Some(String::from("execution_reentry")),
                        trace_summary: "close-current-task failed closed because tracked workspace state changed after the current task review dispatch was recorded.",
                    },
                ));
            }
        }
        let mut authoritative_state = load_authoritative_transition_state(&initial_context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires authoritative harness state.",
            ));
        };
        if task_closure_negative_result_blocks_reviewed_state(
            authoritative_state,
            args.task,
            &initial_reviewed_state_id,
        ) {
            let operator = current_workflow_operator(runtime, &args.plan, true)?;
            let (required_follow_up, recommended_command) =
                close_current_task_follow_up_and_command(&operator);
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "validated",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command,
                    rederive_via_workflow_operator: None,
                    required_follow_up,
                    trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                }),
                &operator,
            ));
        }
        if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
            && current_record.closure_record_id == initial_closure_record_id
            && current_record.dispatch_id == dispatch_id
        {
            let (review_summary_hash, verification_summary_hash) =
                close_current_task_summary_hashes(args)?;
            if current_record.review_result == args.review_result.as_str()
                && current_record.review_summary_hash == review_summary_hash.as_str()
                && current_record.verification_result == verification_result
                && current_record.verification_summary_hash == verification_summary_hash.as_str()
            {
                if !projection_refresh_only_candidate {
                    let postconditions_would_mutate =
                        current_task_closure_postconditions_would_mutate(
                            authoritative_state,
                            args.task,
                            &initial_closure_record_id,
                            &current_record.reviewed_state_id,
                        );
                    let reason_codes = if postconditions_would_mutate
                        && close_current_task_public_mutation_allowed(&status, args.task)
                    {
                        let _write_authority = claim_step_write_authority(runtime)?;
                        resolve_already_current_task_closure_postconditions(
                            authoritative_state,
                            args.task,
                            &initial_closure_record_id,
                            &current_record.reviewed_state_id,
                        )?
                    } else {
                        Vec::new()
                    };
                    return Ok(close_current_task_already_current_output(
                        args.task,
                        initial_closure_record_id,
                        "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                        reason_codes,
                    ));
                }
            } else if !projection_refresh_only_candidate
                && current_positive_closure_matches_incoming_results(
                    &current_record,
                    args.review_result.as_str(),
                    verification_result,
                )
            {
                let postconditions_would_mutate = current_task_closure_postconditions_would_mutate(
                    authoritative_state,
                    args.task,
                    &initial_closure_record_id,
                    &current_record.reviewed_state_id,
                );
                let mut reason_codes = if postconditions_would_mutate
                    && close_current_task_public_mutation_allowed(&status, args.task)
                {
                    let _write_authority = claim_step_write_authority(runtime)?;
                    resolve_already_current_task_closure_postconditions(
                        authoritative_state,
                        args.task,
                        &initial_closure_record_id,
                        &current_record.reviewed_state_id,
                    )?
                } else {
                    Vec::new()
                };
                reason_codes.push(String::from("summary_hash_drift_ignored"));
                return Ok(close_current_task_already_current_output(
                    args.task,
                    initial_closure_record_id,
                    "Current task already has a positive recorded task closure for the supplied dispatch lineage; summary-only drift was ignored.",
                    reason_codes,
                ));
            } else if !projection_refresh_only_candidate {
                let operator = current_workflow_operator(runtime, &args.plan, true)?;
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(initial_closure_record_id),
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: Some(String::from("execution_reentry")),
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
        }
    }
    let mut summary_hashes = candidate_dispatch_id
        .is_none()
        .then(|| close_current_task_summary_hashes(args))
        .transpose()?;
    let dispatch_id = if let Some(dispatch_id) = candidate_dispatch_id {
        dispatch_id
    } else {
        ensure_current_review_dispatch_id(
            &initial_context,
            ReviewDispatchScopeArg::Task,
            Some(args.task),
            args.dispatch_id.as_deref(),
        )?
    };
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    ensure_task_dispatch_id_matches(&context, args.task, &dispatch_id)?;
    let operator = current_workflow_operator(runtime, &args.plan, true)?;
    let strategy_checkpoint_fingerprint =
        authoritative_strategy_checkpoint_fingerprint_checked(&context)?.ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "close-current-task requires authoritative strategy checkpoint provenance.",
            )
        })?;
    let reviewed_state_id = current_task_reviewed_state_id(&context, args.task)?;
    let raw_reviewed_state_id = current_task_raw_reviewed_state_id(&context, args.task)?;
    let contract_identity = current_task_contract_identity(&context, args.task)?;
    let closure_record_id = current_task_closure_record_id(&context, args.task)?;
    match task_dispatch_reviewed_state_status(
        &context,
        args.task,
        &reviewed_state_id,
        &raw_reviewed_state_id,
    )? {
        TaskDispatchReviewedStateStatus::Current => {}
        TaskDispatchReviewedStateStatus::MissingReviewedStateBinding => {
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "blocked",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: Some(String::from("request_external_review")),
                    trace_summary: "close-current-task failed closed because the current task review dispatch lineage does not bind a current reviewed state.",
                }),
                &operator,
            ));
        }
        TaskDispatchReviewedStateStatus::StaleReviewedState => {
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "blocked",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: Some(String::from("execution_reentry")),
                    trace_summary: "close-current-task failed closed because tracked workspace state changed after the current task review dispatch was recorded.",
                }),
                &operator,
            ));
        }
    }
    if summary_hashes.is_none() {
        summary_hashes = Some(close_current_task_summary_hashes(args)?);
    }
    let (review_summary_hash, verification_summary_hash) = summary_hashes
        .as_ref()
        .expect("summary hashes should exist after summary validation");
    {
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires authoritative harness state.",
            ));
        };
        if task_closure_negative_result_blocks_reviewed_state(
            authoritative_state,
            args.task,
            &reviewed_state_id,
        ) {
            let operator = current_workflow_operator(runtime, &args.plan, true)?;
            let (required_follow_up, recommended_command) =
                close_current_task_follow_up_and_command(&operator);
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "validated",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command,
                    rederive_via_workflow_operator: None,
                    required_follow_up,
                    trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                }),
                &operator,
            ));
        }
        if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
            && current_record.closure_record_id == closure_record_id
            && current_record.dispatch_id == dispatch_id
        {
            if current_record.review_result == args.review_result.as_str()
                && current_record.review_summary_hash == review_summary_hash.as_str()
                && current_record.verification_result == verification_result
                && current_record.verification_summary_hash == verification_summary_hash.as_str()
            {
                if !projection_refresh_only_candidate {
                    let postconditions_would_mutate =
                        current_task_closure_postconditions_would_mutate(
                            authoritative_state,
                            args.task,
                            &closure_record_id,
                            &current_record.reviewed_state_id,
                        );
                    let reason_codes = if postconditions_would_mutate
                        && close_current_task_public_mutation_allowed(&status, args.task)
                    {
                        let _write_authority = claim_step_write_authority(runtime)?;
                        resolve_already_current_task_closure_postconditions(
                            authoritative_state,
                            args.task,
                            &closure_record_id,
                            &current_record.reviewed_state_id,
                        )?
                    } else {
                        Vec::new()
                    };
                    return Ok(close_current_task_already_current_output(
                        args.task,
                        closure_record_id,
                        "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                        reason_codes,
                    ));
                }
            } else if !projection_refresh_only_candidate
                && current_positive_closure_matches_incoming_results(
                    &current_record,
                    args.review_result.as_str(),
                    verification_result,
                )
            {
                let postconditions_would_mutate = current_task_closure_postconditions_would_mutate(
                    authoritative_state,
                    args.task,
                    &closure_record_id,
                    &current_record.reviewed_state_id,
                );
                let mut reason_codes = if postconditions_would_mutate
                    && close_current_task_public_mutation_allowed(&status, args.task)
                {
                    let _write_authority = claim_step_write_authority(runtime)?;
                    resolve_already_current_task_closure_postconditions(
                        authoritative_state,
                        args.task,
                        &closure_record_id,
                        &current_record.reviewed_state_id,
                    )?
                } else {
                    Vec::new()
                };
                reason_codes.push(String::from("summary_hash_drift_ignored"));
                return Ok(close_current_task_already_current_output(
                    args.task,
                    closure_record_id,
                    "Current task already has a positive recorded task closure for the supplied dispatch lineage; summary-only drift was ignored.",
                    reason_codes,
                ));
            } else if !projection_refresh_only_candidate {
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(closure_record_id),
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: Some(String::from("execution_reentry")),
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
        }
    }
    match close_current_task_outcome_class(args.review_result, args.verification_result) {
        CloseCurrentTaskOutcomeClass::Positive => {
            let effective_reviewed_surface_paths =
                current_task_effective_reviewed_surface_paths(&context, args.task)?;
            require_close_current_task_public_mutation(&status, args.task)?;
            refresh_task_closure_projections_with_context(
                runtime,
                &context,
                TaskClosureReceiptRefresh {
                    execution_run_id: &execution_run_id,
                    strategy_checkpoint_fingerprint: &strategy_checkpoint_fingerprint,
                    active_contract_fingerprint: None,
                    task: args.task,
                    claim_write_authority: true,
                },
            )?;
            let _write_authority = claim_step_write_authority(runtime)?;
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            let locked_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            ensure_task_dispatch_id_matches(&locked_context, args.task, &dispatch_id)?;
            if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
                && current_record.closure_record_id == closure_record_id
                && current_record.dispatch_id == dispatch_id
            {
                if current_record.review_result == "pass"
                    && current_record.verification_result == "pass"
                {
                    let mut reason_codes = resolve_already_current_task_closure_postconditions(
                        authoritative_state,
                        args.task,
                        &closure_record_id,
                        &current_record.reviewed_state_id,
                    )?;
                    if current_record.review_summary_hash != review_summary_hash.as_str()
                        || current_record.verification_summary_hash
                            != verification_summary_hash.as_str()
                    {
                        reason_codes.push(String::from("summary_hash_drift_ignored"));
                    }
                    return Ok(close_current_task_already_current_output(
                        args.task,
                        closure_record_id,
                        "Current task closure is already current for this dispatch lineage; refreshed receipt projections from the existing closure baseline.",
                        reason_codes,
                    ));
                }
                if current_record.review_result == args.review_result.as_str()
                    && current_record.review_summary_hash == review_summary_hash.as_str()
                    && current_record.verification_result == verification_result
                    && current_record.verification_summary_hash
                        == verification_summary_hash.as_str()
                {
                    let reason_codes = resolve_already_current_task_closure_postconditions(
                        authoritative_state,
                        args.task,
                        &closure_record_id,
                        &current_record.reviewed_state_id,
                    )?;
                    return Ok(close_current_task_already_current_output(
                        args.task,
                        closure_record_id,
                        "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                        reason_codes,
                    ));
                }
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(closure_record_id),
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: Some(String::from("execution_reentry")),
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            if authoritative_state
                .task_closure_negative_result(args.task)
                .is_some_and(|negative_record| {
                    task_closure_negative_result_blocks_current_reviewed_state(
                        negative_record
                            .semantic_reviewed_state_id
                            .as_deref()
                            .unwrap_or(negative_record.reviewed_state_id.as_str()),
                        Some(reviewed_state_id.as_str()),
                    )
                })
            {
                let (required_follow_up, recommended_command) =
                    close_current_task_follow_up_and_command(&operator);
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command,
                        rederive_via_workflow_operator: None,
                        required_follow_up,
                        trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            let superseded_task_closure_records = superseded_task_closure_records(
                &context,
                authoritative_state,
                args.task,
                &closure_record_id,
                &effective_reviewed_surface_paths,
            );
            let superseded_task_closure_ids = superseded_task_closure_records
                .iter()
                .map(|record| record.closure_record_id.clone())
                .collect::<Vec<_>>();
            let superseded_tasks = superseded_task_closure_records
                .iter()
                .map(|record| record.task)
                .collect::<Vec<_>>();
            materialize_current_task_closure_from_close_inputs(
                authoritative_state,
                CurrentTaskClosureMaterialization {
                    task: args.task,
                    dispatch_id: &dispatch_id,
                    closure_record_id: &closure_record_id,
                    execution_run_id: &execution_run_id,
                    reviewed_state_id: &raw_reviewed_state_id,
                    semantic_reviewed_state_id: &reviewed_state_id,
                    contract_identity: &contract_identity,
                    effective_reviewed_surface_paths: &effective_reviewed_surface_paths,
                    review_result: args.review_result.as_str(),
                    review_summary_hash,
                    verification_result,
                    verification_summary_hash,
                    superseded_tasks: &superseded_tasks,
                    superseded_task_closure_ids: &superseded_task_closure_ids,
                },
            )?;
            Ok(CloseCurrentTaskOutput {
                action: String::from("recorded"),
                task_number: args.task,
                dispatch_validation_action: String::from("validated"),
                closure_action: String::from("recorded"),
                task_closure_status: String::from("current"),
                superseded_task_closure_ids,
                closure_record_id: Some(closure_record_id),
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: None,
                blocking_scope: None,
                blocking_task: None,
                blocking_reason_codes: Vec::new(),
                authoritative_next_action: None,
                trace_summary: String::from(
                    "Validated task review dispatch lineage and refreshed authoritative task review and verification receipts.",
                ),
            })
        }
        CloseCurrentTaskOutcomeClass::Negative => {
            require_close_current_task_public_mutation(&status, args.task)?;
            let _write_authority = claim_step_write_authority(runtime)?;
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            let locked_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            ensure_task_dispatch_id_matches(&locked_context, args.task, &dispatch_id)?;
            if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
                && current_record.closure_record_id == closure_record_id
                && current_record.dispatch_id == dispatch_id
            {
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(closure_record_id),
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: Some(String::from("execution_reentry")),
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            if authoritative_state
                .task_closure_negative_result(args.task)
                .is_some_and(|negative_record| {
                    task_closure_negative_result_blocks_current_reviewed_state(
                        negative_record
                            .semantic_reviewed_state_id
                            .as_deref()
                            .unwrap_or(negative_record.reviewed_state_id.as_str()),
                        Some(reviewed_state_id.as_str()),
                    )
                })
            {
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command: operator.recommended_command.clone(),
                        rederive_via_workflow_operator: None,
                        required_follow_up: negative_result_required_follow_up(
                            runtime,
                            &args.plan,
                            &operator,
                            Some(authoritative_state),
                        ),
                        trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            record_negative_task_closure(
                authoritative_state,
                NegativeTaskClosureWrite {
                    task: args.task,
                    dispatch_id: &dispatch_id,
                    reviewed_state_id: &reviewed_state_id,
                    semantic_reviewed_state_id: Some(&reviewed_state_id),
                    contract_identity: &contract_identity,
                    review_result: args.review_result.as_str(),
                    review_summary_hash,
                    verification_result,
                    verification_summary_hash,
                },
            )?;
            Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "validated",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: close_current_task_follow_up_and_command(&operator).1,
                    rederive_via_workflow_operator: None,
                    required_follow_up: negative_result_required_follow_up(
                        runtime,
                        &args.plan,
                        &operator,
                        Some(authoritative_state),
                    ),
                    trace_summary: "Task closure remained blocked because the supplied review or verification outcome was not passing.",
                }),
                &operator,
            ))
        }
        CloseCurrentTaskOutcomeClass::Invalid => Ok(blocked_close_current_task_output(
            BlockedCloseCurrentTaskOutputContext {
                task_number: args.task,
                dispatch_validation_action: "validated",
                task_closure_status: "not_current",
                closure_record_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: Some(String::from("run_verification")),
                trace_summary: "close-current-task failed closed because a passing task review requires verification before closure recording can continue.",
            },
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseCurrentTaskOutcomeClass {
    Positive,
    Negative,
    Invalid,
}

fn close_current_task_outcome_class(
    review_result: ReviewOutcomeArg,
    verification_result: VerificationOutcomeArg,
) -> CloseCurrentTaskOutcomeClass {
    match (review_result, verification_result) {
        (ReviewOutcomeArg::Pass, VerificationOutcomeArg::Pass) => {
            CloseCurrentTaskOutcomeClass::Positive
        }
        (ReviewOutcomeArg::Pass, VerificationOutcomeArg::NotRun) => {
            CloseCurrentTaskOutcomeClass::Invalid
        }
        (ReviewOutcomeArg::Fail, _) | (ReviewOutcomeArg::Pass, VerificationOutcomeArg::Fail) => {
            CloseCurrentTaskOutcomeClass::Negative
        }
    }
}

pub fn record_branch_closure(
    runtime: &ExecutionRuntime,
    args: &RecordBranchClosureArgs,
) -> Result<RecordBranchClosureOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let operator = current_workflow_operator(runtime, &args.plan, false)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let mut reviewed_state = current_branch_reviewed_state(&context)?;
    if let Some(blocked_output) =
        blocked_branch_closure_output_for_invalid_current_task_closure(&context)?
    {
        return Ok(blocked_output);
    }
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires authoritative harness state.",
        ));
    };
    if let Some(output) = branch_closure_already_current_empty_lineage_exemption_output(
        &context,
        authoritative_state,
        &reviewed_state,
    )? {
        return Ok(output);
    }
    let rerecording_assessment = branch_closure_rerecording_assessment(&context)?;
    let changed_paths = rerecording_assessment.changed_paths.clone();
    let supported_late_stage_rerecording =
        !changed_paths.is_empty() && rerecording_assessment.supported;
    let branch_closure_recording_ready = (operator.phase == "document_release_pending"
        && operator.phase_detail == "branch_closure_recording_required_for_release_readiness")
        || supported_late_stage_rerecording;
    if !branch_closure_recording_ready {
        if operator_requires_review_state_repair(&operator) {
            return Ok(RecordBranchClosureOutput {
                action: String::from("blocked"),
                branch_closure_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: Some(String::from("repair_review_state")),
                trace_summary: String::from(
                    "record-branch-closure failed closed because workflow/operator requires review-state repair before branch-closure recording can proceed.",
                ),
            });
        } else if operator.review_state_status == "clean" {
            return Ok(shared_out_of_phase_record_branch_closure_output(
                &args.plan,
                current_authoritative_branch_closure_id(&context)?,
                "record-branch-closure failed closed because the current phase must be re-derived through workflow/operator before branch-closure recording can proceed.",
            ));
        } else {
            return Ok(RecordBranchClosureOutput {
                action: String::from("blocked"),
                branch_closure_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: blocked_follow_up_for_operator(&operator),
                trace_summary: String::from(
                    "record-branch-closure failed closed because workflow/operator did not expose branch_closure_recording_required_for_release_readiness.",
                ),
            });
        }
    }
    if !changed_paths.is_empty() {
        if !rerecording_assessment.supported {
            let trace_summary = match rerecording_assessment.unsupported_reason {
                Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => {
                    "record-branch-closure failed closed because no still-current task-closure baseline remains for authoritative branch re-recording."
                }
                Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => {
                    "record-branch-closure failed closed because the approved plan does not declare Late-Stage Surface metadata, so post-closure repo drift cannot be classified as trusted late-stage-only."
                }
                Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
                    "record-branch-closure failed closed because branch drift escaped the trusted Late-Stage Surface."
                }
            };
            return Ok(RecordBranchClosureOutput {
                action: String::from("blocked"),
                branch_closure_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: Some(String::from("repair_review_state")),
                trace_summary: trace_summary.to_owned(),
            });
        }
        let late_stage_surface = rerecording_assessment.late_stage_surface.as_slice();
        reviewed_state.provenance_basis =
            String::from("task_closure_lineage_plus_late_stage_surface_exemption");
        reviewed_state.source_task_closure_ids = shared_branch_source_task_closure_ids(
            &context,
            &current_branch_task_closure_records(&context)?,
            Some(late_stage_surface),
        );
        if reviewed_state.source_task_closure_ids.is_empty() {
            reviewed_state.effective_reviewed_branch_surface =
                late_stage_surface_only_branch_surface(&changed_paths);
        }
    }
    if let Some(output) =
        branch_closure_already_current_output(&context, authoritative_state, &reviewed_state)?
    {
        return Ok(output);
    }
    if reviewed_state.source_task_closure_ids.is_empty()
        && reviewed_state.provenance_basis
            != "task_closure_lineage_plus_late_stage_surface_exemption"
    {
        return Ok(RecordBranchClosureOutput {
            action: String::from("blocked"),
            branch_closure_id: None,
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            superseded_branch_closure_ids: Vec::new(),
            required_follow_up: Some(String::from("repair_review_state")),
            trace_summary: String::from(
                "record-branch-closure failed closed because no authoritative still-current task-closure provenance remains for the requested branch surface.",
            ),
        });
    }
    let base_branch_closure_id = deterministic_branch_closure_record_id(&context, &reviewed_state);
    let branch_closure_id =
        authoritative_state.next_available_branch_closure_record_id(&base_branch_closure_id);
    let superseded_branch_closure_ids =
        superseded_branch_closure_ids_from_previous_current(overlay.as_ref(), &branch_closure_id);
    let branch_closure_source = render_branch_closure_artifact(
        &context,
        &branch_closure_id,
        BranchClosureProjectionInput {
            contract_identity: &reviewed_state.contract_identity,
            base_branch: &reviewed_state.base_branch,
            reviewed_state_id: &reviewed_state.reviewed_state_id,
            effective_reviewed_branch_surface: &reviewed_state.effective_reviewed_branch_surface,
            source_task_closure_ids: &reviewed_state.source_task_closure_ids,
            provenance_basis: &reviewed_state.provenance_basis,
            superseded_branch_closure_ids: &superseded_branch_closure_ids,
        },
    )?;
    let branch_closure_fingerprint = sha256_hex(branch_closure_source.as_bytes());
    record_current_branch_closure(
        authoritative_state,
        BranchClosureWrite {
            branch_closure_id: &branch_closure_id,
            source_plan_path: &context.plan_rel,
            source_plan_revision: context.plan_document.plan_revision,
            repo_slug: &context.runtime.repo_slug,
            branch_name: &context.runtime.branch_name,
            base_branch: &reviewed_state.base_branch,
            reviewed_state_id: &reviewed_state.reviewed_state_id,
            semantic_reviewed_state_id: Some(&reviewed_state.semantic_reviewed_state_id),
            contract_identity: &reviewed_state.contract_identity,
            effective_reviewed_branch_surface: &reviewed_state.effective_reviewed_branch_surface,
            source_task_closure_ids: &reviewed_state.source_task_closure_ids,
            provenance_basis: &reviewed_state.provenance_basis,
            closure_status: "current",
            superseded_branch_closure_ids: &superseded_branch_closure_ids,
            branch_closure_fingerprint: Some(&branch_closure_fingerprint),
        },
    )?;
    let published_branch_closure_fingerprint =
        publish_authoritative_artifact(runtime, "branch-closure", &branch_closure_source)?;
    debug_assert_eq!(
        published_branch_closure_fingerprint,
        branch_closure_fingerprint
    );
    write_project_artifact(
        runtime,
        &format!("branch-closure-{}.md", &branch_closure_id),
        &branch_closure_source,
    )?;
    Ok(RecordBranchClosureOutput {
        action: String::from("recorded"),
        branch_closure_id: Some(branch_closure_id),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        superseded_branch_closure_ids,
        required_follow_up: None,
        trace_summary: String::from(
            "Recorded a current branch closure for the still-current reviewed branch state.",
        ),
    })
}

fn advance_late_stage_result_label(result: Option<AdvanceLateStageResultArg>) -> &'static str {
    result
        .map(AdvanceLateStageResultArg::as_str)
        .unwrap_or("unspecified")
}

fn require_advance_late_stage_summary_file<'a>(
    args: &'a AdvanceLateStageArgs,
    stage_label: &str,
) -> Result<&'a Path, JsonFailure> {
    args.summary_file.as_deref().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!(
                "summary_file_required: {stage_label} advance-late-stage requires --summary-file."
            ),
        )
    })
}

fn require_advance_late_stage_public_mutation(
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    require_public_mutation(
        status,
        PublicMutationRequest {
            kind: PublicMutationKind::AdvanceLateStage,
            task: None,
            step: None,
            transfer_mode: None,
            transfer_scope: None,
            command_name: "advance-late-stage",
        },
        FailureClass::ExecutionStateNotReady,
    )
}

pub fn advance_late_stage(
    runtime: &ExecutionRuntime,
    args: &AdvanceLateStageArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let status = status_with_shared_routing_or_context(runtime, &args.plan, &context)?;
    let supplied_result_label = advance_late_stage_result_label(args.result);
    let current_branch_closure = current_authoritative_branch_closure_binding_optional(&context)?;
    let branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
    let operator_without_external_review = current_workflow_operator(runtime, &args.plan, false);
    let final_review_recording_requested = args.dispatch_id.is_some()
        || args.reviewer_source.is_some()
        || args.reviewer_id.is_some()
        || (matches!(
            args.result,
            Some(AdvanceLateStageResultArg::Pass | AdvanceLateStageResultArg::Fail)
        ) && operator_without_external_review
            .as_ref()
            .ok()
            .is_some_and(|operator| operator.phase == "final_review_pending"));
    if final_review_recording_requested {
        let _write_authority = claim_step_write_authority(runtime)?;
        if args.branch_closure_id.is_some() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "final_review_branch_closure_id_invalid: final-review advance-late-stage does not accept --branch-closure-id; use the workflow/operator recording_context branch_closure_id.",
            ));
        }
        let reviewer_source = args
            .reviewer_source
            .as_deref()
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "reviewer_source_required: final-review advance-late-stage requires --reviewer-source.",
                )
            })?;
        if !shared_reviewer_source_is_valid(reviewer_source) {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "reviewer_source_invalid: final-review advance-late-stage requires --reviewer-source fresh-context-subagent|cross-model|human-independent-reviewer.",
            ));
        }
        let reviewer_id = args.reviewer_id.as_deref().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "reviewer_id_required: final-review advance-late-stage requires --reviewer-id.",
            )
        })?;
        let result = match args.result {
            Some(AdvanceLateStageResultArg::Pass) => "pass",
            Some(AdvanceLateStageResultArg::Fail) => "fail",
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "final_review_result_invalid: final-review advance-late-stage requires --result pass|fail.",
                ));
            }
        };
        let summary_file = require_advance_late_stage_summary_file(args, "final-review")?;
        let final_review_recording_ready = |operator: &ExecutionRoutingState| {
            operator.review_state_status == "clean"
                && operator.phase == "final_review_pending"
                && operator.phase_detail == "final_review_recording_ready"
                && operator
                    .recording_context
                    .as_ref()
                    .and_then(|context| context.branch_closure_id.as_deref())
                    == branch_closure_id.as_deref()
        };
        let candidate_dispatch_id = current_review_dispatch_id_candidate(
            &context,
            ReviewDispatchScopeArg::FinalReview,
            None,
            args.dispatch_id.as_deref(),
        )?;
        let operator = match current_workflow_operator(runtime, &args.plan, true) {
            Ok(operator) => operator,
            Err(error) if error.error_class == FailureClass::InstructionParseFailed.as_str() => {
                return Ok(shared_out_of_phase_advance_late_stage_output(
                    &args.plan,
                    AdvanceLateStageOutputContext {
                        stage_path: "final_review",
                        delegated_primitive: "record-final-review",
                        branch_closure_id: branch_closure_id.clone(),
                        dispatch_id: None,
                        result,
                        external_review_result_ready: true,
                        trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                    },
                ));
            }
            Err(error) => return Err(error),
        };
        let final_review_override_out_of_phase =
            late_stage_negative_result_override_active(&operator);
        let dispatch_current_before_record = candidate_dispatch_id
            .as_deref()
            .map(|dispatch_id| {
                ensure_final_review_dispatch_id_matches(&context, dispatch_id).is_ok()
            })
            .unwrap_or(false);
        if operator.review_state_status == "clean"
            && !final_review_override_out_of_phase
            && dispatch_current_before_record
            && let Some(current_branch_closure) = current_branch_closure.as_ref()
            && let Some(output) = equivalent_current_final_review_rerun(
                &context,
                current_branch_closure,
                EquivalentFinalReviewRerunParams {
                    stage_path: "final_review",
                    delegated_primitive: "record-final-review",
                    dispatch_id: candidate_dispatch_id
                        .as_deref()
                        .expect("candidate dispatch id should exist when marked current"),
                    reviewer_source,
                    reviewer_id,
                    result,
                    summary_file,
                    required_follow_up: (result == "fail")
                        .then(|| negative_result_follow_up(&operator))
                        .flatten(),
                },
            )?
        {
            return Ok(output);
        }
        let allow_fail_recording_while_override_out_of_phase = result == "fail"
            && final_review_override_out_of_phase
            && dispatch_current_before_record;
        if !final_review_recording_ready(&operator)
            && !allow_fail_recording_while_override_out_of_phase
        {
            return Ok(advance_late_stage_follow_up_or_requery_output(
                &operator,
                &args.plan,
                false,
                AdvanceLateStageOutputContext {
                    stage_path: "final_review",
                    delegated_primitive: "record-final-review",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: None,
                    result,
                    external_review_result_ready: true,
                    trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                },
            ));
        }
        require_advance_late_stage_public_mutation(&status)?;
        let summary = read_nonempty_summary_file(summary_file, "summary")?;
        let normalized_summary_hash = summary_hash(&summary);
        let dispatch_id = if let Some(dispatch_id) = candidate_dispatch_id {
            dispatch_id
        } else {
            ensure_current_review_dispatch_id(
                &context,
                ReviewDispatchScopeArg::FinalReview,
                None,
                args.dispatch_id.as_deref(),
            )?
        };
        let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
        let current_branch_closure =
            current_authoritative_branch_closure_binding_optional(&context)?;
        let branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
        let dispatch_current =
            ensure_final_review_dispatch_id_matches(&context, &dispatch_id).is_ok();
        let operator = match current_workflow_operator(runtime, &args.plan, true) {
            Ok(operator) => operator,
            Err(error) if error.error_class == FailureClass::InstructionParseFailed.as_str() => {
                return Ok(shared_out_of_phase_advance_late_stage_output(
                    &args.plan,
                    AdvanceLateStageOutputContext {
                        stage_path: "final_review",
                        delegated_primitive: "record-final-review",
                        branch_closure_id: branch_closure_id.clone(),
                        dispatch_id: Some(dispatch_id.clone()),
                        result,
                        external_review_result_ready: true,
                        trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                    },
                ));
            }
            Err(error) => return Err(error),
        };
        let final_review_override_out_of_phase =
            late_stage_negative_result_override_active(&operator);
        let allow_fail_recording_while_override_out_of_phase =
            result == "fail" && final_review_override_out_of_phase && dispatch_current;
        if (!final_review_recording_ready(&operator)
            && !allow_fail_recording_while_override_out_of_phase)
            || !dispatch_current
        {
            if operator.review_state_status == "clean"
                && !final_review_override_out_of_phase
                && let Some(current_branch_closure) = current_branch_closure.as_ref()
                && let Some(output) = equivalent_current_final_review_rerun(
                    &context,
                    current_branch_closure,
                    EquivalentFinalReviewRerunParams {
                        stage_path: "final_review",
                        delegated_primitive: "record-final-review",
                        dispatch_id: &dispatch_id,
                        reviewer_source,
                        reviewer_id,
                        result,
                        summary_file,
                        required_follow_up: (result == "fail")
                            .then(|| negative_result_follow_up(&operator))
                            .flatten(),
                    },
                )?
            {
                return Ok(output);
            }
            return Ok(advance_late_stage_follow_up_or_requery_output(
                &operator,
                &args.plan,
                dispatch_current,
                AdvanceLateStageOutputContext {
                    stage_path: "final_review",
                    delegated_primitive: "record-final-review",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: Some(dispatch_id.clone()),
                    result,
                    external_review_result_ready: true,
                    trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                },
            ));
        }
        let current_branch_closure = authoritative_current_branch_closure_binding(
            &context,
            "advance-late-stage final-review",
        )?;
        let branch_closure_id = current_branch_closure.branch_closure_id.clone();
        ensure_final_review_dispatch_id_matches(&context, &dispatch_id)?;
        let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
        let semantic_reviewed_state_id = current_branch_closure.semantic_reviewed_state_id.clone();
        let browser_qa_required = current_plan_requires_browser_qa(&context);
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage requires authoritative harness state.",
            ));
        };
        let final_review_evidence = resolve_final_review_evidence(&context)?;
        if let (
            Some(current_branch_closure_id),
            Some(current_dispatch_id),
            Some(current_reviewer_source),
            Some(current_reviewer_id),
            Some(current_result),
            Some(current_summary_hash),
        ) = (
            authoritative_state.current_final_review_branch_closure_id(),
            authoritative_state.current_final_review_dispatch_id(),
            authoritative_state.current_final_review_reviewer_source(),
            authoritative_state.current_final_review_reviewer_id(),
            authoritative_state.current_final_review_result(),
            authoritative_state.current_final_review_summary_hash(),
        ) && current_branch_closure_id == branch_closure_id
            && current_dispatch_id == dispatch_id
        {
            let equivalent_current_result = current_reviewer_source == reviewer_source
                && current_reviewer_id == reviewer_id
                && current_result == result
                && current_summary_hash == normalized_summary_hash;
            if equivalent_current_result {
                if current_final_review_record_is_still_authoritative(
                    &context,
                    authoritative_state,
                    CurrentFinalReviewAuthorityCheck {
                        branch_closure_id: &branch_closure_id,
                        dispatch_id: &dispatch_id,
                        reviewer_source,
                        reviewer_id,
                        result,
                        normalized_summary_hash: &normalized_summary_hash,
                    },
                )? {
                    if result == "fail" && final_review_override_out_of_phase {
                        return Ok(shared_out_of_phase_advance_late_stage_output(
                            &args.plan,
                            AdvanceLateStageOutputContext {
                                stage_path: "final_review",
                                delegated_primitive: "record-final-review",
                                branch_closure_id: Some(branch_closure_id),
                                dispatch_id: Some(dispatch_id.clone()),
                                result,
                                external_review_result_ready: true,
                                trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                            },
                        ));
                    }
                    return Ok(AdvanceLateStageOutput {
                        action: String::from("already_current"),
                        stage_path: String::from("final_review"),
                        delegated_primitive: String::from("record-final-review"),
                        branch_closure_id: Some(branch_closure_id),
                        dispatch_id: Some(dispatch_id.clone()),
                        result: result.to_owned(),
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: (result == "fail")
                            .then(|| {
                                negative_result_required_follow_up(
                                    runtime,
                                    &args.plan,
                                    &operator,
                                    Some(authoritative_state),
                                )
                            })
                            .flatten(),
                        trace_summary: String::from(
                            "Current branch closure already has an equivalent recorded final-review outcome.",
                        ),
                    });
                }
                return Ok(shared_out_of_phase_advance_late_stage_output(
                    &args.plan,
                    AdvanceLateStageOutputContext {
                        stage_path: "final_review",
                        delegated_primitive: "record-final-review",
                        branch_closure_id: Some(branch_closure_id.clone()),
                        dispatch_id: Some(dispatch_id.clone()),
                        result,
                        external_review_result_ready: true,
                        trace_summary: "advance-late-stage failed closed because the current final-review record is no longer authoritative and workflow/operator must re-derive the next safe step.",
                    },
                ));
            } else {
                return Ok(AdvanceLateStageOutput {
                    action: String::from("blocked"),
                    stage_path: String::from("final_review"),
                    delegated_primitive: String::from("record-final-review"),
                    branch_closure_id: Some(branch_closure_id),
                    dispatch_id: Some(dispatch_id.clone()),
                    result: result.to_owned(),
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: None,
                    trace_summary: String::from(
                        "advance-late-stage failed closed because the current branch closure already has a conflicting recorded final-review outcome for this dispatch lineage.",
                    ),
                });
            }
        }
        let rendered_final_review = render_final_review_artifacts(
            runtime,
            &context,
            &branch_closure_id,
            &reviewed_state_id,
            &final_review_evidence.base_branch,
            FinalReviewProjectionInput {
                dispatch_id: &dispatch_id,
                reviewer_source,
                reviewer_id,
                result,
                deviations_required: final_review_evidence.deviations_required,
                summary: &summary,
            },
        )?;
        let final_review_source = rendered_final_review.final_review_source;
        let final_review_fingerprint = sha256_hex(final_review_source.as_bytes());
        let release_readiness_record_id = authoritative_state
            .current_release_readiness_record_id()
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "advance-late-stage final-review requires a current release-readiness record id.",
                )
            })?;
        persist_final_review_record(
            authoritative_state,
            FinalReviewWrite {
                branch_closure_id: &branch_closure_id,
                release_readiness_record_id: &release_readiness_record_id,
                dispatch_id: &dispatch_id,
                reviewer_source,
                reviewer_id,
                result,
                final_review_fingerprint: Some(final_review_fingerprint.as_str()),
                deviations_required: Some(final_review_evidence.deviations_required),
                browser_qa_required,
                source_plan_path: &context.plan_rel,
                source_plan_revision: context.plan_document.plan_revision,
                repo_slug: &context.runtime.repo_slug,
                branch_name: &context.runtime.branch_name,
                base_branch: &final_review_evidence.base_branch,
                reviewed_state_id: &reviewed_state_id,
                semantic_reviewed_state_id: semantic_reviewed_state_id.as_deref(),
                summary: &summary,
                summary_hash: &normalized_summary_hash,
            },
        )?;
        let published =
            publish_authoritative_artifact(runtime, "final-review", &final_review_source)?;
        debug_assert_eq!(published, final_review_fingerprint);
        let reviewer_artifact_name = rendered_final_review
            .reviewer_artifact_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::EvidenceWriteFailed,
                    "Could not derive final-review reviewer artifact file name.",
                )
            })?;
        write_project_artifact(
            runtime,
            reviewer_artifact_name,
            &rendered_final_review.reviewer_source_text,
        )?;
        write_project_artifact(
            runtime,
            &format!(
                "featureforge-{}-code-review-{}.md",
                runtime.safe_branch,
                timestamp_slug()
            ),
            &final_review_source,
        )?;
        let (
            code,
            recommended_command,
            rederive_via_workflow_operator,
            required_follow_up,
            trace_summary,
        ) = if result == "fail" && final_review_override_out_of_phase {
            (
                Some(String::from("out_of_phase_requery_required")),
                Some(recommended_operator_command(&args.plan, true)),
                Some(true),
                None,
                String::from(
                    "Recorded final-review evidence for the current dispatch lineage; workflow/operator must be requeried to continue from the active override lane.",
                ),
            )
        } else {
            (
                None,
                None,
                None,
                (result == "fail")
                    .then(|| {
                        negative_result_required_follow_up(
                            runtime,
                            &args.plan,
                            &operator,
                            Some(authoritative_state),
                        )
                    })
                    .flatten(),
                String::from(
                    "Validated final-review dispatch lineage and recorded final-review evidence from authoritative late-stage state.",
                ),
            )
        };
        return Ok(AdvanceLateStageOutput {
            action: String::from("recorded"),
            stage_path: String::from("final_review"),
            delegated_primitive: String::from("record-final-review"),
            branch_closure_id: Some(branch_closure_id),
            dispatch_id: Some(dispatch_id.clone()),
            result: result.to_owned(),
            code,
            recommended_command,
            rederive_via_workflow_operator,
            required_follow_up,
            trace_summary,
        });
    }

    if args.branch_closure_id.is_some()
        || args.reviewer_source.is_some()
        || args.reviewer_id.is_some()
    {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "release_readiness_argument_mismatch: release-readiness advance-late-stage does not accept final-review-only arguments.",
        ));
    }
    let operator = match operator_without_external_review {
        Ok(operator) => operator,
        Err(error) if error.error_class == FailureClass::InstructionParseFailed.as_str() => {
            return Ok(shared_out_of_phase_advance_late_stage_output(
                &args.plan,
                AdvanceLateStageOutputContext {
                    stage_path: "release_readiness",
                    delegated_primitive: "record-release-readiness",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: None,
                    result: supplied_result_label,
                    external_review_result_ready: false,
                    trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
                },
            ));
        }
        Err(error) => return Err(error),
    };
    if operator.phase == "document_release_pending"
        && operator.phase_detail == "branch_closure_recording_required_for_release_readiness"
    {
        if args.result.is_some() || args.summary_file.is_some() || args.dispatch_id.is_some() {
            return Ok(shared_out_of_phase_advance_late_stage_output(
                &args.plan,
                AdvanceLateStageOutputContext {
                    stage_path: "release_readiness",
                    delegated_primitive: "record-release-readiness",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: None,
                    result: supplied_result_label,
                    external_review_result_ready: false,
                    trace_summary: "advance-late-stage failed closed because branch-closure recording is required before release-readiness arguments are valid.",
                },
            ));
        }
        require_advance_late_stage_public_mutation(&status)?;
        let output = record_branch_closure(
            runtime,
            &RecordBranchClosureArgs {
                plan: args.plan.clone(),
            },
        )?;
        return Ok(AdvanceLateStageOutput {
            action: output.action,
            stage_path: String::from("branch_closure"),
            delegated_primitive: String::from("record-branch-closure"),
            branch_closure_id: output.branch_closure_id,
            dispatch_id: None,
            result: String::from("recorded"),
            code: output.code,
            recommended_command: output.recommended_command,
            rederive_via_workflow_operator: output.rederive_via_workflow_operator,
            required_follow_up: output.required_follow_up,
            trace_summary: output.trace_summary,
        });
    }
    if operator.review_state_status == "clean"
        && operator.phase == "qa_pending"
        && operator.phase_detail == "qa_recording_required"
    {
        let result = match args.result {
            Some(AdvanceLateStageResultArg::Pass) => ReviewOutcomeArg::Pass,
            Some(AdvanceLateStageResultArg::Fail) => ReviewOutcomeArg::Fail,
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "qa_result_invalid: QA advance-late-stage requires --result pass|fail.",
                ));
            }
        };
        let summary_file = require_advance_late_stage_summary_file(args, "QA")?;
        require_advance_late_stage_public_mutation(&status)?;
        let output = record_qa(
            runtime,
            &RecordQaArgs {
                plan: args.plan.clone(),
                result,
                summary_file: summary_file.to_path_buf(),
            },
        )?;
        return Ok(AdvanceLateStageOutput {
            action: output.action,
            stage_path: String::from("browser_qa"),
            delegated_primitive: String::from("record-qa"),
            branch_closure_id: Some(output.branch_closure_id),
            dispatch_id: None,
            result: output.result,
            code: output.code,
            recommended_command: output.recommended_command,
            rederive_via_workflow_operator: output.rederive_via_workflow_operator,
            required_follow_up: output.required_follow_up,
            trace_summary: output.trace_summary,
        });
    }
    let result = match args.result {
        Some(AdvanceLateStageResultArg::Ready) => "ready",
        Some(AdvanceLateStageResultArg::Blocked) => "blocked",
        _ => {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "release_readiness_result_invalid: release-readiness advance-late-stage requires --result ready|blocked.",
            ));
        }
    };
    let summary_file = require_advance_late_stage_summary_file(args, "release-readiness")?;
    let release_route_ready = operator.review_state_status == "clean"
        && operator.phase == "document_release_pending"
        && matches!(
            operator.phase_detail.as_str(),
            "release_readiness_recording_ready" | "release_blocker_resolution_required"
        );
    if !release_route_ready {
        if operator.review_state_status == "clean"
            && let Some(current_branch_closure) = current_branch_closure.as_ref()
            && let Some(output) = equivalent_current_release_readiness_rerun(
                &context,
                current_branch_closure,
                "release_readiness",
                "record-release-readiness",
                result,
                summary_file,
            )?
        {
            return Ok(output);
        }
        if current_branch_closure.is_none() {
            require_public_mutation(
                &status,
                PublicMutationRequest {
                    kind: PublicMutationKind::AdvanceLateStage,
                    task: None,
                    step: None,
                    transfer_mode: None,
                    transfer_scope: None,
                    command_name: "advance-late-stage",
                },
                FailureClass::ExecutionStateNotReady,
            )?;
        }
        return Ok(release_readiness_follow_up_or_requery_output(
            &operator,
            &args.plan,
            AdvanceLateStageOutputContext {
                stage_path: "release_readiness",
                delegated_primitive: "record-release-readiness",
                branch_closure_id: branch_closure_id.clone(),
                dispatch_id: None,
                result,
                external_review_result_ready: false,
                trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
            },
        ));
    }
    require_advance_late_stage_public_mutation(&status)?;
    let summary = read_nonempty_summary_file(summary_file, "summary")?;
    let normalized_summary_hash = summary_hash(&summary);
    let current_branch_closure = authoritative_current_branch_closure_binding(
        &context,
        "advance-late-stage release-readiness",
    )?;
    let branch_closure_id = current_branch_closure.branch_closure_id.clone();
    let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
    let semantic_reviewed_state_id = current_branch_closure.semantic_reviewed_state_id.clone();
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage requires authoritative harness state.",
        ));
    };
    if let Some(current_record) = authoritative_state
        .current_release_readiness_record()
        .filter(|record| record.branch_closure_id == branch_closure_id)
    {
        if current_record.result == result && current_record.summary_hash == normalized_summary_hash
        {
            return Ok(AdvanceLateStageOutput {
                action: String::from("already_current"),
                stage_path: String::from("release_readiness"),
                delegated_primitive: String::from("record-release-readiness"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: None,
                result: result.to_owned(),
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: (result == "blocked")
                    .then(|| String::from("resolve_release_blocker")),
                trace_summary: String::from(
                    "Current branch closure already has an equivalent recorded release-readiness outcome.",
                ),
            });
        }
        if current_record.result != "blocked" {
            return Ok(AdvanceLateStageOutput {
                action: String::from("blocked"),
                stage_path: String::from("release_readiness"),
                delegated_primitive: String::from("record-release-readiness"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: None,
                result: result.to_owned(),
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: None,
                trace_summary: String::from(
                    "advance-late-stage failed closed because the current branch closure already has a conflicting recorded release-readiness outcome.",
                ),
            });
        }
    }
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ReleaseArtifactNotFresh,
            "advance-late-stage release-readiness requires a resolvable base branch.",
        )
    })?;
    let release_source = render_release_readiness_artifact(
        &context,
        &branch_closure_id,
        &reviewed_state_id,
        &base_branch,
        result,
        &summary,
    )?;
    let release_fingerprint = if result == "ready" {
        Some(sha256_hex(release_source.as_bytes()))
    } else {
        None
    };
    persist_release_readiness_record(
        authoritative_state,
        ReleaseReadinessWrite {
            branch_closure_id: &branch_closure_id,
            source_plan_path: &context.plan_rel,
            source_plan_revision: context.plan_document.plan_revision,
            repo_slug: &runtime.repo_slug,
            branch_name: &context.runtime.branch_name,
            base_branch: &base_branch,
            reviewed_state_id: &reviewed_state_id,
            semantic_reviewed_state_id: semantic_reviewed_state_id.as_deref(),
            result,
            release_docs_fingerprint: release_fingerprint.as_deref(),
            summary: &summary,
            summary_hash: &normalized_summary_hash,
            generated_by_identity: "featureforge/release-readiness",
        },
    )?;
    write_project_artifact(
        runtime,
        &format!(
            "featureforge-{}-release-readiness-{}.md",
            runtime.safe_branch,
            timestamp_slug()
        ),
        &release_source,
    )?;
    if let Some(release_docs_fingerprint) = release_fingerprint.as_deref() {
        let published = publish_authoritative_artifact(runtime, "release-docs", &release_source)?;
        debug_assert_eq!(published, release_docs_fingerprint);
    }
    Ok(AdvanceLateStageOutput {
        action: String::from("recorded"),
        stage_path: String::from("release_readiness"),
        delegated_primitive: String::from("record-release-readiness"),
        branch_closure_id: Some(branch_closure_id),
        dispatch_id: None,
        result: result.to_owned(),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        required_follow_up: (result == "blocked").then(|| String::from("resolve_release_blocker")),
        trace_summary: String::from(
            "Recorded release-readiness late-stage evidence for the current branch closure.",
        ),
    })
}

pub fn record_release_readiness(
    runtime: &ExecutionRuntime,
    args: &RecordReleaseReadinessArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let current_branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
    let Some(current_branch_closure_id) = current_branch_closure_id else {
        let params = AdvanceLateStageOutputContext {
            stage_path: "release_readiness",
            delegated_primitive: "record-release-readiness",
            branch_closure_id: Some(args.branch_closure_id.clone()),
            dispatch_id: None,
            result: args.result.as_str(),
            external_review_result_ready: false,
            trace_summary: "record-release-readiness failed closed because no authoritative current branch closure is available.",
        };
        if let Ok(operator) = current_workflow_operator(runtime, &args.plan, false) {
            return Ok(release_readiness_follow_up_or_requery_output(
                &operator, &args.plan, params,
            ));
        }
        return Ok(shared_out_of_phase_advance_late_stage_output(
            &args.plan, params,
        ));
    };
    if current_branch_closure_id != args.branch_closure_id {
        return Ok(shared_out_of_phase_advance_late_stage_output(
            &args.plan,
            AdvanceLateStageOutputContext {
                stage_path: "release_readiness",
                delegated_primitive: "record-release-readiness",
                branch_closure_id: Some(args.branch_closure_id.clone()),
                dispatch_id: None,
                result: args.result.as_str(),
                external_review_result_ready: false,
                trace_summary: "record-release-readiness failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
            },
        ));
    }
    let result = match args.result {
        crate::cli::plan_execution::ReleaseReadinessOutcomeArg::Ready => {
            AdvanceLateStageResultArg::Ready
        }
        crate::cli::plan_execution::ReleaseReadinessOutcomeArg::Blocked => {
            AdvanceLateStageResultArg::Blocked
        }
    };
    advance_late_stage(
        runtime,
        &AdvanceLateStageArgs {
            plan: args.plan.clone(),
            dispatch_id: None,
            branch_closure_id: None,
            reviewer_source: None,
            reviewer_id: None,
            result: Some(result),
            summary_file: Some(args.summary_file.clone()),
        },
    )
}

pub fn record_final_review(
    runtime: &ExecutionRuntime,
    args: &RecordFinalReviewArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let current_branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
    if current_branch_closure_id.as_deref() != Some(args.branch_closure_id.as_str()) {
        return Ok(shared_out_of_phase_advance_late_stage_output(
            &args.plan,
            AdvanceLateStageOutputContext {
                stage_path: "final_review",
                delegated_primitive: "record-final-review",
                branch_closure_id: Some(args.branch_closure_id.clone()),
                dispatch_id: Some(args.dispatch_id.clone()),
                result: args.result.as_str(),
                external_review_result_ready: true,
                trace_summary: "record-final-review failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
            },
        ));
    }
    let result = match args.result {
        ReviewOutcomeArg::Pass => AdvanceLateStageResultArg::Pass,
        ReviewOutcomeArg::Fail => AdvanceLateStageResultArg::Fail,
    };
    advance_late_stage(
        runtime,
        &AdvanceLateStageArgs {
            plan: args.plan.clone(),
            dispatch_id: Some(args.dispatch_id.clone()),
            branch_closure_id: None,
            reviewer_source: Some(args.reviewer_source.clone()),
            reviewer_id: Some(args.reviewer_id.clone()),
            result: Some(result),
            summary_file: Some(args.summary_file.clone()),
        },
    )
}

pub fn record_qa(
    runtime: &ExecutionRuntime,
    args: &RecordQaArgs,
) -> Result<RecordQaOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let current_branch_closure = current_authoritative_branch_closure_binding_optional(&context)?;
    let branch_closure_id =
        current_authoritative_branch_closure_id_optional(&context)?.unwrap_or_default();
    let (operator, runtime_state) =
        current_workflow_operator_with_runtime_state(runtime, &args.plan, false)?;
    let mut required_follow_up = blocked_follow_up_for_operator(&operator);
    if required_follow_up.is_none()
        && operator.phase == "executing"
        && operator.phase_detail == "execution_in_progress"
        && operator.review_state_status == "clean"
        && operator.current_branch_closure_id.is_none()
        && operator
            .blocking_reason_codes
            .iter()
            .any(|code| code == "derived_review_state_missing")
    {
        required_follow_up = Some(String::from("repair_review_state"));
    }
    let qa_refresh_reroute_active =
        shared_finish_requires_test_plan_refresh(runtime_state.gate_snapshot.gate_finish.as_ref())
            || (operator.phase == "qa_pending"
                && operator.phase_detail == "test_plan_refresh_required");
    if qa_refresh_reroute_active {
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from("out_of_phase_requery_required")),
            recommended_command: Some(recommended_operator_command(&args.plan, false)),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "record-qa failed closed because workflow/operator requires a fresh current-branch test plan before QA recording can proceed.",
            ),
        });
    }
    if required_follow_up.as_deref() == Some("repair_review_state")
        && operator.phase == "executing"
        && operator.phase_detail == "execution_reentry_required"
        && operator.review_state_status == "missing_current_closure"
        && operator.current_branch_closure_id.is_none()
    {
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from("out_of_phase_requery_required")),
            recommended_command: Some(recommended_operator_command(&args.plan, false)),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "record-qa failed closed because workflow/operator must be requeried before QA recording can proceed.",
            ),
        });
    }
    if required_follow_up.as_deref() == Some("repair_review_state")
        && operator.review_state_status == "clean"
    {
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from("out_of_phase_requery_required")),
            recommended_command: Some(recommended_operator_command(&args.plan, false)),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "record-qa failed closed because workflow/operator must be requeried before QA recording can proceed.",
            ),
        });
    }
    if operator.review_state_status != "clean" {
        if operator.review_state_status == "stale_unreviewed"
            || required_follow_up.as_deref() != Some("repair_review_state")
        {
            return Ok(RecordQaOutput {
                action: String::from("blocked"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                code: Some(String::from("out_of_phase_requery_required")),
                recommended_command: Some(recommended_operator_command(&args.plan, false)),
                rederive_via_workflow_operator: Some(true),
                required_follow_up: None,
                trace_summary: String::from(
                    "record-qa failed closed because the current phase must be re-derived through workflow/operator before QA recording can proceed.",
                ),
            });
        }
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            required_follow_up,
            trace_summary: String::from(
                "record-qa failed closed because workflow/operator did not expose qa_recording_required for the current branch closure.",
            ),
        });
    }
    let qa_override_out_of_phase = late_stage_negative_result_override_active(&operator);
    if operator.phase != "qa_pending" || operator.phase_detail != "qa_recording_required" {
        let allow_fail_recording_while_override_out_of_phase =
            args.result == ReviewOutcomeArg::Fail && qa_override_out_of_phase;
        if !allow_fail_recording_while_override_out_of_phase
            && equivalent_current_browser_qa_rerun_allowed(&operator, args.result.as_str())
            && let Some(current_branch_closure) = current_branch_closure.as_ref()
            && let Some(output) = equivalent_current_browser_qa_rerun(
                &context,
                current_branch_closure,
                &runtime_state.gate_snapshot,
                args.result.as_str(),
                &args.summary_file,
                (args.result == ReviewOutcomeArg::Fail)
                    .then(|| negative_result_follow_up(&operator))
                    .flatten(),
            )?
        {
            return Ok(output);
        }
        if !allow_fail_recording_while_override_out_of_phase {
            return Ok(RecordQaOutput {
                action: String::from("blocked"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                code: Some(String::from("out_of_phase_requery_required")),
                recommended_command: Some(recommended_operator_command(&args.plan, false)),
                rederive_via_workflow_operator: Some(true),
                required_follow_up: None,
                trace_summary: String::from(
                    "record-qa failed closed because the current phase is out of band for QA recording; reroute through workflow/operator.",
                ),
            });
        }
    }
    let current_branch_closure =
        authoritative_current_branch_closure_binding(&context, "record-qa")?;
    let branch_closure_id = current_branch_closure.branch_closure_id.clone();
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-qa requires authoritative harness state.",
        ));
    };
    let provided_summary_hash = optional_summary_hash(&args.summary_file);
    if let (Some(current_qa_branch_closure_id), Some(current_result), Some(current_summary_hash)) = (
        authoritative_state.current_qa_branch_closure_id(),
        authoritative_state.current_qa_result(),
        authoritative_state.current_qa_summary_hash(),
    ) && current_qa_branch_closure_id == branch_closure_id
    {
        let equivalent_current_result = provided_summary_hash.as_deref()
            == Some(current_summary_hash)
            && current_result == args.result.as_str();
        if provided_summary_hash.is_some() && !equivalent_current_result {
            return Ok(RecordQaOutput {
                action: String::from("blocked"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: None,
                trace_summary: String::from(
                    "record-qa failed closed because the current branch closure already has a conflicting recorded browser QA outcome.",
                ),
            });
        }
    }
    let final_review_record_id = authoritative_state
        .current_final_review_record_id()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "record-qa requires a current final-review record id.",
            )
        })?;
    let test_plan_path = if let Some(path) = current_authoritative_test_plan_path_from_qa_record(
        runtime,
        authoritative_state,
        &branch_closure_id,
        &final_review_record_id,
    ) {
        Some(path)
    } else {
        match current_test_plan_artifact_path(&context) {
            Ok(path) => Some(path),
            Err(error)
                if error.error_class == FailureClass::ExecutionStateNotReady.as_str()
                    || error.error_class == FailureClass::QaArtifactNotFresh.as_str() =>
            {
                return Ok(RecordQaOutput {
                    action: String::from("blocked"),
                    branch_closure_id,
                    result: args.result.as_str().to_owned(),
                    code: Some(String::from("out_of_phase_requery_required")),
                    recommended_command: Some(recommended_operator_command(&args.plan, false)),
                    rederive_via_workflow_operator: Some(true),
                    required_follow_up: None,
                    trace_summary: String::from(
                        "record-qa failed closed because workflow/operator must refresh the current test plan before QA recording can proceed.",
                    ),
                });
            }
            Err(error) => return Err(error),
        }
    };
    let summary = read_nonempty_summary_file(&args.summary_file, "summary")?;
    let summary_hash = qa_summary_hash(&summary);
    let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
    let semantic_reviewed_state_id = current_branch_closure.semantic_reviewed_state_id.clone();
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::QaArtifactNotFresh,
            "record-qa requires a resolvable base branch.",
        )
    })?;
    let qa_source = render_qa_artifact(
        runtime,
        &context,
        QaProjectionInput {
            branch_closure_id: &branch_closure_id,
            reviewed_state_id: &reviewed_state_id,
            result: args.result.as_str(),
            summary: &summary,
            base_branch: &base_branch,
            test_plan_path: test_plan_path.as_deref(),
        },
    )?;
    let authoritative_test_plan_write = if let Some(test_plan_path) = test_plan_path.as_deref() {
        let authoritative_test_plan_source =
            fs::read_to_string(test_plan_path).map_err(|error| {
                JsonFailure::new(
                    FailureClass::EvidenceWriteFailed,
                    format!(
                        "Could not read current test-plan artifact {}: {error}",
                        test_plan_path.display()
                    ),
                )
            })?;
        let authoritative_test_plan_fingerprint =
            sha256_hex(authoritative_test_plan_source.as_bytes());
        let authoritative_test_plan_path = harness_authoritative_artifact_path(
            &runtime.state_dir,
            &runtime.repo_slug,
            &runtime.branch_name,
            &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
        );
        let needs_publish = authoritative_test_plan_path != test_plan_path;
        Some((
            authoritative_test_plan_path,
            authoritative_test_plan_source,
            authoritative_test_plan_fingerprint,
            needs_publish,
        ))
    } else {
        None
    };
    let source_test_plan_fingerprint = authoritative_test_plan_write
        .as_ref()
        .map(|(_, _, fingerprint, _)| fingerprint.clone());
    let authoritative_qa_source = if let Some((authoritative_test_plan_path, _, _, _)) =
        authoritative_test_plan_write.as_ref()
    {
        rewrite_rebuild_source_test_plan_header(&qa_source, authoritative_test_plan_path)
    } else {
        qa_source.clone()
    };
    let qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    let authoritative_qa_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("browser-qa-{qa_fingerprint}.md"),
    );
    record_browser_qa(
        authoritative_state,
        BrowserQaWrite {
            branch_closure_id: &branch_closure_id,
            final_review_record_id: &final_review_record_id,
            source_plan_path: &context.plan_rel,
            source_plan_revision: context.plan_document.plan_revision,
            repo_slug: &runtime.repo_slug,
            branch_name: &context.runtime.branch_name,
            base_branch: &base_branch,
            reviewed_state_id: &reviewed_state_id,
            semantic_reviewed_state_id: semantic_reviewed_state_id.as_deref(),
            result: args.result.as_str(),
            browser_qa_fingerprint: Some(qa_fingerprint.as_str()),
            source_test_plan_fingerprint: source_test_plan_fingerprint.as_deref(),
            summary: &summary,
            summary_hash: &summary_hash,
            generated_by_identity: "featureforge/qa",
        },
    )?;
    if let Some((authoritative_test_plan_path, authoritative_test_plan_source, _, true)) =
        authoritative_test_plan_write
    {
        write_atomic_file(
            &authoritative_test_plan_path,
            &authoritative_test_plan_source,
        )
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!(
                    "Could not write current test-plan artifact {}: {error}",
                    authoritative_test_plan_path.display()
                ),
            )
        })?;
    }
    write_project_artifact(
        runtime,
        &format!(
            "featureforge-{}-test-outcome-{}.md",
            runtime.safe_branch,
            timestamp_slug()
        ),
        &qa_source,
    )?;
    write_atomic_file(&authoritative_qa_path, &authoritative_qa_source).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not write browser QA artifact {}: {error}",
                authoritative_qa_path.display()
            ),
        )
    })?;
    let (
        code,
        recommended_command,
        rederive_via_workflow_operator,
        required_follow_up,
        trace_summary,
    ) = if args.result == ReviewOutcomeArg::Fail && qa_override_out_of_phase {
        (
            Some(String::from("out_of_phase_requery_required")),
            Some(recommended_operator_command(&args.plan, false)),
            Some(true),
            None,
            String::from(
                "Recorded browser QA evidence for the current branch closure; workflow/operator must be requeried to continue from the active override lane.",
            ),
        )
    } else {
        (
            None,
            None,
            None,
            (args.result == ReviewOutcomeArg::Fail)
                .then(|| {
                    negative_result_required_follow_up(
                        runtime,
                        &args.plan,
                        &operator,
                        Some(authoritative_state),
                    )
                })
                .flatten(),
            String::from(
                "Recorded browser QA evidence for the current branch closure and approved test plan.",
            ),
        )
    };
    Ok(RecordQaOutput {
        action: String::from("recorded"),
        branch_closure_id,
        result: args.result.as_str().to_owned(),
        code,
        recommended_command,
        rederive_via_workflow_operator,
        required_follow_up,
        trace_summary,
    })
}

pub fn rebuild_evidence(
    runtime: &ExecutionRuntime,
    args: &RebuildEvidenceArgs,
) -> Result<RebuildEvidenceOutput, JsonFailure> {
    let request = normalize_rebuild_evidence_request(args)?;
    if request.max_jobs > 1 {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "max_jobs_parallel_unsupported: rebuild-evidence currently supports only --max-jobs 1.",
        ));
    }
    let started_at = Instant::now();
    let context = load_execution_context_for_rebuild(runtime, &request.plan)?;
    if context.evidence.source.is_none() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "session_not_found: no execution evidence session exists for the approved plan revision.",
        ));
    }
    let matched_scope_ids = matched_rebuild_scope_ids(&context, &request);
    let candidates = discover_rebuild_candidates(&context, &request)?;
    if (!request.tasks.is_empty() || !request.steps.is_empty()) && candidates.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!(
                "scope_empty: requested scope matched approved plan steps [{}] but none currently require rebuild.",
                matched_scope_ids.join(", ")
            ),
        ));
    }
    let filter = RebuildEvidenceFilter {
        all: request.all,
        tasks: request.tasks.clone(),
        steps: request.raw_steps.clone(),
        include_open: request.include_open,
        skip_manual_fallback: request.skip_manual_fallback,
        continue_on_error: request.continue_on_error,
        max_jobs: request.max_jobs,
        no_output: request.no_output,
        json: request.json,
    };
    let scope = rebuild_scope_label(&request);

    if request.dry_run {
        let targets = candidates
            .iter()
            .map(planned_rebuild_target)
            .collect::<Vec<_>>();
        return Ok(RebuildEvidenceOutput {
            session_root: context.runtime.repo_root.to_string_lossy().into_owned(),
            dry_run: true,
            filter,
            scope,
            counts: RebuildEvidenceCounts {
                planned: targets.len() as u32,
                rebuilt: 0,
                manual: 0,
                failed: 0,
                noop: u32::from(targets.is_empty()),
            },
            duration_ms: started_at.elapsed().as_millis() as u64,
            targets,
            exit_code: 0,
        });
    }

    if candidates.is_empty() {
        refresh_rebuild_downstream_truth(runtime, &args.plan)?;
        return Ok(RebuildEvidenceOutput {
            session_root: context.runtime.repo_root.to_string_lossy().into_owned(),
            dry_run: false,
            filter,
            scope,
            counts: RebuildEvidenceCounts {
                planned: 0,
                rebuilt: 0,
                manual: 0,
                failed: 0,
                noop: 1,
            },
            duration_ms: started_at.elapsed().as_millis() as u64,
            targets: Vec::new(),
            exit_code: 0,
        });
    }

    let mut targets = Vec::with_capacity(candidates.len());
    let mut counts = RebuildEvidenceCounts {
        planned: candidates.len() as u32,
        rebuilt: 0,
        manual: 0,
        failed: 0,
        noop: 0,
    };
    let candidate_batch_is_manual_only = request.skip_manual_fallback && !candidates.is_empty();
    let mut saw_strict_manual_failure = false;
    let mut saw_precondition_failure = false;
    let mut saw_non_precondition_failure = false;

    for (index, candidate) in candidates.iter().enumerate() {
        let target = execute_rebuild_candidate_projection_only(&request, candidate);
        match target.status.as_str() {
            "rebuilt" => counts.rebuilt += 1,
            "manual_required" => counts.manual += 1,
            "failed" => {
                counts.failed += 1;
                match target.failure_class.as_deref() {
                    Some("manual_required") => {
                        saw_strict_manual_failure = true;
                    }
                    Some(failure_class) if is_rebuild_precondition_failure(failure_class) => {
                        saw_precondition_failure = true;
                    }
                    _ => {
                        saw_non_precondition_failure = true;
                    }
                }
            }
            _ => {}
        }
        let should_stop = target.status == "failed"
            && target.failure_class.as_deref() != Some("artifact_read_error")
            && !request.continue_on_error;
        targets.push(target);
        if should_stop || index + 1 == candidates.len() {
            break;
        }
    }

    let strict_manual_only = candidate_batch_is_manual_only
        && saw_strict_manual_failure
        && !saw_precondition_failure
        && !saw_non_precondition_failure;
    refresh_rebuild_downstream_truth(runtime, &args.plan)?;
    let exit_code = if strict_manual_only {
        3
    } else if saw_non_precondition_failure || saw_strict_manual_failure {
        2
    } else if saw_precondition_failure {
        1
    } else {
        0
    };

    Ok(RebuildEvidenceOutput {
        session_root: context.runtime.repo_root.to_string_lossy().into_owned(),
        dry_run: false,
        filter,
        scope,
        counts,
        duration_ms: started_at.elapsed().as_millis() as u64,
        targets,
        exit_code,
    })
}

pub fn materialize_projections(
    runtime: &ExecutionRuntime,
    args: &MaterializeProjectionsArgs,
) -> Result<MaterializeProjectionsOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let mode = if args.state_dir {
        ProjectionWriteMode::StateDirOnly
    } else {
        ProjectionWriteMode::ProjectionExport
    };
    let mut written_paths = Vec::new();
    if matches!(
        args.scope,
        MaterializeProjectionScopeArg::Execution | MaterializeProjectionScopeArg::All
    ) {
        crate::execution::state::validate_state_dir_evidence_projection_before_materialization(
            &context,
        )?;
        let rendered = render_execution_projections(&context);
        written_paths.extend(write_execution_projection_read_models(
            &context, &rendered, mode,
        )?);
    }
    if matches!(
        args.scope,
        MaterializeProjectionScopeArg::LateStage | MaterializeProjectionScopeArg::All
    ) {
        let authoritative_state = load_authoritative_transition_state(&context)?;
        if let Some(authoritative_state) = authoritative_state.as_ref() {
            written_paths.extend(materialize_late_stage_projection_artifacts(
                runtime,
                &context,
                authoritative_state,
                mode,
            )?);
        }
    }
    Ok(MaterializeProjectionsOutput {
        action: String::from("materialized"),
        projection_mode: mode.as_str().to_owned(),
        written_paths,
        runtime_truth_changed: false,
        trace_summary: match mode {
            ProjectionWriteMode::ProjectionExport if args.tracked => String::from(
                "Materialized projection export files from authoritative runtime state; `--tracked` is a deprecated alias and approved plan/evidence files were not modified.",
            ),
            ProjectionWriteMode::ProjectionExport => String::from(
                "Materialized projection export files from authoritative runtime state; approved plan/evidence files were not modified.",
            ),
            ProjectionWriteMode::StateDirOnly => String::from(
                "Materialized state-dir projection files from authoritative runtime state.",
            ),
            ProjectionWriteMode::Disabled => {
                String::from("Projection materialization was disabled; no files were written.")
            }
        },
    })
}

fn is_rebuild_precondition_failure(failure_class: &str) -> bool {
    matches!(
        failure_class,
        "artifact_read_error" | "state_transition_blocked" | "target_race"
    )
}

fn execute_rebuild_candidate_projection_only(
    request: &crate::execution::state::RebuildEvidenceRequest,
    candidate: &RebuildEvidenceCandidate,
) -> RebuildEvidenceTarget {
    let attempt_id_before = candidate
        .attempt_number
        .map(|attempt| format!("{}:{}:{}", candidate.task, candidate.step, attempt));
    let mut target = RebuildEvidenceTarget {
        task_id: candidate.task,
        step_id: candidate.step,
        target_kind: candidate.target_kind.clone(),
        pre_invalidation_reason: candidate.pre_invalidation_reason.clone(),
        status: String::from("planned"),
        verify_mode: candidate.verify_mode.clone(),
        verify_command: candidate.verify_command.clone(),
        attempt_id_before,
        attempt_id_after: None,
        verification_hash: None,
        error: None,
        failure_class: None,
    };

    if candidate.target_kind == "artifact_read_error" {
        target.status = String::from("failed");
        target.failure_class = Some(String::from("artifact_read_error"));
        target.error = Some(candidate.pre_invalidation_reason.clone());
        return target;
    }
    let projection_only_message = String::from(
        "projection_only: rebuild-evidence only regenerates derived projections; replay stale execution with reopen/begin/complete when execution work must be rerun.",
    );
    if request.skip_manual_fallback {
        target.status = String::from("failed");
        target.failure_class = Some(String::from("manual_required"));
        target.error = Some(format!("manual_required: {projection_only_message}"));
        return target;
    }
    if target.failure_class.is_none() {
        target.status = String::from("manual_required");
        target.failure_class = Some(String::from("manual_required"));
        target.error = Some(projection_only_message);
    }
    target
}

fn refresh_rebuild_downstream_truth(
    runtime: &ExecutionRuntime,
    plan: &Path,
) -> Result<(), JsonFailure> {
    let context = load_execution_context_for_rebuild(runtime, plan)?;
    let _write_authority = claim_step_write_authority(runtime)?;
    let rendered = render_execution_projections(&context);
    let _ = write_execution_projection_read_models(
        &context,
        &rendered,
        ProjectionWriteMode::StateDirOnly,
    )?;
    let authoritative_state = load_authoritative_transition_state(&context)?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        let _ = regenerate_projection_artifacts_from_authoritative_state(
            runtime,
            &context,
            authoritative_state,
        )?;
    }
    Ok(())
}

fn ensure_task_dispatch_id_matches(
    context: &ExecutionContext,
    task: u32,
    dispatch_id: &str,
) -> Result<(), JsonFailure> {
    let lineage_key = format!("task-{task}");
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let expected_dispatch_from_lineage = overlay
        .as_ref()
        .and_then(|overlay| {
            overlay
                .strategy_review_dispatch_lineage
                .get(&lineage_key)
                .and_then(|record| record.dispatch_id.as_deref())
                .map(str::to_owned)
        })
        .or_else(|| {
            load_authoritative_transition_state(context)
                .ok()
                .flatten()
                .and_then(|state| state.task_review_dispatch_id(task))
        })
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if let Some(expected_dispatch) = expected_dispatch_from_lineage.as_deref() {
        if expected_dispatch != dispatch_id.trim() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "dispatch_id_mismatch: close-current-task expected dispatch `{expected_dispatch}` for task {task}."
                ),
            ));
        }
        return Ok(());
    }
    Err(JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        format!(
            "close-current-task requires a current task review dispatch lineage for task {task}."
        ),
    ))
}

fn task_dispatch_reviewed_state_status(
    context: &ExecutionContext,
    task: u32,
    semantic_reviewed_state_id: &str,
    raw_reviewed_state_id: &str,
) -> Result<TaskDispatchReviewedStateStatus, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "close-current-task requires authoritative review-dispatch lineage state.",
        )
    })?;
    let lineage_key = format!("task-{task}");
    let recorded_semantic_reviewed_state_id = overlay
        .strategy_review_dispatch_lineage
        .get(&lineage_key)
        .and_then(|record| record.semantic_reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let recorded_raw_reviewed_state_id = overlay
        .strategy_review_dispatch_lineage
        .get(&lineage_key)
        .and_then(|record| record.reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(
        match (
            recorded_semantic_reviewed_state_id,
            recorded_raw_reviewed_state_id,
        ) {
            (Some(recorded), _) if recorded == semantic_reviewed_state_id.trim() => {
                TaskDispatchReviewedStateStatus::Current
            }
            (Some(_), _) => TaskDispatchReviewedStateStatus::StaleReviewedState,
            (None, Some(recorded)) if recorded == raw_reviewed_state_id.trim() => {
                TaskDispatchReviewedStateStatus::Current
            }
            (None, Some(_)) => TaskDispatchReviewedStateStatus::StaleReviewedState,
            (None, None) => TaskDispatchReviewedStateStatus::MissingReviewedStateBinding,
        },
    )
}

fn ensure_final_review_dispatch_id_matches(
    context: &ExecutionContext,
    dispatch_id: &str,
) -> Result<(), JsonFailure> {
    let current_branch_closure =
        authoritative_current_branch_closure_binding(context, "advance-late-stage final-review")?;
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage final-review path requires authoritative dispatch lineage state.",
        )
    })?;
    let expected_dispatch_from_lineage = overlay
        .final_review_dispatch_lineage
        .as_ref()
        .and_then(|record| {
            let expected_branch_closure_id = record.branch_closure_id.as_deref()?;
            if current_branch_closure.branch_closure_id != expected_branch_closure_id {
                return None;
            }
            record.dispatch_id.as_deref()
        })
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(expected_dispatch) = expected_dispatch_from_lineage {
        if expected_dispatch != dispatch_id.trim() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "dispatch_id_mismatch: advance-late-stage expected final-review dispatch `{expected_dispatch}`."
                ),
            ));
        }
        return Ok(());
    }
    Err(JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        "advance-late-stage final-review path requires a current final-review dispatch lineage.",
    ))
}

fn close_current_task_summary_hashes(
    args: &CloseCurrentTaskArgs,
) -> Result<(String, String), JsonFailure> {
    let review_summary = read_nonempty_summary_file(&args.review_summary_file, "review summary")?;
    let review_summary_hash = summary_hash(&review_summary);
    let verification_summary_hash = if matches!(
        args.verification_result,
        VerificationOutcomeArg::Pass | VerificationOutcomeArg::Fail
    ) {
        let verification_summary = read_nonempty_summary_file(
            args.verification_summary_file.as_ref().ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "verification_summary_required: close-current-task requires --verification-summary-file when --verification-result=pass|fail.",
                )
            })?,
            "verification summary",
        )?;
        summary_hash(&verification_summary)
    } else {
        String::new()
    };
    Ok((review_summary_hash, verification_summary_hash))
}

fn superseded_branch_closure_ids_from_previous_current(
    overlay: Option<&StatusAuthoritativeOverlay>,
    branch_closure_id: &str,
) -> Vec<String> {
    overlay
        .and_then(|overlay| overlay.current_branch_closure_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != branch_closure_id)
        .map(|value| vec![value.to_owned()])
        .unwrap_or_default()
}

fn read_nonempty_summary_file(path: &Path, label: &str) -> Result<String, JsonFailure> {
    let source = fs::read_to_string(path).map_err(|error| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("Could not read {label} file {}: {error}", path.display()),
        )
    })?;
    let normalized = normalize_summary_content(&source);
    if normalized.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("{label}_empty: {label} file may not be blank after whitespace normalization."),
        ));
    }
    Ok(normalized)
}

fn optional_summary_hash(path: &Path) -> Option<String> {
    let source = fs::read_to_string(path).ok()?;
    let normalized = normalize_summary_content(&source);
    if normalized.is_empty() {
        return None;
    }
    Some(summary_hash(&normalized))
}

fn current_plan_requires_browser_qa(context: &ExecutionContext) -> Option<bool> {
    match context.plan_document.qa_requirement.as_deref() {
        Some("required") => Some(true),
        Some("not-required") => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CurrentBranchClosureBinding {
    branch_closure_id: String,
    reviewed_state_id: String,
    semantic_reviewed_state_id: Option<String>,
}

fn current_authoritative_branch_closure_binding_optional(
    context: &ExecutionContext,
) -> Result<Option<CurrentBranchClosureBinding>, JsonFailure> {
    Ok(
        usable_current_branch_closure_identity(context).map(|current_identity| {
            CurrentBranchClosureBinding {
                branch_closure_id: current_identity.branch_closure_id,
                reviewed_state_id: current_identity.reviewed_state_id,
                semantic_reviewed_state_id: current_identity.semantic_reviewed_state_id,
            }
        }),
    )
}

fn current_authoritative_branch_closure_id_optional(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    Ok(
        current_authoritative_branch_closure_binding_optional(context)?
            .map(|binding| binding.branch_closure_id),
    )
}

fn authoritative_current_branch_closure_binding(
    context: &ExecutionContext,
    command_label: &str,
) -> Result<CurrentBranchClosureBinding, JsonFailure> {
    let Some(current_identity) = current_authoritative_branch_closure_binding_optional(context)?
    else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!("{command_label} requires a current branch closure."),
        ));
    };
    Ok(current_identity)
}

fn equivalent_current_release_readiness_rerun(
    context: &ExecutionContext,
    current_branch_closure: &CurrentBranchClosureBinding,
    stage_path: &str,
    delegated_primitive: &str,
    result: &str,
    summary_file: &Path,
) -> Result<Option<AdvanceLateStageOutput>, JsonFailure> {
    let Some(candidate_summary_hash) = optional_summary_hash(summary_file) else {
        return Ok(None);
    };
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(None);
    };
    let Some(record) = authoritative_state.current_release_readiness_record() else {
        return Ok(None);
    };
    if record.branch_closure_id != current_branch_closure.branch_closure_id
        || record.result != result
        || record.summary_hash != candidate_summary_hash
    {
        return Ok(None);
    }
    Ok(Some(AdvanceLateStageOutput {
        action: String::from("already_current"),
        stage_path: stage_path.to_owned(),
        delegated_primitive: delegated_primitive.to_owned(),
        branch_closure_id: Some(current_branch_closure.branch_closure_id.clone()),
        dispatch_id: None,
        result: result.to_owned(),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        required_follow_up: (result == "blocked").then(|| String::from("resolve_release_blocker")),
        trace_summary: String::from(
            "Current branch closure already has an equivalent recorded release-readiness outcome.",
        ),
    }))
}

fn equivalent_current_final_review_rerun(
    context: &ExecutionContext,
    current_branch_closure: &CurrentBranchClosureBinding,
    params: EquivalentFinalReviewRerunParams<'_>,
) -> Result<Option<AdvanceLateStageOutput>, JsonFailure> {
    let Some(candidate_summary_hash) = optional_summary_hash(params.summary_file) else {
        return Ok(None);
    };
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(None);
    };
    let matches_current_record = authoritative_state.current_final_review_branch_closure_id()
        == Some(current_branch_closure.branch_closure_id.as_str())
        && authoritative_state.current_final_review_dispatch_id() == Some(params.dispatch_id)
        && authoritative_state.current_final_review_reviewer_source()
            == Some(params.reviewer_source)
        && authoritative_state.current_final_review_reviewer_id() == Some(params.reviewer_id)
        && authoritative_state.current_final_review_result() == Some(params.result)
        && authoritative_state.current_final_review_summary_hash()
            == Some(candidate_summary_hash.as_str());
    if !matches_current_record {
        return Ok(None);
    }
    if !current_final_review_record_is_still_authoritative(
        context,
        authoritative_state,
        CurrentFinalReviewAuthorityCheck {
            branch_closure_id: &current_branch_closure.branch_closure_id,
            dispatch_id: params.dispatch_id,
            reviewer_source: params.reviewer_source,
            reviewer_id: params.reviewer_id,
            result: params.result,
            normalized_summary_hash: &candidate_summary_hash,
        },
    )? {
        return Ok(None);
    }
    Ok(Some(AdvanceLateStageOutput {
        action: String::from("already_current"),
        stage_path: params.stage_path.to_owned(),
        delegated_primitive: params.delegated_primitive.to_owned(),
        branch_closure_id: Some(current_branch_closure.branch_closure_id.clone()),
        dispatch_id: Some(params.dispatch_id.to_owned()),
        result: params.result.to_owned(),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        required_follow_up: params.required_follow_up,
        trace_summary: String::from(
            "Current branch closure already has an equivalent recorded final-review outcome.",
        ),
    }))
}

fn equivalent_current_browser_qa_rerun(
    context: &ExecutionContext,
    current_branch_closure: &CurrentBranchClosureBinding,
    gate_snapshot: &RuntimeGateSnapshot,
    result: &str,
    summary_file: &Path,
    required_follow_up: Option<String>,
) -> Result<Option<RecordQaOutput>, JsonFailure> {
    let Some(candidate_summary_hash) = optional_summary_hash(summary_file) else {
        return Ok(None);
    };
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(None);
    };
    let matches_current_record = authoritative_state.current_qa_branch_closure_id()
        == Some(current_branch_closure.branch_closure_id.as_str())
        && authoritative_state.current_qa_result() == Some(result)
        && authoritative_state.current_qa_summary_hash() == Some(candidate_summary_hash.as_str());
    if !matches_current_record {
        return Ok(None);
    }
    if rerun_invalidated_by_repo_writes(
        gate_snapshot.gate_review.as_ref(),
        gate_snapshot.gate_finish.as_ref(),
    ) {
        return Ok(None);
    }
    let current_record = authoritative_state.current_browser_qa_record();
    if current_record
        .as_ref()
        .and_then(|record| record.source_test_plan_fingerprint.as_deref())
        .map(str::trim)
        .filter(|fingerprint| !fingerprint.is_empty())
        .is_none()
    {
        match current_test_plan_artifact_path(context) {
            Ok(_) => {}
            Err(error)
                if error.error_class == FailureClass::ExecutionStateNotReady.as_str()
                    || error.error_class == FailureClass::QaArtifactNotFresh.as_str() =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Some(RecordQaOutput {
        action: String::from("already_current"),
        branch_closure_id: current_branch_closure.branch_closure_id.clone(),
        result: result.to_owned(),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        required_follow_up,
        trace_summary: String::from(
            "Current branch closure already has an equivalent recorded browser QA outcome.",
        ),
    }))
}

fn equivalent_current_browser_qa_rerun_allowed(
    operator: &ExecutionRoutingState,
    result: &str,
) -> bool {
    if operator.review_state_status != "clean" {
        return false;
    }
    match result {
        "pass" => {
            operator.phase == "qa_pending" && operator.phase_detail == "qa_recording_required"
        }
        "fail" => matches!(
            operator.phase_detail.as_str(),
            "execution_reentry_required"
                | "handoff_recording_required"
                | "planning_reentry_required"
        ),
        _ => false,
    }
}

fn rerun_invalidated_by_repo_writes(
    gate_review: Option<&crate::execution::state::GateResult>,
    gate_finish: Option<&crate::execution::state::GateResult>,
) -> bool {
    const REPO_WRITE_INVALIDATION_CODES: &[&str] = &[
        "review_artifact_worktree_dirty",
        REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
    ];
    let gate_has_reason = |gate: Option<&crate::execution::state::GateResult>| {
        gate.is_some_and(|gate| {
            gate.reason_codes.iter().any(|code| {
                REPO_WRITE_INVALIDATION_CODES
                    .iter()
                    .any(|expected| code == expected)
            })
        })
    };
    gate_has_reason(gate_review) || gate_has_reason(gate_finish)
}

fn current_test_plan_artifact_path(context: &ExecutionContext) -> Result<PathBuf, JsonFailure> {
    current_test_plan_artifact_path_for_qa_recording(context)
}

fn current_authoritative_test_plan_path_from_qa_record(
    runtime: &ExecutionRuntime,
    authoritative_state: &AuthoritativeTransitionState,
    branch_closure_id: &str,
    final_review_record_id: &str,
) -> Option<PathBuf> {
    let record = authoritative_state.current_browser_qa_record()?;
    if record.branch_closure_id != branch_closure_id
        || record.final_review_record_id.as_deref() != Some(final_review_record_id)
    {
        return None;
    }
    let fingerprint = record
        .source_test_plan_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("test-plan-{fingerprint}.md"),
    ))
}

fn current_workflow_operator(
    runtime: &ExecutionRuntime,
    plan: &Path,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    let (routing, _) =
        current_workflow_operator_with_runtime_state(runtime, plan, external_review_result_ready)?;
    Ok(routing)
}

fn current_workflow_operator_with_runtime_state(
    runtime: &ExecutionRuntime,
    plan: &Path,
    external_review_result_ready: bool,
) -> Result<(ExecutionRoutingState, RuntimeState), JsonFailure> {
    // Mutators consume the execution-owned routing boundary here instead of calling
    // `query_workflow_routing_state_for_runtime` directly, but they still project the same
    // execution query contract through the shared router decision.
    let read_scope = load_execution_read_scope_for_mutation(runtime, plan, true)?;
    let (mut routing, route_decision, runtime_state) =
        project_runtime_routing_state_with_reduced_state(
            &read_scope,
            external_review_result_ready,
            false,
        )?;
    routing.phase = route_decision.phase;
    routing.phase_detail = route_decision.phase_detail;
    routing.review_state_status = route_decision.review_state_status;
    routing.next_action = route_decision.next_action;
    routing.recommended_command = route_decision.recommended_command;
    Ok((routing, runtime_state))
}

fn negative_result_required_follow_up(
    runtime: &ExecutionRuntime,
    plan: &Path,
    operator_with_external_ready: &ExecutionRoutingState,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<String> {
    let task_negative_result_present = operator_with_external_ready
        .blocking_task
        .and_then(|task| {
            authoritative_state.and_then(|state| state.task_closure_negative_result(task))
        })
        .is_some();
    let current_branch_closure_id = operator_with_external_ready
        .current_branch_closure_id
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let current_final_review =
        authoritative_state.and_then(AuthoritativeTransitionState::current_final_review_record);
    let current_browser_qa =
        authoritative_state.and_then(AuthoritativeTransitionState::current_browser_qa_record);
    if shared_negative_result_requires_execution_reentry(
        task_negative_result_present,
        operator_with_external_ready.workflow_phase.as_str(),
        current_branch_closure_id,
        current_final_review
            .as_ref()
            .map(|record| record.branch_closure_id.as_str()),
        current_final_review
            .as_ref()
            .map(|record| record.result.as_str()),
        current_browser_qa
            .as_ref()
            .map(|record| record.branch_closure_id.as_str()),
        current_browser_qa
            .as_ref()
            .map(|record| record.result.as_str()),
    ) {
        return Some(String::from("execution_reentry"));
    }
    let _ = (runtime, plan);
    negative_result_follow_up(operator_with_external_ready)
}

fn late_stage_negative_result_override_active(operator: &ExecutionRoutingState) -> bool {
    matches!(
        operator.phase_detail.as_str(),
        "handoff_recording_required" | "planning_reentry_required"
    )
}

fn recommended_operator_command(plan: &Path, external_review_result_ready: bool) -> String {
    workflow_operator_requery_command(plan, external_review_result_ready)
}

fn close_current_task_command_matches_follow_up(
    required_follow_up: Option<&str>,
    recommended_command: &str,
) -> bool {
    match required_follow_up {
        Some("execution_reentry") => {
            recommended_command.starts_with("featureforge plan execution begin --plan ")
                || recommended_command.starts_with("featureforge plan execution reopen --plan ")
                || recommended_command.starts_with("featureforge plan execution complete --plan ")
        }
        Some("repair_review_state") => recommended_command
            .starts_with("featureforge plan execution repair-review-state --plan "),
        Some("request_external_review")
        | Some("wait_for_external_review_result")
        | Some("run_verification") => {
            recommended_command.starts_with("featureforge workflow operator --plan ")
        }
        Some("record_handoff") => {
            recommended_command.starts_with("featureforge plan execution transfer --plan ")
        }
        Some("advance_late_stage") | Some("resolve_release_blocker") => {
            recommended_command.contains("featureforge plan execution advance-late-stage --plan")
        }
        Some(_) | None => false,
    }
}

fn close_current_task_follow_up_and_command(
    operator: &ExecutionRoutingState,
) -> (Option<String>, Option<String>) {
    let required_follow_up = close_current_task_required_follow_up(operator);
    let recommended_command = required_follow_up.as_ref().and_then(|follow_up| {
        operator
            .recommended_command
            .clone()
            .filter(|recommended_command| {
                close_current_task_command_matches_follow_up(
                    Some(follow_up.as_str()),
                    recommended_command.as_str(),
                )
            })
    });
    (required_follow_up, recommended_command)
}

fn with_close_current_task_operator_blocker_metadata(
    mut output: CloseCurrentTaskOutput,
    operator: &ExecutionRoutingState,
) -> CloseCurrentTaskOutput {
    output.blocking_scope = operator.blocking_scope.clone();
    output.blocking_task = operator.blocking_task;
    output.blocking_reason_codes = operator.blocking_reason_codes.clone();
    output.authoritative_next_action = operator.recommended_command.clone();
    output
}

#[cfg(test)]
fn blocked_close_current_task_output_from_operator(
    task_number: u32,
    operator: &ExecutionRoutingState,
    trace_summary: &str,
) -> CloseCurrentTaskOutput {
    let (required_follow_up, recommended_command) =
        close_current_task_follow_up_and_command(operator);
    with_close_current_task_operator_blocker_metadata(
        CloseCurrentTaskOutput {
            action: String::from("blocked"),
            task_number,
            dispatch_validation_action: String::from("blocked"),
            closure_action: String::from("blocked"),
            task_closure_status: String::from("not_current"),
            superseded_task_closure_ids: Vec::new(),
            closure_record_id: None,
            code: None,
            recommended_command,
            rederive_via_workflow_operator: None,
            required_follow_up,
            blocking_scope: None,
            blocking_task: None,
            blocking_reason_codes: Vec::new(),
            authoritative_next_action: None,
            trace_summary: trace_summary.to_owned(),
        },
        operator,
    )
}

fn blocked_close_current_task_output(
    params: BlockedCloseCurrentTaskOutputContext<'_>,
) -> CloseCurrentTaskOutput {
    let BlockedCloseCurrentTaskOutputContext {
        task_number,
        dispatch_validation_action,
        task_closure_status,
        closure_record_id,
        code,
        recommended_command,
        rederive_via_workflow_operator,
        required_follow_up,
        trace_summary,
    } = params;
    CloseCurrentTaskOutput {
        action: String::from("blocked"),
        task_number,
        dispatch_validation_action: dispatch_validation_action.to_owned(),
        closure_action: String::from("blocked"),
        task_closure_status: task_closure_status.to_owned(),
        superseded_task_closure_ids: Vec::new(),
        closure_record_id,
        code,
        recommended_command,
        rederive_via_workflow_operator,
        required_follow_up,
        blocking_scope: None,
        blocking_task: None,
        blocking_reason_codes: Vec::new(),
        authoritative_next_action: None,
        trace_summary: trace_summary.to_owned(),
    }
}

fn shared_out_of_phase_record_branch_closure_output(
    plan: &Path,
    branch_closure_id: Option<String>,
    trace_summary: &str,
) -> RecordBranchClosureOutput {
    RecordBranchClosureOutput {
        action: String::from("blocked"),
        branch_closure_id,
        code: Some(String::from("out_of_phase_requery_required")),
        recommended_command: Some(recommended_operator_command(plan, false)),
        rederive_via_workflow_operator: Some(true),
        superseded_branch_closure_ids: Vec::new(),
        required_follow_up: None,
        trace_summary: trace_summary.to_owned(),
    }
}

fn shared_out_of_phase_advance_late_stage_output(
    plan: &Path,
    params: AdvanceLateStageOutputContext<'_>,
) -> AdvanceLateStageOutput {
    let AdvanceLateStageOutputContext {
        stage_path,
        delegated_primitive,
        branch_closure_id,
        dispatch_id,
        result,
        external_review_result_ready,
        trace_summary,
    } = params;
    AdvanceLateStageOutput {
        action: String::from("blocked"),
        stage_path: stage_path.to_owned(),
        delegated_primitive: delegated_primitive.to_owned(),
        branch_closure_id,
        dispatch_id,
        result: result.to_owned(),
        code: Some(String::from("out_of_phase_requery_required")),
        recommended_command: Some(recommended_operator_command(
            plan,
            external_review_result_ready,
        )),
        rederive_via_workflow_operator: Some(true),
        required_follow_up: None,
        trace_summary: trace_summary.to_owned(),
    }
}

struct AdvanceLateStageOutputContext<'a> {
    stage_path: &'a str,
    delegated_primitive: &'a str,
    branch_closure_id: Option<String>,
    dispatch_id: Option<String>,
    result: &'a str,
    external_review_result_ready: bool,
    trace_summary: &'a str,
}

fn advance_late_stage_follow_up_or_requery_output(
    operator: &ExecutionRoutingState,
    plan: &Path,
    dispatch_lineage_matches: bool,
    params: AdvanceLateStageOutputContext<'_>,
) -> AdvanceLateStageOutput {
    let AdvanceLateStageOutputContext {
        stage_path,
        delegated_primitive,
        branch_closure_id,
        dispatch_id,
        result,
        external_review_result_ready,
        trace_summary,
    } = params;
    if let Some(required_follow_up) = late_stage_required_follow_up(stage_path, operator) {
        if stage_path == "final_review"
            && required_follow_up == "request_external_review"
            && dispatch_id.is_some()
            && !dispatch_lineage_matches
        {
            return shared_out_of_phase_advance_late_stage_output(
                plan,
                AdvanceLateStageOutputContext {
                    stage_path,
                    delegated_primitive,
                    branch_closure_id,
                    dispatch_id,
                    result,
                    external_review_result_ready,
                    trace_summary,
                },
            );
        }
        return AdvanceLateStageOutput {
            action: String::from("blocked"),
            stage_path: stage_path.to_owned(),
            delegated_primitive: delegated_primitive.to_owned(),
            branch_closure_id,
            dispatch_id,
            result: result.to_owned(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            required_follow_up: Some(required_follow_up),
            trace_summary: trace_summary.to_owned(),
        };
    }
    shared_out_of_phase_advance_late_stage_output(
        plan,
        AdvanceLateStageOutputContext {
            stage_path,
            delegated_primitive,
            branch_closure_id,
            dispatch_id,
            result,
            external_review_result_ready,
            trace_summary,
        },
    )
}

fn release_readiness_follow_up_or_requery_output(
    operator: &ExecutionRoutingState,
    plan: &Path,
    params: AdvanceLateStageOutputContext<'_>,
) -> AdvanceLateStageOutput {
    let AdvanceLateStageOutputContext {
        stage_path,
        delegated_primitive,
        branch_closure_id,
        dispatch_id,
        result,
        external_review_result_ready,
        trace_summary,
    } = params;
    if let Some(required_follow_up) = release_readiness_required_follow_up(operator) {
        return AdvanceLateStageOutput {
            action: String::from("blocked"),
            stage_path: stage_path.to_owned(),
            delegated_primitive: delegated_primitive.to_owned(),
            branch_closure_id,
            dispatch_id,
            result: result.to_owned(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            required_follow_up: Some(required_follow_up),
            trace_summary: trace_summary.to_owned(),
        };
    }
    shared_out_of_phase_advance_late_stage_output(
        plan,
        AdvanceLateStageOutputContext {
            stage_path,
            delegated_primitive,
            branch_closure_id,
            dispatch_id,
            result,
            external_review_result_ready,
            trace_summary,
        },
    )
}

fn qa_summary_hash(summary: &str) -> String {
    summary_hash(summary)
}

fn deterministic_record_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for part in parts {
        hasher.update(b"\n");
        hasher.update(part.as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{prefix}-{}", &digest[..16])
}

struct BranchReviewedState {
    base_branch: String,
    contract_identity: String,
    effective_reviewed_branch_surface: String,
    provenance_basis: String,
    reviewed_state_id: String,
    semantic_reviewed_state_id: String,
    source_task_closure_ids: Vec<String>,
}

struct TaskClosureReceiptRefresh<'a> {
    execution_run_id: &'a str,
    strategy_checkpoint_fingerprint: &'a str,
    active_contract_fingerprint: Option<&'a str>,
    task: u32,
    claim_write_authority: bool,
}

struct CurrentTaskClosureMaterialization<'a> {
    task: u32,
    dispatch_id: &'a str,
    closure_record_id: &'a str,
    execution_run_id: &'a str,
    reviewed_state_id: &'a str,
    semantic_reviewed_state_id: &'a str,
    contract_identity: &'a str,
    effective_reviewed_surface_paths: &'a [String],
    review_result: &'a str,
    review_summary_hash: &'a str,
    verification_result: &'a str,
    verification_summary_hash: &'a str,
    superseded_tasks: &'a [u32],
    superseded_task_closure_ids: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskDispatchReviewedStateStatus {
    Current,
    MissingReviewedStateBinding,
    StaleReviewedState,
}

fn current_branch_reviewed_state(
    context: &ExecutionContext,
) -> Result<BranchReviewedState, JsonFailure> {
    let source_task_closure_ids = current_branch_source_task_closure_ids(context)?;
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            "record-branch-closure requires an authoritative base-branch binding.",
        )
    })?;
    let reviewed_state_id = format!("git_tree:{}", context.current_tracked_tree_sha()?);
    let semantic_reviewed_state_id =
        semantic_workspace_snapshot(context)?.semantic_workspace_tree_id;
    Ok(BranchReviewedState {
        base_branch: base_branch.clone(),
        contract_identity: branch_definition_identity_for_context(context),
        effective_reviewed_branch_surface: String::from("repo_tracked_content"),
        provenance_basis: String::from("task_closure_lineage"),
        reviewed_state_id,
        semantic_reviewed_state_id,
        source_task_closure_ids,
    })
}

fn deterministic_branch_closure_record_id(
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> String {
    let source_task_closure_ids = reviewed_state.source_task_closure_ids.join("\n");
    deterministic_record_id(
        "branch-closure",
        &[
            &context.plan_rel,
            &context.runtime.branch_name,
            &reviewed_state.base_branch,
            &reviewed_state.semantic_reviewed_state_id,
            &reviewed_state.contract_identity,
            &reviewed_state.provenance_basis,
            &reviewed_state.effective_reviewed_branch_surface,
            source_task_closure_ids.as_str(),
        ],
    )
}

fn branch_closure_record_matches_reviewed_state(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> Result<bool, JsonFailure> {
    let semantic_matches =
        branch_closure_record_semantically_matches_reviewed_state(record, context, reviewed_state)?;
    Ok(record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == reviewed_state.base_branch
        && semantic_matches
        && record.contract_identity == reviewed_state.contract_identity
        && record.source_task_closure_ids == reviewed_state.source_task_closure_ids
        && record.provenance_basis == reviewed_state.provenance_basis
        && record._effective_reviewed_branch_surface
            == reviewed_state.effective_reviewed_branch_surface)
}

fn branch_closure_record_matches_empty_lineage_late_stage_exemption(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> Result<bool, JsonFailure> {
    let semantic_matches =
        branch_closure_record_semantically_matches_reviewed_state(record, context, reviewed_state)?;
    Ok(record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == reviewed_state.base_branch
        && semantic_matches
        && record.contract_identity == reviewed_state.contract_identity
        && record.source_task_closure_ids.is_empty()
        && record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption"
        && branch_closure_record_matches_plan_exemption(context, record))
}

fn branch_closure_record_semantically_matches_reviewed_state(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> Result<bool, JsonFailure> {
    if record
        .semantic_reviewed_state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|recorded| recorded == reviewed_state.semantic_reviewed_state_id)
    {
        return Ok(true);
    }
    let Some(recorded_raw_tree) = record
        .reviewed_state_id
        .strip_prefix("git_tree:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    let Some(current_raw_tree) = reviewed_state
        .reviewed_state_id
        .strip_prefix("git_tree:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    semantic_paths_changed_between_raw_trees(context, recorded_raw_tree, current_raw_tree)
        .map(|changed_paths| changed_paths.is_empty())
}

fn blocked_branch_closure_output_for_invalid_current_task_closure(
    context: &ExecutionContext,
) -> Result<Option<RecordBranchClosureOutput>, JsonFailure> {
    if let Some(failure) = structural_current_task_closure_failures(context)?
        .into_iter()
        .next()
    {
        return Ok(Some(RecordBranchClosureOutput {
            action: String::from("blocked"),
            branch_closure_id: None,
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            superseded_branch_closure_ids: Vec::new(),
            required_follow_up: Some(String::from("repair_review_state")),
            trace_summary: format!(
                "record-branch-closure failed closed because {}",
                failure.message
            ),
        }));
    }
    Ok(None)
}

pub(crate) fn task_closure_contributes_to_branch_surface(
    context: &ExecutionContext,
    current_record: &CurrentTaskClosureRecord,
) -> bool {
    shared_task_closure_contributes_to_branch_surface(context, current_record)
}

#[cfg(test)]
fn task_closure_record_covers_path(current_record: &CurrentTaskClosureRecord, path: &str) -> bool {
    current_record
        .effective_reviewed_surface_paths
        .iter()
        .any(|surface_path| {
            path_matches_late_stage_surface(path, std::slice::from_ref(surface_path))
        })
}

fn current_authoritative_branch_closure_id(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    current_authoritative_branch_closure_id_optional(context)
}

fn branch_closure_already_current_output(
    context: &ExecutionContext,
    authoritative_state: &mut AuthoritativeTransitionState,
    reviewed_state: &BranchReviewedState,
) -> Result<Option<RecordBranchClosureOutput>, JsonFailure> {
    let Some(current_identity) = usable_current_branch_closure_identity(context) else {
        return Ok(None);
    };
    let current_record_matches = authoritative_state
        .branch_closure_record(&current_identity.branch_closure_id)
        .map(|record| {
            Ok::<bool, JsonFailure>(
                branch_closure_record_matches_reviewed_state(&record, context, reviewed_state)?
                    || branch_closure_record_matches_empty_lineage_late_stage_exemption(
                        &record,
                        context,
                        reviewed_state,
                    )?,
            )
        })
        .transpose()?
        .unwrap_or(false);
    if !current_record_matches {
        return Ok(None);
    }
    authoritative_state.restore_current_branch_closure_overlay_fields(
        &current_identity.branch_closure_id,
        &reviewed_state.reviewed_state_id,
        &reviewed_state.contract_identity,
    )?;
    authoritative_state.set_review_state_repair_follow_up(None)?;
    authoritative_state
        .persist_if_dirty_with_failpoint_and_command(None, "record_branch_closure")?;
    Ok(Some(RecordBranchClosureOutput {
        action: String::from("already_current"),
        branch_closure_id: Some(current_identity.branch_closure_id),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        superseded_branch_closure_ids: Vec::new(),
        required_follow_up: None,
        trace_summary: String::from(
            "Current reviewed branch state already has an authoritative current branch closure.",
        ),
    }))
}

fn branch_closure_already_current_empty_lineage_exemption_output(
    context: &ExecutionContext,
    authoritative_state: &mut AuthoritativeTransitionState,
    reviewed_state: &BranchReviewedState,
) -> Result<Option<RecordBranchClosureOutput>, JsonFailure> {
    let Some(current_identity) = usable_current_branch_closure_identity(context) else {
        return Ok(None);
    };
    let current_record_matches = authoritative_state
        .branch_closure_record(&current_identity.branch_closure_id)
        .map(|record| {
            branch_closure_record_matches_empty_lineage_late_stage_exemption(
                &record,
                context,
                reviewed_state,
            )
        })
        .transpose()?
        .unwrap_or(false);
    if !current_record_matches {
        return Ok(None);
    }
    authoritative_state.restore_current_branch_closure_overlay_fields(
        &current_identity.branch_closure_id,
        &reviewed_state.reviewed_state_id,
        &reviewed_state.contract_identity,
    )?;
    authoritative_state.set_review_state_repair_follow_up(None)?;
    authoritative_state
        .persist_if_dirty_with_failpoint_and_command(None, "record_branch_closure")?;
    Ok(Some(RecordBranchClosureOutput {
        action: String::from("already_current"),
        branch_closure_id: Some(current_identity.branch_closure_id),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        superseded_branch_closure_ids: Vec::new(),
        required_follow_up: None,
        trace_summary: String::from(
            "Current reviewed branch state already has an authoritative current branch closure.",
        ),
    }))
}

#[derive(Debug, Clone)]
struct SupersededTaskClosureRecord {
    task: u32,
    closure_record_id: String,
}

fn current_branch_source_task_closure_ids(
    context: &ExecutionContext,
) -> Result<Vec<String>, JsonFailure> {
    Ok(shared_branch_source_task_closure_ids(
        context,
        &current_branch_task_closure_records(context)?,
        None,
    ))
}

fn current_branch_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    if load_authoritative_transition_state(context)?.is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires authoritative current task-closure state.",
        ));
    }
    Ok(still_current_task_closure_records(context)?
        .into_iter()
        .filter(|record| task_closure_contributes_to_branch_surface(context, record))
        .collect())
}

fn current_task_closure_record_id(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    let current_lineage = task_completion_lineage_fingerprint(context, task_number).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "record-branch-closure could not determine still-current task-closure lineage for task {task_number}."
            ),
        )
    })?;
    Ok(deterministic_record_id(
        "task-closure",
        &[
            &context.plan_rel,
            &task_number.to_string(),
            &current_lineage,
        ],
    ))
}

fn current_task_reviewed_state_id(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    if task_completion_lineage_fingerprint(context, task_number).is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "close-current-task could not determine the still-current reviewed state for task {task_number}."
            ),
        ));
    }
    Ok(semantic_workspace_snapshot(context)?.semantic_workspace_tree_id)
}

fn current_task_raw_reviewed_state_id(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    if task_completion_lineage_fingerprint(context, task_number).is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "close-current-task could not determine the still-current raw reviewed state for task {task_number}."
            ),
        ));
    }
    Ok(format!("git_tree:{}", context.current_tracked_tree_sha()?))
}

fn current_task_contract_identity(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    task_definition_identity_for_task(context, task_number)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "close-current-task could not determine semantic task contract identity for task {task_number}."
            ),
        )
    })
}

fn current_task_effective_reviewed_surface_paths(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<Vec<String>, JsonFailure> {
    let mut surface_paths = context
        .tasks_by_number
        .get(&task_number)
        .map(|task| {
            task.files
                .iter()
                .map(|entry| entry.path.clone())
                .filter(|path| {
                    path != NO_REPO_FILES_MARKER
                        && !is_runtime_owned_execution_control_plane_path(context, path)
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    for step in context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == task_number)
    {
        let attempt = latest_attempt_for_step(&context.evidence, task_number, step.step_number).ok_or_else(
            || {
                JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    format!(
                        "close-current-task could not resolve completed evidence for task {task_number} step {}.",
                        step.step_number
                    ),
                )
            },
        )?;
        if attempt.status != "Completed" {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                format!(
                    "close-current-task requires completed evidence for task {task_number} step {}.",
                    step.step_number
                ),
            ));
        }
        for path in attempt
            .files
            .iter()
            .chain(attempt.file_proofs.iter().map(|proof| &proof.path))
        {
            if path != NO_REPO_FILES_MARKER
                && !is_runtime_owned_execution_control_plane_path(context, path)
            {
                surface_paths.insert(path.clone());
            }
        }
    }
    if surface_paths.is_empty() {
        surface_paths.insert(String::from(NO_REPO_FILES_MARKER));
    }
    Ok(surface_paths.into_iter().collect())
}

fn task_surface_paths_overlap(left: &[String], right: &[String]) -> bool {
    let left_paths = normalized_effective_task_surface_paths(left);
    let right_paths = normalized_effective_task_surface_paths(right);
    !left_paths.is_disjoint(&right_paths)
}

fn normalized_effective_task_surface_paths(paths: &[String]) -> BTreeSet<String> {
    paths
        .iter()
        .filter(|path| path.as_str() != NO_REPO_FILES_MARKER)
        .filter_map(|path| normalize_repo_relative_path(path).ok())
        .collect::<BTreeSet<_>>()
}

fn task_closure_record_matches_active_plan_and_runtime_scope(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    record: &CurrentTaskClosureRecord,
) -> bool {
    let plan_matches = record.source_plan_path.as_deref() == Some(context.plan_rel.as_str())
        && record.source_plan_revision == Some(context.plan_document.plan_revision);
    if !plan_matches {
        return false;
    }
    match authoritative_state.execution_run_id_opt() {
        Some(active_run_id) => record.execution_run_id.as_deref() == Some(active_run_id.as_str()),
        None => record
            .execution_run_id
            .as_deref()
            .is_none_or(|run_id| run_id.trim().is_empty()),
    }
}

fn superseded_task_closure_records(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    task_number: u32,
    closure_record_id: &str,
    effective_reviewed_surface_paths: &[String],
) -> Vec<SupersededTaskClosureRecord> {
    authoritative_state
        .task_closure_history_records()
        .into_iter()
        .filter(|record| record.closure_record_id != closure_record_id)
        .filter(|record| {
            task_closure_record_matches_active_plan_and_runtime_scope(
                context,
                authoritative_state,
                record,
            )
        })
        .filter(|record| {
            record.task == task_number
                || task_surface_paths_overlap(
                    &record.effective_reviewed_surface_paths,
                    effective_reviewed_surface_paths,
                )
        })
        .map(|record| SupersededTaskClosureRecord {
            task: record.task,
            closure_record_id: record.closure_record_id,
        })
        .collect()
}

fn current_final_review_record_is_still_authoritative(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    check: CurrentFinalReviewAuthorityCheck<'_>,
) -> Result<bool, JsonFailure> {
    let Some(record) = authoritative_state.current_final_review_record() else {
        return Ok(false);
    };
    if record.branch_closure_id != check.branch_closure_id
        || record.dispatch_id != check.dispatch_id
        || record.reviewer_source != check.reviewer_source
        || record.reviewer_id != check.reviewer_id
        || record.result != check.result
        || record.summary_hash != check.normalized_summary_hash
        || record.source_plan_path != context.plan_rel
        || record.source_plan_revision != context.plan_document.plan_revision
        || record.repo_slug != context.runtime.repo_slug
        || record.branch_name != context.runtime.branch_name
    {
        return Ok(false);
    }
    let Some(current_base_branch) = context.current_release_base_branch() else {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Current final-review authority could not resolve the expected base branch.",
        ));
    };
    if record.base_branch != current_base_branch {
        return Ok(false);
    }
    if check.result == "fail" {
        return Ok(true);
    }
    if !final_review_dispatch_lineage_is_current_for_rerun(
        context,
        authoritative_state,
        check.branch_closure_id,
        check.dispatch_id,
    )? {
        return Ok(false);
    }
    Ok(true)
}

fn final_review_dispatch_lineage_is_current_for_rerun(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    expected_branch_closure_id: &str,
    expected_dispatch_id: &str,
) -> Result<bool, JsonFailure> {
    let runtime_state = crate::execution::reducer::reduce_runtime_state(
        context,
        Some(authoritative_state),
        semantic_workspace_snapshot(context)?,
    )?;
    if shared_final_review_dispatch_still_current(
        runtime_state.gate_snapshot.gate_review.as_ref(),
        runtime_state.gate_snapshot.gate_finish.as_ref(),
    ) {
        return match ensure_final_review_dispatch_id_matches(context, expected_dispatch_id) {
            Ok(_) => Ok(true),
            Err(error)
                if matches!(
                    error.error_class.as_str(),
                    "ExecutionStateNotReady" | "InvalidCommandInput"
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        };
    }
    Ok(
        authoritative_state.current_final_review_dispatch_id() == Some(expected_dispatch_id)
            && authoritative_state.current_final_review_branch_closure_id()
                == Some(expected_branch_closure_id),
    )
}

fn resolve_final_review_evidence(
    context: &ExecutionContext,
) -> Result<ResolvedFinalReviewEvidence, JsonFailure> {
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ReviewArtifactNotFresh,
            "final-review recording requires a resolvable base branch.",
        )
    })?;
    let execution_context_key = format!("{}@{}", context.runtime.branch_name, base_branch);
    let deviations_required = authoritative_matching_execution_topology_downgrade_records_checked(
        context,
        &execution_context_key,
    )?
    .iter()
    .any(|record| !record.rerun_guidance_superseded);
    Ok(ResolvedFinalReviewEvidence {
        base_branch,
        deviations_required,
    })
}

#[cfg(test)]
fn rebuild_downstream_truth_stale(message: impl Into<String>) -> JsonFailure {
    JsonFailure::new(FailureClass::StaleProvenance, message.into())
}

#[cfg(test)]
fn rewrite_branch_final_review_artifacts(
    review_path: &Path,
    reviewer_artifact_path: &Path,
    current_head: &str,
    strategy_checkpoint_fingerprint: &str,
) -> Result<(), JsonFailure> {
    let _ = (
        review_path,
        reviewer_artifact_path,
        current_head,
        strategy_checkpoint_fingerprint,
    );
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical final-review proof in place",
    ))
}

#[cfg(test)]
fn rewrite_branch_head_bound_artifact(path: &Path, current_head: &str) -> Result<(), JsonFailure> {
    let _ = (path, current_head);
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical head-bound artifacts in place",
    ))
}

#[cfg(test)]
fn rewrite_branch_qa_artifact(
    qa_path: &Path,
    current_head: &str,
    test_plan_path: &Path,
) -> Result<(), JsonFailure> {
    let _ = (qa_path, current_head, test_plan_path);
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical QA artifacts in place",
    ))
}

fn rewrite_rebuild_source_test_plan_header(source: &str, test_plan_path: &Path) -> String {
    rewrite_markdown_header(
        source,
        "Source Test Plan",
        &format!("`{}`", test_plan_path.display()),
    )
}

fn rewrite_markdown_header(source: &str, header: &str, value: &str) -> String {
    let prefix = format!("**{header}:**");
    let rewritten = source
        .lines()
        .map(|line| {
            if line.trim().starts_with(&prefix) {
                format!("**{header}:** {value}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{rewritten}\n")
}

fn refresh_task_closure_projections_with_context(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    refresh: TaskClosureReceiptRefresh<'_>,
) -> Result<(), JsonFailure> {
    let task_steps = context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == refresh.task)
        .map(|step_state| step_state.step_number)
        .collect::<Vec<_>>();
    for step in task_steps {
        refresh_unit_review_receipt_for_step(
            runtime,
            context,
            refresh.execution_run_id,
            refresh.strategy_checkpoint_fingerprint,
            refresh.active_contract_fingerprint,
            refresh.task,
            step,
        )?;
    }
    refresh_task_verification_receipt_for_task(
        runtime,
        context,
        refresh.execution_run_id,
        refresh.strategy_checkpoint_fingerprint,
        refresh.task,
    )?;
    let _write_authority = refresh
        .claim_write_authority
        .then(|| claim_step_write_authority(runtime))
        .transpose()?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.refresh_task_review_dispatch_lineage(context, refresh.task)?;
        authoritative_state
            .persist_if_dirty_with_failpoint_and_command(None, "close_current_task")?;
    }
    Ok(())
}

fn materialize_current_task_closure_from_close_inputs(
    authoritative_state: &mut AuthoritativeTransitionState,
    materialization: CurrentTaskClosureMaterialization<'_>,
) -> Result<(), JsonFailure> {
    record_current_task_closure(
        authoritative_state,
        CurrentTaskClosureWrite {
            task: materialization.task,
            dispatch_id: materialization.dispatch_id,
            closure_record_id: materialization.closure_record_id,
            execution_run_id: Some(materialization.execution_run_id),
            reviewed_state_id: materialization.reviewed_state_id,
            semantic_reviewed_state_id: Some(materialization.semantic_reviewed_state_id),
            contract_identity: materialization.contract_identity,
            effective_reviewed_surface_paths: materialization.effective_reviewed_surface_paths,
            review_result: materialization.review_result,
            review_summary_hash: materialization.review_summary_hash,
            verification_result: materialization.verification_result,
            verification_summary_hash: materialization.verification_summary_hash,
            superseded_tasks: materialization.superseded_tasks,
            superseded_task_closure_ids: materialization.superseded_task_closure_ids,
        },
    )
}

fn refresh_unit_review_receipt_for_step(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    execution_run_id: &str,
    strategy_checkpoint_fingerprint: &str,
    active_contract_fingerprint: Option<&str>,
    task: u32,
    step: u32,
) -> Result<(), JsonFailure> {
    let Some(attempt) = latest_attempt_for_step(&context.evidence, task, step) else {
        return Ok(());
    };
    if attempt.status != "Completed" {
        return Ok(());
    }
    let Some(packet_fingerprint) = attempt
        .packet_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let Some(reviewed_checkpoint_sha) = attempt
        .head_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };

    let execution_unit_id = format!("task-{task}-step-{step}");
    let reviewer_source =
        existing_unit_review_reviewer_source(runtime, execution_run_id, &execution_unit_id)
            .unwrap_or_else(|| String::from("fresh-context-subagent"));
    let generated_at = Timestamp::now().to_string();
    let unsigned_source = if let Some(active_contract_fingerprint) = active_contract_fingerprint {
        let approved_unit_contract_fingerprint = approved_unit_contract_fingerprint_for_review(
            active_contract_fingerprint,
            packet_fingerprint,
            &execution_unit_id,
        );
        let execution_context_key = current_worktree_lease_execution_context_key(
            execution_run_id,
            &execution_unit_id,
            &context.plan_rel,
            context.plan_document.plan_revision,
            &context.runtime.branch_name,
            reviewed_checkpoint_sha,
        );
        let lease_fingerprint = serial_unit_review_lease_fingerprint(
            execution_run_id,
            &execution_unit_id,
            &execution_context_key,
            reviewed_checkpoint_sha,
            packet_fingerprint,
            &approved_unit_contract_fingerprint,
        );
        let Some(reconcile_result_proof_fingerprint) =
            reconcile_result_proof_fingerprint_for_review(
                &context.runtime.repo_root,
                reviewed_checkpoint_sha,
            )
        else {
            return Ok(());
        };
        let reviewed_worktree = fs::canonicalize(&context.runtime.repo_root)
            .unwrap_or_else(|_| context.runtime.repo_root.clone());
        format!(
            "# Unit Review Result\n**Review Stage:** featureforge:unit-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {reviewer_source}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Source Plan:** {}\n**Source Plan Revision:** {}\n**Execution Run ID:** {execution_run_id}\n**Execution Unit ID:** {execution_unit_id}\n**Lease Fingerprint:** {lease_fingerprint}\n**Execution Context Key:** {execution_context_key}\n**Approved Task Packet Fingerprint:** {packet_fingerprint}\n**Approved Unit Contract Fingerprint:** {approved_unit_contract_fingerprint}\n**Reconciled Result SHA:** {reviewed_checkpoint_sha}\n**Reconcile Result Proof Fingerprint:** {reconcile_result_proof_fingerprint}\n**Reconcile Mode:** identity_preserving\n**Reviewed Worktree:** {}\n**Reviewed Checkpoint SHA:** {reviewed_checkpoint_sha}\n**Result:** pass\n**Generated By:** featureforge:unit-review\n**Generated At:** {generated_at}\n",
            context.plan_rel,
            context.plan_document.plan_revision,
            reviewed_worktree.display(),
        )
    } else {
        format!(
            "# Unit Review Result\n**Review Stage:** featureforge:unit-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {reviewer_source}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Source Plan:** {}\n**Source Plan Revision:** {}\n**Execution Run ID:** {execution_run_id}\n**Execution Unit ID:** {execution_unit_id}\n**Reviewed Checkpoint SHA:** {reviewed_checkpoint_sha}\n**Approved Task Packet Fingerprint:** {packet_fingerprint}\n**Result:** pass\n**Generated By:** featureforge:unit-review\n**Generated At:** {generated_at}\n",
            context.plan_rel, context.plan_document.plan_revision,
        )
    };
    let receipt_fingerprint = canonical_unit_review_receipt_fingerprint(&unsigned_source);
    let source = format!(
        "# Unit Review Result\n**Receipt Fingerprint:** {receipt_fingerprint}\n{}",
        unsigned_source.trim_start_matches("# Unit Review Result\n")
    );

    write_authoritative_unit_review_receipt_artifact(
        runtime,
        execution_run_id,
        &execution_unit_id,
        &source,
    )?;
    Ok(())
}

fn existing_unit_review_reviewer_source(
    runtime: &ExecutionRuntime,
    execution_run_id: &str,
    execution_unit_id: &str,
) -> Option<String> {
    let receipt_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("unit-review-{execution_run_id}-{execution_unit_id}.md"),
    );
    let source = fs::read_to_string(receipt_path).ok()?;
    source.lines().find_map(|line| {
        line.trim()
            .strip_prefix("**Reviewer Source:**")
            .map(str::trim)
            .filter(|value| matches!(*value, "fresh-context-subagent" | "cross-model"))
            .map(ToOwned::to_owned)
    })
}

fn refresh_task_verification_receipt_for_task(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    execution_run_id: &str,
    strategy_checkpoint_fingerprint: &str,
    task: u32,
) -> Result<(), JsonFailure> {
    let task_steps = context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == task)
        .collect::<Vec<_>>();
    if task_steps.is_empty() {
        return Ok(());
    }

    let mut verification_commands = Vec::new();
    let mut verification_results = Vec::new();
    for step_state in task_steps {
        if !step_state.checked {
            return Ok(());
        }
        let Some(attempt) =
            latest_attempt_for_step(&context.evidence, task, step_state.step_number)
        else {
            return Ok(());
        };
        if attempt.status != "Completed" {
            return Ok(());
        }
        if let Some(verify_command) = attempt
            .verify_command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            verification_commands.push(verify_command.to_owned());
        }
        let verification_summary = attempt.verification_summary.trim();
        if !verification_summary.is_empty() {
            verification_results.push(verification_summary.to_owned());
        }
    }

    if verification_results.is_empty() {
        return Ok(());
    }
    if verification_commands.is_empty() {
        verification_commands.push(String::from("manual verification recorded"));
    }

    let receipt_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("task-verification-{execution_run_id}-task-{task}.md"),
    );
    let generated_at = Timestamp::now().to_string();
    let source = format!(
        "# Task Verification Result\n**Source Plan:** {}\n**Source Plan Revision:** {}\n**Execution Run ID:** {execution_run_id}\n**Task Number:** {task}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Verification Commands:** {}\n**Verification Results:** {}\n**Result:** pass\n**Generated By:** featureforge:verification-before-completion\n**Generated At:** {generated_at}\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        verification_commands.join(" && "),
        verification_results.join(" | "),
    );
    write_atomic(&receipt_path, &source)
}

fn latest_attempt_for_step(
    evidence: &ExecutionEvidence,
    task: u32,
    step: u32,
) -> Option<&EvidenceAttempt> {
    evidence
        .attempts
        .iter()
        .filter(|attempt| attempt.task_number == task && attempt.step_number == step)
        .max_by_key(|attempt| attempt.attempt_number)
}

fn canonical_unit_review_receipt_fingerprint(source: &str) -> String {
    let filtered = source
        .lines()
        .filter(|line| !line.trim().starts_with("**Receipt Fingerprint:**"))
        .collect::<Vec<_>>()
        .join("\n");
    sha256_hex(filtered.as_bytes())
}

fn current_worktree_lease_execution_context_key(
    execution_run_id: &str,
    execution_unit_id: &str,
    source_plan_path: &str,
    source_plan_revision: u32,
    authoritative_integration_branch: &str,
    reviewed_checkpoint_commit_sha: &str,
) -> String {
    sha256_hex(
        format!(
            "run={execution_run_id}\nunit={execution_unit_id}\nplan={source_plan_path}\nplan_revision={source_plan_revision}\nbranch={authoritative_integration_branch}\nreviewed_checkpoint={reviewed_checkpoint_commit_sha}\n"
        )
        .as_bytes(),
    )
}

fn serial_unit_review_lease_fingerprint(
    execution_run_id: &str,
    execution_unit_id: &str,
    execution_context_key: &str,
    reviewed_checkpoint_commit_sha: &str,
    approved_task_packet_fingerprint: &str,
    approved_unit_contract_fingerprint: &str,
) -> String {
    sha256_hex(
        format!(
            "serial-unit-review:{execution_run_id}:{execution_unit_id}:{execution_context_key}:{reviewed_checkpoint_commit_sha}:{approved_task_packet_fingerprint}:{approved_unit_contract_fingerprint}"
        )
        .as_bytes(),
    )
}

fn approved_unit_contract_fingerprint_for_review(
    active_contract_fingerprint: &str,
    approved_task_packet_fingerprint: &str,
    execution_unit_id: &str,
) -> String {
    sha256_hex(
        format!(
            "approved-unit-contract:{active_contract_fingerprint}:{approved_task_packet_fingerprint}:{execution_unit_id}"
        )
        .as_bytes(),
    )
}

#[cfg(test)]
fn verify_command_launcher(verify_command: &str) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        ("cmd", vec![String::from("/C"), verify_command.to_owned()])
    } else {
        ("sh", vec![String::from("-lc"), verify_command.to_owned()])
    }
}

fn reconcile_result_proof_fingerprint_for_review(
    repo_root: &Path,
    reconcile_result_commit_sha: &str,
) -> Option<String> {
    commit_object_fingerprint(repo_root, reconcile_result_commit_sha)
}

fn planned_rebuild_target(candidate: &RebuildEvidenceCandidate) -> RebuildEvidenceTarget {
    RebuildEvidenceTarget {
        task_id: candidate.task,
        step_id: candidate.step,
        target_kind: candidate.target_kind.clone(),
        pre_invalidation_reason: candidate.pre_invalidation_reason.clone(),
        status: String::from("planned"),
        verify_mode: candidate.verify_mode.clone(),
        verify_command: candidate.verify_command.clone(),
        attempt_id_before: candidate
            .attempt_number
            .map(|attempt| format!("{}:{}:{}", candidate.task, candidate.step, attempt)),
        attempt_id_after: None,
        verification_hash: None,
        error: None,
        failure_class: None,
    }
}

fn rebuild_scope_label(request: &crate::execution::state::RebuildEvidenceRequest) -> String {
    if !request.raw_steps.is_empty() {
        String::from("step")
    } else if !request.tasks.is_empty() {
        String::from("task")
    } else {
        String::from("all")
    }
}

fn matched_rebuild_scope_ids(
    context: &ExecutionContext,
    request: &crate::execution::state::RebuildEvidenceRequest,
) -> Vec<String> {
    let task_filter = request.tasks.iter().copied().collect::<BTreeSet<_>>();
    let step_filter = request.steps.iter().copied().collect::<BTreeSet<_>>();
    context
        .steps
        .iter()
        .filter(|step| {
            (task_filter.is_empty() || task_filter.contains(&step.task_number))
                && (step_filter.is_empty()
                    || step_filter.contains(&(step.task_number, step.step_number)))
        })
        .map(|step| format!("{}:{}", step.task_number, step.step_number))
        .collect()
}

fn step_index(context: &ExecutionContext, task: u32, step: u32) -> Option<usize> {
    context
        .steps
        .iter()
        .position(|candidate| candidate.task_number == task && candidate.step_number == step)
}

fn truncate_summary(summary: &str) -> String {
    if summary.chars().count() <= 120 {
        return summary.to_owned();
    }
    let truncated = summary.chars().take(117).collect::<String>();
    format!("{truncated}...")
}

fn canonicalize_files(files: &[String]) -> Result<Vec<String>, JsonFailure> {
    let mut normalized = files
        .iter()
        .map(|path| {
            let path = normalize_repo_relative_path(path).map_err(|_| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "Evidence file paths must be normalized repo-relative paths inside the repo root.",
                )
            })?;
            Ok(path)
        })
        .collect::<Result<Vec<_>, JsonFailure>>()?;
    normalized.sort();
    normalized.dedup();
    Ok(if normalized.is_empty() {
        vec![String::from(NO_REPO_FILES_MARKER)]
    } else {
        normalized
    })
}

fn canonicalize_repo_visible_paths(
    repo_root: &Path,
    files: &[String],
) -> Result<Vec<String>, JsonFailure> {
    let missing = files
        .iter()
        .filter(|path| !repo_root.join(path).exists())
        .cloned()
        .collect::<BTreeSet<_>>();
    if missing.is_empty() {
        return Ok(files.to_vec());
    }

    let rename_map = rename_backed_paths(repo_root, &missing)?;
    let mut canonical = files
        .iter()
        .map(|path| {
            rename_map
                .get(path)
                .cloned()
                .unwrap_or_else(|| path.clone())
        })
        .collect::<Vec<_>>();
    canonical.sort();
    canonical.dedup();
    Ok(canonical)
}

fn rename_backed_paths(
    repo_root: &Path,
    missing: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not discover the repository while canonicalizing rename-backed file paths: {error}"
            ),
        )
    })?;
    let head_tree = repo.head_tree_id_or_empty().map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not determine the HEAD tree while canonicalizing rename-backed file paths: {error}"
            ),
        )
    })?;
    let index = repo.index_or_empty().map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not open the repository index while canonicalizing rename-backed file paths: {error}"
            ),
        )
    })?;

    let mut paths = BTreeMap::new();
    repo.tree_index_status(
        head_tree.detach().as_ref(),
        &index,
        None,
        gix::status::tree_index::TrackRenames::AsConfigured,
        |change, _, _| {
            if let gix::diff::index::ChangeRef::Rewrite {
                source_location,
                location,
                copy,
                ..
            } = change
                && !copy
            {
                let source = String::from_utf8_lossy(source_location.as_ref()).into_owned();
                if missing.contains(&source) {
                    let destination = String::from_utf8_lossy(location.as_ref()).into_owned();
                    paths.insert(source, destination);
                    if paths.len() == missing.len() {
                        return Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Break(()));
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
        },
    )
    .map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not canonicalize rename-backed file paths from the current change set: {error}"
            ),
        )
    })?;
    Ok(paths)
}

fn default_files_for_task(context: &ExecutionContext, task_number: u32) -> Vec<String> {
    let Some(task) = context.tasks_by_number.get(&task_number) else {
        return vec![String::from(NO_REPO_FILES_MARKER)];
    };
    let mut files = task
        .files
        .iter()
        .map(|entry| entry.path.clone())
        .filter(|path| context.runtime.repo_root.join(path).exists())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    if files.is_empty() {
        vec![String::from(NO_REPO_FILES_MARKER)]
    } else {
        files
    }
}

fn next_attempt_number(evidence: &ExecutionEvidence, task: u32, step: u32) -> u32 {
    evidence
        .attempts
        .iter()
        .filter(|attempt| attempt.task_number == task && attempt.step_number == step)
        .map(|attempt| attempt.attempt_number)
        .max()
        .unwrap_or(0)
        + 1
}

fn record_execution_projection_fingerprints(
    authoritative_state: Option<&mut AuthoritativeTransitionState>,
    rendered: &RenderedExecutionProjections,
) -> Result<(), JsonFailure> {
    if let Some(authoritative_state) = authoritative_state {
        authoritative_state.set_execution_projection_fingerprints(
            &sha256_hex(rendered.plan.as_bytes()),
            &sha256_hex(rendered.evidence.as_bytes()),
        )?;
    }
    Ok(())
}

fn invalidate_latest_completed_attempt(
    context: &mut ExecutionContext,
    task: u32,
    step: u32,
    reason: &str,
) -> Result<(), JsonFailure> {
    let attempt_index =
        context
            .evidence
            .attempts
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, attempt)| {
                (attempt.task_number == task
                    && attempt.step_number == step
                    && attempt.status == "Completed")
                    .then_some(index)
            });
    let Some(attempt_index) = attempt_index else {
        return Ok(());
    };
    context.evidence.attempts[attempt_index].status = String::from("Invalidated");
    context.evidence.attempts[attempt_index].recorded_at = Timestamp::now().to_string();
    context.evidence.attempts[attempt_index].invalidation_reason = reason.to_owned();
    Ok(())
}

fn persist_authoritative_state_with_rollback(
    authoritative_state: &AuthoritativeTransitionState,
    command: &str,
    plan_path: &Path,
    original_plan: &str,
    evidence_path: &Path,
    _original_evidence: Option<&str>,
    failpoint: &str,
) -> Result<(), JsonFailure> {
    let rollback = AuthoritativePersistRollback {
        plan_path,
        original_plan,
        evidence_path,
        failpoint,
    };
    persist_authoritative_state_with_step_hint_and_rollback(
        authoritative_state,
        command,
        None,
        rollback,
    )
}

struct AuthoritativePersistRollback<'a> {
    plan_path: &'a Path,
    original_plan: &'a str,
    evidence_path: &'a Path,
    failpoint: &'a str,
}

fn persist_authoritative_state_with_step_hint_and_rollback(
    authoritative_state: &AuthoritativeTransitionState,
    command: &str,
    step_hint: Option<(u32, u32)>,
    rollback: AuthoritativePersistRollback<'_>,
) -> Result<(), JsonFailure> {
    let original_evidence = rollback_evidence_source(rollback.evidence_path)?;
    let outcome = match authoritative_state
        .persist_if_dirty_with_failpoint_command_outcome_and_step_hint(
            Some(rollback.failpoint),
            command,
            step_hint,
        ) {
        Ok(outcome) => outcome,
        Err(error) => {
            restore_plan_and_evidence(
                rollback.plan_path,
                rollback.original_plan,
                rollback.evidence_path,
                original_evidence.as_deref(),
            );
            return Err(error);
        }
    };
    if let Some(error) = outcome.projection_refresh_failure {
        if !outcome.authoritative_event_committed {
            restore_plan_and_evidence(
                rollback.plan_path,
                rollback.original_plan,
                rollback.evidence_path,
                original_evidence.as_deref(),
            );
        }
        return Err(error);
    }
    Ok(())
}

fn rollback_evidence_source(evidence_path: &Path) -> Result<Option<String>, JsonFailure> {
    match fs::read_to_string(evidence_path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Could not read tracked execution evidence before authoritative mutation rollback setup: {error}"
            ),
        )),
    }
}

fn restore_plan_and_evidence(
    plan_path: &Path,
    original_plan: &str,
    evidence_path: &Path,
    original_evidence: Option<&str>,
) {
    let _ = fs::write(plan_path, original_plan);
    match original_evidence {
        Some(source) => {
            let _ = fs::write(evidence_path, source);
        }
        None => {
            let _ = fs::remove_file(evidence_path);
        }
    }
}

fn maybe_trigger_failpoint(name: &str) -> Result<(), JsonFailure> {
    if std::env::var("FEATUREFORGE_PLAN_EXECUTION_TEST_FAILPOINT")
        .ok()
        .as_deref()
        == Some(name)
    {
        return Err(JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Injected plan execution failpoint: {name}"),
        ));
    }
    Ok(())
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), JsonFailure> {
    write_atomic_file(path, contents).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not persist {}: {error}", path.display()),
        )
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

#[cfg(test)]
mod unit_tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use serde_json::json;
    use tempfile::TempDir;

    use super::{
        AdvanceLateStageOutputContext, CloseCurrentTaskOutcomeClass,
        CurrentFinalReviewAuthorityCheck, FinalReviewProjectionInput,
        advance_late_stage_follow_up_or_requery_output,
        blocked_close_current_task_output_from_operator, blocked_follow_up_for_operator,
        close_current_task_outcome_class, close_current_task_required_follow_up,
        current_final_review_record_is_still_authoritative, late_stage_required_follow_up,
        normalized_late_stage_surface, path_matches_late_stage_surface,
        render_final_review_artifacts, rewrite_branch_final_review_artifacts,
        rewrite_branch_head_bound_artifact, rewrite_branch_qa_artifact,
        superseded_branch_closure_ids_from_previous_current,
        task_closure_contributes_to_branch_surface, task_closure_record_covers_path,
        verify_command_launcher,
    };
    use crate::cli::plan_execution::{ReviewOutcomeArg, VerificationOutcomeArg};
    use crate::contracts::plan::parse_plan_file;
    use crate::diagnostics::FailureClass;
    use crate::execution::final_review::resolve_release_base_branch;
    use crate::execution::leases::StatusAuthoritativeOverlay;
    use crate::execution::leases::authoritative_state_path;
    use crate::execution::query::ExecutionRoutingState;
    use crate::execution::state::{
        EvidenceFormat, EvidenceSourceOrigin, ExecutionContext, ExecutionEvidence,
        ExecutionRuntime, NO_REPO_FILES_MARKER,
    };
    use crate::execution::transitions::CurrentTaskClosureRecord;
    use crate::execution::transitions::load_authoritative_transition_state;
    use crate::git::sha256_hex;
    use crate::paths::harness_authoritative_artifact_path;
    use crate::workflow::status::WorkflowRoute;

    #[test]
    fn verify_command_launcher_matches_platform_contract() {
        let (program, args) = verify_command_launcher("printf rebuilt");
        if cfg!(windows) {
            assert_eq!(program, "cmd");
            assert_eq!(
                args,
                vec![String::from("/C"), String::from("printf rebuilt")]
            );
        } else {
            assert_eq!(program, "sh");
            assert_eq!(
                args,
                vec![String::from("-lc"), String::from("printf rebuilt")]
            );
        }
    }

    #[test]
    fn task_closure_contributes_to_branch_surface_excludes_no_repo_marker_only_records() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut runtime =
            ExecutionRuntime::discover(&repo_root).expect("repo runtime should be discoverable");
        let tempdir = TempDir::new().expect("tempdir should exist");
        runtime.state_dir = tempdir.path().join("state");
        fs::create_dir_all(&runtime.state_dir).expect("state dir should be creatable");
        let plan_rel = "tests/codex-runtime/fixtures/plan-contract/valid-plan.md";
        let plan_abs = repo_root.join(plan_rel);
        let plan_document = parse_plan_file(&plan_abs).expect("plan document should parse");
        let plan_source = fs::read_to_string(&plan_abs).expect("plan source should read");
        let context = ExecutionContext {
            runtime,
            plan_rel: String::from(plan_rel),
            plan_abs: plan_abs.clone(),
            plan_document,
            plan_source,
            steps: Vec::new(),
            local_execution_progress_markers_present: false,
            legacy_open_step_projection_present: false,
            tasks_by_number: Default::default(),
            evidence_rel: String::from(
                "docs/archive/featureforge/execution-evidence/placeholder.md",
            ),
            evidence_abs: repo_root
                .join("docs/archive/featureforge/execution-evidence/placeholder.md"),
            evidence: ExecutionEvidence {
                format: EvidenceFormat::Empty,
                plan_path: String::from(plan_rel),
                plan_revision: 0,
                plan_fingerprint: None,
                source_spec_path: String::from(
                    "docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md",
                ),
                source_spec_revision: 0,
                source_spec_fingerprint: None,
                attempts: Vec::new(),
                source: None,
                source_origin: EvidenceSourceOrigin::Empty,
                tracked_progress_present: false,
                tracked_source: None,
            },
            authoritative_evidence_projection_fingerprint: None,
            source_spec_source: String::new(),
            source_spec_path: repo_root
                .join("docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md"),
            execution_fingerprint: String::from("unit-test-execution-fingerprint"),
            tracked_tree_sha_cache: OnceLock::new(),
            semantic_workspace_snapshot_cache: OnceLock::new(),
            reviewed_tree_sha_cache: std::cell::RefCell::new(BTreeMap::new()),
            head_sha_cache: OnceLock::new(),
            release_base_branch_cache: OnceLock::new(),
            tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
        };
        let no_repo_only = CurrentTaskClosureRecord {
            task: 1,
            source_plan_path: Some(String::from("docs/featureforge/plans/example.md")),
            source_plan_revision: Some(1),
            execution_run_id: Some(String::from("run-1")),
            dispatch_id: String::from("dispatch-1"),
            closure_record_id: String::from("task-1-closure"),
            reviewed_state_id: String::from("git_tree:abc123"),
            semantic_reviewed_state_id: Some(String::from("semantic_tree:abc123")),
            contract_identity: String::from("contract-1"),
            effective_reviewed_surface_paths: vec![String::from(NO_REPO_FILES_MARKER)],
            review_result: String::from("pass"),
            review_summary_hash: String::from("summary"),
            verification_result: String::from("pass"),
            verification_summary_hash: String::from("verification"),
            closure_status: Some(String::from("current")),
        };
        let mixed_surface = CurrentTaskClosureRecord {
            effective_reviewed_surface_paths: vec![
                String::from(NO_REPO_FILES_MARKER),
                String::from("src/runtime.rs"),
            ],
            ..no_repo_only.clone()
        };

        assert!(
            !task_closure_contributes_to_branch_surface(&context, &no_repo_only),
            "no-repo-only task closures must not influence branch-surface baseline derivation"
        );
        assert!(
            task_closure_contributes_to_branch_surface(&context, &mixed_surface),
            "task closures that still cover repo-visible paths must contribute to branch-surface baseline derivation"
        );
    }

    #[test]
    fn rewrite_branch_final_review_artifacts_refuses_to_rebind_review_history() {
        let tempdir = TempDir::new().expect("tempdir should exist");
        let reviewer_artifact = tempdir.path().join("reviewer.md");
        let review_receipt = tempdir.path().join("review.md");
        let original_reviewer =
            "**Strategy Checkpoint Fingerprint:** old-checkpoint\n**Head SHA:** old-head\n";
        let original_review = format!(
            "**Strategy Checkpoint Fingerprint:** old-checkpoint\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** old-fingerprint\n**Head SHA:** old-head\n",
            reviewer_artifact.display()
        );
        fs::write(&reviewer_artifact, original_reviewer)
            .expect("reviewer artifact fixture should write");
        fs::write(&review_receipt, &original_review).expect("review receipt fixture should write");

        let error = rewrite_branch_final_review_artifacts(
            &review_receipt,
            &reviewer_artifact,
            "new-head",
            "new-checkpoint",
        )
        .expect_err("append-only repair must not rewrite historical final-review proof in place");

        assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
        assert_eq!(
            fs::read_to_string(&reviewer_artifact)
                .expect("reviewer artifact should remain readable"),
            original_reviewer
        );
        assert_eq!(
            fs::read_to_string(&review_receipt).expect("review receipt should remain readable"),
            original_review
        );
    }

    #[test]
    fn current_final_review_record_authoritativeness_prefers_runtime_record_over_artifact_tamper() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut runtime =
            ExecutionRuntime::discover(&repo_root).expect("repo runtime should be discoverable");
        let tempdir = TempDir::new().expect("tempdir should exist");
        runtime.state_dir = tempdir.path().join("state");
        fs::create_dir_all(&runtime.state_dir).expect("state dir should be creatable");

        let plan_rel = "tests/codex-runtime/fixtures/plan-contract/valid-plan.md";
        let plan_abs = repo_root.join(plan_rel);
        let plan_document = parse_plan_file(&plan_abs).expect("plan document should parse");
        let plan_source = fs::read_to_string(&plan_abs).expect("plan source should read");
        let context = ExecutionContext {
            runtime: runtime.clone(),
            plan_rel: String::from(plan_rel),
            plan_abs: plan_abs.clone(),
            plan_document,
            plan_source,
            steps: Vec::new(),
            local_execution_progress_markers_present: false,
            legacy_open_step_projection_present: false,
            tasks_by_number: Default::default(),
            evidence_rel: String::from(
                "docs/archive/featureforge/execution-evidence/placeholder.md",
            ),
            evidence_abs: repo_root
                .join("docs/archive/featureforge/execution-evidence/placeholder.md"),
            evidence: ExecutionEvidence {
                format: EvidenceFormat::Empty,
                plan_path: String::from(plan_rel),
                plan_revision: 0,
                plan_fingerprint: None,
                source_spec_path: String::from(
                    "docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md",
                ),
                source_spec_revision: 0,
                source_spec_fingerprint: None,
                attempts: Vec::new(),
                source: None,
                source_origin: EvidenceSourceOrigin::Empty,
                tracked_progress_present: false,
                tracked_source: None,
            },
            authoritative_evidence_projection_fingerprint: None,
            source_spec_source: String::new(),
            source_spec_path: repo_root
                .join("docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md"),
            execution_fingerprint: String::from("unit-test-execution-fingerprint"),
            tracked_tree_sha_cache: OnceLock::new(),
            semantic_workspace_snapshot_cache: OnceLock::new(),
            reviewed_tree_sha_cache: std::cell::RefCell::new(BTreeMap::new()),
            head_sha_cache: OnceLock::new(),
            release_base_branch_cache: OnceLock::new(),
            tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
        };
        let base_branch =
            resolve_release_base_branch(&context.runtime.git_dir, &context.runtime.branch_name)
                .expect("base branch should resolve for the current repo");
        let branch_closure_id = "unit-test-branch-closure";
        let reviewed_state_id = format!(
            "git_tree:{}",
            crate::execution::current_truth::current_repo_tracked_tree_sha(
                &context.runtime.repo_root
            )
            .expect("tracked tree sha should resolve for unit coverage")
        );
        let execution_run_id = "run-unit-test";
        let branch_contract_identity = super::branch_definition_identity_for_context(&context);
        let dispatch_id = "unit-test-final-review-dispatch";
        let reviewer_source = "fresh-context-subagent";
        let reviewer_id = "unit-reviewer-001";
        let summary = "Independent final review passed in unit coverage.";
        let summary_hash = sha256_hex(summary.as_bytes());
        let strategy_checkpoint_fingerprint = sha256_hex(b"unit-test-strategy-checkpoint");
        let state_path = authoritative_state_path(&context);
        fs::create_dir_all(
            state_path
                .parent()
                .expect("authoritative state path should have a parent dir"),
        )
        .expect("authoritative state dir should be creatable");
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&json!({
                "latest_authoritative_sequence": 1,
                "harness_phase": "final_review_pending",
                "last_strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
            }))
            .expect("seed authoritative state should serialize"),
        )
        .expect("seed authoritative state should write");
        let rendered = render_final_review_artifacts(
            &runtime,
            &context,
            branch_closure_id,
            reviewed_state_id.as_str(),
            &base_branch,
            FinalReviewProjectionInput {
                dispatch_id,
                reviewer_source,
                reviewer_id,
                result: "pass",
                deviations_required: false,
                summary,
            },
        )
        .expect("final-review artifacts should render for unit coverage");
        let final_review_fingerprint = sha256_hex(rendered.final_review_source.as_bytes());
        let final_review_path = harness_authoritative_artifact_path(
            &runtime.state_dir,
            &runtime.repo_slug,
            &runtime.branch_name,
            &format!("final-review-{final_review_fingerprint}.md"),
        );
        fs::create_dir_all(
            rendered
                .reviewer_artifact_path
                .parent()
                .expect("reviewer artifact should have a parent directory"),
        )
        .expect("reviewer artifact dir should be creatable");
        fs::create_dir_all(
            final_review_path
                .parent()
                .expect("final-review artifact should have a parent directory"),
        )
        .expect("final-review artifact dir should be creatable");
        fs::write(
            &rendered.reviewer_artifact_path,
            &rendered.reviewer_source_text,
        )
        .expect("reviewer artifact should write");
        fs::write(&final_review_path, &rendered.final_review_source)
            .expect("final-review artifact should write");

        fs::write(
            &state_path,
            serde_json::to_string_pretty(&json!({
                "latest_authoritative_sequence": 1,
                "harness_phase": "ready_for_branch_completion",
                "last_strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
                "current_branch_closure_id": branch_closure_id,
                "current_branch_closure_reviewed_state_id": reviewed_state_id.as_str(),
                "current_branch_closure_contract_identity": branch_contract_identity.clone(),
                "current_task_closure_records": {
                    "task-1": {
                        "task": 1,
                        "source_plan_path": context.plan_rel,
                        "source_plan_revision": context.plan_document.plan_revision,
                        "execution_run_id": execution_run_id,
                        "dispatch_id": "unit-test-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "reviewed_state_id": reviewed_state_id.as_str(),
                        "contract_identity": super::current_task_contract_identity(&context, 1)
                            .expect("task contract identity should resolve"),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": "task-review-summary",
                        "verification_result": "pass",
                        "verification_summary_hash": "task-verification-summary",
                        "closure_status": "current"
                    }
                },
                "task_closure_record_history": {
                    "task-1-closure": {
                        "task": 1,
                        "source_plan_path": context.plan_rel,
                        "source_plan_revision": context.plan_document.plan_revision,
                        "execution_run_id": execution_run_id,
                        "dispatch_id": "unit-test-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "reviewed_state_id": reviewed_state_id.as_str(),
                        "contract_identity": super::current_task_contract_identity(&context, 1)
                            .expect("task contract identity should resolve"),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": "task-review-summary",
                        "verification_result": "pass",
                        "verification_summary_hash": "task-verification-summary",
                        "closure_status": "current"
                    }
                },
                "branch_closure_records": {
                    (branch_closure_id): {
                        "branch_closure_id": branch_closure_id,
                        "source_plan_path": context.plan_rel,
                        "source_plan_revision": context.plan_document.plan_revision,
                        "repo_slug": runtime.repo_slug,
                        "branch_name": runtime.branch_name,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id.as_str(),
                        "contract_identity": branch_contract_identity,
                        "effective_reviewed_branch_surface": "repo_tracked_content",
                        "source_task_closure_ids": ["task-1-closure"],
                        "provenance_basis": "task_closure_lineage",
                        "closure_status": "current",
                        "superseded_branch_closure_ids": []
                    }
                },
                "current_final_review_record_id": "unit-final-review-record",
                "current_final_review_branch_closure_id": branch_closure_id,
                "current_final_review_dispatch_id": dispatch_id,
                "current_final_review_reviewer_source": reviewer_source,
                "current_final_review_reviewer_id": reviewer_id,
                "current_final_review_result": "pass",
                "current_final_review_summary_hash": summary_hash,
                "final_review_dispatch_lineage": {
                    "execution_run_id": execution_run_id,
                    "dispatch_id": dispatch_id,
                    "branch_closure_id": branch_closure_id
                },
                "final_review_record_history": {
                    "unit-final-review-record": {
                        "record_id": "unit-final-review-record",
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": branch_closure_id,
                        "source_plan_path": context.plan_rel,
                        "source_plan_revision": context.plan_document.plan_revision,
                        "repo_slug": runtime.repo_slug,
                        "branch_name": runtime.branch_name,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id.as_str(),
                        "dispatch_id": dispatch_id,
                        "reviewer_source": reviewer_source,
                        "reviewer_id": reviewer_id,
                        "result": "pass",
                        "final_review_fingerprint": final_review_fingerprint,
                        "browser_qa_required": false,
                        "summary": summary,
                        "summary_hash": summary_hash
                    }
                }
            }))
            .expect("authoritative state fixture should serialize"),
        )
        .expect("authoritative state fixture should write");
        let fixture_payload: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&state_path).expect("fixture should be readable"),
        )
        .expect("fixture should deserialize after write");
        crate::execution::event_log::sync_fixture_event_log_for_tests(
            &state_path,
            &fixture_payload,
        )
        .expect("unit test fixture sync should publish typed event replay");

        let authoritative_state = load_authoritative_transition_state(&context)
            .expect("authoritative state should load")
            .expect("authoritative state should exist");
        assert!(
            current_final_review_record_is_still_authoritative(
                &context,
                &authoritative_state,
                CurrentFinalReviewAuthorityCheck {
                    branch_closure_id,
                    dispatch_id,
                    reviewer_source,
                    reviewer_id,
                    result: "pass",
                    normalized_summary_hash: &summary_hash,
                },
            )
            .expect("authoritativeness check should succeed for intact artifacts")
        );

        fs::write(
            &final_review_path,
            "# Code Review Result\n\nTampered final-review receipt.\n",
        )
        .expect("tampered final-review receipt should write");

        let authoritative_state = load_authoritative_transition_state(&context)
            .expect("authoritative state should reload after final-review tamper")
            .expect("authoritative state should still exist after final-review tamper");
        assert!(
            current_final_review_record_is_still_authoritative(
                &context,
                &authoritative_state,
                CurrentFinalReviewAuthorityCheck {
                    branch_closure_id,
                    dispatch_id,
                    reviewer_source,
                    reviewer_id,
                    result: "pass",
                    normalized_summary_hash: &summary_hash,
                },
            )
            .expect("authoritativeness check should keep trusting the current record after final-review artifact tamper")
        );

        fs::write(&final_review_path, &rendered.final_review_source)
            .expect("intact final-review receipt should restore");
        fs::write(
            &rendered.reviewer_artifact_path,
            "# Code Review Result\n\nTampered reviewer artifact.\n",
        )
        .expect("tampered reviewer artifact should write");

        let authoritative_state = load_authoritative_transition_state(&context)
            .expect("authoritative state should reload")
            .expect("authoritative state should still exist");
        assert!(
            current_final_review_record_is_still_authoritative(
                &context,
                &authoritative_state,
                CurrentFinalReviewAuthorityCheck {
                    branch_closure_id,
                    dispatch_id,
                    reviewer_source,
                    reviewer_id,
                    result: "pass",
                    normalized_summary_hash: &summary_hash,
                },
            )
            .expect("authoritativeness check should keep trusting the current record after reviewer-artifact tamper")
        );
    }

    #[test]
    fn rewrite_branch_head_bound_artifact_refuses_to_rebind_history() {
        let tempdir = TempDir::new().expect("tempdir should exist");
        let artifact = tempdir.path().join("artifact.md");
        let original = "**Head SHA:** old-head\n";
        fs::write(&artifact, original).expect("head-bound artifact fixture should write");

        let error = rewrite_branch_head_bound_artifact(&artifact, "new-head")
            .expect_err("append-only repair must not rewrite historical head-bound artifacts");

        assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
        assert_eq!(
            fs::read_to_string(&artifact).expect("artifact should remain readable"),
            original
        );
    }

    #[test]
    fn rewrite_branch_qa_artifact_refuses_to_rebind_history() {
        let tempdir = TempDir::new().expect("tempdir should exist");
        let qa_artifact = tempdir.path().join("qa.md");
        let test_plan = tempdir.path().join("test-plan.md");
        let original = "**Head SHA:** old-head\n**Source Test Plan:** `old-plan.md`\n";
        fs::write(&qa_artifact, original).expect("qa artifact fixture should write");
        fs::write(&test_plan, "placeholder").expect("test plan fixture should write");

        let error = rewrite_branch_qa_artifact(&qa_artifact, "new-head", &test_plan)
            .expect_err("append-only repair must not rewrite historical QA artifacts");

        assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
        assert_eq!(
            fs::read_to_string(&qa_artifact).expect("qa artifact should remain readable"),
            original
        );
    }

    #[test]
    fn superseded_branch_closure_ids_reports_previous_current_binding() {
        let overlay: StatusAuthoritativeOverlay = serde_json::from_value(json!({
            "current_branch_closure_id": "branch-release-closure-old"
        }))
        .expect("overlay fixture should deserialize");

        let superseded = superseded_branch_closure_ids_from_previous_current(
            Some(&overlay),
            "branch-release-closure-new",
        );

        assert_eq!(superseded, vec![String::from("branch-release-closure-old")]);
    }

    #[test]
    fn normalized_late_stage_surface_rejects_invalid_entries() {
        for invalid in [
            "/README.md",
            "../README.md",
            "C:/README.md",
            "C:\\README.md",
            "docs/*.md",
            "docs/?",
            "docs/[a]",
            "docs/{a}",
        ] {
            let error =
                normalized_late_stage_surface(&format!("**Late-Stage Surface:** {invalid}\n"))
                    .expect_err("invalid Late-Stage Surface entries must fail closed");
            assert_eq!(
                error.error_class,
                FailureClass::InvalidCommandInput.as_str()
            );
        }
    }

    #[test]
    fn path_matches_late_stage_surface_distinguishes_file_and_directory_entries() {
        assert!(path_matches_late_stage_surface(
            "docs/release.md",
            &[String::from("docs/")]
        ));
        assert!(path_matches_late_stage_surface(
            "docs",
            &[String::from("docs/")]
        ));
        assert!(!path_matches_late_stage_surface(
            "docs-release.md",
            &[String::from("docs/")]
        ));
        assert!(path_matches_late_stage_surface(
            "README.md",
            &[String::from("README.md")]
        ));
        assert!(!path_matches_late_stage_surface(
            "README.md.bak",
            &[String::from("README.md")]
        ));
    }

    #[test]
    fn path_matches_late_stage_surface_is_case_sensitive() {
        assert!(path_matches_late_stage_surface(
            "README.md",
            &[String::from("README.md")]
        ));
        assert!(!path_matches_late_stage_surface(
            "readme.md",
            &[String::from("README.md")]
        ));
    }

    #[test]
    fn task_closure_record_covers_path_respects_directory_surface_entries() {
        let record = CurrentTaskClosureRecord {
            task: 1,
            dispatch_id: String::from("task-1-dispatch"),
            closure_record_id: String::from("task-1-closure"),
            source_plan_path: Some(String::from("docs/featureforge/plans/test-plan.md")),
            source_plan_revision: Some(1),
            execution_run_id: Some(String::from("run-1")),
            reviewed_state_id: String::from("git_tree:deadbeef"),
            semantic_reviewed_state_id: Some(String::from("semantic_tree:deadbeef")),
            contract_identity: String::from("task-1-contract"),
            effective_reviewed_surface_paths: vec![String::from("src/")],
            review_result: String::from("pass"),
            review_summary_hash: String::from("review-hash"),
            verification_result: String::from("pass"),
            verification_summary_hash: String::from("verification-hash"),
            closure_status: Some(String::from("current")),
        };

        assert!(task_closure_record_covers_path(&record, "src/lib.rs"));
        assert!(task_closure_record_covers_path(&record, "src"));
        assert!(!task_closure_record_covers_path(
            &record,
            "src-generated/lib.rs"
        ));
    }

    #[test]
    fn blocked_follow_up_prefers_shared_repair_route_before_branch_closure_fallback() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("executing"),
            phase_detail: String::from("branch_closure_recording_required_for_release_readiness"),
            review_state_status: String::from("stale_unreviewed"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::new(),
            recommended_command: None,
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        assert_eq!(
            blocked_follow_up_for_operator(&operator),
            Some(String::from("repair_review_state"))
        );
        assert_eq!(
            late_stage_required_follow_up("final_review", &operator),
            Some(String::from("repair_review_state"))
        );
    }

    #[test]
    fn advance_late_stage_final_review_with_dispatch_id_requeries_when_dispatch_follow_up_is_required()
     {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("final_review_pending"),
            phase_detail: String::from("final_review_dispatch_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::new(),
            recommended_command: None,
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        let output = advance_late_stage_follow_up_or_requery_output(
            &operator,
            Path::new("docs/featureforge/plans/example.md"),
            false,
            AdvanceLateStageOutputContext {
                stage_path: "final_review",
                delegated_primitive: "record-final-review",
                branch_closure_id: Some(String::from("branch-closure-1")),
                dispatch_id: Some(String::from("dispatch-123")),
                result: "pass",
                external_review_result_ready: true,
                trace_summary: "advance-late-stage failed closed because workflow/operator requery is required.",
            },
        );

        assert_eq!(output.action, "blocked");
        assert_eq!(
            output.code.as_deref(),
            Some("out_of_phase_requery_required")
        );
        assert_eq!(
            output.recommended_command.as_deref(),
            Some(
                "featureforge workflow operator --plan docs/featureforge/plans/example.md --external-review-result-ready",
            )
        );
        assert_eq!(output.rederive_via_workflow_operator, Some(true));
        assert_eq!(output.required_follow_up, None);
    }

    #[test]
    fn advance_late_stage_final_review_with_matching_dispatch_lineage_keeps_dispatch_follow_up() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("final_review_pending"),
            phase_detail: String::from("final_review_dispatch_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::new(),
            recommended_command: None,
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        let output = advance_late_stage_follow_up_or_requery_output(
            &operator,
            Path::new("docs/featureforge/plans/example.md"),
            true,
            AdvanceLateStageOutputContext {
                stage_path: "final_review",
                delegated_primitive: "record-final-review",
                branch_closure_id: Some(String::from("branch-closure-1")),
                dispatch_id: Some(String::from("dispatch-123")),
                result: "pass",
                external_review_result_ready: true,
                trace_summary: "advance-late-stage follow-up required.",
            },
        );

        assert_eq!(output.action, "blocked");
        assert_eq!(output.code, None);
        assert_eq!(output.recommended_command, None);
        assert_eq!(output.rederive_via_workflow_operator, None);
        assert_eq!(
            output.required_follow_up,
            Some(String::from("request_external_review"))
        );
    }

    #[test]
    fn blocked_follow_up_routes_clean_execution_reentry_repair_state_to_repair_review_state() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("executing"),
            phase_detail: String::from("execution_reentry_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("repair review state / reenter execution"),
            recommended_command: Some(String::from(
                "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
            )),
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        assert_eq!(
            blocked_follow_up_for_operator(&operator),
            Some(String::from("repair_review_state"))
        );
        assert_eq!(
            late_stage_required_follow_up("release_readiness", &operator),
            Some(String::from("repair_review_state"))
        );
        assert_eq!(
            late_stage_required_follow_up("final_review", &operator),
            Some(String::from("repair_review_state"))
        );
    }

    #[test]
    fn close_current_task_follow_up_preserves_structural_repair_state_lane() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("executing"),
            phase_detail: String::from("execution_reentry_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("repair review state / reenter execution"),
            recommended_command: Some(String::from(
                "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
            )),
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        assert_eq!(
            close_current_task_required_follow_up(&operator),
            Some(String::from("repair_review_state"))
        );
    }

    #[test]
    fn close_current_task_follow_up_preserves_stale_repair_state_lane() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("executing"),
            phase_detail: String::from("execution_reentry_required"),
            review_state_status: String::from("stale_unreviewed"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("repair review state / reenter execution"),
            recommended_command: Some(String::from(
                "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
            )),
            blocking_scope: None,
            blocking_task: Some(2),
            external_wait_state: None,
            blocking_reason_codes: vec![String::from("prior_task_current_closure_stale")],
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        assert_eq!(
            close_current_task_required_follow_up(&operator),
            Some(String::from("repair_review_state"))
        );
    }

    #[test]
    fn close_current_task_follow_up_waits_for_external_review_result_when_task_review_is_pending() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("task_closure_pending"),
            phase_detail: String::from("task_review_result_pending"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("wait for external review result"),
            recommended_command: None,
            blocking_scope: Some(String::from("task")),
            blocking_task: Some(1),
            external_wait_state: Some(String::from("waiting_for_external_review_result")),
            blocking_reason_codes: vec![String::from("prior_task_review_not_green")],
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: Some(String::from("dispatch-task-1")),
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        assert_eq!(
            close_current_task_required_follow_up(&operator),
            Some(String::from("wait_for_external_review_result"))
        );
    }

    #[test]
    fn close_current_task_follow_up_preserves_request_external_review_for_non_task_dispatch_phase()
    {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("final_review_pending"),
            phase: String::from("final_review_pending"),
            phase_detail: String::from("final_review_dispatch_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("request external review"),
            recommended_command: None,
            blocking_scope: Some(String::from("branch")),
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: vec![String::from("final_review_dispatch_missing")],
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: Some(String::from("branch-closure-1")),
            current_release_readiness_result: Some(String::from("pass")),
            base_branch: None,
        };

        assert_eq!(
            close_current_task_required_follow_up(&operator),
            Some(String::from("request_external_review"))
        );
    }

    #[test]
    fn close_current_task_follow_up_requires_verification_when_verification_is_missing() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("task_closure_pending"),
            phase_detail: String::from("task_review_result_pending"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("wait for external review result"),
            recommended_command: None,
            blocking_scope: Some(String::from("task")),
            blocking_task: Some(1),
            external_wait_state: None,
            blocking_reason_codes: vec![String::from("prior_task_verification_missing")],
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: Some(String::from("dispatch-task-1")),
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        assert_eq!(
            close_current_task_required_follow_up(&operator),
            Some(String::from("run_verification"))
        );
    }

    #[test]
    fn close_current_task_outcome_class_treats_review_fail_verification_pass_as_negative() {
        assert_eq!(
            close_current_task_outcome_class(ReviewOutcomeArg::Fail, VerificationOutcomeArg::Pass),
            CloseCurrentTaskOutcomeClass::Negative
        );
    }

    #[test]
    fn blocked_close_current_task_output_from_operator_keeps_shared_follow_up_and_command() {
        let operator = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_receipt: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("executing"),
            phase_detail: String::from("execution_reentry_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("repair review state / reenter execution"),
            recommended_command: Some(String::from(
                "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
            )),
            blocking_scope: None,
            blocking_task: Some(3),
            external_wait_state: None,
            blocking_reason_codes: vec![String::from(
                "prior_task_current_closure_reviewed_state_malformed",
            )],
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };
        let output = blocked_close_current_task_output_from_operator(
            3,
            &operator,
            "close-current-task must return the shared blocked route when routing is out-of-phase.",
        );
        assert_eq!(output.action, "blocked");
        assert_eq!(output.code, None);
        assert_eq!(
            output.required_follow_up,
            Some(String::from("repair_review_state"))
        );
        assert_eq!(
            output.recommended_command.as_deref(),
            Some(
                "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md"
            )
        );
        assert_eq!(output.rederive_via_workflow_operator, None);
    }
}
