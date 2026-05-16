use std::collections::BTreeSet;

use serde::Serialize;

use crate::diagnostics::JsonFailure;
use crate::execution::closure_graph::{
    AuthoritativeClosureGraph, ClosureGraphSignals, ClosureScope,
};
use crate::execution::context::ExecutionContext;
use crate::execution::current_closure_projection::stale_current_task_closure_records_from_authoritative_state;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, branch_closure_rerecording_assessment_with_authority,
    current_branch_closure_has_tracked_drift, current_task_negative_result_task,
    late_stage_missing_current_closure_stale_provenance_present_with_authority,
    stale_reason_codes_for_late_stage_projection,
};
use crate::execution::gate_reason_codes::push_files_proven_drifted_reason_code_once;
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::reentry_reconcile::{TargetlessStaleAuthority, TargetlessStaleReconcile};
use crate::execution::review_route_tokens::{
    REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY, REVIEW_STATE_MISSING_CURRENT_CLOSURE,
    REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::stale_target_selection::select_earliest_task_stale_target;
use crate::execution::status::{GateResult, PlanExecutionStatus};
use crate::execution::status_support::task_closure_baseline_bridge_ready_for_stale_target_with_authority;
use crate::execution::transitions::AuthoritativeTransitionState;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimeGateSnapshot {
    pub(crate) preflight: Option<GateResult>,
    pub(crate) gate_review: Option<GateResult>,
    pub(crate) gate_finish: Option<GateResult>,
    pub(crate) stale_reason_codes: Vec<String>,
    pub(crate) stale_targets: Vec<AuthoritativeStaleTarget>,
    pub(crate) branch_closure_tracked_drift: bool,
    pub(crate) late_stage_stale_unreviewed: bool,
    pub(crate) missing_current_closure_stale_provenance: bool,
}

impl RuntimeGateSnapshot {
    pub(crate) fn has_authoritative_stale_binding(&self, status: &PlanExecutionStatus) -> bool {
        authoritative_stale_binding_present(
            self.stale_targets.iter(),
            status,
            self.branch_closure_tracked_drift,
            self.missing_current_closure_stale_provenance,
        )
    }

    pub(crate) fn has_authoritative_stale_target(&self) -> bool {
        authoritative_stale_target_present(
            self.stale_targets.iter(),
            self.branch_closure_tracked_drift,
            self.missing_current_closure_stale_provenance,
        )
    }

    pub(crate) fn earliest_task_stale_target(&self) -> Option<u32> {
        self.earliest_task_stale_target_details()
            .and_then(|target| target.task)
    }

    pub(crate) fn earliest_task_stale_target_details(&self) -> Option<&AuthoritativeStaleTarget> {
        select_earliest_task_stale_target(self.stale_targets.iter())
    }

    pub(crate) fn stale_record_ids(&self) -> Vec<String> {
        let mut record_ids = self
            .stale_targets
            .iter()
            .filter(|target| stale_closure_record_target(target))
            .filter_map(|target| target.record_id.clone())
            .collect::<Vec<_>>();
        record_ids.sort();
        record_ids.dedup();
        record_ids
    }

    pub(crate) fn task_stale_record_ids(&self) -> Vec<String> {
        let mut record_ids = self
            .stale_targets
            .iter()
            .filter(|target| target.scope == AuthoritativeStaleTargetScope::Task)
            .filter(|target| stale_closure_record_target(target))
            .filter_map(|target| target.record_id.clone())
            .collect::<Vec<_>>();
        record_ids.sort();
        record_ids.dedup();
        record_ids
    }

    pub(crate) fn task_stale_tasks(&self) -> Vec<u32> {
        let mut tasks = self
            .stale_targets
            .iter()
            .filter(|target| target.scope == AuthoritativeStaleTargetScope::Task)
            .filter_map(|target| target.task)
            .collect::<Vec<_>>();
        tasks.sort_unstable();
        tasks.dedup();
        tasks
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AuthoritativeStaleTarget {
    pub(crate) scope: AuthoritativeStaleTargetScope,
    pub(crate) task: Option<u32>,
    pub(crate) step: Option<u32>,
    pub(crate) record_id: Option<String>,
    pub(crate) source: AuthoritativeStaleTargetSource,
    pub(crate) reason_code: String,
    #[serde(skip)]
    pub(crate) task_closure_bridge_allowed: bool,
}

impl AuthoritativeStaleTarget {
    pub(crate) fn can_drive_public_repair(&self) -> bool {
        self.source.can_drive_public_repair()
    }

    pub(crate) fn is_actionable_task_reentry_target(&self, status: &PlanExecutionStatus) -> bool {
        self.scope == AuthoritativeStaleTargetScope::Task
            && self.can_drive_public_repair()
            && self.source != AuthoritativeStaleTargetSource::BaselineBridge
            && self.task.is_some()
            && self.is_bound_stale_target(status)
    }

    pub(crate) fn is_bound_stale_target(&self, status: &PlanExecutionStatus) -> bool {
        if !self.can_drive_public_repair() {
            return false;
        }
        match self.scope {
            AuthoritativeStaleTargetScope::Task => {
                self.task.is_some()
                    && !self.is_current_task_closure_for_status(status)
                    && !self.is_open_execution_step_for_status(status)
            }
            AuthoritativeStaleTargetScope::Branch | AuthoritativeStaleTargetScope::Milestone => {
                true
            }
        }
    }

    fn is_current_task_closure_for_status(&self, status: &PlanExecutionStatus) -> bool {
        self.scope == AuthoritativeStaleTargetScope::Task
            && self.record_id.as_deref().is_some_and(|record_id| {
                status
                    .current_task_closures
                    .iter()
                    .any(|closure| closure.closure_record_id == record_id)
            })
    }

    fn is_open_execution_step_for_status(&self, status: &PlanExecutionStatus) -> bool {
        let Some(task) = self.task else {
            return false;
        };
        let open_step = if status.resume_task == Some(task) {
            status.resume_step
        } else if status.active_task == Some(task) {
            status.active_step
        } else {
            return false;
        };
        self.step
            .is_none_or(|target_step| open_step == Some(target_step))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthoritativeStaleTargetScope {
    Task,
    Branch,
    Milestone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthoritativeStaleTargetSource {
    ClosureGraph,
    GateReview,
    GateFinish,
    Preflight,
    NegativeResult,
    BaselineBridge,
    ProjectionOnly,
}

pub(crate) const CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN: &str = "closure_graph_stale_target";
pub(crate) const FALLBACK_STALE_TARGET_REASON_CODE: &str = CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN;

impl AuthoritativeStaleTargetSource {
    pub(crate) const fn can_drive_public_repair(self) -> bool {
        !matches!(self, Self::ProjectionOnly)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ClosureGraph => "closure_graph",
            Self::GateReview => "gate_review",
            Self::GateFinish => "gate_finish",
            Self::Preflight => "preflight",
            Self::NegativeResult => "negative_result",
            Self::BaselineBridge => "baseline_bridge",
            Self::ProjectionOnly => "projection_only",
        }
    }

    pub(crate) fn execution_reentry_source_token(self) -> &'static str {
        match self {
            Self::ClosureGraph => CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN,
            _ => self.as_str(),
        }
    }
}

pub(crate) struct StaleTargetProjectionInputs<'a> {
    pub(crate) context: &'a ExecutionContext,
    pub(crate) event_authority_state: Option<&'a AuthoritativeTransitionState>,
    pub(crate) overlay: Option<&'a StatusAuthoritativeOverlay>,
    pub(crate) overlay_current_branch_closure_id: Option<&'a str>,
    pub(crate) status: &'a PlanExecutionStatus,
    pub(crate) preflight: Option<&'a GateResult>,
    pub(crate) gate_review: Option<&'a GateResult>,
    pub(crate) gate_finish: Option<&'a GateResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StaleTargetProjection {
    pub(crate) stale_reason_codes: Vec<String>,
    pub(crate) stale_targets: Vec<AuthoritativeStaleTarget>,
    pub(crate) branch_closure_tracked_drift: bool,
    pub(crate) late_stage_stale_unreviewed: bool,
    pub(crate) missing_current_closure_stale_provenance: bool,
}

impl StaleTargetProjection {
    pub(crate) fn has_authoritative_stale_binding(&self, status: &PlanExecutionStatus) -> bool {
        authoritative_stale_binding_present(
            self.stale_targets.iter(),
            status,
            self.branch_closure_tracked_drift,
            self.missing_current_closure_stale_provenance,
        )
    }

    pub(crate) fn earliest_task_stale_target(&self) -> Option<u32> {
        select_earliest_task_stale_target(self.stale_targets.iter()).and_then(|target| target.task)
    }
}

fn authoritative_stale_binding_present<'a>(
    stale_targets: impl IntoIterator<Item = &'a AuthoritativeStaleTarget>,
    status: &PlanExecutionStatus,
    branch_closure_tracked_drift: bool,
    missing_current_closure_stale_provenance: bool,
) -> bool {
    stale_targets
        .into_iter()
        .any(|target| target.is_bound_stale_target(status))
        || branch_closure_tracked_drift
        || missing_current_closure_stale_provenance
}

fn authoritative_stale_target_present<'a>(
    stale_targets: impl IntoIterator<Item = &'a AuthoritativeStaleTarget>,
    branch_closure_tracked_drift: bool,
    missing_current_closure_stale_provenance: bool,
) -> bool {
    stale_targets
        .into_iter()
        .any(AuthoritativeStaleTarget::can_drive_public_repair)
        || branch_closure_tracked_drift
        || missing_current_closure_stale_provenance
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StaleTaskClosureBridgeTarget {
    pub(crate) task: Option<u32>,
    pub(crate) task_closure_bridge_allowed: bool,
}

impl From<&AuthoritativeStaleTarget> for StaleTaskClosureBridgeTarget {
    fn from(target: &AuthoritativeStaleTarget) -> Self {
        Self {
            task: target.task,
            task_closure_bridge_allowed: target.task_closure_bridge_allowed,
        }
    }
}

pub(crate) fn stale_task_closure_bridge_allows_task(
    stale_target: Option<StaleTaskClosureBridgeTarget>,
    task_number: u32,
) -> bool {
    let Some(stale_target) = stale_target else {
        return true;
    };
    stale_task_closure_bridge_allows_task_parts(
        stale_target
            .task
            .map(|task| (task, stale_target.task_closure_bridge_allowed)),
        task_number,
    )
}

pub(crate) fn stale_task_closure_bridge_allows_task_parts(
    stale_target: Option<(u32, bool)>,
    task_number: u32,
) -> bool {
    let Some((stale_task, task_closure_bridge_allowed)) = stale_target else {
        return true;
    };
    if stale_task < task_number {
        return false;
    }
    stale_task > task_number || task_closure_bridge_allowed
}

pub(crate) fn authoritative_stale_target_allows_task_closure_bridge(
    earliest_task_stale_target: Option<&AuthoritativeStaleTarget>,
    task_number: u32,
) -> bool {
    stale_task_closure_bridge_allows_task(
        earliest_task_stale_target.map(StaleTaskClosureBridgeTarget::from),
        task_number,
    )
}

pub(crate) fn project_authoritative_stale_targets(
    inputs: StaleTargetProjectionInputs<'_>,
) -> Result<StaleTargetProjection, JsonFailure> {
    let StaleTargetProjectionInputs {
        context,
        event_authority_state,
        overlay,
        overlay_current_branch_closure_id,
        status,
        preflight,
        gate_review,
        gate_finish,
    } = inputs;
    let mut stale_reason_codes = stale_reason_codes_for_late_stage_projection(
        status,
        stale_reason_codes_from_snapshot(preflight, gate_review, gate_finish).iter(),
    );
    let branch_closure_tracked_drift =
        current_branch_closure_has_tracked_drift(context, event_authority_state)?;
    if branch_closure_tracked_drift {
        push_files_proven_drifted_reason_code_once(&mut stale_reason_codes);
    }
    let missing_current_closure_stale_provenance = status.review_state_status
        == REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && late_stage_missing_current_closure_stale_provenance_present_with_authority(
            context,
            status,
            event_authority_state,
        )?;
    let late_stage_stale_unreviewed =
        status.review_state_status == REVIEW_STATE_STALE_UNREVIEWED || branch_closure_tracked_drift;
    let closure_graph = AuthoritativeClosureGraph::from_state(
        event_authority_state,
        &ClosureGraphSignals::from_authoritative_state(
            event_authority_state,
            overlay_current_branch_closure_id,
            late_stage_stale_unreviewed,
            missing_current_closure_stale_provenance,
            stale_reason_codes.clone(),
        ),
    );
    let mut stale_targets = stale_targets_from_closure_graph(&closure_graph, event_authority_state);
    append_current_task_stale_targets(&mut stale_targets, context, event_authority_state)?;
    let branch_rerecording_assessment = event_authority_state
        .map(|_| {
            branch_closure_rerecording_assessment_with_authority(context, event_authority_state)
        })
        .transpose()?;
    append_task_closure_baseline_stale_target(
        &mut stale_targets,
        context,
        status,
        overlay,
        event_authority_state,
        branch_rerecording_assessment.as_ref(),
    )?;
    append_gate_stale_targets(
        &mut stale_targets,
        preflight,
        AuthoritativeStaleTargetSource::Preflight,
    );
    append_gate_stale_targets(
        &mut stale_targets,
        gate_review,
        AuthoritativeStaleTargetSource::GateReview,
    );
    append_gate_stale_targets(
        &mut stale_targets,
        gate_finish,
        AuthoritativeStaleTargetSource::GateFinish,
    );
    append_negative_result_stale_target(&mut stale_targets, status, overlay, event_authority_state);
    remove_current_task_closure_stale_targets(&mut stale_targets, status);
    Ok(StaleTargetProjection {
        stale_reason_codes,
        stale_targets,
        branch_closure_tracked_drift,
        late_stage_stale_unreviewed,
        missing_current_closure_stale_provenance,
    })
}

pub(crate) fn project_stale_unreviewed_closures(
    status: &mut PlanExecutionStatus,
    gate_snapshot: &RuntimeGateSnapshot,
) {
    let mut stale_record_ids = if status.review_state_status == REVIEW_STATE_STALE_UNREVIEWED
        || status.review_state_status == REVIEW_STATE_MISSING_CURRENT_CLOSURE
    {
        gate_snapshot.stale_record_ids()
    } else {
        gate_snapshot.task_stale_record_ids()
    };
    stale_record_ids.sort();
    stale_record_ids.dedup();
    status.stale_unreviewed_closures = stale_record_ids;

    let has_authoritative_stale_target =
        targetless_stale_authority_for_gate_snapshot(gate_snapshot)
            .has_authoritative_stale_target();
    if TargetlessStaleReconcile::status_needs_marker_with_authority(
        status,
        has_authoritative_stale_target,
    ) {
        TargetlessStaleReconcile::ensure_status_diagnostic(status);
    } else {
        TargetlessStaleReconcile::clear_status_diagnostic(status);
    }
}

pub(crate) fn targetless_stale_authority_for_gate_snapshot(
    gate_snapshot: &RuntimeGateSnapshot,
) -> TargetlessStaleAuthority {
    TargetlessStaleAuthority::new(gate_snapshot.has_authoritative_stale_target())
}

pub(crate) struct ReviewStateStaleClosureProjectionInputs<'a> {
    pub(crate) status: &'a PlanExecutionStatus,
    pub(crate) gate_snapshot: &'a RuntimeGateSnapshot,
    pub(crate) task_scope_stale_unreviewed: bool,
    pub(crate) task_scope_structural_reason_present: bool,
    pub(crate) branch_scope_structural_reason_present: bool,
}

pub(crate) struct ReviewStateStaleClosureProjection {
    pub(crate) late_stage_stale_projection_active: bool,
    pub(crate) stale_unreviewed_closures: Vec<String>,
}

pub(crate) fn project_review_state_stale_unreviewed_closures(
    inputs: ReviewStateStaleClosureProjectionInputs<'_>,
) -> ReviewStateStaleClosureProjection {
    let ReviewStateStaleClosureProjectionInputs {
        status,
        gate_snapshot,
        task_scope_stale_unreviewed,
        task_scope_structural_reason_present,
        branch_scope_structural_reason_present,
    } = inputs;
    let status_reports_stale_unreviewed_closures = !status.stale_unreviewed_closures.is_empty();
    let late_stage_stale_projection_active = gate_snapshot.late_stage_stale_unreviewed
        || gate_snapshot.missing_current_closure_stale_provenance
        || (gate_snapshot.branch_closure_tracked_drift
            && (status.review_state_status != REVIEW_STATE_MISSING_CURRENT_CLOSURE
                || status_reports_stale_unreviewed_closures));
    let reducer_task_stale_record_ids = gate_snapshot.task_stale_record_ids();
    let stale_unreviewed_closures = if task_scope_structural_reason_present {
        reducer_task_stale_record_ids.clone()
    } else if late_stage_stale_projection_active {
        status.stale_unreviewed_closures.clone()
    } else if branch_scope_structural_reason_present {
        Vec::new()
    } else if task_scope_stale_unreviewed {
        reducer_task_stale_record_ids
    } else {
        Vec::new()
    };
    ReviewStateStaleClosureProjection {
        late_stage_stale_projection_active,
        stale_unreviewed_closures,
    }
}

pub(crate) fn closure_baseline_candidate_task(context: &ExecutionContext) -> Option<u32> {
    if let Some(next_unchecked_task) = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number)
    {
        return context
            .tasks_by_number
            .keys()
            .copied()
            .filter(|task_number| *task_number < next_unchecked_task)
            .max();
    }
    context.tasks_by_number.keys().copied().max()
}

fn stale_targets_from_closure_graph(
    closure_graph: &AuthoritativeClosureGraph,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Vec<AuthoritativeStaleTarget> {
    let mut targets = closure_graph
        .stale_unreviewed_evaluations()
        .into_iter()
        .flat_map(|evaluation| {
            let reason_codes = if evaluation.stale_reason_codes.is_empty() {
                vec![String::from(FALLBACK_STALE_TARGET_REASON_CODE)]
            } else {
                evaluation.stale_reason_codes
            };
            let task = evaluation.identity.task_number;
            let record_id = evaluation.identity.record_id.clone();
            reason_codes
                .into_iter()
                .map(move |reason_code| AuthoritativeStaleTarget {
                    scope: stale_target_scope_from_closure_scope(evaluation.identity.scope),
                    task,
                    step: None,
                    record_id: Some(record_id.clone()),
                    source: AuthoritativeStaleTargetSource::ClosureGraph,
                    task_closure_bridge_allowed:
                        closure_graph_stale_target_allows_task_closure_bridge(
                            authoritative_state,
                            task,
                            &record_id,
                            &reason_code,
                        ),
                    reason_code,
                })
        })
        .collect::<Vec<_>>();
    for record_id in closure_graph.stale_projection_only_record_ids() {
        targets.push(AuthoritativeStaleTarget {
            scope: AuthoritativeStaleTargetScope::Milestone,
            task: None,
            step: None,
            record_id: Some(record_id),
            source: AuthoritativeStaleTargetSource::ProjectionOnly,
            reason_code: String::from("projection_only_stale_target"),
            task_closure_bridge_allowed: false,
        });
    }
    targets
}

fn closure_graph_stale_target_allows_task_closure_bridge(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    task: Option<u32>,
    record_id: &str,
    reason_code: &str,
) -> bool {
    reason_code != crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
        && task.is_some_and(|task| {
            authoritative_state.is_some_and(|state| {
                state.task_closure_history_lineage_present(task, Some(record_id))
            })
        })
}

fn append_current_task_stale_targets(
    stale_targets: &mut Vec<AuthoritativeStaleTarget>,
    context: &ExecutionContext,
    event_authority_state: Option<&AuthoritativeTransitionState>,
) -> Result<(), JsonFailure> {
    let records = match event_authority_state {
        Some(state) => stale_current_task_closure_records_from_authoritative_state(context, state)?,
        None => Vec::new(),
    };
    for record in records {
        if stale_targets.iter().any(|target| {
            target.scope == AuthoritativeStaleTargetScope::Task
                && target.task == Some(record.task)
                && target.record_id.as_deref() == Some(record.closure_record_id.as_str())
        }) {
            continue;
        }
        stale_targets.push(AuthoritativeStaleTarget {
            scope: AuthoritativeStaleTargetScope::Task,
            task: Some(record.task),
            step: None,
            record_id: Some(record.closure_record_id),
            source: AuthoritativeStaleTargetSource::ClosureGraph,
            reason_code: String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE),
            task_closure_bridge_allowed: false,
        });
    }
    Ok(())
}

fn append_task_closure_baseline_stale_target(
    stale_targets: &mut Vec<AuthoritativeStaleTarget>,
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_rerecording_assessment: Option<&BranchRerecordingAssessment>,
) -> Result<(), JsonFailure> {
    if status.handoff_required
        || status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::phase::PHASE_HANDOFF_REQUIRED)
    {
        return Ok(());
    }
    if !status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE
                | REVIEW_STATE_STALE_UNREVIEWED
        )
    }) {
        return Ok(());
    }
    let Some(task) = status
        .blocking_task
        .or(status.resume_task)
        .or(status.active_task)
        .or_else(|| closure_baseline_candidate_task(context))
    else {
        return Ok(());
    };
    let earliest_stale_task =
        select_earliest_task_stale_target(stale_targets.iter()).and_then(|target| target.task);
    let baseline_bridge_reason_present = status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
        )
    });
    if !baseline_bridge_reason_present {
        return Ok(());
    }
    if status
        .current_task_closures
        .iter()
        .any(|closure| closure.task == task)
    {
        return Ok(());
    }
    let Some(branch_rerecording_assessment) = branch_rerecording_assessment else {
        return Ok(());
    };
    if !task_closure_baseline_bridge_ready_for_stale_target_with_authority(
        context,
        status,
        task,
        earliest_stale_task,
        overlay,
        authoritative_state,
        branch_rerecording_assessment,
    )? {
        return Ok(());
    }
    if stale_targets.iter().any(|target| {
        target.scope == AuthoritativeStaleTargetScope::Task && target.task == Some(task)
    }) {
        return Ok(());
    }
    let reason_code = task_stale_target_reason_code(status)
        .unwrap_or_else(|| String::from(FALLBACK_STALE_TARGET_REASON_CODE));
    stale_targets.push(AuthoritativeStaleTarget {
        scope: AuthoritativeStaleTargetScope::Task,
        task: Some(task),
        step: None,
        record_id: None,
        source: AuthoritativeStaleTargetSource::BaselineBridge,
        reason_code,
        task_closure_bridge_allowed: true,
    });
    Ok(())
}

fn task_stale_target_reason_code(status: &PlanExecutionStatus) -> Option<String> {
    [
        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE,
        REVIEW_STATE_STALE_UNREVIEWED,
    ]
    .into_iter()
    .find(|candidate| {
        status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == candidate)
    })
    .map(str::to_owned)
}

fn append_gate_stale_targets(
    stale_targets: &mut Vec<AuthoritativeStaleTarget>,
    gate: Option<&GateResult>,
    source: AuthoritativeStaleTargetSource,
) {
    let Some(gate) = gate else {
        return;
    };
    for reason_code in &gate.reason_codes {
        if !crate::execution::closure_graph::reason_code_indicates_stale_unreviewed(reason_code) {
            continue;
        }
        stale_targets.push(AuthoritativeStaleTarget {
            scope: gate_target_scope(source, gate),
            task: None,
            step: None,
            record_id: gate
                .current_branch_closure_id
                .clone()
                .or_else(|| gate.finish_review_gate_pass_branch_closure_id.clone()),
            source,
            reason_code: reason_code.clone(),
            task_closure_bridge_allowed: false,
        });
    }
}

fn append_negative_result_stale_target(
    stale_targets: &mut Vec<AuthoritativeStaleTarget>,
    status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
    event_authority_state: Option<&AuthoritativeTransitionState>,
) {
    let Some(task) = current_task_negative_result_task(status, overlay, event_authority_state)
    else {
        return;
    };
    let stricter_task_stale_target_present = stale_targets.iter().any(|target| {
        target.scope == AuthoritativeStaleTargetScope::Task
            && target.task == Some(task)
            && !target.task_closure_bridge_allowed
    });
    stale_targets.push(AuthoritativeStaleTarget {
        scope: AuthoritativeStaleTargetScope::Task,
        task: Some(task),
        step: None,
        record_id: Some(format!("task-{task}")),
        source: AuthoritativeStaleTargetSource::NegativeResult,
        reason_code: String::from(REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY),
        task_closure_bridge_allowed: !stricter_task_stale_target_present,
    });
}

fn current_task_closure_ids(status: &PlanExecutionStatus) -> BTreeSet<&str> {
    status
        .current_task_closures
        .iter()
        .map(|closure| closure.closure_record_id.as_str())
        .collect()
}

fn target_is_current_task_closure(
    target: &AuthoritativeStaleTarget,
    current_closure_ids: &BTreeSet<&str>,
) -> bool {
    target.scope == AuthoritativeStaleTargetScope::Task
        && target
            .record_id
            .as_deref()
            .is_some_and(|record_id| current_closure_ids.contains(record_id))
}

fn remove_current_task_closure_stale_targets(
    stale_targets: &mut Vec<AuthoritativeStaleTarget>,
    status: &PlanExecutionStatus,
) {
    let current_closure_ids = current_task_closure_ids(status);
    remove_current_task_closure_stale_targets_for_ids(stale_targets, &current_closure_ids);
}

fn remove_current_task_closure_stale_targets_for_ids(
    stale_targets: &mut Vec<AuthoritativeStaleTarget>,
    current_closure_ids: &BTreeSet<&str>,
) {
    stale_targets.retain(|target| !target_is_current_task_closure(target, current_closure_ids));
}

fn stale_closure_record_target(target: &AuthoritativeStaleTarget) -> bool {
    if !target.can_drive_public_repair() {
        return false;
    }
    !matches!(
        target.source,
        AuthoritativeStaleTargetSource::BaselineBridge
            | AuthoritativeStaleTargetSource::NegativeResult
    )
}

fn stale_target_scope_from_closure_scope(scope: ClosureScope) -> AuthoritativeStaleTargetScope {
    match scope {
        ClosureScope::Task => AuthoritativeStaleTargetScope::Task,
        ClosureScope::Branch => AuthoritativeStaleTargetScope::Branch,
        ClosureScope::Milestone => AuthoritativeStaleTargetScope::Milestone,
    }
}

fn gate_target_scope(
    source: AuthoritativeStaleTargetSource,
    gate: &GateResult,
) -> AuthoritativeStaleTargetScope {
    if source == AuthoritativeStaleTargetSource::Preflight {
        return AuthoritativeStaleTargetScope::Task;
    }
    if gate.current_branch_closure_id.is_some()
        || gate.finish_review_gate_pass_branch_closure_id.is_some()
    {
        AuthoritativeStaleTargetScope::Branch
    } else {
        AuthoritativeStaleTargetScope::Milestone
    }
}

fn stale_reason_codes_from_snapshot(
    preflight: Option<&GateResult>,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> Vec<String> {
    let mut reason_codes = Vec::new();
    for reason_code in preflight
        .into_iter()
        .chain(gate_review)
        .chain(gate_finish)
        .flat_map(|gate| gate.reason_codes.iter())
    {
        if crate::execution::closure_graph::reason_code_indicates_stale_unreviewed(reason_code)
            && !reason_codes.iter().any(|existing| existing == reason_code)
        {
            reason_codes.push(reason_code.clone());
        }
    }
    reason_codes
}

#[cfg(test)]
mod unit_tests;
