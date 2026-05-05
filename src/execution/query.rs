// Execution-owned review-state query layer.
// workflow consumes this module as a read-only client rather than reconstructing
// authoritative review-state truth from storage internals.

use std::path::PathBuf;

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::closure_diagnostics::public_task_boundary_decision;
use crate::execution::closure_graph::{AuthoritativeClosureGraph, ClosureGraphSignals};
use crate::execution::command_eligibility::PublicCommand;
use crate::execution::current_closure_projection::project_current_task_closures;
#[cfg(test)]
use crate::execution::current_truth::late_stage_stale_unreviewed as shared_late_stage_stale_unreviewed;
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason, ReviewStateRepairReroute,
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    branch_closure_rerecording_assessment,
    current_task_negative_result_task as shared_current_task_negative_result_task,
    late_stage_qa_blocked as shared_late_stage_qa_blocked,
    late_stage_release_blocked as shared_late_stage_release_blocked,
    late_stage_release_truth_blocked as shared_late_stage_release_truth_blocked,
    late_stage_review_blocked as shared_late_stage_review_blocked,
    late_stage_review_truth_blocked as shared_late_stage_review_truth_blocked,
    normalized_plan_qa_requirement as shared_normalized_plan_qa_requirement,
    task_review_result_requires_verification_reason_codes,
    task_scope_overlay_restore_required as shared_task_scope_overlay_restore_required,
    task_scope_stale_review_state_reason_present as shared_task_scope_stale_review_state_reason_present,
};
#[cfg(test)]
use crate::execution::current_truth::{
    FollowUpOverrideInputs,
    public_late_stage_stale_unreviewed as shared_public_late_stage_stale_unreviewed,
    resolve_follow_up_override as resolve_shared_follow_up_override,
};
use crate::execution::follow_up::{
    normalize_persisted_repair_follow_up_token, normalize_public_routing_follow_up_token,
};
use crate::execution::harness::HarnessPhase;
#[cfg(test)]
use crate::execution::next_action::{
    NextActionDecision, NextActionKind, compute_next_action_decision, public_next_action_text,
};
use crate::execution::phase;
use crate::execution::reducer::{RuntimeState, reduce_execution_read_scope};
use crate::execution::router::{
    RouteDecision, project_non_runtime_workflow_routing_state, project_runtime_routing_state,
    required_follow_up_from_route_decision, route_decision_from_routing,
};
use crate::execution::runtime::state_dir as default_state_dir;
use crate::execution::runtime_provenance::{RuntimeProvenance, runtime_provenance_for_paths};
use crate::execution::stale_target_projection::{
    ReviewStateStaleClosureProjection, ReviewStateStaleClosureProjectionInputs,
    project_review_state_stale_unreviewed_closures,
};
use crate::execution::state::{
    ExecutionContext, ExecutionReadScope, ExecutionRuntime, GateResult, PlanExecutionStatus,
    apply_public_read_invariants_to_status,
    apply_shared_routing_projection_to_read_scope_with_routing,
    current_branch_closure_structural_review_state_reason, load_execution_read_scope,
    missing_derived_review_state_fields, qa_pending_requires_test_plan_refresh,
    shared_repair_review_state_reroute_decision, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason,
    usable_current_branch_closure_identity_from_authoritative_state,
};
#[cfg(test)]
use crate::execution::state::{load_execution_context, load_execution_context_for_exact_plan};
use crate::execution::status::PublicReviewStateTaskClosure;
use crate::git::discover_slug_identity_and_head;
use crate::workflow::late_stage_precedence::{
    GateState, LateStageSignals, resolve as resolve_late_stage_precedence,
};
#[cfg(test)]
use crate::workflow::pivot::pivot_decision_reason_codes;
use crate::workflow::status::{
    WorkflowRoute, WorkflowRuntime, explicit_plan_override_route as resolve_explicit_plan_override,
};

pub type ReviewStateTaskClosure = PublicReviewStateTaskClosure;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewStateBranchClosure {
    pub branch_closure_id: String,
    pub reviewed_state_id: Option<String>,
    pub contract_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewStateSnapshot {
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    pub superseded_closures: Vec<String>,
    pub stale_unreviewed_closures: Vec<String>,
    pub missing_derived_overlays: Vec<String>,
    pub branch_drift_confined_to_late_stage_surface: bool,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Default)]
pub struct WorkflowExecutionState {
    pub execution_context: Option<ExecutionContext>,
    pub execution_status: Option<PlanExecutionStatus>,
    pub preflight: Option<GateResult>,
    pub gate_review: Option<GateResult>,
    pub gate_finish: Option<GateResult>,
    pub review_state_snapshot: Option<ReviewStateSnapshot>,
    pub task_scope_overlay_restore_required: bool,
    pub task_negative_result_task: Option<u32>,
    pub task_negative_result_verification_failed: bool,
    pub task_review_dispatch_id: Option<String>,
    pub final_review_dispatch_id: Option<String>,
    pub final_review_dispatch_lineage_present: bool,
    pub current_branch_closure_id: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub current_release_readiness_result: Option<String>,
    pub current_final_review_branch_closure_id: Option<String>,
    pub current_final_review_result: Option<String>,
    pub current_qa_branch_closure_id: Option<String>,
    pub current_qa_result: Option<String>,
    pub base_branch: Option<String>,
    pub qa_requirement: Option<String>,
    pub qa_pending_test_plan_refresh_required: bool,
    pub persisted_repair_review_state_follow_up: Option<String>,
    pub repair_review_state_follow_up: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionRoutingRecordingContext {
    pub task_number: Option<u32>,
    pub dispatch_id: Option<String>,
    pub branch_closure_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionRoutingExecutionCommandContext {
    pub command_kind: String,
    pub task_number: Option<u32>,
    pub step_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionRoutingState {
    pub route: WorkflowRoute,
    #[serde(skip)]
    pub(crate) route_decision: Option<RouteDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_provenance: Option<RuntimeProvenance>,
    pub execution_status: Option<PlanExecutionStatus>,
    pub preflight: Option<GateResult>,
    pub gate_review: Option<GateResult>,
    pub gate_finish: Option<GateResult>,
    pub workflow_phase: String,
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub qa_requirement: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub recording_context: Option<ExecutionRoutingRecordingContext>,
    pub execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    pub next_action: String,
    #[serde(skip)]
    pub recommended_public_command: Option<PublicCommand>,
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_wait_state: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocking_reason_codes: Vec<String>,
    pub reason_family: String,
    pub diagnostic_reason_codes: Vec<String>,
    pub task_review_dispatch_id: Option<String>,
    pub final_review_dispatch_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub current_release_readiness_result: Option<String>,
    pub base_branch: Option<String>,
}

impl ExecutionRoutingState {
    pub(crate) fn bind_public_command(
        &mut self,
        command: PublicCommand,
        execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    ) {
        let recommended_command = command
            .to_invocation()
            .map(|_| command.to_display_command());
        if let Some(route_decision) = self.route_decision.as_mut() {
            route_decision.bind_public_command(Some(command.clone()));
            route_decision.execution_command_context = execution_command_context.clone();
        }
        self.recommended_command = recommended_command;
        self.recommended_public_command = Some(command);
        self.execution_command_context = execution_command_context;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowRoutingDecision {
    pub(crate) phase: String,
    pub(crate) phase_detail: String,
    pub(crate) review_state_status: String,
    pub(crate) recording_context: Option<ExecutionRoutingRecordingContext>,
    pub(crate) execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    pub(crate) next_action: String,
    pub(crate) recommended_public_command: Option<PublicCommand>,
    pub(crate) recommended_command: Option<String>,
    pub(crate) blocking_scope: Option<String>,
    pub(crate) blocking_task: Option<u32>,
    pub(crate) external_wait_state: Option<String>,
    pub(crate) blocking_reason_codes: Vec<String>,
}

pub(crate) fn required_follow_up_from_routing(routing: &ExecutionRoutingState) -> Option<String> {
    if let Some(route_decision) = routing.route_decision.as_ref() {
        return required_follow_up_from_route_decision(route_decision);
    }
    route_decision_from_routing(routing, &[]).required_follow_up
}

pub(crate) fn normalize_public_follow_up_alias(required_follow_up: Option<&str>) -> Option<&str> {
    normalize_public_routing_follow_up_token(required_follow_up)
}

pub(crate) fn normalize_persisted_follow_up_alias(
    required_follow_up: Option<&str>,
) -> Option<&str> {
    normalize_persisted_repair_follow_up_token(required_follow_up)
}

pub(crate) fn task_review_result_requires_verification<'a>(
    reason_codes: impl IntoIterator<Item = &'a str>,
) -> bool {
    task_review_result_requires_verification_reason_codes(reason_codes)
}

pub fn query_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    query_review_state_internal(runtime, &args.plan, true)
}

fn query_review_state_internal(
    runtime: &ExecutionRuntime,
    plan_path: &std::path::Path,
    exact_plan_override: bool,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    let mut read_scope = load_execution_read_scope(runtime, plan_path, exact_plan_override)?;
    apply_shared_routing_projection_to_read_scope_with_routing(&mut read_scope, false, false)?;
    apply_public_read_invariants_to_status(&mut read_scope.status);
    review_state_snapshot_from_read_scope_with_status(&read_scope, &read_scope.status)
}

pub(crate) fn review_state_snapshot_from_read_scope_with_status(
    read_scope: &ExecutionReadScope,
    status: &PlanExecutionStatus,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    let runtime_state = read_scope.runtime_state.as_ref().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "review-state query requires reducer output before stale closure projection.",
        )
    })?;
    review_state_snapshot_from_read_scope_with_runtime_state(read_scope, status, runtime_state)
}

fn review_state_snapshot_from_read_scope_with_runtime_state(
    read_scope: &ExecutionReadScope,
    status: &PlanExecutionStatus,
    runtime_state: &RuntimeState,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    let context = &read_scope.context;
    let overlay = read_scope.overlay.as_ref();
    let authoritative_state = read_scope.authoritative_state.as_ref();
    let gate_snapshot = &runtime_state.gate_snapshot;
    let late_stage_stale_unreviewed = gate_snapshot.late_stage_stale_unreviewed;
    let late_stage_missing_current_closure_public_truth =
        gate_snapshot.missing_current_closure_stale_provenance;
    let task_scope_stale_unreviewed = task_scope_review_state_is_stale_unreviewed(status);
    let task_scope_structural_reason = task_scope_structural_review_state_reason(status);
    let branch_scope_structural_reason =
        current_branch_closure_structural_review_state_reason(status);
    let closure_graph = AuthoritativeClosureGraph::from_state(
        authoritative_state,
        &ClosureGraphSignals::from_authoritative_state(
            authoritative_state,
            overlay.and_then(|overlay| overlay.current_branch_closure_id.as_deref()),
            late_stage_stale_unreviewed,
            late_stage_missing_current_closure_public_truth,
            gate_snapshot.stale_reason_codes.clone(),
        ),
    );
    let current_task_closures = project_current_task_closures(context, authoritative_state)?
        .into_iter()
        .filter(|closure| {
            closure_graph
                .current_task_closure(closure.task)
                .is_none_or(|evaluation| evaluation.identity.record_id == closure.closure_record_id)
        })
        .collect::<Vec<_>>();
    let current_branch_closure = closure_graph.current_branch_closure().map(|evaluation| {
        let branch_closure_id = evaluation.identity.record_id.clone();
        let contract_identity = usable_current_branch_closure_identity_from_authoritative_state(
            context,
            authoritative_state,
        )
        .filter(|identity| identity.branch_closure_id == branch_closure_id)
        .map(|identity| identity.contract_identity);
        let status_binding_matches_graph =
            status.current_branch_closure_id.as_deref() == Some(branch_closure_id.as_str());
        ReviewStateBranchClosure {
            branch_closure_id,
            reviewed_state_id: if status_binding_matches_graph {
                status.current_branch_reviewed_state_id.clone()
            } else {
                None
            }
            .or_else(|| evaluation.identity.tracked_tree_fingerprint.clone()),
            contract_identity,
        }
    });
    let mut superseded_closures = closure_graph
        .superseded_record_ids()
        .into_iter()
        .filter(|record_id| closure_graph.evaluation(record_id).is_some())
        .collect::<Vec<_>>();
    superseded_closures.sort_by(|left, right| {
        closure_graph
            .superseded_by(left)
            .cmp(&closure_graph.superseded_by(right))
            .then(left.cmp(right))
    });
    let ReviewStateStaleClosureProjection {
        late_stage_stale_projection_active,
        stale_unreviewed_closures,
    } = project_review_state_stale_unreviewed_closures(ReviewStateStaleClosureProjectionInputs {
        status,
        gate_snapshot,
        task_scope_stale_unreviewed,
        task_scope_structural_reason_present: task_scope_structural_reason.is_some(),
        branch_scope_structural_reason_present: branch_scope_structural_reason.is_some(),
    });
    let missing_derived_overlays =
        missing_derived_review_state_fields(authoritative_state, overlay);
    let missing_derived_overlays_present = !missing_derived_overlays.is_empty();
    let branch_rerecording_assessment = late_stage_stale_projection_active
        .then(|| branch_closure_rerecording_assessment(context).ok())
        .flatten();
    Ok(ReviewStateSnapshot {
        branch_drift_confined_to_late_stage_surface: late_stage_stale_projection_active
            && branch_rerecording_assessment
                .as_ref()
                .is_some_and(|assessment| assessment.drift_confined_to_late_stage_surface),
        current_task_closures,
        current_branch_closure,
        superseded_closures,
        stale_unreviewed_closures,
        missing_derived_overlays,
        trace_summary: if late_stage_stale_projection_active {
            match branch_rerecording_assessment
                .as_ref()
                .and_then(|assessment| assessment.unsupported_reason)
            {
                Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => {
                    String::from(
                        "Review state is stale_unreviewed relative to the current workspace, and the approved plan does not declare Late-Stage Surface metadata to classify branch drift as trusted late-stage-only.",
                    )
                }
                Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) => {
                    String::from(
                        "Review state is stale_unreviewed relative to the current workspace, and tracked drift escapes the approved Late-Stage Surface.",
                    )
                }
                Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => {
                    String::from(
                        "Review state is stale_unreviewed relative to the current workspace, and no still-current task-closure baseline remains for authoritative branch reroute.",
                    )
                }
                None => String::from(
                    "Review state is stale_unreviewed relative to the current workspace.",
                ),
            }
        } else if task_scope_stale_unreviewed {
            String::from(
                "Review state is stale_unreviewed relative to the current task-closure set.",
            )
        } else if task_scope_structural_reason.is_some() {
            String::from(
                "Current task-closure review-state provenance is structurally invalid and requires execution reentry.",
            )
        } else if branch_scope_structural_reason.is_some() {
            String::from(
                "Current branch-closure reviewed-state provenance is structurally invalid and requires branch-closure repair.",
            )
        } else if status.review_state_status == "missing_current_closure" {
            String::from(
                "Review state is missing_current_closure because the active workflow phase still requires a current reviewed closure.",
            )
        } else if missing_derived_overlays_present {
            String::from(
                "Review state is blocked because derivable overlay fields are missing from authoritative state and must be reconciled.",
            )
        } else {
            String::from("Review state is already current for the present workspace.")
        },
    })
}

pub fn query_workflow_execution_state(
    runtime: &ExecutionRuntime,
    plan_path: &str,
) -> Result<WorkflowExecutionState, JsonFailure> {
    query_workflow_execution_state_internal(runtime, plan_path, false, None, true)
}

fn query_workflow_execution_state_internal(
    runtime: &ExecutionRuntime,
    plan_path: &str,
    exact_plan_override: bool,
    preloaded_read_scope: Option<&ExecutionReadScope>,
    _require_exact_execution_command: bool,
) -> Result<WorkflowExecutionState, JsonFailure> {
    if plan_path.is_empty() {
        return Ok(WorkflowExecutionState::default());
    }
    let plan_path_buf = PathBuf::from(plan_path);
    let owned_read_scope;
    let read_scope = if let Some(read_scope) = preloaded_read_scope {
        read_scope
    } else {
        owned_read_scope = load_execution_read_scope(runtime, &plan_path_buf, exact_plan_override)?;
        &owned_read_scope
    };
    let runtime_state = reduce_execution_read_scope(read_scope)?;
    workflow_execution_state_from_runtime_state(read_scope, &runtime_state)
}

pub(crate) fn workflow_execution_state_from_runtime_state(
    read_scope: &ExecutionReadScope,
    runtime_state: &RuntimeState,
) -> Result<WorkflowExecutionState, JsonFailure> {
    let context = runtime_state.context.clone();
    let projected_execution_status = runtime_state.status.clone();
    let review_state_snapshot = review_state_snapshot_from_read_scope_with_runtime_state(
        read_scope,
        &projected_execution_status,
        runtime_state,
    )?;
    let execution_status = runtime_state.status.clone();
    let overlay = read_scope.overlay.clone();
    let authoritative_state = read_scope.authoritative_state.as_ref();
    let preflight = runtime_state.preflight.clone();
    let gate_review = runtime_state.gate_review.clone();
    let gate_finish = runtime_state.gate_finish.clone();
    let task_review_dispatch_id = runtime_state.task_review_dispatch_id.clone();
    let task_negative_result_task = shared_current_task_negative_result_task(
        &execution_status,
        overlay.as_ref(),
        authoritative_state,
    );
    let task_negative_result_verification_failed = task_negative_result_task
        .and_then(|task| {
            authoritative_state
                .and_then(|state| state.task_closure_negative_result(task))
                .map(|record| record.verification_result.eq_ignore_ascii_case("fail"))
        })
        .unwrap_or(false);
    let task_scope_overlay_restore_required = execution_status.execution_started == "yes"
        && shared_task_scope_overlay_restore_required(
            &review_state_snapshot.missing_derived_overlays,
            authoritative_state,
        );
    let authoritative_current_branch_closure_id = runtime_state
        .authoritative_current_branch_closure_id
        .clone();
    let additional_branch_drift_signal = runtime_state.gate_snapshot.branch_closure_tracked_drift;
    let repair_route_decision = shared_repair_review_state_reroute_decision(
        &context,
        &execution_status,
        authoritative_state,
        gate_review.as_ref(),
        gate_finish.as_ref(),
        task_scope_overlay_restore_required,
        additional_branch_drift_signal,
    );
    let branch_reroute_still_valid = repair_route_decision.branch_reroute_still_valid;
    let persisted_repair_follow_up = repair_route_decision.persisted_repair_follow_up.as_deref();
    let task_scope_repair_precedence_active =
        repair_route_decision.task_scope_repair_precedence_active;
    let mut repair_review_state_follow_up = match repair_route_decision.repair_reroute {
        ReviewStateRepairReroute::RecordBranchClosure => Some(String::from("advance_late_stage")),
        ReviewStateRepairReroute::ExecutionReentry => Some(String::from("execution_reentry")),
        ReviewStateRepairReroute::None => None,
    };
    let persisted_branch_reroute_without_current_binding = repair_review_state_follow_up.as_deref()
        != Some("execution_reentry")
        && persisted_repair_follow_up == Some("advance_late_stage")
        && !task_scope_repair_precedence_active
        && branch_reroute_still_valid
        && execution_status.current_branch_closure_id.is_none();
    if persisted_branch_reroute_without_current_binding {
        repair_review_state_follow_up = Some(String::from("advance_late_stage"));
    }
    let final_review_dispatch_authority = runtime_state.final_review_dispatch_authority.clone();
    let final_review_dispatch_id = final_review_dispatch_authority.dispatch_id.clone();
    let late_stage_bindings = runtime_state.late_stage_bindings.clone();
    let final_review_dispatch_lineage_present = final_review_dispatch_authority.lineage_present;
    let finish_review_gate_pass_branch_closure_id =
        late_stage_bindings.finish_review_gate_pass_branch_closure_id;
    let current_release_readiness_result = late_stage_bindings.current_release_readiness_result;
    let current_final_review_branch_closure_id =
        late_stage_bindings.current_final_review_branch_closure_id;
    let current_final_review_result = late_stage_bindings.current_final_review_result;
    let current_qa_branch_closure_id = late_stage_bindings.current_qa_branch_closure_id;
    let current_qa_result = late_stage_bindings.current_qa_result;
    let base_branch = runtime_state.base_branch.clone();
    let qa_pending_test_plan_refresh_required =
        shared_normalized_plan_qa_requirement(context.plan_document.qa_requirement.as_deref())
            .as_deref()
            == Some("required")
            && qa_pending_requires_test_plan_refresh(&context, gate_finish.as_ref());
    Ok(WorkflowExecutionState {
        execution_context: Some(context.clone()),
        execution_status: Some(execution_status),
        preflight,
        gate_review,
        gate_finish,
        review_state_snapshot: Some(review_state_snapshot),
        task_scope_overlay_restore_required,
        task_negative_result_task,
        task_negative_result_verification_failed,
        task_review_dispatch_id,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
        current_branch_closure_id: authoritative_current_branch_closure_id,
        finish_review_gate_pass_branch_closure_id,
        current_release_readiness_result,
        current_final_review_branch_closure_id,
        current_final_review_result,
        current_qa_branch_closure_id,
        current_qa_result,
        base_branch,
        qa_requirement: shared_normalized_plan_qa_requirement(
            context.plan_document.qa_requirement.as_deref(),
        ),
        qa_pending_test_plan_refresh_required,
        persisted_repair_review_state_follow_up: persisted_repair_follow_up.map(str::to_owned),
        repair_review_state_follow_up,
    })
}

pub fn query_workflow_routing_state(
    current_dir: &std::path::Path,
    plan_override: Option<&std::path::Path>,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    query_workflow_routing_state_internal(
        current_dir,
        plan_override,
        external_review_result_ready,
        None,
        None,
        true,
    )
}

pub fn query_workflow_routing_state_for_runtime(
    runtime: &ExecutionRuntime,
    plan_override: Option<&std::path::Path>,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    query_workflow_routing_state_internal(
        &runtime.repo_root,
        plan_override,
        external_review_result_ready,
        Some(runtime),
        None,
        true,
    )
}

fn query_workflow_routing_state_internal(
    current_dir: &std::path::Path,
    plan_override: Option<&std::path::Path>,
    external_review_result_ready: bool,
    runtime_override: Option<&ExecutionRuntime>,
    preloaded_read_scope: Option<&ExecutionReadScope>,
    _require_exact_execution_command: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    let (fallback_identity, fallback_head_sha) = discover_slug_identity_and_head(current_dir);
    let workflow = if let Some(runtime) = runtime_override {
        WorkflowRuntime::discover_read_only_for_state_dir(&runtime.repo_root, &runtime.state_dir)
            .map_err(|error| {
                JsonFailure::new(
                    FailureClass::BranchDetectionFailed,
                    format!(
                        "Could not discover workflow runtime for execution query (repo_slug={}, branch_name={}, head_sha={}): {error}",
                        fallback_identity.repo_slug,
                        fallback_identity.branch_name,
                        fallback_head_sha.as_deref().unwrap_or("unknown"),
                    ),
                )
            })?
    } else {
        WorkflowRuntime::discover_read_only(current_dir).map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not discover workflow runtime for execution query (repo_slug={}, branch_name={}, head_sha={}): {error}",
                    fallback_identity.repo_slug,
                    fallback_identity.branch_name,
                    fallback_head_sha.as_deref().unwrap_or("unknown"),
                ),
            )
        })?
    };
    let mut route = workflow.resolve().map_err(JsonFailure::from)?;
    if let Some(plan_override) = plan_override {
        route = explicit_route_for_plan_override(&workflow, &route, plan_override)?;
    }
    let explicit_plan_query = plan_override.is_some();
    let engineering_approval_fidelity_blocked =
        route_is_engineering_approval_fidelity_blocked(&route);
    let should_load_execution_state = !route.plan_path.is_empty()
        && (route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY
            || explicit_plan_query
            || engineering_approval_fidelity_blocked);
    if should_load_execution_state {
        let runtime = runtime_override
            .cloned()
            .unwrap_or(ExecutionRuntime::discover(current_dir)?);
        if let Some(read_scope) = preloaded_read_scope {
            let (mut routing, _) =
                project_runtime_routing_state(&runtime, read_scope, external_review_result_ready)?;
            attach_runtime_provenance(&mut routing, Some(&runtime), current_dir);
            if engineering_approval_fidelity_blocked
                && projected_runtime_route_is_before_execution_entry(&routing)
            {
                let (mut routing, _) = project_non_runtime_workflow_routing_state(
                    route,
                    external_review_result_ready,
                )?;
                attach_runtime_provenance(&mut routing, Some(&runtime), current_dir);
                return Ok(routing);
            }
            if route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY
                || engineering_approval_fidelity_blocked
            {
                routing.route = route;
            }
            apply_read_surface_invariants_to_routing(&mut routing);
            return Ok(routing);
        }
        let mut read_scope = load_execution_read_scope(
            &runtime,
            std::path::Path::new(&route.plan_path),
            explicit_plan_query,
        )?;
        let (mut routing, _) = apply_shared_routing_projection_to_read_scope_with_routing(
            &mut read_scope,
            external_review_result_ready,
            false,
        )?;
        routing.execution_status = Some(read_scope.status);
        attach_runtime_provenance(&mut routing, Some(&runtime), current_dir);
        if engineering_approval_fidelity_blocked
            && projected_runtime_route_is_before_execution_entry(&routing)
        {
            let (mut routing, _) =
                project_non_runtime_workflow_routing_state(route, external_review_result_ready)?;
            attach_runtime_provenance(&mut routing, Some(&runtime), current_dir);
            return Ok(routing);
        }
        if route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY
            || engineering_approval_fidelity_blocked
        {
            routing.route = route;
        }
        apply_read_surface_invariants_to_routing(&mut routing);
        return Ok(routing);
    }
    let (mut routing, _) =
        project_non_runtime_workflow_routing_state(route, external_review_result_ready)?;
    attach_runtime_provenance(&mut routing, runtime_override, current_dir);
    Ok(routing)
}

fn attach_runtime_provenance(
    routing: &mut ExecutionRoutingState,
    runtime_override: Option<&ExecutionRuntime>,
    current_dir: &std::path::Path,
) {
    routing.runtime_provenance = runtime_override
        .map(ExecutionRuntime::runtime_provenance)
        .or_else(|| {
            ExecutionRuntime::discover(current_dir)
                .ok()
                .map(|runtime| runtime.runtime_provenance())
        })
        .or_else(|| {
            Some(runtime_provenance_for_paths(
                current_dir,
                &default_state_dir(),
            ))
        });
}

pub fn apply_read_surface_invariants_to_routing(routing: &mut ExecutionRoutingState) {
    let Some(status) = routing.execution_status.as_mut() else {
        return;
    };
    crate::execution::invariants::inject_read_surface_invariant_test_violation(status);
    let invariant_projection_already_active =
        crate::execution::invariants::read_surface_invariant_projection_active(status);
    let before = status.clone();
    crate::execution::invariants::apply_read_surface_invariants(status);
    if *status == before && !invariant_projection_already_active {
        return;
    }
    let status = status.clone();
    sync_routing_surface_from_status(routing, &status);
}

fn sync_routing_surface_from_status(
    routing: &mut ExecutionRoutingState,
    status: &PlanExecutionStatus,
) {
    routing.route_decision = None;
    if let Some(phase) = status.phase.clone() {
        routing.workflow_phase = phase.clone();
        routing.phase = phase;
    }
    routing.phase_detail.clone_from(&status.phase_detail);
    routing
        .review_state_status
        .clone_from(&status.review_state_status);
    routing.recording_context =
        status
            .recording_context
            .as_ref()
            .map(|context| ExecutionRoutingRecordingContext {
                task_number: context.task_number,
                dispatch_id: context.dispatch_id.clone(),
                branch_closure_id: context.branch_closure_id.clone(),
            });
    routing.execution_command_context = status.execution_command_context.as_ref().map(|context| {
        ExecutionRoutingExecutionCommandContext {
            command_kind: context.command_kind.clone(),
            task_number: context.task_number,
            step_id: context.step_id,
        }
    });
    routing.next_action.clone_from(&status.next_action);
    routing
        .recommended_public_command
        .clone_from(&status.recommended_public_command);
    routing
        .recommended_command
        .clone_from(&status.recommended_command);
    routing.blocking_scope.clone_from(&status.blocking_scope);
    routing.blocking_task = status.blocking_task;
    routing
        .external_wait_state
        .clone_from(&status.external_wait_state);
    routing
        .blocking_reason_codes
        .clone_from(&status.blocking_reason_codes);
}

pub(crate) fn default_phase_for_status(status: &PlanExecutionStatus) -> String {
    if matches!(
        status.harness_phase,
        HarnessPhase::ContractDrafting | HarnessPhase::PivotRequired
    ) {
        String::from(phase::PHASE_PIVOT_REQUIRED)
    } else if status.harness_phase == HarnessPhase::HandoffRequired
        || (status.phase_detail == phase::DETAIL_EXECUTION_IN_PROGRESS
            && status.execution_command_context.is_some())
    {
        String::from(phase::PHASE_HANDOFF_REQUIRED)
    } else if (status.harness_phase == HarnessPhase::Executing
        && matches!(
            status.phase_detail.as_str(),
            phase::DETAIL_HANDOFF_RECORDING_REQUIRED
                | phase::DETAIL_PLANNING_REENTRY_REQUIRED
                | phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        ))
        || (status.execution_started == "yes"
            && status.phase_detail == phase::DETAIL_EXECUTION_IN_PROGRESS
            && status.execution_command_context.is_none())
    {
        String::from(phase::PHASE_EXECUTING)
    } else if status.phase_detail == phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED
        && status.harness_phase != HarnessPhase::ExecutionPreflight
    {
        status.harness_phase.to_string()
    } else {
        status
            .phase
            .clone()
            .unwrap_or_else(|| status.harness_phase.to_string())
    }
}

pub(crate) fn canonical_phase_for_shared_decision(
    default_phase: &str,
    phase_detail: &str,
) -> String {
    match phase_detail {
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING | phase::DETAIL_TASK_CLOSURE_RECORDING_READY => {
            String::from(phase::PHASE_TASK_CLOSURE_PENDING)
        }
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED => {
            String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING)
        }
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
            if default_phase == phase::PHASE_DOCUMENT_RELEASE_PENDING =>
        {
            String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING)
        }
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
        | phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
        | phase::DETAIL_FINAL_REVIEW_RECORDING_READY => {
            String::from(phase::PHASE_FINAL_REVIEW_PENDING)
        }
        phase::DETAIL_QA_RECORDING_REQUIRED | phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED => {
            String::from(phase::PHASE_QA_PENDING)
        }
        phase::DETAIL_FINISH_REVIEW_GATE_READY | phase::DETAIL_FINISH_COMPLETION_GATE_READY => {
            String::from(phase::PHASE_READY_FOR_BRANCH_COMPLETION)
        }
        phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED => {
            if matches!(
                default_phase,
                phase::PHASE_PIVOT_REQUIRED | phase::PHASE_HANDOFF_REQUIRED
            ) {
                default_phase.to_owned()
            } else {
                String::from(phase::PHASE_EXECUTION_PREFLIGHT)
            }
        }
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => String::from(phase::PHASE_EXECUTING),
        phase::DETAIL_EXECUTION_IN_PROGRESS => {
            if matches!(
                default_phase,
                phase::PHASE_EXECUTION_PREFLIGHT | phase::PHASE_HANDOFF_REQUIRED
            ) {
                default_phase.to_owned()
            } else {
                String::from(phase::PHASE_EXECUTING)
            }
        }
        phase::DETAIL_PLANNING_REENTRY_REQUIRED => String::from(phase::PHASE_PIVOT_REQUIRED),
        phase::DETAIL_HANDOFF_RECORDING_REQUIRED => {
            if default_phase == phase::PHASE_EXECUTING {
                String::from(phase::PHASE_EXECUTING)
            } else {
                String::from(phase::PHASE_HANDOFF_REQUIRED)
            }
        }
        _ => default_phase.to_owned(),
    }
}

pub(crate) fn blocking_scope_for_phase_detail(
    phase_detail: &str,
    blocking_task: Option<u32>,
    status: Option<&PlanExecutionStatus>,
    review_state_status: &str,
) -> Option<String> {
    let scope = match phase_detail {
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING | phase::DETAIL_TASK_CLOSURE_RECORDING_READY => {
            Some("task")
        }
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
        | phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
        | phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
        | phase::DETAIL_FINAL_REVIEW_RECORDING_READY
        | phase::DETAIL_QA_RECORDING_REQUIRED
        | phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED
        | phase::DETAIL_FINISH_REVIEW_GATE_READY
        | phase::DETAIL_FINISH_COMPLETION_GATE_READY => Some("branch"),
        phase::DETAIL_PLANNING_REENTRY_REQUIRED => Some("workflow"),
        phase::DETAIL_HANDOFF_RECORDING_REQUIRED => {
            if blocking_task.is_some() {
                Some("task")
            } else {
                Some("workflow")
            }
        }
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => {
            if review_state_status == "stale_unreviewed"
                && let Some(task) = blocking_task
                && status.is_some_and(|status| status_has_task_blocking_record(status, task))
            {
                Some("task")
            } else if review_state_status == "stale_unreviewed"
                && blocking_task.is_some()
                && status.is_some_and(|status| !status.stale_unreviewed_closures.is_empty())
            {
                Some("task")
            } else if matches!(
                review_state_status,
                "missing_current_closure" | "stale_unreviewed"
            ) || status.is_some_and(|status| {
                shared_branch_closure_refresh_missing_current_closure(status)
                    || current_branch_closure_structural_review_state_reason(status).is_some()
            }) {
                Some("branch")
            } else if blocking_task.is_some() {
                Some("task")
            } else {
                Some("workflow")
            }
        }
        _ => None,
    };
    scope.map(str::to_owned)
}

fn status_has_task_blocking_record(status: &PlanExecutionStatus, task: u32) -> bool {
    status.blocking_records.iter().any(|record| {
        record.scope_type == "task"
            && record
                .scope_key
                .strip_prefix("task-")
                .and_then(|raw| {
                    let digits = raw
                        .chars()
                        .take_while(|character| character.is_ascii_digit())
                        .collect::<String>();
                    (!digits.is_empty()).then_some(digits)
                })
                .and_then(|digits| digits.parse::<u32>().ok())
                == Some(task)
    })
}

pub(crate) fn external_wait_state_for_phase_detail(
    phase_detail: &str,
    blocking_reason_codes: &[String],
    external_review_result_ready: bool,
) -> Option<String> {
    if external_review_result_ready {
        return None;
    }
    match phase_detail {
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING
            if !task_review_result_requires_verification(
                blocking_reason_codes.iter().map(String::as_str),
            ) =>
        {
            Some(String::from("waiting_for_external_review_result"))
        }
        phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING => {
            Some(String::from("waiting_for_external_review_result"))
        }
        _ => None,
    }
}

pub(crate) fn compact_operator_reason_codes(
    status: Option<&PlanExecutionStatus>,
    phase_detail: &str,
    review_state_status: &str,
) -> Vec<String> {
    fn push_unique_reason(reason_codes: &mut Vec<String>, code: &str) {
        if !reason_codes.iter().any(|existing| existing == code) {
            reason_codes.push(code.to_owned());
        }
    }

    let mut reason_codes = Vec::new();
    if review_state_status == "missing_current_closure" {
        push_unique_reason(&mut reason_codes, "missing_current_closure");
    }
    if review_state_status == "stale_unreviewed" {
        push_unique_reason(&mut reason_codes, "stale_unreviewed");
    }
    if let Some(status) = status {
        if matches!(
            phase_detail,
            phase::DETAIL_TASK_CLOSURE_RECORDING_READY
                | phase::DETAIL_TASK_REVIEW_RESULT_PENDING
                | phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        ) {
            let boundary_decision = public_task_boundary_decision(status);
            for code in boundary_decision.public_reason_codes {
                push_unique_reason(&mut reason_codes, &code);
            }
        }
        if phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            && status.blocking_task.is_some()
            && status.blocking_step.is_none()
            && status.active_task.is_none()
            && status.active_step.is_none()
            && status.resume_task.is_none()
            && status.resume_step.is_none()
        {
            push_unique_reason(&mut reason_codes, "task_closure_baseline_repair_candidate");
        }
        if phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED {
            for code in [
                "prior_task_current_closure_invalid",
                "prior_task_current_closure_reviewed_state_malformed",
                "prior_task_current_closure_missing",
                "prior_task_current_closure_stale",
                "current_task_closure_overlay_restore_required",
            ] {
                if status
                    .reason_codes
                    .iter()
                    .any(|reason_code| reason_code == code)
                {
                    push_unique_reason(&mut reason_codes, code);
                }
            }
        }
        if phase_detail == phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED {
            push_unique_reason(
                &mut reason_codes,
                phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
            );
        }
        if status
            .reason_codes
            .iter()
            .any(|code| code == "blocked_on_plan_revision")
        {
            push_unique_reason(&mut reason_codes, "blocked_on_plan_revision");
        }
        if status
            .reason_codes
            .iter()
            .any(|code| code == "write_authority_conflict")
            || status.write_authority_state == "conflict"
        {
            push_unique_reason(&mut reason_codes, "write_authority_conflict");
        }
        if status
            .reason_codes
            .iter()
            .any(|code| code == "repo_state_drift")
            || status.repo_state_drift_state == "drifted"
        {
            push_unique_reason(&mut reason_codes, "repo_state_drift");
        }
        if status
            .reason_codes
            .iter()
            .any(|code| code == "recovering_incomplete_authoritative_mutation")
        {
            push_unique_reason(
                &mut reason_codes,
                "recovering_incomplete_authoritative_mutation",
            );
        }
    }
    reason_codes
}

pub(crate) fn late_stage_observability_for_phase(
    phase: &str,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> (String, Vec<String>) {
    if !matches!(
        phase,
        phase::PHASE_DOCUMENT_RELEASE_PENDING
            | phase::PHASE_FINAL_REVIEW_PENDING
            | phase::PHASE_QA_PENDING
            | phase::PHASE_READY_FOR_BRANCH_COMPLETION
    ) {
        return (String::new(), Vec::new());
    }

    let Some(gate_finish) = gate_finish else {
        return (String::new(), Vec::new());
    };

    let mut diagnostic_reason_codes = gate_finish.reason_codes.clone();
    if let Some(gate_review) = gate_review {
        for reason_code in &gate_review.reason_codes {
            if !diagnostic_reason_codes
                .iter()
                .any(|existing| existing == reason_code)
            {
                diagnostic_reason_codes.push(reason_code.clone());
            }
        }
    }

    let release_blocked = shared_late_stage_release_blocked(Some(gate_finish))
        || shared_late_stage_release_truth_blocked(gate_review);
    let review_blocked = shared_late_stage_review_truth_blocked(gate_review)
        || shared_late_stage_review_blocked(Some(gate_finish));
    let qa_blocked = shared_late_stage_qa_blocked(Some(gate_finish));
    if !(gate_finish.allowed || release_blocked || review_blocked || qa_blocked) {
        return (
            String::from("fallback_fail_closed"),
            diagnostic_reason_codes,
        );
    }

    let decision = resolve_late_stage_precedence(LateStageSignals {
        release: GateState::from_blocked(release_blocked),
        review: GateState::from_blocked(review_blocked),
        qa: GateState::from_blocked(qa_blocked),
    });
    (decision.reason_family.to_owned(), diagnostic_reason_codes)
}

#[cfg(test)]
fn late_stage_repair_review_state_status(
    workflow_phase: &str,
    execution_status: Option<&PlanExecutionStatus>,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
    current_branch_closure_id: Option<&str>,
    _current_release_readiness_result: Option<&str>,
) -> Option<&'static str> {
    if !matches!(
        workflow_phase,
        phase::PHASE_DOCUMENT_RELEASE_PENDING
            | phase::PHASE_FINAL_REVIEW_PENDING
            | phase::PHASE_QA_PENDING
            | phase::PHASE_READY_FOR_BRANCH_COMPLETION
    ) {
        return None;
    }

    let stale_review_state = execution_status.is_some_and(|status| {
        shared_public_late_stage_stale_unreviewed(status, gate_review, gate_finish)
            || status.review_state_status == "stale_unreviewed"
    }) || (execution_status.is_none()
        && shared_late_stage_stale_unreviewed(gate_review, gate_finish));
    if stale_review_state {
        return Some("stale_unreviewed");
    }

    execution_status.and_then(|status| {
        if current_branch_closure_structural_review_state_reason(status).is_some() {
            return Some("missing_current_closure");
        }
        (current_branch_closure_id.is_some()
            && status
                .reason_codes
                .iter()
                .any(|code| code == "derived_review_state_missing"))
        .then_some("clean")
    })
}

fn explicit_route_for_plan_override(
    workflow: &WorkflowRuntime,
    resolved_route: &WorkflowRoute,
    plan_override: &std::path::Path,
) -> Result<WorkflowRoute, JsonFailure> {
    resolve_explicit_plan_override(workflow, resolved_route, plan_override)
        .map_err(JsonFailure::from)
}

fn route_is_engineering_approval_fidelity_blocked(route: &WorkflowRoute) -> bool {
    route.status == "plan_review_required"
        && route.next_skill == "featureforge:plan-eng-review"
        && route
            .reason_codes
            .iter()
            .any(|code| code.starts_with("engineering_approval_"))
}

fn projected_runtime_route_is_before_execution_entry(routing: &ExecutionRoutingState) -> bool {
    routing.phase_detail == phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED
}

fn task_scope_review_state_is_stale_unreviewed(status: &PlanExecutionStatus) -> bool {
    shared_task_scope_stale_review_state_reason_present(task_scope_review_state_repair_reason(
        status,
    ))
}

#[cfg(test)]
fn required_execution_command_for_routing(
    current_dir: &std::path::Path,
    runtime_override: Option<&ExecutionRuntime>,
    plan_path: &str,
    status: &PlanExecutionStatus,
    exact_plan_query: bool,
    message: &str,
) -> Result<(ExecutionRoutingExecutionCommandContext, String), JsonFailure> {
    let runtime = if let Some(runtime) = runtime_override {
        runtime.clone()
    } else {
        ExecutionRuntime::discover(current_dir)?
    };
    let plan_path_buf = PathBuf::from(plan_path);
    let context = if exact_plan_query {
        load_execution_context_for_exact_plan(&runtime, &plan_path_buf)?
    } else {
        load_execution_context(&runtime, &plan_path_buf)?
    };
    let decision = compute_next_action_decision(&context, status, plan_path)
        .ok_or_else(|| JsonFailure::new(FailureClass::MalformedExecutionState, message))?;
    let command = decision
        .recommended_public_command
        .as_ref()
        .ok_or_else(|| JsonFailure::new(FailureClass::MalformedExecutionState, message))?;
    let request = command
        .to_mutation_request()
        .ok_or_else(|| JsonFailure::new(FailureClass::MalformedExecutionState, message))?;
    Ok((
        ExecutionRoutingExecutionCommandContext {
            command_kind: request.command_name.to_owned(),
            task_number: request.task,
            step_id: request.step,
        },
        command.to_display_command(),
    ))
}

#[cfg(test)]
mod routing_helper_tests {
    use super::*;
    use crate::execution::current_truth::{
        task_review_dispatch_task, task_review_result_pending_task,
    };
    use crate::execution::router::{
        SharedNextActionRoutingInputs, shared_next_action_seed_from_decision,
    };
    use crate::execution::state::status_from_context;
    use crate::test_support::init_committed_test_repo;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn unresolved_execution_context() -> (TempDir, ExecutionRuntime, ExecutionContext, String) {
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/codex-runtime/fixtures/workflow-artifacts");
        let repo_dir = TempDir::new().expect("routing-helper temp repo should exist");
        let repo_root = repo_dir.path();
        let plan_rel =
            String::from("docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md");
        let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
        let plan_path = repo_root.join(&plan_rel);
        let spec_path = repo_root.join(spec_rel);

        init_committed_test_repo(
            repo_root,
            "# routing-helper-test\n",
            "routing-helper unit tests",
        );

        fs::create_dir_all(
            spec_path
                .parent()
                .expect("routing-helper spec fixture path should have a parent"),
        )
        .expect("routing-helper spec fixture directory should create");
        fs::create_dir_all(
            plan_path
                .parent()
                .expect("routing-helper plan fixture path should have a parent"),
        )
        .expect("routing-helper plan fixture directory should create");
        fs::copy(
            fixture_root.join("specs/2026-03-22-runtime-integration-hardening-design.md"),
            &spec_path,
        )
        .expect("routing-helper spec fixture should copy");
        let plan_source = fs::read_to_string(
            fixture_root.join("plans/2026-03-22-runtime-integration-hardening.md"),
        )
        .expect("routing-helper plan fixture should read")
        .replace(
            "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
            spec_rel,
        );
        fs::write(&plan_path, plan_source).expect("routing-helper plan fixture should write");

        let runtime =
            ExecutionRuntime::discover(repo_root).expect("routing-helper runtime should discover");
        let context = load_execution_context(&runtime, Path::new(&plan_rel))
            .expect("routing-helper plan should load");
        (repo_dir, runtime, context, plan_rel)
    }

    fn unresolved_status() -> PlanExecutionStatus {
        let (_repo_dir, _runtime, context, _plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("routing-helper status should derive");
        status.blocking_task = Some(1);
        status.blocking_step = None;
        status
    }

    fn routing_state_for_follow_up(
        phase_detail: &str,
        blocking_reason_codes: Vec<String>,
    ) -> ExecutionRoutingState {
        ExecutionRoutingState {
            route_decision: None,
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("ok"),
                next_skill: String::from("featureforge:workflow"),
                spec_path: String::new(),
                plan_path: String::new(),
                contract_state: String::new(),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
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
            phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
            phase_detail: phase_detail.to_owned(),
            review_state_status: String::from("clean"),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::new(),
            recommended_public_command: None,
            recommended_command: None,
            blocking_scope: Some(String::from("task")),
            blocking_task: Some(1),
            external_wait_state: None,
            blocking_reason_codes,
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: Some(String::from("dispatch-task-1")),
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        }
    }

    #[test]
    fn task_review_result_pending_task_demotes_verification_reason_codes_to_diagnostics() {
        let status = unresolved_status();
        for reason_code in [
            "prior_task_verification_missing",
            "prior_task_verification_missing_legacy",
            "task_verification_summary_malformed",
        ] {
            let mut mutated = status.clone();
            mutated.reason_codes = vec![reason_code.to_owned()];
            assert_eq!(
                task_review_result_pending_task(&mutated, Some("dispatch-task-1")),
                None,
                "{reason_code} should stay diagnostic-only instead of re-entering the task review result lane"
            );
        }
    }

    #[test]
    fn task_review_result_pending_task_excludes_invalid_review_reason_codes() {
        let status = unresolved_status();
        for reason_code in [
            "task_review_not_independent",
            "task_review_artifact_malformed",
        ] {
            let mut mutated = status.clone();
            mutated.reason_codes = vec![reason_code.to_owned()];
            assert_eq!(
                task_review_result_pending_task(&mutated, Some("dispatch-task-1")),
                None,
                "{reason_code} should not route through pending external-review wait lanes"
            );
        }
    }

    #[test]
    fn task_review_dispatch_task_ignores_stale_dispatch_reason_code() {
        let mut status = unresolved_status();
        status.reason_codes = vec![String::from("prior_task_review_dispatch_stale")];
        assert_eq!(
            task_review_dispatch_task(&status),
            None,
            "stale dispatch projection lineage is diagnostic-only for public routing",
        );
    }

    #[test]
    fn task_review_result_pending_task_excludes_stale_dispatch_reason_code() {
        let mut status = unresolved_status();
        status.reason_codes = vec![String::from("prior_task_review_dispatch_stale")];
        assert_eq!(
            task_review_result_pending_task(&status, Some("dispatch-task-1")),
            None,
            "stale dispatch should no longer route through the pending-review lane",
        );
    }

    #[test]
    fn required_follow_up_from_routing_keeps_non_green_review_in_execution_reentry_lane() {
        let routing = routing_state_for_follow_up(
            phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
            vec![String::from("prior_task_review_not_green")],
        );
        let routing = ExecutionRoutingState {
            recommended_public_command: Some(PublicCommand::RepairReviewState {
                plan: String::from("<approved-plan-path>"),
            }),
            recommended_command: Some(String::from(
                "featureforge plan execution repair-review-state --plan <approved-plan-path>",
            )),
            ..routing
        };
        assert_eq!(
            required_follow_up_from_routing(&routing).as_deref(),
            Some("repair_review_state")
        );
    }

    #[test]
    fn required_follow_up_from_routing_requires_verification_for_verification_blockers() {
        let routing = routing_state_for_follow_up(
            phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
            vec![String::from("prior_task_verification_missing")],
        );
        assert_eq!(
            required_follow_up_from_routing(&routing).as_deref(),
            Some("run_verification")
        );
    }

    #[test]
    fn verification_blockers_align_task_review_pending_follow_up_with_public_next_action() {
        let routing = routing_state_for_follow_up(
            phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
            vec![String::from("prior_task_verification_missing_legacy")],
        );
        assert_eq!(
            required_follow_up_from_routing(&routing).as_deref(),
            Some("run_verification")
        );
        let decision = NextActionDecision {
            kind: NextActionKind::WaitForTaskReviewResult,
            phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
            phase_detail: String::from(phase::DETAIL_TASK_REVIEW_RESULT_PENDING),
            review_state_status: String::from("clean"),
            task_number: Some(1),
            step_number: None,
            blocking_task: Some(1),
            blocking_reason_codes: routing.blocking_reason_codes.clone(),
            recommended_public_command: None,
        };
        assert_eq!(public_next_action_text(&decision), "run verification");
    }

    #[test]
    fn external_wait_state_omits_external_review_wait_for_verification_blockers() {
        assert_eq!(
            external_wait_state_for_phase_detail(
                phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
                &[String::from("prior_task_verification_missing")],
                false,
            ),
            None
        );
        assert_eq!(
            external_wait_state_for_phase_detail(
                phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
                &[String::from("prior_task_review_not_green")],
                false,
            )
            .as_deref(),
            Some("waiting_for_external_review_result")
        );
    }

    #[test]
    fn external_review_ready_keeps_verification_blockers_out_of_pending_review_lane() {
        let (_repo_dir, _runtime, context, plan_rel) = unresolved_execution_context();
        let mut status = unresolved_status();
        status.execution_started = String::from("yes");
        status.phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
        status.blocking_task = Some(1);
        status.blocking_step = None;
        status.reason_codes = vec![
            String::from("prior_task_current_closure_missing"),
            String::from("task_closure_baseline_repair_candidate"),
            String::from("prior_task_verification_missing"),
        ];

        let routing = shared_next_action_seed_from_decision(
            &context,
            &status,
            SharedNextActionRoutingInputs {
                plan_path: &plan_rel,
                external_review_result_ready: true,
                require_exact_execution_command: false,
                task_review_dispatch_id: Some("dispatch-task-1"),
                final_review_dispatch_id: None,
                final_review_dispatch_lineage_present: false,
                current_branch_closure_id: None,
            },
        )
        .expect("routing derivation should succeed")
        .expect("diagnostic verification blockers should still produce a routing decision");

        assert_ne!(
            routing.phase_detail,
            phase::DETAIL_TASK_REVIEW_RESULT_PENDING
        );
        assert_ne!(routing.next_action, "run verification");
    }

    #[test]
    fn late_stage_repair_review_state_required_overrides_missing_release_readiness_when_stale() {
        let mut status = unresolved_status();
        status.review_state_status = String::from("stale_unreviewed");

        assert!(
            late_stage_repair_review_state_status(
                phase::PHASE_FINAL_REVIEW_PENDING,
                Some(&status),
                None,
                None,
                Some("branch-release-closure"),
                None,
            )
            .is_some(),
            "stale reviewed state must route through repair-review-state before late-stage prerequisite recovery"
        );
    }

    #[test]
    fn late_stage_repair_review_state_keeps_projection_loss_out_of_stale_status() {
        let mut status = unresolved_status();
        status.reason_codes = vec![String::from("derived_review_state_missing")];

        assert_eq!(
            late_stage_repair_review_state_status(
                phase::PHASE_FINAL_REVIEW_PENDING,
                Some(&status),
                None,
                None,
                Some("branch-release-closure"),
                Some("ready"),
            ),
            Some("clean")
        );
    }

    #[test]
    fn follow_up_override_pivot_query_check_rejects_body_only_decoy_strings() {
        let (_repo_dir, _runtime, context, plan_rel) = unresolved_execution_context();
        let reason_codes = vec![String::from("blocked_on_plan_revision")];
        let head_sha = context
            .current_head_sha()
            .expect("head sha should resolve for pivot query check");
        let expected_decision_reason_codes =
            pivot_decision_reason_codes(&reason_codes, true, false).join(", ");
        let artifact_dir = context
            .runtime
            .state_dir
            .join("projects")
            .join(&context.runtime.repo_slug);
        fs::create_dir_all(&artifact_dir).expect("pivot artifact dir should be creatable");
        let artifact_path = artifact_dir.join(format!(
            "test-{}-workflow-pivot-999999999.md",
            context.runtime.safe_branch
        ));
        let decoy_source = format!(
            "# Workflow Pivot Record\n\
**Source Plan:** `docs/featureforge/plans/wrong.md`\n\
**Branch:** wrong-branch\n\
**Repo:** wrong/repo\n\
**Head SHA:** deadbeef\n\
**Decision Reason Codes:** wrong\n\
**Generated By:** featureforge:workflow-record-pivot\n\
\n\
mirror **Source Plan:** `{}`\n\
mirror **Branch:** {}\n\
mirror **Repo:** {}\n\
mirror **Head SHA:** {}\n\
mirror **Decision Reason Codes:** {}\n\
mirror **Generated By:** featureforge:workflow-record-pivot\n",
            plan_rel,
            context.runtime.branch_name,
            context.runtime.repo_slug,
            head_sha,
            expected_decision_reason_codes
        );
        fs::write(&artifact_path, decoy_source).expect("decoy pivot artifact should write");

        let follow_up = resolve_shared_follow_up_override(FollowUpOverrideInputs {
            state_dir: &context.runtime.state_dir,
            repo_slug: &context.runtime.repo_slug,
            safe_branch: &context.runtime.safe_branch,
            branch_name: &context.runtime.branch_name,
            plan_path: &plan_rel,
            head_sha: Some(&head_sha),
            workflow_phase: Some(phase::PHASE_PIVOT_REQUIRED),
            harness_phase: None,
            handoff_required: false,
            handoff_decision_scope: None,
            reason_codes: &reason_codes,
            qa_requirement: Some("required"),
        });
        fs::remove_file(&artifact_path).expect("decoy pivot artifact should clean up");

        assert_eq!(
            follow_up, "repair_review_state",
            "follow_up_override pivot clearing must rely on authoritative checkpoint headers, not body substring matches"
        );
    }

    #[test]
    fn required_execution_command_prefers_injected_runtime_over_ambient_state_dir() {
        let (_repo_dir, runtime, context, plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("routing-helper status should derive");
        status.execution_command_context = None;
        status.recommended_command = None;

        let unrelated_dir = TempDir::new().expect("unrelated current_dir should exist");
        let (_, recommended_command) = required_execution_command_for_routing(
            unrelated_dir.path(),
            Some(&runtime),
            &plan_rel,
            &status,
            false,
            "routing-helper should recover the exact execution command from the injected runtime",
        )
        .expect("runtime override should recover the exact execution command");

        assert!(
            recommended_command.starts_with("featureforge plan execution "),
            "expected an execution command, got {recommended_command}"
        );
        assert!(
            recommended_command.contains(&plan_rel),
            "recovered command should stay bound to the injected runtime plan path"
        );
    }
}
