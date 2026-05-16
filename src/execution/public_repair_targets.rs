use std::collections::BTreeSet;

use crate::execution::authority::active_worktree_lease_release_preview_from_authority;
use crate::execution::closure_diagnostics::{
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
    TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
};
use crate::execution::closure_dispatch::current_review_dispatch_id_if_still_current;
use crate::execution::command_eligibility::{PublicCommand, PublicCommandKind};
use crate::execution::current_task_closure_cleanup::{
    current_task_closure_postconditions_would_mutate,
    worktree_lease_release_decision_for_current_task_closures_from_authority,
};
use crate::execution::current_truth::{
    legacy_repair_follow_up_unbound,
    resolve_actionable_repair_follow_up_for_status_with_source_hash,
    stale_provenance_after_authoritative_closure_is_diagnostic,
};
use crate::execution::follow_up::{RepairFollowUpRecord, execution_step_repair_target_id};
use crate::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::phase;
use crate::execution::public_repair_target_reasons::{
    PublicRepairTargetReason, persisted_review_state_repair_follow_up_reason,
};
use crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE;
use crate::execution::route_plan::{
    RouteDecision, external_wait_state_is_external_wait, state_kind_blocks_local_mutation,
    state_kind_is_blocked_runtime_bug, targetless_stale_reconcile_for_phase,
};
use crate::execution::state::{
    ExecutionContext, PlanExecutionStatus, PublicRepairTarget,
    current_branch_closure_structural_review_state_reason,
    task_scope_structural_review_state_reason,
};
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

pub(crate) fn close_current_task_public_repair_target_for_task<'a>(
    targets: impl IntoIterator<Item = &'a PublicRepairTarget>,
    task: u32,
) -> bool {
    targets.into_iter().any(|target| {
        PublicCommandKind::CloseCurrentTask.matches_public_mutation_token(&target.command_kind)
            && target.task == Some(task)
            && target.step.is_none()
    })
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
    if persisted_follow_up
        == Some(crate::execution::review_route_tokens::FOLLOW_UP_CLOSE_CURRENT_TASK)
    {
        return targets;
    }
    push_authoritative_reopen_targets(&mut targets, authoritative_state);
    push_task_closure_repair_targets(&mut targets, context, status, authoritative_state);
    if persisted_follow_up
        == Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY)
    {
        push_persisted_execution_reentry_target(&mut targets, persisted_follow_up_record.as_ref());
    }
    targets
}

pub(crate) fn public_repair_targets_for_route_decision(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
    route_repair_target_candidates: &[PublicRepairTarget],
) -> Vec<PublicRepairTarget> {
    if route_decision_is_diagnostic_only(route_decision) {
        return Vec::new();
    }
    if targetless_stale_reconcile_for_phase(
        &route_decision.phase_detail,
        &route_decision.blocking_reason_codes,
    ) {
        return Vec::new();
    }

    let mut targets = Vec::new();
    if external_wait_state_is_external_wait(status.external_wait_state.as_deref()) {
        return targets;
    }
    push_route_owned_reopen_target(&mut targets, route_decision);
    push_route_owned_task_closure_target(&mut targets, route_decision);
    push_allowed_authority_candidates(
        &mut targets,
        status,
        route_decision,
        route_repair_target_candidates,
    );
    push_route_owned_repair_review_state_target(&mut targets, status, route_decision);
    push_route_owned_advance_late_stage_target(&mut targets, route_decision);
    targets
}

fn route_decision_is_diagnostic_only(route_decision: &RouteDecision) -> bool {
    crate::execution::next_action::runtime_route_is_diagnostic(
        &route_decision.state_kind,
        &route_decision.phase_detail,
    )
}

fn push_route_owned_reopen_target(
    targets: &mut Vec<PublicRepairTarget>,
    route_decision: &RouteDecision,
) {
    if let Some(route_request) = route_decision
        .recommended_public_command
        .as_ref()
        .and_then(PublicCommand::to_mutation_request)
        && route_request.kind == PublicCommandKind::Reopen
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: public_command_kind_token(PublicCommandKind::Reopen),
                task: route_request.task,
                step: route_request.step,
                reason_code: PublicRepairTargetReason::RouteExecutionReentryRequired.reason_code(),
                source_record_id: Some(String::from("route_decision:reopen")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn push_route_owned_task_closure_target(
    targets: &mut Vec<PublicRepairTarget>,
    route_decision: &RouteDecision,
) {
    if route_decision.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && let Some(task) = route_decision
            .recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: public_command_kind_token(PublicCommandKind::CloseCurrentTask),
                task: Some(task),
                step: None,
                reason_code: PublicRepairTargetReason::RouteTaskClosureRecordingReady.reason_code(),
                source_record_id: Some(String::from("route_decision:task_closure_recording_ready")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn push_allowed_authority_candidates(
    targets: &mut Vec<PublicRepairTarget>,
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
    route_repair_target_candidates: &[PublicRepairTarget],
) {
    for candidate in route_repair_target_candidates {
        if route_allows_public_repair_target_candidate(status, route_decision, candidate) {
            push_public_repair_target_once(targets, candidate.clone());
        }
    }
}

fn push_route_owned_repair_review_state_target(
    targets: &mut Vec<PublicRepairTarget>,
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
) {
    let route_exposes_task_closure_repair = route_decision.phase_detail
        == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && route_decision
            .blocking_reason_codes
            .iter()
            .any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
                        | TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
                )
            });
    let repair_review_state_target_allowed = route_decision.phase_detail
        == phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        || !state_kind_blocks_local_mutation(&route_decision.state_kind);
    if (route_exposes_task_closure_repair
        || route_decision_exposes_repair_review_state_target(status, route_decision))
        && repair_review_state_target_allowed
    {
        let reason_code = if route_exposes_task_closure_repair {
            PublicRepairTargetReason::RouteTaskClosureRepairStateRefresh
        } else {
            PublicRepairTargetReason::RouteRepairReviewStateAvailable
        };
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: public_command_kind_token(PublicCommandKind::RepairReviewState),
                task: None,
                step: None,
                reason_code: reason_code.reason_code(),
                source_record_id: Some(format!("route_decision:{}", route_decision.phase_detail)),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn push_route_owned_advance_late_stage_target(
    targets: &mut Vec<PublicRepairTarget>,
    route_decision: &RouteDecision,
) {
    if route_recommended_public_command_is(route_decision, PublicCommandKind::AdvanceLateStage)
        && route_decision.phase_detail != phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        && !state_kind_blocks_local_mutation(&route_decision.state_kind)
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: public_command_kind_token(PublicCommandKind::AdvanceLateStage),
                task: None,
                step: None,
                reason_code: PublicRepairTargetReason::RouteAdvanceLateStageReady.reason_code(),
                source_record_id: Some(String::from("route_decision:advance_late_stage")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn route_allows_public_repair_target_candidate(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
    candidate: &PublicRepairTarget,
) -> bool {
    match public_repair_target_kind(candidate) {
        Some(PublicCommandKind::Reopen) => {
            PublicRepairTargetReason::is_reopen_route_candidate(&candidate.reason_code)
                || (route_decision.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                    && candidate.task.is_some()
                    && route_reentry_target_matches_candidate(route_decision, candidate))
        }
        Some(PublicCommandKind::CloseCurrentTask) => {
            candidate.task.is_some()
                && (explicit_close_current_task_candidate(candidate)
                    || (route_decision.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
                        && route_recording_target_matches_candidate(route_decision, candidate)))
        }
        Some(PublicCommandKind::RepairReviewState) => {
            route_decision_exposes_repair_review_state_target(status, route_decision)
                && !state_kind_is_blocked_runtime_bug(&route_decision.state_kind)
        }
        Some(PublicCommandKind::AdvanceLateStage) => {
            route_recommended_public_command_is(route_decision, PublicCommandKind::AdvanceLateStage)
                && route_decision.phase_detail != phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
                && !state_kind_is_blocked_runtime_bug(&route_decision.state_kind)
        }
        _ => false,
    }
}

fn public_repair_target_kind(candidate: &PublicRepairTarget) -> Option<PublicCommandKind> {
    [
        PublicCommandKind::Reopen,
        PublicCommandKind::CloseCurrentTask,
        PublicCommandKind::RepairReviewState,
        PublicCommandKind::AdvanceLateStage,
    ]
    .into_iter()
    .find(|kind| kind.matches_public_mutation_token(&candidate.command_kind))
}

fn route_reentry_target_matches_candidate(
    route_decision: &RouteDecision,
    candidate: &PublicRepairTarget,
) -> bool {
    if let Some(route_request) = route_decision
        .recommended_public_command
        .as_ref()
        .and_then(PublicCommand::to_mutation_request)
        && route_request.kind == PublicCommandKind::Reopen
    {
        return candidate.task == route_request.task && candidate.step == route_request.step;
    }
    route_decision
        .execution_command_context
        .as_ref()
        .is_some_and(|context| {
            context.task_number == candidate.task && context.step_id == candidate.step
        })
}

fn route_recording_target_matches_candidate(
    route_decision: &RouteDecision,
    candidate: &PublicRepairTarget,
) -> bool {
    route_decision
        .recording_context
        .as_ref()
        .is_some_and(|context| context.task_number == candidate.task)
}

fn explicit_close_current_task_candidate(candidate: &PublicRepairTarget) -> bool {
    PublicRepairTargetReason::is_close_current_task_explicit(&candidate.reason_code)
}

fn route_recommended_public_command_is(
    route_decision: &RouteDecision,
    kind: PublicCommandKind,
) -> bool {
    route_decision
        .recommended_public_command
        .as_ref()
        .is_some_and(|command| command.kind() == kind)
}

pub(crate) fn route_decision_exposes_repair_review_state_target(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
) -> bool {
    let stale_provenance_diagnostic_only =
        stale_provenance_after_authoritative_closure_is_diagnostic(status)
            && route_decision.review_state_status == "clean";
    let phase_exposes_repair_review_state = matches!(
        route_decision.phase_detail.as_str(),
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            | phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
            | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
            | phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    ) && !stale_provenance_diagnostic_only;
    route_recommended_public_command_is(route_decision, PublicCommandKind::RepairReviewState)
        || route_decision.required_follow_up.as_deref() == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        || route_decision.review_state_status != "clean"
        || task_scope_structural_review_state_reason(status).is_some()
        || current_branch_closure_structural_review_state_reason(status).is_some()
        || status.blocking_records.iter().any(|record| {
            record.record_type == "review_state"
                && record.required_follow_up.as_deref() == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        })
        || phase_exposes_repair_review_state
        || route_decision
            .blocking_reason_codes
            .iter()
            .any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
                        | TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
                ) || (reason_code == REASON_CODE_STALE_PROVENANCE
                    && !stale_provenance_after_authoritative_closure_is_diagnostic(status))
            })
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
                command_kind: public_command_kind_token(PublicCommandKind::RepairReviewState),
                task: persisted_follow_up_record.and_then(|record| record.target_task),
                step: persisted_follow_up_record.and_then(|record| record.target_step),
                reason_code: persisted_review_state_repair_follow_up_reason(follow_up),
                source_record_id: persisted_follow_up_record
                    .and_then(|record| record.target_record_id.clone())
                    .or_else(|| Some(format!("review_state_repair_follow_up:{follow_up}"))),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    if persisted_follow_up
        == Some(crate::execution::review_route_tokens::FOLLOW_UP_CLOSE_CURRENT_TASK)
        && let Some(task) = persisted_follow_up_record.and_then(|record| record.target_task)
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: public_command_kind_token(PublicCommandKind::CloseCurrentTask),
                task: Some(task),
                step: None,
                reason_code: PublicRepairTargetReason::PersistedTaskClosureFollowUp.reason_code(),
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
                command_kind: public_command_kind_token(PublicCommandKind::Reopen),
                task: Some(target.task),
                step: Some(target.step),
                reason_code: PublicRepairTargetReason::ExplicitReopenRepairTarget.reason_code(),
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
    let worktree_lease_cleanup_tasks =
        worktree_lease_cleanup_tasks_from_authority(context, authoritative_state)
            .unwrap_or_default();
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
                    command_kind: public_command_kind_token(PublicCommandKind::CloseCurrentTask),
                    task: Some(record.task),
                    step: None,
                    reason_code:
                        PublicRepairTargetReason::AuthoritativeTaskClosurePostconditionCleanup
                            .reason_code(),
                    source_record_id: Some(record.closure_record_id.clone()),
                    expires_when_fingerprint_changes: true,
                },
            );
        }
        if worktree_lease_cleanup_tasks.contains(&record.task) {
            push_public_repair_target_once(
                targets,
                PublicRepairTarget {
                    command_kind: public_command_kind_token(PublicCommandKind::CloseCurrentTask),
                    task: Some(record.task),
                    step: None,
                    reason_code: PublicRepairTargetReason::CurrentTaskClosureWorktreeLeaseCleanup
                        .reason_code(),
                    source_record_id: Some(record.closure_record_id.clone()),
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
    if status.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && let Some(task) = status
            .recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        push_public_repair_target_once(
            targets,
            PublicRepairTarget {
                command_kind: public_command_kind_token(PublicCommandKind::CloseCurrentTask),
                task: Some(task),
                step: None,
                reason_code: PublicRepairTargetReason::StatusTaskClosureRecordingReady
                    .reason_code(),
                source_record_id: Some(format!(
                    "{}:{task}",
                    PublicRepairTargetReason::StatusTaskClosureRecordingReady.as_str()
                )),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn worktree_lease_cleanup_tasks_from_authority(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<BTreeSet<u32>, crate::diagnostics::JsonFailure> {
    let release_preview = active_worktree_lease_release_preview_from_authority(
        authoritative_state,
        |run_identity, active_fingerprints, active_bindings| {
            worktree_lease_release_decision_for_current_task_closures_from_authority(
                context,
                authoritative_state,
                run_identity.execution_run_id.as_str(),
                active_fingerprints,
                active_bindings,
                crate::execution::review_route_tokens::FOLLOW_UP_CLOSE_CURRENT_TASK,
                None,
            )
        },
    )?;
    Ok(release_preview
        .map(|decision| {
            decision
                .released_by
                .into_iter()
                .map(|(task, _closure_id)| task)
                .collect()
        })
        .unwrap_or_default())
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
                command_kind: public_command_kind_token(PublicCommandKind::CloseCurrentTask),
                task: Some(task),
                step: None,
                reason_code: PublicRepairTargetReason::TaskReviewDispatchClosureReady.reason_code(),
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
                command_kind: public_command_kind_token(PublicCommandKind::CloseCurrentTask),
                task: entry.task,
                step: None,
                reason_code: PublicRepairTargetReason::AuthoritativePreflightRecoveryTaskClosure
                    .reason_code(),
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
            command_kind: public_command_kind_token(PublicCommandKind::Reopen),
            task: Some(task),
            step: Some(step),
            reason_code: PublicRepairTargetReason::PersistedExecutionReentryFollowUp.reason_code(),
            source_record_id: record
                .target_record_id
                .clone()
                .or_else(|| Some(format!("review_state_repair_follow_up_task:{task}"))),
            expires_when_fingerprint_changes: true,
        },
    );
}

fn public_command_kind_token(kind: PublicCommandKind) -> String {
    kind.public_mutation_token().to_owned()
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
