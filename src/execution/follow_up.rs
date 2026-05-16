use serde::{Deserialize, Serialize};

use crate::execution::gate_reason_codes::finish_review_gate_already_current_reason_code;
use crate::execution::review_route_tokens::{
    FOLLOW_UP_ADVANCE_LATE_STAGE, FOLLOW_UP_CLOSE_CURRENT_TASK, FOLLOW_UP_EXECUTION_REENTRY,
    FOLLOW_UP_GATE_FINISH, FOLLOW_UP_GATE_REVIEW, FOLLOW_UP_RECORD_HANDOFF,
    FOLLOW_UP_REPAIR_REVIEW_STATE, FOLLOW_UP_REQUEST_EXTERNAL_REVIEW,
    FOLLOW_UP_RESOLVE_RELEASE_BLOCKER, FOLLOW_UP_RUN_VERIFICATION,
    FOLLOW_UP_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
};
use crate::git::sha256_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FollowUpKind {
    RepairReviewState,
    AdvanceLateStage,
    ExecutionReentry,
    CloseCurrentTask,
    RequestExternalReview,
    WaitForExternalReviewResult,
    RunVerification,
    ResolveReleaseBlocker,
    RecordHandoff,
    GateReview,
    GateFinish,
}

const PUBLIC_FOLLOW_UP_KINDS: &[FollowUpKind] = &[
    FollowUpKind::RepairReviewState,
    FollowUpKind::AdvanceLateStage,
    FollowUpKind::ExecutionReentry,
    FollowUpKind::CloseCurrentTask,
    FollowUpKind::RequestExternalReview,
    FollowUpKind::WaitForExternalReviewResult,
    FollowUpKind::RunVerification,
    FollowUpKind::ResolveReleaseBlocker,
    FollowUpKind::RecordHandoff,
    FollowUpKind::GateReview,
    FollowUpKind::GateFinish,
];

pub fn public_follow_up_tokens() -> impl Iterator<Item = &'static str> {
    PUBLIC_FOLLOW_UP_KINDS
        .iter()
        .copied()
        .map(FollowUpKind::public_token)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FollowUpAliasContext {
    PublicRouting,
    PersistedRepairState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepairFollowUpRecord {
    pub(crate) kind: RepairFollowUpKind,
    pub(crate) target_scope: RepairTargetScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_task: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_step: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) semantic_workspace_state_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_route_decision_hash: Option<String>,
    pub(crate) created_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) created_at: Option<String>,
    pub(crate) expires_on_plan_fingerprint_change: bool,
}

pub(crate) fn repair_follow_up_source_decision_hash(decision: &impl Serialize) -> Option<String> {
    serde_json::to_vec(decision)
        .ok()
        .map(|serialized| sha256_hex(&serialized))
}

pub fn execution_step_repair_target_id(task: u32, step: u32) -> String {
    format!("execution-step-{task}-{step}")
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepairFollowUpKind {
    RecordBranchClosure,
    AdvanceLateStage,
    RecordFinalReview,
    RecordQa,
    CloseTask,
    RepairReviewState,
    ExecutionReentry,
    RequestExternalReview,
    WaitForExternalReviewResult,
    RunVerification,
    ResolveReleaseBlocker,
    RecordHandoff,
    GateReview,
    GateFinish,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepairTargetScope {
    TaskClosure,
    BranchClosure,
    ReleaseReadiness,
    FinalReview,
    Qa,
    ExecutionStep,
}

struct FollowUpAliasRule {
    token: &'static str,
    public_kind: Option<FollowUpKind>,
    persisted_repair_kind: Option<FollowUpKind>,
}

const FOLLOW_UP_ALIAS_RULES: &[FollowUpAliasRule] = &[
    FollowUpAliasRule {
        token: "record_branch_closure",
        public_kind: Some(FollowUpKind::AdvanceLateStage),
        persisted_repair_kind: Some(FollowUpKind::AdvanceLateStage),
    },
    FollowUpAliasRule {
        token: "branch_closure",
        public_kind: Some(FollowUpKind::AdvanceLateStage),
        persisted_repair_kind: Some(FollowUpKind::AdvanceLateStage),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_ADVANCE_LATE_STAGE,
        public_kind: Some(FollowUpKind::AdvanceLateStage),
        persisted_repair_kind: Some(FollowUpKind::AdvanceLateStage),
    },
    FollowUpAliasRule {
        token: "record_task_closure",
        public_kind: Some(FollowUpKind::ExecutionReentry),
        persisted_repair_kind: Some(FollowUpKind::CloseCurrentTask),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_CLOSE_CURRENT_TASK,
        public_kind: Some(FollowUpKind::CloseCurrentTask),
        persisted_repair_kind: Some(FollowUpKind::CloseCurrentTask),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_EXECUTION_REENTRY,
        public_kind: Some(FollowUpKind::ExecutionReentry),
        persisted_repair_kind: Some(FollowUpKind::ExecutionReentry),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_REQUEST_EXTERNAL_REVIEW,
        public_kind: Some(FollowUpKind::RequestExternalReview),
        persisted_repair_kind: Some(FollowUpKind::RequestExternalReview),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
        public_kind: Some(FollowUpKind::WaitForExternalReviewResult),
        persisted_repair_kind: Some(FollowUpKind::WaitForExternalReviewResult),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_RUN_VERIFICATION,
        public_kind: Some(FollowUpKind::RunVerification),
        persisted_repair_kind: Some(FollowUpKind::RunVerification),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_RESOLVE_RELEASE_BLOCKER,
        public_kind: Some(FollowUpKind::ResolveReleaseBlocker),
        persisted_repair_kind: Some(FollowUpKind::ResolveReleaseBlocker),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_RECORD_HANDOFF,
        public_kind: Some(FollowUpKind::RecordHandoff),
        persisted_repair_kind: Some(FollowUpKind::RecordHandoff),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_REPAIR_REVIEW_STATE,
        public_kind: Some(FollowUpKind::RepairReviewState),
        persisted_repair_kind: Some(FollowUpKind::RepairReviewState),
    },
    FollowUpAliasRule {
        token: "record_pivot",
        public_kind: Some(FollowUpKind::RepairReviewState),
        persisted_repair_kind: Some(FollowUpKind::RepairReviewState),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_GATE_REVIEW,
        public_kind: Some(FollowUpKind::GateReview),
        persisted_repair_kind: Some(FollowUpKind::GateReview),
    },
    FollowUpAliasRule {
        token: FOLLOW_UP_GATE_FINISH,
        public_kind: Some(FollowUpKind::GateFinish),
        persisted_repair_kind: Some(FollowUpKind::GateFinish),
    },
];

impl FollowUpKind {
    pub(crate) fn public_token(self) -> &'static str {
        match self {
            Self::RepairReviewState => FOLLOW_UP_REPAIR_REVIEW_STATE,
            Self::AdvanceLateStage => FOLLOW_UP_ADVANCE_LATE_STAGE,
            Self::ExecutionReentry => FOLLOW_UP_EXECUTION_REENTRY,
            Self::CloseCurrentTask => FOLLOW_UP_CLOSE_CURRENT_TASK,
            Self::RequestExternalReview => FOLLOW_UP_REQUEST_EXTERNAL_REVIEW,
            Self::WaitForExternalReviewResult => FOLLOW_UP_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
            Self::RunVerification => FOLLOW_UP_RUN_VERIFICATION,
            Self::ResolveReleaseBlocker => FOLLOW_UP_RESOLVE_RELEASE_BLOCKER,
            Self::RecordHandoff => FOLLOW_UP_RECORD_HANDOFF,
            Self::GateReview => FOLLOW_UP_GATE_REVIEW,
            Self::GateFinish => FOLLOW_UP_GATE_FINISH,
        }
    }

    #[cfg(test)]
    pub(crate) fn command_template(self) -> Option<&'static str> {
        match self {
            Self::RepairReviewState => {
                Some("featureforge plan execution repair-review-state --plan <approved-plan-path>")
            }
            Self::AdvanceLateStage | Self::ResolveReleaseBlocker => {
                Some("featureforge plan execution advance-late-stage --plan <approved-plan-path>")
            }
            Self::RecordHandoff => {
                Some("featureforge workflow operator --plan <approved-plan-path>")
            }
            Self::ExecutionReentry
            | Self::RequestExternalReview
            | Self::WaitForExternalReviewResult
            | Self::RunVerification => {
                Some("featureforge workflow operator --plan <approved-plan-path>")
            }
            Self::CloseCurrentTask | Self::GateReview | Self::GateFinish => None,
        }
    }
}

impl RepairFollowUpKind {
    pub(crate) fn from_persisted_token(token: &str) -> Option<Self> {
        match token.trim() {
            "record_branch_closure" | "branch_closure" => Some(Self::RecordBranchClosure),
            FOLLOW_UP_ADVANCE_LATE_STAGE => Some(Self::AdvanceLateStage),
            "record_task_closure" | FOLLOW_UP_CLOSE_CURRENT_TASK => Some(Self::CloseTask),
            FOLLOW_UP_EXECUTION_REENTRY => Some(Self::ExecutionReentry),
            FOLLOW_UP_REPAIR_REVIEW_STATE | "record_pivot" => Some(Self::RepairReviewState),
            FOLLOW_UP_REQUEST_EXTERNAL_REVIEW => Some(Self::RequestExternalReview),
            FOLLOW_UP_WAIT_FOR_EXTERNAL_REVIEW_RESULT => Some(Self::WaitForExternalReviewResult),
            FOLLOW_UP_RUN_VERIFICATION => Some(Self::RunVerification),
            FOLLOW_UP_RESOLVE_RELEASE_BLOCKER => Some(Self::ResolveReleaseBlocker),
            FOLLOW_UP_RECORD_HANDOFF => Some(Self::RecordHandoff),
            FOLLOW_UP_GATE_REVIEW => Some(Self::GateReview),
            FOLLOW_UP_GATE_FINISH => Some(Self::GateFinish),
            _ => None,
        }
    }

    pub(crate) fn persisted_token(self) -> &'static str {
        match self {
            Self::RecordBranchClosure => "record_branch_closure",
            Self::AdvanceLateStage => FOLLOW_UP_ADVANCE_LATE_STAGE,
            Self::RecordFinalReview => "record_final_review",
            Self::RecordQa => "record_qa",
            Self::CloseTask => "record_task_closure",
            Self::RepairReviewState => FOLLOW_UP_REPAIR_REVIEW_STATE,
            Self::ExecutionReentry => FOLLOW_UP_EXECUTION_REENTRY,
            Self::RequestExternalReview => FOLLOW_UP_REQUEST_EXTERNAL_REVIEW,
            Self::WaitForExternalReviewResult => FOLLOW_UP_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
            Self::RunVerification => FOLLOW_UP_RUN_VERIFICATION,
            Self::ResolveReleaseBlocker => FOLLOW_UP_RESOLVE_RELEASE_BLOCKER,
            Self::RecordHandoff => FOLLOW_UP_RECORD_HANDOFF,
            Self::GateReview => FOLLOW_UP_GATE_REVIEW,
            Self::GateFinish => FOLLOW_UP_GATE_FINISH,
        }
    }

    pub(crate) fn public_token(self) -> &'static str {
        match self {
            Self::RecordBranchClosure | Self::AdvanceLateStage => FOLLOW_UP_ADVANCE_LATE_STAGE,
            Self::CloseTask => FOLLOW_UP_CLOSE_CURRENT_TASK,
            other => other.persisted_token(),
        }
    }

    pub(crate) fn target_scope(self) -> RepairTargetScope {
        match self {
            Self::RecordBranchClosure | Self::AdvanceLateStage | Self::ResolveReleaseBlocker => {
                RepairTargetScope::BranchClosure
            }
            Self::RecordFinalReview
            | Self::RequestExternalReview
            | Self::WaitForExternalReviewResult
            | Self::GateReview
            | Self::GateFinish => RepairTargetScope::FinalReview,
            Self::RecordQa | Self::RunVerification => RepairTargetScope::Qa,
            Self::CloseTask => RepairTargetScope::TaskClosure,
            Self::RepairReviewState | Self::ExecutionReentry | Self::RecordHandoff => {
                RepairTargetScope::ExecutionStep
            }
        }
    }
}

pub(crate) fn normalize_follow_up_alias(
    follow_up: Option<&str>,
    context: FollowUpAliasContext,
) -> Option<FollowUpKind> {
    let follow_up = follow_up.map(str::trim).filter(|value| !value.is_empty())?;
    match context {
        FollowUpAliasContext::PublicRouting => public_routing_alias(follow_up),
        FollowUpAliasContext::PersistedRepairState => persisted_repair_state_alias(follow_up),
    }
}

pub(crate) fn normalize_public_routing_follow_up_token(
    follow_up: Option<&str>,
) -> Option<&'static str> {
    normalize_follow_up_alias(follow_up, FollowUpAliasContext::PublicRouting)
        .map(FollowUpKind::public_token)
}

pub(crate) fn normalize_persisted_repair_follow_up_token(
    follow_up: Option<&str>,
) -> Option<&'static str> {
    normalize_follow_up_alias(follow_up, FollowUpAliasContext::PersistedRepairState)
        .map(FollowUpKind::public_token)
}

pub(crate) fn follow_up_from_phase_detail<'a>(
    phase_detail: &str,
    blocking_reason_codes: impl IntoIterator<Item = &'a str>,
) -> Option<FollowUpKind> {
    if phase_detail
        == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
    {
        return Some(FollowUpKind::AdvanceLateStage);
    }
    match phase_detail {
        crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => {
            Some(FollowUpKind::RequestExternalReview)
        }
        crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING => {
            if task_review_result_requires_verification(blocking_reason_codes) {
                Some(FollowUpKind::RunVerification)
            } else {
                Some(FollowUpKind::WaitForExternalReviewResult)
            }
        }
        crate::execution::phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING => {
            Some(FollowUpKind::WaitForExternalReviewResult)
        }
        crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED => {
            Some(FollowUpKind::ResolveReleaseBlocker)
        }
        crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED => {
            Some(FollowUpKind::ExecutionReentry)
        }
        crate::execution::phase::DETAIL_HANDOFF_RECORDING_REQUIRED => {
            Some(FollowUpKind::RecordHandoff)
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn follow_up_command_template(follow_up: Option<&str>) -> Option<String> {
    normalize_follow_up_alias(follow_up, FollowUpAliasContext::PublicRouting)
        .and_then(FollowUpKind::command_template)
        .map(str::to_owned)
}

fn public_routing_alias(follow_up: &str) -> Option<FollowUpKind> {
    FOLLOW_UP_ALIAS_RULES
        .iter()
        .find(|rule| rule.token == follow_up)
        .and_then(|rule| rule.public_kind)
}

fn persisted_repair_state_alias(follow_up: &str) -> Option<FollowUpKind> {
    FOLLOW_UP_ALIAS_RULES
        .iter()
        .find(|rule| rule.token == follow_up)
        .and_then(|rule| rule.persisted_repair_kind)
}

pub(crate) fn direct_gate_follow_up_from_reason_codes<'a>(
    reason_codes: impl IntoIterator<Item = &'a str>,
    routing_review_state_status: Option<&str>,
    routing_required_follow_up: Option<FollowUpKind>,
) -> Option<FollowUpKind> {
    let reason_codes = reason_codes.into_iter().collect::<Vec<_>>();
    if reason_codes.iter().copied().any(|code| {
        matches!(
            code,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
                | crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
                | crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        )
    }) {
        return routing_required_follow_up.or(Some(FollowUpKind::AdvanceLateStage));
    }
    if reason_codes
        .iter()
        .copied()
        .any(finish_review_gate_already_current_reason_code)
    {
        return Some(FollowUpKind::GateFinish);
    }
    if reason_codes.contains(&"finish_review_gate_checkpoint_missing") {
        return Some(FollowUpKind::GateReview);
    }
    if reason_codes
        .iter()
        .copied()
        .any(crate::execution::closure_diagnostics::task_boundary_overlay_restore_reason_code)
    {
        return Some(FollowUpKind::RepairReviewState);
    }
    if reason_codes.contains(&"current_branch_reviewed_state_id_missing") {
        return Some(FollowUpKind::RepairReviewState);
    }
    if reason_codes.contains(&"unfinished_steps_remaining")
        && (routing_review_state_status.is_some_and(|status| status != "clean")
            || routing_required_follow_up == Some(FollowUpKind::RepairReviewState))
    {
        return Some(FollowUpKind::RepairReviewState);
    }
    None
}

fn task_review_result_requires_verification<'a>(
    reason_codes: impl IntoIterator<Item = &'a str>,
) -> bool {
    reason_codes.into_iter().any(
        crate::execution::closure_diagnostics::task_boundary_verification_diagnostic_reason_code,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        FollowUpKind, direct_gate_follow_up_from_reason_codes, follow_up_command_template,
        follow_up_from_phase_detail, normalize_persisted_repair_follow_up_token,
        normalize_public_routing_follow_up_token,
    };
    use crate::execution::command_eligibility::hidden_command_tokens;
    use crate::execution::review_route_tokens::{
        FOLLOW_UP_ADVANCE_LATE_STAGE, FOLLOW_UP_EXECUTION_REENTRY, FOLLOW_UP_REPAIR_REVIEW_STATE,
        REVIEW_STATE_STALE_UNREVIEWED,
    };

    #[test]
    fn public_routing_aliases_are_canonicalized_once() {
        for alias in [
            "record_branch_closure",
            "branch_closure",
            FOLLOW_UP_ADVANCE_LATE_STAGE,
        ] {
            assert_eq!(
                normalize_public_routing_follow_up_token(Some(alias)),
                Some(FOLLOW_UP_ADVANCE_LATE_STAGE)
            );
        }
        for alias in ["record_task_closure", FOLLOW_UP_EXECUTION_REENTRY] {
            assert_eq!(
                normalize_public_routing_follow_up_token(Some(alias)),
                Some(FOLLOW_UP_EXECUTION_REENTRY)
            );
        }
        assert_eq!(
            normalize_public_routing_follow_up_token(Some("record_pivot")),
            Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        );
    }

    #[test]
    fn gate_follow_up_reason_mapping_is_centralized() {
        assert_eq!(
            direct_gate_follow_up_from_reason_codes(
                ["finish_review_gate_checkpoint_missing"],
                Some("clean"),
                None,
            ),
            Some(FollowUpKind::GateReview)
        );
        assert_eq!(
            direct_gate_follow_up_from_reason_codes(
                ["unfinished_steps_remaining"],
                Some(REVIEW_STATE_STALE_UNREVIEWED),
                None,
            ),
            Some(FollowUpKind::RepairReviewState)
        );
        assert_eq!(
            direct_gate_follow_up_from_reason_codes(
                ["unfinished_steps_remaining"],
                Some("clean"),
                Some(FollowUpKind::AdvanceLateStage),
            ),
            None
        );
        assert_eq!(
            direct_gate_follow_up_from_reason_codes(
                [crate::execution::review_route_tokens::REASON_DERIVED_REVIEW_STATE_MISSING],
                Some("stale_unreviewed"),
                Some(FollowUpKind::RepairReviewState),
            ),
            None
        );
        assert_eq!(
            direct_gate_follow_up_from_reason_codes(
                [crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY],
                Some("clean"),
                None,
            ),
            Some(FollowUpKind::AdvanceLateStage)
        );
    }

    #[test]
    fn persisted_repair_aliases_preserve_projection_repair_intent() {
        assert_eq!(
            normalize_persisted_repair_follow_up_token(Some("record_branch_closure")),
            Some(FOLLOW_UP_ADVANCE_LATE_STAGE)
        );
        assert_eq!(
            normalize_persisted_repair_follow_up_token(Some("record_task_closure")),
            Some("close_current_task")
        );
        assert_eq!(
            normalize_persisted_repair_follow_up_token(Some("record_pivot")),
            Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        );
    }

    #[test]
    fn phase_detail_follow_up_resolution_is_shared() {
        assert_eq!(
            follow_up_from_phase_detail(
                crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
                [crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING]
            ),
            Some(FollowUpKind::RunVerification)
        );
        assert_eq!(
            follow_up_from_phase_detail(
                crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
                ["task_review_pending"]
            ),
            Some(FollowUpKind::WaitForExternalReviewResult)
        );
        assert_eq!(
            follow_up_from_phase_detail(
                crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
                std::iter::empty()
            ),
            Some(FollowUpKind::ExecutionReentry)
        );
    }

    #[test]
    fn public_follow_up_templates_do_not_surface_removed_hidden_commands() {
        for follow_up in [
            FOLLOW_UP_REPAIR_REVIEW_STATE,
            FOLLOW_UP_ADVANCE_LATE_STAGE,
            "resolve_release_blocker",
            "record_handoff",
            FOLLOW_UP_EXECUTION_REENTRY,
            "request_external_review",
            "wait_for_external_review_result",
            "run_verification",
        ] {
            let template = follow_up_command_template(Some(follow_up))
                .expect("public follow-up should expose a public command template");
            for hidden_token in hidden_command_tokens() {
                assert!(
                    !template.contains(hidden_token.as_str()),
                    "{follow_up} exposed hidden command token {hidden_token}: {template}"
                );
            }
        }
    }
}
