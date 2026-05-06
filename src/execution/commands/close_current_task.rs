use super::common::*;

pub fn close_current_task(
    runtime: &ExecutionRuntime,
    args: &CloseCurrentTaskArgs,
) -> Result<CloseCurrentTaskOutput, JsonFailure> {
    require_close_current_task_public_flags(args)?;
    let initial_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let authoritative_execution_run_id = load_authoritative_transition_state(&initial_context)?
        .as_ref()
        .and_then(|state| state.execution_run_id_opt());
    let status = status_with_shared_routing_or_context(runtime, &args.plan, &initial_context)?;
    let execution_run_id = authoritative_execution_run_id
        .or_else(|| status.execution_run_id.as_ref().map(|value| value.0.clone()))
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires an active execution run identity from authoritative transition state or preflight seed state.",
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
                    Some(String::from("request_external_review")),
                    PublicFollowUpInputProfile::TaskReview { task: args.task },
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
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because the current task review dispatch lineage does not bind a current reviewed state.",
                    },
                ));
            }
            TaskDispatchReviewedStateStatus::StaleReviewedState => {
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    None,
                    Some(String::from("execution_reentry")),
                    PublicFollowUpInputProfile::None,
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
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because tracked workspace state changed after the current task review dispatch was recorded.",
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
        if task_closure_negative_result_blocks_reviewed_state(
            authoritative_state,
            args.task,
            &initial_reviewed_state_id,
        ) {
            let operator = current_workflow_operator(runtime, &args.plan, true)?;
            let recovery = close_current_task_recovery_contract(&args.plan, &operator, args.task);
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "validated",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: recovery.recommended_command,
                    recommended_public_command_argv: recovery.recommended_public_command_argv,
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                }),
                &operator,
            ));
        }
        if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
            && current_record.closure_record_id == initial_closure_record_id
            && current_record.dispatch_id == dispatch_id
        {
            let (review_summary_hash, verification_summary_hash) =
                close_current_task_summary_hashes(args)?;
            if current_record.review_result == args.review_result.as_str()
                && current_record.review_summary_hash == review_summary_hash.as_str()
                && current_record.verification_result == verification_result
                && current_record.verification_summary_hash == verification_summary_hash.as_str()
            {
                let postconditions_would_mutate = current_task_closure_postconditions_would_mutate(
                    authoritative_state,
                    args.task,
                    &initial_closure_record_id,
                    &current_record.reviewed_state_id,
                );
                let reason_codes = if postconditions_would_mutate {
                    let _write_authority = claim_step_write_authority(runtime)?;
                    resolve_already_current_task_closure_postconditions(
                        &initial_context,
                        authoritative_state,
                        args.task,
                        &initial_closure_record_id,
                    )?
                } else {
                    Vec::new()
                };
                return Ok(close_current_task_already_current_output(
                    args.task,
                    initial_closure_record_id,
                    "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                    reason_codes,
                ));
            } else if current_positive_closure_matches_incoming_results(
                &current_record,
                args.review_result.as_str(),
                verification_result,
            ) {
                let postconditions_would_mutate = current_task_closure_postconditions_would_mutate(
                    authoritative_state,
                    args.task,
                    &initial_closure_record_id,
                    &current_record.reviewed_state_id,
                );
                let mut reason_codes = if postconditions_would_mutate {
                    let _write_authority = claim_step_write_authority(runtime)?;
                    resolve_already_current_task_closure_postconditions(
                        &initial_context,
                        authoritative_state,
                        args.task,
                        &initial_closure_record_id,
                    )?
                } else {
                    Vec::new()
                };
                reason_codes.push(String::from("summary_hash_drift_ignored"));
                return Ok(close_current_task_already_current_output(
                    args.task,
                    initial_closure_record_id,
                    "Current task already has a positive recorded task closure for the supplied dispatch lineage; summary-only drift was ignored.",
                    reason_codes,
                ));
            } else {
                let operator = current_workflow_operator(runtime, &args.plan, true)?;
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    Some(&operator),
                    Some(String::from("execution_reentry")),
                    PublicFollowUpInputProfile::None,
                );
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(initial_closure_record_id),
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
        }
    }
    let mut summary_hashes = None;
    let dispatch_id = if let Some(dispatch_id) = candidate_dispatch_id {
        dispatch_id
    } else {
        // Historical stale/missing dispatch lineage is a status/operator
        // diagnostic. The public mutation path refreshes current dispatch
        // authority here, then the post-refresh checks below fail closed if
        // the runtime still cannot produce current binding.
        ensure_current_review_dispatch_id(
            &initial_context,
            ReviewDispatchScopeArg::Task,
            Some(args.task),
            None,
        )?
    };
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
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
                Some(String::from("request_external_review")),
                PublicFollowUpInputProfile::TaskReview { task: args.task },
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
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because the current task review dispatch lineage does not bind a current reviewed state.",
                }),
                &operator,
            ));
        }
        TaskDispatchReviewedStateStatus::StaleReviewedState => {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                Some(String::from("execution_reentry")),
                PublicFollowUpInputProfile::None,
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
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because tracked workspace state changed after the current task review dispatch was recorded.",
                }),
                &operator,
            ));
        }
    }
    if summary_hashes.is_none() {
        summary_hashes = Some(close_current_task_summary_hashes(args)?);
    }
    let (review_summary_hash, verification_summary_hash) = summary_hashes
        .as_ref()
        .expect("summary hashes should exist after summary validation");
    {
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "close-current-task requires authoritative harness state.",
            ));
        };
        if task_closure_negative_result_blocks_reviewed_state(
            authoritative_state,
            args.task,
            &reviewed_state_id,
        ) {
            let operator = current_workflow_operator(runtime, &args.plan, true)?;
            let recovery = close_current_task_recovery_contract(&args.plan, &operator, args.task);
            return Ok(with_close_current_task_operator_blocker_metadata(
                blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                    task_number: args.task,
                    dispatch_validation_action: "validated",
                    task_closure_status: "not_current",
                    closure_record_id: None,
                    code: None,
                    recommended_command: recovery.recommended_command,
                    recommended_public_command_argv: recovery.recommended_public_command_argv,
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                }),
                &operator,
            ));
        }
        if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
            && current_record.closure_record_id == closure_record_id
            && current_record.dispatch_id == dispatch_id
        {
            if current_record.review_result == args.review_result.as_str()
                && current_record.review_summary_hash == review_summary_hash.as_str()
                && current_record.verification_result == verification_result
                && current_record.verification_summary_hash == verification_summary_hash.as_str()
            {
                let postconditions_would_mutate = current_task_closure_postconditions_would_mutate(
                    authoritative_state,
                    args.task,
                    &closure_record_id,
                    &current_record.reviewed_state_id,
                );
                let reason_codes = if postconditions_would_mutate {
                    let _write_authority = claim_step_write_authority(runtime)?;
                    resolve_already_current_task_closure_postconditions(
                        &context,
                        authoritative_state,
                        args.task,
                        &closure_record_id,
                    )?
                } else {
                    Vec::new()
                };
                return Ok(close_current_task_already_current_output(
                    args.task,
                    closure_record_id,
                    "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                    reason_codes,
                ));
            } else if current_positive_closure_matches_incoming_results(
                &current_record,
                args.review_result.as_str(),
                verification_result,
            ) {
                let postconditions_would_mutate = current_task_closure_postconditions_would_mutate(
                    authoritative_state,
                    args.task,
                    &closure_record_id,
                    &current_record.reviewed_state_id,
                );
                let mut reason_codes = if postconditions_would_mutate {
                    let _write_authority = claim_step_write_authority(runtime)?;
                    resolve_already_current_task_closure_postconditions(
                        &context,
                        authoritative_state,
                        args.task,
                        &closure_record_id,
                    )?
                } else {
                    Vec::new()
                };
                reason_codes.push(String::from("summary_hash_drift_ignored"));
                return Ok(close_current_task_already_current_output(
                    args.task,
                    closure_record_id,
                    "Current task already has a positive recorded task closure for the supplied dispatch lineage; summary-only drift was ignored.",
                    reason_codes,
                ));
            } else {
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    Some(&operator),
                    Some(String::from("execution_reentry")),
                    PublicFollowUpInputProfile::None,
                );
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(closure_record_id),
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
        }
    }
    match close_current_task_outcome_class(args.review_result, args.verification_result) {
        CloseCurrentTaskOutcomeClass::Positive => {
            let effective_reviewed_surface_paths =
                current_task_effective_reviewed_surface_paths(&context, args.task)?;
            require_close_current_task_public_mutation(&status, args.task)?;
            refresh_task_closure_authoritative_lineage_with_context(
                runtime,
                &context,
                TaskClosureLineageRefresh {
                    task: args.task,
                    claim_write_authority: true,
                },
            )?;
            let _write_authority = claim_step_write_authority(runtime)?;
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            let locked_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            ensure_task_dispatch_id_matches(&locked_context, args.task, &dispatch_id)?;
            if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
                && current_record.closure_record_id == closure_record_id
                && current_record.dispatch_id == dispatch_id
            {
                if current_record.review_result == "pass"
                    && current_record.verification_result == "pass"
                {
                    let mut reason_codes = resolve_already_current_task_closure_postconditions(
                        &locked_context,
                        authoritative_state,
                        args.task,
                        &closure_record_id,
                    )?;
                    if current_record.review_summary_hash != review_summary_hash.as_str()
                        || current_record.verification_summary_hash
                            != verification_summary_hash.as_str()
                    {
                        reason_codes.push(String::from("summary_hash_drift_ignored"));
                    }
                    return Ok(close_current_task_already_current_output(
                        args.task,
                        closure_record_id,
                        ALREADY_CURRENT_TASK_CLOSURE_RECORDED_TRACE,
                        reason_codes,
                    ));
                }
                if current_record.review_result == args.review_result.as_str()
                    && current_record.review_summary_hash == review_summary_hash.as_str()
                    && current_record.verification_result == verification_result
                    && current_record.verification_summary_hash
                        == verification_summary_hash.as_str()
                {
                    let reason_codes = resolve_already_current_task_closure_postconditions(
                        &locked_context,
                        authoritative_state,
                        args.task,
                        &closure_record_id,
                    )?;
                    return Ok(close_current_task_already_current_output(
                        args.task,
                        closure_record_id,
                        "Current task already has an equivalent recorded task closure for the supplied dispatch lineage.",
                        reason_codes,
                    ));
                }
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    Some(&operator),
                    Some(String::from("execution_reentry")),
                    PublicFollowUpInputProfile::None,
                );
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(closure_record_id),
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            if authoritative_state
                .task_closure_negative_result(args.task)
                .is_some_and(|negative_record| {
                    task_closure_negative_result_blocks_current_reviewed_state(
                        negative_record
                            .semantic_reviewed_state_id
                            .as_deref()
                            .unwrap_or(negative_record.reviewed_state_id.as_str()),
                        Some(reviewed_state_id.as_str()),
                    )
                })
            {
                let recovery =
                    close_current_task_recovery_contract(&args.plan, &operator, args.task);
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            let superseded_task_closure_records = superseded_task_closure_records(
                &context,
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
                    review_summary_hash,
                    verification_result,
                    verification_summary_hash,
                    superseded_tasks: &superseded_tasks,
                    superseded_task_closure_ids: &superseded_task_closure_ids,
                },
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
            let _write_authority = claim_step_write_authority(runtime)?;
            let mut authoritative_state = load_authoritative_transition_state(&context)?;
            let Some(authoritative_state) = authoritative_state.as_mut() else {
                return Err(JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "close-current-task requires authoritative harness state.",
                ));
            };
            let locked_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            ensure_task_dispatch_id_matches(&locked_context, args.task, &dispatch_id)?;
            if let Some(current_record) = authoritative_state.current_task_closure_result(args.task)
                && current_record.closure_record_id == closure_record_id
                && current_record.dispatch_id == dispatch_id
            {
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    Some(&operator),
                    Some(String::from("execution_reentry")),
                    PublicFollowUpInputProfile::None,
                );
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "current",
                        closure_record_id: Some(closure_record_id),
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because the current task closure already has conflicting equivalent-state inputs for this dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            if authoritative_state
                .task_closure_negative_result(args.task)
                .is_some_and(|negative_record| {
                    task_closure_negative_result_blocks_current_reviewed_state(
                        negative_record
                            .semantic_reviewed_state_id
                            .as_deref()
                            .unwrap_or(negative_record.reviewed_state_id.as_str()),
                        Some(reviewed_state_id.as_str()),
                    )
                })
            {
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
                    PublicFollowUpInputProfile::TaskReview { task: args.task },
                );
                return Ok(with_close_current_task_operator_blocker_metadata(
                    blocked_close_current_task_output(BlockedCloseCurrentTaskOutputContext {
                        task_number: args.task,
                        dispatch_validation_action: "validated",
                        task_closure_status: "not_current",
                        closure_record_id: None,
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: "close-current-task failed closed because a negative task outcome is already authoritative for this still-current reviewed state and dispatch lineage.",
                    }),
                    &operator,
                ));
            }
            record_negative_task_closure(
                authoritative_state,
                NegativeTaskClosureWrite {
                    task: args.task,
                    dispatch_id: &dispatch_id,
                    reviewed_state_id: &reviewed_state_id,
                    semantic_reviewed_state_id: Some(&reviewed_state_id),
                    contract_identity: &contract_identity,
                    review_result: args.review_result.as_str(),
                    review_summary_hash,
                    verification_result,
                    verification_summary_hash,
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
                PublicFollowUpInputProfile::TaskReview { task: args.task },
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
                Some(String::from("run_verification")),
                PublicFollowUpInputProfile::TaskReview { task: args.task },
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
                    required_inputs: recovery.required_inputs,
                    rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                    required_follow_up: recovery.required_follow_up,
                    trace_summary: "close-current-task failed closed because a passing task review requires verification before closure recording can continue.",
                },
            ))
        }
    }
}
