//! Execution runtime ownership map:
//! - query owns the authoritative review-state read model
//! - event_log owns append-only authoritative execution history and legacy migration
//! - reducer owns `RuntimeState = reduce(EventLog, SemanticWorkspaceSnapshot)`
//! - route_plan owns `RouteDecision = route(RuntimeState)`
//! - router projects route decisions into status/operator DTO surfaces
//! - query/review_state project read models and repair adapters from that shared core

use std::path::Path;

pub(crate) mod approved_plan_discovery;
pub mod authority;
pub(crate) mod branch_closure_provenance;
pub(crate) mod closure_diagnostics;
pub(crate) mod closure_dispatch;
pub(crate) mod closure_dispatch_mutation;
pub mod closure_graph;
pub mod command_eligibility;
pub(crate) mod command_model;
pub mod commands;
pub mod context;
pub(crate) mod current_closure_projection;
pub(crate) mod current_task_closure_cleanup;
pub(crate) mod current_task_closure_selection;
pub mod current_truth;
pub mod dependency_index;
pub(crate) mod event_command;
pub mod event_log;
pub(crate) mod fields;
pub mod final_review;
pub mod follow_up;
pub(crate) mod gate_reason_codes;
pub mod gates;
pub mod handoff;
pub mod harness;
pub(crate) mod implementation_gate;
pub mod internal_args;
pub mod invariants;
pub(crate) mod late_stage_precedence;
pub mod leases;
pub mod live_mutation_guard;
pub(crate) mod migration;
pub mod mutate;
pub mod next_action;
pub mod observability;
pub mod phase;
pub mod projection_renderer;
pub mod public_command_types;
pub(crate) mod public_recovery;
pub mod public_repair_target_reasons;
pub(crate) mod public_repair_targets;
pub(crate) mod public_route_guidance;
pub mod query;
pub mod read_model;
pub mod recording;
pub mod reducer;
pub mod reentry_reconcile;
pub(crate) mod repair_route_decision;
pub(crate) mod repair_target_selection;
pub(crate) mod resume_stale_precedence;
pub(crate) mod review_route_tokens;
pub mod review_state;
pub(crate) mod route_plan;
pub mod router;
pub mod runtime;
pub mod runtime_provenance;
pub(crate) mod runtime_truth;
pub mod semantic_identity;
pub(crate) mod stale_target_projection;
pub(crate) mod stale_target_selection;
pub mod state;
pub mod status;
pub(crate) mod status_assembly;
pub(crate) mod status_support;
pub(crate) mod task_scope_key;
pub mod topology;
pub mod transitions;

pub(crate) fn workflow_operator_requery_command(
    _plan: &Path,
    external_review_result_ready: bool,
) -> String {
    public_route_guidance::workflow_operator_json_display_command(external_review_result_ready)
        .to_owned()
}
