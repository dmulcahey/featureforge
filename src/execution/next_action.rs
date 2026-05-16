//! Public next-action display vocabulary and semantic decision facade.
//!
//! Ordered route selection lives in `route_plan::next_action_choice`; this
//! facade exposes only the stable decision types and display helpers consumed by
//! read-model and presentation code.

pub(crate) use crate::execution::route_plan::next_action_choice::{
    NEXT_ACTION_ADVANCE_LATE_STAGE, NEXT_ACTION_CLOSE_CURRENT_TASK,
    NEXT_ACTION_EXECUTION_REENTRY_REQUIRED, NEXT_ACTION_HANDOFF, NEXT_ACTION_PLANNING_REENTRY,
    NEXT_ACTION_REPAIR_REVIEW_STATE, NEXT_ACTION_REQUEST_FINAL_REVIEW,
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED, NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
    NextActionDecision, NextActionKind, diagnostic_next_action_for_route, public_next_action_text,
    runtime_route_is_diagnostic,
};

pub const PUBLIC_NEXT_ACTION_VALUES: &[&str] =
    crate::execution::route_plan::next_action_choice::PUBLIC_NEXT_ACTION_VALUES;
