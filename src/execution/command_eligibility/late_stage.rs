use super::PublicCommand;
use super::mutation_request::PublicMutationRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicAdvanceLateStageMode {
    Basic,
    ReleaseReadiness,
    FinalReviewDispatch,
    Qa,
    FinalReview,
    FinishReview,
    FinishCompletion,
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
        crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => {
            Some(PublicAdvanceLateStageMode::FinalReviewDispatch)
        }
        crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY => {
            Some(PublicAdvanceLateStageMode::FinalReview)
        }
        crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED => {
            Some(PublicAdvanceLateStageMode::Qa)
        }
        crate::execution::phase::DETAIL_FINISH_REVIEW_GATE_READY => {
            Some(PublicAdvanceLateStageMode::FinishReview)
        }
        crate::execution::phase::DETAIL_FINISH_COMPLETION_GATE_READY => {
            Some(PublicAdvanceLateStageMode::FinishCompletion)
        }
        _ => None,
    }
}

pub(crate) fn public_advance_late_stage_mode_for_invocation(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
    result: Option<&str>,
    final_review_inputs_present: bool,
) -> PublicAdvanceLateStageMode {
    let routed_mode = recommended_public_command
        .and_then(public_advance_late_stage_mode_from_command)
        .or_else(|| public_advance_late_stage_mode_for_phase_detail(phase_detail));
    if final_review_inputs_present {
        if routed_mode == Some(PublicAdvanceLateStageMode::FinalReviewDispatch) {
            return PublicAdvanceLateStageMode::FinalReviewDispatch;
        }
        return PublicAdvanceLateStageMode::FinalReview;
    }
    match result {
        Some("ready" | "blocked") => PublicAdvanceLateStageMode::ReleaseReadiness,
        Some("pass" | "fail") => routed_mode
            .filter(|mode| {
                matches!(
                    mode,
                    PublicAdvanceLateStageMode::FinalReview | PublicAdvanceLateStageMode::Qa
                )
            })
            .unwrap_or(PublicAdvanceLateStageMode::Qa),
        _ => routed_mode.unwrap_or(PublicAdvanceLateStageMode::Basic),
    }
}

pub(crate) fn public_advance_late_stage_mode_from_command(
    command: &PublicCommand,
) -> Option<PublicAdvanceLateStageMode> {
    match command {
        PublicCommand::AdvanceLateStage { mode, .. } => Some(*mode),
        _ => None,
    }
}

pub(crate) fn public_advance_late_stage_mode_for_routed_public_command(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> Option<PublicAdvanceLateStageMode> {
    let command_mode =
        recommended_public_command.and_then(public_advance_late_stage_mode_from_command)?;
    match public_advance_late_stage_mode_for_phase_detail(phase_detail) {
        Some(phase_detail_mode) if phase_detail_mode != command_mode => None,
        _ => Some(command_mode),
    }
}

pub(crate) fn public_advance_late_stage_request_mode_matches_routed_public_command(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
    request_mode: Option<PublicAdvanceLateStageMode>,
) -> bool {
    request_mode.is_some_and(|request_mode| {
        public_advance_late_stage_mode_for_routed_public_command(
            phase_detail,
            recommended_public_command,
        ) == Some(request_mode)
    })
}

fn routed_public_command_matches_mode(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
    mode: PublicAdvanceLateStageMode,
) -> bool {
    public_advance_late_stage_mode_for_routed_public_command(
        phase_detail,
        recommended_public_command,
    ) == Some(mode)
}

pub(crate) fn routed_public_command_is_branch_closure(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_matches_mode(
        phase_detail,
        recommended_public_command,
        PublicAdvanceLateStageMode::Basic,
    )
}

pub(crate) fn routed_public_command_is_release_readiness(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_matches_mode(
        phase_detail,
        recommended_public_command,
        PublicAdvanceLateStageMode::ReleaseReadiness,
    )
}

pub(crate) fn routed_public_command_is_final_review_dispatch(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_matches_mode(
        phase_detail,
        recommended_public_command,
        PublicAdvanceLateStageMode::FinalReviewDispatch,
    )
}

pub(crate) fn routed_public_command_is_qa(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_matches_mode(
        phase_detail,
        recommended_public_command,
        PublicAdvanceLateStageMode::Qa,
    )
}

pub(crate) fn routed_public_command_is_final_review(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_matches_mode(
        phase_detail,
        recommended_public_command,
        PublicAdvanceLateStageMode::FinalReview,
    )
}

pub(crate) fn routed_public_command_accepts_final_review_inputs(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_is_final_review_dispatch(phase_detail, recommended_public_command)
        || routed_public_command_is_final_review(phase_detail, recommended_public_command)
}

pub(crate) fn routed_public_command_is_finish_review(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_matches_mode(
        phase_detail,
        recommended_public_command,
        PublicAdvanceLateStageMode::FinishReview,
    )
}

pub(crate) fn routed_public_command_is_finish_completion(
    phase_detail: &str,
    recommended_public_command: Option<&PublicCommand>,
) -> bool {
    routed_public_command_matches_mode(
        phase_detail,
        recommended_public_command,
        PublicAdvanceLateStageMode::FinishCompletion,
    )
}

pub(crate) fn public_advance_late_stage_command_for_phase_detail(
    plan_path: &str,
    phase_detail: &str,
) -> Option<PublicCommand> {
    public_advance_late_stage_mode_for_phase_detail(phase_detail)
        .map(|mode| public_advance_late_stage_command_for_mode(plan_path, mode))
}

pub(crate) fn public_advance_late_stage_command_for_follow_up(
    plan_path: &str,
    phase_detail: &str,
    record_type: Option<&str>,
) -> Option<PublicCommand> {
    public_advance_late_stage_mode_for_phase_detail(phase_detail)
        .or_else(|| public_advance_late_stage_mode_for_record_type(record_type?))
        .map(|mode| public_advance_late_stage_command_for_mode(plan_path, mode))
}

fn public_advance_late_stage_mode_for_record_type(
    record_type: &str,
) -> Option<PublicAdvanceLateStageMode> {
    match record_type {
        "branch_closure" => Some(PublicAdvanceLateStageMode::Basic),
        "release_readiness" => Some(PublicAdvanceLateStageMode::ReleaseReadiness),
        _ => None,
    }
}

fn public_advance_late_stage_command_for_mode(
    plan_path: &str,
    mode: PublicAdvanceLateStageMode,
) -> PublicCommand {
    PublicCommand::AdvanceLateStage {
        plan: plan_path.to_owned(),
        mode,
    }
}

pub(crate) fn public_advance_late_stage_final_review_mutation_request() -> PublicMutationRequest {
    PublicMutationRequest::advance_late_stage(PublicAdvanceLateStageMode::FinalReview)
}

pub(crate) fn public_advance_late_stage_mode_name(
    mode: PublicAdvanceLateStageMode,
) -> &'static str {
    match mode {
        PublicAdvanceLateStageMode::Basic => "branch_closure",
        PublicAdvanceLateStageMode::ReleaseReadiness => "release_readiness",
        PublicAdvanceLateStageMode::FinalReviewDispatch => "final_review_dispatch",
        PublicAdvanceLateStageMode::Qa => "qa",
        PublicAdvanceLateStageMode::FinalReview => "final_review",
        PublicAdvanceLateStageMode::FinishReview => "finish_review",
        PublicAdvanceLateStageMode::FinishCompletion => "finish_completion",
    }
}
