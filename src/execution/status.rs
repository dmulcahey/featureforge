use std::fs;
use std::path::Path;

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::command_eligibility::{PublicCommand, PublicCommandInputRequirement};
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, ChunkingStrategy, DownstreamFreshnessState,
    EvaluationVerdict, EvaluatorKind, EvaluatorPolicyName, ExecutionRunId, HarnessPhase,
    ResetPolicy,
};
use crate::execution::phase::{
    DETAIL_FINAL_REVIEW_RECORDING_READY, DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
    DETAIL_RELEASE_READINESS_RECORDING_READY, DETAIL_TASK_CLOSURE_RECORDING_READY, PHASE_EXECUTING,
    PUBLIC_STATUS_PHASE_VALUES, RECORDING_CONTEXT_PHASE_DETAILS,
};
use crate::execution::public_command_types::RecommendedPublicCommandArgv;
use crate::execution::router::{
    Blocker as RuntimeBlocker, NextPublicAction as RuntimeNextPublicAction,
};
use crate::execution::runtime_provenance::RuntimeProvenance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ReviewStateStatusSchema {
    Clean,
    StaleUnreviewed,
    MissingCurrentClosure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum PhaseDetailSchema {
    BlockedRuntimeBug,
    BranchClosureRecordingRequiredForReleaseReadiness,
    ExecutionInProgress,
    ExecutionPreflightRequired,
    ExecutionReentryRequired,
    FinalReviewDispatchRequired,
    FinalReviewOutcomePending,
    FinalReviewRecordingReady,
    FinishCompletionGateReady,
    FinishReviewGateReady,
    HandoffRecordingRequired,
    PlanningReentryRequired,
    QaRecordingRequired,
    ReleaseBlockerResolutionRequired,
    ReleaseReadinessRecordingReady,
    RuntimeReconcileRequired,
    TaskClosureRecordingReady,
    TaskReviewResultPending,
    TestPlanRefreshRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum StateKindSchema {
    ActionablePublicCommand,
    WaitingExternalInput,
    Terminal,
    BlockedRuntimeBug,
    RuntimeReconcileRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum QaRequirementSchema {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "not-required")]
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ExecutionCommandKindSchema {
    Begin,
    Complete,
    Reopen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum PublicRepairTargetCommandKindSchema {
    Begin,
    Complete,
    Reopen,
    Transfer,
    CloseCurrentTask,
    RepairReviewState,
    AdvanceLateStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RequiredFollowUpSchema {
    ExecutionReentry,
    RepairReviewState,
    RequestExternalReview,
    RunVerification,
    AdvanceLateStage,
    ResolveReleaseBlocker,
    RecordHandoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum NextActionSchema {
    #[serde(rename = "advance late stage")]
    AdvanceLateStage,
    #[serde(rename = "finish branch")]
    FinishBranch,
    #[serde(rename = "close current task")]
    CloseCurrentTask,
    #[serde(rename = "continue execution")]
    ContinueExecution,
    #[serde(rename = "runtime diagnostic required")]
    RuntimeDiagnosticRequired,
    #[serde(rename = "request final review")]
    RequestFinalReview,
    #[serde(rename = "execution reentry required")]
    ExecutionReentryRequired,
    #[serde(rename = "hand off")]
    HandOff,
    #[serde(rename = "pivot / return to planning")]
    PivotReturnToPlanning,
    #[serde(rename = "refresh test plan")]
    RefreshTestPlan,
    #[serde(rename = "repair review state / reenter execution")]
    RepairReviewStateReenterExecution,
    #[serde(rename = "resolve release blocker")]
    ResolveReleaseBlocker,
    #[serde(rename = "run QA")]
    RunQa,
    #[serde(rename = "wait for external review result")]
    WaitForExternalReviewResult,
    #[serde(rename = "run verification")]
    RunVerification,
}

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
    #[schemars(with = "Option<QaRequirementSchema>")]
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
    #[schemars(with = "PhaseDetailSchema")]
    pub phase_detail: String,
    #[schemars(with = "ReviewStateStatusSchema")]
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
    #[schemars(with = "StateKindSchema")]
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
    #[schemars(with = "NextActionSchema")]
    pub next_action: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub recommended_public_command: Option<PublicCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: RecommendedPublicCommandArgv,
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
    #[schemars(with = "Option<RequiredFollowUpSchema>")]
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
    #[schemars(with = "ExecutionCommandKindSchema")]
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicRepairTarget {
    #[schemars(with = "PublicRepairTargetCommandKindSchema")]
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
    inject_plan_execution_phase_schema(&mut schema_json)?;
    tighten_plan_execution_public_context_schemas(&mut schema_json)?;
    tighten_plan_execution_routing_field_schemas(&mut schema_json)?;
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

fn inject_plan_execution_phase_schema(
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
    defs.insert(
        String::from("PublicStatusPhaseSchema"),
        serde_json::json!({
            "enum": PUBLIC_STATUS_PHASE_VALUES,
            "type": "string"
        }),
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
    Ok(())
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
    annotate_schema_property(
        properties,
        "recommended_command",
        "Display-only compatibility summary; do not parse or execute this string. Use recommended_public_command_argv when present.",
    )?;
    annotate_schema_property(
        properties,
        "recommended_public_command_argv",
        "Executable public command argv authority when present. Run these tokens as argv instead of parsing recommended_command.",
    )?;
    annotate_schema_property(
        properties,
        "required_inputs",
        "Parseable input contract for the routed public command when executable argv cannot be emitted yet.",
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
