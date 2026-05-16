use crate::execution::query::ExecutionRoutingState;
use crate::execution::reentry_reconcile::TargetlessStaleReconcile;
use crate::execution::state::{PlanExecutionStatus, StatusBlockingRecord};
use crate::execution::task_scope_key::task_scope_key_task_number;

use super::decision::{Blocker, NextPublicAction};
use super::state_kind::{
    state_kind_is_external_wait, state_kind_is_planning_reentry_required,
    state_kind_is_runtime_diagnostic, state_kind_is_terminal,
};

pub(crate) fn targetless_stale_reconcile_blockers(phase_detail: &str) -> Vec<Blocker> {
    let reconcile = TargetlessStaleReconcile;
    vec![Blocker {
        category: String::from("runtime_bug"),
        scope_type: String::from("runtime"),
        scope_key: phase_detail.to_owned(),
        record_id: None,
        next_public_action: None,
        details: String::from(reconcile.detail()),
    }]
}

pub(crate) fn blocking_task_from_blockers(blockers: &[Blocker]) -> Option<u32> {
    blockers.iter().find_map(|blocker| {
        (blocker.scope_type == "task")
            .then(|| task_scope_key_task_number(&blocker.scope_key))
            .flatten()
    })
}

fn blocker_from_status_record(
    record: &StatusBlockingRecord,
    next_public_action: Option<&NextPublicAction>,
) -> Blocker {
    let category = match record.scope_type.as_str() {
        "task" => String::from("task_boundary"),
        "branch" => String::from("late_stage"),
        _ => String::from("structural"),
    };
    Blocker {
        category,
        scope_type: record.scope_type.clone(),
        scope_key: record.scope_key.clone(),
        record_id: record.record_id.clone(),
        next_public_action: next_public_action.cloned(),
        details: record.message.clone(),
    }
}

struct BlockerSource<'a> {
    phase_detail: &'a str,
    blocking_scope: Option<&'a str>,
    blocking_task: Option<u32>,
    blocking_records: &'a [StatusBlockingRecord],
    planning_next_skill: Option<&'a str>,
}

pub(crate) fn primary_blocker_for_route(
    routing: &ExecutionRoutingState,
    blocking_records: &[StatusBlockingRecord],
    state_kind: &str,
    next_public_action: Option<&NextPublicAction>,
) -> Vec<Blocker> {
    primary_blocker_for_source(
        BlockerSource {
            phase_detail: &routing.phase_detail,
            blocking_scope: routing.blocking_scope.as_deref(),
            blocking_task: routing.blocking_task,
            blocking_records,
            planning_next_skill: non_empty_str(routing.route.next_skill.as_str()),
        },
        state_kind,
        next_public_action,
    )
}

pub(crate) fn primary_blocker_for_status(
    status: &PlanExecutionStatus,
    state_kind: &str,
    next_public_action: Option<&NextPublicAction>,
) -> Vec<Blocker> {
    primary_blocker_for_source(
        BlockerSource {
            phase_detail: &status.phase_detail,
            blocking_scope: status.blocking_scope.as_deref(),
            blocking_task: status.blocking_task,
            blocking_records: &status.blocking_records,
            planning_next_skill: None,
        },
        state_kind,
        next_public_action,
    )
}

fn primary_blocker_for_source(
    source: BlockerSource<'_>,
    state_kind: &str,
    next_public_action: Option<&NextPublicAction>,
) -> Vec<Blocker> {
    if state_kind_is_terminal(state_kind) {
        return Vec::new();
    }

    if state_kind_is_external_wait(state_kind) {
        let scope_type = source
            .blocking_scope
            .map(str::to_owned)
            .unwrap_or_else(|| String::from("external"));
        let scope_key = source
            .blocking_task
            .map(|task| format!("task-{task}"))
            .unwrap_or_else(|| source.phase_detail.to_owned());
        return vec![Blocker {
            category: String::from("external_input"),
            scope_type,
            scope_key,
            record_id: None,
            next_public_action: next_public_action.cloned(),
            details: String::from("Waiting for external review result."),
        }];
    }

    if state_kind_is_planning_reentry_required(state_kind) {
        return vec![Blocker {
            category: String::from("workflow"),
            scope_type: source
                .blocking_scope
                .map(str::to_owned)
                .unwrap_or_else(|| String::from("workflow")),
            scope_key: source.phase_detail.to_owned(),
            record_id: None,
            next_public_action: next_public_action.cloned(),
            details: planning_reentry_blocker_details(source.planning_next_skill),
        }];
    }

    if let Some(primary) = source.blocking_records.first() {
        return vec![blocker_from_status_record(primary, next_public_action)];
    }

    if state_kind_is_runtime_diagnostic(state_kind) {
        return vec![Blocker {
            category: String::from("runtime_bug"),
            scope_type: String::from("runtime"),
            scope_key: source.phase_detail.to_owned(),
            record_id: None,
            next_public_action: next_public_action.cloned(),
            details: runtime_diagnostic_blocker_details(source.phase_detail),
        }];
    }

    if let Some(next_public_action) = next_public_action {
        return vec![Blocker {
            category: String::from("workflow"),
            scope_type: source
                .blocking_scope
                .map(str::to_owned)
                .unwrap_or_else(|| String::from("route")),
            scope_key: source
                .blocking_task
                .map(|task| format!("task-{task}"))
                .unwrap_or_else(|| source.phase_detail.to_owned()),
            record_id: None,
            next_public_action: Some(next_public_action.clone()),
            details: format!(
                "Follow the public routing lane for `{}`.",
                source.phase_detail
            ),
        }];
    }

    Vec::new()
}

fn runtime_diagnostic_blocker_details(phase_detail: &str) -> String {
    format!("Routing reached `{phase_detail}` without an actionable public recommendation.")
}

fn planning_reentry_blocker_details(planning_next_skill: Option<&str>) -> String {
    let planning_next_skill = planning_next_skill.unwrap_or("featureforge:plan-eng-review");
    format!("Return to {planning_next_skill} for planning reentry before continuing execution.")
}

fn non_empty_str(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn materialize_plan_template(template: &str, plan_path: &str) -> String {
    template.replace("<approved-plan-path>", plan_path)
}

pub(crate) fn materialize_blocker_actions(
    mut blockers: Vec<Blocker>,
    plan_path: &str,
) -> Vec<Blocker> {
    for blocker in &mut blockers {
        if let Some(action) = blocker.next_public_action.as_mut() {
            action.command = materialize_plan_template(&action.command, plan_path);
            if let Some(args_template) = action.args_template.as_mut() {
                *args_template = materialize_plan_template(args_template, plan_path);
            }
        }
    }
    blockers
}
