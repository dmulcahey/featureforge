use crate::execution::command_eligibility::public_execution_mutation_is_authorized;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, late_stage_missing_task_closure_baseline_bridge_supported,
    task_boundary_block_reason_code,
};
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, AuthoritativeStaleTargetScope, AuthoritativeStaleTargetSource,
};
use crate::execution::state::{
    ExecutionContext, GateResult, PlanExecutionStatus, closure_baseline_candidate_task,
    latest_attempted_step_for_task, resolve_execution_command_route_target,
    task_closure_baseline_candidate_can_preempt_stale_target,
    task_closure_baseline_repair_candidate_with_stale_target,
    task_closures_are_non_branch_contributing, task_latest_attempts_are_completed,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionReentryTargetSource {
    BlockingBeginGuard,
    ResumeStep,
    ActiveStep,
    ClosureGraphStaleTarget,
    ExactRouteCommand,
    TaskClosureBaselineRepairCandidate,
    NegativeReviewOrVerificationResult,
}

impl ExecutionReentryTargetSource {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::BlockingBeginGuard => "blocking_begin_guard",
            Self::ResumeStep => "resume_step",
            Self::ActiveStep => "active_step",
            Self::ClosureGraphStaleTarget => "closure_graph_stale_target",
            Self::ExactRouteCommand => "exact_route_command",
            Self::TaskClosureBaselineRepairCandidate => "task_closure_baseline_repair_candidate",
            Self::NegativeReviewOrVerificationResult => "negative_review_or_verification_result",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionReentryTarget {
    pub(crate) task: u32,
    pub(crate) step: Option<u32>,
    pub(crate) source: ExecutionReentryTargetSource,
    pub(crate) reason_code: String,
    pub(crate) source_record_id: Option<String>,
}

impl ExecutionReentryTarget {
    pub(crate) fn new(
        task: u32,
        step: Option<u32>,
        source: ExecutionReentryTargetSource,
        reason_code: &str,
    ) -> Self {
        Self {
            task,
            step,
            source,
            reason_code: reason_code.to_owned(),
            source_record_id: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AuthoritativeStaleReentryTarget<'a> {
    pub(crate) task: u32,
    pub(crate) step: Option<u32>,
    pub(crate) reason_code: &'a str,
    pub(crate) source: AuthoritativeStaleTargetSource,
    pub(crate) source_record_id: Option<&'a str>,
    pub(crate) task_closure_bridge_allowed: bool,
}

impl<'a> AuthoritativeStaleReentryTarget<'a> {
    pub(crate) fn from_stale_target(target: &'a AuthoritativeStaleTarget) -> Option<Self> {
        if target.scope != AuthoritativeStaleTargetScope::Task {
            return None;
        }
        Some(Self {
            task: target.task?,
            step: target.step,
            reason_code: target.reason_code.as_str(),
            source: target.source,
            source_record_id: target.record_id.as_deref().or(Some(target.source.as_str())),
            task_closure_bridge_allowed: target.task_closure_bridge_allowed,
        })
    }

    pub(crate) fn into_execution_reentry_target(self) -> ExecutionReentryTarget {
        ExecutionReentryTarget {
            task: self.task,
            step: self.step,
            source: ExecutionReentryTargetSource::ClosureGraphStaleTarget,
            reason_code: self.reason_code.to_owned(),
            source_record_id: self.source_record_id.map(str::to_owned),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct NextActionAuthorityInputs<'a> {
    pub(crate) persisted_repair_follow_up: Option<&'a str>,
    pub(crate) branch_rerecording_assessment: Option<&'a BranchRerecordingAssessment>,
    pub(crate) gate_finish: Option<&'a GateResult>,
    pub(crate) has_authoritative_stale_target: bool,
    pub(crate) authoritative_stale_target: Option<AuthoritativeStaleReentryTarget<'a>>,
    pub(crate) derived_negative_result_reentry: bool,
}

impl<'a> NextActionAuthorityInputs<'a> {
    pub(crate) fn with_derived_negative_result_reentry(
        self,
        derived_negative_result_reentry: bool,
    ) -> Self {
        Self {
            derived_negative_result_reentry,
            ..self
        }
    }

    pub(crate) fn earliest_stale_task(self) -> Option<u32> {
        self.authoritative_stale_target.map(|target| target.task)
    }

    pub(crate) fn stale_target_allows_task_closure_bridge_for_task(self, task_number: u32) -> bool {
        self.authoritative_stale_target.is_none_or(|target| {
            if target.task < task_number {
                return false;
            }
            target.task > task_number || target.task_closure_bridge_allowed
        })
    }

    pub(crate) fn stale_target_is_baseline_bridge(self) -> bool {
        self.authoritative_stale_target
            .is_some_and(|target| target.source == AuthoritativeStaleTargetSource::BaselineBridge)
    }
}

pub(crate) fn missing_current_closure_allows_task_closure_baseline_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> bool {
    let completed_plan_missing_current_branch_closure = status.current_branch_closure_id.is_none()
        && context.steps.iter().all(|step| step.checked)
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && status.blocking_step.is_none();
    if !completed_plan_missing_current_branch_closure
        && (review_state_status != "missing_current_closure"
            || status.current_branch_closure_id.is_some())
    {
        return true;
    }
    if task_closures_are_non_branch_contributing(status) {
        return false;
    }
    authority_inputs
        .branch_rerecording_assessment
        .is_some_and(late_stage_missing_task_closure_baseline_bridge_supported)
}

pub(crate) fn completed_task_closure_preempts_execution_reentry(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    let clean_review_state = review_state_status == "clean";
    let completed_stale_target_missing_current_closure = review_state_status == "stale_unreviewed"
        && status.blocking_task == Some(task_number)
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && !prior_task_current_closure_requires_repair(status)
        && authority_inputs
            .authoritative_stale_target
            .is_some_and(|target| target.task == task_number && target.task_closure_bridge_allowed);
    (clean_review_state || completed_stale_target_missing_current_closure)
        && status.current_branch_closure_id.is_none()
        && status
            .current_task_closures
            .iter()
            .all(|closure| closure.task != task_number)
        && closure_baseline_candidate_task(context) == Some(task_number)
        && task_latest_attempts_are_completed(context, task_number)
        && (completed_stale_target_missing_current_closure
            || missing_current_closure_allows_task_closure_baseline_route(
                context,
                status,
                authority_inputs,
                review_state_status,
            ))
}

fn prior_task_current_closure_requires_repair(status: &PlanExecutionStatus) -> bool {
    status
        .reason_codes
        .iter()
        .chain(status.blocking_reason_codes.iter())
        .any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_stale"
                    | "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
            )
        })
}

pub(crate) fn task_boundary_blocking_task(status: &PlanExecutionStatus) -> Option<u32> {
    let task_number = status
        .blocking_task
        .or(status.resume_task)
        .or(status.active_task)?;
    let boundary_reason_code = task_boundary_block_reason_code(status).or_else(|| {
        status.reason_codes.iter().find_map(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_current_closure_stale"
                    | "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
                    | "task_cycle_break_active"
            )
            .then_some(reason_code.as_str())
        })
    })?;
    match boundary_reason_code {
        "prior_task_current_closure_missing"
        | "prior_task_current_closure_stale"
        | "prior_task_current_closure_invalid"
        | "prior_task_current_closure_reviewed_state_malformed"
        | "task_cycle_break_active" => Some(task_number),
        _ => None,
    }
}

pub(crate) fn execution_reentry_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<ExecutionReentryTarget> {
    if let Some(task) = task_boundary_blocking_task(status) {
        return Some(ExecutionReentryTarget::new(
            task,
            status
                .blocking_step
                .or(status.resume_step)
                .or(status.active_step),
            ExecutionReentryTargetSource::BlockingBeginGuard,
            "task_boundary_blocking_task",
        ));
    }

    let route_target = resolve_execution_command_route_target(status, plan_path);
    if let (Some(task), Some(step), Some(command)) = (
        status.active_task,
        status.active_step,
        route_target.as_ref(),
    ) && command.command_kind == "complete"
        && command.task_number == task
        && command.step_id == Some(step)
    {
        return Some(ExecutionReentryTarget::new(
            task,
            Some(step),
            ExecutionReentryTargetSource::ActiveStep,
            "active_step_route_continuation",
        ));
    }
    if let (Some(task), Some(step), Some(command)) = (
        status.resume_task,
        status.resume_step,
        route_target.as_ref(),
    ) && command.command_kind == "begin"
        && command.task_number == task
        && command.step_id == Some(step)
        && public_execution_mutation_is_authorized(
            status,
            command.command_kind,
            command.task_number,
            command.step_id,
        )
    {
        return Some(ExecutionReentryTarget::new(
            task,
            Some(step),
            ExecutionReentryTargetSource::ResumeStep,
            "resume_step_route_begin",
        ));
    }

    if let Some(target) = task_closure_baseline_reentry_target(context, status, authority_inputs)
        && task_closure_baseline_candidate_can_preempt_stale_target(
            status,
            target.task,
            authority_inputs.earliest_stale_task(),
        )
        && authority_inputs.stale_target_allows_task_closure_bridge_for_task(target.task)
    {
        return Some(target);
    }
    if let Some(target) =
        resume_step_preempts_later_stale_target(status, authority_inputs.authoritative_stale_target)
    {
        return Some(target);
    }
    if let Some(target) = authority_inputs.authoritative_stale_target
        && !authoritative_stale_target_is_current_task_closure(status, target)
    {
        return Some(target.into_execution_reentry_target());
    }
    if let Some(command) = route_target
        && public_execution_mutation_is_authorized(
            status,
            command.command_kind,
            command.task_number,
            command.step_id,
        )
    {
        return Some(ExecutionReentryTarget::new(
            command.task_number,
            command.step_id,
            ExecutionReentryTargetSource::ExactRouteCommand,
            "exact_route_command",
        ));
    }
    if status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_review_not_green")
        && let Some(task) = status.blocking_task
    {
        return Some(ExecutionReentryTarget::new(
            task,
            status.blocking_step,
            ExecutionReentryTargetSource::NegativeReviewOrVerificationResult,
            "prior_task_review_not_green",
        ));
    }
    if (authority_inputs.derived_negative_result_reentry
        || status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "negative_result_requires_execution_reentry"))
        && let Some(task) = latest_checked_task(context)
    {
        return Some(ExecutionReentryTarget::new(
            task,
            latest_attempted_step_for_task(context, task),
            ExecutionReentryTargetSource::NegativeReviewOrVerificationResult,
            "negative_result_requires_execution_reentry",
        ));
    }
    None
}

fn resume_step_preempts_later_stale_target(
    status: &PlanExecutionStatus,
    stale_target: Option<AuthoritativeStaleReentryTarget<'_>>,
) -> Option<ExecutionReentryTarget> {
    let resume_task = status.resume_task?;
    let resume_step = status.resume_step?;
    let target_task = stale_target
        .map(|target| target.task)
        .or(status.blocking_task)?;
    let target_step = stale_target.and_then(|target| target.step);
    let resume_is_earlier = resume_task < target_task
        || (resume_task == target_task
            && target_step.is_some_and(|stale_step| resume_step < stale_step));
    resume_is_earlier.then(|| {
        ExecutionReentryTarget::new(
            resume_task,
            Some(resume_step),
            ExecutionReentryTargetSource::ResumeStep,
            "resume_step_preempts_later_stale_target",
        )
    })
}

pub(crate) fn task_closure_baseline_reentry_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<ExecutionReentryTarget> {
    let task = closure_baseline_candidate_task(context)?;
    task_closure_baseline_repair_candidate_with_stale_target(
        context,
        status,
        task,
        authority_inputs.earliest_stale_task(),
    )
    .ok()
    .flatten()?;
    Some(ExecutionReentryTarget::new(
        task,
        None,
        ExecutionReentryTargetSource::TaskClosureBaselineRepairCandidate,
        "task_closure_baseline_repair_candidate",
    ))
}

pub(crate) fn authoritative_stale_target_is_current_task_closure(
    status: &PlanExecutionStatus,
    target: AuthoritativeStaleReentryTarget<'_>,
) -> bool {
    let Some(source_record_id) = target.source_record_id else {
        return false;
    };
    status
        .current_task_closures
        .iter()
        .any(|closure| closure.task == target.task && closure.closure_record_id == source_record_id)
}

pub(crate) fn select_authoritative_stale_reentry_target<'a>(
    status: &PlanExecutionStatus,
    stale_targets: impl IntoIterator<Item = &'a AuthoritativeStaleTarget>,
) -> Option<AuthoritativeStaleReentryTarget<'a>> {
    stale_targets
        .into_iter()
        .filter(|target| target.is_actionable_task_reentry_target(status))
        .filter_map(AuthoritativeStaleReentryTarget::from_stale_target)
        .min_by(|left, right| {
            left.task
                .cmp(&right.task)
                .then_with(|| left.step.cmp(&right.step))
                .then_with(|| left.source_record_id.cmp(&right.source_record_id))
                .then_with(|| left.reason_code.cmp(right.reason_code))
        })
}

pub(crate) fn latest_checked_task(context: &ExecutionContext) -> Option<u32> {
    context
        .steps
        .iter()
        .filter(|step| step.checked)
        .map(|step| step.task_number)
        .max()
}
