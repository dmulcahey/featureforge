use std::path::PathBuf;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::closure_diagnostics::task_closure_dispatch_lineage_reason_code;
use crate::execution::context::{ExecutionContext, NoteState};
use crate::execution::current_truth::{
    current_final_review_dispatch_id as shared_current_final_review_dispatch_id,
    current_task_review_dispatch_id as shared_current_task_review_dispatch_id,
};
use crate::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use crate::execution::leases::{
    StatusAuthoritativeOverlay, load_status_authoritative_overlay_checked,
};
use crate::execution::phase;
use crate::execution::read_model::{
    final_review_dispatch_still_current_for_gates, has_authoritative_late_stage_progress,
    is_late_stage_phase, normalize_optional_overlay_value, parse_harness_phase,
    public_status_from_supplied_context_with_shared_routing, status_from_context,
    usable_current_branch_closure_identity,
    usable_current_branch_closure_identity_from_authoritative_state,
};
use crate::execution::read_model_support::{
    active_step, context_all_task_scopes_closed_by_authority, latest_attempted_step_for_task,
    pre_reducer_earliest_unresolved_stale_task,
    task_closure_baseline_repair_candidate_with_stale_target, task_closure_recording_prerequisites,
    task_completion_lineage_fingerprint,
};
use crate::execution::semantic_identity::semantic_workspace_snapshot;
use crate::execution::status::PlanExecutionStatus;
use crate::execution::transitions::{
    AuthoritativeTransitionState, load_authoritative_transition_state,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExistingTaskDispatchReviewedStateStatus {
    Current,
    MissingReviewedStateBinding,
    StaleReviewedState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskDispatchReviewedStateStatus {
    Current,
    MissingReviewedStateBinding,
    StaleReviewedState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewDispatchCycleTarget {
    Bound(u32, u32),
    UnboundCompletedPlan,
    None,
}

pub(crate) fn current_review_dispatch_id_candidate(
    context: &ExecutionContext,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    expected_dispatch_id: Option<&str>,
) -> Result<Option<String>, JsonFailure> {
    if let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(expected_dispatch_id.to_owned()));
    }
    let args = RecordReviewDispatchArgs {
        plan: PathBuf::from(context.plan_rel.clone()),
        scope,
        task,
    };
    current_review_dispatch_id_if_still_current(context, &args)
}

pub(crate) fn current_review_dispatch_id_if_still_current(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
) -> Result<Option<String>, JsonFailure> {
    let lineage_dispatch_id = current_review_dispatch_id_from_lineage(context, args)?;
    Ok(match args.scope {
        ReviewDispatchScopeArg::Task => lineage_dispatch_id,
        ReviewDispatchScopeArg::FinalReview => {
            let Some(dispatch_id) = lineage_dispatch_id else {
                return Ok(None);
            };
            let authoritative_state = load_authoritative_transition_state(context)?;
            let runtime_state = crate::execution::reducer::reduce_runtime_state(
                context,
                authoritative_state.as_ref(),
                semantic_workspace_snapshot(context)?,
            )?;
            final_review_dispatch_still_current_for_gates(
                runtime_state.gate_snapshot.gate_review.as_ref(),
                runtime_state.gate_snapshot.gate_finish.as_ref(),
            )
            .then_some(dispatch_id)
        }
    })
}

pub(crate) fn current_review_dispatch_id_from_lineage(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
) -> Result<Option<String>, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let Some(overlay) = overlay else {
        return Ok(None);
    };
    let current_task_semantic_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    Ok(match args.scope {
        ReviewDispatchScopeArg::Task => shared_current_task_review_dispatch_id(
            args.task,
            args.task
                .and_then(|task| task_completion_lineage_fingerprint(context, task))
                .as_deref(),
            current_task_semantic_reviewed_state_id.as_deref(),
            None,
            Some(&overlay),
        ),
        ReviewDispatchScopeArg::FinalReview => {
            let usable_current_branch_closure_id = usable_current_branch_closure_identity(context)
                .map(|identity| identity.branch_closure_id);
            shared_current_final_review_dispatch_id(
                usable_current_branch_closure_id.as_deref(),
                Some(&overlay),
            )
            .or_else(|| {
                let authoritative_state = load_authoritative_transition_state(context).ok()?;
                let authoritative_branch_closure_id =
                    usable_current_branch_closure_identity_from_authoritative_state(
                        context,
                        authoritative_state.as_ref(),
                    )
                    .map(|identity| identity.branch_closure_id);
                current_final_review_dispatch_id_from_authority(
                    authoritative_branch_closure_id
                        .as_deref()
                        .or(usable_current_branch_closure_id.as_deref()),
                    Some(&overlay),
                    authoritative_state.as_ref(),
                )
            })
        }
    })
}

pub(crate) fn current_task_review_dispatch_id_for_task(
    context: &ExecutionContext,
    task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Option<String> {
    let current_task_lineage_fingerprint =
        task.and_then(|task_number| task_completion_lineage_fingerprint(context, task_number));
    let current_task_semantic_reviewed_state_id = task.and_then(|_| {
        semantic_workspace_snapshot(context)
            .ok()
            .map(|snapshot| snapshot.semantic_workspace_tree_id)
    });
    shared_current_task_review_dispatch_id(
        task,
        current_task_lineage_fingerprint.as_deref(),
        current_task_semantic_reviewed_state_id.as_deref(),
        None,
        overlay,
    )
}

pub(crate) fn current_final_review_dispatch_id_from_authority(
    usable_current_branch_closure_id: Option<&str>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<String> {
    shared_current_final_review_dispatch_id(usable_current_branch_closure_id, overlay).or_else(
        || {
            authoritative_state.and_then(|state| {
                if state.current_final_review_dispatch_lineage_branch_closure_id()
                    == usable_current_branch_closure_id
                {
                    return state
                        .current_final_review_dispatch_lineage_dispatch_id()
                        .or_else(|| state.current_final_review_dispatch_id())
                        .map(str::trim)
                        .filter(|dispatch_id| !dispatch_id.is_empty())
                        .map(ToOwned::to_owned);
                }
                if state.current_final_review_branch_closure_id()
                    == usable_current_branch_closure_id
                {
                    return state
                        .current_final_review_dispatch_id()
                        .map(str::trim)
                        .filter(|dispatch_id| !dispatch_id.is_empty())
                        .map(ToOwned::to_owned);
                }
                None
            })
        },
    )
}

pub(crate) fn ensure_task_dispatch_id_matches(
    context: &ExecutionContext,
    task: u32,
    dispatch_id: &str,
) -> Result<(), JsonFailure> {
    let dispatch_args = RecordReviewDispatchArgs {
        plan: PathBuf::from(context.plan_rel.clone()),
        scope: ReviewDispatchScopeArg::Task,
        task: Some(task),
    };
    let expected_dispatch_from_lineage =
        current_review_dispatch_id_from_lineage(context, &dispatch_args)?;
    if let Some(expected_dispatch) = expected_dispatch_from_lineage.as_deref() {
        if expected_dispatch.trim() != dispatch_id.trim() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "dispatch_id_mismatch: close-current-task expected dispatch `{}` for task {task}.",
                    expected_dispatch.trim()
                ),
            ));
        }
        return Ok(());
    }
    Err(JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        format!(
            "close-current-task requires a current task review dispatch lineage for task {task}."
        ),
    ))
}

pub(crate) fn task_dispatch_reviewed_state_status(
    context: &ExecutionContext,
    task: u32,
    semantic_reviewed_state_id: &str,
    raw_reviewed_state_id: &str,
) -> Result<TaskDispatchReviewedStateStatus, JsonFailure> {
    existing_task_dispatch_reviewed_state_status(
        context,
        task,
        semantic_reviewed_state_id,
        raw_reviewed_state_id,
    )?
    .map(|status| match status {
        ExistingTaskDispatchReviewedStateStatus::Current => {
            TaskDispatchReviewedStateStatus::Current
        }
        ExistingTaskDispatchReviewedStateStatus::MissingReviewedStateBinding => {
            TaskDispatchReviewedStateStatus::MissingReviewedStateBinding
        }
        ExistingTaskDispatchReviewedStateStatus::StaleReviewedState => {
            TaskDispatchReviewedStateStatus::StaleReviewedState
        }
    })
    .ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "close-current-task requires authoritative review-dispatch lineage state.",
        )
    })
}

pub(crate) fn ensure_final_review_dispatch_id_matches(
    context: &ExecutionContext,
    dispatch_id: &str,
) -> Result<(), JsonFailure> {
    let current_branch_closure =
        usable_current_branch_closure_identity(context).ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage final-review requires a current branch closure.",
            )
        })?;
    let dispatch_args = RecordReviewDispatchArgs {
        plan: PathBuf::from(context.plan_rel.clone()),
        scope: ReviewDispatchScopeArg::FinalReview,
        task: None,
    };
    let expected_dispatch_from_lineage =
        current_review_dispatch_id_from_lineage(context, &dispatch_args)?;
    if let Some(expected_dispatch) = expected_dispatch_from_lineage.as_deref() {
        if expected_dispatch.trim() != dispatch_id.trim() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "dispatch_id_mismatch: advance-late-stage expected final-review dispatch `{}`.",
                    expected_dispatch.trim()
                ),
            ));
        }
        return Ok(());
    }
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage final-review path requires authoritative dispatch lineage state.",
        )
    })?;
    let branch_lineage_matches = overlay
        .final_review_dispatch_lineage
        .as_ref()
        .and_then(|record| record.branch_closure_id.as_deref())
        .is_some_and(|branch_closure_id| {
            branch_closure_id == current_branch_closure.branch_closure_id
        });
    Err(JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        if branch_lineage_matches {
            "advance-late-stage final-review path requires a non-empty current final-review dispatch id."
        } else {
            "advance-late-stage final-review path requires a current final-review dispatch lineage."
        },
    ))
}

pub(crate) fn validate_review_dispatch_request(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<(), JsonFailure> {
    match args.scope {
        ReviewDispatchScopeArg::Task => {
            let requested_task = args.task.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "task-scoped review-dispatch recording requires --task <n>.",
                )
            })?;
            let observed_task = match cycle_target {
                ReviewDispatchCycleTarget::Bound(task, _) => task,
                ReviewDispatchCycleTarget::UnboundCompletedPlan => {
                    return Err(JsonFailure::new(
                        FailureClass::InvalidCommandInput,
                        format!(
                            "task-scoped review-dispatch recording for Task {requested_task} is invalid because the approved plan is already at final-review dispatch scope."
                        ),
                    ));
                }
                ReviewDispatchCycleTarget::None => {
                    return Err(JsonFailure::new(
                        FailureClass::ExecutionStateNotReady,
                        "task-scoped review-dispatch recording requires a current task review-dispatch target.",
                    ));
                }
            };
            if requested_task != observed_task {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    format!(
                        "task-scoped review-dispatch recording for Task {requested_task} does not match the current task review-dispatch target Task {observed_task} for plan {}.",
                        context.plan_rel
                    ),
                ));
            }
            Ok(())
        }
        ReviewDispatchScopeArg::FinalReview => {
            if args.task.is_some() {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "final-review dispatch recording does not accept --task.",
                ));
            }
            match cycle_target {
                ReviewDispatchCycleTarget::UnboundCompletedPlan => Ok(()),
                ReviewDispatchCycleTarget::Bound(_, _)
                    if context_all_task_scopes_closed_by_authority(context, None) =>
                {
                    Ok(())
                }
                ReviewDispatchCycleTarget::Bound(_, _) | ReviewDispatchCycleTarget::None => {
                    Err(JsonFailure::new(
                        FailureClass::ExecutionStateNotReady,
                        "final-review dispatch recording requires a completed-plan dispatch target.",
                    ))
                }
            }
        }
    }
}

pub(crate) fn review_dispatch_cycle_target(
    context: &ExecutionContext,
) -> ReviewDispatchCycleTarget {
    if let Some(boundary_target) = review_dispatch_task_boundary_target(context) {
        return boundary_target;
    }
    for state in [
        NoteState::Active,
        NoteState::Blocked,
        NoteState::Interrupted,
    ] {
        if let Some(step) = active_step(context, state) {
            return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
        }
    }
    if context_all_task_scopes_closed_by_authority(context, None) {
        let overlay = load_status_authoritative_overlay_checked(context)
            .ok()
            .and_then(|overlay| overlay);
        let authoritative_phase = overlay.as_ref().and_then(|overlay| {
            normalize_optional_overlay_value(overlay.harness_phase.as_deref())
                .and_then(parse_harness_phase)
        });
        if authoritative_phase.is_some_and(is_late_stage_phase)
            || overlay
                .as_ref()
                .is_some_and(has_authoritative_late_stage_progress)
        {
            return ReviewDispatchCycleTarget::UnboundCompletedPlan;
        }
        if let Some(final_task) = context.tasks_by_number.keys().copied().max() {
            let final_task_closure_missing = load_authoritative_transition_state(context)
                .ok()
                .and_then(|state| state)
                .and_then(|state| {
                    (!state.current_task_closure_overlay_needs_restore()).then_some(state)
                })
                .and_then(|state| state.raw_current_task_closure_result(final_task))
                .is_none();
            if final_task_closure_missing
                && let Some(final_step) = context
                    .steps
                    .iter()
                    .filter(|step| step.task_number == final_task)
                    .map(|step| step.step_number)
                    .max()
            {
                return ReviewDispatchCycleTarget::Bound(final_task, final_step);
            }
        }
        return ReviewDispatchCycleTarget::UnboundCompletedPlan;
    }
    if let Some(attempt) = context.evidence.attempts.iter().rev().find(|attempt| {
        context.steps.iter().any(|step| {
            step.task_number == attempt.task_number && step.step_number == attempt.step_number
        })
    }) {
        return ReviewDispatchCycleTarget::Bound(attempt.task_number, attempt.step_number);
    }
    if let Some(step) = context.steps.iter().rev().find(|step| step.checked) {
        return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
    }
    if let Some(step) = context
        .steps
        .iter()
        .find(|step| step.note_state.is_some() && !step.checked)
    {
        return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
    }
    if !context.evidence.attempts.is_empty()
        && let Some(attempt) = context.evidence.attempts.last()
    {
        return ReviewDispatchCycleTarget::Bound(attempt.task_number, attempt.step_number);
    }
    ReviewDispatchCycleTarget::None
}

pub(crate) fn existing_task_dispatch_reviewed_state_status(
    context: &ExecutionContext,
    task: u32,
    semantic_reviewed_state_id: &str,
    raw_reviewed_state_id: &str,
) -> Result<Option<ExistingTaskDispatchReviewedStateStatus>, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let lineage_key = format!("task-{task}");
    let Some(record) = overlay
        .as_ref()
        .and_then(|overlay| overlay.strategy_review_dispatch_lineage.get(&lineage_key))
    else {
        return Ok(None);
    };
    let recorded_dispatch_id = record
        .dispatch_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if recorded_dispatch_id.is_none() {
        return Ok(Some(
            ExistingTaskDispatchReviewedStateStatus::MissingReviewedStateBinding,
        ));
    }
    let recorded_semantic_reviewed_state_id = record
        .semantic_reviewed_state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let recorded_raw_reviewed_state_id = record
        .reviewed_state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(Some(
        match (
            recorded_semantic_reviewed_state_id,
            recorded_raw_reviewed_state_id,
        ) {
            (Some(recorded), _) if recorded == semantic_reviewed_state_id.trim() => {
                ExistingTaskDispatchReviewedStateStatus::Current
            }
            (Some(_), _) => ExistingTaskDispatchReviewedStateStatus::StaleReviewedState,
            (None, Some(recorded)) if recorded == raw_reviewed_state_id.trim() => {
                ExistingTaskDispatchReviewedStateStatus::Current
            }
            (None, Some(_)) => ExistingTaskDispatchReviewedStateStatus::StaleReviewedState,
            (None, None) => ExistingTaskDispatchReviewedStateStatus::MissingReviewedStateBinding,
        },
    ))
}

pub(crate) fn validate_expected_dispatch_id(
    actual_dispatch_id: &str,
    expected_dispatch_id: Option<&str>,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
) -> Result<(), JsonFailure> {
    let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    if actual_dispatch_id.trim() == expected_dispatch_id {
        return Ok(());
    }
    let detail = expected_dispatch_id_mismatch_detail(expected_dispatch_id, scope, task);
    Err(JsonFailure::new(
        FailureClass::InvalidCommandInput,
        format_dispatch_id_mismatch_message(&detail),
    ))
}

pub(crate) fn expected_dispatch_id_mismatch_error(
    expected_dispatch_id: &str,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
) -> JsonFailure {
    let detail = expected_dispatch_id_mismatch_detail(expected_dispatch_id, scope, task);
    JsonFailure::new(
        FailureClass::InvalidCommandInput,
        format_dispatch_id_mismatch_message(&detail),
    )
}

fn expected_dispatch_id_mismatch_detail(
    expected_dispatch_id: &str,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
) -> String {
    match scope {
        ReviewDispatchScopeArg::Task => format!(
            "close-current-task expected dispatch `{expected_dispatch_id}` for task {}.",
            task.unwrap_or_default()
        ),
        ReviewDispatchScopeArg::FinalReview => {
            format!("advance-late-stage expected final-review dispatch `{expected_dispatch_id}`.")
        }
    }
}

fn format_dispatch_id_mismatch_message(detail: &str) -> String {
    format!("dispatch_id_mismatch: {detail}")
}

fn review_dispatch_task_boundary_target(
    context: &ExecutionContext,
) -> Option<ReviewDispatchCycleTarget> {
    let status = public_status_from_supplied_context_with_shared_routing(context, false)
        .or_else(|_| status_from_context(context))
        .ok();
    if let Some(public_close_target) = status
        .as_ref()
        .and_then(|status| public_close_current_task_cycle_target(context, status))
    {
        return Some(public_close_target);
    }
    let earliest_stale_boundary_task = status
        .as_ref()
        .and_then(|status| pre_reducer_earliest_unresolved_stale_task(context, status));
    if let Some(stale_task) = earliest_stale_boundary_task
        .filter(|task_number| review_dispatch_boundary_blocked_for_task(context, *task_number))
    {
        let step_number = latest_attempted_step_for_task(context, stale_task).or_else(|| {
            context
                .steps
                .iter()
                .filter(|step| step.task_number == stale_task)
                .map(|step| step.step_number)
                .max()
        })?;
        return Some(ReviewDispatchCycleTarget::Bound(stale_task, step_number));
    }
    if let Some(status) = status.as_ref() {
        let boundary_reason_present = status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_current_closure_stale"
                    | "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
                    | "task_cycle_break_active"
            )
        });
        if boundary_reason_present
            && let Some(task_number) = status
                .blocking_task
                .or(status.resume_task)
                .or(status.active_task)
            && review_dispatch_boundary_blocked_for_task(context, task_number)
        {
            let step_number =
                latest_attempted_step_for_task(context, task_number).or_else(|| {
                    context
                        .steps
                        .iter()
                        .filter(|step| step.task_number == task_number)
                        .map(|step| step.step_number)
                        .max()
                })?;
            return Some(ReviewDispatchCycleTarget::Bound(task_number, step_number));
        }
    }
    let task_number = status.as_ref().and_then(|status| {
        context
            .tasks_by_number
            .keys()
            .copied()
            .filter(|candidate_task| {
                review_dispatch_boundary_blocked_for_task(context, *candidate_task)
            })
            .find(|candidate_task| {
                task_closure_baseline_repair_candidate_with_stale_target(
                    context,
                    status,
                    *candidate_task,
                    pre_reducer_earliest_unresolved_stale_task(context, status),
                )
                .ok()
                .flatten()
                .is_some()
            })
    })?;
    let step_number = latest_attempted_step_for_task(context, task_number).or_else(|| {
        context
            .steps
            .iter()
            .filter(|step| step.task_number == task_number)
            .map(|step| step.step_number)
            .max()
    })?;
    Some(ReviewDispatchCycleTarget::Bound(task_number, step_number))
}

fn public_close_current_task_cycle_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Option<ReviewDispatchCycleTarget> {
    let close_current_task_route = status.phase_detail
        == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        || status.next_action == "close current task";
    if !close_current_task_route {
        return None;
    }
    let task_number = status
        .public_repair_targets
        .iter()
        .find(|target| target.command_kind == "close-current-task")
        .and_then(|target| target.task)
        .or_else(|| {
            status
                .recording_context
                .as_ref()
                .and_then(|context| context.task_number)
        })
        .or(status.blocking_task)?;
    let step_number = latest_attempted_step_for_task(context, task_number).or_else(|| {
        context
            .steps
            .iter()
            .filter(|step| step.task_number == task_number)
            .map(|step| step.step_number)
            .max()
    })?;
    Some(ReviewDispatchCycleTarget::Bound(task_number, step_number))
}

fn review_dispatch_boundary_blocked_for_task(context: &ExecutionContext, task_number: u32) -> bool {
    task_closure_recording_prerequisites(context, task_number)
        .ok()
        .is_some_and(|prerequisites| {
            prerequisites
                .dispatch_id
                .as_deref()
                .is_none_or(|dispatch_id| dispatch_id.trim().is_empty())
                || prerequisites
                    .blocking_reason_codes
                    .iter()
                    .chain(prerequisites.diagnostic_reason_codes.iter())
                    .any(|reason_code| task_closure_dispatch_lineage_reason_code(reason_code))
        })
}
