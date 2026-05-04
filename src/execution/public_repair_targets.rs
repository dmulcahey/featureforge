use crate::execution::closure_dispatch::current_review_dispatch_id_if_still_current;
use crate::execution::current_truth::{
    legacy_repair_follow_up_unbound,
    resolve_actionable_repair_follow_up_for_status_with_source_hash,
};
use crate::execution::follow_up::{RepairFollowUpRecord, execution_step_repair_target_id};
use crate::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use crate::execution::recording::current_task_closure_postconditions_would_mutate;
use crate::execution::state::{ExecutionContext, PlanExecutionStatus, PublicRepairTarget};
use crate::execution::topology::load_preflight_acceptance;
use crate::execution::transitions::AuthoritativeTransitionState;

pub(crate) fn public_repair_target_warning_codes(
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Vec<&'static str> {
    if legacy_repair_follow_up_unbound(authoritative_state) {
        vec!["legacy_follow_up_unbound"]
    } else {
        Vec::new()
    }
}

pub(crate) fn public_repair_target_candidates_from_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    source_route_decision_hash: Option<&str>,
) -> Vec<PublicRepairTarget> {
    let Some(authoritative_state) = authoritative_state else {
        return Vec::new();
    };
    let mut targets = Vec::new();
    let persisted_follow_up_record =
        resolve_actionable_repair_follow_up_for_status_with_source_hash(
            context,
            status,
            Some(authoritative_state),
            source_route_decision_hash,
        );
    let persisted_follow_up = persisted_follow_up_record
        .as_ref()
        .map(|record| record.kind.public_token());
    push_persisted_follow_up_target(
        &mut targets,
        persisted_follow_up,
        persisted_follow_up_record.as_ref(),
    );
    if persisted_follow_up == Some("close_current_task") {
        return targets;
    }
    push_authoritative_reopen_targets(&mut targets, authoritative_state);
    push_task_closure_repair_targets(&mut targets, context, status, authoritative_state);
    if persisted_follow_up == Some("execution_reentry") {
        push_persisted_execution_reentry_target(&mut targets, persisted_follow_up_record.as_ref());
    }
    targets
}

fn push_persisted_follow_up_target(
    targets: &mut Vec<PublicRepairTarget>,
    persisted_follow_up: Option<&str>,
    persisted_follow_up_record: Option<&RepairFollowUpRecord>,
) {
    if let Some(follow_up) = persisted_follow_up {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: String::from("repair-review-state"),
                task: persisted_follow_up_record.and_then(|record| record.target_task),
                step: persisted_follow_up_record.and_then(|record| record.target_step),
                reason_code: format!("persisted_review_state_repair_follow_up:{follow_up}"),
                source_record_id: persisted_follow_up_record
                    .and_then(|record| record.target_record_id.clone())
                    .or_else(|| Some(format!("review_state_repair_follow_up:{follow_up}"))),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    if persisted_follow_up == Some("close_current_task")
        && let Some(task) = persisted_follow_up_record.and_then(|record| record.target_task)
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("persisted_task_closure_follow_up"),
                source_record_id: persisted_follow_up_record
                    .and_then(|record| record.target_record_id.clone())
                    .or_else(|| Some(format!("review_state_repair_follow_up_task:{task}"))),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn push_authoritative_reopen_targets(
    targets: &mut Vec<PublicRepairTarget>,
    authoritative_state: &AuthoritativeTransitionState,
) {
    for target in authoritative_state.explicit_reopen_repair_targets() {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: String::from("reopen"),
                task: Some(target.task),
                step: Some(target.step),
                reason_code: String::from("explicit_reopen_repair_target"),
                source_record_id: target
                    .target_record_id
                    .or_else(|| Some(execution_step_repair_target_id(target.task, target.step))),
                expires_when_fingerprint_changes: target.expires_on_plan_fingerprint_change,
            },
        );
    }
}

fn push_task_closure_repair_targets(
    targets: &mut Vec<PublicRepairTarget>,
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: &AuthoritativeTransitionState,
) {
    for record in authoritative_state
        .current_task_closure_results()
        .into_values()
    {
        if current_task_closure_postconditions_would_mutate(
            authoritative_state,
            record.task,
            &record.closure_record_id,
            &record.reviewed_state_id,
        ) {
            push_public_repair_target_once(
                targets,
                PublicRepairTarget {
                    command_kind: String::from("close-current-task"),
                    task: Some(record.task),
                    step: None,
                    reason_code: String::from("authoritative_task_closure_postcondition_cleanup"),
                    source_record_id: Some(record.closure_record_id),
                    expires_when_fingerprint_changes: true,
                },
            );
        }
    }
    push_dispatch_closure_ready_targets(targets, context, authoritative_state);
    if authoritative_state.execution_run_id_opt().is_some()
        && load_preflight_acceptance(&context.runtime).is_err()
    {
        push_preflight_recovery_closure_targets(targets, authoritative_state);
    }
    if status.phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && let Some(task) = status
            .recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("status_task_closure_recording_ready"),
                source_record_id: Some(format!("status_task_closure_recording_ready:{task}")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn push_dispatch_closure_ready_targets(
    targets: &mut Vec<PublicRepairTarget>,
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) {
    for task in context.tasks_by_number.keys().copied() {
        let dispatch_args = RecordReviewDispatchArgs {
            plan: context.plan_abs.clone(),
            scope: ReviewDispatchScopeArg::Task,
            task: Some(task),
        };
        if authoritative_state
            .current_task_closure_result(task)
            .is_some()
            || current_review_dispatch_id_if_still_current(context, &dispatch_args)
                .ok()
                .flatten()
                .is_none()
            || !context
                .steps
                .iter()
                .filter(|step| step.task_number == task)
                .all(|step| step.checked)
        {
            continue;
        }
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("task_review_dispatch_closure_ready"),
                source_record_id: Some(format!("task-review-dispatch:task-{task}")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn push_preflight_recovery_closure_targets(
    targets: &mut Vec<PublicRepairTarget>,
    authoritative_state: &AuthoritativeTransitionState,
) {
    for entry in authoritative_state
        .raw_current_task_closure_state_entries()
        .into_iter()
        .filter(|entry| entry.task.is_some())
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: entry.task,
                step: None,
                reason_code: String::from("authoritative_preflight_recovery_task_closure"),
                source_record_id: entry.closure_record_id,
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn push_persisted_execution_reentry_target(
    targets: &mut Vec<PublicRepairTarget>,
    record: Option<&RepairFollowUpRecord>,
) {
    let Some(record) = record else {
        return;
    };
    let (Some(task), Some(step)) = (record.target_task, record.target_step) else {
        return;
    };
    push_public_repair_target_once(
        targets,
        PublicRepairTarget {
            command_kind: String::from("reopen"),
            task: Some(task),
            step: Some(step),
            reason_code: String::from("persisted_execution_reentry_follow_up"),
            source_record_id: record
                .target_record_id
                .clone()
                .or_else(|| Some(format!("review_state_repair_follow_up_task:{task}"))),
            expires_when_fingerprint_changes: true,
        },
    );
}

fn push_public_repair_target_once(
    targets: &mut Vec<PublicRepairTarget>,
    target: PublicRepairTarget,
) {
    if !targets.iter().any(|existing| {
        existing.command_kind == target.command_kind
            && existing.task == target.task
            && existing.step == target.step
    }) {
        targets.push(target);
    }
}
