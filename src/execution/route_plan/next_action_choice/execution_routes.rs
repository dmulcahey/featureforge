use super::super::execution_targets::{
    ExecutionCommandRouteTarget, execution_command_route_target_has_public_authority,
    resolve_execution_command_route_target,
};
use crate::execution::closure_diagnostics::public_task_boundary_decision;
use crate::execution::command_eligibility::PublicCommandKind;
use crate::execution::current_truth::{
    late_stage_surface_not_declared_reason_code as shared_late_stage_surface_not_declared_reason_code,
    task_boundary_block_reason_code,
};
use crate::execution::gate_reason_codes::qa_requirement_missing_or_invalid_reason_code;
use crate::execution::harness::HarnessPhase;
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_PHASE_DETAIL, TargetlessStaleReconcile,
};
use crate::execution::repair_route_decision::task_closure_baseline_bridge_ready_for_task;
use crate::execution::repair_target_selection::{
    NextActionAuthorityInputs, execution_reentry_target, task_boundary_blocking_task,
};
use crate::execution::state::{
    CurrentTaskClosureBranchRouteFacts, ExecutionContext, PlanExecutionStatus,
    execution_reentry_requires_review_state_repair,
    execution_reentry_requires_review_state_repair_with_authority,
    task_scope_structural_review_state_reason,
};
use crate::execution::status_support::latest_attempted_step_for_task;

use super::late_stage_public_routes::late_stage_decision;
use super::late_stage_repair_routes::persisted_late_stage_reroute_missing_current_closure;
use super::{NextActionDecision, NextActionKind, canonical_review_state_status};

pub(super) fn execution_repair_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> NextActionDecision {
    if let Some(target) = execution_reentry_target(context, status, plan_path, authority_inputs) {
        return execution_repair_decision_for_task(
            status,
            plan_path,
            review_state_status,
            target.task,
        );
    }
    if missing_execution_reentry_target_requires_reconcile(
        status,
        review_state_status,
        authority_inputs,
    ) {
        return missing_execution_reentry_target_decision(status, review_state_status);
    }
    let mut blocking_reason_codes = status.reason_codes.clone();
    if !blocking_reason_codes
        .iter()
        .any(|reason_code| reason_code == "execution_target_missing")
    {
        blocking_reason_codes.push(String::from("execution_target_missing"));
    }
    NextActionDecision {
        kind: NextActionKind::RepairReviewState,
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: review_state_status.to_owned(),
        task_number: None,
        step_number: None,
        blocking_task: None,
        blocking_reason_codes,
    }
}

fn missing_execution_reentry_target_requires_reconcile(
    status: &PlanExecutionStatus,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> bool {
    !authority_inputs.has_authoritative_stale_target
        && TargetlessStaleReconcile::missing_reentry_target_requires_reconcile(
            status,
            review_state_status,
        )
}

pub(super) fn missing_execution_reentry_target_decision(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> NextActionDecision {
    let mut blocking_reason_codes = status.reason_codes.clone();
    TargetlessStaleReconcile::ensure_reason_codes(&mut blocking_reason_codes);
    NextActionDecision {
        kind: NextActionKind::RepairReviewState,
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL),
        review_state_status: review_state_status.to_owned(),
        task_number: None,
        step_number: None,
        blocking_task: None,
        blocking_reason_codes,
    }
}

pub(super) fn execution_repair_decision_for_task(
    status: &PlanExecutionStatus,
    _plan_path: &str,
    review_state_status: &str,
    task_number: u32,
) -> NextActionDecision {
    NextActionDecision {
        kind: NextActionKind::RepairReviewState,
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: review_state_status.to_owned(),
        task_number: Some(task_number),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: Some(task_number),
        blocking_reason_codes: status.reason_codes.clone(),
    }
}

pub(super) fn closure_prerequisite_decision(
    status: &PlanExecutionStatus,
    kind: NextActionKind,
    phase_detail: &str,
    task_number: Option<u32>,
    step_number: Option<u32>,
) -> NextActionDecision {
    NextActionDecision {
        kind,
        phase: String::from(crate::execution::phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail: String::from(phase_detail),
        review_state_status: canonical_review_state_status(status),
        task_number,
        step_number,
        blocking_task: task_number.or(status.blocking_task),
        blocking_reason_codes: status.reason_codes.clone(),
    }
}

pub(super) fn task_closure_recording_ready_decision(
    status: &PlanExecutionStatus,
    plan_path: &str,
    current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
    task_number: u32,
) -> NextActionDecision {
    if current_task_closure_branch_route_facts
        .task_should_route_to_branch_closure(status, task_number)
    {
        return late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        );
    }
    NextActionDecision {
        kind: NextActionKind::CloseCurrentTask,
        phase: String::from(crate::execution::phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail: String::from(crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY),
        review_state_status: String::from("clean"),
        task_number: Some(task_number),
        step_number: None,
        blocking_task: Some(task_number),
        blocking_reason_codes: public_task_boundary_decision(status).public_reason_codes,
    }
}

pub(super) fn execution_reentry_blocking_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<u32> {
    let review_state_status = canonical_review_state_status(status);
    if persisted_late_stage_reroute_missing_current_closure(
        context,
        status,
        authority_inputs,
        review_state_status.as_str(),
    ) {
        return None;
    }
    let boundary_blocking_task = task_boundary_blocking_task(status);
    if let Some(task_number) = boundary_blocking_task
        && task_closure_baseline_bridge_ready_for_task(
            context,
            status,
            authority_inputs,
            review_state_status.as_str(),
            task_number,
        )
    {
        return None;
    }
    boundary_blocking_task.or_else(|| {
        (status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            && status.harness_phase == HarnessPhase::Executing
            && status.blocking_step.is_none()
            && status.active_task.is_none()
            && status.resume_task.is_none()
            && !(status.review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
                && authority_inputs
                    .precomputed_current_task_closure_branch_route_facts()
                    .set_has_non_branch_contributing_closure_without_branch())
            && (!status.reason_codes.is_empty() || status.review_state_status != "clean")
            && task_boundary_block_reason_code(status).is_none()
            && !status
                .reason_codes
                .iter()
                .any(|reason_code| qa_requirement_missing_or_invalid_reason_code(reason_code)))
        .then_some(status.blocking_task)
        .flatten()
    })
}

pub(super) fn completed_execution_missing_branch_closure(
    status: &PlanExecutionStatus,
    context: &ExecutionContext,
    current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
) -> bool {
    status.execution_started == "yes"
        && current_task_closure_branch_route_facts
            .all_plan_task_closures_present_without_branch_closure(
                context.plan_document.tasks.len(),
            )
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.blocking_step.is_none()
}

#[derive(Clone, Copy)]
pub(super) struct ExecutionReentryDecisionInputs<'a> {
    pub(super) current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
    pub(super) authority_inputs: NextActionAuthorityInputs<'a>,
    pub(super) stale_boundary_route: bool,
}

pub(super) fn execution_reentry_decision_for_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    task_number: u32,
    inputs: ExecutionReentryDecisionInputs<'_>,
) -> NextActionDecision {
    let current_task_closure_branch_route_facts = inputs.current_task_closure_branch_route_facts;
    let target_task_missing_current_closure_after_repair = status.blocking_task
        == Some(task_number)
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING)
        && status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID | REASON_CODE_STALE_PROVENANCE
            )
        })
        && current_task_closure_branch_route_facts
            .task_has_no_current_closure(status, task_number);
    if execution_reentry_requires_review_state_repair_with_authority(
        Some(context),
        status,
        inputs.authority_inputs.overlay,
        inputs.authority_inputs.authoritative_state,
    ) && !inputs.stale_boundary_route
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| shared_late_stage_surface_not_declared_reason_code(reason_code))
        && !target_task_missing_current_closure_after_repair
    {
        return execution_repair_decision_for_task(
            status,
            plan_path,
            review_state_status,
            task_number,
        );
    }
    if !inputs.stale_boundary_route
        && let Some(route_target) = resolve_execution_command_route_target(status, plan_path)
        && route_target.task_number == task_number
        && (!route_target.is_begin()
            || execution_command_route_target_has_public_authority(status, &route_target))
        && let Some(mut route_target_decision) =
            decision_from_execution_command_route_target(status, plan_path, Some(route_target))
    {
        route_target_decision.blocking_task =
            route_target_decision.blocking_task.or(Some(task_number));
        if route_target_decision.phase_detail
            != crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS
        {
            route_target_decision.phase_detail =
                String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
        }
        return route_target_decision;
    }
    if let Some(reopen_command) =
        reopen_execution_command_route_target_for_task(context, task_number)
        && let Some(step_id) = reopen_command.step_id
    {
        return NextActionDecision {
            kind: NextActionKind::Reopen,
            phase: String::from(crate::execution::phase::PHASE_EXECUTING),
            phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: review_state_status.to_owned(),
            task_number: Some(task_number),
            step_number: Some(step_id),
            blocking_task: Some(task_number),
            blocking_reason_codes: status.reason_codes.clone(),
        };
    }
    let mut repair_decision =
        execution_repair_decision_for_task(status, plan_path, review_state_status, task_number);
    if inputs.stale_boundary_route
        && !repair_decision
            .blocking_reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE)
    {
        repair_decision.blocking_reason_codes.push(String::from(
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
        ));
    }
    repair_decision
}

fn reopen_execution_command_route_target_for_task(
    context: &ExecutionContext,
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
        kind: PublicCommandKind::Reopen,
        task_number,
        step_id: Some(step_id),
    })
}

pub(super) fn decision_from_execution_command_route_target(
    status: &PlanExecutionStatus,
    _plan_path: &str,
    route_target: Option<ExecutionCommandRouteTarget>,
) -> Option<NextActionDecision> {
    let route_target = route_target?;
    let kind = match route_target.kind {
        PublicCommandKind::Begin => {
            if status.resume_task == Some(route_target.task_number)
                && status.resume_step == route_target.step_id
            {
                NextActionKind::Resume
            } else {
                NextActionKind::Begin
            }
        }
        PublicCommandKind::Reopen => NextActionKind::Reopen,
        PublicCommandKind::Complete => NextActionKind::CloseCurrentTask,
        _ => return None,
    };
    let phase_detail = match route_target.kind {
        PublicCommandKind::Complete => {
            String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS)
        }
        PublicCommandKind::Begin
            if status.blocking_task == Some(route_target.task_number)
                && status.blocking_step == route_target.step_id =>
        {
            String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS)
        }
        PublicCommandKind::Begin
            if status.resume_task == Some(route_target.task_number)
                && status.resume_step == route_target.step_id
                && status.harness_phase == HarnessPhase::Executing
                && execution_reentry_requires_review_state_repair(None, status) =>
        {
            String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS)
        }
        PublicCommandKind::Begin
            if status.blocking_step.is_some()
                && !execution_reentry_requires_review_state_repair(None, status) =>
        {
            String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS)
        }
        _ => String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
    };
    let phase = if phase_detail == crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS {
        String::from(crate::execution::phase::PHASE_HANDOFF_REQUIRED)
    } else {
        String::from(crate::execution::phase::PHASE_EXECUTING)
    };
    Some(NextActionDecision {
        kind,
        phase,
        phase_detail,
        review_state_status: canonical_review_state_status(status),
        task_number: Some(route_target.task_number),
        step_number: route_target.step_id,
        blocking_task: if route_target.kind == PublicCommandKind::Reopen {
            Some(route_target.task_number)
        } else {
            status.blocking_task
        },
        blocking_reason_codes: status.reason_codes.clone(),
    })
}

fn malformed_execution_markers(status: &PlanExecutionStatus) -> bool {
    (status.active_task.is_some() && status.active_step.is_none())
        || (status.active_task.is_none() && status.active_step.is_some())
        || (status.resume_task.is_some() && status.resume_step.is_none())
        || (status.resume_task.is_none() && status.resume_step.is_some())
        || (status.blocking_task.is_some() && status.blocking_step.is_some_and(|step| step == 0))
}

pub(super) fn hard_structural_corruption_detected(
    status: &PlanExecutionStatus,
    current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
) -> bool {
    let blocking_task_missing_current_closure_after_repair = status.blocking_task.is_some()
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING)
        && status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
                    | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
                    | REASON_CODE_STALE_PROVENANCE
            )
        })
        && status.blocking_task.is_some_and(|task_number| {
            current_task_closure_branch_route_facts.task_has_no_current_closure(status, task_number)
        });
    malformed_execution_markers(status)
        || (task_scope_structural_review_state_reason(status).is_some()
            && !blocking_task_missing_current_closure_after_repair)
}
