//! Workflow routing consumes the execution-owned query surface and maps it into
//! public phases and next-action recommendations.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use crate::cli::plan_execution::{RecommendArgs, StatusArgs as ExecutionStatusArgs};
use crate::cli::workflow::{OperatorArgs, PlanArgs};
use crate::contracts::plan::AnalyzePlanReport;
use crate::diagnostics::{DiagnosticError, JsonFailure};
use crate::execution::harness::EvaluatorKind;
use crate::execution::query::{ExecutionRoutingState, query_workflow_routing_state};
use crate::execution::state::{ExecutionRuntime, GateResult, PlanExecutionStatus};
use crate::execution::topology::RecommendOutput;
use crate::workflow::status::{WorkflowPhase, WorkflowRoute};

const WORKFLOW_PHASE_SCHEMA_VERSION: u32 = 2;
const WORKFLOW_DOCTOR_SCHEMA_VERSION: u32 = 2;
const WORKFLOW_HANDOFF_SCHEMA_VERSION: u32 = 2;
const WORKFLOW_OPERATOR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowDoctor {
    pub schema_version: u32,
    pub phase: String,
    pub route_status: String,
    pub next_skill: String,
    pub next_action: String,
    pub next_step: String,
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
    pub route_status: String,
    pub next_skill: String,
    pub contract_state: String,
    pub spec_path: String,
    pub plan_path: String,
    pub execution_started: String,
    pub next_action: String,
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
    pub schema_version: u32,
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qa_requirement: Option<String>,
    pub follow_up_override: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_context: Option<WorkflowOperatorRecordingContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_command_context: Option<WorkflowOperatorExecutionCommandContext>,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    pub spec_path: String,
    pub plan_path: String,
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
    reason_family: String,
    diagnostic_reason_codes: Vec<String>,
    task_review_dispatch_id: Option<String>,
    final_review_dispatch_id: Option<String>,
    finish_review_gate_pass_branch_closure_id: Option<String>,
    qa_requirement: Option<String>,
}

pub fn render_next(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    let mut output = String::new();
    output.push_str("Next action: ");
    output.push_str(next_action_for_context(&context));
    output.push('\n');
    output.push_str("Next safe step: ");
    output.push_str(&next_step_text(&context));
    output.push('\n');
    output.push_str("Reason: ");
    output.push_str(&reason_text(&context));
    output.push('\n');
    Ok(output)
}

pub fn render_artifacts(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(format!(
        "Workflow artifacts\n- Spec: {}\n- Plan: {}\n",
        display_or_none(&context.route.spec_path),
        display_or_none(&context.route.plan_path)
    ))
}

pub fn render_explain(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(format!(
        "Why FeatureForge chose this state\n- State: {}\n- Spec: {}\n- Plan: {}\nWhat to do:\n1. {}\n",
        context.route.status,
        display_or_none(&context.route.spec_path),
        display_or_none(&context.route.plan_path),
        next_step_text(&context)
    ))
}

pub fn phase(current_dir: &Path) -> Result<WorkflowPhase, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(WorkflowPhase {
        schema_version: WORKFLOW_PHASE_SCHEMA_VERSION,
        phase: context.phase.clone(),
        route_status: context.route.status.clone(),
        next_skill: public_next_skill(&context),
        next_step: next_step_text(&context),
        next_action: next_action_for_context(&context).to_owned(),
        reason_family: context.reason_family.clone(),
        diagnostic_reason_codes: context.diagnostic_reason_codes.clone(),
        spec_path: context.route.spec_path.clone(),
        plan_path: context.route.plan_path.clone(),
        route: context.route,
    })
}

pub fn render_phase(current_dir: &Path) -> Result<String, JsonFailure> {
    let context = build_context(current_dir)?;
    Ok(format!(
        "Workflow phase: {}\nRoute status: {}\nNext action: {}\nNext: {}\nSpec: {}\nPlan: {}\n",
        context.phase,
        context.route.status,
        next_action_for_context(&context),
        next_step_text(&context),
        display_or_none(&context.route.spec_path),
        display_or_none(&context.route.plan_path)
    ))
}

pub fn doctor(current_dir: &Path) -> Result<WorkflowDoctor, JsonFailure> {
    let context = build_context(current_dir)?;
    let doctor_phase = doctor_phase_for_context(&context);
    let contract_state = context
        .plan_contract
        .as_ref()
        .map(|report| report.contract_state.clone())
        .unwrap_or_else(|| context.route.contract_state.clone());

    Ok(WorkflowDoctor {
        schema_version: WORKFLOW_DOCTOR_SCHEMA_VERSION,
        phase: doctor_phase,
        route_status: context.route.status.clone(),
        next_skill: public_next_skill(&context),
        next_action: next_action_for_context(&context).to_owned(),
        next_step: next_step_text(&context),
        spec_path: context.route.spec_path.clone(),
        plan_path: context.route.plan_path.clone(),
        contract_state,
        route: context.route,
        execution_status: context.execution_status,
        plan_contract: context.plan_contract,
        preflight: context.preflight,
        gate_review: context.gate_review,
        gate_finish: context.gate_finish,
        task_review_dispatch_id: context.task_review_dispatch_id,
        final_review_dispatch_id: context.final_review_dispatch_id,
    })
}

pub fn render_doctor(current_dir: &Path) -> Result<String, JsonFailure> {
    let doctor = doctor(current_dir)?;
    let mut output = format!(
        "Workflow doctor\nPhase: {}\nRoute status: {}\nNext action: {}\nNext: {}\nContract state: {}\nSpec: {}\nPlan: {}\n",
        doctor.phase,
        doctor.route_status,
        doctor.next_action,
        doctor.next_step,
        doctor.contract_state,
        display_or_none(&doctor.spec_path),
        display_or_none(&doctor.plan_path)
    );
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
    Ok(output)
}

pub fn handoff(current_dir: &Path) -> Result<WorkflowHandoff, JsonFailure> {
    let context = build_context(current_dir)?;
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
    let recommendation = if context.route.status == "implementation_ready"
        && context.phase == "execution_preflight"
        && execution_started != "yes"
        && !context.route.plan_path.is_empty()
    {
        let runtime = ExecutionRuntime::discover(current_dir)?;
        Some(runtime.recommend(&RecommendArgs {
            plan: PathBuf::from(&context.route.plan_path),
            isolated_agents: None,
            session_intent: None,
            workspace_prepared: None,
        })?)
    } else {
        None
    };

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
            "qa_pending" if finish_requires_test_plan_refresh(&context) => (
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
                (skill, reason_text(&context))
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

    Ok(WorkflowHandoff {
        schema_version: WORKFLOW_HANDOFF_SCHEMA_VERSION,
        phase: context.phase.clone(),
        route_status: context.route.status.clone(),
        next_skill: public_next_skill(&context),
        contract_state,
        spec_path: context.route.spec_path.clone(),
        plan_path: context.route.plan_path.clone(),
        execution_started,
        next_action: next_action_for_context(&context).to_owned(),
        reason_family: context.reason_family.clone(),
        diagnostic_reason_codes: context.diagnostic_reason_codes.clone(),
        recommended_skill,
        recommendation_reason,
        route: context.route,
        execution_status: context.execution_status,
        plan_contract: context.plan_contract,
        recommendation,
    })
}

pub fn operator(current_dir: &Path, args: &OperatorArgs) -> Result<WorkflowOperator, JsonFailure> {
    let context = build_context_with_plan(
        current_dir,
        Some(&args.plan),
        args.external_review_result_ready,
    )?;
    let plan_path = operator_plan_path(&context, args);
    Ok(WorkflowOperator {
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
        spec_path: context.route.spec_path.clone(),
        plan_path,
    })
}

pub fn render_operator(operator: WorkflowOperator) -> String {
    let mut output = format!(
        "Workflow operator\nPhase: {}\nDetail: {}\nReview state: {}\nNext action: {}\nSpec: {}\nPlan: {}\n",
        operator.phase,
        operator.phase_detail,
        operator.review_state_status,
        operator.next_action,
        display_or_none(&operator.spec_path),
        display_or_none(&operator.plan_path)
    );
    if let Some(recommended_command) = operator.recommended_command {
        output.push_str(&format!("Recommended command: {recommended_command}\n"));
    }
    output
}

pub fn render_handoff(current_dir: &Path) -> Result<String, JsonFailure> {
    let handoff = handoff(current_dir)?;
    let mut output = String::new();
    output.push_str("Workflow handoff\n");
    output.push_str(&format!("Phase: {}\n", handoff.phase));
    output.push_str(&format!("Route status: {}\n", handoff.route_status));
    output.push_str(&format!("Next action: {}\n", handoff.next_action));
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
    Ok(output)
}

pub fn preflight(current_dir: &Path, args: &PlanArgs) -> Result<GateResult, JsonFailure> {
    let runtime = ExecutionRuntime::discover(current_dir)?;
    runtime.preflight(&execution_status_args(args))
}

pub fn gate_review(current_dir: &Path, args: &PlanArgs) -> Result<GateResult, JsonFailure> {
    let runtime = ExecutionRuntime::discover(current_dir)?;
    runtime.gate_review(&execution_status_args(args))
}

pub fn gate_finish(current_dir: &Path, args: &PlanArgs) -> Result<GateResult, JsonFailure> {
    let runtime = ExecutionRuntime::discover(current_dir)?;
    runtime.gate_finish(&execution_status_args(args))
}

pub fn render_gate(title: &str, gate: &GateResult) -> String {
    let mut output = format!("{}\nAllowed: {}\n", title, gate.allowed);
    if !gate.failure_class.is_empty() {
        output.push_str(&format!("Failure class: {}\n", gate.failure_class));
    }
    output
}

fn build_context(current_dir: &Path) -> Result<OperatorContext, JsonFailure> {
    build_context_with_plan(current_dir, None, false)
}

fn build_context_with_plan(
    current_dir: &Path,
    plan_override: Option<&Path>,
    external_review_result_ready: bool,
) -> Result<OperatorContext, JsonFailure> {
    let routing =
        query_workflow_routing_state(current_dir, plan_override, external_review_result_ready)?;
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
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        ..
    } = routing;
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
        phase: phase.clone(),
        operator_phase: phase,
        operator_phase_detail: phase_detail,
        operator_review_state_status: review_state_status,
        operator_follow_up_override: follow_up_override,
        operator_recording_context: recording_context.map(|context| {
            WorkflowOperatorRecordingContext {
                task_number: context.task_number,
                dispatch_id: context.dispatch_id,
                branch_closure_id: context.branch_closure_id,
            }
        }),
        operator_execution_command_context: execution_command_context.map(|context| {
            WorkflowOperatorExecutionCommandContext {
                command_kind: context.command_kind,
                task_number: context.task_number,
                step_id: context.step_id,
            }
        }),
        operator_next_action: next_action,
        operator_recommended_command: recommended_command,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id,
        final_review_dispatch_id,
        finish_review_gate_pass_branch_closure_id,
        qa_requirement,
    })
}

fn operator_plan_path(context: &OperatorContext, args: &OperatorArgs) -> String {
    if args.plan.as_os_str().is_empty() {
        context.route.plan_path.clone()
    } else {
        args.plan.to_string_lossy().into_owned()
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
    if context.phase == "qa_pending" && finish_requires_test_plan_refresh(context) {
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
        "executing" => {
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
        "executing" => String::from(
            "Execution already started for the approved plan and should continue through the current execution flow.",
        ),
        "pivot_required" => {
            String::from("Execution is blocked pending an approved plan revision.")
        }
        "task_closure_pending" => {
            let dispatch_block_reason = context.execution_status.as_ref().and_then(|status| {
                task_boundary_block_reason_code(status).filter(|reason_code| {
                    matches!(
                        *reason_code,
                        "prior_task_review_dispatch_missing"
                            | "prior_task_review_dispatch_stale"
                    )
                })
            });
            if dispatch_block_reason.is_some() {
                task_boundary_next_step_text(context).unwrap_or_else(|| {
                    String::from(
                        "Execution already started for the approved plan and should continue through the current execution flow.",
                    )
                })
            } else {
                context
                    .execution_status
                    .as_ref()
                    .and_then(task_boundary_reason_text)
                    .unwrap_or_else(|| {
                        String::from(
                            "Execution already started for the approved plan and should continue through the current execution flow.",
                        )
                    })
            }
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

fn display_or_none(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

fn public_next_skill(context: &OperatorContext) -> String {
    context.route.next_skill.clone()
}

fn next_action_for_context(context: &OperatorContext) -> &str {
    &context.operator_next_action
}

fn finish_requires_test_plan_refresh(context: &OperatorContext) -> bool {
    gate_has_any_reason(
        context.gate_finish.as_ref(),
        &[
            "test_plan_artifact_missing",
            "test_plan_artifact_malformed",
            "test_plan_artifact_stale",
            "test_plan_artifact_authoritative_provenance_invalid",
            "test_plan_artifact_generator_mismatch",
        ],
    )
}

fn review_requires_execution_reentry(context: &OperatorContext) -> bool {
    context.phase == "final_review_pending"
        && context
            .gate_review
            .as_ref()
            .is_some_and(|gate| !gate.allowed)
}

fn task_boundary_block_reason_code(status: &PlanExecutionStatus) -> Option<&str> {
    if status.blocking_task.is_none() || status.blocking_step.is_some() {
        return None;
    }
    status.reason_codes.iter().map(String::as_str).find(|code| {
        matches!(
            *code,
            "prior_task_review_not_green"
                | "task_review_not_independent"
                | "task_review_receipt_malformed"
                | "prior_task_verification_missing"
                | "prior_task_verification_missing_legacy"
                | "task_verification_receipt_malformed"
                | "prior_task_review_dispatch_missing"
                | "prior_task_review_dispatch_stale"
                | "task_cycle_break_active"
        )
    })
}

fn task_boundary_reason_text(status: &PlanExecutionStatus) -> Option<String> {
    let blocking_task = status.blocking_task?;
    let reason_code = task_boundary_block_reason_code(status)?;
    let message = match reason_code {
        "prior_task_review_not_green"
        | "task_review_not_independent"
        | "task_review_receipt_malformed" => format!(
            "Task-boundary gate ({reason_code}) is blocking advancement past Task {blocking_task}. Complete dedicated-independent review for Task {blocking_task} until it is green."
        ),
        "prior_task_verification_missing" | "task_verification_receipt_malformed" => format!(
            "Task-boundary gate ({reason_code}) is blocking advancement past Task {blocking_task}. Run verification-before-completion for Task {blocking_task} and record a passing task verification receipt."
        ),
        "prior_task_verification_missing_legacy" => format!(
            "Task-boundary gate ({reason_code}) is blocking advancement past Task {blocking_task}. Backfill Task {blocking_task} verification evidence or record an approved migration marker before starting the next task."
        ),
        "prior_task_review_dispatch_missing" | "prior_task_review_dispatch_stale" => format!(
            "Task-boundary gate ({reason_code}) is blocking advancement past Task {blocking_task}. STOP and dispatch fresh-context dedicated-independent review before any next-task begin."
        ),
        "task_cycle_break_active" => format!(
            "Task-boundary gate ({reason_code}) is blocking advancement past Task {blocking_task}. Resolve cycle-break remediation for Task {blocking_task} before retrying."
        ),
        _ => return None,
    };
    Some(message)
}

fn task_boundary_next_step_text(context: &OperatorContext) -> Option<String> {
    if context.phase != "repairing" && context.phase != "task_closure_pending" {
        return None;
    }
    let status = context.execution_status.as_ref()?;
    let reason = task_boundary_reason_text(status)?;
    let reason_code = task_boundary_block_reason_code(status)?;
    if matches!(
        reason_code,
        "prior_task_review_dispatch_missing" | "prior_task_review_dispatch_stale"
    ) {
        if context.route.plan_path.is_empty() {
            return Some(format!(
                "{reason} Run `featureforge plan execution record-review-dispatch --plan <approved-plan-path> --scope task --task <n>` before any next-task begin."
            ));
        }
        return Some(format!(
            "{reason} Run `featureforge plan execution record-review-dispatch --plan {} --scope task --task <n>` before any next-task begin.",
            context.route.plan_path
        ));
    }
    if context.route.plan_path.is_empty() {
        Some(format!("{reason} Continue in the active execution flow."))
    } else {
        Some(format!(
            "{reason} Continue in the active execution flow for {}.",
            context.route.plan_path
        ))
    }
}

fn gate_has_any_reason(gate: Option<&GateResult>, expected_codes: &[&str]) -> bool {
    gate.is_some_and(|gate| {
        gate.reason_codes
            .iter()
            .any(|code| expected_codes.contains(&code.as_str()))
    })
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

fn execution_status_args(args: &PlanArgs) -> ExecutionStatusArgs {
    ExecutionStatusArgs {
        plan: args.plan.clone(),
    }
}
