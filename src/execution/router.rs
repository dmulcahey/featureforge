use crate::contracts::workflow::WorkflowRoute;
use crate::diagnostics::JsonFailure;
use crate::execution::closure_diagnostics::merge_task_boundary_projection_diagnostics;
use crate::execution::current_truth::normalized_plan_qa_requirement;
use crate::execution::next_action::{NEXT_ACTION_HANDOFF, NEXT_ACTION_PLANNING_REENTRY};
use crate::execution::phase;
use crate::execution::query::{
    ExecutionRoutingState, compact_operator_reason_codes, late_stage_observability_for_phase,
};
use crate::execution::reducer::{RuntimeState, reduce_execution_read_scope};
use crate::execution::route_plan::{
    PublicRouteDecision, RouteDecision, RuntimeRoutePlanInputs, plan_runtime_route,
    route_decision_from_non_runtime_workflow_routing, transfer_handoff_public_command,
};
use crate::execution::state::{ExecutionReadScope, PlanExecutionStatus};

pub(crate) struct RuntimeRoutingProjection {
    pub(crate) routing: ExecutionRoutingState,
    pub(crate) route_decision: RouteDecision,
    pub(crate) runtime_state: RuntimeState,
    pub(crate) status_projection: PlanExecutionStatus,
}

pub(crate) fn project_runtime_routing_state_with_exact_command_requirement(
    read_scope: &ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    let (routing, route_decision, _, _) = project_runtime_routing_state_with_reduced_state(
        read_scope,
        external_review_result_ready,
        require_exact_execution_command,
    )?;
    Ok((routing, route_decision))
}

pub(crate) fn project_runtime_routing_state_with_reduced_state(
    read_scope: &ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<
    (
        ExecutionRoutingState,
        RouteDecision,
        RuntimeState,
        PlanExecutionStatus,
    ),
    JsonFailure,
> {
    let mut runtime_state = reduce_execution_read_scope(read_scope)?;
    let (route_decision, status_projection) = plan_runtime_route(
        &mut runtime_state,
        RuntimeRoutePlanInputs {
            authoritative_state: read_scope.authoritative_state.as_ref(),
            external_review_result_ready,
            require_exact_execution_command,
        },
    )?;
    let route = route_from_runtime_state(&runtime_state);
    let routing = project_routing_from_runtime_state(route, &runtime_state, &route_decision);
    Ok((routing, route_decision, runtime_state, status_projection))
}

pub(crate) fn project_final_runtime_routing_projection(
    read_scope: &ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<RuntimeRoutingProjection, JsonFailure> {
    let (routing, route_decision, runtime_state, status_projection) =
        project_runtime_routing_state_with_reduced_state(
            read_scope,
            external_review_result_ready,
            require_exact_execution_command,
        )?;
    let mut routing = routing;
    project_final_route_decision_onto_routing(&mut routing, &route_decision, &status_projection);
    Ok(RuntimeRoutingProjection {
        routing,
        route_decision,
        runtime_state,
        status_projection,
    })
}

fn project_final_route_decision_onto_routing(
    routing: &mut ExecutionRoutingState,
    route_decision: &RouteDecision,
    status_projection: &PlanExecutionStatus,
) {
    routing.route_decision = Some(route_decision.clone());
    routing.execution_status = Some(status_projection.clone());
    project_route_decision_onto_routing(routing, route_decision);
}

fn project_route_decision_onto_routing(
    routing: &mut ExecutionRoutingState,
    route_decision: &RouteDecision,
) {
    routing.route_decision = Some(route_decision.clone());
    routing.workflow_phase = route_decision.phase.clone();
    routing.phase = route_decision.phase.clone();
    routing.phase_detail = route_decision.phase_detail.clone();
    routing.review_state_status = route_decision.review_state_status.clone();
    routing.recording_context = route_decision.recording_context.clone();
    routing.execution_command_context = route_decision.execution_command_context.clone();
    routing.next_action = route_decision.next_action.clone();
    routing
        .recommended_public_command
        .clone_from(&route_decision.recommended_public_command);
    routing
        .recommended_command
        .clone_from(&route_decision.recommended_command);
    routing
        .blocking_scope
        .clone_from(&route_decision.blocking_scope);
    routing.blocking_task = route_decision.blocking_task;
    routing
        .external_wait_state
        .clone_from(&route_decision.external_wait_state);
    routing
        .blocking_reason_codes
        .clone_from(&route_decision.blocking_reason_codes);
}

pub(crate) fn project_non_runtime_workflow_routing_state(
    route: WorkflowRoute,
    external_review_result_ready: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    let workflow_phase = non_runtime_workflow_phase(&route.status);
    let (phase, phase_detail, next_action, recommended_public_command) =
        match workflow_phase.as_str() {
            phase::PHASE_HANDOFF_REQUIRED => (
                String::from(phase::PHASE_HANDOFF_REQUIRED),
                String::from(phase::DETAIL_HANDOFF_RECORDING_REQUIRED),
                String::from(NEXT_ACTION_HANDOFF),
                Some(transfer_handoff_public_command(&route.plan_path, "branch")),
            ),
            _ => (
                String::from(phase::PHASE_PIVOT_REQUIRED),
                String::from(phase::DETAIL_PLANNING_REENTRY_REQUIRED),
                String::from(NEXT_ACTION_PLANNING_REENTRY),
                None,
            ),
        };
    let blocking_reason_codes = non_runtime_blocking_reason_codes(&route, &phase_detail);
    let (reason_family, diagnostic_reason_codes) =
        late_stage_observability_for_phase(&workflow_phase, None, None);
    let (recommended_command, _, _, _) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let mut routing = ExecutionRoutingState {
        route,
        route_decision: None,
        runtime_provenance: None,
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase,
        phase,
        phase_detail,
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action,
        recommended_public_command,
        recommended_command,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };
    let route_decision = route_decision_from_non_runtime_workflow_routing(
        &routing,
        &[],
        external_review_result_ready,
    );
    project_route_decision_onto_routing(&mut routing, &route_decision);
    Ok((routing, route_decision))
}

fn non_runtime_blocking_reason_codes(route: &WorkflowRoute, phase_detail: &str) -> Vec<String> {
    let mut reason_codes = compact_operator_reason_codes(None, phase_detail, "clean");
    if route.is_engineering_approval_fidelity_blocked() {
        for code in &route.reason_codes {
            if !reason_codes.iter().any(|existing| existing == code) {
                reason_codes.push(code.clone());
            }
        }
    }
    reason_codes
}

fn non_runtime_workflow_phase(route_status: &str) -> String {
    match route_status {
        "spec_draft" => String::from("spec_review"),
        "plan_draft" => String::from("plan_review"),
        "spec_approved_needs_plan" | "stale_plan" => String::from("plan_writing"),
        phase::PHASE_HANDOFF_REQUIRED => String::from(phase::PHASE_HANDOFF_REQUIRED),
        phase::WORKFLOW_STATUS_IMPLEMENTATION_READY => {
            String::from(phase::PHASE_IMPLEMENTATION_HANDOFF)
        }
        other => other.to_owned(),
    }
}

fn route_from_runtime_state(runtime_state: &RuntimeState) -> WorkflowRoute {
    let spec_path = runtime_state
        .context
        .source_spec_path
        .strip_prefix(&runtime_state.context.runtime.repo_root)
        .ok()
        .and_then(|path| path.to_str())
        .unwrap_or_default()
        .to_owned();
    WorkflowRoute {
        schema_version: 3,
        status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
        next_skill: String::new(),
        spec_path,
        plan_path: runtime_state.context.plan_rel.clone(),
        contract_state: String::from("valid"),
        reason_codes: vec![String::from("runtime_state_reduced")],
        diagnostics: Vec::new(),
        plan_fidelity_review: None,
        scan_truncated: false,
        spec_candidate_count: 1,
        plan_candidate_count: 1,
        manifest_path: String::new(),
        root: runtime_state
            .context
            .runtime
            .repo_root
            .display()
            .to_string(),
        reason: String::from("runtime_state_reduced"),
        note: String::from("runtime_state_reduced"),
    }
}

fn project_routing_from_runtime_state(
    route: WorkflowRoute,
    runtime_state: &RuntimeState,
    route_decision: &RouteDecision,
) -> ExecutionRoutingState {
    let route_decision = route_decision.clone();
    let status = runtime_state.status.clone();
    let (reason_family, diagnostic_reason_codes) = late_stage_observability_for_phase(
        &route_decision.phase,
        runtime_state.gate_review.as_ref(),
        runtime_state.gate_finish.as_ref(),
    );
    let diagnostic_reason_codes =
        merge_task_boundary_projection_diagnostics(diagnostic_reason_codes, &status);
    let execution_command_context = route_decision.execution_command_context.clone();
    let blocking_reason_codes = route_decision.blocking_reason_codes.clone();
    ExecutionRoutingState {
        route,
        route_decision: Some(route_decision.clone()),
        runtime_provenance: None,
        execution_status: Some(status.clone()),
        preflight: runtime_state.preflight.clone(),
        gate_review: runtime_state.gate_review.clone(),
        gate_finish: runtime_state.gate_finish.clone(),
        workflow_phase: route_decision.phase.clone(),
        phase: route_decision.phase.clone(),
        phase_detail: route_decision.phase_detail.clone(),
        review_state_status: route_decision.review_state_status.clone(),
        qa_requirement: normalized_plan_qa_requirement(
            runtime_state
                .context
                .plan_document
                .qa_requirement
                .as_deref(),
        ),
        finish_review_gate_pass_branch_closure_id: runtime_state
            .late_stage_bindings
            .finish_review_gate_pass_branch_closure_id
            .clone()
            .or_else(|| status.finish_review_gate_pass_branch_closure_id.clone()),
        recording_context: route_decision.recording_context.clone(),
        execution_command_context,
        next_action: route_decision.next_action.clone(),
        recommended_public_command: route_decision.recommended_public_command.clone(),
        recommended_command: route_decision.recommended_command.clone(),
        blocking_scope: route_decision.blocking_scope.clone(),
        blocking_task: route_decision.blocking_task,
        external_wait_state: route_decision.external_wait_state.clone(),
        blocking_reason_codes,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id: runtime_state.task_review_dispatch_id.clone(),
        final_review_dispatch_id: runtime_state
            .final_review_dispatch_authority
            .dispatch_id
            .clone(),
        current_branch_closure_id: runtime_state
            .authoritative_current_branch_closure_id
            .clone()
            .or(status.current_branch_closure_id.clone()),
        current_release_readiness_result: runtime_state
            .late_stage_bindings
            .current_release_readiness_result
            .clone()
            .or(status.current_release_readiness_state.clone()),
        base_branch: runtime_state.base_branch.clone(),
    }
}
