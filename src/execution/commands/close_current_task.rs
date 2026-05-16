use super::common::*;

const ALREADY_CURRENT_EQUIVALENT_TRACE: &str =
    "Current task already has an equivalent recorded task closure for this reviewed state.";
const ALREADY_CURRENT_SUMMARY_DRIFT_TRACE: &str = "Current task already has a positive recorded task closure for this reviewed state; summary-only drift was ignored.";
const ALREADY_CURRENT_SUMMARY_UNAVAILABLE_TRACE: &str = "Current task already has a positive recorded task closure for this reviewed state; unavailable summary artifacts were ignored.";
const CLOSE_CURRENT_TASK_CONFLICT_TRACE: &str = "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this reviewed state.";
const NEGATIVE_RESULT_BLOCKER_TRACE: &str = "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state.";

pub fn close_current_task(
    runtime: &ExecutionRuntime,
    args: &CloseCurrentTaskArgs,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    require_close_current_task_public_flags(args)?;
    let initial_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    ensure_public_intent_preflight_ready(&initial_context, PublicCommandKind::CloseCurrentTask)?;
    let authoritative_execution_run_id = load_authoritative_transition_state(&initial_context)?
        .as_ref()
        .and_then(|state| state.execution_run_id_opt());
    let mut status = status_with_shared_routing_or_context(runtime, &args.plan, &initial_context)?;
    let execution_run_id = authoritative_execution_run_id.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "close-current-task requires execution preflight and run identity established by begin before it can mutate runtime state.",
        )
    })?;
    let verification_result = args.verification_result.as_str();
    let initial_reviewed_state_id = current_task_reviewed_state_id(&initial_context, args.task)?;
    let initial_raw_reviewed_state_id =
        current_task_raw_reviewed_state_id(&initial_context, args.task)?;
    let initial_closure_record_id = current_task_closure_record_id(&initial_context, args.task)?;
    let candidate_dispatch_id = current_review_dispatch_id_candidate(
        &initial_context,
        ReviewDispatchScopeArg::Task,
        Some(args.task),
        args.dispatch_id.as_deref(),
    )?;
    if let Some(dispatch_id) = candidate_dispatch_id.as_deref() {
        ensure_task_dispatch_id_matches(&initial_context, args.task, dispatch_id)?;
        match task_dispatch_reviewed_state_status(
            &initial_context,
            args.task,
            &initial_reviewed_state_id,
            &initial_raw_reviewed_state_id,
        )? {
            TaskDispatchReviewedStateStatus::Current => {}
            TaskDispatchReviewedStateStatus::MissingReviewedStateBinding => {
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    None,
                    Some(
                        FollowUpKind::RequestExternalReview
                            .public_token()
                            .to_owned(),
                    ),
                );
                return Ok(blocked_close_current_task_output(
                    BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "blocked",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        recommended_public_command_template: recovery
                            .recommended_public_command_template,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because the current runtime-owned task review state does not bind a current reviewed state.",
                    },
                ));
            }
            TaskDispatchReviewedStateStatus::StaleReviewedState => {
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    None,
                    Some(String::from(
                        crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
                    )),
                );
                return Ok(blocked_close_current_task_output(
                    BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "blocked",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        recommended_public_command_template: recovery
                            .recommended_public_command_template,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because tracked workspace state changed after the current runtime-owned task review state was captured.",
                    },
                ));
            }
        }
        let mut authoritative_state = load_authoritative_transition_state(&initial_context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires authoritative harness state.",
            ));
        };
        if let Some(output) = handle_close_current_task_already_current_decision(
            CloseCurrentTaskAlreadyCurrentDecisionRequest {
                runtime,
                context: &initial_context,
                status: &status,
                args,
                authoritative_state,
                operator: None,
                dispatch_id,
                closure_record_id: &initial_closure_record_id,
                reviewed_state_id: &initial_reviewed_state_id,
                raw_reviewed_state_id: &initial_raw_reviewed_state_id,
                replay_trace_mode: AlreadyCurrentReplayTraceMode::InputEquivalence,
                negative_blocker_recovery: NegativeResultBlockerRecovery::CloseCurrentTask,
            },
        )? {
            return Ok(output);
        }
    }
    let dispatch_refreshed = candidate_dispatch_id.is_none();
    let dispatch_id = if let Some(dispatch_id) = candidate_dispatch_id {
        dispatch_id
    } else {
        require_close_current_task_public_mutation(&status, args.task)?;
        // Historical stale/missing dispatch lineage is a status/operator
        // diagnostic. The public mutation path refreshes current dispatch
        // authority here, then the post-refresh checks below fail closed if
        // the runtime still cannot produce current binding.
        ensure_current_review_dispatch_id_for_command(
            &initial_context,
            ReviewDispatchScopeArg::Task,
            Some(args.task),
            None,
            EventCommandOwner::PublicCloseCurrentTask,
        )?
    };
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    if dispatch_refreshed {
        // The dispatch refresh above appended authoritative state. Rebuild the
        // route status before any later eligibility or already-current decision
        // so this mutation does not authorize against a pre-write projection.
        status = status_with_shared_routing_or_context(runtime, &args.plan, &context)?;
    }
    ensure_task_dispatch_id_matches(&context, args.task, &dispatch_id)?;
    let operator = current_workflow_operator(runtime, &args.plan, true)?;
    let _strategy_checkpoint_fingerprint =
        authoritative_strategy_checkpoint_fingerprint_checked(&context)?.ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "close-current-task requires authoritative strategy checkpoint provenance.",
            )
        })?;
    let reviewed_state_id = current_task_reviewed_state_id(&context, args.task)?;
    let raw_reviewed_state_id = current_task_raw_reviewed_state_id(&context, args.task)?;
    let contract_identity = current_task_contract_identity(&context, args.task)?;
    let closure_record_id = current_task_closure_record_id(&context, args.task)?;
    match task_dispatch_reviewed_state_status(
        &context,
        args.task,
        &reviewed_state_id,
        &raw_reviewed_state_id,
    )? {
        TaskDispatchReviewedStateStatus::Current => {}
        TaskDispatchReviewedStateStatus::MissingReviewedStateBinding => {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                Some(
                    FollowUpKind::RequestExternalReview
                        .public_token()
                        .to_owned(),
                ),
            );
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "blocked",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: recovery.recommended_command,
                    recommended_public_command_argv: recovery.recommended_public_command_argv,
                    recommended_public_command_template: recovery
                        .recommended_public_command_template,
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because the current runtime-owned task review state does not bind a current reviewed state.",
                }),
                &operator,
            ));
        }
        TaskDispatchReviewedStateStatus::StaleReviewedState => {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                Some(String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
                )),
            );
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "blocked",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: recovery.recommended_command,
                    recommended_public_command_argv: recovery.recommended_public_command_argv,
                    recommended_public_command_template: recovery
                        .recommended_public_command_template,
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because tracked workspace state changed after the current runtime-owned task review state was captured.",
                }),
                &operator,
            ));
        }
    }
    {
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires authoritative harness state.",
            ));
        };
        if let Some(output) = handle_close_current_task_already_current_decision(
            CloseCurrentTaskAlreadyCurrentDecisionRequest {
                runtime,
                context: &context,
                status: &status,
                args,
                authoritative_state,
                operator: Some(&operator),
                dispatch_id: &dispatch_id,
                closure_record_id: &closure_record_id,
                reviewed_state_id: &reviewed_state_id,
                raw_reviewed_state_id: &raw_reviewed_state_id,
                replay_trace_mode: AlreadyCurrentReplayTraceMode::InputEquivalence,
                negative_blocker_recovery: NegativeResultBlockerRecovery::CloseCurrentTask,
            },
        )? {
            return Ok(output);
        }
    }
    match close_current_task_outcome_class(args.review_result, args.verification_result) {
        CloseCurrentTaskOutcomeClass::Positive => {
            require_close_current_task_public_mutation(&status, args.task)?;
            refresh_task_closure_authoritative_lineage_with_context(
                runtime,
                &context,
                TaskClosureLineageRefresh {
                    task: args.task,
                    claim_write_authority: true,
                },
            )?;
            let locked_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            status = status_with_shared_routing_or_context(runtime, &args.plan, &locked_context)?;
            let operator = current_workflow_operator(runtime, &args.plan, true)?;
            let mut authoritative_state = load_authoritative_transition_state(&locked_context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            ensure_task_dispatch_id_matches(&locked_context, args.task, &dispatch_id)?;
            let reviewed_state_id = current_task_reviewed_state_id(&locked_context, args.task)?;
            let raw_reviewed_state_id =
                current_task_raw_reviewed_state_id(&locked_context, args.task)?;
            let closure_record_id = current_task_closure_record_id(&locked_context, args.task)?;
            let effective_reviewed_surface_paths =
                current_task_effective_reviewed_surface_paths(&locked_context, args.task)?;
            if let Some(output) = handle_close_current_task_already_current_decision(
                CloseCurrentTaskAlreadyCurrentDecisionRequest {
                    runtime,
                    context: &locked_context,
                    status: &status,
                    args,
                    authoritative_state,
                    operator: Some(&operator),
                    dispatch_id: &dispatch_id,
                    closure_record_id: &closure_record_id,
                    reviewed_state_id: &reviewed_state_id,
                    raw_reviewed_state_id: &raw_reviewed_state_id,
                    replay_trace_mode: AlreadyCurrentReplayTraceMode::RecordedPositiveRefresh,
                    negative_blocker_recovery: NegativeResultBlockerRecovery::CloseCurrentTask,
                },
            )? {
                return Ok(output);
            }
            let (review_summary_hash, verification_summary_hash) =
                close_current_task_summary_hashes(args)?;
            let superseded_task_closure_records = superseded_task_closure_records(
                &locked_context,
                authoritative_state,
                args.task,
                &closure_record_id,
                &effective_reviewed_surface_paths,
            );
            let superseded_task_closure_ids = superseded_task_closure_records
                .iter()
                .map(|record| record.closure_record_id.clone())
                .collect::<Vec<_>>();
            let superseded_tasks = superseded_task_closure_records
                .iter()
                .map(|record| record.task)
                .collect::<Vec<_>>();
            let _write_authority = claim_step_write_authority(runtime)?;
            materialize_current_task_closure_from_close_inputs(
                authoritative_state,
                CurrentTaskClosureMaterialization {
                    task: args.task,
                    dispatch_id: &dispatch_id,
                    closure_record_id: &closure_record_id,
                    execution_run_id: &execution_run_id,
                    reviewed_state_id: &raw_reviewed_state_id,
                    semantic_reviewed_state_id: &reviewed_state_id,
                    contract_identity: &contract_identity,
                    effective_reviewed_surface_paths: &effective_reviewed_surface_paths,
                    review_result: args.review_result.as_str(),
                    review_summary_hash: &review_summary_hash,
                    verification_result,
                    verification_summary_hash: &verification_summary_hash,
                    superseded_tasks: &superseded_tasks,
                    superseded_task_closure_ids: &superseded_task_closure_ids,
                },
            )?;
            drop(_write_authority);
            release_resolved_worktree_leases_after_current_task_closure(
                runtime,
                &locked_context,
                args.task,
                &closure_record_id,
                CloseCurrentTaskClosureCleanupState::Recorded,
            )?;
            Ok(CloseCurrentTaskOutput {
                action: String::from("recorded"),
                task_number: args.task,
                dispatch_validation_action: String::from("validated"),
                closure_action: String::from("recorded"),
                task_closure_status: String::from("current"),
                superseded_task_closure_ids,
                closure_record_id: Some(closure_record_id),
                code: None,
                recommended_command: None,
                recommended_public_command_argv: None,
                recommended_public_command_template: None,
                required_inputs: Vec::new(),
                rederive_via_workflow_operator: None,
                required_follow_up: None,
                blocking_scope: None,
                blocking_task: None,
                blocking_reason_codes: Vec::new(),
                authoritative_next_action: None,
                trace_summary: String::from(TASK_CLOSURE_RECORDED_TRACE),
            })
        }
        CloseCurrentTaskOutcomeClass::Negative => {
            require_close_current_task_public_mutation(&status, args.task)?;
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            let locked_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            ensure_task_dispatch_id_matches(&locked_context, args.task, &dispatch_id)?;
            if let Some(output) = handle_close_current_task_already_current_decision(
                CloseCurrentTaskAlreadyCurrentDecisionRequest {
                    runtime,
                    context: &locked_context,
                    status: &status,
                    args,
                    authoritative_state,
                    operator: Some(&operator),
                    dispatch_id: &dispatch_id,
                    closure_record_id: &closure_record_id,
                    reviewed_state_id: &reviewed_state_id,
                    raw_reviewed_state_id: &raw_reviewed_state_id,
                    replay_trace_mode: AlreadyCurrentReplayTraceMode::InputEquivalence,
                    negative_blocker_recovery:
                        NegativeResultBlockerRecovery::NegativeResultFollowUp,
                },
            )? {
                return Ok(output);
            }
            let (review_summary_hash, verification_summary_hash) =
                close_current_task_summary_hashes(args)?;
            let _write_authority = claim_step_write_authority(runtime)?;
            record_negative_task_closure(
                authoritative_state,
                NegativeTaskClosureWrite {
                    task: args.task,
                    dispatch_id: &dispatch_id,
                    reviewed_state_id: &reviewed_state_id,
                    semantic_reviewed_state_id: Some(&reviewed_state_id),
                    contract_identity: &contract_identity,
                    review_result: args.review_result.as_str(),
                    review_summary_hash: &review_summary_hash,
                    verification_result,
                    verification_summary_hash: &verification_summary_hash,
                },
            )?;
            let required_follow_up = negative_result_required_follow_up(
                runtime,
                &args.plan,
                &operator,
                Some(authoritative_state),
            );
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                required_follow_up,
            );
            Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "validated",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: recovery.recommended_command,
                    recommended_public_command_argv: recovery.recommended_public_command_argv,
                    recommended_public_command_template: recovery
                        .recommended_public_command_template,
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "Task closure remained blocked because the supplied review or verification outcome was not passing.",
                }),
                &operator,
            ))
        }
        CloseCurrentTaskOutcomeClass::Invalid => {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                Some(FollowUpKind::RunVerification.public_token().to_owned()),
            );
            Ok(blocked_close_current_task_output(
                BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "validated",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: recovery.recommended_command,
                    recommended_public_command_argv: recovery.recommended_public_command_argv,
                    recommended_public_command_template: recovery
                        .recommended_public_command_template,
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because a passing task review requires verification before closure recording can continue.",
                },
            ))
        }
    }
}

#[derive(Clone, Copy)]
enum AlreadyCurrentReplayTraceMode {
    InputEquivalence,
    RecordedPositiveRefresh,
}

impl AlreadyCurrentReplayTraceMode {
    const fn trace_summary(
        self,
        replay_kind: AlreadyCurrentCloseCurrentTaskReplayKind,
    ) -> &'static str {
        if matches!(
            replay_kind,
            AlreadyCurrentCloseCurrentTaskReplayKind::PositiveWithoutSummaries
        ) {
            return ALREADY_CURRENT_SUMMARY_UNAVAILABLE_TRACE;
        }
        match (self, replay_kind.summary_drift_ignored()) {
            (Self::InputEquivalence, false) => ALREADY_CURRENT_EQUIVALENT_TRACE,
            (Self::InputEquivalence, true) => ALREADY_CURRENT_SUMMARY_DRIFT_TRACE,
            (Self::RecordedPositiveRefresh, _) => ALREADY_CURRENT_TASK_CLOSURE_RECORDED_TRACE,
        }
    }
}

#[derive(Clone, Copy)]
enum NegativeResultBlockerRecovery {
    CloseCurrentTask,
    NegativeResultFollowUp,
}

struct CloseCurrentTaskAlreadyCurrentDecisionRequest<'a> {
    runtime: &'a ExecutionRuntime,
    context: &'a ExecutionContext,
    status: &'a PlanExecutionStatus,
    args: &'a CloseCurrentTaskArgs,
    authoritative_state: &'a mut AuthoritativeTransitionState,
    operator: Option<&'a ExecutionRoutingState>,
    dispatch_id: &'a str,
    closure_record_id: &'a str,
    reviewed_state_id: &'a str,
    raw_reviewed_state_id: &'a str,
    replay_trace_mode: AlreadyCurrentReplayTraceMode,
    negative_blocker_recovery: NegativeResultBlockerRecovery,
}

fn handle_close_current_task_already_current_decision(
    request: CloseCurrentTaskAlreadyCurrentDecisionRequest<'_>,
) -> Result<Option<CloseCurrentTaskOutput>, JsonFailure> {
    if task_closure_negative_result_blocks_reviewed_state(
        request.authoritative_state,
        request.args.task,
        request.reviewed_state_id,
    ) {
        return close_current_task_negative_result_blocker_output(
            request.runtime,
            request.args,
            request.operator,
            request.authoritative_state,
            request.negative_blocker_recovery,
        )
        .map(Some);
    }

    if let Some(current_record) = request
        .authoritative_state
        .current_task_closure_result(request.args.task)
        .filter(|record| {
            record.closure_record_id == request.closure_record_id
                && record.dispatch_id == request.dispatch_id
        })
    {
        return close_current_task_current_closure_decision(request, current_record).map(Some);
    }

    Ok(None)
}

fn close_current_task_current_closure_decision(
    request: CloseCurrentTaskAlreadyCurrentDecisionRequest<'_>,
    current_record: CurrentTaskClosureRecord,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    let postconditions_would_mutate = current_task_closure_postconditions_would_mutate(
        request.authoritative_state,
        request.args.task,
        request.closure_record_id,
        &current_record.reviewed_state_id,
    );
    let cleanup_would_mutate = close_current_task_worktree_lease_cleanup_would_require_authority(
        request.runtime,
        request.context,
        request.args.task,
    );
    let replay_decision = close_current_task_current_closure_replay_decision(
        &request,
        &current_record,
        postconditions_would_mutate,
        cleanup_would_mutate,
    )?;
    let AlreadyCurrentCloseCurrentTaskReplayDecision::Replay(replay_kind) = replay_decision else {
        return close_current_task_conflict_output(
            request.runtime,
            request.args,
            request.operator,
            request.closure_record_id,
        );
    };

    let replay_authorization = authorize_already_current_close_current_task_replay(
        request.status,
        request.args,
        replay_kind,
        postconditions_would_mutate,
        cleanup_would_mutate,
    )?;
    if replay_authorization.allows_mutation() && cleanup_would_mutate {
        release_resolved_worktree_leases_after_current_task_closure(
            request.runtime,
            request.context,
            request.args.task,
            request.closure_record_id,
            CloseCurrentTaskClosureCleanupState::AlreadyCurrent,
        )?;
    }

    let mut reason_codes = if postconditions_would_mutate {
        let _write_authority = claim_step_write_authority(request.runtime)?;
        resolve_already_current_task_closure_postconditions(
            request.context,
            request.authoritative_state,
            request.args.task,
            request.closure_record_id,
        )?
    } else {
        Vec::new()
    };
    if replay_kind.summary_drift_ignored() {
        reason_codes.push(String::from("summary_hash_drift_ignored"));
    }
    if replay_kind.summary_artifact_unavailable_ignored() {
        reason_codes.push(String::from(
            "summary_artifact_unavailable_ignored_for_current_positive_closure",
        ));
    }
    Ok(close_current_task_already_current_output(
        request.args.task,
        request.closure_record_id.to_owned(),
        request.replay_trace_mode.trace_summary(replay_kind),
        reason_codes,
    ))
}

fn close_current_task_current_closure_replay_decision(
    request: &CloseCurrentTaskAlreadyCurrentDecisionRequest<'_>,
    current_record: &CurrentTaskClosureRecord,
    postconditions_would_mutate: bool,
    cleanup_would_mutate: bool,
) -> Result<AlreadyCurrentCloseCurrentTaskReplayDecision, JsonFailure> {
    let summary_hashes = close_current_task_already_current_summary_hashes(
        request,
        current_record,
        postconditions_would_mutate,
        cleanup_would_mutate,
    )?;
    let Some((review_summary_hash, verification_summary_hash)) = summary_hashes else {
        return Ok(AlreadyCurrentCloseCurrentTaskReplayDecision::Replay(
            AlreadyCurrentCloseCurrentTaskReplayKind::PositiveWithoutSummaries,
        ));
    };
    if current_record.review_result == request.args.review_result.as_str()
        && current_record.review_summary_hash == review_summary_hash.as_str()
        && current_record.verification_result == request.args.verification_result.as_str()
        && current_record.verification_summary_hash == verification_summary_hash.as_str()
    {
        return Ok(AlreadyCurrentCloseCurrentTaskReplayDecision::Replay(
            AlreadyCurrentCloseCurrentTaskReplayKind::Exact,
        ));
    }
    if current_positive_closure_matches_incoming_results(
        current_record,
        request.args.review_result.as_str(),
        request.args.verification_result.as_str(),
    ) {
        return Ok(AlreadyCurrentCloseCurrentTaskReplayDecision::Replay(
            AlreadyCurrentCloseCurrentTaskReplayKind::PositiveSummaryDrift,
        ));
    }
    Ok(AlreadyCurrentCloseCurrentTaskReplayDecision::Conflict)
}

fn close_current_task_already_current_summary_hashes(
    request: &CloseCurrentTaskAlreadyCurrentDecisionRequest<'_>,
    current_record: &CurrentTaskClosureRecord,
    postconditions_would_mutate: bool,
    cleanup_would_mutate: bool,
) -> Result<Option<(String, String)>, JsonFailure> {
    match already_current_positive_replay_summary_decision(
        AlreadyCurrentPositiveReplaySummaryRequest {
            args: request.args,
            current_record,
            closure_record_id: request.closure_record_id,
            dispatch_id: request.dispatch_id,
            reviewed_state_id: request.reviewed_state_id,
            raw_reviewed_state_id: request.raw_reviewed_state_id,
            postconditions_would_mutate,
            cleanup_would_mutate,
        },
    )? {
        AlreadyCurrentPositiveReplaySummaryDecision::UseAvailableSummaries {
            review_summary_hash,
            verification_summary_hash,
        } => Ok(Some((review_summary_hash, verification_summary_hash))),
        AlreadyCurrentPositiveReplaySummaryDecision::ReplayWithoutSummaries => Ok(None),
        AlreadyCurrentPositiveReplaySummaryDecision::NeedsStrictSummaryValidation => {
            close_current_task_summary_hashes(request.args).map(Some)
        }
    }
}

fn close_current_task_conflict_output(
    runtime: &ExecutionRuntime,
    args: &CloseCurrentTaskArgs,
    operator: Option<&ExecutionRoutingState>,
    closure_record_id: &str,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    with_close_current_task_blocker_operator(runtime, &args.plan, operator, |operator| {
        let recovery = public_recovery_contract_for_follow_up(
            &args.plan,
            Some(operator),
            Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_EXECUTION_REENTRY,
            )),
        );
        with_close_current_task_operator_blocker_metadata(
            blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                task_number: args.task,
                dispatch_validation_action: "validated",
                task_closure_status: "current",
                closure_record_id: Some(closure_record_id.to_owned()),
                code: None,
                recommended_command: recovery.recommended_command,
                recommended_public_command_argv: recovery.recommended_public_command_argv,
                recommended_public_command_template: recovery.recommended_public_command_template,
                required_inputs: recovery.required_inputs,
                rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                required_follow_up: recovery.required_follow_up,
                trace_summary: CLOSE_CURRENT_TASK_CONFLICT_TRACE,
            }),
            operator,
        )
    })
}

fn close_current_task_negative_result_blocker_output(
    runtime: &ExecutionRuntime,
    args: &CloseCurrentTaskArgs,
    operator: Option<&ExecutionRoutingState>,
    authoritative_state: &AuthoritativeTransitionState,
    recovery_mode: NegativeResultBlockerRecovery,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    with_close_current_task_blocker_operator(runtime, &args.plan, operator, |operator| {
        let recovery = match recovery_mode {
            NegativeResultBlockerRecovery::CloseCurrentTask => {
                close_current_task_recovery_contract(&args.plan, operator)
            }
            NegativeResultBlockerRecovery::NegativeResultFollowUp => {
                let required_follow_up = negative_result_required_follow_up(
                    runtime,
                    &args.plan,
                    operator,
                    Some(authoritative_state),
                );
                public_recovery_contract_for_follow_up(
                    &args.plan,
                    Some(operator),
                    required_follow_up,
                )
            }
        };
        with_close_current_task_operator_blocker_metadata(
            blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                task_number: args.task,
                dispatch_validation_action: "validated",
                task_closure_status: "not_current",
                closure_record_id: None,
                code: None,
                recommended_command: recovery.recommended_command,
                recommended_public_command_argv: recovery.recommended_public_command_argv,
                recommended_public_command_template: recovery.recommended_public_command_template,
                required_inputs: recovery.required_inputs,
                rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                required_follow_up: recovery.required_follow_up,
                trace_summary: NEGATIVE_RESULT_BLOCKER_TRACE,
            }),
            operator,
        )
    })
}

fn with_close_current_task_blocker_operator<F>(
    runtime: &ExecutionRuntime,
    plan: &Path,
    operator: Option<&ExecutionRoutingState>,
    build: F,
) -> Result<CloseCurrentTaskOutput, JsonFailure>
where
    F: FnOnce(&ExecutionRoutingState) -> CloseCurrentTaskOutput,
{
    if let Some(operator) = operator {
        return Ok(build(operator));
    }
    let operator = current_workflow_operator(runtime, plan, true)?;
    Ok(build(&operator))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AlreadyCurrentCloseCurrentTaskReplayAuthorization {
    MutationAllowed,
    ReadOnlyNoop,
}

impl AlreadyCurrentCloseCurrentTaskReplayAuthorization {
    const fn allows_mutation(self) -> bool {
        matches!(self, Self::MutationAllowed)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AlreadyCurrentCloseCurrentTaskReplayKind {
    Exact,
    PositiveSummaryDrift,
    PositiveWithoutSummaries,
}

impl AlreadyCurrentCloseCurrentTaskReplayKind {
    const fn summary_drift_ignored(self) -> bool {
        matches!(self, Self::PositiveSummaryDrift)
    }

    const fn summary_artifact_unavailable_ignored(self) -> bool {
        matches!(self, Self::PositiveWithoutSummaries)
    }

    const fn allows_read_only_noop(self) -> bool {
        matches!(
            self,
            Self::Exact | Self::PositiveSummaryDrift | Self::PositiveWithoutSummaries
        )
    }
}

enum AlreadyCurrentCloseCurrentTaskReplayDecision {
    Replay(AlreadyCurrentCloseCurrentTaskReplayKind),
    Conflict,
}

enum AlreadyCurrentPositiveReplaySummaryDecision {
    UseAvailableSummaries {
        review_summary_hash: String,
        verification_summary_hash: String,
    },
    ReplayWithoutSummaries,
    NeedsStrictSummaryValidation,
}

struct AlreadyCurrentPositiveReplaySummaryRequest<'a> {
    args: &'a CloseCurrentTaskArgs,
    current_record: &'a CurrentTaskClosureRecord,
    closure_record_id: &'a str,
    dispatch_id: &'a str,
    reviewed_state_id: &'a str,
    raw_reviewed_state_id: &'a str,
    postconditions_would_mutate: bool,
    cleanup_would_mutate: bool,
}

fn already_current_positive_replay_summary_decision(
    request: AlreadyCurrentPositiveReplaySummaryRequest<'_>,
) -> Result<AlreadyCurrentPositiveReplaySummaryDecision, JsonFailure> {
    if !current_positive_closure_replay_matches_runtime_identity(&request) {
        return Ok(AlreadyCurrentPositiveReplaySummaryDecision::NeedsStrictSummaryValidation);
    }

    if request.postconditions_would_mutate || request.cleanup_would_mutate {
        return Ok(AlreadyCurrentPositiveReplaySummaryDecision::NeedsStrictSummaryValidation);
    }

    if let Some((review_summary_hash, verification_summary_hash)) =
        optional_close_current_task_summary_hashes(request.args)
    {
        return Ok(
            AlreadyCurrentPositiveReplaySummaryDecision::UseAvailableSummaries {
                review_summary_hash,
                verification_summary_hash,
            },
        );
    }

    Ok(AlreadyCurrentPositiveReplaySummaryDecision::ReplayWithoutSummaries)
}

fn current_positive_closure_replay_matches_runtime_identity(
    request: &AlreadyCurrentPositiveReplaySummaryRequest<'_>,
) -> bool {
    request.current_record.task == request.args.task
        && request.current_record.closure_record_id == request.closure_record_id
        && request.current_record.dispatch_id == request.dispatch_id
        && current_task_closure_record_matches_reviewed_state(
            request.current_record,
            request.reviewed_state_id,
            request.raw_reviewed_state_id,
        )
        && current_positive_closure_matches_incoming_results(
            request.current_record,
            request.args.review_result.as_str(),
            request.args.verification_result.as_str(),
        )
}

fn current_task_closure_record_matches_reviewed_state(
    current_record: &CurrentTaskClosureRecord,
    reviewed_state_id: &str,
    raw_reviewed_state_id: &str,
) -> bool {
    if let Some(semantic_reviewed_state_id) = current_record
        .semantic_reviewed_state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return semantic_reviewed_state_id == reviewed_state_id.trim();
    }
    let recorded_reviewed_state_id = current_record.reviewed_state_id.trim();
    recorded_reviewed_state_id == raw_reviewed_state_id.trim()
        || recorded_reviewed_state_id == reviewed_state_id.trim()
}

fn authorize_already_current_close_current_task_replay(
    status: &PlanExecutionStatus,
    args: &CloseCurrentTaskArgs,
    replay_kind: AlreadyCurrentCloseCurrentTaskReplayKind,
    postconditions_would_mutate: bool,
    cleanup_would_mutate: bool,
) -> Result<AlreadyCurrentCloseCurrentTaskReplayAuthorization, JsonFailure> {
    match require_close_current_task_public_mutation(status, args.task) {
        Ok(()) => Ok(AlreadyCurrentCloseCurrentTaskReplayAuthorization::MutationAllowed),
        Err(_)
            if !postconditions_would_mutate
                && !cleanup_would_mutate
                && replay_kind.allows_read_only_noop() =>
        {
            Ok(AlreadyCurrentCloseCurrentTaskReplayAuthorization::ReadOnlyNoop)
        }
        Err(failure) => Err(failure),
    }
}

fn close_current_task_worktree_lease_cleanup_would_require_authority(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    task_number: u32,
) -> bool {
    current_task_closure_worktree_lease_cleanup_would_mutate(runtime, context, task_number)
        .unwrap_or(true)
}

#[derive(Clone, Copy)]
enum CloseCurrentTaskClosureCleanupState {
    AlreadyCurrent,
    Recorded,
}

impl CloseCurrentTaskClosureCleanupState {
    const fn diagnostic_phrase(self) -> &'static str {
        match self {
            Self::AlreadyCurrent => "found an already-current task closure",
            Self::Recorded => "recorded a current task closure",
        }
    }
}

fn release_resolved_worktree_leases_after_current_task_closure(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    task_number: u32,
    closure_record_id: &str,
    cleanup_state: CloseCurrentTaskClosureCleanupState,
) -> Result<(), JsonFailure> {
    release_worktree_leases_for_current_task_closure_and_persist(
        runtime,
        context,
        task_number,
        EventCommandOwner::PublicCloseCurrentTask.as_str(),
    )
    .map(|_| ())
    .map_err(|failure| JsonFailure {
        error_class: failure.error_class,
        message: format!(
            "close-current-task {} for Task {task_number} with closure record `{closure_record_id}`, but worktree lease cleanup failed before the command could report clean success. The task closure remains authoritative; rederive the next public route after correcting the authoritative worktree lease index. Underlying cleanup error: {}",
            cleanup_state.diagnostic_phrase(),
            failure.message
        ),
    })
}
