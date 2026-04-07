// Execution-owned review-state query layer.
// workflow consumes this module as a read-only client rather than reconstructing
// authoritative review-state truth from storage internals.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::leases::{
    StatusAuthoritativeOverlay, load_status_authoritative_overlay_checked,
};
use crate::execution::mutate::{
    current_branch_closure_baseline_tree_sha, current_repo_tracked_tree_sha,
    normalized_late_stage_surface, path_matches_late_stage_surface, tracked_paths_changed_between,
};
use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
use crate::execution::state::{
    ExecutionContext, ExecutionRuntime, GateResult, PlanExecutionStatus, gate_finish_from_context,
    gate_review_from_context, load_execution_context, missing_derived_review_state_fields,
    resolve_exact_execution_command, status_from_context,
};
use crate::execution::transitions::load_authoritative_transition_state_relaxed;
use crate::workflow::late_stage_precedence::{
    GateState, LateStageSignals, resolve as resolve_late_stage_precedence,
};
use crate::workflow::status::{WorkflowRoute, WorkflowRuntime};

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
    pub task_negative_result_task: Option<u32>,
    pub task_review_dispatch_id: Option<String>,
    pub final_review_dispatch_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub current_release_readiness_result: Option<String>,
    pub current_final_review_branch_closure_id: Option<String>,
    pub current_final_review_result: Option<String>,
    pub current_qa_branch_closure_id: Option<String>,
    pub current_qa_result: Option<String>,
    pub qa_requirement: Option<String>,
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
    pub reason_family: String,
    pub diagnostic_reason_codes: Vec<String>,
    pub task_review_dispatch_id: Option<String>,
    pub final_review_dispatch_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub current_release_readiness_result: Option<String>,
}

pub fn query_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    let context = load_execution_context(runtime, &args.plan)?;
    let status = status_from_context(&context)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    let branch_closure_tracked_drift = branch_closure_has_tracked_drift(runtime, overlay.as_ref())?;
    let late_stage_stale_unreviewed = review_state_is_stale_unreviewed(&context, &status);
    let current_task_closures = authoritative_state
        .as_ref()
        .map(|state| {
            state
                .current_task_closure_results()
                .into_values()
                .map(|record| ReviewStateTaskClosure {
                    task: record.task,
                    closure_record_id: record.closure_record_id,
                    reviewed_state_id: record.reviewed_state_id,
                    contract_identity: record.contract_identity,
                    effective_reviewed_surface_paths: record.effective_reviewed_surface_paths,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let current_branch_closure = authoritative_state
        .as_ref()
        .and_then(|state| state.recoverable_current_branch_closure_identity())
        .map(|identity| ReviewStateBranchClosure {
            branch_closure_id: identity.branch_closure_id,
            reviewed_state_id: Some(identity.reviewed_state_id),
            contract_identity: Some(identity.contract_identity),
        })
        .or_else(|| {
            overlay.as_ref().and_then(|overlay| {
                overlay
                    .current_branch_closure_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|branch_closure_id| ReviewStateBranchClosure {
                        branch_closure_id: branch_closure_id.to_owned(),
                        reviewed_state_id: overlay
                            .current_branch_closure_reviewed_state_id
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                        contract_identity: overlay
                            .current_branch_closure_contract_identity
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                    })
            })
        });
    let superseded_closures = authoritative_state
        .as_ref()
        .map(|state| {
            let mut closures = state.superseded_task_closure_ids();
            closures.extend(state.superseded_branch_closure_ids());
            closures
        })
        .unwrap_or_default();
    let stale_unreviewed_closures = if late_stage_stale_unreviewed || branch_closure_tracked_drift {
        if let Some(current_branch_closure) = current_branch_closure.as_ref() {
            vec![current_branch_closure.branch_closure_id.clone()]
        } else {
            current_task_closures
                .iter()
                .map(|record| record.closure_record_id.clone())
                .collect()
        }
    } else {
        Vec::new()
    };
    let missing_derived_overlays =
        missing_derived_review_state_fields(authoritative_state.as_ref(), overlay.as_ref());
    Ok(ReviewStateSnapshot {
        branch_drift_confined_to_late_stage_surface: (late_stage_stale_unreviewed
            || branch_closure_tracked_drift)
            && branch_drift_is_confined_to_late_stage_surface(runtime, &context, overlay.as_ref())?,
        current_task_closures,
        current_branch_closure,
        superseded_closures,
        stale_unreviewed_closures,
        missing_derived_overlays,
        trace_summary: if late_stage_stale_unreviewed || branch_closure_tracked_drift {
            String::from("Review state is stale_unreviewed relative to the current workspace.")
        } else {
            String::from("Review state is already current for the present workspace.")
        },
    })
}

pub fn query_workflow_execution_state(
    runtime: &ExecutionRuntime,
    plan_path: &str,
) -> Result<WorkflowExecutionState, JsonFailure> {
    if plan_path.is_empty() {
        return Ok(WorkflowExecutionState::default());
    }
    let context = load_execution_context(runtime, &PathBuf::from(plan_path))?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    let status_args = StatusArgs {
        plan: PathBuf::from(plan_path),
    };
    let mut execution_status = runtime.status(&status_args)?;
    if let Some(shared_status) =
        started_status_from_same_branch_worktree(&runtime.repo_root, plan_path, &execution_status)
    {
        execution_status = shared_status;
    }
    let mut preflight = None;
    let mut gate_review = None;
    let mut gate_finish = None;
    if execution_status.execution_started == "yes" {
        if !execution_state_has_open_steps(&execution_status) {
            let review = gate_review_from_context(&context);
            gate_finish = Some(gate_finish_from_context(&context));
            gate_review = Some(review);
        }
    } else if !status_has_accepted_preflight(&execution_status) {
        preflight = Some(runtime.preflight_read_only(&status_args)?);
    }
    let task_review_dispatch_id = overlay.as_ref().and_then(|overlay| {
        execution_status
            .blocking_task
            .and_then(|task_number| {
                overlay
                    .strategy_review_dispatch_lineage
                    .get(&format!("task-{task_number}"))
                    .and_then(|record| record.dispatch_id.clone())
            })
            .or_else(|| {
                overlay
                    .strategy_review_dispatch_lineage
                    .iter()
                    .filter_map(|(key, record)| {
                        let task_number = key.strip_prefix("task-")?.parse::<u32>().ok()?;
                        let dispatch_id = record.dispatch_id.clone()?;
                        Some((task_number, dispatch_id))
                    })
                    .max_by_key(|(task_number, _)| *task_number)
                    .map(|(_, dispatch_id)| dispatch_id)
            })
    });
    let task_negative_result_task = current_task_negative_result_task(
        &execution_status,
        overlay.as_ref(),
        authoritative_state.as_ref(),
    );
    let final_review_dispatch_id = overlay.as_ref().and_then(|overlay| {
        overlay
            .final_review_dispatch_lineage
            .as_ref()
            .and_then(|record| {
                let execution_run_id = record.execution_run_id.as_deref()?;
                if execution_run_id.trim().is_empty() {
                    return None;
                }
                let branch_closure_id = record.branch_closure_id.as_deref()?;
                if overlay.current_branch_closure_id.as_deref()? != branch_closure_id {
                    return None;
                }
                record.dispatch_id.clone()
            })
    });
    let finish_review_gate_pass_branch_closure_id = authoritative_state
        .as_ref()
        .and_then(|state| state.finish_review_gate_pass_branch_closure_id());
    Ok(WorkflowExecutionState {
        execution_status: Some(execution_status),
        preflight,
        gate_review,
        gate_finish,
        task_negative_result_task,
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id: overlay
            .as_ref()
            .and_then(|overlay| overlay.current_branch_closure_id.clone()),
        finish_review_gate_pass_branch_closure_id,
        current_release_readiness_result: overlay
            .as_ref()
            .and_then(|overlay| overlay.current_release_readiness_result.clone()),
        current_final_review_branch_closure_id: authoritative_state
            .as_ref()
            .and_then(|state| state.current_final_review_branch_closure_id())
            .map(str::to_owned),
        current_final_review_result: authoritative_state
            .as_ref()
            .and_then(|state| state.current_final_review_result())
            .map(str::to_owned),
        current_qa_branch_closure_id: authoritative_state
            .as_ref()
            .and_then(|state| state.current_qa_branch_closure_id())
            .map(str::to_owned),
        current_qa_result: authoritative_state
            .as_ref()
            .and_then(|state| state.current_qa_result())
            .map(str::to_owned),
        qa_requirement: context.plan_document.qa_requirement.clone(),
    })
}

pub fn query_workflow_routing_state(
    current_dir: &std::path::Path,
    plan_override: Option<&std::path::Path>,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    let workflow = WorkflowRuntime::discover_read_only(current_dir).map_err(JsonFailure::from)?;
    let mut route = workflow.resolve().map_err(JsonFailure::from)?;
    if let Some(plan_override) = plan_override {
        route.plan_path = plan_override.to_string_lossy().into_owned();
    }

    let mut execution_status = None;
    let mut preflight = None;
    let mut gate_review = None;
    let mut gate_finish = None;
    let mut task_negative_result_task = None;
    let mut task_review_dispatch_id = None;
    let mut final_review_dispatch_id = None;
    let mut current_branch_closure_id = None;
    let mut finish_review_gate_pass_branch_closure_id = None;
    let mut current_release_readiness_result = None;
    let mut current_final_review_branch_closure_id = None;
    let mut current_final_review_result = None;
    let mut current_qa_branch_closure_id = None;
    let mut current_qa_result = None;
    let mut qa_requirement = None;

    if route.status == "implementation_ready" && !route.plan_path.is_empty() {
        let runtime = ExecutionRuntime::discover(current_dir)?;
        let workflow_state = query_workflow_execution_state(&runtime, &route.plan_path)?;
        task_negative_result_task = workflow_state.task_negative_result_task;
        task_review_dispatch_id = workflow_state.task_review_dispatch_id;
        final_review_dispatch_id = workflow_state.final_review_dispatch_id;
        current_branch_closure_id = workflow_state.current_branch_closure_id;
        finish_review_gate_pass_branch_closure_id =
            workflow_state.finish_review_gate_pass_branch_closure_id;
        current_release_readiness_result = workflow_state.current_release_readiness_result;
        current_final_review_branch_closure_id =
            workflow_state.current_final_review_branch_closure_id;
        current_final_review_result = workflow_state.current_final_review_result;
        current_qa_branch_closure_id = workflow_state.current_qa_branch_closure_id;
        current_qa_result = workflow_state.current_qa_result;
        qa_requirement = workflow_state.qa_requirement;
        execution_status = workflow_state.execution_status;
        preflight = workflow_state.preflight;
        gate_review = workflow_state.gate_review;
        gate_finish = workflow_state.gate_finish;
    }

    let workflow_phase = derive_phase(
        &route.status,
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
    let follow_up_override =
        resolve_follow_up_override(workflow_phase.as_str(), execution_status.as_ref());
    let (
        phase,
        phase_detail,
        review_state_status,
        recording_context,
        execution_command_context,
        next_action,
        recommended_command,
    ) = if negative_result_requires_execution_reentry(
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
                let status = execution_status.as_ref().ok_or_else(|| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "workflow/operator could not derive the execution reentry command required after a negative review outcome.",
                    )
                })?;
                if let Some(resolved) = resolve_exact_execution_command(status, &plan_path) {
                    (
                        String::from("executing"),
                        String::from("execution_reentry_required"),
                        String::from("clean"),
                        None,
                        Some(ExecutionRoutingExecutionCommandContext {
                            command_kind: String::from(resolved.command_kind),
                            task_number: Some(resolved.task_number),
                            step_id: resolved.step_id,
                        }),
                        String::from("execution reentry required"),
                        Some(resolved.recommended_command),
                    )
                } else {
                    (
                        String::from("executing"),
                        String::from("execution_reentry_required"),
                        String::from("clean"),
                        None,
                        None,
                        String::from("execution reentry required"),
                        None,
                    )
                }
            }
        }
    } else if matches!(
        workflow_phase.as_str(),
        "document_release_pending"
            | "final_review_pending"
            | "qa_pending"
            | "ready_for_branch_completion"
    ) && (late_stage_stale_unreviewed(gate_review.as_ref(), gate_finish.as_ref())
        || execution_status
            .as_ref()
            .is_some_and(|status| status.review_state_status == "stale_unreviewed"))
    {
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
    } else if let Some(status) = execution_status.as_ref() {
        if task_review_dispatch_stale(status).is_some() {
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
        } else if let Some(task_number) = task_review_dispatch_task(status) {
            (
                String::from("task_closure_pending"),
                String::from("task_review_dispatch_required"),
                String::from("clean"),
                None,
                None,
                String::from("dispatch review"),
                Some(format!(
                    "featureforge plan execution record-review-dispatch --plan {plan_path} --scope task --task {task_number}"
                )),
            )
        } else if let Some(task_number) =
            task_review_result_pending_task(status, task_review_dispatch_id.as_deref())
        {
            let recommended_command = if external_review_result_ready {
                task_review_dispatch_id.as_ref().map(|dispatch_id| {
                    format!(
                        "featureforge plan execution close-current-task --plan {plan_path} --task {task_number} --dispatch-id {dispatch_id} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
                    )
                })
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
                    task_review_dispatch_id.as_ref().map(|dispatch_id| {
                        ExecutionRoutingRecordingContext {
                            task_number: Some(task_number),
                            dispatch_id: Some(dispatch_id.clone()),
                            branch_closure_id: None,
                        }
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
        } else if workflow_phase == "final_review_pending" && current_branch_closure_id.is_none() {
            (
                String::from("document_release_pending"),
                String::from("branch_closure_recording_required_for_release_readiness"),
                String::from("missing_current_closure"),
                None,
                None,
                String::from("record branch closure"),
                Some(format!(
                    "featureforge plan execution record-branch-closure --plan {plan_path}"
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
        } else if workflow_phase == "final_review_pending" && final_review_dispatch_id.is_none() {
            (
                String::from("final_review_pending"),
                String::from("final_review_dispatch_required"),
                String::from("clean"),
                None,
                None,
                String::from("dispatch final review"),
                Some(format!(
                    "featureforge plan execution record-review-dispatch --plan {plan_path} --scope final-review"
                )),
            )
        } else if workflow_phase == "final_review_pending" {
            let recommended_command = if external_review_result_ready {
                final_review_dispatch_id.as_ref().map(|dispatch_id| {
                    format!(
                        "featureforge plan execution advance-late-stage --plan {plan_path} --dispatch-id {dispatch_id} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>"
                    )
                })
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
                    final_review_dispatch_id.as_ref().map(|dispatch_id| {
                        ExecutionRoutingRecordingContext {
                            task_number: None,
                            dispatch_id: Some(dispatch_id.clone()),
                            branch_closure_id: current_branch_closure_id.clone(),
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
            if let Some(branch_closure_id) = current_branch_closure_id.as_ref() {
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
                    String::from("record branch closure"),
                    Some(format!(
                        "featureforge plan execution record-branch-closure --plan {plan_path}"
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
                String::from("record branch closure"),
                Some(format!(
                    "featureforge plan execution record-branch-closure --plan {plan_path}"
                )),
            )
        } else if workflow_phase == "qa_pending" {
            match qa_requirement.as_deref() {
                Some("required") if finish_requires_test_plan_refresh(gate_finish.as_ref()) => (
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
                        "featureforge plan execution record-qa --plan {plan_path} --result pass|fail --summary-file <path>"
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
                            String::from("run finish completion gate"),
                            Some(format!(
                                "featureforge plan execution gate-finish --plan {plan_path}"
                            )),
                        )
                    } else {
                        (
                            String::from("ready_for_branch_completion"),
                            String::from("finish_review_gate_ready"),
                            String::from("clean"),
                            None,
                            None,
                            String::from("run finish review gate"),
                            Some(format!(
                                "featureforge plan execution gate-review --plan {plan_path}"
                            )),
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
                    if let Some(resolved) = resolve_exact_execution_command(status, &plan_path) {
                        (
                            String::from("executing"),
                            String::from("execution_in_progress"),
                            String::from("clean"),
                            None,
                            Some(ExecutionRoutingExecutionCommandContext {
                                command_kind: String::from(resolved.command_kind),
                                task_number: Some(resolved.task_number),
                                step_id: resolved.step_id,
                            }),
                            String::from("continue execution"),
                            Some(resolved.recommended_command),
                        )
                    } else {
                        (
                            String::from("executing"),
                            String::from("execution_in_progress"),
                            String::from("clean"),
                            None,
                            None,
                            String::from("continue execution"),
                            None,
                        )
                    }
                }
                "repairing" => (
                    String::from("executing"),
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
            }
        }
    } else {
        let phase = workflow_phase.clone();
        let phase_detail = match workflow_phase.as_str() {
            "handoff_required" => "handoff_recording_required",
            "pivot_required" => "planning_reentry_required",
            _ => "planning_reentry_required",
        };
        let recommended_command = match workflow_phase.as_str() {
            "handoff_required" => Some(format!(
                "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
            )),
            "pivot_required" => Some(format!(
                "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
            )),
            _ => None,
        };
        (
            phase,
            String::from(phase_detail),
            String::from("clean"),
            None,
            None,
            next_action_for_context_like(
                workflow_phase.as_str(),
                gate_review.as_ref(),
                gate_finish.as_ref(),
            )
            .replace('_', " "),
            recommended_command,
        )
    };

    Ok(ExecutionRoutingState {
        route,
        execution_status,
        preflight,
        gate_review,
        gate_finish,
        workflow_phase,
        phase,
        phase_detail,
        review_state_status,
        qa_requirement,
        follow_up_override,
        finish_review_gate_pass_branch_closure_id,
        recording_context,
        execution_command_context,
        next_action,
        recommended_command,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id,
        current_release_readiness_result,
    })
}

fn current_task_negative_result_task(
    execution_status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&crate::execution::transitions::AuthoritativeTransitionState>,
) -> Option<u32> {
    if execution_status.blocking_step.is_some() {
        return None;
    }
    let task = execution_status.blocking_task?;
    let negative_result = authoritative_state?.task_closure_negative_result(task)?;
    let lineage = overlay?
        .strategy_review_dispatch_lineage
        .get(&format!("task-{task}"))?;
    let lineage_dispatch_id = lineage.dispatch_id.as_deref()?.trim();
    let lineage_reviewed_state_id = lineage.reviewed_state_id.as_deref()?.trim();
    if lineage_dispatch_id.is_empty() || lineage_reviewed_state_id.is_empty() {
        return None;
    }
    (negative_result.dispatch_id == lineage_dispatch_id
        && negative_result.reviewed_state_id == lineage_reviewed_state_id)
        .then_some(task)
}

fn negative_result_requires_execution_reentry(
    task_negative_result_present: bool,
    workflow_phase: &str,
    current_branch_closure_id: Option<&str>,
    current_final_review_branch_closure_id: Option<&str>,
    current_final_review_result: Option<&str>,
    current_qa_branch_closure_id: Option<&str>,
    current_qa_result: Option<&str>,
) -> bool {
    if matches!(workflow_phase, "handoff_required" | "pivot_required") {
        return false;
    }

    if task_negative_result_present {
        return true;
    }

    let final_review_failed = current_final_review_result == Some("fail")
        && current_final_review_branch_closure_id
            .zip(current_branch_closure_id)
            .is_some_and(|(recorded, current)| recorded == current);
    let qa_failed = current_qa_result == Some("fail")
        && current_qa_branch_closure_id
            .zip(current_branch_closure_id)
            .is_some_and(|(recorded, current)| recorded == current);

    final_review_failed || qa_failed
}

pub(crate) fn resolve_follow_up_override(
    workflow_phase: &str,
    execution_status: Option<&PlanExecutionStatus>,
) -> String {
    let raw_pivot_required = workflow_phase == "pivot_required"
        || execution_status.is_some_and(|status| {
            status.harness_phase == HarnessPhase::PivotRequired
                || status.reason_codes.iter().any(|code| {
                    matches!(
                        code.as_str(),
                        "blocked_on_plan_revision" | "qa_requirement_missing_or_invalid"
                    )
                })
        });
    let raw_handoff_required = workflow_phase == "handoff_required"
        || execution_status.is_some_and(|status| {
            status.harness_phase == HarnessPhase::HandoffRequired || status.handoff_required
        });

    if raw_pivot_required {
        String::from("record_pivot")
    } else if raw_handoff_required {
        String::from("record_handoff")
    } else {
        String::from("none")
    }
}

fn review_state_is_stale_unreviewed(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    if status.execution_started != "yes" || execution_state_has_open_steps(status) {
        return false;
    }

    let gate_review = gate_review_from_context(context);
    let gate_finish = gate_finish_from_context(context);
    late_stage_stale_unreviewed(Some(&gate_review), Some(&gate_finish))
}

fn execution_state_has_open_steps(status: &PlanExecutionStatus) -> bool {
    status.active_task.is_some() || status.blocking_task.is_some() || status.resume_task.is_some()
}

fn status_has_accepted_preflight(status: &PlanExecutionStatus) -> bool {
    status
        .execution_run_id
        .as_ref()
        .is_some_and(|run_id| !run_id.as_str().trim().is_empty())
        || status.harness_phase == HarnessPhase::ExecutionPreflight
}

fn late_stage_stale_unreviewed(
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    gate_has_any_reason(
        gate_finish,
        &[
            "review_artifact_worktree_dirty",
            REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
            "release_docs_state_stale",
            "release_docs_state_not_fresh",
            "final_review_state_stale",
            "final_review_state_not_fresh",
            "browser_qa_state_stale",
            "browser_qa_state_not_fresh",
        ],
    ) || gate_has_any_reason(
        gate_review,
        &["release_docs_state_stale", "release_docs_state_not_fresh"],
    )
}

fn gate_has_any_reason(gate: Option<&GateResult>, reason_codes: &[&str]) -> bool {
    gate.is_some_and(|gate| {
        gate.reason_codes
            .iter()
            .any(|code| reason_codes.iter().any(|expected| code == expected))
    })
}

fn started_status_from_same_branch_worktree(
    current_repo_root: &std::path::Path,
    plan_path: &str,
    local_status: &PlanExecutionStatus,
) -> Option<PlanExecutionStatus> {
    if local_status.execution_started == "yes" {
        return None;
    }
    let relative_plan = PathBuf::from(plan_path);
    same_branch_worktrees(current_repo_root)
        .into_iter()
        .filter(|root| root != current_repo_root)
        .find_map(|worktree_root| {
            let runtime = ExecutionRuntime::discover(&worktree_root).ok()?;
            let status = runtime
                .status(&StatusArgs {
                    plan: relative_plan.clone(),
                })
                .ok()?;
            if status.execution_started == "yes" {
                Some(status)
            } else {
                None
            }
        })
}

fn same_branch_worktrees(current_repo_root: &std::path::Path) -> Vec<PathBuf> {
    let output = match Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(current_repo_root)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let mut entries: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut worktree_root: Option<PathBuf> = None;
    let mut branch_ref: Option<String> = None;

    let flush_entry = |entries: &mut Vec<(PathBuf, Option<String>)>,
                       worktree_root: &mut Option<PathBuf>,
                       branch_ref: &mut Option<String>| {
        if let Some(root) = worktree_root.take() {
            entries.push((root, branch_ref.take()));
        }
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.is_empty() {
            flush_entry(&mut entries, &mut worktree_root, &mut branch_ref);
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            flush_entry(&mut entries, &mut worktree_root, &mut branch_ref);
            worktree_root = Some(PathBuf::from(path));
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch ") {
            branch_ref = Some(branch.to_owned());
        }
    }
    flush_entry(&mut entries, &mut worktree_root, &mut branch_ref);

    let current_root =
        fs::canonicalize(current_repo_root).unwrap_or_else(|_| current_repo_root.to_path_buf());
    let mut current_branch_ref = entries.iter().find_map(|(root, branch)| {
        let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if canonical_root == current_root {
            branch.clone()
        } else {
            None
        }
    });

    if current_branch_ref.is_none() {
        let branch_output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(current_repo_root)
            .output();
        if let Ok(output) = branch_output
            && output.status.success()
        {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !branch.is_empty() && branch != "HEAD" {
                current_branch_ref = Some(format!("refs/heads/{branch}"));
            }
        }
    }

    let Some(current_branch_ref) = current_branch_ref else {
        return Vec::new();
    };

    entries
        .into_iter()
        .filter_map(|(root, branch)| {
            if branch.as_deref() == Some(current_branch_ref.as_str()) {
                Some(root)
            } else {
                None
            }
        })
        .collect()
}

fn branch_closure_has_tracked_drift(
    runtime: &ExecutionRuntime,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Result<bool, JsonFailure> {
    let Some(overlay) = overlay else {
        return Ok(false);
    };
    let Some(baseline_tree_sha) = current_branch_closure_baseline_tree_sha(overlay) else {
        return Ok(false);
    };
    Ok(current_repo_tracked_tree_sha(&runtime.repo_root)? != baseline_tree_sha)
}

fn branch_drift_is_confined_to_late_stage_surface(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Result<bool, JsonFailure> {
    let Some(overlay) = overlay else {
        return Ok(false);
    };
    let Some(baseline_tree_sha) = current_branch_closure_baseline_tree_sha(overlay) else {
        return Ok(false);
    };
    let current_tree_sha = current_repo_tracked_tree_sha(&runtime.repo_root)?;
    if current_tree_sha == baseline_tree_sha {
        return Ok(false);
    }
    let changed_paths =
        tracked_paths_changed_between(&runtime.repo_root, baseline_tree_sha, &current_tree_sha)?;
    if changed_paths.is_empty() {
        return Ok(false);
    }
    let late_stage_surface = normalized_late_stage_surface(&context.plan_source)?;
    if late_stage_surface.is_empty() {
        return Ok(false);
    }
    Ok(changed_paths
        .iter()
        .all(|path| path_matches_late_stage_surface(path, &late_stage_surface)))
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

    let late_stage_progress = routing_status_has_late_stage_progress(execution_status);

    if execution_status.execution_started != "yes" {
        if status_has_accepted_preflight(execution_status)
            || preflight.map(|result| result.allowed).unwrap_or(false)
        {
            return String::from("execution_preflight");
        }
        return String::from("implementation_handoff");
    }

    if execution_status.review_state_status == "missing_current_closure" {
        return String::from("document_release_pending");
    }

    if !late_stage_progress && task_boundary_block_reason_code(execution_status).is_some() {
        return String::from("task_closure_pending");
    }

    if !late_stage_progress && execution_state_has_open_steps(execution_status) {
        return String::from("executing");
    }

    let Some(gate_finish) = gate_finish else {
        return String::from("final_review_pending");
    };

    if gate_finish
        .reason_codes
        .iter()
        .any(|code| code == "qa_requirement_missing_or_invalid")
    {
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

    let release_blocked =
        late_stage_release_blocked(gate_finish) || late_stage_release_truth_blocked(gate_review);
    let review_blocked =
        late_stage_review_truth_blocked(gate_review) || late_stage_review_blocked(gate_finish);
    let qa_blocked = late_stage_qa_blocked(gate_finish);

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

    let release_blocked =
        late_stage_release_blocked(gate_finish) || late_stage_release_truth_blocked(gate_review);
    let review_blocked =
        late_stage_review_truth_blocked(gate_review) || late_stage_review_blocked(gate_finish);
    let qa_blocked = late_stage_qa_blocked(gate_finish);
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

fn authoritative_public_phase(status: &PlanExecutionStatus) -> Option<&'static str> {
    if status.latest_authoritative_sequence == INITIAL_AUTHORITATIVE_SEQUENCE {
        return None;
    }

    if matches!(
        status.harness_phase,
        HarnessPhase::Executing | HarnessPhase::Repairing
    ) && routing_status_has_late_stage_progress(status)
    {
        return None;
    }

    match status.harness_phase {
        HarnessPhase::Repairing => Some(HarnessPhase::Executing.as_str()),
        HarnessPhase::FinalReviewPending
        | HarnessPhase::QaPending
        | HarnessPhase::DocumentReleasePending
        | HarnessPhase::ReadyForBranchCompletion => None,
        _ => Some(status.harness_phase.as_str()),
    }
}

fn routing_status_has_late_stage_progress(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_some()
        || status.current_release_readiness_state.is_some()
        || status.finish_review_gate_pass_branch_closure_id.is_some()
        || status.current_final_review_branch_closure_id.is_some()
        || status.current_final_review_result.is_some()
        || status.current_qa_branch_closure_id.is_some()
        || status.current_qa_result.is_some()
        || matches!(
            status.current_final_review_state.as_str(),
            "fresh" | "stale"
        )
        || matches!(status.current_qa_state.as_str(), "fresh" | "stale")
}

fn task_boundary_block_reason_code(status: &PlanExecutionStatus) -> Option<&str> {
    if status.blocking_task.is_none() || status.blocking_step.is_some() {
        return None;
    }
    status.reason_codes.iter().map(String::as_str).find(|code| {
        matches!(
            *code,
            "prior_task_review_not_green"
                | "task_review_not_independent"
                | "task_review_receipt_malformed"
                | "prior_task_verification_missing"
                | "prior_task_verification_missing_legacy"
                | "task_verification_receipt_malformed"
                | "prior_task_review_dispatch_missing"
                | "prior_task_review_dispatch_stale"
                | "task_cycle_break_active"
        )
    })
}

fn task_review_dispatch_task(status: &PlanExecutionStatus) -> Option<u32> {
    let blocking_task = status.blocking_task?;
    let reason_code = task_boundary_block_reason_code(status)?;
    if reason_code == "prior_task_review_dispatch_missing" {
        Some(blocking_task)
    } else {
        None
    }
}

fn task_review_dispatch_stale(status: &PlanExecutionStatus) -> Option<u32> {
    let blocking_task = status.blocking_task?;
    let reason_code = task_boundary_block_reason_code(status)?;
    (reason_code == "prior_task_review_dispatch_stale").then_some(blocking_task)
}

fn task_review_result_pending_task(
    status: &PlanExecutionStatus,
    dispatch_id: Option<&str>,
) -> Option<u32> {
    if status.blocking_step.is_some() {
        return None;
    }
    let blocking_task = status.blocking_task?;
    let dispatch_id = dispatch_id?.trim();
    if dispatch_id.is_empty() {
        return None;
    }
    if status
        .reason_codes
        .iter()
        .any(|code| code == "prior_task_review_not_green")
    {
        Some(blocking_task)
    } else {
        None
    }
}

fn finish_requires_test_plan_refresh(gate_finish: Option<&GateResult>) -> bool {
    gate_has_any_reason(
        gate_finish,
        &[
            "test_plan_artifact_missing",
            "test_plan_artifact_malformed",
            "test_plan_artifact_stale",
            "test_plan_artifact_authoritative_provenance_invalid",
            "test_plan_artifact_generator_mismatch",
        ],
    )
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

fn late_stage_release_blocked(gate_finish: &GateResult) -> bool {
    gate_finish.failure_class == "ReleaseArtifactNotFresh"
        || gate_has_any_reason(
            Some(gate_finish),
            &[
                "release_artifact_authoritative_provenance_invalid",
                "release_docs_state_missing",
                "release_docs_state_stale",
                "release_docs_state_not_fresh",
            ],
        )
}

fn late_stage_release_truth_blocked(gate_review: Option<&GateResult>) -> bool {
    gate_has_any_reason(
        gate_review,
        &[
            "release_docs_state_missing",
            "release_docs_state_stale",
            "release_docs_state_not_fresh",
        ],
    )
}

fn late_stage_review_truth_blocked(gate_review: Option<&GateResult>) -> bool {
    gate_has_any_reason(
        gate_review,
        &[
            "review_artifact_authoritative_provenance_invalid",
            "final_review_state_missing",
            "final_review_state_stale",
            "final_review_state_not_fresh",
        ],
    )
}

fn late_stage_review_blocked(gate_finish: &GateResult) -> bool {
    gate_finish.failure_class == "ReviewArtifactNotFresh"
        || gate_has_any_reason(
            Some(gate_finish),
            &[
                "review_artifact_authoritative_provenance_invalid",
                "final_review_state_missing",
                "final_review_state_stale",
                "final_review_state_not_fresh",
            ],
        )
}

fn late_stage_qa_blocked(gate_finish: &GateResult) -> bool {
    gate_finish.failure_class == "QaArtifactNotFresh"
        || gate_has_any_reason(
            Some(gate_finish),
            &[
                "qa_artifact_authoritative_provenance_invalid",
                "test_plan_artifact_authoritative_provenance_invalid",
                "browser_qa_state_missing",
                "browser_qa_state_stale",
                "browser_qa_state_not_fresh",
            ],
        )
}
