//! Review-state explain/reconcile adapters over execution-owned query and recording services.
//!
//! reconcile/explain commands stay thin over query and recording boundaries instead of
//! reaching into authoritative storage or rendered artifacts directly.

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason, branch_closure_rerecording_assessment,
    missing_derived_task_scope_overlays, task_review_result_requires_verification_reason_codes,
};
use crate::execution::next_action::{
    NextActionDecision, NextActionKind, compute_next_action_decision_with_inputs,
};
use crate::execution::query::{
    ExecutionRoutingState, ReviewStateBranchClosure, ReviewStateSnapshot, ReviewStateTaskClosure,
    follow_up_from_phase_detail as shared_follow_up_from_phase_detail,
    normalize_persisted_follow_up_alias as shared_normalize_persisted_follow_up_alias,
    normalize_public_follow_up_alias as shared_normalize_public_follow_up_alias,
    query_review_state, query_workflow_routing_state_for_runtime,
    query_workflow_routing_state_for_runtime_with_read_scope_best_effort,
    required_follow_up_from_routing, review_state_snapshot_from_read_scope_with_status,
};
use crate::execution::recording::{
    clear_current_branch_closure_for_structural_repair,
    clear_current_task_closure_results_for_execution_reentry,
    clear_current_task_closure_results_for_structural_repair,
    clear_current_task_closure_results_for_structural_repair_scope_keys,
    clear_open_step_state as clear_open_step_state_recording,
    clear_task_review_dispatch_lineage_for_execution_reentry as clear_task_dispatch_lineage,
    clear_task_review_dispatch_lineage_for_structural_repair as clear_task_dispatch_lineage_for_structural_repair_recording,
    persist_review_state_repair_follow_up, restore_review_state_projection_overlays,
};
use crate::execution::state::{
    ExecutionContext, ExecutionReadScope, ExecutionReentryCurrentTaskClosureTargets,
    ExecutionRuntime, PlanExecutionStatus, current_branch_closure_structural_review_state_reason,
    current_final_review_dispatch_authority_for_context,
    current_task_review_dispatch_id_for_status, execution_reentry_current_task_closure_targets,
    load_execution_context_for_exact_plan, load_execution_read_scope,
    task_closure_baseline_repair_candidate, task_scope_structural_review_state_reason,
    unprojected_status_from_read_scope,
};
use crate::execution::transitions::materialize_legacy_open_step_state_if_needed;

#[derive(Debug, Clone, Serialize)]
/// Runtime struct.
pub struct ExplainReviewStateOutput {
    /// Runtime field.
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    /// Runtime field.
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    /// Runtime field.
    pub superseded_closures: Vec<String>,
    /// Runtime field.
    pub stale_unreviewed_closures: Vec<String>,
    /// Runtime field.
    pub missing_derived_overlays: Vec<String>,
    /// Runtime field.
    pub next_action: String,
    /// Runtime field.
    pub recommended_command: Option<String>,
    /// Runtime field.
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
/// Runtime struct.
pub struct ReconcileReviewStateOutput {
    /// Runtime field.
    pub action: String,
    /// Runtime field.
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    /// Runtime field.
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    /// Runtime field.
    pub superseded_closures: Vec<String>,
    /// Runtime field.
    pub stale_unreviewed_closures: Vec<String>,
    /// Runtime field.
    pub missing_derived_overlays: Vec<String>,
    /// Runtime field.
    pub actions_performed: Vec<String>,
    /// Runtime field.
    pub recommended_command: String,
    /// Runtime field.
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
/// Runtime struct.
pub struct RepairReviewStateOutput {
    /// Runtime field.
    pub action: String,
    /// Runtime field.
    pub current_task_closures: Vec<ReviewStateTaskClosure>,
    /// Runtime field.
    pub current_branch_closure: Option<ReviewStateBranchClosure>,
    /// Runtime field.
    pub superseded_closures: Vec<String>,
    /// Runtime field.
    pub stale_unreviewed_closures: Vec<String>,
    /// Runtime field.
    pub missing_derived_overlays: Vec<String>,
    /// Runtime field.
    pub actions_performed: Vec<String>,
    /// Runtime field.
    pub required_follow_up: Option<String>,
    /// Runtime field.
    pub recommended_command: Option<String>,
    /// Runtime field.
    pub trace_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub phase_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub blocking_step: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// Runtime field.
    pub blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub authoritative_next_action: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepairBlockerKind {
    TaskScopeStructural,
    UnrecoverableTaskScope,
    StaleUnreviewed,
    MissingDerivedTaskScope,
    BranchScopeStructural,
    MissingDerivedBranchScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepairAction {
    RestoreProjectionOverlays,
    StructuralTaskScope {
        blocking_task: Option<u32>,
        clear_dispatch_lineage_for_structural_repair: bool,
    },
    ReentryTask {
        blocking_task: Option<u32>,
    },
    DispatchLineage {
        task_number: Option<u32>,
    },
    ReentryBranch,
}

#[derive(Debug, Clone)]
struct RepairPlan {
    blocker_kind: Option<RepairBlockerKind>,
    target_task: Option<u32>,
    target_step: Option<u32>,
    actions_to_perform: Vec<RepairAction>,
    post_repair_next_action: Option<NextActionDecision>,
}

struct RepairAnalysisInputs<'a> {
    snapshot: &'a ReviewStateSnapshot,
    post_repair_next_action: Option<NextActionDecision>,
    status_target_task: Option<u32>,
    task_scope_structural_blocking_record_present: bool,
    branch_rerecording_supported: bool,
    plan_complete: bool,
    execution_reentry_targets: &'a ExecutionReentryCurrentTaskClosureTargets,
    task_scope_structural_reason: Option<&'a str>,
    branch_scope_structural_reason: Option<&'a str>,
    unrecoverable_task_scope_task: Option<u32>,
    overlay_restore_available: Option<()>,
}

struct RepairPhaseBundle {
    read_scope: ExecutionReadScope,
    status: PlanExecutionStatus,
    snapshot: ReviewStateSnapshot,
    task_review_dispatch_id: Option<String>,
    final_review_dispatch_id: Option<String>,
    final_review_dispatch_lineage_present: bool,
    task_scope_structural_reason: Option<String>,
    branch_scope_structural_reason: Option<String>,
    execution_reentry_targets: ExecutionReentryCurrentTaskClosureTargets,
    unrecoverable_task_scope_task: Option<u32>,
    overlay_restore_available: bool,
}

struct RepairPlanAnalysis {
    repair_plan: RepairPlan,
    branch_rerecording_unsupported_reason: Option<BranchRerecordingUnsupportedReason>,
}

fn post_repair_next_action_from_phase_bundle(
    phase_bundle: &RepairPhaseBundle,
    status_args: &StatusArgs,
) -> Option<NextActionDecision> {
    compute_next_action_decision_with_inputs(
        &phase_bundle.read_scope.context,
        &phase_bundle.status,
        &phase_bundle.read_scope.context.plan_rel,
        status_args.external_review_result_ready,
        phase_bundle.task_review_dispatch_id.as_deref(),
        phase_bundle.final_review_dispatch_id.as_deref(),
        phase_bundle.final_review_dispatch_lineage_present,
    )
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn explain_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ExplainReviewStateOutput, JsonFailure> {
    let snapshot = query_review_state(runtime, args)?;
    let (next_action, recommended_command) = match query_workflow_routing_state_for_runtime(
        runtime,
        Some(&args.plan),
        args.external_review_result_ready,
    ) {
        Ok(routing) => (routing.next_action, routing.recommended_command),
        Err(_) => (
            String::from("requery workflow operator"),
            Some(recommended_operator_command(
                args,
                args.external_review_result_ready,
            )),
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

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn reconcile_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ReconcileReviewStateOutput, JsonFailure> {
    {
        let snapshot = query_review_state(runtime, args)?;
        let read_scope = load_execution_read_scope(runtime, &args.plan, true)?;
        let context = read_scope.context;
        let status = read_scope.status;
        let task_review_dispatch_id = current_task_review_dispatch_id_for_status(
            &context,
            &status,
            read_scope.overlay.as_ref(),
        );
        let final_review_dispatch_authority = current_final_review_dispatch_authority_for_context(
            &context,
            read_scope.overlay.as_ref(),
            read_scope.authoritative_state.as_ref(),
        );
        let branch_rerecording_assessment = branch_closure_rerecording_assessment(&context)?;
        let branch_rerecording_supported = branch_rerecording_assessment.supported;
        let branch_rerecording_unsupported_reason =
            branch_rerecording_assessment.unsupported_reason;
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
                    branch_rerecording_unavailable_trace(
                        branch_rerecording_unsupported_reason,
                        match reason_code {
                            "current_branch_closure_reviewed_state_malformed" => {
                                "Reconcile review state cannot repair a malformed current branch-closure reviewed-state identity, and no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required."
                            }
                            _ => {
                                "Reconcile review state cannot repair the current branch-closure review-state blocker, and no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required."
                            }
                        },
                        "Reconcile review state cannot repair the current branch-closure review-state blocker because the approved plan does not declare Late-Stage Surface metadata, so execution reentry is still required.",
                        "Reconcile review state cannot repair the current branch-closure review-state blocker because tracked drift escapes the approved Late-Stage Surface, so execution reentry is still required.",
                    )
                },
            });
        }
        if snapshot.missing_derived_overlays.is_empty()
            && snapshot.stale_unreviewed_closures.is_empty()
        {
            let routing = query_workflow_routing_state_for_runtime(
                runtime,
                Some(&args.plan),
                args.external_review_result_ready,
            )
            .ok();
            if routing
                .as_ref()
                .is_some_and(routing_projects_review_state_execution_reentry)
            {
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
                    trace_summary: String::from(
                        "Reconcile review state cannot resolve this repair-state blocker; repair-review-state must rederive the exact execution reentry target.",
                    ),
                });
            }
            if routing
                .as_ref()
                .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
            {
                let recommend_branch_closure = routing.as_ref().is_some_and(|routing| {
                    routing.phase_detail
                        == "branch_closure_recording_required_for_release_readiness"
                });
                return Ok(ReconcileReviewStateOutput {
                    action: String::from("blocked"),
                    current_task_closures: snapshot.current_task_closures,
                    current_branch_closure: snapshot.current_branch_closure,
                    superseded_closures: snapshot.superseded_closures,
                    stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
                    missing_derived_overlays: snapshot.missing_derived_overlays,
                    actions_performed: Vec::new(),
                    recommended_command: if recommend_branch_closure && branch_rerecording_supported
                    {
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
                        branch_rerecording_unavailable_trace(
                            branch_rerecording_unsupported_reason,
                            "Reconcile review state cannot mint a missing current branch closure because no still-current task-closure baseline remains to derive it, so execution reentry is still required.",
                            "Reconcile review state cannot mint a missing current branch closure because the approved plan does not declare Late-Stage Surface metadata, so execution reentry is still required.",
                            "Reconcile review state cannot mint a missing current branch closure because tracked drift escapes the approved Late-Stage Surface, so execution reentry is still required.",
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
                recommended_command: reconcile_recommended_command(
                    args,
                    &context,
                    &status,
                    task_review_dispatch_id.as_deref(),
                    final_review_dispatch_authority.dispatch_id.as_deref(),
                    final_review_dispatch_authority.lineage_present,
                )?,
                trace_summary: String::from(
                    "No derived review-state overlays required reconciliation.",
                ),
            });
        }

        let actions_performed = if snapshot.missing_derived_overlays.is_empty() {
            Vec::new()
        } else {
            restore_review_state_projection_overlays(runtime, &context)?
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
                recommended_command: reconcile_recommended_command(
                    args,
                    &context,
                    &status,
                    task_review_dispatch_id.as_deref(),
                    final_review_dispatch_authority.dispatch_id.as_deref(),
                    final_review_dispatch_authority.lineage_present,
                )?,
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
            let refreshed_routing = query_workflow_routing_state_for_runtime(
                runtime,
                Some(&args.plan),
                args.external_review_result_ready,
            )
            .ok();
            let late_stage_repair_command = format!(
                "featureforge plan execution repair-review-state --plan {}",
                args.plan.display()
            );
            let recommended_command = if refreshed_routing
                .as_ref()
                .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
            {
                if refreshed_routing.as_ref().is_some_and(|routing| {
                    routing.phase_detail
                        == "branch_closure_recording_required_for_release_readiness"
                }) && branch_rerecording_supported
                {
                    recommended_branch_closure_command(args)
                } else {
                    late_stage_repair_command
                }
            } else if refreshed_routing
                .as_ref()
                .is_some_and(routing_projects_review_state_execution_reentry)
            {
                late_stage_repair_command
            } else {
                recommended_operator_command(args, args.external_review_result_ready)
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
        let refreshed_routing = query_workflow_routing_state_for_runtime(
            runtime,
            Some(&args.plan),
            args.external_review_result_ready,
        )
        .ok();
        if refreshed_routing
            .as_ref()
            .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
        {
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
                recommended_command: if recommend_branch_closure && branch_rerecording_supported {
                    recommended_branch_closure_command(args)
                } else {
                    format!(
                        "featureforge plan execution repair-review-state --plan {}",
                        args.plan.display()
                    )
                },
                trace_summary: if branch_rerecording_supported {
                    if restored_any_overlays {
                        String::from(
                            "Reconcile review state restored derivable overlays, but branch closure must still be recorded before late-stage progression can continue.",
                        )
                    } else {
                        String::from(
                            "Reconcile review state cannot mint a missing current branch closure; branch closure must be recorded before late-stage progression can continue.",
                        )
                    }
                } else {
                    branch_rerecording_unavailable_trace(
                        branch_rerecording_unsupported_reason,
                        if restored_any_overlays {
                            "Reconcile review state restored derivable overlays, but no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required."
                        } else {
                            "Reconcile review state cannot mint a missing current branch closure because no still-current task-closure baseline remains to derive it, so execution reentry is still required."
                        },
                        if restored_any_overlays {
                            "Reconcile review state restored derivable overlays, but the approved plan does not declare Late-Stage Surface metadata, so execution reentry is still required."
                        } else {
                            "Reconcile review state cannot mint a missing current branch closure because the approved plan does not declare Late-Stage Surface metadata, so execution reentry is still required."
                        },
                        if restored_any_overlays {
                            "Reconcile review state restored derivable overlays, but tracked drift escapes the approved Late-Stage Surface, so execution reentry is still required."
                        } else {
                            "Reconcile review state cannot mint a missing current branch closure because tracked drift escapes the approved Late-Stage Surface, so execution reentry is still required."
                        },
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
            recommended_command: recommended_operator_command(
                args,
                args.external_review_result_ready,
            ),
            trace_summary: String::from(
                "Reconciled missing derived review-state overlays from authoritative closure records.",
            ),
        })
    }
}

fn load_repair_phase_bundle(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairPhaseBundle, JsonFailure> {
    let read_scope = load_execution_read_scope(runtime, &args.plan, true)?;
    let routing = query_workflow_routing_state_for_runtime_with_read_scope_best_effort(
        runtime,
        &read_scope,
        args.external_review_result_ready,
    )
    .ok();
    let status = if let Some(routing_status) = routing
        .as_ref()
        .and_then(|state| state.execution_status.clone())
    {
        routing_status
    } else {
        unprojected_status_from_read_scope(&read_scope)?
    };
    let snapshot = review_state_snapshot_from_read_scope_with_status(&read_scope, &status)?;
    let task_review_dispatch_id = current_task_review_dispatch_id_for_status(
        &read_scope.context,
        &status,
        read_scope.overlay.as_ref(),
    );
    let final_review_dispatch_authority = current_final_review_dispatch_authority_for_context(
        &read_scope.context,
        read_scope.overlay.as_ref(),
        read_scope.authoritative_state.as_ref(),
    );
    let task_scope_structural_reason =
        task_scope_structural_review_state_reason(&status).map(str::to_owned);
    let branch_scope_structural_reason =
        current_branch_closure_structural_review_state_reason(&status).map(str::to_owned);
    let execution_reentry_targets =
        execution_reentry_current_task_closure_targets(&read_scope.context)?;
    let unrecoverable_task_scope_task =
        unrecoverable_task_scope_authority_loss_task_from_read_scope(&read_scope, &status);
    Ok(RepairPhaseBundle {
        overlay_restore_available: read_scope.authoritative_state.is_some(),
        read_scope,
        status,
        snapshot,
        task_review_dispatch_id,
        final_review_dispatch_id: final_review_dispatch_authority.dispatch_id,
        final_review_dispatch_lineage_present: final_review_dispatch_authority.lineage_present,
        task_scope_structural_reason,
        branch_scope_structural_reason,
        execution_reentry_targets,
        unrecoverable_task_scope_task,
    })
}

fn task_scope_structural_blocking_record_present(status: &PlanExecutionStatus) -> bool {
    status.blocking_records.iter().any(|record| {
        matches!(
            record.code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
        )
    })
}

fn analyze_repair_phase_bundle(
    phase_bundle: &RepairPhaseBundle,
    status_args: &StatusArgs,
) -> Result<RepairPlanAnalysis, JsonFailure> {
    let branch_rerecording_assessment =
        branch_closure_rerecording_assessment(&phase_bundle.read_scope.context)?;
    let plan_complete = phase_bundle
        .read_scope
        .context
        .steps
        .iter()
        .all(|step| step.checked);
    let repair_plan = analyze_repair_plan(RepairAnalysisInputs {
        snapshot: &phase_bundle.snapshot,
        post_repair_next_action: post_repair_next_action_from_phase_bundle(
            phase_bundle,
            status_args,
        ),
        status_target_task: phase_bundle
            .status
            .blocking_task
            .or(phase_bundle.status.resume_task)
            .or(phase_bundle.status.active_task),
        task_scope_structural_blocking_record_present:
            task_scope_structural_blocking_record_present(&phase_bundle.status),
        branch_rerecording_supported: branch_rerecording_assessment.supported,
        plan_complete,
        execution_reentry_targets: &phase_bundle.execution_reentry_targets,
        task_scope_structural_reason: phase_bundle.task_scope_structural_reason.as_deref(),
        branch_scope_structural_reason: phase_bundle.branch_scope_structural_reason.as_deref(),
        unrecoverable_task_scope_task: phase_bundle.unrecoverable_task_scope_task,
        overlay_restore_available: phase_bundle.overlay_restore_available.then_some(()),
    });
    Ok(RepairPlanAnalysis {
        repair_plan,
        branch_rerecording_unsupported_reason: branch_rerecording_assessment.unsupported_reason,
    })
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn repair_review_state_command(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairReviewStateOutput, JsonFailure> {
    // Step 2 requires legacy open-step migration at mutator entry, while Step 7 requires the
    // repair core itself to remain analysis-first and mutation-free until the blocker is known.
    let entry_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let materialized_open_step =
        materialize_legacy_open_step_state_if_needed(runtime, &entry_context)?;
    let mut output = repair_review_state(runtime, args)?;
    if materialized_open_step {
        output
            .actions_performed
            .insert(0, String::from("materialized_current_open_step_state"));
    }
    Ok(output)
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn repair_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairReviewStateOutput, JsonFailure> {
    {
        let status_args = args.clone();
        let mut actions_performed = Vec::new();
        let mut phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
        let mut analysis = analyze_repair_phase_bundle(&phase_bundle, &status_args)?;
        if !analysis.repair_plan.actions_to_perform.is_empty() {
            execute_repair_actions(
                runtime,
                &phase_bundle.read_scope.context,
                &analysis.repair_plan,
                &phase_bundle.execution_reentry_targets,
                &mut actions_performed,
            )?;
            phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
            analysis = analyze_repair_phase_bundle(&phase_bundle, &status_args)?;
        }
        let repair_plan = analysis.repair_plan;
        let repaired_any_overlays = !actions_performed.is_empty();
        let snapshot = phase_bundle.snapshot.clone();
        let task_scope_structural_reason = phase_bundle.task_scope_structural_reason.clone();
        let branch_scope_structural_reason = phase_bundle.branch_scope_structural_reason.clone();
        let branch_rerecording_unsupported_reason = analysis.branch_rerecording_unsupported_reason;
        let shared_next_action = repair_plan
            .post_repair_next_action
            .clone()
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "repair-review-state failed closed because the runtime could not derive an authoritative shared next action for `{}` after reconcile.",
                        status_args.plan.display()
                    ),
                )
            })?;
        let required_follow_up = repair_required_follow_up_from_next_action(&shared_next_action);
        let performed_task_scope_structural_cleanup = actions_performed.iter().any(|action| {
            action.starts_with("cleared_current_task_closure_scope_")
                || action.starts_with("cleared_current_task_closure_task_")
                || action.starts_with("cleared_task_review_dispatch_lineage_task_")
        });
        let stale_unreviewed_closures = if performed_task_scope_structural_cleanup
            || matches!(
                repair_plan.blocker_kind,
                Some(RepairBlockerKind::TaskScopeStructural)
            ) {
            Vec::new()
        } else {
            snapshot.stale_unreviewed_closures.clone()
        };
        let recommended_command =
            if let Some(required_follow_up_lane) = required_follow_up.as_deref() {
                if shared_next_action.recommended_command.is_some() {
                    Some(recommended_follow_up_command(
                        &status_args,
                        Some(required_follow_up_lane),
                        Some(&shared_next_action),
                    )?)
                } else {
                    None
                }
            } else {
                shared_next_action.recommended_command.clone()
            };
        let persisted_required_follow_up =
            shared_normalize_persisted_follow_up_alias(required_follow_up.as_deref());
        let authoritative_phase = Some(shared_next_action.phase.clone());
        let authoritative_phase_detail = Some(shared_next_action.phase_detail.clone());
        let public_required_follow_up =
            shared_normalize_public_follow_up_alias(required_follow_up.as_deref())
                .map(str::to_owned);
        persist_review_state_repair_follow_up(
            runtime,
            &phase_bundle.read_scope.context,
            persisted_required_follow_up,
        )?;
        let final_routing = query_workflow_routing_state_for_runtime(
            runtime,
            Some(status_args.plan.as_path()),
            status_args.external_review_result_ready,
        )?;
        let final_required_follow_up = {
            let routed_follow_up = shared_normalize_public_follow_up_alias(
                required_follow_up_from_routing(&final_routing).as_deref(),
            )
            .map(str::to_owned);
            if routed_follow_up.as_deref() == Some("repair_review_state")
                && public_required_follow_up.as_deref() != Some("repair_review_state")
            {
                public_required_follow_up.clone()
            } else {
                routed_follow_up
            }
        };
        if final_routing.phase_detail == "task_closure_recording_ready" {
            let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
            let authoritative_next_action = final_routing.recommended_command.clone();
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: None,
                recommended_command: final_routing.recommended_command,
                trace_summary: String::from(
                    "Repair review state reconciled stale task-boundary state and refreshed routing; task closure is ready to record or refresh.",
                ) + blocker_metadata.as_str(),
                phase: Some(final_routing.phase.clone()),
                phase_detail: Some(final_routing.phase_detail.clone()),
                blocking_task: final_routing.blocking_task,
                blocking_step: None,
                blocking_reason_codes: final_routing.blocking_reason_codes,
                authoritative_next_action,
            });
        }
        if let Some(required_follow_up) = final_required_follow_up {
            let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
            let authoritative_next_action = final_routing.recommended_command.clone();
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: Some(required_follow_up.clone()),
                recommended_command: final_routing.recommended_command,
                trace_summary: repair_follow_up_trace_summary(
                    required_follow_up.as_str(),
                    branch_rerecording_unsupported_reason,
                    task_scope_structural_reason.as_deref(),
                    branch_scope_structural_reason.as_deref(),
                ) + blocker_metadata.as_str(),
                phase: Some(final_routing.phase.clone()),
                phase_detail: Some(final_routing.phase_detail.clone()),
                blocking_task: final_routing.blocking_task,
                blocking_step: None,
                blocking_reason_codes: final_routing.blocking_reason_codes,
                authoritative_next_action,
            });
        }
        if shared_next_action.kind == NextActionKind::CloseCurrentTask
            && shared_next_action.phase_detail == "task_closure_recording_ready"
        {
            let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
            let authoritative_next_action = shared_next_action.recommended_command;
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: None,
                recommended_command,
                trace_summary: String::from(
                    "Repair review state reconciled stale task-boundary state and refreshed routing; task closure is ready to record or refresh.",
                ) + blocker_metadata.as_str(),
                phase: authoritative_phase,
                phase_detail: authoritative_phase_detail,
                blocking_task: shared_next_action
                    .blocking_task
                    .or(shared_next_action.task_number),
                blocking_step: shared_next_action.step_number,
                blocking_reason_codes: shared_next_action.blocking_reason_codes,
                authoritative_next_action,
            });
        }
        if let Some(required_follow_up) = public_required_follow_up {
            let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
            let authoritative_next_action = shared_next_action.recommended_command;
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: Some(required_follow_up.clone()),
                recommended_command,
                trace_summary: repair_follow_up_trace_summary(
                    required_follow_up.as_str(),
                    branch_rerecording_unsupported_reason,
                    task_scope_structural_reason.as_deref(),
                    branch_scope_structural_reason.as_deref(),
                ) + blocker_metadata.as_str(),
                phase: authoritative_phase,
                phase_detail: authoritative_phase_detail,
                blocking_task: shared_next_action
                    .blocking_task
                    .or(shared_next_action.task_number),
                blocking_step: shared_next_action.step_number,
                blocking_reason_codes: shared_next_action.blocking_reason_codes,
                authoritative_next_action,
            });
        }
        if shared_next_action.kind == NextActionKind::RepairReviewState {
            let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
            let authoritative_next_action = shared_next_action.recommended_command.clone();
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: None,
                recommended_command: shared_next_action.recommended_command,
                trace_summary: String::from(
                    "Repair review state reconciled available overlays but unresolved authoritative blockers still require repair-review-state reconciliation.",
                ) + blocker_metadata.as_str(),
                phase: authoritative_phase,
                phase_detail: authoritative_phase_detail,
                blocking_task: shared_next_action
                    .blocking_task
                    .or(shared_next_action.task_number),
                blocking_step: shared_next_action.step_number,
                blocking_reason_codes: shared_next_action.blocking_reason_codes,
                authoritative_next_action,
            });
        }

        let authoritative_next_action = shared_next_action.recommended_command;
        Ok(RepairReviewStateOutput {
            action: if repaired_any_overlays {
                String::from("reconciled")
            } else {
                String::from("already_current")
            },
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: None,
            recommended_command,
            trace_summary: if repaired_any_overlays {
                String::from(
                    "Repaired missing derived review-state overlays from authoritative closure records.",
                )
            } else {
                snapshot.trace_summary
            },
            phase: authoritative_phase,
            phase_detail: authoritative_phase_detail,
            blocking_task: None,
            blocking_step: None,
            blocking_reason_codes: Vec::new(),
            authoritative_next_action,
        })
    }
}

fn unrecoverable_task_scope_authority_loss_task_from_read_scope(
    read_scope: &ExecutionReadScope,
    status: &PlanExecutionStatus,
) -> Option<u32> {
    let context = &read_scope.context;
    let overlay = read_scope.overlay.as_ref()?;
    if status.execution_started != "yes"
        || status.active_task.is_some()
        || status.resume_task.is_some()
    {
        return None;
    }
    let authoritative_state = read_scope.authoritative_state.as_ref()?;
    let earliest_checked_dispatched_task = overlay
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
        .min();
    if let Some(task_number) = earliest_checked_dispatched_task
        && authoritative_state
            .current_task_closure_result(task_number)
            .is_none()
        && authoritative_state
            .task_closure_negative_result(task_number)
            .is_none()
        && task_closure_baseline_repair_candidate(context, status, task_number)
            .ok()
            .flatten()
            .is_none()
    {
        return Some(task_number);
    }
    None
}

fn clear_task_review_dispatch_lineage_for_execution_reentry(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    task_number: Option<u32>,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let Some(task_number) = task_number else {
        return Ok(());
    };
    if clear_task_dispatch_lineage(runtime, context, task_number)? {
        actions_performed.push(format!(
            "cleared_task_review_dispatch_lineage_task_{task_number}"
        ));
    }
    Ok(())
}

fn clear_task_scope_state_for_execution_reentry(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    blocking_task: Option<u32>,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let task_number = blocking_task.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "repair-review-state failed closed because execution reentry cleanup requires an exact shared task target.",
        )
    })?;
    let cleared_tasks = clear_current_task_closure_results_for_execution_reentry(
        runtime,
        context,
        vec![task_number],
    )?;
    for task_number in cleared_tasks {
        actions_performed.push(format!("cleared_current_task_closure_task_{task_number}"));
    }
    if clear_current_branch_closure_for_structural_repair(runtime, context)? {
        actions_performed.push(String::from("cleared_current_branch_closure"));
    }
    Ok(())
}

fn clear_task_scope_state_for_structural_repair(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    execution_reentry_targets: &ExecutionReentryCurrentTaskClosureTargets,
    blocking_task: Option<u32>,
    clear_dispatch_lineage_for_structural_repair: bool,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let mut structural_tasks = execution_reentry_targets.structural_tasks.clone();
    structural_tasks.sort_unstable();
    structural_tasks.dedup();
    let mut structural_scope_keys = execution_reentry_targets
        .structural_scope_keys
        .iter()
        .filter(|scope_key| {
            scope_key
                .strip_prefix("task-")
                .and_then(|raw| raw.parse::<u32>().ok())
                .is_some()
        })
        .cloned()
        .collect::<Vec<_>>();
    let non_task_structural_scope_keys = execution_reentry_targets
        .structural_scope_keys
        .iter()
        .filter(|scope_key| {
            scope_key
                .strip_prefix("task-")
                .and_then(|raw| raw.parse::<u32>().ok())
                .is_none()
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut stale_tasks = execution_reentry_targets.stale_tasks.clone();
    if let Some(task_number) = blocking_task {
        structural_tasks.retain(|candidate| *candidate == task_number);
        stale_tasks.retain(|candidate| *candidate == task_number);
        let target_scope_key = format!("task-{task_number}");
        structural_scope_keys.retain(|scope_key| scope_key == &target_scope_key);
    }
    structural_scope_keys.extend(non_task_structural_scope_keys);
    stale_tasks.retain(|task_number| !structural_tasks.contains(task_number));
    let dispatch_lineage_tasks = if clear_dispatch_lineage_for_structural_repair {
        blocking_task
            .into_iter()
            .filter(|task_number| {
                structural_tasks.contains(task_number) || stale_tasks.contains(task_number)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let cleared_scope_keys = clear_current_task_closure_results_for_structural_repair_scope_keys(
        runtime,
        context,
        structural_scope_keys,
    )?;
    for scope_key in cleared_scope_keys {
        actions_performed.push(format!("cleared_current_task_closure_scope_{scope_key}"));
    }
    let cleared_structural_tasks = clear_current_task_closure_results_for_structural_repair(
        runtime,
        context,
        structural_tasks.clone(),
    )?;
    for task_number in cleared_structural_tasks {
        actions_performed.push(format!("cleared_current_task_closure_task_{task_number}"));
    }
    let cleared_stale_tasks = clear_current_task_closure_results_for_execution_reentry(
        runtime,
        context,
        stale_tasks.clone(),
    )?;
    for task_number in cleared_stale_tasks {
        actions_performed.push(format!("cleared_current_task_closure_task_{task_number}"));
    }
    if clear_open_step_state_recording(runtime, context)? {
        actions_performed.push(String::from("cleared_current_open_step_state"));
    }
    if clear_dispatch_lineage_for_structural_repair {
        for task_number in dispatch_lineage_tasks {
            let cleared = if structural_tasks.contains(&task_number) {
                clear_task_dispatch_lineage_for_structural_repair_recording(
                    runtime,
                    context,
                    task_number,
                )?
            } else {
                clear_task_dispatch_lineage(runtime, context, task_number)?
            };
            if cleared {
                actions_performed.push(format!(
                    "cleared_task_review_dispatch_lineage_task_{task_number}"
                ));
            }
        }
    }
    Ok(())
}

fn clear_branch_scope_state_for_execution_reentry(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    if clear_current_branch_closure_for_structural_repair(runtime, context)? {
        actions_performed.push(String::from("cleared_current_branch_closure"));
    }
    Ok(())
}

fn analyze_repair_plan(inputs: RepairAnalysisInputs<'_>) -> RepairPlan {
    {
        let stale_unreviewed_execution_reentry_required =
            !(inputs.snapshot.stale_unreviewed_closures.is_empty()
                || inputs.snapshot.branch_drift_confined_to_late_stage_surface
                    && inputs.branch_rerecording_supported);
        let missing_derived_task_scope_repair_planned =
            !inputs.snapshot.missing_derived_overlays.is_empty()
                && missing_derived_task_scope_overlays(&inputs.snapshot.missing_derived_overlays);
        let missing_derived_branch_scope_repair_planned =
            !inputs.snapshot.missing_derived_overlays.is_empty()
                && !inputs.branch_rerecording_supported;

        let structural_task_scope_detected = inputs.task_scope_structural_reason.is_some()
            || inputs.task_scope_structural_blocking_record_present
            || !inputs
                .execution_reentry_targets
                .structural_scope_keys
                .is_empty()
            || !inputs.execution_reentry_targets.structural_tasks.is_empty();
        let blocker_kind = if structural_task_scope_detected {
            Some(RepairBlockerKind::TaskScopeStructural)
        } else if inputs.unrecoverable_task_scope_task.is_some() {
            Some(RepairBlockerKind::UnrecoverableTaskScope)
        } else if stale_unreviewed_execution_reentry_required {
            Some(RepairBlockerKind::StaleUnreviewed)
        } else if missing_derived_task_scope_repair_planned {
            Some(RepairBlockerKind::MissingDerivedTaskScope)
        } else if inputs.branch_scope_structural_reason.is_some() {
            Some(RepairBlockerKind::BranchScopeStructural)
        } else if missing_derived_branch_scope_repair_planned {
            Some(RepairBlockerKind::MissingDerivedBranchScope)
        } else {
            None
        };

        let shared_target_task = inputs
            .post_repair_next_action
            .as_ref()
            .and_then(|decision| decision.blocking_task.or(decision.task_number));
        let shared_target_step = inputs
            .post_repair_next_action
            .as_ref()
            .and_then(|decision| decision.step_number);

        let mut target_task = repair_blocker_target_task(
            blocker_kind,
            shared_target_task,
            inputs.status_target_task,
            inputs.execution_reentry_targets,
            inputs.unrecoverable_task_scope_task,
        );

        let shared_required_follow_up =
            inputs
                .post_repair_next_action
                .as_ref()
                .and_then(|decision| {
                    repair_required_follow_up_from_next_action(decision).or_else(|| {
                        (decision.kind == NextActionKind::RepairReviewState
                            && decision
                                .recommended_command
                                .as_deref()
                                .is_some_and(|command| {
                                    command_invokes_subcommand(command, "repair-review-state")
                                }))
                        .then(|| String::from("repair_review_state"))
                    })
                });
        let stale_dispatch_lineage_blocking_task = inputs
            .post_repair_next_action
            .as_ref()
            .filter(|decision| {
                decision.phase_detail == "execution_reentry_required"
                    && decision
                        .blocking_reason_codes
                        .iter()
                        .any(|code| code == "prior_task_review_dispatch_stale")
                    && repair_required_follow_up_from_next_action(decision).as_deref()
                        == Some("execution_reentry")
            })
            .and_then(|decision| decision.blocking_task.or(decision.task_number));
        let stale_unreviewed_status_present = inputs
            .post_repair_next_action
            .as_ref()
            .is_some_and(|decision| decision.review_state_status == "stale_unreviewed");
        let stale_unreviewed_branch_reroute_available =
            (!inputs.snapshot.stale_unreviewed_closures.is_empty()
                || stale_unreviewed_status_present)
                && inputs.branch_rerecording_supported
                && inputs.snapshot.branch_drift_confined_to_late_stage_surface;
        if stale_unreviewed_branch_reroute_available
            && matches!(blocker_kind, Some(RepairBlockerKind::StaleUnreviewed))
        {
            target_task = None;
        }
        let stale_dispatch_lineage_cleanup_for_shared_target = stale_dispatch_lineage_blocking_task
            .is_some_and(|task_number| target_task == Some(task_number));
        let mut required_follow_up = shared_required_follow_up;
        if required_follow_up.as_deref() == Some("repair_review_state") {
            match blocker_kind {
                Some(
                    RepairBlockerKind::TaskScopeStructural
                    | RepairBlockerKind::UnrecoverableTaskScope
                    | RepairBlockerKind::MissingDerivedTaskScope,
                ) => {
                    required_follow_up = Some(String::from("execution_reentry"));
                }
                Some(RepairBlockerKind::StaleUnreviewed)
                    if !stale_unreviewed_branch_reroute_available =>
                {
                    required_follow_up = Some(String::from("execution_reentry"));
                }
                _ => {}
            }
        }

        let mut actions_to_perform = Vec::new();
        let should_restore_projection_overlays = inputs.overlay_restore_available.is_some()
            && (!inputs.snapshot.missing_derived_overlays.is_empty()
                || inputs.task_scope_structural_reason.is_some()
                || inputs.branch_scope_structural_reason.is_some());
        if should_restore_projection_overlays {
            actions_to_perform.push(RepairAction::RestoreProjectionOverlays);
        }
        let defer_missing_derived_task_scope_cleanup = matches!(
            blocker_kind,
            Some(RepairBlockerKind::MissingDerivedTaskScope)
        ) && should_restore_projection_overlays
            && inputs.plan_complete;
        let preserve_task_scope_for_late_stage_branch_reroute =
            matches!(blocker_kind, Some(RepairBlockerKind::StaleUnreviewed))
                && inputs.plan_complete
                && stale_unreviewed_branch_reroute_available;
        let execution_reentry_target_task = target_task;
        match blocker_kind {
            Some(RepairBlockerKind::TaskScopeStructural) => {
                if execution_reentry_target_task.is_some() {
                    actions_to_perform.push(RepairAction::StructuralTaskScope {
                        blocking_task: execution_reentry_target_task,
                        clear_dispatch_lineage_for_structural_repair:
                            stale_dispatch_lineage_cleanup_for_shared_target
                                && execution_reentry_target_task.is_some_and(|task_number| {
                                    stale_dispatch_lineage_blocking_task == Some(task_number)
                                }),
                    });
                }
            }
            Some(RepairBlockerKind::UnrecoverableTaskScope)
                if required_follow_up.as_deref() == Some("execution_reentry") =>
            {
                if execution_reentry_target_task.is_some() {
                    if stale_dispatch_lineage_cleanup_for_shared_target
                        && execution_reentry_target_task.is_some_and(|task_number| {
                            stale_dispatch_lineage_blocking_task == Some(task_number)
                        })
                    {
                        actions_to_perform.push(RepairAction::DispatchLineage {
                            task_number: execution_reentry_target_task,
                        });
                    }
                    actions_to_perform.push(RepairAction::ReentryTask {
                        blocking_task: execution_reentry_target_task,
                    });
                }
            }
            Some(RepairBlockerKind::StaleUnreviewed)
                if required_follow_up.as_deref() == Some("execution_reentry") =>
            {
                if execution_reentry_target_task.is_some()
                    && !preserve_task_scope_for_late_stage_branch_reroute
                {
                    if stale_dispatch_lineage_cleanup_for_shared_target
                        && execution_reentry_target_task.is_some_and(|task_number| {
                            stale_dispatch_lineage_blocking_task == Some(task_number)
                        })
                    {
                        actions_to_perform.push(RepairAction::DispatchLineage {
                            task_number: execution_reentry_target_task,
                        });
                    }
                    if !stale_unreviewed_branch_reroute_available
                        && inputs.snapshot.current_branch_closure.is_some()
                    {
                        actions_to_perform.push(RepairAction::ReentryBranch);
                    }
                    actions_to_perform.push(RepairAction::ReentryTask {
                        blocking_task: execution_reentry_target_task,
                    });
                }
            }
            Some(RepairBlockerKind::MissingDerivedTaskScope)
                if required_follow_up.as_deref() == Some("execution_reentry")
                    && !defer_missing_derived_task_scope_cleanup =>
            {
                if execution_reentry_target_task.is_some() {
                    if stale_dispatch_lineage_cleanup_for_shared_target
                        && execution_reentry_target_task.is_some_and(|task_number| {
                            stale_dispatch_lineage_blocking_task == Some(task_number)
                        })
                    {
                        actions_to_perform.push(RepairAction::DispatchLineage {
                            task_number: execution_reentry_target_task,
                        });
                    }
                    actions_to_perform.push(RepairAction::ReentryTask {
                        blocking_task: execution_reentry_target_task,
                    });
                }
            }
            Some(
                RepairBlockerKind::BranchScopeStructural
                | RepairBlockerKind::MissingDerivedBranchScope,
            ) => {
                actions_to_perform.push(RepairAction::ReentryBranch);
            }
            _ => {}
        }

        let target_step = if target_task == shared_target_task {
            shared_target_step
        } else {
            None
        };

        RepairPlan {
            blocker_kind,
            target_task,
            target_step,
            actions_to_perform,
            post_repair_next_action: inputs.post_repair_next_action,
        }
    }
}

fn first_task_number(candidates: &[u32]) -> Option<u32> {
    candidates.iter().copied().min()
}

fn first_task_number_from_scope_keys(scope_keys: &[String]) -> Option<u32> {
    scope_keys
        .iter()
        .filter_map(|scope_key| {
            scope_key
                .strip_prefix("task-")
                .and_then(|raw| raw.parse::<u32>().ok())
        })
        .min()
}

fn repair_blocker_target_task(
    blocker_kind: Option<RepairBlockerKind>,
    shared_target_task: Option<u32>,
    status_target_task: Option<u32>,
    execution_reentry_targets: &ExecutionReentryCurrentTaskClosureTargets,
    unrecoverable_task_scope_task: Option<u32>,
) -> Option<u32> {
    match blocker_kind {
        Some(RepairBlockerKind::TaskScopeStructural) => shared_target_task
            .or(status_target_task)
            .or_else(|| first_task_number(&execution_reentry_targets.structural_tasks))
            .or_else(|| {
                first_task_number_from_scope_keys(&execution_reentry_targets.structural_scope_keys)
            }),
        Some(RepairBlockerKind::UnrecoverableTaskScope) => unrecoverable_task_scope_task
            .or(status_target_task)
            .or(shared_target_task),
        Some(RepairBlockerKind::StaleUnreviewed) => {
            first_task_number(&execution_reentry_targets.stale_tasks)
                .or(status_target_task)
                .or(shared_target_task)
        }
        Some(RepairBlockerKind::MissingDerivedTaskScope) => {
            first_task_number(&execution_reentry_targets.stale_tasks)
                .or_else(|| first_task_number(&execution_reentry_targets.structural_tasks))
                .or_else(|| {
                    first_task_number_from_scope_keys(
                        &execution_reentry_targets.structural_scope_keys,
                    )
                })
                .or(unrecoverable_task_scope_task)
                .or(status_target_task)
                .or(shared_target_task)
        }
        Some(
            RepairBlockerKind::BranchScopeStructural | RepairBlockerKind::MissingDerivedBranchScope,
        )
        | None => shared_target_task,
    }
}

fn execute_repair_actions(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    plan: &RepairPlan,
    execution_reentry_targets: &ExecutionReentryCurrentTaskClosureTargets,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    for action in &plan.actions_to_perform {
        match action {
            RepairAction::RestoreProjectionOverlays => {
                let restored = restore_review_state_projection_overlays(runtime, context)?;
                for restored_action in restored {
                    if !actions_performed
                        .iter()
                        .any(|existing| existing == &restored_action)
                    {
                        actions_performed.push(restored_action);
                    }
                }
            }
            RepairAction::StructuralTaskScope {
                blocking_task,
                clear_dispatch_lineage_for_structural_repair,
            } => {
                clear_task_scope_state_for_structural_repair(
                    runtime,
                    context,
                    execution_reentry_targets,
                    *blocking_task,
                    *clear_dispatch_lineage_for_structural_repair,
                    actions_performed,
                )?;
            }
            RepairAction::ReentryTask { blocking_task } => {
                clear_task_scope_state_for_execution_reentry(
                    runtime,
                    context,
                    *blocking_task,
                    actions_performed,
                )?;
            }
            RepairAction::DispatchLineage { task_number } => {
                clear_task_review_dispatch_lineage_for_execution_reentry(
                    runtime,
                    context,
                    *task_number,
                    actions_performed,
                )?;
            }
            RepairAction::ReentryBranch => {
                clear_branch_scope_state_for_execution_reentry(
                    runtime,
                    context,
                    actions_performed,
                )?;
            }
        }
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

fn command_invokes_subcommand(command: &str, subcommand: &str) -> bool {
    command.split_whitespace().any(|token| token == subcommand)
}

fn reconcile_recommended_command(
    args: &StatusArgs,
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task_review_dispatch_id: Option<&str>,
    final_review_dispatch_id: Option<&str>,
    final_review_dispatch_lineage_present: bool,
) -> Result<String, JsonFailure> {
    let shared_next_action = compute_next_action_decision_with_inputs(
        context,
        status,
        &context.plan_rel,
        args.external_review_result_ready,
        task_review_dispatch_id,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
    )
    .ok_or_else(|| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "reconcile-review-state failed closed because reconciliation requires an authoritative shared next action.",
        )
    })?;
    let required_follow_up = repair_required_follow_up_from_next_action(&shared_next_action);
    if shared_next_action.recommended_command.is_none() {
        return Ok(recommended_operator_command(
            args,
            args.external_review_result_ready,
        ));
    }
    recommended_follow_up_command(
        args,
        required_follow_up.as_deref(),
        Some(&shared_next_action),
    )
}

fn repair_required_follow_up_from_next_action(decision: &NextActionDecision) -> Option<String> {
    match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume | NextActionKind::Reopen => {
            let phase_follow_up = shared_follow_up_from_phase_detail(
                decision.phase_detail.as_str(),
                decision.blocking_reason_codes.iter().map(String::as_str),
            )
            .map(str::to_owned);
            if phase_follow_up.is_some() {
                return phase_follow_up;
            }
            let recommended = decision.recommended_command.as_deref()?;
            if command_invokes_subcommand(recommended, "begin")
                || command_invokes_subcommand(recommended, "reopen")
            {
                return Some(String::from("execution_reentry"));
            }
            None
        }
        NextActionKind::CloseCurrentTask => shared_follow_up_from_phase_detail(
            decision.phase_detail.as_str(),
            decision.blocking_reason_codes.iter().map(String::as_str),
        )
        .map(str::to_owned),
        NextActionKind::RepairReviewState => {
            let recommended = decision.recommended_command.as_deref()?;
            let shared_execution_reentry_command = command_invokes_subcommand(recommended, "begin")
                || command_invokes_subcommand(recommended, "reopen")
                || command_invokes_subcommand(recommended, "complete");
            if shared_execution_reentry_command {
                Some(String::from("execution_reentry"))
            } else if command_invokes_subcommand(recommended, "advance-late-stage") {
                Some(String::from("advance_late_stage"))
            } else if command_invokes_subcommand(recommended, "transfer") {
                Some(String::from("record_handoff"))
            } else if command_invokes_subcommand(recommended, "record-pivot") {
                Some(String::from("record_pivot"))
            } else {
                None
            }
        }
        NextActionKind::RequestTaskReview | NextActionKind::RequestFinalReview => {
            Some(String::from("request_external_review"))
        }
        NextActionKind::WaitForTaskReviewResult => {
            if task_review_result_requires_verification_reason_codes(
                decision.blocking_reason_codes.iter().map(String::as_str),
            ) {
                Some(String::from("run_verification"))
            } else {
                Some(String::from("wait_for_external_review_result"))
            }
        }
        NextActionKind::WaitForFinalReviewResult => {
            Some(String::from("wait_for_external_review_result"))
        }
        NextActionKind::AdvanceLateStage => match decision.phase_detail.as_str() {
            "branch_closure_recording_required_for_release_readiness" => {
                Some(String::from("record_branch_closure"))
            }
            "release_blocker_resolution_required" => Some(String::from("resolve_release_blocker")),
            _ => None,
        },
        NextActionKind::PlanningReentry => Some(String::from("record_pivot")),
        NextActionKind::Handoff => Some(String::from("record_handoff")),
        NextActionKind::RefreshTestPlan | NextActionKind::RunQa | NextActionKind::FinishBranch => {
            None
        }
    }
}

fn repair_follow_up_trace_summary(
    required_follow_up: &str,
    branch_rerecording_unsupported_reason: Option<BranchRerecordingUnsupportedReason>,
    task_scope_structural_reason: Option<&str>,
    branch_scope_structural_reason: Option<&str>,
) -> String {
    match required_follow_up {
        "record_branch_closure" | "advance_late_stage" => String::from(
            "Repair review state reconciled projections and refreshed routing; branch closure must be re-recorded before late-stage progression can continue.",
        ),
        "execution_reentry" => {
            if task_scope_structural_reason.is_some() {
                return String::from(
                    "Repair review state reconciled structural task-scope blockers, but execution reentry is still required before progress can continue.",
                );
            }
            if branch_scope_structural_reason.is_some()
                || branch_rerecording_unsupported_reason.is_some()
            {
                return branch_rerecording_unavailable_trace(
                    branch_rerecording_unsupported_reason,
                    "Repair review state reconciled available branch-scope state, but no still-current task-closure baseline remains to derive a replacement branch closure, so execution reentry is still required.",
                    "Repair review state reconciled available branch-scope state, but the approved plan does not declare Late-Stage Surface metadata, so execution reentry is still required.",
                    "Repair review state reconciled available branch-scope state, but tracked drift escapes the approved Late-Stage Surface, so execution reentry is still required.",
                );
            }
            String::from(
                "Repair review state reconciled projections and refreshed routing; execution reentry is still required before progress can continue.",
            )
        }
        "request_external_review" => String::from(
            "Repair review state reconciled projections and refreshed routing; an external review dispatch is the next required step.",
        ),
        "resolve_release_blocker" => String::from(
            "Repair review state reconciled projections and refreshed routing; release blockers must be resolved before late-stage progression can continue.",
        ),
        "record_handoff" => String::from(
            "Repair review state reconciled projections and refreshed routing; record a handoff before continuing.",
        ),
        "record_pivot" => String::from(
            "Repair review state reconciled projections and refreshed routing; planning reentry is required before continuing.",
        ),
        _ => {
            format!(
                "Repair review state reconciled projections and refreshed routing; required follow-up is {required_follow_up}."
            )
        }
    }
}

fn repair_blocker_metadata_suffix(plan: &RepairPlan) -> String {
    let Some(blocker_kind) = plan.blocker_kind else {
        return String::new();
    };
    let blocker = match blocker_kind {
        RepairBlockerKind::TaskScopeStructural => "task_scope_structural",
        RepairBlockerKind::UnrecoverableTaskScope => "unrecoverable_task_scope",
        RepairBlockerKind::StaleUnreviewed => "stale_unreviewed",
        RepairBlockerKind::MissingDerivedTaskScope => "missing_derived_task_scope",
        RepairBlockerKind::BranchScopeStructural => "branch_scope_structural",
        RepairBlockerKind::MissingDerivedBranchScope => "missing_derived_branch_scope",
    };
    let mut metadata = format!(" [blocker={blocker}");
    if let Some(task) = plan.target_task {
        metadata.push_str(format!(", target_task={task}").as_str());
    }
    if let Some(step) = plan.target_step {
        metadata.push_str(format!(", target_step={step}").as_str());
    }
    if let Some(next_action_command) = plan
        .post_repair_next_action
        .as_ref()
        .and_then(|decision| decision.recommended_command.as_deref())
    {
        metadata.push_str(format!(", authoritative_next_action={next_action_command}").as_str());
    }
    metadata.push(']');
    metadata
}

fn branch_rerecording_unavailable_trace(
    unsupported_reason: Option<BranchRerecordingUnsupportedReason>,
    missing_task_closure_baseline_message: &str,
    missing_late_stage_surface_message: &str,
    drift_escapes_late_stage_surface_message: &str,
) -> String {
    match unsupported_reason {
        Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => {
            String::from(missing_late_stage_surface_message)
        }
        Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) => {
            String::from(drift_escapes_late_stage_surface_message)
        }
        Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) | None => {
            String::from(missing_task_closure_baseline_message)
        }
    }
}

fn recommended_follow_up_command(
    args: &StatusArgs,
    required_follow_up: Option<&str>,
    shared_decision: Option<&NextActionDecision>,
) -> Result<String, JsonFailure> {
    let shared_decision = shared_decision.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "repair-review-state failed closed because follow-up routing requires a shared next-action decision.",
        )
    })?;
    let recommended_command = shared_decision
        .recommended_command
        .clone()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "repair-review-state failed closed because the shared next-action decision did not provide a recommended command.",
            )
        })?;
    if let Some(required_follow_up) = required_follow_up
        && !command_matches_required_follow_up(
            args,
            required_follow_up,
            recommended_command.as_str(),
        )
    {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "repair-review-state failed closed because the shared next-action command does not match the required follow-up lane.",
        ));
    }
    Ok(recommended_command)
}

fn command_matches_required_follow_up(
    _args: &StatusArgs,
    required_follow_up: &str,
    recommended_command: &str,
) -> bool {
    command_matches_required_follow_up_lane(required_follow_up, recommended_command)
}

fn command_matches_required_follow_up_lane(
    required_follow_up: &str,
    recommended_command: &str,
) -> bool {
    match required_follow_up {
        "execution_reentry" => {
            recommended_command.starts_with("featureforge plan execution begin --plan ")
                || recommended_command.starts_with("featureforge plan execution reopen --plan ")
                || recommended_command.starts_with("featureforge plan execution complete --plan ")
        }
        "record_branch_closure" | "advance_late_stage" | "resolve_release_blocker" => {
            recommended_command.contains("featureforge plan execution advance-late-stage --plan")
        }
        "request_external_review" | "wait_for_external_review_result" | "run_verification" => {
            recommended_command.starts_with("featureforge workflow operator --plan ")
        }
        "record_handoff" => {
            recommended_command.starts_with("featureforge plan execution transfer --plan ")
        }
        "record_pivot" => {
            recommended_command.starts_with("featureforge workflow record-pivot --plan ")
        }
        _ => true,
    }
}

fn recommended_operator_command(args: &StatusArgs, external_review_result_ready: bool) -> String {
    let mut command = format!(
        "featureforge workflow operator --plan {}",
        args.plan.display()
    );
    if external_review_result_ready {
        command.push_str(" --external-review-result-ready");
    }
    command
}

fn recommended_branch_closure_command(args: &StatusArgs) -> String {
    format!(
        "featureforge plan execution advance-late-stage --plan {}",
        args.plan.display()
    )
}
