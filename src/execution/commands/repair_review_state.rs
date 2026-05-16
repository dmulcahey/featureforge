use super::common::*;

use crate::execution::command_eligibility::PublicCommand;
use crate::execution::current_truth::resolve_actionable_repair_follow_up_for_status;
use crate::execution::follow_up::{
    FollowUpAliasContext, RepairFollowUpRecord, execution_step_repair_target_id,
    normalize_follow_up_alias,
};
use crate::execution::next_action::diagnostic_next_action_for_route;
use crate::execution::public_recovery::public_recovery_contract_for_follow_up;
use crate::execution::query::required_follow_up_from_routing;
use crate::execution::recording::{
    clear_current_branch_closure_for_structural_repair,
    clear_current_task_closure_results_for_execution_reentry,
    clear_current_task_closure_results_for_structural_repair,
    clear_current_task_closure_results_for_structural_repair_scope_keys,
    clear_open_step_state as clear_open_step_state_recording,
    clear_task_review_dispatch_lineage_for_execution_reentry as clear_task_dispatch_lineage,
    clear_task_review_dispatch_lineage_for_structural_repair as clear_task_dispatch_lineage_for_structural_repair_recording,
    persist_review_state_repair_follow_up,
    release_worktree_leases_for_current_task_closures_and_persist,
    resolve_current_task_closure_postconditions_for_current_workspace_and_persist,
    restore_review_state_projection_overlays, review_state_repair_follow_up_would_mutate,
};
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_DETAIL, TargetlessStaleReconcile,
};
use crate::execution::repair_route_decision::{
    PostRepairRouteFollowUpState, RepairBlockerKind, RepairReviewStateFollowUpInputs,
    repair_review_state_final_required_follow_up, repair_review_state_follow_up_decision,
};
use crate::execution::review_state::{
    RepairAction, RepairPhaseBundle, RepairPlan, RepairReviewStateOutput, RepairRouteActionKind,
    analyze_repair_phase_bundle, branch_closure_repair_route_decision,
    diagnostic_only_close_current_task_recovery_output, execution_reentry_repair_surfaces,
    explicit_execution_reentry_target, load_repair_phase_bundle, repair_blocker_metadata_suffix,
    repair_can_establish_empty_lineage_branch_reroute, repair_follow_up_trace_summary,
    repair_review_state_close_current_task_output, route_decision_surfaces, route_for_plan,
    target_bound_repair_follow_up_record, targetless_stale_reconcile_output,
};
use crate::execution::route_plan::{
    required_follow_up_from_route_decision, state_kind_or_phase_is_runtime_diagnostic,
};
use crate::execution::state::worktree_lease_public_gate_reason_code;
use crate::execution::task_scope_key::task_scope_key_task_number;
use crate::git::sha256_hex;

pub fn repair_review_state_command(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairReviewStateOutput, JsonFailure> {
    repair_review_state(runtime, args)
}

fn require_repair_review_state_mutation(status: &PlanExecutionStatus) -> Result<(), JsonFailure> {
    require_public_mutation(
        status,
        PublicMutationRequest::repair_review_state(),
        FailureClass::ExecutionStateNotReady,
    )
}

fn repair_review_state_has_explicit_target(status: &PlanExecutionStatus) -> bool {
    status.public_repair_targets.iter().any(|target| {
        PublicCommandKind::RepairReviewState.matches_public_mutation_token(&target.command_kind)
    })
}

fn repair_review_state_external_wait_output(
    phase_bundle: RepairPhaseBundle,
    actions_performed: Vec<String>,
) -> RepairReviewStateOutput {
    let recovery = public_recovery_contract_for_follow_up(
        Path::new(&phase_bundle.read_scope.context.plan_rel),
        None,
        Some(
            FollowUpKind::WaitForExternalReviewResult
                .public_token()
                .to_owned(),
        ),
    );
    RepairReviewStateOutput {
        action: String::from("blocked"),
        current_task_closures: phase_bundle.snapshot.current_task_closures,
        current_branch_closure: phase_bundle.snapshot.current_branch_closure,
        superseded_closures: phase_bundle.snapshot.superseded_closures,
        stale_unreviewed_closures: phase_bundle.snapshot.stale_unreviewed_closures,
        missing_derived_overlays: phase_bundle.snapshot.missing_derived_overlays,
        actions_performed,
        required_follow_up: recovery.required_follow_up,
        next_action: None,
        recommended_command: recovery.recommended_command,
        recommended_public_command_argv: recovery.recommended_public_command_argv,
        recommended_public_command_template: recovery.recommended_public_command_template,
        required_inputs: recovery.required_inputs,
        trace_summary: String::from(
            "Repair review state refreshed routing, but an external review result is pending; no local repair mutation is authorized until that result is available.",
        ),
        phase: phase_bundle
            .status
            .phase
            .clone()
            .or_else(|| Some(phase_bundle.route_decision.phase.clone())),
        phase_detail: Some(phase_bundle.status.phase_detail.clone()),
        blocking_task: phase_bundle.status.blocking_task,
        blocking_step: phase_bundle.status.blocking_step,
        blocking_reason_codes: phase_bundle.status.blocking_reason_codes.clone(),
        authoritative_next_action: None,
    }
}

struct RepairReviewStateSelfLoopBlock {
    snapshot: crate::execution::query::ReviewStateSnapshot,
    actions_performed: Vec<String>,
    blocker_metadata: String,
    authoritative_phase: Option<String>,
    authoritative_phase_detail: Option<String>,
    blocking_task: Option<u32>,
    blocking_step: Option<u32>,
    blocking_reason_codes: Vec<String>,
}

fn repair_review_state_self_loop_blocked_output(
    context: RepairReviewStateSelfLoopBlock,
) -> RepairReviewStateOutput {
    let RepairReviewStateSelfLoopBlock {
        snapshot,
        actions_performed,
        blocker_metadata,
        authoritative_phase,
        authoritative_phase_detail,
        blocking_task,
        blocking_step,
        blocking_reason_codes,
    } = context;
    RepairReviewStateOutput {
        action: String::from("blocked"),
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        actions_performed,
        required_follow_up: None,
        next_action: None,
        recommended_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        trace_summary: String::from(
            "Repair review state reconciled available runtime state, but the route still resolves back to repair-review-state. Stop and report this runtime diagnostic instead of rerunning the same command.",
        ) + blocker_metadata.as_str(),
        phase: authoritative_phase,
        phase_detail: authoritative_phase_detail,
        blocking_task,
        blocking_step,
        blocking_reason_codes,
        authoritative_next_action: None,
    }
}

fn clear_resolved_task_cycle_break_for_repair_review_state(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    actions_performed: &mut Vec<String>,
) -> Result<bool, JsonFailure> {
    let cycle_break_active = status
        .reason_codes
        .iter()
        .chain(status.blocking_reason_codes.iter())
        .any(|reason_code| reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE);
    if !cycle_break_active {
        return Ok(false);
    }
    if status.external_wait_state.as_deref()
        == Some(crate::execution::review_route_tokens::EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT)
        && !repair_review_state_has_explicit_target(status)
    {
        return Ok(false);
    }
    let task_number = status
        .blocking_task
        .or(status.active_task)
        .or(status.resume_task)
        .or_else(|| {
            status
                .execution_command_context
                .as_ref()
                .and_then(|context| context.task_number)
        });
    let Some(task_number) = task_number else {
        return Ok(false);
    };
    require_repair_review_state_mutation(status)?;
    let Some(closure_record_id) =
        resolve_current_task_closure_postconditions_for_current_workspace_and_persist(
            runtime,
            context,
            task_number,
            None,
        )?
    else {
        return Ok(false);
    };
    actions_performed.push(format!(
        "cleared_resolved_task_cycle_break_task_{task_number}_{closure_record_id}"
    ));
    Ok(true)
}

fn release_resolved_worktree_leases_for_repair_review_state(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    actions_performed: &mut Vec<String>,
) -> Result<bool, JsonFailure> {
    let worktree_lease_blocker = status
        .reason_codes
        .iter()
        .chain(status.blocking_reason_codes.iter())
        .any(|reason_code| worktree_lease_public_gate_reason_code(reason_code));
    if !worktree_lease_blocker {
        return Ok(false);
    }
    require_repair_review_state_mutation(status)?;
    let resolved = release_worktree_leases_for_current_task_closures_and_persist(
        runtime,
        context,
        crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
    )?;
    if resolved.is_empty() {
        return Ok(false);
    }
    for (task_number, closure_record_id) in resolved {
        actions_performed.push(format!(
            "released_resolved_worktree_lease_task_{task_number}_{closure_record_id}"
        ));
    }
    Ok(true)
}

fn review_state_follow_up_persist_would_mutate(
    context: &ExecutionContext,
    follow_up: Option<&RepairFollowUpRecord>,
) -> Result<bool, JsonFailure> {
    review_state_repair_follow_up_would_mutate(context, follow_up)
}

fn persist_execution_reentry_repair_target_and_refresh_routing(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    step: u32,
) -> Result<ExecutionRoutingState, JsonFailure> {
    require_repair_review_state_mutation(status)?;
    let created_sequence = status.latest_authoritative_sequence.saturating_add(1);
    persist_review_state_repair_follow_up(
        runtime,
        context,
        Some(&RepairFollowUpRecord {
            kind: RepairFollowUpKind::ExecutionReentry,
            target_scope: RepairFollowUpKind::ExecutionReentry.target_scope(),
            target_task: Some(task),
            target_step: Some(step),
            target_record_id: Some(execution_step_repair_target_id(task, step)),
            semantic_workspace_state_id: Some(
                semantic_workspace_snapshot(context)?.semantic_workspace_tree_id,
            ),
            source_route_decision_hash: Some(sha256_hex(
                format!(
                    "execution_reentry:{}:{}:{}:{}",
                    task, step, status.phase_detail, status.review_state_status
                )
                .as_bytes(),
            )),
            created_sequence,
            created_at: Some(Timestamp::now().to_string()),
            expires_on_plan_fingerprint_change: true,
        }),
    )?;
    route_for_plan(runtime, args)
}

pub fn repair_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairReviewStateOutput, JsonFailure> {
    let status_args = args.clone();
    let mut actions_performed = Vec::new();
    let mut phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
    if phase_bundle.status.phase_detail
        == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    {
        require_repair_review_state_mutation(&phase_bundle.status)?;
    }
    if clear_resolved_task_cycle_break_for_repair_review_state(
        runtime,
        &phase_bundle.read_scope.context,
        &phase_bundle.status,
        &mut actions_performed,
    )? {
        phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
    }
    if release_resolved_worktree_leases_for_repair_review_state(
        runtime,
        &phase_bundle.read_scope.context,
        &phase_bundle.status,
        &mut actions_performed,
    )? {
        phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
    }
    let mut analysis = analyze_repair_phase_bundle(&phase_bundle, &status_args)?;
    let original_repair_plan = analysis.repair_plan.clone();
    let original_branch_rerecording_unsupported_reason =
        analysis.branch_rerecording_unsupported_reason;
    let original_empty_lineage_branch_reroute_repairable =
        repair_can_establish_empty_lineage_branch_reroute(
            &phase_bundle,
            original_branch_rerecording_unsupported_reason,
        );
    let original_branch_closure_target_id = phase_bundle
        .snapshot
        .current_branch_closure
        .as_ref()
        .map(|closure| closure.branch_closure_id.clone())
        .or_else(|| phase_bundle.status.current_branch_closure_id.clone());
    if !analysis.repair_plan.actions_to_perform.is_empty() {
        if phase_bundle.status.external_wait_state.as_deref()
            == Some(
                crate::execution::review_route_tokens::EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT,
            )
            && !repair_review_state_has_explicit_target(&phase_bundle.status)
        {
            return Ok(repair_review_state_external_wait_output(
                phase_bundle,
                actions_performed,
            ));
        }
        require_repair_review_state_mutation(&phase_bundle.status)?;
        execute_repair_actions(
            runtime,
            &phase_bundle.read_scope.context,
            &analysis.repair_plan,
            &phase_bundle,
            &mut actions_performed,
        )?;
        phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
        if clear_resolved_task_cycle_break_for_repair_review_state(
            runtime,
            &phase_bundle.read_scope.context,
            &phase_bundle.status,
            &mut actions_performed,
        )? {
            phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
        }
        if release_resolved_worktree_leases_for_repair_review_state(
            runtime,
            &phase_bundle.read_scope.context,
            &phase_bundle.status,
            &mut actions_performed,
        )? {
            phase_bundle = load_repair_phase_bundle(runtime, &status_args)?;
        }
        analysis = analyze_repair_phase_bundle(&phase_bundle, &status_args)?;
    }
    let repair_plan = analysis.repair_plan;
    let repaired_any_overlays = !actions_performed.is_empty();
    let snapshot = phase_bundle.snapshot.clone();
    let task_scope_structural_reason = phase_bundle.task_scope_structural_reason.clone();
    let branch_scope_structural_reason = phase_bundle.branch_scope_structural_reason.clone();
    let branch_rerecording_unsupported_reason = analysis.branch_rerecording_unsupported_reason;
    let stale_reentry_repair_plan = if !actions_performed.is_empty()
        && original_repair_plan.blocker_kind == Some(RepairBlockerKind::StaleUnreviewed)
    {
        &original_repair_plan
    } else {
        &repair_plan
    };
    let stale_reentry_branch_rerecording_unsupported_reason =
        branch_rerecording_unsupported_reason.or(original_branch_rerecording_unsupported_reason);
    let route_decision = repair_plan.post_repair_route_decision.clone();
    let route_action = repair_plan.post_repair_route_action.clone();
    let performed_current_task_closure_cleanup = actions_performed.iter().any(|action| {
        action.starts_with("cleared_current_task_closure_scope_")
            || action.starts_with("cleared_current_task_closure_task_")
    });
    let cleared_current_branch_closure = actions_performed
        .iter()
        .any(|action| action == "cleared_current_branch_closure");
    let persisted_close_task_follow_up_target = resolve_actionable_repair_follow_up_for_status(
        &phase_bundle.read_scope.context,
        &phase_bundle.status,
        phase_bundle.read_scope.authoritative_state.as_ref(),
    )
    .filter(|record| record.kind == RepairFollowUpKind::CloseTask)
    .and_then(|record| record.target_task)
    .or_else(|| {
        // A CloseTask follow-up intentionally clears the current closure row before
        // the next repair-review-state rerun, so the generic exact-binding resolver
        // can reject the just-written record. This fallback is limited to that
        // runtime-owned record and the empty post-repair closure checks below.
        phase_bundle
            .read_scope
            .authoritative_state
            .as_ref()
            .and_then(|state| state.review_state_repair_follow_up_record())
            .filter(|record| record.kind == RepairFollowUpKind::CloseTask)
            .and_then(|record| record.target_task)
    });
    let empty_lineage_branch_reroute_repairable = repair_can_establish_empty_lineage_branch_reroute(
        &phase_bundle,
        branch_rerecording_unsupported_reason,
    );
    let route_required_follow_up = required_follow_up_from_route_decision(&route_decision);
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
    let follow_up_decision =
        repair_review_state_follow_up_decision(RepairReviewStateFollowUpInputs {
            repair_plan: repair_plan.follow_up_state(),
            stale_reentry_repair_plan: stale_reentry_repair_plan.follow_up_state(),
            route: PostRepairRouteFollowUpState {
                state_kind: &route_decision.state_kind,
                phase_detail: &route_decision.phase_detail,
                review_state_status: &route_decision.review_state_status,
                required_follow_up: route_required_follow_up.as_deref(),
                blocking_reason_codes: &route_action.blocking_reason_codes,
                recommends_execution_reentry: route_action.recommends_execution_reentry,
            },
            performed_current_task_closure_cleanup,
            persisted_close_task_follow_up_target,
            cleared_current_branch_closure,
            current_task_closures_empty: snapshot.current_task_closures.is_empty(),
            stale_unreviewed_closures_empty: stale_unreviewed_closures.is_empty(),
            task_scope_structural_reason_present: task_scope_structural_reason.is_some(),
            branch_scope_structural_reason_present: branch_scope_structural_reason.is_some(),
            post_repair_status_current_task_closures_empty: phase_bundle
                .status
                .current_task_closures
                .is_empty(),
            branch_rerecording_supported: branch_rerecording_unsupported_reason.is_none(),
            empty_lineage_branch_reroute_repairable,
            original_empty_lineage_branch_reroute_repairable,
            missing_derived_overlays_empty: snapshot.missing_derived_overlays.is_empty(),
        });
    let authoritative_phase = Some(route_decision.phase.clone());
    let authoritative_phase_detail = Some(route_decision.phase_detail.clone());
    let public_required_follow_up = follow_up_decision.public_required_follow_up.clone();
    let mut persisted_follow_up_record =
        follow_up_decision
            .persisted_required_follow_up
            .map(|follow_up| {
                target_bound_repair_follow_up_record(
                    follow_up,
                    &phase_bundle,
                    stale_reentry_repair_plan,
                    &repair_plan,
                    &route_decision,
                    follow_up_decision.target.task,
                    follow_up_decision.target.step,
                )
            });
    if original_empty_lineage_branch_reroute_repairable
        && let Some(record) = persisted_follow_up_record.as_mut()
        && record.kind == RepairFollowUpKind::RecordBranchClosure
        && record.target_record_id.is_none()
    {
        record
            .target_record_id
            .clone_from(&original_branch_closure_target_id);
    }
    let repair_follow_up_would_mutate = review_state_follow_up_persist_would_mutate(
        &phase_bundle.read_scope.context,
        persisted_follow_up_record.as_ref(),
    )?;
    if repair_follow_up_would_mutate
        || follow_up_decision.current_route_requires_no_repair_follow_up
    {
        if phase_bundle.status.external_wait_state.as_deref()
            == Some(
                crate::execution::review_route_tokens::EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT,
            )
            && !repair_review_state_has_explicit_target(&phase_bundle.status)
        {
            return Ok(repair_review_state_external_wait_output(
                phase_bundle,
                actions_performed,
            ));
        }
        require_repair_review_state_mutation(&phase_bundle.status)?;
        persist_review_state_repair_follow_up(
            runtime,
            &phase_bundle.read_scope.context,
            persisted_follow_up_record.as_ref(),
        )?;
    }
    let final_routing = route_for_plan(runtime, &status_args)?;
    if follow_up_decision.task_closure_repair_ready_for_recording
        && task_scope_structural_reason.is_none()
        && branch_scope_structural_reason.is_none()
        && snapshot.current_task_closures.is_empty()
        && let Some(task_number) = follow_up_decision
            .task_closure_repair_target_task
            .or(final_routing.blocking_task)
    {
        return Ok(repair_review_state_close_current_task_output(
            snapshot,
            stale_unreviewed_closures.clone(),
            actions_performed,
            &final_routing,
            task_number,
            String::from(
                "Repair review state reconciled stale task-boundary state and refreshed routing; task closure is ready for close-current-task.",
            ) + repair_blocker_metadata_suffix(&repair_plan).as_str(),
        ));
    }
    let final_required_follow_up = repair_review_state_final_required_follow_up(
        required_follow_up_from_routing(&final_routing).as_deref(),
        public_required_follow_up.as_deref(),
    );
    if (empty_lineage_branch_reroute_repairable || original_empty_lineage_branch_reroute_repairable)
        && cleared_current_branch_closure
        && task_scope_structural_reason.is_none()
        && branch_scope_structural_reason.is_none()
        && final_routing.phase_detail
            == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
    {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        let branch_route_decision = branch_closure_repair_route_decision(&phase_bundle)?;
        let (
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
        ) = route_decision_surfaces(&branch_route_decision);
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: stale_unreviewed_closures.clone(),
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            )),
            next_action: None,
            recommended_command: recommended_command.clone(),
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
            trace_summary: String::from(
                "Repair review state reconciled projections and refreshed routing; branch closure must be re-recorded before late-stage progression can continue.",
            ) + blocker_metadata.as_str(),
            phase: Some(branch_route_decision.phase),
            phase_detail: Some(branch_route_decision.phase_detail),
            blocking_task: None,
            blocking_step: None,
            blocking_reason_codes: branch_route_decision.blocking_reason_codes,
            authoritative_next_action: None,
        });
    }
    if final_routing.phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && public_required_follow_up.as_deref()
            != Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY)
    {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        let trace_summary = String::from(
            "Repair review state reconciled stale task-boundary state and refreshed routing; task closure is ready for close-current-task.",
        ) + blocker_metadata.as_str();
        let Some(task_number) = final_routing.blocking_task else {
            return Ok(diagnostic_only_close_current_task_recovery_output(
                snapshot,
                stale_unreviewed_closures.clone(),
                actions_performed,
                &final_routing,
                None,
                trace_summary,
            ));
        };
        return Ok(repair_review_state_close_current_task_output(
            snapshot,
            stale_unreviewed_closures.clone(),
            actions_performed,
            &final_routing,
            task_number,
            trace_summary,
        ));
    }
    if repair_plan.blocker_kind == Some(RepairBlockerKind::TaskClosureBaselineBridge)
        && let Some(task_number) = repair_plan.target_task.or(final_routing.blocking_task)
    {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        return Ok(repair_review_state_close_current_task_output(
            snapshot,
            stale_unreviewed_closures.clone(),
            actions_performed,
            &final_routing,
            task_number,
            String::from(
                "Repair review state reconciled stale task-boundary state and refreshed routing; task closure is ready for close-current-task.",
            ) + blocker_metadata.as_str(),
        ));
    }
    let final_route_requires_branch_rerecording = final_routing.phase_detail
        == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        && final_routing
            .execution_status
            .as_ref()
            .is_some_and(|status| {
                status.current_branch_closure_id.is_some()
                    && (status.current_branch_meaningful_drift
                        || status.blocking_records.iter().any(|record| {
                            record.record_type == "branch_closure"
                                && record.review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
                                && record.required_follow_up.as_deref()
                                    == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE)
                        }))
            });
    if final_route_requires_branch_rerecording
        && task_scope_structural_reason.is_none()
        && branch_scope_structural_reason.is_none()
    {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        if final_routing.current_release_readiness_result.is_some() {
            let branch_route_decision = match final_routing.recommended_public_command.as_ref() {
                Some(PublicCommand::AdvanceLateStage { .. }) => {
                    if let Some(route_decision) = final_routing.route_decision.as_ref() {
                        route_decision.clone()
                    } else {
                        branch_closure_repair_route_decision(&phase_bundle)?
                    }
                }
                _ => branch_closure_repair_route_decision(&phase_bundle)?,
            };
            let (
                recommended_command,
                recommended_public_command_argv,
                recommended_public_command_template,
                required_inputs,
            ) = route_decision_surfaces(&branch_route_decision);
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures: stale_unreviewed_closures.clone(),
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: Some(String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
                )),
                next_action: None,
                recommended_command: recommended_command.clone(),
                recommended_public_command_argv,
                recommended_public_command_template,
                required_inputs,
                trace_summary: String::from(
                    "Repair review state reconciled projections and refreshed routing; public late-stage advancement must refresh branch lineage before final review can continue.",
                ) + blocker_metadata.as_str(),
                phase: Some(branch_route_decision.phase),
                phase_detail: Some(branch_route_decision.phase_detail),
                blocking_task: branch_route_decision
                    .recording_context
                    .and_then(|context| context.task_number)
                    .or(final_routing.blocking_task),
                blocking_step: None,
                blocking_reason_codes: branch_route_decision.blocking_reason_codes,
                authoritative_next_action: None,
            });
        }
        let branch_route_decision = match final_routing.recommended_public_command.as_ref() {
            Some(PublicCommand::AdvanceLateStage { .. }) => {
                if let Some(route_decision) = final_routing.route_decision.as_ref() {
                    route_decision.clone()
                } else {
                    branch_closure_repair_route_decision(&phase_bundle)?
                }
            }
            _ => branch_closure_repair_route_decision(&phase_bundle)?,
        };
        let (
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
        ) = route_decision_surfaces(&branch_route_decision);
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: stale_unreviewed_closures.clone(),
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            )),
            next_action: None,
            recommended_command: recommended_command.clone(),
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
            trace_summary: String::from(
                "Repair review state reconciled projections and refreshed routing; branch closure must be re-recorded before late-stage progression can continue.",
            ) + blocker_metadata.as_str(),
            phase: Some(branch_route_decision.phase),
            phase_detail: Some(branch_route_decision.phase_detail),
            blocking_task: branch_route_decision
                .recording_context
                .and_then(|context| context.task_number)
                .or(final_routing.blocking_task),
            blocking_step: None,
            blocking_reason_codes: branch_route_decision.blocking_reason_codes,
            authoritative_next_action: None,
        });
    }
    if stale_reentry_repair_plan.blocker_kind == Some(RepairBlockerKind::StaleUnreviewed)
        && stale_reentry_branch_rerecording_unsupported_reason.is_some()
        && task_scope_structural_reason.is_none()
        && branch_scope_structural_reason.is_none()
    {
        let Some((task_number, step_number)) =
            explicit_execution_reentry_target(stale_reentry_repair_plan)
        else {
            let blocker_metadata = repair_blocker_metadata_suffix(stale_reentry_repair_plan);
            return Ok(targetless_stale_reconcile_output(
                snapshot,
                stale_unreviewed_closures.clone(),
                actions_performed,
                final_routing.blocking_reason_codes.clone(),
                blocker_metadata,
                final_routing.recommended_public_command.clone(),
            ));
        };
        let final_routing = persist_execution_reentry_repair_target_and_refresh_routing(
            runtime,
            &status_args,
            &phase_bundle.read_scope.context,
            &phase_bundle.status,
            task_number,
            step_number,
        )?;
        let (reopen_command, reopen_command_argv, reopen_command_template, required_inputs) =
            execution_reentry_repair_surfaces(Some(&final_routing), task_number, step_number);
        let blocker_metadata = repair_blocker_metadata_suffix(stale_reentry_repair_plan);
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: stale_unreviewed_closures.clone(),
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
            )),
            next_action: None,
            recommended_command: reopen_command.clone(),
            recommended_public_command_argv: reopen_command_argv,
            recommended_public_command_template: reopen_command_template,
            required_inputs,
            trace_summary: repair_follow_up_trace_summary(
                crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
                stale_reentry_branch_rerecording_unsupported_reason,
                task_scope_structural_reason.as_deref(),
                branch_scope_structural_reason.as_deref(),
            ) + blocker_metadata.as_str(),
            phase: Some(String::from(crate::execution::phase::PHASE_EXECUTING)),
            phase_detail: Some(String::from(
                crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
            )),
            blocking_task: Some(task_number),
            blocking_step: Some(step_number),
            blocking_reason_codes: final_routing.blocking_reason_codes.clone(),
            authoritative_next_action: None,
        });
    }
    if let Some(required_follow_up) = final_required_follow_up.as_deref() {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        let required_follow_up_kind = normalize_follow_up_alias(
            Some(required_follow_up),
            FollowUpAliasContext::PublicRouting,
        );
        if required_follow_up_kind == Some(FollowUpKind::RepairReviewState) {
            return Ok(repair_review_state_self_loop_blocked_output(
                RepairReviewStateSelfLoopBlock {
                    snapshot,
                    actions_performed,
                    blocker_metadata,
                    authoritative_phase,
                    authoritative_phase_detail,
                    blocking_task: route_action.blocking_task.or(route_action.task_number),
                    blocking_step: route_action.step_number,
                    blocking_reason_codes: route_action.blocking_reason_codes.clone(),
                },
            ));
        }
        let public_required_follow_up = if required_follow_up_kind
            == Some(FollowUpKind::RequestExternalReview)
            && final_routing.phase_detail
                == crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
            && matches!(
                final_routing.recommended_public_command.as_ref(),
                Some(PublicCommand::AdvanceLateStage { .. })
            ) {
            FollowUpKind::AdvanceLateStage.public_token()
        } else {
            required_follow_up
        };
        let recovery = public_recovery_contract_for_follow_up(
            &status_args.plan,
            Some(&final_routing),
            Some(public_required_follow_up.to_owned()),
        );
        let (
            output_required_follow_up,
            output_recommended_command,
            output_recommended_public_command_argv,
            output_recommended_public_command_template,
            output_required_inputs,
        ) = (
            recovery.required_follow_up,
            recovery.recommended_command,
            recovery.recommended_public_command_argv,
            recovery.recommended_public_command_template,
            recovery.required_inputs,
        );
        if required_follow_up == crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY
            && task_scope_structural_reason.is_none()
            && branch_scope_structural_reason.is_none()
            && repair_plan.blocker_kind != Some(RepairBlockerKind::StaleUnreviewed)
            && final_routing
                .blocking_reason_codes
                .iter()
                .any(|code| code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING)
            && let Some(task_number) = final_routing.blocking_task
        {
            return Ok(repair_review_state_close_current_task_output(
                snapshot,
                stale_unreviewed_closures.clone(),
                actions_performed,
                &final_routing,
                task_number,
                String::from(
                    "Repair review state reconciled stale task-boundary state and refreshed routing; task closure is ready for close-current-task.",
                ) + blocker_metadata.as_str(),
            ));
        }
        if required_follow_up == crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY
            && task_scope_structural_reason.is_none()
            && repair_plan.blocker_kind == Some(RepairBlockerKind::StaleUnreviewed)
        {
            let Some((task_number, step_number)) = explicit_execution_reentry_target(&repair_plan)
            else {
                let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
                return Ok(targetless_stale_reconcile_output(
                    snapshot,
                    stale_unreviewed_closures.clone(),
                    actions_performed,
                    final_routing.blocking_reason_codes.clone(),
                    blocker_metadata,
                    final_routing.recommended_public_command.clone(),
                ));
            };
            let final_routing = persist_execution_reentry_repair_target_and_refresh_routing(
                runtime,
                &status_args,
                &phase_bundle.read_scope.context,
                &phase_bundle.status,
                task_number,
                step_number,
            )?;
            let (reopen_command, reopen_command_argv, reopen_command_template, required_inputs) =
                execution_reentry_repair_surfaces(Some(&final_routing), task_number, step_number);
            return Ok(RepairReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures: stale_unreviewed_closures.clone(),
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed,
                required_follow_up: Some(String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
                )),
                next_action: None,
                recommended_command: reopen_command.clone(),
                recommended_public_command_argv: reopen_command_argv,
                recommended_public_command_template: reopen_command_template,
                required_inputs,
                trace_summary: repair_follow_up_trace_summary(
                    crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
                    branch_rerecording_unsupported_reason,
                    task_scope_structural_reason.as_deref(),
                    branch_scope_structural_reason.as_deref(),
                ) + blocker_metadata.as_str(),
                phase: Some(String::from(crate::execution::phase::PHASE_EXECUTING)),
                phase_detail: Some(String::from(
                    crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
                )),
                blocking_task: Some(task_number),
                blocking_step: Some(step_number),
                blocking_reason_codes: final_routing.blocking_reason_codes.clone(),
                authoritative_next_action: None,
            });
        }
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: stale_unreviewed_closures.clone(),
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: output_required_follow_up,
            next_action: None,
            recommended_command: output_recommended_command,
            recommended_public_command_argv: output_recommended_public_command_argv,
            recommended_public_command_template: output_recommended_public_command_template,
            required_inputs: output_required_inputs,
            trace_summary: repair_follow_up_trace_summary(
                public_required_follow_up,
                branch_rerecording_unsupported_reason,
                task_scope_structural_reason.as_deref(),
                branch_scope_structural_reason.as_deref(),
            ) + blocker_metadata.as_str(),
            phase: Some(final_routing.phase.clone()),
            phase_detail: Some(final_routing.phase_detail.clone()),
            blocking_task: final_routing.blocking_task,
            blocking_step: None,
            blocking_reason_codes: final_routing.blocking_reason_codes.clone(),
            authoritative_next_action: None,
        });
    }
    if route_action.kind == RepairRouteActionKind::CloseCurrentTask
        && route_action.phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && public_required_follow_up.as_deref()
            != Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY)
    {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        let trace_summary = String::from(
            "Repair review state reconciled stale task-boundary state and refreshed routing; task closure is ready for close-current-task.",
        ) + blocker_metadata.as_str();
        let Some(task_number) = route_action.blocking_task.or(route_action.task_number) else {
            return Ok(diagnostic_only_close_current_task_recovery_output(
                snapshot,
                stale_unreviewed_closures.clone(),
                actions_performed,
                &final_routing,
                None,
                trace_summary,
            ));
        };
        return Ok(repair_review_state_close_current_task_output(
            snapshot,
            stale_unreviewed_closures.clone(),
            actions_performed,
            &final_routing,
            task_number,
            trace_summary,
        ));
    }
    if let Some(required_follow_up) = public_required_follow_up {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        let recovery = public_recovery_contract_for_follow_up(
            &status_args.plan,
            Some(&final_routing),
            Some(required_follow_up.clone()),
        );
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: stale_unreviewed_closures.clone(),
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: recovery.required_follow_up,
            next_action: None,
            recommended_command: recovery.recommended_command,
            recommended_public_command_argv: recovery.recommended_public_command_argv,
            recommended_public_command_template: recovery.recommended_public_command_template,
            required_inputs: recovery.required_inputs,
            trace_summary: repair_follow_up_trace_summary(
                required_follow_up.as_str(),
                branch_rerecording_unsupported_reason,
                task_scope_structural_reason.as_deref(),
                branch_scope_structural_reason.as_deref(),
            ) + blocker_metadata.as_str(),
            phase: authoritative_phase,
            phase_detail: authoritative_phase_detail,
            blocking_task: route_action.blocking_task.or(route_action.task_number),
            blocking_step: route_action.step_number,
            blocking_reason_codes: route_action.blocking_reason_codes.clone(),
            authoritative_next_action: None,
        });
    }
    if route_action.kind == RepairRouteActionKind::RepairReviewState {
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        return Ok(repair_review_state_self_loop_blocked_output(
            RepairReviewStateSelfLoopBlock {
                snapshot,
                actions_performed,
                blocker_metadata,
                authoritative_phase,
                authoritative_phase_detail,
                blocking_task: route_action.blocking_task.or(route_action.task_number),
                blocking_step: route_action.step_number,
                blocking_reason_codes: route_action.blocking_reason_codes.clone(),
            },
        ));
    }
    if !stale_unreviewed_closures.is_empty()
        && repair_plan.blocker_kind == Some(RepairBlockerKind::StaleUnreviewed)
        && branch_rerecording_unsupported_reason.is_some()
    {
        let Some((task_number, step_number)) = explicit_execution_reentry_target(&repair_plan)
        else {
            let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
            return Ok(targetless_stale_reconcile_output(
                snapshot,
                stale_unreviewed_closures,
                actions_performed,
                route_action.blocking_reason_codes.clone(),
                blocker_metadata,
                route_action.recommended_public_command.clone(),
            ));
        };
        let final_routing = persist_execution_reentry_repair_target_and_refresh_routing(
            runtime,
            &status_args,
            &phase_bundle.read_scope.context,
            &phase_bundle.status,
            task_number,
            step_number,
        )?;
        let (reopen_command, reopen_command_argv, reopen_command_template, required_inputs) =
            execution_reentry_repair_surfaces(Some(&final_routing), task_number, step_number);
        let blocker_metadata = repair_blocker_metadata_suffix(&repair_plan);
        return Ok(RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up: Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
            )),
            next_action: None,
            recommended_command: reopen_command.clone(),
            recommended_public_command_argv: reopen_command_argv,
            recommended_public_command_template: reopen_command_template,
            required_inputs,
            trace_summary: repair_follow_up_trace_summary(
                crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
                branch_rerecording_unsupported_reason,
                task_scope_structural_reason.as_deref(),
                branch_scope_structural_reason.as_deref(),
            ) + blocker_metadata.as_str(),
            phase: Some(String::from(crate::execution::phase::PHASE_EXECUTING)),
            phase_detail: Some(String::from(
                crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
            )),
            blocking_task: Some(task_number),
            blocking_step: Some(step_number),
            blocking_reason_codes: final_routing.blocking_reason_codes.clone(),
            authoritative_next_action: None,
        });
    }

    let targetless_runtime_reconcile = authoritative_phase_detail
        .as_deref()
        .and_then(|phase_detail| {
            TargetlessStaleReconcile::from_phase_and_reason_codes(
                phase_detail,
                &route_action.blocking_reason_codes,
            )
        })
        .is_some();
    let diagnostic_route = targetless_runtime_reconcile
        || state_kind_or_phase_is_runtime_diagnostic(
            &route_decision.state_kind,
            &route_decision.phase_detail,
        );
    let diagnostic_next_action = diagnostic_next_action_for_route(
        &route_decision.state_kind,
        &route_decision.phase_detail,
        route_action.recommended_command_argv().is_some(),
        !route_action.required_inputs().is_empty(),
    );
    let blocking_reason_codes = if diagnostic_route {
        route_action.blocking_reason_codes.clone()
    } else {
        Vec::new()
    };
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
        next_action: diagnostic_next_action,
        recommended_command: route_action.recommended_command(),
        recommended_public_command_argv: route_action.recommended_command_argv(),
        recommended_public_command_template: route_action.recommended_command_template(),
        required_inputs: route_action.required_inputs(),
        trace_summary: if repaired_any_overlays {
            String::from(
                "Repaired missing derived review-state overlays from authoritative closure records.",
            )
        } else if targetless_runtime_reconcile {
            String::from(TARGETLESS_STALE_RECONCILE_DETAIL)
        } else if diagnostic_route {
            String::from(
                "Repair review state cannot mutate while the public runtime route is diagnostic-only.",
            )
        } else {
            snapshot.trace_summary
        },
        phase: authoritative_phase,
        phase_detail: authoritative_phase_detail,
        blocking_task: None,
        blocking_step: None,
        blocking_reason_codes,
        authoritative_next_action: None,
    })
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
    if clear_open_step_state_recording(runtime, context)? {
        actions_performed.push(String::from("cleared_current_open_step_state"));
    }
    Ok(())
}

fn clear_task_scope_state_for_structural_repair(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    phase_bundle: &RepairPhaseBundle,
    blocking_task: Option<u32>,
    clear_dispatch_lineage_for_structural_repair: bool,
    actions_performed: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let execution_reentry_targets = &phase_bundle.execution_reentry_targets;
    let mut structural_tasks = execution_reentry_targets.structural_tasks.clone();
    structural_tasks.sort_unstable();
    structural_tasks.dedup();
    let mut structural_scope_keys = execution_reentry_targets
        .structural_scope_keys
        .iter()
        .filter(|scope_key| task_scope_key_task_number(scope_key).is_some())
        .cloned()
        .collect::<Vec<_>>();
    let non_task_structural_scope_keys = execution_reentry_targets
        .structural_scope_keys
        .iter()
        .filter(|scope_key| task_scope_key_task_number(scope_key).is_none())
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

fn execute_repair_actions(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    plan: &RepairPlan,
    phase_bundle: &RepairPhaseBundle,
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
                    phase_bundle,
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
