use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::execution::closure_diagnostics::task_boundary_projection_diagnostic_reason_code;
use crate::execution::route_plan::{
    STATE_KIND_ACTIONABLE_PUBLIC_COMMAND, STATE_KIND_PLANNING_REENTRY_REQUIRED,
    STATE_KIND_TERMINAL, STATE_KIND_WAITING_EXTERNAL_INPUT, external_wait_state_is_external_wait,
    state_kind_is_external_wait, state_kind_is_planning_reentry_required,
    state_kind_is_runtime_diagnostic, state_kind_is_terminal,
};

pub(crate) const KIND_ACTIONABLE_PUBLIC_COMMAND: &str = STATE_KIND_ACTIONABLE_PUBLIC_COMMAND;
pub(crate) const KIND_WAITING_EXTERNAL_INPUT: &str = STATE_KIND_WAITING_EXTERNAL_INPUT;
pub(crate) const KIND_PLANNING_REENTRY_REQUIRED: &str = STATE_KIND_PLANNING_REENTRY_REQUIRED;
pub(crate) const KIND_RUNTIME_DIAGNOSTIC_REQUIRED: &str = "runtime_diagnostic_required";
pub(crate) const KIND_TERMINAL: &str = STATE_KIND_TERMINAL;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DoctorResolution {
    #[schemars(with = "DoctorResolutionKindSchema")]
    pub kind: String,
    pub stop_reasons: Vec<String>,
    pub command_available: bool,
    pub input_contract_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum DoctorResolutionKindSchema {
    ActionablePublicCommand,
    WaitingExternalInput,
    PlanningReentryRequired,
    RuntimeDiagnosticRequired,
    Terminal,
}

pub(crate) struct DoctorResolutionInput<'a> {
    pub(crate) command_available: bool,
    pub(crate) required_input_count: usize,
    pub(crate) external_wait_state: Option<&'a str>,
    pub(crate) blocking_reason_codes: &'a [String],
    pub(crate) diagnostic_reason_codes: &'a [String],
    pub(crate) state_kind: &'a str,
}

pub(crate) fn derive_doctor_resolution(input: DoctorResolutionInput<'_>) -> DoctorResolution {
    if input.command_available {
        return DoctorResolution {
            kind: KIND_ACTIONABLE_PUBLIC_COMMAND.to_owned(),
            stop_reasons: Vec::new(),
            command_available: true,
            input_contract_available: false,
        };
    }

    if input.required_input_count > 0 {
        return DoctorResolution {
            kind: KIND_ACTIONABLE_PUBLIC_COMMAND.to_owned(),
            stop_reasons: Vec::new(),
            command_available: false,
            input_contract_available: true,
        };
    }

    if external_wait_state_is_external_wait(input.external_wait_state) {
        let wait_state =
            non_empty(input.external_wait_state).unwrap_or("waiting_for_external_review_result");
        return DoctorResolution {
            kind: KIND_WAITING_EXTERNAL_INPUT.to_owned(),
            stop_reasons: vec![wait_state.to_owned()],
            command_available: false,
            input_contract_available: false,
        };
    }

    if state_kind_is_planning_reentry_required(input.state_kind) {
        let stop_reasons = ordered_unique(
            input
                .blocking_reason_codes
                .iter()
                .chain(input.diagnostic_reason_codes.iter())
                .map(String::as_str),
        );
        return DoctorResolution {
            kind: KIND_PLANNING_REENTRY_REQUIRED.to_owned(),
            stop_reasons: non_empty_reasons(
                stop_reasons,
                input.state_kind,
                KIND_PLANNING_REENTRY_REQUIRED,
            ),
            command_available: false,
            input_contract_available: false,
        };
    }

    let diagnostic_reason_codes =
        ordered_unique(input.diagnostic_reason_codes.iter().map(String::as_str));
    let terminal_projection_diagnostics_only = state_kind_is_terminal(input.state_kind)
        && !diagnostic_reason_codes.is_empty()
        && diagnostic_reason_codes
            .iter()
            .all(|reason_code| task_boundary_projection_diagnostic_reason_code(reason_code));
    if (!diagnostic_reason_codes.is_empty() && !terminal_projection_diagnostics_only)
        || state_kind_is_runtime_diagnostic(input.state_kind)
        || state_kind_is_external_wait(input.state_kind)
    {
        let stop_reasons = ordered_unique(
            input
                .diagnostic_reason_codes
                .iter()
                .chain(input.blocking_reason_codes.iter())
                .map(String::as_str),
        );
        return DoctorResolution {
            kind: KIND_RUNTIME_DIAGNOSTIC_REQUIRED.to_owned(),
            stop_reasons: non_empty_reasons(
                stop_reasons,
                input.state_kind,
                KIND_RUNTIME_DIAGNOSTIC_REQUIRED,
            ),
            command_available: false,
            input_contract_available: false,
        };
    }

    let stop_reasons = ordered_unique(input.blocking_reason_codes.iter().map(String::as_str));
    DoctorResolution {
        kind: KIND_TERMINAL.to_owned(),
        stop_reasons: non_empty_reasons(stop_reasons, input.state_kind, KIND_TERMINAL),
        command_available: false,
        input_contract_available: false,
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|candidate| {
        let trimmed = candidate.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn non_empty_reasons(mut reasons: Vec<String>, state_kind: &str, fallback: &str) -> Vec<String> {
    if !reasons.is_empty() {
        return reasons;
    }
    if let Some(state_kind) = non_empty(Some(state_kind)) {
        reasons.push(state_kind.to_owned());
    } else {
        reasons.push(fallback.to_owned());
    }
    reasons
}

fn ordered_unique<'a>(codes: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut ordered = Vec::new();
    for code in codes {
        let Some(code) = non_empty(Some(code)) else {
            continue;
        };
        if !ordered.iter().any(|existing| existing == code) {
            ordered.push(code.to_owned());
        }
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn actionable_argv_masks_lower_precedence_stop_signals() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: true,
            required_input_count: 0,
            external_wait_state: Some("waiting_for_external_review_result"),
            blocking_reason_codes: &strings(&["blocking"]),
            diagnostic_reason_codes: &strings(&["diagnostic"]),
            state_kind: KIND_RUNTIME_DIAGNOSTIC_REQUIRED,
        });

        assert_eq!(resolution.kind, KIND_ACTIONABLE_PUBLIC_COMMAND);
        assert!(resolution.command_available);
        assert!(!resolution.input_contract_available);
        assert!(resolution.stop_reasons.is_empty());
    }

    #[test]
    fn required_inputs_are_actionable_without_command_availability() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 2,
            external_wait_state: Some("waiting_for_external_review_result"),
            blocking_reason_codes: &strings(&["blocking"]),
            diagnostic_reason_codes: &strings(&["diagnostic"]),
            state_kind: KIND_WAITING_EXTERNAL_INPUT,
        });

        assert_eq!(resolution.kind, KIND_ACTIONABLE_PUBLIC_COMMAND);
        assert!(!resolution.command_available);
        assert!(resolution.input_contract_available);
        assert!(resolution.stop_reasons.is_empty());
    }

    #[test]
    fn external_wait_uses_wait_state_as_stop_reason() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: Some("waiting_for_external_review_result"),
            blocking_reason_codes: &[],
            diagnostic_reason_codes: &[],
            state_kind: KIND_WAITING_EXTERNAL_INPUT,
        });

        assert_eq!(resolution.kind, KIND_WAITING_EXTERNAL_INPUT);
        assert_eq!(
            resolution.stop_reasons,
            ["waiting_for_external_review_result"]
        );
        assert!(!resolution.command_available);
    }

    #[test]
    fn diagnostic_reasons_preserve_canonical_order_and_deduplicate() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: None,
            blocking_reason_codes: &strings(&["code_b", "code_a"]),
            diagnostic_reason_codes: &strings(&["code_a", "code_c"]),
            state_kind: KIND_RUNTIME_DIAGNOSTIC_REQUIRED,
        });

        assert_eq!(resolution.kind, KIND_RUNTIME_DIAGNOSTIC_REQUIRED);
        assert_eq!(resolution.stop_reasons, ["code_a", "code_c", "code_b"]);
        assert!(!resolution.command_available);
    }

    #[test]
    fn terminal_projection_only_diagnostics_stay_terminal() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: None,
            blocking_reason_codes: &[],
            diagnostic_reason_codes: &strings(&[
                crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE,
            ]),
            state_kind: KIND_TERMINAL,
        });

        assert_eq!(resolution.kind, KIND_TERMINAL);
        assert_eq!(resolution.stop_reasons, [KIND_TERMINAL]);
        assert!(!resolution.command_available);
    }

    #[test]
    fn terminal_non_projection_diagnostics_still_require_runtime_diagnostic() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: None,
            blocking_reason_codes: &[],
            diagnostic_reason_codes: &strings(&["runtime_corruption"]),
            state_kind: KIND_TERMINAL,
        });

        assert_eq!(resolution.kind, KIND_RUNTIME_DIAGNOSTIC_REQUIRED);
        assert_eq!(resolution.stop_reasons, ["runtime_corruption"]);
        assert!(!resolution.command_available);
    }

    #[test]
    fn blocking_only_terminal_state_stays_terminal() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: None,
            blocking_reason_codes: &strings(&["blocking_a", "blocking_b"]),
            diagnostic_reason_codes: &[],
            state_kind: KIND_TERMINAL,
        });

        assert_eq!(resolution.kind, KIND_TERMINAL);
        assert_eq!(resolution.stop_reasons, ["blocking_a", "blocking_b"]);
        assert!(!resolution.command_available);
    }

    #[test]
    fn waiting_state_without_external_wait_signal_is_diagnostic_not_terminal() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: None,
            blocking_reason_codes: &[],
            diagnostic_reason_codes: &[],
            state_kind: KIND_WAITING_EXTERNAL_INPUT,
        });

        assert_eq!(resolution.kind, KIND_RUNTIME_DIAGNOSTIC_REQUIRED);
        assert_eq!(resolution.stop_reasons, [KIND_WAITING_EXTERNAL_INPUT]);
        assert!(!resolution.command_available);
    }

    #[test]
    fn planning_reentry_state_without_command_is_actionable_planning_not_diagnostic() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: None,
            blocking_reason_codes: &strings(&["missing_plan_fidelity_review_artifact"]),
            diagnostic_reason_codes: &strings(&["stale_plan_fidelity_review_artifact"]),
            state_kind: crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED,
        });

        assert_eq!(resolution.kind, KIND_PLANNING_REENTRY_REQUIRED);
        assert_eq!(
            resolution.stop_reasons,
            [
                "missing_plan_fidelity_review_artifact",
                "stale_plan_fidelity_review_artifact"
            ]
        );
        assert!(!resolution.command_available);
    }

    #[test]
    fn unrelated_external_wait_state_does_not_create_external_wait_resolution() {
        let resolution = derive_doctor_resolution(DoctorResolutionInput {
            command_available: false,
            required_input_count: 0,
            external_wait_state: Some("waiting_for_unregistered_signal"),
            blocking_reason_codes: &[],
            diagnostic_reason_codes: &[],
            state_kind: KIND_RUNTIME_DIAGNOSTIC_REQUIRED,
        });

        assert_eq!(resolution.kind, KIND_RUNTIME_DIAGNOSTIC_REQUIRED);
        assert_eq!(resolution.stop_reasons, [KIND_RUNTIME_DIAGNOSTIC_REQUIRED]);
        assert!(!resolution.command_available);
    }
}
