use serde::Serialize;

use crate::execution::current_truth::{
    branch_closure_refresh_missing_current_closure,
    task_review_result_requires_verification_reason_codes,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::phase;
use crate::execution::stale_target_selection::projected_earliest_stale_task_candidate_from_status;
use crate::execution::state::{
    PlanExecutionStatus, current_branch_closure_structural_review_state_reason,
};
use crate::execution::task_scope_key::task_scope_key_task_number;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub(crate) struct ExecutionBlockingProjection {
    pub(crate) scope: Option<String>,
    pub(crate) task: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExecutionBlockingProjectionInputs<'a> {
    pub(crate) phase_detail: &'a str,
    pub(crate) review_state_status: &'a str,
    pub(crate) status: Option<&'a PlanExecutionStatus>,
    pub(crate) fallback_scope: Option<&'a str>,
    pub(crate) fallback_task: Option<u32>,
    pub(crate) execution_command_task: Option<u32>,
    pub(crate) recording_task: Option<u32>,
    pub(crate) blocker_task: Option<u32>,
}

pub(crate) fn default_phase_for_status(status: &PlanExecutionStatus) -> String {
    if matches!(
        status.harness_phase,
        HarnessPhase::ContractDrafting | HarnessPhase::PivotRequired
    ) {
        String::from(phase::PHASE_PIVOT_REQUIRED)
    } else if status.harness_phase == HarnessPhase::HandoffRequired
        || (status.phase_detail == phase::DETAIL_EXECUTION_IN_PROGRESS
            && status.execution_command_context.is_some())
    {
        String::from(phase::PHASE_HANDOFF_REQUIRED)
    } else if (status.harness_phase == HarnessPhase::Executing
        && matches!(
            status.phase_detail.as_str(),
            phase::DETAIL_HANDOFF_RECORDING_REQUIRED
                | phase::DETAIL_PLANNING_REENTRY_REQUIRED
                | phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        ))
        || (status.execution_started == "yes"
            && status.phase_detail == phase::DETAIL_EXECUTION_IN_PROGRESS
            && status.execution_command_context.is_none())
    {
        String::from(phase::PHASE_EXECUTING)
    } else if status.phase_detail == phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED
        && status.harness_phase != HarnessPhase::ExecutionPreflight
    {
        status.harness_phase.to_string()
    } else {
        status
            .phase
            .clone()
            .unwrap_or_else(|| status.harness_phase.to_string())
    }
}

pub(crate) fn project_execution_blocking(
    inputs: ExecutionBlockingProjectionInputs<'_>,
) -> ExecutionBlockingProjection {
    let mut task = inputs.fallback_task;
    match inputs.phase_detail {
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => {
            task = inputs
                .execution_command_task
                .or(inputs.blocker_task)
                .or_else(|| inputs.status.and_then(blocking_task_from_status_records))
                .or_else(|| {
                    inputs
                        .status
                        .and_then(projected_earliest_stale_task_candidate_from_status)
                })
                .or(task);
        }
        phase::DETAIL_TASK_CLOSURE_RECORDING_READY => {
            task = inputs
                .recording_task
                .or(task)
                .or_else(|| inputs.status.and_then(blocking_task_from_status_records))
                .or_else(|| {
                    inputs
                        .status
                        .and_then(projected_earliest_stale_task_candidate_from_status)
                });
        }
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS => {
            task = None;
        }
        _ => {}
    }
    let scope = blocking_scope_for_phase_detail(
        inputs.phase_detail,
        task,
        inputs.status,
        inputs.review_state_status,
    )
    .or_else(|| inputs.fallback_scope.map(str::to_owned));
    ExecutionBlockingProjection { scope, task }
}

pub(crate) fn canonical_phase_for_shared_decision(
    default_phase: &str,
    phase_detail: &str,
) -> String {
    match phase_detail {
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING | phase::DETAIL_TASK_CLOSURE_RECORDING_READY => {
            String::from(phase::PHASE_TASK_CLOSURE_PENDING)
        }
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED => {
            String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING)
        }
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
            if default_phase == phase::PHASE_DOCUMENT_RELEASE_PENDING =>
        {
            String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING)
        }
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
        | phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
        | phase::DETAIL_FINAL_REVIEW_RECORDING_READY => {
            String::from(phase::PHASE_FINAL_REVIEW_PENDING)
        }
        phase::DETAIL_QA_RECORDING_REQUIRED | phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED => {
            String::from(phase::PHASE_QA_PENDING)
        }
        phase::DETAIL_FINISH_REVIEW_GATE_READY | phase::DETAIL_FINISH_COMPLETION_GATE_READY => {
            String::from(phase::PHASE_READY_FOR_BRANCH_COMPLETION)
        }
        phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED => {
            if matches!(
                default_phase,
                phase::PHASE_PIVOT_REQUIRED | phase::PHASE_HANDOFF_REQUIRED
            ) {
                default_phase.to_owned()
            } else {
                String::from(phase::PHASE_EXECUTION_PREFLIGHT)
            }
        }
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => String::from(phase::PHASE_EXECUTING),
        phase::DETAIL_EXECUTION_IN_PROGRESS => {
            if matches!(
                default_phase,
                phase::PHASE_EXECUTION_PREFLIGHT | phase::PHASE_HANDOFF_REQUIRED
            ) {
                default_phase.to_owned()
            } else {
                String::from(phase::PHASE_EXECUTING)
            }
        }
        phase::DETAIL_PLANNING_REENTRY_REQUIRED => String::from(phase::PHASE_PIVOT_REQUIRED),
        phase::DETAIL_HANDOFF_RECORDING_REQUIRED => {
            if default_phase == phase::PHASE_EXECUTING {
                String::from(phase::PHASE_EXECUTING)
            } else {
                String::from(phase::PHASE_HANDOFF_REQUIRED)
            }
        }
        _ => default_phase.to_owned(),
    }
}

pub(crate) fn blocking_scope_for_phase_detail(
    phase_detail: &str,
    blocking_task: Option<u32>,
    status: Option<&PlanExecutionStatus>,
    review_state_status: &str,
) -> Option<String> {
    let scope = match phase_detail {
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING | phase::DETAIL_TASK_CLOSURE_RECORDING_READY => {
            Some("task")
        }
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
        | phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
        | phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
        | phase::DETAIL_FINAL_REVIEW_RECORDING_READY
        | phase::DETAIL_QA_RECORDING_REQUIRED
        | phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED
        | phase::DETAIL_FINISH_REVIEW_GATE_READY
        | phase::DETAIL_FINISH_COMPLETION_GATE_READY => Some("branch"),
        phase::DETAIL_PLANNING_REENTRY_REQUIRED => Some("workflow"),
        phase::DETAIL_HANDOFF_RECORDING_REQUIRED => {
            if blocking_task.is_some() {
                Some("task")
            } else {
                Some("workflow")
            }
        }
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => {
            if review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
                && let Some(task) = blocking_task
                && status.is_some_and(|status| status_has_task_blocking_record(status, task))
            {
                Some("task")
            } else if review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
                && blocking_task.is_some()
                && status.is_some_and(|status| !status.stale_unreviewed_closures.is_empty())
            {
                Some("task")
            } else if matches!(
                review_state_status,
                crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
                    | crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            ) || status.is_some_and(|status| {
                branch_closure_refresh_missing_current_closure(status)
                    || current_branch_closure_structural_review_state_reason(status).is_some()
            }) {
                Some("branch")
            } else if blocking_task.is_some() {
                Some("task")
            } else {
                Some("workflow")
            }
        }
        _ => None,
    };
    scope.map(str::to_owned)
}

pub(crate) fn blocking_task_from_status_records(status: &PlanExecutionStatus) -> Option<u32> {
    status.blocking_records.iter().find_map(|record| {
        (record.scope_type == "task")
            .then(|| task_scope_key_task_number(&record.scope_key))
            .flatten()
    })
}

fn status_has_task_blocking_record(status: &PlanExecutionStatus, task: u32) -> bool {
    status.blocking_records.iter().any(|record| {
        record.scope_type == "task" && task_scope_key_task_number(&record.scope_key) == Some(task)
    })
}

pub(crate) fn external_wait_state_for_phase_detail(
    phase_detail: &str,
    blocking_reason_codes: &[String],
    external_review_result_ready: bool,
) -> Option<String> {
    if external_review_result_ready {
        return None;
    }
    match phase_detail {
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING
            if !task_review_result_requires_verification_reason_codes(
                blocking_reason_codes.iter().map(String::as_str),
            ) =>
        {
            Some(String::from(
                crate::execution::review_route_tokens::EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT,
            ))
        }
        phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING => Some(String::from(
            crate::execution::review_route_tokens::EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_wait_state_omits_external_review_wait_for_verification_blockers() {
        assert_eq!(
            external_wait_state_for_phase_detail(
                phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
                &[String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING)],
                false,
            ),
            None
        );
        assert_eq!(
            external_wait_state_for_phase_detail(
                phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
                &[String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN)],
                false,
            )
            .as_deref(),
            Some("waiting_for_external_review_result")
        );
    }
}
