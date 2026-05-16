use std::fs;
use std::path::Path;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::contracts::plan::PLAN_QA_REQUIREMENT_VALUES;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::command_eligibility::{
    PUBLIC_EXECUTION_COMMAND_KIND_VALUES, PUBLIC_REPAIR_TARGET_COMMAND_KIND_VALUES, PublicCommand,
    PublicCommandInputRequirement,
};
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, ChunkingStrategy, DownstreamFreshnessState,
    EvaluationVerdict, EvaluatorKind, EvaluatorPolicyName, ExecutionRunId, HarnessPhase,
    ResetPolicy,
};
use crate::execution::next_action::PUBLIC_NEXT_ACTION_VALUES;
use crate::execution::phase::{
    DETAIL_FINAL_REVIEW_RECORDING_READY, DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
    DETAIL_RELEASE_READINESS_RECORDING_READY, DETAIL_TASK_CLOSURE_RECORDING_READY, PHASE_EXECUTING,
    PLAN_EXECUTION_STATUS_PHASE_DETAIL_VALUES, PUBLIC_STATUS_PHASE_VALUES,
    RECORDING_CONTEXT_PHASE_DETAILS,
};
use crate::execution::public_command_types::{
    RecommendedPublicCommandArgv, RecommendedPublicCommandTemplate,
};
use crate::execution::review_route_tokens::{
    PUBLIC_REVIEW_STATE_STATUS_VALUES, REQUIRED_FOLLOW_UP_SCHEMA_VALUES,
};
use crate::execution::route_plan::PUBLIC_STATE_KIND_VALUES;
use crate::execution::route_plan::{
    Blocker as RuntimeBlocker, NextPublicAction as RuntimeNextPublicAction,
};
use crate::execution::runtime_provenance::RuntimeProvenance;

pub const REQUIRED_FOLLOW_UP_SCHEMA_DESCRIPTION: &str =
    "Required follow-up intent token; record_handoff is compatibility metadata, not executable.";
pub const PUBLIC_COMMAND_TEMPLATE_KIND_SCHEMA_DESCRIPTION: &str =
    "Public command intent label for a template; not executable by itself.";
pub const RECOMMENDED_COMMAND_SCHEMA_DESCRIPTION: &str =
    "Display-only compatibility summary; not executable.";
pub const RECOMMENDED_PUBLIC_COMMAND_ARGV_SCHEMA_DESCRIPTION: &str =
    "Executable public command argv when present.";
pub const PLAN_EXECUTION_STATUS_RECOMMENDED_PUBLIC_COMMAND_ARGV_SCHEMA_DESCRIPTION: &str = "Diagnostic mirror of operator-derived public command argv when present; workflow operator JSON remains the normal executable route authority.";
pub const RECOMMENDED_PUBLIC_COMMAND_TEMPLATE_SCHEMA_DESCRIPTION: &str =
    "Non-executable public command template for input-required routes.";
pub const REQUIRED_INPUTS_SCHEMA_DESCRIPTION: &str =
    "Input names required to materialize a public command template.";
pub const NEXT_ACTION_SCHEMA_DESCRIPTION: &str =
    "Display-only route diagnostic label; not executable.";
pub const NEXT_PUBLIC_ACTION_DISPLAY_ONLY_SCHEMA_DESCRIPTION: &str =
    "Legacy compatibility marker; nested command fields are display-only.";
pub const NEXT_PUBLIC_ACTION_COMMAND_SCHEMA_DESCRIPTION: &str =
    "Display-only legacy command summary; not executable.";
pub const NEXT_PUBLIC_ACTION_ARGS_TEMPLATE_SCHEMA_DESCRIPTION: &str =
    "Display-only legacy argument template summary; not executable.";
pub const ROUTE_NEXT_PUBLIC_ACTION_SCHEMA_DESCRIPTION: &str =
    "Optional display-only public route action summary; not executable.";
pub const BLOCKER_NEXT_PUBLIC_ACTION_SCHEMA_DESCRIPTION: &str =
    "Optional display-only blocker action summary; not executable.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanExecutionStatus {
    #[schemars(range(min = 3, max = 3))]
    pub schema_version: u32,
    pub plan_revision: u32,
    pub execution_run_id: Option<ExecutionRunId>,
    #[serde(skip_serializing)]
    #[schemars(skip)]
    pub workspace_state_id: String,
    pub current_branch_reviewed_state_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    #[serde(skip_serializing)]
    #[schemars(skip)]
    pub current_branch_meaningful_drift: bool,
    pub current_task_closures: Vec<PublicReviewStateTaskClosure>,
    pub superseded_closures_summary: Vec<String>,
    pub stale_unreviewed_closures: Vec<String>,
    pub current_release_readiness_state: Option<String>,
    pub current_final_review_state: String,
    pub current_qa_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_final_review_branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_final_review_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_qa_branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_qa_result: Option<String>,
    pub qa_requirement: Option<String>,
    pub latest_authoritative_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    // Keep the DTO attribute compatible with the pre-extraction status type.
    // Task 8's explicit schema baseline is injected in write_plan_execution_schema.
    #[schemars(skip)]
    pub phase: Option<String>,
    pub harness_phase: HarnessPhase,
    pub chunk_id: ChunkId,
    pub chunking_strategy: Option<ChunkingStrategy>,
    pub evaluator_policy: Option<EvaluatorPolicyName>,
    pub reset_policy: Option<ResetPolicy>,
    pub review_stack: Option<Vec<String>>,
    pub active_contract_path: Option<String>,
    pub active_contract_fingerprint: Option<String>,
    pub required_evaluator_kinds: Vec<EvaluatorKind>,
    pub completed_evaluator_kinds: Vec<EvaluatorKind>,
    pub pending_evaluator_kinds: Vec<EvaluatorKind>,
    pub non_passing_evaluator_kinds: Vec<EvaluatorKind>,
    pub aggregate_evaluation_state: AggregateEvaluationState,
    pub last_evaluation_report_path: Option<String>,
    pub last_evaluation_report_fingerprint: Option<String>,
    pub last_evaluation_evaluator_kind: Option<EvaluatorKind>,
    pub last_evaluation_verdict: Option<EvaluationVerdict>,
    pub current_chunk_retry_count: u32,
    pub current_chunk_retry_budget: u32,
    pub current_chunk_pivot_threshold: u32,
    pub handoff_required: bool,
    pub open_failed_criteria: Vec<String>,
    pub write_authority_state: String,
    pub write_authority_holder: Option<String>,
    pub write_authority_worktree: Option<String>,
    pub repo_state_baseline_head_sha: Option<String>,
    pub repo_state_baseline_worktree_fingerprint: Option<String>,
    pub repo_state_drift_state: String,
    pub dependency_index_state: String,
    pub final_review_state: DownstreamFreshnessState,
    pub browser_qa_state: DownstreamFreshnessState,
    pub release_docs_state: DownstreamFreshnessState,
    pub last_final_review_artifact_fingerprint: Option<String>,
    pub last_browser_qa_artifact_fingerprint: Option<String>,
    pub last_release_docs_artifact_fingerprint: Option<String>,
    pub strategy_state: String,
    pub last_strategy_checkpoint_fingerprint: Option<String>,
    pub strategy_checkpoint_kind: String,
    pub strategy_reset_required: bool,
    pub phase_detail: String,
    pub review_state_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PublicRecordingContext")]
    pub recording_context: Option<PublicRecordingContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PublicExecutionCommandContext")]
    pub execution_command_context: Option<PublicExecutionCommandContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_reentry_target_source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_repair_targets: Vec<PublicRepairTarget>,
    pub blocking_records: Vec<StatusBlockingRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_wait_state: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub projection_diagnostics: Vec<String>,
    pub state_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_public_action: Option<RuntimeNextPublicAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<RuntimeBlocker>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_provenance: Option<RuntimeProvenance>,
    pub semantic_workspace_tree_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_workspace_tree_id: Option<String>,
    pub next_action: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub recommended_public_command: Option<PublicCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: RecommendedPublicCommandArgv,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub reason_codes: Vec<String>,
    pub execution_mode: String,
    pub execution_fingerprint: String,
    pub evidence_path: String,
    pub projection_mode: String,
    pub state_dir_projection_paths: Vec<String>,
    pub tracked_projection_paths: Vec<String>,
    pub tracked_projections_current: bool,
    pub execution_started: String,
    pub warning_codes: Vec<String>,
    pub active_task: Option<u32>,
    pub active_step: Option<u32>,
    pub blocking_task: Option<u32>,
    pub blocking_step: Option<u32>,
    pub resume_task: Option<u32>,
    pub resume_step: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GateDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub remediation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GateResult {
    pub allowed: bool,
    pub action: String,
    pub failure_class: String,
    pub reason_codes: Vec<String>,
    pub warning_codes: Vec<String>,
    pub diagnostics: Vec<GateDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub workspace_state_id: Option<String>,
    pub current_branch_reviewed_state_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub recommended_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct GateProjectionInputs<'a> {
    pub(crate) gate_review: &'a GateResult,
    pub(crate) gate_finish: &'a GateResult,
}

#[derive(Debug)]
pub struct GateState {
    pub allowed: bool,
    pub failure_class: String,
    pub reason_codes: Vec<String>,
    pub warning_codes: Vec<String>,
    pub diagnostics: Vec<GateDiagnostic>,
    pub action: String,
    pub code: Option<String>,
    pub workspace_state_id: Option<String>,
    pub current_branch_reviewed_state_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub recommended_command: Option<String>,
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    pub rederive_via_workflow_operator: Option<bool>,
}

impl Default for GateState {
    fn default() -> Self {
        Self {
            allowed: true,
            failure_class: String::new(),
            reason_codes: Vec::new(),
            warning_codes: Vec::new(),
            diagnostics: Vec::new(),
            action: String::from("passed"),
            code: None,
            workspace_state_id: None,
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            recommended_command: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
        }
    }
}

impl GateState {
    pub fn from_result(result: GateResult) -> Self {
        Self {
            allowed: result.allowed,
            action: result.action,
            failure_class: result.failure_class,
            reason_codes: result.reason_codes,
            warning_codes: result.warning_codes,
            diagnostics: result.diagnostics,
            code: result.code,
            workspace_state_id: result.workspace_state_id,
            current_branch_reviewed_state_id: result.current_branch_reviewed_state_id,
            current_branch_closure_id: result.current_branch_closure_id,
            finish_review_gate_pass_branch_closure_id: result
                .finish_review_gate_pass_branch_closure_id,
            recommended_command: result.recommended_command,
            recommended_public_command_template: result.recommended_public_command_template,
            required_inputs: result.required_inputs,
            rederive_via_workflow_operator: result.rederive_via_workflow_operator,
        }
    }

    pub fn fail(
        &mut self,
        failure_class: FailureClass,
        code: &str,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) {
        self.allowed = false;
        if self.failure_class.is_empty() {
            self.failure_class = failure_class.as_str().to_owned();
        }
        if !self.reason_codes.iter().any(|existing| existing == code) {
            self.reason_codes.push(code.to_owned());
            self.diagnostics.push(GateDiagnostic {
                code: code.to_owned(),
                severity: String::from("error"),
                message: message.into(),
                remediation: remediation.into(),
            });
        }
    }

    pub fn warn(&mut self, code: &str) {
        if !self.warning_codes.iter().any(|existing| existing == code) {
            self.warning_codes.push(code.to_owned());
        }
    }

    pub fn finish(mut self) -> GateResult {
        if self.failure_class.is_empty() {
            self.allowed = true;
        }
        GateResult {
            allowed: self.allowed,
            action: if self.allowed {
                String::from("passed")
            } else {
                String::from("blocked")
            },
            failure_class: self.failure_class,
            reason_codes: self.reason_codes,
            warning_codes: self.warning_codes,
            diagnostics: self.diagnostics,
            code: self.code,
            workspace_state_id: self.workspace_state_id,
            current_branch_reviewed_state_id: self.current_branch_reviewed_state_id,
            current_branch_closure_id: self.current_branch_closure_id,
            finish_review_gate_pass_branch_closure_id: self
                .finish_review_gate_pass_branch_closure_id,
            recommended_command: self.recommended_command,
            recommended_public_command_template: self.recommended_public_command_template,
            required_inputs: self.required_inputs,
            rederive_via_workflow_operator: self.rederive_via_workflow_operator,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StatusBlockingRecord {
    pub code: String,
    pub scope_type: String,
    pub scope_key: String,
    pub record_type: String,
    pub record_id: Option<String>,
    pub review_state_status: String,
    pub required_follow_up: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicReviewStateTaskClosure {
    pub task: u32,
    pub closure_record_id: String,
    pub reviewed_state_id: String,
    pub contract_identity: String,
    pub effective_reviewed_surface_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicRecordingContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicExecutionCommandContext {
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicRepairTarget {
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    pub reason_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_record_id: Option<String>,
    pub expires_when_fingerprint_changes: bool,
}

pub fn write_plan_execution_schema(output_dir: &Path) -> Result<(), JsonFailure> {
    fs::create_dir_all(output_dir).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not create schema directory {}: {error}",
                output_dir.display()
            ),
        )
    })?;
    let schema = schema_for!(PlanExecutionStatus);
    let mut schema_json = serde_json::to_value(&schema).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not serialize plan execution schema value: {error}"),
        )
    })?;
    if let Some(required) = schema_json
        .get_mut("required")
        .and_then(serde_json::Value::as_array_mut)
    {
        required.retain(|field| {
            !matches!(
                field.as_str(),
                Some("recording_context" | "execution_command_context")
            )
        });
    }
    inject_plan_execution_route_vocabulary_schemas(&mut schema_json)?;
    tighten_plan_execution_public_context_schemas(&mut schema_json)?;
    tighten_public_repair_target_schema(&mut schema_json)?;
    tighten_plan_execution_routing_field_schemas(&mut schema_json)?;
    annotate_plan_execution_required_follow_up_schema(&mut schema_json)?;
    annotate_plan_execution_public_command_template_schema(&mut schema_json)?;
    annotate_plan_execution_next_public_action_schema(&mut schema_json)?;
    tighten_plan_execution_phase_bound_recording_context_contracts(&mut schema_json)?;
    let payload = serde_json::to_string_pretty(&schema_json).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not serialize plan execution schema: {error}"),
        )
    })?;
    fs::write(
        output_dir.join("plan-execution-status.schema.json"),
        payload,
    )
    .map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not write plan execution schema: {error}"),
        )
    })?;
    Ok(())
}

fn inject_plan_execution_route_vocabulary_schemas(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let root = schema_json.as_object_mut().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            "Generated plan execution schema root should be an object.",
        )
    })?;
    let defs = root
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `$defs`.",
            )
        })?;
    insert_string_enum_definition(defs, "PublicStatusPhaseSchema", PUBLIC_STATUS_PHASE_VALUES);
    insert_string_enum_definition(
        defs,
        "PhaseDetailSchema",
        PLAN_EXECUTION_STATUS_PHASE_DETAIL_VALUES,
    );
    insert_string_enum_definition(
        defs,
        "ReviewStateStatusSchema",
        PUBLIC_REVIEW_STATE_STATUS_VALUES,
    );
    insert_string_enum_definition(defs, "StateKindSchema", PUBLIC_STATE_KIND_VALUES);
    insert_string_enum_definition(defs, "QaRequirementSchema", PLAN_QA_REQUIREMENT_VALUES);
    insert_string_enum_definition(defs, "NextActionSchema", PUBLIC_NEXT_ACTION_VALUES);
    insert_string_enum_definition(
        defs,
        "RequiredFollowUpSchema",
        REQUIRED_FOLLOW_UP_SCHEMA_VALUES,
    );
    insert_string_enum_definition(
        defs,
        "ExecutionCommandKindSchema",
        PUBLIC_EXECUTION_COMMAND_KIND_VALUES,
    );
    insert_string_enum_definition(
        defs,
        "PublicRepairTargetCommandKindSchema",
        PUBLIC_REPAIR_TARGET_COMMAND_KIND_VALUES,
    );
    let properties = root
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing top-level `properties`.",
            )
        })?;
    properties.insert(
        String::from("phase"),
        serde_json::json!({
            "anyOf": [
                { "$ref": "#/$defs/PublicStatusPhaseSchema" },
                { "type": "null" }
            ]
        }),
    );
    set_schema_property_ref(properties, "phase_detail", "PhaseDetailSchema")?;
    set_schema_property_ref(properties, "review_state_status", "ReviewStateStatusSchema")?;
    set_schema_property_ref(properties, "state_kind", "StateKindSchema")?;
    set_schema_property_ref(properties, "next_action", "NextActionSchema")?;
    set_schema_property_nullable_ref(properties, "qa_requirement", "QaRequirementSchema")?;
    Ok(())
}

fn insert_string_enum_definition(
    defs: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
    values: &[&str],
) {
    defs.insert(
        String::from(name),
        serde_json::json!({
            "enum": values,
            "type": "string"
        }),
    );
}

fn set_schema_property_ref(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    def_name: &str,
) -> Result<(), JsonFailure> {
    require_schema_property(properties, field)?;
    properties.insert(
        String::from(field),
        serde_json::json!({ "$ref": format!("#/$defs/{def_name}") }),
    );
    Ok(())
}

fn set_schema_property_nullable_ref(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    def_name: &str,
) -> Result<(), JsonFailure> {
    require_schema_property(properties, field)?;
    properties.insert(
        String::from(field),
        serde_json::json!({
            "anyOf": [
                { "$ref": format!("#/$defs/{def_name}") },
                { "type": "null" }
            ]
        }),
    );
    Ok(())
}

fn require_schema_property(
    properties: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<(), JsonFailure> {
    if properties.contains_key(field) {
        Ok(())
    } else {
        Err(JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Plan execution schema is missing `{field}`."),
        ))
    }
}

fn tighten_plan_execution_public_context_schemas(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let defs = schema_json
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `$defs`.",
            )
        })?;
    let execution_context = defs
        .get_mut("PublicExecutionCommandContext")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `PublicExecutionCommandContext`.",
            )
        })?;
    tighten_public_execution_command_context_schema(execution_context)?;
    let recording_context = defs
        .get_mut("PublicRecordingContext")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `PublicRecordingContext`.",
            )
        })?;
    tighten_public_recording_context_schema(recording_context)?;
    Ok(())
}

fn tighten_plan_execution_routing_field_schemas(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let properties = schema_json
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing top-level `properties`.",
            )
        })?;
    tighten_schema_property_type(properties, "recommended_command", "string")?;
    annotate_schema_property(properties, "next_action", NEXT_ACTION_SCHEMA_DESCRIPTION)?;
    annotate_schema_property(
        properties,
        "next_public_action",
        ROUTE_NEXT_PUBLIC_ACTION_SCHEMA_DESCRIPTION,
    )?;
    annotate_schema_property(
        properties,
        "recommended_command",
        RECOMMENDED_COMMAND_SCHEMA_DESCRIPTION,
    )?;
    annotate_schema_property(
        properties,
        "recommended_public_command_argv",
        PLAN_EXECUTION_STATUS_RECOMMENDED_PUBLIC_COMMAND_ARGV_SCHEMA_DESCRIPTION,
    )?;
    annotate_schema_property(
        properties,
        "recommended_public_command_template",
        RECOMMENDED_PUBLIC_COMMAND_TEMPLATE_SCHEMA_DESCRIPTION,
    )?;
    annotate_schema_property(
        properties,
        "required_inputs",
        REQUIRED_INPUTS_SCHEMA_DESCRIPTION,
    )?;
    Ok(())
}

fn annotate_schema_property(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    description: &str,
) -> Result<(), JsonFailure> {
    let property = properties
        .get_mut(field)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!("Plan execution schema is missing `{field}`."),
            )
        })?;
    property.insert(
        String::from("description"),
        serde_json::Value::from(description),
    );
    Ok(())
}

fn annotate_plan_execution_required_follow_up_schema(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    set_required_follow_up_record_schema(schema_json)?;
    let required_follow_up = schema_json
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|defs| defs.get_mut("RequiredFollowUpSchema"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `RequiredFollowUpSchema`.",
            )
        })?;
    required_follow_up.insert(
        String::from("description"),
        serde_json::Value::from(REQUIRED_FOLLOW_UP_SCHEMA_DESCRIPTION),
    );
    Ok(())
}

fn set_required_follow_up_record_schema(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let properties = schema_json
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|defs| defs.get_mut("StatusBlockingRecord"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `StatusBlockingRecord.properties`.",
            )
        })?;
    set_schema_property_nullable_ref(properties, "required_follow_up", "RequiredFollowUpSchema")
}

fn annotate_plan_execution_public_command_template_schema(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let properties = schema_json
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|defs| defs.get_mut("PublicCommandTemplate"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `PublicCommandTemplate.properties`.",
            )
        })?;
    annotate_schema_property(
        properties,
        "command_kind",
        PUBLIC_COMMAND_TEMPLATE_KIND_SCHEMA_DESCRIPTION,
    )?;
    Ok(())
}

fn annotate_plan_execution_next_public_action_schema(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let defs = schema_json
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `$defs`.",
            )
        })?;
    let next_public_action = defs
        .get_mut("NextPublicAction")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `NextPublicAction`.",
            )
        })?;
    let properties = next_public_action
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution NextPublicAction schema is missing `properties`.",
            )
        })?;
    annotate_schema_property(
        properties,
        "display_only",
        NEXT_PUBLIC_ACTION_DISPLAY_ONLY_SCHEMA_DESCRIPTION,
    )?;
    annotate_schema_property(
        properties,
        "command",
        NEXT_PUBLIC_ACTION_COMMAND_SCHEMA_DESCRIPTION,
    )?;
    annotate_schema_property(
        properties,
        "args_template",
        NEXT_PUBLIC_ACTION_ARGS_TEMPLATE_SCHEMA_DESCRIPTION,
    )?;
    let blocker_properties = defs
        .get_mut("Blocker")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|blocker| blocker.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `Blocker.properties`.",
            )
        })?;
    annotate_schema_property(
        blocker_properties,
        "next_public_action",
        BLOCKER_NEXT_PUBLIC_ACTION_SCHEMA_DESCRIPTION,
    )?;
    Ok(())
}

fn tighten_plan_execution_phase_bound_recording_context_contracts(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    append_phase_bound_recording_context_requirements(
        schema_json,
        DETAIL_TASK_CLOSURE_RECORDING_READY,
        &["task_number"],
    )?;
    append_phase_bound_recording_context_requirements(
        schema_json,
        DETAIL_RELEASE_READINESS_RECORDING_READY,
        &["branch_closure_id"],
    )?;
    append_phase_bound_recording_context_requirements(
        schema_json,
        DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
        &["branch_closure_id"],
    )?;
    append_phase_bound_recording_context_requirements(
        schema_json,
        DETAIL_FINAL_REVIEW_RECORDING_READY,
        &["branch_closure_id"],
    )?;
    append_phase_detail_field_forbidden_outside_allowed_phase_details(
        schema_json,
        "recording_context",
        RECORDING_CONTEXT_PHASE_DETAILS,
    )?;
    append_phase_field_forbidden_outside_const_phase(
        schema_json,
        "harness_phase",
        PHASE_EXECUTING,
        "execution_command_context",
    )?;
    Ok(())
}

fn tighten_public_execution_command_context_schema(
    schema: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFailure> {
    let properties = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Execution command context schema is missing `properties`.",
            )
        })?;
    tighten_schema_property_type(properties, "task_number", "integer")?;
    tighten_schema_property_type(properties, "step_id", "integer")?;
    set_schema_property_ref(properties, "command_kind", "ExecutionCommandKindSchema")?;
    schema.insert(
        String::from("required"),
        serde_json::json!(["command_kind", "task_number", "step_id"]),
    );
    schema.insert(
        String::from("additionalProperties"),
        serde_json::Value::Bool(false),
    );
    Ok(())
}

fn tighten_public_recording_context_schema(
    schema: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFailure> {
    let properties = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Recording context schema is missing `properties`.",
            )
        })?;
    tighten_schema_property_type(properties, "branch_closure_id", "string")?;
    tighten_schema_property_type(properties, "dispatch_id", "string")?;
    tighten_schema_property_type(properties, "task_number", "integer")?;
    schema.insert(
        String::from("additionalProperties"),
        serde_json::Value::Bool(false),
    );
    schema.insert(String::from("minProperties"), serde_json::Value::from(1));
    schema.insert(
        String::from("anyOf"),
        serde_json::json!([
            { "required": ["branch_closure_id"] },
            { "required": ["task_number"] }
        ]),
    );
    Ok(())
}

fn tighten_public_repair_target_schema(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let properties = schema_json
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|defs| defs.get_mut("PublicRepairTarget"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `PublicRepairTarget.properties`.",
            )
        })?;
    set_schema_property_ref(
        properties,
        "command_kind",
        "PublicRepairTargetCommandKindSchema",
    )
}

fn tighten_schema_property_type(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected_type: &str,
) -> Result<(), JsonFailure> {
    let property = properties
        .get_mut(field)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!("Schema is missing property `{field}`."),
            )
        })?;
    property.insert(
        String::from("type"),
        serde_json::Value::String(String::from(expected_type)),
    );
    Ok(())
}

fn append_phase_bound_recording_context_requirements(
    schema_json: &mut serde_json::Value,
    phase_detail: &str,
    required_fields: &[&str],
) -> Result<(), JsonFailure> {
    let all_of = schema_json
        .as_object_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Generated plan execution schema root should be an object.",
            )
        })?
        .entry("allOf")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Generated plan execution schema allOf should be an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": {
                    "const": phase_detail
                }
            }
        },
        "then": {
            "required": ["recording_context"],
            "properties": {
                "recording_context": {
                    "required": required_fields
                }
            }
        }
    }));
    Ok(())
}

fn append_phase_detail_field_forbidden_outside_allowed_phase_details(
    schema_json: &mut serde_json::Value,
    field: &str,
    allowed_phase_details: &[&str],
) -> Result<(), JsonFailure> {
    let all_of = schema_json
        .as_object_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Generated plan execution schema root should be an object.",
            )
        })?
        .entry("allOf")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Generated plan execution schema allOf should be an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": {
                    "enum": allowed_phase_details
                }
            }
        },
        "else": {
            "not": {
                "required": [field]
            }
        }
    }));
    Ok(())
}

fn append_phase_field_forbidden_outside_const_phase(
    schema_json: &mut serde_json::Value,
    phase_field: &str,
    phase_value: &str,
    guarded_field: &str,
) -> Result<(), JsonFailure> {
    let all_of = schema_json
        .as_object_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Generated plan execution schema root should be an object.",
            )
        })?
        .entry("allOf")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Generated plan execution schema allOf should be an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                phase_field: {
                    "const": phase_value
                }
            }
        },
        "else": {
            "not": {
                "required": [guarded_field]
            }
        }
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::Value;

    use super::*;

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("featureforge-{label}-{nanos}"))
    }

    fn schema_enum_values(schema: &Value, pointer: &str) -> Vec<String> {
        schema
            .pointer(pointer)
            .and_then(|value| value.get("enum"))
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("schema pointer `{pointer}` should expose enum values"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| {
                        panic!("schema pointer `{pointer}` should contain only string enum values")
                    })
                    .to_owned()
            })
            .collect()
    }

    fn assert_schema_enum_matches(schema: &Value, pointer: &str, expected: &[&str]) {
        assert_eq!(
            schema_enum_values(schema, pointer),
            expected
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
            "schema enum `{pointer}` should be derived from its runtime owner constants"
        );
    }

    #[test]
    fn plan_execution_schema_enums_match_runtime_owner_constants() {
        let output_dir = unique_temp_dir("plan-execution-owner-vocab");
        write_plan_execution_schema(&output_dir).expect("plan execution schema should write");
        let schema: Value = serde_json::from_str(
            &fs::read_to_string(output_dir.join("plan-execution-status.schema.json"))
                .expect("plan execution schema should read"),
        )
        .expect("plan execution schema should parse");

        assert_schema_enum_matches(
            &schema,
            "/$defs/PublicStatusPhaseSchema",
            PUBLIC_STATUS_PHASE_VALUES,
        );
        assert_schema_enum_matches(
            &schema,
            "/$defs/PhaseDetailSchema",
            PLAN_EXECUTION_STATUS_PHASE_DETAIL_VALUES,
        );
        assert_schema_enum_matches(
            &schema,
            "/$defs/ReviewStateStatusSchema",
            PUBLIC_REVIEW_STATE_STATUS_VALUES,
        );
        assert_schema_enum_matches(&schema, "/$defs/StateKindSchema", PUBLIC_STATE_KIND_VALUES);
        assert_schema_enum_matches(
            &schema,
            "/$defs/QaRequirementSchema",
            PLAN_QA_REQUIREMENT_VALUES,
        );
        assert_schema_enum_matches(
            &schema,
            "/$defs/NextActionSchema",
            PUBLIC_NEXT_ACTION_VALUES,
        );
        assert_schema_enum_matches(
            &schema,
            "/$defs/RequiredFollowUpSchema",
            REQUIRED_FOLLOW_UP_SCHEMA_VALUES,
        );
        assert_schema_enum_matches(
            &schema,
            "/$defs/ExecutionCommandKindSchema",
            PUBLIC_EXECUTION_COMMAND_KIND_VALUES,
        );
        assert_schema_enum_matches(
            &schema,
            "/$defs/PublicRepairTargetCommandKindSchema",
            PUBLIC_REPAIR_TARGET_COMMAND_KIND_VALUES,
        );

        let _ = fs::remove_dir_all(output_dir);
    }
}
