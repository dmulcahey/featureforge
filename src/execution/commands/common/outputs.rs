use super::*;

pub(in crate::execution::commands) const TASK_CLOSURE_RECORDED_TRACE: &str =
    "Recorded or refreshed the current task closure from authoritative review state.";
pub(in crate::execution::commands) const ALREADY_CURRENT_TASK_CLOSURE_RECORDED_TRACE: &str =
    "Current task closure is already current for this reviewed state.";

#[derive(Debug, Clone, Serialize)]
pub struct CloseCurrentTaskOutput {
    pub action: String,
    pub task_number: u32,
    pub dispatch_validation_action: String,
    pub closure_action: String,
    pub task_closure_status: String,
    pub superseded_task_closure_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closure_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authoritative_next_action: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterializeProjectionsOutput {
    pub action: String,
    pub projection_mode: String,
    pub written_paths: Vec<String>,
    pub runtime_truth_changed: bool,
    pub trace_summary: String,
}

pub(in crate::execution::commands) const INTERNAL_EXECUTION_FLAGS_ENV: &str =
    "FEATUREFORGE_ALLOW_INTERNAL_EXECUTION_FLAGS";

pub(in crate::execution::commands) fn internal_execution_flags_enabled() -> bool {
    std::env::var(INTERNAL_EXECUTION_FLAGS_ENV).as_deref() == Ok("1")
}

pub(in crate::execution::commands) fn require_internal_execution_flag_allowed(
    flag: &str,
    command: &str,
) -> Result<(), JsonFailure> {
    if internal_execution_flags_enabled() {
        return Ok(());
    }
    Err(JsonFailure::new(
        FailureClass::InvalidCommandInput,
        format!(
            "{flag} is an internal compatibility flag and is not available in normal public execution. Run {command} without it."
        ),
    ))
}

pub(in crate::execution::commands) fn require_close_current_task_public_flags(
    args: &CloseCurrentTaskArgs,
) -> Result<(), JsonFailure> {
    if args.dispatch_id.is_some() {
        require_internal_execution_flag_allowed("--dispatch-id", "close-current-task")?;
    }
    Ok(())
}

pub(in crate::execution::commands) fn require_advance_late_stage_public_flags(
    args: &AdvanceLateStageArgs,
) -> Result<(), JsonFailure> {
    if args.dispatch_id.is_some() {
        require_internal_execution_flag_allowed("--dispatch-id", "advance-late-stage")?;
    }
    if args.branch_closure_id.is_some() {
        require_internal_execution_flag_allowed("--branch-closure-id", "advance-late-stage")?;
    }
    Ok(())
}

pub(in crate::execution::commands) fn close_current_task_already_current_output(
    task_number: u32,
    closure_record_id: String,
    trace_summary: &str,
    mut reason_codes: Vec<String>,
) -> CloseCurrentTaskOutput {
    reason_codes.sort();
    reason_codes.dedup();
    CloseCurrentTaskOutput {
        action: String::from("already_current"),
        task_number,
        dispatch_validation_action: String::from("validated"),
        closure_action: String::from("already_current"),
        task_closure_status: String::from("current"),
        superseded_task_closure_ids: Vec::new(),
        closure_record_id: Some(closure_record_id),
        code: None,
        recommended_command: None,
        recommended_public_command_argv: None,
        rederive_via_workflow_operator: None,
        required_follow_up: None,
        blocking_scope: None,
        blocking_task: None,
        blocking_reason_codes: reason_codes,
        authoritative_next_action: None,
        trace_summary: String::from(trace_summary),
    }
}

pub(in crate::execution::commands) fn resolve_already_current_task_closure_postconditions(
    context: &ExecutionContext,
    authoritative_state: &mut AuthoritativeTransitionState,
    task_number: u32,
    closure_record_id: &str,
) -> Result<Vec<String>, JsonFailure> {
    if resolve_current_task_closure_postconditions_for_current_workspace(
        context,
        authoritative_state,
        task_number,
        Some(closure_record_id),
    )?
    .is_some()
    {
        authoritative_state
            .persist_if_dirty_with_failpoint_and_command(None, "close_current_task")?;
        return Ok(vec![String::from(
            "current_task_closure_postconditions_resolved",
        )]);
    }
    Ok(Vec::new())
}

pub(in crate::execution::commands) fn current_positive_closure_matches_incoming_results(
    current_record: &CurrentTaskClosureRecord,
    review_result: &str,
    verification_result: &str,
) -> bool {
    current_record.review_result == "pass"
        && current_record.verification_result == "pass"
        && review_result == "pass"
        && verification_result == "pass"
        && current_record
            .closure_status
            .as_deref()
            .is_none_or(|status| status == "current")
}

pub(in crate::execution::commands) fn task_closure_negative_result_blocks_reviewed_state(
    authoritative_state: &AuthoritativeTransitionState,
    task: u32,
    reviewed_state_id: &str,
) -> bool {
    authoritative_state
        .task_closure_negative_result(task)
        .is_some_and(|negative_record| {
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_record
                    .semantic_reviewed_state_id
                    .as_deref()
                    .unwrap_or(negative_record.reviewed_state_id.as_str()),
                Some(reviewed_state_id),
            )
        })
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordBranchClosureOutput {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    pub superseded_branch_closure_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvanceLateStageOutput {
    pub action: String,
    pub stage_path: String,
    pub delegated_primitive: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordQaOutput {
    pub action: String,
    pub branch_closure_id: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_follow_up: Option<String>,
    pub trace_summary: String,
}

pub(in crate::execution::commands) struct CurrentFinalReviewAuthorityCheck<'a> {
    pub(in crate::execution::commands) branch_closure_id: &'a str,
    pub(in crate::execution::commands) dispatch_id: &'a str,
    pub(in crate::execution::commands) reviewer_source: &'a str,
    pub(in crate::execution::commands) reviewer_id: &'a str,
    pub(in crate::execution::commands) result: &'a str,
    pub(in crate::execution::commands) normalized_summary_hash: &'a str,
}

pub(in crate::execution::commands) struct EquivalentFinalReviewRerunParams<'a> {
    pub(in crate::execution::commands) stage_path: &'a str,
    pub(in crate::execution::commands) delegated_primitive: &'a str,
    pub(in crate::execution::commands) dispatch_id: &'a str,
    pub(in crate::execution::commands) reviewer_source: &'a str,
    pub(in crate::execution::commands) reviewer_id: &'a str,
    pub(in crate::execution::commands) result: &'a str,
    pub(in crate::execution::commands) summary_file: &'a Path,
    pub(in crate::execution::commands) required_follow_up: Option<String>,
}

pub(in crate::execution::commands) struct ResolvedFinalReviewEvidence {
    pub(in crate::execution::commands) base_branch: String,
    pub(in crate::execution::commands) deviations_required: bool,
}

pub(in crate::execution::commands) struct BlockedCloseCurrentTaskOutputContext<'a> {
    pub(in crate::execution::commands) task_number: u32,
    pub(in crate::execution::commands) dispatch_validation_action: &'a str,
    pub(in crate::execution::commands) task_closure_status: &'a str,
    pub(in crate::execution::commands) closure_record_id: Option<String>,
    pub(in crate::execution::commands) code: Option<String>,
    pub(in crate::execution::commands) recommended_command: Option<String>,
    pub(in crate::execution::commands) recommended_public_command_argv: Option<Vec<String>>,
    pub(in crate::execution::commands) rederive_via_workflow_operator: Option<bool>,
    pub(in crate::execution::commands) required_follow_up: Option<String>,
    pub(in crate::execution::commands) trace_summary: &'a str,
}
