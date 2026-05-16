//! Shared repair-route classification and follow-up selection.
//!
//! This module owns repair/reentry route decisions that are consumed by the
//! read model, router, next-action projection, and repair mutation path. Child
//! modules may own focused route families, but presentation and mutation
//! surfaces should consume these decisions instead of rebuilding status and
//! reason-code predicates locally.

mod baseline_bridge;

pub(crate) use baseline_bridge::{
    ExecutionReentryTaskClosureBridgeInputs, baseline_bridge_reducer_precedence,
    execution_reentry_task_closure_bridge_route_task, status_has_current_task_closure_for_task,
    task_closure_baseline_bridge_blocking_task_route_task,
    task_closure_baseline_bridge_candidate_route_task,
    task_closure_baseline_bridge_external_review_route_task,
    task_closure_baseline_bridge_late_stage_missing_current_closure_route_task,
    task_closure_baseline_bridge_missing_baseline_unsupported_route_task,
    task_closure_baseline_bridge_open_step_preempted_by_closure_recording,
    task_closure_baseline_bridge_persisted_close_current_task_route_task,
    task_closure_baseline_bridge_ready_for_task, task_closure_baseline_bridge_reentry_target,
    task_closure_baseline_bridge_repair_review_state_route,
    task_closure_baseline_bridge_route_ready_for_status, task_closure_baseline_bridge_route_task,
    task_closure_baseline_bridge_stale_boundary_route_ready,
    task_closure_baseline_bridge_target_task_with_authority,
    task_closure_baseline_bridge_task_review_pending_route_task,
    task_closure_baseline_bridge_task_review_result_ready_promotes_recording,
};

use crate::execution::current_truth::{BranchRerecordingAssessment, ReviewStateRepairReroute};
use crate::execution::follow_up::{RepairFollowUpKind, normalize_public_routing_follow_up_token};
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::phase;
use crate::execution::repair_target_selection::{
    ExecutionReentryTarget, NextActionAuthorityInputs, execution_reentry_target,
    select_authoritative_stale_reentry_target,
};
use crate::execution::review_route_tokens::{
    FOLLOW_UP_ADVANCE_LATE_STAGE, FOLLOW_UP_EXECUTION_REENTRY, FOLLOW_UP_REPAIR_REVIEW_STATE,
    REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::route_plan::state_kind_is_terminal;
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, RuntimeGateSnapshot, StaleTargetProjection,
};
use crate::execution::stale_target_selection::{
    select_first_task_number, select_first_task_number_from_scope_keys,
    select_repair_plan_stale_target_task,
};
use crate::execution::state::{
    ExecutionContext, GateResult, PlanExecutionStatus, PublicRepairTarget,
    latest_attempted_step_for_task,
};
use crate::execution::transitions::AuthoritativeTransitionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepairBlockerKind {
    TaskScopeStructural,
    UnrecoverableTaskScope,
    TaskClosureBaselineBridge,
    StaleUnreviewed,
    MissingDerivedTaskScope,
    BranchScopeStructural,
    MissingDerivedBranchScope,
}

pub(crate) struct RepairPlanTargetInputs<'a> {
    pub(crate) context: &'a ExecutionContext,
    pub(crate) post_repair_blocking_task: Option<u32>,
    pub(crate) post_repair_task_number: Option<u32>,
    pub(crate) post_repair_step_number: Option<u32>,
    pub(crate) post_repair_phase_detail: &'a str,
    pub(crate) post_repair_review_state_status: &'a str,
    pub(crate) task_closure_baseline_bridge_target: Option<u32>,
    pub(crate) closure_graph_stale_target: Option<u32>,
    pub(crate) branch_stale_source_task: Option<u32>,
    pub(crate) status_target_task: Option<u32>,
    pub(crate) task_scope_structural_detected: bool,
    pub(crate) task_scope_structural_tasks: &'a [u32],
    pub(crate) task_scope_structural_scope_keys: &'a [String],
    pub(crate) stale_tasks: &'a [u32],
    pub(crate) unrecoverable_task_scope_task: Option<u32>,
    pub(crate) stale_unreviewed_execution_reentry_required: bool,
    pub(crate) missing_derived_task_scope_repair_planned: bool,
    pub(crate) missing_derived_branch_scope_repair_planned: bool,
    pub(crate) stale_unreviewed_closures_present: bool,
    pub(crate) task_scope_structural_reason_present: bool,
    pub(crate) branch_scope_structural_reason_present: bool,
    pub(crate) task_scope_structural_blocking_record_present: bool,
    pub(crate) branch_rerecording_supported: bool,
    pub(crate) empty_lineage_branch_reroute_repairable: bool,
    pub(crate) missing_derived_overlays_empty: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct NextActionAuthorityReadScope<'a> {
    pub(crate) overlay: Option<&'a StatusAuthoritativeOverlay>,
    pub(crate) authoritative_state: Option<&'a AuthoritativeTransitionState>,
    pub(crate) persisted_repair_follow_up: Option<&'a str>,
    pub(crate) branch_rerecording_assessment: Option<&'a BranchRerecordingAssessment>,
    pub(crate) gate_finish: Option<&'a GateResult>,
    pub(crate) route_repair_target_candidates: &'a [PublicRepairTarget],
}

impl<'a> NextActionAuthorityReadScope<'a> {
    fn with_gate_finish(self, gate_finish: Option<&'a GateResult>) -> Self {
        Self {
            gate_finish,
            ..self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepairPlanTargetDecision {
    pub(crate) blocker_kind: Option<RepairBlockerKind>,
    pub(crate) target_task: Option<u32>,
    pub(crate) target_step: Option<u32>,
    pub(crate) stale_unreviewed_branch_reroute_available: bool,
    pub(crate) exact_reducer_stale_reentry_target: bool,
}

pub(crate) fn repair_plan_target_decision(
    inputs: RepairPlanTargetInputs<'_>,
) -> RepairPlanTargetDecision {
    let shared_target_task = inputs
        .post_repair_blocking_task
        .or(inputs.post_repair_task_number);
    let shared_target_step = inputs.post_repair_step_number;
    let structural_target_task = shared_target_task
        .or(inputs.status_target_task)
        .or_else(|| select_first_task_number(inputs.task_scope_structural_tasks))
        .or_else(|| {
            select_first_task_number_from_scope_keys(inputs.task_scope_structural_scope_keys)
        });
    let stale_target_task = select_repair_plan_stale_target_task(
        inputs.stale_tasks,
        inputs.closure_graph_stale_target,
        inputs.branch_stale_source_task,
    );
    let exact_reducer_stale_reentry_target = inputs
        .closure_graph_stale_target
        .is_some_and(|stale_task| shared_target_task == Some(stale_task))
        && shared_target_step.is_some()
        && inputs.post_repair_phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED;
    let reducer_stale_reentry_target = inputs.closure_graph_stale_target.is_some()
        && inputs.post_repair_phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED;
    let stale_boundary_preempts_structural = inputs.stale_unreviewed_execution_reentry_required
        && inputs.task_scope_structural_detected
        && stale_target_task.is_some_and(|stale_task| {
            structural_target_task.is_none_or(|structural_task| stale_task <= structural_task)
        });
    let blocker_kind = if stale_boundary_preempts_structural {
        Some(RepairBlockerKind::StaleUnreviewed)
    } else if inputs.task_scope_structural_detected {
        Some(RepairBlockerKind::TaskScopeStructural)
    } else if inputs.unrecoverable_task_scope_task.is_some() {
        Some(RepairBlockerKind::UnrecoverableTaskScope)
    } else if inputs.task_closure_baseline_bridge_target.is_some() {
        Some(RepairBlockerKind::TaskClosureBaselineBridge)
    } else if exact_reducer_stale_reentry_target
        || reducer_stale_reentry_target
        || inputs.stale_unreviewed_execution_reentry_required
    {
        Some(RepairBlockerKind::StaleUnreviewed)
    } else if inputs.missing_derived_task_scope_repair_planned {
        Some(RepairBlockerKind::MissingDerivedTaskScope)
    } else if inputs.branch_scope_structural_reason_present {
        Some(RepairBlockerKind::BranchScopeStructural)
    } else if inputs.missing_derived_branch_scope_repair_planned {
        Some(RepairBlockerKind::MissingDerivedBranchScope)
    } else {
        None
    };

    let mut target_task = target_task_for_blocker(
        blocker_kind,
        shared_target_task,
        inputs.status_target_task,
        inputs.task_scope_structural_tasks,
        inputs.task_scope_structural_scope_keys,
        inputs.stale_tasks,
        inputs.unrecoverable_task_scope_task,
    );
    if matches!(
        blocker_kind,
        Some(RepairBlockerKind::TaskClosureBaselineBridge)
    ) {
        target_task = inputs.task_closure_baseline_bridge_target.or(target_task);
    }
    if matches!(blocker_kind, Some(RepairBlockerKind::StaleUnreviewed)) && target_task.is_none() {
        target_task = select_repair_plan_stale_target_task(
            inputs.stale_tasks,
            inputs.closure_graph_stale_target,
            inputs.branch_stale_source_task,
        );
    }

    let stale_unreviewed_status_present =
        inputs.post_repair_review_state_status == REVIEW_STATE_STALE_UNREVIEWED;
    let task_scope_stale_target_present =
        inputs.closure_graph_stale_target.is_some() || !inputs.stale_tasks.is_empty();
    let task_scope_repair_blocker = matches!(
        blocker_kind,
        Some(
            RepairBlockerKind::TaskScopeStructural
                | RepairBlockerKind::UnrecoverableTaskScope
                | RepairBlockerKind::TaskClosureBaselineBridge
                | RepairBlockerKind::MissingDerivedTaskScope
        )
    );
    let stale_unreviewed_branch_reroute_available = (inputs.stale_unreviewed_closures_present
        || stale_unreviewed_status_present)
        && (inputs.branch_rerecording_supported || inputs.empty_lineage_branch_reroute_repairable)
        && !task_scope_stale_target_present
        && !task_scope_repair_blocker
        && inputs.status_target_task.is_none()
        && !inputs.task_scope_structural_reason_present
        && !inputs.task_scope_structural_blocking_record_present
        && !inputs.branch_scope_structural_reason_present
        && inputs.missing_derived_overlays_empty;
    if stale_unreviewed_branch_reroute_available
        && matches!(blocker_kind, Some(RepairBlockerKind::StaleUnreviewed))
    {
        target_task = None;
    }

    RepairPlanTargetDecision {
        blocker_kind,
        target_task,
        target_step: repair_target_step(
            inputs.context,
            target_task,
            shared_target_task,
            shared_target_step,
        ),
        stale_unreviewed_branch_reroute_available,
        exact_reducer_stale_reentry_target,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepairPlanRequiredFollowUpInputs<'a> {
    pub(crate) blocker_kind: Option<RepairBlockerKind>,
    pub(crate) shared_required_follow_up: Option<&'a str>,
    pub(crate) stale_unreviewed_branch_reroute_available: bool,
}

pub(crate) fn repair_plan_required_follow_up_decision(
    inputs: RepairPlanRequiredFollowUpInputs<'_>,
) -> Option<String> {
    if matches!(
        inputs.blocker_kind,
        Some(RepairBlockerKind::TaskClosureBaselineBridge)
    ) {
        return None;
    }
    if inputs.stale_unreviewed_branch_reroute_available {
        return Some(String::from(FOLLOW_UP_ADVANCE_LATE_STAGE));
    }
    let required_follow_up = inputs.shared_required_follow_up?;
    if required_follow_up == FOLLOW_UP_REPAIR_REVIEW_STATE
        && repair_review_state_follow_up_routes_to_execution_reentry(
            inputs.blocker_kind,
            inputs.stale_unreviewed_branch_reroute_available,
        )
    {
        return Some(String::from(FOLLOW_UP_EXECUTION_REENTRY));
    }
    Some(required_follow_up.to_owned())
}

fn repair_review_state_follow_up_routes_to_execution_reentry(
    blocker_kind: Option<RepairBlockerKind>,
    stale_unreviewed_branch_reroute_available: bool,
) -> bool {
    matches!(
        blocker_kind,
        Some(
            RepairBlockerKind::TaskScopeStructural
                | RepairBlockerKind::UnrecoverableTaskScope
                | RepairBlockerKind::MissingDerivedTaskScope
        )
    ) || matches!(blocker_kind, Some(RepairBlockerKind::StaleUnreviewed))
        && !stale_unreviewed_branch_reroute_available
}

fn repair_target_step(
    context: &ExecutionContext,
    target_task: Option<u32>,
    shared_target_task: Option<u32>,
    shared_target_step: Option<u32>,
) -> Option<u32> {
    let task = target_task?;
    if shared_target_task == Some(task) {
        return shared_target_step.or_else(|| latest_attempted_step_for_task(context, task));
    }
    latest_attempted_step_for_task(context, task)
}

fn target_task_for_blocker(
    blocker_kind: Option<RepairBlockerKind>,
    shared_target_task: Option<u32>,
    status_target_task: Option<u32>,
    structural_tasks: &[u32],
    structural_scope_keys: &[String],
    stale_tasks: &[u32],
    unrecoverable_task_scope_task: Option<u32>,
) -> Option<u32> {
    match blocker_kind {
        Some(RepairBlockerKind::TaskScopeStructural) => shared_target_task
            .or(status_target_task)
            .or_else(|| select_first_task_number(structural_tasks))
            .or_else(|| select_first_task_number_from_scope_keys(structural_scope_keys)),
        Some(RepairBlockerKind::UnrecoverableTaskScope) => unrecoverable_task_scope_task
            .or(status_target_task)
            .or(shared_target_task),
        Some(RepairBlockerKind::TaskClosureBaselineBridge) => shared_target_task
            .or(status_target_task)
            .or_else(|| select_first_task_number(stale_tasks)),
        Some(RepairBlockerKind::StaleUnreviewed) => select_first_task_number(stale_tasks)
            .or(status_target_task)
            .or(shared_target_task),
        Some(RepairBlockerKind::MissingDerivedTaskScope) => select_first_task_number(stale_tasks)
            .or_else(|| select_first_task_number(structural_tasks))
            .or_else(|| select_first_task_number_from_scope_keys(structural_scope_keys))
            .or(unrecoverable_task_scope_task)
            .or(status_target_task)
            .or(shared_target_task),
        Some(
            RepairBlockerKind::BranchScopeStructural | RepairBlockerKind::MissingDerivedBranchScope,
        ) => shared_target_task,
        None => shared_target_task,
    }
}

pub(crate) fn next_action_authority_inputs_from_gate_snapshot<'a>(
    status: &PlanExecutionStatus,
    gate_snapshot: &'a RuntimeGateSnapshot,
    read_scope: NextActionAuthorityReadScope<'a>,
) -> NextActionAuthorityInputs<'a> {
    next_action_authority_inputs_from_stale_targets(
        status,
        &gate_snapshot.stale_targets,
        gate_snapshot.has_authoritative_stale_binding(status),
        read_scope.with_gate_finish(gate_snapshot.gate_finish.as_ref()),
    )
}

pub(crate) fn next_action_authority_inputs_from_stale_projection<'a>(
    status: &PlanExecutionStatus,
    stale_projection: &'a StaleTargetProjection,
    read_scope: NextActionAuthorityReadScope<'a>,
) -> NextActionAuthorityInputs<'a> {
    next_action_authority_inputs_from_stale_targets(
        status,
        &stale_projection.stale_targets,
        stale_projection.has_authoritative_stale_binding(status),
        read_scope,
    )
}

pub(crate) fn next_action_authority_inputs_from_stale_targets<'a>(
    status: &PlanExecutionStatus,
    stale_targets: &'a [AuthoritativeStaleTarget],
    has_authoritative_stale_target: bool,
    read_scope: NextActionAuthorityReadScope<'a>,
) -> NextActionAuthorityInputs<'a> {
    NextActionAuthorityInputs {
        overlay: read_scope.overlay,
        authoritative_state: read_scope.authoritative_state,
        persisted_repair_follow_up: read_scope.persisted_repair_follow_up,
        branch_rerecording_assessment: read_scope.branch_rerecording_assessment,
        gate_finish: read_scope.gate_finish,
        route_repair_target_candidates: read_scope.route_repair_target_candidates,
        has_authoritative_stale_target,
        authoritative_stale_target: select_authoritative_stale_reentry_target(
            status,
            stale_targets,
        ),
        ..NextActionAuthorityInputs::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepairFollowUpDecision {
    pub(crate) execution_reentry_target: Option<ExecutionReentryTarget>,
    pub(crate) repair_reroute: ReviewStateRepairReroute,
}

impl RepairFollowUpDecision {
    pub(crate) fn requires_execution_reentry(&self) -> bool {
        self.repair_reroute == ReviewStateRepairReroute::ExecutionReentry
            && self.execution_reentry_target.is_some()
    }

    pub(crate) fn requires_planning_reentry(&self) -> bool {
        self.repair_reroute == ReviewStateRepairReroute::ExecutionReentry
            && self.execution_reentry_target.is_none()
    }
}

pub(crate) fn repair_follow_up_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
    repair_reroute: ReviewStateRepairReroute,
) -> RepairFollowUpDecision {
    RepairFollowUpDecision {
        execution_reentry_target: execution_reentry_target(
            context,
            status,
            plan_path,
            authority_inputs,
        ),
        repair_reroute,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepairPlanFollowUpState<'a> {
    pub(crate) blocker_kind: Option<RepairBlockerKind>,
    pub(crate) target_task: Option<u32>,
    pub(crate) target_step: Option<u32>,
    pub(crate) required_follow_up: Option<&'a str>,
    pub(crate) post_route_task: Option<u32>,
    pub(crate) post_route_blocking_task: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PostRepairRouteFollowUpState<'a> {
    pub(crate) state_kind: &'a str,
    pub(crate) phase_detail: &'a str,
    pub(crate) review_state_status: &'a str,
    pub(crate) required_follow_up: Option<&'a str>,
    pub(crate) blocking_reason_codes: &'a [String],
    pub(crate) recommends_execution_reentry: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepairReviewStateFollowUpInputs<'a> {
    pub(crate) repair_plan: RepairPlanFollowUpState<'a>,
    pub(crate) stale_reentry_repair_plan: RepairPlanFollowUpState<'a>,
    pub(crate) route: PostRepairRouteFollowUpState<'a>,
    pub(crate) performed_current_task_closure_cleanup: bool,
    pub(crate) persisted_close_task_follow_up_target: Option<u32>,
    pub(crate) cleared_current_branch_closure: bool,
    pub(crate) current_task_closures_empty: bool,
    pub(crate) stale_unreviewed_closures_empty: bool,
    pub(crate) task_scope_structural_reason_present: bool,
    pub(crate) branch_scope_structural_reason_present: bool,
    pub(crate) post_repair_status_current_task_closures_empty: bool,
    pub(crate) branch_rerecording_supported: bool,
    pub(crate) empty_lineage_branch_reroute_repairable: bool,
    pub(crate) original_empty_lineage_branch_reroute_repairable: bool,
    pub(crate) missing_derived_overlays_empty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RepairFollowUpTarget {
    pub(crate) task: Option<u32>,
    pub(crate) step: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepairReviewStateFollowUpDecision {
    pub(crate) required_follow_up: Option<String>,
    pub(crate) public_required_follow_up: Option<String>,
    pub(crate) persisted_required_follow_up: Option<RepairFollowUpKind>,
    pub(crate) target: RepairFollowUpTarget,
    pub(crate) task_closure_repair_ready_for_recording: bool,
    pub(crate) task_closure_repair_target_task: Option<u32>,
    pub(crate) current_route_requires_no_repair_follow_up: bool,
}

pub(crate) fn repair_review_state_follow_up_decision(
    inputs: RepairReviewStateFollowUpInputs<'_>,
) -> RepairReviewStateFollowUpDecision {
    let routed_task_closure_repair_target_task = inputs
        .repair_plan
        .target_task
        .or(inputs.stale_reentry_repair_plan.target_task)
        .or(inputs.repair_plan.post_route_blocking_task)
        .or(inputs.repair_plan.post_route_task);
    let task_closure_repair_target_task =
        routed_task_closure_repair_target_task.or(inputs.persisted_close_task_follow_up_target);
    let task_closure_cleanup_promotes_recording = inputs.performed_current_task_closure_cleanup
        && !inputs.cleared_current_branch_closure
        && inputs.current_task_closures_empty
        && inputs.stale_unreviewed_closures_empty
        && !inputs.task_scope_structural_reason_present
        && !inputs.branch_scope_structural_reason_present
        && task_closure_repair_target_task.is_some()
        && inputs.post_repair_status_current_task_closures_empty;
    let persisted_close_task_follow_up_promotes_recording =
        inputs.persisted_close_task_follow_up_target.is_some()
            && !inputs.cleared_current_branch_closure
            && inputs.current_task_closures_empty
            && inputs.stale_unreviewed_closures_empty
            && !inputs.task_scope_structural_reason_present
            && !inputs.branch_scope_structural_reason_present
            && inputs.post_repair_status_current_task_closures_empty;
    let task_closure_repair_ready_for_recording = task_closure_cleanup_promotes_recording
        || persisted_close_task_follow_up_promotes_recording;

    let mut required_follow_up = inputs
        .repair_plan
        .required_follow_up
        .map(str::to_owned)
        .or_else(|| inputs.route.required_follow_up.map(str::to_owned));
    if required_follow_up.is_none()
        && inputs.stale_reentry_repair_plan.blocker_kind == Some(RepairBlockerKind::StaleUnreviewed)
        && inputs.stale_reentry_repair_plan.required_follow_up == Some(FOLLOW_UP_EXECUTION_REENTRY)
        && !task_closure_cleanup_promotes_recording
        && !persisted_close_task_follow_up_promotes_recording
    {
        required_follow_up = Some(String::from(FOLLOW_UP_EXECUTION_REENTRY));
    }
    if required_follow_up.as_deref() == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        && matches!(
            inputs.repair_plan.blocker_kind,
            Some(
                RepairBlockerKind::TaskScopeStructural
                    | RepairBlockerKind::UnrecoverableTaskScope
                    | RepairBlockerKind::MissingDerivedTaskScope
                    | RepairBlockerKind::StaleUnreviewed
            )
        )
        && inputs.route.recommends_execution_reentry
    {
        required_follow_up = Some(String::from(FOLLOW_UP_EXECUTION_REENTRY));
    }

    let persist_branch_reroute_follow_up = ((!inputs.stale_unreviewed_closures_empty
        && inputs.branch_rerecording_supported
        && !inputs.cleared_current_branch_closure)
        || inputs.empty_lineage_branch_reroute_repairable
        || inputs.original_empty_lineage_branch_reroute_repairable)
        && !inputs.task_scope_structural_reason_present
        && !inputs.branch_scope_structural_reason_present
        && inputs.missing_derived_overlays_empty;
    let task_closure_recording_follow_up_ready = task_closure_cleanup_promotes_recording
        || (required_follow_up.as_deref() == Some(FOLLOW_UP_EXECUTION_REENTRY)
            && !inputs.task_scope_structural_reason_present
            && !inputs.branch_scope_structural_reason_present
            && inputs
                .route
                .blocking_reason_codes
                .iter()
                .any(|code| code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING));
    let branch_rerecording_follow_up_ready = required_follow_up.as_deref()
        == Some(FOLLOW_UP_ADVANCE_LATE_STAGE)
        && inputs.branch_rerecording_supported
        && !inputs.task_scope_structural_reason_present
        && !inputs.branch_scope_structural_reason_present
        && !inputs.cleared_current_branch_closure;
    let current_route_requires_no_repair_follow_up =
        state_kind_is_terminal(inputs.route.state_kind)
            && inputs.route.phase_detail == phase::DETAIL_FINISH_COMPLETION_GATE_READY
            && inputs.route.review_state_status == "clean";
    let persisted_required_follow_up = if current_route_requires_no_repair_follow_up {
        None
    } else if persist_branch_reroute_follow_up || branch_rerecording_follow_up_ready {
        Some(RepairFollowUpKind::RecordBranchClosure)
    } else if task_closure_recording_follow_up_ready {
        Some(RepairFollowUpKind::CloseTask)
    } else if inputs.route.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && inputs
            .stale_reentry_repair_plan
            .target_task
            .or(inputs.repair_plan.target_task)
            .is_some()
    {
        Some(RepairFollowUpKind::ExecutionReentry)
    } else {
        required_follow_up
            .as_deref()
            .and_then(RepairFollowUpKind::from_persisted_token)
    };
    let public_required_follow_up = required_follow_up
        .as_deref()
        .and_then(|follow_up| normalize_public_routing_follow_up_token(Some(follow_up)))
        .map(str::to_owned);
    let target = repair_follow_up_target_binding(
        persisted_required_follow_up,
        inputs.stale_reentry_repair_plan,
        inputs.repair_plan,
    );
    RepairReviewStateFollowUpDecision {
        required_follow_up,
        public_required_follow_up,
        persisted_required_follow_up,
        target,
        task_closure_repair_ready_for_recording,
        task_closure_repair_target_task,
        current_route_requires_no_repair_follow_up,
    }
}

pub(crate) fn repair_follow_up_target_binding(
    persisted_follow_up: Option<RepairFollowUpKind>,
    stale_reentry_repair_plan: RepairPlanFollowUpState<'_>,
    repair_plan: RepairPlanFollowUpState<'_>,
) -> RepairFollowUpTarget {
    match persisted_follow_up {
        Some(RepairFollowUpKind::ExecutionReentry) => {
            execution_reentry_repair_follow_up_target(stale_reentry_repair_plan, repair_plan)
        }
        Some(RepairFollowUpKind::CloseTask) => RepairFollowUpTarget {
            task: close_task_repair_follow_up_target(stale_reentry_repair_plan, repair_plan),
            step: None,
        },
        _ => RepairFollowUpTarget {
            task: None,
            step: None,
        },
    }
}

fn execution_reentry_repair_follow_up_target(
    stale_reentry_repair_plan: RepairPlanFollowUpState<'_>,
    repair_plan: RepairPlanFollowUpState<'_>,
) -> RepairFollowUpTarget {
    if let Some(task) = stale_reentry_repair_plan.target_task {
        return RepairFollowUpTarget {
            task: Some(task),
            step: stale_reentry_repair_plan.target_step,
        };
    }
    RepairFollowUpTarget {
        task: repair_plan.target_task,
        step: repair_plan.target_step,
    }
}

fn close_task_repair_follow_up_target(
    stale_reentry_repair_plan: RepairPlanFollowUpState<'_>,
    repair_plan: RepairPlanFollowUpState<'_>,
) -> Option<u32> {
    repair_plan
        .target_task
        .or(repair_plan.post_route_task)
        .or(repair_plan.post_route_blocking_task)
        .or(stale_reentry_repair_plan.target_task)
}

pub(crate) fn repair_review_state_final_required_follow_up(
    routed_required_follow_up: Option<&str>,
    repair_public_required_follow_up: Option<&str>,
) -> Option<String> {
    let routed_follow_up = routed_required_follow_up
        .and_then(|follow_up| normalize_public_routing_follow_up_token(Some(follow_up)))
        .map(str::to_owned);
    if routed_follow_up.as_deref() == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        && repair_public_required_follow_up != Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
    {
        repair_public_required_follow_up.map(str::to_owned)
    } else {
        routed_follow_up
    }
}
