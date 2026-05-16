use crate::execution::repair_route_decision::RepairFollowUpDecision;
use crate::execution::review_route_tokens::{
    REVIEW_STATE_MISSING_CURRENT_CLOSURE, REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::stale_target_projection::StaleTargetProjection;
use crate::execution::status::PlanExecutionStatus;

use super::SharedRepairReviewStateRerouteDecision;

#[derive(Debug, Clone)]
pub(crate) struct StatusAssemblyFacts {
    pub(crate) stale_projection: StaleTargetProjection,
    pub(crate) repair_follow_up: StatusRepairFollowUpFacts,
    pub(crate) review_state: StatusReviewStateFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusRepairFollowUpFacts {
    pub(crate) branch_reroute_still_valid: bool,
    pub(crate) persisted_repair_follow_up: Option<String>,
    pub(crate) requires_execution_reentry: bool,
    pub(crate) requires_planning_reentry: bool,
    pub(crate) records_branch_closure: bool,
}

impl StatusRepairFollowUpFacts {
    pub(crate) fn from_decisions(
        repair_review_state: &SharedRepairReviewStateRerouteDecision,
        repair_follow_up: &RepairFollowUpDecision,
        records_branch_closure: bool,
    ) -> Self {
        Self {
            branch_reroute_still_valid: repair_review_state.branch_reroute_still_valid,
            persisted_repair_follow_up: repair_review_state.persisted_repair_follow_up.clone(),
            requires_execution_reentry: repair_follow_up.requires_execution_reentry(),
            requires_planning_reentry: repair_follow_up.requires_planning_reentry(),
            records_branch_closure,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusReviewStateInputs {
    pub(crate) repair_follow_up_requires_execution_reentry: bool,
    pub(crate) repair_follow_up_records_branch_closure: bool,
    pub(crate) branch_scope_stale_unreviewed: bool,
    pub(crate) task_boundary_unresolved_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusReviewStateFacts {
    pub(crate) inputs: StatusReviewStateInputs,
    pub(crate) status: String,
}

pub(crate) fn prerelease_branch_closure_refresh_required(status: &PlanExecutionStatus) -> bool {
    status.harness_phase == crate::execution::harness::HarnessPhase::DocumentReleasePending
        && status.current_release_readiness_state.is_none()
        && status.current_branch_closure_id.is_some()
        && status.current_branch_meaningful_drift
}

pub(crate) fn effective_review_state_status(
    status: &PlanExecutionStatus,
    candidate_review_state_status: &str,
) -> String {
    if candidate_review_state_status != "clean" {
        return candidate_review_state_status.to_owned();
    }
    if prerelease_branch_closure_refresh_required(status)
        || status.phase_detail
            == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
    {
        return String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE);
    }
    if status.current_branch_closure_id.is_none()
        && status
            .reason_codes
            .iter()
            .any(|code| code == REVIEW_STATE_MISSING_CURRENT_CLOSURE)
    {
        return String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE);
    }
    if !status.stale_unreviewed_closures.is_empty() {
        return String::from(REVIEW_STATE_STALE_UNREVIEWED);
    }
    String::from("clean")
}

pub(crate) fn effective_route_review_state_status(
    status: &PlanExecutionStatus,
    phase_detail: &str,
    candidate_review_state_status: &str,
) -> String {
    if status.review_state_status == REVIEW_STATE_STALE_UNREVIEWED
        || (!status.stale_unreviewed_closures.is_empty()
            && phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY)
    {
        return String::from(REVIEW_STATE_STALE_UNREVIEWED);
    }
    if candidate_review_state_status == "clean"
        && phase_detail
            == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
    {
        return String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE);
    }
    effective_review_state_status(status, candidate_review_state_status)
}
