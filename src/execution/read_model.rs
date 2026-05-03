use std::collections::BTreeSet;
use std::path::Path;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::closure_diagnostics::task_closure_recording_status_reason_codes;
use crate::execution::closure_dispatch::{
    current_final_review_dispatch_id_from_authority, current_task_review_dispatch_id_for_task,
};
use crate::execution::closure_graph::{AuthoritativeClosureGraph, ClosureGraphSignals};
#[cfg(test)]
use crate::execution::context::EvidenceAttempt;
use crate::execution::context::{
    ExecutionContext, NoteState, clear_projection_only_execution_progress,
    has_other_same_branch_worktree, hash_contract_plan, load_execution_context,
    load_execution_context_for_exact_plan, load_execution_context_for_mutation,
    overlay_execution_evidence_attempts_from_authority, overlay_step_state_from_authority,
    refresh_execution_fingerprint, same_branch_worktrees,
};
use crate::execution::current_closure_projection::{
    project_current_task_closure_repair_reason_codes, project_current_task_closures,
    still_current_task_closure_records,
    still_current_task_closure_records_from_authoritative_state,
    structural_current_task_closure_failures,
};
#[cfg(test)]
use crate::execution::current_truth::task_review_result_pending_task;
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason, ReviewStateRepairReroute,
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    branch_closure_rerecording_assessment,
    branch_source_task_closure_ids as shared_branch_source_task_closure_ids,
    current_branch_closure_has_tracked_drift as shared_current_branch_closure_has_tracked_drift,
    current_late_stage_branch_bindings as shared_current_late_stage_branch_bindings,
    current_task_negative_result_task as shared_current_task_negative_result_task,
    final_review_dispatch_still_current as shared_final_review_dispatch_still_current,
    late_stage_missing_current_closure_stale_provenance_present as shared_late_stage_missing_current_closure_stale_provenance_present,
    late_stage_qa_blocked as shared_late_stage_qa_blocked,
    late_stage_release_blocked as shared_late_stage_release_blocked,
    late_stage_review_blocked as shared_late_stage_review_blocked,
    late_stage_review_truth_blocked as shared_late_stage_review_truth_blocked,
    live_review_state_repair_reroute as shared_live_review_state_repair_reroute,
    live_review_state_status_for_reroute as shared_live_review_state_status_for_reroute,
    live_task_scope_repair_precedence_active as shared_live_task_scope_repair_precedence_active,
    negative_result_requires_execution_reentry as shared_negative_result_requires_execution_reentry,
    normalized_late_stage_surface,
    normalized_plan_qa_requirement as shared_normalized_plan_qa_requirement,
    parse_late_stage_surface_only_branch_surface, path_matches_late_stage_surface,
    public_late_stage_rederivation_basis_present,
    public_late_stage_stale_unreviewed as shared_public_late_stage_stale_unreviewed,
    public_review_state_stale_unreviewed_for_reroute as shared_public_review_state_stale_unreviewed_for_reroute,
    qa_requirement_policy_invalid as shared_qa_requirement_policy_invalid,
    release_readiness_result_for_branch_closure as shared_release_readiness_result_for_branch_closure,
    resolve_actionable_repair_follow_up_for_status, task_closure_contributes_to_branch_surface,
    task_scope_overlay_restore_required as shared_task_scope_overlay_restore_required,
    task_scope_stale_review_state_reason_present as shared_task_scope_stale_review_state_reason_present,
};
use crate::execution::fields::FIELD_HANDOFF_REQUIRED;
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, DownstreamFreshnessState, EvaluationVerdict, EvaluatorKind,
    ExecutionRunId, HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE,
};
#[cfg(test)]
use crate::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use crate::execution::leases::{
    StatusAuthoritativeOverlay, authoritative_state_path, load_status_authoritative_overlay_checked,
};
#[cfg(test)]
use crate::execution::next_action::{NextActionDecision, NextActionKind, public_next_action_text};
use crate::execution::next_action::{
    compute_next_action_decision, execution_command_route_target_from_decision,
    execution_reentry_target, select_authoritative_stale_reentry_target,
};
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::phase;
use crate::execution::projection_renderer::{
    execution_projection_read_model_metadata, normal_projection_write_mode,
};
use crate::execution::query::ExecutionRoutingState;
use crate::execution::read_model_support::{
    active_step, authoritative_execution_run_id_from_state,
    context_all_task_scopes_closed_by_authority, current_execution_run_id_with_authority,
    execution_started, latest_attempted_step_for_task, prior_task_number_for_begin,
    projected_earliest_stale_task_from_status, qa_pending_requires_test_plan_refresh,
    require_prior_task_closure_for_begin, resolve_branch_closure_reviewed_tree_sha,
    stale_unreviewed_allows_task_closure_baseline_bridge, task_boundary_reason_code_from_message,
    task_closure_baseline_repair_candidate_with_stale_target, task_closure_recording_prerequisites,
};
use crate::execution::reducer::RuntimeState;
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_MISSING_AUTHORITY_CODE, TARGETLESS_STALE_RECONCILE_REASON_CODE,
    TargetlessStaleReconcile,
    task_closure_baseline_repair_candidate_reason_present as shared_task_closure_baseline_repair_candidate_reason_present,
};
use crate::execution::router::{RouteDecision, route_decision_with_status_blockers};
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::semantic_identity::{
    branch_definition_identity_for_context, semantic_workspace_snapshot,
    task_definition_identity_for_task,
};
use crate::execution::stale_target_projection::project_stale_unreviewed_closures;
#[cfg(test)]
use crate::execution::state::record_review_dispatch_blocked_output_from_gate;
#[cfg(test)]
use crate::execution::status::PublicReviewStateTaskClosure;
use crate::execution::status::{
    GateProjectionInputs, GateResult, GateState, PlanExecutionStatus,
    PublicExecutionCommandContext, PublicRecordingContext, StatusBlockingRecord,
};
use crate::execution::topology::{pending_chunk_id, preflight_acceptance_for_context};
use crate::execution::transitions::{
    AuthoritativeTransitionState, PersistedReviewStateFieldClass, classify_review_state_field,
    load_authoritative_transition_state, load_authoritative_transition_state_relaxed,
};
use crate::workflow::late_stage_precedence::{
    GateState as PrecedenceGateState, LateStageSignals, resolve as resolve_late_stage_precedence,
};
#[cfg(test)]
use crate::workflow::pivot::{
    WorkflowPivotRecordIdentity, current_workflow_pivot_record_exists, pivot_decision_reason_codes,
};

mod late_stage;
mod public_route_projection;
mod review_state;
mod task_state;

use late_stage::authoritative_late_stage_rederivation_basis_present;
pub(crate) use late_stage::{
    apply_late_stage_precedence_status_overlay, has_authoritative_late_stage_progress,
};
pub(crate) use public_route_projection::{
    apply_shared_routing_projection_to_read_scope,
    apply_shared_routing_projection_to_read_scope_with_routing,
};
pub(crate) use review_state::{
    FinalReviewDispatchAuthority, current_final_review_dispatch_authority_for_context,
    current_task_review_dispatch_id_for_status, shared_repair_review_state_reroute_decision,
};
use task_state::completed_plan_missing_current_closure_task;
pub(crate) use task_state::{
    ExecutionCommandRouteTarget, ExecutionReentryCurrentTaskClosureTargets,
    apply_task_boundary_status_overlay,
    execution_reentry_current_task_closure_targets_from_stale_tasks, recommended_execution_source,
    reopen_execution_command_route_target_for_task, require_public_execution_command_route_target,
    resolve_execution_command_route_target,
};

#[cfg(test)]
pub(crate) fn resolve_execution_command_route_target_from_context(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<ExecutionCommandRouteTarget> {
    task_state::resolve_execution_command_route_target_from_context(context, status, plan_path)
}

pub(crate) struct ExecutionReadScope {
    pub(crate) context: ExecutionContext,
    pub(crate) status: PlanExecutionStatus,
    pub(crate) overlay: Option<StatusAuthoritativeOverlay>,
    pub(crate) authoritative_state: Option<AuthoritativeTransitionState>,
    pub(crate) runtime_state: Option<RuntimeState>,
}

pub(crate) struct ExecutionDerivedTruth {
    pub(crate) status: PlanExecutionStatus,
    pub(crate) overlay: Option<StatusAuthoritativeOverlay>,
    pub(crate) task_review_dispatch_id: Option<String>,
    pub(crate) final_review_dispatch_authority: FinalReviewDispatchAuthority,
}

pub(crate) struct SharedRepairReviewStateRerouteDecision {
    pub(crate) branch_reroute_still_valid: bool,
    pub(crate) branch_drift_escapes_late_stage_surface: bool,
    pub(crate) late_stage_surface_not_declared: bool,
    pub(crate) persisted_repair_follow_up: Option<String>,
    pub(crate) raw_late_stage_review_state_status: Option<&'static str>,
    pub(crate) task_scope_repair_precedence_active: bool,
    pub(crate) repair_reroute: ReviewStateRepairReroute,
}

pub(crate) fn status_from_context_with_shared_routing(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut read_scope =
        load_execution_read_scope_for_mutation(runtime, Path::new(&context.plan_rel), true)?;
    apply_shared_routing_projection_to_read_scope(
        runtime,
        &mut read_scope,
        external_review_result_ready,
        true,
    )?;
    Ok(read_scope.status)
}

pub(crate) fn apply_public_read_invariants_to_status(status: &mut PlanExecutionStatus) {
    crate::execution::invariants::inject_read_surface_invariant_test_violation(status);
    crate::execution::invariants::apply_read_surface_invariants(status);
}

pub(crate) fn public_status_from_context_with_shared_routing(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut status =
        status_from_context_with_shared_routing(runtime, context, external_review_result_ready)?;
    apply_public_read_invariants_to_status(&mut status);
    Ok(status)
}

pub(crate) fn public_status_from_supplied_context_with_shared_routing(
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut context = context.clone();
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    overlay_execution_evidence_attempts_from_authority(&mut context, authoritative_state.as_ref())?;
    overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    refresh_execution_fingerprint(&mut context);
    let derived = derive_execution_truth_from_authority(&context, authoritative_state.as_ref())?;
    let mut read_scope = ExecutionReadScope {
        context,
        status: derived.status,
        overlay: derived.overlay,
        authoritative_state,
        runtime_state: None,
    };
    apply_shared_routing_projection_to_read_scope_with_routing(
        &mut read_scope,
        external_review_result_ready,
        true,
    )?;
    apply_public_read_invariants_to_status(&mut read_scope.status);
    Ok(read_scope.status)
}

pub(crate) fn derive_execution_truth_from_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<ExecutionDerivedTruth, JsonFailure> {
    derive_execution_truth_from_authority_with_gates(context, authoritative_state, None)
}

pub(crate) fn derive_execution_truth_from_authority_with_gates(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<ExecutionDerivedTruth, JsonFailure> {
    let overlay = status_overlay_from_authoritative_snapshot(context, authoritative_state)?;
    let status = status_from_context_with_overlay(
        context,
        overlay.as_ref(),
        true,
        authoritative_state,
        true,
        gate_projection,
    )?;
    let task_review_dispatch_id =
        current_task_review_dispatch_id_for_status(context, &status, overlay.as_ref());
    let final_review_dispatch_authority = current_final_review_dispatch_authority_for_context(
        context,
        overlay.as_ref(),
        authoritative_state,
    );
    Ok(ExecutionDerivedTruth {
        status,
        overlay,
        task_review_dispatch_id,
        final_review_dispatch_authority,
    })
}

pub(crate) fn load_execution_read_scope(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionReadScope, JsonFailure> {
    let context = load_execution_read_context(runtime, plan_path, exact_plan_override)?;
    finalize_execution_read_scope(runtime, exact_plan_override, context)
}

pub(crate) fn load_execution_read_scope_for_mutation(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionReadScope, JsonFailure> {
    let context = load_execution_context_for_mutation(runtime, plan_path)?;
    finalize_execution_read_scope(runtime, exact_plan_override, context)
}

fn finalize_execution_read_scope(
    runtime: &ExecutionRuntime,
    exact_plan_override: bool,
    mut context: ExecutionContext,
) -> Result<ExecutionReadScope, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    overlay_execution_evidence_attempts_from_authority(&mut context, authoritative_state.as_ref())?;
    overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    refresh_execution_fingerprint(&mut context);
    let derived = derive_execution_truth_from_authority(&context, authoritative_state.as_ref())?;
    let overlay = derived.overlay;
    let mut status = derived.status;
    let local_contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let local_evidence_progress_present = context.evidence.tracked_progress_present;
    let local_projection_only_execution_started =
        status.execution_started == "yes" && !context.local_execution_progress_markers_present;
    let local_has_other_same_branch_worktree = has_other_same_branch_worktree(runtime);
    let local_started_execution = status.execution_started == "yes";
    let local_probe = LocalSameBranchReadScopeProbe {
        plan_rel: &context.plan_rel,
        contract_plan_fingerprint: &local_contract_plan_fingerprint,
        evidence_progress_present: local_evidence_progress_present,
        projection_only_execution_started: local_projection_only_execution_started,
        started_execution: local_started_execution,
        semantic_workspace_state_id: &status_workspace_state_id(&context)?,
    };
    let read_scope = if let Some(adopted_scope) =
        started_execution_read_scope_from_same_branch_worktree(
            runtime,
            local_probe,
            exact_plan_override,
        )? {
        adopted_scope
    } else {
        if local_started_execution
            && local_projection_only_execution_started
            && local_has_other_same_branch_worktree
        {
            clear_projection_only_execution_progress(&mut context);
            refresh_execution_fingerprint(&mut context);
            status = derive_execution_truth_from_authority(&context, None)?.status;
            normalize_non_started_same_branch_status(&mut status);
            return Ok(ExecutionReadScope {
                context,
                status,
                overlay: None,
                authoritative_state: None,
                runtime_state: None,
            });
        }
        if local_has_other_same_branch_worktree {
            normalize_non_started_same_branch_status(&mut status);
        }
        ExecutionReadScope {
            context,
            status,
            overlay,
            authoritative_state,
            runtime_state: None,
        }
    };
    Ok(read_scope)
}

fn normalize_non_started_same_branch_status(status: &mut PlanExecutionStatus) {
    if status.execution_started == "yes"
        && status.phase_detail == phase::DETAIL_EXECUTION_IN_PROGRESS
    {
        status.execution_started = String::from("no");
        status.active_task = None;
        status.active_step = None;
        status.resume_task = None;
        status.resume_step = None;
    } else if status.execution_started != "no"
        || status.phase_detail != phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    {
        return;
    }
    status.phase = Some(String::from(phase::PHASE_EXECUTION_PREFLIGHT));
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED);
    status.next_action = String::from("execution preflight");
    status.recommended_command = None;
    status.recording_context = None;
    status.execution_command_context = None;
    status.blocking_scope = None;
    status.blocking_task = None;
    status.blocking_reason_codes.clear();
}

pub(crate) fn status_overlay_from_authoritative_snapshot(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Option<StatusAuthoritativeOverlay>, JsonFailure> {
    authoritative_state
        .map(|state| {
            serde_json::from_value(status_overlay_payload_from_authoritative_snapshot(
                &state.state_payload_snapshot(),
            ))
            .map_err(|error| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Authoritative harness state is malformed in {}: {error}",
                        authoritative_state_path(context).display()
                    ),
                )
            })
        })
        .transpose()
}

fn status_overlay_payload_from_authoritative_snapshot(
    snapshot: &serde_json::Value,
) -> serde_json::Value {
    let Some(source) = snapshot.as_object() else {
        return serde_json::Value::Object(serde_json::Map::new());
    };
    let mut overlay = serde_json::Map::new();
    for field in [
        "harness_phase",
        "chunk_id",
        "latest_authoritative_sequence",
        "authoritative_sequence",
        "active_contract_path",
        "active_contract_fingerprint",
        "required_evaluator_kinds",
        "completed_evaluator_kinds",
        "pending_evaluator_kinds",
        "non_passing_evaluator_kinds",
        "aggregate_evaluation_state",
        "last_evaluation_report_path",
        "last_evaluation_report_fingerprint",
        "last_evaluation_evaluator_kind",
        "last_evaluation_verdict",
        "current_chunk_retry_count",
        "current_chunk_retry_budget",
        "current_chunk_pivot_threshold",
        FIELD_HANDOFF_REQUIRED,
        "open_failed_criteria",
        "write_authority_state",
        "write_authority_holder",
        "write_authority_worktree",
        "repo_state_baseline_head_sha",
        "repo_state_baseline_worktree_fingerprint",
        "repo_state_drift_state",
        "dependency_index_state",
        "final_review_state",
        "browser_qa_state",
        "release_docs_state",
        "last_final_review_artifact_fingerprint",
        "last_browser_qa_artifact_fingerprint",
        "last_release_docs_artifact_fingerprint",
        "strategy_state",
        "last_strategy_checkpoint_fingerprint",
        "strategy_checkpoint_kind",
        "strategy_cycle_break_task",
        "strategy_cycle_break_step",
        "strategy_cycle_break_checkpoint_fingerprint",
        "strategy_reset_required",
        "strategy_review_dispatch_lineage",
        "final_review_dispatch_lineage",
        "current_branch_closure_id",
        "current_branch_closure_reviewed_state_id",
        "current_branch_closure_contract_identity",
        "current_release_readiness_result",
        "reason_codes",
    ] {
        if let Some(value) = source.get(field)
            && !value.is_null()
        {
            overlay.insert(field.to_owned(), value.clone());
        }
    }
    serde_json::Value::Object(overlay)
}

fn load_execution_read_context(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionContext, JsonFailure> {
    if exact_plan_override {
        load_execution_context_for_exact_plan(runtime, plan_path)
    } else {
        load_execution_context(runtime, plan_path)
    }
}

struct LocalSameBranchReadScopeProbe<'a> {
    plan_rel: &'a str,
    contract_plan_fingerprint: &'a str,
    evidence_progress_present: bool,
    projection_only_execution_started: bool,
    started_execution: bool,
    semantic_workspace_state_id: &'a str,
}

fn started_execution_read_scope_from_same_branch_worktree(
    current_runtime: &ExecutionRuntime,
    local_probe: LocalSameBranchReadScopeProbe<'_>,
    exact_plan_override: bool,
) -> Result<Option<ExecutionReadScope>, JsonFailure> {
    if local_probe.started_execution && !local_probe.projection_only_execution_started {
        return Ok(None);
    }
    if local_probe.evidence_progress_present {
        return Ok(None);
    }
    let relative_plan = Path::new(local_probe.plan_rel);
    Ok(same_branch_worktrees(&current_runtime.repo_root)
        .into_iter()
        .filter(|root| root != &current_runtime.repo_root)
        .find_map(|worktree_root| {
            let discovered_runtime = ExecutionRuntime::discover(&worktree_root).ok()?;
            if current_runtime.branch_name == "current"
                || discovered_runtime.branch_name == "current"
                || discovered_runtime.branch_name != current_runtime.branch_name
            {
                return None;
            }
            let runtime = ExecutionRuntime {
                state_dir: current_runtime.state_dir.clone(),
                ..discovered_runtime
            };
            let mut context =
                load_execution_read_context(&runtime, relative_plan, exact_plan_override).ok()?;
            if hash_contract_plan(&context.plan_source) != local_probe.contract_plan_fingerprint {
                return None;
            }
            let authoritative_state = load_authoritative_transition_state_relaxed(&context).ok()?;
            overlay_step_state_from_authority(&mut context, authoritative_state.as_ref()).ok()?;
            let derived =
                derive_execution_truth_from_authority(&context, authoritative_state.as_ref())
                    .ok()?;
            let semantic_workspace_state_id = status_workspace_state_id(&context).ok()?;
            (derived.status.execution_started == "yes"
                && semantic_workspace_state_id == local_probe.semantic_workspace_state_id)
                .then_some(ExecutionReadScope {
                    context,
                    status: derived.status,
                    overlay: derived.overlay,
                    authoritative_state,
                    runtime_state: None,
                })
        }))
}

pub fn status_from_context(context: &ExecutionContext) -> Result<PlanExecutionStatus, JsonFailure> {
    status_from_context_with_overlay(context, None, false, None, false, None)
}

pub(crate) fn status_from_context_with_overlay(
    context: &ExecutionContext,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let loaded_authoritative_state;
    let authoritative_state = if use_preloaded_authoritative_state {
        preloaded_authoritative_state
    } else {
        loaded_authoritative_state = load_authoritative_transition_state(context)?;
        loaded_authoritative_state.as_ref()
    };
    let preflight_acceptance = match preflight_acceptance_for_context(context) {
        Ok(acceptance) => acceptance,
        Err(error) => {
            if authoritative_execution_run_id_from_state(authoritative_state).is_some() {
                None
            } else {
                return Err(error);
            }
        }
    };
    let started = execution_started(context, authoritative_state);
    let warning_codes = Vec::new();
    let execution_run_id = current_execution_run_id_with_authority(context, authoritative_state)?
        .map(ExecutionRunId::new);
    let chunk_id = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.chunk_id.clone())
        .unwrap_or_else(|| pending_chunk_id(context));
    let chunking_strategy = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.chunking_strategy);
    let evaluator_policy = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.evaluator_policy.clone());
    let reset_policy = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.reset_policy);
    let review_stack = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.review_stack.clone());
    let semantic_snapshot = semantic_workspace_snapshot(context)?;
    let projection_metadata =
        execution_projection_read_model_metadata(context, normal_projection_write_mode()?)?;

    let mut status = PlanExecutionStatus {
        schema_version: 3,
        plan_revision: context.plan_document.plan_revision,
        execution_run_id,
        workspace_state_id: semantic_snapshot.raw_workspace_tree_id.clone(),
        current_branch_reviewed_state_id: None,
        current_branch_closure_id: None,
        current_branch_meaningful_drift: false,
        current_task_closures: Vec::new(),
        superseded_closures_summary: Vec::new(),
        stale_unreviewed_closures: Vec::new(),
        current_release_readiness_state: None,
        current_final_review_state: String::from("not_required"),
        current_qa_state: String::from("not_required"),
        current_final_review_branch_closure_id: None,
        current_final_review_result: None,
        current_qa_branch_closure_id: None,
        current_qa_result: None,
        qa_requirement: None,
        latest_authoritative_sequence: INITIAL_AUTHORITATIVE_SEQUENCE,
        phase: None,
        harness_phase: if started {
            HarnessPhase::Executing
        } else if preflight_acceptance.is_some() {
            HarnessPhase::ExecutionPreflight
        } else {
            HarnessPhase::ImplementationHandoff
        },
        chunk_id,
        chunking_strategy,
        evaluator_policy,
        reset_policy,
        review_stack,
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
        write_authority_state: String::from("preflight_pending"),
        write_authority_holder: None,
        write_authority_worktree: None,
        repo_state_baseline_head_sha: None,
        repo_state_baseline_worktree_fingerprint: None,
        repo_state_drift_state: String::from("preflight_pending"),
        dependency_index_state: String::from("missing"),
        final_review_state: DownstreamFreshnessState::NotRequired,
        browser_qa_state: DownstreamFreshnessState::NotRequired,
        release_docs_state: DownstreamFreshnessState::NotRequired,
        last_final_review_artifact_fingerprint: None,
        last_browser_qa_artifact_fingerprint: None,
        last_release_docs_artifact_fingerprint: None,
        strategy_state: String::from("checkpoint_missing"),
        last_strategy_checkpoint_fingerprint: None,
        strategy_checkpoint_kind: String::from("none"),
        strategy_reset_required: false,
        phase_detail: String::from(phase::DETAIL_PLANNING_REENTRY_REQUIRED),
        review_state_status: String::from("clean"),
        recording_context: None,
        execution_command_context: None,
        execution_reentry_target_source: None,
        public_repair_targets: Vec::new(),
        blocking_records: Vec::new(),
        blocking_scope: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        projection_diagnostics: Vec::new(),
        state_kind: String::from("actionable_public_command"),
        next_public_action: None,
        blockers: Vec::new(),
        semantic_workspace_tree_id: semantic_snapshot.semantic_workspace_tree_id,
        raw_workspace_tree_id: Some(semantic_snapshot.raw_workspace_tree_id),
        next_action: String::from("inspect_workflow"),
        recommended_public_command: None,
        recommended_public_command_argv: None,
        required_inputs: Vec::new(),
        recommended_command: None,
        finish_review_gate_pass_branch_closure_id: None,
        reason_codes: Vec::new(),
        execution_mode: context.plan_document.execution_mode.clone(),
        execution_fingerprint: context.execution_fingerprint.clone(),
        evidence_path: context.evidence_rel.clone(),
        projection_mode: projection_metadata.projection_mode,
        state_dir_projection_paths: projection_metadata.state_dir_projection_paths,
        tracked_projection_paths: projection_metadata.tracked_projection_paths,
        tracked_projections_current: projection_metadata.tracked_projections_current,
        execution_started: if started {
            String::from("yes")
        } else {
            String::from("no")
        },
        warning_codes,
        active_task: None,
        active_step: None,
        blocking_task: None,
        blocking_step: None,
        resume_task: None,
        resume_step: None,
    };

    project_authoritative_open_step_status_fields(context, &mut status);

    apply_authoritative_status_overlay(
        context,
        &mut status,
        preloaded_overlay,
        use_preloaded_overlay,
    )?;
    apply_task_boundary_status_overlay(context, &mut status);
    apply_current_task_closure_repair_status_overlay(context, &mut status);
    suppress_preempted_resume_status_fields(context, &mut status);
    apply_late_stage_precedence_status_overlay(
        context,
        &mut status,
        authoritative_state,
        gate_projection,
    );
    populate_public_status_contract_fields(
        context,
        &mut status,
        preloaded_overlay,
        use_preloaded_overlay,
        authoritative_state,
        true,
        gate_projection,
    )?;
    Ok(status)
}

fn project_authoritative_open_step_status_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    if let Some(step) = active_step(context, NoteState::Active) {
        status.active_task = Some(step.task_number);
        status.active_step = Some(step.step_number);
        status.resume_task = None;
        status.resume_step = None;
        if status.blocking_step.is_some() {
            status.blocking_task = None;
            status.blocking_step = None;
        }
        return;
    }
    if let Some(step) = active_step(context, NoteState::Blocked) {
        status.active_task = None;
        status.active_step = None;
        status.resume_task = None;
        status.resume_step = None;
        status.blocking_task = Some(step.task_number);
        status.blocking_step = Some(step.step_number);
        return;
    }
    if let Some(step) = active_step(context, NoteState::Interrupted) {
        status.active_task = None;
        status.active_step = None;
        status.resume_task = Some(step.task_number);
        status.resume_step = Some(step.step_number);
        if status.blocking_step.is_some() {
            status.blocking_task = None;
            status.blocking_step = None;
        }
    }
}

pub(crate) fn apply_authoritative_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
) -> Result<(), JsonFailure> {
    let state_path = authoritative_state_path(context);
    let loaded_overlay;
    let overlay = if use_preloaded_overlay {
        preloaded_overlay
    } else {
        loaded_overlay = load_status_authoritative_overlay_checked(context)?;
        loaded_overlay.as_ref()
    };
    let Some(overlay) = overlay else {
        return Ok(());
    };

    if let Some(phase) = normalize_optional_overlay_value(overlay.harness_phase.as_deref()) {
        status.harness_phase = parse_harness_phase(phase).ok_or_else(|| {
            malformed_overlay_field(
                &state_path,
                "harness_phase",
                phase,
                "must be one of the public harness phases",
            )
        })?;
    }

    if let Some(chunk_id) = normalize_optional_overlay_value(overlay.chunk_id.as_deref()) {
        status.chunk_id = ChunkId::new(chunk_id.to_owned());
    }

    if let Some(sequence) = overlay
        .latest_authoritative_sequence
        .or(overlay.authoritative_sequence)
    {
        status.latest_authoritative_sequence = sequence;
    }

    let (active_contract_path, active_contract_fingerprint) = parse_overlay_active_contract_fields(
        overlay.active_contract_path.as_deref(),
        overlay.active_contract_fingerprint.as_deref(),
        &state_path,
    )?;
    status.active_contract_path = active_contract_path;
    status.active_contract_fingerprint = active_contract_fingerprint;

    status.required_evaluator_kinds = parse_evaluator_kinds(
        &overlay.required_evaluator_kinds,
        "required_evaluator_kinds",
        &state_path,
    )?;
    status.completed_evaluator_kinds = parse_evaluator_kinds(
        &overlay.completed_evaluator_kinds,
        "completed_evaluator_kinds",
        &state_path,
    )?;
    status.pending_evaluator_kinds = parse_evaluator_kinds(
        &overlay.pending_evaluator_kinds,
        "pending_evaluator_kinds",
        &state_path,
    )?;
    status.non_passing_evaluator_kinds = parse_evaluator_kinds(
        &overlay.non_passing_evaluator_kinds,
        "non_passing_evaluator_kinds",
        &state_path,
    )?;

    if let Some(value) =
        normalize_optional_overlay_value(overlay.aggregate_evaluation_state.as_deref())
    {
        status.aggregate_evaluation_state =
            parse_aggregate_evaluation_state(value).ok_or_else(|| {
                malformed_overlay_field(
                    &state_path,
                    "aggregate_evaluation_state",
                    value,
                    "must be pass, fail, blocked, or pending",
                )
            })?;
    }

    status.last_evaluation_report_path =
        normalize_optional_overlay_value(overlay.last_evaluation_report_path.as_deref())
            .map(str::to_owned);
    status.last_evaluation_report_fingerprint =
        normalize_optional_overlay_value(overlay.last_evaluation_report_fingerprint.as_deref())
            .map(str::to_owned);
    status.last_evaluation_evaluator_kind = parse_optional_evaluator_kind(
        overlay.last_evaluation_evaluator_kind.as_deref(),
        "last_evaluation_evaluator_kind",
        &state_path,
    )?;
    status.last_evaluation_verdict = parse_optional_evaluation_verdict(
        overlay.last_evaluation_verdict.as_deref(),
        "last_evaluation_verdict",
        &state_path,
    )?;

    if let Some(value) = overlay.current_chunk_retry_count {
        status.current_chunk_retry_count = value;
    }
    if let Some(value) = overlay.current_chunk_retry_budget {
        status.current_chunk_retry_budget = value;
    }
    if let Some(value) = overlay.current_chunk_pivot_threshold {
        status.current_chunk_pivot_threshold = value;
    }
    if let Some(value) = overlay.handoff_required {
        status.handoff_required = value;
    }
    if !overlay.open_failed_criteria.is_empty() {
        status.open_failed_criteria = overlay.open_failed_criteria.clone();
    }
    if let Some(value) = normalize_optional_overlay_value(overlay.write_authority_state.as_deref())
    {
        status.write_authority_state = value.to_owned();
    }
    status.write_authority_holder =
        normalize_optional_overlay_value(overlay.write_authority_holder.as_deref())
            .map(str::to_owned);
    status.write_authority_worktree =
        normalize_optional_overlay_value(overlay.write_authority_worktree.as_deref())
            .map(str::to_owned);
    status.repo_state_baseline_head_sha =
        normalize_optional_overlay_value(overlay.repo_state_baseline_head_sha.as_deref())
            .map(str::to_owned);
    status.repo_state_baseline_worktree_fingerprint = normalize_optional_overlay_value(
        overlay.repo_state_baseline_worktree_fingerprint.as_deref(),
    )
    .map(str::to_owned);
    if let Some(value) = normalize_optional_overlay_value(overlay.repo_state_drift_state.as_deref())
    {
        status.repo_state_drift_state = value.to_owned();
    }
    if let Some(value) = normalize_optional_overlay_value(overlay.dependency_index_state.as_deref())
    {
        status.dependency_index_state = value.to_owned();
    }
    if let Some(value) = parse_optional_downstream_freshness_state(
        overlay.final_review_state.as_deref(),
        "final_review_state",
        &state_path,
    )? {
        status.final_review_state = value;
    }
    if let Some(value) = parse_optional_downstream_freshness_state(
        overlay.browser_qa_state.as_deref(),
        "browser_qa_state",
        &state_path,
    )? {
        status.browser_qa_state = value;
    }
    if let Some(value) = parse_optional_downstream_freshness_state(
        overlay.release_docs_state.as_deref(),
        "release_docs_state",
        &state_path,
    )? {
        status.release_docs_state = value;
    }
    status.last_final_review_artifact_fingerprint =
        normalize_optional_overlay_value(overlay.last_final_review_artifact_fingerprint.as_deref())
            .map(str::to_owned);
    status.last_browser_qa_artifact_fingerprint =
        normalize_optional_overlay_value(overlay.last_browser_qa_artifact_fingerprint.as_deref())
            .map(str::to_owned);
    status.last_release_docs_artifact_fingerprint =
        normalize_optional_overlay_value(overlay.last_release_docs_artifact_fingerprint.as_deref())
            .map(str::to_owned);
    if let Some(value) = normalize_optional_overlay_value(overlay.strategy_state.as_deref()) {
        status.strategy_state = value.to_owned();
    }
    status.last_strategy_checkpoint_fingerprint =
        normalize_optional_overlay_value(overlay.last_strategy_checkpoint_fingerprint.as_deref())
            .map(str::to_owned);
    if let Some(value) =
        normalize_optional_overlay_value(overlay.strategy_checkpoint_kind.as_deref())
    {
        status.strategy_checkpoint_kind = value.to_owned();
    }
    if let Some(value) = overlay.strategy_reset_required {
        status.strategy_reset_required = value;
    }
    if !overlay.reason_codes.is_empty() {
        status.reason_codes =
            parse_reason_codes(&overlay.reason_codes, "reason_codes", &state_path)?;
    }
    status.current_branch_closure_id =
        normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref())
            .map(str::to_owned);
    status.current_branch_reviewed_state_id = normalize_optional_overlay_value(
        overlay.current_branch_closure_reviewed_state_id.as_deref(),
    )
    .map(str::to_owned);
    status.current_release_readiness_state =
        normalize_optional_overlay_value(overlay.current_release_readiness_result.as_deref())
            .map(str::to_owned);

    Ok(())
}

pub(crate) fn normalize_optional_overlay_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn push_missing_derived_field(missing: &mut Vec<String>, field: &str) {
    if classify_review_state_field(field)
        == Some(PersistedReviewStateFieldClass::AuthoritativeAppendOnlyHistory)
    {
        return;
    }
    if !missing.iter().any(|existing| existing == field) {
        missing.push(field.to_owned());
    }
}

pub(crate) fn missing_derived_review_state_fields(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Vec<String> {
    let mut missing = Vec::new();
    if let Some(authoritative_state) = authoritative_state
        && authoritative_state.current_task_closure_overlay_needs_restore()
    {
        push_missing_derived_field(&mut missing, "current_task_closure_records");
    }

    let Some(overlay) = overlay else {
        return missing;
    };
    let overlay_current_branch_closure_id =
        normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref());

    let Some(authoritative_state) = authoritative_state else {
        if overlay_current_branch_closure_id.is_some() {
            push_missing_derived_field(&mut missing, "current_branch_closure_id");
            if normalize_optional_overlay_value(
                overlay.current_branch_closure_reviewed_state_id.as_deref(),
            )
            .is_none()
            {
                push_missing_derived_field(
                    &mut missing,
                    "current_branch_closure_reviewed_state_id",
                );
            }
            if normalize_optional_overlay_value(
                overlay.current_branch_closure_contract_identity.as_deref(),
            )
            .is_none()
            {
                push_missing_derived_field(
                    &mut missing,
                    "current_branch_closure_contract_identity",
                );
            }
        }
        return missing;
    };

    let bound_current_branch_closure = authoritative_state.bound_current_branch_closure_identity();
    let current_branch_closure_id = bound_current_branch_closure
        .as_ref()
        .map(|identity| identity.branch_closure_id.as_str());
    if let Some(current_identity) = bound_current_branch_closure.as_ref() {
        if overlay_current_branch_closure_id != Some(current_identity.branch_closure_id.as_str()) {
            push_missing_derived_field(&mut missing, "current_branch_closure_id");
        }
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_reviewed_state_id.as_deref(),
        ) != Some(current_identity.reviewed_state_id.as_str())
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_reviewed_state_id");
        }
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_contract_identity.as_deref(),
        ) != Some(current_identity.contract_identity.as_str())
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_contract_identity");
        }
    } else if overlay_current_branch_closure_id.is_some() {
        push_missing_derived_field(&mut missing, "current_branch_closure_id");
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_reviewed_state_id.as_deref(),
        )
        .is_none()
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_reviewed_state_id");
        }
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_contract_identity.as_deref(),
        )
        .is_none()
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_contract_identity");
        }
    }

    if let Some(record) = authoritative_state.current_final_review_record()
        && current_branch_closure_id == Some(record.branch_closure_id.as_str())
    {
        if authoritative_state
            .current_final_review_record_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_record_id");
        }
        if authoritative_state
            .current_final_review_branch_closure_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_branch_closure_id");
        }
        if authoritative_state
            .current_final_review_dispatch_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_dispatch_id");
        }
        if authoritative_state
            .current_final_review_reviewer_source()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_reviewer_source");
        }
        if authoritative_state
            .current_final_review_reviewer_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_reviewer_id");
        }
        if authoritative_state.current_final_review_result().is_none() {
            push_missing_derived_field(&mut missing, "current_final_review_result");
        }
        if authoritative_state
            .current_final_review_summary_hash()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_summary_hash");
        }
    }

    if let Some(record) = authoritative_state.current_browser_qa_record()
        && current_branch_closure_id == Some(record.branch_closure_id.as_str())
    {
        if authoritative_state.current_qa_record_id().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_record_id");
        }
        if authoritative_state.current_qa_branch_closure_id().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_branch_closure_id");
        }
        if authoritative_state.current_qa_result().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_result");
        }
        if authoritative_state.current_qa_summary_hash().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_summary_hash");
        }
    }

    missing
}

pub(crate) fn apply_current_task_closure_repair_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    if context.steps.iter().any(|step| !step.checked) {
        return;
    }
    for reason_code in project_current_task_closure_repair_reason_codes(context) {
        push_status_reason_code_once(status, &reason_code);
    }
}

pub(crate) fn suppress_preempted_resume_status_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    let Some(resume_task) = status.resume_task else {
        return;
    };
    let stale_preempts_resume = projected_earliest_stale_task_from_status(status)
        .is_some_and(|earliest_task| earliest_task < resume_task);
    let bridge_preempts_resume = status.blocking_task.is_some_and(|blocking_task| {
        task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            blocking_task,
            projected_earliest_stale_task_from_status(status),
        )
        .ok()
        .flatten()
        .is_some()
            && stale_unreviewed_allows_task_closure_baseline_bridge(context, status, blocking_task)
                .unwrap_or(false)
    });
    let execution_reentry_preempts_resume = status.phase_detail
        == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && status.blocking_task.is_some_and(|blocking_task| {
            blocking_task != resume_task && blocking_task < resume_task
        });
    let cycle_break_preempts_resume = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
        && load_status_authoritative_overlay_checked(context)
            .ok()
            .flatten()
            .and_then(|overlay| overlay.strategy_cycle_break_task)
            .is_some_and(|cycle_break_task| {
                cycle_break_task != resume_task && cycle_break_task < resume_task
            });
    if stale_preempts_resume
        || bridge_preempts_resume
        || execution_reentry_preempts_resume
        || cycle_break_preempts_resume
    {
        status.resume_task = None;
        status.resume_step = None;
    }
}

pub(crate) fn populate_public_status_contract_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<(), JsonFailure> {
    let loaded_overlay;
    let overlay = if use_preloaded_overlay {
        preloaded_overlay
    } else {
        loaded_overlay = load_status_authoritative_overlay_checked(context)?;
        loaded_overlay.as_ref()
    };
    let loaded_event_authority_state;
    // This wrapper is reduced from `execution-harness/events.jsonl`; it is not a direct
    // `state.json` truth read, even though the helper retains the historical type name.
    let event_authority_state = if use_preloaded_authoritative_state {
        preloaded_authoritative_state
    } else {
        loaded_event_authority_state = load_authoritative_transition_state(context)?;
        loaded_event_authority_state.as_ref()
    };
    if let Some(current_identity) =
        validated_current_branch_closure_identity_from_authoritative_state(
            context,
            event_authority_state,
        )
    {
        status.current_branch_closure_id = Some(current_identity.branch_closure_id.clone());
        if resolve_branch_closure_reviewed_tree_sha(
            &context.runtime.repo_root,
            &current_identity.branch_closure_id,
            &current_identity.reviewed_state_id,
        )
        .is_ok()
        {
            status.current_branch_reviewed_state_id = Some(current_identity.reviewed_state_id);
        } else {
            status.current_branch_reviewed_state_id = None;
            push_status_reason_code_once(status, "current_branch_closure_reviewed_state_malformed");
        }
    } else {
        status.current_branch_closure_id = None;
        status.current_branch_reviewed_state_id = None;
    }
    let closure_graph = AuthoritativeClosureGraph::from_state(
        event_authority_state,
        &ClosureGraphSignals::from_authoritative_state(
            event_authority_state,
            overlay.and_then(|overlay| overlay.current_branch_closure_id.as_deref()),
            false,
            false,
            Vec::new(),
        ),
    );
    status.current_release_readiness_state = None;
    status.current_final_review_branch_closure_id = None;
    status.current_final_review_result = None;
    status.current_qa_branch_closure_id = None;
    status.current_qa_result = None;
    let current_late_stage_branch_closure_id = status
        .current_branch_reviewed_state_id
        .as_ref()
        .and(status.current_branch_closure_id.as_ref())
        .cloned();
    let late_stage_bindings = shared_current_late_stage_branch_bindings(
        event_authority_state,
        current_late_stage_branch_closure_id.as_deref(),
        status.current_branch_reviewed_state_id.as_deref(),
    );
    status.current_release_readiness_state =
        late_stage_bindings.current_release_readiness_result.clone();
    status.current_final_review_branch_closure_id = late_stage_bindings
        .current_final_review_branch_closure_id
        .clone();
    status.current_final_review_result = late_stage_bindings.current_final_review_result.clone();
    status.current_qa_branch_closure_id = late_stage_bindings.current_qa_branch_closure_id.clone();
    status.current_qa_result = late_stage_bindings.current_qa_result.clone();
    status.qa_requirement =
        shared_normalized_plan_qa_requirement(context.plan_document.qa_requirement.as_deref());
    if status.current_release_readiness_state.is_some() {
        status.release_docs_state = DownstreamFreshnessState::Fresh;
    } else {
        status.release_docs_state = DownstreamFreshnessState::NotRequired;
        status.last_release_docs_artifact_fingerprint = None;
    }
    if status.current_final_review_branch_closure_id.is_some()
        && status.current_final_review_result.is_some()
    {
        status.final_review_state = DownstreamFreshnessState::Fresh;
    } else {
        status.final_review_state = DownstreamFreshnessState::NotRequired;
        status.last_final_review_artifact_fingerprint = None;
    }
    if status.current_qa_branch_closure_id.is_some() && status.current_qa_result.is_some() {
        status.browser_qa_state = DownstreamFreshnessState::Fresh;
    } else if status.current_final_review_result.is_some()
        && status.qa_requirement.as_deref() == Some("required")
    {
        status.browser_qa_state = DownstreamFreshnessState::Missing;
        status.last_browser_qa_artifact_fingerprint = None;
    } else {
        status.browser_qa_state = DownstreamFreshnessState::NotRequired;
        status.last_browser_qa_artifact_fingerprint = None;
    }
    let authoritative_downstream_truth_present = status.current_branch_closure_id.is_some()
        || event_authority_state.is_some_and(|state| {
            state.current_release_readiness_record_id().is_some()
                || state.current_final_review_record_id().is_some()
                || state.current_qa_record_id().is_some()
        });
    if !authoritative_downstream_truth_present {
        status.final_review_state = DownstreamFreshnessState::NotRequired;
        status.browser_qa_state = DownstreamFreshnessState::NotRequired;
        status.release_docs_state = DownstreamFreshnessState::NotRequired;
        status.last_final_review_artifact_fingerprint = None;
        status.last_browser_qa_artifact_fingerprint = None;
        status.last_release_docs_artifact_fingerprint = None;
    }
    status.current_final_review_state =
        downstream_freshness_state_label(status.final_review_state).to_owned();
    status.current_qa_state = downstream_freshness_state_label(status.browser_qa_state).to_owned();
    status.current_branch_meaningful_drift =
        shared_current_branch_closure_has_tracked_drift(context, event_authority_state)
            .unwrap_or(false);
    status.current_task_closures = project_current_task_closures(context, event_authority_state)?;
    status.superseded_closures_summary = closure_graph.superseded_record_ids();
    status.finish_review_gate_pass_branch_closure_id =
        late_stage_bindings.finish_review_gate_pass_branch_closure_id;
    if let Some(late_stage_phase) = canonical_late_stage_phase_from_bindings(status) {
        status.harness_phase = late_stage_phase;
    }

    let fallback_gate_review;
    let fallback_gate_finish;
    let (gate_review, gate_finish) = match gate_projection {
        Some(gate_projection) => (gate_projection.gate_review, gate_projection.gate_finish),
        None => {
            fallback_gate_review = GateState::default().finish();
            fallback_gate_finish = GateState::default().finish();
            (&fallback_gate_review, &fallback_gate_finish)
        }
    };
    let missing_derived_overlays =
        missing_derived_review_state_fields(event_authority_state, overlay);
    if !missing_derived_overlays.is_empty() {
        push_status_reason_code_once(status, "derived_review_state_missing");
    }
    let task_scope_overlay_restore_required = status.execution_started == "yes"
        && shared_task_scope_overlay_restore_required(
            &missing_derived_overlays,
            event_authority_state,
        );
    if let Some(event_authority_state) = event_authority_state
        && event_authority_state.current_task_closure_overlay_needs_restore()
    {
        push_status_reason_code_once(status, "current_task_closure_overlay_restore_required");
    }
    if task_scope_overlay_restore_required {
        status.harness_phase = HarnessPhase::Executing;
    }
    let repair_route_decision = shared_repair_review_state_reroute_decision(
        context,
        status,
        event_authority_state,
        Some(gate_review),
        Some(gate_finish),
        task_scope_overlay_restore_required,
        false,
    );
    let branch_reroute_still_valid = repair_route_decision.branch_reroute_still_valid;
    let branch_drift_escapes_late_stage_surface =
        repair_route_decision.branch_drift_escapes_late_stage_surface;
    if repair_route_decision.late_stage_surface_not_declared {
        push_status_reason_code_once(status, "late_stage_surface_not_declared");
    }
    if branch_drift_escapes_late_stage_surface {
        push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
        push_status_reason_code_once(status, "branch_drift_escapes_late_stage_surface");
    }
    let persisted_repair_follow_up = repair_route_decision.persisted_repair_follow_up.as_deref();
    let raw_late_stage_review_state_status =
        repair_route_decision.raw_late_stage_review_state_status;
    let task_scope_repair_precedence_active =
        repair_route_decision.task_scope_repair_precedence_active;
    let repair_reroute = repair_route_decision.repair_reroute;
    if status.blocking_task.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && status.current_branch_closure_id.is_none()
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "late_stage_surface_not_declared")
        && let Some(missing_task) = completed_plan_missing_current_closure_task(context, status)
    {
        push_status_reason_code_once(status, "prior_task_current_closure_missing");
        status.blocking_task = Some(missing_task);
        status.blocking_step = None;
    }
    let execution_reentry_target_available = execution_reentry_target(
        context,
        status,
        context.plan_rel.as_str(),
        crate::execution::next_action::NextActionAuthorityInputs::default(),
    )
    .is_some();
    let repair_follow_up_requires_execution_reentry = repair_reroute
        == ReviewStateRepairReroute::ExecutionReentry
        && execution_reentry_target_available;
    let repair_follow_up_requires_planning_reentry = repair_reroute
        == ReviewStateRepairReroute::ExecutionReentry
        && !execution_reentry_target_available;
    let persisted_branch_reroute_without_current_binding =
        !repair_follow_up_requires_execution_reentry
            && persisted_repair_follow_up == Some("advance_late_stage")
            && !task_scope_repair_precedence_active
            && branch_reroute_still_valid
            && status.current_branch_closure_id.is_some();
    let persisted_branch_reroute_with_current_binding = !repair_follow_up_requires_execution_reentry
        && persisted_repair_follow_up == Some("advance_late_stage")
        && !task_scope_repair_precedence_active
        && branch_reroute_still_valid
        && raw_late_stage_review_state_status == Some("stale_unreviewed")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
        && status.current_branch_closure_id.is_some();
    let repair_follow_up_records_branch_closure = repair_reroute
        == ReviewStateRepairReroute::RecordBranchClosure
        || persisted_branch_reroute_without_current_binding
        || persisted_branch_reroute_with_current_binding;
    let authoritative_release_readiness_result =
        authoritative_release_readiness_result_for_current_branch(
            event_authority_state,
            current_late_stage_branch_closure_id.as_deref(),
        );
    let authoritative_release_readiness_current = authoritative_release_readiness_result.is_some();
    let confined_late_stage_branch_drift_with_release_readiness =
        authoritative_release_readiness_current
            && repair_route_decision.branch_reroute_still_valid
            && current_late_stage_branch_closure_id.is_some()
            && status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE);
    if (repair_follow_up_records_branch_closure
        || confined_late_stage_branch_drift_with_release_readiness)
        && authoritative_release_readiness_current
        && status.current_release_readiness_state.is_none()
    {
        status.current_release_readiness_state = authoritative_release_readiness_result;
        if status.current_release_readiness_state.as_deref() == Some("ready") {
            status.release_docs_state = DownstreamFreshnessState::Fresh;
        }
    }
    let branch_closure_refresh_missing_current_closure =
        shared_branch_closure_refresh_missing_current_closure(status);
    if repair_follow_up_requires_execution_reentry {
        status.harness_phase = HarnessPhase::Executing;
    } else if repair_follow_up_requires_planning_reentry {
        status.harness_phase = HarnessPhase::PivotRequired;
    } else if repair_follow_up_records_branch_closure
        && persisted_repair_follow_up == Some("advance_late_stage")
    {
        status.harness_phase = if status.current_release_readiness_state.is_some()
            || authoritative_release_readiness_current
        {
            HarnessPhase::FinalReviewPending
        } else {
            HarnessPhase::DocumentReleasePending
        };
    }
    let task_boundary_unresolved_stale =
        projected_earliest_stale_task_from_status(status).is_some();
    status.review_state_status = derive_public_review_state_status(
        status,
        gate_review,
        gate_finish,
        repair_follow_up_requires_execution_reentry,
        repair_follow_up_records_branch_closure,
        branch_drift_escapes_late_stage_surface,
        task_boundary_unresolved_stale,
    );
    let persisted_branch_reroute_viable = persisted_repair_follow_up == Some("advance_late_stage")
        && status.current_branch_closure_id.is_some();
    let branch_closure_recording_basis_missing = status.review_state_status
        == "missing_current_closure"
        && !branch_reroute_still_valid
        && !branch_closure_refresh_missing_current_closure
        && !persisted_branch_reroute_viable;
    let authoritative_task_closure_baseline_present = event_authority_state.is_some_and(|state| {
        !state.current_task_closure_results().is_empty()
            || context
                .tasks_by_number
                .keys()
                .any(|task| state.raw_current_task_closure_state_entry(*task).is_some())
    });
    let late_stage_surface_requires_planning_reentry = status.current_branch_closure_id.is_none()
        && status.current_task_closures.is_empty()
        && !authoritative_task_closure_baseline_present
        && status.blocking_task.is_none()
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "late_stage_surface_not_declared");
    let late_stage_missing_current_closure_stale_provenance =
        shared_late_stage_missing_current_closure_stale_provenance_present(context, status)?;
    let preserve_canonical_late_stage_harness_phase = branch_closure_recording_basis_missing
        && is_late_stage_phase(status.harness_phase)
        && (late_stage_missing_current_closure_stale_provenance
            || status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
            || persisted_repair_follow_up == Some("advance_late_stage"))
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE);
    if authoritative_task_closure_baseline_present
        && status.harness_phase == HarnessPhase::PivotRequired
        && status.current_branch_closure_id.is_none()
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    if late_stage_surface_requires_planning_reentry
        && status.current_branch_closure_id.is_none()
        && let Some(task) = context.tasks_by_number.keys().copied().max()
    {
        status.harness_phase = HarnessPhase::Executing;
        status.blocking_task = Some(task);
        status.blocking_step = None;
        push_status_reason_code_once(status, "prior_task_current_closure_missing");
        push_status_reason_code_once(status, "task_closure_baseline_repair_candidate");
    }
    if late_stage_surface_requires_planning_reentry && status.blocking_task.is_none() {
        status.harness_phase = HarnessPhase::PivotRequired;
    } else if branch_closure_recording_basis_missing
        && !preserve_canonical_late_stage_harness_phase
        && !late_stage_surface_requires_planning_reentry
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    let _negative_result_phase_detail = apply_negative_result_status_overlay(
        context,
        status,
        gate_finish,
        overlay,
        event_authority_state,
    );
    if TargetlessStaleReconcile::status_needs_marker_for_status(status) {
        push_status_reason_code_once(status, TARGETLESS_STALE_RECONCILE_REASON_CODE);
    }
    clear_route_projection_fields(status);
    if (task_scope_overlay_restore_required || branch_closure_recording_basis_missing)
        && !preserve_canonical_late_stage_harness_phase
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    let persisted_branch_reroute_projection = status.execution_started == "yes"
        && !task_scope_overlay_restore_required
        && status.current_branch_closure_id.is_some()
        && status.review_state_status == "missing_current_closure"
        && branch_reroute_still_valid
        && persisted_repair_follow_up == Some("advance_late_stage");
    if persisted_branch_reroute_projection {
        status.harness_phase = HarnessPhase::DocumentReleasePending;
    }
    status.blocking_records = compute_status_blocking_records(context, status, gate_finish)?;

    Ok(())
}

fn clear_route_projection_fields(status: &mut PlanExecutionStatus) {
    status.phase = None;
    status.phase_detail.clear();
    status.recording_context = None;
    status.execution_command_context = None;
    status.execution_reentry_target_source = None;
    status.public_repair_targets.clear();
    status.next_action.clear();
    status.recommended_command = None;
    status.blocking_scope = None;
    status.external_wait_state = None;
    status.blocking_reason_codes.clear();
    status.state_kind.clear();
    status.next_public_action = None;
    status.blockers.clear();
}

fn canonical_late_stage_phase_from_bindings(status: &PlanExecutionStatus) -> Option<HarnessPhase> {
    if status.execution_started != "yes"
        || status.current_branch_closure_id.is_none()
        || status.active_task.is_some()
        || status.active_step.is_some()
        || status.resume_task.is_some()
        || status.resume_step.is_some()
        || status.blocking_task.is_some()
        || status.blocking_step.is_some()
        || matches!(
            status.harness_phase,
            HarnessPhase::PivotRequired | HarnessPhase::HandoffRequired
        )
    {
        return None;
    }
    if status.current_release_readiness_state.as_deref() != Some("ready") {
        return Some(HarnessPhase::DocumentReleasePending);
    }
    if status.current_final_review_result.is_none() {
        return Some(HarnessPhase::FinalReviewPending);
    }
    if status.qa_requirement.as_deref() == Some("required") && status.current_qa_result.is_none() {
        return Some(HarnessPhase::QaPending);
    }
    Some(HarnessPhase::ReadyForBranchCompletion)
}

pub(crate) fn compute_status_blocking_records(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
) -> Result<Vec<StatusBlockingRecord>, JsonFailure> {
    let task_structural_records =
        derive_structural_current_task_closure_blocking_records(context, status)?;
    let base_blocking_records = derive_public_blocking_records(status, gate_finish);
    if let Some(structural_records) = task_structural_records
        .as_ref()
        .filter(|records| !records.is_empty())
    {
        if status.review_state_status == "stale_unreviewed" {
            return Ok(merge_status_blocking_records(
                base_blocking_records,
                structural_records.clone(),
            ));
        }
        return Ok(structural_records.clone());
    }
    if let Some(record) = derive_branch_rerecording_blocking_record(context, status)? {
        return Ok(vec![record]);
    }
    let branch_structural_records =
        derive_structural_current_branch_closure_blocking_records(status);
    let blocking_records = if status.review_state_status == "stale_unreviewed" {
        task_structural_records
            .into_iter()
            .chain(branch_structural_records)
            .fold(base_blocking_records, merge_status_blocking_records)
    } else if let Some(structural_records) =
        task_structural_records.filter(|records| !records.is_empty())
    {
        structural_records
    } else if let Some(structural_records) =
        branch_structural_records.filter(|records| !records.is_empty())
    {
        structural_records
    } else {
        base_blocking_records
    };
    Ok(blocking_records)
}

fn authoritative_release_readiness_result_for_current_branch(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    current_branch_closure_id: Option<&str>,
) -> Option<String> {
    shared_release_readiness_result_for_branch_closure(
        authoritative_state,
        current_branch_closure_id,
    )
}

fn derive_branch_rerecording_blocking_record(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Result<Option<StatusBlockingRecord>, JsonFailure> {
    if !semantic_branch_rerecording_required(context, status) {
        return Ok(None);
    }
    let assessment = branch_closure_rerecording_assessment(context)?;
    let branch_closure_id = status
        .current_branch_closure_id
        .clone()
        .unwrap_or_else(|| String::from("current"));
    let review_state_status = if status.review_state_status == "clean" {
        String::from("missing_current_closure")
    } else {
        status.review_state_status.clone()
    };
    if assessment.supported {
        return Ok(Some(StatusBlockingRecord {
            code: String::from("missing_current_closure"),
            scope_type: String::from("branch"),
            scope_key: branch_closure_id,
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status,
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "The current branch closure must be re-recorded before late-stage progression can continue.",
            ),
        }));
    }
    let message = match assessment.unsupported_reason {
        Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => String::from(
            "The current branch closure can no longer be safely re-recorded from authoritative current task-closure truth, so review-state repair must reroute execution before late-stage progression can continue.",
        ),
        Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => String::from(
            "The current branch closure cannot be safely re-recorded because the approved plan does not declare Late-Stage Surface metadata for classifying post-closure drift. Repair review state must reroute through execution reentry before late-stage progression can continue.",
        ),
        Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
            String::from(
                "The current branch closure cannot be safely re-recorded because branch drift escapes the approved Late-Stage Surface. Repair review state must reroute execution before late-stage progression can continue.",
            )
        }
    };
    Ok(Some(StatusBlockingRecord {
        code: String::from("missing_current_closure"),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_type: String::from("review_state"),
        record_id: Some(branch_closure_id),
        review_state_status,
        required_follow_up: Some(String::from("repair_review_state")),
        message,
    }))
}

fn semantic_branch_rerecording_required(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    let persisted_branch_follow_up = resolve_actionable_repair_follow_up_for_status(
        context,
        status,
        authoritative_state.as_ref(),
    )
    .is_some_and(|record| record.kind.public_token() == "advance_late_stage");
    if status.current_branch_meaningful_drift {
        let release_readiness_already_recorded =
            authoritative_release_readiness_result_for_current_branch(
                authoritative_state.as_ref(),
                status.current_branch_closure_id.as_deref(),
            )
            .is_some();
        return !(persisted_branch_follow_up && release_readiness_already_recorded);
    }
    if status.current_branch_closure_id.is_none() {
        return false;
    }
    persisted_branch_follow_up
}

fn downstream_freshness_state_label(state: DownstreamFreshnessState) -> &'static str {
    match state {
        DownstreamFreshnessState::NotRequired => "not_required",
        DownstreamFreshnessState::Missing => "missing",
        DownstreamFreshnessState::Fresh => "fresh",
        DownstreamFreshnessState::Stale => "stale",
    }
}

fn merge_status_blocking_records(
    mut base_records: Vec<StatusBlockingRecord>,
    extra_records: Vec<StatusBlockingRecord>,
) -> Vec<StatusBlockingRecord> {
    for record in extra_records {
        if !base_records.contains(&record) {
            base_records.push(record);
        }
    }
    base_records
}

pub(crate) fn prerelease_branch_closure_refresh_required(status: &PlanExecutionStatus) -> bool {
    status.harness_phase == HarnessPhase::DocumentReleasePending
        && status.current_release_readiness_state.is_none()
        && status.current_branch_closure_id.is_some()
        && status.current_branch_meaningful_drift
}

pub(crate) fn live_review_state_status_for_reroute_from_status(
    status: &PlanExecutionStatus,
    late_stage_stale_unreviewed: bool,
) -> Option<&'static str> {
    if shared_branch_closure_refresh_missing_current_closure(status) {
        return Some("missing_current_closure");
    }
    shared_live_review_state_status_for_reroute(
        late_stage_stale_unreviewed,
        current_branch_closure_structural_review_state_reason(status).is_some()
            || shared_branch_closure_refresh_missing_current_closure(status)
            || (matches!(
                status.harness_phase,
                HarnessPhase::DocumentReleasePending
                    | HarnessPhase::FinalReviewPending
                    | HarnessPhase::QaPending
                    | HarnessPhase::ReadyForBranchCompletion
            ) && status.current_branch_closure_id.is_none()),
    )
}

fn derive_public_review_state_status(
    status: &PlanExecutionStatus,
    gate_review: &GateResult,
    gate_finish: &GateResult,
    repair_follow_up_requires_execution_reentry: bool,
    repair_follow_up_records_branch_closure: bool,
    branch_scope_stale_unreviewed: bool,
    task_boundary_unresolved_stale: bool,
) -> String {
    let task_boundary_stale_unreviewed_bridge = task_boundary_unresolved_stale
        && status.blocking_task.is_some()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && task_closure_baseline_repair_candidate_reason_present(status)
        && status
            .reason_codes
            .iter()
            .any(|code| code == "prior_task_current_closure_missing")
        && !status.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "prior_task_review_dispatch_missing"
                    | "prior_task_review_dispatch_stale"
                    | "prior_task_review_not_green"
                    | "prior_task_verification_missing"
                    | "prior_task_verification_missing_legacy"
                    | "task_review_not_independent"
                    | "task_review_artifact_malformed"
                    | "task_verification_summary_malformed"
                    | "prior_task_current_closure_stale"
            )
        });
    let task_scope_stale_unreviewed =
        !task_closure_baseline_repair_candidate_reason_present(status)
            && status.reason_codes.iter().any(|code| {
                matches!(
                    code.as_str(),
                    "prior_task_review_dispatch_stale" | "prior_task_current_closure_stale"
                )
            });
    let execution_evidence_fingerprint_mismatch = gate_review
        .reason_codes
        .iter()
        .chain(gate_finish.reason_codes.iter())
        .any(|code| {
            matches!(
                code.as_str(),
                "plan_fingerprint_mismatch" | "spec_fingerprint_mismatch"
            )
        });
    let resumed_task_stale_unreviewed = (status.resume_task.is_some()
        || status.resume_step.is_some())
        && status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE);
    let late_stage_stale_signals =
        shared_public_late_stage_stale_unreviewed(status, Some(gate_review), Some(gate_finish))
            || branch_scope_stale_unreviewed;
    let stale_provenance_task_boundary = !status.current_task_closures.is_empty()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.current_branch_closure_id.is_some()
        && status.current_branch_reviewed_state_id.is_some()
        && !status.semantic_workspace_tree_id.is_empty()
        && !status.current_branch_meaningful_drift
        && status.current_release_readiness_state.is_none()
        && status.current_final_review_result.is_none()
        && status.current_qa_result.is_none()
        && execution_evidence_fingerprint_mismatch
        && status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE)
        && public_late_stage_rederivation_basis_present(status)
        && !branch_scope_stale_unreviewed;
    let task_scope_execution_reentry_active = (status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_step.is_some())
        && status.current_branch_closure_id.is_none()
        && !public_late_stage_rederivation_basis_present(status);
    let late_stage_stale_unreviewed =
        late_stage_stale_signals && !task_scope_execution_reentry_active;
    let prerelease_refresh_missing_current_closure =
        prerelease_branch_closure_refresh_required(status);
    if task_boundary_stale_unreviewed_bridge {
        return String::from("stale_unreviewed");
    }
    if stale_provenance_task_boundary {
        return String::from("stale_unreviewed");
    }
    if repair_follow_up_requires_execution_reentry
        && !prerelease_refresh_missing_current_closure
        && !branch_scope_stale_unreviewed
        && !status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE)
    {
        return String::from("clean");
    }
    if status.stale_unreviewed_closures.is_empty()
        && !task_boundary_unresolved_stale
        && !status.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                REASON_CODE_STALE_PROVENANCE | "prior_task_current_closure_stale"
            )
        })
        && (task_scope_structural_review_state_reason(status).is_some()
            || task_scope_overlay_repair_required(status))
    {
        return String::from("clean");
    }
    if resumed_task_stale_unreviewed {
        return String::from("stale_unreviewed");
    }
    if current_branch_closure_structural_review_state_reason(status).is_some() {
        return String::from("missing_current_closure");
    }
    if repair_follow_up_records_branch_closure {
        if status.current_release_readiness_state.is_some() {
            return String::from("clean");
        }
        return String::from("missing_current_closure");
    }
    if prerelease_refresh_missing_current_closure {
        return String::from("missing_current_closure");
    }
    if task_scope_stale_unreviewed {
        return String::from("stale_unreviewed");
    }
    if status.harness_phase == HarnessPhase::DocumentReleasePending
        && status.current_branch_closure_id.is_some()
        && status.current_release_readiness_state.is_none()
        && !status.current_branch_meaningful_drift
        && !branch_scope_stale_unreviewed
    {
        return String::from("clean");
    }
    if late_stage_stale_unreviewed && status.current_branch_closure_id.is_some() {
        return String::from("stale_unreviewed");
    }
    if matches!(
        status.harness_phase,
        HarnessPhase::DocumentReleasePending
            | HarnessPhase::FinalReviewPending
            | HarnessPhase::QaPending
            | HarnessPhase::ReadyForBranchCompletion
    ) && (status.current_branch_closure_id.is_none()
        || prerelease_branch_closure_refresh_required(status))
    {
        return String::from("missing_current_closure");
    }
    if late_stage_stale_unreviewed {
        return String::from("stale_unreviewed");
    }
    String::from("clean")
}

fn status_workflow_phase(status: &PlanExecutionStatus) -> &'static str {
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending => phase::PHASE_DOCUMENT_RELEASE_PENDING,
        HarnessPhase::FinalReviewPending => phase::PHASE_FINAL_REVIEW_PENDING,
        HarnessPhase::QaPending => phase::PHASE_QA_PENDING,
        HarnessPhase::ReadyForBranchCompletion => phase::PHASE_READY_FOR_BRANCH_COMPLETION,
        HarnessPhase::HandoffRequired => phase::PHASE_HANDOFF_REQUIRED,
        HarnessPhase::PivotRequired => phase::PHASE_PIVOT_REQUIRED,
        HarnessPhase::Executing => phase::PHASE_EXECUTING,
        _ => phase::PHASE_EXECUTING,
    }
}

fn status_late_stage_prerequisite_reroute_active(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
) -> bool {
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending => true,
        HarnessPhase::FinalReviewPending => {
            status.current_branch_closure_id.is_none()
                || status.current_release_readiness_state.as_deref() != Some("ready")
        }
        HarnessPhase::QaPending => {
            status.current_branch_closure_id.is_none()
                || (shared_normalized_plan_qa_requirement(
                    context.plan_document.qa_requirement.as_deref(),
                )
                .as_deref()
                    == Some("required")
                    && qa_pending_requires_test_plan_refresh(context, Some(gate_finish)))
        }
        _ => false,
    }
}

fn apply_negative_result_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    gate_finish: &GateResult,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<&'static str> {
    if status_late_stage_prerequisite_reroute_active(context, status, gate_finish) {
        return None;
    }
    let task_negative_result_task =
        shared_current_task_negative_result_task(status, overlay, authoritative_state);
    if task_negative_result_task.is_some() {
        push_status_reason_code_once(status, "prior_task_review_not_green");
    }
    if !shared_negative_result_requires_execution_reentry(
        task_negative_result_task.is_some(),
        status_workflow_phase(status),
        status.current_branch_closure_id.as_deref(),
        status.current_final_review_branch_closure_id.as_deref(),
        status.current_final_review_result.as_deref(),
        status.current_qa_branch_closure_id.as_deref(),
        status.current_qa_result.as_deref(),
    ) {
        return None;
    }
    status.harness_phase = HarnessPhase::Executing;
    status.review_state_status = String::from("clean");
    status.stale_unreviewed_closures.clear();
    status.reason_codes.retain(|reason_code| {
        reason_code != TARGETLESS_STALE_RECONCILE_REASON_CODE
            && reason_code != TARGETLESS_STALE_MISSING_AUTHORITY_CODE
    });
    status.blocking_reason_codes.retain(|reason_code| {
        reason_code != TARGETLESS_STALE_RECONCILE_REASON_CODE
            && reason_code != TARGETLESS_STALE_MISSING_AUTHORITY_CODE
    });
    push_status_reason_code_once(status, "negative_result_requires_execution_reentry");
    Some(phase::DETAIL_EXECUTION_REENTRY_REQUIRED)
}

#[cfg(test)]
fn current_workflow_pivot_record_exists_for_status_decision(
    context: &ExecutionContext,
    reason_codes: &[String],
    qa_requirement: Option<&str>,
) -> bool {
    if context.plan_rel.trim().is_empty() {
        return false;
    }
    let head_sha = match context.current_head_sha() {
        Ok(head_sha) => head_sha,
        Err(_) => return false,
    };
    let qa_requirement_missing_or_invalid =
        !matches!(qa_requirement, Some("required") | Some("not-required"));
    let decision_reason_codes =
        pivot_decision_reason_codes(reason_codes, true, qa_requirement_missing_or_invalid);
    current_workflow_pivot_record_exists(
        &context.runtime.state_dir,
        WorkflowPivotRecordIdentity {
            repo_slug: &context.runtime.repo_slug,
            safe_branch: &context.runtime.safe_branch,
            plan_path: &context.plan_rel,
            branch_name: &context.runtime.branch_name,
            head_sha: &head_sha,
            decision_reason_codes: &decision_reason_codes,
        },
    )
}

#[cfg(test)]
fn derive_public_phase_detail(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    gate_review: &GateResult,
    gate_finish: &GateResult,
    review_state_status: &str,
    task_review_dispatch_id: Option<&str>,
    final_review_dispatch_id: Option<&str>,
) -> String {
    if status.harness_phase != HarnessPhase::PivotRequired
        && task_closure_baseline_repair_candidate_reason_present(status)
        && status.blocking_step.is_none()
        && status.blocking_task.is_some_and(|task| {
            task_closure_baseline_repair_candidate_with_stale_target(
                context,
                status,
                task,
                projected_earliest_stale_task_from_status(status),
            )
            .map(|candidate| candidate.is_some())
            .unwrap_or(false)
        })
    {
        return String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
    }
    if execution_reentry_requires_review_state_repair(Some(context), status) {
        return String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    }
    if task_review_result_pending_task(status, task_review_dispatch_id).is_some() {
        return String::from(phase::DETAIL_TASK_REVIEW_RESULT_PENDING);
    }
    if review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
        && crate::execution::current_truth::worktree_drift_escapes_late_stage_surface(context)
            .unwrap_or(false)
    {
        return String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    }
    if review_state_status == "missing_current_closure" {
        return String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS);
    }
    if review_state_status == "stale_unreviewed" {
        return String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    }

    match status.harness_phase {
        HarnessPhase::ReadyForBranchCompletion => {
            if status
                .finish_review_gate_pass_branch_closure_id
                .as_ref()
                .zip(status.current_branch_closure_id.as_ref())
                .is_some_and(|(checkpoint, current)| checkpoint == current)
                && gate_finish.allowed
            {
                String::from(phase::DETAIL_FINISH_COMPLETION_GATE_READY)
            } else {
                String::from(phase::DETAIL_FINISH_REVIEW_GATE_READY)
            }
        }
        HarnessPhase::DocumentReleasePending => {
            document_release_pending_phase_detail(status).to_owned()
        }
        HarnessPhase::FinalReviewPending => {
            if status.current_branch_closure_id.is_none() {
                String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS)
            } else if status.current_release_readiness_state.as_deref() != Some("ready") {
                if status.current_release_readiness_state.as_deref() == Some("blocked") {
                    String::from(phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED)
                } else {
                    String::from(phase::DETAIL_RELEASE_READINESS_RECORDING_READY)
                }
            } else if final_review_dispatch_id.is_some()
                && shared_final_review_dispatch_still_current(Some(gate_review), Some(gate_finish))
            {
                String::from(phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING)
            } else {
                String::from(phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED)
            }
        }
        HarnessPhase::QaPending => {
            if status.current_branch_closure_id.is_none() {
                String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS)
            } else if shared_normalized_plan_qa_requirement(
                context.plan_document.qa_requirement.as_deref(),
            )
            .as_deref()
                == Some("required")
                && qa_pending_requires_test_plan_refresh(context, Some(gate_finish))
            {
                String::from(phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED)
            } else {
                String::from(phase::DETAIL_QA_RECORDING_REQUIRED)
            }
        }
        HarnessPhase::ExecutionPreflight => {
            String::from(phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED)
        }
        HarnessPhase::Executing => {
            if status.active_task.is_some()
                || status.blocking_step.is_some()
                || status.resume_task.is_some()
            {
                String::from(phase::DETAIL_EXECUTION_IN_PROGRESS)
            } else {
                String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED)
            }
        }
        HarnessPhase::PivotRequired => String::from(phase::DETAIL_PLANNING_REENTRY_REQUIRED),
        HarnessPhase::HandoffRequired => String::from(phase::DETAIL_HANDOFF_RECORDING_REQUIRED),
        _ => String::from(phase::DETAIL_EXECUTION_IN_PROGRESS),
    }
}

pub(crate) fn document_release_pending_phase_detail(status: &PlanExecutionStatus) -> &'static str {
    match (
        status.current_release_readiness_state.as_deref(),
        status.release_docs_state,
    ) {
        (Some("blocked"), _) => phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
        (_, DownstreamFreshnessState::Fresh) => phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
        _ => phase::DETAIL_RELEASE_READINESS_RECORDING_READY,
    }
}

fn task_closure_baseline_repair_candidate_reason_present(status: &PlanExecutionStatus) -> bool {
    shared_task_closure_baseline_repair_candidate_reason_present(status)
}

#[cfg(test)]
fn derive_public_next_action(
    status: &PlanExecutionStatus,
    phase_detail: &str,
    _recommended_command: Option<&str>,
) -> String {
    let kind = match phase_detail {
        phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED => NextActionKind::Begin,
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING => NextActionKind::WaitForTaskReviewResult,
        phase::DETAIL_TASK_CLOSURE_RECORDING_READY => NextActionKind::CloseCurrentTask,
        phase::DETAIL_FINISH_COMPLETION_GATE_READY | phase::DETAIL_FINISH_REVIEW_GATE_READY => {
            NextActionKind::FinishBranch
        }
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        | phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | phase::DETAIL_FINAL_REVIEW_RECORDING_READY => NextActionKind::AdvanceLateStage,
        phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED => NextActionKind::AdvanceLateStage,
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => NextActionKind::RequestFinalReview,
        phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING => NextActionKind::WaitForFinalReviewResult,
        phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED => NextActionKind::RefreshTestPlan,
        phase::DETAIL_QA_RECORDING_REQUIRED => NextActionKind::RunQa,
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => NextActionKind::Resume,
        phase::DETAIL_HANDOFF_RECORDING_REQUIRED => NextActionKind::Handoff,
        phase::DETAIL_PLANNING_REENTRY_REQUIRED => NextActionKind::PlanningReentry,
        _ => NextActionKind::Resume,
    };
    public_next_action_text(&NextActionDecision {
        kind,
        phase: status
            .phase
            .clone()
            .unwrap_or_else(|| String::from(phase::PHASE_EXECUTING)),
        phase_detail: String::from(phase_detail),
        review_state_status: status.review_state_status.clone(),
        task_number: status.active_task.or(status.resume_task),
        step_number: status.active_step.or(status.resume_step),
        blocking_task: status.blocking_task,
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_public_command: None,
    })
}

fn derive_public_blocking_records(
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
) -> Vec<StatusBlockingRecord> {
    if let Some(blocking_record) = TargetlessStaleReconcile::status_blocking_record(status) {
        return vec![blocking_record];
    }

    if status
        .reason_codes
        .iter()
        .any(|reason| reason == "derived_review_state_missing")
    {
        if branch_scope_missing_derived_overlays_require_rerecord(status) {
            return vec![StatusBlockingRecord {
                code: String::from("missing_current_closure"),
                scope_type: String::from("branch"),
                scope_key: String::from("current"),
                record_type: String::from("branch_closure"),
                record_id: None,
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("advance_late_stage")),
                message: String::from(
                    "The active late-stage phase requires a current branch closure, but the authoritative branch-closure record is missing and must be re-recorded before late-stage progression can continue.",
                ),
            }];
        }
        let scope_key = status
            .current_branch_closure_id
            .clone()
            .or_else(|| {
                status
                    .current_task_closures
                    .first()
                    .map(|closure| closure.closure_record_id.clone())
            })
            .unwrap_or_else(|| String::from("current"));
        return vec![StatusBlockingRecord {
            code: String::from("derived_review_state_missing"),
            scope_type: String::from(if scope_key.starts_with("task-") {
                "task"
            } else {
                "branch"
            }),
            scope_key: scope_key.clone(),
            record_type: String::from("review_state"),
            record_id: Some(scope_key),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("repair_review_state")),
            message: String::from(
                "Derived review-state overlays or milestone indexes are missing and must be repaired before late-stage progression can continue.",
            ),
        }];
    }

    if status.review_state_status == "stale_unreviewed" {
        if status.stale_unreviewed_closures.is_empty() {
            return TargetlessStaleReconcile::status_blocking_record(status)
                .into_iter()
                .collect();
        }
        let late_stage_surface_not_declared = status
            .reason_codes
            .iter()
            .any(|reason| reason == "late_stage_surface_not_declared");
        let code = if late_stage_surface_not_declared {
            String::from("late_stage_surface_not_declared")
        } else {
            String::from("stale_unreviewed")
        };
        let message = if late_stage_surface_not_declared {
            String::from(
                "The current reviewed state is stale, and the approved plan does not declare Late-Stage Surface metadata to classify post-closure drift as trusted late-stage-only. Repair review state must reroute through execution reentry.",
            )
        } else {
            String::from(
                "The current reviewed state is stale because later workspace changes landed after the latest reviewed closure.",
            )
        };
        return status
            .stale_unreviewed_closures
            .iter()
            .cloned()
            .map(|scope_key| StatusBlockingRecord {
                code: code.clone(),
                scope_type: String::from(if scope_key.starts_with("task-") {
                    "task"
                } else {
                    "branch"
                }),
                scope_key: scope_key.clone(),
                record_type: String::from("review_state"),
                record_id: Some(scope_key),
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("repair_review_state")),
                message: message.clone(),
            })
            .collect();
    }

    if let Some(reason_code) = task_scope_structural_review_state_reason(status) {
        let task_number = status
            .blocking_task
            .or_else(|| {
                status
                    .current_task_closures
                    .first()
                    .map(|closure| closure.task)
            })
            .unwrap_or_default();
        let message = match reason_code {
            "prior_task_current_closure_invalid" => format!(
                "Task {task_number} is blocked because the current task-closure provenance is not valid for the active approved plan."
            ),
            "prior_task_current_closure_reviewed_state_malformed" => format!(
                "Task {task_number} is blocked because the current task-closure reviewed-state identity is malformed."
            ),
            _ => format!(
                "Task {task_number} is blocked because the current task-closure review state requires repair before execution can continue."
            ),
        };
        return vec![StatusBlockingRecord {
            code: reason_code.to_owned(),
            scope_type: String::from("task"),
            scope_key: format!("task-{task_number}"),
            record_type: String::from("review_state"),
            record_id: status
                .current_task_closures
                .iter()
                .find(|closure| closure.task == task_number)
                .map(|closure| closure.closure_record_id.clone()),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("repair_review_state")),
            message,
        }];
    }

    if status.current_branch_closure_id.is_some()
        && let Some(reason_code) = current_branch_closure_structural_review_state_reason(status)
    {
        let branch_closure_id = status
            .current_branch_closure_id
            .clone()
            .unwrap_or_else(|| String::from("current"));
        let message = match reason_code {
            "current_branch_closure_reviewed_state_malformed" => format!(
                "Branch closure {branch_closure_id} is blocked because the current branch-closure reviewed-state identity is malformed."
            ),
            _ => format!(
                "Branch closure {branch_closure_id} requires review-state repair before late-stage progression can continue."
            ),
        };
        return vec![StatusBlockingRecord {
            code: reason_code.to_owned(),
            scope_type: String::from("branch"),
            scope_key: branch_closure_id.clone(),
            record_type: String::from("review_state"),
            record_id: Some(branch_closure_id),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("repair_review_state")),
            message,
        }];
    }

    if status.review_state_status == "missing_current_closure" {
        if execution_reentry_requires_review_state_repair(None, status)
            || status.reason_codes.iter().any(|reason| {
                matches!(
                    reason.as_str(),
                    "late_stage_surface_not_declared" | "branch_drift_escapes_late_stage_surface"
                )
            })
        {
            let scope_key = status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current"));
            let late_stage_surface_not_declared = status
                .reason_codes
                .iter()
                .any(|reason| reason == "late_stage_surface_not_declared");
            return vec![StatusBlockingRecord {
                code: String::from("missing_current_closure"),
                scope_type: String::from("branch"),
                scope_key: scope_key.clone(),
                record_type: String::from("review_state"),
                record_id: Some(scope_key),
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("repair_review_state")),
                message: if late_stage_surface_not_declared {
                    String::from(
                        "The current branch closure cannot be safely re-recorded because the approved plan does not declare Late-Stage Surface metadata for classifying post-closure drift. Repair review state must reroute through execution reentry before late-stage progression can continue.",
                    )
                } else {
                    String::from(
                        "The current branch closure can no longer be safely re-recorded from authoritative current task-closure truth, so review-state repair must reroute execution before late-stage progression can continue.",
                    )
                },
            }];
        }
        return vec![StatusBlockingRecord {
            code: String::from("missing_current_closure"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "The current branch closure must be recorded before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("release_readiness"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("resolve_release_blocker")),
            message: String::from(
                "The latest release-readiness result for the current branch closure is blocked and must be resolved before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_RELEASE_READINESS_RECORDING_READY {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_RELEASE_READINESS_RECORDING_READY),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("release_readiness"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "A current release-readiness result for the active branch closure is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("final_review_dispatch"),
            record_id: None,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("request_external_review")),
            message: String::from(
                "A fresh external final review is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_QA_RECORDING_REQUIRED {
        return vec![StatusBlockingRecord {
            code: String::from(phase::DETAIL_QA_RECORDING_REQUIRED),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("qa_result"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "A current QA result for the active branch closure is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == phase::DETAIL_FINISH_COMPLETION_GATE_READY && !gate_finish.allowed {
        return vec![StatusBlockingRecord {
            code: String::from("finish_review_gate_checkpoint_missing"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("finish_review_gate_pass_checkpoint"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "The current branch closure still needs a fresh finish-review checkpoint before branch completion can proceed.",
            ),
        }];
    }

    Vec::new()
}

fn branch_scope_missing_derived_overlays_require_rerecord(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_none()
        && status.review_state_status == "missing_current_closure"
        && status.phase_detail
            == phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
}

fn derive_structural_current_task_closure_blocking_records(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Result<Option<Vec<StatusBlockingRecord>>, JsonFailure> {
    if task_scope_structural_review_state_reason(status).is_none() {
        return Ok(None);
    }
    let structural_records = structural_current_task_closure_failures(context)?
        .into_iter()
        .filter_map(|failure| {
            let message = match failure.reason_code.as_str() {
                "prior_task_current_closure_invalid" => match failure.task {
                    Some(task_number) => format!(
                        "Task {task_number} is blocked because the current task-closure provenance is not valid for the active approved plan."
                    ),
                    None => format!(
                        "Current task-closure entry `{}` is blocked because its authoritative provenance is not valid for the active approved plan.",
                        failure.scope_key
                    ),
                },
                "prior_task_current_closure_reviewed_state_malformed" => {
                    let task_number = failure.task?;
                    format!(
                        "Task {task_number} is blocked because the current task-closure reviewed-state identity is malformed."
                    )
                }
                _ => return None,
            };
            Some(StatusBlockingRecord {
                code: failure.reason_code,
                scope_type: String::from("task"),
                scope_key: failure.scope_key,
                record_type: String::from("review_state"),
                record_id: failure.closure_record_id,
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("repair_review_state")),
                message,
            })
        })
        .collect::<Vec<_>>();
    if !structural_records.is_empty() {
        return Ok(Some(structural_records));
    }
    Ok(None)
}

fn derive_structural_current_branch_closure_blocking_records(
    status: &PlanExecutionStatus,
) -> Option<Vec<StatusBlockingRecord>> {
    let reason_code = current_branch_closure_structural_review_state_reason(status)?;
    let branch_closure_id = status.current_branch_closure_id.clone()?;
    let message = match reason_code {
        "current_branch_closure_reviewed_state_malformed" => format!(
            "Branch closure {branch_closure_id} is blocked because the current branch-closure reviewed-state identity is malformed."
        ),
        _ => format!(
            "Branch closure {branch_closure_id} requires review-state repair before late-stage progression can continue."
        ),
    };
    Some(vec![StatusBlockingRecord {
        code: reason_code.to_owned(),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_type: String::from("review_state"),
        record_id: Some(branch_closure_id),
        review_state_status: status.review_state_status.clone(),
        required_follow_up: Some(String::from("repair_review_state")),
        message,
    }])
}

pub(crate) fn status_workspace_state_id(context: &ExecutionContext) -> Result<String, JsonFailure> {
    Ok(semantic_workspace_snapshot(context)?.semantic_workspace_tree_id)
}

pub(crate) fn validated_current_branch_closure_identity(
    context: &ExecutionContext,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    validated_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state.as_ref(),
    )
}

fn validated_current_branch_closure_identity_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let state = authoritative_state?;
    let identity = state.bound_current_branch_closure_identity()?;
    let record = state.branch_closure_record(&identity.branch_closure_id)?;
    let current_base_branch = context.current_release_base_branch()?;
    let semantic_contract_identity = branch_definition_identity_for_context(context);
    let contract_identity_matches = identity.contract_identity == record.contract_identity
        && normalized_branch_contract_identity_for_current_truth(
            context,
            &current_base_branch,
            &identity.contract_identity,
        )
        .is_some_and(|normalized| normalized == semantic_contract_identity);
    let late_stage_surface =
        if record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption" {
            normalized_late_stage_surface(&context.plan_source).ok()
        } else {
            None
        };
    let expected_source_task_closure_ids = shared_branch_source_task_closure_ids(
        context,
        &still_current_task_closure_records_from_authoritative_state(context, state).ok()?,
        late_stage_surface.as_deref(),
    );
    let mut normalized_record_source_task_closure_ids = record.source_task_closure_ids.clone();
    normalized_record_source_task_closure_ids.sort();
    normalized_record_source_task_closure_ids.dedup();
    (record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == current_base_branch
        && contract_identity_matches
        && record.source_task_closure_ids.len() == normalized_record_source_task_closure_ids.len()
        && normalized_record_source_task_closure_ids == expected_source_task_closure_ids
        && branch_closure_record_matches_plan_exemption(context, &record))
    .then_some(identity)
}

fn normalized_branch_contract_identity_for_current_truth(
    context: &ExecutionContext,
    _base_branch: &str,
    observed_identity: &str,
) -> Option<String> {
    let semantic = branch_definition_identity_for_context(context);
    (observed_identity == semantic).then_some(semantic)
}

pub(crate) fn usable_current_branch_closure_identity(
    context: &ExecutionContext,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    usable_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state.as_ref(),
    )
}

pub(crate) fn usable_current_branch_closure_identity_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let identity = validated_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state,
    )?;
    resolve_branch_closure_reviewed_tree_sha(
        &context.runtime.repo_root,
        &identity.branch_closure_id,
        &identity.reviewed_state_id,
    )
    .ok()?;
    Some(identity)
}

pub(crate) fn branch_closure_record_matches_plan_exemption(
    context: &ExecutionContext,
    record: &crate::execution::transitions::BranchClosureRecord,
) -> bool {
    if record.provenance_basis != "task_closure_lineage_plus_late_stage_surface_exemption"
        || !record.source_task_closure_ids.is_empty()
    {
        return true;
    }
    let Ok(late_stage_surface) = normalized_late_stage_surface(&context.plan_source) else {
        return false;
    };
    !late_stage_surface.is_empty()
        && parse_late_stage_surface_only_branch_surface(&record._effective_reviewed_branch_surface)
            .is_some_and(|recorded_surface| {
                !recorded_surface.is_empty()
                    && recorded_surface
                        .iter()
                        .all(|entry| path_matches_late_stage_surface(entry, &late_stage_surface))
            })
}

pub(crate) fn task_contract_identity_matches_expected(
    context: &ExecutionContext,
    task_number: u32,
    observed_identity: &str,
) -> Result<bool, JsonFailure> {
    Ok(normalized_task_contract_identity_for_current_truth(
        context,
        task_number,
        observed_identity,
    )?
    .is_some())
}

fn normalized_task_contract_identity_for_current_truth(
    context: &ExecutionContext,
    task_number: u32,
    observed_identity: &str,
) -> Result<Option<String>, JsonFailure> {
    let Some(semantic) = task_definition_identity_for_task(context, task_number)? else {
        return Ok(None);
    };
    Ok((observed_identity == semantic).then_some(semantic))
}

pub(crate) fn current_branch_reviewed_state_id(context: &ExecutionContext) -> Option<String> {
    let identity = usable_current_branch_closure_identity(context)?;
    Some(identity.reviewed_state_id)
}

pub(crate) fn current_branch_closure_id(context: &ExecutionContext) -> Option<String> {
    validated_current_branch_closure_identity(context).map(|identity| identity.branch_closure_id)
}

pub(crate) fn finish_review_gate_pass_branch_closure_id(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    Ok(shared_current_late_stage_branch_bindings(
        load_authoritative_transition_state(context)?.as_ref(),
        current_branch_closure_id(context).as_deref(),
        current_branch_reviewed_state_id(context).as_deref(),
    )
    .finish_review_gate_pass_branch_closure_id)
}

fn push_status_reason_code_once(status: &mut PlanExecutionStatus, reason_code: &str) {
    if !status
        .reason_codes
        .iter()
        .any(|existing| existing == reason_code)
    {
        status.reason_codes.push(reason_code.to_owned());
    }
}

pub(crate) fn task_scope_review_state_repair_reason(status: &PlanExecutionStatus) -> Option<&str> {
    status
        .reason_codes
        .iter()
        .map(String::as_str)
        .find(|code| {
            matches!(
                *code,
                "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
            )
        })
        .or_else(|| {
            status
                .reason_codes
                .iter()
                .map(String::as_str)
                .find(|code| matches!(*code, "prior_task_current_closure_stale"))
        })
}

pub(crate) fn current_branch_closure_structural_review_state_reason(
    status: &PlanExecutionStatus,
) -> Option<&str> {
    status
        .reason_codes
        .iter()
        .map(String::as_str)
        .find(|code| matches!(*code, "current_branch_closure_reviewed_state_malformed"))
}

pub(crate) fn task_scope_structural_review_state_reason(
    status: &PlanExecutionStatus,
) -> Option<&str> {
    task_scope_review_state_repair_reason(status).filter(|reason_code| {
        matches!(
            *reason_code,
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
        )
    })
}

pub(crate) fn execution_reentry_requires_review_state_repair(
    context: Option<&ExecutionContext>,
    status: &PlanExecutionStatus,
) -> bool {
    let task_scope_repair_required = task_scope_overlay_repair_required(status)
        || task_scope_structural_review_state_reason(status).is_some()
        || (matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        ) && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_stale"));
    if task_closure_baseline_repair_candidate_reason_present(status) && !task_scope_repair_required
    {
        if status.review_state_status == "stale_unreviewed" {
            if let Some(context) = context
                && let Some(task) = status.blocking_task
                && stale_unreviewed_allows_task_closure_baseline_bridge(context, status, task)
                    .unwrap_or(false)
            {
                return false;
            }
        } else {
            return false;
        }
    }
    execution_reentry_repair_projection_active(status)
        || (status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "derived_review_state_missing")
            && (status.current_branch_closure_id.is_some()
                || task_scope_overlay_repair_required(status)))
        || (status.current_branch_closure_id.is_some()
            && current_branch_closure_structural_review_state_reason(status).is_some())
        || task_scope_repair_required
}

fn execution_reentry_repair_projection_active(status: &PlanExecutionStatus) -> bool {
    status.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && (status.review_state_status == "stale_unreviewed"
            || status.reason_codes.iter().any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "derived_review_state_missing"
                        | "prior_task_current_closure_invalid"
                        | "prior_task_current_closure_reviewed_state_malformed"
                        | "current_branch_closure_reviewed_state_malformed"
                )
            }))
}

fn task_scope_overlay_repair_required(status: &PlanExecutionStatus) -> bool {
    status.harness_phase == HarnessPhase::Executing
        && status.reason_codes.iter().any(|reason_code| {
            reason_code == "current_task_closure_overlay_restore_required"
                || reason_code == "task_closure_negative_result_overlay_restore_required"
        })
        && status.current_branch_closure_id.is_none()
}

pub(crate) fn is_late_stage_phase(phase: HarnessPhase) -> bool {
    matches!(
        phase,
        HarnessPhase::FinalReviewPending
            | HarnessPhase::QaPending
            | HarnessPhase::DocumentReleasePending
            | HarnessPhase::ReadyForBranchCompletion
    )
}

fn status_release_blocked(gate_finish: &GateResult) -> bool {
    shared_late_stage_release_blocked(Some(gate_finish))
}

fn status_review_blocked(gate_finish: &GateResult) -> bool {
    shared_late_stage_review_blocked(Some(gate_finish))
}

fn status_review_truth_blocked(gate_review: &GateResult) -> bool {
    shared_late_stage_review_truth_blocked(Some(gate_review))
}

pub(crate) fn final_review_dispatch_still_current_for_gates(
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    shared_final_review_dispatch_still_current(gate_review, gate_finish)
}

fn status_qa_blocked(gate_finish: &GateResult) -> bool {
    shared_late_stage_qa_blocked(Some(gate_finish))
}

pub(crate) fn parse_harness_phase(value: &str) -> Option<HarnessPhase> {
    match value {
        phase::PHASE_IMPLEMENTATION_HANDOFF => Some(HarnessPhase::ImplementationHandoff),
        phase::PHASE_EXECUTION_PREFLIGHT => Some(HarnessPhase::ExecutionPreflight),
        "contract_drafting" => Some(HarnessPhase::ContractDrafting),
        "contract_pending_approval" => Some(HarnessPhase::ContractPendingApproval),
        "contract_approved" => Some(HarnessPhase::ContractApproved),
        phase::PHASE_EXECUTING => Some(HarnessPhase::Executing),
        "evaluating" => Some(HarnessPhase::Evaluating),
        "repairing" => Some(HarnessPhase::Repairing),
        phase::PHASE_PIVOT_REQUIRED => Some(HarnessPhase::PivotRequired),
        phase::PHASE_HANDOFF_REQUIRED => Some(HarnessPhase::HandoffRequired),
        phase::PHASE_FINAL_REVIEW_PENDING => Some(HarnessPhase::FinalReviewPending),
        phase::PHASE_QA_PENDING => Some(HarnessPhase::QaPending),
        phase::PHASE_DOCUMENT_RELEASE_PENDING => Some(HarnessPhase::DocumentReleasePending),
        phase::PHASE_READY_FOR_BRANCH_COMPLETION => Some(HarnessPhase::ReadyForBranchCompletion),
        _ => None,
    }
}

fn parse_aggregate_evaluation_state(value: &str) -> Option<AggregateEvaluationState> {
    match value {
        "pass" => Some(AggregateEvaluationState::Pass),
        "fail" => Some(AggregateEvaluationState::Fail),
        "blocked" => Some(AggregateEvaluationState::Blocked),
        "pending" => Some(AggregateEvaluationState::Pending),
        _ => None,
    }
}

fn parse_downstream_freshness_state(value: &str) -> Option<DownstreamFreshnessState> {
    match value {
        "not_required" => Some(DownstreamFreshnessState::NotRequired),
        "missing" => Some(DownstreamFreshnessState::Missing),
        "fresh" => Some(DownstreamFreshnessState::Fresh),
        "stale" => Some(DownstreamFreshnessState::Stale),
        _ => None,
    }
}

fn parse_overlay_active_contract_fields(
    active_contract_path: Option<&str>,
    active_contract_fingerprint: Option<&str>,
    state_path: &Path,
) -> Result<(Option<String>, Option<String>), JsonFailure> {
    let active_contract_path =
        normalize_optional_overlay_value(active_contract_path).map(str::to_owned);
    let active_contract_fingerprint =
        normalize_optional_overlay_value(active_contract_fingerprint).map(str::to_owned);

    let (Some(active_contract_path), Some(active_contract_fingerprint)) = (
        active_contract_path.clone(),
        active_contract_fingerprint.clone(),
    ) else {
        if active_contract_path.is_some() || active_contract_fingerprint.is_some() {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative harness state must set active_contract_path and active_contract_fingerprint together in {}.",
                    state_path.display()
                ),
            ));
        }
        return Ok((None, None));
    };

    if active_contract_path.contains('/') || active_contract_path.contains('\\') {
        return Err(non_authoritative_overlay_field(
            state_path,
            "active_contract_path",
            &active_contract_path,
            "must be a single authoritative artifact file name",
        ));
    }

    let expected_file = format!("contract-{active_contract_fingerprint}.md");
    if active_contract_path != expected_file {
        let expectation = format!("must match `{expected_file}`");
        return Err(malformed_overlay_field(
            state_path,
            "active_contract_path",
            &active_contract_path,
            &expectation,
        ));
    }

    Ok((
        Some(active_contract_path),
        Some(active_contract_fingerprint),
    ))
}

fn malformed_overlay_field(
    state_path: &Path,
    field_name: &str,
    value: &str,
    expectation: &str,
) -> JsonFailure {
    JsonFailure::new(
        FailureClass::MalformedExecutionState,
        format!(
            "Authoritative harness state field `{field_name}` is malformed in {}: `{value}` ({expectation}).",
            state_path.display()
        ),
    )
}

fn non_authoritative_overlay_field(
    state_path: &Path,
    field_name: &str,
    value: &str,
    expectation: &str,
) -> JsonFailure {
    JsonFailure::new(
        FailureClass::NonAuthoritativeArtifact,
        format!(
            "Authoritative harness state field `{field_name}` is non-authoritative in {}: `{value}` ({expectation}).",
            state_path.display()
        ),
    )
}

fn parse_evaluator_kinds(
    values: &[String],
    field_name: &str,
    state_path: &Path,
) -> Result<Vec<EvaluatorKind>, JsonFailure> {
    values
        .iter()
        .map(|value| {
            let value = value.trim();
            parse_evaluator_kind(value).ok_or_else(|| {
                malformed_overlay_field(
                    state_path,
                    field_name,
                    value,
                    "must contain only spec_compliance or code_quality",
                )
            })
        })
        .collect()
}

fn parse_evaluator_kind(value: &str) -> Option<EvaluatorKind> {
    match value {
        "spec_compliance" => Some(EvaluatorKind::SpecCompliance),
        "code_quality" => Some(EvaluatorKind::CodeQuality),
        _ => None,
    }
}

fn parse_evaluation_verdict(value: &str) -> Option<EvaluationVerdict> {
    match value {
        "pass" => Some(EvaluationVerdict::Pass),
        "fail" => Some(EvaluationVerdict::Fail),
        "blocked" => Some(EvaluationVerdict::Blocked),
        _ => None,
    }
}

fn parse_optional_evaluator_kind(
    value: Option<&str>,
    field_name: &str,
    state_path: &Path,
) -> Result<Option<EvaluatorKind>, JsonFailure> {
    let Some(value) = normalize_optional_overlay_value(value) else {
        return Ok(None);
    };
    parse_evaluator_kind(value).map(Some).ok_or_else(|| {
        malformed_overlay_field(
            state_path,
            field_name,
            value,
            "must be spec_compliance or code_quality",
        )
    })
}

fn parse_optional_evaluation_verdict(
    value: Option<&str>,
    field_name: &str,
    state_path: &Path,
) -> Result<Option<EvaluationVerdict>, JsonFailure> {
    let Some(value) = normalize_optional_overlay_value(value) else {
        return Ok(None);
    };
    parse_evaluation_verdict(value).map(Some).ok_or_else(|| {
        malformed_overlay_field(
            state_path,
            field_name,
            value,
            "must be pass, fail, or blocked",
        )
    })
}

fn parse_optional_downstream_freshness_state(
    value: Option<&str>,
    field_name: &str,
    state_path: &Path,
) -> Result<Option<DownstreamFreshnessState>, JsonFailure> {
    let Some(value) = normalize_optional_overlay_value(value) else {
        return Ok(None);
    };
    parse_downstream_freshness_state(value)
        .map(Some)
        .ok_or_else(|| {
            malformed_overlay_field(
                state_path,
                field_name,
                value,
                "must be not_required, missing, fresh, or stale",
            )
        })
}

fn parse_reason_codes(
    values: &[String],
    field_name: &str,
    state_path: &Path,
) -> Result<Vec<String>, JsonFailure> {
    values
        .iter()
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                return Err(malformed_overlay_field(
                    state_path,
                    field_name,
                    "<empty>",
                    "must contain non-empty strings",
                ));
            }
            Ok(value.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod execution_command_route_target_tests {
    use super::*;
    use crate::test_support::init_committed_test_repo;
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn unresolved_execution_context() -> (TempDir, ExecutionContext, String) {
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/codex-runtime/fixtures/workflow-artifacts");
        let repo_dir = TempDir::new().expect("exact-command temp repo should exist");
        let repo_root = repo_dir.path();
        let plan_rel =
            String::from("docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md");
        let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
        let plan_path = repo_root.join(&plan_rel);
        let spec_path = repo_root.join(spec_rel);

        init_committed_test_repo(
            repo_root,
            "# exact-command-test\n",
            "exact-command unit tests",
        );

        fs::create_dir_all(
            spec_path
                .parent()
                .expect("spec fixture path should have a parent"),
        )
        .expect("spec fixture directory should create");
        fs::create_dir_all(
            plan_path
                .parent()
                .expect("plan fixture path should have a parent"),
        )
        .expect("plan fixture directory should create");
        fs::copy(
            fixture_root.join("specs/2026-03-22-runtime-integration-hardening-design.md"),
            &spec_path,
        )
        .expect("exact-command unit-test spec fixture should copy");
        let plan_source = fs::read_to_string(
            fixture_root.join("plans/2026-03-22-runtime-integration-hardening.md"),
        )
        .expect("exact-command unit-test plan fixture should read")
        .replace(
            "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
            spec_rel,
        );
        fs::write(&plan_path, plan_source)
            .expect("exact-command unit-test plan fixture should write");

        let runtime =
            ExecutionRuntime::discover(repo_root).expect("temp repo runtime should discover");
        let context = load_execution_context(&runtime, Path::new(&plan_rel))
            .expect("runtime integration hardening plan should load for exact-command unit tests");
        (repo_dir, context, plan_rel)
    }

    fn closure_baseline_candidate_context() -> (TempDir, ExecutionContext, String) {
        let (repo_dir, mut context, plan_rel) = unresolved_execution_context();
        for step in &mut context.steps {
            if step.task_number == 1 {
                step.checked = true;
            }
        }
        let head_sha = context
            .current_head_sha()
            .expect("closure-baseline candidate fixture should resolve head sha");
        context.evidence.attempts = context
            .steps
            .iter()
            .filter(|step| step.task_number == 1)
            .map(|step| EvidenceAttempt {
                task_number: step.task_number,
                step_number: step.step_number,
                attempt_number: 1,
                status: String::from("Completed"),
                recorded_at: String::from("2026-04-19T00:00:00Z"),
                execution_source: String::from("featureforge:executing-plans"),
                claim: format!(
                    "closure-baseline candidate fixture completed task {} step {}",
                    step.task_number, step.step_number
                ),
                files: Vec::new(),
                file_proofs: Vec::new(),
                verify_command: None,
                verification_summary: String::from("closure-baseline candidate fixture"),
                invalidation_reason: String::new(),
                packet_fingerprint: Some(format!(
                    "packet-fingerprint-task-{}-step-{}",
                    step.task_number, step.step_number
                )),
                head_sha: Some(head_sha.clone()),
                base_sha: Some(head_sha.clone()),
                source_contract_path: None,
                source_contract_fingerprint: None,
                source_evaluation_report_fingerprint: None,
                evaluator_verdict: None,
                failing_criterion_ids: Vec::new(),
                source_handoff_fingerprint: None,
                repo_state_baseline_head_sha: None,
                repo_state_baseline_worktree_fingerprint: None,
            })
            .collect();
        let authoritative_state_path = authoritative_state_path(&context);
        fs::create_dir_all(
            authoritative_state_path
                .parent()
                .expect("authoritative state path should have a parent"),
        )
        .expect("authoritative state directory should create");
        fs::write(
            &authoritative_state_path,
            serde_json::json!({
                "last_strategy_checkpoint_fingerprint": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "run_identity": {
                    "execution_run_id": "run-exact-phase-detail"
                },
                "task_closure_record_history": {
                    "task-closure-1-historical": {
                        "task": 1,
                        "record_status": "historical"
                    }
                }
            })
            .to_string(),
        )
        .expect("authoritative state for closure-baseline candidate should write");
        (repo_dir, context, plan_rel)
    }

    fn late_stage_status_for_review_state_tests() -> PlanExecutionStatus {
        let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for review-state tests");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::FinalReviewPending;
        status.current_branch_closure_id = Some(String::from("branch-closure-1"));
        status
    }

    #[test]
    fn branch_closure_refresh_missing_current_closure_uses_meaningful_drift_not_raw_id_mismatch() {
        let mut status = late_stage_status_for_review_state_tests();
        status.current_branch_reviewed_state_id = Some(String::from("git_tree:baseline"));
        status.workspace_state_id = String::from("git_tree:current");
        status.current_release_readiness_state = None;

        status.current_branch_meaningful_drift = false;
        assert!(
            !shared_branch_closure_refresh_missing_current_closure(&status),
            "raw reviewed/workspace state-id mismatch without meaningful filtered drift must not trigger branch-closure refresh"
        );

        status.current_branch_meaningful_drift = true;
        assert!(
            shared_branch_closure_refresh_missing_current_closure(&status),
            "branch-closure refresh should trigger when meaningful filtered drift is present"
        );
    }

    #[test]
    fn prerelease_branch_closure_refresh_requires_meaningful_drift_signal() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::DocumentReleasePending;
        status.current_branch_reviewed_state_id = Some(String::from("git_tree:baseline"));
        status.workspace_state_id = String::from("git_tree:current");
        status.current_release_readiness_state = None;

        status.current_branch_meaningful_drift = false;
        assert!(
            !prerelease_branch_closure_refresh_required(&status),
            "DocumentReleasePending must not require branch closure refresh when only raw reviewed/workspace mismatch is present"
        );

        status.current_branch_meaningful_drift = true;
        assert!(
            prerelease_branch_closure_refresh_required(&status),
            "DocumentReleasePending should require branch closure refresh when meaningful filtered drift is present"
        );
    }

    fn gate_result_with_reason(reason_code: &str) -> GateResult {
        GateResult {
            allowed: false,
            action: String::from("blocked"),
            failure_class: String::from("StaleProvenance"),
            reason_codes: vec![reason_code.to_owned()],
            warning_codes: Vec::new(),
            diagnostics: Vec::new(),
            code: None,
            workspace_state_id: None,
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            recommended_command: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
        }
    }

    #[test]
    fn resolve_execution_command_route_target_from_context_uses_first_unchecked_step_without_markers()
     {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for exact-command test");
        status.execution_started = String::from("yes");
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
        status.harness_phase = HarnessPhase::Executing;
        status.execution_mode = String::from("featureforge:executing-plans");

        let resolved = resolve_execution_command_route_target_from_context(
            &context,
            &status,
            plan_rel.as_str(),
        )
        .expect("marker-free started execution should derive the first unchecked step");

        assert_eq!(resolved.command_kind, "begin");
        assert_eq!(resolved.task_number, 1);
        assert_eq!(resolved.step_id, Some(1));
    }

    #[test]
    fn resolve_execution_command_route_target_from_context_fails_closed_for_malformed_active_marker()
     {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for exact-command test");
        status.execution_started = String::from("yes");
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
        status.harness_phase = HarnessPhase::Executing;
        status.active_task = Some(1);
        status.active_step = None;

        assert!(
            resolve_execution_command_route_target_from_context(
                &context,
                &status,
                plan_rel.as_str()
            )
            .is_none(),
            "malformed active execution markers must fail closed instead of synthesizing a begin command"
        );
    }

    #[test]
    fn derive_public_review_state_status_treats_not_fresh_late_gate_reasons_as_stale_unreviewed() {
        for reason_code in [
            "release_docs_state_not_fresh",
            "final_review_state_not_fresh",
            "browser_qa_state_not_fresh",
        ] {
            let status = late_stage_status_for_review_state_tests();
            let gate_review = gate_result_with_reason(reason_code);
            let gate_finish = gate_result_with_reason(reason_code);
            assert_eq!(
                derive_public_review_state_status(
                    &status,
                    &gate_review,
                    &gate_finish,
                    false,
                    false,
                    false,
                    false,
                ),
                "stale_unreviewed",
                "late-stage reason code `{reason_code}` must classify as stale_unreviewed",
            );
        }
    }

    #[test]
    fn derive_public_review_state_status_ignores_late_stage_staleness_during_execution_reentry() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.resume_task = Some(1);
        status.resume_step = Some(1);
        status.current_branch_closure_id = None;

        let gate_review = gate_result_with_reason("release_docs_state_not_fresh");
        let gate_finish = gate_result_with_reason("release_docs_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "clean",
            "late-stage stale gate reasons must not override task-scope execution reentry truth",
        );
    }

    #[test]
    fn derive_public_review_state_status_marks_resumed_late_stage_reroute_as_stale_unreviewed() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.resume_task = Some(1);
        status.resume_step = Some(1);
        status.current_branch_closure_id = None;
        status
            .reason_codes
            .push(String::from(REASON_CODE_STALE_PROVENANCE));

        let gate_review = gate_result_with_reason("release_docs_state_not_fresh");
        let gate_finish = gate_result_with_reason("release_docs_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "stale_unreviewed",
            "a resumed task rerouted out of late-stage phase must require review-state repair",
        );
    }

    #[test]
    fn derive_public_review_state_status_marks_stale_late_stage_truth_even_when_harness_phase_stays_executing()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_release_readiness_state = Some(String::from("ready"));

        let gate_review = gate_result_with_reason("release_docs_state_not_fresh");
        let gate_finish = gate_result_with_reason("release_docs_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "stale_unreviewed",
            "late-stage stale truth must surface from current branch bindings even if harness phase lags in executing",
        );
    }

    #[test]
    fn derive_public_review_state_status_marks_late_stage_stale_provenance_execution_reentry_as_stale_unreviewed()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.blocking_task = Some(1);
        status.current_branch_reviewed_state_id = status.raw_workspace_tree_id.clone();
        status.current_task_closures = vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];
        status
            .reason_codes
            .push(String::from(REASON_CODE_STALE_PROVENANCE));
        status.release_docs_state = DownstreamFreshnessState::Fresh;
        status.final_review_state = DownstreamFreshnessState::Fresh;
        status.browser_qa_state = DownstreamFreshnessState::Fresh;

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                true,
                false,
                false,
                false,
            ),
            "stale_unreviewed",
            "late-stage stale provenance routed back to execution must remain stale_unreviewed for shared review-state consumers",
        );
    }

    #[test]
    fn public_late_stage_stale_unreviewed_requires_bound_late_stage_target_ids() {
        let mut status = late_stage_status_for_review_state_tests();
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.final_review_state = DownstreamFreshnessState::Stale;

        let gate_review = gate_result_with_reason("final_review_state_not_fresh");
        let gate_finish = gate_result_with_reason("final_review_state_not_fresh");

        assert!(
            public_late_stage_rederivation_basis_present(&status),
            "fixture should still surface late-stage informational basis even after bound target ids are cleared"
        );
        assert!(
            !shared_public_late_stage_stale_unreviewed(
                &status,
                Some(&gate_review),
                Some(&gate_finish),
            ),
            "late-stage stale routing must not activate when no branch/final-review/qa binding ids remain"
        );
    }

    #[test]
    fn derive_public_review_state_status_ignores_unbound_late_stage_staleness_after_current_task_closure_refresh()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.final_review_state = DownstreamFreshnessState::Stale;
        status.current_task_closures = vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];

        let gate_review = gate_result_with_reason("final_review_state_not_fresh");
        let gate_finish = gate_result_with_reason("final_review_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "clean",
            "unbound late-stage stale signals must remain informational once the current task closure is refreshed and no late-stage binding ids remain"
        );
    }

    #[test]
    fn task_scope_review_state_repair_reason_prefers_structural_current_closure_failures() {
        let mut status = late_stage_status_for_review_state_tests();
        status.reason_codes = vec![
            String::from("prior_task_current_closure_stale"),
            String::from("prior_task_current_closure_invalid"),
        ];

        assert_eq!(
            task_scope_review_state_repair_reason(&status),
            Some("prior_task_current_closure_invalid")
        );
        assert_eq!(
            task_scope_structural_review_state_reason(&status),
            Some("prior_task_current_closure_invalid")
        );
    }

    #[test]
    fn derive_public_blocking_records_includes_follow_up_for_finish_checkpoint_blocker() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_FINISH_COMPLETION_GATE_READY);
        let gate_finish = gate_result_with_reason("finish_review_gate_checkpoint_missing");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            "finish_review_gate_checkpoint_missing"
        );
        assert_eq!(
            blocking_records[0].required_follow_up,
            Some(String::from("advance_late_stage")),
            "finish-checkpoint blockers should expose a concrete public follow-up lane",
        );
    }

    #[test]
    fn record_review_dispatch_blocked_output_uses_shared_out_of_phase_contract_when_requery_is_required()
     {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let args = RecordReviewDispatchArgs {
            plan: PathBuf::from(&plan_rel),
            scope: ReviewDispatchScopeArg::Task,
            task: Some(1),
        };
        let gate = gate_result_with_reason("task_closure_not_recording_ready");

        let output = record_review_dispatch_blocked_output_from_gate(&context, &args, gate);
        let output_json =
            serde_json::to_value(output).expect("review-dispatch output should serialize");

        assert_eq!(
            output_json["code"],
            Value::from("out_of_phase_requery_required")
        );
        assert_eq!(
            output_json["recommended_command"],
            Value::from(format!(
                "featureforge workflow operator --plan {}",
                context.plan_rel
            ))
        );
        assert_eq!(
            output_json["rederive_via_workflow_operator"],
            Value::Bool(true)
        );
    }

    #[test]
    fn derive_public_blocking_records_omits_task_review_dispatch_required_lane() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from("task_review_dispatch_required");
        status.blocking_task = Some(2);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert!(
            blocking_records.is_empty(),
            "task-review dispatch projection lineage is diagnostic-only and must not create public blockers: {blocking_records:?}"
        );
    }

    #[test]
    fn derive_public_blocking_records_routes_targetless_stale_to_runtime_diagnostic() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("stale_unreviewed");
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.current_task_closures.clear();
        status.reason_codes.clear();
        status.blocking_task = None;
        status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
        TargetlessStaleReconcile::ensure_status_diagnostic(&mut status);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert_eq!(blocking_records[0].required_follow_up, None);
    }

    #[test]
    fn derive_public_blocking_records_never_fabricates_current_branch_for_targetless_stale() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("stale_unreviewed");
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = Some(String::from("branch-closure-current"));
        status.current_task_closures.clear();
        status.reason_codes.clear();
        status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
        TargetlessStaleReconcile::ensure_status_diagnostic(&mut status);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert!(
            blocking_records
                .iter()
                .all(|record| record.scope_key != "current"
                    && record.record_id.as_deref() != Some("current")
                    && record.record_id.as_deref() != Some("branch-closure-current")),
            "targetless stale records must not invent current or branch targets: {blocking_records:?}"
        );
    }

    #[test]
    fn derive_public_blocking_records_targetless_stale_preempts_derived_current_fallback() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("stale_unreviewed");
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = None;
        status.current_task_closures.clear();
        status.reason_codes = vec![String::from("derived_review_state_missing")];
        status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
        TargetlessStaleReconcile::ensure_status_diagnostic(&mut status);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert_eq!(blocking_records[0].required_follow_up, None);
    }

    #[test]
    fn derive_public_next_action_uses_verification_lane_for_task_review_verification_blockers() {
        let mut status = late_stage_status_for_review_state_tests();
        status.phase_detail = String::from(phase::DETAIL_TASK_REVIEW_RESULT_PENDING);
        status.blocking_task = Some(1);
        status.reason_codes = vec![String::from("prior_task_verification_missing")];

        let next_action =
            derive_public_next_action(&status, phase::DETAIL_TASK_REVIEW_RESULT_PENDING, None);
        assert_eq!(
            next_action, "run verification",
            "verification-missing task-boundary blockers should route public next_action through the verification lane"
        );

        status.reason_codes = vec![String::from("prior_task_review_not_green")];
        let wait_action =
            derive_public_next_action(&status, phase::DETAIL_TASK_REVIEW_RESULT_PENDING, None);
        assert_eq!(
            wait_action, "wait for external review result",
            "review-pending blockers should remain in the external-review wait lane"
        );
    }

    #[test]
    fn derive_public_phase_detail_allows_close_current_task_when_baseline_candidate_lacks_dispatch()
    {
        let (_repo_dir, context, _plan_rel) = closure_baseline_candidate_context();
        let mut status = status_from_context(&context)
            .expect("status should derive for task-closure baseline candidate phase-detail test");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::Executing;
        status.review_state_status = String::from("clean");
        status.current_task_closures.clear();
        status.reason_codes = vec![
            String::from("task_closure_baseline_repair_candidate"),
            String::from("prior_task_review_dispatch_missing"),
        ];
        status.blocking_task = Some(1);
        status.blocking_step = None;

        let gate_review = gate_result_with_reason("irrelevant");
        let gate_finish = gate_result_with_reason("irrelevant");
        let phase_detail = derive_public_phase_detail(
            &context,
            &status,
            &gate_review,
            &gate_finish,
            "clean",
            None,
            None,
        );
        assert_eq!(
            phase_detail,
            phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
            "task-closure baseline repair candidates should route directly to closure recording when dispatch lineage can be derived by close-current-task",
        );
        assert_eq!(
            derive_public_next_action(&status, &phase_detail, None),
            "close current task",
            "task-closure baseline repair candidates should keep next_action on close-current-task",
        );
    }

    #[test]
    fn derive_public_phase_detail_keeps_close_current_task_lane_for_verification_pending_baseline_repair()
     {
        let (_repo_dir, context, _plan_rel) = closure_baseline_candidate_context();
        let mut status = status_from_context(&context)
            .expect("status should derive for verification-pending closure routing test");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::Executing;
        status.review_state_status = String::from("clean");
        status.blocking_task = Some(1);
        status.blocking_step = None;
        status.current_task_closures.clear();
        status.reason_codes = vec![
            String::from("prior_task_current_closure_missing"),
            String::from("task_closure_baseline_repair_candidate"),
            String::from("prior_task_verification_missing"),
        ];

        let gate_review = gate_result_with_reason("irrelevant");
        let gate_finish = gate_result_with_reason("irrelevant");
        let phase_detail = derive_public_phase_detail(
            &context,
            &status,
            &gate_review,
            &gate_finish,
            "clean",
            Some("dispatch-task-1"),
            None,
        );
        assert_eq!(
            phase_detail,
            phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
            "verification-pending missing-baseline routes must stay on close-current-task so the mutation guard can return the exact verification follow-up"
        );
        assert_eq!(
            derive_public_next_action(&status, &phase_detail, None),
            "close current task",
            "verification-pending missing-baseline routes must keep next_action on close-current-task"
        );
    }

    #[test]
    fn derive_public_blocking_records_includes_qa_recording_required_lane() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_QA_RECORDING_REQUIRED);
        status.current_branch_closure_id = Some(String::from("branch-closure-qa"));
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            phase::DETAIL_QA_RECORDING_REQUIRED
        );
        assert_eq!(blocking_records[0].scope_type, "branch");
        assert_eq!(blocking_records[0].scope_key, "branch-closure-qa");
        assert_eq!(blocking_records[0].record_type, "qa_result");
        assert_eq!(
            blocking_records[0].required_follow_up,
            Some(String::from("advance_late_stage"))
        );
    }

    #[test]
    fn follow_up_override_pivot_status_check_rejects_body_only_decoy_strings() {
        let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
        let head_sha = context
            .current_head_sha()
            .expect("head sha should resolve for pivot override check");
        let reason_codes = vec![String::from("blocked_on_plan_revision")];
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
            context.plan_rel,
            context.runtime.branch_name,
            context.runtime.repo_slug,
            head_sha,
            expected_decision_reason_codes
        );
        fs::write(&artifact_path, decoy_source).expect("decoy pivot artifact should write");

        let matched = current_workflow_pivot_record_exists_for_status_decision(
            &context,
            &reason_codes,
            Some("required"),
        );
        fs::remove_file(&artifact_path).expect("decoy pivot artifact should clean up");

        assert!(
            !matched,
            "pivot follow_up_override clearing must not accept body-only decoy strings"
        );
    }
}
