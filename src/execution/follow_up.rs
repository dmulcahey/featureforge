use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git::sha256_hex;

use crate::execution::workflow_operator_requery_command;

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
        token: "advance_late_stage",
        public_kind: Some(FollowUpKind::AdvanceLateStage),
        persisted_repair_kind: Some(FollowUpKind::AdvanceLateStage),
    },
    FollowUpAliasRule {
        token: "record_task_closure",
        public_kind: Some(FollowUpKind::ExecutionReentry),
        persisted_repair_kind: Some(FollowUpKind::CloseCurrentTask),
    },
    FollowUpAliasRule {
        token: "close_current_task",
        public_kind: Some(FollowUpKind::CloseCurrentTask),
        persisted_repair_kind: Some(FollowUpKind::CloseCurrentTask),
    },
    FollowUpAliasRule {
        token: "execution_reentry",
        public_kind: Some(FollowUpKind::ExecutionReentry),
        persisted_repair_kind: Some(FollowUpKind::ExecutionReentry),
    },
    FollowUpAliasRule {
        token: "request_external_review",
        public_kind: Some(FollowUpKind::RequestExternalReview),
        persisted_repair_kind: Some(FollowUpKind::RequestExternalReview),
    },
    FollowUpAliasRule {
        token: "wait_for_external_review_result",
        public_kind: Some(FollowUpKind::WaitForExternalReviewResult),
        persisted_repair_kind: Some(FollowUpKind::WaitForExternalReviewResult),
    },
    FollowUpAliasRule {
        token: "run_verification",
        public_kind: Some(FollowUpKind::RunVerification),
        persisted_repair_kind: Some(FollowUpKind::RunVerification),
    },
    FollowUpAliasRule {
        token: "resolve_release_blocker",
        public_kind: Some(FollowUpKind::ResolveReleaseBlocker),
        persisted_repair_kind: Some(FollowUpKind::ResolveReleaseBlocker),
    },
    FollowUpAliasRule {
        token: "record_handoff",
        public_kind: Some(FollowUpKind::RecordHandoff),
        persisted_repair_kind: Some(FollowUpKind::RecordHandoff),
    },
    FollowUpAliasRule {
        token: "repair_review_state",
        public_kind: Some(FollowUpKind::RepairReviewState),
        persisted_repair_kind: Some(FollowUpKind::RepairReviewState),
    },
    FollowUpAliasRule {
        token: "record_pivot",
        public_kind: Some(FollowUpKind::RepairReviewState),
        persisted_repair_kind: Some(FollowUpKind::RepairReviewState),
    },
    FollowUpAliasRule {
        token: "gate_review",
        public_kind: Some(FollowUpKind::GateReview),
        persisted_repair_kind: Some(FollowUpKind::GateReview),
    },
    FollowUpAliasRule {
        token: "gate_finish",
        public_kind: Some(FollowUpKind::GateFinish),
        persisted_repair_kind: Some(FollowUpKind::GateFinish),
    },
];

impl FollowUpKind {
    pub(crate) fn public_token(self) -> &'static str {
        match self {
            Self::RepairReviewState => "repair_review_state",
            Self::AdvanceLateStage => "advance_late_stage",
            Self::ExecutionReentry => "execution_reentry",
            Self::CloseCurrentTask => "close_current_task",
            Self::RequestExternalReview => "request_external_review",
            Self::WaitForExternalReviewResult => "wait_for_external_review_result",
            Self::RunVerification => "run_verification",
            Self::ResolveReleaseBlocker => "resolve_release_blocker",
            Self::RecordHandoff => "record_handoff",
            Self::GateReview => "gate_review",
            Self::GateFinish => "gate_finish",
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
            Self::RecordHandoff => Some(
                "featureforge plan execution transfer --plan <approved-plan-path> --scope task|branch --to <owner> --reason <reason>",
            ),
            Self::ExecutionReentry
            | Self::RequestExternalReview
            | Self::WaitForExternalReviewResult
            | Self::RunVerification => {
                Some("featureforge workflow operator --plan <approved-plan-path>")
            }
            Self::CloseCurrentTask | Self::GateReview | Self::GateFinish => None,
        }
    }

    pub(crate) fn materialized_command(
        self,
        plan_path: &Path,
        external_review_result_ready: bool,
    ) -> Option<String> {
        match self {
            Self::GateReview | Self::GateFinish => Some(workflow_operator_requery_command(
                plan_path,
                external_review_result_ready,
            )),
            Self::RepairReviewState => Some(format!(
                "featureforge plan execution repair-review-state --plan {}",
                plan_path.display()
            )),
            Self::AdvanceLateStage | Self::ResolveReleaseBlocker => Some(format!(
                "featureforge plan execution advance-late-stage --plan {}",
                plan_path.display()
            )),
            Self::RecordHandoff => Some(format!(
                "featureforge plan execution transfer --plan {} --scope task|branch --to <owner> --reason <reason>",
                plan_path.display()
            )),
            Self::ExecutionReentry
            | Self::RequestExternalReview
            | Self::WaitForExternalReviewResult
            | Self::RunVerification => Some(workflow_operator_requery_command(
                plan_path,
                external_review_result_ready,
            )),
            Self::CloseCurrentTask => None,
        }
    }
}

impl RepairFollowUpKind {
    pub(crate) fn from_public_follow_up(kind: FollowUpKind) -> Self {
        match kind {
            FollowUpKind::RepairReviewState => Self::RepairReviewState,
            FollowUpKind::AdvanceLateStage => Self::AdvanceLateStage,
            FollowUpKind::ExecutionReentry => Self::ExecutionReentry,
            FollowUpKind::CloseCurrentTask => Self::CloseTask,
            FollowUpKind::RequestExternalReview => Self::RequestExternalReview,
            FollowUpKind::WaitForExternalReviewResult => Self::WaitForExternalReviewResult,
            FollowUpKind::RunVerification => Self::RunVerification,
            FollowUpKind::ResolveReleaseBlocker => Self::ResolveReleaseBlocker,
            FollowUpKind::RecordHandoff => Self::RecordHandoff,
            FollowUpKind::GateReview => Self::GateReview,
            FollowUpKind::GateFinish => Self::GateFinish,
        }
    }

    pub(crate) fn from_persisted_token(token: &str) -> Option<Self> {
        match token.trim() {
            "record_branch_closure" | "branch_closure" => Some(Self::RecordBranchClosure),
            "advance_late_stage" => Some(Self::AdvanceLateStage),
            "record_task_closure" | "close_current_task" => Some(Self::CloseTask),
            "execution_reentry" => Some(Self::ExecutionReentry),
            "repair_review_state" | "record_pivot" => Some(Self::RepairReviewState),
            "request_external_review" => Some(Self::RequestExternalReview),
            "wait_for_external_review_result" => Some(Self::WaitForExternalReviewResult),
            "run_verification" => Some(Self::RunVerification),
            "resolve_release_blocker" => Some(Self::ResolveReleaseBlocker),
            "record_handoff" => Some(Self::RecordHandoff),
            "gate_review" => Some(Self::GateReview),
            "gate_finish" => Some(Self::GateFinish),
            _ => None,
        }
    }

    pub(crate) fn persisted_token(self) -> &'static str {
        match self {
            Self::RecordBranchClosure => "record_branch_closure",
            Self::AdvanceLateStage => "advance_late_stage",
            Self::RecordFinalReview => "record_final_review",
            Self::RecordQa => "record_qa",
            Self::CloseTask => "record_task_closure",
            Self::RepairReviewState => "repair_review_state",
            Self::ExecutionReentry => "execution_reentry",
            Self::RequestExternalReview => "request_external_review",
            Self::WaitForExternalReviewResult => "wait_for_external_review_result",
            Self::RunVerification => "run_verification",
            Self::ResolveReleaseBlocker => "resolve_release_blocker",
            Self::RecordHandoff => "record_handoff",
            Self::GateReview => "gate_review",
            Self::GateFinish => "gate_finish",
        }
    }

    pub(crate) fn public_token(self) -> &'static str {
        match self {
            Self::RecordBranchClosure | Self::AdvanceLateStage => "advance_late_stage",
            Self::CloseTask => "close_current_task",
            other => other.persisted_token(),
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

pub(crate) fn materialized_follow_up_kind_command(
    follow_up: FollowUpKind,
    plan_path: &Path,
    external_review_result_ready: bool,
) -> Option<String> {
    follow_up.materialized_command(plan_path, external_review_result_ready)
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

pub(crate) fn missing_branch_closure_gate_follow_up(
    routing_review_state_status: Option<&str>,
    routing_required_follow_up: Option<FollowUpKind>,
) -> FollowUpKind {
    if routing_review_state_status == Some("missing_current_closure")
        || routing_required_follow_up == Some(FollowUpKind::AdvanceLateStage)
    {
        FollowUpKind::AdvanceLateStage
    } else {
        FollowUpKind::RepairReviewState
    }
}

pub(crate) fn direct_gate_follow_up_from_reason_codes<'a>(
    reason_codes: impl IntoIterator<Item = &'a str>,
    routing_review_state_status: Option<&str>,
    routing_required_follow_up: Option<FollowUpKind>,
) -> Option<FollowUpKind> {
    let reason_codes = reason_codes.into_iter().collect::<Vec<_>>();
    if reason_codes.contains(&"finish_review_gate_already_current") {
        return Some(FollowUpKind::GateFinish);
    }
    if reason_codes.contains(&"finish_review_gate_checkpoint_missing") {
        return Some(FollowUpKind::GateReview);
    }
    if reason_codes.contains(&"current_task_closure_overlay_restore_required") {
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
    reason_codes.into_iter().any(|reason_code| {
        matches!(
            reason_code,
            "prior_task_verification_missing"
                | "prior_task_verification_missing_legacy"
                | "task_verification_summary_malformed"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        FollowUpKind, direct_gate_follow_up_from_reason_codes, follow_up_command_template,
        follow_up_from_phase_detail, missing_branch_closure_gate_follow_up,
        normalize_persisted_repair_follow_up_token, normalize_public_routing_follow_up_token,
    };

    #[test]
    fn public_routing_aliases_are_canonicalized_once() {
        for alias in [
            "record_branch_closure",
            "branch_closure",
            "advance_late_stage",
        ] {
            assert_eq!(
                normalize_public_routing_follow_up_token(Some(alias)),
                Some("advance_late_stage")
            );
        }
        for alias in ["record_task_closure", "execution_reentry"] {
            assert_eq!(
                normalize_public_routing_follow_up_token(Some(alias)),
                Some("execution_reentry")
            );
        }
        assert_eq!(
            normalize_public_routing_follow_up_token(Some("record_pivot")),
            Some("repair_review_state")
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
                Some("stale_unreviewed"),
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
    }

    #[test]
    fn missing_branch_closure_gate_follow_up_uses_shared_required_follow_up_taxonomy() {
        assert_eq!(
            missing_branch_closure_gate_follow_up(Some("missing_current_closure"), None),
            FollowUpKind::AdvanceLateStage
        );
        assert_eq!(
            missing_branch_closure_gate_follow_up(
                Some("clean"),
                Some(FollowUpKind::AdvanceLateStage)
            ),
            FollowUpKind::AdvanceLateStage
        );
        assert_eq!(
            missing_branch_closure_gate_follow_up(
                Some("clean"),
                Some(FollowUpKind::RepairReviewState)
            ),
            FollowUpKind::RepairReviewState
        );
    }

    #[test]
    fn persisted_repair_aliases_preserve_projection_repair_intent() {
        assert_eq!(
            normalize_persisted_repair_follow_up_token(Some("record_branch_closure")),
            Some("advance_late_stage")
        );
        assert_eq!(
            normalize_persisted_repair_follow_up_token(Some("record_task_closure")),
            Some("close_current_task")
        );
        assert_eq!(
            normalize_persisted_repair_follow_up_token(Some("record_pivot")),
            Some("repair_review_state")
        );
    }

    #[test]
    fn phase_detail_follow_up_resolution_is_shared() {
        assert_eq!(
            follow_up_from_phase_detail(
                crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
                ["prior_task_verification_missing"]
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
            "repair_review_state",
            "advance_late_stage",
            "resolve_release_blocker",
            "record_handoff",
            "execution_reentry",
            "request_external_review",
            "wait_for_external_review_result",
            "run_verification",
        ] {
            let template = follow_up_command_template(Some(follow_up))
                .expect("public follow-up should expose a public command template");
            for hidden_token in [
                "record-review-dispatch",
                "gate-review",
                "gate-finish",
                "rebuild-evidence",
                "plan execution preflight",
                "plan execution recommend",
                "workflow recommend",
            ] {
                assert!(
                    !template.contains(hidden_token),
                    "{follow_up} exposed hidden command token {hidden_token}: {template}"
                );
            }
        }
    }
}
