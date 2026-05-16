use crate::execution::current_truth::{
    BranchRerecordingAssessment, late_stage_missing_task_closure_baseline_bridge_supported,
    task_boundary_block_reason_code,
};
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::resume_stale_precedence::{
    ResumeStalePrecedence, ResumeStalePrecedenceInputs, StalePreemptionTarget,
};
use crate::execution::review_route_tokens::{
    REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY, REVIEW_STATE_MISSING_CURRENT_CLOSURE,
    REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::route_plan::{
    execution_command_route_target_has_authority, resolve_execution_command_route_target,
};
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, AuthoritativeStaleTargetScope, AuthoritativeStaleTargetSource,
    stale_task_closure_bridge_allows_task_parts,
};
use crate::execution::stale_target_selection::{
    StaleBoundaryCandidate, select_actionable_stale_reentry_target, stale_reentry_source_record_id,
};
use crate::execution::state::{
    CurrentTaskClosureBranchRouteFacts, ExecutionContext, GateResult, PlanExecutionStatus,
    PublicRepairTarget, closure_baseline_candidate_task,
    current_task_closure_branch_route_facts_from_status, latest_attempted_step_for_task,
    task_closure_baseline_candidate_can_preempt_stale_target,
    task_closure_baseline_repair_candidate_with_stale_target_and_authority,
    task_latest_attempts_are_completed,
};
use crate::execution::transitions::AuthoritativeTransitionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionReentryTargetSource {
    BlockingBeginGuard,
    ResumeStep,
    ActiveStep,
    AuthoritativeStaleTarget(AuthoritativeStaleTargetSource),
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
            Self::AuthoritativeStaleTarget(source) => source.execution_reentry_source_token(),
            Self::ExactRouteCommand => "exact_route_command",
            Self::TaskClosureBaselineRepairCandidate => crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
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
            source_record_id: stale_reentry_source_record_id(target),
            task_closure_bridge_allowed: target.task_closure_bridge_allowed,
        })
    }

    pub(crate) fn into_execution_reentry_target(self) -> ExecutionReentryTarget {
        ExecutionReentryTarget {
            task: self.task,
            step: self.step,
            source: ExecutionReentryTargetSource::AuthoritativeStaleTarget(self.source),
            reason_code: self.reason_code.to_owned(),
            source_record_id: self.source_record_id.map(str::to_owned),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct NextActionAuthorityInputs<'a> {
    pub(crate) overlay: Option<&'a StatusAuthoritativeOverlay>,
    pub(crate) authoritative_state: Option<&'a AuthoritativeTransitionState>,
    pub(crate) persisted_repair_follow_up: Option<&'a str>,
    pub(crate) branch_rerecording_assessment: Option<&'a BranchRerecordingAssessment>,
    pub(crate) gate_finish: Option<&'a GateResult>,
    pub(crate) route_repair_target_candidates: &'a [PublicRepairTarget],
    pub(crate) has_authoritative_stale_target: bool,
    pub(crate) authoritative_stale_target: Option<AuthoritativeStaleReentryTarget<'a>>,
    pub(crate) derived_negative_result_reentry: bool,
    pub(crate) current_task_closure_branch_route_facts: Option<CurrentTaskClosureBranchRouteFacts>,
}

impl<'a> NextActionAuthorityInputs<'a> {
    pub(crate) fn with_current_task_closure_branch_route_facts(
        self,
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
    ) -> Self {
        Self {
            current_task_closure_branch_route_facts: Some(current_task_closure_branch_route_facts),
            ..self
        }
    }

    pub(crate) fn with_derived_negative_result_reentry(
        self,
        derived_negative_result_reentry: bool,
    ) -> Self {
        Self {
            derived_negative_result_reentry,
            ..self
        }
    }

    pub(crate) fn current_task_closure_branch_route_facts_or_derive(
        self,
        context: &ExecutionContext,
        status: &PlanExecutionStatus,
    ) -> CurrentTaskClosureBranchRouteFacts {
        self.current_task_closure_branch_route_facts
            .unwrap_or_else(|| current_task_closure_branch_route_facts_from_status(context, status))
    }

    pub(crate) fn precomputed_current_task_closure_branch_route_facts(
        self,
    ) -> CurrentTaskClosureBranchRouteFacts {
        self.current_task_closure_branch_route_facts.expect(
            "route planning must seed CurrentTaskClosureBranchRouteFacts before selecting child routes",
        )
    }

    pub(crate) fn stale_target_allows_task_closure_bridge_for_task(self, task_number: u32) -> bool {
        stale_task_closure_bridge_allows_task_parts(
            self.authoritative_stale_target
                .map(|target| (target.task, target.task_closure_bridge_allowed)),
            task_number,
        )
    }
}

pub(crate) fn missing_current_closure_allows_task_closure_baseline_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> bool {
    let current_task_closure_branch_route_facts =
        authority_inputs.current_task_closure_branch_route_facts_or_derive(context, status);
    let completed_plan_missing_current_branch_closure = current_task_closure_branch_route_facts
        .missing_branch_closure()
        && context.steps.iter().all(|step| step.checked)
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && status.blocking_step.is_none();
    if !completed_plan_missing_current_branch_closure
        && (review_state_status != REVIEW_STATE_MISSING_CURRENT_CLOSURE
            || current_task_closure_branch_route_facts.branch_closure_recorded())
    {
        return true;
    }
    if current_task_closure_branch_route_facts.set_is_non_branch_contributing() {
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
    let completed_stale_target_missing_current_closure = review_state_status
        == REVIEW_STATE_STALE_UNREVIEWED
        && status.blocking_task == Some(task_number)
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && !prior_task_current_closure_requires_repair(status)
        && authority_inputs
            .authoritative_stale_target
            .is_some_and(|target| target.task == task_number && target.task_closure_bridge_allowed);
    (clean_review_state || completed_stale_target_missing_current_closure)
        && authority_inputs
            .current_task_closure_branch_route_facts_or_derive(context, status)
            .branch_missing_and_task_has_no_current_closure(status, task_number)
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
            crate::execution::closure_diagnostics::task_boundary_current_closure_repair_reason_code(
                reason_code,
            )
        })
}

pub(crate) fn task_boundary_blocking_task(status: &PlanExecutionStatus) -> Option<u32> {
    let task_number = status.blocking_task.or(status.active_task)?;
    let boundary_reason_code = task_boundary_block_reason_code(status).or_else(|| {
        status.reason_codes.iter().find_map(|reason_code| {
            crate::execution::closure_diagnostics::task_boundary_current_closure_boundary_reason_code(
                reason_code,
            )
            .then_some(reason_code.as_str())
        })
    })?;
    crate::execution::closure_diagnostics::task_boundary_current_closure_boundary_reason_code(
        boundary_reason_code,
    )
    .then_some(task_number)
}

pub(crate) fn execution_reentry_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<ExecutionReentryTarget> {
    let route_target = resolve_execution_command_route_target(status, plan_path);
    let legal_resume_begin_route = route_target.as_ref().is_some_and(|command| {
        command.is_begin()
            && execution_command_route_target_has_authority(
                status,
                command,
                authority_inputs.route_repair_target_candidates,
            )
    });
    let precedence = repair_precedence(status, authority_inputs, legal_resume_begin_route);
    let earliest_stale_task = precedence.earliest_stale_task;
    let current_task_closure_branch_route_facts =
        authority_inputs.current_task_closure_branch_route_facts_or_derive(context, status);

    if let Some(target) = task_closure_baseline_reentry_target_with_authority(
        context,
        status,
        earliest_stale_task,
        authority_inputs,
    ) && task_closure_baseline_candidate_can_preempt_stale_target(
        status,
        target.task,
        earliest_stale_task,
    ) && authority_inputs.stale_target_allows_task_closure_bridge_for_task(target.task)
    {
        return Some(target);
    }
    if let Some(task) = task_boundary_blocking_task(status) {
        let matching_stale_target = authority_inputs
            .authoritative_stale_target
            .filter(|target| target.task == task)
            .filter(|target| {
                !authoritative_stale_target_is_current_task_closure(
                    status,
                    *target,
                    current_task_closure_branch_route_facts,
                )
            });
        let mut target = ExecutionReentryTarget::new(
            task,
            status.blocking_step.or(status.active_step),
            matching_stale_target
                .map_or(ExecutionReentryTargetSource::BlockingBeginGuard, |target| {
                    ExecutionReentryTargetSource::AuthoritativeStaleTarget(target.source)
                }),
            "task_boundary_blocking_task",
        );
        if let Some(stale_target) = matching_stale_target {
            target.source_record_id = stale_target.source_record_id.map(str::to_owned);
        }
        return Some(target);
    }

    if let (Some(task), Some(step), Some(command)) = (
        status.active_task,
        status.active_step,
        route_target.as_ref(),
    ) && command.is_complete()
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
    if let Some(binding) = precedence.stale_preempted_by_resume {
        return Some(ExecutionReentryTarget::new(
            binding.task,
            Some(binding.step),
            ExecutionReentryTargetSource::ResumeStep,
            "resume_step_preempts_later_stale_target",
        ));
    }
    if let (Some(task), Some(step), Some(command)) = (
        status.resume_task,
        status.resume_step,
        route_target.as_ref(),
    ) && command.is_begin()
        && command.task_number == task
        && command.step_id == Some(step)
        && legal_resume_begin_route
    {
        return Some(ExecutionReentryTarget::new(
            task,
            Some(step),
            ExecutionReentryTargetSource::ResumeStep,
            "resume_step_route_begin",
        ));
    }

    if let Some(target) = authority_inputs.authoritative_stale_target
        && !authoritative_stale_target_is_current_task_closure(
            status,
            target,
            current_task_closure_branch_route_facts,
        )
    {
        return Some(target.into_execution_reentry_target());
    }
    if let Some(command) = route_target
        && execution_command_route_target_has_authority(
            status,
            &command,
            authority_inputs.route_repair_target_candidates,
        )
    {
        return Some(ExecutionReentryTarget::new(
            command.task_number,
            command.step_id,
            ExecutionReentryTargetSource::ExactRouteCommand,
            "exact_route_command",
        ));
    }
    if status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_boundary_negative_review_reason_code(
            reason_code,
        )
    }) && let Some(task) = status.blocking_task
    {
        return Some(ExecutionReentryTarget::new(
            task,
            status.blocking_step,
            ExecutionReentryTargetSource::NegativeReviewOrVerificationResult,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN,
        ));
    }
    if (authority_inputs.derived_negative_result_reentry
        || status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY))
        && let Some(task) = latest_checked_task(context)
    {
        return Some(ExecutionReentryTarget::new(
            task,
            latest_attempted_step_for_task(context, task),
            ExecutionReentryTargetSource::NegativeReviewOrVerificationResult,
            REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY,
        ));
    }
    None
}

fn repair_precedence(
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    legal_resume_begin_route: bool,
) -> ResumeStalePrecedence {
    let stale_target = authority_inputs.authoritative_stale_target;
    let authoritative_stale_boundary = stale_target.map(|target| {
        StaleBoundaryCandidate::from_authoritative_stale_target(target.task, target.source)
    });
    ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
        status,
        review_state_status: status.review_state_status.as_str(),
        open_step_task: None,
        authoritative_stale_boundary,
        baseline_stale_boundary_task: None,
        exact_resume_stale_task_target: authoritative_stale_boundary
            .map(StaleBoundaryCandidate::task),
        stale_preemption_target: stale_target.map(|target| StalePreemptionTarget {
            task: target.task,
            step: target.step,
        }),
        legal_resume_begin_route,
        targetless_stale_has_concrete_public_target: true,
    })
}

pub(crate) fn task_closure_baseline_reentry_target_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    earliest_stale_task: Option<u32>,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<ExecutionReentryTarget> {
    let task = closure_baseline_candidate_task(context)?;
    let branch_rerecording_assessment = authority_inputs.branch_rerecording_assessment?;
    task_closure_baseline_repair_candidate_with_stale_target_and_authority(
        context,
        status,
        task,
        earliest_stale_task,
        authority_inputs.overlay,
        authority_inputs.authoritative_state,
        branch_rerecording_assessment,
    )
    .ok()
    .flatten()?;
    Some(ExecutionReentryTarget::new(
        task,
        None,
        ExecutionReentryTargetSource::TaskClosureBaselineRepairCandidate,
        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
    ))
}

pub(crate) fn authoritative_stale_target_is_current_task_closure(
    status: &PlanExecutionStatus,
    target: AuthoritativeStaleReentryTarget<'_>,
    current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
) -> bool {
    let Some(source_record_id) = target.source_record_id else {
        return false;
    };
    current_task_closure_branch_route_facts.stale_target_matches_current_task_closure(
        status,
        target.task,
        source_record_id,
    )
}

pub(crate) fn select_authoritative_stale_reentry_target<'a>(
    status: &PlanExecutionStatus,
    stale_targets: impl IntoIterator<Item = &'a AuthoritativeStaleTarget>,
) -> Option<AuthoritativeStaleReentryTarget<'a>> {
    select_actionable_stale_reentry_target(status, stale_targets)
        .and_then(AuthoritativeStaleReentryTarget::from_stale_target)
}

pub(crate) fn latest_checked_task(context: &ExecutionContext) -> Option<u32> {
    context
        .steps
        .iter()
        .filter(|step| step.checked)
        .map(|step| step.task_number)
        .max()
}
