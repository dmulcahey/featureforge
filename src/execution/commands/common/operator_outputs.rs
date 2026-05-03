use super::*;

pub(in crate::execution::commands) fn current_workflow_operator(
    runtime: &ExecutionRuntime,
    plan: &Path,
    external_review_result_ready: bool,
) -> Result<ExecutionRoutingState, JsonFailure> {
    let (routing, _) =
        current_workflow_operator_with_runtime_state(runtime, plan, external_review_result_ready)?;
    Ok(routing)
}

pub(in crate::execution::commands) fn current_workflow_operator_with_runtime_state(
    runtime: &ExecutionRuntime,
    plan: &Path,
    external_review_result_ready: bool,
) -> Result<(ExecutionRoutingState, RuntimeState), JsonFailure> {
    // Mutators consume the execution-owned routing boundary here instead of calling
    // `query_workflow_routing_state_for_runtime` directly, but they still project the same
    // execution query contract through the shared router decision.
    let read_scope = load_execution_read_scope_for_mutation(runtime, plan, true)?;
    let (mut routing, route_decision, runtime_state) =
        project_runtime_routing_state_with_reduced_state(
            &read_scope,
            external_review_result_ready,
            false,
        )?;
    routing.phase = route_decision.phase;
    routing.phase_detail = route_decision.phase_detail;
    routing.review_state_status = route_decision.review_state_status;
    routing.next_action = route_decision.next_action;
    routing.recommended_public_command = route_decision.recommended_public_command;
    routing.recommended_command = route_decision.recommended_command;
    Ok((routing, runtime_state))
}

pub(in crate::execution::commands) fn negative_result_required_follow_up(
    runtime: &ExecutionRuntime,
    plan: &Path,
    operator_with_external_ready: &ExecutionRoutingState,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<String> {
    let task_negative_result_present = operator_with_external_ready
        .blocking_task
        .and_then(|task| {
            authoritative_state.and_then(|state| state.task_closure_negative_result(task))
        })
        .is_some();
    let current_branch_closure_id = operator_with_external_ready
        .current_branch_closure_id
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let current_final_review =
        authoritative_state.and_then(AuthoritativeTransitionState::current_final_review_record);
    let current_browser_qa =
        authoritative_state.and_then(AuthoritativeTransitionState::current_browser_qa_record);
    if shared_negative_result_requires_execution_reentry(
        task_negative_result_present,
        operator_with_external_ready.workflow_phase.as_str(),
        current_branch_closure_id,
        current_final_review
            .as_ref()
            .map(|record| record.branch_closure_id.as_str()),
        current_final_review
            .as_ref()
            .map(|record| record.result.as_str()),
        current_browser_qa
            .as_ref()
            .map(|record| record.branch_closure_id.as_str()),
        current_browser_qa
            .as_ref()
            .map(|record| record.result.as_str()),
    ) {
        return Some(String::from("execution_reentry"));
    }
    let _ = (runtime, plan);
    negative_result_follow_up(operator_with_external_ready)
}

pub(in crate::execution::commands) fn late_stage_negative_result_override_active(
    operator: &ExecutionRoutingState,
) -> bool {
    matches!(
        operator.phase_detail.as_str(),
        crate::execution::phase::DETAIL_HANDOFF_RECORDING_REQUIRED
            | crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED
    )
}

pub(in crate::execution::commands) fn workflow_operator_requery_public_command(
    plan: &Path,
    external_review_result_ready: bool,
) -> PublicCommand {
    PublicCommand::WorkflowOperator {
        plan: plan.display().to_string(),
        external_review_result_ready,
    }
}

pub(in crate::execution::commands) fn public_command_surfaces(
    command: &PublicCommand,
) -> (String, Vec<String>) {
    (command.to_display_command(), command.to_argv())
}

pub(in crate::execution::commands) fn optional_public_command_surfaces(
    command: Option<&PublicCommand>,
) -> (Option<String>, Option<Vec<String>>) {
    (
        recommended_public_command_display(command),
        recommended_public_command_argv(command),
    )
}

pub(in crate::execution::commands) fn workflow_operator_requery_surfaces(
    plan: &Path,
    external_review_result_ready: bool,
) -> (String, Vec<String>) {
    public_command_surfaces(&workflow_operator_requery_public_command(
        plan,
        external_review_result_ready,
    ))
}

pub(in crate::execution::commands) fn workflow_operator_requery_optional_surfaces(
    plan: &Path,
    external_review_result_ready: bool,
) -> (Option<String>, Option<Vec<String>>) {
    let (recommended_command, recommended_public_command_argv) =
        workflow_operator_requery_surfaces(plan, external_review_result_ready);
    (
        Some(recommended_command),
        Some(recommended_public_command_argv),
    )
}

pub(in crate::execution::commands) struct CloseCurrentTaskFollowUpRecommendation {
    pub(in crate::execution::commands) required_follow_up: Option<String>,
    pub(in crate::execution::commands) recommended_command: Option<String>,
    pub(in crate::execution::commands) recommended_public_command_argv: Option<Vec<String>>,
    pub(in crate::execution::commands) required_inputs: Vec<PublicCommandInputRequirement>,
}

fn close_current_task_public_command_surfaces(
    command: Option<&PublicCommand>,
) -> (
    Option<String>,
    Option<Vec<String>>,
    Vec<PublicCommandInputRequirement>,
) {
    let (recommended_command, recommended_public_command_argv) =
        optional_public_command_surfaces(command);
    (
        recommended_command,
        recommended_public_command_argv,
        required_inputs_for_public_command(command),
    )
}

pub(in crate::execution::commands) fn close_current_task_command_matches_follow_up(
    required_follow_up: Option<&str>,
    recommended_command: &PublicCommand,
) -> bool {
    match required_follow_up {
        Some("execution_reentry") => matches!(
            recommended_command,
            PublicCommand::Begin { .. }
                | PublicCommand::Reopen { .. }
                | PublicCommand::Complete { .. }
        ),
        Some("repair_review_state") => {
            matches!(recommended_command, PublicCommand::RepairReviewState { .. })
        }
        Some("request_external_review")
        | Some("wait_for_external_review_result")
        | Some("run_verification") => {
            matches!(recommended_command, PublicCommand::WorkflowOperator { .. })
        }
        Some("record_handoff") => matches!(
            recommended_command,
            PublicCommand::TransferHandoff { .. } | PublicCommand::TransferRepairStep { .. }
        ),
        Some("advance_late_stage") | Some("resolve_release_blocker") => {
            matches!(recommended_command, PublicCommand::AdvanceLateStage { .. })
        }
        Some(_) | None => false,
    }
}

pub(in crate::execution::commands) fn close_current_task_recommendation_for_follow_up(
    required_follow_up: Option<&str>,
    operator: &ExecutionRoutingState,
) -> (
    Option<String>,
    Option<Vec<String>>,
    Vec<PublicCommandInputRequirement>,
) {
    let recommended_public_command = required_follow_up.and_then(|follow_up| {
        operator
            .recommended_public_command
            .as_ref()
            .filter(|command| {
                close_current_task_command_matches_follow_up(Some(follow_up), command)
            })
    });
    close_current_task_public_command_surfaces(recommended_public_command)
}

pub(in crate::execution::commands) fn close_current_task_follow_up_and_command(
    operator: &ExecutionRoutingState,
) -> CloseCurrentTaskFollowUpRecommendation {
    let required_follow_up = close_current_task_required_follow_up(operator);
    let (recommended_command, recommended_public_command_argv, required_inputs) =
        close_current_task_recommendation_for_follow_up(required_follow_up.as_deref(), operator);
    CloseCurrentTaskFollowUpRecommendation {
        required_follow_up,
        recommended_command,
        recommended_public_command_argv,
        required_inputs,
    }
}

pub(in crate::execution::commands) fn with_close_current_task_operator_blocker_metadata(
    mut output: CloseCurrentTaskOutput,
    operator: &ExecutionRoutingState,
) -> CloseCurrentTaskOutput {
    output.blocking_scope = operator.blocking_scope.clone();
    output.blocking_task = operator.blocking_task;
    output.blocking_reason_codes = operator.blocking_reason_codes.clone();
    output.authoritative_next_action =
        close_current_task_public_command_surfaces(operator.recommended_public_command.as_ref()).0;
    output
}

#[cfg(test)]
pub(in crate::execution::commands) fn blocked_close_current_task_output_from_operator(
    task_number: u32,
    operator: &ExecutionRoutingState,
    trace_summary: &str,
) -> CloseCurrentTaskOutput {
    let follow_up = close_current_task_follow_up_and_command(operator);
    with_close_current_task_operator_blocker_metadata(
        CloseCurrentTaskOutput {
            action: String::from("blocked"),
            task_number,
            dispatch_validation_action: String::from("blocked"),
            closure_action: String::from("blocked"),
            task_closure_status: String::from("not_current"),
            superseded_task_closure_ids: Vec::new(),
            closure_record_id: None,
            code: None,
            recommended_command: follow_up.recommended_command,
            recommended_public_command_argv: follow_up.recommended_public_command_argv,
            required_inputs: follow_up.required_inputs,
            rederive_via_workflow_operator: None,
            required_follow_up: follow_up.required_follow_up,
            blocking_scope: None,
            blocking_task: None,
            blocking_reason_codes: Vec::new(),
            authoritative_next_action: None,
            trace_summary: trace_summary.to_owned(),
        },
        operator,
    )
}

pub(in crate::execution::commands) fn blocked_close_current_task_output(
    params: BlockedCloseCurrentTaskOutputContext<'_>,
) -> CloseCurrentTaskOutput {
    let BlockedCloseCurrentTaskOutputContext {
        task_number,
        dispatch_validation_action,
        task_closure_status,
        closure_record_id,
        code,
        recommended_command,
        recommended_public_command_argv,
        required_inputs,
        rederive_via_workflow_operator,
        required_follow_up,
        trace_summary,
    } = params;
    CloseCurrentTaskOutput {
        action: String::from("blocked"),
        task_number,
        dispatch_validation_action: dispatch_validation_action.to_owned(),
        closure_action: String::from("blocked"),
        task_closure_status: task_closure_status.to_owned(),
        superseded_task_closure_ids: Vec::new(),
        closure_record_id,
        code,
        recommended_command,
        recommended_public_command_argv,
        required_inputs,
        rederive_via_workflow_operator,
        required_follow_up,
        blocking_scope: None,
        blocking_task: None,
        blocking_reason_codes: Vec::new(),
        authoritative_next_action: None,
        trace_summary: trace_summary.to_owned(),
    }
}

pub(in crate::execution::commands) fn shared_out_of_phase_record_branch_closure_output(
    plan: &Path,
    branch_closure_id: Option<String>,
    trace_summary: &str,
) -> RecordBranchClosureOutput {
    let (recommended_command, recommended_public_command_argv) =
        workflow_operator_requery_surfaces(plan, false);
    RecordBranchClosureOutput {
        action: String::from("blocked"),
        branch_closure_id,
        code: Some(String::from("out_of_phase_requery_required")),
        recommended_command: Some(recommended_command),
        recommended_public_command_argv: Some(recommended_public_command_argv),
        required_inputs: Vec::new(),
        rederive_via_workflow_operator: Some(true),
        superseded_branch_closure_ids: Vec::new(),
        required_follow_up: None,
        trace_summary: trace_summary.to_owned(),
    }
}

pub(in crate::execution::commands) fn shared_out_of_phase_advance_late_stage_output(
    plan: &Path,
    params: AdvanceLateStageOutputContext<'_>,
) -> AdvanceLateStageOutput {
    let AdvanceLateStageOutputContext {
        stage_path,
        operation,
        branch_closure_id,
        dispatch_id,
        result,
        external_review_result_ready,
        trace_summary,
    } = params;
    let (recommended_command, recommended_public_command_argv) =
        workflow_operator_requery_surfaces(plan, external_review_result_ready);
    AdvanceLateStageOutput {
        action: String::from("blocked"),
        stage_path: stage_path.to_owned(),
        intent: String::from("advance_late_stage"),
        operation: operation.to_owned(),
        branch_closure_id,
        dispatch_id,
        result: result.to_owned(),
        code: Some(String::from("out_of_phase_requery_required")),
        recommended_command: Some(recommended_command),
        recommended_public_command_argv: Some(recommended_public_command_argv),
        required_inputs: Vec::new(),
        rederive_via_workflow_operator: Some(true),
        required_follow_up: None,
        trace_summary: trace_summary.to_owned(),
    }
}

pub(in crate::execution::commands) struct AdvanceLateStageOutputContext<'a> {
    pub(in crate::execution::commands) stage_path: &'a str,
    pub(in crate::execution::commands) operation: &'a str,
    pub(in crate::execution::commands) branch_closure_id: Option<String>,
    pub(in crate::execution::commands) dispatch_id: Option<String>,
    pub(in crate::execution::commands) result: &'a str,
    pub(in crate::execution::commands) external_review_result_ready: bool,
    pub(in crate::execution::commands) trace_summary: &'a str,
}

pub(in crate::execution::commands) fn advance_late_stage_follow_up_or_requery_output(
    operator: &ExecutionRoutingState,
    plan: &Path,
    dispatch_lineage_matches: bool,
    params: AdvanceLateStageOutputContext<'_>,
) -> AdvanceLateStageOutput {
    let AdvanceLateStageOutputContext {
        stage_path,
        operation,
        branch_closure_id,
        dispatch_id,
        result,
        external_review_result_ready,
        trace_summary,
    } = params;
    if let Some(required_follow_up) = late_stage_required_follow_up(stage_path, operator) {
        if stage_path == "final_review"
            && required_follow_up == "request_external_review"
            && dispatch_id.is_some()
            && !dispatch_lineage_matches
        {
            return shared_out_of_phase_advance_late_stage_output(
                plan,
                AdvanceLateStageOutputContext {
                    stage_path,
                    operation,
                    branch_closure_id,
                    dispatch_id,
                    result,
                    external_review_result_ready,
                    trace_summary,
                },
            );
        }
        return AdvanceLateStageOutput {
            action: String::from("blocked"),
            stage_path: stage_path.to_owned(),
            intent: String::from("advance_late_stage"),
            operation: operation.to_owned(),
            branch_closure_id,
            dispatch_id,
            result: result.to_owned(),
            code: None,
            recommended_command: None,
            recommended_public_command_argv: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
            required_follow_up: Some(required_follow_up),
            trace_summary: trace_summary.to_owned(),
        };
    }
    shared_out_of_phase_advance_late_stage_output(
        plan,
        AdvanceLateStageOutputContext {
            stage_path,
            operation,
            branch_closure_id,
            dispatch_id,
            result,
            external_review_result_ready,
            trace_summary,
        },
    )
}

pub(in crate::execution::commands) fn release_readiness_follow_up_or_requery_output(
    operator: &ExecutionRoutingState,
    plan: &Path,
    params: AdvanceLateStageOutputContext<'_>,
) -> AdvanceLateStageOutput {
    let AdvanceLateStageOutputContext {
        stage_path,
        operation,
        branch_closure_id,
        dispatch_id,
        result,
        external_review_result_ready,
        trace_summary,
    } = params;
    if let Some(required_follow_up) = release_readiness_required_follow_up(operator) {
        return AdvanceLateStageOutput {
            action: String::from("blocked"),
            stage_path: stage_path.to_owned(),
            intent: String::from("advance_late_stage"),
            operation: operation.to_owned(),
            branch_closure_id,
            dispatch_id,
            result: result.to_owned(),
            code: None,
            recommended_command: None,
            recommended_public_command_argv: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
            required_follow_up: Some(required_follow_up),
            trace_summary: trace_summary.to_owned(),
        };
    }
    shared_out_of_phase_advance_late_stage_output(
        plan,
        AdvanceLateStageOutputContext {
            stage_path,
            operation,
            branch_closure_id,
            dispatch_id,
            result,
            external_review_result_ready,
            trace_summary,
        },
    )
}

pub(in crate::execution::commands) fn qa_summary_hash(summary: &str) -> String {
    summary_hash(summary)
}

pub(in crate::execution::commands) fn deterministic_record_id(
    prefix: &str,
    parts: &[&str],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for part in parts {
        hasher.update(b"\n");
        hasher.update(part.as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{prefix}-{}", &digest[..16])
}
