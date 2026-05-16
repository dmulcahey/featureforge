use crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE;
use crate::execution::current_truth::late_stage_surface_not_declared_reason_code;
use crate::execution::phase;
use crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE;
use crate::execution::review_route_tokens::{
    FOLLOW_UP_REPAIR_REVIEW_STATE, REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::stale_target_selection::{
    StaleBoundaryCandidate, select_earliest_stale_boundary_candidate,
};
use crate::execution::state::PlanExecutionStatus;
use crate::execution::status_support::projected_earliest_stale_task_from_status;
use crate::execution::task_scope_key::task_scope_key_task_number;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumePreemptionReason {
    EarlierStaleBoundary,
    TaskClosureBaselineBridge,
    ExecutionReentryBlocker,
    TaskCycleBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResumeStepBinding {
    pub(crate) task: u32,
    pub(crate) step: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StalePreemptionTarget {
    pub(crate) task: u32,
    pub(crate) step: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResumeStalePrecedenceInputs<'a> {
    pub(crate) status: &'a PlanExecutionStatus,
    pub(crate) review_state_status: &'a str,
    pub(crate) open_step_task: Option<u32>,
    pub(crate) authoritative_stale_boundary: Option<StaleBoundaryCandidate>,
    pub(crate) baseline_stale_boundary_task: Option<u32>,
    pub(crate) exact_resume_stale_task_target: Option<u32>,
    pub(crate) stale_preemption_target: Option<StalePreemptionTarget>,
    pub(crate) legal_resume_begin_route: bool,
    pub(crate) targetless_stale_has_concrete_public_target: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResumeStatusSuppressionInputs<'a> {
    pub(crate) status: &'a PlanExecutionStatus,
    pub(crate) strategy_cycle_break_task: Option<u32>,
    pub(crate) task_closure_baseline_bridge_preempts_resume: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResumeStalePrecedence {
    pub(crate) earliest_stale_boundary: Option<StaleBoundaryCandidate>,
    pub(crate) earliest_stale_task: Option<u32>,
    pub(crate) exact_resume_stale_task: Option<u32>,
    pub(crate) resume_preempted_by: Option<ResumePreemptionReason>,
    pub(crate) stale_preempted_by_resume: Option<ResumeStepBinding>,
    pub(crate) open_step_preempted_by_earlier_stale: bool,
    pub(crate) open_step_not_after_earliest_stale_boundary: bool,
    pub(crate) open_step_precedes_earliest_stale_boundary: bool,
    pub(crate) stale_resume_begin_route_candidate: bool,
    pub(crate) targetless_stale_reconcile_required: bool,
}

impl ResumeStalePrecedence {
    pub(crate) fn from_inputs(inputs: ResumeStalePrecedenceInputs<'_>) -> Self {
        let earliest_stale_boundary = select_earliest_stale_boundary_candidate(
            inputs.authoritative_stale_boundary,
            inputs.baseline_stale_boundary_task,
        );
        let earliest_stale_task = earliest_stale_boundary.map(StaleBoundaryCandidate::task);
        let exact_resume_stale_task =
            exact_resume_stale_task(inputs.status, inputs.exact_resume_stale_task_target);
        let open_step_preempted_by_earlier_stale = inputs
            .open_step_task
            .is_some_and(|task| earliest_stale_task.is_some_and(|earliest| earliest < task));
        let open_step_not_after_earliest_stale_boundary =
            earliest_stale_task.is_none_or(|earliest_task| {
                inputs
                    .open_step_task
                    .is_some_and(|open_task| open_task <= earliest_task)
            });
        let open_step_precedes_earliest_stale_boundary =
            earliest_stale_task.is_some_and(|earliest_task| {
                inputs
                    .open_step_task
                    .is_some_and(|open_task| open_task < earliest_task)
            });
        let stale_preempted_by_resume = inputs
            .legal_resume_begin_route
            .then(|| {
                resume_step_preempts_later_stale_target(
                    inputs.status,
                    inputs.stale_preemption_target,
                )
            })
            .flatten();
        let stale_resume_begin_route_candidate = inputs.legal_resume_begin_route
            && exact_resume_stale_task.is_some()
            && inputs.review_state_status == REVIEW_STATE_STALE_UNREVIEWED
            && inputs.status.active_task.is_none()
            && inputs.status.active_step.is_none()
            && inputs
                .status
                .resume_task
                .zip(inputs.status.resume_step)
                .is_some()
            && exact_resume_stale_task == inputs.status.resume_task;
        let targetless_stale_reconcile_required = inputs
            .status
            .reason_codes
            .iter()
            .any(|code| code == TARGETLESS_STALE_RECONCILE_REASON_CODE)
            && !inputs.targetless_stale_has_concrete_public_target;

        Self {
            earliest_stale_boundary,
            earliest_stale_task,
            exact_resume_stale_task,
            resume_preempted_by: None,
            stale_preempted_by_resume,
            open_step_preempted_by_earlier_stale,
            open_step_not_after_earliest_stale_boundary,
            open_step_precedes_earliest_stale_boundary,
            stale_resume_begin_route_candidate,
            targetless_stale_reconcile_required,
        }
    }

    pub(crate) fn for_status_suppression(inputs: ResumeStatusSuppressionInputs<'_>) -> Self {
        let resume_preempted_by = resume_status_preemption_reason(
            inputs.status,
            inputs.strategy_cycle_break_task,
            inputs.task_closure_baseline_bridge_preempts_resume,
        );
        let earliest_stale_task = projected_earliest_stale_task_from_status(inputs.status);

        Self {
            earliest_stale_boundary: None,
            earliest_stale_task,
            exact_resume_stale_task: exact_resume_stale_task(inputs.status, earliest_stale_task),
            resume_preempted_by,
            stale_preempted_by_resume: None,
            open_step_preempted_by_earlier_stale: false,
            open_step_not_after_earliest_stale_boundary: true,
            open_step_precedes_earliest_stale_boundary: false,
            stale_resume_begin_route_candidate: false,
            targetless_stale_reconcile_required: false,
        }
    }

    pub(crate) fn resume_preempted_by_stale(self) -> bool {
        self.resume_preempted_by.is_some()
    }
}

pub(crate) fn stale_review_state_blocking_record_task_numbers(
    status: &PlanExecutionStatus,
) -> impl Iterator<Item = u32> + '_ {
    status
        .blocking_records
        .iter()
        .filter(|record| record.scope_type == "task")
        .filter(|record| record.record_type == "review_state")
        .filter(|record| {
            record.required_follow_up.as_deref() == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        })
        .filter(|record| stale_review_state_blocking_record_code(&record.code))
        .filter_map(|record| task_scope_key_task_number(&record.scope_key))
}

fn exact_resume_stale_task(
    status: &PlanExecutionStatus,
    exact_resume_stale_task_target: Option<u32>,
) -> Option<u32> {
    let resume_task = status.resume_task?;
    status.resume_step?;
    stale_review_state_blocking_record_task_numbers(status)
        .find(|task| *task == resume_task)
        .or_else(|| (exact_resume_stale_task_target == Some(resume_task)).then_some(resume_task))
}

fn resume_status_preemption_reason(
    status: &PlanExecutionStatus,
    strategy_cycle_break_task: Option<u32>,
    task_closure_baseline_bridge_preempts_resume: bool,
) -> Option<ResumePreemptionReason> {
    let resume_task = status.resume_task?;
    let projected_earliest_stale_task = projected_earliest_stale_task_from_status(status);
    if projected_earliest_stale_task.is_some_and(|earliest_task| earliest_task < resume_task) {
        return Some(ResumePreemptionReason::EarlierStaleBoundary);
    }
    if task_closure_baseline_bridge_preempts_resume {
        return Some(ResumePreemptionReason::TaskClosureBaselineBridge);
    }
    if status.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && status.blocking_task.is_some_and(|blocking_task| {
            blocking_task != resume_task && blocking_task < resume_task
        })
    {
        return Some(ResumePreemptionReason::ExecutionReentryBlocker);
    }
    let task_cycle_break_active = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE);
    if task_cycle_break_active
        && strategy_cycle_break_task.is_some_and(|cycle_break_task| {
            cycle_break_task != resume_task && cycle_break_task < resume_task
        })
    {
        return Some(ResumePreemptionReason::TaskCycleBreak);
    }
    None
}

fn resume_step_preempts_later_stale_target(
    status: &PlanExecutionStatus,
    stale_target: Option<StalePreemptionTarget>,
) -> Option<ResumeStepBinding> {
    let resume_task = status.resume_task?;
    let resume_step = status.resume_step?;
    let target_task = stale_target
        .map(|target| target.task)
        .or(status.blocking_task)?;
    let target_step = stale_target.and_then(|target| target.step);
    let resume_is_earlier = resume_task < target_task
        || (resume_task == target_task
            && target_step.is_some_and(|stale_step| resume_step < stale_step));
    resume_is_earlier.then_some(ResumeStepBinding {
        task: resume_task,
        step: resume_step,
    })
}

fn stale_review_state_blocking_record_code(code: &str) -> bool {
    code == REVIEW_STATE_STALE_UNREVIEWED || late_stage_surface_not_declared_reason_code(code)
}
