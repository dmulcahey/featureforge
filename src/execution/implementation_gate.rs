use crate::contracts::plan::{
    PlanFidelityReviewReport, engineering_approval_fidelity_message,
    engineering_approval_fidelity_reason_codes, evaluate_plan_fidelity_review,
    is_engineering_approval_fidelity_reason_code, plan_fidelity_allows_implementation,
    plan_fidelity_verification_incomplete_report,
};
use crate::contracts::spec::parse_spec_source;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::context::ExecutionContext;
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::phase;
use crate::execution::status::PlanExecutionStatus;

pub(crate) fn apply_pre_execution_plan_fidelity_gate(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    let Some(gate) = pre_execution_plan_fidelity_block(context, status) else {
        return;
    };
    let reason_codes = engineering_approval_fidelity_reason_codes(&gate);
    for reason_code in &reason_codes {
        push_unique(&mut status.reason_codes, reason_code);
        push_unique(&mut status.blocking_reason_codes, reason_code);
    }

    status.phase = Some(String::from(phase::PHASE_PIVOT_REQUIRED));
    status.harness_phase = HarnessPhase::PivotRequired;
    status.phase_detail = String::from(phase::DETAIL_PLANNING_REENTRY_REQUIRED);
    status.state_kind = String::from("waiting_external_input");
    status.next_action = String::from("pivot / return to planning");
    status.review_state_status = String::from("clean");
    status.recording_context = None;
    status.execution_command_context = None;
    status.execution_reentry_target_source = None;
    status.public_repair_targets.clear();
    status.blocking_records.clear();
    status.blocking_scope = Some(String::from("workflow"));
    status.blocking_task = None;
    status.blocking_step = None;
    status.external_wait_state = None;
    status.next_public_action = None;
    status.blockers.clear();
    status.recommended_public_command = None;
    status.recommended_public_command_argv = None;
    status.required_inputs.clear();
    status.recommended_command = None;
}

pub(crate) fn pre_execution_plan_fidelity_failure(
    status: &PlanExecutionStatus,
) -> Option<JsonFailure> {
    if status.phase_detail != phase::DETAIL_PLANNING_REENTRY_REQUIRED {
        return None;
    }
    let primary_reason_code = status
        .reason_codes
        .iter()
        .chain(status.blocking_reason_codes.iter())
        .find(|code| is_engineering_approval_fidelity_reason_code(code))?;
    Some(JsonFailure::new(
        FailureClass::PlanNotExecutionReady,
        format!(
            "{} reason_code={primary_reason_code}; next_skill=featureforge:plan-eng-review",
            engineering_approval_fidelity_message(primary_reason_code),
        ),
    ))
}

fn pre_execution_plan_fidelity_block(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Option<PlanFidelityReviewReport> {
    if !pre_execution_plan_fidelity_gate_applies(context, status) {
        return None;
    }
    let gate = evaluate_context_plan_fidelity(context);
    (!plan_fidelity_allows_implementation(&gate)).then_some(gate)
}

fn pre_execution_plan_fidelity_gate_applies(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    // A plan can carry a persisted execution-mode header before any runtime
    // progress markers exist, so the first-entry gate must follow runtime
    // progress rather than the header value.
    context.plan_document.workflow_state == "Engineering Approved"
        && !implementation_entry_started(context, status)
}

pub(crate) fn implementation_entry_started(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    status.execution_started == "yes"
        || status.harness_phase == HarnessPhase::Executing
        || status.active_task.is_some()
        || status.active_step.is_some()
        || status.blocking_task.is_some()
        || status.blocking_step.is_some()
        || status.resume_task.is_some()
        || status.resume_step.is_some()
        || !context.evidence.attempts.is_empty()
        || !status.current_task_closures.is_empty()
        || (status.execution_run_id.is_some()
            && status.latest_authoritative_sequence > INITIAL_AUTHORITATIVE_SEQUENCE + 1)
}

fn evaluate_context_plan_fidelity(context: &ExecutionContext) -> PlanFidelityReviewReport {
    let spec = match parse_spec_source(
        &context.source_spec_path,
        context.source_spec_source.clone(),
    ) {
        Ok(spec) => spec,
        Err(_) => {
            return plan_fidelity_verification_incomplete_report(
                "Plan-fidelity review cannot be validated until the source spec parses cleanly, including a parseable Requirement Index.",
            );
        }
    };
    evaluate_plan_fidelity_review(&spec, &context.plan_document, &context.runtime.repo_root)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}
