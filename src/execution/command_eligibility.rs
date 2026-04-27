use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::query::{
    ExecutionRoutingState,
    normalize_public_follow_up_alias as shared_normalize_public_follow_up_alias,
    required_follow_up_from_routing as shared_required_follow_up_from_routing,
};
use crate::execution::state::PlanExecutionStatus;

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
    pub transfer_mode: Option<PublicTransferMode>,
    pub transfer_scope: Option<String>,
    pub command_name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicTransferMode {
    RepairStep,
    WorkflowHandoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublicCommandShape {
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
    },
    AdvanceLateStage {
        plan: String,
    },
    DiagnosticMaterializeProjections {
        plan: String,
        repo_export: bool,
    },
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

impl PublicCommandShape {
    pub(crate) fn parse(command: &str) -> Option<Self> {
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
                ParsedFlags::parse(rest)?;
                Some(Self::AdvanceLateStage {
                    plan: (*plan).to_owned(),
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
                Some(Self::DiagnosticMaterializeProjections {
                    plan: (*plan).to_owned(),
                    repo_export: flags.repo_export,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn to_display_command(&self) -> String {
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
                "featureforge plan execution complete --plan {plan} --task {task} --step {step}{}{}",
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
            } => format!(
                "featureforge plan execution reopen --plan {plan} --task {task} --step {step}{}{}{}",
                optional_flag(" --source ", source.as_deref()),
                optional_flag(" --reason ", reason.as_deref()),
                optional_flag(" --expect-execution-fingerprint ", fingerprint.as_deref())
            ),
            Self::TransferRepairStep {
                plan,
                task,
                step,
                fingerprint,
            } => format!(
                "featureforge plan execution transfer --plan {plan} --repair-task {task} --repair-step {step}{}",
                optional_flag(" --expect-execution-fingerprint ", fingerprint.as_deref())
            ),
            Self::TransferHandoff { plan, scope } => {
                format!(
                    "featureforge plan execution transfer --plan {plan} --scope {scope} --to <owner> --reason <reason>"
                )
            }
            Self::CloseCurrentTask { plan, task } => format!(
                "featureforge plan execution close-current-task --plan {plan}{}",
                optional_flag(" --task ", task.map(|task| task.to_string()).as_deref())
            ),
            Self::AdvanceLateStage { plan } => {
                format!("featureforge plan execution advance-late-stage --plan {plan}")
            }
            Self::DiagnosticMaterializeProjections { plan, repo_export } => {
                let suffix = if *repo_export {
                    " --repo-export --confirm-repo-export"
                } else {
                    ""
                };
                format!("featureforge plan execution materialize-projections --plan {plan}{suffix}")
            }
        }
    }

    pub(crate) fn to_mutation_request(&self) -> Option<PublicMutationRequest> {
        match self {
            Self::RepairReviewState { .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::RepairReviewState,
                task: None,
                step: None,
                transfer_mode: None,
                transfer_scope: None,
                command_name: "repair-review-state",
            }),
            Self::Begin { task, step, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Begin,
                task: Some(*task),
                step: Some(*step),
                transfer_mode: None,
                transfer_scope: None,
                command_name: "begin",
            }),
            Self::Complete { task, step, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Complete,
                task: Some(*task),
                step: Some(*step),
                transfer_mode: None,
                transfer_scope: None,
                command_name: "complete",
            }),
            Self::Reopen { task, step, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Reopen,
                task: Some(*task),
                step: Some(*step),
                transfer_mode: None,
                transfer_scope: None,
                command_name: "reopen",
            }),
            Self::TransferRepairStep { task, step, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Transfer,
                task: Some(*task),
                step: Some(*step),
                transfer_mode: Some(PublicTransferMode::RepairStep),
                transfer_scope: None,
                command_name: "transfer",
            }),
            Self::TransferHandoff { scope, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::Transfer,
                task: None,
                step: None,
                transfer_mode: Some(PublicTransferMode::WorkflowHandoff),
                transfer_scope: Some(scope.clone()),
                command_name: "transfer",
            }),
            Self::CloseCurrentTask { task, .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::CloseCurrentTask,
                task: *task,
                step: None,
                transfer_mode: None,
                transfer_scope: None,
                command_name: "close-current-task",
            }),
            Self::AdvanceLateStage { .. } => Some(PublicMutationRequest {
                kind: PublicMutationKind::AdvanceLateStage,
                task: None,
                step: None,
                transfer_mode: None,
                transfer_scope: None,
                command_name: "advance-late-stage",
            }),
            Self::WorkflowOperator { .. }
            | Self::Status { .. }
            | Self::DiagnosticMaterializeProjections { .. } => None,
        }
    }
}

#[derive(Default)]
struct ParsedFlags {
    execution_mode: Option<String>,
    expect_execution_fingerprint: Option<String>,
    source: Option<String>,
    reason: Option<String>,
    repo_export: bool,
}

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
                "--reason" => {
                    parsed.reason = Some((*tokens.get(index + 1)?).to_owned());
                    index += 2;
                }
                "--to"
                | "--result"
                | "--summary-file"
                | "--review-summary-file"
                | "--verification-summary-file"
                | "--reviewer-id"
                | "--reviewer-source"
                | "--review-result"
                | "--verification-result"
                | "--dispatch-id"
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

fn parse_close_current_task_flags(tokens: &[&str]) -> Option<()> {
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            "--dispatch-id"
            | "--review-result"
            | "--review-summary-file"
            | "--verification-result"
            | "--verification-summary-file" => {
                let _ = tokens.get(index + 1)?;
                index += 2;
            }
            "[--verification-summary-file" => {
                if tokens.get(index + 1..index + 5)? != ["<path>", "when", "verification", "ran]"] {
                    return None;
                }
                index += 5;
            }
            _ => return None,
        }
    }
    Some(())
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

const HIDDEN_COMMAND_TOKENS: &[&str] = &[
    "record-pivot",
    "record-review-dispatch",
    "gate-review",
    "gate-finish",
    "rebuild-evidence",
    "plan execution internal",
    "reconcile-review-state",
    "plan execution preflight",
    "plan execution recommend",
    "workflow recommend",
    "workflow preflight",
];

pub(crate) fn command_invokes_hidden_lane(command: &str) -> bool {
    HIDDEN_COMMAND_TOKENS
        .iter()
        .any(|token| command.contains(token))
}

pub(crate) fn command_is_legal_public_command(command: &str) -> bool {
    PublicCommandShape::parse(command).is_some_and(|shape| !shape.to_display_command().is_empty())
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

    if status.state_kind == "blocked_runtime_bug" {
        return MutationEligibilityDecision::reject(
            "mutation_blocked_runtime_bug",
            format!(
                "{} cannot mutate while public runtime status is blocked_runtime_bug.",
                request.command_name
            ),
        );
    }

    if status.phase_detail == "runtime_reconcile_required"
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
        && status.phase_detail == "runtime_reconcile_required"
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

    if status.public_repair_targets.iter().any(|target| {
        public_repair_target_matches_request(
            target.command_kind.as_str(),
            target.task,
            target.step,
            request,
        )
    }) {
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
            transfer_mode: None,
            transfer_scope: None,
            command_name,
        },
    )
    .allowed
}

pub(crate) fn public_mutation_request_from_command(command: &str) -> Option<PublicMutationRequest> {
    if command_invokes_hidden_lane(command) {
        return None;
    }
    PublicCommandShape::parse(command).and_then(|shape| shape.to_mutation_request())
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
        .recommended_command
        .as_deref()
        .or_else(|| {
            status
                .next_public_action
                .as_ref()
                .map(|action| action.command.as_str())
        })
        .filter(|command| {
            !command_invokes_hidden_lane(command) && command_is_legal_public_command(command)
        })
        .unwrap_or("none");
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
    Err(JsonFailure::new(
        failure_class,
        format!(
            "{} failed closed: requested task {task} step {step} is not the exact public route and no explicit repair target is bound. Next public action: {next_public_command}. reason_code={}; phase_detail={}; state_kind={}; runtime_reconcile_required={}; blocked_runtime_bug={}; route_reason_codes=[{reason_codes}]; detail={}",
            request.command_name,
            decision.reason_code,
            status.phase_detail,
            status.state_kind,
            status.phase_detail == "runtime_reconcile_required",
            status.state_kind == "blocked_runtime_bug",
            decision.detail,
        ),
    ))
}

fn status_blocks_non_exact_public_mutation(status: &PlanExecutionStatus) -> bool {
    status.phase_detail == "runtime_reconcile_required"
        || matches!(
            status.state_kind.as_str(),
            "blocked_runtime_bug" | "terminal" | "waiting_external_input"
        )
}

fn request_matches_resume_begin(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    request.kind == PublicMutationKind::Begin
        && status.phase_detail == "execution_in_progress"
        && status.execution_started == "yes"
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task == request.task
        && status.resume_step == request.step
}

fn route_exposes_public_mutation_request(
    status: &PlanExecutionStatus,
    request: &PublicMutationRequest,
) -> bool {
    status
        .recommended_command
        .as_deref()
        .into_iter()
        .chain(
            status
                .next_public_action
                .as_ref()
                .map(|action| action.command.as_str()),
        )
        .any(|command| public_route_command_matches_request(command, request))
}

fn public_route_command_matches_request(command: &str, request: &PublicMutationRequest) -> bool {
    let Some(route_request) = public_mutation_request_from_command(command) else {
        return false;
    };
    public_mutation_requests_match(&route_request, request)
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
            && route_request.transfer_scope == request.transfer_scope
            && route_request.task == request.task
            && route_request.step == request.step;
    }
    route_request.task == request.task && route_request.step == request.step
}

fn parse_u32_token(token: &str) -> Option<u32> {
    token.parse::<u32>().ok()
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
        let shapes = [
            PublicCommandShape::WorkflowOperator {
                plan: String::from("docs/plan.md"),
                external_review_result_ready: false,
            },
            PublicCommandShape::Status {
                plan: String::from("docs/plan.md"),
            },
            PublicCommandShape::RepairReviewState {
                plan: String::from("docs/plan.md"),
            },
            PublicCommandShape::Begin {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                execution_mode: Some(String::from("featureforge:executing-plans")),
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommandShape::Complete {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                source: Some(String::from("featureforge:executing-plans")),
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommandShape::Reopen {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                source: Some(String::from("featureforge:executing-plans")),
                reason: Some(String::from("repair")),
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommandShape::TransferRepairStep {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                fingerprint: Some(String::from("fingerprint")),
            },
            PublicCommandShape::TransferHandoff {
                plan: String::from("docs/plan.md"),
                scope: String::from("task"),
            },
            PublicCommandShape::CloseCurrentTask {
                plan: String::from("docs/plan.md"),
                task: Some(1),
            },
            PublicCommandShape::AdvanceLateStage {
                plan: String::from("docs/plan.md"),
            },
            PublicCommandShape::DiagnosticMaterializeProjections {
                plan: String::from("docs/plan.md"),
                repo_export: true,
            },
        ];

        for shape in shapes {
            let display = shape.to_display_command();
            let parsed = PublicCommandShape::parse(&display)
                .unwrap_or_else(|| panic!("shape should parse from `{display}`"));
            assert_eq!(parsed, shape, "round trip failed for `{display}`");
            assert!(command_is_legal_public_command(&display));
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
    fn close_current_task_public_template_accepts_documented_optional_summary_hint() {
        let command = "featureforge plan execution close-current-task --plan docs/plan.md --task 1 --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]";

        assert!(command_is_legal_public_command(command));
        assert_eq!(
            public_mutation_request_from_command(command)
                .expect("template should map to public close-current-task mutation")
                .kind,
            PublicMutationKind::CloseCurrentTask
        );
    }
}
