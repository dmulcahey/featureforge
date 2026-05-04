use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use crate::diagnostics::JsonFailure;
use crate::execution::context::ExecutionContext;
use crate::execution::final_review::parse_artifact_document;
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::phase;
use crate::execution::status::PlanExecutionStatus;
use crate::paths::harness_authoritative_artifact_path;

pub(crate) const TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES: &[&str] = &[
    "prior_task_review_dispatch_missing",
    "prior_task_review_dispatch_stale",
    "prior_task_verification_missing",
    "prior_task_verification_missing_legacy",
    "task_review_not_independent",
    "task_review_artifact_malformed",
    "task_verification_summary_malformed",
];

pub(crate) fn task_boundary_projection_diagnostic_reason_code(reason_code: &str) -> bool {
    TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES.contains(&reason_code)
}

const PUBLIC_TASK_BOUNDARY_REASON_CODES: &[&str] = &[
    "prior_task_current_closure_missing",
    "prior_task_current_closure_stale",
    "prior_task_current_closure_invalid",
    "prior_task_current_closure_reviewed_state_malformed",
    "task_cycle_break_active",
    "current_task_closure_overlay_restore_required",
    "prior_task_review_not_green",
    "task_closure_baseline_repair_candidate",
    phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PublicTaskBoundaryDecision {
    pub task: Option<u32>,
    pub state: PublicTaskBoundaryState,
    pub public_reason_codes: Vec<String>,
    pub diagnostic_reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PublicTaskBoundaryState {
    Clean,
    CurrentClosureMissing,
    CurrentClosureStale,
    NegativeReviewCurrent,
    CycleBreakActive,
    OverlayRestoreRequired,
    TaskClosureRecordingReady,
    ExecutionReentryRequired,
}

pub(crate) fn public_task_boundary_reason_code(reason_code: &str) -> bool {
    PUBLIC_TASK_BOUNDARY_REASON_CODES.contains(&reason_code)
}

pub(crate) fn public_task_boundary_decision(
    status: &PlanExecutionStatus,
) -> PublicTaskBoundaryDecision {
    let task_scope = status.blocking_step.is_none()
        && (status.blocking_task.is_some()
            || status.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            || status.reason_codes.iter().any(|reason_code| {
                public_task_boundary_reason_code(reason_code)
                    || task_boundary_projection_diagnostic_reason_code(reason_code)
            }));
    if !task_scope {
        return PublicTaskBoundaryDecision {
            task: None,
            state: PublicTaskBoundaryState::Clean,
            public_reason_codes: Vec::new(),
            diagnostic_reason_codes: Vec::new(),
        };
    }

    let public_reason_codes =
        reason_codes_matching(&status.reason_codes, public_task_boundary_reason_code);
    let diagnostic_reason_codes = reason_codes_matching(
        &status.reason_codes,
        task_boundary_projection_diagnostic_reason_code,
    );
    let state = if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
    {
        PublicTaskBoundaryState::CycleBreakActive
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "current_task_closure_overlay_restore_required")
    {
        PublicTaskBoundaryState::OverlayRestoreRequired
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_review_not_green")
    {
        PublicTaskBoundaryState::NegativeReviewCurrent
    } else if public_reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
        )
    }) {
        PublicTaskBoundaryState::ExecutionReentryRequired
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_current_closure_stale")
    {
        PublicTaskBoundaryState::CurrentClosureStale
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_current_closure_missing")
    {
        PublicTaskBoundaryState::CurrentClosureMissing
    } else if status.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        || public_reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "task_closure_baseline_repair_candidate"
                    | phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            )
        })
    {
        PublicTaskBoundaryState::TaskClosureRecordingReady
    } else {
        PublicTaskBoundaryState::Clean
    };

    PublicTaskBoundaryDecision {
        task: status.blocking_task,
        state,
        public_reason_codes,
        diagnostic_reason_codes,
    }
}

pub(crate) fn apply_task_boundary_projection_diagnostics(status: &mut PlanExecutionStatus) {
    status.projection_diagnostics = public_task_boundary_decision(status).diagnostic_reason_codes;
}

pub(crate) fn merge_status_projection_diagnostics(
    mut diagnostic_reason_codes: Vec<String>,
    status: &PlanExecutionStatus,
) -> Vec<String> {
    for reason_code in &status.projection_diagnostics {
        if !diagnostic_reason_codes
            .iter()
            .any(|existing| existing == reason_code)
        {
            diagnostic_reason_codes.push(reason_code.clone());
        }
    }
    diagnostic_reason_codes
}

pub(crate) fn merge_task_boundary_projection_diagnostics(
    mut diagnostic_reason_codes: Vec<String>,
    status: &PlanExecutionStatus,
) -> Vec<String> {
    for reason_code in public_task_boundary_decision(status).diagnostic_reason_codes {
        if !diagnostic_reason_codes
            .iter()
            .any(|existing| existing == &reason_code)
        {
            diagnostic_reason_codes.push(reason_code);
        }
    }
    diagnostic_reason_codes
}

fn reason_codes_matching(reason_codes: &[String], predicate: fn(&str) -> bool) -> Vec<String> {
    let mut matched = Vec::new();
    for reason_code in reason_codes {
        if predicate(reason_code) && !matched.iter().any(|existing| existing == reason_code) {
            matched.push(reason_code.clone());
        }
    }
    matched
}

pub(crate) fn task_closure_recording_reason_code(reason_code: &str) -> bool {
    task_closure_recording_blocking_reason_code(reason_code)
        || task_boundary_projection_diagnostic_reason_code(reason_code)
}

pub(crate) fn task_closure_recording_status_reason_codes(
    blocking_reason_codes: &[String],
    diagnostic_reason_codes: &[String],
) -> Vec<String> {
    let mut reason_codes = Vec::new();
    for reason_code in blocking_reason_codes
        .iter()
        .chain(diagnostic_reason_codes.iter())
        .filter(|reason_code| task_closure_recording_reason_code(reason_code))
    {
        if !reason_codes.iter().any(|existing| existing == reason_code) {
            reason_codes.push(reason_code.clone());
        }
    }
    reason_codes
}

fn task_closure_recording_blocking_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        "prior_task_review_not_green" | "prior_task_current_closure_stale"
    )
}

pub(crate) fn task_closure_dispatch_lineage_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        "prior_task_review_dispatch_missing" | "prior_task_review_dispatch_stale"
    )
}

pub(crate) fn task_closure_recording_diagnostic_reason_codes(
    task: u32,
    dispatch_id: Option<&str>,
    current_semantic_reviewed_state_id: Option<&str>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Vec<String> {
    let mut diagnostic_reason_codes = Vec::new();
    let lineage_key = format!("task-{task}");
    let lineage_record =
        overlay.and_then(|overlay| overlay.strategy_review_dispatch_lineage.get(&lineage_key));
    let lineage_dispatch_id = lineage_record
        .and_then(|record| record.dispatch_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let lineage_semantic_reviewed_state_id = lineage_record
        .and_then(|record| record.semantic_reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let dispatch_id = dispatch_id.map(str::trim).filter(|value| !value.is_empty());
    let current_semantic_reviewed_state_id = current_semantic_reviewed_state_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let lineage_reviewed_state_matches = current_semantic_reviewed_state_id
        .zip(lineage_semantic_reviewed_state_id)
        .is_some_and(|(current, lineage)| current == lineage);
    match (dispatch_id, lineage_dispatch_id) {
        (None, None) => {
            push_task_closure_diagnostic_reason_code_once(
                &mut diagnostic_reason_codes,
                "prior_task_review_dispatch_missing",
            );
        }
        (None, Some(_)) | (Some(_), None) => {
            push_task_closure_diagnostic_reason_code_once(
                &mut diagnostic_reason_codes,
                "prior_task_review_dispatch_stale",
            );
        }
        (Some(dispatch_id), Some(lineage_dispatch_id)) if dispatch_id != lineage_dispatch_id => {
            push_task_closure_diagnostic_reason_code_once(
                &mut diagnostic_reason_codes,
                "prior_task_review_dispatch_stale",
            );
        }
        _ => {}
    }
    if dispatch_id.is_some() && !lineage_reviewed_state_matches {
        push_task_closure_diagnostic_reason_code_once(
            &mut diagnostic_reason_codes,
            "prior_task_review_dispatch_stale",
        );
    }
    diagnostic_reason_codes
}

pub(crate) fn push_task_closure_diagnostic_reason_code_once(
    reason_codes: &mut Vec<String>,
    reason_code: &str,
) {
    if !reason_codes.iter().any(|existing| existing == reason_code) {
        reason_codes.push(reason_code.to_owned());
    }
}

pub(crate) fn push_task_closure_pending_verification_reason_codes_for_run(
    context: &ExecutionContext,
    task: u32,
    execution_run_id: &str,
    treat_missing_review_receipts_as_malformed: bool,
    diagnostic_reason_codes: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let mut any_review_receipt_present = false;
    let mut all_review_receipts_valid = true;

    for step_state in context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == task && step_state.checked)
    {
        let review_receipt_path = authoritative_unit_review_receipt_path(
            context,
            execution_run_id,
            task,
            step_state.step_number,
        );
        let review_receipt_metadata = match fs::symlink_metadata(&review_receipt_path) {
            Ok(metadata) => {
                any_review_receipt_present = true;
                metadata
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                all_review_receipts_valid = false;
                if treat_missing_review_receipts_as_malformed {
                    push_task_closure_diagnostic_reason_code_once(
                        diagnostic_reason_codes,
                        "task_review_artifact_malformed",
                    );
                }
                continue;
            }
            Err(_) => {
                any_review_receipt_present = true;
                all_review_receipts_valid = false;
                push_task_closure_diagnostic_reason_code_once(
                    diagnostic_reason_codes,
                    "task_review_artifact_malformed",
                );
                continue;
            }
        };
        if review_receipt_metadata.file_type().is_symlink() || !review_receipt_metadata.is_file() {
            all_review_receipts_valid = false;
            push_task_closure_diagnostic_reason_code_once(
                diagnostic_reason_codes,
                "task_review_artifact_malformed",
            );
            continue;
        }
        let review_document = parse_artifact_document(&review_receipt_path);
        if review_document.title.as_deref() != Some("# Unit Review Result")
            || review_document
                .headers
                .get("Review Stage")
                .map(String::as_str)
                != Some("featureforge:unit-review")
            || review_document.headers.get("Result").map(String::as_str) != Some("pass")
            || review_document
                .headers
                .get("Generated By")
                .map(String::as_str)
                != Some("featureforge:unit-review")
        {
            all_review_receipts_valid = false;
            push_task_closure_diagnostic_reason_code_once(
                diagnostic_reason_codes,
                "task_review_artifact_malformed",
            );
            continue;
        }
        match review_document
            .headers
            .get("Reviewer Provenance")
            .map(String::as_str)
        {
            Some("dedicated-independent") => {}
            Some(_) => {
                all_review_receipts_valid = false;
                push_task_closure_diagnostic_reason_code_once(
                    diagnostic_reason_codes,
                    "task_review_not_independent",
                );
            }
            None => {
                all_review_receipts_valid = false;
                push_task_closure_diagnostic_reason_code_once(
                    diagnostic_reason_codes,
                    "task_review_artifact_malformed",
                );
            }
        }
    }

    if !any_review_receipt_present || !all_review_receipts_valid {
        return Ok(());
    }

    let verification_receipt_path =
        authoritative_task_verification_receipt_path(context, execution_run_id, task);
    let verification_receipt_metadata = match fs::symlink_metadata(&verification_receipt_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            push_task_closure_diagnostic_reason_code_once(
                diagnostic_reason_codes,
                "prior_task_verification_missing",
            );
            None
        }
        Err(_) => {
            push_task_closure_diagnostic_reason_code_once(
                diagnostic_reason_codes,
                "task_verification_summary_malformed",
            );
            None
        }
    };
    if let Some(verification_receipt_metadata) = verification_receipt_metadata {
        if verification_receipt_metadata.file_type().is_symlink()
            || !verification_receipt_metadata.is_file()
        {
            push_task_closure_diagnostic_reason_code_once(
                diagnostic_reason_codes,
                "task_verification_summary_malformed",
            );
        } else {
            let verification_document = parse_artifact_document(&verification_receipt_path);
            let task_header_matches = verification_document
                .headers
                .get("Task Number")
                .and_then(|value| value.trim().parse::<u32>().ok())
                == Some(task);
            if verification_document.title.as_deref() != Some("# Task Verification Result")
                || verification_document
                    .headers
                    .get("Result")
                    .map(String::as_str)
                    != Some("pass")
                || verification_document
                    .headers
                    .get("Generated By")
                    .map(String::as_str)
                    != Some("featureforge:verification-before-completion")
                || verification_document
                    .headers
                    .get("Execution Run ID")
                    .map(String::as_str)
                    != Some(execution_run_id)
                || verification_document
                    .headers
                    .get("Source Plan")
                    .map(String::as_str)
                    != Some(context.plan_rel.as_str())
                || !task_header_matches
            {
                push_task_closure_diagnostic_reason_code_once(
                    diagnostic_reason_codes,
                    "task_verification_summary_malformed",
                );
            }
        }
    }

    Ok(())
}

pub(crate) fn authoritative_unit_review_receipt_path(
    context: &ExecutionContext,
    execution_run_id: &str,
    task_number: u32,
    step_number: u32,
) -> PathBuf {
    harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        &format!("unit-review-{execution_run_id}-task-{task_number}-step-{step_number}.md"),
    )
}

fn authoritative_task_verification_receipt_path(
    context: &ExecutionContext,
    execution_run_id: &str,
    task_number: u32,
) -> PathBuf {
    harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        &format!("task-verification-{execution_run_id}-task-{task_number}.md"),
    )
}
