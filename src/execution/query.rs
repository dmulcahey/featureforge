// Execution-owned review-state query layer.
// workflow consumes this module as a read-only client rather than reconstructing
// authoritative review-state truth from storage internals.

use std::path::PathBuf;

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::closure_graph::{
    AuthoritativeClosureGraph, ClosureGraphSignals, reason_code_indicates_stale_unreviewed,
};
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason, FollowUpOverrideInputs, IntentLevelCommandTemplateInputs,
    ReviewStateRepairReroute,
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    branch_closure_rerecording_assessment,
    current_branch_closure_has_tracked_drift as shared_current_branch_closure_has_tracked_drift,
    current_final_review_dispatch_id as shared_current_final_review_dispatch_id,
    current_late_stage_branch_bindings as shared_current_late_stage_branch_bindings,
    current_task_negative_result_task as shared_current_task_negative_result_task,
    current_task_review_dispatch_id as shared_current_task_review_dispatch_id,
    execution_state_has_open_steps as shared_execution_state_has_open_steps,
    final_review_dispatch_still_current, finish_requires_test_plan_refresh,
    handoff_decision_scope as shared_handoff_decision_scope, intent_level_command_template,
    late_stage_missing_current_closure_stale_provenance_present as shared_late_stage_missing_current_closure_stale_provenance_present,
    late_stage_qa_blocked as shared_late_stage_qa_blocked,
    late_stage_release_blocked as shared_late_stage_release_blocked,
    late_stage_release_truth_blocked as shared_late_stage_release_truth_blocked,
    late_stage_review_blocked as shared_late_stage_review_blocked,
    late_stage_review_truth_blocked as shared_late_stage_review_truth_blocked,
    late_stage_stale_unreviewed as shared_late_stage_stale_unreviewed,
    live_review_state_repair_reroute as shared_live_review_state_repair_reroute,
    live_task_scope_repair_precedence_active as shared_live_task_scope_repair_precedence_active,
    negative_result_requires_execution_reentry as shared_negative_result_requires_execution_reentry,
    normalized_plan_qa_requirement as shared_normalized_plan_qa_requirement,
    public_late_stage_rederivation_basis_present as late_stage_rederivation_basis_present,
    public_late_stage_stale_unreviewed as shared_public_late_stage_stale_unreviewed,
    public_review_state_stale_unreviewed_for_reroute as shared_public_review_state_stale_unreviewed_for_reroute,
    qa_requirement_policy_invalid as shared_qa_requirement_policy_invalid,
    resolve_follow_up_override as resolve_shared_follow_up_override,
    task_boundary_block_reason_code as shared_task_boundary_block_reason_code,
    task_review_dispatch_task, task_review_result_pending_task,
    task_scope_overlay_restore_required as shared_task_scope_overlay_restore_required,
    task_scope_stale_review_state_reason_present as shared_task_scope_stale_review_state_reason_present,
};
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
#[cfg(test)]
use crate::execution::state::status_from_context;
use crate::execution::state::{
    ExecutionContext, ExecutionReadScope, ExecutionRuntime, GateResult, PlanExecutionStatus,
    current_branch_closure_structural_review_state_reason, current_head_sha,
    execution_reentry_requires_review_state_repair, gate_finish_from_context,
    gate_review_from_context, live_review_state_status_for_reroute_from_status,
    load_execution_context, load_execution_context_for_exact_plan, load_execution_read_scope,
    missing_derived_review_state_fields, preflight_from_context,
    prerelease_branch_closure_refresh_required, qa_pending_requires_test_plan_refresh,
    require_public_exact_execution_command, resolve_exact_execution_command_from_context,
    stale_current_task_closure_record_ids, still_current_task_closure_records,
    task_completion_lineage_fingerprint, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason,
    usable_current_branch_closure_identity_from_authoritative_state,
};
use crate::git::discover_slug_identity_and_head;
use crate::workflow::late_stage_precedence::{
    GateState, LateStageSignals, resolve as resolve_late_stage_precedence,
};
#[cfg(test)]
use crate::workflow::pivot::pivot_decision_reason_codes;
use crate::workflow::status::{
    WorkflowRoute, WorkflowRuntime, explicit_plan_override_route as resolve_explicit_plan_override,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewStateTaskClosure {
    pub task: u32,
    pub closure_record_id: String,
    pub reviewed_state_id: String,
    pub contract_identity: String,
    pub effective_reviewed_surface_paths: Vec<String>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkflowExecutionState {
    pub execution_status: Option<PlanExecutionStatus>,
    pub preflight: Option<GateResult>,
    pub gate_review: Option<GateResult>,
    pub gate_finish: Option<GateResult>,
    pub review_state_snapshot: Option<ReviewStateSnapshot>,
    pub task_scope_overlay_restore_required: bool,
    pub task_negative_result_task: Option<u32>,
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
    pub execution_status: Option<PlanExecutionStatus>,
    pub preflight: Option<GateResult>,
    pub gate_review: Option<GateResult>,
    pub gate_finish: Option<GateResult>,
    pub workflow_phase: String,
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub qa_requirement: Option<String>,
    pub follow_up_override: String,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub recording_context: Option<ExecutionRoutingRecordingContext>,
    pub execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    pub next_action: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkflowRoutingDecision {
    phase: String,
    phase_detail: String,
    review_state_status: String,
    recording_context: Option<ExecutionRoutingRecordingContext>,
    execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    next_action: String,
    recommended_command: Option<String>,
    blocking_scope: Option<String>,
    blocking_task: Option<u32>,
    external_wait_state: Option<String>,
    blocking_reason_codes: Vec<String>,
}

pub(crate) fn required_follow_up_from_routing(routing: &ExecutionRoutingState) -> Option<String> {
    if routing.phase_detail == "branch_closure_recording_required_for_release_readiness" {
        return Some(String::from("advance_late_stage"));
    }
    if routing_requires_review_state_repair(routing) {
        return Some(String::from("repair_review_state"));
    }
    match routing.phase_detail.as_str() {
        "task_review_dispatch_required" | "final_review_dispatch_required" => {
            Some(String::from("request_external_review"))
        }
        "release_blocker_resolution_required" => Some(String::from("resolve_release_blocker")),
        "execution_reentry_required" => Some(String::from("execution_reentry")),
        "handoff_recording_required" => Some(String::from("record_handoff")),
        "planning_reentry_required" => Some(String::from("record_pivot")),
        _ => None,
    }
}

fn routing_requires_review_state_repair(routing: &ExecutionRoutingState) -> bool {
    if routing.review_state_status == "stale_unreviewed" {
        return true;
    }
    if routing.phase_detail != "execution_reentry_required" {
        return false;
    }
    if routing.execution_command_context.is_none() {
        return true;
    }
    if routing.review_state_status != "clean" {
        return true;
    }
    routing.execution_status.as_ref().is_some_and(|status| {
        task_scope_structural_review_state_reason(status).is_some()
            || task_scope_review_state_repair_reason(status).is_some()
            || current_branch_closure_structural_review_state_reason(status).is_some()
            || status
                .reason_codes
                .iter()
                .any(|code| code == "derived_review_state_missing")
    })
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
    let read_scope = load_execution_read_scope(runtime, plan_path, exact_plan_override)?;
    review_state_snapshot_from_read_scope(&read_scope)
}

fn review_state_snapshot_from_read_scope(
    read_scope: &ExecutionReadScope,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    let context = &read_scope.context;
    let status = &read_scope.status;
    let overlay = read_scope.overlay.as_ref();
    let authoritative_state = read_scope.authoritative_state.as_ref();
    let branch_closure_tracked_drift =
        shared_current_branch_closure_has_tracked_drift(context, authoritative_state)?;
    let gate_review = gate_review_from_context(context);
    let gate_finish = gate_finish_from_context(context);
    let late_stage_stale_unreviewed = shared_public_review_state_stale_unreviewed_for_reroute(
        context,
        authoritative_state,
        status,
        Some(&gate_review),
        Some(&gate_finish),
    )
    .unwrap_or_else(|_| review_state_is_stale_unreviewed(context, status));
    let late_stage_missing_current_closure_public_truth = status.review_state_status
        == "missing_current_closure"
        && shared_late_stage_missing_current_closure_stale_provenance_present(context, status)?;
    let status_reports_stale_unreviewed_closures = !status.stale_unreviewed_closures.is_empty();
    let late_stage_stale_projection_active = late_stage_stale_unreviewed
        || late_stage_missing_current_closure_public_truth
        || (branch_closure_tracked_drift
            && (status.review_state_status != "missing_current_closure"
                || status_reports_stale_unreviewed_closures));
    let task_scope_stale_unreviewed = task_scope_review_state_is_stale_unreviewed(status);
    let task_scope_structural_reason = task_scope_structural_review_state_reason(status);
    let branch_scope_structural_reason =
        current_branch_closure_structural_review_state_reason(status);
    let closure_graph = AuthoritativeClosureGraph::from_state(
        authoritative_state,
        &ClosureGraphSignals::from_authoritative_state(
            authoritative_state,
            overlay.and_then(|overlay| overlay.current_branch_closure_id.as_deref()),
            late_stage_stale_unreviewed || branch_closure_tracked_drift,
            late_stage_missing_current_closure_public_truth,
            stale_reason_codes_from_gate_results(&gate_review, &gate_finish),
        ),
    );
    let current_task_closures = still_current_task_closure_records(context)?
        .into_iter()
        .filter(|record| {
            closure_graph
                .current_task_closure(record.task)
                .is_none_or(|evaluation| evaluation.identity.record_id == record.closure_record_id)
        })
        .map(|record| ReviewStateTaskClosure {
            task: record.task,
            closure_record_id: record.closure_record_id,
            reviewed_state_id: record.reviewed_state_id,
            contract_identity: record.contract_identity,
            effective_reviewed_surface_paths: record.effective_reviewed_surface_paths,
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
    let late_stage_stale_unreviewed_closures = closure_graph.stale_unreviewed_record_ids();
    let stale_unreviewed_closures = if task_scope_structural_reason.is_some() {
        stale_current_task_closure_record_ids(context)?
    } else if branch_scope_structural_reason.is_some() {
        Vec::new()
    } else if late_stage_stale_projection_active {
        late_stage_stale_unreviewed_closures
    } else if task_scope_stale_unreviewed {
        stale_current_task_closure_record_ids(context)?
    } else {
        Vec::new()
    };
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

fn stale_reason_codes_from_gate_results(
    gate_review: &GateResult,
    gate_finish: &GateResult,
) -> Vec<String> {
    let mut reason_codes = Vec::new();
    for reason_code in gate_review
        .reason_codes
        .iter()
        .chain(gate_finish.reason_codes.iter())
    {
        if reason_code_indicates_stale_unreviewed(reason_code)
            && !reason_codes.iter().any(|existing| existing == reason_code)
        {
            reason_codes.push(reason_code.clone());
        }
    }
    reason_codes
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
    require_exact_execution_command: bool,
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
    let review_state_snapshot = review_state_snapshot_from_read_scope(read_scope)?;
    let context = read_scope.context.clone();
    let execution_status = read_scope.status.clone();
    let overlay = read_scope.overlay.clone();
    let authoritative_state = read_scope.authoritative_state.as_ref();
    if require_exact_execution_command {
        require_public_exact_execution_command(&context, &execution_status)?;
    }
    let mut preflight = None;
    let mut gate_review = None;
    let mut gate_finish = None;
    if execution_status.execution_started == "yes" {
        if !shared_execution_state_has_open_steps(&execution_status) {
            let review = gate_review_from_context(&context);
            gate_finish = Some(gate_finish_from_context(&context));
            gate_review = Some(review);
        }
    } else if !status_has_accepted_preflight(&execution_status) {
        preflight = Some(preflight_from_context(&context));
    }
    let current_task_lineage_fingerprint = execution_status
        .blocking_task
        .and_then(|task_number| task_completion_lineage_fingerprint(&context, task_number));
    let current_task_reviewed_state_id = execution_status.blocking_task.and_then(|_| {
        context
            .current_tracked_tree_sha()
            .ok()
            .map(|tree_sha| format!("git_tree:{tree_sha}"))
    });
    let task_review_dispatch_id = shared_current_task_review_dispatch_id(
        execution_status.blocking_task,
        current_task_lineage_fingerprint.as_deref(),
        current_task_reviewed_state_id.as_deref(),
        overlay.as_ref(),
    );
    let task_negative_result_task = shared_current_task_negative_result_task(
        &execution_status,
        overlay.as_ref(),
        authoritative_state,
    );
    let task_scope_overlay_restore_required = execution_status.execution_started == "yes"
        && shared_task_scope_overlay_restore_required(
            &review_state_snapshot.missing_derived_overlays,
            authoritative_state,
        );
    let usable_current_branch_closure_identity =
        usable_current_branch_closure_identity_from_authoritative_state(
            &context,
            authoritative_state,
        );
    let usable_current_branch_closure_id = usable_current_branch_closure_identity
        .as_ref()
        .map(|identity| identity.branch_closure_id.clone());
    let usable_current_branch_reviewed_state_id = usable_current_branch_closure_identity
        .as_ref()
        .map(|identity| identity.reviewed_state_id.clone());
    let authoritative_current_branch_closure_id = usable_current_branch_closure_id.clone();
    let branch_reroute_still_valid = branch_closure_rerecording_assessment(&context)
        .map(|assessment| assessment.supported)
        .unwrap_or(false);
    let persisted_repair_follow_up =
        authoritative_state.and_then(|state| state.review_state_repair_follow_up());
    let live_stale_unreviewed = shared_public_review_state_stale_unreviewed_for_reroute(
        &context,
        authoritative_state,
        &execution_status,
        gate_review.as_ref(),
        gate_finish.as_ref(),
    )
    .unwrap_or_else(|_| {
        review_state_is_stale_unreviewed(&context, &execution_status)
            || shared_current_branch_closure_has_tracked_drift(&context, authoritative_state)
                .unwrap_or(false)
    });
    let live_review_state_status =
        live_review_state_status_for_reroute_from_status(&execution_status, live_stale_unreviewed);
    let task_scope_stale_reason_present = shared_task_scope_stale_review_state_reason_present(
        task_scope_review_state_repair_reason(&execution_status),
    );
    let task_scope_repair_precedence_active = shared_live_task_scope_repair_precedence_active(
        task_scope_overlay_restore_required,
        task_scope_structural_review_state_reason(&execution_status).is_some(),
        task_scope_stale_reason_present,
        persisted_repair_follow_up,
        branch_reroute_still_valid,
        live_review_state_status,
    );
    let repair_review_state_follow_up = match shared_live_review_state_repair_reroute(
        persisted_repair_follow_up,
        task_scope_repair_precedence_active,
        branch_reroute_still_valid,
        live_review_state_status,
        shared_branch_closure_refresh_missing_current_closure(&execution_status),
    ) {
        ReviewStateRepairReroute::RecordBranchClosure => {
            Some(String::from("record_branch_closure"))
        }
        ReviewStateRepairReroute::ExecutionReentry => Some(String::from("execution_reentry")),
        ReviewStateRepairReroute::None => None,
    };
    let final_review_dispatch_id = shared_current_final_review_dispatch_id(
        usable_current_branch_closure_id.as_deref(),
        overlay.as_ref(),
    );
    let final_review_dispatch_lineage_present = overlay
        .as_ref()
        .and_then(|overlay| overlay.final_review_dispatch_lineage.as_ref())
        .and_then(|record| {
            let execution_run_id = record.execution_run_id.as_deref()?;
            if execution_run_id.trim().is_empty() {
                return None;
            }
            let branch_closure_id = record.branch_closure_id.as_deref()?;
            if usable_current_branch_closure_id.as_deref() != Some(branch_closure_id) {
                return None;
            }
            record
                .dispatch_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .is_some();
    let late_stage_bindings = shared_current_late_stage_branch_bindings(
        authoritative_state,
        authoritative_current_branch_closure_id.as_deref(),
        usable_current_branch_reviewed_state_id.as_deref(),
    );
    let finish_review_gate_pass_branch_closure_id =
        late_stage_bindings.finish_review_gate_pass_branch_closure_id;
    let current_release_readiness_result = late_stage_bindings.current_release_readiness_result;
    let current_final_review_branch_closure_id =
        late_stage_bindings.current_final_review_branch_closure_id;
    let current_final_review_result = late_stage_bindings.current_final_review_result;
    let current_qa_branch_closure_id = late_stage_bindings.current_qa_branch_closure_id;
    let current_qa_result = late_stage_bindings.current_qa_result;
    let base_branch = authoritative_state.and_then(|state| {
        authoritative_current_branch_closure_id
            .as_deref()
            .or(current_final_review_branch_closure_id.as_deref())
            .or(current_qa_branch_closure_id.as_deref())
            .or(finish_review_gate_pass_branch_closure_id.as_deref())
            .and_then(|branch_closure_id| {
                state
                    .branch_closure_record(branch_closure_id)
                    .map(|record| record.base_branch)
            })
            .or_else(|| {
                state
                    .current_release_readiness_record()
                    .map(|record| record.base_branch)
            })
            .or_else(|| state.current_final_review_record().map(|record| record.base_branch))
            .or_else(|| state.current_browser_qa_record().map(|record| record.base_branch))
    });
    let qa_pending_test_plan_refresh_required =
        shared_normalized_plan_qa_requirement(context.plan_document.qa_requirement.as_deref())
            .as_deref()
            == Some("required")
            && qa_pending_requires_test_plan_refresh(&context, gate_finish.as_ref());
    Ok(WorkflowExecutionState {
        execution_status: Some(execution_status),
        preflight,
        gate_review,
        gate_finish,
        review_state_snapshot: Some(review_state_snapshot),
        task_scope_overlay_restore_required,
        task_negative_result_task,
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

pub(crate) fn query_workflow_routing_state_for_runtime_with_read_scope(
    runtime: &ExecutionRuntime,
    read_scope: &ExecutionReadScope,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    query_workflow_routing_state_internal(
        &runtime.repo_root,
        Some(std::path::Path::new(read_scope.context.plan_rel.as_str())),
        external_review_result_ready,
        Some(runtime),
        Some(read_scope),
        true,
    )
}

pub(crate) fn query_workflow_routing_state_for_runtime_with_read_scope_best_effort(
    runtime: &ExecutionRuntime,
    read_scope: &ExecutionReadScope,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    query_workflow_routing_state_internal(
        &runtime.repo_root,
        Some(std::path::Path::new(read_scope.context.plan_rel.as_str())),
        external_review_result_ready,
        Some(runtime),
        Some(read_scope),
        false,
    )
}

fn query_workflow_routing_state_internal(
    current_dir: &std::path::Path,
    plan_override: Option<&std::path::Path>,
    external_review_result_ready: bool,
    runtime_override: Option<&ExecutionRuntime>,
    preloaded_read_scope: Option<&ExecutionReadScope>,
    require_exact_execution_command: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    let workflow = if let Some(runtime) = runtime_override {
        WorkflowRuntime::discover_read_only_for_state_dir(&runtime.repo_root, &runtime.state_dir)
            .map_err(JsonFailure::from)?
    } else {
        WorkflowRuntime::discover_read_only(current_dir).map_err(JsonFailure::from)?
    };
    let mut route = workflow.resolve().map_err(JsonFailure::from)?;
    if let Some(plan_override) = plan_override {
        route = explicit_route_for_plan_override(&workflow, &route, plan_override)?;
    }
    let mut execution_status = None;
    let mut preflight = None;
    let mut gate_review = None;
    let mut gate_finish = None;
    let mut task_scope_overlay_restore_required = false;
    let mut review_state_snapshot = None;
    let mut task_negative_result_task = None;
    let mut task_review_dispatch_id = None;
    let mut final_review_dispatch_id = None;
    let mut final_review_dispatch_lineage_present = false;
    let mut current_branch_closure_id = None;
    let mut finish_review_gate_pass_branch_closure_id = None;
    let mut current_release_readiness_result = None;
    let mut current_final_review_branch_closure_id = None;
    let mut current_final_review_result = None;
    let mut current_qa_branch_closure_id = None;
    let mut current_qa_result = None;
    let mut base_branch = None;
    let mut qa_requirement = None;
    let mut qa_pending_test_plan_refresh_required = false;
    let mut persisted_repair_review_state_follow_up = None;
    let mut repair_review_state_follow_up = None;
    let mut resolved_runtime = runtime_override.cloned();

    let explicit_plan_query = plan_override.is_some();
    let should_load_execution_state = !route.plan_path.is_empty()
        && (route.status == "implementation_ready" || explicit_plan_query);
    if should_load_execution_state {
        let runtime = resolved_runtime
            .clone()
            .unwrap_or(ExecutionRuntime::discover(current_dir)?);
        resolved_runtime = Some(runtime.clone());
        let workflow_state = query_workflow_execution_state_internal(
            &runtime,
            &route.plan_path,
            explicit_plan_query,
            preloaded_read_scope,
            require_exact_execution_command,
        )?;
        let WorkflowExecutionState {
            execution_status: workflow_execution_status,
            preflight: workflow_preflight,
            gate_review: workflow_gate_review,
            gate_finish: workflow_gate_finish,
            review_state_snapshot: workflow_review_state_snapshot,
            task_scope_overlay_restore_required: workflow_task_scope_overlay_restore_required,
            task_negative_result_task: workflow_task_negative_result_task,
            task_review_dispatch_id: workflow_task_review_dispatch_id,
            final_review_dispatch_id: workflow_final_review_dispatch_id,
            final_review_dispatch_lineage_present: workflow_final_review_dispatch_lineage_present,
            current_branch_closure_id: workflow_current_branch_closure_id,
            finish_review_gate_pass_branch_closure_id:
                workflow_finish_review_gate_pass_branch_closure_id,
            current_release_readiness_result: workflow_current_release_readiness_result,
            current_final_review_branch_closure_id: workflow_current_final_review_branch_closure_id,
            current_final_review_result: workflow_current_final_review_result,
            current_qa_branch_closure_id: workflow_current_qa_branch_closure_id,
            current_qa_result: workflow_current_qa_result,
            base_branch: workflow_base_branch,
            qa_requirement: workflow_qa_requirement,
            qa_pending_test_plan_refresh_required: workflow_qa_pending_test_plan_refresh_required,
            persisted_repair_review_state_follow_up:
                workflow_persisted_repair_review_state_follow_up,
            repair_review_state_follow_up: workflow_repair_review_state_follow_up,
        } = workflow_state;
        review_state_snapshot = workflow_review_state_snapshot;
        task_scope_overlay_restore_required = workflow_task_scope_overlay_restore_required;
        task_negative_result_task = workflow_task_negative_result_task;
        task_review_dispatch_id = workflow_task_review_dispatch_id;
        final_review_dispatch_id = workflow_final_review_dispatch_id;
        final_review_dispatch_lineage_present = workflow_final_review_dispatch_lineage_present;
        current_branch_closure_id = workflow_current_branch_closure_id;
        finish_review_gate_pass_branch_closure_id =
            workflow_finish_review_gate_pass_branch_closure_id;
        current_release_readiness_result = workflow_current_release_readiness_result;
        current_final_review_branch_closure_id = workflow_current_final_review_branch_closure_id;
        current_final_review_result = workflow_current_final_review_result;
        current_qa_branch_closure_id = workflow_current_qa_branch_closure_id;
        current_qa_result = workflow_current_qa_result;
        base_branch = workflow_base_branch;
        qa_requirement = workflow_qa_requirement;
        qa_pending_test_plan_refresh_required = workflow_qa_pending_test_plan_refresh_required;
        persisted_repair_review_state_follow_up = workflow_persisted_repair_review_state_follow_up;
        repair_review_state_follow_up = workflow_repair_review_state_follow_up;
        execution_status = workflow_execution_status;
        preflight = workflow_preflight;
        gate_review = workflow_gate_review;
        gate_finish = workflow_gate_finish;
    }

    let route_status_for_phase = if route.status == "implementation_ready"
        || explicit_plan_execution_route_required(execution_status.as_ref())
    {
        "implementation_ready"
    } else {
        route.status.as_str()
    };
    let workflow_phase = derive_phase(
        route_status_for_phase,
        execution_status.as_ref(),
        preflight.as_ref(),
        gate_review.as_ref(),
        gate_finish.as_ref(),
    );
    let (reason_family, diagnostic_reason_codes) = late_stage_observability_for_phase(
        &workflow_phase,
        gate_review.as_ref(),
        gate_finish.as_ref(),
    );
    let plan_path = route.plan_path.clone();
    let exact_plan_query = explicit_plan_query;
    let (repo_slug, safe_branch, branch_name, head_sha) =
        if let Some(runtime) = resolved_runtime.as_ref() {
            (
                runtime.repo_slug.clone(),
                runtime.safe_branch.clone(),
                runtime.branch_name.clone(),
                current_head_sha(&runtime.repo_root).ok(),
            )
        } else {
            let (slug_identity, head_sha) = discover_slug_identity_and_head(current_dir);
            (
                slug_identity.repo_slug,
                slug_identity.safe_branch,
                slug_identity.branch_name,
                head_sha,
            )
        };
    let follow_up_override = resolve_shared_follow_up_override(FollowUpOverrideInputs {
        state_dir: &workflow.state_dir,
        repo_slug: &repo_slug,
        safe_branch: &safe_branch,
        branch_name: &branch_name,
        plan_path: &plan_path,
        head_sha: head_sha.as_deref(),
        workflow_phase: Some(workflow_phase.as_str()),
        harness_phase: execution_status.as_ref().map(|status| status.harness_phase),
        handoff_required: execution_status
            .as_ref()
            .is_some_and(|status| status.handoff_required),
        handoff_decision_scope: execution_status.as_ref().and_then(|status| {
            shared_handoff_decision_scope(
                status.active_task,
                status.blocking_task,
                status.resume_task,
                status.handoff_required,
                Some(status.harness_phase),
            )
        }),
        reason_codes: execution_status
            .as_ref()
            .map(|status| status.reason_codes.as_slice())
            .unwrap_or(route.reason_codes.as_slice()),
        qa_requirement: qa_requirement.as_deref(),
    });
    let (
        phase,
        phase_detail,
        review_state_status,
        recording_context,
        execution_command_context,
        next_action,
        recommended_command,
    ) = if let Some(status) = execution_status.as_ref() {
        let late_stage_review_state_stale_unreviewed = review_state_snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.stale_unreviewed_closures.is_empty());
        let status_projects_review_state_repair_execution_reentry =
            execution_reentry_requires_review_state_repair(status)
                && status.review_state_status != "clean"
                && !(repair_review_state_follow_up.as_deref() == Some("record_branch_closure")
                    && current_branch_closure_structural_review_state_reason(status).is_none());
        if task_scope_overlay_restore_required
            || status_projects_review_state_repair_execution_reentry
            || ((workflow_phase == "executing" || workflow_phase == "task_closure_pending")
                && (((status.resume_task.is_some() || status.resume_step.is_some())
                    && execution_reentry_requires_review_state_repair(status))
                    || task_scope_structural_review_state_reason(status).is_some()
                    || shared_task_scope_stale_review_state_reason_present(
                        task_scope_review_state_repair_reason(status),
                    )))
        {
            (
                String::from("executing"),
                String::from("execution_reentry_required"),
                status.review_state_status.clone(),
                None,
                None,
                String::from("repair review state / reenter execution"),
                Some(format!(
                    "featureforge plan execution repair-review-state --plan {plan_path}"
                )),
            )
        } else if matches!(
            workflow_phase.as_str(),
            "document_release_pending"
                | "final_review_pending"
                | "qa_pending"
                | "ready_for_branch_completion"
        ) && late_stage_review_state_stale_unreviewed
            && !prerelease_branch_closure_refresh_required(status)
        {
            if repair_review_state_follow_up.as_deref() == Some("record_branch_closure") {
                (
                    String::from("document_release_pending"),
                    String::from("branch_closure_recording_required_for_release_readiness"),
                    String::from("missing_current_closure"),
                    None,
                    None,
                    String::from("advance late stage"),
                    Some(format!(
                        "featureforge plan execution advance-late-stage --plan {plan_path}"
                    )),
                )
            } else {
                (
                    String::from("executing"),
                    String::from("execution_reentry_required"),
                    String::from("stale_unreviewed"),
                    None,
                    None,
                    String::from("repair review state / reenter execution"),
                    Some(format!(
                        "featureforge plan execution repair-review-state --plan {plan_path}"
                    )),
                )
            }
        } else if repair_review_state_follow_up.as_deref() == Some("execution_reentry") {
            let exact_execution_command = required_execution_command_for_routing(
                current_dir,
                runtime_override,
                &plan_path,
                status,
                exact_plan_query,
                "workflow/operator could not derive the exact execution command required after repair-review-state rerouted late-stage review truth back to execution.",
            );
            let (execution_command_context, recommended_command) = if require_exact_execution_command {
                let (execution_command_context, recommended_command) = exact_execution_command?;
                (Some(execution_command_context), Some(recommended_command))
            } else {
                exact_execution_command
                    .ok()
                    .map_or((None, None), |(execution_command_context, recommended_command)| {
                        (Some(execution_command_context), Some(recommended_command))
                    })
            };
            (
                String::from("executing"),
                String::from("execution_reentry_required"),
                String::from("clean"),
                None,
                execution_command_context,
                String::from("execution reentry required"),
                recommended_command,
            )
        } else if repair_review_state_follow_up.as_deref() == Some("record_branch_closure") {
            (
                String::from("document_release_pending"),
                String::from("branch_closure_recording_required_for_release_readiness"),
                String::from("missing_current_closure"),
                None,
                None,
                String::from("advance late stage"),
                Some(format!(
                    "featureforge plan execution advance-late-stage --plan {plan_path}"
                )),
            )
        } else if workflow_phase == "executing"
            && late_stage_rederivation_basis_present(status)
            && execution_reentry_requires_review_state_repair(status)
        {
            (
                String::from("executing"),
                String::from("execution_reentry_required"),
                status.review_state_status.clone(),
                None,
                None,
                String::from("repair review state / reenter execution"),
                Some(format!(
                    "featureforge plan execution repair-review-state --plan {plan_path}"
                )),
            )
        } else if let Some(repair_review_state_status) = late_stage_repair_review_state_status(
            workflow_phase.as_str(),
            execution_status.as_ref(),
            gate_review.as_ref(),
            gate_finish.as_ref(),
            current_branch_closure_id.as_deref(),
            current_release_readiness_result.as_deref(),
        ) {
            (
                String::from("executing"),
                String::from("execution_reentry_required"),
                String::from(repair_review_state_status),
                None,
                None,
                String::from("repair review state / reenter execution"),
                Some(format!(
                    "featureforge plan execution repair-review-state --plan {plan_path}"
                )),
            )
        } else if !late_stage_prerequisite_reroute_active(
            workflow_phase.as_str(),
            qa_requirement.as_deref(),
            qa_pending_test_plan_refresh_required,
            current_branch_closure_id.as_deref(),
            current_release_readiness_result.as_deref(),
        ) && shared_negative_result_requires_execution_reentry(
            task_negative_result_task.is_some(),
            workflow_phase.as_str(),
            current_branch_closure_id.as_deref(),
            current_final_review_branch_closure_id.as_deref(),
            current_final_review_result.as_deref(),
            current_qa_branch_closure_id.as_deref(),
            current_qa_result.as_deref(),
        ) {
            match follow_up_override.as_str() {
                "record_handoff" => (
                    String::from("handoff_required"),
                    String::from("handoff_recording_required"),
                    String::from("clean"),
                    None,
                    None,
                    String::from("hand off"),
                    Some(format!(
                        "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
                    )),
                ),
                "record_pivot" => (
                    String::from("pivot_required"),
                    String::from("planning_reentry_required"),
                    String::from("clean"),
                    None,
                    None,
                    String::from("pivot / return to planning"),
                    Some(format!(
                        "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                    )),
                ),
                _ => {
                    let exact_execution_command = required_execution_command_for_routing(
                        current_dir,
                        runtime_override,
                        &plan_path,
                        status,
                        exact_plan_query,
                        "workflow/operator could not derive the execution reentry command required after a negative review outcome.",
                    );
                    let (execution_command_context, recommended_command) =
                        if require_exact_execution_command {
                            let (execution_command_context, recommended_command) =
                                exact_execution_command?;
                            (Some(execution_command_context), Some(recommended_command))
                        } else {
                            exact_execution_command.ok().map_or(
                                (None, None),
                                |(execution_command_context, recommended_command)| {
                                    (Some(execution_command_context), Some(recommended_command))
                                },
                            )
                        };
                    (
                        String::from("executing"),
                        String::from("execution_reentry_required"),
                        String::from("clean"),
                        None,
                        execution_command_context,
                        String::from("execution reentry required"),
                        recommended_command,
                    )
                }
            }
        } else {
            if let Some(task_number) =
                task_review_result_pending_task(status, task_review_dispatch_id.as_deref())
            {
                let recommended_command = if external_review_result_ready {
                    Some(format!(
                        "featureforge plan execution close-current-task --plan {plan_path} --task {task_number} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
                    ))
                } else {
                    None
                };
                (
                    String::from("task_closure_pending"),
                    if external_review_result_ready {
                        String::from("task_closure_recording_ready")
                    } else {
                        String::from("task_review_result_pending")
                    },
                    String::from("clean"),
                    if external_review_result_ready {
                        Some(ExecutionRoutingRecordingContext {
                            task_number: Some(task_number),
                            dispatch_id: task_review_dispatch_id.clone(),
                            branch_closure_id: None,
                        })
                    } else {
                        None
                    },
                    None,
                    if external_review_result_ready {
                        String::from("close current task")
                    } else {
                        String::from("wait for external review result")
                    },
                    recommended_command,
                )
            } else if let Some(task_number) = task_review_dispatch_task(status) {
                if external_review_result_ready {
                    (
                        String::from("task_closure_pending"),
                        String::from("task_closure_recording_ready"),
                        String::from("clean"),
                        Some(ExecutionRoutingRecordingContext {
                            task_number: Some(task_number),
                            dispatch_id: task_review_dispatch_id.clone(),
                            branch_closure_id: None,
                        }),
                        None,
                        String::from("close current task"),
                        Some(format!(
                            "featureforge plan execution close-current-task --plan {plan_path} --task {task_number} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
                        )),
                    )
                } else {
                    (
                        String::from("task_closure_pending"),
                        String::from("task_review_dispatch_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("request task review"),
                        None,
                    )
                }
            } else if workflow_phase == "final_review_pending"
                && current_branch_closure_id.is_none()
            {
                (
                    String::from("document_release_pending"),
                    String::from("branch_closure_recording_required_for_release_readiness"),
                    String::from("missing_current_closure"),
                    None,
                    None,
                    String::from("advance late stage"),
                    Some(format!(
                        "featureforge plan execution advance-late-stage --plan {plan_path}"
                    )),
                )
            } else if workflow_phase == "final_review_pending"
                && current_release_readiness_result.as_deref() != Some("ready")
            {
                (
                    String::from("document_release_pending"),
                    String::from(
                        if current_release_readiness_result.as_deref() == Some("blocked") {
                            "release_blocker_resolution_required"
                        } else {
                            "release_readiness_recording_ready"
                        },
                    ),
                    String::from("clean"),
                    current_branch_closure_id.as_ref().map(|branch_closure_id| {
                        ExecutionRoutingRecordingContext {
                            task_number: None,
                            dispatch_id: None,
                            branch_closure_id: Some(branch_closure_id.clone()),
                        }
                    }),
                    None,
                    String::from(
                        if current_release_readiness_result.as_deref() == Some("blocked") {
                            "resolve release blocker"
                        } else {
                            "advance late stage"
                        },
                    ),
                    Some(format!(
                        "featureforge plan execution advance-late-stage --plan {plan_path} --result ready|blocked --summary-file <path>"
                    )),
                )
            } else if workflow_phase == "final_review_pending"
                && ((!external_review_result_ready
                    && (!final_review_dispatch_lineage_present
                        || final_review_dispatch_id.is_none()))
                    || !final_review_dispatch_still_current(
                        gate_review.as_ref(),
                        gate_finish.as_ref(),
                    ))
            {
                (
                    String::from("final_review_pending"),
                    String::from("final_review_dispatch_required"),
                    String::from("clean"),
                    None,
                    None,
                    String::from("request final review"),
                    None,
                )
            } else if workflow_phase == "final_review_pending" {
                let recommended_command = if external_review_result_ready {
                    Some(format!(
                        "featureforge plan execution advance-late-stage --plan {plan_path} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>"
                    ))
                } else {
                    None
                };
                (
                    String::from("final_review_pending"),
                    if external_review_result_ready {
                        String::from("final_review_recording_ready")
                    } else {
                        String::from("final_review_outcome_pending")
                    },
                    String::from("clean"),
                    if external_review_result_ready {
                        current_branch_closure_id.as_ref().map(|branch_closure_id| {
                            ExecutionRoutingRecordingContext {
                                task_number: None,
                                dispatch_id: final_review_dispatch_id.clone(),
                                branch_closure_id: Some(branch_closure_id.clone()),
                            }
                        })
                    } else {
                        None
                    },
                    None,
                    if external_review_result_ready {
                        String::from("advance late stage")
                    } else {
                        String::from("wait for external review result")
                    },
                    recommended_command,
                )
            } else if workflow_phase == "document_release_pending" {
                if status.review_state_status == "missing_current_closure" {
                    (
                        String::from("document_release_pending"),
                        String::from("branch_closure_recording_required_for_release_readiness"),
                        String::from("missing_current_closure"),
                        None,
                        None,
                        String::from("advance late stage"),
                        Some(format!(
                            "featureforge plan execution advance-late-stage --plan {plan_path}"
                        )),
                    )
                } else if let Some(branch_closure_id) = current_branch_closure_id.as_ref() {
                    (
                        String::from("document_release_pending"),
                        String::from(
                            if current_release_readiness_result.as_deref() == Some("blocked") {
                                "release_blocker_resolution_required"
                            } else {
                                "release_readiness_recording_ready"
                            },
                        ),
                        String::from("clean"),
                        Some(ExecutionRoutingRecordingContext {
                            task_number: None,
                            dispatch_id: None,
                            branch_closure_id: Some(branch_closure_id.clone()),
                        }),
                        None,
                        String::from(
                            if current_release_readiness_result.as_deref() == Some("blocked") {
                                "resolve release blocker"
                            } else {
                                "advance late stage"
                            },
                        ),
                        Some(format!(
                            "featureforge plan execution advance-late-stage --plan {plan_path} --result ready|blocked --summary-file <path>"
                        )),
                    )
                } else {
                    (
                        String::from("document_release_pending"),
                        String::from("branch_closure_recording_required_for_release_readiness"),
                        String::from("missing_current_closure"),
                        None,
                        None,
                        String::from("advance late stage"),
                        Some(format!(
                            "featureforge plan execution advance-late-stage --plan {plan_path}"
                        )),
                    )
                }
            } else if workflow_phase == "qa_pending" && current_branch_closure_id.is_none() {
                (
                    String::from("document_release_pending"),
                    String::from("branch_closure_recording_required_for_release_readiness"),
                    String::from("missing_current_closure"),
                    None,
                    None,
                    String::from("advance late stage"),
                    Some(format!(
                        "featureforge plan execution advance-late-stage --plan {plan_path}"
                    )),
                )
            } else if workflow_phase == "qa_pending" {
                match qa_requirement.as_deref() {
                    Some("required") if qa_pending_test_plan_refresh_required => (
                        String::from("qa_pending"),
                        String::from("test_plan_refresh_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("refresh test plan"),
                        None,
                    ),
                    Some("required") => (
                        String::from("qa_pending"),
                        String::from("qa_recording_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("run QA"),
                        Some(format!(
                            "featureforge plan execution advance-late-stage --plan {plan_path} --result pass|fail --summary-file <path>"
                        )),
                    ),
                    _ => (
                        String::from("pivot_required"),
                        String::from("planning_reentry_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("pivot / return to planning"),
                        Some(format!(
                            "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                        )),
                    ),
                }
            } else if workflow_phase == "ready_for_branch_completion" {
                match qa_requirement.as_deref() {
                    Some("required") | Some("not-required") => {
                        if finish_review_gate_pass_branch_closure_id
                            .as_ref()
                            .zip(current_branch_closure_id.as_ref())
                            .is_some_and(|(checkpoint, current)| checkpoint == current)
                            && gate_finish.as_ref().is_some_and(|gate| gate.allowed)
                        {
                            (
                                String::from("ready_for_branch_completion"),
                                String::from("finish_completion_gate_ready"),
                                String::from("clean"),
                                None,
                                None,
                                String::from("finish branch"),
                                None,
                            )
                        } else {
                            (
                                String::from("ready_for_branch_completion"),
                                String::from("finish_review_gate_ready"),
                                String::from("clean"),
                                None,
                                None,
                                String::from("finish branch"),
                                None,
                            )
                        }
                    }
                    _ => (
                        String::from("pivot_required"),
                        String::from("planning_reentry_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("pivot / return to planning"),
                        Some(format!(
                            "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                        )),
                    ),
                }
            } else {
                match workflow_phase.as_str() {
                    "executing" => {
                        let mut execution_command_context =
                            optional_execution_command_from_status(status);
                        let mut recommended_command = None;
                        if execution_command_context.is_some()
                            && let Ok((exact_command, exact_recommended)) =
                                required_execution_command_for_routing(
                                    current_dir,
                                    runtime_override,
                                    &plan_path,
                                    status,
                                    exact_plan_query,
                                    "workflow/operator could not derive the exact execution command for the current execution state.",
                                )
                        {
                            execution_command_context = Some(exact_command);
                            recommended_command = Some(exact_recommended);
                        }
                        let has_explicit_reentry_context = status.active_task.is_none()
                            && status.blocking_step.is_none()
                            && status.resume_task.is_none()
                            && execution_command_context.as_ref().is_some_and(|context| {
                                matches!(context.command_kind.as_str(), "begin" | "reopen")
                            });
                        let mut phase_detail = if has_explicit_reentry_context {
                            String::from("execution_reentry_required")
                        } else if shared_execution_state_has_open_steps(status)
                            || status.active_task.is_some()
                            || status.blocking_task.is_some()
                            || status.resume_task.is_some()
                        {
                            String::from("execution_in_progress")
                        } else {
                            String::from("execution_reentry_required")
                        };
                        let mut next_action = if phase_detail == "execution_in_progress" {
                            String::from("continue execution")
                        } else {
                            String::from("execution reentry required")
                        };
                        let marker_free_execution = status.active_task.is_none()
                            && status.blocking_task.is_none()
                            && status.resume_task.is_none()
                            && execution_command_context.is_none();
                        if marker_free_execution
                            && (status.execution_mode == "featureforge:executing-plans"
                                || (phase_detail == "execution_in_progress"
                                    && !shared_execution_state_has_open_steps(status)))
                        {
                            let exact_execution_command = required_execution_command_for_routing(
                                current_dir,
                                runtime_override,
                                &plan_path,
                                status,
                                exact_plan_query,
                                "workflow/operator could not derive the exact execution command for marker-free execution reentry.",
                            );
                            if require_exact_execution_command {
                                let (exact_command, exact_recommended) = exact_execution_command?;
                                phase_detail = String::from("execution_reentry_required");
                                next_action = String::from("execution reentry required");
                                execution_command_context = Some(exact_command);
                                recommended_command = Some(exact_recommended);
                            } else if let Ok((exact_command, exact_recommended)) =
                                exact_execution_command
                            {
                                phase_detail = String::from("execution_reentry_required");
                                next_action = String::from("execution reentry required");
                                execution_command_context = Some(exact_command);
                                recommended_command = Some(exact_recommended);
                            }
                        }
                        (
                            String::from("executing"),
                            phase_detail,
                            String::from("clean"),
                            None,
                            execution_command_context,
                            next_action,
                            recommended_command,
                        )
                    }
                    "repairing" => {
                        let exact_execution_command = required_execution_command_for_routing(
                            current_dir,
                            runtime_override,
                            &plan_path,
                            status,
                            exact_plan_query,
                            "workflow/operator could not derive the exact execution command for the current execution state.",
                        );
                        let (execution_command_context, recommended_command) =
                            if require_exact_execution_command {
                                let (execution_command_context, recommended_command) =
                                    exact_execution_command?;
                                (Some(execution_command_context), Some(recommended_command))
                            } else {
                                exact_execution_command.ok().map_or(
                                    (None, None),
                                    |(execution_command_context, recommended_command)| {
                                        (Some(execution_command_context), Some(recommended_command))
                                    },
                                )
                            };
                        (
                            String::from("executing"),
                            String::from("execution_reentry_required"),
                            String::from("clean"),
                            None,
                            execution_command_context,
                            String::from("execution reentry required"),
                            recommended_command,
                        )
                    }
                    "execution_preflight" | "implementation_handoff" => (
                        workflow_phase.clone(),
                        String::from("execution_in_progress"),
                        String::from("clean"),
                        None,
                        None,
                        next_action_for_context_like(
                            workflow_phase.as_str(),
                            gate_review.as_ref(),
                            gate_finish.as_ref(),
                        )
                        .replace('_', " "),
                        None,
                    ),
                    "handoff_required" => (
                        String::from("handoff_required"),
                        String::from("handoff_recording_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("hand off"),
                        Some(format!(
                            "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
                        )),
                    ),
                    "pivot_required" => (
                        String::from("pivot_required"),
                        String::from("planning_reentry_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("pivot / return to planning"),
                        Some(format!(
                            "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                        )),
                    ),
                    _ => (
                        String::from("pivot_required"),
                        String::from("planning_reentry_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("pivot / return to planning"),
                        Some(format!(
                            "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                        )),
                    ),
                }
            }
        }
    } else if let Some(repair_review_state_status) = late_stage_repair_review_state_status(
        workflow_phase.as_str(),
        execution_status.as_ref(),
        gate_review.as_ref(),
        gate_finish.as_ref(),
        current_branch_closure_id.as_deref(),
        current_release_readiness_result.as_deref(),
    ) {
        (
            String::from("executing"),
            String::from("execution_reentry_required"),
            String::from(repair_review_state_status),
            None,
            None,
            String::from("repair review state / reenter execution"),
            Some(format!(
                "featureforge plan execution repair-review-state --plan {plan_path}"
            )),
        )
    } else if !late_stage_prerequisite_reroute_active(
        workflow_phase.as_str(),
        qa_requirement.as_deref(),
        qa_pending_test_plan_refresh_required,
        current_branch_closure_id.as_deref(),
        current_release_readiness_result.as_deref(),
    ) && shared_negative_result_requires_execution_reentry(
        task_negative_result_task.is_some(),
        workflow_phase.as_str(),
        current_branch_closure_id.as_deref(),
        current_final_review_branch_closure_id.as_deref(),
        current_final_review_result.as_deref(),
        current_qa_branch_closure_id.as_deref(),
        current_qa_result.as_deref(),
    ) {
        (
            String::from("executing"),
            String::from("execution_reentry_required"),
            String::from("clean"),
            None,
            None,
            String::from("execution reentry required"),
            None,
        )
    } else {
        let (phase, phase_detail, next_action, recommended_command) = match workflow_phase.as_str()
        {
            "handoff_required" => (
                String::from("handoff_required"),
                String::from("handoff_recording_required"),
                String::from("hand off"),
                Some(format!(
                    "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
                )),
            ),
            _ => (
                String::from("pivot_required"),
                String::from("planning_reentry_required"),
                String::from("pivot / return to planning"),
                Some(format!(
                    "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                )),
            ),
        };
        (
            phase,
            phase_detail,
            String::from("clean"),
            None,
            None,
            next_action,
            recommended_command,
        )
    };

    let decision = canonicalize_routing_decision(CanonicalRoutingInputs {
        external_review_result_ready,
        plan_path: &plan_path,
        status: execution_status.as_ref(),
        persisted_repair_review_state_follow_up: persisted_repair_review_state_follow_up.as_deref(),
        repair_review_state_follow_up: repair_review_state_follow_up.as_deref(),
        seed: WorkflowRoutingDecision {
            phase,
            phase_detail,
            review_state_status,
            recording_context,
            execution_command_context,
            next_action,
            recommended_command,
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
        },
    })?;

    Ok(ExecutionRoutingState {
        route,
        execution_status,
        preflight,
        gate_review,
        gate_finish,
        workflow_phase,
        phase: decision.phase,
        phase_detail: decision.phase_detail,
        review_state_status: decision.review_state_status,
        qa_requirement,
        follow_up_override,
        finish_review_gate_pass_branch_closure_id,
        recording_context: decision.recording_context,
        execution_command_context: decision.execution_command_context,
        next_action: decision.next_action,
        recommended_command: decision.recommended_command,
        blocking_scope: decision.blocking_scope,
        blocking_task: decision.blocking_task,
        external_wait_state: decision.external_wait_state,
        blocking_reason_codes: decision.blocking_reason_codes,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id,
        current_release_readiness_result,
        base_branch,
    })
}

struct CanonicalRoutingInputs<'a> {
    external_review_result_ready: bool,
    plan_path: &'a str,
    status: Option<&'a PlanExecutionStatus>,
    persisted_repair_review_state_follow_up: Option<&'a str>,
    repair_review_state_follow_up: Option<&'a str>,
    seed: WorkflowRoutingDecision,
}

fn canonicalize_routing_decision(
    inputs: CanonicalRoutingInputs<'_>,
) -> Result<WorkflowRoutingDecision, JsonFailure> {
    let CanonicalRoutingInputs {
        external_review_result_ready,
        plan_path,
        status,
        persisted_repair_review_state_follow_up,
        repair_review_state_follow_up,
        mut seed,
    } = inputs;

    if status.is_some_and(|status| status.execution_started == "yes")
        && status
            .and_then(|status| status.current_branch_closure_id.as_deref())
            .is_some()
        && persisted_repair_review_state_follow_up == Some("record_branch_closure")
        && repair_review_state_follow_up == Some("record_branch_closure")
    {
        seed.phase = String::from("document_release_pending");
        seed.phase_detail = String::from("branch_closure_recording_required_for_release_readiness");
        seed.review_state_status = String::from("missing_current_closure");
        seed.recording_context = None;
        seed.execution_command_context = None;
        seed.next_action = String::from("advance late stage");
        seed.recommended_command = Some(format!(
            "featureforge plan execution advance-late-stage --plan {plan_path}"
        ));
    }

    if seed.blocking_task.is_none() {
        seed.blocking_task = status.and_then(|status| status.blocking_task);
    }
    if seed.blocking_scope.is_none() {
        seed.blocking_scope = blocking_scope_for_phase_detail(
            &seed.phase_detail,
            seed.blocking_task,
            status,
            seed.review_state_status.as_str(),
        );
    }
    seed.external_wait_state =
        external_wait_state_for_phase_detail(&seed.phase_detail, external_review_result_ready);
    if seed.blocking_reason_codes.is_empty() {
        seed.blocking_reason_codes = compact_operator_reason_codes(
            status,
            seed.phase_detail.as_str(),
            seed.review_state_status.as_str(),
        );
    }

    seed = normalize_intent_level_routing_decision(seed, plan_path, status);

    Ok(seed)
}

fn normalize_intent_level_routing_decision(
    mut decision: WorkflowRoutingDecision,
    plan_path: &str,
    _status: Option<&PlanExecutionStatus>,
) -> WorkflowRoutingDecision {
    let task_number = decision
        .recording_context
        .as_ref()
        .and_then(|context| context.task_number)
        .or(decision.blocking_task);
    let dispatch_id = decision
        .recording_context
        .as_ref()
        .and_then(|context| context.dispatch_id.as_deref());
    let intent_template = intent_level_command_template(IntentLevelCommandTemplateInputs {
        phase_detail: decision.phase_detail.as_str(),
        plan_path,
        task_number,
        dispatch_id,
    });
    if let Some(next_action) = intent_template.next_action {
        decision.next_action = next_action;
    }
    if let Some(recommended_command) = intent_template.recommended_command {
        decision.recommended_command = recommended_command;
    }
    decision
}

fn blocking_scope_for_phase_detail(
    phase_detail: &str,
    blocking_task: Option<u32>,
    status: Option<&PlanExecutionStatus>,
    review_state_status: &str,
) -> Option<String> {
    let scope = match phase_detail {
        "task_review_dispatch_required"
        | "task_review_result_pending"
        | "task_closure_recording_ready" => Some("task"),
        "branch_closure_recording_required_for_release_readiness"
        | "release_readiness_recording_ready"
        | "release_blocker_resolution_required"
        | "final_review_dispatch_required"
        | "final_review_outcome_pending"
        | "final_review_recording_ready"
        | "qa_recording_required"
        | "test_plan_refresh_required"
        | "finish_review_gate_ready"
        | "finish_completion_gate_ready" => Some("branch"),
        "planning_reentry_required" | "handoff_recording_required" => Some("workflow"),
        "execution_reentry_required" => {
            if blocking_task.is_some() {
                Some("task")
            } else if matches!(
                review_state_status,
                "missing_current_closure" | "stale_unreviewed"
            ) || status.is_some_and(|status| {
                shared_branch_closure_refresh_missing_current_closure(status)
                    || current_branch_closure_structural_review_state_reason(status).is_some()
            }) {
                Some("branch")
            } else {
                Some("workflow")
            }
        }
        _ => None,
    };
    scope.map(str::to_owned)
}

fn external_wait_state_for_phase_detail(
    phase_detail: &str,
    external_review_result_ready: bool,
) -> Option<String> {
    if external_review_result_ready {
        return None;
    }
    match phase_detail {
        "task_review_result_pending" | "final_review_outcome_pending" => {
            Some(String::from("waiting_for_external_review_result"))
        }
        _ => None,
    }
}

fn compact_operator_reason_codes(
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
            "task_review_dispatch_required" | "task_closure_recording_ready"
        ) {
            if status
                .reason_codes
                .iter()
                .any(|code| code == "prior_task_review_dispatch_missing")
            {
                push_unique_reason(&mut reason_codes, "prior_task_review_dispatch_missing");
            }
            if status
                .reason_codes
                .iter()
                .any(|code| code == "prior_task_review_dispatch_stale")
            {
                push_unique_reason(&mut reason_codes, "prior_task_review_dispatch_stale");
            }
            push_unique_reason(&mut reason_codes, "task_review_dispatch_required");
        }
        if phase_detail == "final_review_dispatch_required" {
            push_unique_reason(&mut reason_codes, "final_review_dispatch_required");
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

fn review_state_is_stale_unreviewed(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    if status.execution_started != "yes" || shared_execution_state_has_open_steps(status) {
        return false;
    }

    let gate_review = gate_review_from_context(context);
    let gate_finish = gate_finish_from_context(context);
    shared_public_late_stage_stale_unreviewed(status, Some(&gate_review), Some(&gate_finish))
}

fn status_has_accepted_preflight(status: &PlanExecutionStatus) -> bool {
    status
        .execution_run_id
        .as_ref()
        .is_some_and(|run_id| !run_id.as_str().trim().is_empty())
        || status.harness_phase == HarnessPhase::ExecutionPreflight
}

fn explicit_plan_execution_route_required(status: Option<&PlanExecutionStatus>) -> bool {
    let Some(status) = status else {
        return false;
    };
    if status.execution_started != "yes" {
        return false;
    }
    let pre_late_stage_missing_branch_closure_only =
        matches!(
            status.harness_phase,
            HarnessPhase::DocumentReleasePending
                | HarnessPhase::FinalReviewPending
                | HarnessPhase::QaPending
                | HarnessPhase::ReadyForBranchCompletion
        ) && status.current_branch_closure_id.is_none()
            && status.current_release_readiness_state.is_none()
            && status.current_final_review_result.is_none()
            && status.current_qa_result.is_none();
    if pre_late_stage_missing_branch_closure_only {
        return false;
    }
    let pre_late_stage_execution_terminal_only = status.harness_phase == HarnessPhase::Executing
        && !shared_execution_state_has_open_steps(status)
        && status.active_task.is_none()
        && status.blocking_task.is_none()
        && status.resume_task.is_none()
        && status.current_branch_closure_id.is_none()
        && status.current_release_readiness_state.is_none()
        && status.current_final_review_result.is_none()
        && status.current_qa_result.is_none()
        && status.review_state_status == "clean";
    if pre_late_stage_execution_terminal_only {
        return false;
    }
    if shared_execution_state_has_open_steps(status)
        || status.active_task.is_some()
        || status.blocking_task.is_some()
        || status.resume_task.is_some()
        || status.review_state_status != "clean"
    {
        return true;
    }
    status.harness_phase != HarnessPhase::PivotRequired
}

fn derive_phase(
    route_status: &str,
    execution_status: Option<&PlanExecutionStatus>,
    preflight: Option<&GateResult>,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> String {
    if route_status != "implementation_ready" {
        return match route_status {
            "spec_draft" => String::from("spec_review"),
            "plan_draft" => String::from("plan_review"),
            "spec_approved_needs_plan" | "stale_plan" => String::from("plan_writing"),
            other => other.to_owned(),
        };
    }

    let Some(execution_status) = execution_status else {
        return String::from("implementation_handoff");
    };

    if let Some(authoritative_phase) = authoritative_public_phase(execution_status) {
        return authoritative_phase.to_owned();
    }

    if execution_status.execution_started != "yes" {
        if status_has_accepted_preflight(execution_status)
            || preflight.map(|result| result.allowed).unwrap_or(false)
        {
            if execution_status.execution_mode == "featureforge:executing-plans" {
                return String::from("executing");
            }
            return String::from("execution_preflight");
        }
        return String::from("implementation_handoff");
    }

    if execution_reentry_requires_review_state_repair(execution_status) {
        return String::from("executing");
    }

    if shared_task_boundary_block_reason_code(execution_status).is_some() {
        return String::from("task_closure_pending");
    }

    if shared_execution_state_has_open_steps(execution_status) {
        return String::from("executing");
    }

    if execution_status.review_state_status == "missing_current_closure" {
        return String::from("document_release_pending");
    }

    let Some(gate_finish) = gate_finish else {
        return String::from("final_review_pending");
    };

    if shared_qa_requirement_policy_invalid(Some(gate_finish)) {
        return String::from("pivot_required");
    }

    if gate_finish.allowed && gate_review.is_some_and(|gate| gate.allowed) {
        return String::from("ready_for_branch_completion");
    }
    if gate_finish
        .reason_codes
        .iter()
        .any(|code| code == "finish_review_gate_checkpoint_missing")
    {
        return String::from("ready_for_branch_completion");
    }

    let release_blocked = shared_late_stage_release_blocked(Some(gate_finish))
        || shared_late_stage_release_truth_blocked(gate_review);
    let review_blocked = shared_late_stage_review_truth_blocked(gate_review)
        || shared_late_stage_review_blocked(Some(gate_finish));
    let qa_blocked = shared_late_stage_qa_blocked(Some(gate_finish));

    if !(gate_finish.allowed || release_blocked || review_blocked || qa_blocked) {
        return String::from("final_review_pending");
    }

    let decision = resolve_late_stage_precedence(LateStageSignals {
        release: GateState::from_blocked(release_blocked),
        review: GateState::from_blocked(review_blocked),
        qa: GateState::from_blocked(qa_blocked),
    });
    decision.phase.to_owned()
}

fn late_stage_observability_for_phase(
    phase: &str,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> (String, Vec<String>) {
    if !matches!(
        phase,
        "document_release_pending"
            | "final_review_pending"
            | "qa_pending"
            | "ready_for_branch_completion"
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
        "document_release_pending"
            | "final_review_pending"
            | "qa_pending"
            | "ready_for_branch_completion"
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

fn late_stage_prerequisite_reroute_active(
    workflow_phase: &str,
    qa_requirement: Option<&str>,
    qa_pending_test_plan_refresh_required: bool,
    current_branch_closure_id: Option<&str>,
    current_release_readiness_result: Option<&str>,
) -> bool {
    match workflow_phase {
        "document_release_pending" => true,
        "final_review_pending" => {
            current_branch_closure_id.is_none() || current_release_readiness_result != Some("ready")
        }
        "qa_pending" => {
            current_branch_closure_id.is_none()
                || (qa_requirement == Some("required") && qa_pending_test_plan_refresh_required)
        }
        _ => false,
    }
}

fn explicit_route_for_plan_override(
    workflow: &WorkflowRuntime,
    resolved_route: &WorkflowRoute,
    plan_override: &std::path::Path,
) -> Result<WorkflowRoute, JsonFailure> {
    resolve_explicit_plan_override(workflow, resolved_route, plan_override)
        .map_err(JsonFailure::from)
}

fn authoritative_public_phase(status: &PlanExecutionStatus) -> Option<&'static str> {
    if status.latest_authoritative_sequence == INITIAL_AUTHORITATIVE_SEQUENCE {
        return None;
    }

    match status.harness_phase {
        HarnessPhase::Repairing | HarnessPhase::Executing => {
            (shared_execution_state_has_open_steps(status)
                || !late_stage_rederivation_basis_present(status))
            .then_some(HarnessPhase::Executing.as_str())
        }
        HarnessPhase::FinalReviewPending
        | HarnessPhase::QaPending
        | HarnessPhase::DocumentReleasePending
        | HarnessPhase::ReadyForBranchCompletion => None,
        _ => Some(status.harness_phase.as_str()),
    }
}

fn task_scope_review_state_is_stale_unreviewed(status: &PlanExecutionStatus) -> bool {
    shared_task_scope_stale_review_state_reason_present(task_scope_review_state_repair_reason(
        status,
    ))
}

fn optional_execution_command_from_status(
    status: &PlanExecutionStatus,
) -> Option<ExecutionRoutingExecutionCommandContext> {
    status
        .execution_command_context
        .as_ref()
        .cloned()
        .map(|context| ExecutionRoutingExecutionCommandContext {
            command_kind: context.command_kind,
            task_number: context.task_number,
            step_id: context.step_id,
        })
}

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
    let resolved = resolve_exact_execution_command_from_context(&context, status, plan_path)
        .ok_or_else(|| JsonFailure::new(FailureClass::MalformedExecutionState, message))?;
    Ok((
        ExecutionRoutingExecutionCommandContext {
            command_kind: String::from(resolved.command_kind),
            task_number: Some(resolved.task_number),
            step_id: resolved.step_id,
        },
        resolved.recommended_command,
    ))
}

fn next_action_for_context_like(
    phase: &str,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> &'static str {
    if phase == "final_review_pending" && gate_review.is_some_and(|gate| !gate.allowed) {
        "return_to_execution"
    } else if phase == "qa_pending" && finish_requires_test_plan_refresh(gate_finish) {
        "refresh_test_plan"
    } else {
        next_action_for_phase(phase)
    }
}

fn next_action_for_phase(phase: &str) -> &'static str {
    match phase {
        "needs_brainstorming"
        | "brainstorming"
        | "spec_review"
        | "plan_writing"
        | "plan_review"
        | "plan_update"
        | "workflow_unresolved" => "use_next_skill",
        "implementation_handoff" | "execution_preflight" => "execution_preflight",
        "executing"
        | "contract_drafting"
        | "contract_pending_approval"
        | "contract_approved"
        | "evaluating"
        | "repairing"
        | "handoff_required" => "return_to_execution",
        "pivot_required" => "plan_update",
        "final_review_pending" => "request_code_review",
        "qa_pending" => "run_qa_only",
        "document_release_pending" => "run_document_release",
        "ready_for_branch_completion" => "finish_branch",
        _ => "inspect_workflow",
    }
}

#[cfg(test)]
mod routing_helper_tests {
    use super::*;
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

    #[test]
    fn task_review_result_pending_task_accepts_boundary_repair_reason_codes() {
        let status = unresolved_status();
        for reason_code in [
            "prior_task_review_not_green",
            "task_review_not_independent",
            "task_review_receipt_malformed",
            "prior_task_verification_missing",
            "prior_task_verification_missing_legacy",
            "task_verification_receipt_malformed",
        ] {
            let mut mutated = status.clone();
            mutated.reason_codes = vec![reason_code.to_owned()];
            assert_eq!(
                task_review_result_pending_task(&mutated, Some("dispatch-task-1")),
                Some(1),
                "{reason_code} should keep task-boundary routing in the task review result lane"
            );
        }
    }

    #[test]
    fn late_stage_repair_review_state_required_overrides_missing_release_readiness_when_stale() {
        let mut status = unresolved_status();
        status.review_state_status = String::from("stale_unreviewed");

        assert!(
            late_stage_repair_review_state_status(
                "final_review_pending",
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
                "final_review_pending",
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
            workflow_phase: Some("pivot_required"),
            harness_phase: None,
            handoff_required: false,
            handoff_decision_scope: None,
            reason_codes: &reason_codes,
            qa_requirement: Some("required"),
        });
        fs::remove_file(&artifact_path).expect("decoy pivot artifact should clean up");

        assert!(
            follow_up == "record_pivot",
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
