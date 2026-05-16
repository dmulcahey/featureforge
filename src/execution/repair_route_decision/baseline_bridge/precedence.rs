use crate::execution::repair_target_selection::NextActionAuthorityInputs;
use crate::execution::resume_stale_precedence::{
    ResumeStalePrecedence, ResumeStalePrecedenceInputs, StalePreemptionTarget,
};
use crate::execution::stale_target_projection::AuthoritativeStaleTargetSource;
use crate::execution::stale_target_selection::StaleBoundaryCandidate;
use crate::execution::state::PlanExecutionStatus;

pub(super) fn baseline_bridge_authority_precedence(
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> ResumeStalePrecedence {
    let stale_target = authority_inputs.authoritative_stale_target;
    baseline_bridge_precedence(
        status,
        stale_target.map(|target| {
            StaleBoundaryCandidate::from_authoritative_stale_target(target.task, target.source)
        }),
        stale_target.map(|target| StalePreemptionTarget {
            task: target.task,
            step: target.step,
        }),
    )
}

pub(crate) fn baseline_bridge_reducer_precedence(
    status: &PlanExecutionStatus,
    reducer_stale_target: Option<u32>,
) -> ResumeStalePrecedence {
    baseline_bridge_precedence(
        status,
        reducer_stale_target.map(|task| {
            StaleBoundaryCandidate::from_authoritative_stale_target(
                task,
                AuthoritativeStaleTargetSource::ClosureGraph,
            )
        }),
        reducer_stale_target.map(|task| StalePreemptionTarget { task, step: None }),
    )
}

fn baseline_bridge_precedence(
    status: &PlanExecutionStatus,
    authoritative_stale_boundary: Option<StaleBoundaryCandidate>,
    stale_preemption_target: Option<StalePreemptionTarget>,
) -> ResumeStalePrecedence {
    ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
        status,
        review_state_status: status.review_state_status.as_str(),
        open_step_task: None,
        authoritative_stale_boundary,
        baseline_stale_boundary_task: None,
        exact_resume_stale_task_target: authoritative_stale_boundary
            .map(StaleBoundaryCandidate::task),
        stale_preemption_target,
        legal_resume_begin_route: false,
        targetless_stale_has_concrete_public_target: true,
    })
}
