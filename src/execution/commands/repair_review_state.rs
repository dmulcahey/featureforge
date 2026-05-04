use super::common::*;

use crate::execution::review_state::RepairReviewStateOutput;

pub fn repair_review_state(
    runtime: &ExecutionRuntime,
    args: &StatusArgs,
) -> Result<RepairReviewStateOutput, JsonFailure> {
    crate::execution::review_state::repair_review_state_command(runtime, args)
}
