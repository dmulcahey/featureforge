//! Shared read-model derivation helpers for task-boundary, closure-currentness,
//! dispatch-lineage, and execution-started state.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::closure_diagnostics::{
    push_task_closure_pending_verification_reason_codes_for_run,
    task_closure_dispatch_lineage_reason_code, task_closure_recording_diagnostic_reason_codes,
};
use crate::execution::closure_dispatch::current_review_dispatch_id_if_still_current;
use crate::execution::closure_graph::{AuthoritativeClosureGraph, ClosureGraphSignals};
use crate::execution::context::{
    EvidenceAttempt, ExecutionContext, ExecutionEvidence, NO_REPO_FILES_MARKER, NoteState,
    PlanStepState,
};
use crate::execution::current_closure_projection::{
    TaskCurrentClosureStatus, current_task_closure_overlay_restore_required,
    task_current_closure_status, task_current_closure_status_from_authoritative_state,
};
use crate::execution::current_truth::{
    branch_closure_rerecording_assessment, finish_requires_test_plan_refresh,
    is_runtime_owned_execution_control_plane_path,
    late_stage_missing_current_closure_stale_provenance_present as shared_late_stage_missing_current_closure_stale_provenance_present,
    late_stage_missing_task_closure_baseline_bridge_supported, normalized_late_stage_surface,
    path_matches_late_stage_surface,
    stale_reason_codes_for_late_stage_projection as shared_stale_reason_codes_for_late_stage_projection,
};
use crate::execution::final_review::authoritative_strategy_checkpoint_fingerprint_checked;
use crate::execution::harness::HarnessPhase;
use crate::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use crate::execution::leases::load_status_authoritative_overlay_checked;
use crate::execution::read_model::{
    normalize_optional_overlay_value, task_contract_identity_matches_expected,
    task_scope_structural_review_state_reason,
};
use crate::execution::semantic_identity::{
    semantic_paths_changed_between_raw_trees, semantic_workspace_snapshot,
};
use crate::execution::status::{GateResult, PlanExecutionStatus};
use crate::execution::topology::preflight_acceptance_for_context;
use crate::execution::transitions::{
    AuthoritativeTransitionState, load_authoritative_transition_state,
    load_authoritative_transition_state_relaxed,
};
use crate::git::{discover_repository, sha256_hex};

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

// Bootstrap-only helper used while constructing the reducer input status. After RuntimeState
// exists, consumers must use reducer-projected stale targets instead.
pub(crate) fn pre_reducer_earliest_unresolved_stale_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Option<u32> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    let late_stage_missing_current_closure_stale_provenance =
        shared_late_stage_missing_current_closure_stale_provenance_present(context, status)
            .unwrap_or(false);
    let closure_graph = AuthoritativeClosureGraph::from_state(
        authoritative_state.as_ref(),
        &ClosureGraphSignals::from_authoritative_state(
            authoritative_state.as_ref(),
            None,
            status.review_state_status == "stale_unreviewed",
            status.review_state_status == "missing_current_closure"
                && late_stage_missing_current_closure_stale_provenance,
            shared_stale_reason_codes_for_late_stage_projection(
                status,
                std::iter::empty::<&String>(),
            ),
        ),
    );
    closure_graph.earliest_unresolved_stale_task_number()
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
    let Some(prior_task) = prior_task_number_for_begin(context, target_task) else {
        return Ok(());
    };

    if prior_task_cycle_break_active(context, prior_task)? {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "task_cycle_break_active",
            format!(
                "Task {prior_task} is in cycle-break remediation; Task {target_task} may not begin until remediation closes."
            ),
        ));
    }

    if current_task_closure_overlay_restore_required(context)? {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "current_task_closure_overlay_restore_required",
            format!(
                "Task {target_task} may not begin because current task-closure overlays are missing and must be repaired before task-boundary advancement can continue. Run `featureforge plan execution repair-review-state --plan {}` before starting Task {target_task}.",
                context.plan_rel
            ),
        ));
    }

    match task_current_closure_status(context, prior_task)? {
        TaskCurrentClosureStatus::Current => return Ok(()),
        TaskCurrentClosureStatus::Stale => {
            return Err(task_boundary_error(
                FailureClass::ExecutionStateNotReady,
                "prior_task_current_closure_stale",
                format!(
                    "Task {target_task} may not begin because Task {prior_task} current task closure no longer matches the current reviewed workspace state. Run `featureforge plan execution repair-review-state --plan {}` before starting Task {target_task}.",
                    context.plan_rel
                ),
            ));
        }
        TaskCurrentClosureStatus::Missing => {}
    }

    ensure_prior_task_current_closure_record(context, prior_task, target_task)?;
    Ok(())
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
            "prior_task_review_not_green",
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
    let current_semantic_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let authoritative_state = load_authoritative_transition_state(context)?;
    let dispatch_args = RecordReviewDispatchArgs {
        plan: context.plan_abs.clone(),
        scope: ReviewDispatchScopeArg::Task,
        task: Some(task),
    };
    let dispatch_id = current_review_dispatch_id_if_still_current(context, &dispatch_args)
        .ok()
        .flatten();
    let blocking_reason_codes = task_closure_recording_blocking_reason_codes(
        task,
        current_semantic_reviewed_state_id.as_deref(),
        authoritative_state.as_ref(),
    );
    let current_positive_task_closure_present = authoritative_state.as_ref().is_some_and(|state| {
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
            overlay.as_ref(),
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
        && let Some(execution_run_id) = current_execution_run_id(context)?
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

fn task_cycle_break_reason_targets_repaired_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<bool, JsonFailure> {
    if !status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
    {
        return Ok(true);
    }
    let cycle_break_binding = load_status_authoritative_overlay_checked(context)?.map(|overlay| {
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
        return Ok(bound_cycle_break_task == task);
    }
    if matches!(
        cycle_break_binding.as_ref(),
        Some((None, Some(_), _)) | Some((None, _, Some(_)))
    ) {
        return Ok(false);
    }
    Ok(status.blocking_task == Some(task))
}

pub(crate) fn stale_unreviewed_allows_task_closure_baseline_bridge(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<bool, JsonFailure> {
    stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
        context,
        status,
        task,
        projected_earliest_stale_task_from_status(status)
            .or_else(|| pre_reducer_earliest_unresolved_stale_task(context, status)),
    )
}

pub(crate) fn projected_earliest_stale_task_from_status(
    status: &PlanExecutionStatus,
) -> Option<u32> {
    status
        .blocking_records
        .iter()
        .filter(|record| record.scope_type == "task")
        .filter_map(|record| task_number_from_task_scope_key(&record.scope_key))
        .chain(
            status
                .stale_unreviewed_closures
                .iter()
                .filter_map(|record_id| task_number_from_task_scope_key(record_id)),
        )
        .chain(
            status
                .public_repair_targets
                .iter()
                .filter_map(|target| target.task),
        )
        .min()
}

fn task_number_from_task_scope_key(scope_key: &str) -> Option<u32> {
    let raw = scope_key.strip_prefix("task-")?;
    let digits = raw
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
}

pub(crate) fn stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
) -> Result<bool, JsonFailure> {
    let reducer_stale_reentry_targets_task = status.execution_reentry_target_source.as_deref()
        == Some("closure_graph_stale_target")
        && earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task == task);
    if status.review_state_status != "stale_unreviewed" && !reducer_stale_reentry_targets_task {
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
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "late_stage_surface_not_declared"
                | "prior_task_review_not_green"
        )
    }) {
        return Ok(false);
    }
    if !task_closure_recording_runtime_truth_ready(context, task)? {
        return Ok(false);
    }
    let authoritative_state = load_authoritative_transition_state(context)?;
    if reducer_stale_reentry_targets_task
        && !authoritative_task_closure_history_lineage_present(authoritative_state.as_ref(), task)
    {
        return Ok(false);
    }
    if !task_cycle_break_reason_targets_repaired_task(context, status, task)? {
        return Ok(false);
    }
    let task_boundary_stale_truth_blocker = status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_missing"
                | "prior_task_current_closure_stale"
                | "task_closure_baseline_repair_candidate"
                | "task_cycle_break_active"
        )
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
    let Some(authoritative_state) = load_authoritative_transition_state(context)? else {
        return Ok(None);
    };
    if authoritative_state.execution_run_id_opt().is_none() {
        return Ok(None);
    }
    let strategy_checkpoint_present =
        authoritative_strategy_checkpoint_fingerprint_checked(context)?.is_some();
    let current_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    if current_reviewed_state_id
        .as_deref()
        .is_none_or(|reviewed_state_id| reviewed_state_id.trim().is_empty())
    {
        return Ok(None);
    }
    match task_current_closure_status_from_authoritative_state(context, task, &authoritative_state)
    {
        Ok(TaskCurrentClosureStatus::Missing) => {}
        Ok(TaskCurrentClosureStatus::Current | TaskCurrentClosureStatus::Stale) | Err(_) => {
            // Current positive task-closure records are authoritative. Review/verification
            // markdown projections cannot create a task-boundary repair lane once the shared
            // currentness classifier sees a task-closure record for this task.
            return Ok(None);
        }
    }
    let prerequisites = task_closure_recording_prerequisites(context, task)?;
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
    let closure_recording_runtime_truth_ready =
        task_scope_matches_task && task_closure_recording_runtime_truth_ready(context, task)?;
    let stale_bridge_allowed =
        stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
            context,
            status,
            task,
            earliest_unresolved_stale_task,
        )?;
    if !strategy_checkpoint_present {
        return Ok(None);
    }
    if !closure_recording_runtime_truth_ready {
        return Ok(None);
    }
    let dispatch_args = RecordReviewDispatchArgs {
        plan: context.plan_abs.clone(),
        scope: ReviewDispatchScopeArg::Task,
        task: Some(task),
    };
    if let Some(current_dispatch_id) =
        current_review_dispatch_id_if_still_current(context, &dispatch_args)?
    {
        dispatch_id = Some(current_dispatch_id);
    }
    let close_current_task_bridge_blocked =
        prerequisites
            .blocking_reason_codes
            .iter()
            .any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "prior_task_review_not_green"
                        | "prior_task_current_closure_stale"
                            if !stale_bridge_allowed
                )
            });
    if close_current_task_bridge_blocked {
        return Ok(None);
    }
    let late_stage_missing_task_closure_baseline_bridge = status
        .current_branch_closure_id
        .is_none()
        && context.steps.iter().all(|step| step.checked)
        && (status.review_state_status == "missing_current_closure"
            || status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED)
        && late_stage_missing_task_closure_baseline_bridge_supported(
            &branch_closure_rerecording_assessment(context)?,
        );
    if late_stage_missing_task_closure_baseline_bridge
        && !authoritative_task_closure_baseline_truth_present(&authoritative_state, task)
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
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "late_stage_surface_not_declared"
        )
    }) {
        return Ok(None);
    }
    if status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_review_not_green" | "prior_task_current_closure_stale"
        )
    }) && !(status.review_state_status == "stale_unreviewed" && stale_bridge_allowed)
    {
        return Ok(None);
    }
    if status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
        && !task_cycle_break_reason_targets_repaired_task(context, status, task)?
    {
        return Ok(None);
    }
    if status.review_state_status == "stale_unreviewed" && !stale_bridge_allowed {
        let replay_required = status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_review_not_green" | "prior_task_current_closure_stale"
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
        HarnessPhase::DocumentReleasePending if status.current_branch_closure_id.is_none() => {
            late_stage_missing_task_closure_baseline_bridge_supported(
                &branch_closure_rerecording_assessment(context)?,
            )
        }
        _ => false,
    };
    if !closure_repair_phase_supported {
        return Ok(None);
    }
    if status.current_branch_closure_id.is_none()
        && task_closures_are_non_branch_contributing(status)
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
    match earliest_unresolved_stale_task {
        None => true,
        Some(stale_task) if candidate_task > stale_task => false,
        Some(stale_task) if candidate_task == stale_task => true,
        Some(_) => {
            status.blocking_task == Some(candidate_task)
                && status.blocking_step.is_none()
                && status.reason_codes.iter().any(|reason_code| {
                    matches!(
                        reason_code.as_str(),
                        "prior_task_current_closure_missing"
                            | "task_closure_baseline_repair_candidate"
                    )
                })
        }
    }
}

pub(crate) fn task_closure_baseline_bridge_ready_for_stale_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
) -> Result<bool, JsonFailure> {
    if earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task < task) {
        return Ok(false);
    }
    if task_scope_structural_review_state_reason(status).is_some() {
        return Ok(false);
    }
    if status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "late_stage_surface_not_declared"
                | "prior_task_review_not_green"
        )
    }) {
        return Ok(false);
    }
    if task_closure_baseline_repair_candidate_with_stale_target(
        context,
        status,
        task,
        earliest_unresolved_stale_task,
    )?
    .is_none()
    {
        return Ok(false);
    }
    if stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
        context,
        status,
        task,
        earliest_unresolved_stale_task,
    )? {
        return Ok(true);
    }
    let cycle_break_targets_task = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
        && task_cycle_break_reason_targets_repaired_task(context, status, task)?;
    if cycle_break_targets_task {
        return task_closure_recording_runtime_truth_ready(context, task);
    }
    Ok(false)
}

pub(crate) fn task_closure_recording_runtime_truth_ready(
    context: &ExecutionContext,
    task: u32,
) -> Result<bool, JsonFailure> {
    Ok(
        authoritative_strategy_checkpoint_fingerprint_checked(context)?.is_some()
            && task_completion_lineage_fingerprint(context, task).is_some()
            && context
                .current_tracked_tree_sha()
                .ok()
                .is_some_and(|tree_sha| !tree_sha.trim().is_empty()),
    )
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

pub(crate) fn task_closures_are_non_branch_contributing(status: &PlanExecutionStatus) -> bool {
    !status.current_task_closures.is_empty()
        && status.current_task_closures.iter().all(|closure| {
            !closure.effective_reviewed_surface_paths.is_empty()
                && closure
                    .effective_reviewed_surface_paths
                    .iter()
                    .all(|path| path == NO_REPO_FILES_MARKER)
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
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is not bound to the active approved plan revision.",
                closure.task
            ),
        ));
    }
    if closure.review_result != "pass" || closure.verification_result != "pass" {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is not a passing reviewed closure for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure.contract_identity.trim().is_empty() {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
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
            "prior_task_current_closure_invalid",
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
            "prior_task_current_closure_invalid",
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
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is not current for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure.effective_reviewed_surface_paths.is_empty() {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
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
            "prior_task_current_closure_invalid",
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
                    "prior_task_current_closure_stale",
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
                "prior_task_current_closure_reviewed_state_malformed",
                format!("Task {task_number} current task closure {detail}"),
            )
        },
        |detail| {
            task_boundary_error(
                FailureClass::MalformedExecutionState,
                "prior_task_current_closure_reviewed_state_malformed",
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
                    "current_branch_closure_reviewed_state_malformed: Branch closure {branch_closure_id} {detail}"
                ),
            )
        },
        |detail| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "current_branch_closure_reviewed_state_malformed: Branch closure {branch_closure_id} reviewed_state_id could not be resolved: {detail}"
                ),
            )
        },
    )
}

fn ensure_prior_task_current_closure_record(
    context: &ExecutionContext,
    prior_task: u32,
    target_task: u32,
) -> Result<(), JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?.ok_or_else(|| {
        task_boundary_error(
            FailureClass::MalformedExecutionState,
            "prior_task_current_closure_missing",
            format!(
                "Task {target_task} may not begin because Task {prior_task} current task closure state is unavailable."
            ),
        )
    })?;
    let current_record = authoritative_state
        .current_task_closure_result(prior_task)
        .ok_or_else(|| {
            task_boundary_error(
                FailureClass::ExecutionStateNotReady,
                "prior_task_current_closure_missing",
                format!(
                    "Task {target_task} may not begin because Task {prior_task} does not yet have a current task closure. Run `featureforge workflow operator --plan {} --external-review-result-ready`, then follow the recommended `close-current-task` command before starting Task {target_task}.",
                    context.plan_rel
                ),
            )
        })?;
    validate_current_task_closure_record(context, &current_record)?;
    Ok(())
}

fn prior_task_cycle_break_active(
    context: &ExecutionContext,
    prior_task: u32,
) -> Result<bool, JsonFailure> {
    let Some(overlay) = load_status_authoritative_overlay_checked(context)? else {
        return Ok(false);
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
        return Ok(false);
    }
    let Some(cycle_break_task) = overlay.strategy_cycle_break_task else {
        return Ok(false);
    };
    Ok(cycle_break_task == prior_task)
}

fn current_execution_run_id(context: &ExecutionContext) -> Result<Option<String>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    current_execution_run_id_with_authority(context, authoritative_state.as_ref())
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
