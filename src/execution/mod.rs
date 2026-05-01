//! Execution runtime ownership map:
//! - query owns the authoritative review-state read model
//! - event_log owns append-only authoritative execution history and legacy migration
//! - reducer owns `RuntimeState = reduce(EventLog, SemanticWorkspaceSnapshot)`
//! - router owns `RouteDecision = route(RuntimeState)`
//! - query/review_state project read models and repair adapters from that shared core

use std::path::Path;

pub mod authority;
pub mod closure_graph;
pub mod command_eligibility;
pub(crate) mod command_model;
pub mod commands;
pub mod context;
pub(crate) mod current_closure_projection;
pub mod current_truth;
pub mod dependency_index;
pub mod event_log;
pub(crate) mod fields;
pub mod final_review;
pub mod follow_up;
pub mod gates;
pub mod handoff;
pub mod harness;
pub mod internal_args;
pub mod invariants;
pub(crate) mod late_stage_route_selection;
pub mod leases;
pub mod mutate;
pub mod next_action;
pub mod observability;
pub mod phase;
pub mod projection_renderer;
pub(crate) mod public_route_selection;
pub mod query;
pub mod read_model;
pub(crate) mod read_model_support;
pub mod recording;
pub mod reducer;
pub mod reentry_reconcile;
pub(crate) mod repair_target_selection;
pub mod review_state;
pub mod router;
pub mod runtime;
pub mod semantic_identity;
pub(crate) mod stale_target_projection;
pub mod state;
pub mod status;
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
