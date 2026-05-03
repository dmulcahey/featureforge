//! Workflow routing consumes the execution-owned query surface and maps it into
//! public phases and next-action recommendations.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cli::workflow::OperatorArgs;
use crate::contracts::plan::AnalyzePlanReport;
use crate::diagnostics::{DiagnosticError, FailureClass, JsonFailure};
use crate::execution::closure_diagnostics::merge_status_projection_diagnostics;
use crate::execution::command_eligibility::PublicCommandInputRequirement;
use crate::execution::harness::EvaluatorKind;
use crate::execution::phase;
use crate::execution::public_command_types::RecommendedPublicCommandArgv;
use crate::execution::query::{
    ExecutionRoutingState, query_workflow_routing_state, query_workflow_routing_state_for_runtime,
    task_review_result_requires_verification,
};
use crate::execution::router::{
    Blocker as RuntimeBlocker, NextPublicAction as RuntimeNextPublicAction,
    RouteDecision as RuntimeRouteDecision, route_decision_from_routing,
};
use crate::execution::state::{ExecutionRuntime, GateResult, PlanExecutionStatus};
use crate::execution::topology::RecommendOutput;
use crate::workflow::status::{WorkflowPhase, WorkflowRoute};

const WORKFLOW_PHASE_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_DOCTOR_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_HANDOFF_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_OPERATOR_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone)]
pub struct DoctorArgs {
    pub plan: Option<PathBuf>,
    pub external_review_result_ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowOperatorPhaseSchema {
    Blocked,
    Executing,
    ExecutionPreflight,
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
    BlockedRuntimeBug,
    ExecutionInProgress,
    ExecutionPreflightRequired,
    ExecutionReentryRequired,
    TaskReviewResultPending,
    TaskClosureRecordingReady,
    BranchClosureRecordingRequiredForReleaseReadiness,
    ReleaseReadinessRecordingReady,
    ReleaseBlockerResolutionRequired,
    FinalReviewDispatchRequired,
    FinalReviewOutcomePending,
    FinalReviewRecordingReady,
    QaRecordingRequired,
    RuntimeReconcileRequired,
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
enum WorkflowOperatorStateKindSchema {
    ActionablePublicCommand,
    WaitingExternalInput,
    Terminal,
    BlockedRuntimeBug,
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
pub struct WorkflowDoctor {
    pub schema_version: u32,
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub route_status: String,
    pub next_skill: String,
    pub next_action: String,
    pub next_step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: RecommendedPublicCommandArgv,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_wait_state: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocking_reason_codes: Vec<String>,
    pub spec_path: String,
    pub plan_path: String,
    pub contract_state: String,
    pub route: WorkflowRoute,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_status: Option<PlanExecutionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_contract: Option<AnalyzePlanReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preflight: Option<GateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_review: Option<GateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_finish: Option<GateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_review_dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_review_dispatch_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowHandoff {
    pub schema_version: u32,
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub route_status: String,
    pub next_skill: String,
    pub contract_state: String,
    pub spec_path: String,
    pub plan_path: String,
    pub execution_started: String,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: RecommendedPublicCommandArgv,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    #[schemars(with = "WorkflowOperatorStateKindSchema")]
    pub state_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_public_action: Option<RuntimeNextPublicAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<RuntimeBlocker>,
    pub semantic_workspace_tree_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_workspace_tree_id: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason_family: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_reason_codes: Vec<String>,
    pub recommended_skill: String,
    pub recommendation_reason: String,
    pub route: WorkflowRoute,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_status: Option<PlanExecutionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_contract: Option<AnalyzePlanReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<RecommendOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowOperator {
    #[schemars(range(min = 3, max = 3))]
    pub schema_version: u32,
    #[schemars(with = "WorkflowOperatorPhaseSchema")]
    pub phase: String,
    #[schemars(with = "WorkflowOperatorPhaseDetailSchema")]
    pub phase_detail: String,
    #[schemars(with = "WorkflowOperatorReviewStateStatusSchema")]
    pub review_state_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<WorkflowOperatorQaRequirementSchema>")]
    pub qa_requirement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorkflowOperatorRecordingContext")]
    pub recording_context: Option<WorkflowOperatorRecordingContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorkflowOperatorExecutionCommandContext")]
    pub execution_command_context: Option<WorkflowOperatorExecutionCommandContext>,
    #[schemars(with = "WorkflowOperatorNextActionSchema")]
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: RecommendedPublicCommandArgv,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_wait_state: Option<String>,
    pub blocking_reason_codes: Vec<String>,
    #[schemars(with = "WorkflowOperatorStateKindSchema")]
    pub state_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_public_action: Option<RuntimeNextPublicAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<RuntimeBlocker>,
    pub semantic_workspace_tree_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_workspace_tree_id: Option<String>,
    pub spec_path: String,
    pub plan_path: String,
    pub projection_mode: String,
    pub state_dir_projection_paths: Vec<String>,
    pub tracked_projection_paths: Vec<String>,
    pub tracked_projections_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowOperatorRecordingContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowOperatorExecutionCommandContext {
    #[schemars(with = "WorkflowOperatorCommandKindSchema")]
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    operator_recording_context: Option<WorkflowOperatorRecordingContext>,
    operator_execution_command_context: Option<WorkflowOperatorExecutionCommandContext>,
    operator_next_action: String,
    operator_recommended_command: Option<String>,
    operator_recommended_public_command_argv: RecommendedPublicCommandArgv,
    operator_required_inputs: Vec<PublicCommandInputRequirement>,
    operator_base_branch: Option<String>,
    operator_blocking_scope: Option<String>,
    operator_blocking_task: Option<u32>,
    operator_external_wait_state: Option<String>,
    operator_blocking_reason_codes: Vec<String>,
    operator_state_kind: String,
    operator_next_public_action: Option<RuntimeNextPublicAction>,
    operator_blockers: Vec<RuntimeBlocker>,
    operator_semantic_workspace_tree_id: String,
    operator_raw_workspace_tree_id: Option<String>,
    reason_family: String,
    diagnostic_reason_codes: Vec<String>,
    task_review_dispatch_id: Option<String>,
    final_review_dispatch_id: Option<String>,
    finish_review_gate_pass_branch_closure_id: Option<String>,
    qa_requirement: Option<String>,
}

pub fn render_next(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_next_from_context(&context))
}

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

pub fn render_artifacts(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_artifacts_from_context(&context))
}

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

pub fn render_explain(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_explain_from_context(&context))
}

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

pub fn phase(current_dir: &Path) -> Result<WorkflowPhase, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(phase_from_context(context))
}

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

pub fn render_phase(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(render_phase_from_context(&context))
}

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

pub fn doctor(current_dir: &Path) -> Result<WorkflowDoctor, JsonFailure> {
    doctor_with_args(
        current_dir,
        &DoctorArgs {
            plan: None,
            external_review_result_ready: false,
        },
    )
}

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

pub fn doctor_for_runtime(runtime: &ExecutionRuntime) -> Result<WorkflowDoctor, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(doctor_from_context(context))
}

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

pub fn doctor_phase_and_next_for_runtime_with_args(
    runtime: &ExecutionRuntime,
    args: &DoctorArgs,
) -> Result<(WorkflowDoctor, String, String), JsonFailure> {
    let context = build_context_with_plan_for_runtime(
        runtime,
        args.plan.as_deref(),
        args.external_review_result_ready,
    )?;
    let phase_text = render_phase_from_context(&context);
    let next_text = render_next_from_context(&context);
    let doctor = doctor_from_context(context);
    Ok((doctor, phase_text, next_text))
}

fn doctor_from_context(context: OperatorContext) -> WorkflowDoctor {
    let doctor_phase = doctor_phase_for_context(&context);
    let contract_state = context
        .plan_contract
        .as_ref()
        .map(|report| report.contract_state.clone())
        .unwrap_or_else(|| context.route.contract_state.clone());
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
        recommended_public_command_argv: context.operator_recommended_public_command_argv.clone(),
        required_inputs: context.operator_required_inputs.clone(),
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
        required_inputs: Vec::new(),
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
            | "plan_fingerprint_mismatch"
    )
}

fn doctor_synthetic_gate_review_failure_class(reason_codes: &[String]) -> String {
    if reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "stale_provenance"
                | "stale_unreviewed"
                | "post_review_repo_write_detected"
                | "plan_fingerprint_mismatch"
        )
    }) {
        String::from("StaleProvenance")
    } else {
        String::from("ExecutionStateNotReady")
    }
}

pub fn render_doctor(current_dir: &Path) -> Result<String, JsonFailure> {
    render_doctor_with_args(
        current_dir,
        &DoctorArgs {
            plan: None,
            external_review_result_ready: false,
        },
    )
}

pub fn render_doctor_with_args(
    current_dir: &Path,
    args: &DoctorArgs,
) -> Result<String, JsonFailure> {
    let doctor = doctor_with_args(current_dir, args)?;
    Ok(render_doctor_output(&doctor))
}

pub fn render_doctor_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let doctor = doctor_for_runtime(runtime)?;
    Ok(render_doctor_output(&doctor))
}

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
        output.push_str(&format!("Blocking scope: {blocking_scope}\n"));
    }
    if let Some(blocking_task) = doctor.blocking_task {
        output.push_str(&format!("Blocking task: {blocking_task}\n"));
    }
    if let Some(external_wait_state) = doctor.external_wait_state.as_deref() {
        output.push_str(&format!("External wait: {external_wait_state}\n"));
    }
    if !doctor.blocking_reason_codes.is_empty() {
        output.push_str(&format!(
            "Blocking reason codes: {}\n",
            reason_codes_text(&doctor.blocking_reason_codes)
        ));
    }
    if let Some(execution_status) = doctor.execution_status.as_ref() {
        append_execution_status_metadata(&mut output, execution_status);
    }
    if let Some(preflight) = doctor.preflight.as_ref() {
        output.push_str(&format!(
            "Preflight reason codes: {}\n",
            reason_codes_text(&preflight.reason_codes)
        ));
    }
    if let Some(gate_review) = doctor.gate_review.as_ref() {
        output.push_str(&format!(
            "Review gate reason codes: {}\n",
            reason_codes_text(&gate_review.reason_codes)
        ));
    }
    if let Some(gate_finish) = doctor.gate_finish.as_ref() {
        output.push_str(&format!(
            "Finish gate reason codes: {}\n",
            reason_codes_text(&gate_finish.reason_codes)
        ));
    }
    output
}

pub fn handoff(current_dir: &Path) -> Result<WorkflowHandoff, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(handoff_from_context(context, None))
}

pub fn handoff_for_runtime(runtime: &ExecutionRuntime) -> Result<WorkflowHandoff, JsonFailure> {
    let context = build_context_for_runtime(runtime)?;
    Ok(handoff_from_context(context, None))
}

fn handoff_from_context(
    context: OperatorContext,
    recommendation: Option<RecommendOutput>,
) -> WorkflowHandoff {
    let contract_state = context
        .plan_contract
        .as_ref()
        .map(|report| report.contract_state.clone())
        .unwrap_or_else(|| context.route.contract_state.clone());
    let execution_started = context
        .execution_status
        .as_ref()
        .map(|status| status.execution_started.clone())
        .unwrap_or_else(|| String::from("no"));
    let (recommended_skill, recommendation_reason) = if let Some(recommendation) =
        recommendation.as_ref()
    {
        (
            recommendation.recommended_skill.clone(),
            recommendation.reason.clone(),
        )
    } else {
        match context.phase.as_str() {
            phase::PHASE_EXECUTING => {
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
            phase::PHASE_IMPLEMENTATION_HANDOFF => (String::new(), reason_text(&context)),
            phase::PHASE_FINAL_REVIEW_PENDING if review_requires_execution_reentry(&context) => {
                let skill = context
                    .execution_status
                    .as_ref()
                    .map(|status| status.execution_mode.clone())
                    .unwrap_or_default();
                (skill, reason_text(&context))
            }
            phase::PHASE_FINAL_REVIEW_PENDING => (
                String::from("featureforge:requesting-code-review"),
                reason_text(&context),
            ),
            phase::PHASE_QA_PENDING
                if context.operator_phase_detail == phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED =>
            {
                (
                    String::from("featureforge:plan-eng-review"),
                    reason_text(&context),
                )
            }
            phase::PHASE_QA_PENDING => {
                (String::from("featureforge:qa-only"), reason_text(&context))
            }
            phase::PHASE_DOCUMENT_RELEASE_PENDING => (
                String::from("featureforge:document-release"),
                reason_text(&context),
            ),
            phase::PHASE_READY_FOR_BRANCH_COMPLETION => (
                String::from("featureforge:finishing-a-development-branch"),
                reason_text(&context),
            ),
            phase::PHASE_PIVOT_REQUIRED => (
                String::from("featureforge:writing-plans"),
                reason_text(&context),
            ),
            phase::PHASE_TASK_CLOSURE_PENDING => {
                let skill = context
                    .execution_status
                    .as_ref()
                    .map(|status| status.execution_mode.clone())
                    .unwrap_or_default();
                let recommendation_reason =
                    task_boundary_next_step_text(&context).unwrap_or_else(|| reason_text(&context));
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
        recommended_public_command_argv: context.operator_recommended_public_command_argv.clone(),
        required_inputs: context.operator_required_inputs.clone(),
        state_kind: context.operator_state_kind.clone(),
        next_public_action: context.operator_next_public_action.clone(),
        blockers: context.operator_blockers.clone(),
        semantic_workspace_tree_id: context.operator_semantic_workspace_tree_id.clone(),
        raw_workspace_tree_id: context.operator_raw_workspace_tree_id.clone(),
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

pub fn operator(current_dir: &Path, args: &OperatorArgs) -> Result<WorkflowOperator, JsonFailure> {
    let context = build_context_with_plan(
        current_dir,
        Some(&args.plan),
        args.external_review_result_ready,
    )?;
    Ok(operator_from_context(context, args))
}

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
    let projection_mode = context
        .execution_status
        .as_ref()
        .map(|status| status.projection_mode.clone())
        .unwrap_or_default();
    let state_dir_projection_paths = context
        .execution_status
        .as_ref()
        .map(|status| status.state_dir_projection_paths.clone())
        .unwrap_or_default();
    let tracked_projection_paths = context
        .execution_status
        .as_ref()
        .map(|status| status.tracked_projection_paths.clone())
        .unwrap_or_default();
    let tracked_projections_current = context
        .execution_status
        .as_ref()
        .is_some_and(|status| status.tracked_projections_current);
    WorkflowOperator {
        schema_version: WORKFLOW_OPERATOR_SCHEMA_VERSION,
        phase: context.operator_phase.clone(),
        phase_detail: context.operator_phase_detail.clone(),
        review_state_status: context.operator_review_state_status.clone(),
        qa_requirement: context.qa_requirement.clone(),
        finish_review_gate_pass_branch_closure_id: context
            .finish_review_gate_pass_branch_closure_id
            .clone(),
        recording_context: context.operator_recording_context.clone(),
        execution_command_context: context.operator_execution_command_context.clone(),
        next_action: context.operator_next_action.clone(),
        recommended_command: context.operator_recommended_command.clone(),
        recommended_public_command_argv: context.operator_recommended_public_command_argv.clone(),
        required_inputs: context.operator_required_inputs.clone(),
        base_branch: context.operator_base_branch.clone(),
        blocking_scope: context.operator_blocking_scope.clone(),
        blocking_task: context.operator_blocking_task,
        external_wait_state: context.operator_external_wait_state.clone(),
        blocking_reason_codes: context.operator_blocking_reason_codes.clone(),
        state_kind: context.operator_state_kind.clone(),
        next_public_action: context.operator_next_public_action.clone(),
        blockers: context.operator_blockers.clone(),
        semantic_workspace_tree_id: context.operator_semantic_workspace_tree_id.clone(),
        raw_workspace_tree_id: context.operator_raw_workspace_tree_id.clone(),
        spec_path: context.route.spec_path.clone(),
        plan_path,
        projection_mode,
        state_dir_projection_paths,
        tracked_projection_paths,
        tracked_projections_current,
    }
}

pub fn render_operator(operator: WorkflowOperator) -> String {
    let recording_context = operator.recording_context.clone();
    let execution_command_context = operator.execution_command_context.clone();
    let mut output = format!(
        "Workflow operator\nPhase: {}\nPhase detail: {}\nReview state: {}\nState kind: {}\nNext action: {}\nSpec: {}\nPlan: {}\n",
        operator.phase,
        operator.phase_detail,
        operator.review_state_status,
        operator.state_kind,
        operator.next_action,
        display_or_none(&operator.spec_path),
        display_or_none(&operator.plan_path)
    );
    if let Some(qa_requirement) = operator.qa_requirement {
        output.push_str(&format!("QA requirement: {qa_requirement}\n"));
    }
    if let Some(checkpoint) = operator.finish_review_gate_pass_branch_closure_id {
        output.push_str(&format!("Finish gate checkpoint: {checkpoint}\n"));
    }
    if !operator.projection_mode.is_empty() {
        output.push_str(&format!("Projection mode: {}\n", operator.projection_mode));
        output.push_str(&format!(
            "State-dir projections: {}\n",
            projection_paths_text(&operator.state_dir_projection_paths)
        ));
        output.push_str(&format!(
            "Tracked projections: {}\n",
            projection_paths_text(&operator.tracked_projection_paths)
        ));
        output.push_str(&format!(
            "Tracked projections current: {}\n",
            operator.tracked_projections_current
        ));
    }
    if let Some(recording_context) = recording_context.as_ref() {
        output.push_str(&format!(
            "Recording context: {}\n",
            format_operator_recording_context(recording_context)
        ));
    }
    if let Some(execution_command_context) = execution_command_context.as_ref() {
        output.push_str(&format!(
            "Execution command context: {}\n",
            format_operator_execution_command_context(execution_command_context)
        ));
    }
    if let Some(blocking_scope) = operator.blocking_scope.as_deref() {
        output.push_str(&format!("Blocking scope: {blocking_scope}\n"));
    }
    if let Some(blocking_task) = operator.blocking_task {
        output.push_str(&format!("Blocking task: {blocking_task}\n"));
    }
    if let Some(external_wait_state) = operator.external_wait_state.as_deref() {
        output.push_str(&format!("External wait: {external_wait_state}\n"));
    }
    if !operator.blocking_reason_codes.is_empty() {
        output.push_str(&format!(
            "Blocking reason codes: {}\n",
            reason_codes_text(&operator.blocking_reason_codes)
        ));
    }
    if !operator.semantic_workspace_tree_id.is_empty() {
        output.push_str(&format!(
            "Semantic workspace tree id: {}\n",
            operator.semantic_workspace_tree_id
        ));
    }
    if let Some(raw_workspace_tree_id) = operator.raw_workspace_tree_id.as_deref() {
        output.push_str(&format!("Raw workspace tree id: {raw_workspace_tree_id}\n"));
    }
    if let Some(next_public_action) = operator.next_public_action.as_ref() {
        output.push_str(&format!(
            "Next public action: {}\n",
            next_public_action.command
        ));
    }
    if !operator.blockers.is_empty() {
        output.push_str("Blockers:\n");
        for blocker in &operator.blockers {
            output.push_str(&format!(
                "- {} scope={} next={}\n",
                blocker.category,
                blocker.scope_key,
                blocker.next_public_action.as_deref().unwrap_or("none")
            ));
        }
    }
    if let Some(recommended_command) = operator.recommended_command {
        output.push_str(&format!("Recommended command: {recommended_command}\n"));
    }
    output
}

pub fn render_handoff(current_dir: &Path) -> Result<String, JsonFailure> {
    let handoff = handoff(current_dir)?;
    Ok(render_handoff_output(&handoff))
}

pub fn render_handoff_for_runtime(runtime: &ExecutionRuntime) -> Result<String, JsonFailure> {
    let handoff = handoff_for_runtime(runtime)?;
    Ok(render_handoff_output(&handoff))
}

fn render_handoff_output(handoff: &WorkflowHandoff) -> String {
    let mut output = String::new();
    output.push_str("Workflow handoff\n");
    output.push_str(&format!("Phase: {}\n", handoff.phase));
    output.push_str(&format!("Phase detail: {}\n", handoff.phase_detail));
    output.push_str(&format!("Review state: {}\n", handoff.review_state_status));
    output.push_str(&format!("Route status: {}\n", handoff.route_status));
    output.push_str(&format!("Next action: {}\n", handoff.next_action));
    output.push_str(&format!(
        "Recommended command: {}\n",
        optional_text(handoff.recommended_command.as_deref())
    ));
    output.push_str(&format!("State kind: {}\n", handoff.state_kind));
    if !handoff.semantic_workspace_tree_id.is_empty() {
        output.push_str(&format!(
            "Semantic workspace tree id: {}\n",
            handoff.semantic_workspace_tree_id
        ));
    }
    if let Some(raw_workspace_tree_id) = handoff.raw_workspace_tree_id.as_deref() {
        output.push_str(&format!("Raw workspace tree id: {raw_workspace_tree_id}\n"));
    }
    if let Some(next_public_action) = handoff.next_public_action.as_ref() {
        output.push_str(&format!(
            "Next public action: {}\n",
            next_public_action.command
        ));
    }
    if !handoff.blockers.is_empty() {
        output.push_str("Blockers:\n");
        for blocker in &handoff.blockers {
            output.push_str(&format!(
                "- {} scope={} next={}\n",
                blocker.category,
                blocker.scope_key,
                blocker.next_public_action.as_deref().unwrap_or("none")
            ));
        }
    }
    output.push_str(&format!("Spec: {}\n", display_or_none(&handoff.spec_path)));
    output.push_str(&format!("Plan: {}\n", display_or_none(&handoff.plan_path)));
    if !handoff.recommended_skill.is_empty() {
        output.push_str(&format!(
            "Recommended skill: {}\n",
            handoff.recommended_skill
        ));
    }
    if !handoff.recommendation_reason.is_empty() {
        output.push_str(&format!("Reason: {}\n", handoff.recommendation_reason));
    }
    if let Some(execution_status) = handoff.execution_status.as_ref() {
        append_execution_status_metadata(&mut output, execution_status);
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
    let (routing, route_decision) = if let Some(plan_path) = plan_override {
        if !current_dir.join(plan_path).is_file() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "Workflow plan override file does not exist.",
            ));
        }
        let runtime = ExecutionRuntime::discover(current_dir)?;
        let routing = query_workflow_routing_state_for_runtime(
            &runtime,
            Some(plan_path),
            external_review_result_ready,
        )?;
        let route_decision = routing.route_decision.clone();
        (routing, route_decision)
    } else {
        let routing =
            query_workflow_routing_state(current_dir, None, external_review_result_ready)?;
        let route_decision = routing.route_decision.clone();
        (routing, route_decision)
    };
    build_context_from_routing(routing, route_decision)
}

fn build_context_with_plan_for_runtime(
    runtime: &ExecutionRuntime,
    plan_override: Option<&Path>,
    external_review_result_ready: bool,
) -> Result<OperatorContext, JsonFailure> {
    let (routing, route_decision) = if let Some(plan_path) = plan_override {
        if !runtime.repo_root.join(plan_path).is_file() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "Workflow plan override file does not exist.",
            ));
        }
        let routing = query_workflow_routing_state_for_runtime(
            runtime,
            Some(plan_path),
            external_review_result_ready,
        )?;
        let route_decision = routing.route_decision.clone();
        (routing, route_decision)
    } else {
        let routing =
            query_workflow_routing_state_for_runtime(runtime, None, external_review_result_ready)?;
        let route_decision = routing.route_decision.clone();
        (routing, route_decision)
    };
    build_context_from_routing(routing, route_decision)
}

fn build_context_from_routing(
    routing: ExecutionRoutingState,
    route_decision_override: Option<RuntimeRouteDecision>,
) -> Result<OperatorContext, JsonFailure> {
    let route_decision = route_decision_override.unwrap_or_else(|| {
        let blocking_records = routing
            .execution_status
            .as_ref()
            .map(|status| status.blocking_records.as_slice())
            .unwrap_or(&[]);
        route_decision_from_routing(&routing, blocking_records)
    });
    let ExecutionRoutingState {
        route,
        execution_status,
        preflight,
        gate_review,
        gate_finish,
        workflow_phase: _,
        phase: routing_phase,
        phase_detail: _,
        review_state_status,
        qa_requirement,
        finish_review_gate_pass_branch_closure_id,
        recording_context: _,
        execution_command_context,
        next_action: _,
        recommended_command: _,
        base_branch,
        blocking_scope,
        blocking_task,
        external_wait_state,
        blocking_reason_codes,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id: _,
        ..
    } = routing;
    let operator_phase = execution_status
        .as_ref()
        .and_then(|status| status.phase.clone())
        .unwrap_or_else(|| route_decision.phase.clone());
    let operator_phase_detail = execution_status
        .as_ref()
        .map(|status| status.phase_detail.clone())
        .unwrap_or_else(|| route_decision.phase_detail.clone());
    let operator_next_action = execution_status
        .as_ref()
        .map(|status| status.next_action.clone())
        .unwrap_or_else(|| route_decision.next_action.clone());
    let operator_recommended_command = route_decision.recommended_command.clone();
    let operator_recommended_public_command_argv = route_decision.public_command_argv();
    let operator_required_inputs = route_decision.required_inputs.clone();
    let operator_recording_context =
        route_decision
            .recording_context
            .as_ref()
            .map(|context| WorkflowOperatorRecordingContext {
                task_number: context.task_number,
                dispatch_id: context.dispatch_id.clone(),
                branch_closure_id: context.branch_closure_id.clone(),
            });
    let operator_execution_command_context = execution_status
        .as_ref()
        .and_then(|status| status.execution_command_context.as_ref())
        .map(|context| WorkflowOperatorExecutionCommandContext {
            command_kind: context.command_kind.clone(),
            task_number: context.task_number,
            step_id: context.step_id,
        })
        .or_else(|| {
            execution_command_context.map(|context| WorkflowOperatorExecutionCommandContext {
                command_kind: context.command_kind,
                task_number: context.task_number,
                step_id: context.step_id,
            })
        });
    let preflight_not_started = execution_status
        .as_ref()
        .is_some_and(|status| status.execution_started != "yes");
    let display_phase = if route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY
        && preflight_not_started
        && matches!(
            routing_phase.as_str(),
            phase::PHASE_IMPLEMENTATION_HANDOFF | phase::PHASE_EXECUTION_PREFLIGHT
        ) {
        String::from(phase::PHASE_EXECUTION_PREFLIGHT)
    } else if operator_phase == phase::PHASE_PIVOT_REQUIRED
        || execution_status
            .as_ref()
            .is_some_and(|status| status.execution_started == "yes")
        || routing_phase != phase::PHASE_IMPLEMENTATION_HANDOFF
    {
        operator_phase.clone()
    } else {
        routing_phase
    };
    let operator_base_branch = base_branch;
    let operator_review_state_status = execution_status
        .as_ref()
        .map(|status| status.review_state_status.clone())
        .unwrap_or(review_state_status);
    let mut operator_blocking_scope = execution_status
        .as_ref()
        .and_then(|status| status.blocking_scope.clone())
        .or(blocking_scope);
    let mut operator_blocking_task = execution_status
        .as_ref()
        .and_then(|status| status.blocking_task)
        .or(blocking_task);
    let operator_external_wait_state = execution_status
        .as_ref()
        .and_then(|status| status.external_wait_state.clone())
        .or(external_wait_state);
    let operator_blocking_reason_codes = execution_status
        .as_ref()
        .map(|status| status.blocking_reason_codes.clone())
        .unwrap_or(blocking_reason_codes);
    let operator_diagnostic_reason_codes = execution_status
        .as_ref()
        .map(|status| merge_status_projection_diagnostics(diagnostic_reason_codes.clone(), status))
        .unwrap_or(diagnostic_reason_codes);
    if operator_phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && let Some(task_number) = operator_execution_command_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        operator_blocking_scope = Some(String::from("task"));
        operator_blocking_task = Some(task_number);
    } else if operator_phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && let Some(task_number) = execution_status
            .as_ref()
            .and_then(task_blocking_record_task)
    {
        operator_blocking_scope = Some(String::from("task"));
        operator_blocking_task = Some(task_number);
    } else if operator_phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && let Some(task_number) = operator_recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        operator_blocking_scope = Some(String::from("task"));
        operator_blocking_task = Some(task_number);
    }
    let (
        operator_state_kind,
        operator_next_public_action,
        operator_blockers,
        operator_semantic_workspace_tree_id,
        operator_raw_workspace_tree_id,
    ) = execution_status
        .as_ref()
        .map(|status| {
            (
                status.state_kind.clone(),
                status.next_public_action.clone(),
                status.blockers.clone(),
                status.semantic_workspace_tree_id.clone(),
                status.raw_workspace_tree_id.clone(),
            )
        })
        .unwrap_or_else(|| {
            (
                route_decision.state_kind.clone(),
                route_decision.next_public_action.clone(),
                route_decision.blockers.clone(),
                String::new(),
                None,
            )
        });
    let plan_contract = if route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY {
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
        phase: display_phase,
        operator_phase,
        operator_phase_detail,
        operator_review_state_status,
        operator_recording_context,
        operator_execution_command_context,
        operator_next_action,
        operator_recommended_command,
        operator_recommended_public_command_argv,
        operator_required_inputs,
        operator_base_branch,
        operator_blocking_scope,
        operator_blocking_task,
        operator_external_wait_state,
        operator_blocking_reason_codes,
        operator_state_kind,
        operator_next_public_action,
        operator_blockers,
        operator_semantic_workspace_tree_id,
        operator_raw_workspace_tree_id,
        reason_family,
        diagnostic_reason_codes: operator_diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        finish_review_gate_pass_branch_closure_id,
        qa_requirement,
    })
}

fn task_blocking_record_task(status: &PlanExecutionStatus) -> Option<u32> {
    status.blocking_records.iter().find_map(|record| {
        if record.scope_type != "task" {
            return None;
        }
        let raw = record.scope_key.strip_prefix("task-")?;
        let digits = raw
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>();
        (!digits.is_empty())
            .then(|| digits.parse::<u32>().ok())
            .flatten()
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
    if context.route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY
        && context
            .execution_status
            .as_ref()
            .is_some_and(|status| status.execution_started != "yes")
        && matches!(
            context.phase.as_str(),
            phase::PHASE_IMPLEMENTATION_HANDOFF | phase::PHASE_EXECUTION_PREFLIGHT
        )
    {
        return String::from(phase::PHASE_EXECUTION_PREFLIGHT);
    }

    if context.phase == phase::PHASE_HANDOFF_REQUIRED
        && context.operator_phase_detail == phase::DETAIL_EXECUTION_IN_PROGRESS
        && context
            .execution_status
            .as_ref()
            .is_some_and(|status| status.execution_started == "yes")
    {
        return String::from(phase::PHASE_EXECUTING);
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
    if context.phase == phase::PHASE_QA_PENDING
        && context.operator_phase_detail == phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED
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
        phase::PHASE_EXECUTION_PREFLIGHT | phase::PHASE_IMPLEMENTATION_HANDOFF => {
            if plan_path.is_empty() {
                String::from("Return to execution preflight for the approved plan.")
            } else {
                format!("Return to execution preflight for the approved plan: {plan_path}")
            }
        }
        phase::PHASE_EXECUTING => {
            if plan_path.is_empty() {
                String::from("Return to the current execution flow for the approved plan.")
            } else {
                format!("Return to the current execution flow for the approved plan: {plan_path}")
            }
        }
        "contract_drafting"
        | "contract_pending_approval"
        | "contract_approved"
        | "evaluating"
        | phase::PHASE_TASK_CLOSURE_PENDING
        | phase::PHASE_HANDOFF_REQUIRED => {
            if plan_path.is_empty() {
                String::from("Return to the current execution flow for the approved plan.")
            } else {
                format!("Return to the current execution flow for the approved plan: {plan_path}")
            }
        }
        phase::PHASE_PIVOT_REQUIRED => {
            if plan_path.is_empty() {
                String::from("Update and re-approve the plan before continuing execution.")
            } else {
                format!("Update and re-approve the plan before continuing execution: {plan_path}")
            }
        }
        phase::PHASE_FINAL_REVIEW_PENDING => {
            if plan_path.is_empty() {
                String::from("Use featureforge:requesting-code-review for the final review gate.")
            } else {
                format!(
                    "Use featureforge:requesting-code-review for the approved plan before branch completion: {plan_path}"
                )
            }
        }
        phase::PHASE_QA_PENDING => String::from(
            "Run featureforge:qa-only and return with a fresh QA result artifact before branch completion.",
        ),
        phase::PHASE_DOCUMENT_RELEASE_PENDING => String::from(
            "Run featureforge:document-release and return with a fresh release-readiness artifact before branch completion.",
        ),
        phase::PHASE_READY_FOR_BRANCH_COMPLETION => {
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
        phase::PHASE_EXECUTION_PREFLIGHT => String::from(
            "The approved plan matches the latest approved spec and preflight is the next safe boundary.",
        ),
        phase::PHASE_IMPLEMENTATION_HANDOFF => context
            .execution_preflight_block_reason
            .clone()
            .unwrap_or_else(|| {
                String::from(
                    "The approved plan is ready, but execution preflight is still blocked by the current workspace state.",
                )
            }),
        phase::PHASE_EXECUTING => task_boundary_reason_text(context).unwrap_or_else(|| {
            String::from(
                "Execution already started for the approved plan and should continue through the current execution flow.",
            )
        }),
        phase::PHASE_PIVOT_REQUIRED => {
            String::from("Execution is blocked pending an approved plan revision.")
        }
        phase::PHASE_TASK_CLOSURE_PENDING => {
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
        | phase::PHASE_HANDOFF_REQUIRED => String::from(
            "Execution already started for the approved plan and should continue through the current execution flow.",
        ),
        phase::PHASE_FINAL_REVIEW_PENDING => gate_first_diagnostic_message(context.gate_review.as_ref())
            .or_else(|| gate_first_diagnostic_message(context.gate_finish.as_ref()))
            .unwrap_or_else(|| {
                String::from("Execution is blocked on the final review gate for the approved plan.")
            }),
        phase::PHASE_QA_PENDING | phase::PHASE_DOCUMENT_RELEASE_PENDING => {
            gate_first_diagnostic_message(context.gate_finish.as_ref())
                .unwrap_or_else(|| context.route.reason.clone())
        }
        phase::PHASE_READY_FOR_BRANCH_COMPLETION => {
            String::from("All required late-stage artifacts are fresh for the current HEAD.")
        }
        _ => context.route.reason.clone(),
    }
}

fn display_or_none(value: &str) -> &str {
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
    context.phase == phase::PHASE_FINAL_REVIEW_PENDING
        && context.operator_phase_detail != phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
        && context
            .gate_review
            .as_ref()
            .is_some_and(|gate| !gate.allowed)
}

fn task_boundary_reason_text(context: &OperatorContext) -> Option<String> {
    let blocking_task = context.operator_blocking_task?;
    let message = match context.operator_phase_detail.as_str() {
        "task_review_dispatch_required" => format!(
            "Task {blocking_task} closure reached a retired task-review dispatch lane. Rerun workflow/operator after repairing runtime routing; normal task closure must use close-current-task."
        ),
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING => {
            if task_review_result_pending_requires_verification(context) {
                format!(
                    "Task {blocking_task} closure cannot be recorded/refreshed yet. Run verification and then record task closure for Task {blocking_task}."
                )
            } else if operator_blocking_reason_present(context, "task_review_not_independent")
                || operator_blocking_reason_present(context, "task_review_artifact_malformed")
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
        phase::DETAIL_TASK_CLOSURE_RECORDING_READY => {
            if operator_blocking_reason_present(context, "task_closure_baseline_bridge_ready") {
                format!(
                    "Task {blocking_task} execution replay is already complete enough to refresh closure truth. Record/refresh Task {blocking_task} closure now. Do not reopen the step again."
                )
            } else {
                format!(
                    "Task {blocking_task} closure is ready to record/refresh. Record or refresh Task {blocking_task} closure now."
                )
            }
        }
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => {
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
        || context.phase == phase::PHASE_TASK_CLOSURE_PENDING
        || (context.phase == phase::PHASE_EXECUTING
            && context.operator_phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
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
    output.push_str(&format!(
        "Execution reason codes: {}\n",
        reason_codes_text(&status.reason_codes)
    ));
    output.push_str(&format!(
        "Evaluator required kinds: {}\n",
        evaluator_kinds_text(&status.required_evaluator_kinds)
    ));
    output.push_str(&format!(
        "Evaluator completed kinds: {}\n",
        evaluator_kinds_text(&status.completed_evaluator_kinds)
    ));
    output.push_str(&format!(
        "Evaluator pending kinds: {}\n",
        evaluator_kinds_text(&status.pending_evaluator_kinds)
    ));
    output.push_str(&format!(
        "Evaluator non-passing kinds: {}\n",
        evaluator_kinds_text(&status.non_passing_evaluator_kinds)
    ));
    output.push_str(&format!(
        "Evaluator last kind: {}\n",
        optional_evaluator_kind_text(status.last_evaluation_evaluator_kind)
    ));
    output.push_str(&format!(
        "Write authority state: {}\n",
        status.write_authority_state
    ));
    output.push_str(&format!(
        "Write authority holder: {}\n",
        optional_text(status.write_authority_holder.as_deref())
    ));
    output.push_str(&format!(
        "Write authority worktree: {}\n",
        optional_text(status.write_authority_worktree.as_deref())
    ));
    output.push_str(&format!("Strategy state: {}\n", status.strategy_state));
    output.push_str(&format!(
        "Strategy checkpoint kind: {}\n",
        status.strategy_checkpoint_kind
    ));
    output.push_str(&format!(
        "Strategy checkpoint fingerprint: {}\n",
        optional_text(status.last_strategy_checkpoint_fingerprint.as_deref())
    ));
    output.push_str(&format!(
        "Strategy reset required: {}\n",
        if status.strategy_reset_required {
            "yes"
        } else {
            "no"
        }
    ));
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
            .map(evaluator_kind_text)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn projection_paths_text(paths: &[String]) -> String {
    if paths.is_empty() {
        String::from("none")
    } else {
        paths.join(", ")
    }
}

fn evaluator_kind_text(kind: &EvaluatorKind) -> &'static str {
    match kind {
        EvaluatorKind::SpecCompliance => "spec_compliance",
        EvaluatorKind::CodeQuality => "code_quality",
    }
}

fn optional_evaluator_kind_text(value: Option<EvaluatorKind>) -> &'static str {
    match value {
        Some(value) => evaluator_kind_text(&value),
        None => "none",
    }
}

fn optional_text(value: Option<&str>) -> &str {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("none")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::command_eligibility::PublicCommandInputKind;
    use crate::workflow::status::WorkflowRoute;

    fn task_boundary_context(
        phase_detail: &str,
        blocking_reason_codes: &[&str],
        recommended_command: Option<&str>,
    ) -> OperatorContext {
        OperatorContext {
            route: WorkflowRoute {
                schema_version: 3,
                status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
                next_skill: String::from("featureforge:executing-plans"),
                spec_path: String::from("docs/featureforge/specs/example.md"),
                plan_path: String::from("docs/featureforge/plans/example.md"),
                contract_state: String::from("approved"),
                reason_codes: Vec::new(),
                diagnostics: Vec::new(),
                plan_fidelity_review: None,
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
            phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
            operator_phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
            operator_phase_detail: String::from(phase_detail),
            operator_review_state_status: String::from("clean"),
            operator_recording_context: None,
            operator_execution_command_context: None,
            operator_next_action: String::from("wait for external review result"),
            operator_recommended_command: recommended_command.map(str::to_owned),
            operator_recommended_public_command_argv: None,
            operator_required_inputs: Vec::new(),
            operator_base_branch: Some(String::from("main")),
            operator_blocking_scope: Some(String::from("task")),
            operator_blocking_task: Some(1),
            operator_external_wait_state: None,
            operator_blocking_reason_codes: blocking_reason_codes
                .iter()
                .map(|reason| String::from(*reason))
                .collect(),
            operator_state_kind: String::from("actionable_public_command"),
            operator_next_public_action: None,
            operator_blockers: Vec::new(),
            operator_semantic_workspace_tree_id: String::new(),
            operator_raw_workspace_tree_id: None,
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
            phase: String::from(phase::PHASE_EXECUTING),
            phase_detail: String::from(phase::DETAIL_EXECUTION_IN_PROGRESS),
            review_state_status: String::from("clean"),
            qa_requirement: Some(String::from("required")),
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
            recommended_command: None,
            recommended_public_command_argv: None,
            required_inputs: vec![PublicCommandInputRequirement {
                name: String::from("claim"),
                kind: PublicCommandInputKind::Text,
                values: Vec::new(),
                must_exist: false,
                required_when: None,
            }],
            base_branch: Some(String::from("main")),
            blocking_scope: Some(String::from("task")),
            blocking_task: Some(1),
            external_wait_state: None,
            blocking_reason_codes: vec![String::from("stale_unreviewed")],
            state_kind: String::from("actionable_public_command"),
            next_public_action: Some(RuntimeNextPublicAction {
                command: String::from("featureforge plan execution close-current-task --plan ..."),
                args_template: Some(String::from(
                    "featureforge plan execution close-current-task --plan ...",
                )),
            }),
            blockers: vec![RuntimeBlocker {
                category: String::from("task_boundary"),
                scope_type: String::from("task"),
                scope_key: String::from("task-1"),
                record_id: Some(String::from("dispatch-1")),
                next_public_action: Some(String::from("close_current_task")),
                details: String::from("Task review result pending."),
            }],
            semantic_workspace_tree_id: String::from("semantic_tree:abc"),
            raw_workspace_tree_id: Some(String::from("git_tree:def")),
            spec_path: String::from("docs/featureforge/specs/sample.md"),
            plan_path: String::from("docs/featureforge/plans/sample.md"),
            projection_mode: String::from("state_dir_only"),
            state_dir_projection_paths: vec![String::from("/tmp/state/projection.md")],
            tracked_projection_paths: vec![String::from(
                "docs/featureforge/execution-evidence/sample.md",
            )],
            tracked_projections_current: false,
        });

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
            phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
            &["prior_task_verification_missing"],
            Some(
                "featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            ),
        );

        let reason = task_boundary_reason_text(&context)
            .expect("task-boundary reason text should be available for task_review_result_pending");
        assert!(
            reason.contains("Run verification and then record task closure"),
            "verification-missing task-boundary reason text should mention verification + closure recording, got {reason}"
        );

        let next_step = task_boundary_next_step_text(&context).expect(
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
