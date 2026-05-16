use crate::diagnostics::JsonFailure;
use crate::execution::public_repair_targets::public_repair_target_warning_codes;
use crate::execution::query::ExecutionRoutingState;
use crate::execution::route_plan::RouteDecision;
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::status::PlanExecutionStatus;
use crate::execution::status_assembly::require_public_execution_command_route_target;
use crate::execution::transitions::AuthoritativeTransitionState;

use super::ExecutionReadScope;

fn project_public_repair_target_warning_codes(
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) {
    for warning_code in public_repair_target_warning_codes(authoritative_state) {
        if !status
            .warning_codes
            .iter()
            .any(|existing| existing == warning_code)
        {
            status.warning_codes.push(warning_code.to_owned());
        }
    }
}

pub(crate) fn apply_shared_routing_projection_to_read_scope(
    _runtime: &ExecutionRuntime,
    read_scope: &mut ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(), JsonFailure> {
    apply_shared_routing_projection_to_read_scope_with_routing(
        read_scope,
        external_review_result_ready,
        require_exact_execution_command,
    )?;
    Ok(())
}

pub(crate) fn apply_shared_routing_projection_to_read_scope_with_routing(
    read_scope: &mut ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    let projection = crate::execution::router::project_final_runtime_routing_projection(
        read_scope,
        external_review_result_ready,
        require_exact_execution_command,
    )?;
    let routing = projection.routing;
    let route_decision = projection.route_decision;
    let runtime_state = projection.runtime_state;
    let mut status_projection = projection.status_projection;
    // The router owns route/status projection. The read model only layers
    // read-surface diagnostics that do not choose or revise the public route.
    project_public_repair_target_warning_codes(
        &mut status_projection,
        read_scope.authoritative_state.as_ref(),
    );
    status_projection.semantic_workspace_tree_id = runtime_state
        .semantic_workspace
        .semantic_workspace_tree_id
        .clone();
    status_projection.raw_workspace_tree_id = Some(
        runtime_state
            .semantic_workspace
            .raw_workspace_tree_id
            .clone(),
    );
    read_scope.status = status_projection;
    if require_exact_execution_command {
        require_public_execution_command_route_target(&read_scope.status)?;
    }
    read_scope.runtime_state = Some(runtime_state);
    Ok((routing, route_decision))
}
