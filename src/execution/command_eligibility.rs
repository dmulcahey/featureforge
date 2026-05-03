use std::sync::OnceLock;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::query::{
    ExecutionRoutingState,
    normalize_public_follow_up_alias as shared_normalize_public_follow_up_alias,
    required_follow_up_from_routing as shared_required_follow_up_from_routing,
};
use crate::execution::state::PlanExecutionStatus;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicMutationKind {
    Begin,
    Complete,
    Reopen,
    Transfer,
    CloseCurrentTask,
    RepairReviewState,
    AdvanceLateStage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicMutationRequest {
    pub kind: PublicMutationKind,
    pub task: Option<u32>,
    pub step: Option<u32>,
    pub expect_execution_fingerprint: Option<String>,
    pub transfer_mode: Option<PublicTransferMode>,
    pub transfer_scope: Option<String>,
    pub command_name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicCommandKind {
    WorkflowOperator,
    Status,
    RepairReviewState,
    CloseCurrentTask,
    AdvanceLateStage,
    Begin,
    Complete,
    Reopen,
    Transfer,
    MaterializeProjectionsStateDirOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicAdvanceLateStageMode {
    Basic,
    ReleaseReadiness,
    Qa,
    FinalReview,
}

pub(crate) fn public_advance_late_stage_mode_for_phase_detail(
    phase_detail: &str,
) -> Option<PublicAdvanceLateStageMode> {
    match phase_detail {
        crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS => {
            Some(PublicAdvanceLateStageMode::Basic)
        }
        crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED => {
            Some(PublicAdvanceLateStageMode::ReleaseReadiness)
        }
        crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY => {
            Some(PublicAdvanceLateStageMode::FinalReview)
        }
        crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED => {
            Some(PublicAdvanceLateStageMode::Qa)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicTransferMode {
    RepairStep,
    WorkflowHandoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicCommand {
    WorkflowOperator {
        plan: String,
        external_review_result_ready: bool,
    },
    Status {
        plan: String,
    },
    RepairReviewState {
        plan: String,
    },
    Begin {
        plan: String,
        task: u32,
        step: u32,
        execution_mode: Option<String>,
        fingerprint: Option<String>,
    },
    Complete {
        plan: String,
        task: u32,
        step: u32,
        source: Option<String>,
        fingerprint: Option<String>,
    },
    Reopen {
        plan: String,
        task: u32,
        step: u32,
        source: Option<String>,
        reason: Option<String>,
        fingerprint: Option<String>,
    },
    TransferRepairStep {
        plan: String,
        task: u32,
        step: u32,
        fingerprint: Option<String>,
    },
    TransferHandoff {
        plan: String,
        scope: String,
    },
    CloseCurrentTask {
        plan: String,
        task: Option<u32>,
        result_inputs_required: bool,
    },
    AdvanceLateStage {
        plan: String,
        mode: PublicAdvanceLateStageMode,
    },
    MaterializeProjectionsStateDirOnly {
        plan: String,
        scope: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicCommandInvocation {
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublicCommandInputKind {
    Text,
    Enum,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PublicCommandInputRequirement {
    pub name: String,
    pub kind: PublicCommandInputKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub must_exist: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_when: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationEligibilitySource {
    ExactRoute,
    ExplicitRepairTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationEligibilityDecision {
    pub allowed: bool,
    pub source: Option<MutationEligibilitySource>,
    pub reason_code: &'static str,
    pub detail: String,
}

impl PublicMutationKind {
    pub fn from_execution_command_kind(command_kind: &str) -> Option<Self> {
        match command_kind {
            "begin" => Some(Self::Begin),
            "complete" => Some(Self::Complete),
            "reopen" => Some(Self::Reopen),
            _ => None,
        }
    }

    fn execution_command_kind(&self) -> Option<&'static str> {
        match self {
            Self::Begin => Some("begin"),
            Self::Complete => Some("complete"),
            Self::Reopen => Some("reopen"),
            Self::Transfer
            | Self::CloseCurrentTask
            | Self::RepairReviewState
            | Self::AdvanceLateStage => None,
        }
    }

    pub fn public_command_name(&self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Complete => "complete",
            Self::Reopen => "reopen",
            Self::Transfer => "transfer",
            Self::CloseCurrentTask => "close-current-task",
            Self::RepairReviewState => "repair-review-state",
            Self::AdvanceLateStage => "advance-late-stage",
        }
    }
}

impl PublicCommand {
    pub fn kind(&self) -> PublicCommandKind {
        match self {
            Self::WorkflowOperator { .. } => PublicCommandKind::WorkflowOperator,
            Self::Status { .. } => PublicCommandKind::Status,
            Self::RepairReviewState { .. } => PublicCommandKind::RepairReviewState,
            Self::Begin { .. } => PublicCommandKind::Begin,
            Self::Complete { .. } => PublicCommandKind::Complete,
            Self::Reopen { .. } => PublicCommandKind::Reopen,
            Self::TransferRepairStep { .. } | Self::TransferHandoff { .. } => {
                PublicCommandKind::Transfer
            }
            Self::CloseCurrentTask { .. } => PublicCommandKind::CloseCurrentTask,
            Self::AdvanceLateStage { .. } => PublicCommandKind::AdvanceLateStage,
            Self::MaterializeProjectionsStateDirOnly { .. } => {
                PublicCommandKind::MaterializeProjectionsStateDirOnly
            }
        }
    }

    pub fn expect_execution_fingerprint(&self) -> Option<&str> {
        match self {
            Self::Begin { fingerprint, .. }
            | Self::Complete { fingerprint, .. }
            | Self::Reopen { fingerprint, .. }
            | Self::TransferRepairStep { fingerprint, .. } => fingerprint.as_deref(),
            Self::WorkflowOperator { .. }
            | Self::Status { .. }
            | Self::RepairReviewState { .. }
            | Self::TransferHandoff { .. }
            | Self::CloseCurrentTask { .. }
            | Self::AdvanceLateStage { .. }
            | Self::MaterializeProjectionsStateDirOnly { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn parse_display_command(command: &str) -> Option<Self> {
        let tokens = command.split_whitespace().collect::<Vec<_>>();
        match tokens.as_slice() {
            ["featureforge", "workflow", "operator", "--plan", plan] => {
                Some(Self::WorkflowOperator {
                    plan: (*plan).to_owned(),
                    external_review_result_ready: false,
                })
            }
            [
                "featureforge",
                "workflow",
                "operator",
                "--plan",
                plan,
                "--external-review-result-ready",
            ] => Some(Self::WorkflowOperator {
                plan: (*plan).to_owned(),
                external_review_result_ready: true,
            }),
            [
                "featureforge",
                "workflow",
                "operator",
                "--plan",
                plan,
                "--json",
            ] => Some(Self::WorkflowOperator {
                plan: (*plan).to_owned(),
                external_review_result_ready: false,
            }),
            [
                "featureforge",
                "plan",
                "execution",
                "status",
                "--plan",
                plan,
            ] => Some(Self::Status {
                plan: (*plan).to_owned(),
            }),
            [
                "featureforge",
                "plan",
                "execution",
                "repair-review-state",
                "--plan",
                plan,
            ] => Some(Self::RepairReviewState {
                plan: (*plan).to_owned(),
            }),
            [
                "featureforge",
                "plan",
                "execution",
                "begin",
                "--plan",
                plan,
                "--task",
                task,
                "--step",
                step,
                rest @ ..,
            ] => {
                let flags = ParsedFlags::parse(rest)?;
                Some(Self::Begin {
                    plan: (*plan).to_owned(),
                    task: parse_u32_token(task)?,
                    step: parse_u32_token(step)?,
                    execution_mode: flags.execution_mode,
                    fingerprint: flags.expect_execution_fingerprint,
                })
            }
            [
                "featureforge",
                "plan",
                "execution",
                "complete",
                "--plan",
                plan,
                "--task",
                task,
                "--step",
                step,
                rest @ ..,
            ] => {
                let flags = ParsedFlags::parse(rest)?;
                Some(Self::Complete {
                    plan: (*plan).to_owned(),
                    task: parse_u32_token(task)?,
                    step: parse_u32_token(step)?,
                    source: flags.source,
                    fingerprint: flags.expect_execution_fingerprint,
                })
            }
            [
                "featureforge",
                "plan",
                "execution",
                "reopen",
                "--plan",
                plan,
                "--task",
                task,
                "--step",
                step,
                rest @ ..,
            ] => {
                let flags = ParsedFlags::parse(rest)?;
                Some(Self::Reopen {
                    plan: (*plan).to_owned(),
                    task: parse_u32_token(task)?,
                    step: parse_u32_token(step)?,
                    source: flags.source,
                    reason: flags.reason,
                    fingerprint: flags.expect_execution_fingerprint,
                })
            }
            [
                "featureforge",
                "plan",
                "execution",
                "transfer",
                "--plan",
                plan,
                "--repair-task",
                task,
                "--repair-step",
                step,
                rest @ ..,
            ] => {
                let flags = ParsedFlags::parse(rest)?;
                Some(Self::TransferRepairStep {
                    plan: (*plan).to_owned(),
                    task: parse_u32_token(task)?,
                    step: parse_u32_token(step)?,
                    fingerprint: flags.expect_execution_fingerprint,
                })
            }
            [
                "featureforge",
                "plan",
                "execution",
                "transfer",
                "--plan",
                plan,
                "--scope",
                scope,
                rest @ ..,
            ] => {
                ParsedFlags::parse(rest)?;
                Some(Self::TransferHandoff {
                    plan: (*plan).to_owned(),
                    scope: (*scope).to_owned(),
                })
            }
            [
                "featureforge",
                "plan",
                "execution",
                "close-current-task",
                "--plan",
                plan,
            ] => Some(Self::CloseCurrentTask {
                plan: (*plan).to_owned(),
                task: None,
                result_inputs_required: false,
            }),
            [
                "featureforge",
                "plan",
                "execution",
                "close-current-task",
                "--plan",
                plan,
                "--task",
                task,
                rest @ ..,
            ] => {
                parse_close_current_task_flags(rest)?;
                Some(Self::CloseCurrentTask {
                    plan: (*plan).to_owned(),
                    task: Some(parse_u32_token(task)?),
                    result_inputs_required: !rest.is_empty(),
                })
            }
            [
                "featureforge",
                "plan",
                "execution",
                "advance-late-stage",
                "--plan",
                plan,
            ] => Some(Self::AdvanceLateStage {
                plan: (*plan).to_owned(),
                mode: PublicAdvanceLateStageMode::Basic,
            }),
            [
                "featureforge",
                "plan",
                "execution",
                "advance-late-stage",
                "--plan",
                plan,
                rest @ ..,
            ] => {
                let flags = ParsedFlags::parse(rest)?;
                Some(Self::AdvanceLateStage {
                    plan: (*plan).to_owned(),
                    mode: advance_late_stage_mode_from_flags(&flags),
                })
            }
            [
                "featureforge",
                "plan",
                "execution",
                "materialize-projections",
                "--plan",
                plan,
                rest @ ..,
            ] => {
                let flags = ParsedFlags::parse(rest)?;
                if flags.repo_export {
                    return None;
                }
                Some(Self::MaterializeProjectionsStateDirOnly {
                    plan: (*plan).to_owned(),
                    scope: flags.scope,
                })
            }
            _ => None,
        }
    }

    pub fn to_display_command(&self) -> String {
        match self {
            Self::WorkflowOperator {
                plan,
                external_review_result_ready,
            } => {
                let suffix = if *external_review_result_ready {
                    " --external-review-result-ready"
                } else {
                    ""
                };
                format!("featureforge workflow operator --plan {plan}{suffix}")
            }
            Self::Status { plan } => format!("featureforge plan execution status --plan {plan}"),
            Self::RepairReviewState { plan } => {
                format!("featureforge plan execution repair-review-state --plan {plan}")
            }
            Self::Begin {
                plan,
                task,
                step,
                execution_mode,
                fingerprint,
            } => format!(
                "featureforge plan execution begin --plan {plan} --task {task} --step {step}{}{}",
                optional_flag(" --execution-mode ", execution_mode.as_deref()),
                optional_flag(" --expect-execution-fingerprint ", fingerprint.as_deref())
            ),
            Self::Complete {
                plan,
                task,
                step,
                source,
                fingerprint,
            } => format!(
                "featureforge plan execution complete --plan {plan} --task {task} --step {step}{}{}; requires claim and verification inputs",
                optional_flag(" --source ", source.as_deref()),
                optional_flag(" --expect-execution-fingerprint ", fingerprint.as_deref())
            ),
            Self::Reopen {
                plan,
                task,
                step,
                source,
                reason,
                fingerprint,
            } => {
                let reason_suffix = concrete_optional_value(reason.as_deref())
                    .map(|reason| format!(" --reason {reason}"))
                    .unwrap_or_else(|| String::from("; requires reason input"));
                format!(
                    "featureforge plan execution reopen --plan {plan} --task {task} --step {step}{}{reason_suffix}{}",
                    optional_flag(" --source ", source.as_deref()),
                    optional_flag(" --expect-execution-fingerprint ", fingerprint.as_deref())
                )
            }
            Self::TransferRepairStep {
                plan,
                task,
                step,
                fingerprint,
            } => format!(
                "featureforge plan execution transfer --plan {plan} --repair-task {task} --repair-step {step}{}; requires source and reason inputs",
                optional_flag(" --expect-execution-fingerprint ", fingerprint.as_deref())
            ),
            Self::TransferHandoff { plan, scope } => {
                format!(
                    "featureforge plan execution transfer --plan {plan} --scope {scope}; requires owner and reason inputs"
                )
            }
            Self::CloseCurrentTask {
                plan,
                task,
                result_inputs_required,
            } => {
                let task_arg =
                    optional_flag(" --task ", task.map(|task| task.to_string()).as_deref());
                let result_requirement = if *result_inputs_required {
                    "; requires review and verification inputs"
                } else {
                    ""
                };
                format!(
                    "featureforge plan execution close-current-task --plan {plan}{task_arg}{result_requirement}"
                )
            }
            Self::AdvanceLateStage { plan, mode } => match mode {
                PublicAdvanceLateStageMode::Basic => {
                    format!("featureforge plan execution advance-late-stage --plan {plan}")
                }
                PublicAdvanceLateStageMode::ReleaseReadiness => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; requires release-readiness result and summary file inputs"
                ),
                PublicAdvanceLateStageMode::Qa => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; requires QA result and summary file inputs"
                ),
                PublicAdvanceLateStageMode::FinalReview => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; requires final-review reviewer, result, and summary file inputs"
                ),
            },
            Self::MaterializeProjectionsStateDirOnly { plan, scope } => {
                format!(
                    "featureforge plan execution materialize-projections --plan {plan}{}",
                    optional_flag(" --scope ", scope.as_deref())
                )
            }
        }
    }

    pub fn to_invocation(&self) -> Option<PublicCommandInvocation> {
        if !self.required_inputs().is_empty() {
            return None;
        }
        let mut argv = vec![String::from("featureforge")];
        match self {
            Self::WorkflowOperator {
                plan,
                external_review_result_ready,
            } => {
                push_args(&mut argv, ["workflow", "operator", "--plan"]);
                argv.push(plan.clone());
                if *external_review_result_ready {
                    argv.push(String::from("--external-review-result-ready"));
                }
            }
            Self::Status { plan } => {
                push_args(&mut argv, ["plan", "execution", "status", "--plan"]);
                argv.push(plan.clone());
            }
            Self::RepairReviewState { plan } => {
                push_args(
                    &mut argv,
                    ["plan", "execution", "repair-review-state", "--plan"],
                );
                argv.push(plan.clone());
            }
            Self::Begin {
                plan,
                task,
                step,
                execution_mode,
                fingerprint,
            } => {
                push_execution_task_step_args(&mut argv, "begin", plan, *task, *step);
                push_optional_flag(&mut argv, "--execution-mode", execution_mode.as_deref());
                push_optional_flag(
                    &mut argv,
                    "--expect-execution-fingerprint",
                    fingerprint.as_deref(),
                );
            }
            Self::Complete {
                plan,
                task,
                step,
                source,
                fingerprint,
            } => {
                push_execution_task_step_args(&mut argv, "complete", plan, *task, *step);
                push_optional_flag(&mut argv, "--source", source.as_deref());
                push_optional_flag(
                    &mut argv,
                    "--expect-execution-fingerprint",
                    fingerprint.as_deref(),
                );
            }
            Self::Reopen {
                plan,
                task,
                step,
                source,
                reason,
                fingerprint,
            } => {
                push_execution_task_step_args(&mut argv, "reopen", plan, *task, *step);
                push_optional_flag(&mut argv, "--source", source.as_deref());
                push_optional_flag(&mut argv, "--reason", reason.as_deref());
                push_optional_flag(
                    &mut argv,
                    "--expect-execution-fingerprint",
                    fingerprint.as_deref(),
                );
            }
            Self::TransferRepairStep {
                plan,
                task,
                step,
                fingerprint,
            } => {
                push_args(&mut argv, ["plan", "execution", "transfer", "--plan"]);
                argv.push(plan.clone());
                push_arg_value(&mut argv, "--repair-task", task.to_string());
                push_arg_value(&mut argv, "--repair-step", step.to_string());
                push_optional_flag(
                    &mut argv,
                    "--expect-execution-fingerprint",
                    fingerprint.as_deref(),
                );
            }
            Self::TransferHandoff { plan, scope } => {
                push_args(&mut argv, ["plan", "execution", "transfer", "--plan"]);
                argv.push(plan.clone());
                push_arg_value(&mut argv, "--scope", scope.clone());
            }
            Self::CloseCurrentTask {
                plan,
                task,
                result_inputs_required: _,
            } => {
                push_args(
                    &mut argv,
                    ["plan", "execution", "close-current-task", "--plan"],
                );
                argv.push(plan.clone());
                if let Some(task) = task {
                    push_arg_value(&mut argv, "--task", task.to_string());
                }
            }
            Self::AdvanceLateStage { plan, mode } => {
                push_args(
                    &mut argv,
                    ["plan", "execution", "advance-late-stage", "--plan"],
                );
                argv.push(plan.clone());
                match mode {
                    PublicAdvanceLateStageMode::Basic => {}
                    PublicAdvanceLateStageMode::ReleaseReadiness
                    | PublicAdvanceLateStageMode::Qa
                    | PublicAdvanceLateStageMode::FinalReview => {}
                }
            }
            Self::MaterializeProjectionsStateDirOnly { plan, scope } => {
                push_args(
                    &mut argv,
                    ["plan", "execution", "materialize-projections", "--plan"],
                );
                argv.push(plan.clone());
                push_optional_flag(&mut argv, "--scope", scope.as_deref());
            }
        }
        if public_argv_has_template_tokens(&argv) {
            return None;
        }
        Some(PublicCommandInvocation { argv })
    }

    pub fn to_argv(&self) -> Vec<String> {
        self.to_invocation()
            .expect("public command argv requested for a command with missing inputs")
            .argv
    }

    pub fn required_inputs(&self) -> Vec<PublicCommandInputRequirement> {
        match self {
            Self::Begin { fingerprint, .. }
                if concrete_optional_value(fingerprint.as_deref()).is_none() =>
            {
                vec![input_text("expect_execution_fingerprint")]
            }
            Self::Complete {
                source,
                fingerprint,
                ..
            } => {
                let mut inputs = Vec::new();
                if concrete_optional_value(source.as_deref()).is_none() {
                    inputs.push(input_execution_source("source"));
                }
                inputs.push(input_text("claim"));
                inputs.push(input_enum(
                    "verification_mode",
                    ["manual_summary", "command_result"],
                ));
                inputs.push(input_text_when(
                    "manual_verify_summary",
                    "verification_mode=manual_summary",
                ));
                inputs.push(input_text_when(
                    "verify_command",
                    "verification_mode=command_result",
                ));
                inputs.push(input_text_when(
                    "verify_result",
                    "verification_mode=command_result",
                ));
                if concrete_optional_value(fingerprint.as_deref()).is_none() {
                    inputs.push(input_text("expect_execution_fingerprint"));
                }
                inputs
            }
            Self::Reopen {
                source,
                reason,
                fingerprint,
                ..
            } => {
                let mut inputs = Vec::new();
                if concrete_optional_value(source.as_deref()).is_none() {
                    inputs.push(input_execution_source("source"));
                }
                if concrete_optional_value(reason.as_deref()).is_none() {
                    inputs.push(input_text("reason"));
                }
                if concrete_optional_value(fingerprint.as_deref()).is_none() {
                    inputs.push(input_text("expect_execution_fingerprint"));
                }
                inputs
            }
            Self::TransferRepairStep { fingerprint, .. } => {
                let mut inputs = vec![input_execution_source("source"), input_text("reason")];
                if concrete_optional_value(fingerprint.as_deref()).is_none() {
                    inputs.push(input_text("expect_execution_fingerprint"));
                }
                inputs
            }
            Self::TransferHandoff { scope, .. } => {
                let mut inputs = Vec::new();
                if concrete_optional_value(Some(scope)).is_none() {
                    inputs.push(input_enum("scope", ["task", "branch"]));
                }
                inputs.push(input_text("owner"));
                inputs.push(input_text("reason"));
                inputs
            }
            Self::CloseCurrentTask { task: None, .. } => vec![input_text("task")],
            Self::CloseCurrentTask {
                result_inputs_required: true,
                ..
            } => close_current_task_result_inputs(),
            Self::AdvanceLateStage { mode, .. } => match mode {
                PublicAdvanceLateStageMode::Basic => Vec::new(),
                PublicAdvanceLateStageMode::ReleaseReadiness => vec![
                    input_enum("result", ["ready", "blocked"]),
                    input_existing_path("summary_file"),
                ],
                PublicAdvanceLateStageMode::Qa => vec![
                    input_enum("result", ["pass", "fail"]),
                    input_existing_path("summary_file"),
                ],
                PublicAdvanceLateStageMode::FinalReview => vec![
                    input_enum(
                        "reviewer_source",
                        [
                            "fresh-context-subagent",
                            "cross-model",
                            "human-independent-reviewer",
                        ],
                    ),
                    input_text("reviewer_id"),
                    input_enum("result", ["pass", "fail"]),
                    input_existing_path("summary_file"),
                ],
            },
            Self::WorkflowOperator { .. }
            | Self::Status { .. }
            | Self::RepairReviewState { .. }
            | Self::Begin { .. }
            | Self::CloseCurrentTask { .. }
            | Self::MaterializeProjectionsStateDirOnly { .. } => Vec::new(),
        }
    }

    pub fn to_mutation_request(&self) -> Option<PublicMutationRequest> {
        match self {
            Self::RepairReviewState { .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::RepairReviewState,
                task: None,
                step: None,
                expect_execution_fingerprint: None,
                transfer_mode: None,
                transfer_scope: None,
                command_name: "repair-review-state",
            }),
            Self::Begin {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Begin,
                task: Some(*task),
                step: Some(*step),
                expect_execution_fingerprint: fingerprint.clone(),
                transfer_mode: None,
                transfer_scope: None,
                command_name: "begin",
            }),
            Self::Complete {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Complete,
                task: Some(*task),
                step: Some(*step),
                expect_execution_fingerprint: fingerprint.clone(),
                transfer_mode: None,
                transfer_scope: None,
                command_name: "complete",
            }),
            Self::Reopen {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Reopen,
                task: Some(*task),
                step: Some(*step),
                expect_execution_fingerprint: fingerprint.clone(),
                transfer_mode: None,
                transfer_scope: None,
                command_name: "reopen",
            }),
            Self::TransferRepairStep {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Transfer,
                task: Some(*task),
                step: Some(*step),
                expect_execution_fingerprint: fingerprint.clone(),
                transfer_mode: Some(PublicTransferMode::RepairStep),
                transfer_scope: None,
                command_name: "transfer",
            }),
            Self::TransferHandoff { scope, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Transfer,
                task: None,
                step: None,
                expect_execution_fingerprint: None,
                transfer_mode: Some(PublicTransferMode::WorkflowHandoff),
                transfer_scope: concrete_optional_value(Some(scope)).map(str::to_owned),
                command_name: "transfer",
            }),
            Self::CloseCurrentTask { task, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::CloseCurrentTask,
                task: *task,
                step: None,
                expect_execution_fingerprint: None,
                transfer_mode: None,
                transfer_scope: None,
                command_name: "close-current-task",
            }),
            Self::AdvanceLateStage { .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::AdvanceLateStage,
                task: None,
                step: None,
                expect_execution_fingerprint: None,
                transfer_mode: None,
                transfer_scope: None,
                command_name: "advance-late-stage",
            }),
            Self::WorkflowOperator { .. }
            | Self::Status { .. }
            | Self::MaterializeProjectionsStateDirOnly { .. } => None,
        }
    }
}

pub(crate) fn recommended_public_command_argv(
    command: Option<&PublicCommand>,
) -> Option<Vec<String>> {
    command
        .and_then(PublicCommand::to_invocation)
        .map(|invocation| invocation.argv)
}

pub(crate) fn recommended_public_command_display(
    command: Option<&PublicCommand>,
) -> Option<String> {
    command.and_then(|command| {
        command
            .to_invocation()
            .map(|_| command.to_display_command())
    })
}

pub(crate) fn required_inputs_for_public_command(
    command: Option<&PublicCommand>,
) -> Vec<PublicCommandInputRequirement> {
    command
        .map(PublicCommand::required_inputs)
        .unwrap_or_default()
}

pub(crate) fn public_command_recommendation_surfaces(
    command: Option<&PublicCommand>,
) -> (
    Option<String>,
    Option<Vec<String>>,
    Vec<PublicCommandInputRequirement>,
) {
    (
        recommended_public_command_display(command),
        recommended_public_command_argv(command),
        required_inputs_for_public_command(command),
    )
}

pub(crate) fn public_argv_has_template_tokens(argv: &[String]) -> bool {
    argv.iter()
        .any(|part| public_argv_part_is_template_token(part))
        || argv.windows(3).any(matches_optional_verification_phrase)
}

fn concrete_optional_value(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !public_argv_part_is_template_token(value))
}

fn public_argv_part_is_template_token(part: &str) -> bool {
    let trimmed = part.trim();
    is_known_template_token(trimmed)
        || trimmed
            .split_once('=')
            .is_some_and(|(_, value)| is_known_template_token(value.trim()))
}

fn is_known_template_token(value: &str) -> bool {
    matches!(
        value,
        "<approved-plan-path>"
            | "<claim>"
            | "<summary>"
            | "<owner>"
            | "<reason>"
            | "<path>"
            | "<source>"
            | "<id>"
            | "pass|fail"
            | "pass|fail|not-run"
            | "ready|blocked"
            | "task|branch"
            | "[when verification ran]"
    )
}

fn matches_optional_verification_phrase(window: &[String]) -> bool {
    matches!(
        window,
        [first, second, third]
            if (first == "[when" || first == "when")
                && second == "verification"
                && (third == "ran]" || third == "ran")
    )
}

fn input_text(name: &str) -> PublicCommandInputRequirement {
    PublicCommandInputRequirement {
        name: name.to_owned(),
        kind: PublicCommandInputKind::Text,
        values: Vec::new(),
        must_exist: false,
        required_when: None,
    }
}

fn input_text_when(name: &str, required_when: &str) -> PublicCommandInputRequirement {
    PublicCommandInputRequirement {
        required_when: Some(required_when.to_owned()),
        ..input_text(name)
    }
}

fn input_enum<const N: usize>(name: &str, values: [&str; N]) -> PublicCommandInputRequirement {
    PublicCommandInputRequirement {
        name: name.to_owned(),
        kind: PublicCommandInputKind::Enum,
        values: values.into_iter().map(str::to_owned).collect(),
        must_exist: false,
        required_when: None,
    }
}

fn input_execution_source(name: &str) -> PublicCommandInputRequirement {
    input_enum(
        name,
        [
            "featureforge:executing-plans",
            "featureforge:subagent-driven-development",
        ],
    )
}

fn input_existing_path(name: &str) -> PublicCommandInputRequirement {
    PublicCommandInputRequirement {
        name: name.to_owned(),
        kind: PublicCommandInputKind::Path,
        values: Vec::new(),
        must_exist: true,
        required_when: None,
    }
}

fn input_existing_path_when(name: &str, required_when: &str) -> PublicCommandInputRequirement {
    PublicCommandInputRequirement {
        required_when: Some(required_when.to_owned()),
        ..input_existing_path(name)
    }
}

fn close_current_task_result_inputs() -> Vec<PublicCommandInputRequirement> {
    vec![
        input_enum("review_result", ["pass", "fail"]),
        input_existing_path("review_summary_file"),
        input_enum("verification_result", ["pass", "fail", "not-run"]),
        input_existing_path_when("verification_summary_file", "verification_result!=not-run"),
    ]
}

#[cfg(test)]
#[derive(Default)]
struct ParsedFlags {
    execution_mode: Option<String>,
    expect_execution_fingerprint: Option<String>,
    source: Option<String>,
    reason: Option<String>,
    result: Option<String>,
    scope: Option<String>,
    reviewer_source: bool,
    reviewer_id: bool,
    repo_export: bool,
}

#[cfg(test)]
impl ParsedFlags {
    fn parse(tokens: &[&str]) -> Option<Self> {
        let mut parsed = Self::default();
        let mut index = 0;
        while index < tokens.len() {
            match tokens[index] {
                "--execution-mode" => {
                    parsed.execution_mode = Some((*tokens.get(index + 1)?).to_owned());
                    index += 2;
                }
                "--expect-execution-fingerprint" => {
                    parsed.expect_execution_fingerprint =
                        Some((*tokens.get(index + 1)?).to_owned());
                    index += 2;
                }
                "--source" => {
                    parsed.source = Some((*tokens.get(index + 1)?).to_owned());
                    index += 2;
                }
                "--scope" => {
                    parsed.scope = Some((*tokens.get(index + 1)?).to_owned());
                    index += 2;
                }
                "--reason" => {
                    parsed.reason = Some((*tokens.get(index + 1)?).to_owned());
                    index += 2;
                }
                "--result" => {
                    parsed.result = Some((*tokens.get(index + 1)?).to_owned());
                    index += 2;
                }
                "--reviewer-source" => {
                    let _ = tokens.get(index + 1)?;
                    parsed.reviewer_source = true;
                    index += 2;
                }
                "--reviewer-id" => {
                    let _ = tokens.get(index + 1)?;
                    parsed.reviewer_id = true;
                    index += 2;
                }
                "--to"
                | "--summary-file"
                | "--review-summary-file"
                | "--verification-summary-file"
                | "--review-result"
                | "--verification-result"
                | "--claim"
                | "--manual-verify-summary" => {
                    let _ = tokens.get(index + 1)?;
                    index += 2;
                }
                "--repo-export" | "--confirm-repo-export" | "--json" => {
                    if tokens[index] == "--repo-export" {
                        parsed.repo_export = true;
                    }
                    index += 1;
                }
                _ => return None,
            }
        }
        Some(parsed)
    }
}

#[cfg(test)]
fn parse_close_current_task_flags(tokens: &[&str]) -> Option<()> {
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            "--review-result"
            | "--review-summary-file"
            | "--verification-result"
            | "--verification-summary-file" => {
                let _ = tokens.get(index + 1)?;
                index += 2;
            }
            _ => return None,
        }
    }
    Some(())
}

#[cfg(test)]
fn advance_late_stage_mode_from_flags(flags: &ParsedFlags) -> PublicAdvanceLateStageMode {
    if flags.reviewer_source || flags.reviewer_id {
        return PublicAdvanceLateStageMode::FinalReview;
    }
    match flags.result.as_deref() {
        Some("ready" | "blocked") => PublicAdvanceLateStageMode::ReleaseReadiness,
        Some("pass" | "fail") => PublicAdvanceLateStageMode::Qa,
        _ => PublicAdvanceLateStageMode::Basic,
    }
}

impl MutationEligibilityDecision {
    fn allow(source: MutationEligibilitySource, reason_code: &'static str, detail: String) -> Self {
        Self {
            allowed: true,
            source: Some(source),
            reason_code,
            detail,
        }
    }

    fn reject(reason_code: &'static str, detail: String) -> Self {
        Self {
            allowed: false,
            source: None,
            reason_code,
            detail,
        }
    }
}

fn hidden_token(parts: &[&str], separator: &str) -> String {
    parts.join(separator)
}

pub(crate) fn hidden_command_tokens() -> &'static [String] {
    static TOKENS: OnceLock<Vec<String>> = OnceLock::new();
    TOKENS.get_or_init(|| {
        vec![
            hidden_token(&["record", "pivot"], "-"),
            hidden_token(&["record", "review", "dispatch"], "-"),
            hidden_token(&["gate", "review"], "-"),
            hidden_token(&["gate", "finish"], "-"),
            hidden_token(&["rebuild", "evidence"], "-"),
            hidden_token(&["plan", "execution", "internal"], " "),
            hidden_token(&["reconcile", "review", "state"], "-"),
            hidden_token(&["plan", "execution", "preflight"], " "),
            hidden_token(&["plan", "execution", "recommend"], " "),
            hidden_token(&["workflow", "recommend"], " "),
            hidden_token(&["workflow", "preflight"], " "),
        ]
    })
}

pub(crate) fn command_invokes_hidden_lane(command: &str) -> bool {
    hidden_command_tokens()
        .iter()
        .any(|token| command.contains(token))
}

#[cfg(test)]
pub(crate) fn command_is_legal_public_command(command: &str) -> bool {
    PublicCommand::parse_display_command(command)
        .is_some_and(|command| !command.to_display_command().is_empty())
}

pub fn decide_public_mutation(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> MutationEligibilityDecision {
    if request.command_name != request.kind.public_command_name() {
        return MutationEligibilityDecision::reject(
            "mutation_hidden_or_unsupported_command",
            format!(
                "{} is not a supported public command token for {:?}.",
                request.command_name, request.kind
            ),
        );
    }

    if status.state_kind == crate::execution::phase::DETAIL_BLOCKED_RUNTIME_BUG {
        return MutationEligibilityDecision::reject(
            "mutation_blocked_runtime_bug",
            format!(
                "{} cannot mutate while public runtime status is blocked_runtime_bug.",
                request.command_name
            ),
        );
    }

    if status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        && request.kind != PublicMutationKind::RepairReviewState
    {
        return MutationEligibilityDecision::reject(
            "mutation_runtime_reconcile_required",
            format!(
                "{} cannot mutate while public runtime status requires reconcile; repair-review-state is the only eligible mutation lane.",
                request.command_name
            ),
        );
    }

    if status_waits_for_external_review_result(status) {
        return MutationEligibilityDecision::reject(
            "mutation_waiting_external_input",
            format!(
                "{} cannot mutate while public runtime status is waiting for external input.",
                request.command_name
            ),
        );
    }

    if request_matches_resume_begin(status, request) {
        return MutationEligibilityDecision::allow(
            MutationEligibilitySource::ExactRoute,
            "mutation_resume_begin_authorized",
            format!(
                "{} is authorized by the public execution resume target.",
                request.command_name
            ),
        );
    }

    if request.kind == PublicMutationKind::RepairReviewState
        && status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    {
        return MutationEligibilityDecision::allow(
            MutationEligibilitySource::ExactRoute,
            "mutation_runtime_reconcile_repair_authorized",
            String::from(
                "repair-review-state is authorized by the public runtime reconcile route.",
            ),
        );
    }

    if let Some(command_kind) = request.kind.execution_command_kind()
        && status
            .execution_command_context
            .as_ref()
            .is_some_and(|context| {
                context.command_kind == command_kind
                    && context.task_number == request.task
                    && context.step_id == request.step
            })
        && request_fingerprint_matches_status(status, request)
    {
        return MutationEligibilityDecision::allow(
            MutationEligibilitySource::ExactRoute,
            "mutation_exact_route_authorized",
            format!(
                "{} is authorized by the exact public execution route.",
                request.command_name
            ),
        );
    }

    if request.kind.execution_command_kind().is_none()
        && route_exposes_public_mutation_request(status, request)
    {
        return MutationEligibilityDecision::allow(
            MutationEligibilitySource::ExactRoute,
            "mutation_exact_route_authorized",
            format!(
                "{} is authorized by the exact public route.",
                request.command_name
            ),
        );
    }

    if request_fingerprint_matches_status(status, request)
        && status.public_repair_targets.iter().any(|target| {
            public_repair_target_matches_request(
                target.command_kind.as_str(),
                target.task,
                target.step,
                request,
            )
        })
    {
        return MutationEligibilityDecision::allow(
            MutationEligibilitySource::ExplicitRepairTarget,
            "mutation_explicit_repair_target_authorized",
            format!(
                "{} is authorized by an explicit public repair target.",
                request.command_name
            ),
        );
    }

    if status_blocks_non_exact_public_mutation(status) {
        return MutationEligibilityDecision::reject(
            "mutation_blocked_until_exact_public_route",
            format!(
                "{} is blocked while the runtime is in phase_detail={} state_kind={}; only the exact public route or an explicit repair target may mutate.",
                request.command_name, status.phase_detail, status.state_kind
            ),
        );
    }

    MutationEligibilityDecision::reject(
        "mutation_not_route_authorized",
        format!(
            "{} is not the exact public route and no explicit repair target is bound.",
            request.command_name
        ),
    )
}

fn status_waits_for_external_review_result(status: &PlanExecutionStatus) -> bool {
    status.external_wait_state.as_deref() == Some("waiting_for_external_review_result")
}

pub(crate) fn public_execution_mutation_is_authorized(
    status: &PlanExecutionStatus,
    command_kind: &str,
    task: u32,
    step: Option<u32>,
) -> bool {
    let Some(kind) = PublicMutationKind::from_execution_command_kind(command_kind) else {
        return false;
    };
    let command_name = kind.public_command_name();
    decide_public_mutation(
        status,
        &PublicMutationRequest {
            kind,
            task: Some(task),
            step,
            expect_execution_fingerprint: None,
            transfer_mode: None,
            transfer_scope: None,
            command_name,
        },
    )
    .allowed
}

#[cfg(test)]
fn public_mutation_request_from_command(command: &str) -> Option<PublicMutationRequest> {
    if command_invokes_hidden_lane(command) {
        return None;
    }
    PublicCommand::parse_display_command(command).and_then(|command| command.to_mutation_request())
}

pub(crate) fn require_public_mutation(
    status: &PlanExecutionStatus,
    request: PublicMutationRequest,
    failure_class: FailureClass,
) -> Result<(), JsonFailure> {
    let decision = decide_public_mutation(status, &request);
    if decision.allowed {
        return Ok(());
    }
    let next_public_command = status
        .recommended_public_command
        .as_ref()
        .map(PublicCommand::to_display_command)
        .or_else(|| {
            status
                .next_public_action
                .as_ref()
                .map(|action| action.command.clone())
        })
        .filter(|command| !command_invokes_hidden_lane(command))
        .unwrap_or_else(|| String::from("none"));
    let task = request
        .task
        .map_or_else(|| String::from("none"), |task| task.to_string());
    let step = request
        .step
        .map_or_else(|| String::from("none"), |step| step.to_string());
    let reason_codes = if status.reason_codes.is_empty() {
        String::from("none")
    } else {
        status.reason_codes.join(",")
    };
    let public_repair_targets = if status.public_repair_targets.is_empty() {
        String::from("none")
    } else {
        status
            .public_repair_targets
            .iter()
            .map(|target| {
                format!(
                    "{}:{:?}:{:?}:{}",
                    target.command_kind, target.task, target.step, target.reason_code
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let blocking_records = if status.blocking_records.is_empty() {
        String::from("none")
    } else {
        status
            .blocking_records
            .iter()
            .map(|record| {
                format!(
                    "{}:{:?}:{}",
                    record.record_type, record.required_follow_up, record.review_state_status
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    Err(JsonFailure::new(
        failure_class,
        format!(
            "{} failed closed: requested task {task} step {step} is not the exact public route and no explicit repair target is bound. Next public action: {next_public_command}. reason_code={}; phase_detail={}; state_kind={}; runtime_reconcile_required={}; blocked_runtime_bug={}; route_reason_codes=[{reason_codes}]; public_repair_targets=[{public_repair_targets}]; blocking_records=[{blocking_records}]; detail={}",
            request.command_name,
            decision.reason_code,
            status.phase_detail,
            status.state_kind,
            status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED,
            status.state_kind == crate::execution::phase::DETAIL_BLOCKED_RUNTIME_BUG,
            decision.detail,
        ),
    ))
}

fn status_blocks_non_exact_public_mutation(status: &PlanExecutionStatus) -> bool {
    status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        || matches!(
            status.state_kind.as_str(),
            crate::execution::phase::DETAIL_BLOCKED_RUNTIME_BUG
                | "terminal"
                | "waiting_external_input"
        )
}

fn request_matches_resume_begin(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    request.kind == PublicMutationKind::Begin
        && status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS
        && status.execution_started == "yes"
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task == request.task
        && status.resume_step == request.step
        && request_fingerprint_matches_status(status, request)
}

fn route_exposes_public_mutation_request(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    status
        .recommended_public_command
        .as_ref()
        .and_then(PublicCommand::to_mutation_request)
        .is_some_and(|route_request| public_mutation_requests_match(&route_request, request))
}

fn public_repair_target_matches_request(
    command_kind: &str,
    task: Option<u32>,
    step: Option<u32>,
    request: &PublicMutationRequest,
) -> bool {
    command_kind == request.kind.public_command_name()
        && task == request.task
        && step == request.step
        && (request.kind != PublicMutationKind::Transfer
            || request.transfer_mode == Some(PublicTransferMode::RepairStep))
}

fn public_mutation_requests_match(
    route_request: &PublicMutationRequest,
    request: &PublicMutationRequest,
) -> bool {
    if route_request.kind != request.kind {
        return false;
    }
    if request.kind == PublicMutationKind::Transfer {
        return route_request.transfer_mode == request.transfer_mode
            && public_transfer_scope_matches(route_request, request)
            && route_request.task == request.task
            && route_request.step == request.step
            && public_mutation_fingerprint_matches(route_request, request);
    }
    route_request.task == request.task
        && route_request.step == request.step
        && public_mutation_fingerprint_matches(route_request, request)
}

fn public_mutation_fingerprint_matches(
    route_request: &PublicMutationRequest,
    request: &PublicMutationRequest,
) -> bool {
    match (
        route_request.expect_execution_fingerprint.as_deref(),
        request.expect_execution_fingerprint.as_deref(),
    ) {
        (_, None) => true,
        (Some(route_fingerprint), Some(request_fingerprint)) => {
            route_fingerprint == request_fingerprint
        }
        (None, Some(_)) => false,
    }
}

fn public_transfer_scope_matches(
    route_request: &PublicMutationRequest,
    request: &PublicMutationRequest,
) -> bool {
    match route_request.transfer_mode {
        Some(PublicTransferMode::WorkflowHandoff) if route_request.transfer_scope.is_none() => {
            request
                .transfer_scope
                .as_deref()
                .is_some_and(|scope| matches!(scope, "task" | "branch"))
        }
        _ => route_request.transfer_scope == request.transfer_scope,
    }
}

fn request_fingerprint_matches_status(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    request
        .expect_execution_fingerprint
        .as_deref()
        .is_none_or(|fingerprint| fingerprint == status.execution_fingerprint)
}

#[cfg(test)]
fn parse_u32_token(token: &str) -> Option<u32> {
    token.parse::<u32>().ok()
}

fn push_args<const N: usize>(argv: &mut Vec<String>, args: [&str; N]) {
    argv.extend(args.into_iter().map(String::from));
}

fn push_arg_value(argv: &mut Vec<String>, flag: &str, value: String) {
    argv.push(flag.to_owned());
    argv.push(value);
}

fn push_optional_flag(argv: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        push_arg_value(argv, flag, value.to_owned());
    }
}

fn push_execution_task_step_args(
    argv: &mut Vec<String>,
    command: &str,
    plan: &str,
    task: u32,
    step: u32,
) {
    push_args(argv, ["plan", "execution"]);
    argv.push(command.to_owned());
    push_arg_value(argv, "--plan", plan.to_owned());
    push_arg_value(argv, "--task", task.to_string());
    push_arg_value(argv, "--step", step.to_string());
}

fn optional_flag(prefix: &str, value: Option<&str>) -> String {
    value.map_or_else(String::new, |value| format!("{prefix}{value}"))
}

fn normalize_public_follow_up(follow_up: &str) -> Option<String> {
    shared_normalize_public_follow_up_alias(Some(follow_up)).map(str::to_owned)
}

pub(crate) fn operator_requires_review_state_repair(operator: &ExecutionRoutingState) -> bool {
    shared_required_follow_up_from_routing(operator).as_deref() == Some("repair_review_state")
}

pub(crate) fn blocked_follow_up_for_operator(operator: &ExecutionRoutingState) -> Option<String> {
    shared_required_follow_up_from_routing(operator)
        .as_deref()
        .and_then(normalize_public_follow_up)
}

pub(crate) fn close_current_task_required_follow_up(
    operator: &ExecutionRoutingState,
) -> Option<String> {
    blocked_follow_up_for_operator(operator)
}

pub(crate) fn late_stage_required_follow_up(
    stage_path: &str,
    operator: &ExecutionRoutingState,
) -> Option<String> {
    let required_follow_up = blocked_follow_up_for_operator(operator)?;
    if stage_path == "release_readiness"
        && !matches!(
            required_follow_up.as_str(),
            "advance_late_stage" | "repair_review_state"
        )
    {
        return None;
    }
    if stage_path == "final_review"
        && !matches!(
            required_follow_up.as_str(),
            "request_external_review" | "repair_review_state"
        )
    {
        return None;
    }
    Some(required_follow_up)
}

pub(crate) fn release_readiness_required_follow_up(
    operator: &ExecutionRoutingState,
) -> Option<String> {
    blocked_follow_up_for_operator(operator).and_then(|required_follow_up| {
        matches!(
            required_follow_up.as_str(),
            "advance_late_stage" | "repair_review_state"
        )
        .then_some(required_follow_up)
    })
}

pub(crate) fn negative_result_follow_up(operator: &ExecutionRoutingState) -> Option<String> {
    blocked_follow_up_for_operator(operator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_command_shapes_round_trip_and_drive_mutation_requests() {
        let commands = [
            PublicCommand::WorkflowOperator {
                plan: String::from("docs/plan.md"),
                external_review_result_ready: false,
            },
            PublicCommand::Status {
                plan: String::from("docs/plan.md"),
            },
            PublicCommand::RepairReviewState {
                plan: String::from("docs/plan.md"),
            },
            PublicCommand::Begin {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                execution_mode: Some(String::from("featureforge:executing-plans")),
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommand::Complete {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                source: Some(String::from("featureforge:executing-plans")),
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommand::Reopen {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                source: Some(String::from("featureforge:executing-plans")),
                reason: Some(String::from("repair")),
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommand::TransferRepairStep {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommand::TransferHandoff {
                plan: String::from("docs/plan.md"),
                scope: String::from("task"),
            },
            PublicCommand::CloseCurrentTask {
                plan: String::from("docs/plan.md"),
                task: Some(1),
                result_inputs_required: true,
            },
            PublicCommand::AdvanceLateStage {
                plan: String::from("docs/plan.md"),
                mode: PublicAdvanceLateStageMode::Basic,
            },
            PublicCommand::MaterializeProjectionsStateDirOnly {
                plan: String::from("docs/plan.md"),
                scope: None,
            },
        ];

        for command in commands {
            let display = command.to_display_command();
            if command.to_invocation().is_some() {
                let parsed = PublicCommand::parse_display_command(&display)
                    .unwrap_or_else(|| panic!("typed command should parse from `{display}`"));
                assert_eq!(parsed, command, "round trip failed for `{display}`");
                assert!(command_is_legal_public_command(&display));
            } else {
                assert!(
                    PublicCommand::parse_display_command(&display).is_none(),
                    "missing-input display should not parse as an exact public command: `{display}`"
                );
                assert!(
                    !command_is_legal_public_command(&display),
                    "missing-input display should not be mutation authority: `{display}`"
                );
            }
        }
    }

    #[test]
    fn malformed_command_suffixes_do_not_pass_public_shape_parsing() {
        let commands = [
            "featureforge plan execution begin --plan docs/plan.md --task 1 --step 2 --expect-execution-fingerprint fp --unexpected",
            "featureforge plan execution close-current-task --plan docs/plan.md --task 1 --review-result pass --review-summary-file review.md --verification-result pass --unexpected",
        ];

        for command in commands {
            assert!(!command_is_legal_public_command(command));
            assert!(public_mutation_request_from_command(command).is_none());
        }
    }

    #[test]
    fn hidden_and_debug_commands_are_unrepresentable_as_typed_public_commands() {
        let commands = vec![
            format!(
                "featureforge plan execution {} --plan docs/plan.md --scope task --task 1",
                ["record", "review", "dispatch"].join("-")
            ),
            format!(
                "featureforge plan execution {} --plan docs/plan.md",
                ["gate", "review"].join("-")
            ),
            format!(
                "featureforge plan execution {} --plan docs/plan.md",
                ["gate", "finish"].join("-")
            ),
            format!(
                "featureforge plan execution {} --plan docs/plan.md",
                ["rebuild", "evidence"].join("-")
            ),
            format!(
                "featureforge {} --plan docs/plan.md",
                ["plan", "execution", "preflight"].join(" ")
            ),
            format!(
                "featureforge plan execution internal {} --plan docs/plan.md",
                ["record", "branch", "closure"].join("-")
            ),
            format!(
                "featureforge {} --plan docs/plan.md",
                ["plan", "execution", "recommend"].join(" ")
            ),
            format!(
                "featureforge plan execution {} --plan docs/plan.md",
                ["reconcile", "review", "state"].join("-")
            ),
            format!(
                "featureforge {} --plan docs/plan.md",
                ["workflow", "preflight"].join(" ")
            ),
            format!(
                "featureforge {} --plan docs/plan.md",
                ["workflow", "recommend"].join(" ")
            ),
        ];

        for command in &commands {
            assert!(
                PublicCommand::parse_display_command(command).is_none(),
                "hidden/debug command must not parse as typed public command: {command}"
            );
            assert!(!command_is_legal_public_command(command));
        }
    }

    #[test]
    fn internal_flags_are_unrepresentable_as_typed_public_commands() {
        let commands = [
            "featureforge plan execution begin --plan docs/plan.md --task 1 --step 2 --dispatch-id dispatch-1",
            "featureforge plan execution complete --plan docs/plan.md --task 1 --step 2 --dispatch-id dispatch-1 --claim done --manual-verify-summary summary.md",
            "featureforge plan execution close-current-task --plan docs/plan.md --task 1 --dispatch-id dispatch-1 --review-result pass --review-summary-file review.md --verification-result pass",
            "featureforge plan execution advance-late-stage --plan docs/plan.md --dispatch-id dispatch-1",
            "featureforge plan execution advance-late-stage --plan docs/plan.md --branch-closure-id branch-closure-1",
        ];

        for command in commands {
            assert!(
                PublicCommand::parse_display_command(command).is_none(),
                "internal flag must not parse as typed public command: {command}"
            );
            assert!(!command_is_legal_public_command(command));
            assert!(public_mutation_request_from_command(command).is_none());
        }
    }

    #[test]
    fn close_current_task_public_command_accepts_concrete_result_flags() {
        let command = "featureforge plan execution close-current-task --plan docs/plan.md --task 1 --review-result pass --review-summary-file review.md --verification-result pass --verification-summary-file verification.md";

        assert!(command_is_legal_public_command(command));
        assert_eq!(
            public_mutation_request_from_command(command)
                .expect("concrete command should map to public close-current-task mutation")
                .kind,
            PublicMutationKind::CloseCurrentTask
        );
    }

    #[test]
    fn missing_input_commands_do_not_emit_executable_argv() {
        let command = PublicCommand::CloseCurrentTask {
            plan: String::from("docs/plan.md"),
            task: Some(1),
            result_inputs_required: true,
        };

        assert_eq!(
            command.to_display_command(),
            "featureforge plan execution close-current-task --plan docs/plan.md --task 1; requires review and verification inputs"
        );
        assert_eq!(
            recommended_public_command_argv(Some(&command)),
            None,
            "commands with unresolved result inputs must not emit executable argv"
        );
        assert_eq!(
            required_inputs_for_public_command(Some(&command))
                .into_iter()
                .map(|input| input.name)
                .collect::<Vec<_>>(),
            vec![
                "review_result",
                "review_summary_file",
                "verification_result",
                "verification_summary_file"
            ]
        );
    }

    #[test]
    fn placeholder_handoff_scope_is_typed_required_input_not_argv() {
        let command = PublicCommand::TransferHandoff {
            plan: String::from("docs/plan.md"),
            scope: String::from("task|branch"),
        };

        assert_eq!(
            recommended_public_command_argv(Some(&command)),
            None,
            "commands with unresolved handoff scope must not emit executable argv"
        );
        let required_inputs = required_inputs_for_public_command(Some(&command));
        assert_eq!(
            required_inputs
                .iter()
                .map(|input| input.name.as_str())
                .collect::<Vec<_>>(),
            vec!["scope", "owner", "reason"]
        );
        assert_eq!(
            required_inputs[0]
                .values
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["task", "branch"],
            "unresolved handoff scope should expose concrete enum values"
        );
    }

    #[test]
    fn bound_argv_allows_literal_template_punctuation_in_plan_paths() {
        let plan = "docs/featureforge/plans/[release]|candidate plan.md";
        let argv = recommended_public_command_argv(Some(&PublicCommand::Begin {
            plan: plan.to_owned(),
            task: 1,
            step: 1,
            execution_mode: Some(String::from("featureforge:executing-plans")),
            fingerprint: Some(String::from("fingerprint")),
        }))
        .expect("fully bound argv should remain executable despite literal path punctuation");

        assert!(
            argv.windows(2)
                .any(|window| window[0] == "--plan" && window[1] == plan),
            "bound plan path should be preserved as a single executable argv element: {argv:?}"
        );
        assert!(!public_argv_has_template_tokens(&argv));
    }
}
