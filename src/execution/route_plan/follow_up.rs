use crate::execution::command_eligibility::{
    PublicCommand, public_advance_late_stage_command_for_follow_up,
    public_advance_late_stage_command_for_phase_detail,
};
use crate::execution::follow_up::{
    FollowUpAliasContext, FollowUpKind, follow_up_from_phase_detail, normalize_follow_up_alias,
    normalize_public_routing_follow_up_token,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::phase;
use crate::execution::query::ExecutionRoutingExecutionCommandContext;
use crate::execution::reducer::RuntimeState;
use crate::execution::reentry_reconcile::TargetlessStaleReconcile;
use crate::execution::repair_target_selection::{
    NextActionAuthorityInputs, execution_reentry_target,
};
use crate::execution::review_route_tokens::{
    FOLLOW_UP_REPAIR_REVIEW_STATE, REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::state::{
    PlanExecutionStatus, current_branch_closure_structural_review_state_reason,
    task_scope_review_state_repair_reason, task_scope_structural_review_state_reason,
};

use super::decision::RouteDecision;
use super::public_commands::{repair_review_state_public_command, transfer_handoff_public_command};

pub(crate) fn execution_reentry_target_source_for_route(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    phase_detail: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<String> {
    if phase_detail != phase::DETAIL_EXECUTION_REENTRY_REQUIRED {
        return None;
    }
    let selected_target = execution_reentry_target(
        &runtime_state.context,
        status,
        &runtime_state.context.plan_rel,
        authority_inputs,
    );
    selected_target.map(|target| target.source.as_str().to_owned())
}

pub(crate) fn required_follow_up_from_route_decision(
    route_decision: &RouteDecision,
) -> Option<String> {
    route_decision.required_follow_up.clone()
}

pub(crate) fn derive_required_follow_up<'a>(
    status: &PlanExecutionStatus,
    phase_detail: &str,
    review_state_status: &str,
    blocking_reason_codes: impl IntoIterator<Item = &'a str>,
    execution_command_context: Option<&ExecutionRoutingExecutionCommandContext>,
) -> Option<String> {
    derive_required_follow_up_from_optional_status(
        Some(status),
        phase_detail,
        review_state_status,
        blocking_reason_codes,
        execution_command_context,
    )
}

pub(super) fn derive_required_follow_up_from_optional_status<'a>(
    status: Option<&PlanExecutionStatus>,
    phase_detail: &str,
    review_state_status: &str,
    blocking_reason_codes: impl IntoIterator<Item = &'a str>,
    execution_command_context: Option<&ExecutionRoutingExecutionCommandContext>,
) -> Option<String> {
    let blocking_reason_codes = blocking_reason_codes.into_iter().collect::<Vec<_>>();
    if TargetlessStaleReconcile::from_phase_and_reason_code_strs(
        phase_detail,
        blocking_reason_codes.iter().copied(),
    )
    .is_some()
    {
        return None;
    }
    if route_requires_review_state_repair(
        status,
        phase_detail,
        review_state_status,
        execution_command_context,
    ) {
        return Some(String::from(FOLLOW_UP_REPAIR_REVIEW_STATE));
    }
    if review_state_status != "clean"
        && let Some(required_follow_up) = status
            .and_then(|status| status.blocking_records.first())
            .and_then(|record| record.required_follow_up.as_deref())
            .and_then(|follow_up| normalize_public_routing_follow_up_token(Some(follow_up)))
    {
        return Some(required_follow_up.to_owned());
    }
    follow_up_from_phase_detail(phase_detail, blocking_reason_codes.iter().copied())
        .map(|follow_up| follow_up.public_token().to_owned())
}

fn route_requires_review_state_repair(
    status: Option<&PlanExecutionStatus>,
    phase_detail: &str,
    review_state_status: &str,
    execution_command_context: Option<&ExecutionRoutingExecutionCommandContext>,
) -> bool {
    if review_state_status == REVIEW_STATE_STALE_UNREVIEWED {
        return true;
    }
    if phase_detail != phase::DETAIL_EXECUTION_REENTRY_REQUIRED {
        return false;
    }
    if execution_command_context.is_none() {
        return true;
    }
    if review_state_status != "clean" {
        return true;
    }
    status.is_some_and(|status| {
        let late_stage_stale_provenance_without_branch_binding =
            matches!(
                status.harness_phase,
                HarnessPhase::DocumentReleasePending
                    | HarnessPhase::FinalReviewPending
                    | HarnessPhase::QaPending
                    | HarnessPhase::ReadyForBranchCompletion
            ) && status.current_branch_closure_id.is_none()
                && !status.stale_unreviewed_closures.is_empty()
                && status
                    .reason_codes
                    .iter()
                    .any(|code| code == REASON_CODE_STALE_PROVENANCE);
        task_scope_structural_review_state_reason(status).is_some()
            || task_scope_review_state_repair_reason(status).is_some()
            || current_branch_closure_structural_review_state_reason(status).is_some()
            || late_stage_stale_provenance_without_branch_binding
    })
}

pub(crate) fn public_command_for_required_follow_up(
    required_follow_up: Option<&str>,
    plan_path: &str,
    phase_detail: &str,
    record_type: Option<&str>,
) -> Option<PublicCommand> {
    match normalize_follow_up_alias(required_follow_up, FollowUpAliasContext::PublicRouting)? {
        FollowUpKind::RepairReviewState => Some(repair_review_state_public_command(plan_path)),
        FollowUpKind::ResolveReleaseBlocker => public_advance_late_stage_command_for_phase_detail(
            plan_path,
            phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
        ),
        FollowUpKind::AdvanceLateStage => {
            public_advance_late_stage_command_for_follow_up(plan_path, phase_detail, record_type)
        }
        FollowUpKind::RecordHandoff => {
            Some(transfer_handoff_public_command(plan_path, "task|branch"))
        }
        FollowUpKind::ExecutionReentry
        | FollowUpKind::RequestExternalReview
        | FollowUpKind::WaitForExternalReviewResult
        | FollowUpKind::RunVerification => Some(PublicCommand::WorkflowOperator {
            plan: plan_path.to_owned(),
            external_review_result_ready: false,
            json: true,
        }),
        FollowUpKind::CloseCurrentTask | FollowUpKind::GateReview | FollowUpKind::GateFinish => {
            None
        }
    }
}
