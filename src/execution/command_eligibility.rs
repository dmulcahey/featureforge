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

    fn public_command_prefix(&self) -> &'static str {
        match self {
            Self::Begin => "featureforge plan execution begin --plan ",
            Self::Complete => "featureforge plan execution complete --plan ",
            Self::Reopen => "featureforge plan execution reopen --plan ",
            Self::Transfer => "featureforge plan execution transfer --plan ",
            Self::CloseCurrentTask => "featureforge plan execution close-current-task --plan ",
            Self::RepairReviewState => "featureforge plan execution repair-review-state --plan ",
            Self::AdvanceLateStage => "featureforge plan execution advance-late-stage --plan ",
        }
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

const LEGAL_PUBLIC_COMMAND_PREFIXES: &[&str] = &[
    "featureforge workflow operator --plan ",
    "featureforge plan execution status --plan ",
    "featureforge plan execution repair-review-state --plan ",
    "featureforge plan execution begin --plan ",
    "featureforge plan execution complete --plan ",
    "featureforge plan execution reopen --plan ",
    "featureforge plan execution transfer --plan ",
    "featureforge plan execution close-current-task --plan ",
    "featureforge plan execution advance-late-stage --plan ",
    "featureforge plan execution materialize-projections --plan ",
];

pub(crate) fn command_invokes_hidden_lane(command: &str) -> bool {
    HIDDEN_COMMAND_TOKENS
        .iter()
        .any(|token| command.contains(token))
}

pub(crate) fn command_is_legal_public_command(command: &str) -> bool {
    LEGAL_PUBLIC_COMMAND_PREFIXES
        .iter()
        .any(|prefix| command.starts_with(prefix))
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
    if command_invokes_hidden_lane(command) || !command_is_legal_public_command(command) {
        return None;
    }
    let (kind, command_name) = if command
        .starts_with(PublicMutationKind::Begin.public_command_prefix())
    {
        (PublicMutationKind::Begin, "begin")
    } else if command.starts_with(PublicMutationKind::Complete.public_command_prefix()) {
        (PublicMutationKind::Complete, "complete")
    } else if command.starts_with(PublicMutationKind::Reopen.public_command_prefix()) {
        (PublicMutationKind::Reopen, "reopen")
    } else if command.starts_with(PublicMutationKind::Transfer.public_command_prefix()) {
        (PublicMutationKind::Transfer, "transfer")
    } else if command.starts_with(PublicMutationKind::CloseCurrentTask.public_command_prefix()) {
        (PublicMutationKind::CloseCurrentTask, "close-current-task")
    } else if command.starts_with(PublicMutationKind::RepairReviewState.public_command_prefix()) {
        (PublicMutationKind::RepairReviewState, "repair-review-state")
    } else if command.starts_with(PublicMutationKind::AdvanceLateStage.public_command_prefix()) {
        (PublicMutationKind::AdvanceLateStage, "advance-late-stage")
    } else {
        return None;
    };
    Some(PublicMutationRequest {
        kind,
        task: mutation_task_from_command(command, &kind),
        step: mutation_step_from_command(command, &kind),
        transfer_mode: transfer_mode_from_command(command, &kind),
        transfer_scope: transfer_scope_from_command(command, &kind),
        command_name,
    })
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

fn mutation_task_from_command(command: &str, kind: &PublicMutationKind) -> Option<u32> {
    match kind {
        PublicMutationKind::Transfer => parse_u32_flag(command, "--repair-task"),
        _ => parse_u32_flag(command, "--task"),
    }
}

fn mutation_step_from_command(command: &str, kind: &PublicMutationKind) -> Option<u32> {
    match kind {
        PublicMutationKind::Transfer => parse_u32_flag(command, "--repair-step"),
        _ => parse_u32_flag(command, "--step"),
    }
}

fn transfer_mode_from_command(
    command: &str,
    kind: &PublicMutationKind,
) -> Option<PublicTransferMode> {
    if *kind != PublicMutationKind::Transfer {
        return None;
    }
    if parse_u32_flag(command, "--repair-task").is_some()
        || parse_u32_flag(command, "--repair-step").is_some()
    {
        return Some(PublicTransferMode::RepairStep);
    }
    parse_string_flag(command, "--scope")
        .is_some()
        .then_some(PublicTransferMode::WorkflowHandoff)
}

fn transfer_scope_from_command(command: &str, kind: &PublicMutationKind) -> Option<String> {
    (*kind == PublicMutationKind::Transfer)
        .then(|| parse_string_flag(command, "--scope"))
        .flatten()
}

fn parse_u32_flag(command: &str, flag: &str) -> Option<u32> {
    let mut parts = command.split_whitespace();
    while let Some(part) = parts.next() {
        if part != flag {
            continue;
        }
        return parts.next().and_then(|raw| raw.parse::<u32>().ok());
    }
    None
}

fn parse_string_flag(command: &str, flag: &str) -> Option<String> {
    let mut parts = command.split_whitespace();
    while let Some(part) = parts.next() {
        if part != flag {
            continue;
        }
        return parts.next().map(str::to_owned);
    }
    None
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
