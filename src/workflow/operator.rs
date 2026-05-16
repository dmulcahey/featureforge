//! Workflow routing consumes the execution-owned query surface and maps it into
//! public phases and next-action recommendations.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use crate::cli::workflow::OperatorArgs;
use crate::contracts::plan::AnalyzePlanReport;
use crate::contracts::workflow::{WorkflowPhase, WorkflowRoute};
use crate::diagnostics::{DiagnosticError, FailureClass, JsonFailure};
use crate::execution::closure_diagnostics::{
    TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_ARTIFACT_MALFORMED,
    TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_NOT_INDEPENDENT,
    TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN, merge_status_projection_diagnostics,
    task_boundary_closure_baseline_bridge_ready_reason_code,
};
use crate::execution::command_eligibility::{PublicCommandInputRequirement, PublicCommandKind};
use crate::execution::harness::{EvaluatorKind, HarnessPhase};
use crate::execution::next_action::runtime_route_is_diagnostic;
use crate::execution::public_command_types::{
    PublicCommandInputValues, RecommendedPublicCommandArgv, RecommendedPublicCommandTemplate,
    materialize_public_command_argv,
};
use crate::execution::query::{
    ExecutionRoutingState, query_workflow_routing_state, query_workflow_routing_state_for_runtime,
    task_review_result_requires_verification,
};
use crate::execution::review_route_tokens::{
    doctor_synthetic_gate_review_failure_class, doctor_synthetic_gate_review_reason_code,
};
use crate::execution::route_plan::{
    Blocker as RuntimeBlocker, NextPublicAction as RuntimeNextPublicAction,
    state_kind_is_blocked_runtime_bug,
};
use crate::execution::runtime_provenance::{
    ControlPlaneSource, RuntimeProvenance, SelfHostingContext, StateDirKind,
};
use crate::execution::state::{
    ExecutionRuntime, GateResult, PlanExecutionStatus, PublicRepairTarget,
};
use crate::execution::status_assembly::public_status_warning_code;
use crate::execution::status_support::PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT;
use crate::execution::topology::RecommendOutput;
use crate::execution::{phase, workflow_operator_requery_command};
use crate::workflow::doctor_dashboard::{
    render_doctor_dashboard, render_doctor_dashboard_with_external_review_hint,
};
use crate::workflow::doctor_resolution::{
    DoctorResolution, DoctorResolutionInput, derive_doctor_resolution,
};
use crate::workflow::recommendation::{
    ExplicitRecommendation, HandoffRecommendationInput, WorkflowRecommendation,
    handoff_recommendation, next_text_for_phase,
};
use crate::workflow::status::MISSING_PLAN_OVERRIDE_MESSAGE;

const WORKFLOW_PHASE_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_DOCTOR_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_HANDOFF_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_OPERATOR_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperatorJsonGuidancePurpose {
    CommandExecutionAuthority,
    DiagnosticOrientation,
    RouteOrientation,
}

#[derive(Debug, Clone)]
pub struct DoctorArgs {
    pub plan: Option<PathBuf>,
    pub external_review_result_ready: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
    pub resolution: DoctorResolution,
    pub diagnostic_reason_codes: Vec<String>,
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
    pub runtime_provenance: Option<RuntimeProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_hosting_warning: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
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
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qa_requirement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorkflowOperatorRecordingContext")]
    pub recording_context: Option<WorkflowOperatorRecordingContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WorkflowOperatorExecutionCommandContext")]
    pub execution_command_context: Option<WorkflowOperatorExecutionCommandContext>,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_argv: RecommendedPublicCommandArgv,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_reason_codes: Vec<String>,
    pub state_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_public_action: Option<RuntimeNextPublicAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<RuntimeBlocker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_repair_targets: Vec<PublicRepairTarget>,
    pub semantic_workspace_tree_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_workspace_tree_id: Option<String>,
    pub spec_path: String,
    pub plan_path: String,
    pub projection_mode: String,
    pub state_dir_projection_paths: Vec<String>,
    pub tracked_projection_paths: Vec<String>,
    pub tracked_projections_current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_provenance: Option<RuntimeProvenance>,
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
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<u32>,
}

struct OperatorContext {
    route: WorkflowRoute,
    runtime_provenance: Option<RuntimeProvenance>,
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
    operator_recommended_public_command_template: RecommendedPublicCommandTemplate,
    operator_required_inputs: Vec<PublicCommandInputRequirement>,
    operator_base_branch: Option<String>,
    operator_blocking_scope: Option<String>,
    operator_blocking_task: Option<u32>,
    operator_external_wait_state: Option<String>,
    operator_blocking_reason_codes: Vec<String>,
    operator_state_kind: String,
    operator_next_public_action: Option<RuntimeNextPublicAction>,
    operator_blockers: Vec<RuntimeBlocker>,
    operator_public_repair_targets: Vec<PublicRepairTarget>,
    operator_semantic_workspace_tree_id: String,
    operator_raw_workspace_tree_id: Option<String>,
    external_review_result_ready: bool,
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
    let required_inputs = required_inputs_line(&context.operator_required_inputs);
    let json_guidance_purpose = operator_json_guidance_purpose_for_context(context);
    format!(
        "Workflow phase: {}\nPhase detail: {}\nReview state: {}\nRoute status: {}\nNext action: {}\nDisplay command summary: {}\n{}\n{}Next: {}\nSpec: {}\nPlan: {}\n",
        context.phase,
        context.operator_phase_detail,
        context.operator_review_state_status,
        context.route.status,
        next_action_for_context(context),
        optional_text(context.operator_recommended_command.as_deref()),
        operator_json_rerun_guidance(
            &context.route.plan_path,
            context.external_review_result_ready,
            json_guidance_purpose,
        ),
        required_inputs.as_deref().unwrap_or(""),
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
    let gate_review = doctor_gate_review(&context).map(sanitize_doctor_gate_warning_codes);
    let gate_finish = context
        .gate_finish
        .clone()
        .map(sanitize_doctor_gate_warning_codes);
    let preflight = context
        .preflight
        .clone()
        .map(sanitize_doctor_gate_warning_codes);
    let runtime_provenance = context.runtime_provenance.clone();
    let self_hosting_warning = doctor_self_hosting_warning(runtime_provenance.as_ref());
    let resolution = derive_doctor_resolution(DoctorResolutionInput {
        command_available: context.operator_recommended_public_command_argv.is_some(),
        required_input_count: context.operator_required_inputs.len(),
        external_wait_state: context.operator_external_wait_state.as_deref(),
        blocking_reason_codes: &context.operator_blocking_reason_codes,
        diagnostic_reason_codes: &context.diagnostic_reason_codes,
        state_kind: &context.operator_state_kind,
    });

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
        recommended_public_command_template: context
            .operator_recommended_public_command_template
            .clone(),
        required_inputs: context.operator_required_inputs.clone(),
        resolution,
        diagnostic_reason_codes: context.diagnostic_reason_codes.clone(),
        blocking_scope: context.operator_blocking_scope.clone(),
        blocking_task: context.operator_blocking_task,
        external_wait_state: context.operator_external_wait_state.clone(),
        blocking_reason_codes: context.operator_blocking_reason_codes.clone(),
        spec_path: context.route.spec_path.clone(),
        plan_path: context.route.plan_path.clone(),
        contract_state,
        route: context.route,
        runtime_provenance,
        self_hosting_warning,
        execution_status: context.execution_status,
        plan_contract: context.plan_contract,
        preflight,
        gate_review,
        gate_finish,
        task_review_dispatch_id: context.task_review_dispatch_id,
        final_review_dispatch_id: context.final_review_dispatch_id,
    }
}

fn sanitize_doctor_gate_warning_codes(mut gate: GateResult) -> GateResult {
    gate.recommended_command = None;
    gate.warning_codes = gate
        .warning_codes
        .iter()
        .map(|code| public_status_warning_code(code))
        .collect();
    gate
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
        if gate_review.failure_class == FailureClass::StaleExecutionEvidence.as_str()
            || doctor_synthetic_gate_review_failure_class(
                gate_review.reason_codes.iter().map(String::as_str),
            ) == FailureClass::StaleProvenance.as_str()
        {
            gate_review.failure_class = String::from(FailureClass::StaleProvenance.as_str());
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
        failure_class: String::from(doctor_synthetic_gate_review_failure_class(
            reason_codes.iter().map(String::as_str),
        )),
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
        recommended_command: None,
        recommended_public_command_template: context
            .operator_recommended_public_command_template
            .clone(),
        required_inputs: context.operator_required_inputs.clone(),
        rederive_via_workflow_operator: None,
    })
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
    Ok(render_doctor_dashboard_with_external_review_hint(
        &doctor,
        args.external_review_result_ready,
    ))
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
    Ok(render_doctor_dashboard_with_external_review_hint(
        &doctor,
        args.external_review_result_ready,
    ))
}

fn render_doctor_output(doctor: &WorkflowDoctor) -> String {
    render_doctor_dashboard(doctor)
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
    let projected_recommendation =
        handoff_recommendation_for_context(&context, &execution_started, recommendation.as_ref());

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
        recommended_public_command_template: context
            .operator_recommended_public_command_template
            .clone(),
        required_inputs: context.operator_required_inputs.clone(),
        state_kind: context.operator_state_kind.clone(),
        next_public_action: context.operator_next_public_action.clone(),
        blockers: context.operator_blockers.clone(),
        semantic_workspace_tree_id: context.operator_semantic_workspace_tree_id.clone(),
        raw_workspace_tree_id: context.operator_raw_workspace_tree_id.clone(),
        reason_family: context.reason_family.clone(),
        diagnostic_reason_codes: context.diagnostic_reason_codes.clone(),
        recommended_skill: projected_recommendation.skill,
        recommendation_reason: projected_recommendation.reason,
        route: context.route,
        execution_status: context.execution_status,
        plan_contract: context.plan_contract,
        recommendation,
    }
}

pub fn operator(current_dir: &Path, args: &OperatorArgs) -> Result<WorkflowOperator, JsonFailure> {
    let mut context = build_context_with_plan(
        current_dir,
        Some(&args.plan),
        args.external_review_result_ready,
    )?;
    apply_operator_template_inputs(&mut context, args)?;
    Ok(operator_from_context(context, args))
}

pub fn operator_for_runtime(
    runtime: &ExecutionRuntime,
    args: &OperatorArgs,
) -> Result<WorkflowOperator, JsonFailure> {
    let mut context = build_context_with_plan_for_runtime(
        runtime,
        Some(&args.plan),
        args.external_review_result_ready,
    )?;
    apply_operator_template_inputs(&mut context, args)?;
    Ok(operator_from_context(context, args))
}

fn apply_operator_template_inputs(
    context: &mut OperatorContext,
    args: &OperatorArgs,
) -> Result<(), JsonFailure> {
    if args.inputs.is_empty() {
        return Ok(());
    }
    let Some(template) = context
        .operator_recommended_public_command_template
        .as_ref()
    else {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "workflow operator --input requires a current recommended_public_command_template route.",
        ));
    };
    let input_values = parse_operator_input_values(&args.inputs)?;
    let argv = materialize_public_command_argv(template, &input_values).map_err(|error| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("workflow operator could not materialize the public command template: {error}"),
        )
    })?;
    context.operator_recommended_public_command_argv = Some(argv);
    context.operator_recommended_public_command_template = None;
    context.operator_required_inputs.clear();
    Ok(())
}

fn parse_operator_input_values(inputs: &[String]) -> Result<PublicCommandInputValues, JsonFailure> {
    let mut values = PublicCommandInputValues::new();
    for input in inputs {
        let Some((name, value)) = input.split_once('=') else {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!("workflow operator --input value `{input}` must use NAME=VALUE syntax."),
            ));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "workflow operator --input requires a non-empty input name.",
            ));
        }
        if values.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!("workflow operator --input specified `{name}` more than once."),
            ));
        }
    }
    Ok(values)
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
        recommended_public_command_template: context
            .operator_recommended_public_command_template
            .clone(),
        required_inputs: context.operator_required_inputs.clone(),
        base_branch: context.operator_base_branch.clone(),
        blocking_scope: context.operator_blocking_scope.clone(),
        blocking_task: context.operator_blocking_task,
        external_wait_state: context.operator_external_wait_state.clone(),
        blocking_reason_codes: context.operator_blocking_reason_codes.clone(),
        diagnostic_reason_codes: context.diagnostic_reason_codes.clone(),
        state_kind: context.operator_state_kind.clone(),
        next_public_action: context.operator_next_public_action.clone(),
        blockers: context.operator_blockers.clone(),
        public_repair_targets: context.operator_public_repair_targets.clone(),
        semantic_workspace_tree_id: context.operator_semantic_workspace_tree_id.clone(),
        raw_workspace_tree_id: context.operator_raw_workspace_tree_id.clone(),
        spec_path: context.route.spec_path.clone(),
        plan_path,
        projection_mode,
        state_dir_projection_paths,
        tracked_projection_paths,
        tracked_projections_current,
        runtime_provenance: context.runtime_provenance,
    }
}

pub fn render_operator(operator: WorkflowOperator) -> String {
    render_operator_with_external_review_hint(operator, false)
}

pub fn render_operator_with_external_review_hint(
    operator: WorkflowOperator,
    external_review_result_ready: bool,
) -> String {
    let recording_context = operator.recording_context.clone();
    let execution_command_context = operator.execution_command_context.clone();
    let json_guidance_purpose = operator_json_guidance_purpose_for_operator(&operator);
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
    if let Some(warning) = doctor_self_hosting_warning(operator.runtime_provenance.as_ref()) {
        output.push_str(&format!("Warning: {warning}\n"));
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
    let renders_command_summary = operator.recommended_command.is_some()
        || operator.recommended_public_command_template.is_some()
        || !operator.required_inputs.is_empty()
        || operator.next_public_action.is_some()
        || operator
            .blockers
            .iter()
            .any(|blocker| blocker.next_public_action.is_some());
    let renders_diagnostic_route_guidance =
        runtime_route_is_diagnostic(&operator.state_kind, &operator.phase_detail);
    if let Some(next_public_action) = operator.next_public_action.as_ref() {
        output.push_str(&format!(
            "Next public action display summary: {}\n",
            next_public_action.command
        ));
    }
    if !operator.blockers.is_empty() {
        output.push_str("Blockers:\n");
        for blocker in &operator.blockers {
            output.push_str(&format!(
                "- {} scope={} display_only_next_summary={}\n",
                blocker.category,
                blocker.scope_key,
                blocker
                    .next_public_action
                    .as_ref()
                    .map(|action| action.command.as_str())
                    .unwrap_or("none")
            ));
        }
    }
    if let Some(recommended_command) = operator.recommended_command.as_deref() {
        output.push_str(&format!("Display command summary: {recommended_command}\n"));
    }
    if let Some(required_inputs) = required_inputs_line(&operator.required_inputs) {
        output.push_str(&required_inputs);
    }
    if renders_command_summary || renders_diagnostic_route_guidance {
        output.push_str(&operator_json_rerun_guidance(
            &operator.plan_path,
            external_review_result_ready,
            json_guidance_purpose,
        ));
        output.push('\n');
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
        "Display command summary: {}\n{}\n",
        optional_text(handoff.recommended_command.as_deref()),
        operator_json_rerun_guidance(
            &handoff.plan_path,
            false,
            operator_json_guidance_purpose_for_handoff(handoff)
        )
    ));
    if let Some(required_inputs) = required_inputs_line(&handoff.required_inputs) {
        output.push_str(&required_inputs);
    }
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
            "Next public action display summary: {}\n",
            next_public_action.command
        ));
    }
    if !handoff.blockers.is_empty() {
        output.push_str("Blockers:\n");
        for blocker in &handoff.blockers {
            output.push_str(&format!(
                "- {} scope={} display_only_next_summary={}\n",
                blocker.category,
                blocker.scope_key,
                blocker
                    .next_public_action
                    .as_ref()
                    .map(|action| action.command.as_str())
                    .unwrap_or("none")
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
    let routing = if let Some(plan_path) = plan_override {
        if !current_dir.join(plan_path).is_file() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                MISSING_PLAN_OVERRIDE_MESSAGE,
            ));
        }
        let runtime = ExecutionRuntime::discover(current_dir)?;
        query_workflow_routing_state_for_runtime(
            &runtime,
            Some(plan_path),
            external_review_result_ready,
        )?
    } else {
        query_workflow_routing_state(current_dir, None, external_review_result_ready)?
    };
    build_context_from_routing(routing, external_review_result_ready)
}

fn build_context_with_plan_for_runtime(
    runtime: &ExecutionRuntime,
    plan_override: Option<&Path>,
    external_review_result_ready: bool,
) -> Result<OperatorContext, JsonFailure> {
    let routing = if let Some(plan_path) = plan_override {
        if !runtime.repo_root.join(plan_path).is_file() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                MISSING_PLAN_OVERRIDE_MESSAGE,
            ));
        }
        query_workflow_routing_state_for_runtime(
            runtime,
            Some(plan_path),
            external_review_result_ready,
        )?
    } else {
        query_workflow_routing_state_for_runtime(runtime, None, external_review_result_ready)?
    };
    build_context_from_routing(routing, external_review_result_ready)
}

fn build_context_from_routing(
    routing: ExecutionRoutingState,
    external_review_result_ready: bool,
) -> Result<OperatorContext, JsonFailure> {
    let ExecutionRoutingState {
        route,
        route_decision,
        runtime_provenance,
        execution_status,
        preflight,
        gate_review,
        gate_finish,
        workflow_phase: _,
        phase: routing_phase,
        phase_detail: _,
        review_state_status: _,
        qa_requirement,
        finish_review_gate_pass_branch_closure_id,
        recording_context: _,
        execution_command_context: _,
        next_action: _,
        recommended_command: _,
        base_branch,
        blocking_scope: _,
        blocking_task: _,
        external_wait_state: _,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id: _,
        ..
    } = routing;
    let route_decision = route_decision.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ResolverContractViolation,
            "Workflow operator routing state is missing its finalized route decision.",
        )
    })?;
    let operator_phase = route_decision.phase.clone();
    let operator_phase_detail = route_decision.phase_detail.clone();
    let operator_next_action = route_decision.next_action.clone();
    let operator_recommended_command = route_decision.recommended_command.clone();
    let operator_recommended_public_command_argv = route_decision.public_command_argv();
    let operator_recommended_public_command_template = route_decision.public_command_template();
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
    let operator_execution_command_context =
        route_decision
            .execution_command_context
            .as_ref()
            .map(|context| WorkflowOperatorExecutionCommandContext {
                command_kind: context.command_kind.clone(),
                task_number: context.task_number,
                step_id: context.step_id,
            });
    let preflight_not_started = execution_status
        .as_ref()
        .is_some_and(|status| status.execution_started != "yes");
    // Presentation keeps the pre-execution handoff display stable while all
    // actionable route fields come from the finalized route decision.
    let display_phase = if route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY
        && preflight_not_started
        && matches!(
            HarnessPhase::parse(routing_phase.as_str()),
            Some(HarnessPhase::ImplementationHandoff | HarnessPhase::ExecutionPreflight)
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
    let operator_review_state_status = route_decision.review_state_status.clone();
    let operator_blocking_scope = route_decision.blocking_scope.clone();
    let operator_blocking_task = route_decision.blocking_task;
    let operator_external_wait_state = route_decision.external_wait_state.clone();
    let operator_blocking_reason_codes = route_decision.blocking_reason_codes.clone();
    let operator_diagnostic_reason_codes = execution_status
        .as_ref()
        .map(|status| merge_status_projection_diagnostics(diagnostic_reason_codes.clone(), status))
        .unwrap_or(diagnostic_reason_codes);
    let (operator_semantic_workspace_tree_id, operator_raw_workspace_tree_id) = execution_status
        .as_ref()
        .map(|status| {
            (
                status.semantic_workspace_tree_id.clone(),
                status.raw_workspace_tree_id.clone(),
            )
        })
        .unwrap_or_else(|| (String::new(), None));
    let operator_state_kind = route_decision.state_kind.clone();
    let operator_next_public_action = route_decision.next_public_action.clone();
    let operator_blockers = route_decision.blockers.clone();
    let operator_public_repair_targets = route_decision.public_repair_targets.clone();
    let plan_contract = if route.status == phase::WORKFLOW_STATUS_IMPLEMENTATION_READY {
        analyze_plan_if_available(&route).map_err(JsonFailure::from)?
    } else {
        None
    };

    Ok(OperatorContext {
        route,
        runtime_provenance,
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
        operator_recommended_public_command_template,
        operator_required_inputs,
        operator_base_branch,
        operator_blocking_scope,
        operator_blocking_task,
        operator_external_wait_state,
        operator_blocking_reason_codes,
        operator_state_kind,
        operator_next_public_action,
        operator_blockers,
        operator_public_repair_targets,
        operator_semantic_workspace_tree_id,
        operator_raw_workspace_tree_id,
        external_review_result_ready,
        reason_family,
        diagnostic_reason_codes: operator_diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        finish_review_gate_pass_branch_closure_id,
        qa_requirement,
    })
}

fn doctor_self_hosting_warning(runtime_provenance: Option<&RuntimeProvenance>) -> Option<String> {
    let provenance = runtime_provenance?;
    let mut warnings = Vec::new();
    if provenance.control_plane_source == ControlPlaneSource::Workspace
        && provenance.state_dir_kind == StateDirKind::Live
        && provenance.self_hosting_context == SelfHostingContext::FeatureforgeRepo
    {
        warnings.push(String::from(
            "workspace runtime with live FeatureForge state detected; rerun live workflow commands via ~/.featureforge/install/bin/featureforge",
        ));
    }
    if let Some(skill_warning) = provenance
        .skill_discovery
        .as_ref()
        .and_then(|discovery| discovery.warning.as_deref())
    {
        warnings.push(skill_warning.to_owned());
    }
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join(" "))
    }
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
            HarnessPhase::parse(context.phase.as_str()),
            Some(HarnessPhase::ImplementationHandoff | HarnessPhase::ExecutionPreflight)
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
    if context.operator_phase_detail == phase::DETAIL_BLOCKED_RUNTIME_BUG
        || state_kind_is_blocked_runtime_bug(&context.operator_state_kind)
    {
        return String::from(
            "Stop and report this runtime diagnostic; do not invent runtime mutations or reconstruct artifacts manually. Inspect workflow operator JSON blocking_reason_codes only to explain the blocker.",
        );
    }
    if context.phase == phase::PHASE_QA_PENDING
        && context.operator_phase_detail == phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED
    {
        return String::from(
            "Route to featureforge:plan-eng-review for current-branch test-plan refresh before browser QA or branch completion; do not hand-edit or reconstruct the artifact.",
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

fn reason_text(context: &OperatorContext) -> String {
    if context.phase == phase::PHASE_EXECUTION_PREFLIGHT {
        return String::from(
            "The approved plan matches the latest approved spec and preflight is the next safe boundary.",
        );
    }
    let recommendation =
        handoff_recommendation_for_context(context, execution_started_for_context(context), None);
    if recommendation.reason.is_empty() {
        context.route.reason.clone()
    } else {
        recommendation.reason
    }
}

fn handoff_recommendation_for_context(
    context: &OperatorContext,
    execution_started: &str,
    recommendation: Option<&RecommendOutput>,
) -> WorkflowRecommendation {
    let task_boundary_reason = task_boundary_reason_text(context);
    let task_boundary_next_step = task_boundary_next_step_text(context);
    let gate_review_message = gate_first_diagnostic_message(context.gate_review.as_ref());
    let gate_finish_message = gate_first_diagnostic_message(context.gate_finish.as_ref());
    handoff_recommendation(HandoffRecommendationInput {
        explicit: recommendation.map(|recommendation| ExplicitRecommendation {
            skill: recommendation.recommended_skill.as_str(),
            reason: recommendation.reason.as_str(),
        }),
        phase: context.phase.as_str(),
        phase_detail: context.operator_phase_detail.as_str(),
        route: &context.route,
        execution_started,
        execution_mode: context
            .execution_status
            .as_ref()
            .map(|status| status.execution_mode.as_str()),
        execution_preflight_block_reason: context.execution_preflight_block_reason.as_deref(),
        review_requires_execution_reentry: review_requires_execution_reentry(context),
        task_boundary_next_step: task_boundary_next_step.as_deref(),
        task_boundary_reason: task_boundary_reason.as_deref(),
        gate_review_message: gate_review_message.as_deref(),
        gate_finish_message: gate_finish_message.as_deref(),
    })
}

fn execution_started_for_context(context: &OperatorContext) -> &str {
    context
        .execution_status
        .as_ref()
        .map(|status| status.execution_started.as_str())
        .unwrap_or("no")
}

fn display_or_none(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

pub(crate) fn operator_json_rerun_guidance(
    plan_path: &str,
    external_review_result_ready: bool,
    purpose: OperatorJsonGuidancePurpose,
) -> String {
    let Some(command) =
        operator_requery_command_for_nonempty_plan_path(plan_path, external_review_result_ready)
    else {
        return missing_plan_operator_json_guidance(purpose);
    };
    let external_review_hint = if external_review_result_ready {
        "External review result is marked ready."
    } else {
        "Use --external-review-result-ready only after an external review result exists."
    };
    operator_json_guidance(&command, external_review_hint, purpose)
}

pub(crate) fn operator_json_external_review_wait_guidance(plan_path: &str) -> String {
    let Some(command) = operator_requery_command_for_nonempty_plan_path(plan_path, true) else {
        return missing_plan_operator_json_guidance(OperatorJsonGuidancePurpose::RouteOrientation);
    };
    operator_json_guidance(
        &command,
        "Run this external-ready query only after an external review result exists.",
        OperatorJsonGuidancePurpose::RouteOrientation,
    )
}

fn operator_json_guidance_suffix() -> &'static str {
    PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT
}

fn missing_plan_operator_json_guidance(purpose: OperatorJsonGuidancePurpose) -> String {
    format!(
        "{} Approved plan path unavailable; obtain the approved plan path before querying workflow operator JSON.",
        operator_json_guidance_prefix(purpose)
    )
}

fn operator_json_guidance(
    command: &str,
    external_review_hint: &str,
    purpose: OperatorJsonGuidancePurpose,
) -> String {
    format!(
        "{} Query workflow operator JSON: {command}; {external_review_hint} {}.",
        operator_json_guidance_prefix(purpose),
        operator_json_guidance_suffix()
    )
}

fn operator_json_guidance_prefix(purpose: OperatorJsonGuidancePurpose) -> &'static str {
    match purpose {
        OperatorJsonGuidancePurpose::CommandExecutionAuthority => "Command execution authority:",
        OperatorJsonGuidancePurpose::DiagnosticOrientation => "Diagnostic orientation:",
        OperatorJsonGuidancePurpose::RouteOrientation => "Route orientation:",
    }
}

fn operator_json_guidance_purpose(
    has_executable_surface: bool,
    state_kind: &str,
    phase_detail: &str,
) -> OperatorJsonGuidancePurpose {
    if has_executable_surface {
        return OperatorJsonGuidancePurpose::CommandExecutionAuthority;
    }
    if runtime_route_is_diagnostic(state_kind, phase_detail) {
        return OperatorJsonGuidancePurpose::DiagnosticOrientation;
    }
    OperatorJsonGuidancePurpose::RouteOrientation
}

fn operator_json_guidance_purpose_for_context(
    context: &OperatorContext,
) -> OperatorJsonGuidancePurpose {
    operator_json_guidance_purpose(
        has_executable_public_surface(
            &context.operator_recommended_public_command_argv,
            &context.operator_recommended_public_command_template,
        ),
        &context.operator_state_kind,
        &context.operator_phase_detail,
    )
}

fn operator_json_guidance_purpose_for_operator(
    operator: &WorkflowOperator,
) -> OperatorJsonGuidancePurpose {
    operator_json_guidance_purpose(
        has_executable_public_surface(
            &operator.recommended_public_command_argv,
            &operator.recommended_public_command_template,
        ),
        &operator.state_kind,
        &operator.phase_detail,
    )
}

fn operator_json_guidance_purpose_for_handoff(
    handoff: &WorkflowHandoff,
) -> OperatorJsonGuidancePurpose {
    operator_json_guidance_purpose(
        has_executable_public_surface(
            &handoff.recommended_public_command_argv,
            &handoff.recommended_public_command_template,
        ),
        &handoff.state_kind,
        &handoff.phase_detail,
    )
}

fn has_executable_public_surface(
    argv: &RecommendedPublicCommandArgv,
    template: &RecommendedPublicCommandTemplate,
) -> bool {
    argv.is_some() || template.is_some()
}

fn operator_requery_command_for_nonempty_plan_path(
    plan_path: &str,
    external_review_result_ready: bool,
) -> Option<String> {
    let plan_path = plan_path.trim();
    if plan_path.is_empty() {
        return None;
    }
    Some(workflow_operator_requery_command(
        Path::new(plan_path),
        external_review_result_ready,
    ))
}

fn required_inputs_line(inputs: &[PublicCommandInputRequirement]) -> Option<String> {
    let names = inputs
        .iter()
        .map(|input| input.name.as_str())
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();
    (!names.is_empty()).then(|| format!("Required inputs: {}\n", names.join(", ")))
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
    context.operator_phase == phase::PHASE_FINAL_REVIEW_PENDING
        && context.operator_phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && context
            .operator_execution_command_context
            .as_ref()
            .is_some_and(|execution_context| {
                [
                    PublicCommandKind::Begin,
                    PublicCommandKind::Complete,
                    PublicCommandKind::Reopen,
                ]
                .iter()
                .any(|kind| kind.matches_public_mutation_token(&execution_context.command_kind))
            })
}

fn task_boundary_reason_text(context: &OperatorContext) -> Option<String> {
    let blocking_task = context.operator_blocking_task?;
    let message = match context.operator_phase_detail.as_str() {
        phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED => format!(
            "Task {blocking_task} closure reached a retired task-review dispatch lane. Stop on workflow/operator JSON blocking_reason_codes; normal task closure must use typed close-current-task argv when available."
        ),
        phase::DETAIL_TASK_REVIEW_RESULT_PENDING => {
            if task_review_result_pending_requires_verification(context) {
                format!(
                    "Task {blocking_task} closure is waiting for verification evidence before close-current-task can complete the task boundary."
                )
            } else if operator_blocking_reason_present(
                context,
                TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_NOT_INDEPENDENT,
            ) || operator_blocking_reason_present(
                context,
                TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_ARTIFACT_MALFORMED,
            ) || operator_blocking_reason_present(
                context,
                TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN,
            ) {
                format!(
                    "Task {blocking_task} closure is waiting for a passing dedicated-independent review before close-current-task can complete the task boundary."
                )
            } else {
                format!(
                    "Task {blocking_task} closure is waiting for the outstanding review result before close-current-task can complete the task boundary."
                )
            }
        }
        phase::DETAIL_TASK_CLOSURE_RECORDING_READY => {
            if operator_blocking_reason_present_with(
                context,
                task_boundary_closure_baseline_bridge_ready_reason_code,
            ) {
                format!(
                    "Task {blocking_task} execution replay is already complete enough for close-current-task. Use the routed close-current-task argv/template now; do not reopen the step again."
                )
            } else {
                format!(
                    "Task {blocking_task} closure is ready for close-current-task. Use the routed close-current-task argv/template now."
                )
            }
        }
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => {
            if operator_blocking_reason_present(
                context,
                TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN,
            ) {
                format!(
                    "Task {blocking_task} closure is waiting on remediation because the latest dedicated-independent review is not green. Reenter execution for Task {blocking_task}; close-current-task remains the task-boundary command after a passing rerun review."
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
    operator_blocking_reason_present_with(context, |code| code == reason_code)
}

fn operator_blocking_reason_present_with(
    context: &OperatorContext,
    predicate: impl Fn(&str) -> bool,
) -> bool {
    context
        .operator_blocking_reason_codes
        .iter()
        .map(String::as_str)
        .any(&predicate)
        || context.execution_status.iter().any(|status| {
            status
                .reason_codes
                .iter()
                .map(String::as_str)
                .any(&predicate)
        })
}

fn task_boundary_next_step_text(context: &OperatorContext) -> Option<String> {
    if !task_boundary_guidance_applies(context) {
        return None;
    }
    let reason = task_boundary_reason_text(context)?;
    if let Some(recommended_command) = context.operator_recommended_command.as_deref() {
        return Some(format!(
            "{reason} Query workflow/operator with --json; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}; display command summary: {recommended_command}"
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
    use crate::contracts::workflow::WorkflowRoute;
    use crate::execution::command_eligibility::PublicCommandInputKind;
    use crate::execution::public_command_types::PublicCommandTemplate;

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
            runtime_provenance: None,
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
            operator_recommended_public_command_template: None,
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
            operator_public_repair_targets: Vec::new(),
            operator_semantic_workspace_tree_id: String::new(),
            operator_raw_workspace_tree_id: None,
            external_review_result_ready: false,
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
            recommended_public_command_template: None,
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
            blocking_reason_codes: vec![String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            )],
            diagnostic_reason_codes: Vec::new(),
            state_kind: String::from("actionable_public_command"),
            next_public_action: Some(RuntimeNextPublicAction {
                display_only: true,
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
                next_public_action: Some(RuntimeNextPublicAction {
                    display_only: true,
                    command: String::from("close_current_task"),
                    args_template: None,
                }),
                details: String::from("Task review result pending."),
            }],
            public_repair_targets: Vec::new(),
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
            runtime_provenance: Some(RuntimeProvenance {
                binary_path: String::from("/tmp/workspace/featureforge/target/debug/featureforge"),
                binary_realpath: String::from(
                    "/tmp/workspace/featureforge/target/debug/featureforge",
                ),
                runtime_root: String::from("/tmp/workspace/featureforge"),
                repo_root: String::from("/tmp/workspace/featureforge"),
                state_dir: String::from("/Users/alice/.featureforge"),
                state_dir_kind: StateDirKind::Live,
                control_plane_source: ControlPlaneSource::Workspace,
                self_hosting_context: SelfHostingContext::FeatureforgeRepo,
                workspace_runtime_warning: Some(String::from("workspace runtime warning")),
                skill_discovery: None,
            }),
        });

        assert!(rendered.contains("QA requirement: required"));
        assert!(
            rendered.contains("Warning: workspace runtime with live FeatureForge state detected")
        );
        assert!(rendered.contains("Finish gate checkpoint: branch-closure-1"));
        assert!(rendered.contains(
            "Recording context: task_number=1, dispatch_id=dispatch-1, branch_closure_id=branch-closure-1"
        ));
        assert!(rendered.contains(
            "Execution command context: command_kind=complete, task_number=1, step_id=2"
        ));
        assert!(rendered.contains("Next public action display summary:"));
        assert!(rendered.contains("display_only_next_summary=close_current_task"));
        assert_operator_stop_guidance(&rendered);
        assert!(rendered.contains("Required inputs: claim"));
        let command_shaped_next_public_action = ["Next public action", ": featureforge"].concat();
        assert!(
            !rendered.contains(&command_shaped_next_public_action),
            "command-shaped text must be labeled as display-only: {rendered}"
        );
        assert!(
            !rendered.contains(" next=featureforge"),
            "blocker command-shaped text must be labeled as display-only: {rendered}"
        );
    }

    #[test]
    fn workflow_operator_json_surfaces_projection_diagnostics() {
        let mut context = task_boundary_context(phase::DETAIL_EXECUTION_IN_PROGRESS, &[], None);
        context.diagnostic_reason_codes = vec![String::from(
            crate::execution::review_route_tokens::REASON_DERIVED_REVIEW_STATE_MISSING,
        )];

        let operator = operator_from_context(
            context,
            &OperatorArgs {
                plan: PathBuf::new(),
                external_review_result_ready: false,
                inputs: Vec::new(),
                json: true,
            },
        );

        assert_eq!(
            operator.diagnostic_reason_codes,
            vec![String::from(
                crate::execution::review_route_tokens::REASON_DERIVED_REVIEW_STATE_MISSING
            )],
            "workflow operator JSON must expose projection diagnostics without route authority"
        );
    }

    #[test]
    fn doctor_gate_sanitizer_drops_nested_display_command_but_keeps_typed_template() {
        let gate = GateResult {
            allowed: false,
            action: String::from("blocked"),
            failure_class: String::from(FailureClass::ExecutionStateNotReady.as_str()),
            reason_codes: vec![String::from("test_reason")],
            warning_codes: vec![String::from("test_warning")],
            diagnostics: Vec::new(),
            code: None,
            workspace_state_id: Some(String::from("semantic_tree:test")),
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            recommended_command: Some(String::from(
                "featureforge plan execution advance-late-stage --plan docs/plan.md",
            )),
            recommended_public_command_template: Some(PublicCommandTemplate {
                command_kind: String::from("advance_late_stage"),
                base_argv: vec![
                    String::from("featureforge"),
                    String::from("plan"),
                    String::from("execution"),
                    String::from("advance-late-stage"),
                    String::from("--plan"),
                    String::from("docs/plan.md"),
                ],
                required_input_names: vec![String::from("result")],
                input_bindings: Vec::new(),
            }),
            required_inputs: vec![PublicCommandInputRequirement {
                name: String::from("result"),
                kind: PublicCommandInputKind::Enum,
                values: vec![String::from("pass"), String::from("fail")],
                must_exist: false,
                required_when: None,
            }],
            rederive_via_workflow_operator: None,
        };

        let sanitized = sanitize_doctor_gate_warning_codes(gate);

        assert!(
            sanitized.recommended_command.is_none(),
            "nested doctor gates must not expose display-only command strings"
        );
        assert!(
            sanitized.recommended_public_command_template.is_some(),
            "typed templates should remain available when already present"
        );
        assert_eq!(
            sanitized.required_inputs.len(),
            1,
            "required input metadata should survive display-command stripping"
        );
    }

    #[test]
    fn no_plan_rerun_guidance_is_non_command_text() {
        let operator_guidance =
            operator_json_rerun_guidance("", false, OperatorJsonGuidancePurpose::RouteOrientation);
        let forbidden_none_plan = ["--plan ", "none"].concat();
        assert_eq!(
            operator_guidance,
            "Route orientation: Approved plan path unavailable; obtain the approved plan path before querying workflow operator JSON."
        );
        assert!(
            !operator_guidance.contains("featureforge workflow operator --plan"),
            "operator no-plan guidance must not render an executable-looking operator command: {operator_guidance}"
        );
        assert!(
            !operator_guidance.contains(&forbidden_none_plan),
            "operator no-plan guidance must not render a synthetic none plan path: {operator_guidance}"
        );

        let external_ready_guidance = operator_json_rerun_guidance(
            "docs/plan.md",
            true,
            OperatorJsonGuidancePurpose::RouteOrientation,
        );
        assert!(
            external_ready_guidance
                .contains("featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready --json"),
            "operator external-ready guidance must preserve the external-result-ready flag: {external_ready_guidance}"
        );
        assert!(
            !external_ready_guidance.contains("featureforge workflow operator --plan docs/plan.md"),
            "operator external-ready guidance must not render concrete shell-like plan paths: {external_ready_guidance}"
        );
        assert!(
            external_ready_guidance.contains("External review result is marked ready."),
            "operator external-ready guidance must name why the external flag is legal: {external_ready_guidance}"
        );
        assert_operator_stop_guidance(&external_ready_guidance);
    }

    #[test]
    fn executable_rerun_guidance_is_command_authority_text() {
        let guidance = operator_json_rerun_guidance(
            "docs/plan.md",
            false,
            OperatorJsonGuidancePurpose::CommandExecutionAuthority,
        );
        assert!(
            guidance.contains("Command execution authority: Query workflow operator JSON:"),
            "executable route guidance should keep command-authority labeling: {guidance}"
        );
        assert_operator_stop_guidance(&guidance);
    }

    #[test]
    fn render_phase_and_handoff_no_plan_text_is_non_command() {
        let mut phase_context =
            task_boundary_context(phase::DETAIL_TASK_REVIEW_RESULT_PENDING, &[], None);
        let forbidden_none_plan = ["--plan ", "none"].concat();
        let phase_with_plan_text = render_phase_from_context(&phase_context);
        assert_operator_stop_guidance(&phase_with_plan_text);
        phase_context.route.plan_path.clear();
        let phase_text = render_phase_from_context(&phase_context);
        assert!(
            phase_text.contains(
                "Route orientation: Approved plan path unavailable; obtain the approved plan path before querying workflow operator JSON."
            ),
            "no-plan phase text should tell agents to obtain the approved plan path: {phase_text}"
        );
        assert!(
            !phase_text.contains("featureforge workflow operator --plan"),
            "no-plan phase text must not render operator command text: {phase_text}"
        );
        assert!(
            !phase_text.contains(&forbidden_none_plan),
            "no-plan phase text must not render a synthetic none plan path: {phase_text}"
        );

        let handoff_context_with_plan =
            task_boundary_context(phase::DETAIL_TASK_REVIEW_RESULT_PENDING, &[], None);
        let handoff_with_plan = handoff_from_context(handoff_context_with_plan, None);
        let handoff_with_plan_text = render_handoff_output(&handoff_with_plan);
        assert_operator_stop_guidance(&handoff_with_plan_text);
        let mut handoff_context =
            task_boundary_context(phase::DETAIL_TASK_REVIEW_RESULT_PENDING, &[], None);
        handoff_context.route.plan_path.clear();
        let handoff = handoff_from_context(handoff_context, None);
        let handoff_text = render_handoff_output(&handoff);
        assert!(
            handoff_text.contains(
                "Route orientation: Approved plan path unavailable; obtain the approved plan path before querying workflow operator JSON."
            ),
            "no-plan handoff text should tell agents to obtain the approved plan path: {handoff_text}"
        );
        assert!(
            !handoff_text.contains("featureforge workflow operator --plan"),
            "no-plan handoff text must not render operator command text: {handoff_text}"
        );
        assert!(
            !handoff_text.contains(&forbidden_none_plan),
            "no-plan handoff text must not render a synthetic none plan path: {handoff_text}"
        );
    }

    fn assert_operator_stop_guidance(rendered: &str) {
        assert!(
            rendered.contains("follow `recommended_public_command_argv` when present"),
            "operator guidance should include typed argv authority: {rendered}"
        );
        assert!(
            rendered.contains("use `required_inputs` as validation metadata"),
            "operator guidance should include required-input validation metadata: {rendered}"
        );
        assert!(
            rendered.contains("recommended_public_command_template.input_bindings"),
            "operator guidance should include typed template binding authority: {rendered}"
        );
        assert!(
            rendered.contains(
                "If neither executable surface is present, stop and report the route diagnostic"
            ),
            "operator guidance should include the canonical no-executable-surface stop rule: {rendered}"
        );
    }

    #[test]
    fn task_boundary_reason_text_uses_verification_language_when_verification_is_missing() {
        let context = task_boundary_context(
            phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
            &[crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING],
            Some(
                "featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            ),
        );

        let reason = task_boundary_reason_text(&context)
            .expect("task-boundary reason text should be available for task_review_result_pending");
        assert!(
            reason.contains(
                "Task 1 closure is waiting for verification evidence before close-current-task can complete the task boundary"
            ),
            "verification-missing task-boundary reason text should mention verification + close-current-task, got {reason}"
        );

        let next_step = task_boundary_next_step_text(&context).expect(
            "task-boundary next-step text should be available for task_review_result_pending",
        );
        assert!(
            next_step.contains(
                "Task 1 closure is waiting for verification evidence before close-current-task can complete the task boundary"
            ),
            "verification-missing next-step text should preserve verification + close-current-task guidance, got {next_step}"
        );
        assert!(
            next_step.contains("featureforge plan execution close-current-task"),
            "task-boundary next-step text should still include the routed command for verification-missing blockers, got {next_step}"
        );
    }

    #[test]
    fn test_plan_refresh_next_step_is_single_plan_eng_review_handoff() {
        let mut context =
            task_boundary_context(phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED, &[], None);
        context.phase = String::from(phase::PHASE_QA_PENDING);
        context.operator_phase = String::from(phase::PHASE_QA_PENDING);
        context.operator_next_action = String::from("refresh test plan");

        let next_step = next_step_text(&context);

        assert!(
            next_step.contains("featureforge:plan-eng-review"),
            "test-plan refresh guidance should keep the regeneration owner explicit, got {next_step}"
        );
        assert!(
            next_step.starts_with("Route to featureforge:plan-eng-review"),
            "test-plan refresh guidance should name one immediate handoff action, got {next_step}"
        );
        assert!(
            next_step.contains("do not hand-edit or reconstruct the artifact"),
            "test-plan refresh guidance should not suggest manual artifact repair, got {next_step}"
        );
        assert!(
            !next_step.contains("Then rerun")
                && !next_step.contains("workflow operator")
                && !next_step.contains("recommended_public_command_argv")
                && !next_step.contains("recommended_public_command_template"),
            "test-plan refresh next_step should stay to one handoff action, got {next_step}"
        );
    }

    #[test]
    fn doctor_self_hosting_warning_requires_workspace_live_featureforge_context() {
        let warning = doctor_self_hosting_warning(Some(&RuntimeProvenance {
            binary_path: String::from("/tmp/workspace/bin/featureforge"),
            binary_realpath: String::from("/tmp/workspace/bin/featureforge"),
            runtime_root: String::from("/tmp/workspace"),
            repo_root: String::from("/tmp/workspace/featureforge"),
            state_dir: String::from("/Users/alice/.featureforge"),
            state_dir_kind: StateDirKind::Live,
            control_plane_source: ControlPlaneSource::Workspace,
            self_hosting_context: SelfHostingContext::FeatureforgeRepo,
            workspace_runtime_warning: Some(String::from("workspace runtime warning")),
            skill_discovery: None,
        }));
        assert_eq!(
            warning.as_deref(),
            Some(
                "workspace runtime with live FeatureForge state detected; rerun live workflow commands via ~/.featureforge/install/bin/featureforge",
            )
        );

        let no_warning = doctor_self_hosting_warning(Some(&RuntimeProvenance {
            binary_path: String::from("/tmp/workspace/bin/featureforge"),
            binary_realpath: String::from("/tmp/workspace/bin/featureforge"),
            runtime_root: String::from("/tmp/workspace"),
            repo_root: String::from("/tmp/workspace/featureforge"),
            state_dir: String::from("/tmp/test-state"),
            state_dir_kind: StateDirKind::Temp,
            control_plane_source: ControlPlaneSource::Workspace,
            self_hosting_context: SelfHostingContext::FeatureforgeRepo,
            workspace_runtime_warning: Some(String::from("workspace runtime warning")),
            skill_discovery: None,
        }));
        assert!(
            no_warning.is_none(),
            "workspace runtime warning should stay compact and only trigger for live self-hosting"
        );
    }

    #[test]
    fn doctor_self_hosting_warning_surfaces_workspace_skill_discovery_guidance() {
        let warning = doctor_self_hosting_warning(Some(&RuntimeProvenance {
            binary_path: String::from("/Users/alice/.featureforge/install/bin/featureforge"),
            binary_realpath: String::from("/Users/alice/.featureforge/install/bin/featureforge"),
            runtime_root: String::from("/Users/alice/.featureforge/install"),
            repo_root: String::from("/tmp/workspace/featureforge"),
            state_dir: String::from("/Users/alice/.featureforge"),
            state_dir_kind: StateDirKind::Live,
            control_plane_source: ControlPlaneSource::Installed,
            self_hosting_context: SelfHostingContext::FeatureforgeRepo,
            workspace_runtime_warning: None,
            skill_discovery: Some(
                crate::execution::runtime_provenance::SkillDiscoveryProvenance {
                    installed_skill_root: String::from("/Users/alice/.featureforge/install/skills"),
                    workspace_skill_root: String::from("/tmp/workspace/featureforge/skills"),
                    active_roots: vec![],
                    active_featureforge_skill_source:
                        crate::execution::runtime_provenance::SkillSource::Workspace,
                    warning: Some(String::from(
                        "workspace skill discovery root detected in active FeatureForge channels",
                    )),
                },
            ),
        }));
        assert!(
            warning.as_deref().is_some_and(|value| value.contains(
                "workspace skill discovery root detected in active FeatureForge channels"
            )),
            "doctor warning should include workspace skill discovery guidance when present"
        );
    }

    #[test]
    fn doctor_resolution_tracks_external_wait_from_operator_context() {
        let mut context =
            task_boundary_context(phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING, &[], None);
        context.phase = String::from(phase::PHASE_FINAL_REVIEW_PENDING);
        context.operator_phase = String::from(phase::PHASE_FINAL_REVIEW_PENDING);
        context.operator_next_action = String::from("wait for external review result");
        context.operator_blocking_scope = Some(String::from("branch"));
        context.operator_blocking_task = None;
        context.operator_external_wait_state =
            Some(String::from("waiting_for_external_review_result"));
        context.operator_state_kind = String::from("waiting_external_input");
        context.task_review_dispatch_id = None;
        context.final_review_dispatch_id = Some(String::from("dispatch-final-review"));

        let doctor = doctor_from_context(context);

        assert_eq!(doctor.phase, phase::PHASE_FINAL_REVIEW_PENDING);
        assert_eq!(
            doctor.phase_detail,
            phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
        );
        assert_eq!(
            doctor.external_wait_state.as_deref(),
            Some("waiting_for_external_review_result")
        );
        assert_eq!(doctor.resolution.kind, "waiting_external_input");
        assert!(!doctor.resolution.command_available);
        assert_eq!(
            doctor.resolution.stop_reasons,
            ["waiting_for_external_review_result"]
        );
    }
}
