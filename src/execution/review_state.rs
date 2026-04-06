use std::path::PathBuf;

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::cli::workflow::OperatorArgs;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::final_review::parse_artifact_document;
use crate::execution::leases::load_status_authoritative_overlay_checked;
use crate::execution::mutate::{
    current_branch_closure_baseline_tree_sha, current_repo_tracked_tree_sha,
    normalized_late_stage_surface, path_matches_late_stage_surface, tracked_paths_changed_between,
};
use crate::execution::state::{ExecutionRuntime, load_execution_context};
use crate::execution::transitions::load_authoritative_transition_state;
use crate::workflow::operator::operator as workflow_operator;

#[derive(Debug, Clone, Serialize)]
pub struct ReviewStateTaskClosure {
    pub task: u32,
    pub closure_record_id: String,
    pub reviewed_state_id: String,
    pub contract_identity: String,
    pub effective_reviewed_surface_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewStateBranchClosure {
    pub branch_closure_id: String,
    pub reviewed_state_id: Option<String>,
    pub contract_identity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainReviewStateOutput {
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    pub superseded_closures: Vec<String>,
    pub stale_unreviewed_closures: Vec<String>,
    pub missing_derived_overlays: Vec<String>,
    pub recommended_command: String,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileReviewStateOutput {
    pub action: String,
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    pub superseded_closures: Vec<String>,
    pub stale_unreviewed_closures: Vec<String>,
    pub missing_derived_overlays: Vec<String>,
    pub actions_performed: Vec<String>,
    pub recommended_command: String,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairReviewStateOutput {
    pub action: String,
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    pub superseded_closures: Vec<String>,
    pub stale_unreviewed_closures: Vec<String>,
    pub missing_derived_overlays: Vec<String>,
    pub actions_performed: Vec<String>,
    pub required_follow_up: Option<String>,
    pub recommended_command: String,
    pub trace_summary: String,
}

pub fn explain_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ExplainReviewStateOutput, JsonFailure> {
    let snapshot = query_review_state(runtime, args)?;
    Ok(ExplainReviewStateOutput {
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        recommended_command: recommended_operator_command(args),
        trace_summary: snapshot.trace_summary,
    })
}

pub fn reconcile_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ReconcileReviewStateOutput, JsonFailure> {
    let snapshot = query_review_state(runtime, args)?;
    if snapshot.missing_derived_overlays.is_empty() && snapshot.stale_unreviewed_closures.is_empty() {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("already_current"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed: Vec::new(),
            recommended_command: recommended_operator_command(args),
            trace_summary: String::from("No derived review-state overlays required reconciliation."),
        });
    }

    let actions_performed = if snapshot.missing_derived_overlays.is_empty() {
        Vec::new()
    } else {
        restore_current_branch_closure_overlay(runtime, args)?
    };
    let restored_any_overlays = !actions_performed.is_empty();
    let refreshed = query_review_state(runtime, args)?;
    if !refreshed.stale_unreviewed_closures.is_empty() {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            recommended_command: recommended_operator_command(args),
            trace_summary: if restored_any_overlays {
                String::from(
                    "Reconcile review state restored derivable overlays, but the reviewed state remains stale_unreviewed and still requires a new execution or recording flow.",
                )
            } else {
                String::from(
                    "Reviewed state is stale_unreviewed; no derivable overlays required reconciliation.",
                )
            },
        });
    }
    if actions_performed.is_empty() && !refreshed.missing_derived_overlays.is_empty() {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            recommended_command: recommended_operator_command(args),
            trace_summary: String::from(
                "Reconcile review state could not derive the missing overlays from authoritative closure records.",
            ),
        });
    }
    Ok(ReconcileReviewStateOutput {
        action: if actions_performed.is_empty() {
            String::from("already_current")
        } else {
            String::from("reconciled")
        },
        current_task_closures: refreshed.current_task_closures,
        current_branch_closure: refreshed.current_branch_closure,
        superseded_closures: refreshed.superseded_closures,
        stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
        missing_derived_overlays: refreshed.missing_derived_overlays,
        actions_performed,
        recommended_command: recommended_operator_command(args),
        trace_summary: String::from(
            "Reconciled missing derived review-state overlays from authoritative closure records.",
        ),
    })
}

pub fn repair_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairReviewStateOutput, JsonFailure> {
    let mut actions_performed = Vec::new();
    let mut snapshot = query_review_state(runtime, args)?;
    if !snapshot.missing_derived_overlays.is_empty() {
        let reconcile = reconcile_review_state(runtime, args)?;
        actions_performed = reconcile.actions_performed;
        snapshot = query_review_state(runtime, args)?;
    }
    if !snapshot.missing_derived_overlays.is_empty() {
        if !snapshot.stale_unreviewed_closures.is_empty() {
            let (required_follow_up, recommended_command, trace_summary) =
                if snapshot.branch_drift_confined_to_late_stage_surface {
                    (
                        Some(String::from("record_branch_closure")),
                        recommended_branch_closure_command(args),
                        String::from(
                            "Repair review state could not restore every derived overlay, but the remaining stale_unreviewed drift is confined to the trusted Late-Stage Surface, so branch closure re-recording is still the next safe step.",
                        ),
                    )
                } else {
                    (
                        Some(String::from("execution_reentry")),
                        recommended_operator_command(args),
                        String::from(
                            "Repair review state could not restore every derived overlay, and the reviewed state remains stale_unreviewed, so execution reentry is still required before any new closure or milestone can be recorded.",
                        ),
                    )
                };
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up,
                recommended_command,
                trace_summary,
            });
        }
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from("record_branch_closure")),
            recommended_command: recommended_branch_closure_command(args),
            trace_summary: String::from(
                "Repair review state could not derive the missing overlays from authoritative closure records, so branch closure must be re-recorded to restore the missing derived state.",
            ),
        });
    }
    let repaired_any_overlays = !actions_performed.is_empty();
    if !snapshot.stale_unreviewed_closures.is_empty() {
        let (required_follow_up, recommended_command, trace_summary) =
            if snapshot.branch_drift_confined_to_late_stage_surface {
                (
                    Some(String::from("record_branch_closure")),
                    recommended_branch_closure_command(args),
                    String::from(
                        "Review state is stale_unreviewed, but the tracked drift is confined to the trusted Late-Stage Surface, so branch closure re-recording is the next safe step.",
                    ),
                )
            } else {
                (
                    Some(String::from("execution_reentry")),
                    recommended_operator_command(args),
                    String::from(
                        "Review state is stale_unreviewed and requires execution reentry before any new closure or milestone can be recorded.",
                    ),
                )
            };
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up,
            recommended_command,
            trace_summary,
        });
    }
    Ok(RepairReviewStateOutput {
        action: if repaired_any_overlays {
            String::from("reconciled")
        } else {
            String::from("already_current")
        },
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        actions_performed,
        required_follow_up: None,
        recommended_command: recommended_operator_command(args),
        trace_summary: if repaired_any_overlays {
            String::from(
                "Repaired missing derived review-state overlays from authoritative closure records.",
            )
        } else {
            snapshot.trace_summary
        },
    })
}

struct ReviewStateSnapshot {
    current_task_closures: Vec<ReviewStateTaskClosure>,
    current_branch_closure: Option<ReviewStateBranchClosure>,
    superseded_closures: Vec<String>,
    stale_unreviewed_closures: Vec<String>,
    missing_derived_overlays: Vec<String>,
    branch_drift_confined_to_late_stage_surface: bool,
    trace_summary: String,
}

fn query_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ReviewStateSnapshot, JsonFailure> {
    let context = load_execution_context(runtime, &args.plan)?;
    let operator = workflow_operator(
        &runtime.repo_root,
        &OperatorArgs {
            plan: args.plan.clone(),
            external_review_result_ready: false,
            json: false,
        },
    )?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let authoritative_state = load_authoritative_transition_state(&context)?;
    let branch_closure_tracked_drift =
        branch_closure_has_tracked_drift(runtime, overlay.as_ref())?;
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
        if operator.review_state_status == "stale_unreviewed" || branch_closure_tracked_drift {
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
        branch_drift_confined_to_late_stage_surface: (operator.review_state_status
            == "stale_unreviewed"
            || branch_closure_tracked_drift)
            && branch_drift_is_confined_to_late_stage_surface(runtime, &context, overlay.as_ref())?,
        current_task_closures,
        current_branch_closure,
        superseded_closures,
        stale_unreviewed_closures,
        missing_derived_overlays,
        trace_summary: if operator.review_state_status == "stale_unreviewed" {
            String::from("Review state is stale_unreviewed relative to the current workspace.")
        } else {
            String::from("Review state is already current for the present workspace.")
        },
    })
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
    context: &crate::execution::state::ExecutionContext,
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

fn restore_current_branch_closure_overlay(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<Vec<String>, JsonFailure> {
    let context = load_execution_context(runtime, &args.plan)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let Some(branch_closure_id) = overlay
        .as_ref()
        .and_then(|overlay| overlay.current_branch_closure_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
    else {
        return Ok(Vec::new());
    };
    let artifact_path = project_artifact_dir(runtime).join(format!("branch-closure-{branch_closure_id}.md"));
    let document = parse_artifact_document(&artifact_path);
    let Some(reviewed_state_id) = document.headers.get("Current Reviewed State ID").cloned() else {
        return Ok(Vec::new());
    };
    let Some(contract_identity) = document.headers.get("Contract Identity").cloned() else {
        return Ok(Vec::new());
    };
    let mut actions_performed = Vec::new();
    if overlay
        .as_ref()
        .and_then(|overlay| overlay.current_branch_closure_reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        actions_performed.push(String::from("restored_current_branch_closure_reviewed_state"));
    }
    if overlay
        .as_ref()
        .and_then(|overlay| overlay.current_branch_closure_contract_identity.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        actions_performed.push(String::from("restored_current_branch_closure_contract_identity"));
    }
    if actions_performed.is_empty() {
        return Ok(actions_performed);
    }

    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "reconcile-review-state requires authoritative harness state.",
        ));
    };
    authoritative_state.set_current_branch_closure_id(
        &branch_closure_id,
        reviewed_state_id.trim(),
        contract_identity.trim(),
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(actions_performed)
}

fn project_artifact_dir(runtime: &ExecutionRuntime) -> PathBuf {
    runtime.state_dir.join("projects").join(&runtime.repo_slug)
}

fn recommended_operator_command(args: &StatusArgs) -> String {
    format!(
        "featureforge workflow operator --plan {}",
        args.plan.display()
    )
}

fn recommended_branch_closure_command(args: &StatusArgs) -> String {
    format!(
        "featureforge plan execution record-branch-closure --plan {}",
        args.plan.display()
    )
}
