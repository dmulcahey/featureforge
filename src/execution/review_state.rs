//! Review-state explain/reconcile adapters over execution-owned query and recording services.
//!
//! reconcile/explain commands stay thin over query and recording boundaries instead of
//! reaching into authoritative storage or rendered artifacts directly.

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::JsonFailure;
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::leases::load_status_authoritative_overlay_checked;
use crate::execution::query::{
    ReviewStateBranchClosure, ReviewStateTaskClosure, query_review_state,
};
use crate::execution::recording::{
    restore_current_branch_closure_overlay as persist_current_branch_closure_overlay,
    restore_current_late_stage_overlays, restore_current_task_closure_overlays,
};
use crate::execution::state::{ExecutionRuntime, load_execution_context, status_from_context};
use crate::execution::transitions::{
    load_authoritative_transition_state, load_authoritative_transition_state_relaxed,
};

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
    if snapshot.missing_derived_overlays.is_empty() && snapshot.stale_unreviewed_closures.is_empty()
    {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("already_current"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed: Vec::new(),
            recommended_command: recommended_operator_command(args),
            trace_summary: String::from(
                "No derived review-state overlays required reconciliation.",
            ),
        });
    }

    let actions_performed = if snapshot.missing_derived_overlays.is_empty() {
        Vec::new()
    } else {
        let context = load_execution_context(runtime, &args.plan)?;
        let mut actions_performed = restore_current_task_closure_overlays(runtime, &context)?;
        actions_performed.extend(restore_current_branch_closure_overlay(runtime, args)?);
        actions_performed.extend(restore_current_late_stage_overlays(runtime, &context)?);
        actions_performed
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
    let unrecoverable_task_scope_task =
        unrecoverable_task_scope_authority_loss_task(runtime, args)?;
    if !snapshot.missing_derived_overlays.is_empty() {
        if !snapshot.stale_unreviewed_closures.is_empty() {
            let (required_follow_up, recommended_command, trace_summary) = if snapshot
                .branch_drift_confined_to_late_stage_surface
            {
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
        if missing_derived_task_scope_overlays(&snapshot.missing_derived_overlays) {
            clear_task_review_dispatch_lineage_for_execution_reentry(
                runtime,
                args,
                unrecoverable_task_scope_task,
                &mut actions_performed,
            )?;
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: Some(String::from("execution_reentry")),
                recommended_command: recommended_operator_command(args),
                trace_summary: String::from(
                    "Repair review state could not derive the missing task-scope overlays from authoritative closure records, so execution reentry is still required before any new closure or milestone can be recorded.",
                ),
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
    if let Some(task_number) = unrecoverable_task_scope_task {
        clear_task_review_dispatch_lineage_for_execution_reentry(
            runtime,
            args,
            Some(task_number),
            &mut actions_performed,
        )?;
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from("execution_reentry")),
            recommended_command: recommended_operator_command(args),
            trace_summary: String::from(
                "Repair review state could not recover authoritative task-scope closure truth after task review dispatch completed, so execution reentry is still required before any new closure or milestone can be recorded.",
            ),
        });
    }
    let repaired_any_overlays = !actions_performed.is_empty();
    if !snapshot.stale_unreviewed_closures.is_empty() {
        let (required_follow_up, recommended_command, trace_summary) = if snapshot
            .branch_drift_confined_to_late_stage_surface
        {
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

fn unrecoverable_task_scope_authority_loss_task(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<Option<u32>, JsonFailure> {
    let context = load_execution_context(runtime, &args.plan)?;
    let status = status_from_context(&context)?;
    let Some(overlay) = load_status_authoritative_overlay_checked(&context)? else {
        return Ok(None);
    };
    let authoritative_sequence = overlay
        .latest_authoritative_sequence
        .or(overlay.authoritative_sequence)
        .unwrap_or(INITIAL_AUTHORITATIVE_SEQUENCE);
    if status.execution_started != "yes"
        || status.active_task.is_some()
        || status.resume_task.is_some()
        || status.current_branch_closure_id.is_some()
        || authoritative_sequence == INITIAL_AUTHORITATIVE_SEQUENCE
        || overlay
            .harness_phase
            .as_deref()
            .map(str::trim)
            .is_some_and(|phase| phase == HarnessPhase::Executing.as_str())
    {
        return Ok(None);
    }
    let Some(authoritative_state) = load_authoritative_transition_state_relaxed(&context)? else {
        return Ok(None);
    };
    let latest_checked_dispatched_task = overlay
        .strategy_review_dispatch_lineage
        .iter()
        .filter_map(|(lineage_key, record)| {
            let task_number = lineage_key
                .strip_prefix("task-")
                .and_then(|task| task.parse::<u32>().ok())
                .or(record.source_task)?;
            let dispatch_id = record.dispatch_id.as_deref().map(str::trim)?;
            if dispatch_id.is_empty() {
                return None;
            }
            context
                .steps
                .iter()
                .filter(|step| step.task_number == task_number)
                .all(|step| step.checked)
                .then_some(task_number)
        })
        .max();
    if let Some(task_number) = latest_checked_dispatched_task
        && authoritative_state
            .current_task_closure_result(task_number)
            .is_none()
        && authoritative_state
            .task_closure_negative_result(task_number)
            .is_none()
    {
        return Ok(Some(task_number));
    }
    Ok(None)
}

fn missing_derived_task_scope_overlays(missing_derived_overlays: &[String]) -> bool {
    missing_derived_overlays.iter().any(|field| {
        matches!(
            field.as_str(),
            "current_task_closure_records" | "task_closure_negative_result_records"
        )
    })
}

fn clear_task_review_dispatch_lineage_for_execution_reentry(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
    task_number: Option<u32>,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let Some(task_number) = task_number else {
        return Ok(());
    };
    let context = load_execution_context(runtime, &args.plan)?;
    let Some(mut authoritative_state) = load_authoritative_transition_state(&context)? else {
        return Ok(());
    };
    if authoritative_state.clear_task_review_dispatch_lineage(task_number)? {
        authoritative_state.persist_if_dirty_with_failpoint(None)?;
        actions_performed.push(format!(
            "cleared_task_review_dispatch_lineage_task_{task_number}"
        ));
    }
    Ok(())
}

fn restore_current_branch_closure_overlay(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<Vec<String>, JsonFailure> {
    let context = load_execution_context(runtime, &args.plan)?;
    let snapshot = query_review_state(runtime, args)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let Some(current_branch_closure) = snapshot.current_branch_closure else {
        return Ok(Vec::new());
    };
    let branch_closure_id = current_branch_closure.branch_closure_id;
    let Some(reviewed_state_id) = current_branch_closure.reviewed_state_id else {
        return Ok(Vec::new());
    };
    let Some(contract_identity) = current_branch_closure.contract_identity else {
        return Ok(Vec::new());
    };
    let mut actions_performed = Vec::new();
    if overlay
        .as_ref()
        .and_then(|overlay| overlay.current_branch_closure_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != Some(branch_closure_id.as_str())
    {
        actions_performed.push(String::from("restored_current_branch_closure_id"));
    }
    if overlay
        .as_ref()
        .and_then(|overlay| overlay.current_branch_closure_reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != Some(reviewed_state_id.as_str())
    {
        actions_performed.push(String::from(
            "restored_current_branch_closure_reviewed_state",
        ));
    }
    if overlay
        .as_ref()
        .and_then(|overlay| overlay.current_branch_closure_contract_identity.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != Some(contract_identity.as_str())
    {
        actions_performed.push(String::from(
            "restored_current_branch_closure_contract_identity",
        ));
    }
    if actions_performed.is_empty() {
        return Ok(actions_performed);
    }

    if !persist_current_branch_closure_overlay(
        runtime,
        &context,
        &branch_closure_id,
        reviewed_state_id.trim(),
        contract_identity.trim(),
    )? {
        return Ok(Vec::new());
    }
    Ok(actions_performed)
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
