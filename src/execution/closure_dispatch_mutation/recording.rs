use std::path::PathBuf;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::authority::ensure_preflight_authoritative_bootstrap_with_existing_authority;
use crate::execution::closure_dispatch::{
    ReviewDispatchCycleTarget, current_review_dispatch_id_from_lineage,
    current_review_dispatch_id_if_still_current, expected_dispatch_id_mismatch_error,
    review_dispatch_cycle_target, validate_expected_dispatch_id, validate_review_dispatch_request,
};
use crate::execution::context::{ExecutionContext, load_execution_context_for_exact_plan};
use crate::execution::harness::RunIdentitySnapshot;
use crate::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use crate::execution::read_model::usable_current_branch_closure_identity_from_authoritative_state;
use crate::execution::topology::persist_preflight_acceptance;
use crate::execution::transitions::{
    claim_step_write_authority, load_authoritative_transition_state,
};

use super::ReviewDispatchMutationAction;

pub(crate) fn ensure_current_review_dispatch_id(
    context: &ExecutionContext,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    expected_dispatch_id: Option<&str>,
) -> Result<String, JsonFailure> {
    ensure_current_review_dispatch_id_for_command(
        context,
        scope,
        task,
        expected_dispatch_id,
        "record_review_dispatch",
    )
}

pub(crate) fn ensure_current_review_dispatch_id_for_command(
    context: &ExecutionContext,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    expected_dispatch_id: Option<&str>,
    command_name: &'static str,
) -> Result<String, JsonFailure> {
    let args = RecordReviewDispatchArgs {
        plan: PathBuf::from(context.plan_rel.clone()),
        scope,
        task,
    };
    let cycle_target = review_dispatch_cycle_target(context);
    validate_review_dispatch_request(context, &args, cycle_target)?;
    if let Some(dispatch_id) = current_review_dispatch_id_if_still_current(context, &args)? {
        validate_expected_dispatch_id(&dispatch_id, expected_dispatch_id, scope, task)?;
        return Ok(dispatch_id);
    }
    if let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Err(expected_dispatch_id_mismatch_error(
            expected_dispatch_id,
            scope,
            task,
        ));
    }
    ensure_review_dispatch_authoritative_bootstrap(context)?;
    let action = record_review_dispatch_strategy_checkpoint_for_command(
        context,
        &args,
        cycle_target,
        command_name,
    )?;
    let refreshed = load_execution_context_for_exact_plan(&context.runtime, &args.plan)?;
    let dispatch_id = match action {
        ReviewDispatchMutationAction::Recorded => {
            current_review_dispatch_id_from_lineage(&refreshed, &args)?.or(match args.scope {
                ReviewDispatchScopeArg::FinalReview => {
                    just_recorded_final_review_dispatch_id_from_authority(&refreshed)?
                }
                ReviewDispatchScopeArg::Task => None,
            })
        }
        ReviewDispatchMutationAction::AlreadyCurrent => {
            current_review_dispatch_id_if_still_current(&refreshed, &args)?
        }
    }
    .ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "review-dispatch lineage binding did not yield a current dispatch id.",
        )
    })?;
    validate_expected_dispatch_id(&dispatch_id, expected_dispatch_id, scope, task)?;
    Ok(dispatch_id)
}

fn just_recorded_final_review_dispatch_id_from_authority(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(None);
    };
    let current_branch_closure_id =
        usable_current_branch_closure_identity_from_authoritative_state(
            context,
            Some(authoritative_state),
        )
        .map(|identity| identity.branch_closure_id);
    if authoritative_state.current_final_review_dispatch_lineage_branch_closure_id()
        != current_branch_closure_id.as_deref()
    {
        return Ok(None);
    }
    Ok(authoritative_state
        .current_final_review_dispatch_lineage_dispatch_id()
        .or_else(|| authoritative_state.current_final_review_dispatch_id())
        .map(str::trim)
        .filter(|dispatch_id| !dispatch_id.is_empty())
        .map(ToOwned::to_owned))
}

pub(crate) fn ensure_review_dispatch_authoritative_bootstrap(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    if load_authoritative_transition_state(context)?
        .as_ref()
        .is_some_and(|state| state.execution_run_id_opt().is_some())
    {
        return Ok(());
    }
    let acceptance = persist_preflight_acceptance(context)?;
    ensure_preflight_authoritative_bootstrap_with_existing_authority(
        &context.runtime,
        RunIdentitySnapshot {
            execution_run_id: acceptance.execution_run_id.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
        },
        acceptance.chunk_id,
    )
}

pub(crate) fn record_review_dispatch_strategy_checkpoint(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<ReviewDispatchMutationAction, JsonFailure> {
    record_review_dispatch_strategy_checkpoint_for_command(
        context,
        args,
        cycle_target,
        "record_review_dispatch",
    )
}

pub(crate) fn record_review_dispatch_strategy_checkpoint_for_command(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
    command_name: &'static str,
) -> Result<ReviewDispatchMutationAction, JsonFailure> {
    let _ = load_authoritative_transition_state(context)?;
    let _write_authority = claim_step_write_authority(&context.runtime)?;
    record_review_dispatch_strategy_checkpoint_without_claim(
        context,
        args,
        cycle_target,
        command_name,
    )
}

fn record_review_dispatch_strategy_checkpoint_without_claim(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
    command_name: &'static str,
) -> Result<ReviewDispatchMutationAction, JsonFailure> {
    if current_review_dispatch_id_if_still_current(context, args)?.is_some() {
        return Ok(ReviewDispatchMutationAction::AlreadyCurrent);
    }
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "authoritative harness state is required before review-dispatch proof can be recorded.",
        ));
    };
    let cycle_target = match cycle_target {
        ReviewDispatchCycleTarget::Bound(_, _)
            if matches!(args.scope, ReviewDispatchScopeArg::FinalReview) =>
        {
            None
        }
        ReviewDispatchCycleTarget::Bound(task, step) => Some((task, step)),
        ReviewDispatchCycleTarget::UnboundCompletedPlan => None,
        ReviewDispatchCycleTarget::None => return Ok(ReviewDispatchMutationAction::AlreadyCurrent),
    };
    authoritative_state.record_review_dispatch_strategy_checkpoint(
        context,
        &context.plan_document.execution_mode,
        cycle_target,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint_and_command(None, command_name)?;
    Ok(ReviewDispatchMutationAction::Recorded)
}
