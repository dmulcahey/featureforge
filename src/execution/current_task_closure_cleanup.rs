use std::collections::BTreeSet;

use crate::diagnostics::JsonFailure;
use crate::execution::authority::WorktreeLeaseReleaseDecision;
use crate::execution::harness::{WorktreeLeaseBindingSnapshot, WorktreeLeaseReleaseRecord};
use crate::execution::state::{
    ExecutionContext, releasable_terminal_worktree_lease_fingerprints_for_task_closure,
    still_current_task_closure_records_from_authoritative_state,
};
use crate::execution::transitions::AuthoritativeTransitionState;

pub(crate) fn worktree_lease_release_decision_for_current_task_closures_from_authority(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    execution_run_id: &str,
    active_fingerprints: &[String],
    active_bindings: &[WorktreeLeaseBindingSnapshot],
    released_by_command: &str,
    task_filter: Option<u32>,
) -> Result<WorktreeLeaseReleaseDecision, JsonFailure> {
    let current_closures =
        still_current_task_closure_records_from_authoritative_state(context, authoritative_state)?;
    let mut released_by = Vec::new();
    let mut release_records = Vec::new();
    let mut releasable = BTreeSet::new();
    for closure in current_closures
        .iter()
        .filter(|closure| closure.review_result == "pass" && closure.verification_result == "pass")
        .filter(|closure| task_filter.is_none_or(|task| closure.task == task))
    {
        let closure_releasable = releasable_terminal_worktree_lease_fingerprints_for_task_closure(
            context,
            Some(execution_run_id),
            active_fingerprints,
            active_bindings,
            closure.task,
        );
        if !closure_releasable.is_empty() {
            for lease_fingerprint in closure_releasable {
                if releasable.insert(lease_fingerprint.clone()) {
                    release_records.push(WorktreeLeaseReleaseRecord {
                        execution_run_id: execution_run_id.to_owned(),
                        lease_fingerprint,
                        source_task: closure.task,
                        source_task_closure_record_id: closure.closure_record_id.clone(),
                        released_by: released_by_command.to_owned(),
                    });
                }
            }
            released_by.push((closure.task, closure.closure_record_id.clone()));
        }
    }
    Ok(WorktreeLeaseReleaseDecision {
        released_by,
        lease_fingerprints: releasable,
        release_records,
    })
}

pub(crate) fn current_task_closure_postconditions_would_mutate(
    authoritative_state: &AuthoritativeTransitionState,
    task_number: u32,
    closure_record_id: &str,
    reviewed_state_id: &str,
) -> bool {
    current_task_closure_postcondition_resolution(
        authoritative_state,
        task_number,
        closure_record_id,
        reviewed_state_id,
    )
    .would_mutate()
}

pub(crate) struct CurrentTaskClosurePostconditionResolution {
    pub(crate) clear_cycle_break: bool,
    pub(crate) clear_repair_follow_up: bool,
}

impl CurrentTaskClosurePostconditionResolution {
    pub(crate) const fn would_mutate(&self) -> bool {
        self.clear_cycle_break || self.clear_repair_follow_up
    }
}

pub(crate) fn current_task_closure_postcondition_resolution(
    authoritative_state: &AuthoritativeTransitionState,
    task_number: u32,
    closure_record_id: &str,
    _reviewed_state_id: &str,
) -> CurrentTaskClosurePostconditionResolution {
    let Some(current_closure) = authoritative_state.current_task_closure_result(task_number) else {
        return CurrentTaskClosurePostconditionResolution {
            clear_cycle_break: false,
            clear_repair_follow_up: false,
        };
    };
    let current_positive_closure_on_current_reviewed_state = current_closure.closure_record_id
        == closure_record_id
        && current_closure.review_result == "pass"
        && current_closure.verification_result == "pass"
        && current_closure
            .closure_status
            .as_deref()
            .is_none_or(|status| status == "current");
    if !current_positive_closure_on_current_reviewed_state
        || authoritative_state
            .task_closure_negative_result(task_number)
            .is_some()
    {
        return CurrentTaskClosurePostconditionResolution {
            clear_cycle_break: false,
            clear_repair_follow_up: false,
        };
    }
    let clear_cycle_break = authoritative_state.strategy_cycle_break_task() == Some(task_number);
    let repair_follow_up_matches_current_closure = authoritative_state
        .review_state_repair_follow_up_task()
        .is_some_and(|task| task == task_number)
        || authoritative_state
            .review_state_repair_follow_up_closure_record_id()
            .is_some_and(|record_id| record_id == closure_record_id);
    let structured_follow_up_kind = authoritative_state
        .review_state_repair_follow_up_record()
        .map(|record| record.kind);
    let legacy_follow_up = authoritative_state.review_state_repair_follow_up();
    let cycle_break_follow_up_matches_task = clear_cycle_break
        && legacy_follow_up
            .is_some_and(|follow_up| matches!(follow_up, "cycle_break" | "cycle_break_repair"));
    let repair_follow_up_kind_can_clear = structured_follow_up_kind.is_some_and(|kind| {
        matches!(
            kind.public_token(),
            crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY
                | crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE
                | crate::execution::review_route_tokens::FOLLOW_UP_CLOSE_CURRENT_TASK
        )
    }) || legacy_follow_up.is_some_and(|follow_up| {
        matches!(
            follow_up,
            crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY
                | crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE
                | "task_closure_baseline_repair"
                | "cycle_break"
                | "cycle_break_repair"
        )
    });
    CurrentTaskClosurePostconditionResolution {
        clear_cycle_break,
        clear_repair_follow_up: repair_follow_up_kind_can_clear
            && (repair_follow_up_matches_current_closure || cycle_break_follow_up_matches_task),
    }
}
