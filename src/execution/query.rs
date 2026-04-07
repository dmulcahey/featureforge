// Execution-owned review-state query layer.
// workflow consumes this module as a read-only client rather than reconstructing
// authoritative review-state truth from storage internals.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::JsonFailure;
use crate::execution::leases::load_status_authoritative_overlay_checked;
use crate::execution::mutate::{
    current_branch_closure_baseline_tree_sha, current_repo_tracked_tree_sha,
    normalized_late_stage_surface, path_matches_late_stage_surface, tracked_paths_changed_between,
};
use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
use crate::execution::state::{
    ExecutionContext, ExecutionRuntime, GateResult, PlanExecutionStatus, gate_finish_from_context,
    gate_review_from_context, load_execution_context, status_from_context,
};
use crate::execution::transitions::load_authoritative_transition_state_relaxed;

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
    pub task_review_dispatch_id: Option<String>,
    pub final_review_dispatch_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub current_release_readiness_result: Option<String>,
    pub qa_requirement: Option<String>,
}

pub fn query_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    let context = load_execution_context(runtime, &args.plan)?;
    let status = status_from_context(&context)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    let branch_closure_tracked_drift =
        branch_closure_has_tracked_drift(runtime, overlay.as_ref())?;
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
    let current_branch_closure = overlay.as_ref().and_then(|overlay| {
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
    });
    let superseded_closures = authoritative_state
        .as_ref()
        .map(|state| {
            let mut closures = state.superseded_task_closure_ids();
            closures.extend(state.superseded_branch_closure_ids());
            closures
        })
        .unwrap_or_default();
    let stale_unreviewed_closures =
        if late_stage_stale_unreviewed || branch_closure_tracked_drift {
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
    let missing_derived_overlays = overlay
        .as_ref()
        .map(|overlay| {
            let mut missing = Vec::new();
            if overlay
                .current_branch_closure_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
            {
                if overlay
                    .current_branch_closure_reviewed_state_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
                {
                    missing.push(String::from("current_branch_closure_reviewed_state_id"));
                }
                if overlay
                    .current_branch_closure_contract_identity
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
                {
                    missing.push(String::from("current_branch_closure_contract_identity"));
                }
            }
            missing
        })
        .unwrap_or_default();
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
    let task_review_dispatch_id = overlay
        .as_ref()
        .and_then(|overlay| {
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
    let final_review_dispatch_id = overlay.as_ref().and_then(|overlay| {
        overlay.final_review_dispatch_lineage.as_ref().and_then(|record| {
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
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id: overlay
            .as_ref()
            .and_then(|overlay| overlay.current_branch_closure_id.clone()),
        finish_review_gate_pass_branch_closure_id,
        current_release_readiness_result: overlay
            .as_ref()
            .and_then(|overlay| overlay.current_release_readiness_result.clone()),
        qa_requirement: context.plan_document.qa_requirement.clone(),
    })
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
    late_stage_stale_unreviewed(
        Some(&gate_review),
        Some(&gate_finish),
    )
}

fn execution_state_has_open_steps(status: &PlanExecutionStatus) -> bool {
    status.active_task.is_some() || status.blocking_task.is_some() || status.resume_task.is_some()
}

fn status_has_accepted_preflight(status: &PlanExecutionStatus) -> bool {
    status
        .execution_run_id
        .as_ref()
        .is_some_and(|run_id| !run_id.as_str().trim().is_empty())
        || status.harness_phase == crate::execution::harness::HarnessPhase::ExecutionPreflight
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
    overlay: Option<&crate::execution::leases::StatusAuthoritativeOverlay>,
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
    overlay: Option<&crate::execution::leases::StatusAuthoritativeOverlay>,
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
    let changed_paths = tracked_paths_changed_between(
        &runtime.repo_root,
        baseline_tree_sha,
        &current_tree_sha,
    )?;
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
