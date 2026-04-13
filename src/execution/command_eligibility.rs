use crate::execution::query::{
    ExecutionRoutingState,
    required_follow_up_from_routing as shared_required_follow_up_from_routing,
};

fn normalize_public_follow_up(follow_up: &str) -> String {
    match follow_up {
        "record_branch_closure" => String::from("advance_late_stage"),
        _ => follow_up.to_owned(),
    }
}

pub(crate) fn operator_requires_review_state_repair(operator: &ExecutionRoutingState) -> bool {
    shared_required_follow_up_from_routing(operator).as_deref() == Some("repair_review_state")
}

pub(crate) fn blocked_follow_up_for_operator(operator: &ExecutionRoutingState) -> Option<String> {
    shared_required_follow_up_from_routing(operator)
        .as_deref()
        .map(normalize_public_follow_up)
}

pub(crate) fn close_current_task_required_follow_up(
    operator: &ExecutionRoutingState,
) -> Option<String> {
    match shared_required_follow_up_from_routing(operator).as_deref() {
        Some("request_external_review")
            if operator.phase_detail == "task_review_dispatch_required" =>
        {
            Some(String::from("request_external_review"))
        }
        Some("repair_review_state") => Some(String::from("repair_review_state")),
        Some("execution_reentry") => Some(String::from("execution_reentry")),
        Some("record_handoff") => Some(String::from("record_handoff")),
        Some("record_pivot") => Some(String::from("record_pivot")),
        _ => None,
    }
}

pub(crate) fn late_stage_required_follow_up(
    stage_path: &str,
    operator: &ExecutionRoutingState,
) -> Option<String> {
    let required_follow_up = blocked_follow_up_for_operator(operator)?;
    if stage_path == "release_readiness"
        && !matches!(
            required_follow_up.as_str(),
            "advance_late_stage" | "repair_review_state"
        )
    {
        return None;
    }
    if stage_path == "final_review"
        && !matches!(
            required_follow_up.as_str(),
            "request_external_review" | "repair_review_state"
        )
    {
        return None;
    }
    Some(required_follow_up)
}

pub(crate) fn release_readiness_required_follow_up(
    operator: &ExecutionRoutingState,
) -> Option<String> {
    blocked_follow_up_for_operator(operator).and_then(|required_follow_up| {
        matches!(required_follow_up.as_str(), "advance_late_stage" | "repair_review_state")
            .then_some(required_follow_up)
    })
}

pub(crate) fn negative_result_follow_up(operator: &ExecutionRoutingState) -> Option<String> {
    match operator.follow_up_override.as_str() {
        "record_handoff" => Some(String::from("record_handoff")),
        "record_pivot" => Some(String::from("record_pivot")),
        _ => Some(String::from("execution_reentry")),
    }
}
