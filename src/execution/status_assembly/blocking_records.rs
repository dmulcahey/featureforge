use crate::diagnostics::JsonFailure;
use crate::execution::closure_diagnostics::{
    BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED,
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED,
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
    current_branch_closure_reviewed_state_malformed_reason_code,
};
use crate::execution::context::ExecutionContext;
use crate::execution::current_closure_projection::structural_current_task_closure_failures_from_authoritative_state;
use crate::execution::current_task_closure_selection::{
    CurrentTaskClosureRouteTarget, preferred_current_task_closure_route_target,
};
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason, REASON_LATE_STAGE_SURFACE_NOT_DECLARED,
    branch_closure_rerecording_assessment_with_authority,
    branch_drift_escapes_late_stage_surface_reason_code as shared_branch_drift_escapes_late_stage_surface_reason_code,
    late_stage_surface_not_declared_reason_code as shared_late_stage_surface_not_declared_reason_code,
    release_readiness_result_for_branch_closure as shared_release_readiness_result_for_branch_closure,
    resolve_actionable_repair_follow_up_for_status,
};
use crate::execution::follow_up::FollowUpKind;
use crate::execution::harness::HarnessPhase;
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::phase;
use crate::execution::reentry_reconcile::{
    TargetlessStaleReconcile, task_closure_baseline_repair_candidate_reason_present,
};
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, AuthoritativeStaleTargetScope,
};
use crate::execution::state::worktree_lease_public_gate_reason_code;
use crate::execution::status::{GateResult, PlanExecutionStatus, StatusBlockingRecord};
use crate::execution::status_support::{
    stale_unreviewed_allows_task_closure_baseline_bridge,
    stale_unreviewed_allows_task_closure_baseline_bridge_with_authority,
    task_scope_structural_review_state_reason,
};
use crate::execution::task_scope_key::{task_scope_key_for_task, task_scope_key_task_number};
use crate::execution::transitions::AuthoritativeTransitionState;

use super::push_status_reason_code_once;

pub(super) fn project_worktree_lease_gate_blockers(
    status: &mut PlanExecutionStatus,
    gate_review: &GateResult,
) {
    if gate_review.allowed {
        return;
    }
    if status.active_task.is_some()
        || status.active_step.is_some()
        || status.resume_task.is_some()
        || status.resume_step.is_some()
    {
        return;
    }
    if status.current_task_closures.is_empty() && status.current_branch_closure_id.is_none() {
        return;
    }
    for reason_code in gate_review
        .reason_codes
        .iter()
        .map(String::as_str)
        .filter(|reason_code| worktree_lease_public_gate_reason_code(reason_code))
    {
        push_status_reason_code_once(status, reason_code);
    }
}

fn worktree_lease_review_state_repair_reason(status: &PlanExecutionStatus) -> Option<&str> {
    status
        .reason_codes
        .iter()
        .map(String::as_str)
        .find(|reason_code| worktree_lease_public_gate_reason_code(reason_code))
}

pub(crate) fn current_branch_closure_structural_review_state_reason(
    status: &PlanExecutionStatus,
) -> Option<&str> {
    status
        .reason_codes
        .iter()
        .map(String::as_str)
        .find(|code| current_branch_closure_reviewed_state_malformed_reason_code(code))
}

pub(crate) fn execution_reentry_requires_review_state_repair(
    context: Option<&ExecutionContext>,
    status: &PlanExecutionStatus,
) -> bool {
    let bridge_allowed = context
        .and_then(|context| status.blocking_task.map(|task| (context, task)))
        .is_some_and(|(context, task)| {
            stale_unreviewed_allows_task_closure_baseline_bridge(context, status, task)
                .unwrap_or(false)
        });
    execution_reentry_requires_review_state_repair_with_bridge_allowance(status, bridge_allowed)
}

pub(crate) fn execution_reentry_requires_review_state_repair_with_authority(
    context: Option<&ExecutionContext>,
    status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    let bridge_allowed = context
        .and_then(|context| status.blocking_task.map(|task| (context, task)))
        .is_some_and(|(context, task)| {
            stale_unreviewed_allows_task_closure_baseline_bridge_with_authority(
                context,
                status,
                task,
                overlay,
                authoritative_state,
            )
            .unwrap_or(false)
        });
    execution_reentry_requires_review_state_repair_with_bridge_allowance(status, bridge_allowed)
}

fn execution_reentry_requires_review_state_repair_with_bridge_allowance(
    status: &PlanExecutionStatus,
    baseline_bridge_allowed: bool,
) -> bool {
    let task_scope_repair_required = task_scope_overlay_repair_required(status)
        || task_scope_structural_review_state_reason(status).is_some()
        || (matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        ) && status.reason_codes.iter().any(|reason_code| {
            reason_code == TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
        }));
    if task_closure_baseline_repair_candidate_reason_present(status) && !task_scope_repair_required
    {
        if status.review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        {
            if baseline_bridge_allowed {
                return false;
            }
        } else {
            return false;
        }
    }
    execution_reentry_repair_projection_active(status)
        || worktree_lease_review_state_repair_reason(status).is_some()
        || (status.current_branch_closure_id.is_some()
            && current_branch_closure_structural_review_state_reason(status).is_some())
        || task_scope_repair_required
}

fn execution_reentry_repair_projection_active(status: &PlanExecutionStatus) -> bool {
    status.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && (status.review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            || status.reason_codes.iter().any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
                        | TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
                        | BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED
                )
            }))
}

pub(in crate::execution::status_assembly) fn task_scope_overlay_repair_required(
    status: &PlanExecutionStatus,
) -> bool {
    status.harness_phase == HarnessPhase::Executing
        && status.reason_codes.iter().any(|reason_code| {
            reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED
                || reason_code == "task_closure_negative_result_overlay_restore_required"
        })
        && status.current_branch_closure_id.is_none()
}

pub(crate) fn compute_status_blocking_records(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
    stale_targets: Option<&[AuthoritativeStaleTarget]>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Vec<StatusBlockingRecord>, JsonFailure> {
    let task_structural_records = derive_structural_current_task_closure_blocking_records(
        context,
        status,
        authoritative_state,
    );
    let base_blocking_records =
        derive_public_blocking_records_with_stale_targets(status, gate_finish, stale_targets);
    if let Some(structural_records) = task_structural_records
        .as_ref()
        .filter(|records| !records.is_empty())
    {
        if status.review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        {
            return Ok(merge_status_blocking_records(
                base_blocking_records,
                structural_records.clone(),
            ));
        }
        return Ok(structural_records.clone());
    }
    if let Some(record) =
        derive_branch_rerecording_blocking_record(context, status, authoritative_state)?
    {
        return Ok(vec![record]);
    }
    let branch_structural_records =
        derive_structural_current_branch_closure_blocking_records(status);
    let blocking_records = if status.review_state_status
        == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
    {
        task_structural_records
            .into_iter()
            .chain(branch_structural_records)
            .fold(base_blocking_records, merge_status_blocking_records)
    } else if let Some(structural_records) =
        task_structural_records.filter(|records| !records.is_empty())
    {
        structural_records
    } else if let Some(structural_records) =
        branch_structural_records.filter(|records| !records.is_empty())
    {
        structural_records
    } else {
        base_blocking_records
    };
    Ok(blocking_records)
}

fn authoritative_release_readiness_result_for_current_branch(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    current_branch_closure_id: Option<&str>,
) -> Option<String> {
    shared_release_readiness_result_for_branch_closure(
        authoritative_state,
        current_branch_closure_id,
    )
}

fn derive_branch_rerecording_blocking_record(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Option<StatusBlockingRecord>, JsonFailure> {
    if !semantic_branch_rerecording_required(context, status, authoritative_state) {
        return Ok(None);
    }
    let assessment =
        branch_closure_rerecording_assessment_with_authority(context, authoritative_state)?;
    let branch_closure_id = status
        .current_branch_closure_id
        .clone()
        .unwrap_or_else(|| String::from("current"));
    let review_state_status = if status.review_state_status == "clean" {
        String::from(crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE)
    } else {
        status.review_state_status.clone()
    };
    if assessment.supported {
        return Ok(Some(StatusBlockingRecord {
            code: String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
            ),
            scope_type: String::from("branch"),
            scope_key: branch_closure_id,
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status,
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            )),
            message: String::from(
                "The current branch closure must be re-recorded before late-stage progression can continue.",
            ),
        }));
    }
    let message = match assessment.unsupported_reason {
        Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => String::from(
            "The current branch closure can no longer be safely re-recorded from authoritative current task-closure truth, so review-state repair must reroute execution before late-stage progression can continue.",
        ),
        Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => String::from(
            "The current branch closure cannot be safely re-recorded because the approved plan does not declare Late-Stage Surface metadata for classifying post-closure drift. Repair review state must reroute through execution reentry before late-stage progression can continue.",
        ),
        Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
            String::from(
                "The current branch closure cannot be safely re-recorded because branch drift escapes the approved Late-Stage Surface. Repair review state must reroute execution before late-stage progression can continue.",
            )
        }
    };
    Ok(Some(StatusBlockingRecord {
        code: String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
        ),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_type: String::from("review_state"),
        record_id: Some(branch_closure_id),
        review_state_status,
        required_follow_up: Some(String::from(
            crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
        )),
        message,
    }))
}

fn semantic_branch_rerecording_required(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    let persisted_branch_follow_up =
        resolve_actionable_repair_follow_up_for_status(context, status, authoritative_state)
            .is_some_and(|record| {
                record.kind.public_token()
                    == crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE
            });
    if status.current_branch_meaningful_drift {
        let release_readiness_already_recorded =
            authoritative_release_readiness_result_for_current_branch(
                authoritative_state,
                status.current_branch_closure_id.as_deref(),
            )
            .is_some();
        return !(persisted_branch_follow_up && release_readiness_already_recorded);
    }
    if status.current_branch_closure_id.is_none() {
        return false;
    }
    persisted_branch_follow_up
}

fn merge_status_blocking_records(
    mut base_records: Vec<StatusBlockingRecord>,
    extra_records: Vec<StatusBlockingRecord>,
) -> Vec<StatusBlockingRecord> {
    for record in extra_records {
        if !base_records.contains(&record) {
            base_records.push(record);
        }
    }
    base_records
}

#[cfg(test)]
pub(crate) fn derive_public_blocking_records(
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
) -> Vec<StatusBlockingRecord> {
    derive_public_blocking_records_with_stale_targets(status, gate_finish, None)
}

fn derive_public_blocking_records_with_stale_targets(
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
    stale_targets: Option<&[AuthoritativeStaleTarget]>,
) -> Vec<StatusBlockingRecord> {
    if let Some(blocking_record) = TargetlessStaleReconcile::status_blocking_record(status) {
        return vec![blocking_record];
    }

    if status.review_state_status
        == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
    {
        if status.stale_unreviewed_closures.is_empty() {
            return TargetlessStaleReconcile::status_blocking_record(status)
                .into_iter()
                .collect();
        }
        let late_stage_surface_not_declared = status
            .reason_codes
            .iter()
            .any(|reason| shared_late_stage_surface_not_declared_reason_code(reason));
        let code = if late_stage_surface_not_declared {
            String::from(REASON_LATE_STAGE_SURFACE_NOT_DECLARED)
        } else {
            String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED)
        };
        let message = if late_stage_surface_not_declared {
            String::from(
                "The current reviewed state is stale, and the approved plan does not declare Late-Stage Surface metadata to classify post-closure drift as trusted late-stage-only. Repair review state must reroute through execution reentry.",
            )
        } else {
            String::from(
                "The current reviewed state is stale because later workspace changes landed after the latest reviewed closure.",
            )
        };
        return status
            .stale_unreviewed_closures
            .iter()
            .cloned()
            .map(|record_id| {
                let scope_key = stale_blocking_record_scope_key(&record_id, stale_targets);
                StatusBlockingRecord {
                    code: code.clone(),
                    scope_type: String::from(if task_scope_key_task_number(&scope_key).is_some() {
                        "task"
                    } else {
                        "branch"
                    }),
                    scope_key,
                    record_type: String::from("review_state"),
                    record_id: Some(record_id),
                    review_state_status: status.review_state_status.clone(),
                    required_follow_up: Some(String::from(
                        crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
                    )),
                    message: message.clone(),
                }
            })
            .collect();
    }

    if let Some(reason_code) = task_scope_structural_review_state_reason(status) {
        let route_target =
            preferred_current_task_closure_route_target(status, status.blocking_task)
                .unwrap_or_else(|| CurrentTaskClosureRouteTarget::from_task(0));
        let task_number = route_target.task;
        let message = match reason_code {
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID => format!(
                "Task {task_number} is blocked because the current task-closure provenance is not valid for the active approved plan."
            ),
            TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED => format!(
                "Task {task_number} is blocked because the current task-closure reviewed-state identity is malformed."
            ),
            _ => format!(
                "Task {task_number} is blocked because the current task-closure review state requires repair before execution can continue."
            ),
        };
        return vec![StatusBlockingRecord {
            code: reason_code.to_owned(),
            scope_type: String::from("task"),
            scope_key: route_target.scope_key,
            record_type: String::from("review_state"),
            record_id: route_target.closure_record_id,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
            )),
            message,
        }];
    }

    if status.current_branch_closure_id.is_some()
        && let Some(reason_code) = current_branch_closure_structural_review_state_reason(status)
    {
        let branch_closure_id = status
            .current_branch_closure_id
            .clone()
            .unwrap_or_else(|| String::from("current"));
        let message = match reason_code {
            BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED => format!(
                "Branch closure {branch_closure_id} is blocked because the current branch-closure reviewed-state identity is malformed."
            ),
            _ => format!(
                "Branch closure {branch_closure_id} requires review-state repair before late-stage progression can continue."
            ),
        };
        return vec![StatusBlockingRecord {
            code: reason_code.to_owned(),
            scope_type: String::from("branch"),
            scope_key: branch_closure_id.clone(),
            record_type: String::from("review_state"),
            record_id: Some(branch_closure_id),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
            )),
            message,
        }];
    }

    if status.review_state_status
        == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
    {
        if execution_reentry_requires_review_state_repair(None, status)
            || status.reason_codes.iter().any(|reason| {
                shared_late_stage_surface_not_declared_reason_code(reason)
                    || shared_branch_drift_escapes_late_stage_surface_reason_code(reason)
            })
        {
            let scope_key = status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current"));
            let late_stage_surface_not_declared = status
                .reason_codes
                .iter()
                .any(|reason| shared_late_stage_surface_not_declared_reason_code(reason));
            return vec![StatusBlockingRecord {
                code: String::from(
                    crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
                ),
                scope_type: String::from("branch"),
                scope_key: scope_key.clone(),
                record_type: String::from("review_state"),
                record_id: Some(scope_key),
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
                )),
                message: if late_stage_surface_not_declared {
                    String::from(
                        "The current branch closure cannot be safely re-recorded because the approved plan does not declare Late-Stage Surface metadata for classifying post-closure drift. Repair review state must reroute through execution reentry before late-stage progression can continue.",
                    )
                } else {
                    String::from(
                        "The current branch closure can no longer be safely re-recorded from authoritative current task-closure truth, so review-state repair must reroute execution before late-stage progression can continue.",
                    )
                },
            }];
        }
        return vec![StatusBlockingRecord {
            code: String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
            ),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            )),
            message: String::from(
                "The current branch closure must be recorded before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("release_readiness"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(
                FollowUpKind::ResolveReleaseBlocker
                    .public_token()
                    .to_owned(),
            ),
            message: String::from(
                "The latest release-readiness result for the current branch closure is blocked and must be resolved before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_RELEASE_READINESS_RECORDING_READY {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_RELEASE_READINESS_RECORDING_READY),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("release_readiness"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            )),
            message: String::from(
                "A current release-readiness result for the active branch closure is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("final_review_dispatch"),
            record_id: None,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(
                FollowUpKind::RequestExternalReview
                    .public_token()
                    .to_owned(),
            ),
            message: String::from(
                "A fresh external final review is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_QA_RECORDING_REQUIRED {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_QA_RECORDING_REQUIRED),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("qa_result"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            )),
            message: String::from(
                "A current QA result for the active branch closure is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_FINISH_COMPLETION_GATE_READY && !gate_finish.allowed {
        return vec![StatusBlockingRecord {
            code: String::from("finish_review_gate_checkpoint_missing"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("finish_review_gate_pass_checkpoint"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            )),
            message: String::from(
                "The current branch closure still needs a fresh finish-review checkpoint before branch completion can proceed.",
            ),
        }];
    }

    Vec::new()
}

fn stale_blocking_record_scope_key(
    record_id: &str,
    stale_targets: Option<&[AuthoritativeStaleTarget]>,
) -> String {
    stale_targets
        .and_then(|targets| {
            targets
                .iter()
                .find(|target| {
                    target.scope == AuthoritativeStaleTargetScope::Task
                        && target.record_id.as_deref() == Some(record_id)
                })
                .and_then(|target| target.task)
                .map(task_scope_key_for_task)
        })
        .unwrap_or_else(|| record_id.to_owned())
}

fn derive_structural_current_task_closure_blocking_records(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<Vec<StatusBlockingRecord>> {
    task_scope_structural_review_state_reason(status)?;
    let structural_records = authoritative_state
        .map(|state| structural_current_task_closure_failures_from_authoritative_state(context, state))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|failure| {
            let message = match failure.reason_code.as_str() {
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID => match failure.task {
                    Some(task_number) => format!(
                        "Task {task_number} is blocked because the current task-closure provenance is not valid for the active approved plan."
                    ),
                    None => format!(
                        "Current task-closure entry `{}` is blocked because its authoritative provenance is not valid for the active approved plan.",
                        failure.scope_key
                    ),
                },
                TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED => {
                    let task_number = failure.task?;
                    format!(
                        "Task {task_number} is blocked because the current task-closure reviewed-state identity is malformed."
                    )
                }
                _ => return None,
            };
            Some(StatusBlockingRecord {
                code: failure.reason_code,
                scope_type: String::from("task"),
                scope_key: failure.scope_key,
                record_type: String::from("review_state"),
                record_id: failure.closure_record_id,
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from(crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE)),
                message,
            })
        })
        .collect::<Vec<_>>();
    if !structural_records.is_empty() {
        return Some(structural_records);
    }
    None
}

fn derive_structural_current_branch_closure_blocking_records(
    status: &PlanExecutionStatus,
) -> Option<Vec<StatusBlockingRecord>> {
    let reason_code = current_branch_closure_structural_review_state_reason(status)?;
    let branch_closure_id = status.current_branch_closure_id.clone()?;
    let message = match reason_code {
        BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED => format!(
            "Branch closure {branch_closure_id} is blocked because the current branch-closure reviewed-state identity is malformed."
        ),
        _ => format!(
            "Branch closure {branch_closure_id} requires review-state repair before late-stage progression can continue."
        ),
    };
    Some(vec![StatusBlockingRecord {
        code: reason_code.to_owned(),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_type: String::from("review_state"),
        record_id: Some(branch_closure_id),
        review_state_status: status.review_state_status.clone(),
        required_follow_up: Some(String::from(
            crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
        )),
        message,
    }])
}
