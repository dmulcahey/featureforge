use std::fmt::Write as _;

use crate::execution::closure_diagnostics::{
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
    TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN,
    TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
};
use crate::execution::review_route_tokens::{
    EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT, REASON_FINAL_REVIEW_STATE_MISSING,
    REASON_RELEASE_DOCS_STATE_MISSING,
};
use crate::execution::route_plan::external_wait_state_is_external_wait;
use crate::execution::status_support::public_typed_operator_route_contract;
use crate::execution::{phase, state::PlanExecutionStatus};
use crate::workflow::doctor_resolution::{
    KIND_PLANNING_REENTRY_REQUIRED, KIND_RUNTIME_DIAGNOSTIC_REQUIRED,
};
use crate::workflow::operator::{
    OperatorJsonGuidancePurpose, WorkflowDoctor, operator_json_external_review_wait_guidance,
    operator_json_rerun_guidance,
};

const BLOCKER_LIMIT: usize = 3;
const WARNING_LIMIT: usize = 2;
const DOCTOR_TYPED_OPERATOR_ROUTE_ACTION: &str = concat!(
    "Query workflow operator JSON. Use `--external-review-result-ready` only after an external review result exists. ",
    public_typed_operator_route_contract!(),
    "."
);
const DOCTOR_EXTERNAL_WAIT_OPERATOR_ROUTE_ACTION: &str = concat!(
    "Wait for the external review result, then run the JSON rerun command in this dashboard and ",
    public_typed_operator_route_contract!(),
    "."
);

pub(crate) fn render_doctor_dashboard(doctor: &WorkflowDoctor) -> String {
    render_doctor_dashboard_with_external_review_hint(doctor, false)
}

pub(crate) fn render_doctor_dashboard_with_external_review_hint(
    doctor: &WorkflowDoctor,
    external_review_result_ready: bool,
) -> String {
    let mut output = String::new();
    output.push_str("Workflow doctor\n\n");
    output.push_str("Header\n");
    write_row(&mut output, "Phase", &doctor.phase);
    write_row(&mut output, "Phase detail", &doctor.phase_detail);
    write_row(&mut output, "Review state", &doctor.review_state_status);
    write_row(&mut output, "Route status", &doctor.route_status);

    output.push_str("\nNext Move\n");
    write_row(&mut output, "Next action", &doctor.next_action);
    write_row(&mut output, "Next step", dashboard_next_step_text(doctor));
    write_row(&mut output, "Resolution kind", &doctor.resolution.kind);
    write_row(
        &mut output,
        "Command available",
        if doctor.resolution.command_available {
            "yes"
        } else {
            "no"
        },
    );
    write_row(
        &mut output,
        "Input contract available",
        if doctor.resolution.input_contract_available {
            "yes"
        } else {
            "no"
        },
    );
    let waiting_for_external_review_result = doctor_waiting_for_external_review_result(doctor);
    if doctor.resolution.command_available
        || doctor.resolution.input_contract_available
        || waiting_for_external_review_result
    {
        let json_rerun_guidance =
            if waiting_for_external_review_result && !external_review_result_ready {
                operator_json_external_review_wait_guidance(&doctor.plan_path)
            } else {
                operator_json_rerun_guidance(
                    &doctor.plan_path,
                    external_review_result_ready,
                    doctor_operator_json_guidance_purpose(doctor),
                )
            };
        write_row(&mut output, "JSON rerun", &json_rerun_guidance);
    }
    if !doctor.required_inputs.is_empty() {
        write_row(
            &mut output,
            "Required inputs",
            &required_input_names_text(doctor),
        );
    }
    if !doctor.next_skill.trim().is_empty() && !doctor_dashboard_text_uses_operator_route(doctor) {
        write_row(&mut output, "Next skill", &doctor.next_skill);
    }

    output.push_str("\nArtifacts\n");
    write_row(&mut output, "Spec", display_or_none(&doctor.spec_path));
    write_row(&mut output, "Plan", display_or_none(&doctor.plan_path));
    write_row(&mut output, "Contract state", &doctor.contract_state);

    if let Some(status) = doctor.execution_status.as_ref() {
        append_execution_section(&mut output, status);
    }

    let blocker_codes = dashboard_blocker_codes(doctor);
    if !blocker_codes.is_empty() {
        output.push_str("\nBlockers\n");
        let action_text = if doctor_resolution_is_runtime_diagnostic(doctor) {
            runtime_diagnostic_action_text
        } else if doctor_resolution_is_planning_reentry(doctor) {
            planning_reentry_action_text
        } else {
            blocker_action_text
        };
        append_limited_code_lines(
            &mut output,
            &blocker_codes,
            BLOCKER_LIMIT,
            "blockers",
            action_text,
        );
    }

    let warning_codes = dashboard_warning_codes(doctor);
    if !warning_codes.is_empty() {
        output.push_str("\nWarnings\n");
        append_limited_code_lines(
            &mut output,
            &warning_codes,
            WARNING_LIMIT,
            "warnings",
            warning_action_text,
        );
    }

    output
}

fn doctor_dashboard_text_uses_operator_route(doctor: &WorkflowDoctor) -> bool {
    contains_direct_late_stage_skill_instruction(&doctor.next_step)
        || contains_direct_late_stage_skill_instruction(&doctor.next_skill)
}

fn doctor_resolution_is_runtime_diagnostic(doctor: &WorkflowDoctor) -> bool {
    doctor.resolution.kind == KIND_RUNTIME_DIAGNOSTIC_REQUIRED
}

fn doctor_resolution_is_planning_reentry(doctor: &WorkflowDoctor) -> bool {
    doctor.resolution.kind == KIND_PLANNING_REENTRY_REQUIRED
}

fn doctor_operator_json_guidance_purpose(doctor: &WorkflowDoctor) -> OperatorJsonGuidancePurpose {
    if doctor.resolution.command_available || doctor.resolution.input_contract_available {
        return OperatorJsonGuidancePurpose::CommandExecutionAuthority;
    }
    if doctor_resolution_is_runtime_diagnostic(doctor) {
        return OperatorJsonGuidancePurpose::DiagnosticOrientation;
    }
    OperatorJsonGuidancePurpose::RouteOrientation
}

fn dashboard_next_step_text(doctor: &WorkflowDoctor) -> &str {
    if doctor_dashboard_text_uses_operator_route(doctor) {
        DOCTOR_TYPED_OPERATOR_ROUTE_ACTION
    } else {
        &doctor.next_step
    }
}

fn doctor_waiting_for_external_review_result(doctor: &WorkflowDoctor) -> bool {
    external_wait_state_is_external_wait(doctor.external_wait_state.as_deref())
        || doctor
            .resolution
            .stop_reasons
            .iter()
            .any(|reason| reason == EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT)
}

fn contains_direct_late_stage_skill_instruction(input: &str) -> bool {
    [
        "featureforge:document-release",
        "featureforge:requesting-code-review",
        "featureforge:qa-only",
        "featureforge:finishing-a-development-branch",
        "Run document-release",
        "Run requesting-code-review",
        "Run qa-only",
    ]
    .iter()
    .any(|needle| input.contains(needle))
}

fn append_execution_section(output: &mut String, status: &PlanExecutionStatus) {
    output.push_str("\nExecution\n");
    write_row(output, "Mode", &status.execution_mode);
    write_row(output, "Started", &status.execution_started);
    write_row(
        output,
        "Active task",
        &task_step_text(status.active_task, status.active_step),
    );
    write_row(
        output,
        "Blocking task",
        &task_step_text(status.blocking_task, status.blocking_step),
    );
    write_row(
        output,
        "Resume task",
        &task_step_text(status.resume_task, status.resume_step),
    );
}

fn dashboard_blocker_codes(doctor: &WorkflowDoctor) -> Vec<&str> {
    let primary = if doctor.resolution.stop_reasons.is_empty() {
        &doctor.blocking_reason_codes
    } else {
        &doctor.resolution.stop_reasons
    };
    ordered_unique_codes(
        primary
            .iter()
            .chain(doctor.blocking_reason_codes.iter())
            .chain(doctor.diagnostic_reason_codes.iter()),
    )
}

fn dashboard_warning_codes(doctor: &WorkflowDoctor) -> Vec<&str> {
    let mut codes = Vec::new();
    if let Some(status) = doctor.execution_status.as_ref() {
        codes.extend(status.warning_codes.iter());
    }
    if let Some(gate) = doctor.preflight.as_ref() {
        codes.extend(gate.warning_codes.iter());
    }
    if let Some(gate) = doctor.gate_review.as_ref() {
        codes.extend(gate.warning_codes.iter());
    }
    if let Some(gate) = doctor.gate_finish.as_ref() {
        codes.extend(gate.warning_codes.iter());
    }
    ordered_unique_codes(codes.into_iter())
}

fn ordered_unique_codes<'a>(codes: impl Iterator<Item = &'a String>) -> Vec<&'a str> {
    let mut ordered = Vec::new();
    for code in codes {
        let code = code.trim();
        if !code.is_empty() && !ordered.contains(&code) {
            ordered.push(code);
        }
    }
    ordered
}

fn append_limited_code_lines(
    output: &mut String,
    codes: &[&str],
    limit: usize,
    overflow_label: &str,
    action_text: fn(&str) -> &'static str,
) {
    for code in codes.iter().take(limit) {
        let _ = writeln!(
            output,
            "- {} - {}",
            sanitize_dashboard_text(code),
            sanitize_dashboard_text(action_text(code))
        );
    }
    if codes.len() > limit {
        let _ = writeln!(output, "+{} more {overflow_label}", codes.len() - limit);
    }
}

fn write_row(output: &mut String, label: &str, value: &str) {
    let _ = writeln!(output, "{label}: {}", sanitize_dashboard_text(value));
}

fn display_or_none(value: &str) -> &str {
    if value.trim().is_empty() {
        "none"
    } else {
        value
    }
}

fn task_step_text(task: Option<u32>, step: Option<u32>) -> String {
    match (task, step) {
        (Some(task), Some(step)) => format!("task-{task} step-{step}"),
        (Some(task), None) => format!("task-{task}"),
        _ => String::from("none"),
    }
}

fn required_input_names_text(doctor: &WorkflowDoctor) -> String {
    doctor
        .required_inputs
        .iter()
        .map(|input| input.name.as_str())
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn blocker_action_text(code: &str) -> &'static str {
    match code {
        "current_stale_closure_overlap" => {
            "Stop and inspect the runtime diagnostic before continuing."
        }
        "document_release_artifact_stale" => DOCTOR_TYPED_OPERATOR_ROUTE_ACTION,
        "execution_reentry_target_missing" => {
            "Stop and report this runtime diagnostic; do not invent runtime mutations or reconstruct artifacts manually."
        }
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => DOCTOR_TYPED_OPERATOR_ROUTE_ACTION,
        REASON_FINAL_REVIEW_STATE_MISSING => {
            "Query workflow operator JSON and follow its typed final-review route."
        }
        TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING => {
            "Use the routed close-current-task argv/template for the current task boundary."
        }
        TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN => {
            "Query workflow operator JSON; when it routes a review lane, complete that review and rerun operator before closure."
        }
        "recommended_mutation_command_rejected" => {
            "Follow the runtime diagnostic route instead of the rejected command."
        }
        REASON_RELEASE_DOCS_STATE_MISSING => DOCTOR_TYPED_OPERATOR_ROUTE_ACTION,
        TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE => {
            "Query workflow operator JSON and follow its typed task-boundary route."
        }
        EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT => DOCTOR_EXTERNAL_WAIT_OPERATOR_ROUTE_ACTION,
        _ => {
            "Query workflow operator JSON and follow its top-level typed argv/template route; stop if no executable surface is present."
        }
    }
}

fn runtime_diagnostic_action_text(_code: &str) -> &'static str {
    "Stop and inspect workflow/operator JSON diagnostics, especially blocking_reason_codes; do not invent runtime mutations or reconstruct artifacts manually."
}

fn planning_reentry_action_text(_code: &str) -> &'static str {
    "Return to the planning/review next step shown above; do not rerun runtime repair commands."
}

fn warning_action_text(code: &str) -> &'static str {
    match code {
        "legacy_evidence_format" => {
            "Treat legacy evidence as advisory and follow workflow operator typed argv."
        }
        "tracked_projection_stale" => {
            "Treat tracked projections as advisory and follow runtime state."
        }
        _ => "Review this non-blocking runtime warning.",
    }
}

fn sanitize_dashboard_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if character.is_control() {
            output.push(' ');
        } else {
            output.push(character);
        }
    }
    output.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::workflow::WorkflowRoute;
    use crate::workflow::doctor_resolution::DoctorResolution;
    use serde_json::Value;

    #[test]
    fn sanitize_dashboard_text_removes_terminal_control_sequences() {
        assert_eq!(
            sanitize_dashboard_text("spec/\u{1b}[31mred\u{1b}[0m\nnext"),
            "spec/red next"
        );
    }

    #[test]
    fn dashboard_text_sanitizes_runtime_strings_without_mutating_json_truth() {
        let doctor = WorkflowDoctor {
            schema_version: 3,
            phase: String::from("blocked"),
            phase_detail: String::from(phase::DETAIL_BLOCKED_RUNTIME_BUG),
            review_state_status: String::from("clean"),
            route_status: String::from("implementation_ready"),
            next_skill: String::new(),
            next_action: String::from("runtime diagnostic required"),
            next_step: String::from("Inspect \u{1b}[31mstate\u{1b}[0m\nnow"),
            recommended_command: None,
            recommended_public_command_argv: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            resolution: DoctorResolution {
                kind: String::from("runtime_diagnostic_required"),
                stop_reasons: vec![String::from("bad\u{1b}[31m_code")],
                command_available: false,
                input_contract_available: false,
            },
            diagnostic_reason_codes: Vec::new(),
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            spec_path: String::from("docs/spec-\u{1b}[31mred\u{1b}[0m.md"),
            plan_path: String::from("docs/plan.md"),
            contract_state: String::from("valid"),
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("implementation_ready"),
                next_skill: String::new(),
                spec_path: String::from("docs/spec-\u{1b}[31mred\u{1b}[0m.md"),
                plan_path: String::from("docs/plan.md"),
                contract_state: String::from("valid"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            runtime_provenance: None,
            self_hosting_warning: None,
            execution_status: None,
            plan_contract: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
        };

        let rendered = render_doctor_dashboard(&doctor);
        assert!(
            !rendered.contains('\u{1b}'),
            "text output must be inert: {rendered}"
        );
        assert!(
            rendered.contains("Next step: Inspect state now"),
            "text output should preserve readable sanitized semantics: {rendered}"
        );
        assert!(
            rendered.contains("Spec: docs/spec-red.md"),
            "text output should sanitize artifact paths: {rendered}"
        );

        let json = serde_json::to_value(&doctor).expect("doctor json should serialize");
        assert_eq!(
            json["spec_path"],
            Value::from("docs/spec-\u{1b}[31mred\u{1b}[0m.md"),
            "JSON mode must preserve runtime truth without text sanitization"
        );
        assert_eq!(
            json["next_step"],
            Value::from("Inspect \u{1b}[31mstate\u{1b}[0m\nnow"),
            "JSON mode must preserve raw next-step truth"
        );
    }

    #[test]
    fn dashboard_text_routes_late_stage_skill_instructions_through_operator_json() {
        let doctor = WorkflowDoctor {
            schema_version: 3,
            phase: String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING),
            phase_detail: String::from(phase::DETAIL_RELEASE_READINESS_RECORDING_READY),
            review_state_status: String::from("clean"),
            route_status: String::from("document_release_pending"),
            next_skill: String::from("featureforge:document-release"),
            next_action: String::from("advance late stage"),
            next_step: String::from(
                "Run featureforge:document-release and return with a fresh release-readiness artifact before branch completion.",
            ),
            recommended_command: None,
            recommended_public_command_argv: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            resolution: DoctorResolution {
                kind: String::from("input_required"),
                stop_reasons: Vec::new(),
                command_available: false,
                input_contract_available: true,
            },
            diagnostic_reason_codes: Vec::new(),
            blocking_scope: None,
            blocking_task: None,
            external_wait_state: None,
            blocking_reason_codes: Vec::new(),
            spec_path: String::from("docs/spec.md"),
            plan_path: String::from("docs/plan.md"),
            contract_state: String::from("valid"),
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("document_release_pending"),
                next_skill: String::from("featureforge:document-release"),
                spec_path: String::from("docs/spec.md"),
                plan_path: String::from("docs/plan.md"),
                contract_state: String::from("valid"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            runtime_provenance: None,
            self_hosting_warning: None,
            execution_status: None,
            plan_contract: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
        };

        let rendered = render_doctor_dashboard(&doctor);
        assert!(
            rendered.contains("recommended_public_command_argv")
                && rendered.contains("recommended_public_command_template.input_bindings")
                && rendered.contains("required_inputs")
                && rendered.contains("workflow operator JSON")
                && rendered.contains("--external-review-result-ready")
                && rendered.contains("stop and report the route diagnostic"),
            "late-stage text dashboard should render typed operator route guidance: {rendered}"
        );
        assert!(
            !rendered.contains("featureforge:document-release")
                && !rendered.contains("Next skill:")
                && !rendered.contains("Run featureforge:"),
            "late-stage text dashboard must not echo direct skill-chain instructions: {rendered}"
        );
    }

    #[test]
    fn dashboard_text_renders_external_wait_operator_json_rerun_contract() {
        let doctor = WorkflowDoctor {
            schema_version: 3,
            phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
            phase_detail: String::from(phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING),
            review_state_status: String::from("current"),
            route_status: String::from("final_review_pending"),
            next_skill: String::new(),
            next_action: String::from("wait for external review result"),
            next_step: String::from("Wait for the external final-review result."),
            recommended_command: None,
            recommended_public_command_argv: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            resolution: DoctorResolution {
                kind: String::from("waiting_external_input"),
                stop_reasons: vec![String::from(EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT)],
                command_available: false,
                input_contract_available: false,
            },
            diagnostic_reason_codes: Vec::new(),
            blocking_scope: Some(String::from("branch")),
            blocking_task: None,
            external_wait_state: Some(String::from(EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT)),
            blocking_reason_codes: Vec::new(),
            spec_path: String::from("docs/spec.md"),
            plan_path: String::from("docs/plan.md"),
            contract_state: String::from("valid"),
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("final_review_pending"),
                next_skill: String::new(),
                spec_path: String::from("docs/spec.md"),
                plan_path: String::from("docs/plan.md"),
                contract_state: String::from("valid"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::new(),
                reason: String::new(),
                note: String::new(),
            },
            runtime_provenance: None,
            self_hosting_warning: None,
            execution_status: None,
            plan_contract: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            task_review_dispatch_id: None,
            final_review_dispatch_id: Some(String::from("dispatch-final")),
        };

        let rendered = render_doctor_dashboard(&doctor);
        assert!(
            rendered.contains(
                "featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready --json"
            ) && rendered.contains("only after an external review result exists")
                && rendered.contains("recommended_public_command_argv")
                && rendered.contains("recommended_public_command_template.input_bindings")
                && rendered.contains("required_inputs"),
            "waiting-external dashboard should render the exact external-ready operator JSON route contract: {rendered}"
        );
        assert!(
            rendered.contains(
                "- waiting_for_external_review_result - Wait for the external review result, then run the JSON rerun command in this dashboard"
            ) && rendered.contains("recommended_public_command_argv")
                && rendered.contains("recommended_public_command_template.input_bindings")
                && rendered.contains("If neither executable surface is present, stop and report the route diagnostic"),
            "waiting-external blocker action should point back to the shared typed operator route contract: {rendered}"
        );
    }

    #[test]
    fn limited_code_lines_append_deterministic_overflow_summary() {
        let mut output = String::new();
        append_limited_code_lines(
            &mut output,
            &["one", "two", "three", "four"],
            3,
            "blockers",
            |_| "act",
        );

        assert_eq!(
            output,
            "- one - act\n- two - act\n- three - act\n+1 more blockers\n"
        );
    }

    #[test]
    fn limited_warning_lines_append_deterministic_overflow_summary() {
        let mut output = String::new();
        append_limited_code_lines(
            &mut output,
            &["one", "two", "three"],
            2,
            "warnings",
            |_| "warn",
        );

        assert_eq!(output, "- one - warn\n- two - warn\n+1 more warnings\n");
    }
}
