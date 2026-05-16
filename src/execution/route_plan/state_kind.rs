use crate::execution::command_eligibility::PublicCommand;
use crate::execution::harness::HarnessPhase;
use crate::execution::phase;
use crate::execution::query::ExecutionRoutingState;
use crate::execution::review_route_tokens::EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT;

pub(crate) const STATE_KIND_ACTIONABLE_PUBLIC_COMMAND: &str = "actionable_public_command";
pub(crate) const STATE_KIND_WAITING_EXTERNAL_INPUT: &str = "waiting_external_input";
pub(crate) const STATE_KIND_TERMINAL: &str = "terminal";
pub(crate) const STATE_KIND_BLOCKED_RUNTIME_BUG: &str = phase::DETAIL_BLOCKED_RUNTIME_BUG;
pub(crate) const STATE_KIND_PLANNING_REENTRY_REQUIRED: &str =
    phase::DETAIL_PLANNING_REENTRY_REQUIRED;
pub(crate) const STATE_KIND_RUNTIME_RECONCILE_REQUIRED: &str =
    phase::DETAIL_RUNTIME_RECONCILE_REQUIRED;

pub const PUBLIC_STATE_KIND_VALUES: &[&str] = &[
    STATE_KIND_ACTIONABLE_PUBLIC_COMMAND,
    STATE_KIND_WAITING_EXTERNAL_INPUT,
    STATE_KIND_TERMINAL,
    STATE_KIND_BLOCKED_RUNTIME_BUG,
    STATE_KIND_PLANNING_REENTRY_REQUIRED,
    STATE_KIND_RUNTIME_RECONCILE_REQUIRED,
];

pub(super) fn derive_state_kind(routing: &ExecutionRoutingState) -> String {
    let recommended_command =
        state_kind_command_marker(routing.recommended_public_command.as_ref());
    classify_state_kind(
        routing.external_wait_state.as_deref(),
        routing.phase == phase::PHASE_READY_FOR_BRANCH_COMPLETION,
        &routing.phase_detail,
        recommended_command,
    )
}

pub(crate) fn state_kind_command_marker(command: Option<&PublicCommand>) -> Option<&'static str> {
    command.map(|_| "public_command")
}

pub(crate) fn derive_state_kind_from_seed(
    external_wait_state: Option<&str>,
    harness_phase: HarnessPhase,
    phase_detail: &str,
    recommended_command: Option<&str>,
) -> String {
    classify_state_kind(
        external_wait_state,
        harness_phase == HarnessPhase::ReadyForBranchCompletion,
        phase_detail,
        recommended_command,
    )
}

pub(crate) fn classify_state_kind(
    external_wait_state: Option<&str>,
    terminal_phase: bool,
    phase_detail: &str,
    recommended_command: Option<&str>,
) -> String {
    if external_wait_state_is_external_wait(external_wait_state) {
        return String::from(STATE_KIND_WAITING_EXTERNAL_INPUT);
    }
    if terminal_phase
        && phase_detail == phase::DETAIL_FINISH_COMPLETION_GATE_READY
        && recommended_command.is_none()
    {
        return String::from(STATE_KIND_TERMINAL);
    }
    if phase_detail == phase::DETAIL_BLOCKED_RUNTIME_BUG && recommended_command.is_none() {
        return String::from(STATE_KIND_BLOCKED_RUNTIME_BUG);
    }
    if phase_detail == phase::DETAIL_PLANNING_REENTRY_REQUIRED && recommended_command.is_none() {
        return String::from(STATE_KIND_PLANNING_REENTRY_REQUIRED);
    }
    if phase_detail == phase::DETAIL_RUNTIME_RECONCILE_REQUIRED && recommended_command.is_none() {
        return String::from(STATE_KIND_RUNTIME_RECONCILE_REQUIRED);
    }
    if recommended_command.is_none()
        && !phase::RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail)
    {
        return String::from(STATE_KIND_BLOCKED_RUNTIME_BUG);
    }
    String::from(STATE_KIND_ACTIONABLE_PUBLIC_COMMAND)
}

pub(crate) fn external_wait_state_is_external_wait(external_wait_state: Option<&str>) -> bool {
    external_wait_state
        .map(str::trim)
        .is_some_and(|state| state == EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT)
}

pub(crate) fn state_kind_is_actionable_public_command(state_kind: &str) -> bool {
    normalize_state_kind(state_kind) == STATE_KIND_ACTIONABLE_PUBLIC_COMMAND
}

pub(crate) fn state_kind_is_external_wait(state_kind: &str) -> bool {
    normalize_state_kind(state_kind) == STATE_KIND_WAITING_EXTERNAL_INPUT
}

pub(crate) fn state_kind_is_terminal(state_kind: &str) -> bool {
    normalize_state_kind(state_kind) == STATE_KIND_TERMINAL
}

pub(crate) fn state_kind_is_blocked_runtime_bug(state_kind: &str) -> bool {
    normalize_state_kind(state_kind) == STATE_KIND_BLOCKED_RUNTIME_BUG
}

pub(crate) fn state_kind_is_planning_reentry_required(state_kind: &str) -> bool {
    normalize_state_kind(state_kind) == STATE_KIND_PLANNING_REENTRY_REQUIRED
}

pub(crate) fn state_kind_is_runtime_diagnostic(state_kind: &str) -> bool {
    let state_kind = normalize_state_kind(state_kind);
    !state_kind.is_empty()
        && !state_kind_is_actionable_public_command(state_kind)
        && !state_kind_is_external_wait(state_kind)
        && !state_kind_is_terminal(state_kind)
        && !state_kind_is_planning_reentry_required(state_kind)
}

pub(crate) fn state_kind_is_runtime_reconcile_required(state_kind: &str) -> bool {
    normalize_state_kind(state_kind) == STATE_KIND_RUNTIME_RECONCILE_REQUIRED
}

pub(crate) fn state_kind_or_phase_is_runtime_diagnostic(
    state_kind: &str,
    phase_detail: &str,
) -> bool {
    state_kind_is_runtime_diagnostic(state_kind)
        || matches!(
            phase_detail,
            phase::DETAIL_BLOCKED_RUNTIME_BUG | phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        )
}

pub(crate) fn state_kind_blocks_local_mutation(state_kind: &str) -> bool {
    state_kind_is_external_wait(state_kind)
        || state_kind_is_terminal(state_kind)
        || state_kind_is_planning_reentry_required(state_kind)
        || state_kind_is_runtime_diagnostic(state_kind)
}

fn normalize_state_kind(state_kind: &str) -> &str {
    state_kind.trim()
}
