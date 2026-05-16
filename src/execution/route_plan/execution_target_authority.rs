use crate::execution::state::{PlanExecutionStatus, PublicRepairTarget};

use super::execution_targets::{
    ExecutionCommandRouteTarget, execution_command_route_status_blocks_progress,
    execution_command_route_target_has_public_authority,
    public_repair_target_matches_execution_route, resolve_execution_command_route_target,
};

pub(crate) fn execution_command_route_target_has_authority(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
    route_repair_target_candidates: &[PublicRepairTarget],
) -> bool {
    execution_command_route_target_has_public_authority(status, target)
        || route_repair_target_candidates_authorize_target(
            status,
            route_repair_target_candidates,
            target,
        )
}

fn route_repair_target_candidates_authorize_target(
    status: &PlanExecutionStatus,
    route_repair_target_candidates: &[PublicRepairTarget],
    target: &ExecutionCommandRouteTarget,
) -> bool {
    if execution_command_route_status_blocks_progress(status) {
        return false;
    }
    if target.is_begin() && status.execution_fingerprint.trim().is_empty() {
        return false;
    }
    public_repair_target_matches_execution_route(
        route_repair_target_candidates.iter(),
        target,
        target.is_begin(),
    )
}

pub(crate) fn legal_execution_begin_route(
    status: &PlanExecutionStatus,
    plan_path: &str,
    route_repair_target_candidates: &[PublicRepairTarget],
) -> bool {
    resolve_execution_command_route_target(status, plan_path).is_some_and(|target| {
        target.is_begin()
            && execution_command_route_target_has_authority(
                status,
                &target,
                route_repair_target_candidates,
            )
    })
}

fn execution_command_route_target_can_drive_next_action(
    status: &PlanExecutionStatus,
    target: &ExecutionCommandRouteTarget,
    route_repair_target_candidates: &[PublicRepairTarget],
) -> bool {
    !target.is_begin()
        || execution_command_route_target_has_authority(
            status,
            target,
            route_repair_target_candidates,
        )
}

pub(crate) fn resolve_execution_command_route_target_for_next_action(
    status: &PlanExecutionStatus,
    plan_path: &str,
    route_repair_target_candidates: &[PublicRepairTarget],
) -> Option<ExecutionCommandRouteTarget> {
    resolve_execution_command_route_target(status, plan_path).filter(|target| {
        execution_command_route_target_can_drive_next_action(
            status,
            target,
            route_repair_target_candidates,
        )
    })
}
