use crate::execution::closure_diagnostics::public_task_boundary_decision;
use crate::execution::command_eligibility::{PublicAdvanceLateStageMode, PublicCommand};
use crate::execution::current_truth::{
    BranchRerecordingAssessment, BranchRerecordingUnsupportedReason,
    branch_closure_rerecording_assessment_with_authority,
    handoff_decision_scope as shared_handoff_decision_scope,
    late_stage_missing_task_closure_baseline_bridge_supported,
    negative_result_requires_execution_reentry, resolve_actionable_repair_follow_up_for_status,
    task_boundary_block_reason_code, task_review_result_pending_task,
    task_review_result_requires_verification_reason_codes,
    worktree_drift_escapes_late_stage_surface,
};
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::late_stage_route_selection::{
    LateStageRouteInputs, late_stage_decision, select_late_stage_public_route,
};
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_PHASE_DETAIL, TargetlessStaleReconcile,
};
pub(crate) use crate::execution::repair_target_selection::{
    AuthoritativeStaleReentryTarget, NextActionAuthorityInputs, execution_reentry_target,
    select_authoritative_stale_reentry_target, task_boundary_blocking_task,
    task_closure_baseline_reentry_target,
};
use crate::execution::state::{
    ExecutionCommandRouteTarget, ExecutionContext, PlanExecutionStatus,
    closure_baseline_candidate_task, execution_reentry_requires_review_state_repair,
    prerelease_branch_closure_refresh_required, recommended_execution_source,
    reopen_execution_command_route_target_for_task, resolve_execution_command_route_target,
    task_closure_baseline_bridge_ready_for_stale_target,
    task_closure_baseline_candidate_can_preempt_stale_target,
    task_closure_baseline_repair_candidate_with_stale_target,
    task_closures_are_non_branch_contributing, task_scope_structural_review_state_reason,
};
use crate::execution::transitions::load_authoritative_transition_state;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NextActionKind {
    Begin,
    Resume,
    Reopen,
    CloseCurrentTask,
    WaitForTaskReviewResult,
    AdvanceLateStage,
    RequestFinalReview,
    WaitForFinalReviewResult,
    RefreshTestPlan,
    RunQa,
    FinishBranch,
    RepairReviewState,
    PlanningReentry,
    Handoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NextActionDecision {
    pub kind: NextActionKind,
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub task_number: Option<u32>,
    pub step_number: Option<u32>,
    pub blocking_task: Option<u32>,
    pub blocking_reason_codes: Vec<String>,
    pub recommended_public_command: Option<PublicCommand>,
}

#[derive(Clone, Copy)]
pub(crate) struct NextActionRequestInputs<'a> {
    pub(crate) plan_path: &'a str,
    pub(crate) external_review_result_ready: bool,
    pub(crate) task_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_lineage_present: bool,
}

pub(crate) fn repair_review_state_public_command(plan_path: &str) -> PublicCommand {
    PublicCommand::RepairReviewState {
        plan: plan_path.to_owned(),
    }
}

pub(crate) fn close_current_task_public_command(
    plan_path: &str,
    task_number: u32,
) -> PublicCommand {
    PublicCommand::CloseCurrentTask {
        plan: plan_path.to_owned(),
        task: Some(task_number),
        result_inputs_required: true,
    }
}

pub(crate) fn transfer_handoff_public_command(plan_path: &str, scope: &str) -> PublicCommand {
    PublicCommand::TransferHandoff {
        plan: plan_path.to_owned(),
        scope: scope.to_owned(),
    }
}

pub(crate) fn advance_late_stage_public_command(
    plan_path: &str,
    mode: PublicAdvanceLateStageMode,
) -> PublicCommand {
    PublicCommand::AdvanceLateStage {
        plan: plan_path.to_owned(),
        mode,
    }
}

fn begin_public_command(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    execution_mode: Option<&str>,
    fingerprint: &str,
) -> PublicCommand {
    PublicCommand::Begin {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        execution_mode: execution_mode.map(str::to_owned),
        fingerprint: Some(fingerprint.to_owned()),
    }
}

fn complete_public_command(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    source: &str,
    fingerprint: &str,
) -> PublicCommand {
    PublicCommand::Complete {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        source: Some(source.to_owned()),
        fingerprint: Some(fingerprint.to_owned()),
    }
}

pub(crate) fn reopen_public_command(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    source: &str,
    fingerprint: &str,
) -> PublicCommand {
    PublicCommand::Reopen {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        source: Some(source.to_owned()),
        reason: Some(runtime_routed_reopen_reason(task_number, step_number)),
        fingerprint: Some(fingerprint.to_owned()),
    }
}

pub(crate) fn reopen_public_command_with_reason(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    source: &str,
    reason: &str,
    fingerprint: Option<&str>,
) -> PublicCommand {
    PublicCommand::Reopen {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        source: Some(source.to_owned()),
        reason: Some(reason.to_owned()),
        fingerprint: fingerprint.map(str::to_owned),
    }
}

fn runtime_routed_reopen_reason(task_number: u32, step_number: u32) -> String {
    format!("runtime-routed-execution-reentry-task-{task_number}-step-{step_number}")
}

pub(crate) fn public_next_action_text(decision: &NextActionDecision) -> String {
    match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume | NextActionKind::Reopen => {
            let negative_result_reentry = decision
                .blocking_reason_codes
                .iter()
                .any(|reason_code| reason_code == "negative_result_requires_execution_reentry");
            let structural_task_repair_lane =
                decision.blocking_reason_codes.iter().any(|reason_code| {
                    matches!(
                        reason_code.as_str(),
                        "prior_task_current_closure_invalid"
                            | "prior_task_current_closure_reviewed_state_malformed"
                    ) || (reason_code == "prior_task_current_closure_stale"
                        && !negative_result_reentry)
                });
            let stale_unreviewed_repair_lane = decision.review_state_status == "stale_unreviewed"
                && decision
                    .recommended_public_command
                    .as_ref()
                    .is_none_or(|command| {
                        matches!(command, PublicCommand::RepairReviewState { .. })
                    });
            if decision.phase == crate::execution::phase::PHASE_EXECUTION_PREFLIGHT
                || decision.phase_detail
                    == crate::execution::phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED
            {
                String::from("continue execution")
            } else if decision.phase_detail
                == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                && (structural_task_repair_lane || stale_unreviewed_repair_lane)
            {
                String::from("repair review state / reenter execution")
            } else if decision.phase_detail
                == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            {
                String::from("execution reentry required")
            } else {
                String::from("continue execution")
            }
        }
        NextActionKind::CloseCurrentTask => {
            if decision.phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            {
                String::from("close current task")
            } else {
                String::from("continue execution")
            }
        }
        NextActionKind::WaitForTaskReviewResult => {
            if task_review_result_requires_verification_reason_codes(
                decision.blocking_reason_codes.iter().map(String::as_str),
            ) {
                String::from("run verification")
            } else {
                String::from("wait for external review result")
            }
        }
        NextActionKind::AdvanceLateStage => {
            if decision.phase_detail
                == crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
            {
                String::from("resolve release blocker")
            } else {
                String::from("advance late stage")
            }
        }
        NextActionKind::RequestFinalReview => String::from("request final review"),
        NextActionKind::WaitForFinalReviewResult => String::from("wait for external review result"),
        NextActionKind::RefreshTestPlan => String::from("refresh test plan"),
        NextActionKind::RunQa => String::from("run QA"),
        NextActionKind::FinishBranch => String::from("finish branch"),
        NextActionKind::RepairReviewState => {
            String::from("repair review state / reenter execution")
        }
        NextActionKind::PlanningReentry => String::from("pivot / return to planning"),
        NextActionKind::Handoff => String::from("hand off"),
    }
}

pub(crate) fn compute_next_action_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<NextActionDecision> {
    compute_next_action_decision_with_inputs(context, status, plan_path, false, None, None, false)
}

pub(crate) fn compute_next_action_decision_with_inputs(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    external_review_result_ready: bool,
    task_review_dispatch_id: Option<&str>,
    final_review_dispatch_id: Option<&str>,
    final_review_dispatch_lineage_present: bool,
) -> Option<NextActionDecision> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    let persisted_repair_follow_up = resolve_actionable_repair_follow_up_for_status(
        context,
        status,
        authoritative_state.as_ref(),
    )
    .map(|record| record.kind.public_token().to_owned());
    let branch_rerecording_assessment =
        branch_closure_rerecording_assessment_with_authority(context, authoritative_state.as_ref())
            .ok();
    compute_next_action_decision_with_authority_inputs(
        context,
        status,
        NextActionRequestInputs {
            plan_path,
            external_review_result_ready,
            task_review_dispatch_id,
            final_review_dispatch_id,
            final_review_dispatch_lineage_present,
        },
        NextActionAuthorityInputs {
            persisted_repair_follow_up: persisted_repair_follow_up.as_deref(),
            branch_rerecording_assessment: branch_rerecording_assessment.as_ref(),
            ..NextActionAuthorityInputs::default()
        },
    )
}

pub(crate) fn compute_next_action_decision_with_authority_inputs(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    request_inputs: NextActionRequestInputs<'_>,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    let NextActionRequestInputs {
        plan_path,
        external_review_result_ready,
        task_review_dispatch_id,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
    } = request_inputs;
    let review_state_status = canonical_review_state_status(status);
    if review_state_status == "stale_unreviewed"
        && !status.stale_unreviewed_closures.is_empty()
        && authority_inputs.authoritative_stale_target.is_none()
        && !authority_inputs.has_authoritative_stale_target
    {
        return Some(missing_execution_reentry_target_decision(
            status,
            review_state_status.as_str(),
        ));
    }
    // Step 4.1 (ordered pass #1): hard structural corruption must route to repair before any
    // open-step, stale-boundary, or closure-prerequisite routing is attempted.
    if hard_structural_corruption_detected(status) {
        if review_state_status == "stale_unreviewed"
            && let Some(stale_task) = authority_inputs.earliest_stale_task()
            && Some(stale_task) != status.blocking_task
        {
            return Some(execution_reentry_decision_for_task(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
                stale_task,
                true,
            ));
        }
        if let Some(task_number) =
            execution_reentry_blocking_task(context, status, authority_inputs)
        {
            return Some(execution_reentry_decision_for_task(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
                task_number,
                false,
            ));
        }
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status.as_str(),
            authority_inputs,
        ));
    }
    if review_state_status == "clean" && completed_execution_missing_branch_closure(status, context)
    {
        return Some(late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        ));
    }
    let persisted_follow_up = authority_inputs
        .persisted_repair_follow_up
        .map(str::to_owned);
    if persisted_follow_up.as_deref() == Some("close_current_task")
        && status.current_branch_closure_id.is_none()
        && status.current_task_closures.is_empty()
        && let Some(task_number) = closure_baseline_candidate_task(context)
        && authority_inputs.stale_target_allows_task_closure_bridge_for_task(task_number)
    {
        return Some(task_closure_recording_ready_decision(
            status,
            plan_path,
            task_number,
        ));
    }
    let persisted_branch_rerecording_follow_up = status.current_branch_closure_id.is_some()
        && persisted_follow_up.as_deref() == Some("advance_late_stage");
    if (review_state_status != "stale_unreviewed"
        || persisted_follow_up.as_deref() == Some("advance_late_stage"))
        && (status.current_branch_meaningful_drift || persisted_branch_rerecording_follow_up)
        && let Some(assessment) = authority_inputs.branch_rerecording_assessment
    {
        let branch_review_state_status = if review_state_status == "clean" {
            "missing_current_closure"
        } else {
            review_state_status.as_str()
        };
        return Some(late_stage_missing_current_closure_decision_from_assessment(
            context,
            status,
            plan_path,
            branch_review_state_status,
            assessment,
            authority_inputs,
        ));
    }
    let handoff_route_active = status.handoff_required
        || status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::phase::PHASE_HANDOFF_REQUIRED);
    let authoritative_stale_task = authority_inputs.earliest_stale_task();
    let baseline_reentry_target = (!handoff_route_active)
        .then(|| task_closure_baseline_reentry_target(context, status, authority_inputs))
        .flatten()
        .filter(|target| {
            task_closure_baseline_candidate_can_preempt_stale_target(
                status,
                target.task,
                authoritative_stale_task,
            ) && authority_inputs.stale_target_allows_task_closure_bridge_for_task(target.task)
        });
    let earliest_stale_boundary = if handoff_route_active {
        None
    } else {
        authoritative_stale_task
            .into_iter()
            .chain(baseline_reentry_target.as_ref().map(|target| target.task))
            .min()
    };
    let open_step_task = status.active_task.or(status.resume_task).or(status
        .blocking_task
        .filter(|_| status.blocking_step.is_some()));
    let open_step_preempted_by_earlier_stale = open_step_task
        .is_some_and(|task| earliest_stale_boundary.is_some_and(|earliest| earliest < task));
    let open_step_matches_earliest_stale_boundary =
        earliest_stale_boundary.is_none_or(|earliest_task| {
            open_step_task.is_some_and(|open_task| open_task == earliest_task)
        });
    let open_step_preempted_by_execution_reentry_blocker =
        execution_reentry_blocking_task(context, status, authority_inputs)
            .is_some_and(|blocking_task| Some(blocking_task) != open_step_task)
            || (status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                && status.blocking_step.is_none()
                && status
                    .blocking_task
                    .is_some_and(|blocking_task| Some(blocking_task) != open_step_task))
            || (review_state_status == "stale_unreviewed"
                && !open_step_matches_earliest_stale_boundary);
    let open_step_preempted_by_closure_recording_ready =
        open_step_task.is_some_and(|task_number| {
            missing_current_closure_allows_task_closure_baseline_route(
                context,
                status,
                authority_inputs,
                review_state_status.as_str(),
            ) && status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == "prior_task_current_closure_missing")
                && stale_unreviewed_bridge_ready_for_task(
                    context,
                    status,
                    authority_inputs,
                    review_state_status.as_str(),
                    task_number,
                )
        });

    // Step 4.1 (ordered pass #2): authoritative open-step state, unless an earlier stale boundary wins.
    if open_step_task.is_some()
        && !open_step_preempted_by_earlier_stale
        && !open_step_preempted_by_execution_reentry_blocker
        && !open_step_preempted_by_closure_recording_ready
        && !task_scope_pivot_override_active(status, review_state_status.as_str())
    {
        let stale_boundary_open_step_resume_allowed = earliest_stale_boundary
            .is_some_and(|earliest_task| open_step_task == Some(earliest_task))
            && review_state_status == "stale_unreviewed"
            && status.blocking_task.is_none()
            && task_scope_structural_review_state_reason(status).is_none();
        if execution_reentry_requires_review_state_repair(Some(context), status)
            && !stale_boundary_open_step_resume_allowed
            && !late_stage_negative_result_reroute(status, review_state_status.as_str())
            && !status.reason_codes.is_empty()
        {
            return Some(execution_repair_decision(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
                authority_inputs,
            ));
        }
        if let Some(route_target_decision) = decision_from_execution_command_route_target(
            status,
            plan_path,
            resolve_execution_command_route_target(status, plan_path),
        ) {
            return Some(route_target_decision);
        }
    }

    // Step 4.1 (ordered pass #3): earliest unresolved stale task-closure boundary.
    if let Some(stale_task) = earliest_stale_boundary {
        let missing_current_task_closure_recording_ready = review_state_status
            != "stale_unreviewed"
            && status.current_branch_closure_id.is_none()
            && status.current_task_closures.is_empty()
            && status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == "prior_task_current_closure_missing");
        let stale_boundary_closure_recording_ready =
            task_scope_structural_review_state_reason(status).is_none()
                && (baseline_reentry_target.as_ref().is_some_and(|target| {
                    target.task == stale_task
                        && review_state_status == "clean"
                        && status.current_task_closures.is_empty()
                        && !status
                            .reason_codes
                            .iter()
                            .any(|reason_code| reason_code == "prior_task_current_closure_stale")
                }) || missing_current_task_closure_recording_ready
                    || (task_closure_baseline_bridge_ready_for_stale_target(
                        context,
                        status,
                        stale_task,
                        authority_inputs.earliest_stale_task(),
                    )
                    .unwrap_or(false)
                        && authority_inputs
                            .stale_target_allows_task_closure_bridge_for_task(stale_task)
                        && missing_current_closure_allows_task_closure_baseline_route(
                            context,
                            status,
                            authority_inputs,
                            review_state_status.as_str(),
                        )
                        && stale_unreviewed_bridge_ready_for_task(
                            context,
                            status,
                            authority_inputs,
                            review_state_status.as_str(),
                            stale_task,
                        )));
        if stale_boundary_closure_recording_ready {
            return Some(task_closure_recording_ready_decision(
                status, plan_path, stale_task,
            ));
        }
        if authority_inputs.stale_target_is_baseline_bridge() {
            return Some(missing_execution_reentry_target_decision(
                status,
                review_state_status.as_str(),
            ));
        }
        return Some(execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            review_state_status.as_str(),
            stale_task,
            true,
        ));
    }

    // Step 4.1 (ordered pass #4): current-task closure recording prerequisites.
    if review_state_status != "stale_unreviewed"
        && execution_reentry_requires_review_state_repair(Some(context), status)
        && !late_stage_negative_result_reroute(status, review_state_status.as_str())
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status.as_str(),
            authority_inputs,
        ));
    }
    if status.harness_phase == HarnessPhase::Executing
        && review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
        && status.phase_detail != crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status.as_str(),
            authority_inputs,
        ));
    }
    if let Some(decision) = late_stage_execution_reentry_decision(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
    ) {
        return Some(decision);
    }
    if let Some(task_number) = task_review_result_pending_task(status, task_review_dispatch_id) {
        let closure_baseline_candidate = task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            task_number,
            authority_inputs.earliest_stale_task(),
        )
        .ok()
        .flatten();
        if task_review_pending_allows_closure_baseline_route(status)
            && missing_current_closure_allows_task_closure_baseline_route(
                context,
                status,
                authority_inputs,
                review_state_status.as_str(),
            )
            && closure_baseline_candidate.is_some()
        {
            return Some(task_closure_recording_ready_decision(
                status,
                plan_path,
                task_number,
            ));
        }
        if external_review_result_ready && external_review_ready_promotes_closure_recording(status)
        {
            return Some(task_closure_recording_ready_decision(
                status,
                plan_path,
                task_number,
            ));
        }
        return Some(closure_prerequisite_decision(
            status,
            NextActionKind::WaitForTaskReviewResult,
            crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING,
            Some(task_number),
            None,
        ));
    }
    if let Some(task_number) = status
        .blocking_task
        .filter(|task_number| {
            status.blocking_step.is_none()
                || blocking_step_allows_task_closure_baseline_route(status, *task_number)
        })
        .filter(|_| {
            matches!(
                status.harness_phase,
                HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
            )
        })
        .filter(|_| {
            stale_provenance_allows_task_closure_baseline_route(
                context,
                status,
                authority_inputs,
                review_state_status.as_str(),
            )
        })
        .filter(|task_number| {
            review_state_status != "stale_unreviewed"
                || stale_unreviewed_bridge_ready_for_task(
                    context,
                    status,
                    authority_inputs,
                    review_state_status.as_str(),
                    *task_number,
                )
        })
        && let Ok(Some(_candidate)) = task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            task_number,
            authority_inputs.earliest_stale_task(),
        )
    {
        return Some(task_closure_recording_ready_decision(
            status,
            plan_path,
            task_number,
        ));
    }
    if let Some(task_number) = status
        .blocking_task
        .filter(|task_number| {
            status.blocking_step.is_none()
                || blocking_step_allows_task_closure_baseline_route(status, *task_number)
        })
        .filter(|_| {
            matches!(
                status.harness_phase,
                HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
            )
        })
        .filter(|_| {
            stale_provenance_allows_task_closure_baseline_route(
                context,
                status,
                authority_inputs,
                review_state_status.as_str(),
            )
        })
        .filter(|task_number| {
            review_state_status != "stale_unreviewed"
                || stale_unreviewed_bridge_ready_for_task(
                    context,
                    status,
                    authority_inputs,
                    review_state_status.as_str(),
                    *task_number,
                )
        })
        .filter(|task_number| {
            task_closure_baseline_repair_candidate_with_stale_target(
                context,
                status,
                *task_number,
                authority_inputs.earliest_stale_task(),
            )
            .ok()
            .flatten()
            .is_some()
        })
        .filter(|_| {
            status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == "prior_task_current_closure_missing")
                && status
                    .reason_codes
                    .iter()
                    .any(|reason_code| reason_code == "task_closure_baseline_repair_candidate")
        })
    {
        return Some(task_closure_recording_ready_decision(
            status,
            plan_path,
            task_number,
        ));
    }
    if external_review_result_ready
        && external_review_ready_promotes_closure_recording(status)
        && (status.blocking_step.is_none()
            || status.blocking_task.is_some_and(|task_number| {
                blocking_step_allows_task_closure_baseline_route(status, task_number)
            }))
        && matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        )
        && let Some(task_number) = status.blocking_task
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "task_closure_baseline_repair_candidate")
        && task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            task_number,
            authority_inputs.earliest_stale_task(),
        )
        .ok()
        .flatten()
        .is_some()
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status.as_str(),
        )
    {
        return Some(task_closure_recording_ready_decision(
            status,
            plan_path,
            task_number,
        ));
    }
    if status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        )
        && status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "task_closure_baseline_repair_candidate"
                    | "current_task_closure_overlay_restore_required"
                    | "task_closure_negative_result_overlay_restore_required"
            )
        })
        && let Some(candidate_task) = closure_baseline_candidate_task(context)
        && let Ok(Some(_candidate)) = task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            candidate_task,
            authority_inputs.earliest_stale_task(),
        )
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status.as_str(),
        )
    {
        return Some(task_closure_recording_ready_decision(
            status,
            plan_path,
            candidate_task,
        ));
    }
    if status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        )
        && review_state_status == "clean"
        && closure_baseline_routing_reason_codes_compatible(status)
        && let Some(candidate_task) = closure_baseline_candidate_task(context)
        && let Ok(Some(_candidate)) = task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            candidate_task,
            authority_inputs.earliest_stale_task(),
        )
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status.as_str(),
        )
    {
        return Some(task_closure_recording_ready_decision(
            status,
            plan_path,
            candidate_task,
        ));
    }
    if let Some(task_number) = execution_reentry_blocking_task(context, status, authority_inputs) {
        return Some(execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            review_state_status.as_str(),
            task_number,
            false,
        ));
    }
    // Step 4.1 (ordered pass #5): late-stage milestones.
    if task_scope_handoff_override_active(status) {
        return Some(task_scope_handoff_decision(
            status,
            plan_path,
            review_state_status.as_str(),
        ));
    }
    if persisted_late_stage_reroute_missing_current_closure(
        context,
        status,
        authority_inputs,
        review_state_status.as_str(),
    ) {
        return Some(late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        ));
    }
    if task_scope_pivot_override_active(status, review_state_status.as_str()) {
        let recommended_public_command = (status.blocking_task.is_some()
            || status.active_task.is_some()
            || status.resume_task.is_some()
            || !status.reason_codes.is_empty())
        .then(|| repair_review_state_public_command(plan_path));
        return Some(NextActionDecision {
            kind: NextActionKind::PlanningReentry,
            phase: String::from(crate::execution::phase::PHASE_PIVOT_REQUIRED),
            phase_detail: String::from(crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED),
            review_state_status,
            task_number: status
                .blocking_task
                .or(status.resume_task)
                .or(status.active_task),
            step_number: status
                .blocking_step
                .or(status.resume_step)
                .or(status.active_step),
            blocking_task: status
                .blocking_task
                .or(status.resume_task)
                .or(status.active_task),
            blocking_reason_codes: status.reason_codes.clone(),
            recommended_public_command,
        });
    }
    if review_state_status == "missing_current_closure" {
        if status.phase_detail == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
            || status.harness_phase == HarnessPhase::DocumentReleasePending
        {
            if status.current_branch_closure_id.is_none() {
                if let Some(assessment) = authority_inputs.branch_rerecording_assessment {
                    return Some(late_stage_missing_current_closure_decision_from_assessment(
                        context,
                        status,
                        plan_path,
                        review_state_status.as_str(),
                        assessment,
                        authority_inputs,
                    ));
                }
                return Some(execution_repair_decision(
                    context,
                    status,
                    plan_path,
                    review_state_status.as_str(),
                    authority_inputs,
                ));
            }
            return Some(late_stage_decision(
                status,
                NextActionKind::AdvanceLateStage,
                crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
                plan_path,
            ));
        }
        if status.current_branch_closure_id.is_none()
            && status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        {
            if let Some(assessment) = authority_inputs.branch_rerecording_assessment {
                return Some(late_stage_missing_current_closure_decision_from_assessment(
                    context,
                    status,
                    plan_path,
                    review_state_status.as_str(),
                    assessment,
                    authority_inputs,
                ));
            }
            return Some(execution_repair_decision(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
                authority_inputs,
            ));
        }
        if let Some(task_number) = task_boundary_blocking_task(status) {
            return Some(execution_reentry_decision_for_task(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
                task_number,
                false,
            ));
        }
        let task_scope_structural_blocker = status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
                    | "prior_task_current_closure_stale"
            )
        });
        if task_scope_structural_blocker {
            return Some(execution_repair_decision(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
                authority_inputs,
            ));
        }
        if let Some(assessment) = authority_inputs.branch_rerecording_assessment {
            return Some(late_stage_missing_current_closure_decision_from_assessment(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
                assessment,
                authority_inputs,
            ));
        }
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status.as_str(),
            authority_inputs,
        ));
    }
    if review_state_status == "stale_unreviewed" {
        return Some(stale_late_stage_repair_decision(
            context,
            status,
            plan_path,
            authority_inputs,
        ));
    }
    if let Some(decision) = select_late_stage_public_route(LateStageRouteInputs {
        context,
        status,
        plan_path,
        external_review_result_ready,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
        gate_finish: authority_inputs.gate_finish,
    }) {
        return Some(decision);
    }

    // Step 4.1 (ordered pass #6): first unchecked step begin.
    if let Some(first_unchecked_step) = context.steps.iter().find(|step| {
        !step.checked
            && !status
                .current_task_closures
                .iter()
                .any(|closure| closure.task == step.task_number)
    }) {
        let task_number = first_unchecked_step.task_number;
        let step_number = first_unchecked_step.step_number;
        let unchecked_step_conflicts_with_current_closure = status
            .current_task_closures
            .iter()
            .any(|closure| closure.task == task_number);
        let authoritative_open_step_marker_loss = status.execution_started == "yes"
            && (status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
                || !context.evidence.attempts.is_empty())
            && status.active_task.is_none()
            && status.active_step.is_none()
            && status.resume_task.is_none()
            && status.resume_step.is_none()
            && status.blocking_task.is_none()
            && status.blocking_step.is_none()
            && status.current_task_closures.is_empty();
        let marker_free_preflight_projection = status.execution_mode != "none"
            && status.execution_started == "no"
            && status.active_task.is_none()
            && status.active_step.is_none()
            && status.resume_task.is_none()
            && status.resume_step.is_none()
            && status.blocking_task.is_none()
            && status.blocking_step.is_none()
            && status.current_task_closures.is_empty();
        let marker_free_evidence_projection = marker_free_preflight_projection
            && task_number == 1
            && step_number == 1
            && (context.legacy_open_step_projection_present
                || !context.evidence.attempts.is_empty());
        if authoritative_open_step_marker_loss {
            return Some(execution_repair_decision_for_task(
                status,
                plan_path,
                review_state_status.as_str(),
                task_number,
            ));
        }
        if status.execution_started == "yes"
            && unchecked_step_conflicts_with_current_closure
            && status.active_task.is_none()
            && status.active_step.is_none()
            && status.resume_task.is_none()
            && status.resume_step.is_none()
            && status.blocking_step.is_none()
        {
            return Some(execution_repair_decision_for_task(
                status,
                plan_path,
                review_state_status.as_str(),
                task_number,
            ));
        }
        let recommended_public_command = (!marker_free_evidence_projection).then(|| {
            let execution_mode =
                (status.execution_mode == "none").then_some("featureforge:executing-plans");
            begin_public_command(
                plan_path,
                task_number,
                step_number,
                execution_mode,
                &status.execution_fingerprint,
            )
        });
        let (phase, phase_detail) = if marker_free_preflight_projection {
            (
                String::from(crate::execution::phase::PHASE_EXECUTION_PREFLIGHT),
                String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS),
            )
        } else if status.execution_started == "yes" {
            (
                String::from(crate::execution::phase::PHASE_EXECUTING),
                String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            )
        } else {
            (
                String::from(crate::execution::phase::PHASE_EXECUTION_PREFLIGHT),
                String::from(crate::execution::phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED),
            )
        };
        return Some(NextActionDecision {
            kind: NextActionKind::Begin,
            phase,
            phase_detail,
            review_state_status,
            task_number: Some(task_number),
            step_number: Some(step_number),
            blocking_task: (status.execution_started == "yes"
                || !status.current_task_closures.is_empty())
            .then_some(task_number),
            blocking_reason_codes: status.reason_codes.clone(),
            recommended_public_command,
        });
    }

    Some(late_stage_decision(
        status,
        NextActionKind::PlanningReentry,
        crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED,
        plan_path,
    ))
}

fn execution_repair_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> NextActionDecision {
    if let Some(target) = execution_reentry_target(context, status, plan_path, authority_inputs) {
        return execution_repair_decision_for_task(
            status,
            plan_path,
            review_state_status,
            target.task,
        );
    }
    if missing_execution_reentry_target_requires_reconcile(
        status,
        review_state_status,
        authority_inputs,
    ) {
        return missing_execution_reentry_target_decision(status, review_state_status);
    }
    let mut blocking_reason_codes = status.reason_codes.clone();
    if !blocking_reason_codes
        .iter()
        .any(|reason_code| reason_code == "execution_target_missing")
    {
        blocking_reason_codes.push(String::from("execution_target_missing"));
    }
    NextActionDecision {
        kind: NextActionKind::RepairReviewState,
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: review_state_status.to_owned(),
        task_number: None,
        step_number: None,
        blocking_task: None,
        blocking_reason_codes,
        recommended_public_command: Some(repair_review_state_public_command(plan_path)),
    }
}

fn missing_execution_reentry_target_requires_reconcile(
    status: &PlanExecutionStatus,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> bool {
    !authority_inputs.has_authoritative_stale_target
        && TargetlessStaleReconcile::missing_reentry_target_requires_reconcile(
            status,
            review_state_status,
        )
}

fn missing_execution_reentry_target_decision(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> NextActionDecision {
    let mut blocking_reason_codes = status.reason_codes.clone();
    TargetlessStaleReconcile::ensure_reason_codes(&mut blocking_reason_codes);
    NextActionDecision {
        kind: NextActionKind::RepairReviewState,
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL),
        review_state_status: review_state_status.to_owned(),
        task_number: None,
        step_number: None,
        blocking_task: None,
        blocking_reason_codes,
        recommended_public_command: None,
    }
}

fn execution_repair_decision_for_task(
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    task_number: u32,
) -> NextActionDecision {
    NextActionDecision {
        kind: NextActionKind::RepairReviewState,
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: review_state_status.to_owned(),
        task_number: Some(task_number),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: Some(task_number),
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_public_command: Some(repair_review_state_public_command(plan_path)),
    }
}

fn closure_prerequisite_decision(
    status: &PlanExecutionStatus,
    kind: NextActionKind,
    phase_detail: &str,
    task_number: Option<u32>,
    step_number: Option<u32>,
) -> NextActionDecision {
    let recommended_public_command = match kind {
        NextActionKind::WaitForTaskReviewResult
        | NextActionKind::RequestFinalReview
        | NextActionKind::WaitForFinalReviewResult => None,
        _ => status.recommended_public_command.clone(),
    };
    NextActionDecision {
        kind,
        phase: String::from(crate::execution::phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail: String::from(phase_detail),
        review_state_status: canonical_review_state_status(status),
        task_number,
        step_number,
        blocking_task: task_number.or(status.blocking_task),
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_public_command,
    }
}

fn task_closure_recording_ready_decision(
    status: &PlanExecutionStatus,
    plan_path: &str,
    task_number: u32,
) -> NextActionDecision {
    if status
        .current_task_closures
        .iter()
        .any(|closure| closure.task == task_number)
        && status.current_branch_closure_id.is_none()
    {
        return late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        );
    }
    NextActionDecision {
        kind: NextActionKind::CloseCurrentTask,
        phase: String::from(crate::execution::phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail: String::from(crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY),
        review_state_status: String::from("clean"),
        task_number: Some(task_number),
        step_number: None,
        blocking_task: Some(task_number),
        blocking_reason_codes: public_task_boundary_decision(status).public_reason_codes,
        recommended_public_command: Some(close_current_task_public_command(plan_path, task_number)),
    }
}

fn stale_late_stage_repair_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> NextActionDecision {
    if status.current_branch_closure_id.is_none()
        && let Some(assessment) = authority_inputs.branch_rerecording_assessment
        && assessment.supported
    {
        return late_stage_missing_current_closure_decision_from_assessment(
            context,
            status,
            plan_path,
            "stale_unreviewed",
            assessment,
            authority_inputs,
        );
    }
    let target = execution_reentry_target(context, status, plan_path, authority_inputs);
    if let Some(target) = target.as_ref()
        && status
            .resume_task
            .or(status.active_task)
            .is_none_or(|open_task| target.task < open_task)
        && status.blocking_step.is_none()
    {
        let task_number = target.task;
        let mut reentry_decision = execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            "stale_unreviewed",
            task_number,
            false,
        );
        if !reentry_decision
            .blocking_reason_codes
            .iter()
            .any(|reason| reason == "stale_unreviewed")
        {
            reentry_decision
                .blocking_reason_codes
                .push(String::from("stale_unreviewed"));
        }
        return reentry_decision;
    }
    let mut blocking_reason_codes = status.reason_codes.clone();
    if !blocking_reason_codes
        .iter()
        .any(|reason| reason == "stale_unreviewed")
    {
        blocking_reason_codes.push(String::from("stale_unreviewed"));
    }
    if let Some(target) = target {
        let mut repair_decision =
            execution_repair_decision_for_task(status, plan_path, "stale_unreviewed", target.task);
        if !repair_decision
            .blocking_reason_codes
            .iter()
            .any(|reason| reason == "stale_unreviewed")
        {
            repair_decision
                .blocking_reason_codes
                .push(String::from("stale_unreviewed"));
        }
        return repair_decision;
    }
    if authority_inputs.has_authoritative_stale_target {
        NextActionDecision {
            kind: NextActionKind::RepairReviewState,
            phase: String::from(crate::execution::phase::PHASE_EXECUTING),
            phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: String::from("stale_unreviewed"),
            task_number: None,
            step_number: None,
            blocking_task: None,
            blocking_reason_codes,
            recommended_public_command: Some(repair_review_state_public_command(plan_path)),
        }
    } else {
        TargetlessStaleReconcile::ensure_reason_codes(&mut blocking_reason_codes);
        NextActionDecision {
            kind: NextActionKind::RepairReviewState,
            phase: String::from(crate::execution::phase::PHASE_EXECUTING),
            phase_detail: String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL),
            review_state_status: String::from("stale_unreviewed"),
            task_number: None,
            step_number: None,
            blocking_task: None,
            blocking_reason_codes,
            recommended_public_command: None,
        }
    }
}

fn late_stage_planning_reentry_decision(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> NextActionDecision {
    NextActionDecision {
        kind: NextActionKind::PlanningReentry,
        phase: String::from(crate::execution::phase::PHASE_PIVOT_REQUIRED),
        phase_detail: String::from(crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED),
        review_state_status: review_state_status.to_owned(),
        task_number: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_public_command: None,
    }
}

fn late_stage_missing_current_closure_decision_from_assessment(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    assessment: &BranchRerecordingAssessment,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> NextActionDecision {
    if task_scope_structural_review_state_reason(status).is_some() {
        let structural_review_state_status = canonical_review_state_status(status);
        return execution_repair_decision(
            context,
            status,
            plan_path,
            structural_review_state_status.as_str(),
            authority_inputs,
        );
    }
    if status.current_branch_closure_id.is_none()
        && status.current_task_closures.is_empty()
        && let Some(task_number) = status.blocking_task
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
    {
        return task_closure_recording_ready_decision(status, plan_path, task_number);
    }
    if status.current_branch_closure_id.is_none()
        && status.current_task_closures.is_empty()
        && let Some(task_number) = closure_baseline_candidate_task(context)
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
    {
        return task_closure_recording_ready_decision(status, plan_path, task_number);
    }
    if status.current_branch_closure_id.is_none()
        && status.current_task_closures.is_empty()
        && let Some(task_number) = closure_baseline_candidate_task(context)
        && task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            task_number,
            authority_inputs.earliest_stale_task(),
        )
        .ok()
        .flatten()
        .is_some()
        && missing_current_closure_allows_task_closure_baseline_route(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
    {
        return task_closure_recording_ready_decision(status, plan_path, task_number);
    }
    if status.current_branch_closure_id.is_none() && status.current_task_closures.is_empty() {
        return execution_reentry_target(context, status, plan_path, authority_inputs)
            .map(|target| {
                execution_reentry_decision_for_task(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    target.task,
                    false,
                )
            })
            .unwrap_or_else(|| {
                execution_repair_decision(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    authority_inputs,
                )
            });
    }
    if assessment.supported {
        return late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        );
    }
    match assessment.unsupported_reason {
        Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => {
            late_stage_planning_reentry_decision(status, review_state_status)
        }
        Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => {
            if authority_inputs.persisted_repair_follow_up == Some("advance_late_stage") {
                return late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
                    plan_path,
                );
            }
            if status.current_branch_closure_id.is_none() {
                if task_closures_are_non_branch_contributing(status) {
                    return execution_repair_decision(
                        context,
                        status,
                        plan_path,
                        review_state_status,
                        authority_inputs,
                    );
                }
                if late_stage_missing_task_closure_baseline_bridge_supported(assessment)
                    && let Some(task_number) = closure_baseline_candidate_task(context)
                    && task_closure_baseline_repair_candidate_with_stale_target(
                        context,
                        status,
                        task_number,
                        authority_inputs.earliest_stale_task(),
                    )
                    .ok()
                    .flatten()
                    .is_some()
                {
                    return task_closure_recording_ready_decision(status, plan_path, task_number);
                }
                return execution_reentry_target(context, status, plan_path, authority_inputs)
                    .map(|target| {
                        execution_reentry_decision_for_task(
                            context,
                            status,
                            plan_path,
                            review_state_status,
                            target.task,
                            false,
                        )
                    })
                    .unwrap_or_else(|| {
                        execution_repair_decision(
                            context,
                            status,
                            plan_path,
                            review_state_status,
                            authority_inputs,
                        )
                    });
            }
            execution_repair_decision(
                context,
                status,
                plan_path,
                review_state_status,
                authority_inputs,
            )
        }
        Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
            execution_reentry_target(context, status, plan_path, authority_inputs)
                .map(|target| {
                    execution_reentry_decision_for_task(
                        context,
                        status,
                        plan_path,
                        review_state_status,
                        target.task,
                        false,
                    )
                })
                .unwrap_or_else(|| {
                    late_stage_planning_reentry_decision(status, review_state_status)
                })
        }
    }
}

fn execution_reentry_blocking_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<u32> {
    let review_state_status = canonical_review_state_status(status);
    if persisted_late_stage_reroute_missing_current_closure(
        context,
        status,
        authority_inputs,
        review_state_status.as_str(),
    ) {
        return None;
    }
    let boundary_blocking_task = task_boundary_blocking_task(status);
    if let Some(task_number) = boundary_blocking_task
        && stale_unreviewed_bridge_ready_for_task(
            context,
            status,
            authority_inputs,
            review_state_status.as_str(),
            task_number,
        )
    {
        return None;
    }
    boundary_blocking_task.or_else(|| {
        (status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            && status.harness_phase == HarnessPhase::Executing
            && status.blocking_step.is_none()
            && status.active_task.is_none()
            && status.resume_task.is_none()
            && !(status.review_state_status == "missing_current_closure"
                && status.current_branch_closure_id.is_none()
                && task_closures_are_non_branch_contributing(status))
            && (!status.reason_codes.is_empty() || status.review_state_status != "clean")
            && task_boundary_block_reason_code(status).is_none()
            && !status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == "qa_requirement_missing_or_invalid"))
        .then_some(status.blocking_task)
        .flatten()
    })
}

fn completed_execution_missing_branch_closure(
    status: &PlanExecutionStatus,
    context: &ExecutionContext,
) -> bool {
    status.execution_started == "yes"
        && status.current_branch_closure_id.is_none()
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.blocking_step.is_none()
        && status.current_task_closures.len() >= context.plan_document.tasks.len()
        && !context.plan_document.tasks.is_empty()
}

fn blocking_step_allows_task_closure_baseline_route(
    status: &PlanExecutionStatus,
    task_number: u32,
) -> bool {
    status.blocking_task == Some(task_number)
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
        && status.review_state_status == "clean"
}

fn execution_reentry_decision_for_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    task_number: u32,
    stale_boundary_route: bool,
) -> NextActionDecision {
    let target_task_missing_current_closure_after_repair = status.blocking_task
        == Some(task_number)
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
        && status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_invalid" | "stale_provenance"
            )
        })
        && status
            .current_task_closures
            .iter()
            .all(|closure| closure.task != task_number);
    if execution_reentry_requires_review_state_repair(Some(context), status)
        && !stale_boundary_route
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "late_stage_surface_not_declared")
        && !target_task_missing_current_closure_after_repair
    {
        return execution_repair_decision_for_task(
            status,
            plan_path,
            review_state_status,
            task_number,
        );
    }
    if !stale_boundary_route
        && let Some(route_target) = resolve_execution_command_route_target(status, plan_path)
        && route_target.task_number == task_number
        && let Some(mut route_target_decision) =
            decision_from_execution_command_route_target(status, plan_path, Some(route_target))
    {
        route_target_decision.blocking_task =
            route_target_decision.blocking_task.or(Some(task_number));
        if route_target_decision.phase_detail
            != crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS
        {
            route_target_decision.phase_detail =
                String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
        }
        return route_target_decision;
    }
    if let Some(reopen_command) =
        reopen_execution_command_route_target_for_task(context, status, plan_path, task_number)
        && let Some(step_id) = reopen_command.step_id
    {
        return NextActionDecision {
            kind: NextActionKind::Reopen,
            phase: String::from(crate::execution::phase::PHASE_EXECUTING),
            phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: review_state_status.to_owned(),
            task_number: Some(task_number),
            step_number: Some(step_id),
            blocking_task: Some(task_number),
            blocking_reason_codes: status.reason_codes.clone(),
            recommended_public_command: Some(reopen_public_command(
                plan_path,
                task_number,
                step_id,
                recommended_execution_source(status.execution_mode.as_str()),
                &status.execution_fingerprint,
            )),
        };
    }
    let mut repair_decision =
        execution_repair_decision_for_task(status, plan_path, review_state_status, task_number);
    if stale_boundary_route
        && !repair_decision
            .blocking_reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_stale")
    {
        repair_decision
            .blocking_reason_codes
            .push(String::from("prior_task_current_closure_stale"));
    }
    repair_decision
}

fn decision_from_execution_command_route_target(
    status: &PlanExecutionStatus,
    plan_path: &str,
    route_target: Option<ExecutionCommandRouteTarget>,
) -> Option<NextActionDecision> {
    let route_target = route_target?;
    let kind = match route_target.command_kind {
        "begin" => {
            if status.resume_task == Some(route_target.task_number)
                && status.resume_step == route_target.step_id
            {
                NextActionKind::Resume
            } else {
                NextActionKind::Begin
            }
        }
        "reopen" => NextActionKind::Reopen,
        "complete" => NextActionKind::CloseCurrentTask,
        _ => return None,
    };
    let phase_detail = match route_target.command_kind {
        "complete" => String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS),
        "begin"
            if status.resume_task == Some(route_target.task_number)
                && status.resume_step == route_target.step_id
                && status.harness_phase == HarnessPhase::Executing
                && execution_reentry_requires_review_state_repair(None, status) =>
        {
            String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS)
        }
        "begin"
            if status.blocking_step.is_some()
                && !execution_reentry_requires_review_state_repair(None, status) =>
        {
            String::from(crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS)
        }
        _ => String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
    };
    let phase = if phase_detail == crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS {
        String::from(crate::execution::phase::PHASE_HANDOFF_REQUIRED)
    } else {
        String::from(crate::execution::phase::PHASE_EXECUTING)
    };
    Some(NextActionDecision {
        kind,
        phase,
        phase_detail,
        review_state_status: canonical_review_state_status(status),
        task_number: Some(route_target.task_number),
        step_number: route_target.step_id,
        blocking_task: if route_target.command_kind == "reopen" {
            Some(route_target.task_number)
        } else {
            status.blocking_task
        },
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_public_command: public_command_from_execution_command_route_target(
            status,
            plan_path,
            &route_target,
        ),
    })
}

fn late_stage_execution_reentry_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    let stale_provenance_present = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "stale_provenance");
    let negative_result_reroute = late_stage_negative_result_reroute(status, review_state_status);
    let stale_provenance_reroute = matches!(
        status.harness_phase,
        HarnessPhase::Executing | HarnessPhase::FinalReviewPending
    ) && review_state_status == "clean"
        && status.current_branch_closure_id.is_some()
        && status.blocking_step.is_none()
        && stale_provenance_present;
    if !(negative_result_reroute || stale_provenance_reroute) {
        return None;
    }
    let reentry_target = execution_reentry_target(
        context,
        status,
        plan_path,
        authority_inputs.with_derived_negative_result_reentry(negative_result_reroute),
    );
    Some(
        reentry_target
            .map(|target| {
                execution_reentry_decision_for_task(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    target.task,
                    negative_result_reroute,
                )
            })
            .unwrap_or_else(|| {
                missing_execution_reentry_target_decision(status, review_state_status)
            }),
    )
}

fn late_stage_negative_result_reroute(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> bool {
    review_state_status == "clean"
        && status.current_branch_closure_id.is_some()
        && status.blocking_step.is_none()
        && (status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "negative_result_requires_execution_reentry")
            || negative_result_requires_execution_reentry(
                false,
                crate::execution::phase::PHASE_EXECUTING,
                status.current_branch_closure_id.as_deref(),
                status.current_final_review_branch_closure_id.as_deref(),
                status.current_final_review_result.as_deref(),
                status.current_qa_branch_closure_id.as_deref(),
                status.current_qa_result.as_deref(),
            ))
}

fn stale_unreviewed_bridge_ready_for_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    let reducer_bridge_ready = task_closure_baseline_bridge_ready_for_stale_target(
        context,
        status,
        task_number,
        authority_inputs.earliest_stale_task(),
    )
    .unwrap_or(false);
    if review_state_status == "stale_unreviewed" {
        return reducer_bridge_ready
            && authority_inputs.stale_target_allows_task_closure_bridge_for_task(task_number);
    }
    let Some(candidate) = task_closure_baseline_repair_candidate_with_stale_target(
        context,
        status,
        task_number,
        authority_inputs.earliest_stale_task(),
    )
    .ok()
    .flatten() else {
        return false;
    };
    let unresolved_stale_task_matches = status.blocking_task == Some(task_number)
        && (review_state_status == "stale_unreviewed"
            || !status.stale_unreviewed_closures.is_empty());
    let dispatch_bound_bridge_candidate = candidate
        .dispatch_id
        .as_deref()
        .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty());
    let has_closure_bridge_reason_signals =
        closure_baseline_routing_reason_codes_compatible(status)
            && status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == "prior_task_current_closure_missing");
    let candidate_only_bridge_signal = closure_baseline_routing_reason_codes_compatible(status)
        && authority_inputs.earliest_stale_task().is_none()
        && status.execution_reentry_target_source.as_deref() != Some("closure_graph_stale_target")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "task_closure_baseline_repair_candidate");
    if review_state_status == "clean" {
        return matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        ) && (reducer_bridge_ready
            || has_closure_bridge_reason_signals
            || candidate_only_bridge_signal
            || (unresolved_stale_task_matches && dispatch_bound_bridge_candidate));
    }
    if review_state_status == "missing_current_closure" {
        return reducer_bridge_ready
            || has_closure_bridge_reason_signals
            || candidate_only_bridge_signal
            || (unresolved_stale_task_matches && dispatch_bound_bridge_candidate);
    }
    false
}

fn task_review_pending_allows_closure_baseline_route(status: &PlanExecutionStatus) -> bool {
    closure_baseline_routing_reason_codes_compatible(status)
        && status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing" | "task_closure_baseline_repair_candidate"
            )
        })
}

fn stale_provenance_allows_task_closure_baseline_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> bool {
    if !missing_current_closure_allows_task_closure_baseline_route(
        context,
        status,
        authority_inputs,
        review_state_status,
    ) {
        return false;
    }
    let stale_provenance_present = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "stale_provenance");
    if review_state_status == "clean" || !stale_provenance_present {
        return true;
    }
    true
}

fn missing_current_closure_allows_task_closure_baseline_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> bool {
    let completed_plan_missing_current_branch_closure = status.current_branch_closure_id.is_none()
        && context.steps.iter().all(|step| step.checked)
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && status.blocking_step.is_none();
    if !completed_plan_missing_current_branch_closure
        && (review_state_status != "missing_current_closure"
            || status.current_branch_closure_id.is_some())
    {
        return true;
    }
    if task_closures_are_non_branch_contributing(status) {
        return false;
    }
    authority_inputs
        .branch_rerecording_assessment
        .is_some_and(late_stage_missing_task_closure_baseline_bridge_supported)
}

fn persisted_late_stage_reroute_missing_current_closure(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> bool {
    review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
        && !status.current_task_closures.is_empty()
        && status.blocking_step.is_none()
        && authority_inputs.persisted_repair_follow_up == Some("advance_late_stage")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "stale_provenance")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
        && !worktree_drift_escapes_late_stage_surface(context).unwrap_or(false)
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "late_stage_surface_not_declared")
}

fn external_review_ready_promotes_closure_recording(status: &PlanExecutionStatus) -> bool {
    closure_baseline_routing_reason_codes_compatible(status)
        && !status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_verification_missing"
                    | "prior_task_verification_missing_legacy"
                    | "task_review_not_independent"
                    | "task_review_artifact_malformed"
                    | "task_verification_summary_malformed"
            )
        })
}

fn closure_baseline_routing_reason_codes_compatible(status: &PlanExecutionStatus) -> bool {
    !status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_review_not_green"
                | "prior_task_current_closure_stale"
                | "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "task_cycle_break_active"
        )
    })
}

fn task_scope_handoff_override_active(status: &PlanExecutionStatus) -> bool {
    status.harness_phase == HarnessPhase::HandoffRequired
}

fn task_scope_pivot_override_active(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> bool {
    let blocker_free_prestart_contract_phase = status.execution_started != "yes"
        && matches!(
            status.harness_phase,
            HarnessPhase::ContractDrafting
                | HarnessPhase::ContractPendingApproval
                | HarnessPhase::ContractApproved
                | HarnessPhase::Evaluating
        )
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && status.reason_codes.is_empty();
    if blocker_free_prestart_contract_phase {
        return false;
    }
    if status.execution_started != "yes"
        && !matches!(
            status.harness_phase,
            HarnessPhase::PivotRequired
                | HarnessPhase::ContractDrafting
                | HarnessPhase::ContractPendingApproval
                | HarnessPhase::ContractApproved
                | HarnessPhase::Evaluating
        )
    {
        return false;
    }
    if review_state_status == "stale_unreviewed" {
        return false;
    }
    if status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "stale_provenance")
    {
        return false;
    }
    matches!(
        status.harness_phase,
        HarnessPhase::PivotRequired
            | HarnessPhase::ContractDrafting
            | HarnessPhase::ContractPendingApproval
            | HarnessPhase::ContractApproved
            | HarnessPhase::Evaluating
    )
}

fn task_scope_handoff_decision(
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
) -> NextActionDecision {
    let task_scoped_handoff = status.blocking_task.is_some()
        || status.active_task.is_some()
        || status.resume_task.is_some();
    let handoff_scope = shared_handoff_decision_scope(
        status.active_task,
        status.blocking_task,
        status.resume_task,
        status.handoff_required,
        Some(status.harness_phase),
    )
    .unwrap_or("branch");
    NextActionDecision {
        kind: NextActionKind::Handoff,
        phase: if task_scoped_handoff {
            String::from(crate::execution::phase::PHASE_EXECUTING)
        } else {
            String::from(crate::execution::phase::PHASE_HANDOFF_REQUIRED)
        },
        phase_detail: String::from(crate::execution::phase::DETAIL_HANDOFF_RECORDING_REQUIRED),
        review_state_status: review_state_status.to_owned(),
        task_number: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_public_command: Some(transfer_handoff_public_command(plan_path, handoff_scope)),
    }
}

pub(crate) fn canonical_review_state_status(status: &PlanExecutionStatus) -> String {
    if status.review_state_status != "clean" {
        return status.review_state_status.clone();
    }
    if prerelease_branch_closure_refresh_required(status)
        || status.phase_detail == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
    {
        return String::from("missing_current_closure");
    }
    if status.current_branch_closure_id.is_none()
        && status
            .reason_codes
            .iter()
            .any(|code| code == "missing_current_closure")
    {
        return String::from("missing_current_closure");
    }
    if !status.stale_unreviewed_closures.is_empty() {
        return String::from("stale_unreviewed");
    }
    String::from("clean")
}

pub(crate) fn execution_command_route_target_from_decision(
    status: &PlanExecutionStatus,
    decision: &NextActionDecision,
    plan_path: &str,
) -> Option<ExecutionCommandRouteTarget> {
    let command_kind = match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume => "begin",
        NextActionKind::Reopen => "reopen",
        NextActionKind::CloseCurrentTask => "complete",
        _ => return None,
    };
    let task_number = decision.task_number?;
    let step_id = decision.step_number;
    if command_kind != "complete" && step_id.is_none() {
        return None;
    }
    if command_kind == "complete" && (status.active_task.is_none() || status.active_step.is_none())
    {
        return None;
    }
    public_command_from_decision(status, decision, plan_path)?;
    Some(ExecutionCommandRouteTarget {
        command_kind,
        task_number,
        step_id,
    })
}

pub(crate) fn public_command_from_decision(
    status: &PlanExecutionStatus,
    decision: &NextActionDecision,
    plan_path: &str,
) -> Option<PublicCommand> {
    let command_kind = match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume => "begin",
        NextActionKind::Reopen => "reopen",
        NextActionKind::CloseCurrentTask => "complete",
        _ => return None,
    };
    let task_number = decision.task_number?;
    let step_id = decision.step_number;
    decision
        .recommended_public_command
        .as_ref()
        .filter(|command| public_command_matches_exact(command, command_kind, task_number, step_id))
        .cloned()
        .or_else(|| {
            synthesized_execution_public_command(
                status,
                plan_path,
                command_kind,
                task_number,
                step_id,
            )
        })
}

fn public_command_matches_exact(
    command: &PublicCommand,
    command_kind: &str,
    task_number: u32,
    step_id: Option<u32>,
) -> bool {
    match command {
        PublicCommand::Begin { task, step, .. } => {
            command_kind == "begin" && *task == task_number && Some(*step) == step_id
        }
        PublicCommand::Complete { task, step, .. } => {
            command_kind == "complete" && *task == task_number && Some(*step) == step_id
        }
        PublicCommand::Reopen { task, step, .. } => {
            command_kind == "reopen" && *task == task_number && Some(*step) == step_id
        }
        _ => false,
    }
}

fn public_command_from_execution_command_route_target(
    status: &PlanExecutionStatus,
    plan_path: &str,
    route_target: &ExecutionCommandRouteTarget,
) -> Option<PublicCommand> {
    let step_id = route_target.step_id?;
    let execution_source = recommended_execution_source(status.execution_mode.as_str());
    match route_target.command_kind {
        "begin" => Some(begin_public_command(
            plan_path,
            route_target.task_number,
            step_id,
            None,
            &status.execution_fingerprint,
        )),
        "complete" => Some(complete_public_command(
            plan_path,
            route_target.task_number,
            step_id,
            execution_source,
            &status.execution_fingerprint,
        )),
        "reopen" => Some(reopen_public_command(
            plan_path,
            route_target.task_number,
            step_id,
            execution_source,
            &status.execution_fingerprint,
        )),
        _ => None,
    }
}

fn synthesized_execution_public_command(
    status: &PlanExecutionStatus,
    plan_path: &str,
    command_kind: &'static str,
    task_number: u32,
    step_id: Option<u32>,
) -> Option<PublicCommand> {
    let execution_source = recommended_execution_source(status.execution_mode.as_str());
    match command_kind {
        "begin" => {
            let step_id = step_id?;
            let execution_mode =
                (status.execution_mode == "none").then_some("featureforge:executing-plans");
            Some(begin_public_command(
                plan_path,
                task_number,
                step_id,
                execution_mode,
                &status.execution_fingerprint,
            ))
        }
        "reopen" => {
            let step_id = step_id?;
            Some(reopen_public_command(
                plan_path,
                task_number,
                step_id,
                execution_source,
                &status.execution_fingerprint,
            ))
        }
        "complete" => {
            let step_id = status.active_step?;
            Some(complete_public_command(
                plan_path,
                task_number,
                step_id,
                execution_source,
                &status.execution_fingerprint,
            ))
        }
        _ => None,
    }
}

fn malformed_execution_markers(status: &PlanExecutionStatus) -> bool {
    (status.active_task.is_some() && status.active_step.is_none())
        || (status.active_task.is_none() && status.active_step.is_some())
        || (status.resume_task.is_some() && status.resume_step.is_none())
        || (status.resume_task.is_none() && status.resume_step.is_some())
        || (status.blocking_task.is_some() && status.blocking_step.is_some_and(|step| step == 0))
}

fn hard_structural_corruption_detected(status: &PlanExecutionStatus) -> bool {
    let blocking_task_missing_current_closure_after_repair = status.blocking_task.is_some()
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
        && status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
                    | "stale_provenance"
            )
        })
        && status.blocking_task.is_some_and(|task_number| {
            status
                .current_task_closures
                .iter()
                .all(|closure| closure.task != task_number)
        });
    malformed_execution_markers(status)
        || (task_scope_structural_review_state_reason(status).is_some()
            && !blocking_task_missing_current_closure_after_repair)
}
