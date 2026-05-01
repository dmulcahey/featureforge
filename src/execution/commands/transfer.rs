use super::common::*;
use crate::execution::handoff::{
    WorkflowTransferRecordIdentity, WorkflowTransferRecordInput,
    current_workflow_transfer_record_path, latest_matching_workflow_transfer_request_record,
    write_workflow_transfer_record,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
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
    let transfer_status = public_status_from_context_with_shared_routing(runtime, &context, false)?;
    require_public_mutation(
        &transfer_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Transfer,
            task: Some(repair_task),
            step: Some(repair_step),
            expect_execution_fingerprint: Some(expect_execution_fingerprint.to_owned()),
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

    let reloaded = load_execution_context_for_mutation(runtime, plan)?;
    status_with_shared_routing_or_context(runtime, plan, &reloaded)
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
            expect_execution_fingerprint: None,
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
    let operator_routes_handoff = operator.phase_detail
        == crate::execution::phase::DETAIL_HANDOFF_RECORDING_REQUIRED
        && matches!(
            operator.phase.as_str(),
            crate::execution::phase::PHASE_HANDOFF_REQUIRED
                | crate::execution::phase::PHASE_EXECUTING
        );
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
        record_runtime_handoff_checkpoint_and_persist(
            authoritative_state,
            &existing_record,
            &existing_fingerprint,
        )?;
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
    record_runtime_handoff_checkpoint_and_persist(
        authoritative_state,
        &record_path,
        &record_fingerprint,
    )?;

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

fn record_runtime_handoff_checkpoint_and_persist(
    authoritative_state: &mut AuthoritativeTransitionState,
    record_path: &Path,
    record_fingerprint: &str,
) -> Result<(), JsonFailure> {
    authoritative_state.record_runtime_handoff_checkpoint(
        &record_path.display().to_string(),
        record_fingerprint,
    )?;
    persist_authoritative_state_without_rollback(authoritative_state, "transfer")
}
