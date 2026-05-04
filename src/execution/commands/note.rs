use super::common::*;

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
    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}
