//! Execution runtime ownership map:
//! - query owns the authoritative review-state read model
//! - recording owns authoritative reviewed-closure and milestone writes
//! - `review_state` owns explain/reconcile intent adapters over those boundaries

use std::path::Path;

/// Runtime module.
pub mod authority;
/// Runtime module.
pub mod closure_graph;
/// Runtime module.
pub mod command_eligibility;
/// Runtime module.
pub mod current_truth;
/// Runtime module.
pub mod dependency_index;
/// Runtime module.
pub mod final_review;
/// Runtime module.
pub mod gates;
/// Runtime module.
pub mod handoff;
/// Runtime module.
pub mod harness;
/// Runtime module.
pub mod leases;
/// Runtime module.
pub mod mutate;
/// Runtime module.
pub mod next_action;
/// Runtime module.
pub mod observability;
/// Runtime module.
pub mod projection_renderer;
/// Runtime module.
pub mod query;
pub mod recording;
pub mod review_state;
/// Runtime module.
pub mod state;
/// Runtime module.
pub mod topology;
/// Runtime module.
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
