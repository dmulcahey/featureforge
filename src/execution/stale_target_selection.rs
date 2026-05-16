//! Shared stale-target selection semantics.
//!
//! Stale-target projection, repair routing, and operator presentation all need
//! the same target ordering. Keep the tie-breakers here so task-boundary and
//! execution-reentry routes cannot silently drift.

use crate::execution::command_eligibility::PublicCommandKind;
use crate::execution::stale_target_projection::CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN;
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, AuthoritativeStaleTargetScope, AuthoritativeStaleTargetSource,
};
use crate::execution::state::PlanExecutionStatus;
use crate::execution::task_scope_key::{
    task_prefixed_record_id_task_number, task_scope_key_task_number,
};
use crate::execution::transitions::AuthoritativeTransitionState;
use crate::execution::{phase, review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED};

pub(crate) fn select_earliest_task_stale_target<'a>(
    stale_targets: impl IntoIterator<Item = &'a AuthoritativeStaleTarget>,
) -> Option<&'a AuthoritativeStaleTarget> {
    stale_targets
        .into_iter()
        .filter(|target| target.scope == AuthoritativeStaleTargetScope::Task)
        .filter(|target| target.task.is_some())
        .min_by(|left, right| {
            left.task
                .cmp(&right.task)
                .then_with(|| left.record_id.cmp(&right.record_id))
                .then_with(|| left.reason_code.cmp(&right.reason_code))
        })
}

pub(crate) fn select_actionable_stale_reentry_target<'a>(
    status: &PlanExecutionStatus,
    stale_targets: impl IntoIterator<Item = &'a AuthoritativeStaleTarget>,
) -> Option<&'a AuthoritativeStaleTarget> {
    stale_targets
        .into_iter()
        .filter(|target| target.is_actionable_task_reentry_target(status))
        .min_by(|left, right| {
            left.task
                .cmp(&right.task)
                .then_with(|| left.step.cmp(&right.step))
                .then_with(|| {
                    stale_reentry_source_record_id(left).cmp(&stale_reentry_source_record_id(right))
                })
                .then_with(|| left.reason_code.cmp(&right.reason_code))
        })
}

pub(crate) fn stale_reentry_source_record_id(target: &AuthoritativeStaleTarget) -> Option<&str> {
    target.record_id.as_deref().or(Some(target.source.as_str()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaleBoundaryCandidateSource {
    AuthoritativeStaleTarget,
    TaskClosureBaselineBridge,
}

impl StaleBoundaryCandidateSource {
    fn from_authoritative_stale_target_source(source: AuthoritativeStaleTargetSource) -> Self {
        match source {
            AuthoritativeStaleTargetSource::BaselineBridge => Self::TaskClosureBaselineBridge,
            _ => Self::AuthoritativeStaleTarget,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StaleBoundaryCandidate {
    task: u32,
    source: StaleBoundaryCandidateSource,
}

impl StaleBoundaryCandidate {
    pub(crate) fn from_authoritative_stale_target(
        task: u32,
        source: AuthoritativeStaleTargetSource,
    ) -> Self {
        Self {
            task,
            source: StaleBoundaryCandidateSource::from_authoritative_stale_target_source(source),
        }
    }

    fn task_closure_baseline_bridge(task: u32) -> Self {
        Self {
            task,
            source: StaleBoundaryCandidateSource::TaskClosureBaselineBridge,
        }
    }

    #[must_use]
    pub(crate) fn task(self) -> u32 {
        self.task
    }

    #[must_use]
    pub(crate) fn source(self) -> StaleBoundaryCandidateSource {
        self.source
    }
}

pub(crate) fn select_earliest_stale_boundary_candidate(
    authoritative_stale_candidate: Option<StaleBoundaryCandidate>,
    baseline_reentry_task: Option<u32>,
) -> Option<StaleBoundaryCandidate> {
    match (authoritative_stale_candidate, baseline_reentry_task) {
        (Some(authoritative), Some(baseline_task)) if baseline_task < authoritative.task() => Some(
            StaleBoundaryCandidate::task_closure_baseline_bridge(baseline_task),
        ),
        (Some(authoritative), _) => Some(authoritative),
        (None, Some(baseline_task)) => Some(StaleBoundaryCandidate::task_closure_baseline_bridge(
            baseline_task,
        )),
        (None, None) => None,
    }
}

pub(crate) fn select_first_task_number(candidates: &[u32]) -> Option<u32> {
    candidates.iter().copied().min()
}

pub(crate) fn select_first_task_number_from_scope_keys(scope_keys: &[String]) -> Option<u32> {
    scope_keys
        .iter()
        .filter_map(|scope_key| task_scope_key_task_number(scope_key))
        .min()
}

pub(crate) fn select_repair_plan_stale_target_task(
    stale_tasks: &[u32],
    closure_graph_stale_target: Option<u32>,
    branch_stale_source_task: Option<u32>,
) -> Option<u32> {
    // Preserve the historical repair-plan precedence: explicit task-scope
    // stale tasks sort first, then the reducer's single closure-graph target,
    // then late-stage branch reroute source task as the broadest fallback.
    select_first_task_number(stale_tasks)
        .or(closure_graph_stale_target)
        .or(branch_stale_source_task)
}

pub(crate) fn select_route_projected_stale_boundary_task(
    status: &PlanExecutionStatus,
) -> Option<u32> {
    let route_task = status
        .blocking_task
        .or(status.resume_task)
        .or(status.active_task);
    let stale_reentry_route = status.review_state_status == REVIEW_STATE_STALE_UNREVIEWED
        && status.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED;
    if status.execution_reentry_target_source.as_deref()
        == Some(CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN)
        || stale_reentry_route
    {
        return route_task;
    }
    status
        .public_repair_targets
        .iter()
        .filter(|target| {
            PublicCommandKind::Reopen.matches_public_mutation_token(&target.command_kind)
        })
        .filter_map(|target| target.task)
        .min()
}

pub(crate) fn projected_earliest_stale_task_candidate_from_status(
    status: &PlanExecutionStatus,
) -> Option<u32> {
    let projected_stale_blocking_task = (status.review_state_status
        == REVIEW_STATE_STALE_UNREVIEWED
        && status.blocking_scope.as_deref() == Some("task"))
    .then_some(status.blocking_task)
    .flatten();
    status
        .blocking_records
        .iter()
        .filter(|record| record.scope_type == "task")
        .filter_map(|record| task_scope_key_task_number(&record.scope_key))
        .chain(projected_stale_blocking_task)
        .chain(
            status
                .stale_unreviewed_closures
                .iter()
                .filter_map(|record_id| task_prefixed_record_id_task_number(record_id)),
        )
        .chain(
            status
                .public_repair_targets
                .iter()
                .filter_map(|target| target.task),
        )
        .min()
}

pub(crate) fn select_branch_stale_source_task(
    authoritative_state: &AuthoritativeTransitionState,
    stale_branch_closure_ids: &[String],
) -> Option<u32> {
    let current_records_by_closure_id = authoritative_state
        .current_task_closure_results()
        .into_values()
        .map(|record| (record.closure_record_id, record.task));
    let history_records_by_closure_id = authoritative_state
        .task_closure_history_records()
        .into_iter()
        .map(|record| (record.closure_record_id, record.task));
    let closure_tasks_by_id = current_records_by_closure_id
        .chain(history_records_by_closure_id)
        .collect::<std::collections::BTreeMap<_, _>>();

    // Preserve the branch/late-stage fallback ordering used by repair-state
    // analysis: stale branch closure ids are already ordered by projection, and
    // each branch record's source task closure ids are read in record order.
    stale_branch_closure_ids
        .iter()
        .filter_map(|closure_id| authoritative_state.branch_closure_record(closure_id))
        .flat_map(|record| record.source_task_closure_ids)
        .find_map(|source_task_closure_id| {
            closure_tasks_by_id.get(&source_task_closure_id).copied()
        })
}

#[cfg(test)]
mod unit_tests;
