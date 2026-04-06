use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use schemars::JsonSchema;
use serde::Serialize;

use crate::cli::plan_execution::{RecommendArgs, StatusArgs as ExecutionStatusArgs};
use crate::cli::workflow::{OperatorArgs, PlanArgs};
use crate::contracts::plan::AnalyzePlanReport;
use crate::diagnostics::{DiagnosticError, JsonFailure};
use crate::execution::harness::{EvaluatorKind, HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::leases::load_status_authoritative_overlay_checked;
use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
use crate::execution::state::{
    ExecutionRuntime, GateResult, PlanExecutionStatus, load_execution_context, status_from_context,
};
use crate::execution::topology::RecommendOutput;
use crate::workflow::late_stage_precedence::{
    GateState, LateStageSignals, resolve as resolve_late_stage_precedence,
};
use crate::workflow::status::{WorkflowPhase, WorkflowRoute, WorkflowRuntime};

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
    reason_family: String,
    diagnostic_reason_codes: Vec<String>,
    task_review_dispatch_id: Option<String>,
    final_review_dispatch_id: Option<String>,
    current_branch_closure_id: Option<String>,
    current_release_readiness_result: Option<String>,
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
            "repairing" => {
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
    let context = build_context_with_plan(current_dir, Some(&args.plan))?;
    let plan_path = operator_plan_path(&context, args);
    let (phase, phase_detail, review_state_status, qa_requirement, recording_context, execution_command_context, next_action, recommended_command) =
        if matches!(
            context.phase.as_str(),
            "document_release_pending"
                | "final_review_pending"
                | "qa_pending"
                | "ready_for_branch_completion"
        ) && late_stage_stale_unreviewed(
            context.gate_review.as_ref(),
            context.gate_finish.as_ref(),
        ) {
            (
                String::from("executing"),
                String::from("execution_reentry_required"),
                String::from("stale_unreviewed"),
                context.qa_requirement.clone(),
                None,
                None,
                String::from("repair review state / reenter execution"),
                Some(format!(
                    "featureforge plan execution repair-review-state --plan {plan_path}"
                )),
            )
        } else if let Some(status) = context.execution_status.as_ref() {
            if let Some(task_number) = task_review_dispatch_task(status) {
                (
                    String::from("task_closure_pending"),
                    String::from("task_review_dispatch_required"),
                    String::from("clean"),
                    None,
                    None,
                    None,
                    String::from("dispatch review"),
                    Some(format!(
                        "featureforge plan execution record-review-dispatch --plan {plan_path} --scope task --task {task_number}"
                    )),
                )
            } else if let Some(task_number) =
                task_review_result_pending_task(status, context.task_review_dispatch_id.as_deref())
            {
                let recommended_command = if args.external_review_result_ready {
                    context.task_review_dispatch_id.as_ref().map(|dispatch_id| {
                        format!(
                            "featureforge plan execution close-current-task --plan {plan_path} --task {task_number} --dispatch-id {dispatch_id} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
                        )
                    })
                } else {
                    None
                };
                (
                    String::from("task_closure_pending"),
                    String::from(if args.external_review_result_ready {
                        "task_closure_recording_ready"
                    } else {
                        "task_review_result_pending"
                    }),
                    String::from("clean"),
                    None,
                    if args.external_review_result_ready {
                        context.task_review_dispatch_id.as_ref().map(|dispatch_id| {
                            WorkflowOperatorRecordingContext {
                                task_number: Some(task_number),
                                dispatch_id: Some(dispatch_id.clone()),
                                branch_closure_id: None,
                            }
                        })
                    } else {
                        None
                    },
                    None,
                    String::from(if args.external_review_result_ready {
                        "close current task"
                    } else {
                        "wait for external review result"
                    }),
                    recommended_command,
                )
            } else if context.phase == "final_review_pending"
                && context.current_branch_closure_id.is_none()
            {
                (
                    String::from("document_release_pending"),
                    String::from("branch_closure_recording_required_for_release_readiness"),
                    String::from("missing_current_closure"),
                    None,
                    None,
                    None,
                    String::from("record branch closure"),
                    Some(format!(
                        "featureforge plan execution record-branch-closure --plan {plan_path}"
                    )),
                )
            } else if context.phase == "final_review_pending"
                && context.current_release_readiness_result.as_deref() != Some("ready")
            {
                (
                    String::from("document_release_pending"),
                    String::from(
                        if context.current_release_readiness_result.as_deref() == Some("blocked") {
                            "release_blocker_resolution_required"
                        } else {
                            "release_readiness_recording_ready"
                        },
                    ),
                    String::from("clean"),
                    None,
                    context.current_branch_closure_id.as_ref().map(|branch_closure_id| {
                        WorkflowOperatorRecordingContext {
                            task_number: None,
                            dispatch_id: None,
                            branch_closure_id: Some(branch_closure_id.clone()),
                        }
                    }),
                    None,
                    String::from(
                        if context.current_release_readiness_result.as_deref() == Some("blocked") {
                            "resolve release blocker"
                        } else {
                            "advance late stage"
                        },
                    ),
                    Some(format!(
                        "featureforge plan execution advance-late-stage --plan {plan_path} --result ready|blocked --summary-file <path>"
                    )),
                )
            } else if context.phase == "final_review_pending"
                && context.final_review_dispatch_id.is_none()
            {
                (
                    String::from("final_review_pending"),
                    String::from("final_review_dispatch_required"),
                    String::from("clean"),
                    None,
                    None,
                    None,
                    String::from("dispatch final review"),
                    Some(format!(
                        "featureforge plan execution record-review-dispatch --plan {plan_path} --scope final-review"
                    )),
                )
            } else if context.phase == "final_review_pending"
                && context.final_review_dispatch_id.is_some()
            {
                let recommended_command = if args.external_review_result_ready {
                    context.final_review_dispatch_id.as_ref().map(|dispatch_id| {
                        format!(
                            "featureforge plan execution advance-late-stage --plan {plan_path} --dispatch-id {dispatch_id} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>"
                        )
                    })
                } else {
                    None
                };
                (
                    String::from("final_review_pending"),
                    String::from(if args.external_review_result_ready {
                        "final_review_recording_ready"
                    } else {
                        "final_review_outcome_pending"
                    }),
                    String::from("clean"),
                    None,
                    if args.external_review_result_ready {
                        context.final_review_dispatch_id.as_ref().map(|dispatch_id| {
                            WorkflowOperatorRecordingContext {
                                task_number: None,
                                dispatch_id: Some(dispatch_id.clone()),
                                branch_closure_id: context.current_branch_closure_id.clone(),
                            }
                        })
                    } else {
                        None
                    },
                    None,
                    String::from(if args.external_review_result_ready {
                        "advance late stage"
                    } else {
                        "wait for external review result"
                    }),
                    recommended_command,
                )
            } else if context.phase == "document_release_pending" {
                if let Some(branch_closure_id) = context.current_branch_closure_id.as_ref() {
                    (
                        String::from("document_release_pending"),
                        String::from(
                            if context.current_release_readiness_result.as_deref() == Some("blocked") {
                                "release_blocker_resolution_required"
                            } else {
                                "release_readiness_recording_ready"
                            },
                        ),
                        String::from("clean"),
                        None,
                        Some(WorkflowOperatorRecordingContext {
                            task_number: None,
                            dispatch_id: None,
                            branch_closure_id: Some(branch_closure_id.clone()),
                        }),
                        None,
                        String::from(
                            if context.current_release_readiness_result.as_deref() == Some("blocked") {
                                "resolve release blocker"
                            } else {
                                "advance late stage"
                            },
                        ),
                        Some(format!(
                            "featureforge plan execution advance-late-stage --plan {plan_path} --result ready|blocked --summary-file <path>"
                        )),
                    )
                } else {
                    (
                        String::from("document_release_pending"),
                        String::from("branch_closure_recording_required_for_release_readiness"),
                        String::from("missing_current_closure"),
                        None,
                        None,
                        None,
                        String::from("record branch closure"),
                        Some(format!(
                            "featureforge plan execution record-branch-closure --plan {plan_path}"
                        )),
                    )
                }
            } else if context.phase == "qa_pending" && context.current_branch_closure_id.is_none() {
                (
                    String::from("document_release_pending"),
                    String::from("branch_closure_recording_required_for_release_readiness"),
                    String::from("missing_current_closure"),
                    None,
                    None,
                    None,
                    String::from("record branch closure"),
                    Some(format!(
                        "featureforge plan execution record-branch-closure --plan {plan_path}"
                    )),
                )
            } else if context.phase == "qa_pending" {
                match context.qa_requirement.as_deref() {
                    Some("required") if finish_requires_test_plan_refresh(&context) => (
                        String::from("qa_pending"),
                        String::from("test_plan_refresh_required"),
                        String::from("clean"),
                        Some(String::from("required")),
                        None,
                        None,
                        String::from("refresh test plan"),
                        None,
                    ),
                    Some("required") => (
                        String::from("qa_pending"),
                        String::from("qa_recording_required"),
                        String::from("clean"),
                        Some(String::from("required")),
                        None,
                        None,
                        String::from("run QA"),
                        Some(format!(
                            "featureforge plan execution record-qa --plan {plan_path} --result pass|fail --summary-file <path>"
                        )),
                    ),
                    _ => (
                        String::from("pivot_required"),
                        String::from("planning_reentry_required"),
                        String::from("clean"),
                        None,
                        None,
                        None,
                        String::from("record pivot"),
                        Some(format!(
                            "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                        )),
                    ),
                }
            } else if context.phase == "ready_for_branch_completion" {
                match context.qa_requirement.as_deref() {
                    Some("required") | Some("not-required") => {
                        let qa_requirement = context.qa_requirement.clone();
                        if context.gate_finish.as_ref().is_some_and(|gate| gate.allowed) {
                            (
                                String::from("ready_for_branch_completion"),
                                String::from("finish_completion_gate_ready"),
                                String::from("clean"),
                                qa_requirement,
                                None,
                                None,
                                String::from("run finish completion gate"),
                                Some(format!(
                                    "featureforge plan execution gate-finish --plan {plan_path}"
                                )),
                            )
                        } else {
                            (
                                String::from("ready_for_branch_completion"),
                                String::from("finish_review_gate_ready"),
                                String::from("clean"),
                                qa_requirement,
                                None,
                                None,
                                String::from("run finish review gate"),
                                Some(format!(
                                    "featureforge plan execution gate-review --plan {plan_path}"
                                )),
                            )
                        }
                    }
                    _ => (
                        String::from("pivot_required"),
                        String::from("planning_reentry_required"),
                        String::from("clean"),
                        None,
                        None,
                        None,
                        String::from("record pivot"),
                        Some(format!(
                            "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                        )),
                    ),
                }
            } else {
                let phase_detail = match context.phase.as_str() {
                    "executing" | "repairing" => "execution_in_progress",
                    "handoff_required" => "handoff_recording_required",
                    "pivot_required" => "planning_reentry_required",
                    _ => "execution_in_progress",
                };
                let recommended_command = match context.phase.as_str() {
                    "handoff_required" => Some(format!(
                        "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
                    )),
                    "pivot_required" => Some(format!(
                        "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                    )),
                    _ => None,
                };
                (
                    context.phase.clone(),
                    String::from(phase_detail),
                    String::from("clean"),
                    None,
                    None,
                    None,
                    next_action_for_context(&context).replace('_', " "),
                    recommended_command,
                )
            }
        } else {
            let phase_detail = match context.phase.as_str() {
                "handoff_required" => "handoff_recording_required",
                "pivot_required" => "planning_reentry_required",
                _ => "planning_reentry_required",
            };
            let recommended_command = match context.phase.as_str() {
                "handoff_required" => Some(format!(
                    "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
                )),
                "pivot_required" => Some(format!(
                    "featureforge workflow record-pivot --plan {plan_path} --reason <reason>"
                )),
                _ => None,
            };
            (
                context.phase.clone(),
                String::from(phase_detail),
                String::from("clean"),
                None,
                None,
                None,
                next_action_for_context(&context).replace('_', " "),
                recommended_command,
            )
        };

    let follow_up_override = match phase.as_str() {
        "handoff_required" => String::from("record_handoff"),
        "pivot_required" => String::from("record_pivot"),
        _ => String::from("none"),
    };

    Ok(WorkflowOperator {
        schema_version: WORKFLOW_OPERATOR_SCHEMA_VERSION,
        phase,
        phase_detail,
        review_state_status,
        qa_requirement,
        follow_up_override,
        finish_review_gate_pass_branch_closure_id: context
            .gate_review
            .as_ref()
            .filter(|gate| gate.allowed)
            .and_then(|_| context.current_branch_closure_id.clone()),
        recording_context,
        execution_command_context,
        next_action,
        recommended_command,
        spec_path: context.route.spec_path,
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
    build_context_with_plan(current_dir, None)
}

fn build_context_with_plan(
    current_dir: &Path,
    plan_override: Option<&Path>,
) -> Result<OperatorContext, JsonFailure> {
    let workflow = WorkflowRuntime::discover_read_only(current_dir).map_err(JsonFailure::from)?;
    let mut route = workflow.resolve().map_err(JsonFailure::from)?;
    if let Some(plan_override) = plan_override {
        route.plan_path = plan_override.to_string_lossy().into_owned();
    }
    let mut execution_status = None;
    let mut plan_contract = None;
    let mut preflight = None;
    let mut gate_review = None;
    let mut gate_finish = None;
    let execution_preflight_block_reason = None;
    let mut task_review_dispatch_id = None;
    let mut final_review_dispatch_id = None;

    if route.status == "implementation_ready" {
        if let Some(report) = analyze_plan_if_available(&route).map_err(JsonFailure::from)? {
            plan_contract = Some(report);
        }
        if !route.plan_path.is_empty() {
            let runtime = ExecutionRuntime::discover(current_dir)?;
            let status_args = ExecutionStatusArgs {
                plan: PathBuf::from(&route.plan_path),
            };
            let dispatch_ids = dispatch_ids_for_plan(&runtime, &route.plan_path)?;
            task_review_dispatch_id = dispatch_ids.task_review_dispatch_id;
            final_review_dispatch_id = dispatch_ids.final_review_dispatch_id;
            let current_branch_closure_id = dispatch_ids.current_branch_closure_id;
            let current_release_readiness_result = dispatch_ids.current_release_readiness_result;
            let qa_requirement = dispatch_ids.qa_requirement;
            match runtime.status(&status_args) {
                Ok(mut status) => {
                    if let Some(shared_status) = started_status_from_same_branch_worktree(
                        &PathBuf::from(&route.root),
                        &route.plan_path,
                        &status,
                    ) {
                        status = shared_status;
                    }
                    if status.execution_started == "yes" {
                        if !execution_state_has_open_steps(&status) {
                            let review = runtime.gate_review(&status_args)?;
                            gate_finish = Some(runtime.gate_finish(&status_args)?);
                            gate_review = Some(review);
                        }
                    } else if !status_has_accepted_preflight(&status) {
                        preflight = Some(runtime.preflight_read_only(&status_args)?);
                    }
                    execution_status = Some(status);
                    return Ok(build_operator_context(OperatorContextInputs {
                        route,
                        execution_status,
                        plan_contract,
                        preflight,
                        gate_review,
                        gate_finish,
                        execution_preflight_block_reason,
                        task_review_dispatch_id,
                        final_review_dispatch_id,
                        current_branch_closure_id,
                        current_release_readiness_result,
                        qa_requirement,
                    }));
                }
                Err(error) => return Err(error),
            }
        }
    }

    Ok(build_operator_context(OperatorContextInputs {
        route,
        execution_status,
        plan_contract,
        preflight,
        gate_review,
        gate_finish,
        execution_preflight_block_reason,
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        qa_requirement: None,
    }))
}

fn build_operator_context(inputs: OperatorContextInputs) -> OperatorContext {
    let phase = derive_phase(
        &inputs.route.status,
        inputs.execution_status.as_ref(),
        inputs.preflight.as_ref(),
        inputs.gate_review.as_ref(),
        inputs.gate_finish.as_ref(),
    );
    let (reason_family, diagnostic_reason_codes) = late_stage_observability_for_phase(
        &phase,
        inputs.gate_review.as_ref(),
        inputs.gate_finish.as_ref(),
    );

    OperatorContext {
        route: inputs.route,
        execution_status: inputs.execution_status,
        plan_contract: inputs.plan_contract,
        preflight: inputs.preflight,
        gate_review: inputs.gate_review,
        gate_finish: inputs.gate_finish,
        execution_preflight_block_reason: inputs.execution_preflight_block_reason,
        phase,
        reason_family,
        diagnostic_reason_codes,
        task_review_dispatch_id: inputs.task_review_dispatch_id,
        final_review_dispatch_id: inputs.final_review_dispatch_id,
        current_branch_closure_id: inputs.current_branch_closure_id,
        current_release_readiness_result: inputs.current_release_readiness_result,
        qa_requirement: inputs.qa_requirement,
    }
}

fn operator_plan_path(context: &OperatorContext, args: &OperatorArgs) -> String {
    if args.plan.as_os_str().is_empty() {
        context.route.plan_path.clone()
    } else {
        args.plan.to_string_lossy().into_owned()
    }
}

#[derive(Default)]
struct DispatchIds {
    task_review_dispatch_id: Option<String>,
    final_review_dispatch_id: Option<String>,
    current_branch_closure_id: Option<String>,
    current_release_readiness_result: Option<String>,
    qa_requirement: Option<String>,
}

struct OperatorContextInputs {
    route: WorkflowRoute,
    execution_status: Option<PlanExecutionStatus>,
    plan_contract: Option<AnalyzePlanReport>,
    preflight: Option<GateResult>,
    gate_review: Option<GateResult>,
    gate_finish: Option<GateResult>,
    execution_preflight_block_reason: Option<String>,
    task_review_dispatch_id: Option<String>,
    final_review_dispatch_id: Option<String>,
    current_branch_closure_id: Option<String>,
    current_release_readiness_result: Option<String>,
    qa_requirement: Option<String>,
}

fn dispatch_ids_for_plan(runtime: &ExecutionRuntime, plan_path: &str) -> Result<DispatchIds, JsonFailure> {
    if plan_path.is_empty() {
        return Ok(DispatchIds::default());
    }
    let context = load_execution_context(runtime, &PathBuf::from(plan_path))?;
    let Some(overlay) = load_status_authoritative_overlay_checked(&context)? else {
        return Ok(DispatchIds::default());
    };
    let status = status_from_context(&context)?;
    let task_review_dispatch_id = status
        .blocking_task
        .and_then(|task_number| {
            overlay
                .strategy_review_dispatch_lineage
                .get(&format!("task-{task_number}"))
                .and_then(|record| record.dispatch_id.clone())
        })
        .or_else(|| {
            overlay
                .strategy_review_dispatch_lineage
                .iter()
                .filter_map(|(key, record)| {
                    let task_number = key.strip_prefix("task-")?.parse::<u32>().ok()?;
                    let dispatch_id = record.dispatch_id.clone()?;
                    Some((task_number, dispatch_id))
                })
                .max_by_key(|(task_number, _)| *task_number)
                .map(|(_, dispatch_id)| dispatch_id)
        });
    let final_review_dispatch_id = overlay
        .final_review_dispatch_lineage
        .and_then(|record| {
            let execution_run_id = record.execution_run_id?;
            if execution_run_id.trim().is_empty() {
                return None;
            }
            let branch_closure_id = record.branch_closure_id?;
            if overlay.current_branch_closure_id.as_deref()? != branch_closure_id.as_str() {
                return None;
            }
            record.dispatch_id
        });
    Ok(DispatchIds {
        task_review_dispatch_id,
        final_review_dispatch_id,
        current_branch_closure_id: overlay.current_branch_closure_id,
        current_release_readiness_result: overlay.current_release_readiness_result,
        qa_requirement: context.plan_document.qa_requirement.clone(),
    })
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

fn started_status_from_same_branch_worktree(
    current_repo_root: &Path,
    plan_path: &str,
    local_status: &PlanExecutionStatus,
) -> Option<PlanExecutionStatus> {
    if local_status.execution_started == "yes" || plan_path.is_empty() {
        return None;
    }

    let current_root =
        fs::canonicalize(current_repo_root).unwrap_or_else(|_| current_repo_root.to_path_buf());
    for worktree_root in same_branch_worktree_roots(current_repo_root) {
        let canonical_root =
            fs::canonicalize(&worktree_root).unwrap_or_else(|_| worktree_root.clone());
        if canonical_root == current_root {
            continue;
        }

        let runtime = match ExecutionRuntime::discover(&worktree_root) {
            Ok(runtime) => runtime,
            Err(_) => continue,
        };
        let status = match runtime.status(&ExecutionStatusArgs {
            plan: PathBuf::from(plan_path),
        }) {
            Ok(status) => status,
            Err(_) => continue,
        };
        if status.execution_started == "yes" {
            return Some(status);
        }
    }
    None
}

fn same_branch_worktree_roots(current_repo_root: &Path) -> Vec<PathBuf> {
    let output = match Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(current_repo_root)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let mut entries: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut worktree_root: Option<PathBuf> = None;
    let mut branch_ref: Option<String> = None;

    let flush_entry = |entries: &mut Vec<(PathBuf, Option<String>)>,
                       worktree_root: &mut Option<PathBuf>,
                       branch_ref: &mut Option<String>| {
        if let Some(root) = worktree_root.take() {
            entries.push((root, branch_ref.take()));
        }
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.is_empty() {
            flush_entry(&mut entries, &mut worktree_root, &mut branch_ref);
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            flush_entry(&mut entries, &mut worktree_root, &mut branch_ref);
            worktree_root = Some(PathBuf::from(path));
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch ") {
            branch_ref = Some(branch.to_owned());
        }
    }
    flush_entry(&mut entries, &mut worktree_root, &mut branch_ref);

    let current_root =
        fs::canonicalize(current_repo_root).unwrap_or_else(|_| current_repo_root.to_path_buf());
    let mut current_branch_ref = entries.iter().find_map(|(root, branch)| {
        let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if canonical_root == current_root {
            branch.clone()
        } else {
            None
        }
    });

    if current_branch_ref.is_none() {
        let branch_output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(current_repo_root)
            .output();
        if let Ok(output) = branch_output
            && output.status.success()
        {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !branch.is_empty() && branch != "HEAD" {
                current_branch_ref = Some(format!("refs/heads/{branch}"));
            }
        }
    }

    let Some(current_branch_ref) = current_branch_ref else {
        return Vec::new();
    };

    entries
        .into_iter()
        .filter_map(|(root, branch)| {
            if branch.as_deref() == Some(current_branch_ref.as_str()) {
                Some(root)
            } else {
                None
            }
        })
        .collect()
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

fn derive_phase(
    route_status: &str,
    execution_status: Option<&PlanExecutionStatus>,
    preflight: Option<&GateResult>,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> String {
    if route_status != "implementation_ready" {
        return match route_status {
            "spec_draft" => String::from("spec_review"),
            "plan_draft" => String::from("plan_review"),
            "spec_approved_needs_plan" | "stale_plan" => String::from("plan_writing"),
            other => other.to_owned(),
        };
    }

    let Some(execution_status) = execution_status else {
        return String::from("implementation_handoff");
    };

    if let Some(authoritative_phase) = authoritative_public_phase(execution_status) {
        return authoritative_phase.to_owned();
    }

    if execution_status.execution_started != "yes" {
        if status_has_accepted_preflight(execution_status)
            || preflight.map(|result| result.allowed).unwrap_or(false)
        {
            return String::from("execution_preflight");
        }
        return String::from("implementation_handoff");
    }

    if task_boundary_block_reason_code(execution_status).is_some() {
        return String::from("repairing");
    }

    if execution_state_has_open_steps(execution_status) {
        return String::from("executing");
    }

    let Some(gate_finish) = gate_finish else {
        return String::from("final_review_pending");
    };

    if gate_finish
        .reason_codes
        .iter()
        .any(|code| code == "qa_requirement_missing_or_invalid")
    {
        return String::from("pivot_required");
    }

    if gate_finish.allowed && gate_review.is_some_and(|gate| gate.allowed) {
        return String::from("ready_for_branch_completion");
    }

    let release_blocked =
        late_stage_release_blocked(gate_finish) || late_stage_release_truth_blocked(gate_review);
    let review_blocked =
        gate_review.is_some_and(|gate| !gate.allowed) || late_stage_review_blocked(gate_finish);
    let qa_blocked = late_stage_qa_blocked(gate_finish);

    if !(gate_finish.allowed || release_blocked || review_blocked || qa_blocked) {
        return String::from("final_review_pending");
    }

    let decision = resolve_late_stage_precedence(LateStageSignals {
        release: GateState::from_blocked(release_blocked),
        review: GateState::from_blocked(review_blocked),
        qa: GateState::from_blocked(qa_blocked),
    });
    decision.phase.to_owned()
}

fn late_stage_observability_for_phase(
    phase: &str,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> (String, Vec<String>) {
    if !matches!(
        phase,
        "document_release_pending"
            | "final_review_pending"
            | "qa_pending"
            | "ready_for_branch_completion"
    ) {
        return (String::new(), Vec::new());
    }

    let Some(gate_finish) = gate_finish else {
        return (String::new(), Vec::new());
    };

    let mut diagnostic_reason_codes = gate_finish.reason_codes.clone();
    if let Some(gate_review) = gate_review {
        for reason_code in &gate_review.reason_codes {
            if !diagnostic_reason_codes
                .iter()
                .any(|existing| existing == reason_code)
            {
                diagnostic_reason_codes.push(reason_code.clone());
            }
        }
    }

    let release_blocked =
        late_stage_release_blocked(gate_finish) || late_stage_release_truth_blocked(gate_review);
    let review_blocked =
        gate_review.is_some_and(|gate| !gate.allowed) || late_stage_review_blocked(gate_finish);
    let qa_blocked = late_stage_qa_blocked(gate_finish);
    if !(gate_finish.allowed || release_blocked || review_blocked || qa_blocked) {
        return (
            String::from("fallback_fail_closed"),
            diagnostic_reason_codes,
        );
    }

    let decision = resolve_late_stage_precedence(LateStageSignals {
        release: GateState::from_blocked(release_blocked),
        review: GateState::from_blocked(review_blocked),
        qa: GateState::from_blocked(qa_blocked),
    });
    (decision.reason_family.to_owned(), diagnostic_reason_codes)
}

fn authoritative_public_phase(status: &PlanExecutionStatus) -> Option<&'static str> {
    if status.latest_authoritative_sequence == INITIAL_AUTHORITATIVE_SEQUENCE {
        return None;
    }

    match status.harness_phase {
        HarnessPhase::FinalReviewPending
        | HarnessPhase::QaPending
        | HarnessPhase::DocumentReleasePending
        | HarnessPhase::ReadyForBranchCompletion => None,
        _ => Some(status.harness_phase.as_str()),
    }
}

fn status_has_accepted_preflight(status: &PlanExecutionStatus) -> bool {
    status
        .execution_run_id
        .as_ref()
        .is_some_and(|run_id| !run_id.as_str().trim().is_empty())
        || status.harness_phase == HarnessPhase::ExecutionPreflight
}

fn execution_state_has_open_steps(status: &PlanExecutionStatus) -> bool {
    status.active_task.is_some() || status.blocking_task.is_some() || status.resume_task.is_some()
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
        | "repairing"
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
        "repairing" => {
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

fn next_action_for_phase(phase: &str) -> &'static str {
    match phase {
        "needs_brainstorming"
        | "brainstorming"
        | "spec_review"
        | "plan_writing"
        | "plan_review"
        | "plan_update"
        | "workflow_unresolved" => "use_next_skill",
        "implementation_handoff" | "execution_preflight" => "execution_preflight",
        "executing"
        | "contract_drafting"
        | "contract_pending_approval"
        | "contract_approved"
        | "evaluating"
        | "repairing"
        | "handoff_required" => "return_to_execution",
        "pivot_required" => "plan_update",
        "final_review_pending" => "request_code_review",
        "qa_pending" => "run_qa_only",
        "document_release_pending" => "run_document_release",
        "ready_for_branch_completion" => "finish_branch",
        _ => "inspect_workflow",
    }
}

fn next_action_for_context(context: &OperatorContext) -> &'static str {
    if review_requires_execution_reentry(context) {
        "return_to_execution"
    } else if context.phase == "qa_pending" && finish_requires_test_plan_refresh(context) {
        "refresh_test_plan"
    } else {
        next_action_for_phase(&context.phase)
    }
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

fn task_review_dispatch_task(status: &PlanExecutionStatus) -> Option<u32> {
    let blocking_task = status.blocking_task?;
    let reason_code = task_boundary_block_reason_code(status)?;
    if matches!(
        reason_code,
        "prior_task_review_dispatch_missing" | "prior_task_review_dispatch_stale"
    ) {
        Some(blocking_task)
    } else {
        None
    }
}

fn task_review_result_pending_task(
    status: &PlanExecutionStatus,
    dispatch_id: Option<&str>,
) -> Option<u32> {
    if status.blocking_step.is_some() {
        return None;
    }
    let blocking_task = status.blocking_task?;
    let dispatch_id = dispatch_id?.trim();
    if dispatch_id.is_empty() {
        return None;
    }
    if status
        .reason_codes
        .iter()
        .any(|code| code == "prior_task_review_not_green")
    {
        Some(blocking_task)
    } else {
        None
    }
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
    if context.phase != "repairing" {
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

fn late_stage_release_blocked(gate_finish: &GateResult) -> bool {
    gate_finish.failure_class == "ReleaseArtifactNotFresh"
        || gate_has_any_reason(
            Some(gate_finish),
            &[
                "release_artifact_authoritative_provenance_invalid",
                "release_docs_state_missing",
                "release_docs_state_stale",
                "release_docs_state_not_fresh",
            ],
        )
}

fn late_stage_release_truth_blocked(gate_review: Option<&GateResult>) -> bool {
    gate_has_any_reason(
        gate_review,
        &[
            "release_docs_state_missing",
            "release_docs_state_stale",
            "release_docs_state_not_fresh",
        ],
    )
}

fn late_stage_review_blocked(gate_finish: &GateResult) -> bool {
    gate_finish.failure_class == "ReviewArtifactNotFresh"
        || gate_has_any_reason(
            Some(gate_finish),
            &[
                "review_artifact_authoritative_provenance_invalid",
                "final_review_state_missing",
                "final_review_state_stale",
                "final_review_state_not_fresh",
            ],
        )
}

fn late_stage_qa_blocked(gate_finish: &GateResult) -> bool {
    gate_finish.failure_class == "QaArtifactNotFresh"
        || gate_has_any_reason(
            Some(gate_finish),
            &[
                "qa_artifact_authoritative_provenance_invalid",
                "test_plan_artifact_authoritative_provenance_invalid",
                "browser_qa_state_missing",
                "browser_qa_state_stale",
                "browser_qa_state_not_fresh",
            ],
        )
}

fn late_stage_stale_unreviewed(
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    gate_has_any_reason(
        gate_finish,
        &[
            "review_artifact_worktree_dirty",
            REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
            "release_docs_state_stale",
            "release_docs_state_not_fresh",
            "final_review_state_stale",
            "final_review_state_not_fresh",
            "browser_qa_state_stale",
            "browser_qa_state_not_fresh",
        ],
    ) || gate_has_any_reason(
        gate_review,
        &["release_docs_state_stale", "release_docs_state_not_fresh"],
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
