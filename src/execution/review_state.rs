//! Review-state explain/reconcile adapters over execution-owned query and recording services.
//!
//! reconcile/explain commands stay thin over query and recording boundaries instead of
//! reaching into authoritative storage or rendered artifacts directly.

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::JsonFailure;
use crate::execution::current_truth::{
    branch_closure_rerecording_supported, missing_derived_task_scope_overlays,
};
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::leases::load_status_authoritative_overlay_checked;
use crate::execution::query::{
    ExecutionRoutingState, ReviewStateBranchClosure, ReviewStateSnapshot, ReviewStateTaskClosure,
    query_review_state, query_workflow_routing_state_for_runtime, required_follow_up_from_routing,
};
use crate::execution::recording::{
    clear_current_branch_closure_for_structural_repair,
    clear_current_task_closure_results_for_execution_reentry,
    clear_current_task_closure_results_for_structural_repair,
    clear_current_task_closure_results_for_structural_repair_scope_keys,
    clear_task_review_dispatch_lineage_for_execution_reentry as clear_task_dispatch_lineage,
    clear_task_review_dispatch_lineage_for_structural_repair as clear_task_dispatch_lineage_for_structural_repair_recording,
    persist_review_state_repair_follow_up,
    restore_current_branch_closure_overlay as persist_current_branch_closure_overlay,
    restore_current_late_stage_overlays, restore_current_task_closure_overlays,
};
use crate::execution::state::{
    ExecutionRuntime, current_branch_closure_structural_review_state_reason,
    execution_reentry_current_task_closure_targets, execution_reentry_current_task_closure_tasks,
    load_execution_context_for_exact_plan, load_execution_read_scope,
    task_scope_structural_review_state_reason,
};
use crate::execution::transitions::load_authoritative_transition_state_relaxed;

#[derive(Debug, Clone, Serialize)]
pub struct ExplainReviewStateOutput {
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    pub superseded_closures: Vec<String>,
    pub stale_unreviewed_closures: Vec<String>,
    pub missing_derived_overlays: Vec<String>,
    pub next_action: String,
    pub recommended_command: Option<String>,
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
    let (next_action, recommended_command) =
        match query_workflow_routing_state_for_runtime(runtime, Some(&args.plan), false) {
            Ok(routing) => (routing.next_action, routing.recommended_command),
            Err(_) => (
                String::from("requery workflow operator"),
                Some(recommended_operator_command(args)),
            ),
        };
    Ok(ExplainReviewStateOutput {
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        next_action,
        recommended_command,
        trace_summary: snapshot.trace_summary,
    })
}

pub fn reconcile_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ReconcileReviewStateOutput, JsonFailure> {
    let snapshot = query_review_state(runtime, args)?;
    let read_scope = load_execution_read_scope(runtime, &args.plan, true)?;
    let context = read_scope.context;
    let status = read_scope.status;
    let branch_rerecording_supported = branch_closure_rerecording_supported(&context)?;
    if let Some(reason_code) = task_scope_structural_review_state_reason(&status) {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed: Vec::new(),
            recommended_command: format!(
                "featureforge plan execution repair-review-state --plan {}",
                args.plan.display()
            ),
            trace_summary: match reason_code {
                "prior_task_current_closure_invalid" => String::from(
                    "Reconcile review state cannot repair structurally invalid current task-closure provenance; execution reentry is still required.",
                ),
                "prior_task_current_closure_reviewed_state_malformed" => String::from(
                    "Reconcile review state cannot repair a malformed current task-closure reviewed-state identity; execution reentry is still required.",
                ),
                _ => String::from(
                    "Reconcile review state cannot repair the current task-closure review-state blocker; execution reentry is still required.",
                ),
            },
        });
    }
    if let Some(reason_code) = current_branch_closure_structural_review_state_reason(&status) {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed: Vec::new(),
            recommended_command: format!(
                "featureforge plan execution repair-review-state --plan {}",
                args.plan.display()
            ),
            trace_summary: if branch_rerecording_supported {
                match reason_code {
                    "current_branch_closure_reviewed_state_malformed" => String::from(
                        "Reconcile review state cannot repair a malformed current branch-closure reviewed-state identity; run repair-review-state to establish the late-stage reroute before branch closure can be re-recorded.",
                    ),
                    _ => String::from(
                        "Reconcile review state cannot repair the current branch-closure review-state blocker; run repair-review-state to establish the late-stage reroute before branch closure can be re-recorded.",
                    ),
                }
            } else {
                match reason_code {
                    "current_branch_closure_reviewed_state_malformed" => String::from(
                        "Reconcile review state cannot repair a malformed current branch-closure reviewed-state identity, and no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required.",
                    ),
                    _ => String::from(
                        "Reconcile review state cannot repair the current branch-closure review-state blocker, and no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required.",
                    ),
                }
            },
        });
    }
    if snapshot.missing_derived_overlays.is_empty() && snapshot.stale_unreviewed_closures.is_empty()
    {
        let routing =
            query_workflow_routing_state_for_runtime(runtime, Some(&args.plan), false).ok();
        if routing
            .as_ref()
            .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
        {
            let recommend_branch_closure = routing.as_ref().is_some_and(|routing| {
                routing.phase_detail == "branch_closure_recording_required_for_release_readiness"
            });
            return Ok(ReconcileReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed: Vec::new(),
                recommended_command: if recommend_branch_closure && branch_rerecording_supported {
                    recommended_branch_closure_command(args)
                } else {
                    format!(
                        "featureforge plan execution repair-review-state --plan {}",
                        args.plan.display()
                    )
                },
                trace_summary: if branch_rerecording_supported {
                    String::from(
                        "Reconcile review state cannot mint a missing current branch closure; branch closure must be recorded before late-stage progression can continue.",
                    )
                } else {
                    String::from(
                        "Reconcile review state cannot mint a missing current branch closure because no still-current task-closure baseline remains to derive it, so execution reentry is still required.",
                    )
                },
            });
        }
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
        let refreshed_routing =
            query_workflow_routing_state_for_runtime(runtime, Some(&args.plan), false).ok();
        let late_stage_repair_command = format!(
            "featureforge plan execution repair-review-state --plan {}",
            args.plan.display()
        );
        let refreshed_branch_rerecording_supported =
            branch_closure_rerecording_supported(&context)?;
        let recommended_command = if refreshed_routing
            .as_ref()
            .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
        {
            if refreshed_routing.as_ref().is_some_and(|routing| {
                routing.phase_detail == "branch_closure_recording_required_for_release_readiness"
            }) && refreshed_branch_rerecording_supported
            {
                recommended_branch_closure_command(args)
            } else {
                late_stage_repair_command.clone()
            }
        } else if refreshed_routing
            .as_ref()
            .is_some_and(routing_projects_review_state_execution_reentry)
        {
            late_stage_repair_command.clone()
        } else {
            recommended_operator_command(args)
        };
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            recommended_command,
            trace_summary: String::from(
                "Reconcile review state could not derive the missing overlays from authoritative closure records.",
            ),
        });
    }
    let refreshed_routing =
        query_workflow_routing_state_for_runtime(runtime, Some(&args.plan), false).ok();
    if refreshed_routing
        .as_ref()
        .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
    {
        let refreshed_branch_rerecording_supported =
            branch_closure_rerecording_supported(&context)?;
        let recommend_branch_closure = refreshed_routing.as_ref().is_some_and(|routing| {
            routing.phase_detail == "branch_closure_recording_required_for_release_readiness"
        });
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            recommended_command: if recommend_branch_closure
                && refreshed_branch_rerecording_supported
            {
                recommended_branch_closure_command(args)
            } else {
                format!(
                    "featureforge plan execution repair-review-state --plan {}",
                    args.plan.display()
                )
            },
            trace_summary: if refreshed_branch_rerecording_supported {
                if restored_any_overlays {
                    String::from(
                        "Reconcile review state restored derivable overlays, but branch closure must still be recorded before late-stage progression can continue.",
                    )
                } else {
                    String::from(
                        "Reconcile review state cannot mint a missing current branch closure; branch closure must be recorded before late-stage progression can continue.",
                    )
                }
            } else if restored_any_overlays {
                String::from(
                    "Reconcile review state restored derivable overlays, but no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required.",
                )
            } else {
                String::from(
                    "Reconcile review state cannot mint a missing current branch closure because no still-current task-closure baseline remains to derive it, so execution reentry is still required.",
                )
            },
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
    let read_scope = load_execution_read_scope(runtime, &args.plan, true)?;
    let context = read_scope.context;
    let mut status = read_scope.status;
    let mut snapshot = query_review_state(runtime, args)?;
    if !snapshot.missing_derived_overlays.is_empty() {
        let reconcile = reconcile_review_state(runtime, args)?;
        actions_performed = reconcile.actions_performed;
        status = load_execution_read_scope(runtime, &args.plan, true)?.status;
        snapshot = query_review_state(runtime, args)?;
    }
    let unrecoverable_task_scope_task =
        unrecoverable_task_scope_authority_loss_task(runtime, args)?;
    let execution_reentry_tasks = execution_reentry_current_task_closure_tasks(&context)?;
    let execution_reentry_targets = execution_reentry_current_task_closure_targets(&context)?;
    if let Some(reason_code) = task_scope_structural_review_state_reason(&status) {
        clear_task_scope_state_for_structural_repair(
            runtime,
            args,
            &execution_reentry_targets,
            status.blocking_task,
            &mut actions_performed,
        )?;
        persist_review_state_repair_follow_up(runtime, &context, None)?;
        let refreshed = query_review_state(runtime, args)?;
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from("execution_reentry")),
            recommended_command: recommended_operator_command(args),
            trace_summary: match reason_code {
                "prior_task_current_closure_invalid" => String::from(
                    "Repair review state detected structurally invalid current task-closure provenance, so execution reentry is still required before any new closure or milestone can be recorded.",
                ),
                "prior_task_current_closure_reviewed_state_malformed" => String::from(
                    "Repair review state detected a malformed current task-closure reviewed-state identity, so execution reentry is still required before any new closure or milestone can be recorded.",
                ),
                _ => String::from(
                    "Repair review state detected a structural current task-closure review-state blocker, so execution reentry is still required before any new closure or milestone can be recorded.",
                ),
            },
        });
    }
    let branch_rerecording_supported = branch_closure_rerecording_supported(&context)?;
    if let Some(reason_code) = current_branch_closure_structural_review_state_reason(&status) {
        if !branch_rerecording_supported {
            clear_branch_scope_state_for_execution_reentry(runtime, args, &mut actions_performed)?;
            persist_review_state_repair_follow_up(runtime, &context, Some("execution_reentry"))?;
            let refreshed = query_review_state(runtime, args)?;
            return Ok(blocked_execution_reentry_output(
                refreshed,
                actions_performed,
                args,
                match reason_code {
                    "current_branch_closure_reviewed_state_malformed" => {
                        "Repair review state detected a malformed current branch-closure reviewed-state identity, but no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required."
                    }
                    _ => {
                        "Repair review state detected a structural current branch-closure review-state blocker, but no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required."
                    }
                },
            ));
        }
        persist_review_state_repair_follow_up(runtime, &context, Some("record_branch_closure"))?;
        let refreshed = query_review_state(runtime, args)?;
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from("record_branch_closure")),
            recommended_command: recommended_branch_closure_command(args),
            trace_summary: match reason_code {
                "current_branch_closure_reviewed_state_malformed" => String::from(
                    "Repair review state detected a malformed current branch-closure reviewed-state identity, so branch closure must be re-recorded before late-stage progression can continue.",
                ),
                _ => String::from(
                    "Repair review state detected a structural current branch-closure review-state blocker, so branch closure must be re-recorded before late-stage progression can continue.",
                ),
            },
        });
    }
    if !snapshot.missing_derived_overlays.is_empty() {
        if missing_derived_task_scope_overlays(&snapshot.missing_derived_overlays) {
            clear_task_review_dispatch_lineage_for_execution_reentry(
                runtime,
                args,
                unrecoverable_task_scope_task,
                &mut actions_performed,
            )?;
            persist_review_state_repair_follow_up(runtime, &context, None)?;
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
        if !snapshot.stale_unreviewed_closures.is_empty() {
            let (required_follow_up, recommended_command, trace_summary) = if snapshot
                .branch_drift_confined_to_late_stage_surface
                && branch_rerecording_supported
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
            persist_review_state_repair_follow_up(
                runtime,
                &context,
                required_follow_up.as_deref(),
            )?;
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
        if !branch_rerecording_supported {
            clear_branch_scope_state_for_execution_reentry(runtime, args, &mut actions_performed)?;
            persist_review_state_repair_follow_up(runtime, &context, Some("execution_reentry"))?;
            let refreshed = query_review_state(runtime, args)?;
            return Ok(blocked_execution_reentry_output(
                refreshed,
                actions_performed,
                args,
                "Repair review state could not derive the missing overlays from authoritative closure records, and no still-current task-closure baseline remains to rebuild the current branch closure, so execution reentry is still required.",
            ));
        }
        persist_review_state_repair_follow_up(runtime, &context, Some("record_branch_closure"))?;
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
        clear_task_scope_state_for_execution_reentry(
            runtime,
            args,
            &execution_reentry_tasks,
            Some(task_number),
            &mut actions_performed,
        )?;
        persist_review_state_repair_follow_up(runtime, &context, None)?;
        let refreshed = query_review_state(runtime, args)?;
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
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
            && branch_rerecording_supported
        {
            (
                Some(String::from("record_branch_closure")),
                recommended_branch_closure_command(args),
                String::from(
                    "Review state is stale_unreviewed, but the tracked drift is confined to the trusted Late-Stage Surface, so branch closure re-recording is the next safe step.",
                ),
            )
        } else {
            clear_task_scope_state_for_execution_reentry(
                runtime,
                args,
                &execution_reentry_tasks,
                status.blocking_task,
                &mut actions_performed,
            )?;
            (
                Some(String::from("execution_reentry")),
                recommended_operator_command(args),
                String::from(
                    "Review state is stale_unreviewed and requires execution reentry before any new closure or milestone can be recorded.",
                ),
            )
        };
        persist_review_state_repair_follow_up(runtime, &context, required_follow_up.as_deref())?;
        let refreshed = if required_follow_up.as_deref() == Some("execution_reentry") {
            query_review_state(runtime, args)?
        } else {
            snapshot.clone()
        };
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            required_follow_up,
            recommended_command,
            trace_summary,
        });
    }
    let routing = query_workflow_routing_state_for_runtime(runtime, Some(&args.plan), false).ok();
    if routing
        .as_ref()
        .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
    {
        if !branch_rerecording_supported {
            clear_branch_scope_state_for_execution_reentry(runtime, args, &mut actions_performed)?;
            persist_review_state_repair_follow_up(runtime, &context, Some("execution_reentry"))?;
            let refreshed = query_review_state(runtime, args)?;
            return Ok(blocked_execution_reentry_output(
                refreshed,
                actions_performed,
                args,
                "Repair review state cannot mint a missing current branch closure because no still-current task-closure baseline remains to derive it, so execution reentry is still required.",
            ));
        }
        persist_review_state_repair_follow_up(runtime, &context, Some("record_branch_closure"))?;
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
                "Repair review state cannot mint a missing current branch closure; branch closure must be recorded before late-stage progression can continue.",
            ),
        });
    }
    persist_review_state_repair_follow_up(runtime, &context, None)?;
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
    let read_scope = load_execution_read_scope(runtime, &args.plan, true)?;
    let context = read_scope.context;
    let status = read_scope.status;
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

fn clear_task_review_dispatch_lineage_for_execution_reentry(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
    task_number: Option<u32>,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let Some(task_number) = task_number else {
        return Ok(());
    };
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    if clear_task_dispatch_lineage(runtime, &context, task_number)? {
        actions_performed.push(format!(
            "cleared_task_review_dispatch_lineage_task_{task_number}"
        ));
    }
    Ok(())
}

fn clear_task_scope_state_for_execution_reentry(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
    execution_reentry_tasks: &[u32],
    blocking_task: Option<u32>,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let mut tasks = execution_reentry_tasks.to_vec();
    if let Some(task_number) = blocking_task
        && !tasks.contains(&task_number)
    {
        tasks.push(task_number);
    }
    tasks.sort_unstable();
    tasks.dedup();

    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let cleared_tasks =
        clear_current_task_closure_results_for_execution_reentry(runtime, &context, tasks.clone())?;
    for task_number in cleared_tasks {
        actions_performed.push(format!("cleared_current_task_closure_task_{task_number}"));
    }
    for task_number in tasks {
        if clear_task_dispatch_lineage(runtime, &context, task_number)? {
            actions_performed.push(format!(
                "cleared_task_review_dispatch_lineage_task_{task_number}"
            ));
        }
    }
    Ok(())
}

fn clear_task_scope_state_for_structural_repair(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
    execution_reentry_targets: &crate::execution::state::ExecutionReentryCurrentTaskClosureTargets,
    blocking_task: Option<u32>,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let mut structural_tasks = execution_reentry_targets.structural_tasks.clone();
    if let Some(task_number) = blocking_task
        && !structural_tasks.contains(&task_number)
        && !execution_reentry_targets.stale_tasks.contains(&task_number)
    {
        structural_tasks.push(task_number);
    }
    structural_tasks.sort_unstable();
    structural_tasks.dedup();
    let stale_tasks = execution_reentry_targets
        .stale_tasks
        .iter()
        .copied()
        .filter(|task_number| !structural_tasks.contains(task_number))
        .collect::<Vec<_>>();

    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let cleared_scope_keys = clear_current_task_closure_results_for_structural_repair_scope_keys(
        runtime,
        &context,
        execution_reentry_targets.structural_scope_keys.clone(),
    )?;
    for scope_key in cleared_scope_keys {
        actions_performed.push(format!("cleared_current_task_closure_scope_{scope_key}"));
    }
    let cleared_structural_tasks = clear_current_task_closure_results_for_structural_repair(
        runtime,
        &context,
        structural_tasks.clone(),
    )?;
    for task_number in cleared_structural_tasks {
        actions_performed.push(format!("cleared_current_task_closure_task_{task_number}"));
    }
    for task_number in structural_tasks {
        if clear_task_dispatch_lineage_for_structural_repair_recording(
            runtime,
            &context,
            task_number,
        )? {
            actions_performed.push(format!(
                "cleared_task_review_dispatch_lineage_task_{task_number}"
            ));
        }
    }
    let cleared_stale_tasks = clear_current_task_closure_results_for_execution_reentry(
        runtime,
        &context,
        stale_tasks.clone(),
    )?;
    for task_number in cleared_stale_tasks {
        actions_performed.push(format!("cleared_current_task_closure_task_{task_number}"));
    }
    for task_number in stale_tasks {
        if clear_task_dispatch_lineage(runtime, &context, task_number)? {
            actions_performed.push(format!(
                "cleared_task_review_dispatch_lineage_task_{task_number}"
            ));
        }
    }
    Ok(())
}

fn clear_branch_scope_state_for_execution_reentry(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    if clear_current_branch_closure_for_structural_repair(runtime, &context)? {
        actions_performed.push(String::from("cleared_current_branch_closure"));
    }
    Ok(())
}

fn late_stage_branch_closure_recording_required(
    routing: &ExecutionRoutingState,
    _args: &StatusArgs,
) -> bool {
    routing.review_state_status == "missing_current_closure"
        && (routing.phase_detail == "branch_closure_recording_required_for_release_readiness"
            || routing_projects_review_state_execution_reentry(routing))
}

fn routing_projects_review_state_execution_reentry(routing: &ExecutionRoutingState) -> bool {
    routing.phase == "executing"
        && routing.phase_detail == "execution_reentry_required"
        && required_follow_up_from_routing(routing).as_deref() == Some("repair_review_state")
}

fn blocked_execution_reentry_output(
    snapshot: ReviewStateSnapshot,
    actions_performed: Vec<String>,
    args: &StatusArgs,
    trace_summary: &str,
) -> RepairReviewStateOutput {
    RepairReviewStateOutput {
        action: String::from("blocked"),
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        actions_performed,
        required_follow_up: Some(String::from("execution_reentry")),
        recommended_command: recommended_operator_command(args),
        trace_summary: trace_summary.to_owned(),
    }
}

fn restore_current_branch_closure_overlay(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<Vec<String>, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
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
