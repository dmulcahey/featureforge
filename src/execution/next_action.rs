use std::collections::BTreeMap;

use crate::execution::current_truth::{
    BranchRerecordingAssessment, BranchRerecordingUnsupportedReason,
    branch_closure_rerecording_assessment_with_authority,
    late_stage_missing_task_closure_baseline_bridge_supported,
    negative_result_requires_execution_reentry, reason_code_requires_test_plan_refresh,
    task_boundary_block_reason_code, task_review_dispatch_task, task_review_result_pending_task,
    task_review_result_requires_verification_reason_codes,
    worktree_drift_escapes_late_stage_surface,
};
use crate::execution::follow_up::normalize_persisted_repair_follow_up_token;
use crate::execution::harness::{HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE};
use crate::execution::state::{
    ExactExecutionCommand, ExecutionContext, GateResult, PlanExecutionStatus,
    document_release_pending_phase_detail, earliest_unresolved_stale_task_from_closure_graph,
    execution_reentry_requires_review_state_repair, gate_finish_from_context,
    prerelease_branch_closure_refresh_required,
    push_task_closure_pending_verification_reason_codes_for_run,
    qa_pending_requires_test_plan_refresh, recommended_execution_source,
    reopen_exact_execution_command_for_task, resolve_exact_execution_command,
    stale_unreviewed_allows_task_closure_baseline_bridge, task_closure_baseline_repair_candidate,
    task_closures_are_non_branch_contributing, task_scope_structural_review_state_reason,
};
use crate::execution::transitions::load_authoritative_transition_state;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NextActionKind {
    Begin,
    Resume,
    Reopen,
    CloseCurrentTask,
    RequestTaskReview,
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
    pub recommended_command: Option<String>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct NextActionAuthorityInputs<'a> {
    pub(crate) persisted_repair_follow_up: Option<&'a str>,
    pub(crate) branch_rerecording_assessment: Option<&'a BranchRerecordingAssessment>,
    pub(crate) gate_finish: Option<&'a GateResult>,
    pub(crate) task_closure_execution_run_ids: Option<&'a BTreeMap<u32, String>>,
}

#[derive(Clone, Copy)]
pub(crate) struct NextActionRequestInputs<'a> {
    pub(crate) plan_path: &'a str,
    pub(crate) external_review_result_ready: bool,
    pub(crate) task_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_lineage_present: bool,
}

pub(crate) fn public_next_action_text(decision: &NextActionDecision) -> String {
    match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume | NextActionKind::Reopen => {
            let structural_task_repair_lane =
                decision.blocking_reason_codes.iter().any(|reason_code| {
                    matches!(
                        reason_code.as_str(),
                        "prior_task_current_closure_invalid"
                            | "prior_task_current_closure_reviewed_state_malformed"
                    )
                });
            let stale_unreviewed_repair_lane = decision.review_state_status == "stale_unreviewed";
            if decision.phase == "execution_preflight"
                || decision.phase_detail == "execution_preflight_required"
            {
                String::from("execution preflight")
            } else if decision.phase_detail == "execution_reentry_required"
                && (structural_task_repair_lane || stale_unreviewed_repair_lane)
            {
                String::from("repair review state / reenter execution")
            } else if decision.phase_detail == "execution_reentry_required" {
                String::from("execution reentry required")
            } else {
                String::from("continue execution")
            }
        }
        NextActionKind::CloseCurrentTask => {
            if decision.phase_detail == "task_closure_recording_ready" {
                String::from("close current task")
            } else {
                String::from("continue execution")
            }
        }
        NextActionKind::RequestTaskReview => String::from("request task review"),
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
            if decision.phase_detail == "release_blocker_resolution_required" {
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
    let persisted_repair_follow_up = authoritative_state.as_ref().and_then(|state| {
        let follow_up = state.review_state_repair_follow_up().map(str::to_owned);
        normalize_persisted_repair_follow_up_token(follow_up.as_deref()).map(str::to_owned)
    });
    let branch_rerecording_assessment =
        branch_closure_rerecording_assessment_with_authority(context, authoritative_state.as_ref())
            .ok();
    let task_closure_execution_run_ids = authoritative_state
        .as_ref()
        .map(|state| {
            state
                .current_task_closure_results()
                .into_iter()
                .filter_map(|(task, record)| record.execution_run_id.map(|run_id| (task, run_id)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let gate_finish = gate_finish_from_context(context);
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
            gate_finish: Some(&gate_finish),
            task_closure_execution_run_ids: Some(&task_closure_execution_run_ids),
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
    // Step 4.1 (ordered pass #1): hard structural corruption must route to repair before any
    // open-step, stale-boundary, or closure-prerequisite routing is attempted.
    if hard_structural_corruption_detected(status) {
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
        ));
    }
    if review_state_status == "clean" && completed_execution_missing_branch_closure(status, context)
    {
        return Some(late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            "branch_closure_recording_required_for_release_readiness",
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
    let earliest_stale_boundary =
        earliest_unresolved_stale_task_from_closure_graph(context, status);
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
            || (status.phase_detail == "execution_reentry_required"
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
            && !status.reason_codes.is_empty()
        {
            return Some(execution_repair_decision(
                context,
                status,
                plan_path,
                review_state_status.as_str(),
            ));
        }
        if let Some(exact_decision) = decision_from_exact_execution_command(
            status,
            resolve_exact_execution_command(status, plan_path),
        ) {
            return Some(exact_decision);
        }
    }

    // Step 4.1 (ordered pass #3): earliest unresolved stale task-closure boundary.
    if let Some(stale_task) = earliest_stale_boundary {
        let missing_current_task_closure_recording_ready =
            status.current_branch_closure_id.is_none()
                && status.current_task_closures.is_empty()
                && status
                    .reason_codes
                    .iter()
                    .any(|reason_code| reason_code == "prior_task_current_closure_missing");
        let stale_boundary_closure_recording_ready =
            task_scope_structural_review_state_reason(status).is_none()
                && (missing_current_task_closure_recording_ready
                    || (task_closure_baseline_repair_candidate(context, status, stale_task)
                        .ok()
                        .flatten()
                        .is_some()
                        && missing_current_closure_allows_task_closure_baseline_route(
                            context,
                            status,
                            authority_inputs,
                            review_state_status.as_str(),
                        )
                        && stale_unreviewed_bridge_ready_for_task(
                            context,
                            status,
                            review_state_status.as_str(),
                            stale_task,
                        )));
        if stale_boundary_closure_recording_ready {
            return Some(task_closure_recording_ready_decision(
                status, plan_path, stale_task,
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
    {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status.as_str(),
        ));
    }
    let late_stage_projection_refresh_candidate_task = if review_state_status
        == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
        && status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
    {
        projection_refresh_candidate_task(context, status, authority_inputs)
    } else {
        None
    };
    if let Some(candidate_task) = late_stage_projection_refresh_candidate_task {
        let projection_refresh_task_route_allowed =
            projection_refresh_candidate_requires_newer_task_closure_baseline(
                context,
                status,
                candidate_task,
            ) && missing_current_closure_allows_task_closure_baseline_route(
                context,
                status,
                authority_inputs,
                review_state_status.as_str(),
            );
        if projection_refresh_task_route_allowed {
            return Some(task_closure_recording_ready_decision(
                status,
                plan_path,
                candidate_task,
            ));
        }
    }
    if status.harness_phase == HarnessPhase::Executing
        && review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
        && status.phase_detail != "execution_reentry_required"
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
        ));
    }
    if let Some(decision) = late_stage_execution_reentry_decision(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
    ) {
        return Some(decision);
    }
    if let Some(task_number) = task_review_dispatch_task(status) {
        return Some(closure_prerequisite_decision(
            status,
            NextActionKind::RequestTaskReview,
            "task_review_dispatch_required",
            Some(task_number),
            None,
        ));
    }
    if let Some(task_number) = task_review_result_pending_task(status, task_review_dispatch_id) {
        let closure_baseline_candidate =
            task_closure_baseline_repair_candidate(context, status, task_number)
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
            "task_review_result_pending",
            Some(task_number),
            None,
        ));
    }
    if matches!(
        status.harness_phase,
        HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
    ) && review_state_status == "missing_current_closure"
        && let Some(task_number) =
            projection_refresh_candidate_task(context, status, authority_inputs)
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
                    review_state_status.as_str(),
                    *task_number,
                )
        })
        && let Ok(Some(_candidate)) =
            task_closure_baseline_repair_candidate(context, status, task_number)
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
                    review_state_status.as_str(),
                    *task_number,
                )
        })
        .filter(|_| task_review_dispatch_task(status).is_none())
        .filter(|task_number| {
            task_closure_baseline_repair_candidate(context, status, *task_number)
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
        && task_closure_baseline_repair_candidate(context, status, task_number)
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
        && let Some(candidate_task) =
            projection_refresh_candidate_task(context, status, authority_inputs)
                .or_else(|| closure_baseline_candidate_task(context))
        && let Ok(Some(_candidate)) =
            task_closure_baseline_repair_candidate(context, status, candidate_task)
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
        && let Ok(Some(candidate)) =
            task_closure_baseline_repair_candidate(context, status, candidate_task)
        && !candidate.projection_refresh_only
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
            "branch_closure_recording_required_for_release_readiness",
            plan_path,
        ));
    }
    if task_scope_pivot_override_active(status, review_state_status.as_str()) {
        let recommended_command = (status.blocking_task.is_some()
            || status.active_task.is_some()
            || status.resume_task.is_some()
            || !status.reason_codes.is_empty())
        .then_some(format!(
            "featureforge plan execution repair-review-state --plan {plan_path}"
        ));
        return Some(NextActionDecision {
            kind: NextActionKind::PlanningReentry,
            phase: String::from("pivot_required"),
            phase_detail: String::from("planning_reentry_required"),
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
            recommended_command,
        });
    }
    if review_state_status == "missing_current_closure" {
        if status.phase_detail == "branch_closure_recording_required_for_release_readiness"
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
                ));
            }
            return Some(late_stage_decision(
                status,
                NextActionKind::AdvanceLateStage,
                "branch_closure_recording_required_for_release_readiness",
                plan_path,
            ));
        }
        if status.current_branch_closure_id.is_none()
            && status.phase_detail == "execution_reentry_required"
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
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending => {
            let phase_detail = document_release_pending_phase_detail(status);
            let kind = if phase_detail == "final_review_dispatch_required" {
                NextActionKind::RequestFinalReview
            } else {
                NextActionKind::AdvanceLateStage
            };
            return Some(late_stage_decision(status, kind, phase_detail, plan_path));
        }
        HarnessPhase::FinalReviewPending => {
            if status.current_branch_closure_id.is_none() {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    "branch_closure_recording_required_for_release_readiness",
                    plan_path,
                ));
            }
            if status.current_release_readiness_state.as_deref() != Some("ready") {
                let phase_detail =
                    if status.current_release_readiness_state.as_deref() == Some("blocked") {
                        "release_blocker_resolution_required"
                    } else {
                        "release_readiness_recording_ready"
                    };
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase_detail,
                    plan_path,
                ));
            }
            let dispatch_lineage_present =
                final_review_dispatch_lineage_present || final_review_dispatch_id.is_some();
            let phase_requires_dispatch = status.phase_detail == "final_review_dispatch_required"
                && (!dispatch_lineage_present || status.current_final_review_result.is_some());
            let refresh_requires_dispatch = final_review_dispatch_requires_refresh(status);
            if phase_requires_dispatch
                || refresh_requires_dispatch
                || (!dispatch_lineage_present && status.current_final_review_result.is_none())
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::RequestFinalReview,
                    "final_review_dispatch_required",
                    plan_path,
                ));
            }
            if status.phase_detail == "final_review_recording_ready"
                || status.current_final_review_result.is_some()
                    && status.current_final_review_branch_closure_id.as_deref()
                        == status.current_branch_closure_id.as_deref()
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    "final_review_recording_ready",
                    plan_path,
                ));
            }
            if external_review_result_ready {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    "final_review_recording_ready",
                    plan_path,
                ));
            }
            return Some(late_stage_decision(
                status,
                NextActionKind::WaitForFinalReviewResult,
                "final_review_outcome_pending",
                plan_path,
            ));
        }
        HarnessPhase::QaPending => {
            if qa_pending_requires_test_plan_refresh(context, authority_inputs.gate_finish)
                || status
                    .reason_codes
                    .iter()
                    .any(|reason_code| reason_code_requires_test_plan_refresh(reason_code))
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::RefreshTestPlan,
                    "test_plan_refresh_required",
                    plan_path,
                ));
            }
            return Some(late_stage_decision(
                status,
                NextActionKind::RunQa,
                "qa_recording_required",
                plan_path,
            ));
        }
        HarnessPhase::ReadyForBranchCompletion => {
            let phase_detail = if status
                .finish_review_gate_pass_branch_closure_id
                .as_deref()
                .zip(status.current_branch_closure_id.as_deref())
                .is_some_and(|(checkpoint, current)| checkpoint == current)
            {
                "finish_completion_gate_ready"
            } else {
                "finish_review_gate_ready"
            };
            return Some(late_stage_decision(
                status,
                NextActionKind::FinishBranch,
                phase_detail,
                plan_path,
            ));
        }
        HarnessPhase::HandoffRequired => {
            return Some(late_stage_decision(
                status,
                NextActionKind::Handoff,
                "handoff_recording_required",
                plan_path,
            ));
        }
        _ => {}
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
        let begin_command = (!marker_free_evidence_projection).then(|| {
            let execution_mode_arg = if status.execution_mode == "none" {
                " --execution-mode featureforge:executing-plans"
            } else {
                ""
            };
            format!(
                "featureforge plan execution begin --plan {plan_path} --task {task_number} --step {step_number}{execution_mode_arg} --expect-execution-fingerprint {}",
                status.execution_fingerprint
            )
        });
        let (phase, phase_detail) = if marker_free_preflight_projection {
            (
                String::from("execution_preflight"),
                String::from("execution_in_progress"),
            )
        } else if status.execution_started == "yes" {
            (
                String::from("executing"),
                String::from("execution_reentry_required"),
            )
        } else {
            (
                String::from("execution_preflight"),
                String::from("execution_preflight_required"),
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
            recommended_command: begin_command,
        });
    }

    Some(late_stage_decision(
        status,
        NextActionKind::PlanningReentry,
        "planning_reentry_required",
        plan_path,
    ))
}

fn execution_repair_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
) -> NextActionDecision {
    if let Some(task_number) = execution_reentry_target_task(context, status, plan_path) {
        return execution_repair_decision_for_task(
            status,
            plan_path,
            review_state_status,
            task_number,
        );
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
        phase: String::from("executing"),
        phase_detail: String::from("execution_reentry_required"),
        review_state_status: review_state_status.to_owned(),
        task_number: None,
        step_number: None,
        blocking_task: None,
        blocking_reason_codes,
        recommended_command: Some(format!(
            "featureforge plan execution repair-review-state --plan {plan_path}"
        )),
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
        phase: String::from("executing"),
        phase_detail: String::from("execution_reentry_required"),
        review_state_status: review_state_status.to_owned(),
        task_number: Some(task_number),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: Some(task_number),
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_command: Some(format!(
            "featureforge plan execution repair-review-state --plan {plan_path}"
        )),
    }
}

fn closure_prerequisite_decision(
    status: &PlanExecutionStatus,
    kind: NextActionKind,
    phase_detail: &str,
    task_number: Option<u32>,
    step_number: Option<u32>,
) -> NextActionDecision {
    let recommended_command = match kind {
        NextActionKind::RequestTaskReview
        | NextActionKind::WaitForTaskReviewResult
        | NextActionKind::RequestFinalReview
        | NextActionKind::WaitForFinalReviewResult => None,
        _ => status.recommended_command.clone(),
    };
    NextActionDecision {
        kind,
        phase: String::from("task_closure_pending"),
        phase_detail: String::from(phase_detail),
        review_state_status: canonical_review_state_status(status),
        task_number,
        step_number,
        blocking_task: task_number.or(status.blocking_task),
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_command,
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
            "branch_closure_recording_required_for_release_readiness",
            plan_path,
        );
    }
    NextActionDecision {
        kind: NextActionKind::CloseCurrentTask,
        phase: String::from("task_closure_pending"),
        phase_detail: String::from("task_closure_recording_ready"),
        review_state_status: String::from("clean"),
        task_number: Some(task_number),
        step_number: None,
        blocking_task: Some(task_number),
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_command: Some(format!(
            "featureforge plan execution close-current-task --plan {plan_path} --task {task_number} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
        )),
    }
}

fn late_stage_decision(
    status: &PlanExecutionStatus,
    kind: NextActionKind,
    phase_detail: &str,
    plan_path: &str,
) -> NextActionDecision {
    let recommended_command = match phase_detail {
        "branch_closure_recording_required_for_release_readiness"
        | "release_readiness_recording_ready"
        | "release_blocker_resolution_required"
        | "final_review_recording_ready" => Some(format!(
            "featureforge plan execution advance-late-stage --plan {plan_path}"
        )),
        "qa_recording_required" => Some(format!(
            "featureforge plan execution advance-late-stage --plan {plan_path} --result pass|fail --summary-file <path>"
        )),
        "final_review_dispatch_required"
        | "final_review_outcome_pending"
        | "test_plan_refresh_required"
        | "finish_review_gate_ready"
        | "finish_completion_gate_ready" => None,
        "handoff_recording_required" => Some(format!(
            "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
        )),
        _ => status.recommended_command.clone(),
    };
    NextActionDecision {
        kind,
        phase: match phase_detail {
            "task_review_dispatch_required"
            | "task_review_result_pending"
            | "task_closure_recording_ready" => String::from("task_closure_pending"),
            "branch_closure_recording_required_for_release_readiness"
            | "release_readiness_recording_ready"
            | "release_blocker_resolution_required" => String::from("document_release_pending"),
            "final_review_dispatch_required"
                if status.harness_phase == HarnessPhase::DocumentReleasePending =>
            {
                String::from("document_release_pending")
            }
            "final_review_dispatch_required"
            | "final_review_outcome_pending"
            | "final_review_recording_ready" => String::from("final_review_pending"),
            "qa_recording_required" | "test_plan_refresh_required" => String::from("qa_pending"),
            "finish_review_gate_ready" | "finish_completion_gate_ready" => {
                String::from("ready_for_branch_completion")
            }
            "handoff_recording_required" => String::from("handoff_required"),
            _ => status.harness_phase.as_str().to_owned(),
        },
        phase_detail: String::from(phase_detail),
        review_state_status: canonical_review_state_status(status),
        task_number: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: status.blocking_task,
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_command,
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
    let target_task = execution_reentry_target_task(context, status, plan_path).or(status
        .blocking_task
        .or(status.resume_task)
        .or(status.active_task));
    if let Some(task_number) = target_task
        && status.resume_task.is_none()
        && status.active_task.is_none()
        && status.blocking_step.is_none()
    {
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
    if let Some(task_number) = target_task {
        let mut repair_decision =
            execution_repair_decision_for_task(status, plan_path, "stale_unreviewed", task_number);
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
    NextActionDecision {
        kind: NextActionKind::RepairReviewState,
        phase: String::from("executing"),
        phase_detail: String::from("execution_reentry_required"),
        review_state_status: String::from("stale_unreviewed"),
        task_number: target_task,
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: target_task,
        blocking_reason_codes,
        recommended_command: Some(format!(
            "featureforge plan execution repair-review-state --plan {plan_path}"
        )),
    }
}

fn late_stage_planning_reentry_decision(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> NextActionDecision {
    NextActionDecision {
        kind: NextActionKind::PlanningReentry,
        phase: String::from("pivot_required"),
        phase_detail: String::from("planning_reentry_required"),
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
        recommended_command: None,
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
        );
    }
    if status.current_branch_closure_id.is_none()
        && let Some(task_number) =
            projection_refresh_candidate_task(context, status, authority_inputs)
        && projection_refresh_candidate_requires_newer_task_closure_baseline(
            context,
            status,
            task_number,
        )
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
        && task_closure_baseline_repair_candidate(context, status, task_number)
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
        return execution_reentry_target_task(context, status, plan_path)
            .map(|task_number| {
                execution_reentry_decision_for_task(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    task_number,
                    false,
                )
            })
            .unwrap_or_else(|| {
                execution_repair_decision(context, status, plan_path, review_state_status)
            });
    }
    if assessment.supported {
        return late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            "branch_closure_recording_required_for_release_readiness",
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
                    "branch_closure_recording_required_for_release_readiness",
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
                    );
                }
                if late_stage_missing_task_closure_baseline_bridge_supported(assessment)
                    && let Some(task_number) = closure_baseline_candidate_task(context)
                    && task_closure_baseline_repair_candidate(context, status, task_number)
                        .ok()
                        .flatten()
                        .is_some()
                {
                    return task_closure_recording_ready_decision(status, plan_path, task_number);
                }
                return execution_reentry_target_task(context, status, plan_path)
                    .map(|task_number| {
                        execution_reentry_decision_for_task(
                            context,
                            status,
                            plan_path,
                            review_state_status,
                            task_number,
                            false,
                        )
                    })
                    .unwrap_or_else(|| {
                        execution_repair_decision(context, status, plan_path, review_state_status)
                    });
            }
            execution_repair_decision(context, status, plan_path, review_state_status)
        }
        Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
            execution_reentry_target_task(context, status, plan_path)
                .map(|task_number| {
                    execution_reentry_decision_for_task(
                        context,
                        status,
                        plan_path,
                        review_state_status,
                        task_number,
                        false,
                    )
                })
                .unwrap_or_else(|| {
                    late_stage_planning_reentry_decision(status, review_state_status)
                })
        }
    }
}

fn task_boundary_blocking_task(status: &PlanExecutionStatus) -> Option<u32> {
    let task_number = status
        .blocking_task
        .or(status.resume_task)
        .or(status.active_task)?;
    let boundary_reason_code = task_boundary_block_reason_code(status).or_else(|| {
        status.reason_codes.iter().find_map(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_current_closure_stale"
                    | "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
                    | "task_cycle_break_active"
            )
            .then_some(reason_code.as_str())
        })
    })?;
    match boundary_reason_code {
        "prior_task_current_closure_missing"
        | "prior_task_current_closure_stale"
        | "prior_task_current_closure_invalid"
        | "prior_task_current_closure_reviewed_state_malformed"
        | "task_cycle_break_active" => Some(task_number),
        _ => None,
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
            review_state_status.as_str(),
            task_number,
        )
    {
        return None;
    }
    boundary_blocking_task.or_else(|| {
        (status.phase_detail == "execution_reentry_required"
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

pub(crate) fn closure_baseline_candidate_task(context: &ExecutionContext) -> Option<u32> {
    if let Some(next_unchecked_task) = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number)
    {
        return context
            .tasks_by_number
            .keys()
            .copied()
            .filter(|task_number| *task_number < next_unchecked_task)
            .max();
    }
    context.tasks_by_number.keys().copied().max()
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
        && let Some(exact_command) = resolve_exact_execution_command(status, plan_path)
        && exact_command.task_number == task_number
        && let Some(mut exact_decision) =
            decision_from_exact_execution_command(status, Some(exact_command))
    {
        exact_decision.blocking_task = exact_decision.blocking_task.or(Some(task_number));
        if exact_decision.phase_detail != "execution_in_progress" {
            exact_decision.phase_detail = String::from("execution_reentry_required");
        }
        return exact_decision;
    }
    if let Some(reopen_command) =
        reopen_exact_execution_command_for_task(context, status, plan_path, task_number)
    {
        return NextActionDecision {
            kind: NextActionKind::Reopen,
            phase: String::from("executing"),
            phase_detail: String::from("execution_reentry_required"),
            review_state_status: review_state_status.to_owned(),
            task_number: Some(task_number),
            step_number: reopen_command.step_id,
            blocking_task: Some(task_number),
            blocking_reason_codes: status.reason_codes.clone(),
            recommended_command: Some(reopen_command.recommended_command),
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

fn decision_from_exact_execution_command(
    status: &PlanExecutionStatus,
    exact_command: Option<ExactExecutionCommand>,
) -> Option<NextActionDecision> {
    let exact_command = exact_command?;
    let kind = match exact_command.command_kind {
        "begin" => {
            if status.resume_task == Some(exact_command.task_number)
                && status.resume_step == exact_command.step_id
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
    let phase_detail = match exact_command.command_kind {
        "complete" => String::from("execution_in_progress"),
        "begin"
            if status.resume_task == Some(exact_command.task_number)
                && status.resume_step == exact_command.step_id
                && status.harness_phase == HarnessPhase::Executing
                && execution_reentry_requires_review_state_repair(None, status) =>
        {
            String::from("execution_in_progress")
        }
        "begin"
            if status.blocking_step.is_some()
                && !execution_reentry_requires_review_state_repair(None, status) =>
        {
            String::from("execution_in_progress")
        }
        _ => String::from("execution_reentry_required"),
    };
    let phase = if phase_detail == "execution_in_progress" {
        String::from("handoff_required")
    } else {
        String::from("executing")
    };
    Some(NextActionDecision {
        kind,
        phase,
        phase_detail,
        review_state_status: canonical_review_state_status(status),
        task_number: Some(exact_command.task_number),
        step_number: exact_command.step_id,
        blocking_task: if exact_command.command_kind == "reopen" {
            Some(exact_command.task_number)
        } else {
            status.blocking_task
        },
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_command: Some(exact_command.recommended_command),
    })
}

pub(crate) fn execution_reentry_target_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<u32> {
    status
        .blocking_task
        .or(status.resume_task)
        .or(status.active_task)
        .or_else(|| earliest_unresolved_stale_task_from_closure_graph(context, status))
        .or_else(|| {
            status
                .current_task_closures
                .iter()
                .map(|closure| closure.task)
                .max()
        })
        .or_else(|| {
            resolve_exact_execution_command(status, plan_path).map(|command| command.task_number)
        })
        .or_else(|| closure_baseline_candidate_task(context))
}

fn late_stage_execution_reentry_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
) -> Option<NextActionDecision> {
    let stale_provenance_present = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "stale_provenance");
    let negative_result_reroute = status.harness_phase == HarnessPhase::Executing
        && review_state_status == "clean"
        && status.current_branch_closure_id.is_some()
        && status.blocking_step.is_none()
        && negative_result_requires_execution_reentry(
            false,
            "executing",
            status.current_branch_closure_id.as_deref(),
            status.current_final_review_branch_closure_id.as_deref(),
            status.current_final_review_result.as_deref(),
            status.current_qa_branch_closure_id.as_deref(),
            status.current_qa_result.as_deref(),
        );
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
    let reentry_task = execution_reentry_target_task(context, status, plan_path);
    Some(
        reentry_task
            .map(|task_number| {
                execution_reentry_decision_for_task(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    task_number,
                    false,
                )
            })
            .unwrap_or_else(|| {
                execution_repair_decision(context, status, plan_path, review_state_status)
            }),
    )
}

fn projection_refresh_candidate_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<u32> {
    if let Some(task_number) =
        current_task_closure_projection_refresh_task(context, status, authority_inputs)
    {
        return Some(task_number);
    }
    context
        .tasks_by_number
        .keys()
        .copied()
        .rev()
        .find_map(|task_number| {
            task_closure_baseline_repair_candidate(context, status, task_number)
                .ok()
                .flatten()
                .filter(|candidate| candidate.projection_refresh_only)
                .map(|candidate| candidate.task)
        })
}

fn current_task_closure_projection_refresh_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<u32> {
    let execution_run_ids = authority_inputs.task_closure_execution_run_ids?;
    status
        .current_task_closures
        .iter()
        .map(|closure| closure.task)
        .rev()
        .find(|task_number| {
            execution_run_ids
                .get(task_number)
                .is_some_and(|execution_run_id| {
                    let mut reason_codes = Vec::new();
                    push_task_closure_pending_verification_reason_codes_for_run(
                        context,
                        *task_number,
                        execution_run_id.as_str(),
                        true,
                        &mut reason_codes,
                    )
                    .is_ok()
                        && reason_codes.iter().any(|reason_code| {
                            matches!(
                                reason_code.as_str(),
                                "prior_task_verification_missing"
                                    | "prior_task_verification_missing_legacy"
                                    | "task_review_receipt_malformed"
                                    | "task_verification_receipt_malformed"
                            )
                        })
                })
        })
}

fn projection_refresh_candidate_requires_newer_task_closure_baseline(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    candidate_task: u32,
) -> bool {
    let first_task_number = context.tasks_by_number.keys().copied().min();
    if first_task_number.is_some_and(|first_task| candidate_task > first_task) {
        return true;
    }
    status
        .current_task_closures
        .iter()
        .map(|closure| closure.task)
        .max()
        .is_some_and(|current_task| candidate_task > current_task)
}

fn stale_unreviewed_bridge_ready_for_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    let Some(candidate) = task_closure_baseline_repair_candidate(context, status, task_number)
        .ok()
        .flatten()
    else {
        return false;
    };
    if review_state_status == "stale_unreviewed" {
        return stale_unreviewed_allows_task_closure_baseline_bridge(context, status, task_number)
            .unwrap_or(false);
    }
    let unresolved_stale_task_matches =
        earliest_unresolved_stale_task_from_closure_graph(context, status) == Some(task_number);
    let dispatch_bound_bridge_candidate = candidate
        .dispatch_id
        .as_deref()
        .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty());
    let has_closure_bridge_reason_signals =
        closure_baseline_routing_reason_codes_compatible(status)
            && status.reason_codes.iter().any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "prior_task_current_closure_missing" | "task_closure_baseline_repair_candidate"
                )
            });
    if review_state_status == "clean" {
        return matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        ) && (has_closure_bridge_reason_signals
            || (unresolved_stale_task_matches && dispatch_bound_bridge_candidate));
    }
    if review_state_status == "missing_current_closure" {
        return has_closure_bridge_reason_signals
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

fn final_review_dispatch_requires_refresh(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "final_review_state_stale" | "final_review_state_not_fresh"
        )
    }) || (status.current_final_review_result.is_some()
        && status.current_final_review_branch_closure_id.as_deref()
            != status.current_branch_closure_id.as_deref())
}

fn external_review_ready_promotes_closure_recording(status: &PlanExecutionStatus) -> bool {
    closure_baseline_routing_reason_codes_compatible(status)
        && !status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_verification_missing"
                    | "prior_task_verification_missing_legacy"
                    | "task_review_not_independent"
                    | "task_review_receipt_malformed"
                    | "task_verification_receipt_malformed"
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
    NextActionDecision {
        kind: NextActionKind::Handoff,
        phase: if task_scoped_handoff {
            String::from("executing")
        } else {
            String::from("handoff_required")
        },
        phase_detail: String::from("handoff_recording_required"),
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
        recommended_command: Some(format!(
            "featureforge plan execution transfer --plan {plan_path} --scope task|branch --to <owner> --reason <reason>"
        )),
    }
}

fn canonical_review_state_status(status: &PlanExecutionStatus) -> String {
    if status.review_state_status != "clean" {
        return status.review_state_status.clone();
    }
    if prerelease_branch_closure_refresh_required(status)
        || status.phase_detail == "branch_closure_recording_required_for_release_readiness"
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

pub(crate) fn exact_execution_command_from_decision(
    status: &PlanExecutionStatus,
    decision: &NextActionDecision,
    plan_path: &str,
) -> Option<ExactExecutionCommand> {
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
    let recommended_command = decision
        .recommended_command
        .clone()
        .or_else(|| {
            let exact = resolve_exact_execution_command(status, plan_path)?;
            (exact.command_kind == command_kind
                && exact.task_number == task_number
                && exact.step_id == step_id)
                .then_some(exact.recommended_command)
        })
        .or_else(|| {
            synthesized_exact_execution_command(
                status,
                plan_path,
                command_kind,
                task_number,
                step_id,
            )
        })?;
    Some(ExactExecutionCommand {
        command_kind,
        task_number,
        step_id,
        recommended_command,
    })
}

fn synthesized_exact_execution_command(
    status: &PlanExecutionStatus,
    plan_path: &str,
    command_kind: &'static str,
    task_number: u32,
    step_id: Option<u32>,
) -> Option<String> {
    let execution_source = recommended_execution_source(status.execution_mode.as_str());
    match command_kind {
        "begin" => {
            let step_id = step_id?;
            let execution_mode_arg = if status.execution_mode == "none" {
                " --execution-mode featureforge:executing-plans"
            } else {
                ""
            };
            Some(format!(
                "featureforge plan execution begin --plan {plan_path} --task {task_number} --step {step_id}{execution_mode_arg} --expect-execution-fingerprint {}",
                status.execution_fingerprint
            ))
        }
        "reopen" => {
            let step_id = step_id?;
            Some(format!(
                "featureforge plan execution reopen --plan {plan_path} --task {task_number} --step {step_id} --source {execution_source} --reason <reason> --expect-execution-fingerprint {}",
                status.execution_fingerprint
            ))
        }
        "complete" => {
            let step_id = status.active_step?;
            Some(format!(
                "featureforge plan execution complete --plan {plan_path} --task {task_number} --step {step_id} --source {execution_source} --claim <claim> --manual-verify-summary <summary> --expect-execution-fingerprint {}",
                status.execution_fingerprint
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
