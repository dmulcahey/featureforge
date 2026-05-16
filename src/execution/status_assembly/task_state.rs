use std::collections::BTreeSet;

use crate::execution::closure_diagnostics::{
    push_projection_diagnostic_once, task_boundary_projection_diagnostic_reason_code,
    task_closure_recording_status_reason_codes,
};
use crate::execution::context::ExecutionContext;
use crate::execution::current_closure_projection::CurrentTaskClosureStructuralFailure;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, branch_closure_rerecording_assessment_with_authority,
    late_stage_surface_not_declared_reason_code as shared_late_stage_surface_not_declared_reason_code,
};
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::status::PlanExecutionStatus;
use crate::execution::status_support::{
    TaskBoundaryAuthorityInputs, prior_task_number_for_begin,
    projected_earliest_stale_task_from_status, require_prior_task_closure_for_begin_with_authority,
    stale_unreviewed_allows_task_closure_baseline_bridge_with_authority,
    task_boundary_reason_code_from_message,
    task_closure_baseline_repair_candidate_with_stale_target_and_authority,
    task_closure_recording_prerequisites_with_authority,
};

use super::{is_late_stage_phase, push_status_reason_code_once};

pub(crate) fn apply_task_boundary_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    inputs: TaskBoundaryStatusInputs<'_>,
) {
    let TaskBoundaryStatusInputs {
        late_stage_basis_present,
        current_task_closure_tasks,
        authoritative_late_stage_progress_present,
        authority,
        branch_rerecording_assessment,
    } = inputs;
    if status.blocking_task.is_some() {
        return;
    }
    if let Some(active_task) = status.active_task {
        if projected_earliest_stale_task_from_status(status).is_none()
            && let Some(prior_task) = prior_task_number_for_begin(context, active_task)
            && let Err(error) = require_prior_task_closure_for_begin_with_authority(
                context,
                active_task,
                authority.authoritative_state(),
                authority.overlay(),
            )
        {
            let mut missing_current_closure_boundary = false;
            if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
                && !status
                    .reason_codes
                    .iter()
                    .any(|existing| existing == reason_code)
            {
                status.reason_codes.push(reason_code.to_owned());
                missing_current_closure_boundary =
                    reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING;
            }
            status.blocking_task = Some(prior_task);
            status.blocking_step = None;
            status.active_task = None;
            status.active_step = None;
            if missing_current_closure_boundary {
                push_task_closure_recording_status_reasons(
                    context,
                    status,
                    prior_task,
                    authority,
                    branch_rerecording_assessment,
                );
            }
        }
        return;
    }
    if let Some(resume_task) = status.resume_task {
        if projected_earliest_stale_task_from_status(status).is_none()
            && let Some(prior_task) = prior_task_number_for_begin(context, resume_task)
            && let Err(error) = require_prior_task_closure_for_begin_with_authority(
                context,
                resume_task,
                authority.authoritative_state(),
                authority.overlay(),
            )
        {
            let mut missing_current_closure_boundary = false;
            if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
                && !status
                    .reason_codes
                    .iter()
                    .any(|existing| existing == reason_code)
            {
                status.reason_codes.push(reason_code.to_owned());
                missing_current_closure_boundary =
                    reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING;
            }
            status.blocking_task = Some(prior_task);
            status.blocking_step = None;
            status.resume_task = None;
            status.resume_step = None;
            if missing_current_closure_boundary {
                push_task_closure_recording_status_reasons(
                    context,
                    status,
                    prior_task,
                    authority,
                    branch_rerecording_assessment,
                );
            }
        }
        return;
    }
    let Some(next_unchecked_task) = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number)
    else {
        let Some(missing_task) = completed_plan_missing_current_closure_task_from_records(
            context,
            current_task_closure_tasks,
        ) else {
            return;
        };
        let stale_provenance_recovery_candidate = status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
            && !status
                .reason_codes
                .iter()
                .any(|reason_code| shared_late_stage_surface_not_declared_reason_code(reason_code));
        if !stale_provenance_recovery_candidate
            && ((status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
                && status.harness_phase != HarnessPhase::Executing)
                || is_late_stage_phase(status.harness_phase)
                || late_stage_basis_present
                || authoritative_late_stage_progress_present)
        {
            return;
        }
        if !stale_provenance_recovery_candidate {
            push_task_closure_recording_status_reasons(
                context,
                status,
                missing_task,
                authority,
                branch_rerecording_assessment,
            );
        }
        push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING);
        status.blocking_task = Some(missing_task);
        status.blocking_step = None;
        return;
    };
    {
        let Some(prior_task) = prior_task_number_for_begin(context, next_unchecked_task) else {
            return;
        };
        let Err(error) = require_prior_task_closure_for_begin_with_authority(
            context,
            next_unchecked_task,
            authority.authoritative_state(),
            authority.overlay(),
        ) else {
            return;
        };
        let mut missing_current_closure_boundary = false;
        if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
            && !status
                .reason_codes
                .iter()
                .any(|existing| existing == reason_code)
        {
            status.reason_codes.push(reason_code.to_owned());
            missing_current_closure_boundary = reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING;
        }
        status.blocking_task = Some(prior_task);
        if missing_current_closure_boundary {
            push_task_closure_recording_status_reasons(
                context,
                status,
                prior_task,
                authority,
                branch_rerecording_assessment,
            );
        }
    }
}

pub(crate) struct TaskBoundaryStatusInputs<'a> {
    pub(crate) late_stage_basis_present: bool,
    pub(crate) current_task_closure_tasks: &'a BTreeSet<u32>,
    pub(crate) authoritative_late_stage_progress_present: bool,
    pub(crate) authority: TaskBoundaryAuthorityInputs<'a>,
    pub(crate) branch_rerecording_assessment: Option<&'a BranchRerecordingAssessment>,
}

fn push_task_closure_recording_status_reasons(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    task: u32,
    authority: TaskBoundaryAuthorityInputs<'_>,
    branch_rerecording_assessment: Option<&BranchRerecordingAssessment>,
) {
    let Ok(prerequisites) = task_closure_recording_prerequisites_with_authority(
        context,
        task,
        authority.overlay(),
        authority.authoritative_state(),
    ) else {
        return;
    };
    let current_dispatch_ready = prerequisites
        .dispatch_id
        .as_deref()
        .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty());
    let fallback_branch_rerecording_assessment;
    let branch_rerecording_assessment = match branch_rerecording_assessment {
        Some(assessment) => Some(assessment),
        None => match authority.authoritative_state() {
            Some(state) => {
                let Ok(assessment) =
                    branch_closure_rerecording_assessment_with_authority(context, Some(state))
                else {
                    return;
                };
                fallback_branch_rerecording_assessment = assessment;
                Some(&fallback_branch_rerecording_assessment)
            }
            None => None,
        },
    };
    let baseline_candidate_present = branch_rerecording_assessment
        .and_then(|assessment| {
            task_closure_baseline_repair_candidate_with_stale_target_and_authority(
                context,
                status,
                task,
                projected_earliest_stale_task_from_status(status),
                authority.overlay(),
                authority.authoritative_state(),
                assessment,
            )
            .ok()
            .flatten()
        })
        .is_some();
    let stale_bridge_ready = stale_unreviewed_allows_task_closure_baseline_bridge_with_authority(
        context,
        status,
        task,
        authority.overlay(),
        authority.authoritative_state(),
    )
    .unwrap_or(false);
    if current_dispatch_ready || baseline_candidate_present {
        push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE);
    }
    if stale_bridge_ready {
        push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_BRIDGE_READY);
    }
    for reason_code in
        task_closure_recording_status_reason_codes(&prerequisites.blocking_reason_codes)
    {
        push_status_reason_code_once(status, &reason_code);
    }
    for reason_code in prerequisites
        .diagnostic_reason_codes
        .iter()
        .filter(|reason_code| task_boundary_projection_diagnostic_reason_code(reason_code))
    {
        push_projection_diagnostic_once(status, reason_code);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionReentryCurrentTaskClosureTargets {
    pub(crate) stale_tasks: Vec<u32>,
    pub(crate) structural_tasks: Vec<u32>,
    pub(crate) structural_scope_keys: Vec<String>,
}

pub(crate) fn execution_reentry_current_task_closure_targets_from_inputs(
    stale_tasks: impl IntoIterator<Item = u32>,
    structural_failures: impl IntoIterator<Item = CurrentTaskClosureStructuralFailure>,
) -> ExecutionReentryCurrentTaskClosureTargets {
    let stale_tasks = stale_tasks.into_iter().collect::<BTreeSet<_>>();
    let mut structural_tasks = BTreeSet::new();
    let mut structural_scope_keys = BTreeSet::new();
    for failure in structural_failures {
        if let Some(task_number) = failure.task {
            structural_tasks.insert(task_number);
        } else {
            structural_scope_keys.insert(failure.scope_key);
        }
    }

    ExecutionReentryCurrentTaskClosureTargets {
        stale_tasks: stale_tasks.into_iter().collect(),
        structural_tasks: structural_tasks.into_iter().collect(),
        structural_scope_keys: structural_scope_keys.into_iter().collect(),
    }
}

pub(crate) fn recommended_execution_source(execution_mode: &str) -> &str {
    match execution_mode {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
            execution_mode
        }
        _ => "featureforge:executing-plans",
    }
}

pub(super) fn completed_plan_missing_current_closure_task_from_records(
    context: &ExecutionContext,
    current_task_closure_tasks: &BTreeSet<u32>,
) -> Option<u32> {
    if context.steps.iter().any(|step| !step.checked) {
        return None;
    }
    let highest_current_task_closure = current_task_closure_tasks.iter().next_back().copied();
    let mut completed_tasks = context
        .steps
        .iter()
        .filter(|step| step.checked)
        .map(|step| step.task_number)
        .collect::<Vec<_>>();
    completed_tasks.sort_unstable();
    completed_tasks.dedup();
    completed_tasks.into_iter().find(|task| {
        !current_task_closure_tasks.contains(task)
            && highest_current_task_closure.is_none_or(|current_task| *task > current_task)
    })
}
