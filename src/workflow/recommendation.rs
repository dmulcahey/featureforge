//! Compatibility recommendation projection for workflow presentation surfaces.
//!
//! `recommended_skill` and recommendation prose are legacy presentation fields.
//! They are derived from route/operator truth here and are not a separate
//! routing authority.

use crate::contracts::workflow::WorkflowRoute;
use crate::execution::harness::HarnessPhase;
use crate::execution::phase;

pub(crate) const SKILL_WRITING_PLANS: &str = "featureforge:writing-plans";
pub(crate) const SKILL_PLAN_ENG_REVIEW: &str = "featureforge:plan-eng-review";
pub(crate) const SKILL_PLAN_FIDELITY_REVIEW: &str = "featureforge:plan-fidelity-review";
pub(crate) const SKILL_REQUESTING_CODE_REVIEW: &str = "featureforge:requesting-code-review";
pub(crate) const SKILL_QA_ONLY: &str = "featureforge:qa-only";
pub(crate) const SKILL_DOCUMENT_RELEASE: &str = "featureforge:document-release";
pub(crate) const SKILL_FINISHING_BRANCH: &str = "featureforge:finishing-a-development-branch";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowRecommendation {
    pub(crate) skill: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExplicitRecommendation<'a> {
    pub(crate) skill: &'a str,
    pub(crate) reason: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HandoffRecommendationInput<'a> {
    pub(crate) explicit: Option<ExplicitRecommendation<'a>>,
    pub(crate) phase: &'a str,
    pub(crate) phase_detail: &'a str,
    pub(crate) route: &'a WorkflowRoute,
    pub(crate) execution_started: &'a str,
    pub(crate) execution_mode: Option<&'a str>,
    pub(crate) execution_preflight_block_reason: Option<&'a str>,
    pub(crate) review_requires_execution_reentry: bool,
    pub(crate) task_boundary_next_step: Option<&'a str>,
    pub(crate) task_boundary_reason: Option<&'a str>,
    pub(crate) gate_review_message: Option<&'a str>,
    pub(crate) gate_finish_message: Option<&'a str>,
}

pub(crate) fn handoff_recommendation(
    input: HandoffRecommendationInput<'_>,
) -> WorkflowRecommendation {
    if let Some(explicit) = input.explicit {
        return WorkflowRecommendation {
            skill: explicit.skill.to_owned(),
            reason: explicit.reason.to_owned(),
        };
    }

    // `task_closure_pending` is a route-only presentation phase, not a
    // persisted HarnessPhase. Keep this exception outside canonical parsing.
    if input.phase == phase::PHASE_TASK_CLOSURE_PENDING {
        return WorkflowRecommendation {
            skill: execution_mode(input),
            reason: input
                .task_boundary_next_step
                .or(input.task_boundary_reason)
                .map(str::to_owned)
                .unwrap_or_else(execution_started_reason),
        };
    }

    match HarnessPhase::parse(input.phase) {
        Some(HarnessPhase::ExecutionPreflight) => WorkflowRecommendation {
            skill: String::new(),
            reason: String::new(),
        },
        Some(HarnessPhase::Executing) => WorkflowRecommendation {
            skill: execution_mode(input),
            reason: execution_started_reason(),
        },
        Some(HarnessPhase::ImplementationHandoff) => WorkflowRecommendation {
            skill: String::new(),
            reason: implementation_handoff_reason(input),
        },
        Some(HarnessPhase::FinalReviewPending) if input.review_requires_execution_reentry => {
            WorkflowRecommendation {
                skill: execution_mode(input),
                reason: final_review_reason(input),
            }
        }
        Some(HarnessPhase::FinalReviewPending) => WorkflowRecommendation {
            skill: route_skill_or(input.route, SKILL_REQUESTING_CODE_REVIEW),
            reason: final_review_reason(input),
        },
        Some(HarnessPhase::QaPending)
            if input.phase_detail == phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED =>
        {
            WorkflowRecommendation {
                skill: route_skill_or(input.route, SKILL_PLAN_ENG_REVIEW),
                reason: qa_or_release_reason(input),
            }
        }
        Some(HarnessPhase::QaPending) => WorkflowRecommendation {
            skill: route_skill_or(input.route, SKILL_QA_ONLY),
            reason: qa_or_release_reason(input),
        },
        Some(HarnessPhase::DocumentReleasePending) => WorkflowRecommendation {
            skill: route_skill_or(input.route, SKILL_DOCUMENT_RELEASE),
            reason: qa_or_release_reason(input),
        },
        Some(HarnessPhase::ReadyForBranchCompletion) => WorkflowRecommendation {
            skill: route_skill_or(input.route, SKILL_FINISHING_BRANCH),
            reason: String::from(
                "All required late-stage artifacts are fresh for the current HEAD.",
            ),
        },
        Some(HarnessPhase::PivotRequired) => WorkflowRecommendation {
            skill: pivot_planning_skill(input.route),
            reason: String::from("Execution is blocked pending an approved plan revision."),
        },
        Some(
            HarnessPhase::ContractDrafting
            | HarnessPhase::ContractPendingApproval
            | HarnessPhase::ContractApproved
            | HarnessPhase::Evaluating
            | HarnessPhase::HandoffRequired,
        ) if input.execution_started == "yes" => WorkflowRecommendation {
            skill: execution_mode(input),
            reason: execution_started_reason(),
        },
        Some(
            HarnessPhase::ContractDrafting
            | HarnessPhase::ContractPendingApproval
            | HarnessPhase::ContractApproved
            | HarnessPhase::Evaluating
            | HarnessPhase::HandoffRequired,
        ) => WorkflowRecommendation {
            skill: String::new(),
            reason: execution_started_reason(),
        },
        Some(HarnessPhase::Repairing) | None if input.execution_started == "yes" => {
            WorkflowRecommendation {
                skill: execution_mode(input),
                reason: execution_started_reason(),
            }
        }
        Some(HarnessPhase::Repairing) | None => WorkflowRecommendation {
            skill: String::new(),
            reason: String::new(),
        },
    }
}

pub(crate) fn next_text_for_phase(
    phase_value: &str,
    route_status: &str,
    plan_path: &str,
    next_skill: &str,
) -> String {
    // `task_closure_pending` is a route-only presentation phase, not a
    // persisted HarnessPhase. It still routes back to the current execution
    // flow, so keep the exception explicit and documented here.
    if phase_value == phase::PHASE_TASK_CLOSURE_PENDING {
        return current_execution_next_text(plan_path);
    }

    match HarnessPhase::parse(phase_value) {
        Some(HarnessPhase::ExecutionPreflight | HarnessPhase::ImplementationHandoff) => {
            if plan_path.is_empty() {
                String::from("Return to execution preflight for the approved plan.")
            } else {
                format!("Return to execution preflight for the approved plan: {plan_path}")
            }
        }
        Some(HarnessPhase::Executing) => current_execution_next_text(plan_path),
        Some(
            HarnessPhase::ContractDrafting
            | HarnessPhase::ContractPendingApproval
            | HarnessPhase::ContractApproved
            | HarnessPhase::Evaluating
            | HarnessPhase::HandoffRequired,
        ) => current_execution_next_text(plan_path),
        Some(HarnessPhase::PivotRequired) => {
            let planning_skill = pivot_planning_skill_from_next_skill(next_skill);
            if plan_path.is_empty() {
                format!(
                    "Return to {planning_skill} and refresh the planning/review gate before continuing execution."
                )
            } else {
                format!(
                    "Return to {planning_skill} for planning reentry on the approved plan before continuing execution: {plan_path}"
                )
            }
        }
        Some(HarnessPhase::FinalReviewPending) => {
            if plan_path.is_empty() {
                String::from("Use featureforge:requesting-code-review for the final review gate.")
            } else {
                format!(
                    "Use featureforge:requesting-code-review for the approved plan before branch completion: {plan_path}"
                )
            }
        }
        Some(HarnessPhase::QaPending) => String::from(
            "Run featureforge:qa-only and return with a fresh QA result artifact before branch completion.",
        ),
        Some(HarnessPhase::DocumentReleasePending) => String::from(
            "Run featureforge:document-release and return with a fresh release-readiness artifact before branch completion.",
        ),
        Some(HarnessPhase::ReadyForBranchCompletion) => {
            String::from("Use featureforge:finishing-a-development-branch.")
        }
        Some(HarnessPhase::Repairing) | None => {
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

fn current_execution_next_text(plan_path: &str) -> String {
    if plan_path.is_empty() {
        String::from("Return to the current execution flow for the approved plan.")
    } else {
        format!("Return to the current execution flow for the approved plan: {plan_path}")
    }
}

pub(crate) fn route_skill_or(route: &WorkflowRoute, fallback: &str) -> String {
    if route.next_skill.trim().is_empty() {
        fallback.to_owned()
    } else {
        route.next_skill.clone()
    }
}

pub(crate) fn pivot_planning_skill(route: &WorkflowRoute) -> String {
    pivot_planning_skill_from_next_skill(&route.next_skill)
}

pub(crate) fn pivot_planning_skill_from_next_skill(next_skill: &str) -> String {
    if next_skill.trim().is_empty() {
        SKILL_PLAN_ENG_REVIEW.to_owned()
    } else {
        next_skill.to_owned()
    }
}

fn execution_mode(input: HandoffRecommendationInput<'_>) -> String {
    input.execution_mode.unwrap_or_default().to_owned()
}

fn implementation_handoff_reason(input: HandoffRecommendationInput<'_>) -> String {
    input
        .execution_preflight_block_reason
        .map(str::to_owned)
        .unwrap_or_else(|| {
            String::from(
                "The approved plan is ready, but execution preflight is still blocked by the current workspace state.",
            )
        })
}

fn final_review_reason(input: HandoffRecommendationInput<'_>) -> String {
    input
        .gate_review_message
        .or(input.gate_finish_message)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            String::from("Execution is blocked on the final review gate for the approved plan.")
        })
}

fn qa_or_release_reason(input: HandoffRecommendationInput<'_>) -> String {
    input
        .gate_finish_message
        .map(str::to_owned)
        .unwrap_or_else(|| input.route.reason.clone())
}

fn execution_started_reason() -> String {
    String::from(
        "Execution already started for the approved plan revision; continue with the current execution flow.",
    )
}
