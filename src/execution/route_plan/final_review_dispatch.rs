use super::decision::{Blocker, PublicRouteDecision, RouteDecision};
use super::planning_facts::RoutePlanningFacts;
use super::public_action::synthesize_next_public_action;
use super::state_kind::{STATE_KIND_ACTIONABLE_PUBLIC_COMMAND, STATE_KIND_WAITING_EXTERNAL_INPUT};
use crate::execution::command_eligibility::public_advance_late_stage_command_for_phase_detail;
use crate::execution::next_action::{
    NEXT_ACTION_ADVANCE_LATE_STAGE, NEXT_ACTION_REQUEST_FINAL_REVIEW,
    NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
};
use crate::execution::phase;
use crate::execution::query::{ExecutionRoutingRecordingContext, compact_operator_reason_codes};
use crate::execution::reducer::RuntimeState;
use crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE;

pub(super) fn final_review_dispatch_route_for_repaired_late_stage_drift(
    runtime_state: &RuntimeState,
    route_facts: &RoutePlanningFacts,
    phase_detail: &str,
    external_review_result_ready: bool,
) -> Option<RouteDecision> {
    let status = &runtime_state.status;
    if phase_detail != phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        || !route_facts.persisted_repair_follow_up_is(FOLLOW_UP_ADVANCE_LATE_STAGE)
        || runtime_state
            .release_readiness_result_for_current_branch
            .as_deref()
            != Some("ready")
        || !status.current_branch_meaningful_drift
        || runtime_state.final_review_outcome_recorded_for_current_dispatch
    {
        return None;
    }
    let branch_closure_id = status.current_branch_closure_id.as_ref()?;
    let route_kind = repaired_drift_final_review_route_kind(
        runtime_state
            .final_review_dispatch_authority
            .lineage_present,
        external_review_result_ready,
    );
    let phase_detail = route_kind.phase_detail().to_owned();
    let recommended_public_command = route_kind
        .exposes_public_command()
        .then(|| {
            public_advance_late_stage_command_for_phase_detail(
                &runtime_state.context.plan_rel,
                &phase_detail,
            )
        })
        .flatten();
    let (recommended_command, invocation, template, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_action = route_kind.next_action().to_owned();
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
    let blockers = vec![Blocker {
        category: String::from("late_stage"),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_id: None,
        next_public_action: next_public_action.clone(),
        details: route_kind.blocker_details().to_owned(),
    }];
    let blocking_reason_codes = compact_operator_reason_codes(Some(status), &phase_detail, "clean");
    let recording_context =
        route_kind
            .records_final_review_outcome()
            .then(|| ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: runtime_state
                    .final_review_dispatch_authority
                    .dispatch_id
                    .clone(),
                branch_closure_id: Some(branch_closure_id.clone()),
            });
    let mut route_decision = RouteDecision {
        state_kind: route_kind.state_kind().to_owned(),
        phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
        phase_detail,
        review_state_status: String::from("clean"),
        next_action,
        blocking_reason_codes,
        recommended_command,
        recommended_public_command,
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up: route_kind.required_follow_up().map(str::to_owned),
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_reentry_target_source: None,
        execution_command_context: None,
        recording_context,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
    };
    route_decision.apply_public_route_projection(Some(status), external_review_result_ready);
    Some(route_decision)
}

#[derive(Clone, Copy)]
enum RepairedDriftFinalReviewRouteKind {
    DispatchReview,
    WaitForReviewResult,
    RecordReviewResult,
}

impl RepairedDriftFinalReviewRouteKind {
    fn phase_detail(self) -> &'static str {
        match self {
            Self::DispatchReview => phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
            Self::WaitForReviewResult => phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING,
            Self::RecordReviewResult => phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
        }
    }

    fn state_kind(self) -> &'static str {
        match self {
            Self::DispatchReview | Self::RecordReviewResult => STATE_KIND_ACTIONABLE_PUBLIC_COMMAND,
            Self::WaitForReviewResult => STATE_KIND_WAITING_EXTERNAL_INPUT,
        }
    }

    fn next_action(self) -> &'static str {
        match self {
            Self::DispatchReview => NEXT_ACTION_REQUEST_FINAL_REVIEW,
            Self::WaitForReviewResult => NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
            Self::RecordReviewResult => NEXT_ACTION_ADVANCE_LATE_STAGE,
        }
    }

    fn blocker_details(self) -> &'static str {
        match self {
            Self::DispatchReview => {
                "A fresh external final review is required before late-stage progression can continue."
            }
            Self::WaitForReviewResult => {
                "The external final review has been dispatched; wait for the review result before continuing."
            }
            Self::RecordReviewResult => {
                "The external final-review result is ready; record it through public advance-late-stage."
            }
        }
    }

    fn required_follow_up(self) -> Option<&'static str> {
        match self {
            Self::DispatchReview | Self::RecordReviewResult => Some(FOLLOW_UP_ADVANCE_LATE_STAGE),
            Self::WaitForReviewResult => None,
        }
    }

    fn exposes_public_command(self) -> bool {
        !matches!(self, Self::WaitForReviewResult)
    }

    fn records_final_review_outcome(self) -> bool {
        matches!(self, Self::RecordReviewResult)
    }
}

fn repaired_drift_final_review_route_kind(
    dispatch_lineage_present: bool,
    external_review_result_ready: bool,
) -> RepairedDriftFinalReviewRouteKind {
    match (dispatch_lineage_present, external_review_result_ready) {
        (false, _) => RepairedDriftFinalReviewRouteKind::DispatchReview,
        (true, false) => RepairedDriftFinalReviewRouteKind::WaitForReviewResult,
        (true, true) => RepairedDriftFinalReviewRouteKind::RecordReviewResult,
    }
}
