use super::*;
use crate::execution::closure_diagnostics::apply_task_boundary_projection_diagnostics;
use crate::execution::public_repair_targets::public_repair_target_warning_codes;

fn project_routing_decision_onto_status(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    routing: &ExecutionRoutingState,
    route_decision: &RouteDecision,
    _require_exact_execution_command: bool,
    authoritative_stale_target: Option<
        crate::execution::next_action::AuthoritativeStaleReentryTarget<'_>,
    >,
) {
    status.phase = Some(route_decision.phase.clone());
    status.harness_phase = if status.execution_started == "no"
        && matches!(status.harness_phase, HarnessPhase::ImplementationHandoff)
    {
        status.harness_phase
    } else {
        match route_decision.phase.as_str() {
            phase::PHASE_DOCUMENT_RELEASE_PENDING => HarnessPhase::DocumentReleasePending,
            phase::PHASE_FINAL_REVIEW_PENDING => HarnessPhase::FinalReviewPending,
            phase::PHASE_QA_PENDING => HarnessPhase::QaPending,
            phase::PHASE_READY_FOR_BRANCH_COMPLETION => HarnessPhase::ReadyForBranchCompletion,
            phase::PHASE_PIVOT_REQUIRED => HarnessPhase::PivotRequired,
            phase::PHASE_HANDOFF_REQUIRED => HarnessPhase::HandoffRequired,
            phase::PHASE_EXECUTING | phase::PHASE_TASK_CLOSURE_PENDING => HarnessPhase::Executing,
            _ => status.harness_phase,
        }
    };
    status.phase_detail = route_decision.phase_detail.clone();
    status.review_state_status = route_decision.review_state_status.clone();
    status.recording_context =
        route_decision
            .recording_context
            .as_ref()
            .map(|context| PublicRecordingContext {
                task_number: context.task_number,
                dispatch_id: context.dispatch_id.clone(),
                branch_closure_id: context.branch_closure_id.clone(),
            });
    status.execution_command_context =
        routing
            .execution_command_context
            .as_ref()
            .map(|context| PublicExecutionCommandContext {
                command_kind: context.command_kind.clone(),
                task_number: context.task_number,
                step_id: context.step_id,
            });
    status.next_action = route_decision.next_action.clone();
    status.recommended_public_command = route_decision.recommended_public_command.clone();
    status.recommended_public_command_argv = route_decision.public_command_argv();
    status.required_inputs = route_decision.required_inputs.clone();
    status.recommended_command = route_decision.recommended_command.clone();
    status.blocking_task = routing.blocking_task;
    status.blocking_scope = routing.blocking_scope.clone();
    status.external_wait_state = routing.external_wait_state.clone();
    status.blocking_reason_codes = routing.blocking_reason_codes.clone();
    if TargetlessStaleReconcile::from_phase_and_reason_codes(
        &status.phase_detail,
        &status.blocking_reason_codes,
    )
    .is_some()
    {
        TargetlessStaleReconcile::ensure_status_diagnostic(status);
    } else {
        TargetlessStaleReconcile::clear_status_diagnostic(status);
    }
    apply_task_boundary_projection_diagnostics(status);
    let public_execution_reentry_target = (route_decision.phase_detail
        == phase::DETAIL_EXECUTION_REENTRY_REQUIRED)
        .then(|| {
            execution_reentry_target(
                context,
                status,
                &context.plan_rel,
                crate::execution::next_action::NextActionAuthorityInputs {
                    authoritative_stale_target,
                    ..crate::execution::next_action::NextActionAuthorityInputs::default()
                },
            )
        })
        .flatten();
    status.execution_reentry_target_source = public_execution_reentry_target
        .as_ref()
        .map(|target| target.source.as_str().to_owned());
    status.public_repair_targets.clear();
    if route_decision.phase_detail
        == phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        && route_decision.review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
    {
        status.blocking_task = None;
        status.blocking_scope = Some(String::from("branch"));
        status.blocking_records = vec![StatusBlockingRecord {
            code: String::from("missing_current_closure"),
            scope_type: String::from("branch"),
            scope_key: String::from("current"),
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status: String::from("missing_current_closure"),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "An authoritative current branch closure record is required before late-stage progression can continue.",
            ),
        }];
    }
    if route_decision.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && let Some(task_number) = status
            .execution_command_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        status.blocking_scope = Some(String::from("task"));
        status.blocking_task = Some(task_number);
    }
}

fn project_public_repair_target_warning_codes(
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) {
    for warning_code in public_repair_target_warning_codes(authoritative_state) {
        if !status
            .warning_codes
            .iter()
            .any(|existing| existing == warning_code)
        {
            status.warning_codes.push(warning_code.to_owned());
        }
    }
}

pub(crate) fn apply_shared_routing_projection_to_read_scope(
    _runtime: &ExecutionRuntime,
    read_scope: &mut ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(), JsonFailure> {
    apply_shared_routing_projection_to_read_scope_with_routing(
        read_scope,
        external_review_result_ready,
        require_exact_execution_command,
    )?;
    Ok(())
}

pub(crate) fn apply_shared_routing_projection_to_read_scope_with_routing(
    read_scope: &mut ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    let (routing, route_decision, runtime_state) =
        crate::execution::router::project_runtime_routing_state_with_reduced_state(
            read_scope,
            external_review_result_ready,
            require_exact_execution_command,
        )?;
    let authoritative_stale_target = select_authoritative_stale_reentry_target(
        &read_scope.status,
        &runtime_state.gate_snapshot.stale_targets,
    );
    project_routing_decision_onto_status(
        &read_scope.context,
        &mut read_scope.status,
        &routing,
        &route_decision,
        require_exact_execution_command,
        authoritative_stale_target,
    );
    project_stale_unreviewed_closures(&mut read_scope.status, &runtime_state.gate_snapshot);
    project_public_repair_target_warning_codes(
        &mut read_scope.status,
        read_scope.authoritative_state.as_ref(),
    );
    let fallback_gate_finish;
    let gate_finish = match runtime_state.gate_snapshot.gate_finish.as_ref() {
        Some(gate_finish) => gate_finish,
        None => {
            fallback_gate_finish = GateState::default().finish();
            &fallback_gate_finish
        }
    };
    read_scope.status.blocking_records =
        compute_status_blocking_records(&read_scope.context, &read_scope.status, gate_finish)?;
    if read_scope.status.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && read_scope.status.blocking_task.is_none()
        && let Some(task_number) = projected_earliest_stale_task_from_status(&read_scope.status)
    {
        read_scope.status.blocking_scope = Some(String::from("task"));
        read_scope.status.blocking_task = Some(task_number);
    }
    project_reducer_stale_target_source(&runtime_state, &mut read_scope.status);
    let route_decision = route_decision_with_status_blockers(
        route_decision,
        &read_scope.status,
        &runtime_state.route_repair_target_candidates,
    );
    read_scope.status.state_kind = route_decision.state_kind.clone();
    read_scope.status.recommended_public_command =
        route_decision.recommended_public_command.clone();
    read_scope.status.recommended_public_command_argv = route_decision.public_command_argv();
    read_scope.status.required_inputs = route_decision.required_inputs.clone();
    read_scope.status.recommended_command = route_decision.recommended_command.clone();
    read_scope.status.next_public_action = route_decision.next_public_action.clone();
    read_scope.status.blockers = route_decision.blockers.clone();
    read_scope.status.public_repair_targets = route_decision.public_repair_targets.clone();
    read_scope.status.semantic_workspace_tree_id = runtime_state
        .semantic_workspace
        .semantic_workspace_tree_id
        .clone();
    read_scope.status.raw_workspace_tree_id = Some(
        runtime_state
            .semantic_workspace
            .raw_workspace_tree_id
            .clone(),
    );
    if require_exact_execution_command {
        require_public_execution_command_route_target(&read_scope.context, &read_scope.status)?;
    }
    read_scope.runtime_state = Some(runtime_state);
    Ok((routing, route_decision))
}

fn project_reducer_stale_target_source(
    runtime_state: &RuntimeState,
    status: &mut PlanExecutionStatus,
) {
    let Some(blocking_task) = status.blocking_task else {
        return;
    };
    let Some(stale_target) = select_authoritative_stale_reentry_target(
        status,
        &runtime_state.gate_snapshot.stale_targets,
    ) else {
        if status.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            && status.reason_codes.iter().any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "prior_task_current_closure_missing" | "task_closure_baseline_repair_candidate"
                )
            })
        {
            status.execution_reentry_target_source = Some(String::from("baseline_bridge"));
        }
        return;
    };
    if stale_target.task != blocking_task {
        return;
    }
    let execution_reentry_target_source = match stale_target.source.as_str() {
        "closure_graph" => "closure_graph_stale_target",
        source => source,
    };
    status.execution_reentry_target_source = Some(execution_reentry_target_source.to_owned());
}
