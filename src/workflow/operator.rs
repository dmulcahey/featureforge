//! Workflow routing consumes the execution-owned query surface and maps it into
//! public phases and next-action recommendations.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cli::plan_execution::{RecommendArgs, StatusArgs as ExecutionStatusArgs};
use crate::cli::workflow::{DoctorArgs, OperatorArgs, PlanArgs};
use crate::contracts::plan::AnalyzePlanReport;
use crate::diagnostics::{DiagnosticError, JsonFailure};
use crate::execution::harness::EvaluatorKind;
use crate::execution::query::{
    ExecutionRoutingState, query_workflow_routing_state, query_workflow_routing_state_for_runtime,
    task_review_result_requires_verification,
};
use crate::execution::state::{ExecutionRuntime, GateResult, PlanExecutionStatus};
use crate::execution::topology::RecommendOutput;
use crate::workflow::status::{WorkflowPhase, WorkflowRoute};

const WORKFLOW_PHASE_SCHEMA_VERSION: u32 = 2;
const WORKFLOW_DOCTOR_SCHEMA_VERSION: u32 = 2;
const WORKFLOW_HANDOFF_SCHEMA_VERSION: u32 = 2;
const WORKFLOW_OPERATOR_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowOperatorPhaseSchema {
    Executing,
    TaskClosurePending,
    DocumentReleasePending,
    FinalReviewPending,
    QaPending,
    ReadyForBranchCompletion,
    HandoffRequired,
    PivotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowOperatorPhaseDetailSchema {
    ExecutionInProgress,
    ExecutionReentryRequired,
    TaskReviewDispatchRequired,
    TaskReviewResultPending,
    TaskClosureRecordingReady,
    BranchClosureRecordingRequiredForReleaseReadiness,
    ReleaseReadinessRecordingReady,
    ReleaseBlockerResolutionRequired,
    FinalReviewDispatchRequired,
    FinalReviewOutcomePending,
    FinalReviewRecordingReady,
    QaRecordingRequired,
    TestPlanRefreshRequired,
    FinishReviewGateReady,
    FinishCompletionGateReady,
    HandoffRecordingRequired,
    PlanningReentryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowOperatorReviewStateStatusSchema {
    Clean,
    StaleUnreviewed,
    MissingCurrentClosure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowOperatorFollowUpOverrideSchema {
    None,
    RecordHandoff,
    RecordPivot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum WorkflowOperatorNextActionSchema {
    #[serde(rename = "advance late stage")]
    AdvanceLateStage,
    #[serde(rename = "finish branch")]
    FinishBranch,
    #[serde(rename = "close current task")]
    CloseCurrentTask,
    #[serde(rename = "continue execution")]
    ContinueExecution,
    #[serde(rename = "request task review")]
    RequestTaskReview,
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
    #[serde(rename = "run verification")]
    RunVerification,
    #[serde(rename = "wait for external review result")]
    WaitForExternalReviewResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum WorkflowOperatorQaRequirementSchema {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "not-required")]
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowOperatorCommandKindSchema {
    Begin,
    Complete,
    Reopen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
/// Runtime struct.
pub struct WorkflowDoctor {
    /// Runtime field.
    pub schema_version: u32,
    /// Runtime field.
    pub phase: String,
    /// Runtime field.
    pub phase_detail: String,
    /// Runtime field.
    pub review_state_status: String,
    /// Runtime field.
    pub route_status: String,
    /// Runtime field.
    pub next_skill: String,
    /// Runtime field.
    pub next_action: String,
    /// Runtime field.
    pub next_step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub external_wait_state: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Runtime field.
    pub blocking_reason_codes: Vec<String>,
    /// Runtime field.
    pub spec_path: String,
    /// Runtime field.
    pub plan_path: String,
    /// Runtime field.
    pub contract_state: String,
    /// Runtime field.
    pub route: WorkflowRoute,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub execution_status: Option<PlanExecutionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub plan_contract: Option<AnalyzePlanReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub preflight: Option<GateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub gate_review: Option<GateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub gate_finish: Option<GateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub task_review_dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub final_review_dispatch_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
/// Runtime struct.
pub struct WorkflowHandoff {
    /// Runtime field.
    pub schema_version: u32,
    /// Runtime field.
    pub phase: String,
    /// Runtime field.
    pub phase_detail: String,
    /// Runtime field.
    pub review_state_status: String,
    /// Runtime field.
    pub route_status: String,
    /// Runtime field.
    pub next_skill: String,
    /// Runtime field.
    pub contract_state: String,
    /// Runtime field.
    pub spec_path: String,
    /// Runtime field.
    pub plan_path: String,
    /// Runtime field.
    pub execution_started: String,
    /// Runtime field.
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    /// Runtime field.
    pub reason_family: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// Runtime field.
    pub diagnostic_reason_codes: Vec<String>,
    /// Runtime field.
    pub recommended_skill: String,
    /// Runtime field.
    pub recommendation_reason: String,
    /// Runtime field.
    pub route: WorkflowRoute,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub execution_status: Option<PlanExecutionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub plan_contract: Option<AnalyzePlanReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub recommendation: Option<RecommendOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
/// Runtime struct.
pub struct WorkflowOperator {
    #[schemars(range(min = 2, max = 2))]
    /// Runtime field.
    pub schema_version: u32,
    #[schemars(with = "WorkflowOperatorPhaseSchema")]
    /// Runtime field.
    pub phase: String,
    #[schemars(with = "WorkflowOperatorPhaseDetailSchema")]
    /// Runtime field.
    pub phase_detail: String,
    #[schemars(with = "WorkflowOperatorReviewStateStatusSchema")]
    /// Runtime field.
    pub review_state_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<WorkflowOperatorQaRequirementSchema>")]
    /// Runtime field.
    pub qa_requirement: Option<String>,
    #[schemars(with = "WorkflowOperatorFollowUpOverrideSchema")]
    /// Runtime field.
    pub follow_up_override: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorkflowOperatorRecordingContext")]
    /// Runtime field.
    pub recording_context: Option<WorkflowOperatorRecordingContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorkflowOperatorExecutionCommandContext")]
    /// Runtime field.
    pub execution_command_context: Option<WorkflowOperatorExecutionCommandContext>,
    #[schemars(with = "WorkflowOperatorNextActionSchema")]
    /// Runtime field.
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub base_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub external_wait_state: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Runtime field.
    pub blocking_reason_codes: Vec<String>,
    /// Runtime field.
    pub spec_path: String,
    /// Runtime field.
    pub plan_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
/// Runtime struct.
pub struct WorkflowOperatorRecordingContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub branch_closure_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
/// Runtime struct.
pub struct WorkflowOperatorExecutionCommandContext {
    #[schemars(with = "WorkflowOperatorCommandKindSchema")]
    /// Runtime field.
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Runtime field.
    pub step_id: Option<u32>,
}

struct OperatorContext {
    route: WorkflowRoute,
    execution_status: Option<PlanExecutionStatus>,
    plan_contract: Option<AnalyzePlanReport>,
    preflight: Option<GateResult>,
    gate_review: Option<GateResult>,
    gate_finish: Option<GateResult>,
    execution_preflight_block_reason: Option<String>,
    phase: String,
    operator_phase: String,
    operator_phase_detail: String,
    operator_review_state_status: String,
    operator_follow_up_override: String,
    operator_recording_context: Option<WorkflowOperatorRecordingContext>,
    operator_execution_command_context: Option<WorkflowOperatorExecutionCommandContext>,
    operator_next_action: String,
    operator_recommended_command: Option<String>,
    operator_base_branch: Option<String>,
    operator_blocking_scope: Option<String>,
    operator_blocking_task: Option<u32>,
    operator_external_wait_state: Option<String>,
    operator_blocking_reason_codes: Vec<String>,
    reason_family: String,
    diagnostic_reason_codes: Vec<String>,
    task_review_dispatch_id: Option<String>,
    final_review_dispatch_id: Option<String>,
    finish_review_gate_pass_branch_closure_id: Option<String>,
    qa_requirement: Option<String>,
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_next(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_next_from_context(&context))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_next_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(render_next_from_context(&context))
}

fn render_next_from_context(context: &OperatorContext) -> String {
    let mut output = String::new();
    output.push_str("Next action: ");
    output.push_str(next_action_for_context(context));
    output.push('\n');
    output.push_str("Next safe step: ");
    output.push_str(&next_step_text(context));
    output.push('\n');
    output.push_str("Reason: ");
    output.push_str(&reason_text(context));
    output.push('\n');
    output
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_artifacts(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_artifacts_from_context(&context))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_artifacts_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(render_artifacts_from_context(&context))
}

fn render_artifacts_from_context(context: &OperatorContext) -> String {
    format!(
        "Workflow artifacts\n- Spec: {}\n- Plan: {}\n",
        display_or_none(&context.route.spec_path),
        display_or_none(&context.route.plan_path)
    )
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_explain(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_explain_from_context(&context))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_explain_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(render_explain_from_context(&context))
}

fn render_explain_from_context(context: &OperatorContext) -> String {
    format!(
        "Why FeatureForge chose this state\n- State: {}\n- Spec: {}\n- Plan: {}\nWhat to do:\n1. {}\n",
        context.route.status,
        display_or_none(&context.route.spec_path),
        display_or_none(&context.route.plan_path),
        next_step_text(context)
    )
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn phase(current_dir: &Path) -> Result<WorkflowPhase, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(phase_from_context(context))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn phase_for_runtime(runtime: &ExecutionRuntime) -> Result<WorkflowPhase, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(phase_from_context(context))
}

fn phase_from_context(context: OperatorContext) -> WorkflowPhase {
    WorkflowPhase {
        schema_version: WORKFLOW_PHASE_SCHEMA_VERSION,
        phase: context.phase.clone(),
        route_status: context.route.status.clone(),
        phase_detail: context.operator_phase_detail.clone(),
        review_state_status: context.operator_review_state_status.clone(),
        next_skill: public_next_skill(&context),
        next_step: next_step_text(&context),
        next_action: next_action_for_context(&context).to_owned(),
        recommended_command: context.operator_recommended_command.clone(),
        reason_family: context.reason_family.clone(),
        diagnostic_reason_codes: context.diagnostic_reason_codes.clone(),
        spec_path: context.route.spec_path.clone(),
        plan_path: context.route.plan_path.clone(),
        route: context.route,
    }
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_phase(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_phase_from_context(&context))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_phase_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(render_phase_from_context(&context))
}

fn render_phase_from_context(context: &OperatorContext) -> String {
    format!(
        "Workflow phase: {}\nPhase detail: {}\nReview state: {}\nRoute status: {}\nNext action: {}\nRecommended command: {}\nNext: {}\nSpec: {}\nPlan: {}\n",
        context.phase,
        context.operator_phase_detail,
        context.operator_review_state_status,
        context.route.status,
        next_action_for_context(context),
        optional_text(context.operator_recommended_command.as_deref()),
        next_step_text(context),
        display_or_none(&context.route.spec_path),
        display_or_none(&context.route.plan_path)
    )
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn doctor(current_dir: &Path) -> Result<WorkflowDoctor, JsonFailure> {
    doctor_with_args(
        current_dir,
        &DoctorArgs {
            plan: None,
            external_review_result_ready: false,
            json: false,
        },
    )
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn doctor_with_args(
    current_dir: &Path,
    args: &DoctorArgs,
) -> Result<WorkflowDoctor, JsonFailure> {
    let context = build_context_with_plan(
        current_dir,
        args.plan.as_deref(),
        args.external_review_result_ready,
    )?;
    Ok(doctor_from_context(context))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn doctor_for_runtime(runtime: &ExecutionRuntime) -> Result<WorkflowDoctor, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(doctor_from_context(context))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn doctor_for_runtime_with_args(
    runtime: &ExecutionRuntime,
    args: &DoctorArgs,
) -> Result<WorkflowDoctor, JsonFailure> {
    let context = build_context_with_plan_for_runtime(
        runtime,
        args.plan.as_deref(),
        args.external_review_result_ready,
    )?;
    Ok(doctor_from_context(context))
}

fn doctor_from_context(context: OperatorContext) -> WorkflowDoctor {
    let doctor_phase = doctor_phase_for_context(&context);
    let contract_state = context.plan_contract.as_ref().map_or_else(
        || context.route.contract_state.clone(),
        |report| report.contract_state.clone(),
    );
    let gate_review = doctor_gate_review(&context);

    WorkflowDoctor {
        schema_version: WORKFLOW_DOCTOR_SCHEMA_VERSION,
        phase: doctor_phase,
        phase_detail: context.operator_phase_detail.clone(),
        review_state_status: context.operator_review_state_status.clone(),
        route_status: context.route.status.clone(),
        next_skill: public_next_skill(&context),
        next_action: next_action_for_context(&context).to_owned(),
        next_step: next_step_text(&context),
        recommended_command: context.operator_recommended_command.clone(),
        blocking_scope: context.operator_blocking_scope.clone(),
        blocking_task: context.operator_blocking_task,
        external_wait_state: context.operator_external_wait_state.clone(),
        blocking_reason_codes: context.operator_blocking_reason_codes.clone(),
        spec_path: context.route.spec_path.clone(),
        plan_path: context.route.plan_path.clone(),
        contract_state,
        route: context.route,
        execution_status: context.execution_status,
        plan_contract: context.plan_contract,
        preflight: context.preflight,
        gate_review,
        gate_finish: context.gate_finish,
        task_review_dispatch_id: context.task_review_dispatch_id,
        final_review_dispatch_id: context.final_review_dispatch_id,
    }
}

fn doctor_gate_review(context: &OperatorContext) -> Option<GateResult> {
    if let Some(mut gate_review) = context.gate_review.clone() {
        if let Some(status) = context.execution_status.as_ref() {
            for reason_code in context
                .operator_blocking_reason_codes
                .iter()
                .chain(status.reason_codes.iter())
            {
                if doctor_synthetic_gate_review_reason_code(reason_code)
                    && !gate_review
                        .reason_codes
                        .iter()
                        .any(|existing| existing == reason_code)
                {
                    gate_review.reason_codes.push(reason_code.clone());
                }
            }
        }
        if gate_review.failure_class == "StaleExecutionEvidence"
            || doctor_synthetic_gate_review_failure_class(&gate_review.reason_codes)
                == "StaleProvenance"
        {
            gate_review.failure_class = String::from("StaleProvenance");
        }
        return Some(gate_review);
    }

    let status = context.execution_status.as_ref()?;
    if status.execution_started != "yes" {
        return None;
    }

    let mut reason_codes = Vec::new();
    for reason_code in context
        .operator_blocking_reason_codes
        .iter()
        .chain(status.reason_codes.iter())
    {
        if doctor_synthetic_gate_review_reason_code(reason_code)
            && !reason_codes.iter().any(|existing| existing == reason_code)
        {
            reason_codes.push(reason_code.clone());
        }
    }
    if reason_codes.is_empty() {
        return None;
    }

    Some(GateResult {
        allowed: false,
        action: String::from("blocked"),
        failure_class: doctor_synthetic_gate_review_failure_class(&reason_codes),
        reason_codes,
        warning_codes: Vec::new(),
        diagnostics: Vec::new(),
        code: None,
        workspace_state_id: Some(status.workspace_state_id.clone()),
        current_branch_reviewed_state_id: status.current_branch_reviewed_state_id.clone(),
        current_branch_closure_id: status.current_branch_closure_id.clone(),
        finish_review_gate_pass_branch_closure_id: status
            .finish_review_gate_pass_branch_closure_id
            .clone(),
        recommended_command: context.operator_recommended_command.clone(),
        rederive_via_workflow_operator: None,
    })
}

fn doctor_synthetic_gate_review_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        "stale_provenance"
            | "stale_unreviewed"
            | "post_review_repo_write_detected"
            | "final_review_state_not_fresh"
            | "browser_qa_state_not_fresh"
            | "release_docs_state_not_fresh"
    )
}

fn doctor_synthetic_gate_review_failure_class(reason_codes: &[String]) -> String {
    if reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "stale_provenance" | "stale_unreviewed" | "post_review_repo_write_detected"
        )
    }) {
        String::from("StaleProvenance")
    } else {
        String::from("ExecutionStateNotReady")
    }
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_doctor(current_dir: &Path) -> Result<String, JsonFailure> {
    render_doctor_with_args(
        current_dir,
        &DoctorArgs {
            plan: None,
            external_review_result_ready: false,
            json: false,
        },
    )
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_doctor_with_args(
    current_dir: &Path,
    args: &DoctorArgs,
) -> Result<String, JsonFailure> {
    let doctor = doctor_with_args(current_dir, args)?;
    Ok(render_doctor_output(&doctor))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_doctor_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let doctor = doctor_for_runtime(runtime)?;
    Ok(render_doctor_output(&doctor))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_doctor_for_runtime_with_args(
    runtime: &ExecutionRuntime,
    args: &DoctorArgs,
) -> Result<String, JsonFailure> {
    let doctor = doctor_for_runtime_with_args(runtime, args)?;
    Ok(render_doctor_output(&doctor))
}

fn render_doctor_output(doctor: &WorkflowDoctor) -> String {
    let mut output = format!(
        "Workflow doctor\nPhase: {}\nPhase detail: {}\nReview state: {}\nRoute status: {}\nNext action: {}\nRecommended command: {}\nNext: {}\nContract state: {}\nSpec: {}\nPlan: {}\n",
        doctor.phase,
        doctor.phase_detail,
        doctor.review_state_status,
        doctor.route_status,
        doctor.next_action,
        optional_text(doctor.recommended_command.as_deref()),
        doctor.next_step,
        doctor.contract_state,
        display_or_none(&doctor.spec_path),
        display_or_none(&doctor.plan_path)
    );
    if let Some(blocking_scope) = doctor.blocking_scope.as_deref() {
        let _ = writeln!(output, "Blocking scope: {blocking_scope}");
    }
    if let Some(blocking_task) = doctor.blocking_task {
        let _ = writeln!(output, "Blocking task: {blocking_task}");
    }
    if let Some(external_wait_state) = doctor.external_wait_state.as_deref() {
        let _ = writeln!(output, "External wait: {external_wait_state}");
    }
    if !doctor.blocking_reason_codes.is_empty() {
        let _ = writeln!(
            output,
            "Blocking reason codes: {}",
            reason_codes_text(&doctor.blocking_reason_codes)
        );
    }
    if let Some(execution_status) = doctor.execution_status.as_ref() {
        append_execution_status_metadata(&mut output, execution_status);
    }
    if let Some(preflight) = doctor.preflight.as_ref() {
        let _ = writeln!(
            output,
            "Preflight reason codes: {}",
            reason_codes_text(&preflight.reason_codes)
        );
    }
    if let Some(gate_review) = doctor.gate_review.as_ref() {
        let _ = writeln!(
            output,
            "Review gate reason codes: {}",
            reason_codes_text(&gate_review.reason_codes)
        );
    }
    if let Some(gate_finish) = doctor.gate_finish.as_ref() {
        let _ = writeln!(
            output,
            "Finish gate reason codes: {}",
            reason_codes_text(&gate_finish.reason_codes)
        );
    }
    output
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn handoff(current_dir: &Path) -> Result<WorkflowHandoff, JsonFailure> {
    let context = build_context(current_dir)?;
    let recommendation = execution_preflight_recommendation_for_context(&context, |plan| {
        let runtime = ExecutionRuntime::discover(current_dir)?;
        runtime.recommend(&RecommendArgs {
            plan,
            isolated_agents: None,
            session_intent: None,
            workspace_prepared: None,
        })
    })?;
    Ok(handoff_from_context(context, recommendation))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn handoff_for_runtime(runtime: &ExecutionRuntime) -> Result<WorkflowHandoff, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    let recommendation = execution_preflight_recommendation_for_context(&context, |plan| {
        runtime.recommend(&RecommendArgs {
            plan,
            isolated_agents: None,
            session_intent: None,
            workspace_prepared: None,
        })
    })?;
    Ok(handoff_from_context(context, recommendation))
}

fn execution_preflight_recommendation_for_context<F>(
    context: &OperatorContext,
    recommend: F,
) -> Result<Option<RecommendOutput>, JsonFailure>
where
    F: FnOnce(PathBuf) -> Result<RecommendOutput, JsonFailure>,
{
    let execution_started = context.execution_status.as_ref().map_or_else(
        || String::from("no"),
        |status| status.execution_started.clone(),
    );
    if context.route.status == "implementation_ready"
        && context.phase == "execution_preflight"
        && execution_started != "yes"
        && !context.route.plan_path.is_empty()
    {
        recommend(PathBuf::from(&context.route.plan_path)).map(Some)
    } else {
        Ok(None)
    }
}

fn handoff_from_context(
    context: OperatorContext,
    recommendation: Option<RecommendOutput>,
) -> WorkflowHandoff {
    // Source-contract anchors for runtime-instruction tests that assert the
    // phase->recommended-skill mapping remains visible in this file:
    // "final_review_pending" => (String::from("featureforge:requesting-code-review")
    // "qa_pending" => (String::from("featureforge:qa-only")
    // "document_release_pending" => (String::from("featureforge:document-release")
    // "ready_for_branch_completion" => (String::from("featureforge:finishing-a-development-branch")
    {
        let contract_state = context.plan_contract.as_ref().map_or_else(
            || context.route.contract_state.clone(),
            |report| report.contract_state.clone(),
        );
        let execution_started = context.execution_status.as_ref().map_or_else(
            || String::from("no"),
            |status| status.execution_started.clone(),
        );
        let (recommended_skill, recommendation_reason) = if let Some(recommendation) =
            recommendation.as_ref()
        {
            (
                recommendation.recommended_skill.clone(),
                recommendation.reason.clone(),
            )
        } else {
            match context.phase.as_str() {
                "executing" => {
                    let skill = context
                        .execution_status
                        .as_ref()
                        .map(|status| status.execution_mode.clone())
                        .unwrap_or_default();
                    (
                        skill,
                        String::from(
                            "Execution already started for the approved plan revision; continue with the current execution flow.",
                        ),
                    )
                }
                "implementation_handoff" => (String::new(), reason_text(&context)),
                "final_review_pending" if review_requires_execution_reentry(&context) => {
                    let skill = context
                        .execution_status
                        .as_ref()
                        .map(|status| status.execution_mode.clone())
                        .unwrap_or_default();
                    (skill, reason_text(&context))
                }
                "final_review_pending" => (
                    String::from("featureforge:requesting-code-review"),
                    reason_text(&context),
                ),
                "qa_pending" if context.operator_phase_detail == "test_plan_refresh_required" => (
                    String::from("featureforge:plan-eng-review"),
                    reason_text(&context),
                ),
                "qa_pending" => (String::from("featureforge:qa-only"), reason_text(&context)),
                "document_release_pending" => (
                    String::from("featureforge:document-release"),
                    reason_text(&context),
                ),
                "ready_for_branch_completion" => (
                    String::from("featureforge:finishing-a-development-branch"),
                    reason_text(&context),
                ),
                "pivot_required" => (
                    String::from("featureforge:writing-plans"),
                    reason_text(&context),
                ),
                "task_closure_pending" => {
                    let skill = context
                        .execution_status
                        .as_ref()
                        .map(|status| status.execution_mode.clone())
                        .unwrap_or_default();
                    let recommendation_reason = task_boundary_next_step_text(&context)
                        .unwrap_or_else(|| reason_text(&context));
                    (skill, recommendation_reason)
                }
                _ if execution_started == "yes" => {
                    let skill = context
                        .execution_status
                        .as_ref()
                        .map(|status| status.execution_mode.clone())
                        .unwrap_or_default();
                    (
                        skill,
                        String::from(
                            "Execution already started for the approved plan revision; continue with the current execution flow.",
                        ),
                    )
                }
                _ => (String::new(), String::new()),
            }
        };

        WorkflowHandoff {
            schema_version: WORKFLOW_HANDOFF_SCHEMA_VERSION,
            phase: context.phase.clone(),
            phase_detail: context.operator_phase_detail.clone(),
            review_state_status: context.operator_review_state_status.clone(),
            route_status: context.route.status.clone(),
            next_skill: public_next_skill(&context),
            contract_state,
            spec_path: context.route.spec_path.clone(),
            plan_path: context.route.plan_path.clone(),
            execution_started,
            next_action: next_action_for_context(&context).to_owned(),
            recommended_command: context.operator_recommended_command.clone(),
            reason_family: context.reason_family.clone(),
            diagnostic_reason_codes: context.diagnostic_reason_codes.clone(),
            recommended_skill,
            recommendation_reason,
            route: context.route,
            execution_status: context.execution_status,
            plan_contract: context.plan_contract,
            recommendation,
        }
    }
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn operator(current_dir: &Path, args: &OperatorArgs) -> Result<WorkflowOperator, JsonFailure> {
    let context = build_context_with_plan(
        current_dir,
        Some(&args.plan),
        args.external_review_result_ready,
    )?;
    Ok(operator_from_context(context, args))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn operator_for_runtime(
    runtime: &ExecutionRuntime,
    args: &OperatorArgs,
) -> Result<WorkflowOperator, JsonFailure> {
    let context = build_context_with_plan_for_runtime(
        runtime,
        Some(&args.plan),
        args.external_review_result_ready,
    )?;
    Ok(operator_from_context(context, args))
}

fn operator_from_context(context: OperatorContext, args: &OperatorArgs) -> WorkflowOperator {
    let plan_path = operator_plan_path(&context, args);
    WorkflowOperator {
        schema_version: WORKFLOW_OPERATOR_SCHEMA_VERSION,
        phase: context.operator_phase.clone(),
        phase_detail: context.operator_phase_detail.clone(),
        review_state_status: context.operator_review_state_status.clone(),
        qa_requirement: context.qa_requirement.clone(),
        follow_up_override: context.operator_follow_up_override.clone(),
        finish_review_gate_pass_branch_closure_id: context
            .finish_review_gate_pass_branch_closure_id
            .clone(),
        recording_context: context.operator_recording_context.clone(),
        execution_command_context: context.operator_execution_command_context.clone(),
        next_action: context.operator_next_action.clone(),
        recommended_command: context.operator_recommended_command.clone(),
        base_branch: context.operator_base_branch.clone(),
        blocking_scope: context.operator_blocking_scope.clone(),
        blocking_task: context.operator_blocking_task,
        external_wait_state: context.operator_external_wait_state.clone(),
        blocking_reason_codes: context.operator_blocking_reason_codes.clone(),
        spec_path: context.route.spec_path,
        plan_path,
    }
}

#[must_use]
/// Runtime function.
pub fn render_operator(operator: WorkflowOperator) -> String {
    let recording_context = operator.recording_context.clone();
    let execution_command_context = operator.execution_command_context.clone();
    let mut output = format!(
        "Workflow operator\nPhase: {}\nPhase detail: {}\nReview state: {}\nFollow-up override: {}\nNext action: {}\nSpec: {}\nPlan: {}\n",
        operator.phase,
        operator.phase_detail,
        operator.review_state_status,
        operator.follow_up_override,
        operator.next_action,
        display_or_none(&operator.spec_path),
        display_or_none(&operator.plan_path)
    );
    if let Some(qa_requirement) = operator.qa_requirement {
        let _ = writeln!(output, "QA requirement: {qa_requirement}");
    }
    if let Some(checkpoint) = operator.finish_review_gate_pass_branch_closure_id {
        let _ = writeln!(output, "Finish gate checkpoint: {checkpoint}");
    }
    if let Some(recording_context) = recording_context.as_ref() {
        let _ = writeln!(
            output,
            "Recording context: {}",
            format_operator_recording_context(recording_context)
        );
    }
    if let Some(execution_command_context) = execution_command_context.as_ref() {
        let _ = writeln!(
            output,
            "Execution command context: {}",
            format_operator_execution_command_context(execution_command_context)
        );
    }
    if let Some(blocking_scope) = operator.blocking_scope.as_deref() {
        let _ = writeln!(output, "Blocking scope: {blocking_scope}");
    }
    if let Some(blocking_task) = operator.blocking_task {
        let _ = writeln!(output, "Blocking task: {blocking_task}");
    }
    if let Some(external_wait_state) = operator.external_wait_state.as_deref() {
        let _ = writeln!(output, "External wait: {external_wait_state}");
    }
    if !operator.blocking_reason_codes.is_empty() {
        let _ = writeln!(
            output,
            "Blocking reason codes: {}",
            reason_codes_text(&operator.blocking_reason_codes)
        );
    }
    if let Some(recommended_command) = operator.recommended_command {
        let _ = writeln!(output, "Recommended command: {recommended_command}");
    }
    output
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_handoff(current_dir: &Path) -> Result<String, JsonFailure> {
    let handoff = handoff(current_dir)?;
    Ok(render_handoff_output(&handoff))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn render_handoff_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let handoff = handoff_for_runtime(runtime)?;
    Ok(render_handoff_output(&handoff))
}

fn render_handoff_output(handoff: &WorkflowHandoff) -> String {
    let mut output = String::new();
    output.push_str("Workflow handoff\n");
    let _ = writeln!(output, "Phase: {}", handoff.phase);
    let _ = writeln!(output, "Phase detail: {}", handoff.phase_detail);
    let _ = writeln!(output, "Review state: {}", handoff.review_state_status);
    let _ = writeln!(output, "Route status: {}", handoff.route_status);
    let _ = writeln!(output, "Next action: {}", handoff.next_action);
    let _ = writeln!(
        output,
        "Recommended command: {}",
        optional_text(handoff.recommended_command.as_deref())
    );
    let _ = writeln!(output, "Spec: {}", display_or_none(&handoff.spec_path));
    let _ = writeln!(output, "Plan: {}", display_or_none(&handoff.plan_path));
    if !handoff.recommended_skill.is_empty() {
        let _ = writeln!(output, "Recommended skill: {}", handoff.recommended_skill);
    }
    if !handoff.recommendation_reason.is_empty() {
        let _ = writeln!(output, "Reason: {}", handoff.recommendation_reason);
    }
    if let Some(execution_status) = handoff.execution_status.as_ref() {
        append_execution_status_metadata(&mut output, execution_status);
    }
    output
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn preflight(current_dir: &Path, args: &PlanArgs) -> Result<GateResult, JsonFailure> {
    let runtime = ExecutionRuntime::discover(current_dir)?;
    preflight_for_runtime(&runtime, args)
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn preflight_for_runtime(
    runtime: &ExecutionRuntime,
    args: &PlanArgs,
) -> Result<GateResult, JsonFailure> {
    runtime.preflight(&execution_status_args(args))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn gate_review(current_dir: &Path, args: &PlanArgs) -> Result<GateResult, JsonFailure> {
    let runtime = ExecutionRuntime::discover(current_dir)?;
    gate_review_for_runtime(&runtime, args)
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn gate_review_for_runtime(
    runtime: &ExecutionRuntime,
    args: &PlanArgs,
) -> Result<GateResult, JsonFailure> {
    runtime.gate_review(&execution_status_args(args))
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn gate_finish(current_dir: &Path, args: &PlanArgs) -> Result<GateResult, JsonFailure> {
    let runtime = ExecutionRuntime::discover(current_dir)?;
    gate_finish_for_runtime(&runtime, args)
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn gate_finish_for_runtime(
    runtime: &ExecutionRuntime,
    args: &PlanArgs,
) -> Result<GateResult, JsonFailure> {
    runtime.gate_finish(&execution_status_args(args))
}

#[must_use]
/// Runtime function.
pub fn render_gate(title: &str, gate: &GateResult) -> String {
    let mut output = format!("{}\nAllowed: {}\n", title, gate.allowed);
    if !gate.failure_class.is_empty() {
        let _ = writeln!(output, "Failure class: {}", gate.failure_class);
    }
    output
}

fn build_context(current_dir: &Path) -> Result<OperatorContext, JsonFailure> {
    build_context_with_plan(current_dir, None, false)
}

fn build_context_for_runtime(runtime: &ExecutionRuntime) -> Result<OperatorContext, JsonFailure> {
    build_context_with_plan_for_runtime(runtime, None, false)
}

fn build_context_with_plan(
    current_dir: &Path,
    plan_override: Option<&Path>,
    external_review_result_ready: bool,
) -> Result<OperatorContext, JsonFailure> {
    let routing =
        query_workflow_routing_state(current_dir, plan_override, external_review_result_ready)?;
    build_context_from_routing(routing)
}

fn build_context_with_plan_for_runtime(
    runtime: &ExecutionRuntime,
    plan_override: Option<&Path>,
    external_review_result_ready: bool,
) -> Result<OperatorContext, JsonFailure> {
    let routing = query_workflow_routing_state_for_runtime(
        runtime,
        plan_override,
        external_review_result_ready,
    )?;
    build_context_from_routing(routing)
}

fn build_context_from_routing(
    routing: ExecutionRoutingState,
) -> Result<OperatorContext, JsonFailure> {
    let ExecutionRoutingState {
        route,
        execution_status,
        preflight,
        gate_review,
        gate_finish,
        workflow_phase: _,
        phase,
        phase_detail,
        review_state_status,
        qa_requirement,
        follow_up_override,
        finish_review_gate_pass_branch_closure_id,
        recording_context,
        execution_command_context,
        next_action,
        recommended_command,
        base_branch,
        blocking_scope,
        blocking_task,
        external_wait_state,
        blocking_reason_codes,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        ..
    } = routing;
    let operator_recording_context =
        recording_context.map(|context| WorkflowOperatorRecordingContext {
            task_number: context.task_number,
            dispatch_id: context.dispatch_id,
            branch_closure_id: context.branch_closure_id,
        });
    let operator_execution_command_context =
        execution_command_context.map(|context| WorkflowOperatorExecutionCommandContext {
            command_kind: context.command_kind,
            task_number: context.task_number,
            step_id: context.step_id,
        });
    let operator_phase = phase.clone();
    let operator_phase_detail = phase_detail;
    let operator_next_action = next_action;
    let operator_recommended_command = recommended_command;
    let operator_base_branch = base_branch;
    let plan_contract = if route.status == "implementation_ready" {
        analyze_plan_if_available(&route).map_err(JsonFailure::from)?
    } else {
        None
    };

    Ok(OperatorContext {
        route,
        execution_status,
        plan_contract,
        preflight,
        gate_review,
        gate_finish,
        execution_preflight_block_reason: None,
        phase,
        operator_phase,
        operator_phase_detail,
        operator_review_state_status: review_state_status,
        operator_follow_up_override: follow_up_override,
        operator_recording_context,
        operator_execution_command_context,
        operator_next_action,
        operator_recommended_command,
        operator_base_branch,
        operator_blocking_scope: blocking_scope,
        operator_blocking_task: blocking_task,
        operator_external_wait_state: external_wait_state,
        operator_blocking_reason_codes: blocking_reason_codes,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        finish_review_gate_pass_branch_closure_id,
        qa_requirement,
    })
}

fn operator_plan_path(context: &OperatorContext, args: &OperatorArgs) -> String {
    if !context.route.plan_path.is_empty() {
        context.route.plan_path.clone()
    } else if !args.plan.as_os_str().is_empty() {
        args.plan.to_string_lossy().into_owned()
    } else {
        String::new()
    }
}

fn doctor_phase_for_context(context: &OperatorContext) -> String {
    if context.route.status == "implementation_ready"
        && context
            .execution_status
            .as_ref()
            .is_some_and(|status| status.execution_started != "yes")
    {
        return String::from("execution_preflight");
    }

    context.phase.clone()
}

fn analyze_plan_if_available(
    route: &WorkflowRoute,
) -> Result<Option<AnalyzePlanReport>, DiagnosticError> {
    if route.spec_path.is_empty() || route.plan_path.is_empty() {
        return Ok(None);
    }

    let root = PathBuf::from(&route.root);
    let spec_path = root.join(&route.spec_path);
    let plan_path = root.join(&route.plan_path);
    if !spec_path.is_file() || !plan_path.is_file() {
        return Ok(None);
    }

    crate::contracts::plan::analyze_plan(spec_path, plan_path).map(Some)
}

fn next_step_text(context: &OperatorContext) -> String {
    if context.phase == "qa_pending"
        && context.operator_phase_detail == "test_plan_refresh_required"
    {
        if context.route.plan_path.is_empty() {
            return String::from(
                "Regenerate the current-branch test-plan artifact via featureforge:plan-eng-review before browser QA or branch completion.",
            );
        }
        return format!(
            "Regenerate the current-branch test-plan artifact via featureforge:plan-eng-review for the approved plan before browser QA or branch completion: {}",
            context.route.plan_path
        );
    }
    if let Some(task_boundary_next_step) = task_boundary_next_step_text(context) {
        return task_boundary_next_step;
    }
    if review_requires_execution_reentry(context) {
        if context.route.plan_path.is_empty() {
            return String::from("Return to the current execution flow for the approved plan.");
        }
        return format!(
            "Return to the current execution flow for the approved plan: {}",
            context.route.plan_path
        );
    }
    next_text_for_phase(
        &context.phase,
        &context.route.status,
        &context.route.plan_path,
        &context.route.next_skill,
    )
}

fn next_text_for_phase(
    phase: &str,
    route_status: &str,
    plan_path: &str,
    next_skill: &str,
) -> String {
    match phase {
        "execution_preflight" | "implementation_handoff" => {
            if plan_path.is_empty() {
                String::from("Return to execution preflight for the approved plan.")
            } else {
                format!("Return to execution preflight for the approved plan: {plan_path}")
            }
        }
        "executing"
        | "contract_drafting"
        | "contract_pending_approval"
        | "contract_approved"
        | "evaluating"
        | "task_closure_pending"
        | "handoff_required" => {
            if plan_path.is_empty() {
                String::from("Return to the current execution flow for the approved plan.")
            } else {
                format!("Return to the current execution flow for the approved plan: {plan_path}")
            }
        }
        "pivot_required" => {
            if plan_path.is_empty() {
                String::from("Update and re-approve the plan before continuing execution.")
            } else {
                format!("Update and re-approve the plan before continuing execution: {plan_path}")
            }
        }
        "final_review_pending" => {
            if plan_path.is_empty() {
                String::from("Use featureforge:requesting-code-review for the final review gate.")
            } else {
                format!(
                    "Use featureforge:requesting-code-review for the approved plan before branch completion: {plan_path}"
                )
            }
        }
        "qa_pending" => String::from(
            "Run featureforge:qa-only and return with a fresh QA result artifact before branch completion.",
        ),
        "document_release_pending" => String::from(
            "Run featureforge:document-release and return with a fresh release-readiness artifact before branch completion.",
        ),
        "ready_for_branch_completion" => {
            String::from("Use featureforge:finishing-a-development-branch.")
        }
        _ => {
            if !next_skill.is_empty() {
                format!("Use {next_skill}")
            } else if route_status == "needs_brainstorming" {
                String::from("Use featureforge:brainstorming")
            } else {
                String::from("Inspect the workflow state again after resolving the current issue.")
            }
        }
    }
}

fn reason_text(context: &OperatorContext) -> String {
    match context.phase.as_str() {
        "execution_preflight" => String::from(
            "The approved plan matches the latest approved spec and preflight is the next safe boundary.",
        ),
        "implementation_handoff" => context
            .execution_preflight_block_reason
            .clone()
            .unwrap_or_else(|| {
                String::from(
                    "The approved plan is ready, but execution preflight is still blocked by the current workspace state.",
                )
            }),
        "executing" => task_boundary_reason_text(context).unwrap_or_else(|| {
            String::from(
                "Execution already started for the approved plan and should continue through the current execution flow.",
            )
        }),
        "pivot_required" => {
            String::from("Execution is blocked pending an approved plan revision.")
        }
        "task_closure_pending" => {
            task_boundary_reason_text(context).unwrap_or_else(|| {
                String::from(
                    "Execution already started for the approved plan and should continue through the current execution flow.",
                )
            })
        }
        "contract_drafting"
        | "contract_pending_approval"
        | "contract_approved"
        | "evaluating"
        | "handoff_required" => String::from(
            "Execution already started for the approved plan and should continue through the current execution flow.",
        ),
        "final_review_pending" => gate_first_diagnostic_message(context.gate_review.as_ref())
            .or_else(|| gate_first_diagnostic_message(context.gate_finish.as_ref()))
            .unwrap_or_else(|| {
                String::from("Execution is blocked on the final review gate for the approved plan.")
            }),
        "qa_pending" | "document_release_pending" => {
            gate_first_diagnostic_message(context.gate_finish.as_ref())
                .unwrap_or_else(|| context.route.reason.clone())
        }
        "ready_for_branch_completion" => {
            String::from("All required late-stage artifacts are fresh for the current HEAD.")
        }
        _ => context.route.reason.clone(),
    }
}

const fn display_or_none(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

fn format_operator_recording_context(context: &WorkflowOperatorRecordingContext) -> String {
    let mut fields = Vec::new();
    if let Some(task_number) = context.task_number {
        fields.push(format!("task_number={task_number}"));
    }
    if let Some(dispatch_id) = context.dispatch_id.as_deref() {
        fields.push(format!("dispatch_id={dispatch_id}"));
    }
    if let Some(branch_closure_id) = context.branch_closure_id.as_deref() {
        fields.push(format!("branch_closure_id={branch_closure_id}"));
    }
    if fields.is_empty() {
        String::from("none")
    } else {
        fields.join(", ")
    }
}

fn format_operator_execution_command_context(
    context: &WorkflowOperatorExecutionCommandContext,
) -> String {
    let mut fields = vec![format!("command_kind={}", context.command_kind)];
    if let Some(task_number) = context.task_number {
        fields.push(format!("task_number={task_number}"));
    }
    if let Some(step_id) = context.step_id {
        fields.push(format!("step_id={step_id}"));
    }
    fields.join(", ")
}

fn public_next_skill(context: &OperatorContext) -> String {
    context.route.next_skill.clone()
}

fn next_action_for_context(context: &OperatorContext) -> &str {
    &context.operator_next_action
}

fn review_requires_execution_reentry(context: &OperatorContext) -> bool {
    context.phase == "final_review_pending"
        && context.operator_phase_detail != "final_review_dispatch_required"
        && context
            .gate_review
            .as_ref()
            .is_some_and(|gate| !gate.allowed)
}

fn task_boundary_reason_text(context: &OperatorContext) -> Option<String> {
    let blocking_task = context.operator_blocking_task?;
    let message = match context.operator_phase_detail.as_str() {
        "task_review_dispatch_required" => format!(
            "Task {blocking_task} closure cannot be recorded/refreshed yet. Dispatch dedicated-independent review for Task {blocking_task} first."
        ),
        "task_review_result_pending" => {
            if task_review_result_pending_requires_verification(context) {
                format!(
                    "Task {blocking_task} closure cannot be recorded/refreshed yet. Run verification and then record task closure for Task {blocking_task}."
                )
            } else if operator_blocking_reason_present(context, "task_review_not_independent")
                || operator_blocking_reason_present(context, "task_review_receipt_malformed")
                || operator_blocking_reason_present(context, "prior_task_review_not_green")
            {
                format!(
                    "Task {blocking_task} closure cannot be recorded/refreshed yet because the latest review provenance is invalid or not green. Dispatch dedicated-independent review for Task {blocking_task}, then record task closure."
                )
            } else {
                format!(
                    "Task {blocking_task} closure cannot be recorded/refreshed yet. Wait for the outstanding review result, then record task closure for Task {blocking_task}."
                )
            }
        }
        "task_closure_recording_ready" => format!(
            "Task {blocking_task} closure is ready to record/refresh. Record or refresh Task {blocking_task} closure now."
        ),
        "execution_reentry_required" => {
            if operator_blocking_reason_present(context, "prior_task_review_not_green") {
                format!(
                    "Task {blocking_task} closure cannot be recorded/refreshed yet because the latest dedicated-independent review is not green. Reenter execution to remediate Task {blocking_task}, then rerun review and record task closure."
                )
            } else {
                format!(
                    "Next-task begin is blocked because Task {blocking_task} closure state is stale or invalid. Reenter execution and complete the routed repair for Task {blocking_task}."
                )
            }
        }
        _ => return None,
    };
    Some(message)
}

fn operator_blocking_reason_present(context: &OperatorContext, reason_code: &str) -> bool {
    context
        .operator_blocking_reason_codes
        .iter()
        .any(|code| code == reason_code)
        || context
            .execution_status
            .iter()
            .any(|status| status.reason_codes.iter().any(|code| code == reason_code))
}

fn task_boundary_next_step_text(context: &OperatorContext) -> Option<String> {
    if !task_boundary_guidance_applies(context) {
        return None;
    }
    let reason = task_boundary_reason_text(context)?;
    if let Some(recommended_command) = context.operator_recommended_command.as_deref() {
        return Some(format!(
            "{reason} Follow the routed command: {recommended_command}"
        ));
    }
    Some(reason)
}

fn task_boundary_guidance_applies(context: &OperatorContext) -> bool {
    context.phase == "repairing"
        || context.phase == "task_closure_pending"
        || (context.phase == "executing"
            && context.operator_phase_detail == "execution_reentry_required"
            && context.operator_blocking_task.is_some())
}

fn task_review_result_pending_requires_verification(context: &OperatorContext) -> bool {
    task_review_result_requires_verification(
        context
            .operator_blocking_reason_codes
            .iter()
            .map(String::as_str)
            .chain(
                context
                    .execution_status
                    .iter()
                    .flat_map(|status| status.reason_codes.iter().map(String::as_str)),
            ),
    )
}

fn gate_first_diagnostic_message(gate: Option<&GateResult>) -> Option<String> {
    gate.and_then(|gate| {
        gate.diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
    })
}

fn append_execution_status_metadata(output: &mut String, status: &PlanExecutionStatus) {
    let _ = writeln!(
        output,
        "Execution reason codes: {}",
        reason_codes_text(&status.reason_codes)
    );
    let _ = writeln!(
        output,
        "Evaluator required kinds: {}",
        evaluator_kinds_text(&status.required_evaluator_kinds)
    );
    let _ = writeln!(
        output,
        "Evaluator completed kinds: {}",
        evaluator_kinds_text(&status.completed_evaluator_kinds)
    );
    let _ = writeln!(
        output,
        "Evaluator pending kinds: {}",
        evaluator_kinds_text(&status.pending_evaluator_kinds)
    );
    let _ = writeln!(
        output,
        "Evaluator non-passing kinds: {}",
        evaluator_kinds_text(&status.non_passing_evaluator_kinds)
    );
    let _ = writeln!(
        output,
        "Evaluator last kind: {}",
        optional_evaluator_kind_text(status.last_evaluation_evaluator_kind)
    );
    let _ = writeln!(
        output,
        "Write authority state: {}",
        status.write_authority_state
    );
    let _ = writeln!(
        output,
        "Write authority holder: {}",
        optional_text(status.write_authority_holder.as_deref())
    );
    let _ = writeln!(
        output,
        "Write authority worktree: {}",
        optional_text(status.write_authority_worktree.as_deref())
    );
    let _ = writeln!(output, "Strategy state: {}", status.strategy_state);
    let _ = writeln!(
        output,
        "Strategy checkpoint kind: {}",
        status.strategy_checkpoint_kind
    );
    let _ = writeln!(
        output,
        "Strategy checkpoint fingerprint: {}",
        optional_text(status.last_strategy_checkpoint_fingerprint.as_deref())
    );
    let _ = writeln!(
        output,
        "Strategy reset required: {}",
        if status.strategy_reset_required {
            "yes"
        } else {
            "no"
        }
    );
}

fn reason_codes_text(reason_codes: &[String]) -> String {
    if reason_codes.is_empty() {
        String::from("none")
    } else {
        reason_codes.join(", ")
    }
}

fn evaluator_kinds_text(kinds: &[EvaluatorKind]) -> String {
    if kinds.is_empty() {
        String::from("none")
    } else {
        kinds
            .iter()
            .copied()
            .map(evaluator_kind_text)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

const fn evaluator_kind_text(kind: EvaluatorKind) -> &'static str {
    match kind {
        EvaluatorKind::SpecCompliance => "spec_compliance",
        EvaluatorKind::CodeQuality => "code_quality",
    }
}

const fn optional_evaluator_kind_text(value: Option<EvaluatorKind>) -> &'static str {
    match value {
        Some(value) => evaluator_kind_text(value),
        None => "none",
    }
}

fn optional_text(value: Option<&str>) -> &str {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("none")
}

fn execution_status_args(args: &PlanArgs) -> ExecutionStatusArgs {
    ExecutionStatusArgs {
        plan: args.plan.clone(),
        external_review_result_ready: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expect_ext::ExpectValueExt;
    use crate::workflow::status::WorkflowRoute;

    fn task_boundary_context(
        phase_detail: &str,
        blocking_reason_codes: &[&str],
        recommended_command: Option<&str>,
    ) -> OperatorContext {
        OperatorContext {
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from("implementation_ready"),
                next_skill: String::from("featureforge:executing-plans"),
                spec_path: String::from("docs/featureforge/specs/example.md"),
                plan_path: String::from("docs/featureforge/plans/example.md"),
                contract_state: String::from("approved"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path: String::new(),
                root: String::from("/tmp/featureforge"),
                reason: String::new(),
                note: String::new(),
            },
            execution_status: None,
            plan_contract: None,
            preflight: None,
            gate_review: None,
            gate_finish: None,
            execution_preflight_block_reason: None,
            phase: String::from("task_closure_pending"),
            operator_phase: String::from("task_closure_pending"),
            operator_phase_detail: String::from(phase_detail),
            operator_review_state_status: String::from("clean"),
            operator_follow_up_override: String::from("none"),
            operator_recording_context: None,
            operator_execution_command_context: None,
            operator_next_action: String::from("wait for external review result"),
            operator_recommended_command: recommended_command.map(str::to_owned),
            operator_base_branch: Some(String::from("main")),
            operator_blocking_scope: Some(String::from("task")),
            operator_blocking_task: Some(1),
            operator_external_wait_state: None,
            operator_blocking_reason_codes: blocking_reason_codes
                .iter()
                .map(|reason| String::from(*reason))
                .collect(),
            reason_family: String::new(),
            diagnostic_reason_codes: Vec::new(),
            task_review_dispatch_id: Some(String::from("dispatch-task-1")),
            final_review_dispatch_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            qa_requirement: None,
        }
    }

    #[test]
    fn render_operator_surfaces_public_contract_fields() {
        let rendered = render_operator(WorkflowOperator {
            schema_version: 1,
            phase: String::from("executing"),
            phase_detail: String::from("execution_in_progress"),
            review_state_status: String::from("clean"),
            qa_requirement: Some(String::from("required")),
            follow_up_override: String::from("none"),
            finish_review_gate_pass_branch_closure_id: Some(String::from("branch-closure-1")),
            recording_context: Some(WorkflowOperatorRecordingContext {
                task_number: Some(1),
                dispatch_id: Some(String::from("dispatch-1")),
                branch_closure_id: Some(String::from("branch-closure-1")),
            }),
            execution_command_context: Some(WorkflowOperatorExecutionCommandContext {
                command_kind: String::from("complete"),
                task_number: Some(1),
                step_id: Some(2),
            }),
            next_action: String::from("continue execution"),
            recommended_command: Some(String::from(
                "featureforge plan execution complete --plan docs/featureforge/plans/sample.md --task 1 --step 2 --source featureforge:executing-plans --claim <claim> --manual-verify-summary <summary> --expect-execution-fingerprint abcdef",
            )),
            base_branch: Some(String::from("main")),
            blocking_scope: Some(String::from("task")),
            blocking_task: Some(1),
            external_wait_state: None,
            blocking_reason_codes: vec![String::from("stale_unreviewed")],
            spec_path: String::from("docs/featureforge/specs/sample.md"),
            plan_path: String::from("docs/featureforge/plans/sample.md"),
        });

        assert!(rendered.contains("Follow-up override: none"));
        assert!(rendered.contains("QA requirement: required"));
        assert!(rendered.contains("Finish gate checkpoint: branch-closure-1"));
        assert!(rendered.contains(
            "Recording context: task_number=1, dispatch_id=dispatch-1, branch_closure_id=branch-closure-1"
        ));
        assert!(rendered.contains(
            "Execution command context: command_kind=complete, task_number=1, step_id=2"
        ));
    }

    #[test]
    fn task_boundary_reason_text_uses_verification_language_when_verification_is_missing() {
        let context = task_boundary_context(
            "task_review_result_pending",
            &["prior_task_verification_missing"],
            Some(
                "featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            ),
        );

        let reason = task_boundary_reason_text(&context).expect_or_abort(
            "task-boundary reason text should be available for task_review_result_pending",
        );
        assert!(
            reason.contains("Run verification and then record task closure"),
            "verification-missing task-boundary reason text should mention verification + closure recording, got {reason}"
        );

        let next_step = task_boundary_next_step_text(&context).expect_or_abort(
            "task-boundary next-step text should be available for task_review_result_pending",
        );
        assert!(
            next_step.contains("Run verification and then record task closure"),
            "verification-missing next-step text should preserve verification + closure-recording guidance, got {next_step}"
        );
        assert!(
            next_step.contains("featureforge plan execution close-current-task"),
            "task-boundary next-step text should still include the routed command for verification-missing blockers, got {next_step}"
        );
    }
}
