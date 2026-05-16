//! Shared execution-status helpers for task-boundary, closure-currentness,
//! dispatch-lineage, and execution-started state.
//!
//! This is the lower helper owner consumed by status assembly, read-model
//! presentation, and runtime truth. It must not depend on read-model or
//! workflow presentation modules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::closure_diagnostics::{
    BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED,
    push_task_closure_pending_verification_reason_codes_for_run,
    task_closure_dispatch_lineage_reason_code, task_closure_recording_diagnostic_reason_codes,
};
use crate::execution::closure_dispatch::current_task_review_dispatch_id_for_task;
use crate::execution::closure_graph::{AuthoritativeClosureGraph, ClosureGraphSignals};
use crate::execution::context::{
    EvidenceAttempt, ExecutionContext, ExecutionEvidence, NO_REPO_FILES_MARKER, NoteState,
    PlanStepState,
};
use crate::execution::current_closure_projection::{
    TaskCurrentClosureStatus, task_current_closure_status_from_authoritative_state,
};
use crate::execution::current_truth::{
    BranchRerecordingAssessment, branch_closure_rerecording_assessment,
    finish_requires_test_plan_refresh, is_runtime_owned_execution_control_plane_path,
    late_stage_missing_current_closure_stale_provenance_present_with_authority as shared_late_stage_missing_current_closure_stale_provenance_present_with_authority,
    late_stage_missing_task_closure_baseline_bridge_supported,
    late_stage_surface_not_declared_reason_code as shared_late_stage_surface_not_declared_reason_code,
    normalized_late_stage_surface, path_matches_late_stage_surface,
    reviewed_surface_paths_contribute_to_branch_surface,
    stale_reason_codes_for_late_stage_projection as shared_stale_reason_codes_for_late_stage_projection,
    task_closure_contributes_to_branch_surface,
};
use crate::execution::final_review::is_canonical_fingerprint;
use crate::execution::harness::HarnessPhase;
use crate::execution::leases::{
    StatusAuthoritativeOverlay, load_status_authoritative_overlay_checked,
};
use crate::execution::semantic_identity::{
    semantic_paths_changed_between_raw_trees, semantic_workspace_snapshot,
    task_definition_identity_for_task,
};
use crate::execution::stale_target_selection::projected_earliest_stale_task_candidate_from_status;
use crate::execution::status::{GateResult, PlanExecutionStatus};
use crate::execution::topology::preflight_acceptance_for_context;
use crate::execution::transitions::{
    AuthoritativeTransitionState, load_authoritative_transition_state,
    load_authoritative_transition_state_relaxed,
};
use crate::git::{discover_repository, sha256_hex};

pub(crate) use crate::execution::public_route_guidance::{
    PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT, WORKFLOW_OPERATOR_EXTERNAL_READY_JSON_DISPLAY_COMMAND,
    WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND, public_typed_operator_route_contract,
};

#[derive(Clone, Copy)]
pub(crate) struct TaskBoundaryAuthorityInputs<'a> {
    overlay: Option<&'a StatusAuthoritativeOverlay>,
    authoritative_state: Option<&'a AuthoritativeTransitionState>,
}

impl<'a> TaskBoundaryAuthorityInputs<'a> {
    pub(crate) fn new(
        overlay: Option<&'a StatusAuthoritativeOverlay>,
        authoritative_state: Option<&'a AuthoritativeTransitionState>,
    ) -> Self {
        Self {
            overlay,
            authoritative_state,
        }
    }

    pub(crate) fn overlay(self) -> Option<&'a StatusAuthoritativeOverlay> {
        self.overlay
    }

    pub(crate) fn authoritative_state(self) -> Option<&'a AuthoritativeTransitionState> {
        self.authoritative_state
    }
}

pub(crate) fn context_all_task_scopes_closed_by_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    let loaded_authoritative_state;
    let authoritative_state = if authoritative_state.is_some() {
        authoritative_state
    } else {
        loaded_authoritative_state = load_authoritative_transition_state_relaxed(context)
            .ok()
            .flatten();
        loaded_authoritative_state.as_ref()
    };
    context_all_task_scopes_closed_from_authority(context, authoritative_state)
}

pub(crate) fn context_all_task_scopes_closed_from_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    if let Some(authoritative_state) = authoritative_state {
        let closed_tasks = authoritative_state
            .current_task_closure_results()
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        if !closed_tasks.is_empty() {
            return context
                .tasks_by_number
                .keys()
                .all(|task| closed_tasks.contains(task));
        }
    }
    context.steps.iter().all(|step| step.checked)
}

pub(crate) fn authoritative_completed_steps_for_context(
    context: &ExecutionContext,
) -> Result<Option<BTreeSet<(u32, u32)>>, JsonFailure> {
    let Some(authoritative_state) = load_authoritative_transition_state(context)? else {
        return Ok(None);
    };
    Ok(Some(authoritative_completed_steps_from_state(
        context,
        &authoritative_state,
    )))
}

fn authoritative_completed_steps_from_state(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> BTreeSet<(u32, u32)> {
    let mut completed_steps = BTreeSet::new();
    for task in authoritative_state.current_task_closure_results().keys() {
        completed_steps.extend(
            context
                .steps
                .iter()
                .filter(|step| step.task_number == *task)
                .map(|step| (step.task_number, step.step_number)),
        );
    }
    if let Some(event_completed_steps) = authoritative_state
        .state_payload_snapshot()
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
    {
        for entry in event_completed_steps.values() {
            if let (Some(task), Some(step)) = (
                entry
                    .get("task")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
                entry
                    .get("step")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
            ) {
                completed_steps.insert((task, step));
            }
        }
    }
    completed_steps
}

pub(crate) fn pre_reducer_earliest_unresolved_stale_task_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<u32> {
    let late_stage_missing_current_closure_stale_provenance =
        shared_late_stage_missing_current_closure_stale_provenance_present_with_authority(
            context,
            status,
            authoritative_state,
        )
        .unwrap_or(false);
    let closure_graph = AuthoritativeClosureGraph::from_state(
        authoritative_state,
        &ClosureGraphSignals::from_authoritative_state(
            authoritative_state,
            None,
            status.review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            status.review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
                && late_stage_missing_current_closure_stale_provenance,
            shared_stale_reason_codes_for_late_stage_projection(
                status,
                std::iter::empty::<&String>(),
            ),
        ),
    );
    closure_graph.earliest_unresolved_stale_task_number()
}

pub(crate) fn normalize_optional_overlay_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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

pub(crate) fn task_scope_review_state_repair_reason(status: &PlanExecutionStatus) -> Option<&str> {
    status
        .reason_codes
        .iter()
        .map(String::as_str)
        .find(|code| {
            matches!(
                *code,
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
                    | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
            )
        })
        .or_else(|| {
            status.reason_codes.iter().map(String::as_str).find(|code| {
                matches!(
                    *code,
                    crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
                )
            })
        })
}

pub(crate) fn task_scope_structural_review_state_reason(
    status: &PlanExecutionStatus,
) -> Option<&str> {
    task_scope_review_state_repair_reason(status).filter(|reason_code| {
        matches!(
            *reason_code,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
        )
    })
}

pub(crate) fn qa_pending_requires_test_plan_refresh(
    context: &ExecutionContext,
    gate_finish: Option<&GateResult>,
) -> bool {
    let _ = context;
    finish_requires_test_plan_refresh(gate_finish)
}

pub(crate) fn prior_task_number_for_begin(
    context: &ExecutionContext,
    target_task: u32,
) -> Option<u32> {
    context
        .tasks_by_number
        .keys()
        .copied()
        .filter(|task_number| *task_number < target_task)
        .max()
}

pub(crate) fn require_prior_task_closure_for_begin(
    context: &ExecutionContext,
    target_task: u32,
) -> Result<(), JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let overlay = load_status_authoritative_overlay_checked(context)?;
    require_prior_task_closure_for_begin_with_authority(
        context,
        target_task,
        authoritative_state.as_ref(),
        overlay.as_ref(),
    )
}

pub(crate) fn require_prior_task_closure_for_begin_with_authority(
    context: &ExecutionContext,
    target_task: u32,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Result<(), JsonFailure> {
    let Some(prior_task) = prior_task_number_for_begin(context, target_task) else {
        return Ok(());
    };

    if prior_task_cycle_break_active_from_overlay(overlay, prior_task) {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE,
            format!(
                "Task {prior_task} is in cycle-break remediation; Task {target_task} may not begin until remediation closes."
            ),
        ));
    }

    let prior_task_closure_status = authoritative_state
        .map_or(Ok(TaskCurrentClosureStatus::Missing), |state| {
            task_current_closure_status_from_authoritative_state(context, prior_task, state)
        })?;
    match prior_task_closure_status {
        TaskCurrentClosureStatus::Current => return Ok(()),
        TaskCurrentClosureStatus::Stale => {
            return Err(task_boundary_error(
                FailureClass::ExecutionStateNotReady,
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
                task_boundary_public_route_remediation(
                    context,
                    format!(
                        "Task {target_task} may not begin because Task {prior_task} current task closure no longer matches the current reviewed workspace state."
                    ),
                ),
            ));
        }
        TaskCurrentClosureStatus::Missing => {}
    }

    if current_task_closure_overlay_restore_required_from_authority(authoritative_state) {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED,
            task_boundary_public_route_remediation(
                context,
                format!(
                    "Task {target_task} may not begin because current task-closure overlays are missing and must be repaired before task-boundary advancement can continue."
                ),
            ),
        ));
    }

    ensure_prior_task_current_closure_record_with_authority(
        context,
        prior_task,
        target_task,
        authoritative_state,
    )?;
    Ok(())
}

fn task_boundary_public_route_remediation(context: &ExecutionContext, message: String) -> String {
    format!(
        "{message} Primary next step: query workflow operator JSON for `{}`; display form `{WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND}`; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}. Diagnostic hint: use `--external-review-result-ready` only after an external review result exists; then re-query display form `{WORKFLOW_OPERATOR_EXTERNAL_READY_JSON_DISPLAY_COMMAND}` before binding the route; verification results alone do not justify that flag; otherwise do not pass `--external-review-result-ready`.",
        context.plan_rel
    )
}

fn current_task_closure_overlay_restore_required_from_authority(
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    authoritative_state.is_some_and(|state| {
        state.current_task_closure_overlay_needs_restore()
            && state.current_task_closure_results().is_empty()
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskClosureBaselineRepairCandidate {
    pub(crate) task: u32,
    pub(crate) dispatch_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskClosureRecordingPrerequisites {
    pub(crate) task: u32,
    pub(crate) dispatch_id: Option<String>,
    pub(crate) blocking_reason_codes: Vec<String>,
    pub(crate) diagnostic_reason_codes: Vec<String>,
}

fn push_task_closure_recording_reason_code_once(reason_codes: &mut Vec<String>, reason_code: &str) {
    if !reason_codes.iter().any(|existing| existing == reason_code) {
        reason_codes.push(reason_code.to_owned());
    }
}

fn task_closure_recording_blocking_reason_codes(
    task: u32,
    current_semantic_reviewed_state_id: Option<&str>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Vec<String> {
    let mut blocking_reason_codes = Vec::new();
    let current_reviewed_state_id = current_semantic_reviewed_state_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if authoritative_state
        .and_then(|state| state.task_closure_negative_result(task))
        .is_some_and(|negative_result| {
            let Some(negative_result_reviewed_state_id) =
                negative_result.semantic_reviewed_state_id.as_deref()
            else {
                return false;
            };
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_result_reviewed_state_id,
                current_reviewed_state_id,
            )
        })
    {
        push_task_closure_recording_reason_code_once(
            &mut blocking_reason_codes,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN,
        );
    }
    blocking_reason_codes
}

pub(crate) fn task_closure_negative_result_blocks_current_reviewed_state(
    negative_result_reviewed_state_id: &str,
    current_reviewed_state_id: Option<&str>,
) -> bool {
    current_reviewed_state_id.is_some_and(|reviewed_state_id| {
        !reviewed_state_id.trim().is_empty()
            && reviewed_state_id == negative_result_reviewed_state_id
    })
}

pub(crate) fn task_closure_recording_prerequisites(
    context: &ExecutionContext,
    task: u32,
) -> Result<TaskClosureRecordingPrerequisites, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let authoritative_state = load_authoritative_transition_state(context)?;
    task_closure_recording_prerequisites_with_authority(
        context,
        task,
        overlay.as_ref(),
        authoritative_state.as_ref(),
    )
}

pub(crate) fn task_closure_recording_prerequisites_with_authority(
    context: &ExecutionContext,
    task: u32,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<TaskClosureRecordingPrerequisites, JsonFailure> {
    let current_semantic_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    let dispatch_id = current_task_review_dispatch_id_for_task(context, Some(task), overlay);
    let blocking_reason_codes = task_closure_recording_blocking_reason_codes(
        task,
        current_semantic_reviewed_state_id.as_deref(),
        authoritative_state,
    );
    let current_positive_task_closure_present = authoritative_state.is_some_and(|state| {
        task_current_closure_status_from_authoritative_state(context, task, state)
            .is_ok_and(|status| status == TaskCurrentClosureStatus::Current)
    });
    let mut diagnostic_reason_codes = if current_positive_task_closure_present {
        Vec::new()
    } else {
        task_closure_recording_diagnostic_reason_codes(
            task,
            dispatch_id.as_deref(),
            current_semantic_reviewed_state_id.as_deref(),
            overlay,
        )
    };
    let dispatch_lineage_diagnostic = diagnostic_reason_codes
        .iter()
        .any(|reason_code| task_closure_dispatch_lineage_reason_code(reason_code));
    if !current_positive_task_closure_present
        && !dispatch_lineage_diagnostic
        && dispatch_id
            .as_deref()
            .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty())
        && let Some(execution_run_id) =
            current_execution_run_id_with_authority(context, authoritative_state)?
    {
        push_task_closure_pending_verification_reason_codes_for_run(
            context,
            task,
            execution_run_id.as_str(),
            false,
            &mut diagnostic_reason_codes,
        )?;
    }
    Ok(TaskClosureRecordingPrerequisites {
        task,
        dispatch_id,
        blocking_reason_codes,
        diagnostic_reason_codes,
    })
}

fn task_cycle_break_reason_targets_repaired_task_with_overlay(
    status: &PlanExecutionStatus,
    task: u32,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> bool {
    if !status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE)
    {
        return true;
    }
    let cycle_break_binding = overlay.map(|overlay| {
        (
            overlay.strategy_cycle_break_task,
            overlay.strategy_cycle_break_step,
            normalize_optional_overlay_value(
                overlay
                    .strategy_cycle_break_checkpoint_fingerprint
                    .as_deref(),
            )
            .map(str::to_owned),
        )
    });
    if let Some((Some(bound_cycle_break_task), _bound_step, _bound_checkpoint_fingerprint)) =
        cycle_break_binding
    {
        return bound_cycle_break_task == task;
    }
    if matches!(
        cycle_break_binding.as_ref(),
        Some((None, Some(_), _)) | Some((None, _, Some(_)))
    ) {
        return false;
    }
    status.blocking_task == Some(task)
}

pub(crate) fn stale_unreviewed_allows_task_closure_baseline_bridge(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<bool, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let authoritative_state = load_authoritative_transition_state(context)?;
    stale_unreviewed_allows_task_closure_baseline_bridge_with_authority(
        context,
        status,
        task,
        overlay.as_ref(),
        authoritative_state.as_ref(),
    )
}

pub(crate) fn stale_unreviewed_allows_task_closure_baseline_bridge_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<bool, JsonFailure> {
    stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target_and_authority(
        context,
        status,
        task,
        projected_earliest_stale_task_from_status(status).or_else(|| {
            pre_reducer_earliest_unresolved_stale_task_with_authority(
                context,
                status,
                authoritative_state,
            )
        }),
        overlay,
        authoritative_state,
    )
}

pub(crate) fn projected_earliest_stale_task_from_status(
    status: &PlanExecutionStatus,
) -> Option<u32> {
    projected_earliest_stale_task_candidate_from_status(status)
}

pub(crate) fn stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target_and_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<bool, JsonFailure> {
    let reducer_stale_reentry_targets_task = status.execution_reentry_target_source.as_deref()
        == Some(crate::execution::stale_target_projection::CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN)
        && earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task == task);
    if status.review_state_status
        != crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && !reducer_stale_reentry_targets_task
    {
        return Ok(false);
    }
    let task_steps = context
        .steps
        .iter()
        .filter(|step| step.task_number == task)
        .collect::<Vec<_>>();
    if task_steps.is_empty() || task_steps.iter().any(|step| !step.checked) {
        return Ok(false);
    }

    if earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task < task) {
        return Ok(false);
    }
    if status.blocking_step.is_some() {
        return Ok(false);
    }
    if task_scope_structural_review_state_reason(status).is_some() {
        return Ok(false);
    }
    if status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_boundary_blocks_closure_baseline_bridge_reason_code(
            reason_code,
        ) || shared_late_stage_surface_not_declared_reason_code(reason_code)
    }) {
        return Ok(false);
    }
    if !task_closure_recording_runtime_truth_ready_with_overlay(context, task, overlay)? {
        return Ok(false);
    }
    if reducer_stale_reentry_targets_task
        && !authoritative_task_closure_history_lineage_present(authoritative_state, task)
    {
        return Ok(false);
    }
    if !task_cycle_break_reason_targets_repaired_task_with_overlay(status, task, overlay) {
        return Ok(false);
    }
    let task_boundary_stale_truth_blocker = status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_boundary_stale_truth_reason_code(reason_code)
    });
    if !task_boundary_stale_truth_blocker && !reducer_stale_reentry_targets_task {
        return Ok(false);
    }
    if status.active_task == Some(task) {
        return Ok(false);
    }
    let current_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    if authoritative_state
        .and_then(|state| state.task_closure_negative_result(task))
        .is_some_and(|negative_result| {
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_result
                    .semantic_reviewed_state_id
                    .as_deref()
                    .unwrap_or(negative_result.reviewed_state_id.as_str()),
                current_reviewed_state_id.as_deref(),
            )
        })
    {
        return Ok(false);
    }
    Ok(true)
}

fn authoritative_task_closure_history_lineage_present(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    task: u32,
) -> bool {
    authoritative_state.is_some_and(|state| state.task_closure_history_lineage_present(task, None))
}

pub(crate) fn task_closure_baseline_repair_candidate_with_stale_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
) -> Result<Option<TaskClosureBaselineRepairCandidate>, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let authoritative_state = load_authoritative_transition_state(context)?;
    let branch_rerecording_assessment = branch_closure_rerecording_assessment(context)?;
    task_closure_baseline_repair_candidate_with_stale_target_and_authority(
        context,
        status,
        task,
        earliest_unresolved_stale_task,
        overlay.as_ref(),
        authoritative_state.as_ref(),
        &branch_rerecording_assessment,
    )
}

pub(crate) fn task_closure_baseline_repair_candidate_with_stale_target_and_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: &BranchRerecordingAssessment,
) -> Result<Option<TaskClosureBaselineRepairCandidate>, JsonFailure> {
    let task_steps = context
        .steps
        .iter()
        .filter(|step| step.task_number == task)
        .collect::<Vec<_>>();
    if task_steps.is_empty() || task_steps.iter().any(|step| !step.checked) {
        return Ok(None);
    }
    if earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task < task) {
        return Ok(None);
    }
    let Some(authoritative_state) = authoritative_state else {
        return Ok(None);
    };
    if authoritative_state.execution_run_id_opt().is_none() {
        return Ok(None);
    }
    let strategy_checkpoint_present =
        authoritative_strategy_checkpoint_fingerprint_from_overlay_checked(overlay)?.is_some();
    let current_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    if current_reviewed_state_id
        .as_deref()
        .is_none_or(|reviewed_state_id| reviewed_state_id.trim().is_empty())
    {
        return Ok(None);
    }
    match task_current_closure_status_from_authoritative_state(context, task, authoritative_state) {
        Ok(TaskCurrentClosureStatus::Missing) => {}
        Ok(TaskCurrentClosureStatus::Current | TaskCurrentClosureStatus::Stale) | Err(_) => {
            // Current positive task-closure records are authoritative. Review/verification
            // markdown projections cannot create a task-boundary repair lane once the shared
            // currentness classifier sees a task-closure record for this task.
            return Ok(None);
        }
    }
    let prerequisites = task_closure_recording_prerequisites_with_authority(
        context,
        task,
        overlay,
        Some(authoritative_state),
    )?;
    let mut dispatch_id = prerequisites.dispatch_id.clone();
    let next_unchecked_task = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number);
    let task_scope_matches_task = (status.blocking_step.is_none()
        && status.blocking_task == Some(task))
        || status.active_task == Some(task)
        || status.resume_task == Some(task)
        || next_unchecked_task.is_some_and(|next_task| task < next_task)
        || (next_unchecked_task.is_none()
            && context.tasks_by_number.keys().copied().max() == Some(task));
    let closure_recording_runtime_truth_ready = task_scope_matches_task
        && task_closure_recording_runtime_truth_ready_with_overlay(context, task, overlay)?;
    let stale_bridge_allowed =
        stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target_and_authority(
            context,
            status,
            task,
            earliest_unresolved_stale_task,
            overlay,
            Some(authoritative_state),
        )?;
    if !strategy_checkpoint_present {
        return Ok(None);
    }
    if !closure_recording_runtime_truth_ready {
        return Ok(None);
    }
    if let Some(current_dispatch_id) =
        current_task_review_dispatch_id_for_task(context, Some(task), overlay)
    {
        dispatch_id = Some(current_dispatch_id);
    }
    let close_current_task_bridge_blocked =
        prerequisites
            .blocking_reason_codes
            .iter()
            .any(|reason_code| {
                !stale_bridge_allowed
                    && crate::execution::closure_diagnostics::task_closure_recording_blocking_reason_code(
                        reason_code,
                    )
            });
    if close_current_task_bridge_blocked {
        return Ok(None);
    }
    let late_stage_missing_task_closure_baseline_bridge = status
        .current_branch_closure_id
        .is_none()
        && context.steps.iter().all(|step| step.checked)
        && (earliest_unresolved_stale_task == Some(task)
            || status.review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
            || status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED)
        && late_stage_missing_task_closure_baseline_bridge_supported(branch_rerecording_assessment);
    if late_stage_missing_task_closure_baseline_bridge
        && !authoritative_task_closure_baseline_truth_present(authoritative_state, task)
    {
        return Ok(None);
    }
    if authoritative_state
        .task_closure_negative_result(task)
        .is_some_and(|negative_result| {
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_result
                    .semantic_reviewed_state_id
                    .as_deref()
                    .unwrap_or(negative_result.reviewed_state_id.as_str()),
                current_reviewed_state_id.as_deref(),
            )
        })
    {
        return Ok(None);
    }
    if status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_boundary_current_closure_structural_reason_code(
            reason_code,
        ) || shared_late_stage_surface_not_declared_reason_code(reason_code)
    }) {
        return Ok(None);
    }
    if status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_closure_recording_blocking_reason_code(
            reason_code,
        )
    }) && !(status.review_state_status
        == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && stale_bridge_allowed)
    {
        return Ok(None);
    }
    if status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_boundary_cycle_break_reason_code(reason_code)
    }) && !task_cycle_break_reason_targets_repaired_task_with_overlay(status, task, overlay)
    {
        return Ok(None);
    }
    if status.review_state_status
        == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && !stale_bridge_allowed
    {
        let replay_required = status.reason_codes.iter().any(|reason_code| {
            crate::execution::closure_diagnostics::task_closure_recording_blocking_reason_code(
                reason_code,
            )
        }) || status.active_task == Some(task)
            || status.resume_task == Some(task)
            || status.blocking_step.is_some();
        if replay_required {
            return Ok(None);
        }
    }
    let closure_repair_phase_supported = match status.harness_phase {
        HarnessPhase::Executing | HarnessPhase::ExecutionPreflight => true,
        HarnessPhase::DocumentReleasePending
        | HarnessPhase::FinalReviewPending
        | HarnessPhase::QaPending
        | HarnessPhase::ReadyForBranchCompletion
            if status.current_branch_closure_id.is_none() =>
        {
            late_stage_missing_task_closure_baseline_bridge
        }
        _ => false,
    };
    if !closure_repair_phase_supported {
        return Ok(None);
    }
    if status.current_branch_closure_id.is_none()
        && current_task_closure_set_is_non_branch_contributing_with_authority(
            context,
            status,
            Some(authoritative_state),
        )
    {
        return Ok(None);
    }
    Ok(Some(TaskClosureBaselineRepairCandidate {
        task,
        dispatch_id,
    }))
}

pub(crate) fn task_closure_baseline_candidate_can_preempt_stale_target(
    status: &PlanExecutionStatus,
    candidate_task: u32,
    earliest_unresolved_stale_task: Option<u32>,
) -> bool {
    let candidate_has_closure_baseline_reason = || {
        status.reason_codes.iter().any(|reason_code| {
            crate::execution::closure_diagnostics::task_boundary_current_closure_missing_reason_code(
                reason_code,
            ) || crate::execution::closure_diagnostics::task_boundary_closure_baseline_repair_candidate_reason_code(
                reason_code,
            )
        })
    };
    match earliest_unresolved_stale_task {
        None => true,
        Some(stale_task) if candidate_task > stale_task => false,
        Some(stale_task) if candidate_task == stale_task => true,
        Some(_) => status.blocking_step.is_none() && candidate_has_closure_baseline_reason(),
    }
}

pub(crate) fn task_closure_baseline_bridge_ready_for_stale_target_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: &BranchRerecordingAssessment,
) -> Result<bool, JsonFailure> {
    if earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task < task) {
        return Ok(false);
    }
    if task_scope_structural_review_state_reason(status).is_some() {
        return Ok(false);
    }
    if status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_boundary_blocks_closure_baseline_bridge_reason_code(
            reason_code,
        ) || shared_late_stage_surface_not_declared_reason_code(reason_code)
    }) {
        return Ok(false);
    }
    if task_closure_baseline_repair_candidate_with_stale_target_and_authority(
        context,
        status,
        task,
        earliest_unresolved_stale_task,
        overlay,
        authoritative_state,
        branch_rerecording_assessment,
    )?
    .is_none()
    {
        return Ok(false);
    }
    if stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target_and_authority(
        context,
        status,
        task,
        earliest_unresolved_stale_task,
        overlay,
        authoritative_state,
    )? {
        return Ok(true);
    }
    let cycle_break_targets_task = status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_boundary_cycle_break_reason_code(reason_code)
    }) && task_cycle_break_reason_targets_repaired_task_with_overlay(
        status, task, overlay,
    );
    if cycle_break_targets_task {
        return task_closure_recording_runtime_truth_ready_with_overlay(context, task, overlay);
    }
    Ok(false)
}

pub(crate) fn task_closure_recording_runtime_truth_ready_with_overlay(
    context: &ExecutionContext,
    task: u32,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Result<bool, JsonFailure> {
    Ok(
        authoritative_strategy_checkpoint_fingerprint_from_overlay_checked(overlay)?.is_some()
            && task_completion_lineage_fingerprint(context, task).is_some()
            && context
                .current_tracked_tree_sha()
                .ok()
                .is_some_and(|tree_sha| !tree_sha.trim().is_empty()),
    )
}

fn authoritative_strategy_checkpoint_fingerprint_from_overlay_checked(
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Result<Option<String>, JsonFailure> {
    let Some(overlay) = overlay else {
        return Ok(None);
    };
    let Some(fingerprint) = overlay
        .last_strategy_checkpoint_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
    else {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Authoritative harness state is missing last_strategy_checkpoint_fingerprint required for final-review provenance binding.",
        ));
    };
    if !is_canonical_fingerprint(&fingerprint) {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Authoritative harness state last_strategy_checkpoint_fingerprint is not a canonical fingerprint.",
        ));
    }
    Ok(Some(fingerprint))
}

fn authoritative_task_closure_baseline_truth_present(
    authoritative_state: &AuthoritativeTransitionState,
    task: u32,
) -> bool {
    authoritative_state
        .raw_current_task_closure_state_entry(task)
        .is_some()
        || authoritative_state
            .current_task_closure_result(task)
            .is_some()
        || authoritative_state.task_closure_history_contains_task(task)
}

fn current_task_closure_set_is_non_branch_contributing_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    if let Some(authoritative_state) = authoritative_state {
        let records =
            crate::execution::current_closure_projection::still_current_task_closure_records_from_authoritative_state(
                context,
                authoritative_state,
            )
            .unwrap_or_default();
        if !records.is_empty() {
            return records
                .iter()
                .all(|record| !task_closure_contributes_to_branch_surface(context, record));
        }
    }
    projected_task_closure_set_is_non_branch_contributing(context, status)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentTaskClosureBranchRouteFacts {
    current_branch_closure_missing: bool,
    current_task_closure_set_present: bool,
    current_task_closure_count: usize,
    current_task_closure_set_non_branch_contributing: bool,
}

impl CurrentTaskClosureBranchRouteFacts {
    #[cfg(test)]
    pub(crate) const fn inactive() -> Self {
        Self {
            current_branch_closure_missing: false,
            current_task_closure_set_present: false,
            current_task_closure_count: 0,
            current_task_closure_set_non_branch_contributing: false,
        }
    }

    pub(crate) fn set_should_route_to_branch_closure(self) -> bool {
        self.current_branch_closure_missing
            && self.current_task_closure_set_present
            && !self.current_task_closure_set_non_branch_contributing
    }

    pub(crate) fn set_is_missing_for_late_stage_reentry(self) -> bool {
        self.current_branch_closure_missing && !self.current_task_closure_set_present
    }

    pub(crate) fn set_has_non_branch_contributing_closure_without_branch(self) -> bool {
        self.current_branch_closure_missing && self.current_task_closure_set_non_branch_contributing
    }

    pub(crate) fn missing_branch_closure(self) -> bool {
        self.current_branch_closure_missing
    }

    pub(crate) fn branch_closure_recorded(self) -> bool {
        !self.current_branch_closure_missing
    }

    pub(crate) fn task_closure_set_present(self) -> bool {
        self.current_task_closure_set_present
    }

    pub(crate) fn task_closure_set_empty(self) -> bool {
        !self.current_task_closure_set_present
    }

    pub(crate) fn missing_current_closure_can_reroute_to_late_stage(self) -> bool {
        self.current_branch_closure_missing && self.current_task_closure_set_present
    }

    pub(crate) fn all_plan_task_closures_present_without_branch_closure(
        self,
        plan_task_count: usize,
    ) -> bool {
        self.current_branch_closure_missing
            && plan_task_count > 0
            && self.current_task_closure_count >= plan_task_count
    }

    pub(crate) fn task_should_route_to_branch_closure(
        self,
        status: &PlanExecutionStatus,
        task_number: u32,
    ) -> bool {
        self.set_should_route_to_branch_closure()
            && self.task_has_current_closure(status, task_number)
    }

    pub(crate) fn branch_missing_and_task_has_no_current_closure(
        self,
        status: &PlanExecutionStatus,
        task_number: u32,
    ) -> bool {
        self.current_branch_closure_missing && self.task_has_no_current_closure(status, task_number)
    }

    pub(crate) fn task_has_no_current_closure(
        self,
        status: &PlanExecutionStatus,
        task_number: u32,
    ) -> bool {
        !self.task_has_current_closure(status, task_number)
    }

    pub(crate) fn set_is_non_branch_contributing(self) -> bool {
        self.current_task_closure_set_non_branch_contributing
    }

    pub(crate) fn task_has_current_closure(
        self,
        status: &PlanExecutionStatus,
        task_number: u32,
    ) -> bool {
        status
            .current_task_closures
            .iter()
            .any(|closure| closure.task == task_number)
    }

    pub(crate) fn stale_target_matches_current_task_closure(
        self,
        status: &PlanExecutionStatus,
        task_number: u32,
        source_record_id: &str,
    ) -> bool {
        self.current_task_closure_set_present
            && status.current_task_closures.iter().any(|closure| {
                closure.task == task_number && closure.closure_record_id == source_record_id
            })
    }
}

pub(crate) fn current_task_closure_branch_route_facts_from_status(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> CurrentTaskClosureBranchRouteFacts {
    CurrentTaskClosureBranchRouteFacts {
        current_branch_closure_missing: status.current_branch_closure_id.is_none(),
        current_task_closure_set_present: !status.current_task_closures.is_empty(),
        current_task_closure_count: status.current_task_closures.len(),
        current_task_closure_set_non_branch_contributing:
            projected_task_closure_set_is_non_branch_contributing(context, status),
    }
}

fn projected_task_closure_set_is_non_branch_contributing(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    !status.current_task_closures.is_empty()
        && status.current_task_closures.iter().all(|closure| {
            !closure.effective_reviewed_surface_paths.is_empty()
                && !reviewed_surface_paths_contribute_to_branch_surface(
                    context,
                    closure
                        .effective_reviewed_surface_paths
                        .iter()
                        .map(String::as_str),
                )
        })
}

pub(crate) fn validate_current_task_closure_record(
    context: &ExecutionContext,
    closure: &crate::execution::transitions::CurrentTaskClosureRecord,
) -> Result<(), JsonFailure> {
    if closure.source_plan_path.as_deref() != Some(context.plan_rel.as_str())
        || closure.source_plan_revision != Some(context.plan_document.plan_revision)
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure is not bound to the active approved plan revision.",
                closure.task
            ),
        ));
    }
    if closure.review_result != "pass" || closure.verification_result != "pass" {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure is not a passing reviewed closure for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure.contract_identity.trim().is_empty() {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure is missing contract identity provenance for the active approved plan.",
                closure.task
            ),
        ));
    }
    if !task_contract_identity_matches_expected(context, closure.task, &closure.contract_identity)?
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure is not bound to the active task contract for the approved plan.",
                closure.task
            ),
        ));
    }
    if closure
        .execution_run_id
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure is missing execution-run provenance for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure
        .closure_status
        .as_deref()
        .is_some_and(|status| status != "current")
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure is not current for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure.effective_reviewed_surface_paths.is_empty() {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure is missing authoritative reviewed-surface provenance for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure
        .effective_reviewed_surface_paths
        .iter()
        .any(|path| path == NO_REPO_FILES_MARKER)
        && closure.effective_reviewed_surface_paths.len() != 1
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
            format!(
                "Task {} current task closure mixes the no-repo-files sentinel with tracked reviewed-surface paths.",
                closure.task
            ),
        ));
    }
    cached_task_closure_reviewed_tree_sha(context, closure)?;
    Ok(())
}

pub(crate) fn task_closure_matches_current_workspace(
    context: &ExecutionContext,
    closure: &crate::execution::transitions::CurrentTaskClosureRecord,
) -> Result<bool, JsonFailure> {
    let surface_paths = closure
        .effective_reviewed_surface_paths
        .iter()
        .filter(|path| {
            path.as_str() != NO_REPO_FILES_MARKER
                && !is_runtime_owned_execution_control_plane_path(context, path)
        })
        .cloned()
        .collect::<Vec<_>>();
    if surface_paths.is_empty() {
        return Ok(true);
    }
    let reviewed_tree_sha = cached_task_closure_reviewed_tree_sha(context, closure)?;
    let current_tree_sha = context.current_tracked_tree_sha()?;
    let changed_paths =
        semantic_paths_changed_between_raw_trees(context, &reviewed_tree_sha, &current_tree_sha)
            .map_err(|error| {
                task_boundary_error(
                    FailureClass::BranchDetectionFailed,
                    crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
                    format!(
                        "Task {} current task closure freshness could not be validated: {}",
                        closure.task, error.message
                    ),
                )
            })?;
    let late_stage_surface =
        normalized_late_stage_surface(&context.plan_source).unwrap_or_default();
    if !late_stage_surface.is_empty()
        && changed_paths
            .iter()
            .all(|path| path_matches_late_stage_surface(path, &late_stage_surface))
    {
        return Ok(true);
    }
    Ok(changed_paths
        .into_iter()
        .all(|path| !path_matches_late_stage_surface(&path, &surface_paths)))
}

fn cached_task_closure_reviewed_tree_sha(
    context: &ExecutionContext,
    closure: &crate::execution::transitions::CurrentTaskClosureRecord,
) -> Result<String, JsonFailure> {
    context.cached_reviewed_tree_sha(
        &closure.reviewed_state_id,
        |repo_root, reviewed_state_id| {
            resolve_task_closure_reviewed_tree_sha(repo_root, closure.task, reviewed_state_id)
        },
    )
}

fn resolve_canonical_reviewed_tree_sha(
    repo_root: &Path,
    reviewed_state_id: &str,
    malformed_error: impl Fn(String) -> JsonFailure,
    unresolved_error: impl Fn(String) -> JsonFailure,
) -> Result<String, JsonFailure> {
    static CANONICAL_REVIEWED_TREE_SHA_CACHE: OnceLock<Mutex<BTreeMap<String, String>>> =
        OnceLock::new();

    let reviewed_state_id = reviewed_state_id.trim();
    let cache_key = format!("{}::{}", repo_root.display(), reviewed_state_id);
    if let Some(cached) = CANONICAL_REVIEWED_TREE_SHA_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("canonical reviewed tree cache lock should not be poisoned")
        .get(&cache_key)
        .cloned()
    {
        return Ok(cached);
    }
    let Some(tree_sha) = reviewed_state_id
        .strip_prefix("git_tree:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(malformed_error(format!(
            "reviewed_state_id must use canonical git_tree identity, got `{reviewed_state_id}`."
        )));
    };
    let object_id = gix::hash::ObjectId::from_hex(tree_sha.as_bytes()).map_err(|error| {
        malformed_error(format!(
            "reviewed_state_id must use a canonical git_tree object id, got `{reviewed_state_id}`: {error}"
        ))
    })?;
    if object_id.to_string() != tree_sha {
        return Err(malformed_error(format!(
            "reviewed_state_id must name the canonical tree object id directly, got `{reviewed_state_id}`."
        )));
    }
    let repo =
        discover_repository(repo_root).map_err(|error| unresolved_error(error.to_string()))?;
    let object = repo
        .find_object(object_id)
        .map_err(|error| unresolved_error(error.to_string()))?;
    if object.kind != gix::object::Kind::Tree {
        return Err(malformed_error(format!(
            "reviewed_state_id must resolve to a tree object directly, got `{}` for `{reviewed_state_id}`.",
            object.kind
        )));
    }
    let resolved_tree_sha = object.id.to_string();
    if !resolved_tree_sha.is_empty() {
        CANONICAL_REVIEWED_TREE_SHA_CACHE
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .expect("canonical reviewed tree cache lock should not be poisoned")
            .insert(cache_key, resolved_tree_sha.clone());
        return Ok(resolved_tree_sha);
    }
    Err(malformed_error(format!(
        "reviewed_state_id must resolve to a git_tree identity, got `{reviewed_state_id}`."
    )))
}

pub(crate) fn resolve_task_closure_reviewed_tree_sha(
    repo_root: &Path,
    task_number: u32,
    reviewed_state_id: &str,
) -> Result<String, JsonFailure> {
    resolve_canonical_reviewed_tree_sha(
        repo_root,
        reviewed_state_id,
        |detail| {
            task_boundary_error(
                FailureClass::MalformedExecutionState,
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED,
                format!("Task {task_number} current task closure {detail}"),
            )
        },
        |detail| {
            task_boundary_error(
                FailureClass::MalformedExecutionState,
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED,
                format!(
                    "Task {task_number} current task closure reviewed_state_id could not be resolved: {detail}"
                ),
            )
        },
    )
}

pub(crate) fn resolve_branch_closure_reviewed_tree_sha(
    repo_root: &Path,
    branch_closure_id: &str,
    reviewed_state_id: &str,
) -> Result<String, JsonFailure> {
    resolve_canonical_reviewed_tree_sha(
        repo_root,
        reviewed_state_id,
        |detail| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "{BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED}: Branch closure {branch_closure_id} {detail}"
                ),
            )
        },
        |detail| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "{BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED}: Branch closure {branch_closure_id} reviewed_state_id could not be resolved: {detail}"
                ),
            )
        },
    )
}

fn ensure_prior_task_current_closure_record_with_authority(
    context: &ExecutionContext,
    prior_task: u32,
    target_task: u32,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<(), JsonFailure> {
    let authoritative_state = authoritative_state.ok_or_else(|| {
        task_boundary_error(
            FailureClass::MalformedExecutionState,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
            task_boundary_public_route_remediation(
                context,
                format!(
                    "Task {target_task} may not begin because Task {prior_task} current task closure state is unavailable."
                ),
            ),
        )
    })?;
    let current_record = authoritative_state
        .current_task_closure_result(prior_task)
        .ok_or_else(|| {
            task_boundary_error(
                FailureClass::ExecutionStateNotReady,
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
                task_boundary_public_route_remediation(
                    context,
                    format!(
                        "Task {target_task} may not begin because Task {prior_task} does not yet have a current task closure."
                    ),
                ),
            )
        })?;
    validate_current_task_closure_record(context, &current_record)?;
    Ok(())
}

fn prior_task_cycle_break_active_from_overlay(
    overlay: Option<&StatusAuthoritativeOverlay>,
    prior_task: u32,
) -> bool {
    let Some(overlay) = overlay else {
        return false;
    };
    let strategy_state = overlay
        .strategy_state
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let strategy_checkpoint_kind = overlay
        .strategy_checkpoint_kind
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if strategy_state != "cycle_breaking" && strategy_checkpoint_kind != "cycle_break" {
        return false;
    }
    let Some(cycle_break_task) = overlay.strategy_cycle_break_task else {
        return false;
    };
    cycle_break_task == prior_task
}

pub(crate) fn current_execution_run_id_with_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Option<String>, JsonFailure> {
    if let Some(execution_run_id) = authoritative_execution_run_id_from_state(authoritative_state) {
        return Ok(Some(execution_run_id));
    }
    fallback_preflight_execution_run_id(context)
}

pub(crate) fn authoritative_execution_run_id_from_state(
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<String> {
    authoritative_state.and_then(AuthoritativeTransitionState::execution_run_id_opt)
}

fn fallback_preflight_execution_run_id(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    Ok(preflight_acceptance_for_context(context)?
        .map(|acceptance| acceptance.execution_run_id.as_str().to_owned()))
}

fn task_boundary_error(
    failure_class: FailureClass,
    reason_code: &str,
    message: impl Into<String>,
) -> JsonFailure {
    JsonFailure::new(failure_class, format!("{reason_code}: {}", message.into()))
}

pub(crate) fn task_boundary_reason_code_from_message(message: &str) -> Option<&str> {
    let (candidate, _) = message.split_once(':')?;
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }
    if candidate
        .as_bytes()
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        Some(candidate)
    } else {
        None
    }
}

pub(crate) fn latest_attempt_for_step(
    evidence: &ExecutionEvidence,
    task_number: u32,
    step_number: u32,
) -> Option<&EvidenceAttempt> {
    evidence
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.task_number == task_number && attempt.step_number == step_number)
}

pub(crate) fn latest_attempted_step_for_task(
    context: &ExecutionContext,
    task_number: u32,
) -> Option<u32> {
    context.evidence.attempts.iter().rev().find_map(|attempt| {
        (attempt.task_number == task_number
            && context.steps.iter().any(|step| {
                step.task_number == task_number && step.step_number == attempt.step_number
            }))
        .then_some(attempt.step_number)
    })
}

pub(crate) fn task_latest_attempts_are_completed(
    context: &ExecutionContext,
    task_number: u32,
) -> bool {
    let Some(task) = context.tasks_by_number.get(&task_number) else {
        return false;
    };
    !task.steps.is_empty()
        && task.steps.iter().all(|step| {
            latest_attempt_for_step(&context.evidence, task_number, step.number)
                .is_some_and(|attempt| attempt.status == "Completed")
        })
}

pub(crate) fn task_completion_lineage_fingerprint(
    context: &ExecutionContext,
    task_number: u32,
) -> Option<String> {
    let task_steps = context
        .steps
        .iter()
        .filter(|step| step.task_number == task_number)
        .collect::<Vec<_>>();
    if task_steps.is_empty() {
        return None;
    }

    let mut payload = format!(
        "plan={}\nplan_revision={}\ntask={task_number}\n",
        context.plan_rel, context.plan_document.plan_revision
    );
    for step in task_steps {
        if !step.checked {
            return None;
        }
        let attempt = latest_attempt_for_step(&context.evidence, task_number, step.step_number)?;
        if attempt.status != "Completed" {
            return None;
        }
        let packet_fingerprint = attempt
            .packet_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let checkpoint_sha = attempt
            .head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let recorded_at = attempt.recorded_at.trim();
        if recorded_at.is_empty() {
            return None;
        }
        payload.push_str(&format!(
            "step={}:attempt={}:recorded_at={recorded_at}:packet={packet_fingerprint}:checkpoint={checkpoint_sha}\n",
            step.step_number, attempt.attempt_number
        ));
    }
    Some(sha256_hex(payload.as_bytes()))
}

pub(crate) fn latest_attempt_indices_by_step(
    evidence: &ExecutionEvidence,
) -> BTreeMap<(u32, u32), usize> {
    let mut indices = BTreeMap::new();
    for (index, attempt) in evidence.attempts.iter().enumerate() {
        indices.insert((attempt.task_number, attempt.step_number), index);
    }
    indices
}

pub(crate) fn latest_completed_attempts_by_step(
    evidence: &ExecutionEvidence,
) -> BTreeMap<(u32, u32), usize> {
    let mut indices = BTreeMap::new();
    for (index, attempt) in evidence.attempts.iter().enumerate() {
        if attempt.status == "Completed" {
            indices.insert((attempt.task_number, attempt.step_number), index);
        }
    }
    indices
}

pub(crate) fn latest_completed_attempts_by_file(
    evidence: &ExecutionEvidence,
    latest_attempts_by_step: &BTreeMap<(u32, u32), usize>,
) -> BTreeMap<String, usize> {
    let mut latest_attempts_by_file = BTreeMap::new();
    for index in latest_attempts_by_step.values().copied() {
        let attempt = &evidence.attempts[index];
        for proof in &attempt.file_proofs {
            if proof.path == NO_REPO_FILES_MARKER {
                continue;
            }
            latest_attempts_by_file.insert(proof.path.clone(), index);
        }
    }
    latest_attempts_by_file
}

pub(crate) fn execution_started(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    authoritative_state.map_or_else(
        || {
            context
                .steps
                .iter()
                .any(|step| step.checked || step.note_state.is_some())
                || !context.evidence.attempts.is_empty()
        },
        AuthoritativeTransitionState::has_authoritative_execution_progress,
    )
}

pub(crate) fn active_step(
    context: &ExecutionContext,
    note_state: NoteState,
) -> Option<&PlanStepState> {
    context
        .steps
        .iter()
        .find(|step| step.note_state == Some(note_state))
}
