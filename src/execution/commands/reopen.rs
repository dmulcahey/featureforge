use super::common::*;

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
    let reopen_status = public_status_from_context_with_shared_routing(runtime, &context, false)?;
    require_public_mutation(
        &reopen_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Reopen,
            task: Some(request.task),
            step: Some(request.step),
            expect_execution_fingerprint: Some(request.expect_execution_fingerprint.clone()),
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

    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}
