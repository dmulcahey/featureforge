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
    NoteArgs, RebuildEvidenceArgs, RecordBranchClosureArgs, RecordFinalReviewArgs, RecordQaArgs,
    RecordReleaseReadinessArgs, ReopenArgs, ReviewOutcomeArg, StatusArgs, TransferArgs,
    VerificationOutcomeArg,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::authority::write_authoritative_unit_review_receipt_artifact;
use crate::execution::command_eligibility::{
    blocked_follow_up_for_operator, close_current_task_required_follow_up,
    late_stage_required_follow_up, negative_result_follow_up,
    operator_requires_review_state_repair, release_readiness_required_follow_up,
};
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason,
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    branch_closure_rerecording_assessment,
    branch_contract_identity as shared_branch_contract_identity,
    branch_source_task_closure_ids as shared_branch_source_task_closure_ids,
    current_branch_closure_baseline_tree_sha as shared_current_branch_closure_baseline_tree_sha,
    current_branch_closure_has_tracked_drift as shared_current_branch_closure_has_tracked_drift,
    final_review_dispatch_still_current as shared_final_review_dispatch_still_current,
    finish_requires_test_plan_refresh as shared_finish_requires_test_plan_refresh,
    handoff_decision_scope as shared_handoff_decision_scope,
    live_review_state_repair_reroute as shared_live_review_state_repair_reroute,
    live_task_scope_repair_precedence_active as shared_live_task_scope_repair_precedence_active,
    normalize_summary_content,
    public_late_stage_stale_unreviewed as shared_public_late_stage_stale_unreviewed,
    public_review_state_stale_unreviewed_for_reroute as shared_public_review_state_stale_unreviewed_for_reroute,
    render_late_stage_surface_only_branch_surface as late_stage_surface_only_branch_surface,
    reviewer_source_is_valid as shared_reviewer_source_is_valid,
    summary_hash,
    task_closure_contributes_to_branch_surface as shared_task_closure_contributes_to_branch_surface,
    task_scope_overlay_restore_required as shared_task_scope_overlay_restore_required,
    task_scope_stale_review_state_reason_present as shared_task_scope_stale_review_state_reason_present,
};
#[cfg(test)]
use crate::execution::current_truth::{
    normalized_late_stage_surface, path_matches_late_stage_surface,
};
use crate::execution::final_review::authoritative_strategy_checkpoint_fingerprint_checked;
use crate::execution::handoff::{
    WorkflowTransferRecordIdentity, WorkflowTransferRecordInput,
    current_workflow_transfer_record_exists, latest_matching_workflow_transfer_request_record,
    write_workflow_transfer_record,
};
use crate::execution::leases::{
    StatusAuthoritativeOverlay,
    authoritative_matching_execution_topology_downgrade_records_checked,
    load_status_authoritative_overlay_checked,
};
use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
use crate::execution::projection_renderer::{
    BranchClosureProjectionInput, FinalReviewProjectionInput, QaProjectionInput,
    publish_authoritative_artifact, regenerate_projection_artifacts_from_authoritative_state,
    render_branch_closure_artifact, render_final_review_artifacts, render_qa_artifact,
    render_release_readiness_artifact, timestamp_slug, write_project_artifact,
};
use crate::execution::query::{
    ExecutionRoutingState, query_review_state, query_workflow_routing_state_for_runtime,
};
use crate::execution::recording::{
    BranchClosureWrite, BrowserQaWrite, CurrentTaskClosureWrite, FinalReviewWrite,
    NegativeTaskClosureWrite, ReleaseReadinessWrite, record_browser_qa,
    record_current_branch_closure, record_current_task_closure,
    record_final_review as persist_final_review_record, record_negative_task_closure,
    record_release_readiness as persist_release_readiness_record,
};
use crate::execution::state::{
    EvidenceAttempt, ExecutionContext, ExecutionEvidence, ExecutionRuntime, FileProof,
    NO_REPO_FILES_MARKER, PacketFingerprintInput, PlanExecutionStatus, PlanStepState,
    RebuildEvidenceCandidate, RebuildEvidenceCounts, RebuildEvidenceFilter, RebuildEvidenceOutput,
    RebuildEvidenceTarget, branch_closure_record_matches_plan_exemption,
    compute_packet_fingerprint, current_file_proof, current_head_sha,
    current_test_plan_artifact_path_for_finish, discover_rebuild_candidates,
    gate_finish_from_context, gate_review_from_context, hash_contract_plan,
    live_review_state_status_for_reroute_from_status, load_execution_context_for_exact_plan,
    load_execution_context_for_mutation, normalize_begin_request, normalize_complete_request,
    normalize_note_request, normalize_rebuild_evidence_request, normalize_reopen_request,
    normalize_source, normalize_transfer_request, require_normalized_text,
    require_preflight_acceptance, require_prior_task_closure_for_begin, status_from_context,
    still_current_task_closure_records, structural_current_task_closure_failures,
    task_completion_lineage_fingerprint, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason, usable_current_branch_closure_identity,
    validate_expected_fingerprint,
};
use crate::execution::transitions::{
    AuthoritativeTransitionState, BranchClosureRecord, CurrentTaskClosureRecord, StepCommand,
    claim_step_write_authority, enforce_active_contract_scope, enforce_authoritative_phase,
    load_authoritative_transition_state,
};
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
    pub trace_summary: String,
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

fn consume_execution_reentry_repair_follow_up(
    authoritative_state: Option<&mut AuthoritativeTransitionState>,
) -> Result<bool, JsonFailure> {
    let Some(authoritative_state) = authoritative_state else {
        return Ok(false);
    };
    if authoritative_state.review_state_repair_follow_up() != Some("execution_reentry") {
        return Ok(false);
    }
    authoritative_state.set_review_state_repair_follow_up(None)?;
    authoritative_state.set_harness_phase_executing()?;
    Ok(true)
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
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Begin)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Begin,
        request.task,
        request.step,
    )?;

    let step_index = step_index(&context, request.task, request.step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested task/step does not exist in the approved plan.",
        )
    })?;
    if context.steps[step_index].checked {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "begin may not target a completed step.",
        ));
    }

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
            let consumed_execution_reentry_follow_up =
                consume_execution_reentry_repair_follow_up(authoritative_state.as_mut())?;
            if consumed_execution_reentry_follow_up
                && let Some(authoritative_state) = authoritative_state.as_mut()
            {
                authoritative_state.persist_if_dirty_with_failpoint(None)?;
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
    let interrupted_step = context
        .steps
        .iter()
        .find(|step| step.note_state == Some(crate::execution::state::NoteState::Interrupted));
    let resuming_interrupted_same_step = interrupted_step.is_some_and(|interrupted| {
        interrupted.task_number == request.task && interrupted.step_number == request.step
    });
    if interrupted_step.is_some() && !resuming_interrupted_same_step {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Interrupted work must resume on the same step.",
        ));
    }

    require_prior_task_closure_for_begin(&context, request.task)?;

    context.steps[step_index].note_state = Some(crate::execution::state::NoteState::Active);
    context.steps[step_index].note_summary = truncate_summary(&require_normalized_text(
        &context.steps[step_index].title,
        FailureClass::InvalidCommandInput,
        "Execution note summaries may not be blank after whitespace normalization.",
    )?);
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.ensure_initial_dispatch_strategy_checkpoint(
            &context,
            &context.plan_document.execution_mode,
        )?;
        consume_execution_reentry_repair_follow_up(Some(authoritative_state))?;
    }

    let rendered_plan = render_plan_source(
        &context.plan_source,
        &context.plan_document.execution_mode,
        &context.steps,
    );
    write_atomic(&context.plan_abs, &rendered_plan)?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "begin_after_plan_write_before_authoritative_state_publish",
        )?;
    }
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
    normalize_source(&request.source, &context.plan_document.execution_mode)?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Complete)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Complete,
        request.task,
        request.step,
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

    let contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let packet_fingerprint = compute_packet_fingerprint(PacketFingerprintInput {
        plan_path: &context.plan_rel,
        plan_revision: context.plan_document.plan_revision,
        plan_fingerprint: &contract_plan_fingerprint,
        source_spec_path: &context.plan_document.source_spec_path,
        source_spec_revision: context.plan_document.source_spec_revision,
        source_spec_fingerprint: &source_spec_fingerprint,
        task: request.task,
        step: request.step,
    });
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

    let rendered_plan = render_plan_source(
        &context.plan_source,
        &context.plan_document.execution_mode,
        &context.steps,
    );
    let plan_fingerprint = sha256_hex(rendered_plan.as_bytes());
    let rendered_evidence =
        render_evidence_source(&context, &plan_fingerprint, &source_spec_fingerprint);
    let consumed_execution_reentry_follow_up =
        consume_execution_reentry_repair_follow_up(authoritative_state.as_mut())?;

    write_plan_and_evidence_with_rollback(
        &context.plan_abs,
        &context.plan_source,
        &rendered_plan,
        &context.evidence_abs,
        context.evidence.source.as_deref(),
        &rendered_evidence,
        "complete_after_plan_write",
    )?;
    if consumed_execution_reentry_follow_up
        && let Some(authoritative_state) = authoritative_state.as_ref()
    {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "complete_after_plan_and_evidence_write_before_authoritative_state_publish",
        )?;
    }
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
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
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

    context.steps[step_index].note_state = Some(request.state);
    context.steps[step_index].note_summary = request.message;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.apply_note_reset_policy(request.state)?;
    }

    let rendered_plan = render_plan_source(
        &context.plan_source,
        &context.plan_document.execution_mode,
        &context.steps,
    );
    write_atomic(&context.plan_abs, &rendered_plan)?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "note_after_plan_write_before_authoritative_state_publish",
        )?;
    }
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
    normalize_source(&request.source, &context.plan_document.execution_mode)?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
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
    if !context.steps[step_index].checked {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "reopen may target only a completed step.",
        ));
    }
    if context
        .steps
        .iter()
        .any(|step| step.note_state == Some(crate::execution::state::NoteState::Interrupted))
    {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "reopen may not create a second parked interrupted step while one already exists.",
        ));
    }

    invalidate_latest_completed_attempt(&mut context, request.task, request.step, &request.reason)?;
    context.steps[step_index].checked = false;
    context.steps[step_index].note_state = Some(crate::execution::state::NoteState::Interrupted);
    context.steps[step_index].note_summary = truncate_summary(&request.reason);
    context.evidence.format = crate::execution::state::EvidenceFormat::V2;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.stale_reopen_provenance()?;
        authoritative_state.record_reopen_strategy_checkpoint(
            &context,
            &context.plan_document.execution_mode,
            request.task,
            request.step,
            &request.reason,
        )?;
        consume_execution_reentry_repair_follow_up(Some(authoritative_state))?;
    }

    let rendered_plan = render_plan_source(
        &context.plan_source,
        &context.plan_document.execution_mode,
        &context.steps,
    );
    let plan_fingerprint = sha256_hex(rendered_plan.as_bytes());
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let rendered_evidence =
        render_evidence_source(&context, &plan_fingerprint, &source_spec_fingerprint);
    write_plan_and_evidence_with_rollback(
        &context.plan_abs,
        &context.plan_source,
        &rendered_plan,
        &context.evidence_abs,
        context.evidence.source.as_deref(),
        &rendered_evidence,
        "reopen_after_plan_write",
    )?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_rollback(
            authoritative_state,
            &context.plan_abs,
            &context.plan_source,
            &context.evidence_abs,
            context.evidence.source.as_deref(),
            "reopen_after_plan_and_evidence_write_before_authoritative_state_publish",
        )?;
    }

    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
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
    let authoritative_state = load_authoritative_transition_state(&context)?;
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

    invalidate_latest_completed_attempt(&mut context, repair_task, repair_step, reason)?;
    context.steps[repair_index].checked = false;
    context.steps[repair_index].note_state = None;
    context.steps[repair_index].note_summary.clear();
    context.steps[active_index].note_state = Some(crate::execution::state::NoteState::Interrupted);
    context.steps[active_index].note_summary = truncate_summary(&format!(
        "Parked for repair of Task {repair_task} Step {repair_step}"
    ));
    context.evidence.format = crate::execution::state::EvidenceFormat::V2;

    let rendered_plan = render_plan_source(
        &context.plan_source,
        &context.plan_document.execution_mode,
        &context.steps,
    );
    let plan_fingerprint = sha256_hex(rendered_plan.as_bytes());
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let rendered_evidence =
        render_evidence_source(&context, &plan_fingerprint, &source_spec_fingerprint);
    write_plan_and_evidence_with_rollback(
        &context.plan_abs,
        &context.plan_source,
        &rendered_plan,
        &context.evidence_abs,
        context.evidence.source.as_deref(),
        &rendered_evidence,
        "transfer_after_plan_write",
    )?;

    let reloaded = load_execution_context_for_mutation(runtime, plan)?;
    status_with_shared_routing_or_context(runtime, plan, &reloaded)
}

fn status_with_shared_routing_or_context(
    runtime: &ExecutionRuntime,
    plan: &Path,
    fallback_context: &ExecutionContext,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let args = StatusArgs {
        plan: plan.to_path_buf(),
        external_review_result_ready: false,
    };
    match runtime.status(&args) {
        Ok(status) => Ok(status),
        Err(error)
            if error.error_class == FailureClass::MalformedExecutionState.as_str()
                && error
                    .message
                    .contains("Legacy pre-harness execution evidence is no longer accepted") =>
        {
            status_from_context(fallback_context)
        }
        Err(error) => Err(error),
    }
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
    if operator.phase != "handoff_required" || operator.phase_detail != "handoff_recording_required"
    {
        return Ok(TransferOutput {
            action: String::from("blocked"),
            scope: scope.to_owned(),
            to: to.to_owned(),
            reason: reason.to_owned(),
            record_path: None,
            code: Some(String::from("out_of_phase_requery_required")),
            recommended_command: Some(recommended_operator_command(plan)),
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
        authoritative_state.persist_if_dirty_with_failpoint(None)?;
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

    if current_workflow_transfer_record_exists(&runtime.state_dir, identity) {
        return Ok(TransferOutput {
            action: String::from("blocked"),
            scope: scope.to_owned(),
            to: to.to_owned(),
            reason: reason.to_owned(),
            record_path: None,
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
    authoritative_state.persist_if_dirty_with_failpoint(None)?;

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

pub fn close_current_task(
    runtime: &ExecutionRuntime,
    args: &CloseCurrentTaskArgs,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let status = status_with_shared_routing_or_context(runtime, &args.plan, &context)?;
    let execution_run_id = status
        .execution_run_id
        .as_ref()
        .map(|value| value.0.clone())
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires an active execution run identity.",
            )
        })?;
    let strategy_checkpoint_fingerprint =
        authoritative_strategy_checkpoint_fingerprint_checked(&context)?.ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "close-current-task requires authoritative strategy checkpoint provenance.",
            )
        })?;
    ensure_task_dispatch_id_matches(&context, args.task, &args.dispatch_id)?;
    let verification_result = args.verification_result.as_str();
    let mut summary_hashes: Option<(String, String)> = None;
    let reviewed_state_id = current_task_reviewed_state_id(&context, args.task)?;
    let contract_identity = current_task_contract_identity(&context, args.task);
    let closure_record_id = current_task_closure_record_id(&context, args.task)?;
    match task_dispatch_reviewed_state_status(
        &context,
        args.task,
        &reviewed_state_id,
    )? {
        TaskDispatchReviewedStateStatus::Current => {}
        TaskDispatchReviewedStateStatus::MissingReviewedStateBinding => {
            return Ok(CloseCurrentTaskOutput {
                action: String::from("blocked"),
                task_number: args.task,
                dispatch_validation_action: String::from("blocked"),
                closure_action: String::from("blocked"),
                task_closure_status: String::from("not_current"),
                superseded_task_closure_ids: Vec::new(),
                closure_record_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: Some(String::from("request_external_review")),
                trace_summary: String::from(
                    "close-current-task failed closed because the current task review dispatch lineage does not bind a current reviewed state.",
                ),
            });
        }
        TaskDispatchReviewedStateStatus::StaleReviewedState => {
            return Ok(CloseCurrentTaskOutput {
                action: String::from("blocked"),
                task_number: args.task,
                dispatch_validation_action: String::from("blocked"),
                closure_action: String::from("blocked"),
                task_closure_status: String::from("not_current"),
                superseded_task_closure_ids: Vec::new(),
                closure_record_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: Some(String::from("execution_reentry")),
                trace_summary: String::from(
                    "close-current-task failed closed because tracked workspace state changed after the current task review dispatch was recorded.",
                ),
            });
        }
    }
    let current_task_recording_ready = |operator: &ExecutionRoutingState| {
        operator.phase == "task_closure_pending"
            && operator.phase_detail == "task_closure_recording_ready"
            && operator.review_state_status == "clean"
            && operator
                .recording_context
                .as_ref()
                .and_then(|context| context.task_number)
                == Some(args.task)
            && operator
                .recording_context
                .as_ref()
                .and_then(|context| context.dispatch_id.as_deref())
                == Some(args.dispatch_id.as_str())
    };
    {
        let _write_authority = claim_step_write_authority(runtime)?;
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires authoritative harness state.",
            ));
        };
        if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
            && current_record.closure_record_id == closure_record_id
            && current_record.dispatch_id == args.dispatch_id
        {
            if summary_hashes.is_none() {
                summary_hashes = Some(close_current_task_summary_hashes(args)?);
            }
            let (review_summary_hash, verification_summary_hash) = summary_hashes
                .as_ref()
                .expect("summary hashes should exist after summary validation");
            if current_record.review_result == args.review_result.as_str()
                && current_record.review_summary_hash == review_summary_hash.as_str()
                && current_record.verification_result == verification_result
                && current_record.verification_summary_hash == verification_summary_hash.as_str()
            {
                return Ok(CloseCurrentTaskOutput {
                    action: String::from("already_current"),
                    task_number: args.task,
                    dispatch_validation_action: String::from("validated"),
                    closure_action: String::from("already_current"),
                    task_closure_status: String::from("current"),
                    superseded_task_closure_ids: Vec::new(),
                    closure_record_id: Some(closure_record_id),
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: None,
                    trace_summary: String::from(
                        "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                    ),
                });
            }
            return Ok(CloseCurrentTaskOutput {
                action: String::from("blocked"),
                task_number: args.task,
                dispatch_validation_action: String::from("validated"),
                closure_action: String::from("blocked"),
                task_closure_status: String::from("current"),
                superseded_task_closure_ids: Vec::new(),
                closure_record_id: Some(closure_record_id),
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: None,
                trace_summary: String::from(
                    "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                ),
            });
        }
        if let Some(negative_record) = authoritative_state.task_closure_negative_result(args.task)
            && negative_record.dispatch_id == args.dispatch_id
            && negative_record.reviewed_state_id == reviewed_state_id
            && negative_record.contract_identity == contract_identity
        {
            let operator = current_workflow_operator(runtime, &args.plan, true)?;
            return Ok(CloseCurrentTaskOutput {
                action: String::from("blocked"),
                task_number: args.task,
                dispatch_validation_action: String::from("validated"),
                closure_action: String::from("blocked"),
                task_closure_status: String::from("not_current"),
                superseded_task_closure_ids: Vec::new(),
                closure_record_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: negative_result_follow_up(&operator),
                trace_summary: String::from(
                    "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                ),
            });
        }
    }
    let operator = current_workflow_operator(runtime, &args.plan, true)?;
    if !current_task_recording_ready(&operator) {
        let required_follow_up = close_current_task_required_follow_up(&operator);
        if required_follow_up.is_none() {
            return Ok(shared_out_of_phase_close_current_task_output(
                &args.plan,
                args.task,
                "close-current-task failed closed because workflow/operator did not expose task_closure_recording_ready for the supplied dispatch lineage.",
            ));
        }
        return Ok(CloseCurrentTaskOutput {
            action: String::from("blocked"),
            task_number: args.task,
            dispatch_validation_action: String::from("blocked"),
            closure_action: String::from("blocked"),
            task_closure_status: String::from("not_current"),
            superseded_task_closure_ids: Vec::new(),
            closure_record_id: None,
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            required_follow_up,
            trace_summary: String::from(
                "close-current-task failed closed because workflow/operator did not expose task_closure_recording_ready for the supplied dispatch lineage.",
            ),
        });
    }
    if summary_hashes.is_none() {
        summary_hashes = Some(close_current_task_summary_hashes(args)?);
    }
    let (review_summary_hash, verification_summary_hash) =
        summary_hashes.expect("summary hashes should exist after summary validation");
    match close_current_task_outcome_class(args.review_result, args.verification_result) {
        CloseCurrentTaskOutcomeClass::Positive => {
            let effective_reviewed_surface_paths =
                current_task_effective_reviewed_surface_paths(&context, args.task)?;
            refresh_rebuild_task_closure_receipts_with_context(
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
            ensure_task_dispatch_id_matches(&locked_context, args.task, &args.dispatch_id)?;
            if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
                && current_record.closure_record_id == closure_record_id
                && current_record.dispatch_id == args.dispatch_id
            {
                if current_record.review_result == args.review_result.as_str()
                    && current_record.review_summary_hash == review_summary_hash
                    && current_record.verification_result == verification_result
                    && current_record.verification_summary_hash == verification_summary_hash
                {
                    return Ok(CloseCurrentTaskOutput {
                        action: String::from("already_current"),
                        task_number: args.task,
                        dispatch_validation_action: String::from("validated"),
                        closure_action: String::from("already_current"),
                        task_closure_status: String::from("current"),
                        superseded_task_closure_ids: Vec::new(),
                        closure_record_id: Some(closure_record_id),
                        code: None,
                        recommended_command: None,
                        rederive_via_workflow_operator: None,
                        required_follow_up: None,
                        trace_summary: String::from(
                            "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                        ),
                    });
                }
                return Ok(CloseCurrentTaskOutput {
                    action: String::from("blocked"),
                    task_number: args.task,
                    dispatch_validation_action: String::from("validated"),
                    closure_action: String::from("blocked"),
                    task_closure_status: String::from("current"),
                    superseded_task_closure_ids: Vec::new(),
                    closure_record_id: Some(closure_record_id),
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: None,
                    trace_summary: String::from(
                        "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    ),
                });
            }
            if let Some(negative_record) =
                authoritative_state.task_closure_negative_result(args.task)
                && negative_record.dispatch_id == args.dispatch_id
                && negative_record.reviewed_state_id == reviewed_state_id
                && negative_record.contract_identity == contract_identity
            {
                return Ok(CloseCurrentTaskOutput {
                    action: String::from("blocked"),
                    task_number: args.task,
                    dispatch_validation_action: String::from("validated"),
                    closure_action: String::from("blocked"),
                    task_closure_status: String::from("not_current"),
                    superseded_task_closure_ids: Vec::new(),
                    closure_record_id: None,
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: negative_result_follow_up(&operator),
                    trace_summary: String::from(
                        "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                    ),
                });
            }
            let superseded_task_closure_records = superseded_task_closure_records(
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
            record_current_task_closure(
                authoritative_state,
                CurrentTaskClosureWrite {
                    task: args.task,
                    dispatch_id: &args.dispatch_id,
                    closure_record_id: &closure_record_id,
                    reviewed_state_id: &reviewed_state_id,
                    contract_identity: &contract_identity,
                    effective_reviewed_surface_paths: &effective_reviewed_surface_paths,
                    review_result: args.review_result.as_str(),
                    review_summary_hash: &review_summary_hash,
                    verification_result,
                    verification_summary_hash: &verification_summary_hash,
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
                trace_summary: String::from(
                    "Validated task review dispatch lineage and refreshed authoritative task review and verification receipts.",
                ),
            })
        }
        CloseCurrentTaskOutcomeClass::Negative => {
            let _write_authority = claim_step_write_authority(runtime)?;
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            let locked_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            ensure_task_dispatch_id_matches(&locked_context, args.task, &args.dispatch_id)?;
            if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
                && current_record.closure_record_id == closure_record_id
                && current_record.dispatch_id == args.dispatch_id
            {
                return Ok(CloseCurrentTaskOutput {
                    action: String::from("blocked"),
                    task_number: args.task,
                    dispatch_validation_action: String::from("validated"),
                    closure_action: String::from("blocked"),
                    task_closure_status: String::from("current"),
                    superseded_task_closure_ids: Vec::new(),
                    closure_record_id: Some(closure_record_id),
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: None,
                    trace_summary: String::from(
                        "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    ),
                });
            }
            if let Some(negative_record) =
                authoritative_state.task_closure_negative_result(args.task)
                && negative_record.dispatch_id == args.dispatch_id
                && negative_record.reviewed_state_id == reviewed_state_id
                && negative_record.contract_identity == contract_identity
            {
                return Ok(CloseCurrentTaskOutput {
                    action: String::from("blocked"),
                    task_number: args.task,
                    dispatch_validation_action: String::from("validated"),
                    closure_action: String::from("blocked"),
                    task_closure_status: String::from("not_current"),
                    superseded_task_closure_ids: Vec::new(),
                    closure_record_id: None,
                    code: None,
                    recommended_command: None,
                    rederive_via_workflow_operator: None,
                    required_follow_up: negative_result_follow_up(&operator),
                    trace_summary: String::from(
                        "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                    ),
                });
            }
            record_negative_task_closure(
                authoritative_state,
                NegativeTaskClosureWrite {
                    task: args.task,
                    dispatch_id: &args.dispatch_id,
                    reviewed_state_id: &reviewed_state_id,
                    contract_identity: &contract_identity,
                    review_result: args.review_result.as_str(),
                    review_summary_hash: &review_summary_hash,
                    verification_result,
                    verification_summary_hash: &verification_summary_hash,
                },
            )?;
            Ok(CloseCurrentTaskOutput {
                action: String::from("blocked"),
                task_number: args.task,
                dispatch_validation_action: String::from("validated"),
                closure_action: String::from("blocked"),
                task_closure_status: String::from("not_current"),
                superseded_task_closure_ids: Vec::new(),
                closure_record_id: None,
                code: None,
                recommended_command: None,
                rederive_via_workflow_operator: None,
                required_follow_up: negative_result_follow_up(&operator),
                trace_summary: String::from(
                    "Task closure remained blocked because the supplied review or verification outcome was not passing.",
                ),
            })
        }
        CloseCurrentTaskOutcomeClass::Invalid => Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "verification_not_run_incompatible_with_passing_review: a passing task closure requires passing verification in the first slice.",
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
    let allow_repair_reroute_record_branch_closure =
        repair_review_state_record_branch_closure_reroute_active(runtime, &context, args)?;
    let allow_pre_release_branch_closure_already_current =
        pre_release_branch_closure_already_current_allowed(&context, &operator, &reviewed_state)?;
    if let Some(blocked_output) =
        blocked_branch_closure_output_for_invalid_current_task_closure(&context)?
    {
        return Ok(blocked_output);
    }
    let branch_closure_recording_ready = operator.phase == "document_release_pending"
        && operator.phase_detail == "branch_closure_recording_required_for_release_readiness";
    if !branch_closure_recording_ready {
        if allow_repair_reroute_record_branch_closure {
            // repair-review-state established a branch-scope reroute for confined Late-Stage
            // Surface drift, so branch-closure re-recording is the current safe follow-up even
            // before workflow/operator is queried again.
        } else if operator_requires_review_state_repair(&operator) {
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
        } else if allow_pre_release_branch_closure_already_current {
            // Allow an idempotent already_current result when the current branch closure still
            // matches the current reviewed state, even though workflow/operator has already moved
            // on to release-readiness recording.
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
    let rerecording_assessment = branch_closure_rerecording_assessment(&context)?;
    let changed_paths = rerecording_assessment.changed_paths.clone();
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
            &current_branch_task_closure_records(&context)?,
            Some(late_stage_surface),
        );
        if reviewed_state.source_task_closure_ids.is_empty() {
            reviewed_state.effective_reviewed_branch_surface =
                late_stage_surface_only_branch_surface(&changed_paths);
        }
    }
    let current_identity = usable_current_branch_closure_identity(&context);
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires authoritative harness state.",
        ));
    };
    if let Some(current_identity) = current_identity.as_ref()
        && authoritative_state
            .branch_closure_record(&current_identity.branch_closure_id)
            .is_some_and(|record| {
                branch_closure_record_matches_reviewed_state(&record, &context, &reviewed_state)
                    || branch_closure_record_matches_empty_lineage_late_stage_exemption(
                        &record,
                        &context,
                        &reviewed_state,
                    )
            })
    {
        authoritative_state.restore_current_branch_closure_overlay_fields(
            &current_identity.branch_closure_id,
            &reviewed_state.reviewed_state_id,
            &reviewed_state.contract_identity,
        )?;
        authoritative_state.set_review_state_repair_follow_up(None)?;
        authoritative_state.persist_if_dirty_with_failpoint(None)?;
        return Ok(RecordBranchClosureOutput {
            action: String::from("already_current"),
            branch_closure_id: Some(current_identity.branch_closure_id.clone()),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            superseded_branch_closure_ids: Vec::new(),
            required_follow_up: None,
            trace_summary: String::from(
                "Current reviewed branch state already has an authoritative current branch closure.",
            ),
        });
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

pub fn advance_late_stage(
    runtime: &ExecutionRuntime,
    args: &AdvanceLateStageArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let supplied_result_label = advance_late_stage_result_label(args.result);
    let current_branch_closure = current_authoritative_branch_closure_binding_optional(&context)?;
    let branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
    if let Some(dispatch_id) = args.dispatch_id.as_ref() {
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
                        trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                    },
                ));
            }
            Err(error) => return Err(error),
        };
        if operator.review_state_status != "clean"
            || operator.phase != "final_review_pending"
            || operator.phase_detail != "final_review_recording_ready"
            || operator
                .recording_context
                .as_ref()
                .and_then(|context| context.dispatch_id.as_deref())
                != Some(dispatch_id.as_str())
        {
            if operator.review_state_status == "clean"
                && let Some(current_branch_closure) = current_branch_closure.as_ref()
                && let Some(output) = equivalent_current_final_review_rerun(
                    &context,
                    current_branch_closure,
                    EquivalentFinalReviewRerunParams {
                        stage_path: "final_review",
                        delegated_primitive: "record-final-review",
                        dispatch_id,
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
                ensure_final_review_dispatch_id_matches(&context, dispatch_id).is_ok(),
                AdvanceLateStageOutputContext {
                    stage_path: "final_review",
                    delegated_primitive: "record-final-review",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: Some(dispatch_id.clone()),
                    result,
                    trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                },
            ));
        }
        let summary = read_nonempty_summary_file(summary_file, "summary")?;
        let normalized_summary_hash = summary_hash(&summary);
        let current_branch_closure = authoritative_current_branch_closure_binding(
            &context,
            "advance-late-stage final-review",
        )?;
        let branch_closure_id = current_branch_closure.branch_closure_id.clone();
        ensure_final_review_dispatch_id_matches(&context, dispatch_id)?;
        let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
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
                        dispatch_id,
                        reviewer_source,
                        reviewer_id,
                        result,
                        normalized_summary_hash: &normalized_summary_hash,
                    },
                )? {
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
                            .then(|| negative_result_follow_up(&operator))
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
                dispatch_id,
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
                dispatch_id,
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
        return Ok(AdvanceLateStageOutput {
            action: String::from("recorded"),
            stage_path: String::from("final_review"),
            delegated_primitive: String::from("record-final-review"),
            branch_closure_id: Some(branch_closure_id),
            dispatch_id: Some(dispatch_id.clone()),
            result: result.to_owned(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            required_follow_up: (result == "fail")
                .then(|| negative_result_follow_up(&operator))
                .flatten(),
            trace_summary: String::from(
                "Validated final-review dispatch lineage and recorded final-review evidence from authoritative late-stage state.",
            ),
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
    let operator = match current_workflow_operator(runtime, &args.plan, false) {
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
                    trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
                },
            ));
        }
        Err(error) => return Err(error),
    };
    if operator.review_state_status == "clean"
        && operator.phase == "document_release_pending"
        && operator.phase_detail == "branch_closure_recording_required_for_release_readiness"
    {
        if args.result.is_some() || args.summary_file.is_some() || args.dispatch_id.is_some() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "branch_closure_argument_mismatch: branch-closure advance-late-stage does not accept --result, --summary-file, or --dispatch-id.",
            ));
        }
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
        return Ok(release_readiness_follow_up_or_requery_output(
            &operator,
            &args.plan,
            AdvanceLateStageOutputContext {
                stage_path: "release_readiness",
                delegated_primitive: "record-release-readiness",
                branch_closure_id: branch_closure_id.clone(),
                dispatch_id: None,
                result,
                trace_summary: "advance-late-stage failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
            },
        ));
    }
    let summary = read_nonempty_summary_file(summary_file, "summary")?;
    let normalized_summary_hash = summary_hash(&summary);
    let current_branch_closure = authoritative_current_branch_closure_binding(
        &context,
        "advance-late-stage release-readiness",
    )?;
    let branch_closure_id = current_branch_closure.branch_closure_id.clone();
    let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
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
        return Ok(AdvanceLateStageOutput {
            action: String::from("blocked"),
            stage_path: String::from("release_readiness"),
            delegated_primitive: String::from("record-release-readiness"),
            branch_closure_id: Some(args.branch_closure_id.clone()),
            dispatch_id: None,
            result: args.result.as_str().to_owned(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            required_follow_up: Some(String::from("record_branch_closure")),
            trace_summary: String::from(
                "record-release-readiness failed closed because no authoritative current branch closure is available.",
            ),
        });
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
    let provided_summary_hash = optional_summary_hash(&args.summary_file);
    let operator = current_workflow_operator(runtime, &args.plan, false)?;
    let required_follow_up = blocked_follow_up_for_operator(&operator);
    let qa_refresh_reroute_active =
        shared_finish_requires_test_plan_refresh(Some(&gate_finish_from_context(&context)))
            || (operator.phase == "qa_pending"
                && operator.phase_detail == "test_plan_refresh_required");
    if qa_refresh_reroute_active {
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from("out_of_phase_requery_required")),
            recommended_command: Some(recommended_operator_command(&args.plan)),
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
            recommended_command: Some(recommended_operator_command(&args.plan)),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "record-qa failed closed because workflow/operator must be requeried before QA recording can proceed.",
            ),
        });
    }
    if required_follow_up.as_deref() == Some("repair_review_state") {
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            required_follow_up,
            trace_summary: String::from(
                "record-qa failed closed because workflow/operator requires review-state repair before QA recording can proceed.",
            ),
        });
    }
    if operator.review_state_status != "clean" {
        if required_follow_up.as_deref() != Some("repair_review_state") {
            return Ok(RecordQaOutput {
                action: String::from("blocked"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                code: Some(String::from("out_of_phase_requery_required")),
                recommended_command: Some(recommended_operator_command(&args.plan)),
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
    if operator.phase != "qa_pending" || operator.phase_detail != "qa_recording_required" {
        if equivalent_current_browser_qa_rerun_allowed(&operator, args.result.as_str())
            && let Some(current_branch_closure) = current_branch_closure.as_ref()
            && let Some(output) = equivalent_current_browser_qa_rerun(
                &context,
                current_branch_closure,
                args.result.as_str(),
                &args.summary_file,
                (args.result == ReviewOutcomeArg::Fail)
                    .then(|| negative_result_follow_up(&operator))
                    .flatten(),
            )?
        {
            return Ok(output);
        }
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from("out_of_phase_requery_required")),
            recommended_command: Some(recommended_operator_command(&args.plan)),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "record-qa failed closed because the current phase is out of band for QA recording; reroute through workflow/operator.",
            ),
        });
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
    let test_plan_path = match current_test_plan_artifact_path(&context) {
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
                recommended_command: Some(recommended_operator_command(&args.plan)),
                rederive_via_workflow_operator: Some(true),
                required_follow_up: None,
                trace_summary: String::from(
                    "record-qa failed closed because workflow/operator must refresh the current test plan before QA recording can proceed.",
                ),
            });
        }
        Err(error) => return Err(error),
    };
    let summary = read_nonempty_summary_file(&args.summary_file, "summary")?;
    let summary_hash = qa_summary_hash(&summary);
    let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
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
        Some((
            authoritative_test_plan_path,
            authoritative_test_plan_source,
            authoritative_test_plan_fingerprint,
        ))
    } else {
        None
    };
    let source_test_plan_fingerprint = authoritative_test_plan_write
        .as_ref()
        .map(|(_, _, fingerprint)| fingerprint.clone());
    let authoritative_qa_source = if let Some((authoritative_test_plan_path, _, _)) =
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
    let final_review_record_id = authoritative_state
        .current_final_review_record_id()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "record-qa requires a current final-review record id.",
            )
        })?;
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
            result: args.result.as_str(),
            browser_qa_fingerprint: Some(qa_fingerprint.as_str()),
            source_test_plan_fingerprint: source_test_plan_fingerprint.as_deref(),
            summary: &summary,
            summary_hash: &summary_hash,
            generated_by_identity: "featureforge/qa",
        },
    )?;
    if let Some((authoritative_test_plan_path, authoritative_test_plan_source, _)) =
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
    Ok(RecordQaOutput {
        action: String::from("recorded"),
        branch_closure_id,
        result: args.result.as_str().to_owned(),
        code: None,
        recommended_command: None,
        rederive_via_workflow_operator: None,
        required_follow_up: (args.result == ReviewOutcomeArg::Fail)
            .then(|| negative_result_follow_up(&operator))
            .flatten(),
        trace_summary: String::from(
            "Recorded browser QA evidence for the current branch closure and approved test plan.",
        ),
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
    let context = load_execution_context_for_exact_plan(runtime, &request.plan)?;
    if !context.evidence_abs.is_file() {
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
    let context = load_execution_context_for_exact_plan(runtime, plan)?;
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
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "close-current-task requires authoritative review-dispatch lineage state.",
        )
    })?;
    let lineage_key = format!("task-{task}");
    let expected_dispatch_from_lineage = overlay
        .strategy_review_dispatch_lineage
        .get(&lineage_key)
        .and_then(|record| record.dispatch_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(expected_dispatch) = expected_dispatch_from_lineage {
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
        format!("close-current-task requires a current task review dispatch lineage for task {task}."),
    ))
}

fn task_dispatch_reviewed_state_status(
    context: &ExecutionContext,
    task: u32,
    reviewed_state_id: &str,
) -> Result<TaskDispatchReviewedStateStatus, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "close-current-task requires authoritative review-dispatch lineage state.",
        )
    })?;
    let lineage_key = format!("task-{task}");
    let recorded_reviewed_state_id = overlay
        .strategy_review_dispatch_lineage
        .get(&lineage_key)
        .and_then(|record| record.reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(match recorded_reviewed_state_id {
        Some(recorded) if recorded == reviewed_state_id.trim() => {
            TaskDispatchReviewedStateStatus::Current
        }
        Some(_) => TaskDispatchReviewedStateStatus::StaleReviewedState,
        None => TaskDispatchReviewedStateStatus::MissingReviewedStateBinding,
    })
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
}

fn current_authoritative_branch_closure_binding_optional(
    context: &ExecutionContext,
) -> Result<Option<CurrentBranchClosureBinding>, JsonFailure> {
    Ok(
        usable_current_branch_closure_identity(context).map(|current_identity| {
            CurrentBranchClosureBinding {
                branch_closure_id: current_identity.branch_closure_id,
                reviewed_state_id: current_identity.reviewed_state_id,
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
    let gate_review = gate_review_from_context(context);
    let gate_finish = gate_finish_from_context(context);
    if rerun_invalidated_by_repo_writes(&gate_review, &gate_finish) {
        return Ok(None);
    }
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
    gate_review: &crate::execution::state::GateResult,
    gate_finish: &crate::execution::state::GateResult,
) -> bool {
    const REPO_WRITE_INVALIDATION_CODES: &[&str] = &[
        "review_artifact_worktree_dirty",
        REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
    ];
    let gate_has_reason = |gate: &crate::execution::state::GateResult| {
        gate.reason_codes.iter().any(|code| {
            REPO_WRITE_INVALIDATION_CODES
                .iter()
                .any(|expected| code == expected)
        })
    };
    gate_has_reason(gate_review) || gate_has_reason(gate_finish)
}

fn current_test_plan_artifact_path(context: &ExecutionContext) -> Result<PathBuf, JsonFailure> {
    current_test_plan_artifact_path_for_finish(context)
}

fn current_workflow_operator(
    runtime: &ExecutionRuntime,
    plan: &Path,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    query_workflow_routing_state_for_runtime(runtime, Some(plan), external_review_result_ready)
}

fn recommended_operator_command(plan: &Path) -> String {
    format!("featureforge workflow operator --plan {}", plan.display())
}

fn shared_out_of_phase_close_current_task_output(
    plan: &Path,
    task_number: u32,
    trace_summary: &str,
) -> CloseCurrentTaskOutput {
    CloseCurrentTaskOutput {
        action: String::from("blocked"),
        task_number,
        dispatch_validation_action: String::from("blocked"),
        closure_action: String::from("blocked"),
        task_closure_status: String::from("not_current"),
        superseded_task_closure_ids: Vec::new(),
        closure_record_id: None,
        code: Some(String::from("out_of_phase_requery_required")),
        recommended_command: Some(recommended_operator_command(plan)),
        rederive_via_workflow_operator: Some(true),
        required_follow_up: None,
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
        recommended_command: Some(recommended_operator_command(plan)),
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
        recommended_command: Some(recommended_operator_command(plan)),
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
    source_task_closure_ids: Vec<String>,
}

struct TaskClosureReceiptRefresh<'a> {
    execution_run_id: &'a str,
    strategy_checkpoint_fingerprint: &'a str,
    active_contract_fingerprint: Option<&'a str>,
    task: u32,
    claim_write_authority: bool,
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
    Ok(BranchReviewedState {
        base_branch: base_branch.clone(),
        contract_identity: shared_branch_contract_identity(
            &context.plan_rel,
            context.plan_document.plan_revision,
            &context.runtime.repo_slug,
            &context.runtime.branch_name,
            &base_branch,
        ),
        effective_reviewed_branch_surface: String::from("repo_tracked_content"),
        provenance_basis: String::from("task_closure_lineage"),
        reviewed_state_id,
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
            &reviewed_state.reviewed_state_id,
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
) -> bool {
    record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == reviewed_state.base_branch
        && record.reviewed_state_id == reviewed_state.reviewed_state_id
        && record.contract_identity == reviewed_state.contract_identity
        && record.source_task_closure_ids == reviewed_state.source_task_closure_ids
        && record.provenance_basis == reviewed_state.provenance_basis
        && record._effective_reviewed_branch_surface
            == reviewed_state.effective_reviewed_branch_surface
}

fn branch_closure_record_matches_empty_lineage_late_stage_exemption(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> bool {
    record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == reviewed_state.base_branch
        && record.reviewed_state_id == reviewed_state.reviewed_state_id
        && record.contract_identity == reviewed_state.contract_identity
        && record.source_task_closure_ids.is_empty()
        && record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption"
        && branch_closure_record_matches_plan_exemption(context, record)
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
    current_record: &CurrentTaskClosureRecord,
) -> bool {
    shared_task_closure_contributes_to_branch_surface(current_record)
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

fn repair_review_state_record_branch_closure_reroute_active(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    args: &RecordBranchClosureArgs,
) -> Result<bool, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(false);
    };
    let snapshot = query_review_state(
        runtime,
        &StatusArgs {
            plan: args.plan.clone(),
            external_review_result_ready: false,
        },
    )?;
    let status = status_with_shared_routing_or_context(runtime, &args.plan, context)?;
    let task_scope_overlay_restore_required = shared_task_scope_overlay_restore_required(
        &snapshot.missing_derived_overlays,
        Some(authoritative_state),
    );
    let gate_review = gate_review_from_context(context);
    let gate_finish = gate_finish_from_context(context);
    let branch_reroute_still_valid =
        branch_closure_rerecording_assessment(context).map(|assessment| assessment.supported)?;
    let live_stale_unreviewed = shared_public_review_state_stale_unreviewed_for_reroute(
        context,
        Some(authoritative_state),
        &status,
        Some(&gate_review),
        Some(&gate_finish),
    )
    .unwrap_or_else(|_| {
        shared_public_late_stage_stale_unreviewed(&status, Some(&gate_review), Some(&gate_finish))
            || shared_current_branch_closure_has_tracked_drift(context, Some(authoritative_state))
                .unwrap_or(false)
    });
    let live_review_state_status =
        live_review_state_status_for_reroute_from_status(&status, live_stale_unreviewed);
    let task_scope_repair_precedence_active = shared_live_task_scope_repair_precedence_active(
        task_scope_overlay_restore_required,
        task_scope_structural_review_state_reason(&status).is_some(),
        shared_task_scope_stale_review_state_reason_present(task_scope_review_state_repair_reason(
            &status,
        )),
        authoritative_state.review_state_repair_follow_up(),
        branch_reroute_still_valid,
        live_review_state_status,
    );
    Ok(shared_live_review_state_repair_reroute(
        authoritative_state.review_state_repair_follow_up(),
        task_scope_repair_precedence_active,
        branch_reroute_still_valid,
        live_review_state_status,
        shared_branch_closure_refresh_missing_current_closure(&status),
    ) == crate::execution::current_truth::ReviewStateRepairReroute::RecordBranchClosure)
}

fn pre_release_branch_closure_already_current_allowed(
    context: &ExecutionContext,
    operator: &ExecutionRoutingState,
    reviewed_state: &BranchReviewedState,
) -> Result<bool, JsonFailure> {
    if operator.review_state_status != "clean"
        || operator.phase != "document_release_pending"
        || !matches!(
            operator.phase_detail.as_str(),
            "release_readiness_recording_ready" | "release_blocker_resolution_required"
        )
    {
        return Ok(false);
    }
    Ok(
        shared_current_branch_closure_baseline_tree_sha(context).is_some()
            && usable_current_branch_closure_identity(context).is_some_and(|identity| {
                identity.contract_identity == reviewed_state.contract_identity
            }),
    )
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
        .filter(task_closure_contributes_to_branch_surface)
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
    Ok(format!("git_tree:{}", context.current_tracked_tree_sha()?))
}

fn current_task_contract_identity(context: &ExecutionContext, task_number: u32) -> String {
    deterministic_record_id(
        "task-contract",
        &[
            &context.plan_rel,
            &context.plan_document.plan_revision.to_string(),
            &task_number.to_string(),
        ],
    )
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
                .filter(|path| path != NO_REPO_FILES_MARKER)
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
            if path != NO_REPO_FILES_MARKER {
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
    let right_paths = right.iter().collect::<BTreeSet<_>>();
    left.iter()
        .filter(|path| path.as_str() != NO_REPO_FILES_MARKER)
        .any(|path| right_paths.contains(path))
}

fn superseded_task_closure_records(
    authoritative_state: &AuthoritativeTransitionState,
    task_number: u32,
    closure_record_id: &str,
    effective_reviewed_surface_paths: &[String],
) -> Vec<SupersededTaskClosureRecord> {
    authoritative_state
        .current_task_closure_results()
        .into_values()
        .filter(|record| record.closure_record_id != closure_record_id)
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
    if !final_review_dispatch_lineage_is_current_for_rerun(context, check.dispatch_id)? {
        return Ok(false);
    }
    Ok(true)
}

fn final_review_dispatch_lineage_is_current_for_rerun(
    context: &ExecutionContext,
    expected_dispatch_id: &str,
) -> Result<bool, JsonFailure> {
    let gate_review = gate_review_from_context(context);
    let gate_finish = gate_finish_from_context(context);
    if !shared_final_review_dispatch_still_current(Some(&gate_review), Some(&gate_finish)) {
        return Ok(false);
    }
    match ensure_final_review_dispatch_id_matches(context, expected_dispatch_id) {
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
    }
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

fn refresh_rebuild_task_closure_receipts_with_context(
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
        let current_task_closure = authoritative_state.current_task_closure_result(refresh.task);
        authoritative_state.refresh_task_review_dispatch_lineage(context, refresh.task)?;
        let dispatch_id = current_task_closure
            .as_ref()
            .map(|record| record.dispatch_id.clone())
            .or_else(|| authoritative_state.task_review_dispatch_id(refresh.task));
        if let (Some(dispatch_id), Some(current_task_closure)) = (dispatch_id, current_task_closure)
        {
            let closure_record_id = current_task_closure_record_id(context, refresh.task)?;
            let reviewed_state_id = current_task_reviewed_state_id(context, refresh.task)?;
            let contract_identity = current_task_contract_identity(context, refresh.task);
            let effective_reviewed_surface_paths =
                current_task_effective_reviewed_surface_paths(context, refresh.task)?;
            let review_result = current_task_closure.review_result;
            let review_summary_hash = current_task_closure.review_summary_hash;
            let verification_result = current_task_closure.verification_result;
            let verification_summary_hash = current_task_closure.verification_summary_hash;
            record_current_task_closure(
                authoritative_state,
                CurrentTaskClosureWrite {
                    task: refresh.task,
                    dispatch_id: &dispatch_id,
                    closure_record_id: &closure_record_id,
                    reviewed_state_id: &reviewed_state_id,
                    contract_identity: &contract_identity,
                    effective_reviewed_surface_paths: &effective_reviewed_surface_paths,
                    review_result: &review_result,
                    review_summary_hash: &review_summary_hash,
                    verification_result: &verification_result,
                    verification_summary_hash: &verification_summary_hash,
                    superseded_tasks: &[],
                    superseded_task_closure_ids: &[],
                },
            )?;
        } else {
            authoritative_state.persist_if_dirty_with_failpoint(None)?;
        }
    }
    Ok(())
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

fn render_plan_source(
    original_source: &str,
    execution_mode: &str,
    steps: &[PlanStepState],
) -> String {
    let step_map = steps
        .iter()
        .map(|step| ((step.task_number, step.step_number), step))
        .collect::<BTreeMap<_, _>>();
    let lines = original_source.lines().collect::<Vec<_>>();
    let mut rendered = Vec::new();
    let mut current_task = None::<u32>;
    let mut suppress_note = false;

    for line in lines {
        if suppress_note {
            if line.is_empty() || line.trim_start().starts_with("**Execution Note:**") {
                continue;
            }
            suppress_note = false;
        }

        if line.starts_with("**Execution Mode:** ") {
            rendered.push(format!("**Execution Mode:** {execution_mode}"));
            continue;
        }

        if let Some(rest) = line.strip_prefix("## Task ") {
            current_task = rest
                .split(':')
                .next()
                .and_then(|value| value.parse::<u32>().ok());
            rendered.push(line.to_owned());
            continue;
        }

        if let Some((_, step_number, _)) = crate::execution::state::parse_step_line(line)
            && let Some(task_number) = current_task
            && let Some(step) = step_map.get(&(task_number, step_number))
        {
            let mark = if step.checked { 'x' } else { ' ' };
            rendered.push(format!(
                "- [{mark}] **Step {}: {}**",
                step.step_number, step.title
            ));
            if let Some(note_state) = step.note_state {
                rendered.push(String::new());
                rendered.push(format!(
                    "  **Execution Note:** {} - {}",
                    note_state.as_str(),
                    step.note_summary
                ));
            }
            suppress_note = true;
            continue;
        }

        rendered.push(line.to_owned());
    }

    format!("{}\n", rendered.join("\n"))
}

fn render_evidence_source(
    context: &ExecutionContext,
    plan_fingerprint: &str,
    source_spec_fingerprint: &str,
) -> String {
    let mut output = Vec::new();
    let topic = Path::new(&context.plan_rel)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("plan");
    output.push(format!("# Execution Evidence: {topic}"));
    output.push(String::new());
    output.push(format!("**Plan Path:** {}", context.plan_rel));
    output.push(format!(
        "**Plan Revision:** {}",
        context.plan_document.plan_revision
    ));
    output.push(format!("**Plan Fingerprint:** {plan_fingerprint}"));
    output.push(format!(
        "**Source Spec Path:** {}",
        context.plan_document.source_spec_path
    ));
    output.push(format!(
        "**Source Spec Revision:** {}",
        context.plan_document.source_spec_revision
    ));
    output.push(format!(
        "**Source Spec Fingerprint:** {source_spec_fingerprint}"
    ));
    output.push(String::new());
    output.push(String::from("## Step Evidence"));

    for step in &context.steps {
        let attempts = context
            .evidence
            .attempts
            .iter()
            .filter(|attempt| {
                attempt.task_number == step.task_number && attempt.step_number == step.step_number
            })
            .collect::<Vec<_>>();
        if attempts.is_empty() {
            continue;
        }
        output.push(String::new());
        output.push(format!(
            "### Task {} Step {}",
            step.task_number, step.step_number
        ));
        for (index, attempt) in attempts.iter().enumerate() {
            if index > 0 {
                output.push(String::new());
            }
            output.push(format!("#### Attempt {}", attempt.attempt_number));
            output.push(format!("**Status:** {}", attempt.status));
            output.push(format!("**Recorded At:** {}", attempt.recorded_at));
            output.push(format!(
                "**Execution Source:** {}",
                attempt.execution_source
            ));
            output.push(format!("**Task Number:** {}", attempt.task_number));
            output.push(format!("**Step Number:** {}", attempt.step_number));
            output.push(format!(
                "**Packet Fingerprint:** {}",
                attempt
                    .packet_fingerprint
                    .clone()
                    .unwrap_or_else(|| String::from("unknown"))
            ));
            output.push(format!(
                "**Head SHA:** {}",
                attempt
                    .head_sha
                    .clone()
                    .unwrap_or_else(|| String::from("unknown"))
            ));
            if let Some(base_sha) = &attempt.base_sha {
                output.push(format!("**Base SHA:** {base_sha}"));
            }
            output.push(format!("**Claim:** {}", attempt.claim));
            if let Some(source_contract_path) = &attempt.source_contract_path {
                output.push(format!("**Source Contract Path:** {source_contract_path}"));
            }
            if let Some(source_contract_fingerprint) = &attempt.source_contract_fingerprint {
                output.push(format!(
                    "**Source Contract Fingerprint:** `{source_contract_fingerprint}`"
                ));
            }
            if let Some(source_evaluation_report_fingerprint) =
                &attempt.source_evaluation_report_fingerprint
            {
                output.push(format!(
                    "**Source Evaluation Report Fingerprint:** `{source_evaluation_report_fingerprint}`"
                ));
            }
            if let Some(evaluator_verdict) = &attempt.evaluator_verdict {
                output.push(format!("**Evaluator Verdict:** {evaluator_verdict}"));
            }
            if !attempt.failing_criterion_ids.is_empty() {
                output.push(String::from("**Failing Criterion IDs:**"));
                for criterion_id in &attempt.failing_criterion_ids {
                    output.push(format!("- `{criterion_id}`"));
                }
            }
            if let Some(source_handoff_fingerprint) = &attempt.source_handoff_fingerprint {
                output.push(format!(
                    "**Source Handoff Fingerprint:** `{source_handoff_fingerprint}`"
                ));
            }
            if let Some(repo_state_baseline_head_sha) = &attempt.repo_state_baseline_head_sha {
                output.push(format!(
                    "**Repo State Baseline Head SHA:** {repo_state_baseline_head_sha}"
                ));
            }
            if let Some(repo_state_baseline_worktree_fingerprint) =
                &attempt.repo_state_baseline_worktree_fingerprint
            {
                output.push(format!(
                    "**Repo State Baseline Worktree Fingerprint:** {repo_state_baseline_worktree_fingerprint}"
                ));
            }
            output.push(String::from("**Files Proven:**"));
            for proof in &attempt.file_proofs {
                output.push(format!("- {} | {}", proof.path, proof.proof));
            }
            if let Some(verify_command) = &attempt.verify_command {
                output.push(format!("**Verify Command:** {verify_command}"));
            }
            output.push(format!(
                "**Verification Summary:** {}",
                attempt.verification_summary
            ));
            output.push(format!(
                "**Invalidation Reason:** {}",
                attempt.invalidation_reason
            ));
        }
    }

    format!("{}\n", output.join("\n"))
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

fn write_plan_and_evidence_with_rollback(
    plan_path: &Path,
    original_plan: &str,
    rendered_plan: &str,
    evidence_path: &Path,
    original_evidence: Option<&str>,
    rendered_evidence: &str,
    failpoint: &str,
) -> Result<(), JsonFailure> {
    write_atomic(plan_path, rendered_plan)?;
    if let Err(error) = maybe_trigger_failpoint(failpoint) {
        restore_plan_and_evidence(plan_path, original_plan, evidence_path, original_evidence);
        return Err(error);
    }
    if let Err(error) = write_atomic(evidence_path, rendered_evidence) {
        restore_plan_and_evidence(plan_path, original_plan, evidence_path, original_evidence);
        return Err(error);
    }
    Ok(())
}

fn persist_authoritative_state_with_rollback(
    authoritative_state: &AuthoritativeTransitionState,
    plan_path: &Path,
    original_plan: &str,
    evidence_path: &Path,
    original_evidence: Option<&str>,
    failpoint: &str,
) -> Result<(), JsonFailure> {
    if let Err(error) = authoritative_state.persist_if_dirty_with_failpoint(Some(failpoint)) {
        restore_plan_and_evidence(plan_path, original_plan, evidence_path, original_evidence);
        return Err(error);
    }
    Ok(())
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
        CloseCurrentTaskOutcomeClass, CurrentFinalReviewAuthorityCheck, FinalReviewProjectionInput,
        blocked_follow_up_for_operator, close_current_task_outcome_class,
        close_current_task_required_follow_up, current_final_review_record_is_still_authoritative,
        late_stage_required_follow_up, normalized_late_stage_surface,
        path_matches_late_stage_surface, render_final_review_artifacts,
        rewrite_branch_final_review_artifacts, rewrite_branch_head_bound_artifact,
        rewrite_branch_qa_artifact, superseded_branch_closure_ids_from_previous_current,
        task_closure_contributes_to_branch_surface, task_closure_record_covers_path,
        verify_command_launcher, AdvanceLateStageOutputContext,
        advance_late_stage_follow_up_or_requery_output,
    };
    use crate::cli::plan_execution::{ReviewOutcomeArg, VerificationOutcomeArg};
    use crate::contracts::plan::parse_plan_file;
    use crate::diagnostics::FailureClass;
    use crate::execution::final_review::resolve_release_base_branch;
    use crate::execution::leases::StatusAuthoritativeOverlay;
    use crate::execution::leases::authoritative_state_path;
    use crate::execution::query::ExecutionRoutingState;
    use crate::execution::state::{
        EvidenceFormat, ExecutionContext, ExecutionEvidence, ExecutionRuntime, NO_REPO_FILES_MARKER,
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
        let no_repo_only = CurrentTaskClosureRecord {
            task: 1,
            source_plan_path: Some(String::from("docs/featureforge/plans/example.md")),
            source_plan_revision: Some(1),
            execution_run_id: Some(String::from("run-1")),
            dispatch_id: String::from("dispatch-1"),
            closure_record_id: String::from("task-1-closure"),
            reviewed_state_id: String::from("git_tree:abc123"),
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
            !task_closure_contributes_to_branch_surface(&no_repo_only),
            "no-repo-only task closures must not influence branch-surface baseline derivation"
        );
        assert!(
            task_closure_contributes_to_branch_surface(&mixed_surface),
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

        let plan_rel =
            "docs/archive/featureforge/plans/2026-03-29-featureforge-project-memory-integration.md";
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
            },
            source_spec_source: String::new(),
            source_spec_path: repo_root
                .join("docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md"),
            execution_fingerprint: String::from("unit-test-execution-fingerprint"),
            tracked_tree_sha_cache: OnceLock::new(),
            reviewed_tree_sha_cache: std::cell::RefCell::new(BTreeMap::new()),
            head_sha_cache: OnceLock::new(),
            release_base_branch_cache: OnceLock::new(),
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
        let branch_contract_identity = super::shared_branch_contract_identity(
            &context.plan_rel,
            context.plan_document.plan_revision,
            &runtime.repo_slug,
            &runtime.branch_name,
            &base_branch,
        );
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
                        "contract_identity": super::current_task_contract_identity(&context, 1),
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
                        "contract_identity": super::current_task_contract_identity(&context, 1),
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
    fn blocked_follow_up_prefers_branch_closure_when_repair_routes_there() {
        let operator = ExecutionRoutingState {
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
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
            follow_up_override: String::from("none"),
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
        };

        assert_eq!(
            blocked_follow_up_for_operator(&operator),
            Some(String::from("advance_late_stage"))
        );
        assert_eq!(
            late_stage_required_follow_up("final_review", &operator),
            None
        );
    }

    #[test]
    fn advance_late_stage_final_review_with_dispatch_id_requeries_when_dispatch_follow_up_is_required()
     {
        let operator = ExecutionRoutingState {
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
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
            follow_up_override: String::from("none"),
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
                trace_summary: "advance-late-stage failed closed because workflow/operator requery is required.",
            },
        );

        assert_eq!(output.action, "blocked");
        assert_eq!(output.code.as_deref(), Some("out_of_phase_requery_required"));
        assert_eq!(
            output.recommended_command.as_deref(),
            Some("featureforge workflow operator --plan docs/featureforge/plans/example.md")
        );
        assert_eq!(output.rederive_via_workflow_operator, Some(true));
        assert_eq!(output.required_follow_up, None);
    }

    #[test]
    fn advance_late_stage_final_review_with_matching_dispatch_lineage_keeps_dispatch_follow_up() {
        let operator = ExecutionRoutingState {
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
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
            follow_up_override: String::from("none"),
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
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
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
            follow_up_override: String::from("none"),
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
    fn close_current_task_follow_up_routes_structural_repair_state_to_repair_review_state() {
        let operator = ExecutionRoutingState {
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
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
            follow_up_override: String::from("none"),
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
        };

        assert_eq!(
            close_current_task_required_follow_up(&operator),
            Some(String::from("repair_review_state"))
        );
    }

    #[test]
    fn close_current_task_outcome_class_treats_review_fail_verification_pass_as_negative() {
        assert_eq!(
            close_current_task_outcome_class(ReviewOutcomeArg::Fail, VerificationOutcomeArg::Pass),
            CloseCurrentTaskOutcomeClass::Negative
        );
    }
}
