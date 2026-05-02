use super::*;

fn project_routing_decision_onto_status(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    routing: &ExecutionRoutingState,
    route_decision: &RouteDecision,
    require_exact_execution_command: bool,
    authoritative_stale_target: Option<
        crate::execution::next_action::AuthoritativeStaleReentryTarget<'_>,
    >,
) {
    if !require_exact_execution_command
        && should_preserve_local_preflight_route(status, route_decision)
    {
        status.phase = Some(String::from(phase::PHASE_EXECUTION_PREFLIGHT));
        status.phase_detail = String::from(phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED);
        status.review_state_status = route_decision.review_state_status.clone();
        status.recording_context = None;
        status.execution_command_context = None;
        status.execution_reentry_target_source = None;
        status.public_repair_targets.clear();
        status.next_action = String::from("execution preflight");
        status.recommended_command = None;
        status.blocking_task = None;
        status.blocking_scope = None;
        status.external_wait_state = None;
        status.blocking_reason_codes.clear();
        status.projection_diagnostics.clear();
        return;
    }
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
        routing
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
    status.recommended_public_command_argv =
        recommended_public_command_argv(status.recommended_public_command.as_ref());
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
    status.projection_diagnostics = public_task_boundary_decision(status).diagnostic_reason_codes;
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
    status.public_repair_targets = public_execution_reentry_target
        .map(|target| {
            vec![PublicRepairTarget {
                command_kind: String::from("reopen"),
                task: Some(target.task),
                step: target.step,
                reason_code: target.reason_code,
                source_record_id: target
                    .source_record_id
                    .or_else(|| Some(target.source.as_str().to_owned())),
                expires_when_fingerprint_changes: true,
            }]
        })
        .unwrap_or_default();
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

pub(crate) fn project_persisted_public_repair_targets(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    source_route_decision_hash: Option<&str>,
) {
    let Some(authoritative_state) = authoritative_state else {
        return;
    };
    if legacy_repair_follow_up_unbound(Some(authoritative_state)) {
        push_status_warning_code_once(status, "legacy_follow_up_unbound");
    }
    let persisted_follow_up_record =
        resolve_actionable_repair_follow_up_for_status_with_source_hash(
            context,
            status,
            Some(authoritative_state),
            source_route_decision_hash,
        );
    let persisted_follow_up = persisted_follow_up_record
        .as_ref()
        .map(|record| record.kind.public_token());
    if let Some(follow_up) = persisted_follow_up {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("repair-review-state"),
                task: persisted_follow_up_record
                    .as_ref()
                    .and_then(|record| record.target_task),
                step: persisted_follow_up_record
                    .as_ref()
                    .and_then(|record| record.target_step),
                reason_code: format!("persisted_review_state_repair_follow_up:{follow_up}"),
                source_record_id: persisted_follow_up_record
                    .as_ref()
                    .and_then(|record| record.target_record_id.clone())
                    .or_else(|| Some(format!("review_state_repair_follow_up:{follow_up}"))),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    for target in authoritative_state.explicit_reopen_repair_targets() {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("reopen"),
                task: Some(target.task),
                step: Some(target.step),
                reason_code: String::from("explicit_reopen_repair_target"),
                source_record_id: target
                    .target_record_id
                    .or_else(|| Some(execution_step_repair_target_id(target.task, target.step))),
                expires_when_fingerprint_changes: target.expires_on_plan_fingerprint_change,
            },
        );
    }
    for record in authoritative_state
        .current_task_closure_results()
        .into_values()
    {
        if current_task_closure_postconditions_would_mutate(
            authoritative_state,
            record.task,
            &record.closure_record_id,
            &record.reviewed_state_id,
        ) {
            push_public_repair_target_once(
                status,
                PublicRepairTarget {
                    command_kind: String::from("close-current-task"),
                    task: Some(record.task),
                    step: None,
                    reason_code: String::from("authoritative_task_closure_postcondition_cleanup"),
                    source_record_id: Some(record.closure_record_id),
                    expires_when_fingerprint_changes: true,
                },
            );
        }
    }
    for task in context.tasks_by_number.keys().copied() {
        if authoritative_state
            .current_task_closure_result(task)
            .is_some()
            || authoritative_state.task_review_dispatch_id(task).is_none()
            || !context
                .steps
                .iter()
                .filter(|step| step.task_number == task)
                .all(|step| step.checked)
        {
            continue;
        }
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("task_review_dispatch_closure_ready"),
                source_record_id: Some(format!("task-review-dispatch:task-{task}")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    if authoritative_state.execution_run_id_opt().is_some()
        && load_preflight_acceptance(&context.runtime).is_err()
    {
        for entry in authoritative_state
            .raw_current_task_closure_state_entries()
            .into_iter()
            .filter(|entry| entry.task.is_some())
        {
            push_public_repair_target_once(
                status,
                PublicRepairTarget {
                    command_kind: String::from("close-current-task"),
                    task: entry.task,
                    step: None,
                    reason_code: String::from("authoritative_preflight_recovery_task_closure"),
                    source_record_id: entry.closure_record_id,
                    expires_when_fingerprint_changes: true,
                },
            );
        }
    }
    if persisted_follow_up != Some("execution_reentry") {
        return;
    }
    let Some(record) = persisted_follow_up_record.as_ref() else {
        return;
    };
    let Some(task) = record.target_task else {
        return;
    };
    let Some(step) = record.target_step else {
        return;
    };
    let target = PublicRepairTarget {
        command_kind: String::from("reopen"),
        task: Some(task),
        step: Some(step),
        reason_code: String::from("persisted_execution_reentry_follow_up"),
        source_record_id: record
            .target_record_id
            .clone()
            .or_else(|| Some(format!("review_state_repair_follow_up_task:{task}"))),
        expires_when_fingerprint_changes: true,
    };
    push_public_repair_target_once(status, target);
}

fn push_public_repair_target_once(status: &mut PlanExecutionStatus, target: PublicRepairTarget) {
    if !status.public_repair_targets.iter().any(|existing| {
        existing.command_kind == target.command_kind
            && existing.task == target.task
            && existing.step == target.step
    }) {
        status.public_repair_targets.push(target);
    }
}

fn explicit_public_target_allowed(status: &PlanExecutionStatus) -> bool {
    status.phase_detail != phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        && status.state_kind != phase::DETAIL_BLOCKED_RUNTIME_BUG
}

fn recommended_public_command_is(status: &PlanExecutionStatus, kind: PublicCommandKind) -> bool {
    status
        .recommended_public_command
        .as_ref()
        .is_some_and(|command| command.kind() == kind)
}

fn route_exposes_repair_review_state_target(status: &PlanExecutionStatus) -> bool {
    recommended_public_command_is(status, PublicCommandKind::RepairReviewState)
        || status.review_state_status != "clean"
        || matches!(
            status.phase_detail.as_str(),
            phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                | phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
                | phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
                | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
                | phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        )
        || (status.phase_detail == phase::DETAIL_FINISH_COMPLETION_GATE_READY
            && status.state_kind == "terminal")
        || status.blocking_reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_review_dispatch_stale"
                    | "stale_provenance"
                    | "task_closure_baseline_repair_candidate"
            )
        })
}

fn project_public_route_mutation_targets(status: &mut PlanExecutionStatus) {
    if status.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && let Some(task) = status
            .recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("route_task_closure_recording_ready"),
                source_record_id: Some(String::from("route_decision:task_closure_recording_ready")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    let route_exposes_task_closure_repair = status.phase_detail
        == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && status.blocking_reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_review_dispatch_stale"
                    | "task_closure_baseline_repair_candidate"
            )
        });
    let repair_review_state_target_allowed = explicit_public_target_allowed(status)
        || status.phase_detail == phase::DETAIL_RUNTIME_RECONCILE_REQUIRED;
    if (route_exposes_task_closure_repair || route_exposes_repair_review_state_target(status))
        && repair_review_state_target_allowed
    {
        let reason_code = if route_exposes_task_closure_repair {
            "route_task_closure_repair_state_refresh"
        } else {
            "route_repair_review_state_available"
        };
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("repair-review-state"),
                task: None,
                step: None,
                reason_code: String::from(reason_code),
                source_record_id: Some(format!("route_decision:{}", status.phase_detail)),
                expires_when_fingerprint_changes: true,
            },
        );
    }

    let recommended_advance =
        recommended_public_command_is(status, PublicCommandKind::AdvanceLateStage);
    if (recommended_advance || status.phase_detail == phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING)
        && explicit_public_target_allowed(status)
    {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("advance-late-stage"),
                task: None,
                step: None,
                reason_code: String::from("route_advance_late_stage_ready"),
                source_record_id: Some(String::from("route_decision:advance_late_stage")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn should_preserve_local_preflight_route(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
) -> bool {
    status.execution_started == "no"
        && route_decision.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && route_decision.review_state_status == "clean"
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.current_task_closures.is_empty()
        && status.reason_codes.is_empty()
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
    project_persisted_public_repair_targets(
        &read_scope.context,
        &mut read_scope.status,
        read_scope.authoritative_state.as_ref(),
        None,
    );
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
    let source_route_decision_hash = repair_follow_up_source_decision_hash(&route_decision);
    project_persisted_public_repair_targets(
        &read_scope.context,
        &mut read_scope.status,
        read_scope.authoritative_state.as_ref(),
        source_route_decision_hash.as_deref(),
    );
    project_stale_unreviewed_closures(&mut read_scope.status, &runtime_state.gate_snapshot);
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
    let route_decision = route_decision_with_status_blockers(route_decision, &read_scope.status);
    read_scope.status.state_kind = route_decision.state_kind.clone();
    read_scope.status.recommended_public_command =
        route_decision.recommended_public_command.clone();
    read_scope.status.recommended_public_command_argv =
        recommended_public_command_argv(read_scope.status.recommended_public_command.as_ref());
    read_scope.status.recommended_command = route_decision.recommended_command.clone();
    read_scope.status.next_public_action = route_decision.next_public_action.clone();
    read_scope.status.blockers = route_decision.blockers.clone();
    project_public_route_mutation_targets(&mut read_scope.status);
    project_reducer_stale_target_source(&runtime_state, &mut read_scope.status);
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
        require_public_exact_execution_command(&read_scope.context, &read_scope.status)?;
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
    for target in &mut status.public_repair_targets {
        if target.task == Some(blocking_task) && target.command_kind == "reopen" {
            target.reason_code = stale_target.reason_code.to_owned();
            target.source_record_id = stale_target
                .source_record_id
                .map(str::to_owned)
                .or_else(|| Some(stale_target.source.as_str().to_owned()));
        }
    }
}
