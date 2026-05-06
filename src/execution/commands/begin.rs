use super::common::*;

pub fn begin(
    runtime: &ExecutionRuntime,
    args: &BeginArgs,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let request = normalize_begin_request(args);
    let mut context = load_execution_context_for_mutation(runtime, &args.plan)?;
    validate_expected_fingerprint(&context, &request.expect_execution_fingerprint)?;

    let requested_execution_mode = request.execution_mode.clone();
    let execution_mode_to_persist = match context.plan_document.execution_mode.as_str() {
        "none" => match request.execution_mode.as_deref() {
            Some("featureforge:executing-plans" | "featureforge:subagent-driven-development") => {
                requested_execution_mode
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
            None
        }
    };
    let begin_status = public_status_from_supplied_context_with_shared_routing(&context, false)?;
    if let Some(failure) =
        crate::execution::implementation_gate::pre_execution_plan_fidelity_failure(&begin_status)
    {
        return Err(failure);
    }
    if let Some(execution_mode) = execution_mode_to_persist {
        context.plan_document.execution_mode = execution_mode;
    }
    let preflight_persistence_required =
        public_intent_preflight_persistence_required(&context, "begin")?;
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state =
        Some(load_or_initialize_authoritative_transition_state(&context)?);
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Begin)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Begin,
        request.task,
        request.step,
    )?;

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
                        expect_execution_fingerprint: Some(
                            request.expect_execution_fingerprint.clone(),
                        ),
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
                persist_authoritative_state_without_rollback(authoritative_state, "begin")?;
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

    let step_index = step_index(&context, request.task, request.step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested task/step does not exist in the approved plan.",
        )
    })?;
    require_public_mutation(
        &begin_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Begin,
            task: Some(request.task),
            step: Some(request.step),
            expect_execution_fingerprint: Some(request.expect_execution_fingerprint.clone()),
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

    if preflight_persistence_required {
        persist_allowed_public_begin_preflight(&context)?;
    }
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
        authoritative_state.set_harness_phase_executing()?;
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
    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}
