//! Execution runtime ownership map:
//! - query owns the authoritative review-state read model
//! - recording owns authoritative reviewed-closure and milestone writes
//! - review_state owns explain/reconcile intent adapters over those boundaries

use std::path::Path;

pub mod authority;
pub mod closure_graph;
pub mod command_eligibility;
pub mod current_truth;
pub mod dependency_index;
pub mod final_review;
pub mod gates;
pub mod handoff;
pub mod harness;
pub mod leases;
pub mod mutate;
pub mod next_action;
pub mod observability;
pub mod projection_renderer;
pub mod query;
pub mod recording;
pub mod review_state;
pub mod state;
pub mod topology;
pub mod transitions;

pub(crate) fn workflow_operator_requery_command(
    plan: &Path,
    external_review_result_ready: bool,
) -> String {
    if external_review_result_ready {
        format!(
            "featureforge workflow operator --plan {} --external-review-result-ready",
            plan.display()
        )
    } else {
        format!("featureforge workflow operator --plan {}", plan.display())
    }
}
