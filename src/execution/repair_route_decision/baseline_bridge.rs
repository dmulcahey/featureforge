//! Task-closure baseline bridge route facts.
//!
//! This child module owns the public route/readiness facts that allow a missing
//! current task-closure baseline to converge through `close-current-task`.
//! Callers get task/readiness answers here; low-level predicate plumbing stays
//! in `predicates` so this file remains a route surface rather than a helper
//! catch-all.

mod precedence;
mod predicates;

use precedence::baseline_bridge_authority_precedence;
pub(crate) use precedence::baseline_bridge_reducer_precedence;
use predicates::{
    baseline_bridge_candidate_present, baseline_bridge_candidate_route_reason_present,
    baseline_bridge_missing_and_candidate_reasons_present,
    closure_baseline_routing_reason_codes_compatible as closure_baseline_routing_reason_codes_compatible_impl,
    current_closure_scope_empty, execution_reentry_task_closure_bridge_route_ready,
    prior_missing_reason_present, prior_stale_reason_present,
    prior_task_closure_progress_edge_required,
    task_closure_baseline_bridge_allows_blocking_step_route_impl,
    task_closure_baseline_bridge_allows_task_review_pending_route_impl,
    task_closure_baseline_bridge_blocking_task_route_ready,
    task_closure_baseline_bridge_external_review_ready_promotes_closure_recording,
    task_closure_baseline_bridge_missing_current_task_recording_ready,
    task_closure_recording_harness,
};

use crate::diagnostics::JsonFailure;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, late_stage_missing_task_closure_baseline_bridge_supported,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::phase;
use crate::execution::repair_target_selection::{
    ExecutionReentryTarget, NextActionAuthorityInputs,
    missing_current_closure_allows_task_closure_baseline_route,
    task_closure_baseline_reentry_target_with_authority,
};
use crate::execution::resume_stale_precedence::ResumeStalePrecedence;
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN,
    authoritative_stale_target_allows_task_closure_bridge,
};
use crate::execution::state::{
    ExecutionContext, PlanExecutionStatus, PublicRepairTarget, closure_baseline_candidate_task,
    task_closure_baseline_candidate_can_preempt_stale_target,
    task_scope_review_state_repair_reason, task_scope_structural_review_state_reason,
};
use crate::execution::status_support::{
    TaskClosureBaselineRepairCandidate,
    task_closure_baseline_bridge_ready_for_stale_target_with_authority,
    task_closure_baseline_repair_candidate_with_stale_target_and_authority,
};
use crate::execution::transitions::AuthoritativeTransitionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskClosureBaselineBridgeRouteDecision {
    pub(crate) ready_for_task: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskClosureBaselineBridgeRepairReviewStateRoute {
    pub(crate) target_task: Option<u32>,
}

impl TaskClosureBaselineBridgeRouteDecision {
    pub(crate) fn for_task(
        context: &ExecutionContext,
        status: &PlanExecutionStatus,
        authority_inputs: NextActionAuthorityInputs<'_>,
        review_state_status: &str,
        task_number: u32,
    ) -> Self {
        let ready_for_task = task_closure_baseline_bridge_ready_for_task_impl(
            context,
            status,
            authority_inputs,
            review_state_status,
            task_number,
        );
        Self { ready_for_task }
    }
}

pub(crate) fn task_closure_baseline_bridge_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Result<Option<u32>, JsonFailure> {
    let Some(task_number) = status.blocking_task else {
        return Ok(None);
    };
    let has_missing_closure_reason = status.reason_codes.iter().any(|reason_code| {
        reason_code
            == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
    });
    let has_baseline_candidate_reason = status.reason_codes.iter().any(|reason_code| {
        reason_code
            == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
    });
    if !(status.current_branch_closure_id.is_none()
        && status.current_task_closures.is_empty()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && has_missing_closure_reason
        && has_baseline_candidate_reason)
    {
        return Ok(None);
    }
    if task_closure_baseline_repair_candidate_for_route(
        context,
        status,
        authority_inputs,
        task_number,
        baseline_bridge_authority_precedence(status, authority_inputs).earliest_stale_task,
    )?
    .is_none()
    {
        return Ok(None);
    }
    Ok(Some(task_number))
}

pub(crate) fn task_closure_baseline_bridge_reentry_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<ExecutionReentryTarget> {
    let precedence = baseline_bridge_authority_precedence(status, authority_inputs);
    let target = task_closure_baseline_reentry_target_with_authority(
        context,
        status,
        precedence.earliest_stale_task,
        authority_inputs,
    )?;
    (task_closure_baseline_candidate_can_preempt_stale_target(
        status,
        target.task,
        precedence.earliest_stale_task,
    ) && authority_inputs.stale_target_allows_task_closure_bridge_for_task(target.task))
    .then_some(target)
}

pub(crate) fn task_closure_baseline_bridge_ready_for_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    TaskClosureBaselineBridgeRouteDecision::for_task(
        context,
        status,
        authority_inputs,
        review_state_status,
        task_number,
    )
    .ready_for_task
}

pub(crate) fn task_closure_baseline_bridge_persisted_close_current_task_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    persisted_follow_up: Option<&str>,
) -> Option<u32> {
    if persisted_follow_up
        != Some(crate::execution::review_route_tokens::FOLLOW_UP_CLOSE_CURRENT_TASK)
        || !current_closure_scope_empty(status)
    {
        return None;
    }
    let task_number = closure_baseline_candidate_task(context)?;
    authority_inputs
        .stale_target_allows_task_closure_bridge_for_task(task_number)
        .then_some(task_number)
}

pub(crate) fn task_closure_baseline_bridge_open_step_preempted_by_closure_recording(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    missing_current_closure_allows_task_closure_baseline_route(
        context,
        status,
        authority_inputs,
        review_state_status,
    ) && prior_missing_reason_present(status)
        && task_closure_baseline_bridge_ready_for_task_impl(
            context,
            status,
            authority_inputs,
            review_state_status,
            task_number,
        )
}

pub(crate) fn task_closure_baseline_bridge_task_review_pending_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> Option<u32> {
    (task_closure_baseline_bridge_allows_task_review_pending_route_impl(status)
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
        && baseline_bridge_candidate_present(context, status, authority_inputs, task_number))
    .then_some(task_number)
}

pub(crate) fn task_closure_baseline_bridge_task_review_result_ready_promotes_recording(
    status: &PlanExecutionStatus,
    external_review_result_ready: bool,
) -> bool {
    external_review_result_ready
        && task_closure_baseline_bridge_external_review_ready_promotes_closure_recording(status)
}

pub(crate) fn task_closure_baseline_bridge_blocking_task_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> Option<u32> {
    let task_number = status.blocking_task?;
    (task_closure_baseline_bridge_blocking_task_route_ready(
        context,
        status,
        authority_inputs,
        review_state_status,
        task_number,
    ) && baseline_bridge_missing_and_candidate_reasons_present(status)
        && baseline_bridge_candidate_present(context, status, authority_inputs, task_number))
    .then_some(task_number)
}

pub(crate) fn task_closure_baseline_bridge_external_review_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    external_review_result_ready: bool,
) -> Option<u32> {
    let task_number = status.blocking_task?;
    (task_closure_baseline_bridge_task_review_result_ready_promotes_recording(
        status,
        external_review_result_ready,
    ) && status.blocking_step.is_none_or(|_| {
        task_closure_baseline_bridge_allows_blocking_step_route_impl(status, task_number)
    }) && task_closure_recording_harness(status)
        && baseline_bridge_missing_and_candidate_reasons_present(status)
        && baseline_bridge_candidate_present(context, status, authority_inputs, task_number)
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status,
        ))
    .then_some(task_number)
}

pub(crate) fn task_closure_baseline_bridge_candidate_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> Option<u32> {
    if status.blocking_task.is_some()
        || status.blocking_step.is_some()
        || !task_closure_recording_harness(status)
    {
        return None;
    }
    let candidate_task = closure_baseline_candidate_task(context)?;
    if !baseline_bridge_candidate_present(context, status, authority_inputs, candidate_task)
        || !missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
    {
        return None;
    }
    let reason_signal_route = baseline_bridge_candidate_route_reason_present(status);
    let clean_marker_free_route = status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && review_state_status == "clean"
        && closure_baseline_routing_reason_codes_compatible(status);
    (reason_signal_route || clean_marker_free_route).then_some(candidate_task)
}

pub(crate) fn task_closure_baseline_bridge_late_stage_missing_current_closure_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> Option<u32> {
    if !current_closure_scope_empty(status) {
        return None;
    }
    if let Some(task_number) = status.blocking_task
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
    {
        return Some(task_number);
    }
    let candidate_task = closure_baseline_candidate_task(context)?;
    if prior_missing_reason_present(status)
        || baseline_bridge_candidate_present(context, status, authority_inputs, candidate_task)
            && missing_current_closure_allows_task_closure_baseline_route(
                context,
                status,
                authority_inputs,
                review_state_status,
            )
    {
        return Some(candidate_task);
    }
    None
}

pub(crate) fn task_closure_baseline_bridge_missing_baseline_unsupported_route_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    assessment: &BranchRerecordingAssessment,
) -> Option<u32> {
    if !late_stage_missing_task_closure_baseline_bridge_supported(assessment) {
        return None;
    }
    let task_number = closure_baseline_candidate_task(context)?;
    baseline_bridge_candidate_present(context, status, authority_inputs, task_number)
        .then_some(task_number)
}

pub(crate) fn task_closure_baseline_bridge_allows_stale_boundary_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    let precedence = baseline_bridge_authority_precedence(status, authority_inputs);
    let stale_target_bridge_ready = task_closure_baseline_bridge_ready_for_stale_target_for_route(
        context,
        status,
        authority_inputs,
        task_number,
        precedence.earliest_stale_task,
    )
    .unwrap_or(false)
        && authority_inputs.stale_target_allows_task_closure_bridge_for_task(task_number);
    let missing_baseline_yields_stale_target =
        missing_baseline_bridge_candidate_ready_for_stale_target_with_route_authority(
            context,
            status,
            authority_inputs,
            task_number,
            precedence.earliest_stale_task,
        );

    (stale_target_bridge_ready || missing_baseline_yields_stale_target)
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
        && task_closure_baseline_bridge_ready_for_task_impl(
            context,
            status,
            authority_inputs,
            review_state_status,
            task_number,
        )
}

pub(crate) fn task_closure_baseline_bridge_stale_boundary_route_ready(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    stale_task: u32,
) -> bool {
    if task_scope_structural_review_state_reason(status).is_some() {
        return false;
    }
    task_closure_baseline_bridge_reentry_target(context, status, authority_inputs).is_some_and(
        |target| {
            target.task == stale_task
                && review_state_status == "clean"
                && status.current_task_closures.is_empty()
                && !prior_stale_reason_present(status)
        },
    ) || task_closure_baseline_bridge_missing_current_task_recording_ready(
        status,
        review_state_status,
    ) || task_closure_baseline_bridge_allows_stale_boundary_route(
        context,
        status,
        authority_inputs,
        review_state_status,
        stale_task,
    )
}

pub(crate) fn closure_baseline_routing_reason_codes_compatible(
    status: &PlanExecutionStatus,
) -> bool {
    closure_baseline_routing_reason_codes_compatible_impl(status)
}

pub(crate) fn task_closure_baseline_bridge_target_task_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    precedence: ResumeStalePrecedence,
    earliest_stale_task_bridge_allowed: bool,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: &BranchRerecordingAssessment,
) -> Result<Option<u32>, JsonFailure> {
    if status.review_state_status
        != crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && status.stale_unreviewed_closures.is_empty()
        && closure_baseline_candidate_task(context).is_none()
    {
        return Ok(None);
    }
    let baseline_candidate_task = closure_baseline_candidate_task(context);
    let earliest_stale_task = precedence.earliest_stale_task;
    let Some(stale_task) = (match (earliest_stale_task, baseline_candidate_task) {
        (Some(_), Some(candidate_task))
            if task_closure_baseline_candidate_can_preempt_stale_target(
                status,
                candidate_task,
                earliest_stale_task,
            ) =>
        {
            Some(candidate_task)
        }
        _ => earliest_stale_task
            .or(status.blocking_task)
            .or(status.active_task)
            .or(baseline_candidate_task),
    }) else {
        return Ok(None);
    };
    if earliest_stale_task == Some(stale_task) && !earliest_stale_task_bridge_allowed {
        return Ok(
            missing_baseline_bridge_candidate_for_stale_target_with_authority(
                context,
                status,
                stale_task,
                earliest_stale_task,
                overlay,
                authoritative_state,
                branch_rerecording_assessment,
            )?
            .then_some(stale_task),
        );
    }
    if !task_closure_baseline_bridge_ready_for_stale_target_with_authority(
        context,
        status,
        stale_task,
        earliest_stale_task,
        overlay,
        authoritative_state,
        branch_rerecording_assessment,
    )? {
        return Ok(None);
    }
    Ok(Some(stale_task))
}

fn non_current_stale_target_can_yield_to_missing_baseline(
    status: &PlanExecutionStatus,
    task: u32,
) -> bool {
    status.blocking_task == Some(task)
        && status.blocking_step.is_none()
        && current_closure_scope_empty(status)
        && baseline_bridge_missing_and_candidate_reasons_present(status)
}

fn missing_baseline_bridge_candidate_for_stale_target_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_stale_task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: &BranchRerecordingAssessment,
) -> Result<bool, JsonFailure> {
    if !non_current_stale_target_can_yield_to_missing_baseline(status, task) {
        return Ok(false);
    }
    Ok(
        task_closure_baseline_repair_candidate_with_stale_target_and_authority(
            context,
            status,
            task,
            earliest_stale_task,
            overlay,
            authoritative_state,
            branch_rerecording_assessment,
        )?
        .is_some(),
    )
}

fn task_closure_baseline_repair_candidate_for_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    task: u32,
    earliest_stale_task: Option<u32>,
) -> Result<Option<TaskClosureBaselineRepairCandidate>, JsonFailure> {
    let Some(assessment) = authority_inputs.branch_rerecording_assessment else {
        return Ok(None);
    };
    task_closure_baseline_repair_candidate_with_stale_target_and_authority(
        context,
        status,
        task,
        earliest_stale_task,
        authority_inputs.overlay,
        authority_inputs.authoritative_state,
        assessment,
    )
}

fn task_closure_baseline_bridge_ready_for_stale_target_for_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    task: u32,
    earliest_stale_task: Option<u32>,
) -> Result<bool, JsonFailure> {
    let Some(assessment) = authority_inputs.branch_rerecording_assessment else {
        return Ok(false);
    };
    task_closure_baseline_bridge_ready_for_stale_target_with_authority(
        context,
        status,
        task,
        earliest_stale_task,
        authority_inputs.overlay,
        authority_inputs.authoritative_state,
        assessment,
    )
}

fn missing_baseline_bridge_candidate_ready_for_stale_target_with_route_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    task: u32,
    earliest_stale_task: Option<u32>,
) -> bool {
    let Some(assessment) = authority_inputs.branch_rerecording_assessment else {
        return false;
    };
    missing_baseline_bridge_candidate_for_stale_target_with_authority(
        context,
        status,
        task,
        earliest_stale_task,
        authority_inputs.overlay,
        authority_inputs.authoritative_state,
        assessment,
    )
    .unwrap_or(false)
}

pub(crate) fn task_closure_baseline_bridge_repair_review_state_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> Option<TaskClosureBaselineBridgeRepairReviewStateRoute> {
    let precedence = baseline_bridge_authority_precedence(status, authority_inputs);
    let earliest_stale_task_bridge_allowed = authority_inputs
        .authoritative_stale_target
        .is_none_or(|target| target.task_closure_bridge_allowed);
    if let Some(assessment) = authority_inputs.branch_rerecording_assessment
        && let Ok(Some(target_task)) = task_closure_baseline_bridge_target_task_with_authority(
            context,
            status,
            precedence,
            earliest_stale_task_bridge_allowed,
            authority_inputs.overlay,
            authority_inputs.authoritative_state,
            assessment,
        )
    {
        return Some(TaskClosureBaselineBridgeRepairReviewStateRoute {
            target_task: Some(target_task),
        });
    }
    (review_state_status == "clean"
        && !status.current_task_closures.is_empty()
        && baseline_bridge_missing_and_candidate_reasons_present(status)
        && task_scope_review_state_repair_reason(status).is_none()
        && task_scope_structural_review_state_reason(status).is_none()
        && closure_baseline_routing_reason_codes_compatible(status))
    .then_some(TaskClosureBaselineBridgeRepairReviewStateRoute { target_task: None })
}

pub(crate) fn task_closure_baseline_bridge_route_ready_for_status(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    earliest_task_stale_target: Option<&AuthoritativeStaleTarget>,
) -> bool {
    if task_scope_review_state_repair_reason(status).is_some()
        || task_scope_structural_review_state_reason(status).is_some()
    {
        return false;
    }
    status.blocking_task.is_some_and(|task_number| {
        let earliest_stale_task = earliest_task_stale_target.and_then(|target| target.task);
        let missing_baseline_yields_stale_target =
            missing_baseline_bridge_candidate_ready_for_stale_target_with_route_authority(
                context,
                status,
                authority_inputs,
                task_number,
                earliest_stale_task,
            );
        if !authoritative_stale_target_allows_task_closure_bridge(
            earliest_task_stale_target,
            task_number,
        ) {
            return missing_baseline_yields_stale_target;
        }
        task_closure_baseline_bridge_ready_for_stale_target_for_route(
            context,
            status,
            authority_inputs,
            task_number,
            earliest_stale_task,
        )
        .unwrap_or(false)
            || missing_baseline_yields_stale_target
    })
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExecutionReentryTaskClosureBridgeInputs<'a> {
    pub(crate) context: &'a ExecutionContext,
    pub(crate) status: &'a PlanExecutionStatus,
    pub(crate) phase_detail: &'a str,
    pub(crate) seed_blocking_task: Option<u32>,
    pub(crate) command_context_task: Option<u32>,
    pub(crate) earliest_task_stale_target: Option<&'a AuthoritativeStaleTarget>,
    pub(crate) close_current_task_repair_targets: &'a [PublicRepairTarget],
    pub(crate) task_review_dispatch_id_present: bool,
    pub(crate) baseline_bridge_route_ready_for_blocking_task: bool,
}

pub(crate) fn execution_reentry_task_closure_bridge_route_task(
    inputs: ExecutionReentryTaskClosureBridgeInputs<'_>,
) -> Option<u32> {
    if inputs.phase_detail != phase::DETAIL_EXECUTION_REENTRY_REQUIRED {
        return None;
    }
    if let Some(task_number) = inputs
        .status
        .blocking_task
        .or(inputs.seed_blocking_task)
        .or(inputs.command_context_task)
        && !status_has_current_task_closure_for_task(inputs.status, task_number)
        && execution_reentry_task_closure_bridge_route_ready(
            inputs.context,
            inputs.earliest_task_stale_target,
            inputs.close_current_task_repair_targets,
            inputs.task_review_dispatch_id_present,
            inputs.baseline_bridge_route_ready_for_blocking_task,
            task_number,
        )
    {
        return Some(task_number);
    }
    if !inputs.status.reason_codes.iter().any(|code| {
        crate::execution::closure_diagnostics::task_boundary_current_closure_stale_reason_code(code)
    }) && prior_task_closure_progress_edge_required(inputs.status)
        && let Some(task_number) = inputs.status.blocking_task
        && authoritative_stale_target_allows_task_closure_bridge(
            inputs.earliest_task_stale_target,
            task_number,
        )
    {
        return Some(task_number);
    }
    None
}

pub(crate) fn status_has_current_task_closure_for_task(
    status: &PlanExecutionStatus,
    task_number: u32,
) -> bool {
    status
        .current_task_closures
        .iter()
        .any(|closure| closure.task == task_number)
}

fn task_closure_baseline_bridge_ready_for_task_impl(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    let precedence = baseline_bridge_authority_precedence(status, authority_inputs);
    let reducer_bridge_ready = task_closure_baseline_bridge_ready_for_stale_target_for_route(
        context,
        status,
        authority_inputs,
        task_number,
        precedence.earliest_stale_task,
    )
    .unwrap_or(false);
    let candidate = task_closure_baseline_repair_candidate_for_route(
        context,
        status,
        authority_inputs,
        task_number,
        precedence.earliest_stale_task,
    )
    .ok()
    .flatten();
    if review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED {
        return reducer_bridge_ready
            && authority_inputs.stale_target_allows_task_closure_bridge_for_task(task_number)
            || candidate.is_some()
                && non_current_stale_target_can_yield_to_missing_baseline(status, task_number);
    }
    let Some(candidate) = candidate else {
        return false;
    };
    let unresolved_stale_task_matches = status.blocking_task == Some(task_number)
        && (review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            || !status.stale_unreviewed_closures.is_empty());
    let dispatch_bound_bridge_candidate = candidate
        .dispatch_id
        .as_deref()
        .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty());
    let has_closure_bridge_reason_signals = closure_baseline_routing_reason_codes_compatible(status)
        && status.reason_codes.iter().any(|reason_code| {
            reason_code
                == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
        });
    let candidate_only_bridge_signal = closure_baseline_routing_reason_codes_compatible(status)
        && precedence.earliest_stale_task.is_none()
        && status.execution_reentry_target_source.as_deref()
            != Some(CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN)
        && status.reason_codes.iter().any(|reason_code| {
            reason_code
                == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
        });
    if review_state_status == "clean" {
        return matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        ) && (reducer_bridge_ready
            || has_closure_bridge_reason_signals
            || candidate_only_bridge_signal
            || (unresolved_stale_task_matches && dispatch_bound_bridge_candidate));
    }
    if review_state_status
        == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
    {
        return reducer_bridge_ready
            || has_closure_bridge_reason_signals
            || candidate_only_bridge_signal
            || (unresolved_stale_task_matches && dispatch_bound_bridge_candidate);
    }
    false
}
