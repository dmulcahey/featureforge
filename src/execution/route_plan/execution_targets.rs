use crate::execution::command_eligibility::PublicCommandKind;
use crate::execution::phase;
use crate::execution::status::{PlanExecutionStatus, PublicRepairTarget};

use super::state_kind::{external_wait_state_is_external_wait, state_kind_blocks_local_mutation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionCommandRouteTarget {
    pub(crate) kind: PublicCommandKind,
    pub(crate) task_number: u32,
    pub(crate) step_id: Option<u32>,
}

impl ExecutionCommandRouteTarget {
    pub(crate) fn command_kind(self) -> &'static str {
        self.kind
            .public_mutation_name()
            .expect("execution route targets must be public execution mutations")
    }

    pub(crate) fn is_begin(self) -> bool {
        self.kind == PublicCommandKind::Begin
    }

    pub(crate) fn is_complete(self) -> bool {
        self.kind == PublicCommandKind::Complete
    }
}

fn execution_route_target(
    kind: PublicCommandKind,
    task_number: u32,
    step_id: u32,
) -> ExecutionCommandRouteTarget {
    ExecutionCommandRouteTarget {
        kind,
        task_number,
        step_id: Some(step_id),
    }
}

pub(crate) fn resolve_execution_command_route_target(
    status: &PlanExecutionStatus,
    _plan_path: &str,
) -> Option<ExecutionCommandRouteTarget> {
    if let Some((task_number, step_id)) = status.active_task.zip(status.active_step) {
        return Some(execution_route_target(
            PublicCommandKind::Complete,
            task_number,
            step_id,
        ));
    }
    status
        .resume_task
        .zip(status.resume_step)
        .or_else(|| status.blocking_task.zip(status.blocking_step))
        .map(|(task_number, step_id)| {
            execution_route_target(PublicCommandKind::Begin, task_number, step_id)
        })
}

pub(crate) fn begin_route_target_matches_open_step_status(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
) -> bool {
    let target_step = target.step_id.map(|step| (target.task_number, step));
    target.is_begin()
        && status.execution_started == "yes"
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.blocking_task.zip(status.blocking_step) == target_step
}

pub(crate) fn execution_command_route_target_matches_public_status(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
) -> bool {
    if execution_command_route_status_blocks_progress(status) {
        return false;
    }
    begin_route_target_matches_open_step_status(status, target)
        || execution_command_context_matches_route_target(status, target)
        || public_status_repair_target_matches_execution_route(status, target, false)
}

pub(crate) fn execution_command_route_target_has_public_authority(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
) -> bool {
    if target.is_begin() {
        fingerprint_bound_begin_route_matches_public_status(status, target)
    } else {
        execution_command_route_target_matches_public_status(status, target)
    }
}

pub(crate) fn fingerprint_bound_begin_route_matches_public_status(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
) -> bool {
    target.is_begin()
        && !execution_command_route_status_blocks_progress(status)
        && !status.execution_fingerprint.trim().is_empty()
        && (begin_route_target_matches_open_step_status(status, target)
            || authoritative_run_identity_matches_resume_begin_route(status, target)
            || execution_command_context_matches_route_target(status, target)
            || public_status_repair_target_matches_execution_route(status, target, true))
}

fn authoritative_run_identity_matches_resume_begin_route(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
) -> bool {
    status
        .execution_run_id
        .as_ref()
        .is_some_and(|run_id| !run_id.0.trim().is_empty())
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.zip(status.resume_step)
            == target.step_id.map(|step| (target.task_number, step))
}

pub(crate) fn execution_command_route_status_blocks_progress(status: &PlanExecutionStatus) -> bool {
    state_kind_blocks_local_mutation(&status.state_kind)
        || status.phase_detail == phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        || external_wait_state_is_external_wait(status.external_wait_state.as_deref())
}

fn execution_command_context_matches_route_target(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
) -> bool {
    status
        .execution_command_context
        .as_ref()
        .is_some_and(|context| {
            context.command_kind == target.command_kind()
                && context.task_number == Some(target.task_number)
                && context.step_id == target.step_id
        })
}

pub(crate) fn public_repair_target_matches_execution_route<'a>(
    targets: impl IntoIterator<Item = &'a PublicRepairTarget>,
    target: &ExecutionCommandRouteTarget,
    require_fingerprint_bound: bool,
) -> bool {
    targets.into_iter().any(|repair_target| {
        (!require_fingerprint_bound || repair_target.expires_when_fingerprint_changes)
            && target
                .kind
                .matches_public_mutation_token(&repair_target.command_kind)
            && repair_target.task == Some(target.task_number)
            && repair_target.step == target.step_id
    })
}

fn public_status_repair_target_matches_execution_route(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
    require_fingerprint_bound: bool,
) -> bool {
    public_repair_target_matches_execution_route(
        status.public_repair_targets.iter(),
        target,
        require_fingerprint_bound,
    )
}
