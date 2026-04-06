use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

use jiff::Timestamp;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::cli::plan_execution::{
    AdvanceLateStageArgs, BeginArgs, CloseCurrentTaskArgs, CompleteArgs, ExecutionModeArg,
    NoteArgs, RecordBranchClosureArgs, RecordQaArgs, RebuildEvidenceArgs, ReopenArgs,
    ReviewOutcomeArg, TransferArgs, VerificationOutcomeArg,
};
use crate::cli::workflow::OperatorArgs as WorkflowOperatorArgs;
use crate::contracts::headers::parse_required_header as parse_plan_header;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::authority::{
    ensure_preflight_authoritative_bootstrap, write_authoritative_unit_review_receipt_artifact,
};
use crate::execution::final_review::{
    FinalReviewReceiptExpectations, authoritative_strategy_checkpoint_fingerprint_checked,
    latest_branch_artifact_path, parse_artifact_document, parse_final_review_receipt,
    resolve_release_base_branch, validate_final_review_receipt,
};
use crate::execution::harness::RunIdentitySnapshot;
use crate::execution::leases::{StatusAuthoritativeOverlay, load_status_authoritative_overlay_checked};
use crate::execution::state::{
    EvidenceAttempt, ExecutionContext, ExecutionEvidence, ExecutionRuntime, FileProof,
    NO_REPO_FILES_MARKER, PacketFingerprintInput, PlanExecutionStatus, PlanStepState,
    RebuildEvidenceCounts, RebuildEvidenceFilter, RebuildEvidenceOutput,
    RebuildEvidenceTarget, RebuildEvidenceCandidate, compute_packet_fingerprint,
    current_file_proof, current_head_sha, discover_rebuild_candidates, hash_contract_plan,
    load_execution_context, load_execution_context_for_mutation, normalize_begin_request,
    normalize_complete_request, normalize_note_request, normalize_rebuild_evidence_request,
    normalize_reopen_request, normalize_source, normalize_transfer_request,
    require_normalized_text, require_preflight_acceptance, require_prior_task_closure_for_begin,
    task_completion_lineage_fingerprint,
    status_from_context, validate_expected_fingerprint,
};
use crate::execution::topology::persist_preflight_acceptance;
use crate::execution::transitions::{
    AuthoritativeTransitionState, FinalReviewResultRecord, StepCommand,
    TaskClosureNegativeResultRecord, TaskClosureResultRecord, claim_step_write_authority,
    enforce_active_contract_scope, enforce_authoritative_phase, load_authoritative_transition_state,
};
use crate::paths::{
    harness_authoritative_artifact_path, normalize_repo_relative_path, normalize_whitespace,
    write_atomic as write_atomic_file,
};
use crate::workflow::operator::{WorkflowOperator, operator as workflow_operator};

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
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordBranchClosureOutput {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
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
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordQaOutput {
    pub action: String,
    pub branch_closure_id: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

struct FinalReviewArtifactInputs<'a> {
    dispatch_id: &'a str,
    reviewer_source: &'a str,
    reviewer_id: &'a str,
    result: &'a str,
    summary: &'a str,
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
            return status_from_context(&context);
        }
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "A different step is already active.",
        ));
    }
    if context
        .steps
        .iter()
        .any(|step| step.note_state == Some(crate::execution::state::NoteState::Blocked))
    {
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
    status_from_context(&reloaded)
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
    let authoritative_state = load_authoritative_transition_state(&context)?;
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
    let head_sha = current_head_sha(&context.runtime.repo_root)?;
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

    write_plan_and_evidence_with_rollback(
        &context.plan_abs,
        &context.plan_source,
        &rendered_plan,
        &context.evidence_abs,
        context.evidence.source.as_deref(),
        &rendered_evidence,
        "complete_after_plan_write",
    )?;
    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_from_context(&reloaded)
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
    status_from_context(&reloaded)
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
    status_from_context(&reloaded)
}

pub fn transfer(
    runtime: &ExecutionRuntime,
    args: &TransferArgs,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let request = normalize_transfer_request(args)?;
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, &args.plan)?;
    validate_expected_fingerprint(&context, &request.expect_execution_fingerprint)?;
    normalize_source(&request.source, &context.plan_document.execution_mode)?;
    let authoritative_state = load_authoritative_transition_state(&context)?;
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Transfer)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Transfer,
        request.repair_task,
        request.repair_step,
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

    let repair_index =
        step_index(&context, request.repair_task, request.repair_step).ok_or_else(|| {
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

    invalidate_latest_completed_attempt(
        &mut context,
        request.repair_task,
        request.repair_step,
        &request.reason,
    )?;
    context.steps[repair_index].checked = false;
    context.steps[repair_index].note_state = None;
    context.steps[repair_index].note_summary.clear();
    context.steps[active_index].note_state = Some(crate::execution::state::NoteState::Interrupted);
    context.steps[active_index].note_summary = truncate_summary(&format!(
        "Parked for repair of Task {} Step {}",
        request.repair_task, request.repair_step
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

    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_from_context(&reloaded)
}

pub fn close_current_task(
    runtime: &ExecutionRuntime,
    args: &CloseCurrentTaskArgs,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    let context = load_execution_context(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let status = status_from_context(&context)?;
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
    let review_summary = read_nonempty_summary_file(&args.review_summary_file, "review summary")?;
    let review_summary_hash = summary_hash(&review_summary);
    let verification_result = args.verification_result.as_str();
    let verification_summary = if matches!(
        args.verification_result,
        VerificationOutcomeArg::Pass | VerificationOutcomeArg::Fail
    ) {
        Some(read_nonempty_summary_file(
            args.verification_summary_file.as_ref().ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "verification_summary_required: close-current-task requires --verification-summary-file when --verification-result=pass|fail.",
                )
            })?,
            "verification summary",
        )?)
    } else {
        None
    };
    let verification_summary_hash = verification_summary
        .as_deref()
        .map(summary_hash)
        .unwrap_or_default();
    let reviewed_state_id = current_task_reviewed_state_id(&context, args.task)?;
    let contract_identity = current_task_contract_identity(&context, args.task);
    let closure_record_id = current_task_closure_record_id(&context, args.task)?;
    let current_task_recording_ready = |operator: &WorkflowOperator| {
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
                required_follow_up: negative_result_follow_up(&operator),
                trace_summary: String::from(
                    "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                ),
            });
        }
    }
    let operator = current_workflow_operator(runtime, &args.plan, true)?;
    if !current_task_recording_ready(&operator) {
        return Ok(CloseCurrentTaskOutput {
            action: String::from("blocked"),
            task_number: args.task,
            dispatch_validation_action: String::from("blocked"),
            closure_action: String::from("blocked"),
            task_closure_status: String::from("not_current"),
            superseded_task_closure_ids: Vec::new(),
            closure_record_id: None,
            required_follow_up: blocked_follow_up_for_operator(&operator),
            trace_summary: String::from(
                "close-current-task failed closed because workflow/operator did not expose task_closure_recording_ready for the supplied dispatch lineage.",
            ),
        });
    }
    match (args.review_result, args.verification_result) {
        (ReviewOutcomeArg::Pass, VerificationOutcomeArg::Pass) => {
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
                    restore_missing_dispatch_lineage: false,
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
            let locked_context = load_execution_context(runtime, &args.plan)?;
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
                return Ok(CloseCurrentTaskOutput {
                    action: String::from("blocked"),
                    task_number: args.task,
                    dispatch_validation_action: String::from("validated"),
                    closure_action: String::from("blocked"),
                    task_closure_status: String::from("not_current"),
                    superseded_task_closure_ids: Vec::new(),
                    closure_record_id: None,
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
            authoritative_state.remove_current_task_closure_results(
                superseded_task_closure_records
                    .iter()
                    .map(|record| record.task),
            )?;
            authoritative_state.append_superseded_task_closure_ids(
                superseded_task_closure_ids.iter().map(String::as_str),
            )?;
            authoritative_state.record_task_closure_result(TaskClosureResultRecord {
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
            })?;
            authoritative_state.persist_if_dirty_with_failpoint(None)?;
            Ok(CloseCurrentTaskOutput {
                action: String::from("recorded"),
                task_number: args.task,
                dispatch_validation_action: String::from("validated"),
                closure_action: String::from("recorded"),
                task_closure_status: String::from("current"),
                superseded_task_closure_ids,
                closure_record_id: Some(closure_record_id),
                required_follow_up: None,
                trace_summary: String::from(
                    "Validated task review dispatch lineage and refreshed authoritative task review and verification receipts.",
                ),
            })
        }
        (ReviewOutcomeArg::Fail, VerificationOutcomeArg::Fail | VerificationOutcomeArg::NotRun)
        | (ReviewOutcomeArg::Pass, VerificationOutcomeArg::Fail) => {
            let _write_authority = claim_step_write_authority(runtime)?;
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            let locked_context = load_execution_context(runtime, &args.plan)?;
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
                return Ok(CloseCurrentTaskOutput {
                    action: String::from("blocked"),
                    task_number: args.task,
                    dispatch_validation_action: String::from("validated"),
                    closure_action: String::from("blocked"),
                    task_closure_status: String::from("not_current"),
                    superseded_task_closure_ids: Vec::new(),
                    closure_record_id: None,
                    required_follow_up: negative_result_follow_up(&operator),
                    trace_summary: String::from(
                        "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                    ),
                });
            }
            authoritative_state.record_task_closure_negative_result(
                TaskClosureNegativeResultRecord {
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
            authoritative_state.persist_if_dirty_with_failpoint(None)?;
            Ok(CloseCurrentTaskOutput {
                action: String::from("blocked"),
                task_number: args.task,
                dispatch_validation_action: String::from("validated"),
                closure_action: String::from("blocked"),
                task_closure_status: String::from("not_current"),
                superseded_task_closure_ids: Vec::new(),
                closure_record_id: None,
                required_follow_up: negative_result_follow_up(&operator),
                trace_summary: String::from(
                    "Task closure remained blocked because the supplied review or verification outcome was not passing.",
                ),
            })
        }
        (ReviewOutcomeArg::Fail, VerificationOutcomeArg::Pass) => Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "verification_result_pass_incompatible_with_review_fail: verification may not be pass when the supplied review result is fail.",
        )),
        (ReviewOutcomeArg::Pass, VerificationOutcomeArg::NotRun) => Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "verification_not_run_incompatible_with_passing_review: a passing task closure requires passing verification in the first slice.",
        )),
    }
}

pub fn record_branch_closure(
    runtime: &ExecutionRuntime,
    args: &RecordBranchClosureArgs,
) -> Result<RecordBranchClosureOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let operator = current_workflow_operator(runtime, &args.plan, false)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let mut reviewed_state = current_branch_reviewed_state(&context)?;
    if let Some(current_branch_closure_id) = overlay
        .as_ref()
        .and_then(|overlay| overlay.current_branch_closure_id.as_ref())
        .filter(|value| !value.trim().is_empty())
    {
        let current_reviewed_state_id = overlay
            .as_ref()
            .and_then(|overlay| overlay.current_branch_closure_reviewed_state_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let current_contract_identity = overlay
            .as_ref()
            .and_then(|overlay| overlay.current_branch_closure_contract_identity.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if operator.phase == "document_release_pending"
            && matches!(
                operator.phase_detail.as_str(),
                "release_readiness_recording_ready" | "release_blocker_resolution_required"
            )
            && current_reviewed_state_id == Some(reviewed_state.reviewed_state_id.as_str())
            && current_contract_identity == Some(reviewed_state.contract_identity.as_str())
        {
            return Ok(RecordBranchClosureOutput {
                action: String::from("already_current"),
                branch_closure_id: Some(current_branch_closure_id.clone()),
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: None,
                trace_summary: String::from(
                    "Current reviewed branch state already has an authoritative current branch closure.",
                ),
            });
        }
    }
    if operator.phase != "document_release_pending"
        || !matches!(
            operator.phase_detail.as_str(),
            "branch_closure_recording_required_for_release_readiness"
                | "release_readiness_recording_ready"
                | "release_blocker_resolution_required"
        )
    {
        return Ok(RecordBranchClosureOutput {
            action: String::from("blocked"),
            branch_closure_id: None,
            superseded_branch_closure_ids: Vec::new(),
            required_follow_up: blocked_follow_up_for_operator(&operator),
            trace_summary: String::from(
                "record-branch-closure failed closed because workflow/operator did not expose branch_closure_recording_required_for_release_readiness.",
            ),
        });
    }
    let authoritative_baseline_tree_sha = if let Some(overlay) = overlay.as_ref()
        && overlay.current_branch_closure_id.as_deref().is_some()
    {
        current_branch_closure_baseline_tree_sha(overlay).map(str::to_owned)
    } else {
        current_task_closure_baseline_tree_sha(&context)?
    };
    if let Some(baseline_tree_sha) = authoritative_baseline_tree_sha
        && format!("git_tree:{baseline_tree_sha}") != reviewed_state.reviewed_state_id
    {
        let current_tree_sha = reviewed_state
            .reviewed_state_id
            .strip_prefix("git_tree:")
            .unwrap_or_default();
        let changed_paths = tracked_paths_changed_between(
            &context.runtime.repo_root,
            &baseline_tree_sha,
            current_tree_sha,
        )?;
        let late_stage_surface = normalized_late_stage_surface(&context.plan_source)?;
        if changed_paths.is_empty()
            || !changed_paths
                .iter()
                .all(|path| path_matches_late_stage_surface(path, &late_stage_surface))
        {
            return Ok(RecordBranchClosureOutput {
                action: String::from("blocked"),
                branch_closure_id: None,
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: Some(String::from("repair_review_state")),
                trace_summary: String::from(
                    "record-branch-closure failed closed because branch drift escaped the trusted Late-Stage Surface.",
                ),
            });
        }
        reviewed_state.provenance_basis =
            String::from("task_closure_lineage_plus_late_stage_surface_exemption");
    }

    let branch_closure_id = deterministic_record_id(
        "branch-closure",
        &[
            &context.plan_rel,
            &context.runtime.branch_name,
            &reviewed_state.reviewed_state_id,
            &reviewed_state.contract_identity,
        ],
    );
    let superseded_branch_closure_ids =
        superseded_branch_closure_ids_from_previous_current(overlay.as_ref(), &branch_closure_id);
    write_project_artifact(
        runtime,
        &format!("branch-closure-{}.md", &branch_closure_id),
        &render_branch_closure_artifact(
            &context,
            &branch_closure_id,
            &reviewed_state,
            &superseded_branch_closure_ids,
        )?,
    )?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires authoritative harness state.",
        ));
    };
    authoritative_state.append_superseded_branch_closure_ids(
        superseded_branch_closure_ids.iter().map(String::as_str),
    )?;
    authoritative_state
        .set_current_branch_closure_id(
            &branch_closure_id,
            &reviewed_state.reviewed_state_id,
            &reviewed_state.contract_identity,
        )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(RecordBranchClosureOutput {
        action: String::from("recorded"),
        branch_closure_id: Some(branch_closure_id),
        superseded_branch_closure_ids,
        required_follow_up: None,
        trace_summary: String::from(
            "Recorded a current branch closure for the still-current reviewed branch state.",
        ),
    })
}

pub fn advance_late_stage(
    runtime: &ExecutionRuntime,
    args: &AdvanceLateStageArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage requires authoritative harness state.",
        )
    })?;
    let branch_closure_id = overlay
        .current_branch_closure_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage requires a current branch closure.",
            )
        })?;
    let summary = read_nonempty_summary_file(&args.summary_file, "summary")?;
    let normalized_summary_hash = summary_hash(&summary);
    if let Some(dispatch_id) = args.dispatch_id.as_ref() {
        let reviewer_source = args
            .reviewer_source
            .as_deref()
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "reviewer_source_required: final-review advance-late-stage requires --reviewer-source.",
                )
            })?;
        let reviewer_id = args.reviewer_id.as_deref().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "reviewer_id_required: final-review advance-late-stage requires --reviewer-id.",
            )
        })?;
        let result = match args.result.trim() {
            "pass" => "pass",
            "fail" => "fail",
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "final_review_result_invalid: final-review advance-late-stage requires --result pass|fail.",
                ));
            }
        };
        let operator = current_workflow_operator(runtime, &args.plan, true)?;
        if operator.review_state_status != "clean" {
            return Ok(AdvanceLateStageOutput {
                action: String::from("blocked"),
                stage_path: String::from("final_review"),
                delegated_primitive: String::from("record-final-review"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: Some(dispatch_id.clone()),
                result: args.result.trim().to_owned(),
                required_follow_up: blocked_follow_up_for_operator(&operator),
                trace_summary: String::from(
                    "advance-late-stage failed closed because workflow/operator did not expose final_review_recording_ready for the supplied dispatch lineage.",
                ),
            });
        }
        {
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "advance-late-stage requires authoritative harness state.",
                ));
            };
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
                if current_reviewer_source == reviewer_source
                    && current_reviewer_id == reviewer_id
                    && current_result == result
                    && current_summary_hash == normalized_summary_hash
                {
                    return Ok(AdvanceLateStageOutput {
                        action: String::from("already_current"),
                        stage_path: String::from("final_review"),
                        delegated_primitive: String::from("record-final-review"),
                        branch_closure_id: Some(branch_closure_id),
                        dispatch_id: Some(dispatch_id.clone()),
                        result: result.to_owned(),
                        required_follow_up: None,
                        trace_summary: String::from(
                            "Current branch closure already has an equivalent recorded final-review outcome.",
                        ),
                    });
                }
                return Ok(AdvanceLateStageOutput {
                    action: String::from("blocked"),
                    stage_path: String::from("final_review"),
                    delegated_primitive: String::from("record-final-review"),
                    branch_closure_id: Some(branch_closure_id),
                    dispatch_id: Some(dispatch_id.clone()),
                    result: result.to_owned(),
                    required_follow_up: None,
                    trace_summary: String::from(
                        "advance-late-stage failed closed because the current branch closure already has a conflicting recorded final-review outcome for this dispatch lineage.",
                    ),
                });
            }
        }
        if operator.phase != "final_review_pending"
            || operator.phase_detail != "final_review_recording_ready"
            || operator
                .recording_context
                .as_ref()
                .and_then(|context| context.dispatch_id.as_deref())
                != Some(dispatch_id.as_str())
        {
            return Ok(AdvanceLateStageOutput {
                action: String::from("blocked"),
                stage_path: String::from("final_review"),
                delegated_primitive: String::from("record-final-review"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: Some(dispatch_id.clone()),
                result: args.result.trim().to_owned(),
                required_follow_up: blocked_follow_up_for_operator(&operator),
                trace_summary: String::from(
                    "advance-late-stage failed closed because workflow/operator did not expose final_review_recording_ready for the supplied dispatch lineage.",
                ),
            });
        }
        ensure_final_review_dispatch_id_matches(&context, dispatch_id)?;
        if !matches!(
            reviewer_source,
            "fresh-context-subagent" | "cross-model" | "human-independent-reviewer"
        ) {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "reviewer_source_invalid: final-review advance-late-stage requires an independent reviewer source.",
            ));
        }
        let reviewed_state_id = overlay
            .current_branch_closure_reviewed_state_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "advance-late-stage final-review requires a current reviewed branch state id.",
                )
            })?;
        let browser_qa_required = current_plan_requires_browser_qa(&context);
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage requires authoritative harness state.",
            ));
        };
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
            if current_reviewer_source == reviewer_source
                && current_reviewer_id == reviewer_id
                && current_result == result
                && current_summary_hash == normalized_summary_hash
            {
                return Ok(AdvanceLateStageOutput {
                    action: String::from("already_current"),
                    stage_path: String::from("final_review"),
                    delegated_primitive: String::from("record-final-review"),
                    branch_closure_id: Some(branch_closure_id),
                    dispatch_id: Some(dispatch_id.clone()),
                    result: result.to_owned(),
                    required_follow_up: (result == "fail")
                        .then(|| negative_result_follow_up(&operator))
                        .flatten(),
                    trace_summary: String::from(
                        "Current branch closure already has an equivalent recorded final-review outcome.",
                    ),
                });
            }
            return Ok(AdvanceLateStageOutput {
                action: String::from("blocked"),
                stage_path: String::from("final_review"),
                delegated_primitive: String::from("record-final-review"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: Some(dispatch_id.clone()),
                result: result.to_owned(),
                required_follow_up: None,
                trace_summary: String::from(
                    "advance-late-stage failed closed because the current branch closure already has a conflicting recorded final-review outcome for this dispatch lineage.",
                ),
            });
        }
        let final_review_source = render_final_review_artifacts(
            runtime,
            &context,
            &branch_closure_id,
            &reviewed_state_id,
            FinalReviewArtifactInputs {
                dispatch_id,
                reviewer_source,
                reviewer_id,
                result,
                summary: &summary,
            },
        )?;
        let final_review_fingerprint = if result == "pass" {
            Some(publish_authoritative_artifact(
                runtime,
                "final-review",
                &final_review_source,
            )?)
        } else {
            None
        };
        authoritative_state.record_final_review_result(FinalReviewResultRecord {
            branch_closure_id: &branch_closure_id,
            dispatch_id,
            reviewer_source,
            reviewer_id,
            result,
            final_review_fingerprint: final_review_fingerprint.as_deref(),
            browser_qa_required,
            summary_hash: &normalized_summary_hash,
        })?;
        authoritative_state.persist_if_dirty_with_failpoint(None)?;
        return Ok(AdvanceLateStageOutput {
            action: String::from("recorded"),
            stage_path: String::from("final_review"),
            delegated_primitive: String::from("record-final-review"),
            branch_closure_id: Some(branch_closure_id),
            dispatch_id: Some(dispatch_id.clone()),
            result: result.to_owned(),
            required_follow_up: (result == "fail")
                .then(|| negative_result_follow_up(&operator))
                .flatten(),
            trace_summary: String::from(
                "Validated final-review dispatch lineage and recorded final-review late-stage evidence.",
            ),
        });
    }

    let operator = current_workflow_operator(runtime, &args.plan, false)?;
    if operator.phase != "document_release_pending"
        || !matches!(
            operator.phase_detail.as_str(),
            "release_readiness_recording_ready" | "release_blocker_resolution_required"
        )
        || operator.review_state_status != "clean"
    {
        return Ok(AdvanceLateStageOutput {
            action: String::from("blocked"),
            stage_path: String::from("release_readiness"),
            delegated_primitive: String::from("record-release-readiness"),
            branch_closure_id: Some(branch_closure_id),
            dispatch_id: None,
            result: args.result.trim().to_owned(),
            required_follow_up: blocked_follow_up_for_operator(&operator),
            trace_summary: String::from(
                "advance-late-stage failed closed because workflow/operator did not expose a release-readiness recording state for the current branch closure.",
            ),
        });
    }

    if args.branch_closure_id.is_some() || args.reviewer_source.is_some() || args.reviewer_id.is_some() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "release_readiness_argument_mismatch: release-readiness advance-late-stage does not accept final-review-only arguments.",
        ));
    }
    let result = match args.result.trim() {
        "ready" => "ready",
        "blocked" => "blocked",
        _ => {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "release_readiness_result_invalid: release-readiness advance-late-stage requires --result ready|blocked.",
            ));
        }
    };
    {
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage requires authoritative harness state.",
            ));
        };
        if let (Some(current_result), Some(current_summary_hash)) = (
            authoritative_state.current_release_readiness_result(),
            authoritative_state.current_release_readiness_summary_hash(),
        ) {
            if current_result == result && current_summary_hash == normalized_summary_hash {
                return Ok(AdvanceLateStageOutput {
                    action: String::from("already_current"),
                    stage_path: String::from("release_readiness"),
                    delegated_primitive: String::from("record-release-readiness"),
                    branch_closure_id: Some(branch_closure_id),
                    dispatch_id: None,
                    result: result.to_owned(),
                    required_follow_up: (result == "blocked")
                        .then(|| String::from("resolve_release_blocker")),
                    trace_summary: String::from(
                        "Current branch closure already has an equivalent recorded release-readiness outcome.",
                    ),
                });
            }
            if current_result != "blocked" {
                return Ok(AdvanceLateStageOutput {
                    action: String::from("blocked"),
                    stage_path: String::from("release_readiness"),
                    delegated_primitive: String::from("record-release-readiness"),
                    branch_closure_id: Some(branch_closure_id),
                    dispatch_id: None,
                    result: result.to_owned(),
                    required_follow_up: None,
                    trace_summary: String::from(
                        "advance-late-stage failed closed because the current branch closure already has a conflicting recorded release-readiness outcome.",
                    ),
                });
            }
        }
    }
    let reviewed_state_id = overlay
        .current_branch_closure_reviewed_state_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage release-readiness requires a current reviewed branch state id.",
            )
        })?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage requires authoritative harness state.",
        ));
    };
    if let (Some(current_result), Some(current_summary_hash)) = (
        authoritative_state.current_release_readiness_result(),
        authoritative_state.current_release_readiness_summary_hash(),
    ) {
        if current_result == result && current_summary_hash == normalized_summary_hash {
            return Ok(AdvanceLateStageOutput {
                action: String::from("already_current"),
                stage_path: String::from("release_readiness"),
                delegated_primitive: String::from("record-release-readiness"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: None,
                result: result.to_owned(),
                required_follow_up: (result == "blocked")
                    .then(|| String::from("resolve_release_blocker")),
                trace_summary: String::from(
                    "Current branch closure already has an equivalent recorded release-readiness outcome.",
                ),
            });
        }
        if current_result != "blocked" {
            return Ok(AdvanceLateStageOutput {
                action: String::from("blocked"),
                stage_path: String::from("release_readiness"),
                delegated_primitive: String::from("record-release-readiness"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: None,
                result: result.to_owned(),
                required_follow_up: None,
                trace_summary: String::from(
                    "advance-late-stage failed closed because the current branch closure already has a conflicting recorded release-readiness outcome.",
                ),
            });
        }
    }
    let release_source = render_release_readiness_artifact(
        runtime,
        &context,
        &branch_closure_id,
        &reviewed_state_id,
        result,
        &summary,
    )?;
    let release_fingerprint = if result == "ready" {
        Some(publish_authoritative_artifact(
            runtime,
            "release-docs",
            &release_source,
        )?)
    } else {
        None
    };
    authoritative_state.record_release_readiness_result(
        result,
        release_fingerprint.as_deref(),
        &normalized_summary_hash,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(AdvanceLateStageOutput {
        action: String::from("recorded"),
        stage_path: String::from("release_readiness"),
        delegated_primitive: String::from("record-release-readiness"),
        branch_closure_id: Some(branch_closure_id),
        dispatch_id: None,
        result: result.to_owned(),
        required_follow_up: (result == "blocked").then(|| String::from("resolve_release_blocker")),
        trace_summary: String::from(
            "Recorded release-readiness late-stage evidence for the current branch closure.",
        ),
    })
}

pub fn record_qa(runtime: &ExecutionRuntime, args: &RecordQaArgs) -> Result<RecordQaOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let operator = current_workflow_operator(runtime, &args.plan, false)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-qa requires authoritative harness state.",
        )
    })?;
    let branch_closure_id = overlay
        .current_branch_closure_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default();
    if operator.review_state_status != "clean" {
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            required_follow_up: blocked_follow_up_for_operator(&operator),
            trace_summary: String::from(
                "record-qa failed closed because workflow/operator did not expose qa_recording_required for the current branch closure.",
            ),
        });
    }
    if operator.phase != "qa_pending" || operator.phase_detail != "qa_recording_required" {
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            required_follow_up: None,
            trace_summary: String::from(
                "record-qa failed closed because the current phase is out of band for QA recording; reroute through workflow/operator.",
            ),
        });
    }
    if branch_closure_id.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-qa requires a current branch closure.",
        ));
    }
    let summary = read_nonempty_summary_file(&args.summary_file, "summary")?;
    let summary_hash = qa_summary_hash(&summary);
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
        if current_result == args.result.as_str() && current_summary_hash == summary_hash {
            return Ok(RecordQaOutput {
                action: String::from("already_current"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                required_follow_up: (args.result == ReviewOutcomeArg::Fail)
                    .then(|| negative_result_follow_up(&operator))
                    .flatten(),
                trace_summary: String::from(
                    "Current branch closure already has an equivalent recorded browser QA outcome.",
                ),
            });
        }
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            required_follow_up: None,
            trace_summary: String::from(
                "record-qa failed closed because the current branch closure already has a conflicting recorded browser QA outcome.",
            ),
        });
    }
    let test_plan_path = current_test_plan_artifact_path(&context).ok();
    let reviewed_state_id = overlay
        .current_branch_closure_reviewed_state_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "record-qa requires a current reviewed branch state id.",
            )
        })?;
    let qa_source = render_qa_artifact(
        runtime,
        &context,
        &branch_closure_id,
        &reviewed_state_id,
        args.result.as_str(),
        &summary,
        test_plan_path.as_deref(),
    )?;
    let qa_fingerprint = if args.result == ReviewOutcomeArg::Pass {
        let authoritative_qa_source = if let Some(test_plan_path) = test_plan_path.as_deref() {
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
            let authoritative_test_plan_path = publish_authoritative_rebuild_artifact(
                runtime,
                &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
                &authoritative_test_plan_source,
            )?;
            rewrite_rebuild_source_test_plan_header(&qa_source, &authoritative_test_plan_path)
        } else {
            qa_source.clone()
        };
        let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
        publish_authoritative_rebuild_artifact(
            runtime,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
            &authoritative_qa_source,
        )?;
        Some(authoritative_qa_fingerprint)
    } else {
        None
    };
    authoritative_state.record_browser_qa_result(
        &branch_closure_id,
        args.result.as_str(),
        qa_fingerprint.as_deref(),
        &summary_hash,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(RecordQaOutput {
        action: String::from("recorded"),
        branch_closure_id,
        result: args.result.as_str().to_owned(),
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
    let context = load_execution_context(runtime, &request.plan)?;
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
        ensure_rebuild_preflight_acceptance(&context)?;
        let refreshed_status = status_from_context(&load_execution_context(runtime, &args.plan)?)?;
        refresh_rebuild_all_task_closure_receipts_if_available(
            runtime,
            &args.plan,
            &refreshed_status,
        )?;
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

    let execution_mode = match context.plan_document.execution_mode.as_str() {
        "featureforge:executing-plans" => ExecutionModeArg::ExecutingPlans,
        "featureforge:subagent-driven-development" => {
            ExecutionModeArg::SubagentDrivenDevelopment
        }
        _ => {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "rebuild-evidence requires an approved plan revision with an execution mode.",
            ));
        }
    };

    ensure_rebuild_preflight_acceptance(&context)?;

    let mut status = status_from_context(&context)?;
    let mut targets = Vec::with_capacity(candidates.len());
    let mut counts = RebuildEvidenceCounts {
        planned: candidates.len() as u32,
        rebuilt: 0,
        manual: 0,
        failed: 0,
        noop: 0,
    };
    let candidate_batch_is_manual_only = request.skip_manual_fallback
        && !candidates.is_empty()
        && candidates.iter().all(|candidate| candidate.verify_command.is_none());
    let mut saw_strict_manual_failure = false;
    let mut saw_precondition_failure = false;
    let mut saw_non_precondition_failure = false;

    for (index, candidate) in candidates.iter().enumerate() {
        let (next_status, target) = execute_rebuild_candidate(
            runtime,
            &request,
            &args.plan,
            execution_mode,
            status,
            candidate,
            index + 1 == candidates.len(),
        )?;
        status = next_status;
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
        if should_stop {
            break;
        }
    }

    let strict_manual_only = candidate_batch_is_manual_only
        && saw_strict_manual_failure
        && !saw_precondition_failure
        && !saw_non_precondition_failure;
    let manual_repairs_still_pending = counts.manual > 0;
    let exit_code = if strict_manual_only {
        3
    } else if saw_non_precondition_failure || saw_strict_manual_failure {
        2
    } else if saw_precondition_failure {
        1
    } else {
        if !manual_repairs_still_pending {
            refresh_rebuild_downstream_truth(runtime, &args.plan)?;
        }
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

fn ensure_rebuild_preflight_acceptance(context: &ExecutionContext) -> Result<(), JsonFailure> {
    let acceptance = persist_preflight_acceptance(context)?;
    ensure_preflight_authoritative_bootstrap(
        &context.runtime,
        RunIdentitySnapshot {
            execution_run_id: acceptance.execution_run_id.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
        },
        acceptance.chunk_id,
    )
}

fn is_rebuild_precondition_failure(failure_class: &str) -> bool {
    matches!(
        failure_class,
        "artifact_read_error" | "state_transition_blocked" | "target_race"
    )
}

struct RebuildCandidateExecutionState {
    status: PlanExecutionStatus,
    target: RebuildEvidenceTarget,
    expected_attempt_number: Option<u32>,
    expected_artifact_epoch: Option<String>,
}

fn execute_rebuild_candidate(
    runtime: &ExecutionRuntime,
    request: &crate::execution::state::RebuildEvidenceRequest,
    plan: &Path,
    execution_mode: ExecutionModeArg,
    mut status: PlanExecutionStatus,
    candidate: &RebuildEvidenceCandidate,
    allow_manual_open_step: bool,
) -> Result<(PlanExecutionStatus, RebuildEvidenceTarget), JsonFailure> {
    let mut current_candidate = candidate.clone();
    let mut expected_attempt_number = current_candidate.attempt_number;
    let mut expected_artifact_epoch = current_candidate.artifact_epoch.clone();
    let attempt_id_before = current_candidate
        .attempt_number
        .map(|attempt| format!("{}:{}:{}", current_candidate.task, current_candidate.step, attempt));
    let mut target = RebuildEvidenceTarget {
        task_id: current_candidate.task,
        step_id: current_candidate.step,
        target_kind: current_candidate.target_kind.clone(),
        pre_invalidation_reason: current_candidate.pre_invalidation_reason.clone(),
        status: String::from("planned"),
        verify_mode: current_candidate.verify_mode.clone(),
        verify_command: current_candidate.verify_command.clone(),
        attempt_id_before,
        attempt_id_after: None,
        verification_hash: None,
        error: None,
        failure_class: None,
    };

    if current_candidate.target_kind == "artifact_read_error" {
        target.status = String::from("failed");
        target.failure_class = Some(String::from("artifact_read_error"));
        target.error = Some(current_candidate.pre_invalidation_reason.clone());
        return Ok((status, target));
    }

    for replay_attempt in 0..=1 {
        if candidate_row_changed(
            runtime,
            plan,
            current_candidate.task,
            current_candidate.step,
            expected_attempt_number,
            expected_artifact_epoch.as_deref(),
        )? {
            if replay_attempt == 0 {
                sleep(Duration::from_millis(10));
                status = refresh_rebuild_status(runtime, plan)?;
                if let Some(refreshed_candidate) = refresh_rebuild_candidate(
                    runtime,
                    request,
                    plan,
                    current_candidate.task,
                    current_candidate.step,
                )? {
                    current_candidate = refreshed_candidate;
                    expected_attempt_number = current_candidate.attempt_number;
                    expected_artifact_epoch = current_candidate.artifact_epoch.clone();
                    target = planned_rebuild_target(&current_candidate);
                }
                continue;
            }
            target.status = String::from("failed");
            target.failure_class = Some(String::from("target_race"));
            target.error = Some(String::from(
                "target_race: the selected target changed during replay; rerun with --max-jobs 1.",
            ));
            return Ok((status, target));
        }

        let result = execute_rebuild_candidate_once(
            runtime,
            request,
            plan,
            execution_mode,
            &current_candidate,
            allow_manual_open_step,
            RebuildCandidateExecutionState {
                status,
                target,
                expected_attempt_number,
                expected_artifact_epoch: expected_artifact_epoch.clone(),
            },
        )?;
        match result.target.failure_class.as_deref() {
            Some("state_transition_blocked" | "target_race") if replay_attempt == 0 => {
                sleep(Duration::from_millis(10));
                status = refresh_rebuild_status(runtime, plan)?;
                if let Some(refreshed_candidate) = refresh_rebuild_candidate(
                    runtime,
                    request,
                    plan,
                    current_candidate.task,
                    current_candidate.step,
                )? {
                    current_candidate = refreshed_candidate;
                    expected_attempt_number = current_candidate.attempt_number;
                    expected_artifact_epoch = current_candidate.artifact_epoch.clone();
                    target = planned_rebuild_target(&current_candidate);
                } else {
                    target = result.target;
                    expected_attempt_number = result.expected_attempt_number;
                    expected_artifact_epoch = result.expected_artifact_epoch;
                }
                continue;
            }
            _ => return Ok((result.status, result.target)),
        }
    }

    Ok((status, target))
}

fn execute_rebuild_candidate_once(
    runtime: &ExecutionRuntime,
    request: &crate::execution::state::RebuildEvidenceRequest,
    plan: &Path,
    execution_mode: ExecutionModeArg,
    candidate: &RebuildEvidenceCandidate,
    allow_manual_open_step: bool,
    replay_state: RebuildCandidateExecutionState,
) -> Result<RebuildCandidateExecutionState, JsonFailure> {
    let RebuildCandidateExecutionState {
        mut status,
        mut target,
        mut expected_attempt_number,
        mut expected_artifact_epoch,
    } = replay_state;
    let verify_command = candidate.verify_command.clone();
    if verify_command.is_none() && !request.skip_manual_fallback && !allow_manual_open_step {
        target.status = String::from("manual_required");
        target.failure_class = Some(String::from("manual_required"));
        target.error = Some(String::from(
            "No stored verify command is available for this target.",
        ));
        return Ok(RebuildCandidateExecutionState {
            status,
            target,
            expected_attempt_number,
            expected_artifact_epoch,
        });
    }

    if candidate.needs_reopen {
        status = clear_superseded_interrupted_rebuild_step(
            runtime,
            request,
            plan,
            status,
            candidate,
        )?;
        let reopened = reopen(
            runtime,
            &ReopenArgs {
                plan: plan.to_path_buf(),
                task: candidate.task,
                step: candidate.step,
                source: execution_mode,
                reason: format!(
                    "Evidence rebuild: {}",
                    candidate.pre_invalidation_reason
                ),
                expect_execution_fingerprint: status.execution_fingerprint.clone(),
            },
        );
        match reopened {
            Ok(next_status) => {
                status = next_status;
                let current_identity = current_attempt_identity(runtime, plan, candidate.task, candidate.step)?;
                expected_attempt_number = current_identity.as_ref().map(|(attempt, _)| *attempt);
                expected_artifact_epoch = current_identity.map(|(_, recorded_at)| recorded_at);
            }
            Err(error) => {
                target.status = String::from("failed");
                target.failure_class = Some(String::from("state_transition_blocked"));
                target.error = Some(error.message.clone());
                return Ok(RebuildCandidateExecutionState {
                    status,
                    target,
                    expected_attempt_number,
                    expected_artifact_epoch,
                });
            }
        }
    }

    let Some(verify_command) = verify_command else {
        if request.skip_manual_fallback {
            target.status = String::from("failed");
            target.failure_class = Some(String::from("manual_required"));
            target.error = Some(String::from(
                "manual_required: no stored verify command is available for this target.",
            ));
        } else {
            target.status = String::from("manual_required");
            target.failure_class = Some(String::from("manual_required"));
            target.error = Some(String::from(
                "No stored verify command is available for this target.",
            ));
        }
        return Ok(RebuildCandidateExecutionState {
            status,
            target,
            expected_attempt_number,
            expected_artifact_epoch,
        });
    };

    let command_output = verify_command_process(&runtime.repo_root, &verify_command).output();
    let command_output = match command_output {
        Ok(output) => output,
        Err(error) => {
            target.status = String::from("failed");
            target.failure_class = Some(String::from("verify_command_failed"));
            target.error = Some(format!("Could not execute verify command: {error}"));
            return Ok(RebuildCandidateExecutionState {
                status,
                target,
                expected_attempt_number,
                expected_artifact_epoch,
            });
        }
    };
    let verify_result = summarize_verify_result(&command_output, request.no_output);
    target.verification_hash = Some(crate::git::sha256_hex(verify_result.as_bytes()));
    if !command_output.status.success() {
        target.status = String::from("failed");
        target.failure_class = Some(String::from("verify_command_failed"));
        target.error = Some(verify_result);
        return Ok(RebuildCandidateExecutionState {
            status,
            target,
            expected_attempt_number,
            expected_artifact_epoch,
        });
    }
    if candidate_row_changed(
        runtime,
        plan,
        candidate.task,
        candidate.step,
        expected_attempt_number,
        expected_artifact_epoch.as_deref(),
    )? {
        target.status = String::from("failed");
        target.failure_class = Some(String::from("target_race"));
        target.error = Some(String::from(
            "target_race: the selected target changed during replay; rerun with --max-jobs 1.",
        ));
        return Ok(RebuildCandidateExecutionState {
            status,
            target,
            expected_attempt_number,
            expected_artifact_epoch,
        });
    }

    if status.active_task != Some(candidate.task) || status.active_step != Some(candidate.step) {
        let begin_result = begin(
            runtime,
            &BeginArgs {
                plan: plan.to_path_buf(),
                task: candidate.task,
                step: candidate.step,
                execution_mode: None,
                expect_execution_fingerprint: status.execution_fingerprint.clone(),
            },
        );
        match begin_result {
            Ok(next_status) => status = next_status,
            Err(error) => {
                if candidate.task > 1 && is_rebuild_task_boundary_receipt_failure(&error.message) {
                    let refreshed_status = refresh_rebuild_status(runtime, plan)?;
                    match refresh_rebuild_task_closure_receipts(
                        runtime,
                        plan,
                        &refreshed_status,
                        candidate.task - 1,
                    ) {
                        Ok(()) => {
                            let retried_status = refresh_rebuild_status(runtime, plan)?;
                            let retry_begin = begin(
                                runtime,
                                &BeginArgs {
                                    plan: plan.to_path_buf(),
                                    task: candidate.task,
                                    step: candidate.step,
                                    execution_mode: None,
                                    expect_execution_fingerprint: retried_status
                                        .execution_fingerprint
                                        .clone(),
                                },
                            );
                            match retry_begin {
                                Ok(next_status) => status = next_status,
                                Err(error) => {
                                    target.status = String::from("failed");
                                    target.failure_class = Some(String::from("state_transition_blocked"));
                                    target.error = Some(error.message.clone());
                                    return Ok(RebuildCandidateExecutionState {
                                        status,
                                        target,
                                        expected_attempt_number,
                                        expected_artifact_epoch,
                                    });
                                }
                            }
                        }
                        Err(refresh_error) => {
                            target.status = String::from("failed");
                            target.failure_class = Some(String::from("state_transition_blocked"));
                            target.error = Some(refresh_error.message.clone());
                            return Ok(RebuildCandidateExecutionState {
                                status,
                                target,
                                expected_attempt_number,
                                expected_artifact_epoch,
                            });
                        }
                    }
                } else {
                    target.status = String::from("failed");
                    target.failure_class = Some(String::from("state_transition_blocked"));
                    target.error = Some(error.message.clone());
                    return Ok(RebuildCandidateExecutionState {
                        status,
                        target,
                        expected_attempt_number,
                        expected_artifact_epoch,
                    });
                }
            }
        }
    }

    let completed = complete(
        runtime,
        &CompleteArgs {
            plan: plan.to_path_buf(),
            task: candidate.task,
            step: candidate.step,
            source: execution_mode,
            claim: candidate.claim.clone(),
            files: candidate.files.clone(),
            verify_command: Some(verify_command),
            verify_result: Some(verify_result.clone()),
            manual_verify_summary: None,
            expect_execution_fingerprint: status.execution_fingerprint.clone(),
        },
    );
    match completed {
        Ok(next_status) => {
            let refreshed_status = match refresh_rebuild_closure_receipts(
                runtime,
                plan,
                &next_status,
                candidate.task,
                candidate.step,
            ) {
                Ok(()) => next_status,
                Err(error) => {
                    target.status = String::from("failed");
                    target.failure_class = Some(String::from("state_transition_blocked"));
                    target.error = Some(error.message.clone());
                    return Ok(RebuildCandidateExecutionState {
                        status,
                        target,
                        expected_attempt_number,
                        expected_artifact_epoch,
                    });
                }
            };
            target.status = String::from("rebuilt");
            target.error = None;
            target.failure_class = None;
            target.attempt_id_after = Some(format!(
                "{}:{}:{}",
                candidate.task,
                candidate.step,
                candidate.attempt_number.unwrap_or(0) + 1
            ));
            Ok(RebuildCandidateExecutionState {
                status: refreshed_status,
                target,
                expected_attempt_number,
                expected_artifact_epoch,
            })
        }
        Err(error) => {
            target.status = String::from("failed");
            target.failure_class = Some(String::from("state_transition_blocked"));
            target.error = Some(error.message.clone());
            Ok(RebuildCandidateExecutionState {
                status,
                target,
                expected_attempt_number,
                expected_artifact_epoch,
            })
        }
    }
}

fn clear_superseded_interrupted_rebuild_step(
    runtime: &ExecutionRuntime,
    request: &crate::execution::state::RebuildEvidenceRequest,
    plan: &Path,
    status: PlanExecutionStatus,
    candidate: &RebuildEvidenceCandidate,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let Some((resume_task, resume_step)) = status
        .resume_task
        .zip(status.resume_step)
    else {
        return Ok(status);
    };
    if (resume_task, resume_step) == (candidate.task, candidate.step) {
        return Ok(status);
    }

    let context = load_execution_context(runtime, plan)?;
    let interrupted_is_targeted = discover_rebuild_candidates(&context, request)?
        .iter()
        .any(|target| target.task == resume_task && target.step == resume_step);
    if !interrupted_is_targeted {
        return Ok(status);
    }

    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, plan)?;
    let Some(interrupted_index) = context.steps.iter().position(|step| {
        step.task_number == resume_task
            && step.step_number == resume_step
            && step.note_state == Some(crate::execution::state::NoteState::Interrupted)
    }) else {
        return Ok(status);
    };

    context.steps[interrupted_index].note_state = None;
    context.steps[interrupted_index].note_summary.clear();

    let rendered_plan = render_plan_source(
        &context.plan_source,
        &context.plan_document.execution_mode,
        &context.steps,
    );
    write_atomic(&context.plan_abs, &rendered_plan)?;

    let reloaded = load_execution_context_for_mutation(runtime, plan)?;
    status_from_context(&reloaded)
}

fn refresh_rebuild_status(
    runtime: &ExecutionRuntime,
    plan: &Path,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let context = load_execution_context(runtime, plan)?;
    status_from_context(&context)
}

fn refresh_rebuild_candidate(
    runtime: &ExecutionRuntime,
    request: &crate::execution::state::RebuildEvidenceRequest,
    plan: &Path,
    task: u32,
    step: u32,
) -> Result<Option<RebuildEvidenceCandidate>, JsonFailure> {
    let context = load_execution_context(runtime, plan)?;
    let candidates = discover_rebuild_candidates(&context, request)?;
    Ok(candidates
        .into_iter()
        .find(|candidate| candidate.task == task && candidate.step == step))
}

fn candidate_row_changed(
    runtime: &ExecutionRuntime,
    plan: &Path,
    task: u32,
    step: u32,
    expected_attempt_number: Option<u32>,
    expected_artifact_epoch: Option<&str>,
) -> Result<bool, JsonFailure> {
    if expected_attempt_number.is_none() && expected_artifact_epoch.is_none() {
        return Ok(false);
    }

    let current_identity = current_attempt_identity(runtime, plan, task, step)?;
    let Some((current_attempt_number, current_recorded_at)) = current_identity else {
        return Ok(true);
    };

    Ok(expected_attempt_number != Some(current_attempt_number)
        || expected_artifact_epoch != Some(current_recorded_at.as_str()))
}

fn current_attempt_identity(
    runtime: &ExecutionRuntime,
    plan: &Path,
    task: u32,
    step: u32,
) -> Result<Option<(u32, String)>, JsonFailure> {
    let context = load_execution_context(runtime, plan)?;
    let latest_attempt = context
        .evidence
        .attempts
        .iter()
        .rev()
        .find(|attempt| {
            attempt.task_number == task && attempt.step_number == step
        });

    let Some(latest_attempt) = latest_attempt else {
        return Ok(None);
    };

    Ok(Some((
        latest_attempt.attempt_number,
        latest_attempt.recorded_at.clone(),
    )))
}

fn refresh_rebuild_closure_receipts(
    runtime: &ExecutionRuntime,
    plan: &Path,
    status: &PlanExecutionStatus,
    task: u32,
    _step: u32,
) -> Result<(), JsonFailure> {
    let Some(execution_run_id) = status.execution_run_id.as_ref().map(|value| value.as_str()) else {
        return Ok(());
    };
    let context = load_execution_context(runtime, plan)?;
    let strategy_checkpoint = authoritative_strategy_checkpoint_fingerprint_checked(&context)?;
    let Some(strategy_checkpoint_fingerprint) = strategy_checkpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let active_contract_fingerprint = load_authoritative_transition_state(&context)?
        .as_ref()
        .and_then(|authority| authority.evidence_provenance().source_contract_fingerprint);
    let checked_tasks = context
        .steps
        .iter()
        .filter(|step_state| step_state.checked)
        .map(|step_state| step_state.task_number)
        .collect::<BTreeSet<_>>();
    for checked_task in checked_tasks {
        refresh_rebuild_task_closure_receipts_with_context(
            runtime,
            &context,
            TaskClosureReceiptRefresh {
                execution_run_id,
                strategy_checkpoint_fingerprint,
                active_contract_fingerprint: active_contract_fingerprint.as_deref(),
                task: checked_task,
                restore_missing_dispatch_lineage: checked_task == task,
                claim_write_authority: true,
            },
        )?;
    }
    Ok(())
}

fn refresh_rebuild_task_closure_receipts(
    runtime: &ExecutionRuntime,
    plan: &Path,
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<(), JsonFailure> {
    let Some(execution_run_id) = status.execution_run_id.as_ref().map(|value| value.as_str()) else {
        return Ok(());
    };
    let context = load_execution_context(runtime, plan)?;
    let strategy_checkpoint = authoritative_strategy_checkpoint_fingerprint_checked(&context)?;
    let Some(strategy_checkpoint_fingerprint) = strategy_checkpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let active_contract_fingerprint = load_authoritative_transition_state(&context)?
        .as_ref()
        .and_then(|authority| authority.evidence_provenance().source_contract_fingerprint);
    refresh_rebuild_task_closure_receipts_with_context(
        runtime,
        &context,
        TaskClosureReceiptRefresh {
            execution_run_id,
            strategy_checkpoint_fingerprint,
            active_contract_fingerprint: active_contract_fingerprint.as_deref(),
            task,
            restore_missing_dispatch_lineage: true,
            claim_write_authority: true,
        },
    )
}

fn refresh_rebuild_all_task_closure_receipts(
    runtime: &ExecutionRuntime,
    plan: &Path,
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    let Some(execution_run_id) = status.execution_run_id.as_ref().map(|value| value.as_str()) else {
        return Ok(());
    };
    let context = load_execution_context(runtime, plan)?;
    let strategy_checkpoint = authoritative_strategy_checkpoint_fingerprint_checked(&context)?;
    let Some(strategy_checkpoint_fingerprint) = strategy_checkpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let active_contract_fingerprint = load_authoritative_transition_state(&context)?
        .as_ref()
        .and_then(|authority| authority.evidence_provenance().source_contract_fingerprint);
    let checked_tasks = context
        .steps
        .iter()
        .filter(|step_state| step_state.checked)
        .map(|step_state| step_state.task_number)
        .collect::<BTreeSet<_>>();
    for task in checked_tasks {
        refresh_rebuild_task_closure_receipts_with_context(
            runtime,
            &context,
            TaskClosureReceiptRefresh {
                execution_run_id,
                strategy_checkpoint_fingerprint,
                active_contract_fingerprint: active_contract_fingerprint.as_deref(),
                task,
                restore_missing_dispatch_lineage: false,
                claim_write_authority: true,
            },
        )?;
    }
    Ok(())
}

fn refresh_rebuild_all_task_closure_receipts_if_available(
    runtime: &ExecutionRuntime,
    plan: &Path,
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    match refresh_rebuild_all_task_closure_receipts(runtime, plan, status) {
        Ok(()) => Ok(()),
        Err(error)
            if error.error_class == "MalformedExecutionState"
                && error
                    .message
                    .contains("last_strategy_checkpoint_fingerprint") =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn refresh_rebuild_downstream_truth(
    runtime: &ExecutionRuntime,
    plan: &Path,
) -> Result<(), JsonFailure> {
    let context = load_execution_context(runtime, plan)?;
    let branch = &context.runtime.branch_name;
    let current_head = current_head_sha(&context.runtime.repo_root).unwrap_or_default();
    let artifact_dir = context
        .runtime
        .state_dir
        .join("projects")
        .join(&context.runtime.repo_slug);
    let final_review_candidate = latest_branch_artifact_path(&artifact_dir, branch, "code-review");
    let test_plan_candidate = latest_branch_artifact_path(&artifact_dir, branch, "test-plan");
    let release_candidate =
        latest_branch_artifact_path(&artifact_dir, branch, "release-readiness");
    if final_review_candidate.is_none() && release_candidate.is_none() {
        return Ok(());
    }
    let Some(base_branch) = resolve_release_base_branch(&context.runtime.git_dir, branch) else {
        return Err(rebuild_downstream_truth_stale(
            "post_rebuild_late_gate_truth_stale: rebuild completed, but the release base branch could not be resolved for downstream artifact validation.",
        ));
    };

    let Some(final_review_path) = final_review_candidate else {
        return Err(rebuild_downstream_truth_stale(
            "post_rebuild_late_gate_truth_stale: rebuild completed, but the current branch is missing a final review artifact to rebind authoritative downstream truth.",
        ));
    };
    let initial_review = parse_artifact_document(&final_review_path);
    if initial_review.title.as_deref() != Some("# Code Review Result") {
        return Err(rebuild_downstream_truth_stale(format!(
            "post_rebuild_late_gate_truth_stale: rebuild completed, but final review artifact {} is malformed.",
            final_review_path.display()
        )));
    }
    let initial_review_receipt = parse_final_review_receipt(&final_review_path);

    let Some(reviewer_artifact_path) = resolve_rebuild_reviewer_artifact_path(
        &final_review_path,
        initial_review_receipt.reviewer_artifact_path.as_deref(),
    ) else {
        return Err(rebuild_downstream_truth_stale(format!(
            "post_rebuild_late_gate_truth_stale: rebuild completed, but final review artifact {} is missing a dedicated reviewer artifact binding.",
            final_review_path.display()
        )));
    };

    let browser_qa_required = match context.plan_document.qa_requirement.as_deref() {
        Some("required") => true,
        Some("not-required") => false,
        _ => {
            return Err(rebuild_downstream_truth_stale(
                "post_rebuild_late_gate_truth_stale: rebuild completed, but the approved plan is missing valid QA Requirement metadata.",
            ));
        }
    };
    let test_plan_path = match test_plan_candidate {
        Some(test_plan_path) => {
            let initial_test_plan = parse_artifact_document(&test_plan_path);
            if initial_test_plan.title.as_deref() != Some("# Test Plan") {
                return Err(rebuild_downstream_truth_stale(format!(
                    "post_rebuild_late_gate_truth_stale: rebuild completed, but test-plan artifact {} is malformed.",
                    test_plan_path.display()
                )));
            }
            Some(test_plan_path)
        }
        None if browser_qa_required => {
            return Err(rebuild_downstream_truth_stale(
                "post_rebuild_late_gate_truth_stale: rebuild completed, but the current branch is missing a test-plan artifact to rebind downstream truth.",
            ));
        }
        None => None,
    };

    let initial_qa_path = if browser_qa_required {
        let qa_path = latest_branch_artifact_path(&artifact_dir, branch, "test-outcome");
        if let Some(qa_path) = qa_path {
            let initial_qa = parse_artifact_document(&qa_path);
            if initial_qa.title.as_deref() != Some("# QA Result") {
                return Err(rebuild_downstream_truth_stale(format!(
                    "post_rebuild_late_gate_truth_stale: rebuild completed, but QA artifact {} is malformed.",
                    qa_path.display()
                )));
            }
            Some(qa_path)
        } else {
            None
        }
    } else {
        None
    };

    let initial_release_path = if let Some(release_path) = release_candidate {
        let initial_release = parse_artifact_document(&release_path);
        if initial_release.title.as_deref() != Some("# Release Readiness Result") {
            return Err(rebuild_downstream_truth_stale(format!(
                "post_rebuild_late_gate_truth_stale: rebuild completed, but release-readiness artifact {} is malformed.",
                release_path.display()
            )));
        }
        Some(release_path)
    } else {
        None
    };

    let Some(strategy_checkpoint_fingerprint) = authoritative_strategy_checkpoint_fingerprint_checked(&context)? else {
        return Ok(());
    };
    rewrite_branch_final_review_artifacts(
        &final_review_path,
        &reviewer_artifact_path,
        &current_head,
        &strategy_checkpoint_fingerprint,
    )?;
    if let Some(test_plan_path) = test_plan_path.as_ref() {
        rewrite_branch_head_bound_artifact(test_plan_path, &current_head)?;
    }
    if let Some(qa_path) = initial_qa_path.as_ref() {
        rewrite_branch_qa_artifact(
            qa_path,
            &current_head,
            test_plan_path
                .as_ref()
                .expect("QA-required rebuild should keep a current branch test-plan artifact"),
        )?;
    }
    if let Some(release_path) = initial_release_path.as_ref() {
        rewrite_branch_head_bound_artifact(release_path, &current_head)?;
    }

    let review = parse_artifact_document(&final_review_path);
    if review.headers.get("Branch") != Some(branch)
        || review.headers.get("Repo") != Some(&context.runtime.repo_slug)
        || review.headers.get("Base Branch") != Some(&base_branch)
        || review.headers.get("Head SHA") != Some(&current_head)
        || review.headers.get("Result") != Some(&String::from("pass"))
        || review.headers.get("Generated By")
            != Some(&String::from("featureforge:requesting-code-review"))
    {
        return Err(rebuild_downstream_truth_stale(format!(
            "post_rebuild_late_gate_truth_stale: rebuild completed, but final review artifact {} does not match the current branch, repo, base branch, or HEAD.",
            final_review_path.display()
        )));
    }
    let review_source = fs::read_to_string(&final_review_path).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not read rebuild final-review artifact {}: {error}",
                final_review_path.display()
            ),
        )
    })?;
    let authoritative_review_fingerprint = sha256_hex(review_source.as_bytes());
    let authoritative_review_path = publish_authoritative_rebuild_artifact(
        runtime,
        &format!("final-review-{authoritative_review_fingerprint}.md"),
        &review_source,
    )?;
    let review_expectations = FinalReviewReceiptExpectations {
        expected_plan_path: &context.plan_rel,
        expected_plan_revision: context.plan_document.plan_revision,
        expected_strategy_checkpoint_fingerprint: Some(&strategy_checkpoint_fingerprint),
        expected_head_sha: &current_head,
        expected_base_branch: &base_branch,
        deviations_required: false,
    };
    let rebound_review_receipt = parse_final_review_receipt(&authoritative_review_path);
    if validate_final_review_receipt(
        &rebound_review_receipt,
        &authoritative_review_path,
        &review_expectations,
    )
    .is_err()
    {
        return Err(rebuild_downstream_truth_stale(format!(
            "post_rebuild_late_gate_truth_stale: rebuild completed, but the rebound authoritative final review artifact {} did not validate against the rebuilt state.",
            authoritative_review_path.display()
        )));
    }

    let authoritative_test_plan_path = if let Some(test_plan_path) = test_plan_path.as_ref() {
        let test_plan = parse_artifact_document(test_plan_path);
        if test_plan.headers.get("Source Plan") != Some(&format!("`{}`", context.plan_rel))
            || test_plan.headers.get("Source Plan Revision")
                != Some(&context.plan_document.plan_revision.to_string())
            || test_plan.headers.get("Branch") != Some(branch)
            || test_plan.headers.get("Repo") != Some(&context.runtime.repo_slug)
            || test_plan.headers.get("Head SHA") != Some(&current_head)
            || test_plan.headers.get("Generated By")
                != Some(&String::from("featureforge:plan-eng-review"))
        {
            return Err(rebuild_downstream_truth_stale(format!(
                "post_rebuild_late_gate_truth_stale: rebuild completed, but test-plan artifact {} does not match the current approved plan or HEAD.",
                test_plan_path.display()
            )));
        }
        let authoritative_test_plan_source = fs::read_to_string(test_plan_path).map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!(
                    "Could not read rebuild test-plan artifact {}: {error}",
                    test_plan_path.display()
                ),
            )
        })?;
        let authoritative_test_plan_fingerprint =
            sha256_hex(authoritative_test_plan_source.as_bytes());
        Some(publish_authoritative_rebuild_artifact(
            runtime,
            &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
            &authoritative_test_plan_source,
        )?)
    } else {
        None
    };

    let authoritative_browser_qa_fingerprint = if let Some(qa_path) = initial_qa_path.as_ref() {
        let test_plan_path = test_plan_path
            .as_ref()
            .expect("QA-required rebuild should validate browser QA against a branch test-plan");
        let authoritative_test_plan_path = authoritative_test_plan_path
            .as_ref()
            .expect("QA-required rebuild should publish an authoritative test-plan artifact");
        let qa = parse_artifact_document(qa_path);
        let qa_source_test_plan_matches = qa
            .headers
            .get("Source Test Plan")
            .map(|value| value.trim_matches('`').trim().to_owned())
            .filter(|value| !value.is_empty())
            .and_then(|raw| {
                let source_path = PathBuf::from(raw);
                let resolved = if source_path.is_absolute() {
                    source_path
                } else {
                    qa_path
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(source_path)
                };
                fs::canonicalize(resolved).ok()
            })
            .and_then(|source| fs::canonicalize(test_plan_path).ok().map(|target| source == target))
            .unwrap_or(false);
        if qa.headers.get("Source Plan") != Some(&format!("`{}`", context.plan_rel))
            || qa.headers.get("Source Plan Revision")
                != Some(&context.plan_document.plan_revision.to_string())
            || qa.headers.get("Branch") != Some(branch)
            || qa.headers.get("Repo") != Some(&context.runtime.repo_slug)
            || qa.headers.get("Head SHA") != Some(&current_head)
            || qa.headers.get("Result") != Some(&String::from("pass"))
            || qa.headers.get("Generated By") != Some(&String::from("featureforge:qa-only"))
            || !qa_source_test_plan_matches
        {
            return Err(rebuild_downstream_truth_stale(format!(
                "post_rebuild_late_gate_truth_stale: rebuild completed, but QA artifact {} does not match the rebuilt branch state.",
                qa_path.display()
            )));
        }
        let qa_source = fs::read_to_string(qa_path).map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!("Could not read rebuild QA artifact {}: {error}", qa_path.display()),
            )
        })?;
        let authoritative_qa_source = rewrite_rebuild_source_test_plan_header(
            &qa_source,
            authoritative_test_plan_path,
        );
        let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
        publish_authoritative_rebuild_artifact(
            runtime,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
            &authoritative_qa_source,
        )?;
        Some(authoritative_qa_fingerprint)
    } else {
        None
    };

    let authoritative_release_fingerprint = if let Some(release_path) = initial_release_path.as_ref()
    {
        let release = parse_artifact_document(release_path);
        if release.headers.get("Source Plan") != Some(&format!("`{}`", context.plan_rel))
            || release.headers.get("Source Plan Revision")
                != Some(&context.plan_document.plan_revision.to_string())
            || release.headers.get("Branch") != Some(branch)
            || release.headers.get("Repo") != Some(&context.runtime.repo_slug)
            || release.headers.get("Base Branch") != Some(&base_branch)
            || release.headers.get("Head SHA") != Some(&current_head)
            || release.headers.get("Result") != Some(&String::from("pass"))
            || release.headers.get("Generated By")
                != Some(&String::from("featureforge:document-release"))
        {
            return Err(rebuild_downstream_truth_stale(format!(
                "post_rebuild_late_gate_truth_stale: rebuild completed, but release-readiness artifact {} does not match the current approved plan or HEAD.",
                release_path.display()
            )));
        }
        let authoritative_release_source = fs::read_to_string(release_path).map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!(
                    "Could not read rebuild release artifact {}: {error}",
                    release_path.display()
                ),
            )
        })?;
        let authoritative_release_fingerprint =
            sha256_hex(authoritative_release_source.as_bytes());
        publish_authoritative_rebuild_artifact(
            runtime,
            &format!("release-docs-{authoritative_release_fingerprint}.md"),
            &authoritative_release_source,
        )?;
        Some(authoritative_release_fingerprint)
    } else {
        None
    };

    let context = load_execution_context(runtime, plan)?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.restore_downstream_truth(
            &authoritative_review_fingerprint,
            browser_qa_required,
            authoritative_browser_qa_fingerprint.as_deref(),
            authoritative_release_fingerprint.as_deref(),
        )?;
        authoritative_state.persist_if_dirty_with_failpoint(None)?;
    }
    Ok(())
}

fn publish_authoritative_rebuild_artifact(
    runtime: &ExecutionRuntime,
    artifact_file_name: &str,
    source: &str,
) -> Result<PathBuf, JsonFailure> {
    let path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        artifact_file_name,
    );
    write_atomic(&path, source)?;
    Ok(path)
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
    let expected_dispatch = overlay
        .strategy_review_dispatch_lineage
        .get(&lineage_key)
        .and_then(|record| record.dispatch_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                format!(
                    "close-current-task requires a current task review dispatch lineage for task {task}."
                ),
            )
        })?;
    if expected_dispatch != dispatch_id.trim() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!(
                "dispatch_id_mismatch: close-current-task expected dispatch `{expected_dispatch}` for task {task}."
            ),
        ));
    }
    Ok(())
}

fn ensure_final_review_dispatch_id_matches(
    context: &ExecutionContext,
    dispatch_id: &str,
) -> Result<(), JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage final-review path requires authoritative dispatch lineage state.",
        )
    })?;
    let expected_dispatch = overlay
        .final_review_dispatch_lineage
        .as_ref()
        .and_then(|record| {
            let expected_branch_closure_id = record.branch_closure_id.as_deref()?;
            if overlay.current_branch_closure_id.as_deref()? != expected_branch_closure_id {
                return None;
            }
            record.dispatch_id.as_deref()
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage final-review path requires a current final-review dispatch lineage.",
            )
        })?;
    if expected_dispatch != dispatch_id.trim() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!(
                "dispatch_id_mismatch: advance-late-stage expected final-review dispatch `{expected_dispatch}`."
            ),
        ));
    }
    Ok(())
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

fn current_plan_requires_browser_qa(context: &ExecutionContext) -> Option<bool> {
    match context.plan_document.qa_requirement.as_deref() {
        Some("required") => Some(true),
        Some("not-required") => Some(false),
        _ => None,
    }
}

fn current_test_plan_artifact_path(context: &ExecutionContext) -> Result<PathBuf, JsonFailure> {
    let artifact_dir = context
        .runtime
        .state_dir
        .join("projects")
        .join(&context.runtime.repo_slug);
    latest_branch_artifact_path(
        &artifact_dir,
        &context.runtime.branch_name,
        "test-plan",
    )
    .ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "Current late-stage recording requires a current test-plan artifact for the current branch.",
        )
    })
}

fn current_workflow_operator(
    runtime: &ExecutionRuntime,
    plan: &Path,
    external_review_result_ready: bool,
) -> Result<WorkflowOperator, JsonFailure> {
    workflow_operator(
        &runtime.repo_root,
        &WorkflowOperatorArgs {
            plan: plan.to_path_buf(),
            external_review_result_ready,
            json: false,
        },
    )
}

fn blocked_follow_up_for_operator(operator: &WorkflowOperator) -> Option<String> {
    if operator.review_state_status == "stale_unreviewed" {
        return Some(String::from("repair_review_state"));
    }
    match operator.phase_detail.as_str() {
        "task_review_dispatch_required" | "final_review_dispatch_required" => {
            Some(String::from("record_review_dispatch"))
        }
        "branch_closure_recording_required_for_release_readiness" => {
            Some(String::from("record_branch_closure"))
        }
        "release_blocker_resolution_required" => Some(String::from("resolve_release_blocker")),
        "execution_reentry_required" => Some(String::from("execution_reentry")),
        "handoff_recording_required" => Some(String::from("record_handoff")),
        "planning_reentry_required" => Some(String::from("record_pivot")),
        _ => None,
    }
}

fn negative_result_follow_up(operator: &WorkflowOperator) -> Option<String> {
    match operator.follow_up_override.as_str() {
        "record_handoff" => Some(String::from("record_handoff")),
        "record_pivot" => Some(String::from("record_pivot")),
        _ => Some(String::from("execution_reentry")),
    }
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

fn project_artifact_dir(runtime: &ExecutionRuntime) -> PathBuf {
    runtime.state_dir.join("projects").join(&runtime.repo_slug)
}

fn write_project_artifact(
    runtime: &ExecutionRuntime,
    file_name: &str,
    source: &str,
) -> Result<PathBuf, JsonFailure> {
    let dir = project_artifact_dir(runtime);
    fs::create_dir_all(&dir).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not create project artifact directory {}: {error}", dir.display()),
        )
    })?;
    let path = dir.join(file_name);
    write_atomic(&path, source)?;
    Ok(path)
}

fn publish_authoritative_artifact(
    runtime: &ExecutionRuntime,
    prefix: &str,
    source: &str,
) -> Result<String, JsonFailure> {
    let fingerprint = sha256_hex(source.as_bytes());
    let file_name = format!("{prefix}-{fingerprint}.md");
    let path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &file_name,
    );
    write_atomic(&path, source)?;
    Ok(fingerprint)
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
    restore_missing_dispatch_lineage: bool,
    claim_write_authority: bool,
}

fn current_branch_reviewed_state(
    context: &ExecutionContext,
) -> Result<BranchReviewedState, JsonFailure> {
    let source_task_closure_ids = current_branch_source_task_closure_ids(context)?;
    let base_branch = resolve_release_base_branch(&context.runtime.git_dir, &context.runtime.branch_name)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                "record-branch-closure requires an authoritative base-branch binding.",
            )
        })?;
    let reviewed_state_id = format!(
        "git_tree:{}",
        current_repo_tracked_tree_sha(&context.runtime.repo_root)?
    );
    Ok(BranchReviewedState {
        base_branch: base_branch.clone(),
        contract_identity: deterministic_record_id(
            "branch-contract",
            &[
                &context.plan_rel,
                &context.plan_document.plan_revision.to_string(),
                &context.runtime.repo_slug,
                &context.runtime.branch_name,
                &base_branch,
            ],
        ),
        effective_reviewed_branch_surface: String::from("repo_tracked_content"),
        provenance_basis: String::from("task_closure_lineage"),
        reviewed_state_id,
        source_task_closure_ids,
    })
}

fn normalize_summary_content(value: &str) -> String {
    let normalized_newlines = value.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed_lines = normalized_newlines
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>();
    let start = trimmed_lines
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(trimmed_lines.len());
    let end = trimmed_lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map(|index| index + 1)
        .unwrap_or(start);
    trimmed_lines[start..end].join("\n")
}

fn summary_hash(value: &str) -> String {
    sha256_hex(normalize_summary_content(value).as_bytes())
}

pub(crate) fn current_repo_tracked_tree_sha(repo_root: &Path) -> Result<String, JsonFailure> {
    let index_path_output = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--git-path", "index"])
        .output()
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!("Could not resolve the git index path for reviewed-state identity: {error}"),
            )
        })?;
    if !index_path_output.status.success() {
        let stderr = String::from_utf8_lossy(&index_path_output.stderr);
        return Err(JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not resolve the git index path for reviewed-state identity: {}",
                stderr.trim()
            ),
        ));
    }
    let index_path_text = String::from_utf8_lossy(&index_path_output.stdout)
        .trim()
        .to_owned();
    let index_path = PathBuf::from(&index_path_text);
    let index_path = if index_path.is_absolute() {
        index_path
    } else {
        repo_root.join(index_path)
    };
    let temp_index_path = std::env::temp_dir().join(format!(
        "featureforge-reviewed-state-{}-{}.index",
        std::process::id(),
        timestamp_slug()
    ));
    fs::copy(&index_path, &temp_index_path).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not copy git index for reviewed-state identity from {}: {error}",
                index_path.display()
            ),
        )
    })?;
    let add_status = Command::new("git")
        .current_dir(repo_root)
        .env("GIT_INDEX_FILE", &temp_index_path)
        .args(["add", "-u", "."])
        .status()
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!("Could not stage tracked worktree content for reviewed-state identity: {error}"),
            )
        })?;
    if !add_status.success() {
        let _ = fs::remove_file(&temp_index_path);
        return Err(JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            "Could not stage tracked worktree content for reviewed-state identity.",
        ));
    }
    let write_tree_output = Command::new("git")
        .current_dir(repo_root)
        .env("GIT_INDEX_FILE", &temp_index_path)
        .args(["write-tree"])
        .output()
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!("Could not compute the reviewed-state tree identity: {error}"),
            )
        })?;
    let _ = fs::remove_file(&temp_index_path);
    if !write_tree_output.status.success() {
        let stderr = String::from_utf8_lossy(&write_tree_output.stderr);
        return Err(JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not compute the reviewed-state tree identity: {}",
                stderr.trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&write_tree_output.stdout).trim().to_owned())
}

pub(crate) fn tracked_paths_changed_between(
    repo_root: &Path,
    baseline_tree_sha: &str,
    current_tree_sha: &str,
) -> Result<Vec<String>, JsonFailure> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "diff",
            "--name-only",
            "--no-renames",
            baseline_tree_sha,
            current_tree_sha,
        ])
        .output()
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!("Could not diff reviewed-state trees for branch reclosure validation: {error}"),
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not diff reviewed-state trees for branch reclosure validation: {}",
                stderr.trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

pub(crate) fn normalized_late_stage_surface(plan_source: &str) -> Result<Vec<String>, JsonFailure> {
    let Some(raw_value) = parse_plan_header(plan_source, "Late-Stage Surface") else {
        return Ok(Vec::new());
    };
    raw_value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(normalize_late_stage_surface_entry)
        .collect()
}

fn normalize_late_stage_surface_entry(entry: &str) -> Result<String, JsonFailure> {
    let mut normalized = entry.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_owned();
    }
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.split('/').any(|segment| segment == "..")
        || normalized.contains('*')
        || normalized.contains('?')
        || normalized.contains('[')
        || normalized.contains(']')
        || normalized.contains('{')
        || normalized.contains('}')
    {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("late_stage_surface_invalid: unsupported Late-Stage Surface entry `{entry}`."),
        ));
    }
    Ok(normalized)
}

pub(crate) fn path_matches_late_stage_surface(path: &str, surface_entries: &[String]) -> bool {
    surface_entries.iter().any(|entry| {
        if let Some(prefix) = entry.strip_suffix('/') {
            path == prefix || path.starts_with(&format!("{prefix}/"))
        } else {
            path == entry
        }
    })
}

pub(crate) fn current_branch_closure_baseline_tree_sha(
    overlay: &StatusAuthoritativeOverlay,
) -> Option<&str> {
    overlay
        .current_branch_closure_reviewed_state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.strip_prefix("git_tree:"))
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone)]
struct SupersededTaskClosureRecord {
    task: u32,
    closure_record_id: String,
}

fn current_branch_source_task_closure_ids(
    context: &ExecutionContext,
) -> Result<Vec<String>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires authoritative current task-closure state.",
        ));
    };
    let source_task_closure_ids = authoritative_state
        .current_task_closure_results()
        .into_values()
        .filter(|record| {
            record
                .effective_reviewed_surface_paths
                .iter()
                .any(|path| path != NO_REPO_FILES_MARKER)
        })
        .map(|record| record.closure_record_id)
        .collect::<Vec<_>>();
    if source_task_closure_ids.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires at least one still-current task closure contributing authoritative reviewed surface.",
        ));
    }
    Ok(source_task_closure_ids)
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
        &[&context.plan_rel, &task_number.to_string(), &current_lineage],
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
    Ok(format!(
        "git_tree:{}",
        current_repo_tracked_tree_sha(&context.runtime.repo_root)?
    ))
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

fn current_task_closure_baseline_tree_sha(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(None);
    };
    let Some(current_record) = authoritative_state
        .current_task_closure_results()
        .into_iter()
        .max_by_key(|(task, _)| *task)
        .map(|(_, record)| record)
    else {
        return Ok(None);
    };
    if current_record.contract_identity.trim().is_empty() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires task-closure contract identity for the current task-closure baseline.",
        ));
    }
    let Some(tree_sha) = current_record
        .reviewed_state_id
        .strip_prefix("git_tree:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires canonical git_tree task-closure reviewed state.",
        ));
    };
    Ok(Some(tree_sha.to_owned()))
}

fn render_branch_closure_artifact(
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state: &BranchReviewedState,
    superseded_branch_closure_ids: &[String],
) -> Result<String, JsonFailure> {
    let current_head = current_head_sha(&context.runtime.repo_root)?;
    let generated_at = Timestamp::now().to_string();
    let source_task_closure_ids = if reviewed_state.source_task_closure_ids.is_empty() {
        String::from("none")
    } else {
        reviewed_state.source_task_closure_ids.join(", ")
    };
    let superseded_branch_closure_ids = if superseded_branch_closure_ids.is_empty() {
        String::from("none")
    } else {
        superseded_branch_closure_ids.join(", ")
    };
    Ok(format!(
        "# Branch Closure Result\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Contract Identity:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed State ID:** {}\n**Effective Reviewed Branch Surface:** {}\n**Source Task Closure IDs:** {}\n**Provenance Basis:** {}\n**Closure Status:** current\n**Superseded Branch Closure IDs:** {}\n**Branch Closure ID:** {}\n**Generated By:** featureforge:record-branch-closure\n**Generated At:** {generated_at}\n\n## Summary\n- current reviewed branch state recorded for late-stage binding.\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        reviewed_state.contract_identity,
        context.runtime.branch_name,
        context.runtime.repo_slug,
        reviewed_state.base_branch,
        current_head,
        reviewed_state.reviewed_state_id,
        reviewed_state.effective_reviewed_branch_surface,
        source_task_closure_ids,
        reviewed_state.provenance_basis,
        superseded_branch_closure_ids,
        branch_closure_id
    ))
}

fn render_release_readiness_artifact(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    result: &str,
    summary: &str,
) -> Result<String, JsonFailure> {
    let base_branch = resolve_release_base_branch(&context.runtime.git_dir, &context.runtime.branch_name)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ReleaseArtifactNotFresh,
                "advance-late-stage release-readiness requires a resolvable base branch.",
            )
        })?;
    let current_head = current_head_sha(&context.runtime.repo_root)?;
    let generated_at = Timestamp::now().to_string();
    let artifact_result = if result == "ready" { "pass" } else { "blocked" };
    let source = format!(
        "# Release Readiness Result\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Result:** {}\n**Generated By:** featureforge:document-release\n**Generated At:** {generated_at}\n\n## Summary\n- {}\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        context.runtime.branch_name,
        runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        artifact_result,
        summary
    );
    write_project_artifact(
        runtime,
        &format!("featureforge-{}-release-readiness-{}.md", runtime.safe_branch, timestamp_slug()),
        &source,
    )?;
    Ok(source)
}

fn render_final_review_artifacts(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    inputs: FinalReviewArtifactInputs<'_>,
) -> Result<String, JsonFailure> {
    let base_branch = resolve_release_base_branch(&context.runtime.git_dir, &context.runtime.branch_name)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ReviewArtifactNotFresh,
                "advance-late-stage final-review requires a resolvable base branch.",
            )
        })?;
    let current_head = current_head_sha(&context.runtime.repo_root)?;
    let strategy_checkpoint_fingerprint =
        authoritative_strategy_checkpoint_fingerprint_checked(context)?.unwrap_or_default();
    let generated_at = Timestamp::now().to_string();
    let reviewer_source_text = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {}\n**Reviewer ID:** {}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Result:** {}\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** {generated_at}\n\n## Summary\n- {}\n",
        inputs.reviewer_source,
        inputs.reviewer_id,
        context.plan_rel,
        context.plan_document.plan_revision,
        context.runtime.branch_name,
        runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        inputs.result,
        inputs.summary
    );
    let reviewer_artifact_path = write_project_artifact(
        runtime,
        &format!(
            "featureforge-{}-independent-review-{}.md",
            runtime.safe_branch,
            timestamp_slug()
        ),
        &reviewer_source_text,
    )?;
    let reviewer_artifact_fingerprint = sha256_hex(
        &fs::read(&reviewer_artifact_path).map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!(
                    "Could not read dedicated reviewer artifact {}: {error}",
                    reviewer_artifact_path.display()
                ),
            )
        })?,
    );
    let final_review_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {}\n**Reviewer ID:** {}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Dispatch ID:** {}\n**Result:** {}\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** {generated_at}\n\n## Summary\n- {}\n",
        inputs.reviewer_source,
        inputs.reviewer_id,
        reviewer_artifact_path.display(),
        reviewer_artifact_fingerprint,
        context.plan_rel,
        context.plan_document.plan_revision,
        context.runtime.branch_name,
        runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        inputs.dispatch_id,
        inputs.result,
        inputs.summary
    );
    write_project_artifact(
        runtime,
        &format!("featureforge-{}-code-review-{}.md", runtime.safe_branch, timestamp_slug()),
        &final_review_source,
    )?;
    Ok(final_review_source)
}

fn render_qa_artifact(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    result: &str,
    summary: &str,
    test_plan_path: Option<&Path>,
) -> Result<String, JsonFailure> {
    let current_head = current_head_sha(&context.runtime.repo_root)?;
    let generated_at = Timestamp::now().to_string();
    let base_branch = resolve_release_base_branch(&context.runtime.git_dir, &context.runtime.branch_name)
        .unwrap_or_else(|| String::from("unknown"));
    let source_test_plan_header = test_plan_path.map_or(String::new(), |path| {
        format!("**Source Test Plan:** `{}`\n", path.display())
    });
    let source = format!(
        "# QA Result\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n{}**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Result:** {}\n**Generated By:** featureforge:qa-only\n**Generated At:** {generated_at}\n\n## Summary\n- {}\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        source_test_plan_header,
        context.runtime.branch_name,
        runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        result,
        summary
    );
    write_project_artifact(
        runtime,
        &format!("featureforge-{}-test-outcome-{}.md", runtime.safe_branch, timestamp_slug()),
        &source,
    )?;
    Ok(source)
}

fn timestamp_slug() -> String {
    Timestamp::now()
        .to_string()
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect()
}

fn rebuild_downstream_truth_stale(message: impl Into<String>) -> JsonFailure {
    JsonFailure::new(FailureClass::StaleProvenance, message.into())
}

fn rewrite_branch_final_review_artifacts(
    review_path: &Path,
    reviewer_artifact_path: &Path,
    current_head: &str,
    strategy_checkpoint_fingerprint: &str,
) -> Result<(), JsonFailure> {
    let _ = (review_path, reviewer_artifact_path, current_head, strategy_checkpoint_fingerprint);
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical final-review proof in place",
    ))
}

fn rewrite_branch_head_bound_artifact(path: &Path, current_head: &str) -> Result<(), JsonFailure> {
    let _ = (path, current_head);
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical head-bound artifacts in place",
    ))
}

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

fn resolve_rebuild_reviewer_artifact_path(
    review_receipt_path: &Path,
    raw_reviewer_artifact_path: Option<&str>,
) -> Option<PathBuf> {
    let raw_reviewer_artifact_path = raw_reviewer_artifact_path
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let reviewer_artifact_path = PathBuf::from(raw_reviewer_artifact_path.trim_matches('`'));
    Some(if reviewer_artifact_path.is_absolute() {
        reviewer_artifact_path
    } else {
        review_receipt_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(reviewer_artifact_path)
    })
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
        if refresh.restore_missing_dispatch_lineage {
            authoritative_state.ensure_task_review_dispatch_lineage(context, refresh.task)?;
        } else {
            authoritative_state.refresh_task_review_dispatch_lineage(context, refresh.task)?;
        }
        authoritative_state.persist_if_dirty_with_failpoint(None)?;
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
    let reviewer_source = existing_unit_review_reviewer_source(
        runtime,
        execution_run_id,
        &execution_unit_id,
    )
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
        let Some(reconcile_result_proof_fingerprint) = reconcile_result_proof_fingerprint_for_review(
            &context.runtime.repo_root,
            reviewed_checkpoint_sha,
        ) else {
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
            context.plan_rel,
            context.plan_document.plan_revision,
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
        let Some(attempt) = latest_attempt_for_step(&context.evidence, task, step_state.step_number)
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

fn verify_command_process(repo_root: &Path, verify_command: &str) -> Command {
    let (program, args) = verify_command_launcher(verify_command);
    let mut command = Command::new(program);
    command.args(args).current_dir(repo_root);
    command
}

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
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["cat-file", "commit", reconcile_result_commit_sha])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let object = String::from_utf8(output.stdout).ok()?;
    Some(sha256_hex(object.as_bytes()))
}

fn is_rebuild_task_boundary_receipt_failure(message: &str) -> bool {
    matches!(
        message.split_once(':').map(|(reason_code, _)| reason_code.trim()),
        Some(
            "prior_task_review_dispatch_missing"
                | "prior_task_review_dispatch_stale"
                | "prior_task_review_not_green"
                | "task_review_not_independent"
                | "task_review_receipt_malformed"
                | "prior_task_verification_missing"
                | "prior_task_verification_missing_legacy"
                | "task_verification_receipt_malformed"
        )
    )
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

fn summarize_verify_result(output: &std::process::Output, no_output: bool) -> String {
    let exit_code = output.status.code().unwrap_or(1);
    let stdout = normalize_whitespace(&String::from_utf8_lossy(&output.stdout));
    let stderr = normalize_whitespace(&String::from_utf8_lossy(&output.stderr));
    let detail = if no_output {
        String::new()
    } else {
        let text = if !stdout.is_empty() { stdout } else { stderr };
        if text.is_empty() {
            String::new()
        } else {
            format!(": {text}")
        }
    };
    if output.status.success() {
        format!("passed{detail}")
    } else {
        format!("failed (exit {exit_code}){detail}")
    }
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
    let repo = gix::discover(repo_root).map_err(|error| {
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
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::{
        normalized_late_stage_surface, path_matches_late_stage_surface,
        rewrite_branch_final_review_artifacts, rewrite_branch_head_bound_artifact,
        rewrite_branch_qa_artifact, superseded_branch_closure_ids_from_previous_current,
        verify_command_launcher,
    };
    use crate::diagnostics::FailureClass;
    use crate::execution::leases::StatusAuthoritativeOverlay;

    #[test]
    fn verify_command_launcher_matches_platform_contract() {
        let (program, args) = verify_command_launcher("printf rebuilt");
        if cfg!(windows) {
            assert_eq!(program, "cmd");
            assert_eq!(args, vec![String::from("/C"), String::from("printf rebuilt")]);
        } else {
            assert_eq!(program, "sh");
            assert_eq!(args, vec![String::from("-lc"), String::from("printf rebuilt")]);
        }
    }

    #[test]
    fn rewrite_branch_final_review_artifacts_refuses_to_rebind_review_history() {
        let tempdir = TempDir::new().expect("tempdir should exist");
        let reviewer_artifact = tempdir.path().join("reviewer.md");
        let review_receipt = tempdir.path().join("review.md");
        let original_reviewer = "**Strategy Checkpoint Fingerprint:** old-checkpoint\n**Head SHA:** old-head\n";
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
            fs::read_to_string(&reviewer_artifact).expect("reviewer artifact should remain readable"),
            original_reviewer
        );
        assert_eq!(
            fs::read_to_string(&review_receipt).expect("review receipt should remain readable"),
            original_review
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
        for invalid in ["/README.md", "../README.md", "docs/*.md", "docs/?", "docs/[a]", "docs/{a}"] {
            let error = normalized_late_stage_surface(&format!("**Late-Stage Surface:** {invalid}\n"))
                .expect_err("invalid Late-Stage Surface entries must fail closed");
            assert_eq!(error.error_class, FailureClass::InvalidCommandInput.as_str());
        }
    }

    #[test]
    fn path_matches_late_stage_surface_distinguishes_file_and_directory_entries() {
        assert!(path_matches_late_stage_surface(
            "docs/release.md",
            &[String::from("docs/")]
        ));
        assert!(path_matches_late_stage_surface("docs", &[String::from("docs/")]));
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
}
