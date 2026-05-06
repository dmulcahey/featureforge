use std::fmt::Write as _;

use crate::execution::{phase, state::PlanExecutionStatus};
use crate::workflow::operator::WorkflowDoctor;

const BLOCKER_LIMIT: usize = 3;
const WARNING_LIMIT: usize = 2;

pub(crate) fn render_doctor_dashboard(doctor: &WorkflowDoctor) -> String {
    let mut output = String::new();
    output.push_str("Workflow doctor\n\n");
    output.push_str("Header\n");
    write_row(&mut output, "Phase", &doctor.phase);
    write_row(&mut output, "Phase detail", &doctor.phase_detail);
    write_row(&mut output, "Review state", &doctor.review_state_status);
    write_row(&mut output, "Route status", &doctor.route_status);

    output.push_str("\nNext Move\n");
    write_row(&mut output, "Next action", &doctor.next_action);
    write_row(&mut output, "Next step", &doctor.next_step);
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
    if !doctor.next_skill.trim().is_empty() {
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
        append_limited_code_lines(
            &mut output,
            &blocker_codes,
            BLOCKER_LIMIT,
            "blockers",
            blocker_action_text,
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

fn reason_code_matches_parts(code: &str, parts: &[&str]) -> bool {
    let mut segments = code.split('_');
    parts.iter().all(|part| segments.next() == Some(*part)) && segments.next().is_none()
}

fn blocker_action_text(code: &str) -> &'static str {
    match code {
        "current_stale_closure_overlap" => {
            "Stop and inspect the runtime diagnostic before continuing."
        }
        "document_release_artifact_stale" => {
            "Run document-release for the current HEAD before final review."
        }
        "execution_reentry_target_missing" => {
            "Repair workflow routing before attempting execution reentry."
        }
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => "Dispatch the independent final reviewer.",
        "final_review_state_missing" => {
            "Dispatch or record final review for the current branch closure."
        }
        value if reason_code_matches_parts(value, &["plan", "fidelity", "receipt", "missing"]) => {
            "Run plan-fidelity-review for the current draft plan revision."
        }
        "prior_task_current_closure_missing" => {
            "Record or refresh the current task closure with review and verification evidence."
        }
        "prior_task_review_not_green" => {
            "Complete a passing independent task review before closure."
        }
        "recommended_mutation_command_rejected" => {
            "Follow the runtime diagnostic route instead of the rejected command."
        }
        "release_docs_state_missing" => {
            "Run document-release before final review or branch completion."
        }
        "task_closure_baseline_repair_candidate" => {
            "Follow the routed close-current-task or repair-review-state path."
        }
        "waiting_for_external_review_result" => {
            "Wait for the external review result, then rerun with external-review-result-ready."
        }
        _ => "Follow workflow operator guidance for this reason code.",
    }
}

fn warning_action_text(code: &str) -> &'static str {
    match code {
        "legacy_evidence_format" => "Refresh execution evidence when the runtime routes it.",
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
    use crate::workflow::doctor_resolution::DoctorResolution;
    use crate::workflow::status::WorkflowRoute;
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
            required_inputs: Vec::new(),
            resolution: DoctorResolution {
                kind: String::from("runtime_diagnostic_required"),
                stop_reasons: vec![String::from("bad\u{1b}[31m_code")],
                command_available: false,
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
