//! Narrow command-facing read boundary.
//!
//! Commands use this facade when they need to re-read authoritative runtime
//! state for validation or post-mutation output. Keeping the imports here
//! prevents command modules from depending on `state.rs` compatibility
//! re-exports of read-model internals.

pub(crate) use crate::execution::read_model::{
    branch_closure_record_matches_plan_exemption, load_execution_read_scope_for_mutation,
    public_status_from_context_with_shared_routing,
    public_status_from_supplied_context_with_shared_routing,
    usable_current_branch_closure_identity,
};
pub(crate) use crate::execution::read_model_support::{
    require_prior_task_closure_for_begin, still_current_task_closure_records,
    structural_current_task_closure_failures,
    task_closure_negative_result_blocks_current_reviewed_state,
    task_completion_lineage_fingerprint,
};
