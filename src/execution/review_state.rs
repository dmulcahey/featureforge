//! Review-state explain/reconcile adapters over execution-owned query and recording services.
//!
//! reconcile/explain commands stay thin over query and recording boundaries instead of
//! reaching into authoritative storage or rendered artifacts directly.

use serde::Serialize;

use crate::cli::plan_execution::StatusArgs;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::branch_closure_provenance::branch_closure_has_empty_lineage_late_stage_surface_exemption;
use crate::execution::closure_diagnostics::BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED;
use crate::execution::command_eligibility::{
    PublicCommand, PublicCommandInputRequirement, PublicCommandKind,
    public_command_recommendation_surfaces,
};
use crate::execution::current_closure_projection::structural_current_task_closure_failures_from_authoritative_state;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, BranchRerecordingUnsupportedReason,
    branch_closure_rerecording_assessment_with_authority, missing_derived_branch_scope_overlays,
    missing_derived_task_scope_overlays,
};
use crate::execution::follow_up::{
    FollowUpAliasContext, FollowUpKind, RepairFollowUpKind, RepairFollowUpRecord,
    execution_step_repair_target_id, normalize_follow_up_alias,
    repair_follow_up_source_decision_hash,
};
use crate::execution::next_action::{
    NEXT_ACTION_CLOSE_CURRENT_TASK, diagnostic_next_action_for_route,
};
use crate::execution::public_command_types::{
    RecommendedPublicCommandArgv, RecommendedPublicCommandTemplate,
};
use crate::execution::query::{
    ExecutionRoutingState, ReviewStateBranchClosure, ReviewStateSnapshot, ReviewStateTaskClosure,
    apply_read_surface_invariants_to_routing, query_review_state, required_follow_up_from_routing,
    review_state_snapshot_from_read_scope_with_status,
};
use crate::execution::recording::restore_review_state_projection_overlays;
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_DETAIL, TARGETLESS_STALE_RECONCILE_PHASE_DETAIL,
    TargetlessStaleReconcile,
};
use crate::execution::repair_route_decision::{
    RepairBlockerKind, RepairPlanFollowUpState, RepairPlanRequiredFollowUpInputs,
    RepairPlanTargetInputs, baseline_bridge_reducer_precedence,
    repair_plan_required_follow_up_decision, repair_plan_target_decision,
    task_closure_baseline_bridge_target_task_with_authority,
};
use crate::execution::route_plan::{
    RouteDecision, branch_closure_recording_route_decision, close_current_task_route_decision,
    required_follow_up_from_route_decision, state_kind_is_blocked_runtime_bug,
};
use crate::execution::stale_target_selection::select_branch_stale_source_task;
use crate::execution::state::{
    ExecutionContext, ExecutionReadScope, ExecutionReentryCurrentTaskClosureTargets,
    ExecutionRuntime, PlanExecutionStatus,
    apply_shared_routing_projection_to_read_scope_with_routing,
    branch_closure_record_matches_plan_exemption,
    current_branch_closure_structural_review_state_reason,
    execution_reentry_current_task_closure_targets_from_inputs, load_execution_read_scope,
    task_scope_structural_review_state_reason,
};
use crate::execution::status_support::{
    PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT,
    task_closure_baseline_repair_candidate_with_stale_target_and_authority,
};
use crate::execution::task_scope_key::task_scope_key_task_number;

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
    pub operator_requery_instruction: String,
    pub trace_summary: String,
}

fn reconcile_operator_rerun_instruction() -> String {
    format!("Re-query workflow operator JSON; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}.")
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    pub recommended_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: RecommendedPublicCommandArgv,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    pub trace_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_step: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authoritative_next_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepairAction {
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
pub(crate) struct RepairPlan {
    pub(crate) blocker_kind: Option<RepairBlockerKind>,
    pub(crate) target_task: Option<u32>,
    pub(crate) target_step: Option<u32>,
    pub(crate) actions_to_perform: Vec<RepairAction>,
    pub(crate) required_follow_up: Option<String>,
    pub(crate) post_repair_route_action: RepairRouteAction,
    pub(crate) post_repair_route_decision: RouteDecision,
}

impl RepairPlan {
    pub(crate) fn follow_up_state(&self) -> RepairPlanFollowUpState<'_> {
        RepairPlanFollowUpState {
            blocker_kind: self.blocker_kind,
            target_task: self.target_task,
            target_step: self.target_step,
            required_follow_up: self.required_follow_up.as_deref(),
            post_route_task: self.post_repair_route_action.task_number,
            post_route_blocking_task: self.post_repair_route_action.blocking_task,
        }
    }
}

struct RepairAnalysisInputs<'a> {
    snapshot: &'a ReviewStateSnapshot,
    post_repair_route_action: RepairRouteAction,
    post_repair_route_decision: &'a RouteDecision,
    task_closure_baseline_bridge_target: Option<u32>,
    task_closure_baseline_bridge_route_action: Option<RepairRouteAction>,
    closure_graph_stale_target: Option<u32>,
    branch_stale_source_task: Option<u32>,
    status_target_task: Option<u32>,
    task_scope_structural_blocking_record_present: bool,
    branch_rerecording_supported: bool,
    empty_lineage_branch_reroute_repairable: bool,
    task_closure_baseline_bridge_route_decision: Option<RouteDecision>,
    plan_complete: bool,
    execution_reentry_targets: &'a ExecutionReentryCurrentTaskClosureTargets,
    task_scope_structural_reason: Option<&'a str>,
    branch_scope_structural_reason: Option<&'a str>,
    unrecoverable_task_scope_task: Option<u32>,
    overlay_restore_available: bool,
    context: &'a ExecutionContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepairRouteActionKind {
    CloseCurrentTask,
    RepairReviewState,
    Other,
}

#[derive(Debug, Clone)]
pub(crate) struct RepairRouteAction {
    pub(crate) kind: RepairRouteActionKind,
    pub(crate) phase_detail: String,
    pub(crate) review_state_status: String,
    pub(crate) task_number: Option<u32>,
    pub(crate) step_number: Option<u32>,
    pub(crate) blocking_task: Option<u32>,
    pub(crate) blocking_reason_codes: Vec<String>,
    pub(crate) recommends_execution_reentry: bool,
    pub(crate) recommended_public_command: Option<PublicCommand>,
    pub(crate) recommended_command: Option<String>,
    pub(crate) recommended_public_command_argv: RecommendedPublicCommandArgv,
    pub(crate) recommended_public_command_template: RecommendedPublicCommandTemplate,
    pub(crate) required_inputs: Vec<PublicCommandInputRequirement>,
}

impl RepairRouteAction {
    pub(crate) fn recommended_command(&self) -> Option<String> {
        self.recommended_command.clone()
    }

    pub(crate) fn recommended_command_argv(&self) -> RecommendedPublicCommandArgv {
        self.recommended_public_command_argv.clone()
    }

    pub(crate) fn recommended_command_template(&self) -> RecommendedPublicCommandTemplate {
        self.recommended_public_command_template.clone()
    }

    pub(crate) fn required_inputs(&self) -> Vec<PublicCommandInputRequirement> {
        self.required_inputs.clone()
    }
}

fn public_recommendation_surfaces(
    command: Option<&PublicCommand>,
) -> (
    Option<String>,
    RecommendedPublicCommandArgv,
    RecommendedPublicCommandTemplate,
    Vec<PublicCommandInputRequirement>,
) {
    let (recommended_command, recommended_public_command_argv, template, required_inputs) =
        public_command_recommendation_surfaces(command);
    (
        recommended_command,
        recommended_public_command_argv,
        template,
        required_inputs,
    )
}

fn public_command_is_repair_review_state(command: Option<&PublicCommand>) -> bool {
    matches!(command, Some(PublicCommand::RepairReviewState { .. }))
}

fn public_command_is_execution_reentry(command: Option<&PublicCommand>) -> bool {
    matches!(
        command,
        Some(
            PublicCommand::Begin { .. }
                | PublicCommand::Complete { .. }
                | PublicCommand::Reopen { .. }
        )
    )
}

pub(crate) fn route_decision_surfaces(
    route_decision: &RouteDecision,
) -> (
    Option<String>,
    Option<Vec<String>>,
    RecommendedPublicCommandTemplate,
    Vec<PublicCommandInputRequirement>,
) {
    (
        RouteDecision::recommended_command_display(route_decision),
        route_decision.public_command_argv(),
        route_decision.public_command_template(),
        route_decision.required_inputs.clone(),
    )
}

struct FinalCloseCurrentTaskRoute {
    recommended_command: Option<String>,
    recommended_public_command_argv: RecommendedPublicCommandArgv,
    recommended_public_command_template: RecommendedPublicCommandTemplate,
    required_inputs: Vec<PublicCommandInputRequirement>,
    phase: String,
    phase_detail: String,
    blocking_task: Option<u32>,
    blocking_reason_codes: Vec<String>,
}

fn close_current_task_route_from_decision(
    route_decision: &RouteDecision,
    task_number: u32,
) -> Option<FinalCloseCurrentTaskRoute> {
    let routed_task = route_decision
        .recommended_public_command
        .as_ref()
        .and_then(PublicCommand::close_current_task_number)?;
    if routed_task != task_number {
        return None;
    }
    let (
        recommended_command,
        recommended_public_command_argv,
        recommended_public_command_template,
        required_inputs,
    ) = route_decision_surfaces(route_decision);
    Some(FinalCloseCurrentTaskRoute {
        recommended_command,
        recommended_public_command_argv,
        recommended_public_command_template,
        required_inputs,
        phase: route_decision.phase.clone(),
        phase_detail: route_decision.phase_detail.clone(),
        blocking_task: route_decision
            .blocking_task
            .or_else(|| {
                route_decision
                    .recording_context
                    .as_ref()
                    .and_then(|context| context.task_number)
            })
            .or(Some(task_number)),
        blocking_reason_codes: route_decision.blocking_reason_codes.clone(),
    })
}

fn final_close_current_task_route(
    final_routing: &ExecutionRoutingState,
    task_number: u32,
) -> Option<FinalCloseCurrentTaskRoute> {
    final_routing
        .route_decision
        .as_ref()
        .and_then(|decision| close_current_task_route_from_decision(decision, task_number))
}

pub(crate) fn diagnostic_only_close_current_task_recovery_output(
    snapshot: ReviewStateSnapshot,
    stale_unreviewed_closures: Vec<String>,
    actions_performed: Vec<String>,
    final_routing: &ExecutionRoutingState,
    task_number: Option<u32>,
    trace_summary: String,
) -> RepairReviewStateOutput {
    RepairReviewStateOutput {
        action: String::from("blocked"),
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        actions_performed,
        required_follow_up: None,
        next_action: None,
        recommended_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        trace_summary: format!(
            "{trace_summary} Route-owned close-current-task command was unavailable for the repaired target; rerun workflow operator JSON and stop if no typed public route is present."
        ),
        phase: Some(final_routing.phase.clone()),
        phase_detail: Some(final_routing.phase_detail.clone()),
        blocking_task: final_routing.blocking_task.or(task_number),
        blocking_step: None,
        blocking_reason_codes: final_routing.blocking_reason_codes.clone(),
        authoritative_next_action: None,
    }
}

pub(crate) fn repair_review_state_close_current_task_output(
    snapshot: ReviewStateSnapshot,
    stale_unreviewed_closures: Vec<String>,
    actions_performed: Vec<String>,
    final_routing: &ExecutionRoutingState,
    task_number: u32,
    trace_summary: String,
) -> RepairReviewStateOutput {
    let Some(close_route) = final_close_current_task_route(final_routing, task_number) else {
        return diagnostic_only_close_current_task_recovery_output(
            snapshot,
            stale_unreviewed_closures,
            actions_performed,
            final_routing,
            Some(task_number),
            trace_summary,
        );
    };
    RepairReviewStateOutput {
        action: String::from("blocked"),
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        actions_performed,
        required_follow_up: None,
        next_action: None,
        recommended_command: close_route.recommended_command,
        recommended_public_command_argv: close_route.recommended_public_command_argv,
        recommended_public_command_template: close_route.recommended_public_command_template,
        required_inputs: close_route.required_inputs,
        trace_summary,
        phase: Some(close_route.phase),
        phase_detail: Some(close_route.phase_detail),
        blocking_task: close_route.blocking_task,
        blocking_step: None,
        blocking_reason_codes: close_route.blocking_reason_codes,
        authoritative_next_action: None,
    }
}

fn repair_runtime_state<'a>(
    phase_bundle: &'a RepairPhaseBundle,
    action: &str,
) -> Result<&'a crate::execution::reducer::RuntimeState, JsonFailure> {
    phase_bundle
        .read_scope
        .runtime_state
        .as_ref()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!("{action} failed closed because reducer route state was unavailable."),
            )
        })
}

pub(crate) fn close_current_task_repair_route_decision(
    phase_bundle: &RepairPhaseBundle,
    task_number: u32,
) -> Result<RouteDecision, JsonFailure> {
    Ok(close_current_task_route_decision(
        repair_runtime_state(
            phase_bundle,
            PublicCommandKind::RepairReviewState.public_mutation_token(),
        )?,
        &phase_bundle.status,
        task_number,
    ))
}

pub(crate) fn branch_closure_repair_route_decision(
    phase_bundle: &RepairPhaseBundle,
) -> Result<RouteDecision, JsonFailure> {
    Ok(branch_closure_recording_route_decision(
        repair_runtime_state(
            phase_bundle,
            PublicCommandKind::RepairReviewState.public_mutation_token(),
        )?,
        &phase_bundle.status,
    ))
}

pub(crate) struct RepairPhaseBundle {
    pub(crate) read_scope: ExecutionReadScope,
    pub(crate) status: PlanExecutionStatus,
    pub(crate) route_decision: RouteDecision,
    pub(crate) snapshot: ReviewStateSnapshot,
    pub(crate) task_scope_structural_reason: Option<String>,
    pub(crate) branch_scope_structural_reason: Option<String>,
    pub(crate) execution_reentry_targets: ExecutionReentryCurrentTaskClosureTargets,
    pub(crate) unrecoverable_task_scope_task: Option<u32>,
    pub(crate) overlay_restore_available: bool,
    pub(crate) branch_rerecording_assessment: BranchRerecordingAssessment,
}

fn require_finalized_repair_route_decision(
    _read_scope: &ExecutionReadScope,
    routing: &ExecutionRoutingState,
    status: &PlanExecutionStatus,
) -> Result<RouteDecision, JsonFailure> {
    routing.route_decision.as_ref().cloned().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ResolverContractViolation,
            format!(
                "repair-review-state failed closed because shared runtime routing did not include a finalized route_decision; refusing to reconstruct route authority from presentation fields. state_kind={}; phase_detail={}; review_state_status={}; reason_codes=[{}]",
                status.state_kind,
                status.phase_detail,
                status.review_state_status,
                status.blocking_reason_codes.join(","),
            ),
        )
    })
}

pub(crate) struct RepairPlanAnalysis {
    pub(crate) repair_plan: RepairPlan,
    pub(crate) branch_rerecording_unsupported_reason: Option<BranchRerecordingUnsupportedReason>,
}

pub(crate) fn explicit_execution_reentry_target(repair_plan: &RepairPlan) -> Option<(u32, u32)> {
    repair_plan.target_task.zip(repair_plan.target_step)
}

fn post_repair_route_action_from_phase_bundle(
    phase_bundle: &RepairPhaseBundle,
) -> RepairRouteAction {
    repair_route_action_from_route_decision(&phase_bundle.route_decision, &phase_bundle.status)
}

fn repair_route_action_from_route_decision(
    route_decision: &RouteDecision,
    status: &PlanExecutionStatus,
) -> RepairRouteAction {
    let execution_task = route_decision
        .execution_command_context
        .as_ref()
        .and_then(|context| context.task_number);
    let execution_step = route_decision
        .execution_command_context
        .as_ref()
        .and_then(|context| context.step_id);
    let recording_task = route_decision
        .recording_context
        .as_ref()
        .and_then(|context| context.task_number);
    let blocking_task = recording_task
        .or(execution_task)
        .or(status.blocking_task)
        .or(status.resume_task)
        .or(status.active_task);
    let kind = if route_decision.phase_detail
        == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        || route_decision.next_action == NEXT_ACTION_CLOSE_CURRENT_TASK
    {
        RepairRouteActionKind::CloseCurrentTask
    } else if route_decision.required_follow_up.as_deref()
        == Some(crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE)
        || public_command_is_repair_review_state(route_decision.recommended_public_command.as_ref())
    {
        RepairRouteActionKind::RepairReviewState
    } else {
        RepairRouteActionKind::Other
    };
    RepairRouteAction {
        kind,
        phase_detail: route_decision.phase_detail.clone(),
        review_state_status: route_decision.review_state_status.clone(),
        task_number: recording_task.or(execution_task).or(blocking_task),
        step_number: execution_step.or(status.blocking_step),
        blocking_task,
        blocking_reason_codes: route_decision.blocking_reason_codes.clone(),
        recommends_execution_reentry: public_command_is_execution_reentry(
            route_decision.recommended_public_command.as_ref(),
        ),
        recommended_public_command: route_decision.recommended_public_command.clone(),
        recommended_command: RouteDecision::recommended_command_display(route_decision),
        recommended_public_command_argv: route_decision.public_command_argv(),
        recommended_public_command_template: route_decision.public_command_template(),
        required_inputs: route_decision.required_inputs.clone(),
    }
}

pub(crate) fn targetless_stale_reconcile_output(
    snapshot: ReviewStateSnapshot,
    stale_unreviewed_closures: Vec<String>,
    actions_performed: Vec<String>,
    blocking_reason_codes: Vec<String>,
    blocker_metadata: String,
    concrete_stale_target_repair_command: Option<PublicCommand>,
) -> RepairReviewStateOutput {
    if !stale_unreviewed_closures.is_empty() {
        let (
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
        ) = public_command_recommendation_surfaces(concrete_stale_target_repair_command.as_ref());
        let required_follow_up =
            if recommended_public_command_argv.is_some() || !required_inputs.is_empty() {
                Some(String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
                ))
            } else {
                None
            };
        return RepairReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed,
            required_follow_up,
            next_action: None,
            recommended_command: recommended_command.clone(),
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
            trace_summary: String::from(
                "Repair review state found a concrete stale branch or milestone target but no task reopen target; repair must continue without fabricating a current task closure target.",
            ) + blocker_metadata.as_str(),
            phase: Some(String::from(crate::execution::phase::PHASE_EXECUTING)),
            phase_detail: Some(String::from(
                crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
            )),
            blocking_task: None,
            blocking_step: None,
            blocking_reason_codes,
            authoritative_next_action: None,
        };
    }
    let mut blocking_reason_codes = blocking_reason_codes;
    TargetlessStaleReconcile::ensure_reason_codes(&mut blocking_reason_codes);
    RepairReviewStateOutput {
        action: String::from("blocked"),
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures,
        missing_derived_overlays: snapshot.missing_derived_overlays,
        actions_performed,
        required_follow_up: None,
        next_action: diagnostic_next_action_for_route(
            crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED,
            TARGETLESS_STALE_RECONCILE_PHASE_DETAIL,
            false,
            false,
        ),
        recommended_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        trace_summary: String::from(TARGETLESS_STALE_RECONCILE_DETAIL) + blocker_metadata.as_str(),
        phase: Some(String::from(crate::execution::phase::PHASE_EXECUTING)),
        phase_detail: Some(String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL)),
        blocking_task: None,
        blocking_step: None,
        blocking_reason_codes,
        authoritative_next_action: None,
    }
}

pub(crate) fn route_for_plan(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ExecutionRoutingState, JsonFailure> {
    let mut read_scope = load_execution_read_scope(runtime, &args.plan, true)?;
    let (mut routing, route_decision) = apply_shared_routing_projection_to_read_scope_with_routing(
        &mut read_scope,
        args.external_review_result_ready,
        false,
    )?;
    routing.route_decision = Some(route_decision);
    routing.execution_status = Some(read_scope.status.clone());
    apply_read_surface_invariants_to_routing(&mut routing);
    Ok(routing)
}

pub fn explain_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<ExplainReviewStateOutput, JsonFailure> {
    let snapshot = query_review_state(runtime, args)?;
    let (next_action, recommended_command, stale_unreviewed_closures) = match runtime.status(args) {
        Ok(status) => {
            let stale_unreviewed_closures = if status.stale_unreviewed_closures.is_empty() {
                snapshot.stale_unreviewed_closures.clone()
            } else {
                status.stale_unreviewed_closures
            };
            (
                status.next_action,
                status.recommended_command,
                stale_unreviewed_closures,
            )
        }
        Err(_) => (
            String::from("requery workflow operator"),
            Some(recommended_operator_command(
                args,
                args.external_review_result_ready,
            )),
            snapshot.stale_unreviewed_closures.clone(),
        ),
    };
    Ok(ExplainReviewStateOutput {
        current_task_closures: snapshot.current_task_closures,
        current_branch_closure: snapshot.current_branch_closure,
        superseded_closures: snapshot.superseded_closures,
        stale_unreviewed_closures,
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
    let branch_rerecording_assessment = branch_closure_rerecording_assessment_with_authority(
        &read_scope.context,
        read_scope.authoritative_state.as_ref(),
    )?;
    let context = read_scope.context;
    let status = runtime.status(args)?;
    if state_kind_is_blocked_runtime_bug(&status.state_kind) {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed: Vec::new(),
            operator_requery_instruction: reconcile_operator_rerun_instruction(),
            trace_summary: String::from(
                "Reconcile review state is blocked because invariant-protected public runtime status reported blocked_runtime_bug.",
            ),
        });
    }
    let branch_rerecording_supported = branch_rerecording_assessment.supported;
    let branch_rerecording_unsupported_reason = branch_rerecording_assessment.unsupported_reason;
    if let Some(reason_code) = task_scope_structural_review_state_reason(&status) {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: snapshot.current_task_closures,
            current_branch_closure: snapshot.current_branch_closure,
            superseded_closures: snapshot.superseded_closures,
            stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
            missing_derived_overlays: snapshot.missing_derived_overlays,
            actions_performed: Vec::new(),
            operator_requery_instruction: reconcile_operator_rerun_instruction(),
            trace_summary: match reason_code {
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID => String::from(
                    "Reconcile review state cannot repair structurally invalid current task-closure provenance; execution reentry is still required.",
                ),
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED => String::from(
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
            operator_requery_instruction: reconcile_operator_rerun_instruction(),
            trace_summary: if branch_rerecording_supported {
                match reason_code {
                    BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED => {
                        String::from(
                            "Reconcile review state cannot repair a malformed current branch-closure reviewed-state identity; run repair-review-state to establish the late-stage reroute before branch closure can be re-recorded.",
                        )
                    }
                    _ => String::from(
                        "Reconcile review state cannot repair the current branch-closure review-state blocker; run repair-review-state to establish the late-stage reroute before branch closure can be re-recorded.",
                    ),
                }
            } else {
                branch_rerecording_unavailable_trace(
                    branch_rerecording_unsupported_reason,
                    match reason_code {
                        BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED => {
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
    if snapshot.missing_derived_overlays.is_empty() && snapshot.stale_unreviewed_closures.is_empty()
    {
        let routing = route_for_plan(runtime, args).ok();
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
                operator_requery_instruction: reconcile_operator_rerun_instruction(),
                trace_summary: String::from(
                    "Reconcile review state cannot resolve this repair-state blocker; repair-review-state must rederive the exact execution reentry target.",
                ),
            });
        }
        if routing
            .as_ref()
            .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
        {
            return Ok(ReconcileReviewStateOutput {
                action: String::from("blocked"),
                current_task_closures: snapshot.current_task_closures,
                current_branch_closure: snapshot.current_branch_closure,
                superseded_closures: snapshot.superseded_closures,
                stale_unreviewed_closures: snapshot.stale_unreviewed_closures,
                missing_derived_overlays: snapshot.missing_derived_overlays,
                actions_performed: Vec::new(),
                operator_requery_instruction: reconcile_operator_rerun_instruction(),
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
            operator_requery_instruction: reconcile_operator_rerun_instruction(),
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
            operator_requery_instruction: reconcile_operator_rerun_instruction(),
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
            operator_requery_instruction: reconcile_operator_rerun_instruction(),
            trace_summary: String::from(
                "Reconcile review state could not derive the missing overlays from authoritative closure records.",
            ),
        });
    }
    let refreshed_routing = route_for_plan(runtime, args).ok();
    if refreshed_routing
        .as_ref()
        .is_some_and(|routing| late_stage_branch_closure_recording_required(routing, args))
    {
        return Ok(ReconcileReviewStateOutput {
            action: String::from("blocked"),
            current_task_closures: refreshed.current_task_closures,
            current_branch_closure: refreshed.current_branch_closure,
            superseded_closures: refreshed.superseded_closures,
            stale_unreviewed_closures: refreshed.stale_unreviewed_closures,
            missing_derived_overlays: refreshed.missing_derived_overlays,
            actions_performed,
            operator_requery_instruction: reconcile_operator_rerun_instruction(),
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
        operator_requery_instruction: reconcile_operator_rerun_instruction(),
        trace_summary: String::from(
            "Reconciled missing derived review-state overlays from authoritative closure records.",
        ),
    })
}

pub(crate) fn load_repair_phase_bundle(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairPhaseBundle, JsonFailure> {
    let mut read_scope = load_execution_read_scope(runtime, &args.plan, true)?;
    let (mut routing, route_decision) = apply_shared_routing_projection_to_read_scope_with_routing(
        &mut read_scope,
        args.external_review_result_ready,
        false,
    )?;
    routing.route_decision = Some(route_decision);
    routing.execution_status = Some(read_scope.status.clone());
    apply_read_surface_invariants_to_routing(&mut routing);
    if let Some(public_status) = routing.execution_status.clone() {
        read_scope.status = public_status;
    }
    let _reduced_status = routing.execution_status.as_ref().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "repair-review-state failed closed because router projection did not include reduced execution status.",
        )
    })?;
    let status = read_scope.status.clone();
    let route_decision = require_finalized_repair_route_decision(&read_scope, &routing, &status)?;
    let snapshot = review_state_snapshot_from_read_scope_with_status(&read_scope, &status)?;
    let task_scope_structural_reason =
        task_scope_structural_review_state_reason(&status).map(str::to_owned);
    let branch_scope_structural_reason =
        current_branch_closure_structural_review_state_reason(&status).map(str::to_owned);
    let reducer_stale_tasks = read_scope
        .runtime_state
        .as_ref()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "repair-review-state failed closed because reducer output was unavailable for stale repair targets.",
            )
        })?
        .gate_snapshot
        .task_stale_tasks();
    let structural_failures =
        read_scope
            .authoritative_state
            .as_ref()
            .map_or_else(Vec::new, |state| {
                structural_current_task_closure_failures_from_authoritative_state(
                    &read_scope.context,
                    state,
                )
            });
    let execution_reentry_targets = execution_reentry_current_task_closure_targets_from_inputs(
        reducer_stale_tasks,
        structural_failures,
    );
    let unrecoverable_task_scope_task =
        unrecoverable_task_scope_authority_loss_task_from_read_scope(&read_scope, &status)?;
    let branch_rerecording_assessment = branch_closure_rerecording_assessment_with_authority(
        &read_scope.context,
        read_scope.authoritative_state.as_ref(),
    )?;
    Ok(RepairPhaseBundle {
        overlay_restore_available: read_scope.authoritative_state.is_some(),
        read_scope,
        status,
        route_decision,
        snapshot,
        task_scope_structural_reason,
        branch_scope_structural_reason,
        execution_reentry_targets,
        unrecoverable_task_scope_task,
        branch_rerecording_assessment,
    })
}

fn task_scope_structural_blocking_record_present(status: &PlanExecutionStatus) -> bool {
    status.blocking_records.iter().any(|record| {
        matches!(
            record.code.as_str(),
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
        )
    })
}

pub(crate) fn analyze_repair_phase_bundle(
    phase_bundle: &RepairPhaseBundle,
    _status_args: &StatusArgs,
) -> Result<RepairPlanAnalysis, JsonFailure> {
    let branch_rerecording_assessment = &phase_bundle.branch_rerecording_assessment;
    let empty_lineage_branch_reroute_repairable = repair_can_establish_empty_lineage_branch_reroute(
        phase_bundle,
        branch_rerecording_assessment.unsupported_reason,
    );
    let plan_complete = phase_bundle
        .read_scope
        .context
        .steps
        .iter()
        .all(|step| step.checked);
    let reducer_stale_target_details =
        phase_bundle
            .read_scope
            .runtime_state
            .as_ref()
            .and_then(|runtime_state| {
                runtime_state
                    .gate_snapshot
                    .earliest_task_stale_target_details()
            });
    let reducer_stale_target = reducer_stale_target_details.and_then(|target| target.task);
    let reducer_stale_target_bridge_allowed =
        reducer_stale_target_details.is_none_or(|target| target.task_closure_bridge_allowed);
    let task_closure_baseline_bridge_target =
        task_closure_baseline_bridge_target_task_with_authority(
            &phase_bundle.read_scope.context,
            &phase_bundle.status,
            baseline_bridge_reducer_precedence(&phase_bundle.status, reducer_stale_target),
            reducer_stale_target_bridge_allowed,
            phase_bundle.read_scope.overlay.as_ref(),
            phase_bundle.read_scope.authoritative_state.as_ref(),
            branch_rerecording_assessment,
        )?;
    let task_closure_baseline_bridge_route_decision =
        if let Some(task_number) = task_closure_baseline_bridge_target {
            Some(close_current_task_repair_route_decision(
                phase_bundle,
                task_number,
            )?)
        } else {
            None
        };
    let task_closure_baseline_bridge_route_action = task_closure_baseline_bridge_route_decision
        .as_ref()
        .map(|route_decision| {
            repair_route_action_from_route_decision(route_decision, &phase_bundle.status)
        });
    let closure_graph_stale_target = reducer_stale_target;
    let branch_stale_source_task = branch_stale_source_task_from_snapshot(phase_bundle);
    let repair_plan = analyze_repair_plan(RepairAnalysisInputs {
        snapshot: &phase_bundle.snapshot,
        post_repair_route_action: post_repair_route_action_from_phase_bundle(phase_bundle),
        post_repair_route_decision: &phase_bundle.route_decision,
        task_closure_baseline_bridge_target,
        task_closure_baseline_bridge_route_action,
        closure_graph_stale_target,
        branch_stale_source_task,
        status_target_task: phase_bundle
            .status
            .blocking_task
            .or(phase_bundle.status.resume_task)
            .or(phase_bundle.status.active_task),
        task_scope_structural_blocking_record_present:
            task_scope_structural_blocking_record_present(&phase_bundle.status),
        branch_rerecording_supported: branch_rerecording_assessment.supported,
        empty_lineage_branch_reroute_repairable,
        task_closure_baseline_bridge_route_decision,
        plan_complete,
        execution_reentry_targets: &phase_bundle.execution_reentry_targets,
        task_scope_structural_reason: phase_bundle.task_scope_structural_reason.as_deref(),
        branch_scope_structural_reason: phase_bundle.branch_scope_structural_reason.as_deref(),
        unrecoverable_task_scope_task: phase_bundle.unrecoverable_task_scope_task,
        overlay_restore_available: phase_bundle.overlay_restore_available,
        context: &phase_bundle.read_scope.context,
    });
    Ok(RepairPlanAnalysis {
        repair_plan,
        branch_rerecording_unsupported_reason: branch_rerecording_assessment.unsupported_reason,
    })
}

fn branch_stale_source_task_from_snapshot(phase_bundle: &RepairPhaseBundle) -> Option<u32> {
    let authoritative_state = phase_bundle.read_scope.authoritative_state.as_ref()?;
    select_branch_stale_source_task(
        authoritative_state,
        &phase_bundle.snapshot.stale_unreviewed_closures,
    )
}

fn unrecoverable_task_scope_authority_loss_task_from_read_scope(
    read_scope: &ExecutionReadScope,
    status: &PlanExecutionStatus,
) -> Result<Option<u32>, JsonFailure> {
    let context = &read_scope.context;
    let Some(overlay) = read_scope.overlay.as_ref() else {
        return Ok(None);
    };
    if status.execution_started != "yes"
        || status.active_task.is_some()
        || status.resume_task.is_some()
    {
        return Ok(None);
    }
    let Some(authoritative_state) = read_scope.authoritative_state.as_ref() else {
        return Ok(None);
    };
    let branch_rerecording_assessment =
        branch_closure_rerecording_assessment_with_authority(context, Some(authoritative_state))?;
    let earliest_checked_dispatched_task = overlay
        .strategy_review_dispatch_lineage
        .iter()
        .filter_map(|(lineage_key, record)| {
            let task_number = task_scope_key_task_number(lineage_key).or(record.source_task)?;
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
        && !authoritative_state.task_closure_history_contains_task(task_number)
        && authoritative_state
            .task_closure_negative_result(task_number)
            .is_none()
        && task_closure_baseline_repair_candidate_with_stale_target_and_authority(
            context,
            status,
            task_number,
            read_scope
                .runtime_state
                .as_ref()
                .and_then(|runtime_state| runtime_state.gate_snapshot.earliest_task_stale_target()),
            Some(overlay),
            Some(authoritative_state),
            &branch_rerecording_assessment,
        )
        .ok()
        .flatten()
        .is_none()
    {
        return Ok(Some(task_number));
    }
    Ok(None)
}

pub(crate) fn repair_can_establish_empty_lineage_branch_reroute(
    phase_bundle: &RepairPhaseBundle,
    unsupported_reason: Option<BranchRerecordingUnsupportedReason>,
) -> bool {
    if unsupported_reason != Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) {
        return false;
    }
    let Some(branch_closure_id) = phase_bundle.status.current_branch_closure_id.as_deref() else {
        return false;
    };
    phase_bundle
        .read_scope
        .authoritative_state
        .as_ref()
        .and_then(|state| state.branch_closure_record(branch_closure_id))
        .is_some_and(|record| {
            branch_closure_has_empty_lineage_late_stage_surface_exemption(
                &record.provenance_basis,
                &record.source_task_closure_ids,
            ) && branch_closure_record_matches_plan_exemption(
                &phase_bundle.read_scope.context,
                &record,
            )
        })
}

fn analyze_repair_plan(inputs: RepairAnalysisInputs<'_>) -> RepairPlan {
    let shared_stale_unreviewed_execution_reentry =
        inputs.post_repair_route_action.review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            && inputs.post_repair_route_action.phase_detail
                == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED;
    let stale_unreviewed_execution_reentry_required = shared_stale_unreviewed_execution_reentry
        || !(inputs.snapshot.stale_unreviewed_closures.is_empty()
            || inputs.snapshot.branch_drift_confined_to_late_stage_surface
                && inputs.branch_rerecording_supported);
    let missing_derived_task_scope_repair_planned =
        !inputs.snapshot.missing_derived_overlays.is_empty()
            && missing_derived_task_scope_overlays(&inputs.snapshot.missing_derived_overlays);
    let missing_derived_branch_scope_repair_planned =
        !inputs.snapshot.missing_derived_overlays.is_empty()
            && missing_derived_branch_scope_overlays(&inputs.snapshot.missing_derived_overlays)
            && (!inputs.branch_rerecording_supported
                || inputs.snapshot.current_task_closures.is_empty());

    let structural_task_scope_detected = inputs.task_scope_structural_reason.is_some()
        || inputs.task_scope_structural_blocking_record_present
        || !inputs
            .execution_reentry_targets
            .structural_scope_keys
            .is_empty()
        || !inputs.execution_reentry_targets.structural_tasks.is_empty();
    let target_decision = repair_plan_target_decision(RepairPlanTargetInputs {
        context: inputs.context,
        post_repair_blocking_task: inputs.post_repair_route_action.blocking_task,
        post_repair_task_number: inputs.post_repair_route_action.task_number,
        post_repair_step_number: inputs.post_repair_route_action.step_number,
        post_repair_phase_detail: &inputs.post_repair_route_decision.phase_detail,
        post_repair_review_state_status: &inputs.post_repair_route_action.review_state_status,
        task_closure_baseline_bridge_target: inputs.task_closure_baseline_bridge_target,
        closure_graph_stale_target: inputs.closure_graph_stale_target,
        branch_stale_source_task: inputs.branch_stale_source_task,
        status_target_task: inputs.status_target_task,
        task_scope_structural_detected: structural_task_scope_detected,
        task_scope_structural_tasks: &inputs.execution_reentry_targets.structural_tasks,
        task_scope_structural_scope_keys: &inputs.execution_reentry_targets.structural_scope_keys,
        stale_tasks: &inputs.execution_reentry_targets.stale_tasks,
        unrecoverable_task_scope_task: inputs.unrecoverable_task_scope_task,
        stale_unreviewed_execution_reentry_required,
        missing_derived_task_scope_repair_planned,
        missing_derived_branch_scope_repair_planned,
        stale_unreviewed_closures_present: !inputs.snapshot.stale_unreviewed_closures.is_empty(),
        task_scope_structural_reason_present: inputs.task_scope_structural_reason.is_some(),
        branch_scope_structural_reason_present: inputs.branch_scope_structural_reason.is_some(),
        task_scope_structural_blocking_record_present: inputs
            .task_scope_structural_blocking_record_present,
        branch_rerecording_supported: inputs.branch_rerecording_supported,
        empty_lineage_branch_reroute_repairable: inputs.empty_lineage_branch_reroute_repairable,
        missing_derived_overlays_empty: inputs.snapshot.missing_derived_overlays.is_empty(),
    });
    let blocker_kind = target_decision.blocker_kind;
    let target_task = target_decision.target_task;
    let target_step = target_decision.target_step;
    let stale_unreviewed_branch_reroute_available =
        target_decision.stale_unreviewed_branch_reroute_available;

    let shared_required_follow_up =
        required_follow_up_from_route_decision(inputs.post_repair_route_decision);
    let stale_dispatch_lineage_blocking_task = (inputs.post_repair_route_decision.phase_detail
        == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && inputs
            .post_repair_route_action
            .blocking_reason_codes
            .iter()
            .any(|code| code == crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE)
        && shared_required_follow_up.as_deref() == Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY))
    .then(|| {
        inputs
            .post_repair_route_action
            .blocking_task
            .or(inputs.post_repair_route_action.task_number)
    })
    .flatten();
    let stale_dispatch_lineage_cleanup_for_shared_target = stale_dispatch_lineage_blocking_task
        .is_some_and(|task_number| target_task == Some(task_number));
    let exact_execution_reentry_already_routed = target_decision.exact_reducer_stale_reentry_target
        || inputs.post_repair_route_decision.phase_detail
            == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            && inputs
                .post_repair_route_decision
                .execution_command_context
                .is_some();
    let required_follow_up =
        repair_plan_required_follow_up_decision(RepairPlanRequiredFollowUpInputs {
            blocker_kind,
            shared_required_follow_up: shared_required_follow_up.as_deref(),
            stale_unreviewed_branch_reroute_available,
        });

    let mut actions_to_perform = Vec::new();
    let should_restore_projection_overlays = inputs.overlay_restore_available
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
        Some(RepairBlockerKind::TaskScopeStructural)
            if execution_reentry_target_task.is_some()
                || !inputs
                    .execution_reentry_targets
                    .structural_scope_keys
                    .is_empty()
                || !inputs.execution_reentry_targets.structural_tasks.is_empty() =>
        {
            actions_to_perform.push(RepairAction::StructuralTaskScope {
                blocking_task: execution_reentry_target_task,
                clear_dispatch_lineage_for_structural_repair:
                    stale_dispatch_lineage_cleanup_for_shared_target
                        && execution_reentry_target_task.is_some_and(|task_number| {
                            stale_dispatch_lineage_blocking_task == Some(task_number)
                        }),
            });
        }
        Some(RepairBlockerKind::UnrecoverableTaskScope)
            if required_follow_up.as_deref()
                == Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY)
                && execution_reentry_target_task.is_some() =>
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
            actions_to_perform.push(RepairAction::ReentryTask {
                blocking_task: execution_reentry_target_task,
            });
        }
        Some(RepairBlockerKind::StaleUnreviewed)
            if required_follow_up.as_deref()
                == Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY)
                && execution_reentry_target_task.is_some()
                && !exact_execution_reentry_already_routed
                && !preserve_task_scope_for_late_stage_branch_reroute =>
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
        Some(RepairBlockerKind::MissingDerivedTaskScope)
            if required_follow_up.as_deref()
                == Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY)
                && !defer_missing_derived_task_scope_cleanup
                && execution_reentry_target_task.is_some() =>
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
            actions_to_perform.push(RepairAction::ReentryTask {
                blocking_task: execution_reentry_target_task,
            });
        }
        Some(
            RepairBlockerKind::BranchScopeStructural | RepairBlockerKind::MissingDerivedBranchScope,
        ) => {
            actions_to_perform.push(RepairAction::ReentryBranch);
        }
        Some(RepairBlockerKind::TaskClosureBaselineBridge)
            if execution_reentry_target_task.is_some_and(|task_number| {
                !inputs.snapshot.stale_unreviewed_closures.is_empty()
                    || inputs
                        .snapshot
                        .current_task_closures
                        .iter()
                        .any(|closure| closure.task == task_number)
            }) =>
        {
            actions_to_perform.push(RepairAction::ReentryTask {
                blocking_task: execution_reentry_target_task,
            });
        }
        _ => {}
    }

    let post_repair_route_action = if matches!(
        blocker_kind,
        Some(RepairBlockerKind::TaskClosureBaselineBridge)
    ) {
        inputs
            .task_closure_baseline_bridge_route_action
            .clone()
            .unwrap_or(inputs.post_repair_route_action)
    } else {
        inputs.post_repair_route_action
    };
    let post_repair_route_decision = if matches!(
        blocker_kind,
        Some(RepairBlockerKind::TaskClosureBaselineBridge)
    ) {
        inputs
            .task_closure_baseline_bridge_route_decision
            .unwrap_or_else(|| inputs.post_repair_route_decision.clone())
    } else {
        inputs.post_repair_route_decision.clone()
    };

    RepairPlan {
        blocker_kind,
        target_task,
        target_step,
        actions_to_perform,
        required_follow_up,
        post_repair_route_action,
        post_repair_route_decision,
    }
}

pub(crate) fn execution_reentry_repair_surfaces(
    routing: Option<&ExecutionRoutingState>,
    task_number: u32,
    step_number: u32,
) -> (
    Option<String>,
    Option<Vec<String>>,
    RecommendedPublicCommandTemplate,
    Vec<PublicCommandInputRequirement>,
) {
    let command = routing.and_then(|routing| {
        routed_execution_reentry_public_command_for_target(routing, task_number, step_number)
    });
    public_recommendation_surfaces(command.as_ref())
}

fn routed_execution_reentry_public_command_for_target(
    routing: &ExecutionRoutingState,
    task_number: u32,
    step_number: u32,
) -> Option<PublicCommand> {
    let command = routing
        .route_decision
        .as_ref()
        .and_then(|decision| decision.recommended_public_command.as_ref())
        .or(routing.recommended_public_command.as_ref())?;
    let request = command.to_mutation_request()?;
    if matches!(
        request.kind,
        PublicCommandKind::Begin | PublicCommandKind::Reopen
    ) && request.task == Some(task_number)
        && request.step == Some(step_number)
    {
        Some(command.clone())
    } else {
        None
    }
}

pub(crate) fn target_bound_repair_follow_up_record(
    kind: RepairFollowUpKind,
    phase_bundle: &RepairPhaseBundle,
    stale_reentry_repair_plan: &RepairPlan,
    repair_plan: &RepairPlan,
    route_decision: &RouteDecision,
    target_task: Option<u32>,
    target_step: Option<u32>,
) -> RepairFollowUpRecord {
    let target_scope = kind.target_scope();
    let target_record_id = repair_follow_up_target_record_id(
        kind,
        target_task,
        phase_bundle,
        stale_reentry_repair_plan,
        repair_plan,
    );
    let semantic_workspace_state_id = phase_bundle
        .read_scope
        .runtime_state
        .as_ref()
        .map(|state| state.semantic_workspace.semantic_workspace_tree_id.clone())
        .or_else(|| {
            crate::execution::semantic_identity::semantic_workspace_snapshot(
                &phase_bundle.read_scope.context,
            )
            .ok()
            .map(|snapshot| snapshot.semantic_workspace_tree_id)
        });
    let created_sequence = phase_bundle
        .read_scope
        .authoritative_state
        .as_ref()
        .map_or(1, |state| {
            state.latest_authoritative_sequence().saturating_add(1)
        });
    RepairFollowUpRecord {
        kind,
        target_scope,
        target_task,
        target_step,
        target_record_id,
        semantic_workspace_state_id,
        source_route_decision_hash: repair_follow_up_source_decision_hash(route_decision),
        created_sequence,
        created_at: Some(jiff::Timestamp::now().to_string()),
        expires_on_plan_fingerprint_change: true,
    }
}

fn repair_follow_up_target_record_id(
    kind: RepairFollowUpKind,
    target_task: Option<u32>,
    phase_bundle: &RepairPhaseBundle,
    stale_reentry_repair_plan: &RepairPlan,
    repair_plan: &RepairPlan,
) -> Option<String> {
    match kind {
        RepairFollowUpKind::RecordBranchClosure
        | RepairFollowUpKind::AdvanceLateStage
        | RepairFollowUpKind::ResolveReleaseBlocker => branch_follow_up_target_record_id(
            phase_bundle.snapshot.current_branch_closure.as_ref(),
            phase_bundle.status.current_branch_closure_id.as_deref(),
        ),
        RepairFollowUpKind::ExecutionReentry => target_task
            .and_then(|task| {
                stale_reentry_repair_plan
                    .target_step
                    .or(repair_plan.target_step)
                    .map(|step| (task, step))
            })
            .or_else(|| {
                phase_bundle
                    .status
                    .public_repair_targets
                    .iter()
                    .find_map(|target| {
                        (PublicCommandKind::Reopen
                            .matches_public_mutation_token(&target.command_kind)
                            && target.task == target_task)
                            .then_some((target.task?, target.step?))
                    })
            })
            .map(|(task, step)| execution_step_repair_target_id(task, step))
            .or_else(|| target_task.map(|task| format!("task-{task}"))),
        RepairFollowUpKind::CloseTask => target_task
            .and_then(|task| {
                phase_bundle
                    .snapshot
                    .current_task_closures
                    .iter()
                    .find(|closure| closure.task == task)
                    .map(|closure| closure.closure_record_id.clone())
            })
            .or_else(|| {
                phase_bundle
                    .status
                    .public_repair_targets
                    .iter()
                    .find_map(|target| {
                        (target.task == target_task)
                            .then(|| target.source_record_id.clone())
                            .flatten()
                    })
            })
            .or_else(|| {
                stale_reentry_repair_plan
                    .target_task
                    .or(repair_plan.target_task)
                    .map(|task| format!("task-{task}"))
            }),
        RepairFollowUpKind::RecordFinalReview
        | RepairFollowUpKind::RequestExternalReview
        | RepairFollowUpKind::WaitForExternalReviewResult
        | RepairFollowUpKind::GateReview
        | RepairFollowUpKind::GateFinish => phase_bundle
            .read_scope
            .authoritative_state
            .as_ref()
            .and_then(|state| state.current_final_review_record_id()),
        RepairFollowUpKind::RecordQa | RepairFollowUpKind::RunVerification => phase_bundle
            .read_scope
            .authoritative_state
            .as_ref()
            .and_then(|state| state.current_qa_record_id()),
        RepairFollowUpKind::RepairReviewState | RepairFollowUpKind::RecordHandoff => None,
    }
}

fn branch_follow_up_target_record_id(
    current_branch_closure: Option<&ReviewStateBranchClosure>,
    status_current_branch_closure_id: Option<&str>,
) -> Option<String> {
    current_branch_closure
        .map(|closure| closure.branch_closure_id.clone())
        .or_else(|| status_current_branch_closure_id.map(ToOwned::to_owned))
}

fn late_stage_branch_closure_recording_required(
    routing: &ExecutionRoutingState,
    _args: &StatusArgs,
) -> bool {
    routing.review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && (routing.phase_detail == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
            || routing_projects_review_state_execution_reentry(routing))
}

fn routing_projects_review_state_execution_reentry(routing: &ExecutionRoutingState) -> bool {
    routing.phase == crate::execution::phase::PHASE_EXECUTING
        && routing.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && required_follow_up_from_routing(routing).as_deref()
            == Some(crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE)
}

pub(crate) fn repair_follow_up_trace_summary(
    required_follow_up: &str,
    branch_rerecording_unsupported_reason: Option<BranchRerecordingUnsupportedReason>,
    task_scope_structural_reason: Option<&str>,
    branch_scope_structural_reason: Option<&str>,
) -> String {
    match normalize_follow_up_alias(
        Some(required_follow_up),
        FollowUpAliasContext::PublicRouting,
    ) {
        Some(FollowUpKind::AdvanceLateStage) => String::from(
            "Repair review state reconciled projections and refreshed routing; branch closure must be re-recorded before late-stage progression can continue.",
        ),
        Some(FollowUpKind::ExecutionReentry) => {
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
        Some(FollowUpKind::RequestExternalReview) => String::from(
            "Repair review state reconciled projections and refreshed routing; an external review dispatch is the next required step.",
        ),
        Some(FollowUpKind::ResolveReleaseBlocker) => String::from(
            "Repair review state reconciled projections and refreshed routing; release blockers must be resolved before late-stage progression can continue.",
        ),
        Some(FollowUpKind::RecordHandoff) => String::from(
            "Repair review state reconciled projections and refreshed routing; follow the public transfer route before continuing.",
        ),
        Some(FollowUpKind::RepairReviewState) => String::from(
            "Repair review state reconciled projections and refreshed routing; planning reentry is required before continuing.",
        ),
        Some(
            FollowUpKind::CloseCurrentTask
            | FollowUpKind::WaitForExternalReviewResult
            | FollowUpKind::RunVerification
            | FollowUpKind::GateReview
            | FollowUpKind::GateFinish,
        )
        | None => {
            format!(
                "Repair review state reconciled projections and refreshed routing; required follow-up is {required_follow_up}."
            )
        }
    }
}

pub(crate) fn repair_blocker_metadata_suffix(plan: &RepairPlan) -> String {
    let Some(blocker_kind) = plan.blocker_kind else {
        return String::new();
    };
    let blocker = match blocker_kind {
        RepairBlockerKind::TaskScopeStructural => "task_scope_structural",
        RepairBlockerKind::UnrecoverableTaskScope => "unrecoverable_task_scope",
        RepairBlockerKind::TaskClosureBaselineBridge => "task_closure_baseline_bridge",
        RepairBlockerKind::StaleUnreviewed => {
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        }
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
    if plan
        .post_repair_route_action
        .recommended_command_argv()
        .is_some()
    {
        metadata.push_str(", typed_public_argv_available=true");
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

fn recommended_operator_command(args: &StatusArgs, external_review_result_ready: bool) -> String {
    PublicCommand::WorkflowOperator {
        plan: args.plan.display().to_string(),
        external_review_result_ready,
        json: true,
    }
    .to_display_command()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::OnceLock;

    use crate::contracts::plan::{PlanDocument, PlanStep, PlanTask, TaskFileEntry};
    use crate::contracts::workflow::WorkflowRoute;
    use crate::execution::command_eligibility::PublicCommandInvocation;
    use crate::execution::context::EvidenceSourceOrigin;
    use crate::execution::public_recovery::public_recovery_contract_for_follow_up;
    use crate::execution::state::{
        EvidenceAttempt, EvidenceFormat, ExecutionEvidence, PlanStepState,
    };

    fn empty_review_state_snapshot() -> ReviewStateSnapshot {
        ReviewStateSnapshot {
            current_task_closures: Vec::new(),
            current_branch_closure: None,
            superseded_closures: Vec::new(),
            stale_unreviewed_closures: Vec::new(),
            missing_derived_overlays: Vec::new(),
            branch_drift_confined_to_late_stage_surface: false,
            trace_summary: String::new(),
        }
    }

    fn test_runtime(root: &Path) -> ExecutionRuntime {
        ExecutionRuntime {
            repo_root: root.to_path_buf(),
            git_dir: root.join(".git"),
            branch_name: String::from("feature/test"),
            repo_slug: String::from("featureforge"),
            safe_branch: String::from("feature-test"),
            state_dir: root.join("state"),
        }
    }

    fn test_task(number: u32, step_number: u32) -> PlanTask {
        PlanTask {
            number,
            title: format!("Task {number}"),
            spec_coverage: vec![String::from("DR-TEST")],
            goal: String::from("Exercise repair routing behavior."),
            context: Vec::new(),
            constraints: Vec::new(),
            done_when: Vec::new(),
            files: vec![TaskFileEntry {
                action: String::from("Modify"),
                path: format!("src/task-{number}.rs"),
            }],
            steps: vec![PlanStep {
                number: step_number,
                text: format!("Step {step_number}"),
            }],
        }
    }

    fn test_context(root: &Path) -> ExecutionContext {
        let task = test_task(1, 3);
        let tasks_by_number = [(1, task.clone())].into_iter().collect::<BTreeMap<_, _>>();
        ExecutionContext {
            runtime: test_runtime(root),
            plan_rel: String::from("docs/featureforge/plans/plan.md"),
            plan_abs: root.join("docs/featureforge/plans/plan.md"),
            plan_document: PlanDocument {
                path: String::from("docs/featureforge/plans/plan.md"),
                workflow_state: String::from("Engineering Approved"),
                plan_revision: 1,
                execution_mode: String::from("featureforge:executing-plans"),
                source_spec_path: String::from("docs/featureforge/specs/spec.md"),
                source_spec_revision: 1,
                last_reviewed_by: String::from("plan-eng-review"),
                qa_requirement: None,
                coverage_matrix: BTreeMap::new(),
                tasks: vec![task],
                source: String::new(),
            },
            plan_source: String::new(),
            steps: vec![PlanStepState {
                task_number: 1,
                step_number: 3,
                title: String::from("Step 3"),
                checked: true,
                note_state: None,
                note_summary: String::new(),
            }],
            local_execution_progress_markers_present: false,
            legacy_open_step_projection_present: false,
            tasks_by_number,
            evidence_rel: String::from("docs/featureforge/execution-evidence/plan-r1-evidence.md"),
            evidence_abs: root.join("docs/featureforge/execution-evidence/plan-r1-evidence.md"),
            evidence: ExecutionEvidence {
                format: EvidenceFormat::Empty,
                plan_path: String::from("docs/featureforge/plans/plan.md"),
                plan_revision: 1,
                plan_fingerprint: None,
                source_spec_path: String::from("docs/featureforge/specs/spec.md"),
                source_spec_revision: 1,
                source_spec_fingerprint: None,
                attempts: vec![EvidenceAttempt {
                    task_number: 1,
                    step_number: 3,
                    attempt_number: 1,
                    status: String::from("Completed"),
                    recorded_at: String::from("2026-05-04T00:00:00Z"),
                    execution_source: String::from("featureforge:executing-plans"),
                    claim: String::from("Task 1 Step 3 was attempted."),
                    files: Vec::new(),
                    file_proofs: Vec::new(),
                    verify_command: None,
                    verification_summary: String::from("verified"),
                    invalidation_reason: String::new(),
                    packet_fingerprint: None,
                    head_sha: None,
                    base_sha: None,
                    source_contract_path: None,
                    source_contract_fingerprint: None,
                    source_evaluation_report_fingerprint: None,
                    evaluator_verdict: None,
                    failing_criterion_ids: Vec::new(),
                    source_handoff_fingerprint: None,
                    repo_state_baseline_head_sha: None,
                    repo_state_baseline_worktree_fingerprint: None,
                }],
                source: None,
                source_origin: EvidenceSourceOrigin::Empty,
                tracked_progress_present: false,
                tracked_source: None,
            },
            authoritative_evidence_projection_fingerprint: None,
            source_spec_source: String::new(),
            source_spec_path: root.join("docs/featureforge/specs/spec.md"),
            execution_fingerprint: String::from("fingerprint"),
            tracked_tree_sha_cache: OnceLock::new(),
            semantic_workspace_snapshot_cache: OnceLock::new(),
            reviewed_tree_sha_cache: RefCell::new(BTreeMap::new()),
            head_sha_cache: OnceLock::new(),
            release_base_branch_cache: OnceLock::new(),
            tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
        }
    }

    fn repair_review_state_route_action() -> RepairRouteAction {
        let command = PublicCommand::RepairReviewState {
            plan: String::from("docs/featureforge/plans/plan.md"),
        };
        RepairRouteAction {
            kind: RepairRouteActionKind::RepairReviewState,
            phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED),
            task_number: None,
            step_number: None,
            blocking_task: None,
            blocking_reason_codes: vec![
                String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE),
                String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED),
            ],
            recommends_execution_reentry: false,
            recommended_public_command: Some(command.clone()),
            recommended_command: Some(command.to_display_command()),
            recommended_public_command_argv: Some(command.to_argv()),
            recommended_public_command_template: command.to_input_template(),
            required_inputs: Vec::new(),
        }
    }

    fn repair_review_state_route_decision() -> RouteDecision {
        let command = PublicCommand::RepairReviewState {
            plan: String::from("docs/featureforge/plans/plan.md"),
        };
        RouteDecision {
            state_kind: String::from("actionable_public_command"),
            phase: String::from(crate::execution::phase::PHASE_EXECUTING),
            phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED),
            next_action: String::from(
                crate::execution::next_action::NEXT_ACTION_REPAIR_REVIEW_STATE,
            ),
            blocking_reason_codes: vec![
                String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE),
                String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED),
            ],
            blocking_scope: Some(String::from("task")),
            blocking_task: None,
            external_wait_state: None,
            recommended_command: Some(command.to_display_command()),
            recommended_public_command: Some(command.clone()),
            invocation: Some(PublicCommandInvocation {
                argv: command.to_argv(),
            }),
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            required_follow_up: Some(String::from(crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE)),
            next_public_action: None,
            blockers: Vec::new(),
            public_repair_targets: Vec::new(),
            execution_reentry_target_source: None,
            execution_command_context: None,
            recording_context: None,
        }
    }

    fn assert_repair_output_recommendation_has_argv(output: &RepairReviewStateOutput) {
        if output.recommended_command.is_some() {
            assert!(
                output.recommended_public_command_argv.is_some(),
                "repair-review-state output must not expose display-only command text: {output:?}"
            );
        }
    }

    fn test_routing_state_with_public_command(command: PublicCommand) -> ExecutionRoutingState {
        let plan = match &command {
            PublicCommand::WorkflowOperator { plan, .. }
            | PublicCommand::Status { plan }
            | PublicCommand::Begin { plan, .. }
            | PublicCommand::Complete { plan, .. }
            | PublicCommand::Reopen { plan, .. }
            | PublicCommand::CloseCurrentTask { plan, .. }
            | PublicCommand::RepairReviewState { plan }
            | PublicCommand::AdvanceLateStage { plan, .. }
            | PublicCommand::TransferHandoff { plan, .. }
            | PublicCommand::TransferRepairStep { plan, .. }
            | PublicCommand::MaterializeProjectionsStateDirOnly { plan, .. } => plan.clone(),
        };
        ExecutionRoutingState {
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from(crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
                next_skill: String::from("featureforge:executing-plans"),
                spec_path: String::from("docs/featureforge/specs/spec.md"),
                plan_path: plan,
                contract_state: String::from("clean"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            route_decision: None,
            runtime_provenance: None,
            execution_status: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
            phase: String::from(crate::execution::phase::PHASE_EXECUTING),
            phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            ),
            qa_requirement: None,
            finish_review_gate_pass_branch_closure_id: None,
            recording_context: None,
            execution_command_context: None,
            next_action: String::from(
                crate::execution::next_action::NEXT_ACTION_REPAIR_REVIEW_STATE,
            ),
            recommended_public_command: Some(command.clone()),
            recommended_command: Some(command.to_display_command()),
            blocking_scope: Some(String::from("task")),
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            current_branch_closure_id: None,
            current_release_readiness_result: None,
            base_branch: None,
        }
    }

    #[test]
    fn mismatched_repair_review_state_follow_up_does_not_fallback_to_prior_command_surfaces() {
        let plan = String::from("docs/featureforge/plans/plan with spaces.md");
        let fallback = PublicCommand::Begin {
            plan: plan.clone(),
            task: 2,
            step: 1,
            execution_mode: Some(String::from("featureforge:executing-plans")),
            fingerprint: Some(String::from("fingerprint-2")),
        };
        let final_routing_command = PublicCommand::RepairReviewState { plan: plan.clone() };
        let final_routing = test_routing_state_with_public_command(final_routing_command);

        let recovery = public_recovery_contract_for_follow_up(
            Path::new(&plan),
            Some(&final_routing),
            Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
            )),
        );
        let (
            fallback_recommended_command,
            fallback_recommended_public_command_argv,
            fallback_recommended_public_command_template,
            fallback_required_inputs,
        ) = public_recommendation_surfaces(Some(&fallback));

        assert!(
            fallback_recommended_command.is_some()
                && fallback_recommended_public_command_argv.is_some()
                && fallback_recommended_public_command_template.is_none()
                && fallback_required_inputs.is_empty(),
            "test must seed an executable fallback command that old repair-review-state output code would have leaked"
        );
        assert!(recovery.required_follow_up.is_none());
        assert!(recovery.recommended_command.is_none());
        assert!(recovery.recommended_public_command_argv.is_none());
        assert!(recovery.recommended_public_command_template.is_none());
        assert!(recovery.required_inputs.is_empty());
        assert!(
            fallback_recommended_public_command_argv
                .as_ref()
                .is_some_and(|argv| argv.iter().any(|part| part == &plan)),
            "argv must preserve the plan path as one argument"
        );
    }

    #[test]
    fn targetless_stale_reconcile_output_derives_recommendation_surfaces_from_public_command() {
        let plan = String::from("docs/featureforge/plans/plan with spaces.md");
        let command = PublicCommand::Reopen {
            plan: plan.clone(),
            task: 2,
            step: 1,
            source: Some(String::from("featureforge:executing-plans")),
            reason: Some(String::from(
                "stale branch target requires execution reentry",
            )),
            fingerprint: Some(String::from("fingerprint-1")),
        };
        let expected_display = command.to_display_command();
        let expected_argv = command.to_argv();

        let output = targetless_stale_reconcile_output(
            empty_review_state_snapshot(),
            vec![String::from("branch:stale")],
            Vec::new(),
            vec![String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            )],
            String::new(),
            Some(command),
        );

        assert_eq!(
            output.required_follow_up.as_deref(),
            Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY)
        );
        assert_eq!(output.recommended_command, Some(expected_display));
        assert_eq!(output.authoritative_next_action, None);
        assert_eq!(
            output.recommended_public_command_argv,
            Some(expected_argv.clone())
        );
        assert_repair_output_recommendation_has_argv(&output);
        assert!(
            expected_argv.iter().any(|part| part == &plan),
            "argv must preserve the plan path as one argument"
        );
    }

    #[test]
    fn targetless_stale_reconcile_output_omits_recommendation_surfaces_without_public_command() {
        let output = targetless_stale_reconcile_output(
            empty_review_state_snapshot(),
            vec![String::from("branch:stale")],
            Vec::new(),
            vec![String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            )],
            String::new(),
            None,
        );

        assert_eq!(output.recommended_command, None);
        assert_eq!(output.recommended_public_command_argv, None);
        assert_eq!(output.authoritative_next_action, None);
        assert_repair_output_recommendation_has_argv(&output);
    }

    #[test]
    fn unrecoverable_task_scope_stale_repair_requires_execution_reentry_not_branch_reroute() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let context = test_context(temp.path());
        let mut snapshot = empty_review_state_snapshot();
        snapshot.branch_drift_confined_to_late_stage_surface = true;
        let execution_reentry_targets = ExecutionReentryCurrentTaskClosureTargets {
            stale_tasks: Vec::new(),
            structural_tasks: Vec::new(),
            structural_scope_keys: Vec::new(),
        };
        let route_decision = repair_review_state_route_decision();

        let repair_plan = analyze_repair_plan(RepairAnalysisInputs {
            snapshot: &snapshot,
            post_repair_route_action: repair_review_state_route_action(),
            post_repair_route_decision: &route_decision,
            task_closure_baseline_bridge_target: None,
            task_closure_baseline_bridge_route_action: None,
            closure_graph_stale_target: None,
            branch_stale_source_task: None,
            status_target_task: None,
            task_scope_structural_blocking_record_present: false,
            branch_rerecording_supported: true,
            empty_lineage_branch_reroute_repairable: false,
            task_closure_baseline_bridge_route_decision: None,
            plan_complete: true,
            execution_reentry_targets: &execution_reentry_targets,
            task_scope_structural_reason: None,
            branch_scope_structural_reason: None,
            unrecoverable_task_scope_task: Some(1),
            overlay_restore_available: false,
            context: &context,
        });

        assert_eq!(
            repair_plan.blocker_kind,
            Some(RepairBlockerKind::UnrecoverableTaskScope)
        );
        assert_eq!(repair_plan.target_task, Some(1));
        assert_eq!(repair_plan.target_step, Some(3));
        assert_eq!(
            repair_plan.required_follow_up.as_deref(),
            Some(crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY),
            "task-scope stale repair must not be promoted into branch late-stage reroute"
        );
        assert!(
            repair_plan.actions_to_perform.iter().any(|action| {
                matches!(
                    action,
                    RepairAction::ReentryTask {
                        blocking_task: Some(1)
                    }
                )
            }),
            "task-scope stale repair should clear the stale task for execution reentry: {repair_plan:?}"
        );
    }

    #[test]
    fn task_closure_baseline_bridge_preempts_exact_stale_reentry_target() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let context = test_context(temp.path());
        let mut snapshot = empty_review_state_snapshot();
        snapshot.current_task_closures = vec![ReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-1-current"),
            reviewed_state_id: String::from("reviewed-state-1"),
            contract_identity: String::from("docs/featureforge/plans/plan.md#task-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];
        snapshot
            .stale_unreviewed_closures
            .push(String::from("task-1-stale"));
        let mut route_action = repair_review_state_route_action();
        route_action.task_number = Some(1);
        route_action.step_number = Some(3);
        route_action.blocking_task = Some(1);
        route_action.recommends_execution_reentry = true;
        let mut route_decision = repair_review_state_route_decision();
        route_decision.blocking_task = Some(1);
        let execution_reentry_targets = ExecutionReentryCurrentTaskClosureTargets {
            stale_tasks: vec![1],
            structural_tasks: Vec::new(),
            structural_scope_keys: Vec::new(),
        };

        let repair_plan = analyze_repair_plan(RepairAnalysisInputs {
            snapshot: &snapshot,
            post_repair_route_action: route_action,
            post_repair_route_decision: &route_decision,
            task_closure_baseline_bridge_target: Some(1),
            task_closure_baseline_bridge_route_action: None,
            closure_graph_stale_target: Some(1),
            branch_stale_source_task: None,
            status_target_task: Some(1),
            task_scope_structural_blocking_record_present: false,
            branch_rerecording_supported: false,
            empty_lineage_branch_reroute_repairable: false,
            task_closure_baseline_bridge_route_decision: None,
            plan_complete: false,
            execution_reentry_targets: &execution_reentry_targets,
            task_scope_structural_reason: None,
            branch_scope_structural_reason: None,
            unrecoverable_task_scope_task: None,
            overlay_restore_available: false,
            context: &context,
        });

        assert_eq!(
            repair_plan.blocker_kind,
            Some(RepairBlockerKind::TaskClosureBaselineBridge),
            "a computed close-current-task baseline bridge must not be downgraded into executable stale reentry"
        );
        assert_eq!(repair_plan.target_task, Some(1));
        assert_eq!(repair_plan.required_follow_up, None);
    }

    #[test]
    fn branch_follow_up_target_record_id_uses_only_branch_closure_truth() {
        let branch_closure = ReviewStateBranchClosure {
            branch_closure_id: String::from("branch-closure-current"),
            reviewed_state_id: None,
            contract_identity: None,
        };
        assert_eq!(
            branch_follow_up_target_record_id(None, None),
            None,
            "branch follow-up target ids must come from branch closure truth, not task stale-closure ids"
        );
        assert_eq!(
            branch_follow_up_target_record_id(Some(&branch_closure), None),
            Some(String::from("branch-closure-current"))
        );
        assert_eq!(
            branch_follow_up_target_record_id(None, Some("branch-closure-status")),
            Some(String::from("branch-closure-status"))
        );
    }
}
