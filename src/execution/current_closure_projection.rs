use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::context::ExecutionContext;
use crate::execution::read_model_support::{
    task_boundary_reason_code_from_message, task_closure_matches_current_workspace,
    validate_current_task_closure_record,
};
use crate::execution::status::PublicReviewStateTaskClosure;
use crate::execution::transitions::{
    AuthoritativeTransitionState, CurrentTaskClosureRecord, RawCurrentTaskClosureStateEntry,
    load_authoritative_transition_state,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskCurrentClosureStatus {
    Missing,
    Current,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentTaskClosureStructuralFailure {
    pub(crate) task: Option<u32>,
    pub(crate) scope_key: String,
    pub(crate) closure_record_id: Option<String>,
    pub(crate) reason_code: String,
    pub(crate) message: String,
}

pub(crate) fn project_current_task_closures(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Vec<PublicReviewStateTaskClosure>, JsonFailure> {
    let records = match authoritative_state {
        Some(state) => still_current_task_closure_records_from_authoritative_state(context, state)?,
        None => still_current_task_closure_records(context)?,
    };
    Ok(records
        .into_iter()
        .map(public_task_closure_from_record)
        .collect())
}

pub(crate) fn project_current_task_closure_repair_reason_codes(
    context: &ExecutionContext,
) -> Vec<String> {
    if context.steps.iter().any(|step| !step.checked) {
        return Vec::new();
    }
    let Ok(structural_failures) = structural_current_task_closure_failures(context) else {
        return Vec::new();
    };
    let mut reason_codes = Vec::new();
    for failure in structural_failures {
        push_reason_code_once(&mut reason_codes, &failure.reason_code);
    }
    let Ok(current_records) = valid_current_task_closure_records(context) else {
        return reason_codes;
    };
    for record in current_records {
        match task_closure_matches_current_workspace(context, &record) {
            Ok(true) => {}
            Ok(false) => {
                push_reason_code_once(&mut reason_codes, "prior_task_current_closure_stale");
            }
            Err(error) => {
                if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message) {
                    push_reason_code_once(&mut reason_codes, reason_code);
                }
            }
        }
    }
    reason_codes
}

pub(crate) fn structural_current_task_closure_failures(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureStructuralFailure>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(Vec::new());
    };
    let recoverable_current_records = authoritative_state.current_task_closure_results();
    let mut failures = authoritative_state
        .raw_current_task_closure_state_entries()
        .into_iter()
        .filter_map(|entry| match entry.task {
            Some(task_number)
                if recoverable_current_records
                    .get(&task_number)
                    .is_some_and(|record| {
                        validate_current_task_closure_record(context, record).is_ok()
                    }) =>
            {
                None
            }
            _ => current_task_closure_structural_failure_from_entry(context, entry),
        })
        .collect::<Vec<_>>();
    let structurally_invalid_tasks = failures
        .iter()
        .filter_map(|failure| failure.task)
        .collect::<std::collections::BTreeSet<_>>();
    failures.extend(
        recoverable_current_records
            .into_values()
            .filter(|record| !structurally_invalid_tasks.contains(&record.task))
            .filter_map(|record| {
                current_task_closure_structural_failure_from_record(context, record)
            }),
    );
    Ok(failures)
}

pub(crate) fn valid_current_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(valid_current_task_closure_records_from_authoritative_state(
        context,
        authoritative_state,
    ))
}

fn valid_current_task_closure_records_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Vec<CurrentTaskClosureRecord> {
    authoritative_state
        .current_task_closure_results()
        .into_values()
        .filter(|record| validate_current_task_closure_record(context, record).is_ok())
        .collect()
}

pub(crate) fn still_current_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(Vec::new());
    };
    still_current_task_closure_records_from_authoritative_state(context, authoritative_state)
}

pub(crate) fn still_current_task_closure_records_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    let mut records = Vec::new();
    for record in authoritative_state
        .current_task_closure_results()
        .into_values()
    {
        if validate_current_task_closure_record(context, &record).is_err() {
            continue;
        }
        if task_closure_matches_current_workspace(context, &record)? {
            records.push(record);
        }
    }
    Ok(records)
}

pub(crate) fn stale_current_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(Vec::new());
    };
    stale_current_task_closure_records_from_authoritative_state(context, authoritative_state)
}

pub(crate) fn stale_current_task_closure_records_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    Ok(
        valid_current_task_closure_records_from_authoritative_state(context, authoritative_state)
            .into_iter()
            .filter(|record| {
                matches!(
                    task_closure_matches_current_workspace(context, record),
                    Ok(false)
                )
            })
            .collect(),
    )
}

pub(crate) fn task_current_closure_status(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<TaskCurrentClosureStatus, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(TaskCurrentClosureStatus::Missing);
    };
    task_current_closure_status_from_authoritative_state(context, task_number, authoritative_state)
}

pub(crate) fn current_task_closure_overlay_restore_required(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    Ok(load_authoritative_transition_state(context)?
        .as_ref()
        .is_some_and(AuthoritativeTransitionState::current_task_closure_overlay_needs_restore))
}

pub(crate) fn task_current_closure_status_from_authoritative_state(
    context: &ExecutionContext,
    task_number: u32,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<TaskCurrentClosureStatus, JsonFailure> {
    let Some(current_closure) = authoritative_state.current_task_closure_result(task_number) else {
        if let Some(entry) = authoritative_state.raw_current_task_closure_state_entry(task_number) {
            return Err(invalid_current_task_closure_error_for_raw_entry(&entry));
        }
        return Ok(TaskCurrentClosureStatus::Missing);
    };
    validate_current_task_closure_record(context, &current_closure)?;
    if task_closure_matches_current_workspace(context, &current_closure)? {
        Ok(TaskCurrentClosureStatus::Current)
    } else {
        Ok(TaskCurrentClosureStatus::Stale)
    }
}

fn public_task_closure_from_record(
    record: CurrentTaskClosureRecord,
) -> PublicReviewStateTaskClosure {
    PublicReviewStateTaskClosure {
        task: record.task,
        closure_record_id: record.closure_record_id,
        reviewed_state_id: record.reviewed_state_id,
        contract_identity: record.contract_identity,
        effective_reviewed_surface_paths: record.effective_reviewed_surface_paths,
    }
}

fn invalid_current_task_closure_error_for_raw_entry(
    entry: &RawCurrentTaskClosureStateEntry,
) -> JsonFailure {
    match entry.task {
        Some(task_number) => task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {task_number} current task closure is malformed or missing authoritative provenance for the active approved plan."
            ),
        ),
        None => task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Current task-closure entry `{}` is malformed or not bound to a valid task for the active approved plan.",
                entry.scope_key
            ),
        ),
    }
}

fn current_task_closure_structural_failure_from_entry(
    context: &ExecutionContext,
    entry: RawCurrentTaskClosureStateEntry,
) -> Option<CurrentTaskClosureStructuralFailure> {
    let error = match entry.record.as_ref() {
        Some(record) => validate_current_task_closure_record(context, record).err()?,
        None => invalid_current_task_closure_error_for_raw_entry(&entry),
    };
    Some(CurrentTaskClosureStructuralFailure {
        task: entry.task,
        scope_key: entry.scope_key,
        closure_record_id: entry.closure_record_id,
        reason_code: task_boundary_reason_code_from_message(&error.message)
            .unwrap_or("prior_task_current_closure_invalid")
            .to_owned(),
        message: error.message,
    })
}

fn current_task_closure_structural_failure_from_record(
    context: &ExecutionContext,
    record: CurrentTaskClosureRecord,
) -> Option<CurrentTaskClosureStructuralFailure> {
    let error = validate_current_task_closure_record(context, &record).err()?;
    Some(CurrentTaskClosureStructuralFailure {
        task: Some(record.task),
        scope_key: format!("task-{}", record.task),
        closure_record_id: Some(record.closure_record_id),
        reason_code: task_boundary_reason_code_from_message(&error.message)
            .unwrap_or("prior_task_current_closure_invalid")
            .to_owned(),
        message: error.message,
    })
}

fn task_boundary_error(
    failure_class: FailureClass,
    reason_code: &str,
    message: impl Into<String>,
) -> JsonFailure {
    JsonFailure::new(failure_class, format!("{reason_code}: {}", message.into()))
}

fn push_reason_code_once(reason_codes: &mut Vec<String>, reason_code: &str) {
    if !reason_codes.iter().any(|existing| existing == reason_code) {
        reason_codes.push(reason_code.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(task: u32, closure_record_id: &str) -> CurrentTaskClosureRecord {
        CurrentTaskClosureRecord {
            task,
            dispatch_id: format!("dispatch-{task}"),
            closure_record_id: closure_record_id.to_owned(),
            reviewed_state_id: format!("git_tree:reviewed-{task}"),
            semantic_reviewed_state_id: None,
            review_result: String::from("pass"),
            review_summary_hash: String::from("review-hash"),
            verification_result: String::from("pass"),
            verification_summary_hash: String::from("verification-hash"),
            source_plan_path: Some(String::from("docs/featureforge/plans/plan.md")),
            source_plan_revision: Some(1),
            contract_identity: format!("contract-{task}"),
            execution_run_id: Some(String::from("run-1")),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
            closure_status: Some(String::from("current")),
        }
    }

    #[test]
    fn public_projection_preserves_current_closure_identity_without_extra_sources() {
        let projected = public_task_closure_from_record(record(2, "closure-task-2"));
        assert_eq!(projected.task, 2);
        assert_eq!(projected.closure_record_id, "closure-task-2");
        assert_eq!(projected.reviewed_state_id, "git_tree:reviewed-2");
        assert_eq!(projected.contract_identity, "contract-2");
        assert_eq!(projected.effective_reviewed_surface_paths, ["README.md"]);
    }

    #[test]
    fn repair_reason_projection_deduplicates_reason_codes() {
        let mut reason_codes = Vec::new();
        push_reason_code_once(&mut reason_codes, "prior_task_current_closure_stale");
        push_reason_code_once(&mut reason_codes, "prior_task_current_closure_stale");
        assert_eq!(reason_codes, ["prior_task_current_closure_stale"]);
    }
}
