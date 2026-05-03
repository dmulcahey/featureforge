use super::*;

pub(crate) fn apply_task_boundary_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    if status.blocking_task.is_some() {
        return;
    }
    if let Some(active_task) = status.active_task {
        if projected_earliest_stale_task_from_status(status).is_none()
            && let Some(prior_task) = prior_task_number_for_begin(context, active_task)
            && let Err(error) = require_prior_task_closure_for_begin(context, active_task)
        {
            let mut missing_current_closure_boundary = false;
            if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
                && !status
                    .reason_codes
                    .iter()
                    .any(|existing| existing == reason_code)
            {
                status.reason_codes.push(reason_code.to_owned());
                missing_current_closure_boundary =
                    reason_code == "prior_task_current_closure_missing";
            }
            status.blocking_task = Some(prior_task);
            status.blocking_step = None;
            status.active_task = None;
            status.active_step = None;
            if missing_current_closure_boundary {
                push_task_closure_recording_status_reasons(context, status, prior_task);
            }
        }
        return;
    }
    if let Some(resume_task) = status.resume_task {
        if projected_earliest_stale_task_from_status(status).is_none()
            && let Some(prior_task) = prior_task_number_for_begin(context, resume_task)
            && let Err(error) = require_prior_task_closure_for_begin(context, resume_task)
        {
            let mut missing_current_closure_boundary = false;
            if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
                && !status
                    .reason_codes
                    .iter()
                    .any(|existing| existing == reason_code)
            {
                status.reason_codes.push(reason_code.to_owned());
                missing_current_closure_boundary =
                    reason_code == "prior_task_current_closure_missing";
            }
            status.blocking_task = Some(prior_task);
            status.blocking_step = None;
            status.resume_task = None;
            status.resume_step = None;
            if missing_current_closure_boundary {
                push_task_closure_recording_status_reasons(context, status, prior_task);
            }
        }
        return;
    }
    let Some(next_unchecked_task) = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number)
    else {
        let Some(missing_task) = completed_plan_missing_current_closure_task(context, status)
        else {
            return;
        };
        let overlay = load_status_authoritative_overlay_checked(context)
            .ok()
            .and_then(|overlay| overlay);
        let stale_provenance_recovery_candidate = status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
            && !status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == "late_stage_surface_not_declared");
        if !stale_provenance_recovery_candidate
            && ((status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
                && status.harness_phase != HarnessPhase::Executing)
                || is_late_stage_phase(status.harness_phase)
                || authoritative_late_stage_rederivation_basis_present(context, status)
                || overlay
                    .as_ref()
                    .is_some_and(has_authoritative_late_stage_progress))
        {
            return;
        }
        if !stale_provenance_recovery_candidate {
            push_task_closure_recording_status_reasons(context, status, missing_task);
        }
        push_status_reason_code_once(status, "prior_task_current_closure_missing");
        status.blocking_task = Some(missing_task);
        status.blocking_step = None;
        return;
    };
    {
        let Some(prior_task) = prior_task_number_for_begin(context, next_unchecked_task) else {
            return;
        };
        let Err(error) = require_prior_task_closure_for_begin(context, next_unchecked_task) else {
            return;
        };
        let mut missing_current_closure_boundary = false;
        if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
            && !status
                .reason_codes
                .iter()
                .any(|existing| existing == reason_code)
        {
            status.reason_codes.push(reason_code.to_owned());
            missing_current_closure_boundary = reason_code == "prior_task_current_closure_missing";
        }
        status.blocking_task = Some(prior_task);
        if missing_current_closure_boundary {
            push_task_closure_recording_status_reasons(context, status, prior_task);
        }
    }
}

fn push_task_closure_recording_status_reasons(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    task: u32,
) {
    let Ok(prerequisites) = task_closure_recording_prerequisites(context, task) else {
        return;
    };
    let current_dispatch_ready = prerequisites
        .dispatch_id
        .as_deref()
        .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty());
    let baseline_candidate_present = task_closure_baseline_repair_candidate_with_stale_target(
        context,
        status,
        task,
        projected_earliest_stale_task_from_status(status),
    )
    .ok()
    .flatten()
    .is_some();
    let stale_bridge_ready =
        stale_unreviewed_allows_task_closure_baseline_bridge(context, status, task)
            .unwrap_or(false);
    if current_dispatch_ready || baseline_candidate_present {
        push_status_reason_code_once(status, "task_closure_baseline_repair_candidate");
    }
    if stale_bridge_ready {
        push_status_reason_code_once(status, "task_closure_baseline_bridge_ready");
    }
    for reason_code in task_closure_recording_status_reason_codes(
        &prerequisites.blocking_reason_codes,
        &prerequisites.diagnostic_reason_codes,
    ) {
        push_status_reason_code_once(status, &reason_code);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionReentryCurrentTaskClosureTargets {
    pub(crate) stale_tasks: Vec<u32>,
    pub(crate) structural_tasks: Vec<u32>,
    pub(crate) structural_scope_keys: Vec<String>,
}

pub(crate) fn execution_reentry_current_task_closure_targets_from_stale_tasks(
    context: &ExecutionContext,
    stale_tasks: impl IntoIterator<Item = u32>,
) -> Result<ExecutionReentryCurrentTaskClosureTargets, JsonFailure> {
    let stale_tasks = stale_tasks.into_iter().collect::<BTreeSet<_>>();
    let mut structural_tasks = BTreeSet::new();
    let mut structural_scope_keys = BTreeSet::new();
    for failure in structural_current_task_closure_failures(context)? {
        if let Some(task_number) = failure.task {
            structural_tasks.insert(task_number);
        } else {
            structural_scope_keys.insert(failure.scope_key);
        }
    }

    Ok(ExecutionReentryCurrentTaskClosureTargets {
        stale_tasks: stale_tasks.into_iter().collect(),
        structural_tasks: structural_tasks.into_iter().collect(),
        structural_scope_keys: structural_scope_keys.into_iter().collect(),
    })
}

pub(crate) struct ExecutionCommandRouteTarget {
    pub command_kind: &'static str,
    pub task_number: u32,
    pub step_id: Option<u32>,
}

pub(crate) fn resolve_execution_command_route_target(
    status: &PlanExecutionStatus,
    _plan_path: &str,
) -> Option<ExecutionCommandRouteTarget> {
    if let Some((task_number, step_id)) = status.active_task.zip(status.active_step) {
        return Some(ExecutionCommandRouteTarget {
            command_kind: "complete",
            task_number,
            step_id: Some(step_id),
        });
    }
    if let Some((task_number, step_id)) = status.resume_task.zip(status.resume_step) {
        return Some(ExecutionCommandRouteTarget {
            command_kind: "begin",
            task_number,
            step_id: Some(step_id),
        });
    }
    if let Some((task_number, step_id)) = status.blocking_task.zip(status.blocking_step) {
        return Some(ExecutionCommandRouteTarget {
            command_kind: "begin",
            task_number,
            step_id: Some(step_id),
        });
    }
    None
}

pub(crate) fn reopen_execution_command_route_target_for_task(
    context: &ExecutionContext,
    _status: &PlanExecutionStatus,
    _plan_path: &str,
    task_number: u32,
) -> Option<ExecutionCommandRouteTarget> {
    let step_id = latest_attempted_step_for_task(context, task_number).or_else(|| {
        context
            .steps
            .iter()
            .find(|step| step.task_number == task_number)
            .map(|step| step.step_number)
    })?;
    Some(ExecutionCommandRouteTarget {
        command_kind: "reopen",
        task_number,
        step_id: Some(step_id),
    })
}

pub(crate) fn recommended_execution_source(execution_mode: &str) -> &str {
    match execution_mode {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
            execution_mode
        }
        _ => "featureforge:executing-plans",
    }
}

pub(super) fn completed_plan_missing_current_closure_task(
    context: &ExecutionContext,
    _status: &PlanExecutionStatus,
) -> Option<u32> {
    if context.steps.iter().any(|step| !step.checked) {
        return None;
    }
    let current_task_closures = still_current_task_closure_records(context)
        .ok()?
        .into_iter()
        .map(|closure| closure.task)
        .collect::<BTreeSet<_>>();
    let highest_current_task_closure = current_task_closures.iter().next_back().copied();
    let mut completed_tasks = context
        .steps
        .iter()
        .filter(|step| step.checked)
        .map(|step| step.task_number)
        .collect::<Vec<_>>();
    completed_tasks.sort_unstable();
    completed_tasks.dedup();
    completed_tasks.into_iter().find(|task| {
        !current_task_closures.contains(task)
            && highest_current_task_closure.is_none_or(|current_task| *task > current_task)
    })
}

pub(crate) fn resolve_execution_command_route_target_from_context(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<ExecutionCommandRouteTarget> {
    let decision = compute_next_action_decision(context, status, plan_path)?;
    execution_command_route_target_from_decision(status, &decision, plan_path)
}

pub(crate) fn require_execution_command_route_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    context_label: &str,
) -> Result<ExecutionCommandRouteTarget, JsonFailure> {
    let command = resolve_execution_command_route_target_from_context(context, status, plan_path)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "{context_label} could not derive the exact execution command for the current execution state."
                ),
            )
        })?;
    Ok(command)
}

fn public_execution_command_route_basis_present(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    status.active_task.is_some()
        || status.active_step.is_some()
        || status.resume_task.is_some()
        || status.resume_step.is_some()
        || status.blocking_task.is_some()
        || status.blocking_step.is_some()
        || status.execution_run_id.is_some()
        || !context.evidence.attempts.is_empty()
        || !status.current_task_closures.is_empty()
        || context.steps.iter().any(|step| !step.checked)
        || status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
        || status
            .active_contract_path
            .as_ref()
            .zip(status.active_contract_fingerprint.as_ref())
            .is_some()
}

fn public_execution_command_route_required(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    let execution_context_present = (status.harness_phase == HarnessPhase::Executing
        || status.harness_phase == HarnessPhase::ExecutionPreflight
        || status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_task.is_some())
        && public_execution_command_route_basis_present(context, status);
    let execution_command_route = (status.execution_started == "yes"
        && matches!(
            status.phase_detail.as_str(),
            phase::DETAIL_EXECUTION_IN_PROGRESS
        ))
        || (status.execution_started != "yes"
            && status.harness_phase == HarnessPhase::ExecutionPreflight
            && matches!(
                status.phase_detail.as_str(),
                phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED
            ));
    execution_context_present
        && execution_command_route
        && status.review_state_status == "clean"
        && !execution_reentry_requires_review_state_repair(Some(context), status)
}

pub(crate) fn require_public_execution_command_route_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    if public_execution_command_route_required(context, status) {
        if status.execution_command_context.is_some() && status.recommended_command.is_some() {
            return Ok(());
        }
        let _ =
            require_execution_command_route_target(context, status, &context.plan_rel, "status")?;
    }
    Ok(())
}
