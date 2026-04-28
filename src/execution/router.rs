use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::diagnostics::JsonFailure;
use crate::execution::command_eligibility::{
    command_invokes_hidden_lane, command_is_legal_public_command, decide_public_mutation,
    public_mutation_request_from_command,
};
use crate::execution::current_truth::{
    CurrentTruthSnapshot, RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS,
    normalized_plan_qa_requirement, public_task_boundary_decision,
    resolve_actionable_repair_follow_up, task_boundary_projection_diagnostic_reason_code,
};
use crate::execution::follow_up::{
    follow_up_command_template, follow_up_from_phase_detail,
    normalize_public_routing_follow_up_token, repair_follow_up_source_decision_hash,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::next_action::{
    AuthoritativeStaleReentryTarget, NextActionAuthorityInputs, NextActionDecision, NextActionKind,
    NextActionRequestInputs, compute_next_action_decision_with_authority_inputs,
    exact_execution_command_from_decision, public_next_action_text,
};
use crate::execution::query::{
    ExecutionRoutingExecutionCommandContext, ExecutionRoutingRecordingContext,
    ExecutionRoutingState, WorkflowRoutingDecision, blocking_scope_for_phase_detail,
    canonical_phase_for_shared_decision, compact_operator_reason_codes, default_phase_for_status,
    external_wait_state_for_phase_detail, late_stage_observability_for_phase,
};
use crate::execution::reducer::{RuntimeState, reduce_execution_read_scope};
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_PHASE_DETAIL, TARGETLESS_STALE_RECONCILE_REASON_CODE,
    TargetlessStaleReconcile,
};
use crate::execution::state::{
    ExecutionReadScope, ExecutionRuntime, PlanExecutionStatus, StatusBlockingRecord,
    current_branch_closure_structural_review_state_reason,
    task_closure_baseline_bridge_ready_for_stale_target, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason,
};
use crate::workflow::status::WorkflowRoute;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NextPublicAction {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Blocker {
    pub category: String,
    pub scope_type: String,
    pub scope_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_public_action: Option<String>,
    pub details: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct RouteDecision {
    pub(crate) state_kind: String,
    pub(crate) phase: String,
    pub(crate) phase_detail: String,
    pub(crate) review_state_status: String,
    pub(crate) next_action: String,
    pub(crate) blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) required_follow_up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_public_action: Option<NextPublicAction>,
    pub(crate) blockers: Vec<Blocker>,
    #[serde(skip)]
    pub(crate) execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    #[serde(skip)]
    pub(crate) recording_context: Option<ExecutionRoutingRecordingContext>,
}

pub(crate) fn project_runtime_routing_state(
    _runtime: &ExecutionRuntime,
    read_scope: &ExecutionReadScope,
    external_review_result_ready: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    project_runtime_routing_state_with_exact_command_requirement(
        read_scope,
        external_review_result_ready,
        false,
    )
}

pub(crate) fn project_runtime_routing_state_with_exact_command_requirement(
    read_scope: &ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    let (routing, route_decision, _) = project_runtime_routing_state_with_reduced_state(
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
) -> Result<(ExecutionRoutingState, RouteDecision, RuntimeState), JsonFailure> {
    let mut runtime_state = reduce_execution_read_scope(read_scope)?;
    let mut route_decision = if require_exact_execution_command {
        route_decision_from_runtime_state_with_inputs(
            &runtime_state,
            external_review_result_ready,
            true,
        )
    } else {
        route_runtime_state(&runtime_state, external_review_result_ready)
    };
    let source_route_decision_hash = repair_follow_up_source_decision_hash(&route_decision);
    let route_bound_follow_up = resolve_actionable_repair_follow_up(
        &runtime_state,
        &CurrentTruthSnapshot::from_authoritative_state(read_scope.authoritative_state.as_ref())
            .with_source_route_decision_hash(source_route_decision_hash.as_deref()),
    )
    .map(|record| record.kind.public_token().to_owned());
    if runtime_state.persisted_repair_follow_up != route_bound_follow_up {
        runtime_state.persisted_repair_follow_up = route_bound_follow_up;
        route_decision = if require_exact_execution_command {
            route_decision_from_runtime_state_with_inputs(
                &runtime_state,
                external_review_result_ready,
                true,
            )
        } else {
            route_runtime_state(&runtime_state, external_review_result_ready)
        };
    }
    let route = route_from_runtime_state(&runtime_state);
    let routing = project_routing_from_runtime_state(
        route,
        &runtime_state,
        &route_decision,
        external_review_result_ready,
    );
    Ok((routing, route_decision, runtime_state))
}

pub(crate) fn project_non_runtime_workflow_routing_state(
    route: WorkflowRoute,
    external_review_result_ready: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    let workflow_phase = non_runtime_workflow_phase(&route.status);
    let (phase, phase_detail, next_action, recommended_command) = match workflow_phase.as_str() {
        "handoff_required" => (
            String::from("handoff_required"),
            String::from("handoff_recording_required"),
            String::from("hand off"),
            Some(format!(
                "featureforge plan execution transfer --plan {} --scope branch --to <owner> --reason <reason>",
                route.plan_path
            )),
        ),
        _ => (
            String::from("pivot_required"),
            String::from("planning_reentry_required"),
            String::from("pivot / return to planning"),
            None,
        ),
    };
    let blocking_reason_codes = compact_operator_reason_codes(None, &phase_detail, "clean");
    let external_wait_state = external_wait_state_for_phase_detail(
        &phase_detail,
        &blocking_reason_codes,
        external_review_result_ready,
    );
    let (reason_family, diagnostic_reason_codes) =
        late_stage_observability_for_phase(&workflow_phase, None, None);
    let mut routing = ExecutionRoutingState {
        route,
        route_decision: None,
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
        recommended_command,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state,
        blocking_reason_codes,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };
    let route_decision = route_decision_from_routing(&routing, &[]);
    routing.route_decision = Some(route_decision.clone());
    Ok((routing, route_decision))
}

fn non_runtime_workflow_phase(route_status: &str) -> String {
    match route_status {
        "spec_draft" => String::from("spec_review"),
        "plan_draft" => String::from("plan_review"),
        "spec_approved_needs_plan" | "stale_plan" => String::from("plan_writing"),
        "handoff_required" => String::from("handoff_required"),
        "implementation_ready" => String::from("implementation_handoff"),
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
        status: String::from("implementation_ready"),
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

#[cfg(test)]
pub(crate) fn shared_next_action_decision(
    context: &crate::execution::state::ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    external_review_result_ready: bool,
    task_review_dispatch_id: Option<&str>,
    final_review_dispatch_id: Option<&str>,
    final_review_dispatch_lineage_present: bool,
) -> Option<NextActionDecision> {
    compute_next_action_decision_with_authority_inputs(
        context,
        status,
        NextActionRequestInputs {
            plan_path,
            external_review_result_ready,
            task_review_dispatch_id,
            final_review_dispatch_id,
            final_review_dispatch_lineage_present,
        },
        NextActionAuthorityInputs::default(),
    )
}

fn shared_next_action_decision_from_runtime_state(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
) -> Option<NextActionDecision> {
    compute_next_action_decision_with_authority_inputs(
        &runtime_state.context,
        &runtime_state.status,
        NextActionRequestInputs {
            plan_path: &runtime_state.context.plan_rel,
            external_review_result_ready,
            task_review_dispatch_id: runtime_state.task_review_dispatch_id.as_deref(),
            final_review_dispatch_id: runtime_state
                .final_review_dispatch_authority
                .dispatch_id
                .as_deref(),
            final_review_dispatch_lineage_present: runtime_state
                .final_review_dispatch_authority
                .lineage_present,
        },
        NextActionAuthorityInputs {
            persisted_repair_follow_up: runtime_state.persisted_repair_follow_up.as_deref(),
            branch_rerecording_assessment: runtime_state.branch_rerecording_assessment.as_ref(),
            gate_finish: runtime_state.gate_snapshot.gate_finish.as_ref(),
            authoritative_stale_target: runtime_state
                .gate_snapshot
                .earliest_task_stale_target_details()
                .and_then(AuthoritativeStaleReentryTarget::from_stale_target),
        },
    )
}

pub(crate) struct SharedNextActionRoutingInputs<'a> {
    pub(crate) plan_path: &'a str,
    #[cfg(test)]
    pub(crate) external_review_result_ready: bool,
    pub(crate) require_exact_execution_command: bool,
    pub(crate) task_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_id: Option<&'a str>,
    #[cfg(test)]
    pub(crate) final_review_dispatch_lineage_present: bool,
    pub(crate) current_branch_closure_id: Option<&'a str>,
}

fn route_decision_from_runtime_state_with_inputs(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> RouteDecision {
    let status = &runtime_state.status;
    if status.review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
        && !status.current_task_closures.is_empty()
        && runtime_state
            .branch_rerecording_assessment
            .as_ref()
            .is_some_and(|assessment| assessment.supported)
        && matches!(
            status.harness_phase,
            HarnessPhase::DocumentReleasePending
                | HarnessPhase::FinalReviewPending
                | HarnessPhase::QaPending
                | HarnessPhase::ReadyForBranchCompletion
                | HarnessPhase::Executing
        )
    {
        return branch_closure_recording_route_decision(runtime_state, status);
    }
    if status
        .reason_codes
        .iter()
        .any(|code| code == TARGETLESS_STALE_RECONCILE_REASON_CODE)
    {
        return runtime_reconcile_route_decision(
            runtime_state,
            status,
            status
                .blocking_task
                .or_else(|| blocking_task_from_status_records(status)),
            TARGETLESS_STALE_RECONCILE_REASON_CODE,
        );
    }
    if status.blocking_records.iter().any(|record| {
        record.record_type == "review_state"
            && record.required_follow_up.as_deref() == Some("repair_review_state")
    }) && !task_closure_baseline_bridge_route_ready(runtime_state, status)
    {
        return repair_review_state_route_decision(
            runtime_state,
            status,
            status
                .blocking_task
                .or_else(|| blocking_task_from_status_records(status)),
            "derived_review_state_missing",
        );
    }
    if let Ok(Some(seed)) = shared_next_action_seed_from_runtime_state(
        runtime_state,
        external_review_result_ready,
        require_exact_execution_command,
    ) {
        if seed.phase_detail == "planning_reentry_required"
            && status.execution_started == "yes"
            && status.current_branch_closure_id.is_none()
            && !status
                .reason_codes
                .iter()
                .any(|code| code == "blocked_on_plan_revision")
            && let Some(task_number) = status
                .blocking_task
                .or_else(|| runtime_state.context.tasks_by_number.keys().copied().max())
        {
            return close_current_task_route_decision(runtime_state, status, task_number);
        }
        if let Some(route_decision) =
            final_review_dispatch_route_for_repaired_late_stage_drift(runtime_state, &seed)
        {
            return route_decision;
        }
        let mut recommended_command = match seed.phase_detail.as_str() {
            "final_review_recording_ready" => Some(format!(
                "featureforge plan execution advance-late-stage --plan {} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>",
                runtime_state.context.plan_rel
            )),
            _ => sanitize_public_recommended_command(
                seed.recommended_command.as_deref(),
                &seed.phase_detail,
            )
            .map(|command| materialize_plan_template(&command, &runtime_state.context.plan_rel)),
        };
        if recommended_command.is_none()
            && !RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&seed.phase_detail.as_str())
            && TargetlessStaleReconcile::from_phase_and_reason_codes(
                &seed.phase_detail,
                &seed.blocking_reason_codes,
            )
            .is_none()
            && let Some(follow_up_command) = status
                .blocking_records
                .first()
                .and_then(|record| follow_up_command_template(record.required_follow_up.as_deref()))
                .map(|template| {
                    materialize_plan_template(&template, &runtime_state.context.plan_rel)
                })
        {
            recommended_command = Some(follow_up_command);
        }
        let next_public_action =
            synthesize_next_public_action(recommended_command.as_deref(), &seed.phase_detail);
        let review_state_status = effective_route_review_state_status(status, &seed);
        let blocking_reason_codes = merge_reason_codes(
            public_route_blocking_reason_codes(status, &seed),
            compact_route_reason_codes(
                status,
                &seed.phase_detail,
                &review_state_status,
                seed.blocking_task,
                None,
            ),
        );
        let next_action = seed.next_action;
        let external_wait_state = external_wait_state_for_phase_detail(
            &seed.phase_detail,
            &blocking_reason_codes,
            external_review_result_ready,
        )
        .or_else(|| status.external_wait_state.clone());
        let state_kind = derive_state_kind_from_seed(
            external_wait_state.as_deref(),
            status.harness_phase,
            &seed.phase_detail,
            recommended_command.as_deref(),
        );
        let blockers =
            if targetless_stale_reconcile_for_phase(&seed.phase_detail, &blocking_reason_codes) {
                targetless_stale_reconcile_blockers(&seed.phase_detail)
            } else {
                let blockers = primary_blocker_for_status(
                    status,
                    state_kind.as_str(),
                    next_public_action.as_ref(),
                );
                materialize_blocker_actions(blockers, &runtime_state.context.plan_rel)
            };
        let required_follow_up = derive_required_follow_up(
            status,
            &seed.phase_detail,
            &review_state_status,
            blocking_reason_codes.iter().map(String::as_str),
            seed.execution_command_context.as_ref(),
        );
        if seed.phase_detail == "execution_reentry_required"
            && status.current_task_closures.is_empty()
            && let Some(task_number) = status.blocking_task.or(seed.blocking_task).or_else(|| {
                seed.execution_command_context
                    .as_ref()
                    .and_then(|context| context.task_number)
            })
            && (task_closure_baseline_bridge_route_ready(runtime_state, status)
                || close_current_task_public_repair_target_present(status, task_number)
                || reducer_dispatch_bridge_ready(runtime_state, status, task_number))
        {
            return close_current_task_route_decision(runtime_state, status, task_number);
        }
        if seed.phase_detail == "execution_reentry_required"
            && !status
                .reason_codes
                .iter()
                .any(|code| code == "prior_task_current_closure_stale")
            && prior_task_closure_progress_edge_required(status)
            && let Some(task_number) = status.blocking_task
        {
            return close_current_task_route_decision(runtime_state, status, task_number);
        }
        return RouteDecision {
            state_kind,
            phase: seed.phase,
            phase_detail: seed.phase_detail,
            review_state_status,
            next_action,
            blocking_reason_codes,
            recommended_command,
            required_follow_up,
            next_public_action,
            blockers,
            execution_command_context: seed.execution_command_context,
            recording_context: seed.recording_context,
        };
    }
    route_decision_for_unroutable_runtime_state(status)
}

fn public_route_blocking_reason_codes(
    status: &PlanExecutionStatus,
    seed: &WorkflowRoutingDecision,
) -> Vec<String> {
    if seed.blocking_task.is_some()
        && status.blocking_step.is_none()
        && matches!(
            seed.phase_detail.as_str(),
            "task_closure_recording_ready"
                | "task_review_result_pending"
                | "execution_reentry_required"
        )
    {
        return seed
            .blocking_reason_codes
            .iter()
            .filter(|reason_code| {
                !task_boundary_projection_diagnostic_reason_code(reason_code)
                    && reason_code.as_str() != "task_review_dispatch_required"
            })
            .cloned()
            .collect();
    }
    if status.blocking_task.is_some()
        && status.blocking_step.is_none()
        && seed.phase_detail == "task_closure_recording_ready"
    {
        return public_task_boundary_decision(status).public_reason_codes;
    }
    seed.blocking_reason_codes.clone()
}

fn prior_task_closure_progress_edge_required(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|code| {
        code == "prior_task_current_closure_missing"
            || code == "prior_task_current_closure_stale"
            || code == "current_task_closure_overlay_restore_required"
            || (code == "derived_review_state_missing"
                && status.review_state_status == "missing_current_closure"
                && status.current_task_closures.is_empty())
    }) && !status.reason_codes.iter().any(|code| {
        matches!(
            code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
        )
    })
}

fn targetless_stale_reconcile_for_phase(phase_detail: &str, reason_codes: &[String]) -> bool {
    TargetlessStaleReconcile::from_phase_and_reason_codes(phase_detail, reason_codes).is_some()
}

fn targetless_stale_reconcile_blockers(phase_detail: &str) -> Vec<Blocker> {
    let reconcile = TargetlessStaleReconcile;
    vec![Blocker {
        category: String::from("runtime_bug"),
        scope_type: String::from("runtime"),
        scope_key: phase_detail.to_owned(),
        record_id: None,
        next_public_action: None,
        details: String::from(reconcile.detail()),
    }]
}

fn task_closure_baseline_bridge_route_ready(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
) -> bool {
    status.blocking_task.is_some_and(|task_number| {
        if !reducer_stale_target_allows_task_closure_bridge(runtime_state, task_number) {
            return false;
        }
        task_closure_baseline_bridge_ready_for_stale_target(
            &runtime_state.context,
            status,
            task_number,
            runtime_state.gate_snapshot.earliest_task_stale_target(),
        )
        .unwrap_or(false)
    })
}

fn reducer_stale_target_allows_task_closure_bridge(
    runtime_state: &RuntimeState,
    task_number: u32,
) -> bool {
    let Some(target) = runtime_state
        .gate_snapshot
        .earliest_task_stale_target_details()
    else {
        return true;
    };
    target.task == Some(task_number) && target.reason_code != "prior_task_current_closure_stale"
}

fn close_current_task_public_repair_target_present(
    status: &PlanExecutionStatus,
    task_number: u32,
) -> bool {
    status.public_repair_targets.iter().any(|target| {
        target.command_kind == "close-current-task" && target.task == Some(task_number)
    })
}

fn reducer_dispatch_bridge_ready(
    runtime_state: &RuntimeState,
    _status: &PlanExecutionStatus,
    task_number: u32,
) -> bool {
    runtime_state.gate_snapshot.earliest_task_stale_target() == Some(task_number)
        && reducer_stale_target_allows_task_closure_bridge(runtime_state, task_number)
        && runtime_state.task_review_dispatch_id.is_some()
        && runtime_state
            .context
            .steps
            .iter()
            .filter(|step| step.task_number == task_number)
            .all(|step| step.checked)
}

pub(crate) fn route_runtime_state(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
) -> RouteDecision {
    route_decision_from_runtime_state_with_inputs(
        runtime_state,
        external_review_result_ready,
        false,
    )
}

pub(crate) fn router_allows_public_recommended_mutation(
    status: &PlanExecutionStatus,
    command: &str,
) -> bool {
    if command_invokes_hidden_lane(command) || !command_is_legal_public_command(command) {
        return false;
    }
    if let Some(request) = public_mutation_request_from_command(command) {
        return decide_public_mutation(status, &request).allowed;
    }
    command.starts_with("featureforge workflow operator --plan ")
        || command.starts_with("featureforge plan execution status --plan ")
}

fn effective_route_review_state_status(
    status: &PlanExecutionStatus,
    seed: &WorkflowRoutingDecision,
) -> String {
    if status.review_state_status == "stale_unreviewed"
        || (!status.stale_unreviewed_closures.is_empty()
            && seed.phase_detail == "task_closure_recording_ready")
    {
        return String::from("stale_unreviewed");
    }
    if status.review_state_status == "missing_current_closure" {
        String::from("missing_current_closure")
    } else {
        seed.review_state_status.clone()
    }
}

fn compact_route_reason_codes(
    status: &PlanExecutionStatus,
    phase_detail: &str,
    review_state_status: &str,
    blocking_task: Option<u32>,
    blocking_step: Option<u32>,
) -> Vec<String> {
    let mut projected_status = status.clone();
    if blocking_task.is_some() {
        projected_status.blocking_task = blocking_task;
        projected_status.blocking_step = blocking_step;
    }
    compact_operator_reason_codes(Some(&projected_status), phase_detail, review_state_status)
}

fn task_number_from_scope_key(scope_key: &str) -> Option<u32> {
    let raw = scope_key.strip_prefix("task-")?;
    let digits = raw
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
}

fn blocking_task_from_status_records(status: &PlanExecutionStatus) -> Option<u32> {
    status.blocking_records.iter().find_map(|record| {
        (record.scope_type == "task")
            .then(|| task_number_from_scope_key(&record.scope_key))
            .flatten()
    })
}

fn blocking_task_from_blockers(blockers: &[Blocker]) -> Option<u32> {
    blockers.iter().find_map(|blocker| {
        (blocker.scope_type == "task")
            .then(|| task_number_from_scope_key(&blocker.scope_key))
            .flatten()
    })
}

fn merge_reason_codes(mut primary: Vec<String>, secondary: Vec<String>) -> Vec<String> {
    for code in secondary {
        if !primary.iter().any(|existing| existing == &code) {
            primary.push(code);
        }
    }
    primary
}

fn close_current_task_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    task_number: u32,
) -> RouteDecision {
    if status
        .current_task_closures
        .iter()
        .any(|closure| closure.task == task_number)
        && status.current_branch_closure_id.is_none()
    {
        return branch_closure_recording_route_decision(runtime_state, status);
    }
    let phase_detail = String::from("task_closure_recording_ready");
    let recommended_command = Some(close_current_task_recording_command(
        &runtime_state.context.plan_rel,
        task_number,
    ));
    let next_public_action =
        synthesize_next_public_action(recommended_command.as_deref(), &phase_detail);
    let state_kind = derive_state_kind_from_seed(
        None,
        HarnessPhase::Executing,
        &phase_detail,
        recommended_command.as_deref(),
    );
    let blockers = materialize_blocker_actions(
        primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
        &runtime_state.context.plan_rel,
    );
    let blocking_reason_codes = compact_route_reason_codes(
        status,
        &phase_detail,
        if status.review_state_status == "stale_unreviewed"
            || !status.stale_unreviewed_closures.is_empty()
        {
            "stale_unreviewed"
        } else {
            &status.review_state_status
        },
        Some(task_number),
        None,
    );
    RouteDecision {
        state_kind,
        phase: String::from("task_closure_pending"),
        phase_detail,
        review_state_status: if status.review_state_status == "stale_unreviewed"
            || !status.stale_unreviewed_closures.is_empty()
        {
            String::from("stale_unreviewed")
        } else {
            status.review_state_status.clone()
        },
        next_action: String::from("close current task"),
        blocking_reason_codes,
        recommended_command,
        required_follow_up: None,
        next_public_action,
        blockers,
        execution_command_context: None,
        recording_context: Some(ExecutionRoutingRecordingContext {
            task_number: Some(task_number),
            dispatch_id: runtime_state.task_review_dispatch_id.clone(),
            branch_closure_id: None,
        }),
    }
}

fn repair_review_state_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    task_number: Option<u32>,
    reason_code: &str,
) -> RouteDecision {
    let phase_detail = String::from("execution_reentry_required");
    let recommended_command = Some(format!(
        "featureforge plan execution repair-review-state --plan {}",
        runtime_state.context.plan_rel
    ));
    let next_public_action =
        synthesize_next_public_action(recommended_command.as_deref(), &phase_detail);
    let review_state_status = status.review_state_status.clone();
    let mut blocking_reason_codes = compact_route_reason_codes(
        status,
        &phase_detail,
        &review_state_status,
        task_number.or(status.blocking_task),
        None,
    );
    if !blocking_reason_codes
        .iter()
        .any(|existing| existing == reason_code)
    {
        blocking_reason_codes.push(reason_code.to_owned());
    }
    let state_kind = derive_state_kind_from_seed(
        None,
        status.harness_phase,
        &phase_detail,
        recommended_command.as_deref(),
    );
    let blockers = materialize_blocker_actions(
        primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
        &runtime_state.context.plan_rel,
    );
    RouteDecision {
        state_kind,
        phase: String::from("executing"),
        phase_detail,
        review_state_status,
        next_action: String::from("repair review state / reenter execution"),
        blocking_reason_codes,
        recommended_command,
        required_follow_up: Some(String::from("repair_review_state")),
        next_public_action,
        blockers,
        execution_command_context: None,
        recording_context: None,
    }
}

fn runtime_reconcile_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    task_number: Option<u32>,
    reason_code: &str,
) -> RouteDecision {
    let phase_detail = String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL);
    let targetless_stale_reconcile =
        TargetlessStaleReconcile::from_reason_code(reason_code).is_some();
    let recommended_command = (!targetless_stale_reconcile).then(|| {
        format!(
            "featureforge plan execution repair-review-state --plan {}",
            runtime_state.context.plan_rel
        )
    });
    let next_public_action =
        synthesize_next_public_action(recommended_command.as_deref(), &phase_detail);
    let review_state_status = status.review_state_status.clone();
    let mut blocking_reason_codes = compact_route_reason_codes(
        status,
        &phase_detail,
        &review_state_status,
        task_number.or(status.blocking_task),
        None,
    );
    if targetless_stale_reconcile {
        TargetlessStaleReconcile::ensure_reason_codes(&mut blocking_reason_codes);
    } else if !blocking_reason_codes
        .iter()
        .any(|existing| existing == reason_code)
    {
        blocking_reason_codes.push(reason_code.to_owned());
    }
    let state_kind = derive_state_kind_from_seed(
        None,
        status.harness_phase,
        &phase_detail,
        recommended_command.as_deref(),
    );
    let blockers = if targetless_stale_reconcile {
        targetless_stale_reconcile_blockers(&phase_detail)
    } else {
        materialize_blocker_actions(
            primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
            &runtime_state.context.plan_rel,
        )
    };
    RouteDecision {
        state_kind,
        phase: String::from("executing"),
        phase_detail,
        review_state_status,
        next_action: String::from("repair review state / reenter execution"),
        blocking_reason_codes,
        recommended_command,
        required_follow_up: (!targetless_stale_reconcile)
            .then(|| String::from("repair_review_state")),
        next_public_action,
        blockers,
        execution_command_context: None,
        recording_context: None,
    }
}

fn branch_closure_recording_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
) -> RouteDecision {
    let phase_detail = String::from("branch_closure_recording_required_for_release_readiness");
    let recommended_command = Some(release_readiness_recording_command(
        &runtime_state.context.plan_rel,
    ));
    let next_public_action =
        synthesize_next_public_action(recommended_command.as_deref(), &phase_detail);
    let blockers = materialize_blocker_actions(
        primary_blocker_for_status(
            status,
            "actionable_public_command",
            next_public_action.as_ref(),
        ),
        &runtime_state.context.plan_rel,
    );
    RouteDecision {
        state_kind: String::from("actionable_public_command"),
        phase: String::from("document_release_pending"),
        phase_detail,
        review_state_status: String::from("missing_current_closure"),
        next_action: String::from("advance late stage"),
        blocking_reason_codes: vec![String::from("missing_current_closure")],
        recommended_command,
        required_follow_up: Some(String::from("advance_late_stage")),
        next_public_action,
        blockers,
        execution_command_context: None,
        recording_context: None,
    }
}

fn final_review_dispatch_route_for_repaired_late_stage_drift(
    runtime_state: &RuntimeState,
    seed: &WorkflowRoutingDecision,
) -> Option<RouteDecision> {
    let status = &runtime_state.status;
    if seed.phase_detail != "branch_closure_recording_required_for_release_readiness"
        || runtime_state.persisted_repair_follow_up.as_deref() != Some("advance_late_stage")
        || runtime_state
            .release_readiness_result_for_current_branch
            .as_deref()
            != Some("ready")
        || !status.current_branch_meaningful_drift
    {
        return None;
    }
    let branch_closure_id = status.current_branch_closure_id.as_ref()?;
    let phase_detail = String::from("final_review_dispatch_required");
    let next_public_action =
        synthesize_next_public_action(None, &phase_detail).map(|mut action| {
            action.command =
                materialize_plan_template(&action.command, &runtime_state.context.plan_rel);
            action.args_template = action.args_template.map(|template| {
                materialize_plan_template(&template, &runtime_state.context.plan_rel)
            });
            action
        });
    let blockers = vec![Blocker {
        category: String::from("late_stage"),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_id: None,
        next_public_action: next_public_action
            .as_ref()
            .map(|action| action.command.clone()),
        details: String::from(
            "A fresh external final review is required before late-stage progression can continue.",
        ),
    }];
    let blocking_reason_codes = compact_operator_reason_codes(Some(status), &phase_detail, "clean");
    Some(RouteDecision {
        state_kind: derive_state_kind_from_seed(
            None,
            HarnessPhase::FinalReviewPending,
            &phase_detail,
            None,
        ),
        phase: String::from("final_review_pending"),
        phase_detail,
        review_state_status: String::from("clean"),
        next_action: String::from("request final review"),
        blocking_reason_codes,
        recommended_command: None,
        required_follow_up: Some(String::from("request_external_review")),
        next_public_action,
        blockers,
        execution_command_context: None,
        recording_context: None,
    })
}

#[cfg(test)]
pub(crate) fn shared_next_action_seed_from_decision(
    context: &crate::execution::state::ExecutionContext,
    status: &PlanExecutionStatus,
    inputs: SharedNextActionRoutingInputs<'_>,
) -> Result<Option<WorkflowRoutingDecision>, JsonFailure> {
    let Some(decision) = shared_next_action_decision(
        context,
        status,
        inputs.plan_path,
        inputs.external_review_result_ready,
        inputs.task_review_dispatch_id,
        inputs.final_review_dispatch_id,
        inputs.final_review_dispatch_lineage_present,
    ) else {
        return Ok(None);
    };
    shared_next_action_seed_from_precomputed_decision(context, status, inputs, decision)
}

fn shared_next_action_seed_from_runtime_state(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<Option<WorkflowRoutingDecision>, JsonFailure> {
    let Some(decision) =
        shared_next_action_decision_from_runtime_state(runtime_state, external_review_result_ready)
    else {
        return Ok(None);
    };
    shared_next_action_seed_from_precomputed_decision(
        &runtime_state.context,
        &runtime_state.status,
        SharedNextActionRoutingInputs {
            plan_path: &runtime_state.context.plan_rel,
            #[cfg(test)]
            external_review_result_ready,
            require_exact_execution_command,
            task_review_dispatch_id: runtime_state.task_review_dispatch_id.as_deref(),
            final_review_dispatch_id: runtime_state
                .final_review_dispatch_authority
                .dispatch_id
                .as_deref(),
            #[cfg(test)]
            final_review_dispatch_lineage_present: runtime_state
                .final_review_dispatch_authority
                .lineage_present,
            current_branch_closure_id: runtime_state.status.current_branch_closure_id.as_deref(),
        },
        decision,
    )
}

fn shared_next_action_seed_from_precomputed_decision(
    context: &crate::execution::state::ExecutionContext,
    status: &PlanExecutionStatus,
    inputs: SharedNextActionRoutingInputs<'_>,
    decision: NextActionDecision,
) -> Result<Option<WorkflowRoutingDecision>, JsonFailure> {
    let mut default_phase = default_phase_for_shared_seed(status, &decision);
    let mut phase_detail = decision.phase_detail.clone();
    let review_state_status = decision.review_state_status.clone();
    let mut recording_context = None;
    let mut execution_command_context = None;
    let mut next_action = public_next_action_text(&decision);
    let mut recommended_command = decision.recommended_command.clone();
    let mut blocking_task = decision.blocking_task;
    let task_review_dispatch_id = inputs.task_review_dispatch_id.map(str::to_owned);
    let final_review_dispatch_id = inputs.final_review_dispatch_id.map(str::to_owned);

    let repair_review_state_reentry = decision.kind == NextActionKind::Reopen
        && (next_action == "repair review state / reenter execution"
            || (phase_detail == "execution_reentry_required"
                && review_state_status == "missing_current_closure"));
    if repair_review_state_reentry {
        recommended_command = Some(format!(
            "featureforge plan execution repair-review-state --plan {}",
            inputs.plan_path
        ));
    }
    let decision_requires_exact_execution_command = matches!(
        decision.kind,
        NextActionKind::Begin | NextActionKind::Resume
    ) || (decision.kind == NextActionKind::Reopen
        && !repair_review_state_reentry)
        || (decision.kind == NextActionKind::CloseCurrentTask
            && status.active_task.is_some()
            && status.active_step.is_some());
    if decision_requires_exact_execution_command {
        let exact_execution_command =
            exact_execution_command_from_decision(status, &decision, inputs.plan_path);
        if decision_requires_exact_execution_command
            && inputs.require_exact_execution_command
            && exact_execution_command.is_none()
        {
            return Ok(None);
        }
        if let Some(exact_execution_command) = exact_execution_command {
            execution_command_context = Some(ExecutionRoutingExecutionCommandContext {
                command_kind: String::from(exact_execution_command.command_kind),
                task_number: Some(exact_execution_command.task_number),
                step_id: exact_execution_command.step_id,
            });
            recommended_command = Some(exact_execution_command.recommended_command);
            if decision.kind == NextActionKind::Reopen {
                blocking_task = Some(exact_execution_command.task_number);
            }
        }
    }
    if next_action == "execution preflight"
        && marker_free_started_execution(context)
        && execution_command_context
            .as_ref()
            .is_some_and(|command| command.task_number == Some(1) && command.step_id == Some(1))
    {
        recommended_command = None;
        if !inputs.require_exact_execution_command {
            default_phase = String::from("executing");
            phase_detail = String::from("execution_in_progress");
            execution_command_context = None;
        }
    }
    if phase_detail == "task_closure_recording_ready"
        && !matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        )
    {
        if decision.kind == NextActionKind::CloseCurrentTask
            && let Some(task_number) = decision.task_number.or(status.blocking_task)
        {
            recording_context = Some(ExecutionRoutingRecordingContext {
                task_number: Some(task_number),
                dispatch_id: task_review_dispatch_id.clone(),
                branch_closure_id: None,
            });
            recommended_command = Some(close_current_task_recording_command(
                inputs.plan_path,
                task_number,
            ));
            next_action = String::from("close current task");
            blocking_task = Some(task_number);
        } else if review_state_status == "missing_current_closure" {
            recommended_command = Some(release_readiness_recording_command(inputs.plan_path));
            next_action = String::from("advance late stage");
            blocking_task = decision.task_number.or(status.blocking_task);
        } else {
            recommended_command = Some(format!(
                "featureforge plan execution repair-review-state --plan {}",
                inputs.plan_path
            ));
            next_action = String::from("repair review state / reenter execution");
            blocking_task = decision.task_number.or(status.blocking_task);
        }
    } else if phase_detail == "task_closure_recording_ready" {
        if let Some(task_number) = decision.task_number.or(status.blocking_task) {
            recording_context = Some(ExecutionRoutingRecordingContext {
                task_number: Some(task_number),
                dispatch_id: task_review_dispatch_id.clone(),
                branch_closure_id: None,
            });
            recommended_command = Some(close_current_task_recording_command(
                inputs.plan_path,
                task_number,
            ));
            next_action = String::from("close current task");
            blocking_task = Some(task_number);
        }
    } else if phase_detail == "final_review_recording_ready" {
        recording_context = inputs.current_branch_closure_id.map(|branch_closure_id| {
            ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: final_review_dispatch_id.clone(),
                branch_closure_id: Some(branch_closure_id.to_owned()),
            }
        });
        recommended_command = Some(final_review_recording_command(inputs.plan_path));
        next_action = String::from("advance late stage");
    } else if matches!(
        phase_detail.as_str(),
        "release_readiness_recording_ready" | "release_blocker_resolution_required"
    ) {
        recording_context = inputs.current_branch_closure_id.map(|branch_closure_id| {
            ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: None,
                branch_closure_id: Some(branch_closure_id.to_owned()),
            }
        });
    }
    if phase_detail == "task_closure_recording_ready"
        && stale_branch_closure_refresh_required(status)
    {
        phase_detail = String::from("branch_closure_recording_required_for_release_readiness");
        recording_context = None;
        execution_command_context = None;
        recommended_command = Some(release_readiness_recording_command(inputs.plan_path));
        next_action = String::from("advance late stage");
        blocking_task = None;
    }

    Ok(Some(WorkflowRoutingDecision {
        phase: canonical_phase_for_shared_decision(default_phase.as_str(), phase_detail.as_str()),
        phase_detail,
        review_state_status,
        recording_context,
        execution_command_context,
        next_action,
        recommended_command,
        blocking_scope: None,
        blocking_task,
        external_wait_state: None,
        blocking_reason_codes: decision.blocking_reason_codes.clone(),
    }))
}

fn default_phase_for_shared_seed(
    status: &PlanExecutionStatus,
    decision: &NextActionDecision,
) -> String {
    if matches!(
        status.harness_phase,
        HarnessPhase::ContractDrafting
            | HarnessPhase::PivotRequired
            | HarnessPhase::HandoffRequired
    ) {
        default_phase_for_status(status)
    } else {
        decision.phase.clone()
    }
}

fn stale_branch_closure_refresh_required(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_some()
        && status.current_branch_meaningful_drift
        && status.blocking_records.iter().any(|record| {
            record.record_type == "branch_closure"
                && record.review_state_status == "missing_current_closure"
        })
}

pub(crate) fn required_follow_up_from_route_decision(
    route_decision: &RouteDecision,
) -> Option<String> {
    route_decision.required_follow_up.clone()
}

fn derive_required_follow_up<'a>(
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

fn derive_required_follow_up_from_optional_status<'a>(
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
        return Some(String::from("repair_review_state"));
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
    if review_state_status == "stale_unreviewed" {
        return true;
    }
    if phase_detail != "execution_reentry_required" {
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
                    .any(|code| code == "stale_provenance");
        task_scope_structural_review_state_reason(status).is_some()
            || task_scope_review_state_repair_reason(status).is_some()
            || current_branch_closure_structural_review_state_reason(status).is_some()
            || status
                .reason_codes
                .iter()
                .any(|code| code == "derived_review_state_missing")
            || late_stage_stale_provenance_without_branch_binding
    })
}

fn close_current_task_recording_command(plan_path: &str, task_number: u32) -> String {
    format!(
        "featureforge plan execution close-current-task --plan {plan_path} --task {task_number} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
    )
}

fn final_review_recording_command(plan_path: &str) -> String {
    format!(
        "featureforge plan execution advance-late-stage --plan {plan_path} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>"
    )
}

fn release_readiness_recording_command(plan_path: &str) -> String {
    format!("featureforge plan execution advance-late-stage --plan {plan_path}")
}

pub(crate) fn route_decision_from_routing(
    routing: &ExecutionRoutingState,
    blocking_records: &[StatusBlockingRecord],
) -> RouteDecision {
    let state_kind = derive_state_kind(routing);
    let recommended_command = sanitize_public_recommended_command(
        routing.recommended_command.as_deref(),
        &routing.phase_detail,
    )
    .map(|command| materialize_plan_template(&command, &routing.route.plan_path));
    let next_public_action = synthesize_next_public_action(
        recommended_command.as_deref(),
        &routing.phase_detail,
    )
    .map(|mut action| {
        action.command = materialize_plan_template(&action.command, &routing.route.plan_path);
        action.args_template = action
            .args_template
            .map(|template| materialize_plan_template(&template, &routing.route.plan_path));
        action
    });
    let blockers = if targetless_stale_reconcile_for_phase(
        &routing.phase_detail,
        &routing.blocking_reason_codes,
    ) {
        targetless_stale_reconcile_blockers(&routing.phase_detail)
    } else {
        let blockers = primary_blocker_for_route(
            routing,
            blocking_records,
            state_kind.as_str(),
            next_public_action.as_ref(),
        );
        materialize_blocker_actions(blockers, &routing.route.plan_path)
    };
    let route_next_action = if state_kind == "blocked_runtime_bug" {
        String::from("runtime diagnostic required")
    } else {
        routing.next_action.clone()
    };
    let route_required_follow_up = if state_kind == "blocked_runtime_bug" {
        None
    } else {
        derive_required_follow_up_from_optional_status(
            routing.execution_status.as_ref(),
            &routing.phase_detail,
            &routing.review_state_status,
            routing.blocking_reason_codes.iter().map(String::as_str),
            routing.execution_command_context.as_ref(),
        )
    };
    RouteDecision {
        state_kind,
        phase: canonical_phase_for_shared_decision(&routing.phase, &routing.phase_detail),
        phase_detail: routing.phase_detail.clone(),
        review_state_status: routing.review_state_status.clone(),
        next_action: route_next_action,
        blocking_reason_codes: routing.blocking_reason_codes.clone(),
        recommended_command,
        required_follow_up: route_required_follow_up,
        next_public_action,
        blockers,
        execution_command_context: routing.execution_command_context.clone(),
        recording_context: routing.recording_context.clone(),
    }
}

fn marker_free_started_execution(context: &crate::execution::state::ExecutionContext) -> bool {
    context.plan_document.execution_mode != "none"
        && context.evidence.attempts.is_empty()
        && context.steps.iter().all(|step| {
            !step.checked && step.note_state.is_none() && step.note_summary.trim().is_empty()
        })
}

fn route_decision_for_unroutable_runtime_state(status: &PlanExecutionStatus) -> RouteDecision {
    let recommended_command = None;
    let next_public_action = None;
    let blockers =
        primary_blocker_for_status(status, "blocked_runtime_bug", next_public_action.as_ref());
    RouteDecision {
        state_kind: String::from("blocked_runtime_bug"),
        phase: canonical_phase_for_shared_decision(
            &default_phase_for_status(status),
            "runtime_route_unavailable",
        ),
        phase_detail: status.phase_detail.clone(),
        review_state_status: status.review_state_status.clone(),
        next_action: String::from("runtime diagnostic required"),
        blocking_reason_codes: compact_operator_reason_codes(
            Some(status),
            &status.phase_detail,
            &status.review_state_status,
        ),
        recommended_command,
        required_follow_up: None,
        next_public_action,
        blockers,
        execution_command_context: None,
        recording_context: None,
    }
}

pub(crate) fn route_decision_with_status_blockers(
    mut route_decision: RouteDecision,
    status: &PlanExecutionStatus,
) -> RouteDecision {
    if targetless_stale_reconcile_for_phase(
        &route_decision.phase_detail,
        &route_decision.blocking_reason_codes,
    ) {
        route_decision.blockers = targetless_stale_reconcile_blockers(&route_decision.phase_detail);
        route_decision.required_follow_up = None;
    } else {
        route_decision.blockers = primary_blocker_for_status(
            status,
            route_decision.state_kind.as_str(),
            route_decision.next_public_action.as_ref(),
        );
        route_decision.required_follow_up = derive_required_follow_up(
            status,
            &route_decision.phase_detail,
            &route_decision.review_state_status,
            route_decision
                .blocking_reason_codes
                .iter()
                .map(String::as_str),
            route_decision.execution_command_context.as_ref(),
        );
    }
    route_decision
}

fn project_routing_from_runtime_state(
    route: WorkflowRoute,
    runtime_state: &RuntimeState,
    route_decision: &RouteDecision,
    _external_review_result_ready: bool,
) -> ExecutionRoutingState {
    let status = runtime_state.status.clone();
    let (reason_family, mut diagnostic_reason_codes) = late_stage_observability_for_phase(
        &route_decision.phase,
        runtime_state.gate_review.as_ref(),
        runtime_state.gate_finish.as_ref(),
    );
    for reason_code in public_task_boundary_decision(&status).diagnostic_reason_codes {
        if !diagnostic_reason_codes
            .iter()
            .any(|existing| existing == &reason_code)
        {
            diagnostic_reason_codes.push(reason_code);
        }
    }
    let mut blocking_scope = status.blocking_scope.clone();
    let mut blocking_task = status.blocking_task;
    let recording_context = match route_decision.phase_detail.as_str() {
        "final_review_recording_ready" => runtime_state
            .authoritative_current_branch_closure_id
            .as_ref()
            .map(|branch_closure_id| ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: runtime_state
                    .final_review_dispatch_authority
                    .dispatch_id
                    .clone(),
                branch_closure_id: Some(branch_closure_id.clone()),
            }),
        "release_readiness_recording_ready" | "release_blocker_resolution_required" => {
            runtime_state
                .authoritative_current_branch_closure_id
                .as_ref()
                .map(|branch_closure_id| ExecutionRoutingRecordingContext {
                    task_number: None,
                    dispatch_id: None,
                    branch_closure_id: Some(branch_closure_id.clone()),
                })
        }
        _ => route_decision.recording_context.clone().or_else(|| {
            status
                .recording_context
                .as_ref()
                .map(|context| ExecutionRoutingRecordingContext {
                    task_number: context.task_number,
                    dispatch_id: context.dispatch_id.clone(),
                    branch_closure_id: context.branch_closure_id.clone(),
                })
        }),
    };
    let execution_command_context = route_decision.execution_command_context.clone();
    if route_decision.phase_detail == "execution_reentry_required"
        && let Some(task_number) = execution_command_context
            .as_ref()
            .and_then(|context| context.task_number)
            .or_else(|| blocking_task_from_blockers(&route_decision.blockers))
            .or_else(|| blocking_task_from_status_records(&status))
    {
        blocking_scope = Some(String::from("task"));
        blocking_task = Some(task_number);
    } else if route_decision.phase_detail == "task_closure_recording_ready"
        && let Some(task_number) = recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        blocking_scope = Some(String::from("task"));
        blocking_task = Some(task_number);
    } else if route_decision.phase_detail
        == "branch_closure_recording_required_for_release_readiness"
    {
        blocking_scope = Some(String::from("branch"));
        blocking_task = None;
    }
    let blocking_reason_codes = route_decision.blocking_reason_codes.clone();
    let blocking_scope = blocking_scope_for_phase_detail(
        &route_decision.phase_detail,
        blocking_task,
        Some(&status),
        &route_decision.review_state_status,
    )
    .or(blocking_scope);
    let external_wait_state = external_wait_state_for_phase_detail(
        &route_decision.phase_detail,
        &blocking_reason_codes,
        _external_review_result_ready,
    )
    .or_else(|| status.external_wait_state.clone());
    ExecutionRoutingState {
        route,
        route_decision: Some(route_decision.clone()),
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
        recording_context,
        execution_command_context,
        next_action: route_decision.next_action.clone(),
        recommended_command: route_decision.recommended_command.clone(),
        blocking_scope,
        blocking_task,
        external_wait_state,
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

fn blocker_from_status_record(
    record: &StatusBlockingRecord,
    phase_detail: &str,
    next_public_action: Option<&NextPublicAction>,
) -> Blocker {
    let category = match record.scope_type.as_str() {
        "task" => String::from("task_boundary"),
        "branch" => String::from("late_stage"),
        _ => String::from("structural"),
    };
    Blocker {
        category,
        scope_type: record.scope_type.clone(),
        scope_key: record.scope_key.clone(),
        record_id: record.record_id.clone(),
        next_public_action: next_public_action
            .map(|action| action.command.clone())
            .or_else(|| {
                (!RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail))
                    .then(|| follow_up_command_template(record.required_follow_up.as_deref()))
                    .flatten()
            }),
        details: record.message.clone(),
    }
}

struct BlockerSource<'a> {
    phase_detail: &'a str,
    blocking_scope: Option<&'a str>,
    blocking_task: Option<u32>,
    blocking_records: &'a [StatusBlockingRecord],
}

fn primary_blocker_for_route(
    routing: &ExecutionRoutingState,
    blocking_records: &[StatusBlockingRecord],
    state_kind: &str,
    next_public_action: Option<&NextPublicAction>,
) -> Vec<Blocker> {
    primary_blocker_for_source(
        BlockerSource {
            phase_detail: &routing.phase_detail,
            blocking_scope: routing.blocking_scope.as_deref(),
            blocking_task: routing.blocking_task,
            blocking_records,
        },
        state_kind,
        next_public_action,
    )
}

fn primary_blocker_for_status(
    status: &PlanExecutionStatus,
    state_kind: &str,
    next_public_action: Option<&NextPublicAction>,
) -> Vec<Blocker> {
    primary_blocker_for_source(
        BlockerSource {
            phase_detail: &status.phase_detail,
            blocking_scope: status.blocking_scope.as_deref(),
            blocking_task: status.blocking_task,
            blocking_records: &status.blocking_records,
        },
        state_kind,
        next_public_action,
    )
}

fn primary_blocker_for_source(
    source: BlockerSource<'_>,
    state_kind: &str,
    next_public_action: Option<&NextPublicAction>,
) -> Vec<Blocker> {
    if state_kind == "terminal" {
        return Vec::new();
    }

    if state_kind == "waiting_external_input" {
        let scope_type = source
            .blocking_scope
            .map(str::to_owned)
            .unwrap_or_else(|| String::from("external"));
        let scope_key = source
            .blocking_task
            .map(|task| format!("task-{task}"))
            .unwrap_or_else(|| source.phase_detail.to_owned());
        return vec![Blocker {
            category: String::from("external_input"),
            scope_type,
            scope_key,
            record_id: None,
            next_public_action: next_public_action.map(|action| action.command.clone()),
            details: String::from("Waiting for external review result."),
        }];
    }

    if let Some(primary) = source.blocking_records.first() {
        return vec![blocker_from_status_record(
            primary,
            source.phase_detail,
            next_public_action,
        )];
    }

    if state_kind == "blocked_runtime_bug" {
        return vec![Blocker {
            category: String::from("runtime_bug"),
            scope_type: String::from("runtime"),
            scope_key: source.phase_detail.to_owned(),
            record_id: None,
            next_public_action: next_public_action.map(|action| action.command.clone()),
            details: format!(
                "Routing reached `{}` without an actionable public recommendation.",
                source.phase_detail
            ),
        }];
    }

    if let Some(next_public_action) = next_public_action {
        return vec![Blocker {
            category: String::from("workflow"),
            scope_type: source
                .blocking_scope
                .map(str::to_owned)
                .unwrap_or_else(|| String::from("route")),
            scope_key: source
                .blocking_task
                .map(|task| format!("task-{task}"))
                .unwrap_or_else(|| source.phase_detail.to_owned()),
            record_id: None,
            next_public_action: Some(next_public_action.command.clone()),
            details: format!(
                "Follow the public routing lane for `{}`.",
                source.phase_detail
            ),
        }];
    }

    Vec::new()
}

fn materialize_plan_template(template: &str, plan_path: &str) -> String {
    template.replace("<approved-plan-path>", plan_path)
}

fn materialize_blocker_actions(mut blockers: Vec<Blocker>, plan_path: &str) -> Vec<Blocker> {
    for blocker in &mut blockers {
        if let Some(action) = blocker.next_public_action.as_mut() {
            *action = materialize_plan_template(action, plan_path);
        }
    }
    blockers
}

fn synthesize_next_public_action(
    recommended_command: Option<&str>,
    phase_detail: &str,
) -> Option<NextPublicAction> {
    if let Some(command) = sanitize_public_recommended_command(recommended_command, phase_detail) {
        return Some(NextPublicAction {
            command: command.clone(),
            args_template: Some(command),
        });
    }
    let command = match phase_detail {
        "final_review_dispatch_required" | "test_plan_refresh_required" => {
            "featureforge workflow operator --plan <approved-plan-path>"
        }
        _ => return None,
    };
    Some(NextPublicAction {
        command: String::from(command),
        args_template: Some(String::from(command)),
    })
}

fn derive_state_kind(routing: &ExecutionRoutingState) -> String {
    let recommended_command = sanitize_public_recommended_command(
        routing.recommended_command.as_deref(),
        &routing.phase_detail,
    );
    classify_state_kind(
        routing.external_wait_state.as_deref(),
        routing.phase == "ready_for_branch_completion",
        &routing.phase_detail,
        recommended_command.as_deref(),
    )
}

fn derive_state_kind_from_seed(
    external_wait_state: Option<&str>,
    harness_phase: HarnessPhase,
    phase_detail: &str,
    recommended_command: Option<&str>,
) -> String {
    classify_state_kind(
        external_wait_state,
        harness_phase == HarnessPhase::ReadyForBranchCompletion,
        phase_detail,
        recommended_command,
    )
}

fn classify_state_kind(
    external_wait_state: Option<&str>,
    terminal_phase: bool,
    phase_detail: &str,
    recommended_command: Option<&str>,
) -> String {
    if let Some(external_wait_state) = external_wait_state
        && external_wait_state == "waiting_for_external_review_result"
    {
        return String::from("waiting_external_input");
    }
    if terminal_phase
        && phase_detail == "finish_completion_gate_ready"
        && recommended_command.is_none()
    {
        return String::from("terminal");
    }
    if phase_detail == "planning_reentry_required" && recommended_command.is_none() {
        return String::from("waiting_external_input");
    }
    if recommended_command.is_none()
        && !RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail)
    {
        return String::from("blocked_runtime_bug");
    }
    String::from("actionable_public_command")
}

fn sanitize_public_recommended_command(
    recommended_command: Option<&str>,
    phase_detail: &str,
) -> Option<String> {
    let had_recommended_command = recommended_command
        .map(str::trim)
        .is_some_and(|command| !command.is_empty());
    let command = recommended_command
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .filter(|command| !command_invokes_hidden_lane(command))
        .filter(|command| command_is_legal_public_command(command))
        .map(ToOwned::to_owned);
    if let Some(command) = command {
        if RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail)
            && command.starts_with(GENERIC_WORKFLOW_OPERATOR_PREFIX)
        {
            return None;
        }
        return Some(command);
    }
    if RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail) {
        return None;
    }
    if !had_recommended_command {
        return None;
    }
    fallback_public_command_for_phase_detail(phase_detail)
}

const GENERIC_WORKFLOW_OPERATOR_PREFIX: &str = "featureforge workflow operator --plan ";

fn fallback_public_command_for_phase_detail(phase_detail: &str) -> Option<String> {
    match phase_detail {
        "final_review_dispatch_required" | "test_plan_refresh_required" => Some(String::from(
            "featureforge workflow operator --plan <approved-plan-path>",
        )),
        "execution_reentry_required" | "planning_reentry_required" => Some(String::from(
            "featureforge plan execution repair-review-state --plan <approved-plan-path>",
        )),
        "branch_closure_recording_required_for_release_readiness"
        | "release_readiness_recording_ready"
        | "release_blocker_resolution_required" => Some(String::from(
            "featureforge plan execution advance-late-stage --plan <approved-plan-path>",
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        primary_blocker_for_route, route_decision_from_routing, synthesize_next_public_action,
    };
    use crate::execution::follow_up::follow_up_command_template as follow_up_to_command_template;
    use crate::execution::harness::{
        AggregateEvaluationState, ChunkId, DownstreamFreshnessState, HarnessPhase,
    };
    use crate::execution::query::ExecutionRoutingState;
    use crate::execution::state::PlanExecutionStatus;
    use crate::workflow::status::WorkflowRoute;

    #[test]
    fn public_follow_up_templates_do_not_surface_removed_hidden_commands() {
        let follow_ups = [
            "repair_review_state",
            "advance_late_stage",
            "resolve_release_blocker",
            "record_handoff",
            "execution_reentry",
            "request_external_review",
            "wait_for_external_review_result",
            "run_verification",
        ];
        let hidden_tokens = [
            "record-review-dispatch",
            "gate-review",
            "gate-finish",
            "rebuild-evidence",
            "plan execution preflight",
            "plan execution recommend",
            "workflow recommend",
            "workflow preflight",
        ];
        for follow_up in follow_ups {
            let template = follow_up_to_command_template(Some(follow_up))
                .expect("known follow-up should map to a command template");
            for hidden in hidden_tokens {
                assert!(
                    !template.contains(hidden),
                    "public follow-up templates must not reference removed hidden commands, saw `{hidden}` in `{template}`"
                );
            }
        }
    }

    #[test]
    fn task_review_dispatch_lane_does_not_expose_public_action_or_blocker_command() {
        assert!(
            synthesize_next_public_action(None, "task_review_dispatch_required").is_none(),
            "task-review dispatch is no longer a public route"
        );
        let routing = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("implementation_ready"),
                next_skill: String::from("featureforge:executing-plans"),
                spec_path: String::from("docs/featureforge/specs/spec.md"),
                plan_path: String::from("docs/featureforge/plans/plan.md"),
                contract_state: String::from("clean"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("task_closure_pending"),
            phase: String::from("task_closure_pending"),
            phase_detail: String::from("task_review_dispatch_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("runtime diagnostic required"),
            recommended_command: None,
            blocking_scope: Some(String::from("task")),
            blocking_task: Some(2),
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };
        let blockers = primary_blocker_for_route(&routing, &[], "actionable_public_command", None);
        assert!(
            blockers.is_empty(),
            "legacy task-review dispatch lanes must not create public blockers: {blockers:?}"
        );
    }

    #[test]
    fn waiting_external_input_omits_public_follow_up_until_result_arrives() {
        let routing = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("implementation_ready"),
                next_skill: String::from("featureforge:requesting-code-review"),
                spec_path: String::from("docs/featureforge/specs/spec.md"),
                plan_path: String::from("docs/featureforge/plans/plan.md"),
                contract_state: String::from("clean"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("final_review_pending"),
            phase: String::from("final_review_pending"),
            phase_detail: String::from("final_review_outcome_pending"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("wait for external review result"),
            recommended_command: None,
            blocking_scope: Some(String::from("branch")),
            blocking_task: None,
            external_wait_state: Some(String::from("waiting_for_external_review_result")),
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: Some(String::from("dispatch-1")),
            current_branch_closure_id: Some(String::from("branch-1")),
            current_release_readiness_result: Some(String::from("ready")),
            base_branch: Some(String::from("main")),
        };

        let decision = route_decision_from_routing(&routing, &[]);
        assert_eq!(decision.state_kind, "waiting_external_input");
        assert!(decision.next_public_action.is_none());
        assert_eq!(decision.blockers.len(), 1);
        assert_eq!(decision.blockers[0].category, "external_input");
        assert_eq!(decision.blockers[0].scope_type, "branch");
        assert_eq!(
            decision.blockers[0].scope_key,
            "final_review_outcome_pending"
        );
        assert!(decision.blockers[0].next_public_action.is_none());
    }

    #[test]
    fn blocked_runtime_bug_surfaces_single_diagnostic_blocker() {
        let routing = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("implementation_ready"),
                next_skill: String::from("featureforge:executing-plans"),
                spec_path: String::from("docs/featureforge/specs/spec.md"),
                plan_path: String::from("docs/featureforge/plans/plan.md"),
                contract_state: String::from("clean"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from("executing"),
            phase: String::from("executing"),
            phase_detail: String::from("execution_reentry_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("execution reentry required"),
            recommended_command: None,
            blocking_scope: Some(String::from("workflow")),
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        };

        let decision = route_decision_from_routing(&routing, &[]);
        assert_eq!(decision.state_kind, "blocked_runtime_bug");
        assert!(decision.next_public_action.is_none());
        assert!(decision.recommended_command.is_none());
        assert!(decision.required_follow_up.is_none());
        assert_eq!(decision.next_action, "runtime diagnostic required");
        assert_eq!(decision.blockers.len(), 1);
        assert_eq!(decision.blockers[0].category, "runtime_bug");
        assert!(decision.blockers[0].next_public_action.is_none());
    }

    #[test]
    fn hidden_recommended_commands_are_sanitized_to_public_follow_ups() {
        let status = PlanExecutionStatus {
            schema_version: 3,
            plan_revision: 1,
            execution_run_id: None,
            workspace_state_id: String::from("semantic_tree:ignored"),
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            current_branch_meaningful_drift: false,
            current_task_closures: Vec::new(),
            superseded_closures_summary: Vec::new(),
            stale_unreviewed_closures: Vec::new(),
            current_release_readiness_state: None,
            current_final_review_state: String::from("missing"),
            current_qa_state: String::from("missing"),
            current_final_review_branch_closure_id: None,
            current_final_review_result: None,
            current_qa_branch_closure_id: None,
            current_qa_result: None,
            qa_requirement: None,
            latest_authoritative_sequence: 1,
            phase: Some(String::from("executing")),
            harness_phase: HarnessPhase::Executing,
            chunk_id: ChunkId(String::from("chunk-1")),
            chunking_strategy: None,
            evaluator_policy: None,
            reset_policy: None,
            review_stack: None,
            active_contract_path: None,
            active_contract_fingerprint: None,
            required_evaluator_kinds: Vec::new(),
            completed_evaluator_kinds: Vec::new(),
            pending_evaluator_kinds: Vec::new(),
            non_passing_evaluator_kinds: Vec::new(),
            aggregate_evaluation_state: AggregateEvaluationState::Pending,
            last_evaluation_report_path: None,
            last_evaluation_report_fingerprint: None,
            last_evaluation_evaluator_kind: None,
            last_evaluation_verdict: None,
            current_chunk_retry_count: 0,
            current_chunk_retry_budget: 0,
            current_chunk_pivot_threshold: 0,
            handoff_required: false,
            open_failed_criteria: Vec::new(),
            write_authority_state: String::from("idle"),
            write_authority_holder: None,
            write_authority_worktree: None,
            repo_state_baseline_head_sha: None,
            repo_state_baseline_worktree_fingerprint: None,
            repo_state_drift_state: String::from("clean"),
            dependency_index_state: String::from("clean"),
            final_review_state: DownstreamFreshnessState::Missing,
            browser_qa_state: DownstreamFreshnessState::Missing,
            release_docs_state: DownstreamFreshnessState::Missing,
            last_final_review_artifact_fingerprint: None,
            last_browser_qa_artifact_fingerprint: None,
            last_release_docs_artifact_fingerprint: None,
            strategy_state: String::from("clean"),
            last_strategy_checkpoint_fingerprint: None,
            strategy_checkpoint_kind: String::from("none"),
            strategy_reset_required: false,
            phase_detail: String::from("task_review_dispatch_required"),
            review_state_status: String::from("clean"),
            recording_context: None,
            execution_command_context: None,
            execution_reentry_target_source: None,
            public_repair_targets: Vec::new(),
            blocking_records: Vec::new(),
            blocking_scope: Some(String::from("task")),
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            projection_diagnostics: Vec::new(),
            state_kind: String::from("actionable_public_command"),
            next_public_action: None,
            blockers: Vec::new(),
            semantic_workspace_tree_id: String::from("semantic_tree:authoritative"),
            raw_workspace_tree_id: Some(String::from("git_tree:debug")),
            next_action: String::from("runtime diagnostic required"),
            recommended_command: Some(String::from(
                "featureforge plan execution record-review-dispatch --plan docs/featureforge/plans/example.md --scope task --task 1",
            )),
            finish_review_gate_pass_branch_closure_id: None,
            reason_codes: Vec::new(),
            execution_mode: String::from("none"),
            execution_fingerprint: String::from("fingerprint"),
            evidence_path: String::from("docs/featureforge/execution-evidence/example"),
            projection_mode: String::from("state_dir_only"),
            state_dir_projection_paths: Vec::new(),
            tracked_projection_paths: Vec::new(),
            tracked_projections_current: false,
            execution_started: String::from("yes"),
            warning_codes: Vec::new(),
            active_task: None,
            active_step: None,
            blocking_task: Some(1),
            blocking_step: None,
            resume_task: None,
            resume_step: None,
        };

        let recommended = super::sanitize_public_recommended_command(
            status.recommended_command.as_deref(),
            &status.phase_detail,
        );
        assert_eq!(recommended.as_deref(), None);
        assert!(
            synthesize_next_public_action(recommended.as_deref(), &status.phase_detail).is_none(),
            "task-review dispatch is diagnostic-only and must not synthesize a public operator loop"
        );
    }

    #[test]
    fn removed_plan_execution_commands_are_not_public_recommendations() {
        for removed in ["preflight", "recommend"] {
            let command =
                format!("featureforge plan execution {removed} --plan docs/plan.md --json");
            assert_eq!(
                super::sanitize_public_recommended_command(
                    Some(command.as_str()),
                    "execution_reentry_required"
                ),
                Some(String::from(
                    "featureforge plan execution repair-review-state --plan <approved-plan-path>"
                )),
                "removed `{removed}` command must sanitize to a public follow-up"
            );
        }
    }
}
