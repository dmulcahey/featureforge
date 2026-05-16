use std::collections::BTreeSet;

#[cfg(test)]
use crate::contracts::plan::PLAN_QA_REQUIREMENT_VALUES;
use crate::diagnostics::JsonFailure;
use crate::execution::context::{ExecutionContext, NoteState};
use crate::execution::current_closure_projection::{
    project_current_task_closure_repair_reason_codes_from_authoritative_state,
    still_current_task_closure_records_from_authoritative_state,
};
use crate::execution::current_truth::{
    BranchRerecordingAssessment, REASON_BRANCH_DRIFT_ESCAPES_LATE_STAGE_SURFACE,
    REASON_LATE_STAGE_SURFACE_NOT_DECLARED, ReviewStateRepairReroute,
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    branch_closure_rerecording_assessment_with_authority,
    final_review_dispatch_still_current as shared_final_review_dispatch_still_current,
    late_stage_surface_not_declared_reason_code as shared_late_stage_surface_not_declared_reason_code,
    task_scope_overlay_restore_required as shared_task_scope_overlay_restore_required,
};
use crate::execution::harness::{
    AggregateEvaluationState, DownstreamFreshnessState, ExecutionRunId, HarnessPhase,
    INITIAL_AUTHORITATIVE_SEQUENCE,
};
use crate::execution::leases::{
    StatusAuthoritativeOverlay, load_status_authoritative_overlay_checked,
};
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::projection_renderer::{
    ProjectionReadModelDetail, execution_projection_read_model_metadata_with_detail,
    normal_projection_write_mode,
};
use crate::execution::reentry_reconcile::task_closure_baseline_repair_candidate_reason_present as shared_task_closure_baseline_repair_candidate_reason_present;
use crate::execution::repair_route_decision::{
    NextActionAuthorityReadScope, next_action_authority_inputs_from_stale_projection,
    repair_follow_up_decision,
};
use crate::execution::resume_stale_precedence::{
    ResumeStalePrecedence, ResumeStatusSuppressionInputs,
};
use crate::execution::review_route_tokens::REASON_DERIVED_REVIEW_STATE_MISSING;
use crate::execution::semantic_identity::semantic_workspace_snapshot;
use crate::execution::stale_target_projection::{
    StaleTargetProjection, StaleTargetProjectionInputs, closure_baseline_candidate_task,
    project_authoritative_stale_targets,
};
use crate::execution::status::{GateProjectionInputs, GateResult, GateState, PlanExecutionStatus};
use crate::execution::status_support::{
    TaskBoundaryAuthorityInputs, active_step, authoritative_execution_run_id_from_state,
    current_execution_run_id_with_authority, execution_started,
    projected_earliest_stale_task_from_status,
    task_closure_baseline_repair_candidate_with_stale_target_and_authority,
};
use crate::execution::topology::{pending_chunk_id, preflight_acceptance_for_context};
use crate::execution::transitions::{
    AuthoritativeTransitionState, load_authoritative_transition_state,
};
#[cfg(test)]
use crate::workflow::pivot::{
    WorkflowPivotRecordIdentity, current_workflow_pivot_record_exists, pivot_decision_reason_codes,
};

mod blocking_records;
mod branch_gate;
mod exact_route;
mod exact_route_surfaces;
mod exact_route_template;
mod facts;
mod late_stage;
mod overlay;
mod public_warnings;
mod review_state;
mod task_state;

pub(crate) use crate::execution::status_support::{
    normalize_optional_overlay_value, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason,
};
#[cfg(test)]
pub(crate) use blocking_records::derive_public_blocking_records;
use blocking_records::project_worktree_lease_gate_blockers;
pub(crate) use blocking_records::{
    compute_status_blocking_records, current_branch_closure_structural_review_state_reason,
    execution_reentry_requires_review_state_repair,
    execution_reentry_requires_review_state_repair_with_authority,
};
pub(crate) use branch_gate::{
    branch_closure_record_matches_plan_exemption,
    current_branch_gate_bindings_from_authoritative_state, usable_current_branch_closure_identity,
    usable_current_branch_closure_identity_from_authoritative_state,
    validated_current_branch_closure_identity,
    validated_current_branch_closure_identity_from_authoritative_state,
};
pub(crate) use exact_route::require_public_execution_command_route_target;
pub(crate) use facts::{
    StatusAssemblyFacts, StatusRepairFollowUpFacts, StatusReviewStateFacts,
    StatusReviewStateInputs, effective_review_state_status, effective_route_review_state_status,
    prerelease_branch_closure_refresh_required,
};
use late_stage::authoritative_late_stage_rederivation_basis_present_with_authority;
pub(crate) use late_stage::{
    apply_late_stage_precedence_status_overlay, has_authoritative_late_stage_progress,
};
pub(crate) use overlay::{
    apply_authoritative_status_overlay, missing_derived_review_state_fields, parse_harness_phase,
};
pub(crate) use public_warnings::public_status_warning_code;
pub(crate) use review_state::{
    FinalReviewDispatchAuthority, current_final_review_dispatch_authority_for_context,
    current_task_review_dispatch_id_for_status, derive_status_review_state_fact,
    shared_repair_review_state_reroute_decision,
};
pub(crate) use task_state::{
    ExecutionReentryCurrentTaskClosureTargets, apply_task_boundary_status_overlay,
    execution_reentry_current_task_closure_targets_from_inputs, recommended_execution_source,
};
use task_state::{
    TaskBoundaryStatusInputs, completed_plan_missing_current_closure_task_from_records,
};

pub(crate) struct SharedRepairReviewStateRerouteDecision {
    pub(crate) branch_rerecording_assessment: Option<BranchRerecordingAssessment>,
    pub(crate) branch_reroute_still_valid: bool,
    pub(crate) branch_drift_escapes_late_stage_surface: bool,
    pub(crate) late_stage_surface_not_declared: bool,
    pub(crate) persisted_repair_follow_up: Option<String>,
    pub(crate) raw_late_stage_review_state_status: Option<&'static str>,
    pub(crate) task_scope_repair_precedence_active: bool,
    pub(crate) repair_reroute: ReviewStateRepairReroute,
}

pub fn status_from_context(context: &ExecutionContext) -> Result<PlanExecutionStatus, JsonFailure> {
    status_from_context_with_overlay(context, None, false, None, false, None)
}

pub(crate) struct StatusAssemblyOutput {
    pub(crate) status: PlanExecutionStatus,
    pub(crate) facts: StatusAssemblyFacts,
}

pub(crate) fn status_from_context_with_overlay(
    context: &ExecutionContext,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<PlanExecutionStatus, JsonFailure> {
    Ok(status_with_facts_from_context_with_overlay(
        context,
        preloaded_overlay,
        use_preloaded_overlay,
        preloaded_authoritative_state,
        use_preloaded_authoritative_state,
        gate_projection,
    )?
    .status)
}

pub(crate) fn status_with_facts_from_context_with_overlay(
    context: &ExecutionContext,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<StatusAssemblyOutput, JsonFailure> {
    status_with_facts_from_context_with_overlay_and_projection_detail(
        context,
        preloaded_overlay,
        use_preloaded_overlay,
        preloaded_authoritative_state,
        use_preloaded_authoritative_state,
        gate_projection,
        ProjectionReadModelDetail::Full,
    )
}

pub(crate) fn status_with_facts_from_context_with_overlay_and_projection_detail(
    context: &ExecutionContext,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
    projection_detail: ProjectionReadModelDetail,
) -> Result<StatusAssemblyOutput, JsonFailure> {
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
    let projection_metadata = execution_projection_read_model_metadata_with_detail(
        context,
        normal_projection_write_mode()?,
        projection_detail,
    )?;
    let loaded_overlay;
    let authoritative_overlay = if use_preloaded_overlay {
        preloaded_overlay
    } else {
        loaded_overlay = load_status_authoritative_overlay_checked(context)?;
        loaded_overlay.as_ref()
    };

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
        phase_detail: String::new(),
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
        state_kind: String::new(),
        next_public_action: None,
        blockers: Vec::new(),
        runtime_provenance: None,
        semantic_workspace_tree_id: semantic_snapshot.semantic_workspace_tree_id,
        raw_workspace_tree_id: Some(semantic_snapshot.raw_workspace_tree_id),
        next_action: String::new(),
        recommended_public_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
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

    apply_authoritative_status_overlay(context, &mut status, authoritative_overlay, true)?;
    let late_stage_basis_present_for_task_boundary =
        authoritative_late_stage_rederivation_basis_present_with_authority(
            context,
            &status,
            authoritative_state,
            authoritative_overlay,
        );
    let current_task_closure_tasks_for_status =
        current_task_closure_tasks_for_status_projection(context, authoritative_state);
    let authoritative_late_stage_progress_present =
        authoritative_overlay.is_some_and(has_authoritative_late_stage_progress);
    apply_task_boundary_status_overlay(
        context,
        &mut status,
        TaskBoundaryStatusInputs {
            late_stage_basis_present: late_stage_basis_present_for_task_boundary,
            current_task_closure_tasks: &current_task_closure_tasks_for_status,
            authoritative_late_stage_progress_present,
            authority: TaskBoundaryAuthorityInputs::new(authoritative_overlay, authoritative_state),
            branch_rerecording_assessment: None,
        },
    );
    apply_current_task_closure_repair_status_overlay(context, &mut status, authoritative_state);
    suppress_preempted_resume_status_fields(
        context,
        &mut status,
        authoritative_overlay.and_then(|overlay| overlay.strategy_cycle_break_task),
        authoritative_overlay,
        authoritative_state,
        None,
    );
    apply_late_stage_precedence_status_overlay(
        context,
        &mut status,
        authoritative_state,
        authoritative_overlay,
        gate_projection,
    );
    let facts = populate_public_status_contract_fields(
        context,
        &mut status,
        authoritative_overlay,
        true,
        authoritative_state,
        true,
        gate_projection,
    )?;
    Ok(StatusAssemblyOutput { status, facts })
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

pub(crate) fn apply_current_task_closure_repair_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) {
    if context.steps.iter().any(|step| !step.checked) {
        return;
    }
    for reason_code in project_current_task_closure_repair_reason_codes_from_authoritative_state(
        context,
        authoritative_state,
    ) {
        push_status_reason_code_once(status, &reason_code);
    }
}

pub(crate) fn suppress_preempted_resume_status_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    strategy_cycle_break_task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: Option<&BranchRerecordingAssessment>,
) {
    let projected_earliest_stale_task = projected_earliest_stale_task_from_status(status);
    let task_closure_baseline_bridge_preempts_resume = task_closure_baseline_bridge_preempts_resume(
        context,
        status,
        projected_earliest_stale_task,
        overlay,
        authoritative_state,
        branch_rerecording_assessment,
    );
    if ResumeStalePrecedence::for_status_suppression(ResumeStatusSuppressionInputs {
        status,
        strategy_cycle_break_task,
        task_closure_baseline_bridge_preempts_resume,
    })
    .resume_preempted_by_stale()
    {
        status.resume_task = None;
        status.resume_step = None;
    }
}

fn current_task_closure_tasks_for_status_projection(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> BTreeSet<u32> {
    authoritative_state
        .and_then(|state| {
            still_current_task_closure_records_from_authoritative_state(context, state).ok()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|record| record.task)
        .collect()
}

fn task_closure_baseline_bridge_preempts_resume(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    projected_earliest_stale_task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: Option<&BranchRerecordingAssessment>,
) -> bool {
    let Some(resume_task) = status.resume_task else {
        return false;
    };
    if projected_earliest_stale_task.is_some_and(|earliest_task| earliest_task < resume_task) {
        return false;
    }
    let Some(blocking_task) = status.blocking_task else {
        return false;
    };
    let fallback_branch_rerecording_assessment;
    let branch_rerecording_assessment = match branch_rerecording_assessment {
        Some(assessment) => assessment,
        None => {
            let Some(authoritative_state) = authoritative_state else {
                return false;
            };
            let Ok(assessment) = branch_closure_rerecording_assessment_with_authority(
                context,
                Some(authoritative_state),
            ) else {
                return false;
            };
            fallback_branch_rerecording_assessment = assessment;
            &fallback_branch_rerecording_assessment
        }
    };
    task_closure_baseline_repair_candidate_with_stale_target_and_authority(
        context,
        status,
        blocking_task,
        projected_earliest_stale_task,
        overlay,
        authoritative_state,
        branch_rerecording_assessment,
    )
    .ok()
    .flatten()
    .is_some()
        && crate::execution::status_support::stale_unreviewed_allows_task_closure_baseline_bridge_with_authority(
            context,
            status,
            blocking_task,
            overlay,
            authoritative_state,
        )
        .unwrap_or(false)
}

pub(crate) fn populate_public_status_contract_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<StatusAssemblyFacts, JsonFailure> {
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
    let current_late_stage_branch_closure_id =
        late_stage::apply_authoritative_late_stage_status_fields(
            context,
            status,
            overlay,
            event_authority_state,
        )?;

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
    project_gate_warning_codes(status, gate_review, gate_finish);
    let missing_derived_overlays =
        missing_derived_review_state_fields(event_authority_state, overlay);
    if !missing_derived_overlays.is_empty() {
        crate::execution::closure_diagnostics::push_projection_diagnostic_once(
            status,
            REASON_DERIVED_REVIEW_STATE_MISSING,
        );
    }
    project_worktree_lease_gate_blockers(status, gate_review);
    let task_closure_overlay_needs_restore = event_authority_state
        .is_some_and(AuthoritativeTransitionState::current_task_closure_overlay_needs_restore);
    let task_closure_overlay_recovered_from_history =
        task_closure_overlay_needs_restore && !status.current_task_closures.is_empty();
    let task_scope_overlay_restore_required = status.execution_started == "yes"
        && !task_closure_overlay_recovered_from_history
        && shared_task_scope_overlay_restore_required(
            &missing_derived_overlays,
            event_authority_state,
        );
    if task_closure_overlay_needs_restore && !task_closure_overlay_recovered_from_history {
        push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED);
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
        push_status_reason_code_once(status, REASON_LATE_STAGE_SURFACE_NOT_DECLARED);
    }
    if branch_drift_escapes_late_stage_surface {
        push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
        push_status_reason_code_once(status, REASON_BRANCH_DRIFT_ESCAPES_LATE_STAGE_SURFACE);
    }
    let persisted_repair_follow_up = repair_route_decision.persisted_repair_follow_up.as_deref();
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
            .any(|reason_code| shared_late_stage_surface_not_declared_reason_code(reason_code))
        && let Some(missing_task) = completed_plan_missing_current_closure_task_from_records(
            context,
            &current_task_closure_tasks_for_status_projection(context, event_authority_state),
        )
    {
        push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING);
        status.blocking_task = Some(missing_task);
        status.blocking_step = None;
    }
    let repair_stale_projection = project_status_stale_facts(
        context,
        status,
        overlay,
        event_authority_state,
        gate_review,
        gate_finish,
    )?;
    apply_task_closure_baseline_bridge_from_stale_projection(
        context,
        status,
        repair_stale_projection.earliest_task_stale_target(),
        overlay,
        event_authority_state,
        repair_route_decision.branch_rerecording_assessment.as_ref(),
    )?;
    let repair_authority_inputs = next_action_authority_inputs_from_stale_projection(
        status,
        &repair_stale_projection,
        NextActionAuthorityReadScope {
            overlay,
            authoritative_state: event_authority_state,
            persisted_repair_follow_up,
            branch_rerecording_assessment: repair_route_decision
                .branch_rerecording_assessment
                .as_ref(),
            gate_finish: Some(gate_finish),
            ..NextActionAuthorityReadScope::default()
        },
    );
    let repair_follow_up_route_decision = repair_follow_up_decision(
        context,
        status,
        context.plan_rel.as_str(),
        repair_authority_inputs,
        repair_reroute,
    );
    let repair_follow_up_requires_execution_reentry =
        repair_follow_up_route_decision.requires_execution_reentry();
    let persisted_branch_reroute_without_current_binding =
        !repair_follow_up_requires_execution_reentry
            && persisted_repair_follow_up
                == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE)
            && !repair_route_decision.task_scope_repair_precedence_active
            && branch_reroute_still_valid
            && status.current_branch_closure_id.is_some();
    let persisted_branch_reroute_with_current_binding = !repair_follow_up_requires_execution_reentry
        && persisted_repair_follow_up
            == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE)
        && !repair_route_decision.task_scope_repair_precedence_active
        && branch_reroute_still_valid
        && repair_route_decision.raw_late_stage_review_state_status
            == Some(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED)
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
        && status.current_branch_closure_id.is_some();
    let repair_follow_up_records_branch_closure = repair_reroute
        == ReviewStateRepairReroute::RecordBranchClosure
        || persisted_branch_reroute_without_current_binding
        || persisted_branch_reroute_with_current_binding;
    let repair_follow_up_facts = StatusRepairFollowUpFacts::from_decisions(
        &repair_route_decision,
        &repair_follow_up_route_decision,
        repair_follow_up_records_branch_closure,
    );
    let persisted_repair_follow_up = repair_follow_up_facts.persisted_repair_follow_up.as_deref();
    let branch_closure_refresh_missing_current_closure =
        shared_branch_closure_refresh_missing_current_closure(status);
    let task_boundary_unresolved_stale = repair_stale_projection
        .earliest_task_stale_target()
        .is_some();
    let review_state_inputs = StatusReviewStateInputs {
        repair_follow_up_requires_execution_reentry: repair_follow_up_facts
            .requires_execution_reentry,
        repair_follow_up_records_branch_closure: repair_follow_up_facts.records_branch_closure,
        branch_scope_stale_unreviewed: branch_drift_escapes_late_stage_surface,
        task_boundary_unresolved_stale,
    };
    let review_state_status =
        derive_status_review_state_fact(status, gate_review, gate_finish, &review_state_inputs);
    let mut review_state_facts = StatusReviewStateFacts {
        inputs: review_state_inputs,
        status: review_state_status,
    };
    status
        .review_state_status
        .clone_from(&review_state_facts.status);
    let persisted_branch_reroute_viable = persisted_repair_follow_up
        == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE)
        && status.current_branch_closure_id.is_some();
    let branch_closure_recording_basis_missing = status.review_state_status
        == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && !repair_follow_up_facts.branch_reroute_still_valid
        && !branch_closure_refresh_missing_current_closure
        && !persisted_branch_reroute_viable;
    late_stage::apply_late_stage_repair_status_overlay(
        context,
        status,
        late_stage::LateStageRepairStatusOverlayInputs {
            gate_finish,
            overlay,
            event_authority_state,
            current_late_stage_branch_closure_id: current_late_stage_branch_closure_id.as_deref(),
            repair_follow_up_facts: &repair_follow_up_facts,
            task_scope_overlay_restore_required,
            branch_closure_recording_basis_missing,
        },
    )?;
    review_state_facts
        .status
        .clone_from(&status.review_state_status);
    reset_route_projection_fields_at_status_boundary(status);
    let final_stale_projection = project_status_stale_facts(
        context,
        status,
        overlay,
        event_authority_state,
        gate_review,
        gate_finish,
    )?;
    status.blocking_records = compute_status_blocking_records(
        context,
        status,
        gate_finish,
        Some(&final_stale_projection.stale_targets),
        event_authority_state,
    )?;
    let facts = StatusAssemblyFacts {
        stale_projection: final_stale_projection,
        repair_follow_up: repair_follow_up_facts,
        review_state: review_state_facts,
    };

    Ok(facts)
}

fn project_status_stale_facts(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
    event_authority_state: Option<&AuthoritativeTransitionState>,
    gate_review: &GateResult,
    gate_finish: &GateResult,
) -> Result<StaleTargetProjection, JsonFailure> {
    project_authoritative_stale_targets(StaleTargetProjectionInputs {
        context,
        event_authority_state,
        overlay,
        overlay_current_branch_closure_id: overlay
            .and_then(|overlay| overlay.current_branch_closure_id.as_deref()),
        status,
        preflight: None,
        gate_review: Some(gate_review),
        gate_finish: Some(gate_finish),
    })
}

fn project_gate_warning_codes(
    status: &mut PlanExecutionStatus,
    gate_review: &GateResult,
    gate_finish: &GateResult,
) {
    for warning_code in gate_review
        .warning_codes
        .iter()
        .chain(gate_finish.warning_codes.iter())
    {
        let warning_code = public_status_warning_code(warning_code);
        if !status
            .warning_codes
            .iter()
            .any(|existing| existing == &warning_code)
        {
            status.warning_codes.push(warning_code);
        }
    }
}

fn reset_route_projection_fields_at_status_boundary(status: &mut PlanExecutionStatus) {
    // Status assembly owns reducer-facing facts and diagnostics only. Public
    // route projection is applied later from RouteDecision, so any legacy or
    // constructor defaults for route fields are defensively cleared here.
    status.phase = None;
    status.phase_detail.clear();
    status.recording_context = None;
    status.execution_command_context = None;
    status.execution_reentry_target_source = None;
    status.public_repair_targets.clear();
    status.next_action.clear();
    status.recommended_public_command = None;
    status.recommended_public_command_argv = None;
    status.recommended_public_command_template = None;
    status.required_inputs.clear();
    status.recommended_command = None;
    status.blocking_scope = None;
    status.external_wait_state = None;
    status.blocking_reason_codes.clear();
    status.state_kind.clear();
    status.next_public_action = None;
    status.blockers.clear();
}

fn apply_task_closure_baseline_bridge_from_stale_projection(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    earliest_stale_task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: Option<&BranchRerecordingAssessment>,
) -> Result<(), JsonFailure> {
    if status.blocking_task.is_some()
        || status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_step.is_some()
        || status.current_branch_closure_id.is_some()
        || context.steps.iter().any(|step| !step.checked)
    {
        return Ok(());
    }
    let Some(task) = closure_baseline_candidate_task(context) else {
        return Ok(());
    };
    let fallback_branch_rerecording_assessment;
    let branch_rerecording_assessment = match branch_rerecording_assessment {
        Some(assessment) => assessment,
        None => {
            let Some(authoritative_state) = authoritative_state else {
                return Ok(());
            };
            fallback_branch_rerecording_assessment =
                branch_closure_rerecording_assessment_with_authority(
                    context,
                    Some(authoritative_state),
                )?;
            &fallback_branch_rerecording_assessment
        }
    };
    let candidate = task_closure_baseline_repair_candidate_with_stale_target_and_authority(
        context,
        status,
        task,
        earliest_stale_task.or_else(|| projected_earliest_stale_task_from_status(status)),
        overlay,
        authoritative_state,
        branch_rerecording_assessment,
    )?;
    if candidate.is_none() {
        return Ok(());
    }
    status.harness_phase = HarnessPhase::Executing;
    status.blocking_task = Some(task);
    status.blocking_step = None;
    push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING);
    push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE);
    Ok(())
}

#[cfg(test)]
pub(crate) fn current_workflow_pivot_record_exists_for_status_decision(
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
        qa_requirement.is_none_or(|value| !PLAN_QA_REQUIREMENT_VALUES.contains(&value));
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

pub(super) fn task_closure_baseline_repair_candidate_reason_present(
    status: &PlanExecutionStatus,
) -> bool {
    shared_task_closure_baseline_repair_candidate_reason_present(status)
}

pub(crate) fn status_workspace_state_id(context: &ExecutionContext) -> Result<String, JsonFailure> {
    Ok(semantic_workspace_snapshot(context)?.semantic_workspace_tree_id)
}

pub(crate) fn is_late_stage_phase(phase: HarnessPhase) -> bool {
    phase.is_late_stage()
}

pub(crate) fn final_review_dispatch_still_current_for_gates(
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    shared_final_review_dispatch_still_current(gate_review, gate_finish)
}

pub(crate) fn push_status_reason_code_once(status: &mut PlanExecutionStatus, reason_code: &str) {
    if !status
        .reason_codes
        .iter()
        .any(|existing| existing == reason_code)
    {
        status.reason_codes.push(reason_code.to_owned());
    }
}
