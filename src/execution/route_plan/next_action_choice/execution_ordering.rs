use super::super::execution_target_authority::{
    legal_execution_begin_route, resolve_execution_command_route_target_for_next_action,
};
use crate::execution::current_truth::task_review_result_pending_task;
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::public_repair_targets::close_current_task_public_repair_target_for_task;
use crate::execution::repair_route_decision::{
    task_closure_baseline_bridge_blocking_task_route_task,
    task_closure_baseline_bridge_candidate_route_task,
    task_closure_baseline_bridge_external_review_route_task,
    task_closure_baseline_bridge_open_step_preempted_by_closure_recording,
    task_closure_baseline_bridge_reentry_target,
    task_closure_baseline_bridge_stale_boundary_route_ready,
    task_closure_baseline_bridge_task_review_pending_route_task,
    task_closure_baseline_bridge_task_review_result_ready_promotes_recording,
};
use crate::execution::repair_target_selection::NextActionAuthorityInputs;
use crate::execution::resume_stale_precedence::{
    ResumeStalePrecedence, ResumeStalePrecedenceInputs, StalePreemptionTarget,
};
use crate::execution::stale_target_selection::{
    StaleBoundaryCandidate, StaleBoundaryCandidateSource,
};
use crate::execution::state::{
    CurrentTaskClosureBranchRouteFacts, ExecutionContext, PlanExecutionStatus,
    execution_reentry_requires_review_state_repair_with_authority,
    task_scope_structural_review_state_reason,
};

use super::execution_routes::{
    ExecutionReentryDecisionInputs, closure_prerequisite_decision,
    completed_execution_missing_branch_closure, decision_from_execution_command_route_target,
    execution_reentry_blocking_task, execution_reentry_decision_for_task,
    execution_repair_decision, execution_repair_decision_for_task,
    hard_structural_corruption_detected, missing_execution_reentry_target_decision,
    task_closure_recording_ready_decision,
};
use super::late_stage_public_routes::late_stage_decision;
use super::late_stage_repair_routes::{
    late_stage_execution_reentry_decision, late_stage_negative_result_reroute,
};
use super::late_stage_routes::task_scope_pivot_override_active;
use super::{NextActionDecision, NextActionKind};

pub(super) struct ExecutionRouteFacts {
    resume_stale_precedence: ResumeStalePrecedence,
    open_step_task: Option<u32>,
    pub(super) legal_begin_route_active: bool,
    open_step_preempted_by_execution_reentry_blocker: bool,
    open_step_preempted_by_closure_recording_ready: bool,
    pub(super) current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
}

pub(super) fn execution_route_facts(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> ExecutionRouteFacts {
    let handoff_route_active = status.handoff_required
        || status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::phase::PHASE_HANDOFF_REQUIRED);
    let authoritative_stale_boundary_candidate =
        authority_inputs.authoritative_stale_target.map(|target| {
            StaleBoundaryCandidate::from_authoritative_stale_target(target.task, target.source)
        });
    let baseline_reentry_target = (!handoff_route_active)
        .then(|| task_closure_baseline_bridge_reentry_target(context, status, authority_inputs))
        .flatten();
    let repair_targets = authority_inputs.route_repair_target_candidates;
    let open_step_task = status.active_task.or(status.resume_task).or(status
        .blocking_task
        .filter(|_| status.blocking_step.is_some()));
    let legal_resume_begin_route = legal_execution_begin_route(status, plan_path, repair_targets);
    let resume_stale_precedence = ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
        status,
        review_state_status,
        open_step_task,
        authoritative_stale_boundary: (!handoff_route_active)
            .then_some(authoritative_stale_boundary_candidate)
            .flatten(),
        baseline_stale_boundary_task: (!handoff_route_active)
            .then(|| baseline_reentry_target.as_ref().map(|target| target.task))
            .flatten(),
        exact_resume_stale_task_target: authoritative_stale_boundary_candidate
            .map(StaleBoundaryCandidate::task),
        stale_preemption_target: authority_inputs.authoritative_stale_target.map(|target| {
            StalePreemptionTarget {
                task: target.task,
                step: target.step,
            }
        }),
        legal_resume_begin_route,
        targetless_stale_has_concrete_public_target: true,
    });
    let open_step_preempted_by_execution_reentry_blocker =
        execution_reentry_blocking_task(context, status, authority_inputs).is_some_and(
            |blocking_task| open_step_task.is_none_or(|open_task| blocking_task < open_task),
        ) || (status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            && status.blocking_step.is_none()
            && status.blocking_task.is_some_and(|blocking_task| {
                open_step_task.is_none_or(|open_task| blocking_task < open_task)
            }))
            || (review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
                && !resume_stale_precedence.open_step_not_after_earliest_stale_boundary);
    let open_step_preempted_by_closure_recording_ready =
        open_step_task.is_some_and(|task_number| {
            task_closure_baseline_bridge_open_step_preempted_by_closure_recording(
                context,
                status,
                authority_inputs,
                review_state_status,
                task_number,
            )
        });
    let current_task_closure_branch_route_facts =
        authority_inputs.precomputed_current_task_closure_branch_route_facts();
    ExecutionRouteFacts {
        resume_stale_precedence,
        open_step_task,
        legal_begin_route_active: legal_resume_begin_route,
        open_step_preempted_by_execution_reentry_blocker,
        open_step_preempted_by_closure_recording_ready,
        current_task_closure_branch_route_facts,
    }
}

pub(super) fn missing_authoritative_stale_reentry_target_route(
    status: &PlanExecutionStatus,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    (review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && !status.stale_unreviewed_closures.is_empty()
        && authority_inputs.authoritative_stale_target.is_none()
        && !authority_inputs.has_authoritative_stale_target)
        .then(|| missing_execution_reentry_target_decision(status, review_state_status))
}

pub(super) fn hard_structural_corruption_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
    facts: &ExecutionRouteFacts,
) -> Option<NextActionDecision> {
    if !hard_structural_corruption_detected(status, facts.current_task_closure_branch_route_facts) {
        return None;
    }
    if review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && let Some(stale_task) = facts.resume_stale_precedence.earliest_stale_task
        && Some(stale_task) != status.blocking_task
    {
        return Some(execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            review_state_status,
            stale_task,
            ExecutionReentryDecisionInputs {
                current_task_closure_branch_route_facts: facts
                    .current_task_closure_branch_route_facts,
                authority_inputs,
                stale_boundary_route: true,
            },
        ));
    }
    if let Some(task_number) = execution_reentry_blocking_task(context, status, authority_inputs) {
        return Some(execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            review_state_status,
            task_number,
            ExecutionReentryDecisionInputs {
                current_task_closure_branch_route_facts: facts
                    .current_task_closure_branch_route_facts,
                authority_inputs,
                stale_boundary_route: false,
            },
        ));
    }
    Some(execution_repair_decision(
        context,
        status,
        plan_path,
        review_state_status,
        authority_inputs,
    ))
}

pub(super) fn completed_execution_missing_branch_closure_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
    facts: &ExecutionRouteFacts,
) -> Option<NextActionDecision> {
    if review_state_status != "clean"
        || !completed_execution_missing_branch_closure(
            status,
            context,
            facts.current_task_closure_branch_route_facts,
        )
    {
        return None;
    }
    if facts
        .current_task_closure_branch_route_facts
        .set_is_non_branch_contributing()
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        ));
    }
    Some(late_stage_decision(
        status,
        NextActionKind::AdvanceLateStage,
        crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
        plan_path,
    ))
}

pub(super) fn open_step_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
    facts: &ExecutionRouteFacts,
) -> Option<NextActionDecision> {
    if facts.open_step_task.is_none()
        || facts
            .resume_stale_precedence
            .open_step_preempted_by_earlier_stale
        || facts.open_step_preempted_by_execution_reentry_blocker
        || facts.open_step_preempted_by_closure_recording_ready
        || task_scope_pivot_override_active(status, review_state_status)
    {
        return None;
    }
    if review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && authority_inputs.has_authoritative_stale_target
        && authority_inputs.authoritative_stale_target.is_none()
    {
        return Some(missing_execution_reentry_target_decision(
            status,
            review_state_status,
        ));
    }
    let earliest_stale_boundary_task = facts
        .resume_stale_precedence
        .earliest_stale_boundary
        .map(StaleBoundaryCandidate::task);
    let stale_boundary_open_step_resume_allowed = earliest_stale_boundary_task
        .is_some_and(|earliest_task| facts.open_step_task == Some(earliest_task))
        && review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && status.blocking_task.is_none()
        && task_scope_structural_review_state_reason(status).is_none()
        || (facts
            .resume_stale_precedence
            .open_step_precedes_earliest_stale_boundary
            && review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            && task_scope_structural_review_state_reason(status).is_none());
    if execution_reentry_requires_review_state_repair_with_authority(
        Some(context),
        status,
        authority_inputs.overlay,
        authority_inputs.authoritative_state,
    ) && !stale_boundary_open_step_resume_allowed
        && !late_stage_negative_result_reroute(
            status,
            review_state_status,
            facts.current_task_closure_branch_route_facts,
        )
        && !status.reason_codes.is_empty()
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        ));
    }
    let repair_targets = authority_inputs.route_repair_target_candidates;
    decision_from_execution_command_route_target(
        status,
        plan_path,
        resolve_execution_command_route_target_for_next_action(status, plan_path, repair_targets),
    )
}

pub(super) fn stale_boundary_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
    facts: &ExecutionRouteFacts,
) -> Option<NextActionDecision> {
    let stale_boundary = facts.resume_stale_precedence.earliest_stale_boundary?;
    let stale_task = stale_boundary.task();
    let task_closure_ready = |task_number| {
        task_closure_recording_ready_decision(
            status,
            plan_path,
            facts.current_task_closure_branch_route_facts,
            task_number,
        )
    };
    if task_closure_baseline_bridge_stale_boundary_route_ready(
        context,
        status,
        authority_inputs,
        review_state_status,
        stale_task,
    ) {
        return Some(task_closure_ready(stale_task));
    }
    let baseline_bridge_boundary =
        stale_boundary.source() == StaleBoundaryCandidateSource::TaskClosureBaselineBridge;
    if baseline_bridge_boundary
        && close_current_task_public_repair_target_for_task(
            authority_inputs.route_repair_target_candidates.iter(),
            stale_task,
        )
    {
        return Some(task_closure_ready(stale_task));
    }
    if baseline_bridge_boundary {
        return Some(missing_execution_reentry_target_decision(
            status,
            review_state_status,
        ));
    }
    Some(execution_reentry_decision_for_task(
        context,
        status,
        plan_path,
        review_state_status,
        stale_task,
        ExecutionReentryDecisionInputs {
            current_task_closure_branch_route_facts: facts.current_task_closure_branch_route_facts,
            authority_inputs,
            stale_boundary_route: true,
        },
    ))
}

pub(super) fn closure_prerequisite_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
    task_review_dispatch_id: Option<&str>,
    external_review_result_ready: bool,
) -> Option<NextActionDecision> {
    let current_task_closure_branch_route_facts =
        authority_inputs.precomputed_current_task_closure_branch_route_facts();
    if review_state_status != crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && execution_reentry_requires_review_state_repair_with_authority(
            Some(context),
            status,
            authority_inputs.overlay,
            authority_inputs.authoritative_state,
        )
        && !late_stage_negative_result_reroute(
            status,
            review_state_status,
            current_task_closure_branch_route_facts,
        )
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        ));
    }
    if status.harness_phase == HarnessPhase::Executing
        && review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && current_task_closure_branch_route_facts.missing_branch_closure()
        && status.phase_detail != crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        ));
    }
    if let Some(decision) = late_stage_execution_reentry_decision(
        context,
        status,
        plan_path,
        review_state_status,
        authority_inputs,
    ) {
        return Some(decision);
    }
    if current_task_closure_branch_route_facts
        .set_has_non_branch_contributing_closure_without_branch()
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        ));
    }
    let task_closure_ready = |task_number| {
        task_closure_recording_ready_decision(
            status,
            plan_path,
            current_task_closure_branch_route_facts,
            task_number,
        )
    };
    if let Some(task_number) = task_review_result_pending_task(status, task_review_dispatch_id) {
        if let Some(task_number) = task_closure_baseline_bridge_task_review_pending_route_task(
            context,
            status,
            authority_inputs,
            review_state_status,
            task_number,
        ) {
            return Some(task_closure_ready(task_number));
        }
        if task_closure_baseline_bridge_task_review_result_ready_promotes_recording(
            status,
            external_review_result_ready,
        ) {
            return Some(task_closure_ready(task_number));
        }
        return Some(closure_prerequisite_decision(
            status,
            NextActionKind::WaitForTaskReviewResult,
            crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
            Some(task_number),
            None,
        ));
    }
    if let Some(task_number) = task_closure_baseline_bridge_blocking_task_route_task(
        context,
        status,
        authority_inputs,
        review_state_status,
    ) {
        return Some(task_closure_ready(task_number));
    }
    if let Some(task_number) = task_closure_baseline_bridge_external_review_route_task(
        context,
        status,
        authority_inputs,
        review_state_status,
        external_review_result_ready,
    ) {
        return Some(task_closure_ready(task_number));
    }
    if let Some(task_number) = task_closure_baseline_bridge_candidate_route_task(
        context,
        status,
        authority_inputs,
        review_state_status,
    ) {
        return Some(task_closure_ready(task_number));
    }
    execution_reentry_blocking_task(context, status, authority_inputs).map(|task_number| {
        execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            review_state_status,
            task_number,
            ExecutionReentryDecisionInputs {
                current_task_closure_branch_route_facts,
                authority_inputs,
                stale_boundary_route: false,
            },
        )
    })
}

pub(super) fn first_unchecked_step_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    facts: &ExecutionRouteFacts,
) -> Option<NextActionDecision> {
    let current_task_closure_branch_route_facts = facts.current_task_closure_branch_route_facts;
    let first_unchecked_step = context.steps.iter().find(|step| {
        !step.checked
            && current_task_closure_branch_route_facts
                .task_has_no_current_closure(status, step.task_number)
    })?;
    let task_number = first_unchecked_step.task_number;
    let step_number = first_unchecked_step.step_number;
    let unchecked_step_conflicts_with_current_closure =
        current_task_closure_branch_route_facts.task_has_current_closure(status, task_number);
    let authoritative_open_step_marker_loss = status.execution_started == "yes"
        && (status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
            || !context.evidence.attempts.is_empty())
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && current_task_closure_branch_route_facts.task_closure_set_empty();
    let marker_free_preflight_projection = status.execution_mode != "none"
        && status.execution_started == "no"
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && current_task_closure_branch_route_facts.task_closure_set_empty();
    let _ = context.legacy_open_step_projection_present;
    if authoritative_open_step_marker_loss {
        return Some(execution_repair_decision_for_task(
            status,
            plan_path,
            review_state_status,
            task_number,
        ));
    }
    if status.execution_started == "yes"
        && unchecked_step_conflicts_with_current_closure
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.blocking_step.is_none()
    {
        return Some(execution_repair_decision_for_task(
            status,
            plan_path,
            review_state_status,
            task_number,
        ));
    }
    let (phase, phase_detail) = if marker_free_preflight_projection {
        (
            String::from(crate::execution::phase::PHASE_EXECUTION_PREFLIGHT),
            String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS),
        )
    } else if status.execution_started == "yes" {
        (
            String::from(crate::execution::phase::PHASE_EXECUTING),
            String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        )
    } else {
        (
            String::from(crate::execution::phase::PHASE_EXECUTION_PREFLIGHT),
            String::from(crate::execution::phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED),
        )
    };
    Some(NextActionDecision {
        kind: NextActionKind::Begin,
        phase,
        phase_detail,
        review_state_status: review_state_status.to_owned(),
        task_number: Some(task_number),
        step_number: Some(step_number),
        blocking_task: (status.execution_started == "yes"
            || current_task_closure_branch_route_facts.task_closure_set_present())
        .then_some(task_number),
        blocking_reason_codes: status.reason_codes.clone(),
    })
}
