use crate::execution::status::{PlanExecutionStatus, PublicReviewStateTaskClosure};
use crate::execution::task_scope_key::task_scope_key_for_task;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentTaskClosureRouteTarget {
    pub(crate) task: u32,
    pub(crate) scope_key: String,
    pub(crate) closure_record_id: Option<String>,
}

impl CurrentTaskClosureRouteTarget {
    pub(crate) fn from_task(task: u32) -> Self {
        Self {
            task,
            scope_key: task_scope_key_for_task(task),
            closure_record_id: None,
        }
    }

    fn from_closure(closure: &PublicReviewStateTaskClosure) -> Self {
        Self {
            task: closure.task,
            scope_key: task_scope_key_for_task(closure.task),
            closure_record_id: Some(closure.closure_record_id.clone()),
        }
    }
}

pub(crate) fn current_task_closure_route_target(
    status: &PlanExecutionStatus,
) -> Option<CurrentTaskClosureRouteTarget> {
    status
        .current_task_closures
        .iter()
        // Task order is the user-visible execution boundary. Ties should not
        // happen, but record id keeps selection deterministic for malformed
        // projections with duplicate current closures.
        .min_by_key(|closure| (closure.task, closure.closure_record_id.as_str()))
        .map(CurrentTaskClosureRouteTarget::from_closure)
}

pub(crate) fn current_task_closure_route_target_for_task(
    status: &PlanExecutionStatus,
    task: u32,
) -> CurrentTaskClosureRouteTarget {
    status
        .current_task_closures
        .iter()
        .filter(|closure| closure.task == task)
        .min_by_key(|closure| closure.closure_record_id.as_str())
        .map(CurrentTaskClosureRouteTarget::from_closure)
        .unwrap_or_else(|| CurrentTaskClosureRouteTarget::from_task(task))
}

pub(crate) fn preferred_current_task_closure_route_target(
    status: &PlanExecutionStatus,
    preferred_task: Option<u32>,
) -> Option<CurrentTaskClosureRouteTarget> {
    preferred_task
        .map(|task| current_task_closure_route_target_for_task(status, task))
        .or_else(|| current_task_closure_route_target(status))
}
