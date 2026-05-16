use schemars::JsonSchema;
use serde::Serialize;

use crate::contracts::plan::{
    PlanFidelityReviewReport, is_engineering_approval_fidelity_reason_code,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowRoute {
    pub schema_version: u32,
    pub status: String,
    pub next_skill: String,
    pub spec_path: String,
    pub plan_path: String,
    pub contract_state: String,
    pub reason_codes: Vec<String>,
    pub diagnostics: Vec<WorkflowDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_fidelity_review: Option<PlanFidelityReviewReport>,
    pub scan_truncated: bool,
    pub spec_candidate_count: usize,
    pub plan_candidate_count: usize,
    pub manifest_path: String,
    pub root: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl WorkflowRoute {
    pub fn is_engineering_approval_fidelity_blocked(&self) -> bool {
        self.status == "plan_review_required"
            && self.next_skill == "featureforge:plan-eng-review"
            && self
                .reason_codes
                .iter()
                .any(|code| is_engineering_approval_fidelity_reason_code(code))
    }
}

pub fn route_is_engineering_approval_fidelity_blocked(route: &WorkflowRoute) -> bool {
    route.is_engineering_approval_fidelity_blocked()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowDiagnostic {
    pub code: String,
    pub severity: String,
    pub artifact: String,
    pub message: String,
    pub remediation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowPhase {
    pub schema_version: u32,
    pub phase: String,
    pub route_status: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub next_skill: String,
    pub next_step: String,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason_family: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_reason_codes: Vec<String>,
    pub spec_path: String,
    pub plan_path: String,
    pub route: WorkflowRoute,
}
