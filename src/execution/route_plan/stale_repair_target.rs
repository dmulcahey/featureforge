#[cfg(test)]
use crate::execution::resume_stale_precedence::stale_review_state_blocking_record_task_numbers;
use crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED;
use crate::execution::state::PlanExecutionStatus;

use super::route_semantics::blocking_task_from_status_records;

pub(crate) fn projected_stale_repair_task(status: &PlanExecutionStatus) -> Option<u32> {
    status
        .blocking_task
        .or_else(|| blocking_task_from_status_records(status))
}

#[cfg(test)]
pub(crate) fn projected_stale_repair_record_task(status: &PlanExecutionStatus) -> Option<u32> {
    stale_review_state_blocking_record_task_numbers(status).min()
}

pub(crate) fn targetless_stale_has_concrete_public_target(
    _status: &PlanExecutionStatus,
    has_authoritative_stale_target: bool,
    has_actionable_stale_reentry_target: bool,
) -> bool {
    has_authoritative_stale_target && has_actionable_stale_reentry_target
}

pub(crate) fn stale_task_scope_lacks_concrete_public_target(
    status: &PlanExecutionStatus,
    review_state_status: &str,
    has_authoritative_stale_target: bool,
    has_actionable_stale_reentry_target: bool,
) -> bool {
    review_state_status == REVIEW_STATE_STALE_UNREVIEWED
        && status.blocking_scope.as_deref() == Some("task")
        && status.blocking_task.is_some()
        && !targetless_stale_has_concrete_public_target(
            status,
            has_authoritative_stale_target,
            has_actionable_stale_reentry_target,
        )
}
