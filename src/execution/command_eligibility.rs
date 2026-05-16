use std::sync::OnceLock;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::follow_up::{FollowUpAliasContext, FollowUpKind, normalize_follow_up_alias};
pub use crate::execution::public_command_types::{
    PublicCommandInputBinding, PublicCommandInputBindingKind, PublicCommandInputKind,
    PublicCommandInputRequirement, PublicCommandInputValues, PublicCommandTemplate,
    PublicCommandTemplateBindingError, PublicCommandTemplateInput, materialize_public_command_argv,
};
use crate::execution::public_route_guidance::{
    EXECUTE_RECOMMENDED_PUBLIC_ARGV_GUIDANCE, OPERATOR_ROUTE_AUTHORITY_REFERENCE,
    WORKFLOW_OPERATOR_TEMPLATE_JSON_QUERY, workflow_operator_json_display_command,
};
use crate::execution::query::{
    ExecutionRoutingState,
    normalize_public_follow_up_alias as shared_normalize_public_follow_up_alias,
    required_follow_up_from_routing as shared_required_follow_up_from_routing,
};
use crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE;
use crate::execution::route_plan::{
    external_wait_state_is_external_wait, state_kind_blocks_local_mutation,
    state_kind_is_blocked_runtime_bug, state_kind_is_external_wait,
    state_kind_is_runtime_reconcile_required, state_kind_or_phase_is_runtime_diagnostic,
};
use crate::execution::state::PlanExecutionStatus;

mod command_kind;
mod execution_target;
mod late_stage;
mod mutation_request;

pub use command_kind::{
    PUBLIC_EXECUTION_COMMAND_KIND_VALUES, PUBLIC_REPAIR_TARGET_COMMAND_KIND_VALUES,
    PublicCommandKind, public_mutation_command_tokens,
};
pub(crate) use execution_target::{
    PublicExecutionCommandTarget, execution_template_inputs_are_bindable,
};
pub use late_stage::PublicAdvanceLateStageMode;
pub(crate) use late_stage::{
    public_advance_late_stage_command_for_follow_up,
    public_advance_late_stage_command_for_phase_detail,
    public_advance_late_stage_final_review_mutation_request,
    public_advance_late_stage_mode_for_invocation, public_advance_late_stage_mode_name,
    public_advance_late_stage_request_mode_matches_routed_public_command,
    routed_public_command_accepts_final_review_inputs, routed_public_command_is_branch_closure,
    routed_public_command_is_final_review, routed_public_command_is_final_review_dispatch,
    routed_public_command_is_finish_completion, routed_public_command_is_finish_review,
    routed_public_command_is_qa, routed_public_command_is_release_readiness,
};
pub use mutation_request::{PublicMutationRequest, PublicTransferMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicCommand {
    WorkflowOperator {
        plan: String,
        external_review_result_ready: bool,
        json: bool,
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

impl PublicCommand {
    pub fn command_kind_name(&self) -> &'static str {
        self.kind().as_str()
    }

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

    pub fn task_closure_result_inputs_required(&self) -> bool {
        matches!(
            self,
            Self::CloseCurrentTask {
                result_inputs_required: true,
                ..
            }
        )
    }

    pub(crate) fn close_current_task_number(&self) -> Option<u32> {
        match self {
            Self::CloseCurrentTask {
                task: Some(task), ..
            } => Some(*task),
            _ => None,
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
                    json: false,
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
                json: false,
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
                json: true,
            }),
            [
                "featureforge",
                "workflow",
                "operator",
                "--plan",
                plan,
                "--external-review-result-ready",
                "--json",
            ] => Some(Self::WorkflowOperator {
                plan: (*plan).to_owned(),
                external_review_result_ready: true,
                json: true,
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
                json,
            } => {
                let suffix = if *external_review_result_ready {
                    " --external-review-result-ready"
                } else {
                    ""
                };
                let json_suffix = if *json { " --json" } else { "" };
                format!("featureforge workflow operator --plan {plan}{suffix}{json_suffix}")
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
                PublicAdvanceLateStageMode::FinalReviewDispatch => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; final-review dispatch checkpoint ready"
                ),
                PublicAdvanceLateStageMode::Qa => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; requires QA result and summary file inputs"
                ),
                PublicAdvanceLateStageMode::FinalReview => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; requires final-review reviewer, result, and summary file inputs"
                ),
                PublicAdvanceLateStageMode::FinishReview => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; finish-review checkpoint ready"
                ),
                PublicAdvanceLateStageMode::FinishCompletion => format!(
                    "featureforge plan execution advance-late-stage --plan {plan}; validates finish completion"
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

    fn to_base_argv(&self) -> Vec<String> {
        let mut argv = vec![String::from("featureforge")];
        match self {
            Self::WorkflowOperator {
                plan,
                external_review_result_ready,
                json,
            } => {
                push_args(&mut argv, ["workflow", "operator", "--plan"]);
                argv.push(plan.clone());
                if *external_review_result_ready {
                    argv.push(String::from("--external-review-result-ready"));
                }
                if *json {
                    argv.push(String::from("--json"));
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
                    | PublicAdvanceLateStageMode::FinalReviewDispatch
                    | PublicAdvanceLateStageMode::Qa
                    | PublicAdvanceLateStageMode::FinalReview
                    | PublicAdvanceLateStageMode::FinishReview
                    | PublicAdvanceLateStageMode::FinishCompletion => {}
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
        argv
    }

    fn to_template_base_argv(&self) -> Vec<String> {
        let mut argv = self.to_base_argv();
        if let Self::TransferHandoff { scope, .. } = self
            && concrete_optional_value(Some(scope)).is_none()
            && let Some(scope_flag_index) = argv.iter().position(|arg| arg == "--scope")
            && argv
                .get(scope_flag_index + 1)
                .is_some_and(|value| value == scope)
        {
            argv.drain(scope_flag_index..=scope_flag_index + 1);
        }
        argv
    }

    pub fn to_input_template(&self) -> Option<PublicCommandTemplate> {
        let required_inputs = self.required_inputs();
        if required_inputs.is_empty() {
            return None;
        }
        Some(PublicCommandTemplate {
            command_kind: self.command_kind_name().to_owned(),
            base_argv: self.to_template_base_argv(),
            required_input_names: required_inputs
                .iter()
                .map(|input| input.name.clone())
                .collect(),
            input_bindings: required_inputs
                .iter()
                .map(public_command_template_input)
                .collect(),
        })
    }

    pub fn to_invocation(&self) -> Option<PublicCommandInvocation> {
        if !self.required_inputs().is_empty() {
            return None;
        }
        let argv = self.to_base_argv();
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
                PublicAdvanceLateStageMode::Basic
                | PublicAdvanceLateStageMode::FinalReviewDispatch
                | PublicAdvanceLateStageMode::FinishReview
                | PublicAdvanceLateStageMode::FinishCompletion => Vec::new(),
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
            Self::RepairReviewState { .. } => Some(PublicMutationRequest::repair_review_state()),
            Self::Begin {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest::begin(
                *task,
                *step,
                fingerprint.clone(),
            )),
            Self::Complete {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest::complete(
                *task,
                *step,
                fingerprint.clone(),
            )),
            Self::Reopen {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest::reopen(
                *task,
                *step,
                fingerprint.clone(),
            )),
            Self::TransferRepairStep {
                task,
                step,
                fingerprint,
                ..
            } => Some(PublicMutationRequest::transfer_repair_step(
                *task,
                *step,
                fingerprint.clone(),
            )),
            Self::TransferHandoff { scope, .. } => Some(PublicMutationRequest::transfer_handoff(
                concrete_optional_value(Some(scope)).map(str::to_owned),
            )),
            Self::CloseCurrentTask { task, .. } => {
                Some(PublicMutationRequest::close_current_task(*task))
            }
            Self::AdvanceLateStage { mode, .. } => {
                Some(PublicMutationRequest::advance_late_stage(*mode))
            }
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
        command.to_invocation().map(|_| match command {
            PublicCommand::WorkflowOperator {
                external_review_result_ready,
                json: true,
                ..
            } => workflow_operator_json_display_command(*external_review_result_ready).to_owned(),
            _ => command.to_display_command(),
        })
    })
}

pub(crate) fn required_inputs_for_public_command(
    command: Option<&PublicCommand>,
) -> Vec<PublicCommandInputRequirement> {
    command
        .map(PublicCommand::required_inputs)
        .unwrap_or_default()
}

pub(crate) fn recommended_public_command_template(
    command: Option<&PublicCommand>,
) -> Option<PublicCommandTemplate> {
    command.and_then(PublicCommand::to_input_template)
}

pub(crate) fn public_command_recommendation_surfaces(
    command: Option<&PublicCommand>,
) -> (
    Option<String>,
    Option<Vec<String>>,
    Option<PublicCommandTemplate>,
    Vec<PublicCommandInputRequirement>,
) {
    (
        recommended_public_command_display(command),
        recommended_public_command_argv(command),
        recommended_public_command_template(command),
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

fn public_command_template_input(
    input: &PublicCommandInputRequirement,
) -> PublicCommandTemplateInput {
    PublicCommandTemplateInput {
        name: input.name.clone(),
        kind: input.kind.clone(),
        binding: public_command_input_binding(&input.name).unwrap_or_else(|| {
            panic!(
                "public command input `{}` must have explicit CLI binding metadata",
                input.name
            )
        }),
        values: input.values.clone(),
        must_exist: input.must_exist,
        required_when: input.required_when.clone(),
        shell_escape_by_caller: false,
    }
}

fn public_command_input_binding(name: &str) -> Option<PublicCommandInputBinding> {
    Some(match name {
        "verification_mode" => PublicCommandInputBinding {
            kind: PublicCommandInputBindingKind::Virtual,
            flag: None,
        },
        "task" => flag_binding("--task"),
        "source" => flag_binding("--source"),
        "reason" => flag_binding("--reason"),
        "expect_execution_fingerprint" => flag_binding("--expect-execution-fingerprint"),
        "claim" => flag_binding("--claim"),
        "manual_verify_summary" => flag_binding("--manual-verify-summary"),
        "verify_command" => flag_binding("--verify-command"),
        "verify_result" => flag_binding("--verify-result"),
        "scope" => flag_binding("--scope"),
        "owner" => flag_binding("--to"),
        "review_result" => flag_binding("--review-result"),
        "review_summary_file" => flag_binding("--review-summary-file"),
        "verification_result" => flag_binding("--verification-result"),
        "verification_summary_file" => flag_binding("--verification-summary-file"),
        "result" => flag_binding("--result"),
        "summary_file" => flag_binding("--summary-file"),
        "reviewer_source" => flag_binding("--reviewer-source"),
        "reviewer_id" => flag_binding("--reviewer-id"),
        _ => return None,
    })
}

fn flag_binding(flag: impl Into<String>) -> PublicCommandInputBinding {
    PublicCommandInputBinding {
        kind: PublicCommandInputBindingKind::Flag,
        flag: Some(flag.into()),
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

pub const HIDDEN_COMMAND_OR_FLAG_TOKENS: &[&str] = &[
    "erpbeq-cvibg",
    "erpbeq-erivrj-qvfcngpu",
    "erpbeq-oenapu-pybfher",
    "erpbeq-eryrnfr-ernqvarff",
    "erpbeq-svany-erivrj",
    "erpbeq-dn",
    "tngr-erivrj",
    "tngr-svavfu",
    "erohvyq-rivqrapr",
    "erpbapvyr-erivrj-fgngr",
    "cyna rkrphgvba vagreany",
    "cyna rkrphgvba cersyvtug",
    "cyna rkrphgvba erpbzzraq",
    "jbexsybj erpbzzraq",
    "jbexsybj cersyvtug",
    "--qvfcngpu-vq",
    "--oenapu-pybfher-vq",
];

static HIDDEN_COMMAND_OR_FLAG_TOKEN_STRINGS: OnceLock<Vec<String>> = OnceLock::new();

pub fn hidden_command_or_flag_tokens() -> &'static [String] {
    HIDDEN_COMMAND_OR_FLAG_TOKEN_STRINGS
        .get_or_init(|| {
            HIDDEN_COMMAND_OR_FLAG_TOKENS
                .iter()
                .map(|encoded| decode_rot13_ascii(encoded))
                .collect()
        })
        .as_slice()
}

fn decode_rot13_ascii(encoded: &str) -> String {
    encoded
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'm' | b'A'..=b'M' => byte + 13,
            b'n'..=b'z' | b'N'..=b'Z' => byte - 13,
            _ => byte,
        })
        .map(char::from)
        .collect()
}

pub(crate) fn hidden_command_tokens() -> &'static [String] {
    hidden_command_or_flag_tokens()
}

pub(crate) fn command_invokes_hidden_lane(command: &str) -> bool {
    hidden_command_tokens()
        .iter()
        .any(|token| command.contains(token.as_str()))
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
    let Some(command_name) = request.public_command_name() else {
        return MutationEligibilityDecision::reject(
            "mutation_hidden_or_unsupported_command",
            format!(
                "{} is not a supported public command token for {:?}.",
                request.command_name_for_diagnostics(),
                request.kind
            ),
        );
    };

    if state_kind_is_blocked_runtime_bug(&status.state_kind) {
        return MutationEligibilityDecision::reject(
            "mutation_blocked_runtime_bug",
            format!(
                "{} cannot mutate while public runtime status is blocked_runtime_bug.",
                command_name
            ),
        );
    }

    if status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED {
        if request.kind != PublicCommandKind::RepairReviewState {
            return MutationEligibilityDecision::reject(
                "mutation_runtime_reconcile_required",
                format!(
                    "{} cannot mutate while public runtime status requires reconcile.",
                    command_name
                ),
            );
        }
        if runtime_reconcile_route_is_diagnostic_only(status) {
            return MutationEligibilityDecision::reject(
                "mutation_runtime_reconcile_diagnostic_only",
                String::from(
                    "repair-review-state cannot mutate while runtime_reconcile_required is diagnostic-only and no public repair target is bound.",
                ),
            );
        }
    }

    let exact_public_route_exposed = request.execution_command_kind().is_none()
        && route_exposes_public_mutation_request(status, request);
    if exact_public_route_exposed && external_wait_exact_route_exception(status, request) {
        return MutationEligibilityDecision::allow(
            MutationEligibilitySource::ExactRoute,
            "mutation_exact_route_authorized",
            format!("{command_name} is authorized by the exact public route."),
        );
    }

    if status_waits_for_external_review_result(status) {
        return MutationEligibilityDecision::reject(
            "mutation_waiting_external_input",
            format!(
                "{} cannot mutate while public runtime status is waiting for external input.",
                command_name
            ),
        );
    }

    if state_kind_blocks_all_public_mutation(status, request) {
        return MutationEligibilityDecision::reject(
            "mutation_state_kind_blocks_local_mutation",
            format!(
                "{} cannot mutate while public runtime status is state_kind={}; stale exact routes and repair targets are ignored until status/operator exposes a current typed public route.",
                command_name, status.state_kind
            ),
        );
    }

    if exact_public_route_exposed {
        return MutationEligibilityDecision::allow(
            MutationEligibilitySource::ExactRoute,
            "mutation_exact_route_authorized",
            format!("{command_name} is authorized by the exact public route."),
        );
    }

    if let Some(command_kind) = request.execution_command_kind()
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
                command_name
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
                command_name
            ),
        );
    }

    if status_blocks_non_exact_public_mutation(status) {
        return MutationEligibilityDecision::reject(
            "mutation_blocked_until_exact_public_route",
            format!(
                "{} is blocked while the runtime is in phase_detail={} state_kind={}; only the exact public route or an explicit repair target may mutate.",
                command_name, status.phase_detail, status.state_kind
            ),
        );
    }

    MutationEligibilityDecision::reject(
        "mutation_not_route_authorized",
        format!(
            "{} is not the exact public route and no explicit repair target is bound.",
            command_name
        ),
    )
}

fn status_waits_for_external_review_result(status: &PlanExecutionStatus) -> bool {
    external_wait_state_is_external_wait(status.external_wait_state.as_deref())
}

fn external_wait_exact_route_exception(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    state_kind_is_external_wait(&status.state_kind)
        && status.phase_detail == crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
        && request.kind == PublicCommandKind::AdvanceLateStage
}

fn state_kind_blocks_all_public_mutation(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    state_kind_blocks_local_mutation(&status.state_kind)
        && !runtime_reconcile_repair_route_exception(status, request)
}

fn runtime_reconcile_repair_route_exception(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    request.kind == PublicCommandKind::RepairReviewState
        && status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        && state_kind_is_runtime_reconcile_required(&status.state_kind)
        && !runtime_reconcile_route_is_diagnostic_only(status)
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
    require_public_mutation_decision(status, request, failure_class).map(|_| ())
}

pub(crate) fn require_public_mutation_decision(
    status: &PlanExecutionStatus,
    request: PublicMutationRequest,
    failure_class: FailureClass,
) -> Result<MutationEligibilityDecision, JsonFailure> {
    let decision = decide_public_mutation(status, &request);
    if decision.allowed {
        return Ok(decision);
    }
    let public_route_summary = public_mutation_failure_route_summary(status);
    let route_field = public_mutation_failure_route_field(status);
    let route_guidance = public_mutation_failure_route_guidance(status);
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
            "{} failed closed: requested task {task} step {step} is not the exact public route and no explicit repair target is bound. typed_public_route={public_route_summary}; route_field={route_field}; route_guidance={route_guidance}; reason_code={}; phase_detail={}; state_kind={}; runtime_reconcile_required={}; blocked_runtime_bug={}; route_reason_codes=[{reason_codes}]; public_repair_targets=[{public_repair_targets}]; blocking_records=[{blocking_records}]; detail={}",
            request.command_name_for_diagnostics(),
            decision.reason_code,
            status.phase_detail,
            status.state_kind,
            status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED,
            state_kind_is_blocked_runtime_bug(&status.state_kind),
            decision.detail,
        ),
    ))
}

fn public_mutation_failure_route_summary(status: &PlanExecutionStatus) -> String {
    status
        .recommended_public_command
        .as_ref()
        .map(public_command_diagnostic_summary)
        .unwrap_or_else(|| String::from("command_kind=none"))
}

fn public_command_diagnostic_summary(command: &PublicCommand) -> String {
    let command_kind = command.command_kind_name();
    match command {
        PublicCommand::WorkflowOperator {
            external_review_result_ready,
            json,
            ..
        } => format!(
            "command_kind={command_kind},external_review_result_ready={external_review_result_ready},json={json}"
        ),
        PublicCommand::Status { .. } | PublicCommand::RepairReviewState { .. } => {
            format!("command_kind={command_kind}")
        }
        PublicCommand::Begin { task, step, .. }
        | PublicCommand::Complete { task, step, .. }
        | PublicCommand::Reopen { task, step, .. }
        | PublicCommand::TransferRepairStep { task, step, .. } => {
            format!("command_kind={command_kind},task={task},step={step}")
        }
        PublicCommand::TransferHandoff { scope, .. } => {
            format!("command_kind={command_kind},scope={scope}")
        }
        PublicCommand::CloseCurrentTask {
            task,
            result_inputs_required,
            ..
        } => {
            let task = task.map_or_else(|| String::from("none"), |task| task.to_string());
            format!(
                "command_kind={command_kind},task={task},result_inputs_required={result_inputs_required}"
            )
        }
        PublicCommand::AdvanceLateStage { mode, .. } => {
            format!(
                "command_kind={command_kind},mode={}",
                public_advance_late_stage_mode_name(*mode)
            )
        }
        PublicCommand::MaterializeProjectionsStateDirOnly { scope, .. } => {
            let scope = scope.as_deref().unwrap_or("none");
            format!("command_kind={command_kind},scope={scope}")
        }
    }
}

fn public_mutation_failure_route_field(status: &PlanExecutionStatus) -> &'static str {
    if status.recommended_public_command_argv.is_some() {
        "recommended_public_command_argv"
    } else if status.recommended_public_command_template.is_some() {
        "recommended_public_command_template.input_bindings"
    } else {
        "none"
    }
}

fn public_mutation_failure_route_guidance(status: &PlanExecutionStatus) -> &'static str {
    if state_kind_or_phase_is_runtime_diagnostic(&status.state_kind, &status.phase_detail)
        || status.next_action
            == crate::execution::next_action::NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED
    {
        return "stop on runtime diagnostic; do not retry mutation";
    }
    if status.recommended_public_command_argv.is_some() {
        EXECUTE_RECOMMENDED_PUBLIC_ARGV_GUIDANCE
    } else if status.recommended_public_command_template.is_some() {
        WORKFLOW_OPERATOR_TEMPLATE_JSON_QUERY
    } else {
        OPERATOR_ROUTE_AUTHORITY_REFERENCE
    }
}

pub(crate) fn runtime_reconcile_route_is_diagnostic_only(status: &PlanExecutionStatus) -> bool {
    status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        && status.recommended_public_command.is_none()
        && status.recommended_public_command_argv.is_none()
        && status.recommended_public_command_template.is_none()
        && status.next_public_action.is_none()
        && status.public_repair_targets.is_empty()
}

fn status_blocks_non_exact_public_mutation(status: &PlanExecutionStatus) -> bool {
    status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        || state_kind_blocks_local_mutation(&status.state_kind)
}

fn route_exposes_public_mutation_request(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    let Some(route_request) = status
        .recommended_public_command
        .as_ref()
        .and_then(PublicCommand::to_mutation_request)
    else {
        return false;
    };
    if route_request.kind == PublicCommandKind::AdvanceLateStage
        && !public_advance_late_stage_request_mode_matches_routed_public_command(
            &status.phase_detail,
            status.recommended_public_command.as_ref(),
            route_request.advance_late_stage_mode,
        )
    {
        return false;
    }
    public_mutation_requests_match(&route_request, request)
}

fn public_repair_target_matches_request(
    command_kind: &str,
    task: Option<u32>,
    step: Option<u32>,
    request: &PublicMutationRequest,
) -> bool {
    if request.kind == PublicCommandKind::AdvanceLateStage {
        return false;
    }
    request.kind.matches_public_mutation_token(command_kind)
        && task == request.task
        && step == request.step
        && (request.kind != PublicCommandKind::Transfer
            || request.transfer_mode == Some(PublicTransferMode::RepairStep))
}

fn public_mutation_requests_match(
    route_request: &PublicMutationRequest,
    request: &PublicMutationRequest,
) -> bool {
    if route_request.kind != request.kind {
        return false;
    }
    if request.kind == PublicCommandKind::Transfer {
        return route_request.transfer_mode == request.transfer_mode
            && public_transfer_scope_matches(route_request, request)
            && route_request.task == request.task
            && route_request.step == request.step
            && public_mutation_fingerprint_matches(route_request, request);
    }
    if request.kind == PublicCommandKind::AdvanceLateStage
        && route_request.advance_late_stage_mode != request.advance_late_stage_mode
    {
        return false;
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
    shared_required_follow_up_from_routing(operator).as_deref()
        == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
}

pub(crate) fn public_advance_late_stage_operator_routes_branch_closure(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_is_branch_closure(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_operator_routes_release_readiness(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_is_release_readiness(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_operator_routes_final_review_dispatch(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_is_final_review_dispatch(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_operator_routes_qa(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_is_qa(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_operator_routes_final_review(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_is_final_review(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_operator_accepts_final_review_inputs(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_accepts_final_review_inputs(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_operator_routes_finish_review(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_is_finish_review(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_operator_routes_finish_completion(
    operator: &ExecutionRoutingState,
) -> bool {
    routed_public_command_is_finish_completion(
        &operator.phase_detail,
        operator.recommended_public_command.as_ref(),
    )
}

pub(crate) fn public_advance_late_stage_status_mutation_allowed(
    status: &PlanExecutionStatus,
    mode: PublicAdvanceLateStageMode,
) -> bool {
    decide_public_mutation(status, &PublicMutationRequest::advance_late_stage(mode)).allowed
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

pub(crate) fn branch_closure_required_follow_up(
    operator: &ExecutionRoutingState,
) -> Option<String> {
    blocked_follow_up_for_operator(operator).and_then(|required_follow_up| {
        (normalize_follow_up_alias(Some(&required_follow_up), FollowUpAliasContext::PublicRouting)
            != Some(FollowUpKind::AdvanceLateStage)
            || operator.phase_detail
                == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS)
            .then_some(required_follow_up)
    })
}

pub(crate) fn late_stage_required_follow_up(
    stage_path: &str,
    operator: &ExecutionRoutingState,
) -> Option<String> {
    let required_follow_up = blocked_follow_up_for_operator(operator)?;
    let required_follow_up_kind = normalize_follow_up_alias(
        Some(&required_follow_up),
        FollowUpAliasContext::PublicRouting,
    )?;
    if stage_path == "release_readiness"
        && !matches!(
            required_follow_up_kind,
            FollowUpKind::AdvanceLateStage | FollowUpKind::RepairReviewState
        )
    {
        return None;
    }
    if stage_path == "final_review"
        && !matches!(
            required_follow_up_kind,
            FollowUpKind::RequestExternalReview | FollowUpKind::RepairReviewState
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
        match normalize_follow_up_alias(Some(&required_follow_up), FollowUpAliasContext::PublicRouting)?
        {
            FollowUpKind::AdvanceLateStage
                if matches!(
                    operator.phase_detail.as_str(),
                    crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
                        |
                    crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY
                        | crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
                ) =>
        {
            Some(required_follow_up)
        }
            FollowUpKind::RepairReviewState => Some(required_follow_up),
        _ => None,
        }
    })
}

pub(crate) fn negative_result_follow_up(operator: &ExecutionRoutingState) -> Option<String> {
    blocked_follow_up_for_operator(operator)
}

#[cfg(test)]
#[path = "command_eligibility_hidden_flag_tests.rs"]
mod hidden_flag_tests;

#[cfg(test)]
#[path = "command_eligibility/unit_tests.rs"]
mod tests;
