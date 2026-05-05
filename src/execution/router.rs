use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::diagnostics::JsonFailure;
use crate::execution::closure_diagnostics::{
    merge_task_boundary_projection_diagnostics, public_task_boundary_decision,
    task_boundary_projection_diagnostic_reason_code,
};
use crate::execution::command_eligibility::{
    PublicAdvanceLateStageMode, PublicCommand, PublicCommandInputRequirement,
    PublicCommandInvocation, PublicCommandKind, PublicMutationKind,
    public_advance_late_stage_mode_for_phase_detail, public_command_recommendation_surfaces,
};
use crate::execution::current_truth::{
    CurrentTruthSnapshot, normalized_plan_qa_requirement, resolve_actionable_repair_follow_up,
};
use crate::execution::follow_up::{
    follow_up_from_phase_detail, normalize_public_routing_follow_up_token,
    repair_follow_up_source_decision_hash,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::next_action::{
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED, NextActionAuthorityInputs, NextActionDecision,
    NextActionRequestInputs, compute_next_action_decision_with_authority_inputs,
    diagnostic_next_action_for_route, repair_review_state_public_command,
    select_authoritative_stale_reentry_target,
};
use crate::execution::phase;
use crate::execution::public_command_types::RecommendedPublicCommandArgv;
use crate::execution::public_repair_targets::public_repair_target_candidates_from_authority;
use crate::execution::public_route_selection::shared_next_action_seed_from_runtime_state;
#[cfg(test)]
pub(crate) use crate::execution::public_route_selection::{
    SharedNextActionRoutingInputs, shared_next_action_seed_from_decision,
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
    ExecutionReadScope, ExecutionRuntime, PlanExecutionStatus, PublicRepairTarget,
    StatusBlockingRecord, current_branch_closure_structural_review_state_reason,
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

pub(crate) type RouteDecision = PublicRouteDecision;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PublicRouteDecision {
    pub(crate) state_kind: String,
    pub(crate) phase: String,
    pub(crate) phase_detail: String,
    pub(crate) review_state_status: String,
    pub(crate) next_action: String,
    pub(crate) blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recommended_command: Option<String>,
    #[serde(skip)]
    pub(crate) recommended_public_command: Option<PublicCommand>,
    #[serde(skip)]
    pub(crate) invocation: Option<PublicCommandInvocation>,
    pub(crate) required_inputs: Vec<PublicCommandInputRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) required_follow_up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_public_action: Option<NextPublicAction>,
    pub(crate) blockers: Vec<Blocker>,
    #[serde(skip)]
    pub(crate) public_repair_targets: Vec<PublicRepairTarget>,
    #[serde(skip)]
    pub(crate) execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    #[serde(skip)]
    pub(crate) recording_context: Option<ExecutionRoutingRecordingContext>,
}

impl PublicRouteDecision {
    pub(crate) fn command_surfaces(
        command: Option<&PublicCommand>,
    ) -> (
        Option<String>,
        Option<PublicCommandInvocation>,
        Vec<PublicCommandInputRequirement>,
    ) {
        let (recommended_command, argv, required_inputs) =
            public_command_recommendation_surfaces(command);
        (
            recommended_command,
            argv.map(|argv| PublicCommandInvocation { argv }),
            required_inputs,
        )
    }

    pub(crate) fn public_command_argv(&self) -> RecommendedPublicCommandArgv {
        self.invocation
            .as_ref()
            .map(|invocation| invocation.argv.clone())
    }

    pub(crate) fn recommended_command_display(&self) -> Option<String> {
        self.recommended_command.clone()
    }

    pub(crate) fn bind_public_command(&mut self, command: Option<PublicCommand>) {
        let (recommended_command, invocation, required_inputs) =
            Self::command_surfaces(command.as_ref());
        self.recommended_command = recommended_command;
        self.recommended_public_command = command;
        self.invocation = invocation;
        self.required_inputs = required_inputs;
    }

    pub(crate) fn normalize_diagnostic_next_action(&mut self) {
        if let Some(next_action) = diagnostic_next_action_for_route(
            &self.state_kind,
            &self.phase_detail,
            self.invocation.is_some(),
            !self.required_inputs.is_empty(),
        ) {
            self.next_action = next_action;
            self.required_follow_up = None;
            self.next_public_action = None;
            self.blockers.clear();
            self.public_repair_targets.clear();
            self.recommended_command = None;
            self.recommended_public_command = None;
        }
    }

    fn is_diagnostic_only(&self) -> bool {
        diagnostic_next_action_for_route(
            &self.state_kind,
            &self.phase_detail,
            self.invocation.is_some(),
            !self.required_inputs.is_empty(),
        )
        .is_some()
    }
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
    let mut source_route_decision_hash = repair_follow_up_source_decision_hash(&route_decision);
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
        source_route_decision_hash = repair_follow_up_source_decision_hash(&route_decision);
    }
    runtime_state.route_repair_target_candidates = public_repair_target_candidates_from_authority(
        &runtime_state.context,
        &runtime_state.status,
        read_scope.authoritative_state.as_ref(),
        source_route_decision_hash.as_deref(),
    );
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
    let (phase, phase_detail, next_action, recommended_public_command) =
        match workflow_phase.as_str() {
            phase::PHASE_HANDOFF_REQUIRED => (
                String::from(phase::PHASE_HANDOFF_REQUIRED),
                String::from(phase::DETAIL_HANDOFF_RECORDING_REQUIRED),
                String::from("hand off"),
                Some(PublicCommand::TransferHandoff {
                    plan: route.plan_path.clone(),
                    scope: String::from("branch"),
                }),
            ),
            _ => (
                String::from(phase::PHASE_PIVOT_REQUIRED),
                String::from(phase::DETAIL_PLANNING_REENTRY_REQUIRED),
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
    let (recommended_command, _, _) =
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

pub(crate) fn shared_next_action_decision_from_runtime_state(
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
            has_authoritative_stale_target: runtime_state
                .gate_snapshot
                .has_authoritative_stale_binding(&runtime_state.status),
            authoritative_stale_target: select_authoritative_stale_reentry_target(
                &runtime_state.status,
                &runtime_state.gate_snapshot.stale_targets,
            ),
            ..NextActionAuthorityInputs::default()
        },
    )
}

fn route_decision_from_runtime_state_with_inputs(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> RouteDecision {
    let status = &runtime_state.status;
    let actionable_stale_reentry_target = select_authoritative_stale_reentry_target(
        status,
        &runtime_state.gate_snapshot.stale_targets,
    );
    let authoritative_stale_target_bound = runtime_state
        .gate_snapshot
        .has_authoritative_stale_binding(status);
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
    if status.review_state_status == "stale_unreviewed"
        && !authoritative_stale_target_bound
        && actionable_stale_reentry_target.is_none()
        && !task_closure_baseline_bridge_route_ready(runtime_state, status)
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
        && actionable_stale_reentry_target
            .is_none_or(|target| Some(target.task) == status.blocking_task)
        && !status
            .reason_codes
            .iter()
            .any(|code| code == "negative_result_requires_execution_reentry")
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
        if seed.phase_detail == phase::DETAIL_PLANNING_REENTRY_REQUIRED
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
        let mut recommended_public_command = match seed.phase_detail.as_str() {
            phase::DETAIL_FINAL_REVIEW_RECORDING_READY => Some(PublicCommand::AdvanceLateStage {
                plan: runtime_state.context.plan_rel.clone(),
                mode: PublicAdvanceLateStageMode::FinalReview,
            }),
            _ => seed.recommended_public_command.clone(),
        };
        let execution_command_context = seed.execution_command_context.clone();
        if recommended_public_command.is_none()
            && !phase::RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS
                .contains(&seed.phase_detail.as_str())
            && TargetlessStaleReconcile::from_phase_and_reason_codes(
                &seed.phase_detail,
                &seed.blocking_reason_codes,
            )
            .is_none()
            && let Some(follow_up_command) = status.blocking_records.first().and_then(|record| {
                public_command_for_required_follow_up(
                    record.required_follow_up.as_deref(),
                    &runtime_state.context.plan_rel,
                    &seed.phase_detail,
                    Some(record.record_type.as_str()),
                )
            })
        {
            recommended_public_command = Some(follow_up_command);
        }
        let (recommended_command, invocation, required_inputs) =
            PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
        let next_public_action = synthesize_next_public_action(
            recommended_public_command.as_ref(),
            &seed.phase_detail,
            &runtime_state.context.plan_rel,
        );
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
        if command_context_reopens_current_task_closure(status, execution_command_context.as_ref())
        {
            return repair_review_state_route_decision(
                runtime_state,
                status,
                seed.blocking_task.or_else(|| {
                    execution_command_context
                        .as_ref()
                        .and_then(|context| context.task_number)
                }),
                "prior_task_current_closure_stale",
            );
        }
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
            state_kind_command_marker(recommended_public_command.as_ref()),
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
            execution_command_context.as_ref(),
        );
        if seed.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            && status.current_task_closures.is_empty()
            && let Some(task_number) = status.blocking_task.or(seed.blocking_task).or_else(|| {
                execution_command_context
                    .as_ref()
                    .and_then(|context| context.task_number)
            })
            && (task_closure_baseline_bridge_route_ready(runtime_state, status)
                || (reducer_stale_target_allows_task_closure_bridge(runtime_state, task_number)
                    && close_current_task_public_repair_target_candidate_present(
                        runtime_state,
                        task_number,
                    ))
                || reducer_dispatch_bridge_ready(runtime_state, status, task_number))
        {
            return close_current_task_route_decision(runtime_state, status, task_number);
        }
        if seed.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            && !status
                .reason_codes
                .iter()
                .any(|code| code == "prior_task_current_closure_stale")
            && prior_task_closure_progress_edge_required(status)
            && let Some(task_number) = status.blocking_task
            && reducer_stale_target_allows_task_closure_bridge(runtime_state, task_number)
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
            recommended_public_command,
            invocation,
            required_inputs,
            required_follow_up,
            next_public_action,
            blockers,
            public_repair_targets: Vec::new(),
            execution_command_context,
            recording_context: seed.recording_context,
        };
    }
    route_decision_for_unroutable_runtime_state(status)
}

fn command_context_reopens_current_task_closure(
    status: &PlanExecutionStatus,
    context: Option<&ExecutionRoutingExecutionCommandContext>,
) -> bool {
    if status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "negative_result_requires_execution_reentry")
    {
        return false;
    }
    let Some(context) = context else {
        return false;
    };
    if context.command_kind != "reopen" {
        return false;
    }
    let Some(task_number) = context.task_number else {
        return false;
    };
    status
        .current_task_closures
        .iter()
        .any(|closure| closure.task == task_number)
}

fn public_route_blocking_reason_codes(
    status: &PlanExecutionStatus,
    seed: &WorkflowRoutingDecision,
) -> Vec<String> {
    if seed.blocking_task.is_some()
        && status.blocking_step.is_none()
        && matches!(
            seed.phase_detail.as_str(),
            phase::DETAIL_TASK_CLOSURE_RECORDING_READY
                | phase::DETAIL_TASK_REVIEW_RESULT_PENDING
                | phase::DETAIL_EXECUTION_REENTRY_REQUIRED
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
        && seed.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
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
    let Some(stale_task) = target.task else {
        return true;
    };
    if stale_task < task_number {
        return false;
    }
    stale_task > task_number || target.task_closure_bridge_allowed
}

fn close_current_task_public_repair_target_candidate_present(
    runtime_state: &RuntimeState,
    task_number: u32,
) -> bool {
    runtime_state
        .route_repair_target_candidates
        .iter()
        .any(|target| {
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

fn effective_route_review_state_status(
    status: &PlanExecutionStatus,
    seed: &WorkflowRoutingDecision,
) -> String {
    if status.review_state_status == "stale_unreviewed"
        || (!status.stale_unreviewed_closures.is_empty()
            && seed.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY)
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

pub(crate) fn close_current_task_route_decision(
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
    let phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
    let recommended_public_command = Some(PublicCommand::CloseCurrentTask {
        plan: runtime_state.context.plan_rel.clone(),
        task: Some(task_number),
        result_inputs_required: true,
    });
    let (recommended_command, invocation, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
    let state_kind = derive_state_kind_from_seed(
        None,
        HarnessPhase::Executing,
        &phase_detail,
        state_kind_command_marker(recommended_public_command.as_ref()),
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
        phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
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
        recommended_public_command,
        invocation,
        required_inputs,
        required_follow_up: None,
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
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
    let phase_detail = String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    let recommended_public_command = Some(repair_review_state_public_command(
        &runtime_state.context.plan_rel,
    ));
    let (recommended_command, invocation, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
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
        state_kind_command_marker(recommended_public_command.as_ref()),
    );
    let blockers = materialize_blocker_actions(
        primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
        &runtime_state.context.plan_rel,
    );
    let next_action = diagnostic_next_action_for_route(
        &state_kind,
        &phase_detail,
        invocation.is_some(),
        !required_inputs.is_empty(),
    )
    .unwrap_or_else(|| String::from("repair review state / reenter execution"));
    let mut decision = RouteDecision {
        state_kind,
        phase: String::from(phase::PHASE_EXECUTING),
        phase_detail,
        review_state_status,
        next_action,
        blocking_reason_codes,
        recommended_command,
        recommended_public_command,
        invocation,
        required_inputs,
        required_follow_up: Some(String::from("repair_review_state")),
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_command_context: None,
        recording_context: None,
    };
    decision.normalize_diagnostic_next_action();
    decision
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
    let recommended_public_command = (!targetless_stale_reconcile)
        .then(|| repair_review_state_public_command(&runtime_state.context.plan_rel));
    let (recommended_command, invocation, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
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
        state_kind_command_marker(recommended_public_command.as_ref()),
    );
    let blockers = if targetless_stale_reconcile {
        targetless_stale_reconcile_blockers(&phase_detail)
    } else {
        materialize_blocker_actions(
            primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
            &runtime_state.context.plan_rel,
        )
    };
    let next_action = diagnostic_next_action_for_route(
        &state_kind,
        &phase_detail,
        invocation.is_some(),
        !required_inputs.is_empty(),
    )
    .unwrap_or_else(|| String::from("repair review state / reenter execution"));
    let mut decision = RouteDecision {
        state_kind,
        phase: String::from(phase::PHASE_EXECUTING),
        phase_detail,
        review_state_status,
        next_action,
        blocking_reason_codes,
        recommended_command,
        recommended_public_command,
        invocation,
        required_inputs,
        required_follow_up: (!targetless_stale_reconcile)
            .then(|| String::from("repair_review_state")),
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_command_context: None,
        recording_context: None,
    };
    decision.normalize_diagnostic_next_action();
    decision
}

pub(crate) fn branch_closure_recording_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
) -> RouteDecision {
    let phase_detail =
        String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS);
    let recommended_public_command = Some(PublicCommand::AdvanceLateStage {
        plan: runtime_state.context.plan_rel.clone(),
        mode: PublicAdvanceLateStageMode::Basic,
    });
    let (recommended_command, invocation, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
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
        phase: String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING),
        phase_detail,
        review_state_status: String::from("missing_current_closure"),
        next_action: String::from("advance late stage"),
        blocking_reason_codes: vec![String::from("missing_current_closure")],
        recommended_command,
        recommended_public_command,
        invocation,
        required_inputs,
        required_follow_up: Some(String::from("advance_late_stage")),
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_command_context: None,
        recording_context: None,
    }
}

fn final_review_dispatch_route_for_repaired_late_stage_drift(
    runtime_state: &RuntimeState,
    seed: &WorkflowRoutingDecision,
) -> Option<RouteDecision> {
    let status = &runtime_state.status;
    if seed.phase_detail != phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
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
    let phase_detail = String::from(phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED);
    let next_public_action =
        synthesize_next_public_action(None, &phase_detail, &runtime_state.context.plan_rel);
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
        phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
        phase_detail,
        review_state_status: String::from("clean"),
        next_action: String::from("request final review"),
        blocking_reason_codes,
        recommended_command: None,
        recommended_public_command: None,
        invocation: None,
        required_inputs: Vec::new(),
        required_follow_up: Some(String::from("request_external_review")),
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_command_context: None,
        recording_context: None,
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

pub(crate) fn route_decision_from_routing(
    routing: &ExecutionRoutingState,
    blocking_records: &[StatusBlockingRecord],
) -> RouteDecision {
    let state_kind = derive_state_kind(routing);
    let recommended_public_command = routing.recommended_public_command.clone();
    let (recommended_command, invocation, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &routing.phase_detail,
        &routing.route.plan_path,
    );
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
    let route_next_action = diagnostic_next_action_for_route(
        &state_kind,
        &routing.phase_detail,
        invocation.is_some(),
        !required_inputs.is_empty(),
    )
    .unwrap_or_else(|| routing.next_action.clone());
    let diagnostic_without_local_action =
        route_next_action == NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED;
    let route_required_follow_up = (!diagnostic_without_local_action)
        .then(|| {
            derive_required_follow_up_from_optional_status(
                routing.execution_status.as_ref(),
                &routing.phase_detail,
                &routing.review_state_status,
                routing.blocking_reason_codes.iter().map(String::as_str),
                routing.execution_command_context.as_ref(),
            )
        })
        .flatten();
    let mut decision = RouteDecision {
        state_kind,
        phase: canonical_phase_for_shared_decision(&routing.phase, &routing.phase_detail),
        phase_detail: routing.phase_detail.clone(),
        review_state_status: routing.review_state_status.clone(),
        next_action: route_next_action,
        blocking_reason_codes: routing.blocking_reason_codes.clone(),
        recommended_command,
        recommended_public_command,
        invocation,
        required_inputs,
        required_follow_up: route_required_follow_up,
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_command_context: routing.execution_command_context.clone(),
        recording_context: routing.recording_context.clone(),
    };
    decision.normalize_diagnostic_next_action();
    decision
}

fn route_decision_for_unroutable_runtime_state(status: &PlanExecutionStatus) -> RouteDecision {
    let recommended_command = None;
    let next_public_action = None;
    let blockers = primary_blocker_for_status(
        status,
        phase::DETAIL_BLOCKED_RUNTIME_BUG,
        next_public_action.as_ref(),
    );
    RouteDecision {
        state_kind: String::from(phase::DETAIL_BLOCKED_RUNTIME_BUG),
        phase: canonical_phase_for_shared_decision(
            &default_phase_for_status(status),
            "runtime_route_unavailable",
        ),
        phase_detail: status.phase_detail.clone(),
        review_state_status: status.review_state_status.clone(),
        next_action: String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED),
        blocking_reason_codes: compact_operator_reason_codes(
            Some(status),
            &status.phase_detail,
            &status.review_state_status,
        ),
        recommended_command,
        recommended_public_command: None,
        invocation: None,
        required_inputs: Vec::new(),
        required_follow_up: None,
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_command_context: None,
        recording_context: None,
    }
}

pub(crate) fn route_decision_with_status_blockers(
    mut route_decision: RouteDecision,
    status: &PlanExecutionStatus,
    route_repair_target_candidates: &[PublicRepairTarget],
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
    route_decision.public_repair_targets = public_repair_targets_from_route_decision(
        status,
        &route_decision,
        route_repair_target_candidates,
    );
    route_decision.normalize_diagnostic_next_action();
    route_decision
}

fn public_repair_targets_from_route_decision(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
    route_repair_target_candidates: &[PublicRepairTarget],
) -> Vec<PublicRepairTarget> {
    if route_decision.is_diagnostic_only() {
        return Vec::new();
    }

    let mut targets = Vec::new();
    if status.external_wait_state.is_some() {
        return targets;
    }
    if let Some(route_request) = route_decision
        .recommended_public_command
        .as_ref()
        .and_then(PublicCommand::to_mutation_request)
        && route_request.kind == PublicMutationKind::Reopen
    {
        push_public_repair_target_once(
            &mut targets,
            PublicRepairTarget {
                command_kind: String::from("reopen"),
                task: route_request.task,
                step: route_request.step,
                reason_code: String::from("route_execution_reentry_required"),
                source_record_id: Some(String::from("route_decision:reopen")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    if route_decision.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && let Some(task) = route_decision
            .recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        push_public_repair_target_once(
            &mut targets,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("route_task_closure_recording_ready"),
                source_record_id: Some(String::from("route_decision:task_closure_recording_ready")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    for candidate in route_repair_target_candidates {
        if route_allows_public_repair_target_candidate(status, route_decision, candidate) {
            push_public_repair_target_once(&mut targets, candidate.clone());
        }
    }

    let route_exposes_task_closure_repair = route_decision.phase_detail
        == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && route_decision
            .blocking_reason_codes
            .iter()
            .any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "prior_task_current_closure_missing"
                        | "prior_task_review_dispatch_stale"
                        | "task_closure_baseline_repair_candidate"
                )
            });
    let repair_review_state_target_allowed = route_decision.phase_detail
        == phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        || route_decision.state_kind != phase::DETAIL_BLOCKED_RUNTIME_BUG;
    if (route_exposes_task_closure_repair
        || route_decision_exposes_repair_review_state_target(status, route_decision))
        && repair_review_state_target_allowed
    {
        let reason_code = if route_exposes_task_closure_repair {
            "route_task_closure_repair_state_refresh"
        } else {
            "route_repair_review_state_available"
        };
        push_public_repair_target_once(
            &mut targets,
            PublicRepairTarget {
                command_kind: String::from("repair-review-state"),
                task: None,
                step: None,
                reason_code: String::from(reason_code),
                source_record_id: Some(format!("route_decision:{}", route_decision.phase_detail)),
                expires_when_fingerprint_changes: true,
            },
        );
    }

    if route_recommended_public_command_is(route_decision, PublicCommandKind::AdvanceLateStage)
        && route_decision.phase_detail != phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        && route_decision.state_kind != phase::DETAIL_BLOCKED_RUNTIME_BUG
    {
        push_public_repair_target_once(
            &mut targets,
            PublicRepairTarget {
                command_kind: String::from("advance-late-stage"),
                task: None,
                step: None,
                reason_code: String::from("route_advance_late_stage_ready"),
                source_record_id: Some(String::from("route_decision:advance_late_stage")),
                expires_when_fingerprint_changes: true,
            },
        );
    }

    targets
}

fn route_allows_public_repair_target_candidate(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
    candidate: &PublicRepairTarget,
) -> bool {
    match candidate.command_kind.as_str() {
        "reopen" => {
            candidate.reason_code == "explicit_reopen_repair_target"
                || candidate.reason_code == "persisted_execution_reentry_follow_up"
                || (route_decision.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                    && candidate.task.is_some()
                    && route_reentry_target_matches_candidate(route_decision, candidate))
        }
        "close-current-task" => {
            candidate.task.is_some()
                && (explicit_close_current_task_candidate(candidate)
                    || (route_decision.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
                        && route_recording_target_matches_candidate(route_decision, candidate)))
        }
        "repair-review-state" => {
            route_decision_exposes_repair_review_state_target(status, route_decision)
                && route_decision.state_kind != phase::DETAIL_BLOCKED_RUNTIME_BUG
        }
        "advance-late-stage" => {
            route_recommended_public_command_is(route_decision, PublicCommandKind::AdvanceLateStage)
                && route_decision.phase_detail != phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
                && route_decision.state_kind != phase::DETAIL_BLOCKED_RUNTIME_BUG
        }
        _ => false,
    }
}

fn route_reentry_target_matches_candidate(
    route_decision: &RouteDecision,
    candidate: &PublicRepairTarget,
) -> bool {
    if let Some(route_request) = route_decision
        .recommended_public_command
        .as_ref()
        .and_then(PublicCommand::to_mutation_request)
        && route_request.kind == PublicMutationKind::Reopen
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
    matches!(
        candidate.reason_code.as_str(),
        "persisted_task_closure_follow_up"
            | "authoritative_task_closure_postcondition_cleanup"
            | "task_review_dispatch_closure_ready"
            | "authoritative_preflight_recovery_task_closure"
            | "status_task_closure_recording_ready"
    )
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

fn route_recommended_public_command_is(
    route_decision: &RouteDecision,
    kind: PublicCommandKind,
) -> bool {
    route_decision
        .recommended_public_command
        .as_ref()
        .is_some_and(|command| command.kind() == kind)
}

fn route_decision_exposes_repair_review_state_target(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
) -> bool {
    route_recommended_public_command_is(route_decision, PublicCommandKind::RepairReviewState)
        || route_decision.required_follow_up.as_deref() == Some("repair_review_state")
        || route_decision.review_state_status != "clean"
        || task_scope_structural_review_state_reason(status).is_some()
        || current_branch_closure_structural_review_state_reason(status).is_some()
        || status.blocking_records.iter().any(|record| {
            record.record_type == "review_state"
                && record.required_follow_up.as_deref() == Some("repair_review_state")
        })
        || matches!(
            route_decision.phase_detail.as_str(),
            phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                | phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
                | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
                | phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        )
        || (route_decision.phase_detail == phase::DETAIL_FINISH_COMPLETION_GATE_READY
            && route_decision.state_kind == "terminal")
        || route_decision
            .blocking_reason_codes
            .iter()
            .any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "prior_task_current_closure_missing"
                        | "prior_task_review_dispatch_stale"
                        | "stale_provenance"
                        | "task_closure_baseline_repair_candidate"
                )
            })
}

fn project_routing_from_runtime_state(
    route: WorkflowRoute,
    runtime_state: &RuntimeState,
    route_decision: &RouteDecision,
    _external_review_result_ready: bool,
) -> ExecutionRoutingState {
    let mut route_decision = route_decision.clone();
    let status = runtime_state.status.clone();
    let (reason_family, diagnostic_reason_codes) = late_stage_observability_for_phase(
        &route_decision.phase,
        runtime_state.gate_review.as_ref(),
        runtime_state.gate_finish.as_ref(),
    );
    let diagnostic_reason_codes =
        merge_task_boundary_projection_diagnostics(diagnostic_reason_codes, &status);
    let mut blocking_scope = status.blocking_scope.clone();
    let mut blocking_task = status.blocking_task;
    let recording_context = match route_decision.phase_detail.as_str() {
        phase::DETAIL_FINAL_REVIEW_RECORDING_READY => runtime_state
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
        phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED => runtime_state
            .authoritative_current_branch_closure_id
            .as_ref()
            .map(|branch_closure_id| ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: None,
                branch_closure_id: Some(branch_closure_id.clone()),
            }),
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
    route_decision.recording_context = recording_context.clone();
    let execution_command_context = route_decision.execution_command_context.clone();
    if route_decision.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && let Some(task_number) = execution_command_context
            .as_ref()
            .and_then(|context| context.task_number)
            .or_else(|| blocking_task_from_blockers(&route_decision.blockers))
            .or_else(|| blocking_task_from_status_records(&status))
    {
        blocking_scope = Some(String::from("task"));
        blocking_task = Some(task_number);
    } else if route_decision.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && let Some(task_number) = recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        blocking_scope = Some(String::from("task"));
        blocking_task = Some(task_number);
    } else if route_decision.phase_detail
        == phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
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
        next_public_action: next_public_action.map(|action| action.command.clone()),
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
        return vec![blocker_from_status_record(primary, next_public_action)];
    }

    if state_kind == phase::DETAIL_BLOCKED_RUNTIME_BUG {
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
    recommended_public_command: Option<&PublicCommand>,
    phase_detail: &str,
    plan_path: &str,
) -> Option<NextPublicAction> {
    if let Some(command) = recommended_public_command
        .filter(|_| !phase::RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail))
        .filter(|command| command.kind() != PublicCommandKind::WorkflowOperator)
        .filter(|command| command.to_invocation().is_some())
        .map(PublicCommand::to_display_command)
    {
        return Some(NextPublicAction {
            command: command.clone(),
            args_template: Some(command),
        });
    }
    let command = match phase_detail {
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED | phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED => {
            PublicCommand::WorkflowOperator {
                plan: plan_path.to_owned(),
                external_review_result_ready: false,
            }
            .to_display_command()
        }
        _ => return None,
    };
    Some(NextPublicAction {
        command: command.clone(),
        args_template: Some(command),
    })
}

fn derive_state_kind(routing: &ExecutionRoutingState) -> String {
    let recommended_command =
        state_kind_command_marker(routing.recommended_public_command.as_ref());
    classify_state_kind(
        routing.external_wait_state.as_deref(),
        routing.phase == phase::PHASE_READY_FOR_BRANCH_COMPLETION,
        &routing.phase_detail,
        recommended_command,
    )
}

fn state_kind_command_marker(command: Option<&PublicCommand>) -> Option<&'static str> {
    command.map(|_| "public_command")
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
        && phase_detail == phase::DETAIL_FINISH_COMPLETION_GATE_READY
        && recommended_command.is_none()
    {
        return String::from("terminal");
    }
    if phase_detail == phase::DETAIL_PLANNING_REENTRY_REQUIRED && recommended_command.is_none() {
        return String::from("waiting_external_input");
    }
    if phase_detail == phase::DETAIL_BLOCKED_RUNTIME_BUG && recommended_command.is_none() {
        return String::from(phase::DETAIL_BLOCKED_RUNTIME_BUG);
    }
    if phase_detail == phase::DETAIL_RUNTIME_RECONCILE_REQUIRED && recommended_command.is_none() {
        return String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
    }
    if recommended_command.is_none()
        && !phase::RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail)
    {
        return String::from(phase::DETAIL_BLOCKED_RUNTIME_BUG);
    }
    String::from("actionable_public_command")
}

#[cfg(test)]
fn public_command_for_phase_detail(phase_detail: &str, plan_path: &str) -> Option<PublicCommand> {
    match phase_detail {
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED | phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED => {
            Some(PublicCommand::WorkflowOperator {
                plan: plan_path.to_owned(),
                external_review_result_ready: false,
            })
        }
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED | phase::DETAIL_PLANNING_REENTRY_REQUIRED => {
            Some(repair_review_state_public_command(plan_path))
        }
        _ => public_advance_late_stage_mode_for_phase_detail(phase_detail).map(|mode| {
            PublicCommand::AdvanceLateStage {
                plan: plan_path.to_owned(),
                mode,
            }
        }),
    }
}

fn public_command_for_required_follow_up(
    required_follow_up: Option<&str>,
    plan_path: &str,
    phase_detail: &str,
    record_type: Option<&str>,
) -> Option<PublicCommand> {
    match normalize_public_routing_follow_up_token(required_follow_up)? {
        "repair_review_state" => Some(repair_review_state_public_command(plan_path)),
        "resolve_release_blocker" => Some(PublicCommand::AdvanceLateStage {
            plan: plan_path.to_owned(),
            mode: PublicAdvanceLateStageMode::ReleaseReadiness,
        }),
        "advance_late_stage" => {
            let mode = public_advance_late_stage_mode_for_phase_detail(phase_detail).or({
                match record_type {
                    Some("branch_closure") => Some(PublicAdvanceLateStageMode::Basic),
                    Some("release_readiness") => Some(PublicAdvanceLateStageMode::ReleaseReadiness),
                    _ => None,
                }
            })?;
            Some(PublicCommand::AdvanceLateStage {
                plan: plan_path.to_owned(),
                mode,
            })
        }
        "record_handoff" => Some(PublicCommand::TransferHandoff {
            plan: plan_path.to_owned(),
            scope: String::from("task|branch"),
        }),
        "execution_reentry"
        | "request_external_review"
        | "wait_for_external_review_result"
        | "run_verification" => Some(PublicCommand::WorkflowOperator {
            plan: plan_path.to_owned(),
            external_review_result_ready: false,
        }),
        "close_current_task" | "gate_review" | "gate_finish" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_state_kind, primary_blocker_for_route, public_command_for_phase_detail,
        route_decision_from_routing, synthesize_next_public_action,
    };
    use crate::execution::command_eligibility::{
        command_invokes_hidden_lane, hidden_command_tokens,
    };
    use crate::execution::follow_up::follow_up_command_template as follow_up_to_command_template;
    use crate::execution::harness::{
        AggregateEvaluationState, ChunkId, DownstreamFreshnessState, HarnessPhase,
    };
    use crate::execution::phase;
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
        for follow_up in follow_ups {
            let template = follow_up_to_command_template(Some(follow_up))
                .expect("known follow-up should map to a command template");
            for hidden in hidden_command_tokens() {
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
            synthesize_next_public_action(
                None,
                "task_review_dispatch_required",
                "docs/featureforge/plans/plan.md"
            )
            .is_none(),
            "task-review dispatch is no longer a public route"
        );
        let routing = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
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
            runtime_provenance: None,
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
            phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
            phase_detail: String::from("task_review_dispatch_required"),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("runtime diagnostic required"),
            recommended_public_command: None,
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
                status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
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
            runtime_provenance: None,
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
            phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
            phase_detail: String::from(phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("wait for external review result"),
            recommended_public_command: None,
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
            phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
        );
        assert!(decision.blockers[0].next_public_action.is_none());
    }

    #[test]
    fn diagnostic_phase_details_without_commands_preserve_diagnostic_state_kind() {
        assert_eq!(
            classify_state_kind(None, false, phase::DETAIL_RUNTIME_RECONCILE_REQUIRED, None),
            phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        );
        assert_eq!(
            classify_state_kind(None, false, phase::DETAIL_BLOCKED_RUNTIME_BUG, None),
            phase::DETAIL_BLOCKED_RUNTIME_BUG
        );
        assert_eq!(
            classify_state_kind(None, false, phase::DETAIL_EXECUTION_REENTRY_REQUIRED, None),
            phase::DETAIL_BLOCKED_RUNTIME_BUG
        );
    }

    #[test]
    fn blocked_runtime_bug_suppresses_public_action_surfaces() {
        let routing = ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
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
            runtime_provenance: None,
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from(phase::PHASE_EXECUTING),
            phase: String::from(phase::PHASE_EXECUTING),
            phase_detail: String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from("execution reentry required"),
            recommended_public_command: None,
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
        assert_eq!(decision.state_kind, phase::DETAIL_BLOCKED_RUNTIME_BUG);
        assert!(decision.next_public_action.is_none());
        assert!(decision.recommended_command.is_none());
        assert!(decision.required_follow_up.is_none());
        assert_eq!(decision.next_action, "runtime diagnostic required");
        assert!(decision.blockers.is_empty());
        assert!(decision.public_repair_targets.is_empty());
    }

    #[test]
    fn hidden_string_recommendations_are_not_route_authority() {
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
            phase: Some(String::from(phase::PHASE_EXECUTING)),
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
            runtime_provenance: None,
            semantic_workspace_tree_id: String::from("semantic_tree:authoritative"),
            raw_workspace_tree_id: Some(String::from("git_tree:debug")),
            next_action: String::from("runtime diagnostic required"),
            recommended_public_command: None,
            recommended_public_command_argv: None,
            required_inputs: Vec::new(),
            recommended_command: Some(format!(
                "featureforge plan execution {} --plan docs/featureforge/plans/example.md --scope task --task 1",
                ["record", "review", "dispatch"].join("-")
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

        assert!(
            synthesize_next_public_action(
                status.recommended_public_command.as_ref(),
                &status.phase_detail,
                "docs/featureforge/plans/plan.md"
            )
            .is_none(),
            "task-review dispatch is diagnostic-only and must not synthesize a public operator loop"
        );
    }

    #[test]
    fn next_public_action_binds_concrete_plan_path_for_operator_fallbacks() {
        let plan_path = "docs/featureforge/plans/plan.md";
        let action = synthesize_next_public_action(
            None,
            phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
            plan_path,
        )
        .expect("final review dispatch should route through workflow/operator");

        assert!(action.command.contains(plan_path));
        assert!(!action.command.contains("<approved-plan-path>"));
        assert!(
            action
                .args_template
                .as_deref()
                .is_some_and(|template| template.contains(plan_path)
                    && !template.contains("<approved-plan-path>")),
            "args_template should also bind the concrete plan path: {action:?}"
        );
    }

    #[test]
    fn removed_plan_execution_commands_are_not_public_route_commands() {
        for removed in ["preflight", "recommend"] {
            let command =
                format!("featureforge plan execution {removed} --plan docs/plan.md --json");
            assert!(command_invokes_hidden_lane(&command));
            assert_eq!(
                public_command_for_phase_detail(
                    phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
                    "<approved-plan-path>",
                )
                .map(|command| command.to_display_command())
                .as_deref(),
                Some("featureforge plan execution repair-review-state --plan <approved-plan-path>"),
                "removed `{removed}` command cannot be parsed into a route command; the typed fallback is explicit"
            );
        }
    }
}
