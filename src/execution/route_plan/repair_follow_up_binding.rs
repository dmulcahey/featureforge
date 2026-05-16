use crate::diagnostics::JsonFailure;
use crate::execution::current_truth::{
    CurrentTruthFollowUpInputs, resolve_actionable_repair_follow_up,
};
use crate::execution::follow_up::repair_follow_up_source_decision_hash;
use crate::execution::public_repair_targets::public_repair_target_candidates_from_authority;
use crate::execution::reducer::RuntimeState;
use crate::execution::transitions::AuthoritativeTransitionState;

use super::{RouteDecision, route_decision_from_runtime_state_with_authority};

pub(super) fn bind_repair_follow_up_to_source_route(
    runtime_state: &mut RuntimeState,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(), JsonFailure> {
    let source_route_decision = source_route_decision_for_repair_follow_up_binding(
        runtime_state,
        authoritative_state,
        external_review_result_ready,
        require_exact_execution_command,
    )?;
    let source_route_decision_hash = repair_follow_up_source_decision_hash(&source_route_decision);
    refresh_route_bound_repair_state(
        runtime_state,
        authoritative_state,
        source_route_decision_hash.as_deref(),
    );
    Ok(())
}

fn source_route_decision_for_repair_follow_up_binding(
    runtime_state: &mut RuntimeState,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<RouteDecision, JsonFailure> {
    let route_repair_target_candidates =
        std::mem::take(&mut runtime_state.route_repair_target_candidates);
    let persisted_repair_follow_up = runtime_state.persisted_repair_follow_up.take();
    let route_decision = route_decision_from_runtime_state_with_authority(
        runtime_state,
        authoritative_state,
        external_review_result_ready,
        require_exact_execution_command,
    );
    runtime_state.route_repair_target_candidates = route_repair_target_candidates;
    runtime_state.persisted_repair_follow_up = persisted_repair_follow_up;
    route_decision
}

fn refresh_route_bound_repair_state(
    runtime_state: &mut RuntimeState,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    source_route_decision_hash: Option<&str>,
) {
    runtime_state.persisted_repair_follow_up = resolve_actionable_repair_follow_up(
        CurrentTruthFollowUpInputs::new(authoritative_state, &runtime_state.status)
            .with_gate_snapshot(Some(&runtime_state.gate_snapshot))
            .with_semantic_workspace_state_id(Some(
                runtime_state
                    .semantic_workspace
                    .semantic_workspace_tree_id
                    .as_str(),
            ))
            .with_source_route_decision_hash(source_route_decision_hash),
    )
    .map(|record| record.kind.public_token().to_owned());
    runtime_state.route_repair_target_candidates = public_repair_target_candidates_from_authority(
        &runtime_state.context,
        &runtime_state.status,
        authoritative_state,
        source_route_decision_hash,
    );
}
