use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RecordReviewDispatchOutput {
    pub allowed: bool,
    pub failure_class: String,
    pub reason_codes: Vec<String>,
    pub warning_codes: Vec<String>,
    pub diagnostics: Vec<GateDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    pub scope: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<String>,
}

impl ExecutionRuntime {
    pub fn status(&self, args: &StatusArgs) -> Result<PlanExecutionStatus, JsonFailure> {
        let mut read_scope = load_execution_read_scope(self, &args.plan, true)?;
        apply_shared_routing_projection_to_read_scope(
            self,
            &mut read_scope,
            args.external_review_result_ready,
            false,
        )?;
        apply_public_read_invariants_to_status(&mut read_scope.status);
        Ok(read_scope.status)
    }

    pub fn topology_recommendation(
        &self,
        args: &RecommendArgs,
    ) -> Result<RecommendOutput, JsonFailure> {
        let read_scope = load_execution_read_scope(self, &args.plan, true)?;
        let context = read_scope.context;
        if read_scope.status.execution_started == "yes" {
            return Err(JsonFailure::new(
                FailureClass::RecommendAfterExecutionStart,
                "recommend is only valid before execution has started for this plan revision.",
            ));
        }
        let (chunking_strategy, evaluator_policy, reset_policy, review_stack, policy_reason_codes) =
            if let Some(preflight_acceptance) = preflight_acceptance_for_context(&context)? {
                (
                    preflight_acceptance.chunking_strategy,
                    preflight_acceptance.evaluator_policy,
                    preflight_acceptance.reset_policy,
                    preflight_acceptance.review_stack,
                    vec![String::from("reused_preflight_acceptance_policy_tuple")],
                )
            } else {
                (
                    default_preflight_chunking_strategy(),
                    default_preflight_evaluator_policy(),
                    default_preflight_reset_policy(),
                    default_preflight_review_stack(),
                    vec![String::from("default_preflight_policy_tuple")],
                )
            };

        let isolated_agents_available = match args.isolated_agents {
            Some(IsolatedAgentsArg::Available) => "yes",
            Some(IsolatedAgentsArg::Unavailable) => "no",
            None => "unknown",
        };
        let session_intent = args
            .session_intent
            .map(|value| value.as_str())
            .unwrap_or("unknown");
        let workspace_prepared = args
            .workspace_prepared
            .map(|value| value.as_str())
            .unwrap_or("unknown");
        let spec_document = parse_spec_file(&context.source_spec_path).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not analyze execution topology because source spec {} is unreadable: {}",
                    context.source_spec_path.display(),
                    error.message()
                ),
            )
        })?;
        let topology_report = analyze_documents(&spec_document, &context.plan_document);
        let execution_context_key = recommendation_execution_context_key(&context);
        let downgrade_records =
            authoritative_matching_execution_topology_downgrade_records_checked(
                &context,
                &execution_context_key,
            )?;
        let learned_guidance = select_active_learned_topology_guidance(
            &downgrade_records,
            topology_report.plan_revision,
            &execution_context_key,
        );

        let tasks_independent = tasks_are_independent(&context.plan_document);
        let current_parallel_path_ready = topology_report.execution_topology_valid
            && topology_report.parallel_lane_ownership_valid
            && topology_report.parallel_workspace_isolation_valid
            && !topology_report.parallel_worktree_groups.is_empty()
            && tasks_independent
            && isolated_agents_available == "yes"
            && workspace_prepared == "yes";
        let topology_context = TopologySelectionContext {
            execution_context_key,
            tasks_independent,
            isolated_agents_available: isolated_agents_available.to_owned(),
            session_intent: session_intent.to_owned(),
            workspace_prepared: workspace_prepared.to_owned(),
            current_parallel_path_ready,
            learned_guidance,
        };
        let topology_recommendation = recommend_topology(&topology_report, &topology_context);

        Ok(RecommendOutput {
            selected_topology: topology_recommendation.selected_topology,
            recommended_skill: topology_recommendation.recommended_skill,
            reason: topology_recommendation.reason,
            decision_flags: topology_recommendation.decision_flags,
            reason_codes: topology_recommendation.reason_codes,
            learned_downgrade_reused: topology_recommendation.learned_downgrade_reused,
            chunking_strategy,
            evaluator_policy,
            reset_policy,
            review_stack,
            policy_reason_codes,
        })
    }

    pub fn preflight_gate(&self, args: &StatusArgs) -> Result<GateResult, JsonFailure> {
        self.preflight_gate_with_mode(args, true)
    }

    pub fn gate_contract(&self, args: &GateContractArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::gates::gate_contract(self, args)
    }

    pub fn record_contract(&self, args: &RecordContractArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::authority::record_contract(self, args)
    }

    pub fn gate_evaluator(&self, args: &GateEvaluatorArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::gates::gate_evaluator(self, args)
    }

    pub fn record_evaluation(
        &self,
        args: &RecordEvaluationArgs,
    ) -> Result<GateResult, JsonFailure> {
        crate::execution::authority::record_evaluation(self, args)
    }

    pub fn gate_handoff(&self, args: &GateHandoffArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::gates::gate_handoff(self, args)
    }

    pub fn record_handoff(&self, args: &RecordHandoffArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::authority::record_handoff(self, args)
    }

    fn preflight_gate_with_mode(
        &self,
        args: &StatusArgs,
        persist_acceptance: bool,
    ) -> Result<GateResult, JsonFailure> {
        let context = if persist_acceptance {
            load_execution_context_for_exact_plan(self, &args.plan)?
        } else {
            load_execution_read_scope(self, &args.plan, true)?.context
        };
        let gate = preflight_from_context(&context);
        if persist_acceptance && gate.allowed {
            let acceptance = persist_preflight_acceptance(&context)?;
            ensure_preflight_authoritative_bootstrap(
                &context.runtime,
                RunIdentitySnapshot {
                    execution_run_id: acceptance.execution_run_id.clone(),
                    source_plan_path: context.plan_rel.clone(),
                    source_plan_revision: context.plan_document.plan_revision,
                },
                acceptance.chunk_id,
            )?;
        }
        Ok(gate)
    }

    pub fn review_gate(&self, args: &StatusArgs) -> Result<GateResult, JsonFailure> {
        match load_execution_context_for_exact_plan(self, &args.plan) {
            Ok(context) => {
                let gate_preview = gate_review_from_context(&context);
                if let Some(mut gate) = gate_review_command_phase_gate(&context, &gate_preview) {
                    gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
                    gate.current_branch_reviewed_state_id =
                        current_branch_reviewed_state_id(&context);
                    gate.current_branch_closure_id =
                        gate_result_current_branch_closure_id(&context, gate.allowed);
                    gate.finish_review_gate_pass_branch_closure_id =
                        finish_review_gate_pass_branch_closure_id(&context)?;
                    if gate.allowed && gate.current_branch_closure_id.is_none() {
                        gate.current_branch_closure_id =
                            gate.finish_review_gate_pass_branch_closure_id.clone();
                    }
                    if !gate.allowed {
                        if gate_should_rederive_via_workflow_operator(
                            &context,
                            &gate,
                            args.external_review_result_ready,
                        ) {
                            apply_out_of_phase_gate_contract(
                                &context,
                                &mut gate,
                                args.external_review_result_ready,
                            );
                        } else {
                            apply_specific_gate_follow_up_contract(
                                &context,
                                &mut gate,
                                args.external_review_result_ready,
                            );
                        }
                    }
                    return Ok(gate);
                }
                let _write_authority = claim_step_write_authority(self)?;
                let context = load_execution_context_for_exact_plan(self, &args.plan)?;
                let mut gate = gate_review_from_context(&context);
                if gate.allowed {
                    persist_finish_review_gate_pass_checkpoint(&context)?;
                    gate.finish_review_gate_pass_branch_closure_id =
                        load_authoritative_transition_state(&context)?
                            .as_ref()
                            .and_then(|state| state.finish_review_gate_pass_branch_closure_id());
                }
                gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
                gate.current_branch_reviewed_state_id = current_branch_reviewed_state_id(&context);
                gate.current_branch_closure_id =
                    gate_result_current_branch_closure_id(&context, gate.allowed);
                if gate.allowed && gate.current_branch_closure_id.is_none() {
                    gate.current_branch_closure_id =
                        gate.finish_review_gate_pass_branch_closure_id.clone();
                }
                if !gate.allowed {
                    if gate_should_rederive_via_workflow_operator(
                        &context,
                        &gate,
                        args.external_review_result_ready,
                    ) {
                        apply_out_of_phase_gate_contract(
                            &context,
                            &mut gate,
                            args.external_review_result_ready,
                        );
                    } else {
                        apply_specific_gate_follow_up_contract(
                            &context,
                            &mut gate,
                            args.external_review_result_ready,
                        );
                    }
                }
                Ok(gate)
            }
            Err(error) if error.error_class == FailureClass::PlanNotExecutionReady.as_str() => {
                let mut gate = GateState::default();
                gate.fail(
                    FailureClass::PlanNotExecutionReady,
                    "plan_not_execution_ready",
                    error.message,
                    "Refresh the approved plan/spec pair before continuing through workflow/operator or plan execution status.",
                );
                Ok(gate.finish())
            }
            Err(error) => Err(error),
        }
    }

    pub fn record_review_dispatch_authority(
        &self,
        args: &RecordReviewDispatchArgs,
    ) -> Result<RecordReviewDispatchOutput, JsonFailure> {
        let initial_context = match load_execution_context_for_exact_plan(self, &args.plan) {
            Ok(context) => context,
            Err(error) if error.error_class == FailureClass::PlanNotExecutionReady.as_str() => {
                return Ok(record_review_dispatch_blocked_output(
                    args,
                    review_dispatch_plan_not_ready_gate(error.message),
                ));
            }
            Err(error) => return Err(error),
        };
        let cycle_target = review_dispatch_cycle_target(&initial_context);
        if let Err(error) = validate_review_dispatch_request(&initial_context, args, cycle_target) {
            if error.error_class == FailureClass::ExecutionStateNotReady.as_str() {
                return Ok(record_review_dispatch_blocked_output_from_gate(
                    &initial_context,
                    args,
                    review_dispatch_out_of_phase_gate(error.message),
                ));
            }
            return Err(error);
        }
        let gate = review_dispatch_gate_from_context(&initial_context, args, cycle_target);
        if !gate.allowed {
            return Ok(record_review_dispatch_blocked_output_from_gate(
                &initial_context,
                args,
                gate,
            ));
        }
        ensure_review_dispatch_authoritative_bootstrap(&initial_context)?;
        let context = match load_execution_context_for_exact_plan(self, &args.plan) {
            Ok(context) => context,
            Err(error) if error.error_class == FailureClass::PlanNotExecutionReady.as_str() => {
                return Ok(record_review_dispatch_blocked_output(
                    args,
                    review_dispatch_plan_not_ready_gate(error.message),
                ));
            }
            Err(error) => return Err(error),
        };
        let cycle_target = review_dispatch_cycle_target(&context);
        if let Err(error) = validate_review_dispatch_request(&context, args, cycle_target) {
            if error.error_class == FailureClass::ExecutionStateNotReady.as_str() {
                return Ok(record_review_dispatch_blocked_output_from_gate(
                    &context,
                    args,
                    review_dispatch_out_of_phase_gate(error.message),
                ));
            }
            return Err(error);
        }
        let gate = review_dispatch_gate_from_context(&context, args, cycle_target);
        if !gate.allowed {
            return Ok(record_review_dispatch_blocked_output_from_gate(
                &context, args, gate,
            ));
        }
        let action = record_review_dispatch_strategy_checkpoint(&context, args, cycle_target)?;
        let refreshed = load_execution_context_for_exact_plan(self, &args.plan)?;
        let gate = review_dispatch_gate_from_context(&refreshed, args, cycle_target);
        let dispatch_id = match action {
            ReviewDispatchMutationAction::Recorded => {
                current_review_dispatch_id_from_lineage(&refreshed, args)?
            }
            ReviewDispatchMutationAction::AlreadyCurrent => {
                current_review_dispatch_id_if_still_current(&refreshed, args)?
            }
        };
        if dispatch_id.is_none() {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "record-review-dispatch recorded lineage but could not reload the current dispatch id.",
            ));
        }
        Ok(RecordReviewDispatchOutput {
            allowed: gate.allowed,
            failure_class: gate.failure_class.clone(),
            reason_codes: gate.reason_codes.clone(),
            warning_codes: gate.warning_codes.clone(),
            diagnostics: gate.diagnostics.clone(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            scope: review_dispatch_scope_label(args.scope),
            action: match action {
                ReviewDispatchMutationAction::Recorded => String::from("recorded"),
                ReviewDispatchMutationAction::AlreadyCurrent => String::from("already_current"),
            },
            dispatch_id,
            recorded_at: matches!(action, ReviewDispatchMutationAction::Recorded)
                .then(|| Timestamp::now().to_string()),
        })
    }

    pub fn finish_gate(&self, args: &StatusArgs) -> Result<GateResult, JsonFailure> {
        let context = load_execution_context_for_exact_plan(self, &args.plan)?;
        let mut gate = gate_finish_from_context(&context);
        gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
        gate.current_branch_reviewed_state_id = current_branch_reviewed_state_id(&context);
        gate.current_branch_closure_id =
            gate_result_current_branch_closure_id(&context, gate.allowed);
        gate.finish_review_gate_pass_branch_closure_id =
            finish_review_gate_pass_branch_closure_id(&context)?;
        if gate.allowed && gate.current_branch_closure_id.is_none() {
            gate.current_branch_closure_id = gate.finish_review_gate_pass_branch_closure_id.clone();
        }
        if !gate.allowed {
            if gate_should_rederive_via_workflow_operator(
                &context,
                &gate,
                args.external_review_result_ready,
            ) {
                apply_out_of_phase_gate_contract(
                    &context,
                    &mut gate,
                    args.external_review_result_ready,
                );
            } else {
                apply_specific_gate_follow_up_contract(
                    &context,
                    &mut gate,
                    args.external_review_result_ready,
                );
            }
        }
        Ok(gate)
    }
}

fn gate_follow_up_routing_state(
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Option<ExecutionRoutingState> {
    let read_scope =
        load_execution_read_scope(&context.runtime, Path::new(&context.plan_rel), true).ok()?;
    crate::execution::router::project_runtime_routing_state_with_exact_command_requirement(
        &read_scope,
        external_review_result_ready,
        false,
    )
    .ok()
    .map(|(routing, _)| routing)
}

fn required_follow_up_kind_from_routing(routing: &ExecutionRoutingState) -> Option<FollowUpKind> {
    normalize_follow_up_alias(
        required_follow_up_from_routing(routing).as_deref(),
        FollowUpAliasContext::PublicRouting,
    )
}

fn current_branch_closure_missing_gate_follow_up(
    routing: Option<&ExecutionRoutingState>,
) -> FollowUpKind {
    missing_branch_closure_gate_follow_up(
        routing.map(|routing| routing.review_state_status.as_str()),
        routing.and_then(required_follow_up_kind_from_routing),
    )
}

fn gate_should_rederive_via_workflow_operator(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> bool {
    gate.allowed
        || specific_gate_direct_recommended_command(context, gate, external_review_result_ready)
            .is_none()
}

fn specific_gate_reason_is_explicit_direct_follow_up(
    gate: &GateResult,
    routing: Option<&ExecutionRoutingState>,
) -> Option<FollowUpKind> {
    direct_gate_follow_up_from_reason_codes(
        gate.reason_codes.iter().map(String::as_str),
        routing.map(|routing| routing.review_state_status.as_str()),
        routing.and_then(required_follow_up_kind_from_routing),
    )
}

fn specific_gate_reason_is_direct_follow_up(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> Option<FollowUpKind> {
    let routing = gate_follow_up_routing_state(context, external_review_result_ready);
    if let Some(reason) = specific_gate_reason_is_explicit_direct_follow_up(gate, routing.as_ref())
    {
        return Some(reason);
    }
    if let Some(routing) = routing.as_ref() {
        if required_follow_up_kind_from_routing(routing) == Some(FollowUpKind::RepairReviewState) {
            return Some(FollowUpKind::RepairReviewState);
        }
        if routing.review_state_status == "missing_current_closure" {
            return Some(current_branch_closure_missing_gate_follow_up(Some(routing)));
        }
    }
    None
}

fn specific_gate_direct_recommended_command(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> Option<String> {
    let routing = gate_follow_up_routing_state(context, external_review_result_ready);
    if let Some(follow_up) =
        specific_gate_reason_is_explicit_direct_follow_up(gate, routing.as_ref())
        && let Some(command) = materialized_follow_up_kind_command(
            follow_up,
            Path::new(&context.plan_rel),
            external_review_result_ready,
        )
    {
        return Some(command);
    }

    if let Some(route_decision) = routing
        .as_ref()
        .and_then(|routing| routing.route_decision.as_ref())
        && let Some(command) = route_decision.recommended_command.as_deref()
    {
        return Some(command.to_owned());
    }

    specific_gate_reason_is_direct_follow_up(context, gate, external_review_result_ready).and_then(
        |follow_up| {
            materialized_follow_up_kind_command(
                follow_up,
                Path::new(&context.plan_rel),
                external_review_result_ready,
            )
        },
    )
}

fn apply_out_of_phase_gate_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    if let Some(command) = gate_follow_up_routing_state(context, external_review_result_ready)
        .and_then(|routing| routing.route_decision)
        .and_then(|decision| decision.recommended_command)
        .filter(|command| !command.starts_with("featureforge workflow operator --plan "))
    {
        gate.code = None;
        gate.recommended_command = Some(command);
        gate.rederive_via_workflow_operator = None;
        return;
    }
    gate.code = Some(String::from("out_of_phase_requery_required"));
    gate.recommended_command = Some(workflow_operator_requery_command(
        Path::new(&context.plan_rel),
        external_review_result_ready,
    ));
    gate.rederive_via_workflow_operator = Some(true);
}

fn apply_out_of_phase_requery_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    gate.code = Some(String::from("out_of_phase_requery_required"));
    gate.recommended_command = Some(workflow_operator_requery_command(
        Path::new(&context.plan_rel),
        external_review_result_ready,
    ));
    gate.rederive_via_workflow_operator = Some(true);
}

fn apply_specific_gate_follow_up_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    if gate.recommended_command.is_some() {
        return;
    }
    gate.recommended_command =
        specific_gate_direct_recommended_command(context, gate, external_review_result_ready);
}

fn record_review_dispatch_blocked_output(
    args: &RecordReviewDispatchArgs,
    gate: GateResult,
) -> RecordReviewDispatchOutput {
    let GateResult {
        failure_class,
        reason_codes,
        warning_codes,
        diagnostics,
        code,
        recommended_command,
        rederive_via_workflow_operator,
        ..
    } = gate;
    RecordReviewDispatchOutput {
        allowed: false,
        failure_class,
        reason_codes,
        warning_codes,
        diagnostics,
        code,
        recommended_command,
        rederive_via_workflow_operator,
        scope: review_dispatch_scope_label(args.scope),
        action: String::from("blocked"),
        dispatch_id: None,
        recorded_at: None,
    }
}

pub(crate) fn record_review_dispatch_blocked_output_from_gate(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    mut gate: GateResult,
) -> RecordReviewDispatchOutput {
    if matches!(args.scope, ReviewDispatchScopeArg::FinalReview)
        && gate.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "derived_review_state_missing" | "current_branch_reviewed_state_id_missing"
            )
        })
    {
        gate.recommended_command = Some(format!(
            "featureforge plan execution repair-review-state --plan {}",
            context.plan_rel
        ));
    } else if matches!(args.scope, ReviewDispatchScopeArg::FinalReview)
        && gate.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
                    | crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
                    | crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY
            )
        })
    {
        gate.recommended_command = if gate
            .reason_codes
            .iter()
            .any(|code| code == crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS)
        {
            Some(format!(
                "featureforge plan execution advance-late-stage --plan {}",
                context.plan_rel
            ))
        } else {
            Some(format!(
                "featureforge plan execution advance-late-stage --plan {} --result ready|blocked --summary-file <path>",
                context.plan_rel
            ))
        };
    } else {
        let routing = gate_follow_up_routing_state(context, false);
        let direct_follow_up =
            specific_gate_reason_is_explicit_direct_follow_up(&gate, routing.as_ref());
        let task_scope_prior_task_requires_requery =
            matches!(args.scope, ReviewDispatchScopeArg::Task)
                && gate
                    .reason_codes
                    .iter()
                    .any(|code| code.starts_with("prior_task_"));
        if gate.allowed || direct_follow_up.is_none() || task_scope_prior_task_requires_requery {
            apply_out_of_phase_requery_contract(context, &mut gate, false);
        } else {
            gate.recommended_command = match direct_follow_up {
                Some(follow_up) => materialized_follow_up_kind_command(
                    follow_up,
                    Path::new(&context.plan_rel),
                    false,
                ),
                None => None,
            };
        }
    }
    record_review_dispatch_blocked_output(args, gate)
}

fn review_dispatch_scope_label(scope: ReviewDispatchScopeArg) -> String {
    match scope {
        ReviewDispatchScopeArg::Task => String::from("task"),
        ReviewDispatchScopeArg::FinalReview => String::from("final-review"),
    }
}

fn review_dispatch_out_of_phase_gate(message: String) -> GateResult {
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::ExecutionStateNotReady,
        "record_review_dispatch_out_of_phase",
        message,
        "Run `featureforge workflow operator --plan <approved-plan-path>` to re-derive the current workflow phase before recording review dispatch.",
    );
    gate.finish()
}

fn review_dispatch_plan_not_ready_gate(message: String) -> GateResult {
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::PlanNotExecutionReady,
        "plan_not_execution_ready",
        message,
        "Refresh the approved plan/spec pair before continuing through workflow/operator or plan execution status.",
    );
    gate.finish()
}

enum ReviewDispatchMutationAction {
    Recorded,
    AlreadyCurrent,
}

fn gate_review_command_phase_gate(
    context: &ExecutionContext,
    gate_review: &GateResult,
) -> Option<GateResult> {
    if !gate_review.allowed {
        return None;
    }
    let checkpoint_current = matches!(
        finish_review_gate_checkpoint_matches_current_branch_closure(context),
        Ok(true)
    );
    if !checkpoint_current || !gate_finish_from_context(context).allowed {
        return None;
    }
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::ExecutionStateNotReady,
        "finish_review_gate_already_current",
        "gate-review is out of phase because the current branch closure already has a fresh persisted finish-review gate checkpoint.",
        format!(
            "Run `featureforge workflow operator --plan {}` and follow the recommended public next step.",
            context.plan_rel
        ),
    );
    Some(gate.finish())
}

pub(crate) fn ensure_current_review_dispatch_id(
    context: &ExecutionContext,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    expected_dispatch_id: Option<&str>,
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
        return Ok(expected_dispatch_id.to_owned());
    }
    ensure_review_dispatch_authoritative_bootstrap(context)?;
    let action = record_review_dispatch_strategy_checkpoint(context, &args, cycle_target)?;
    let refreshed = load_execution_context_for_exact_plan(&context.runtime, &args.plan)?;
    let dispatch_id = match action {
        ReviewDispatchMutationAction::Recorded => {
            current_review_dispatch_id_from_lineage(&refreshed, &args)?
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

pub(crate) fn current_review_dispatch_id_candidate(
    context: &ExecutionContext,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    expected_dispatch_id: Option<&str>,
) -> Result<Option<String>, JsonFailure> {
    if let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(expected_dispatch_id.to_owned()));
    }
    let args = RecordReviewDispatchArgs {
        plan: PathBuf::from(context.plan_rel.clone()),
        scope,
        task,
    };
    let overlay_dispatch_id = current_review_dispatch_id_if_still_current(context, &args)?;
    if overlay_dispatch_id.is_some() {
        return Ok(overlay_dispatch_id);
    }
    if matches!(scope, ReviewDispatchScopeArg::Task)
        && let Some(task) = task
        && let Some(dispatch_id) = load_authoritative_transition_state(context)?
            .as_ref()
            .and_then(|state| state.task_review_dispatch_id(task))
    {
        return Ok(Some(dispatch_id));
    }
    Ok(None)
}

fn validate_expected_dispatch_id(
    actual_dispatch_id: &str,
    expected_dispatch_id: Option<&str>,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
) -> Result<(), JsonFailure> {
    let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    if actual_dispatch_id.trim() == expected_dispatch_id {
        return Ok(());
    }
    let detail = match scope {
        ReviewDispatchScopeArg::Task => format!(
            "close-current-task expected dispatch `{expected_dispatch_id}` for task {}.",
            task.unwrap_or_default()
        ),
        ReviewDispatchScopeArg::FinalReview => {
            format!("advance-late-stage expected final-review dispatch `{expected_dispatch_id}`.")
        }
    };
    Err(JsonFailure::new(
        FailureClass::InvalidCommandInput,
        format!("dispatch_id_mismatch: {detail}"),
    ))
}

fn recommendation_execution_context_key(context: &ExecutionContext) -> String {
    let base_branch = context
        .current_release_base_branch()
        .unwrap_or_else(|| String::from("unknown"));
    format!("{}@{}", context.runtime.branch_name, base_branch)
}

fn record_review_dispatch_strategy_checkpoint_without_claim(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<ReviewDispatchMutationAction, JsonFailure> {
    if current_review_dispatch_id_if_still_current(context, args)?.is_some() {
        return Ok(ReviewDispatchMutationAction::AlreadyCurrent);
    }
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "Authoritative harness state is required before record-review-dispatch can record review-dispatch proof.",
        ));
    };
    let cycle_target = match cycle_target {
        ReviewDispatchCycleTarget::Bound(_, _)
            if matches!(args.scope, ReviewDispatchScopeArg::FinalReview)
                && context.steps.iter().all(|step| step.checked) =>
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
    authoritative_state
        .persist_if_dirty_with_failpoint_and_command(None, "record_review_dispatch")?;
    Ok(ReviewDispatchMutationAction::Recorded)
}

fn record_review_dispatch_strategy_checkpoint(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<ReviewDispatchMutationAction, JsonFailure> {
    let _ = load_authoritative_transition_state(context)?;
    let _write_authority = claim_step_write_authority(&context.runtime)?;
    record_review_dispatch_strategy_checkpoint_without_claim(context, args, cycle_target)
}

fn ensure_review_dispatch_authoritative_bootstrap(
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

#[derive(Clone, Copy)]
enum ReviewDispatchCycleTarget {
    Bound(u32, u32),
    UnboundCompletedPlan,
    None,
}

fn validate_review_dispatch_request(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<(), JsonFailure> {
    match args.scope {
        ReviewDispatchScopeArg::Task => {
            let requested_task = args.task.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "record-review-dispatch --scope task requires --task <n>.",
                )
            })?;
            let observed_task = match cycle_target {
                ReviewDispatchCycleTarget::Bound(task, _) => task,
                ReviewDispatchCycleTarget::UnboundCompletedPlan => {
                    return Err(JsonFailure::new(
                        FailureClass::InvalidCommandInput,
                        format!(
                            "record-review-dispatch --scope task --task {requested_task} is invalid because the approved plan is already at final-review dispatch scope."
                        ),
                    ));
                }
                ReviewDispatchCycleTarget::None => {
                    return Err(JsonFailure::new(
                        FailureClass::ExecutionStateNotReady,
                        "record-review-dispatch --scope task requires a current task review-dispatch target.",
                    ));
                }
            };
            if requested_task != observed_task {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    format!(
                        "record-review-dispatch --scope task --task {requested_task} does not match the current task review-dispatch target Task {observed_task} for plan {}.",
                        context.plan_rel
                    ),
                ));
            }
            Ok(())
        }
        ReviewDispatchScopeArg::FinalReview => {
            if args.task.is_some() {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "record-review-dispatch --scope final-review does not accept --task.",
                ));
            }
            match cycle_target {
                ReviewDispatchCycleTarget::UnboundCompletedPlan => Ok(()),
                ReviewDispatchCycleTarget::Bound(_, _)
                    if context_all_task_scopes_closed_by_authority(context, None) =>
                {
                    Ok(())
                }
                ReviewDispatchCycleTarget::Bound(_, _) | ReviewDispatchCycleTarget::None => {
                    Err(JsonFailure::new(
                        FailureClass::ExecutionStateNotReady,
                        "record-review-dispatch --scope final-review requires a completed-plan final-review dispatch target.",
                    ))
                }
            }
        }
    }
}

fn review_dispatch_cycle_target(context: &ExecutionContext) -> ReviewDispatchCycleTarget {
    if let Some(boundary_target) = review_dispatch_task_boundary_target(context) {
        return boundary_target;
    }
    for state in [
        NoteState::Active,
        NoteState::Blocked,
        NoteState::Interrupted,
    ] {
        if let Some(step) = active_step(context, state) {
            return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
        }
    }
    if context_all_task_scopes_closed_by_authority(context, None) {
        let overlay = load_status_authoritative_overlay_checked(context)
            .ok()
            .and_then(|overlay| overlay);
        let authoritative_phase = overlay.as_ref().and_then(|overlay| {
            normalize_optional_overlay_value(overlay.harness_phase.as_deref())
                .and_then(parse_harness_phase)
        });
        if authoritative_phase.is_some_and(is_late_stage_phase)
            || overlay
                .as_ref()
                .is_some_and(has_authoritative_late_stage_progress)
        {
            return ReviewDispatchCycleTarget::UnboundCompletedPlan;
        }
        if let Some(final_task) = context.tasks_by_number.keys().copied().max() {
            let final_task_closure_missing = load_authoritative_transition_state(context)
                .ok()
                .and_then(|state| state)
                .and_then(|state| {
                    (!state.current_task_closure_overlay_needs_restore()).then_some(state)
                })
                .and_then(|state| state.raw_current_task_closure_result(final_task))
                .is_none();
            if final_task_closure_missing
                && let Some(final_step) = context
                    .steps
                    .iter()
                    .filter(|step| step.task_number == final_task)
                    .map(|step| step.step_number)
                    .max()
            {
                return ReviewDispatchCycleTarget::Bound(final_task, final_step);
            }
        }
        return ReviewDispatchCycleTarget::UnboundCompletedPlan;
    }
    if let Some(attempt) = context.evidence.attempts.iter().rev().find(|attempt| {
        context.steps.iter().any(|step| {
            step.task_number == attempt.task_number && step.step_number == attempt.step_number
        })
    }) {
        return ReviewDispatchCycleTarget::Bound(attempt.task_number, attempt.step_number);
    }
    if let Some(step) = context.steps.iter().rev().find(|step| step.checked) {
        return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
    }
    if let Some(step) = context
        .steps
        .iter()
        .find(|step| step.note_state.is_some() && !step.checked)
    {
        return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
    }
    if !context.evidence.attempts.is_empty()
        && let Some(attempt) = context.evidence.attempts.last()
    {
        return ReviewDispatchCycleTarget::Bound(attempt.task_number, attempt.step_number);
    }
    ReviewDispatchCycleTarget::None
}

fn review_dispatch_task_boundary_target(
    context: &ExecutionContext,
) -> Option<ReviewDispatchCycleTarget> {
    let status = status_from_context(context).ok();
    let earliest_stale_boundary_task = status
        .as_ref()
        .and_then(|status| pre_reducer_earliest_unresolved_stale_task(context, status));
    if let Some(stale_task) = earliest_stale_boundary_task
        .filter(|task_number| review_dispatch_boundary_blocked_for_task(context, *task_number))
    {
        let step_number = latest_attempted_step_for_task(context, stale_task).or_else(|| {
            context
                .steps
                .iter()
                .filter(|step| step.task_number == stale_task)
                .map(|step| step.step_number)
                .max()
        })?;
        return Some(ReviewDispatchCycleTarget::Bound(stale_task, step_number));
    }
    if let Some(status) = status.as_ref() {
        let boundary_reason_present = status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_current_closure_stale"
                    | "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
                    | "task_cycle_break_active"
            )
        });
        if boundary_reason_present
            && let Some(task_number) = status
                .blocking_task
                .or(status.resume_task)
                .or(status.active_task)
            && review_dispatch_boundary_blocked_for_task(context, task_number)
        {
            let step_number =
                latest_attempted_step_for_task(context, task_number).or_else(|| {
                    context
                        .steps
                        .iter()
                        .filter(|step| step.task_number == task_number)
                        .map(|step| step.step_number)
                        .max()
                })?;
            return Some(ReviewDispatchCycleTarget::Bound(task_number, step_number));
        }
    }
    let task_number = status.as_ref().and_then(|status| {
        context
            .tasks_by_number
            .keys()
            .copied()
            .filter(|candidate_task| {
                review_dispatch_boundary_blocked_for_task(context, *candidate_task)
            })
            .find(|candidate_task| {
                task_closure_baseline_repair_candidate_with_stale_target(
                    context,
                    status,
                    *candidate_task,
                    pre_reducer_earliest_unresolved_stale_task(context, status),
                )
                .ok()
                .flatten()
                .is_some()
            })
    })?;
    let step_number = latest_attempted_step_for_task(context, task_number).or_else(|| {
        context
            .steps
            .iter()
            .filter(|step| step.task_number == task_number)
            .map(|step| step.step_number)
            .max()
    })?;
    Some(ReviewDispatchCycleTarget::Bound(task_number, step_number))
}

fn review_dispatch_boundary_blocked_for_task(context: &ExecutionContext, task_number: u32) -> bool {
    task_closure_recording_prerequisites(context, task_number)
        .ok()
        .is_some_and(|prerequisites| {
            prerequisites
                .dispatch_id
                .as_deref()
                .is_none_or(|dispatch_id| dispatch_id.trim().is_empty())
                || prerequisites
                    .blocking_reason_codes
                    .iter()
                    .any(|reason_code| {
                        matches!(
                            reason_code.as_str(),
                            "prior_task_review_dispatch_missing"
                                | "prior_task_review_dispatch_stale"
                        )
                    })
        })
}

fn review_dispatch_gate_from_context(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> GateResult {
    match args.scope {
        ReviewDispatchScopeArg::Task => {
            let task_number = args.task.or(match cycle_target {
                ReviewDispatchCycleTarget::Bound(task_number, _) => Some(task_number),
                _ => None,
            });
            if let Some(task_number) = task_number {
                return task_review_dispatch_gate_from_context(context, task_number);
            }
        }
        ReviewDispatchScopeArg::FinalReview => {
            return final_review_dispatch_gate_from_context(context);
        }
    }
    gate_review_from_context_internal(context, false)
}

fn final_review_dispatch_gate_from_context(context: &ExecutionContext) -> GateResult {
    let mut gate = GateState::from_result(gate_review_base_result(context, false));
    if !gate.allowed {
        return gate.finish();
    }

    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(state) => state,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unreadable",
                error.message,
                "Restore authoritative harness state readability and retry final-review dispatch.",
            );
            return gate.finish();
        }
    };
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(overlay) => overlay,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_overlay_unreadable",
                error.message,
                "Restore authoritative overlay readability and retry final-review dispatch.",
            );
            return gate.finish();
        }
    };
    let missing_derived_overlays =
        missing_derived_review_state_fields(authoritative_state.as_ref(), overlay.as_ref());
    if missing_derived_overlays.iter().any(|field| {
        matches!(
            field.as_str(),
            "current_branch_closure_id"
                | "current_branch_closure_reviewed_state_id"
                | "current_branch_closure_contract_identity"
        )
    }) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "derived_review_state_missing",
            "Final-review dispatch is blocked because current branch-closure bindings require review-state repair before late-stage progression can continue.",
            format!(
                "Run `featureforge plan execution repair-review-state --plan {}` before dispatching final review.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }
    let Some(current_branch_closure_id) = validated_current_branch_closure_identity(context)
        .map(|identity| identity.branch_closure_id)
    else {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            "Final-review dispatch is blocked because no current reviewed branch closure exists.",
            format!(
                "Run `featureforge plan execution advance-late-stage --plan {}` before dispatching final review.",
                context.plan_rel
            ),
        );
        return gate.finish();
    };
    if current_branch_reviewed_state_id(context).is_none() {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "current_branch_reviewed_state_id_missing",
            "Final-review dispatch is blocked because the current branch-closure reviewed state requires repair before late-stage progression can continue.",
            format!(
                "Run `featureforge plan execution repair-review-state --plan {}` before dispatching final review.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }

    let release_readiness_result = authoritative_state
        .as_ref()
        .and_then(|state| {
            state
                .current_release_readiness_record_id()
                .as_deref()
                .and_then(|record_id| state.release_readiness_record_by_id(record_id))
        })
        .and_then(|record| {
            (record.branch_closure_id == current_branch_closure_id).then_some(record.result)
        });
    if release_readiness_result.as_deref() == Some("blocked") {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
            "Final-review dispatch is blocked because the current branch closure still has a blocked release-readiness result.",
            format!(
                "Run `featureforge plan execution advance-late-stage --plan {} --result ready|blocked --summary-file <path>` after resolving the release blocker.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }
    if release_readiness_result.as_deref() != Some("ready") {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY,
            "Final-review dispatch is blocked because the current branch closure does not yet have a current release-readiness result `ready`.",
            format!(
                "Run `featureforge plan execution advance-late-stage --plan {} --result ready|blocked --summary-file <path>` before dispatching final review.",
                context.plan_rel
            ),
        );
    }
    gate.finish()
}

fn task_review_dispatch_gate_from_context(
    context: &ExecutionContext,
    task_number: u32,
) -> GateResult {
    let mut gate = GateState::default();
    let task_steps: Vec<_> = context
        .steps
        .iter()
        .filter(|step| step.task_number == task_number)
        .collect();
    if task_steps.is_empty() {
        gate.fail(
            FailureClass::InvalidCommandInput,
            "task_not_found",
            format!(
                "Task {task_number} does not exist in the approved plan and cannot be used for record-review-dispatch."
            ),
            "Choose a valid task number from the approved plan.",
        );
        return gate.finish();
    }

    if current_task_closure_overlay_restore_required(context).unwrap_or(false) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "current_task_closure_overlay_restore_required",
            format!(
                "Task {task_number} review dispatch is blocked because current task-closure overlays are missing and must be repaired before recording more review-dispatch lineage for this task."
            ),
            format!(
                "Run `featureforge plan execution repair-review-state --plan {}` before recording more review-dispatch lineage for Task {task_number}.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }

    for state in [
        NoteState::Active,
        NoteState::Blocked,
        NoteState::Interrupted,
    ] {
        if let Some(step) =
            active_step(context, state).filter(|step| step.task_number == task_number)
        {
            let (reason_code, message, remediation) = match state {
                NoteState::Active => (
                    "active_step_in_progress",
                    format!(
                        "Task {task_number} review dispatch is blocked while Step {} remains active.",
                        step.step_number
                    ),
                    "Complete, interrupt, or resolve the active step before dispatching task review.",
                ),
                NoteState::Blocked => (
                    "blocked_step",
                    format!(
                        "Task {task_number} review dispatch is blocked while Step {} remains blocked.",
                        step.step_number
                    ),
                    "Resolve the blocked step before dispatching task review.",
                ),
                NoteState::Interrupted => (
                    "interrupted_work_unresolved",
                    format!(
                        "Task {task_number} review dispatch is blocked while Step {} remains interrupted.",
                        step.step_number
                    ),
                    "Resume or explicitly resolve the interrupted step before dispatching task review.",
                ),
            };
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                reason_code,
                message,
                remediation,
            );
        }
    }

    for step in task_steps {
        if !step.checked {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "unfinished_task_steps_remaining",
                format!(
                    "Task {task_number} review dispatch is blocked while Step {} remains unchecked.",
                    step.step_number
                ),
                "Finish all steps in the task before dispatching task review.",
            );
            continue;
        }
        let Some(attempt) =
            latest_attempt_for_step(&context.evidence, step.task_number, step.step_number)
        else {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {task_number} Step {} is checked but missing execution evidence.",
                    step.step_number
                ),
                "Reopen the step or record matching execution evidence before dispatching task review.",
            );
            continue;
        };
        if attempt.status != "Completed" {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {task_number} Step {} no longer has a completed evidence attempt.",
                    step.step_number
                ),
                "Reopen the step or complete it again with fresh evidence before dispatching task review.",
            );
        }
    }

    match task_current_closure_status(context, task_number) {
        Ok(TaskCurrentClosureStatus::Current) => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "task_current_closure_already_current",
                format!(
                    "Task {task_number} review dispatch is out of phase because Task {task_number} already has a current passing task closure for the active approved plan."
                ),
                "Re-derive the workflow phase before recording more review-dispatch lineage for this task.",
            );
        }
        Ok(TaskCurrentClosureStatus::Missing) => {}
        Ok(TaskCurrentClosureStatus::Stale) => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "prior_task_current_closure_stale",
                format!(
                    "Task {task_number} review dispatch is blocked because Task {task_number} current task closure no longer matches the current reviewed workspace state."
                ),
                format!(
                    "Run `featureforge plan execution repair-review-state --plan {}` before recording fresh review-dispatch lineage for Task {task_number}.",
                    context.plan_rel
                ),
            );
        }
        Err(error) => {
            let failure_class =
                if error.error_class == FailureClass::MalformedExecutionState.as_str() {
                    FailureClass::MalformedExecutionState
                } else {
                    FailureClass::ExecutionStateNotReady
                };
            let reason_code = task_boundary_reason_code_from_message(&error.message)
                .unwrap_or("task_current_closure_state_invalid");
            gate.fail(
                failure_class,
                reason_code,
                format!(
                    "Task {task_number} review dispatch is blocked because the current task-closure state is not trustworthy: {}",
                    error.message
                ),
                "Repair the current task-closure state before recording more review-dispatch lineage for this task.",
            );
        }
    }

    gate.finish()
}

fn select_active_learned_topology_guidance(
    records: &[ExecutionTopologyDowngradeRecord],
    plan_revision: u32,
    execution_context_key: &str,
) -> Option<LearnedTopologyGuidance> {
    records
        .iter()
        .rev()
        .find(|record| {
            record.source_plan_revision == plan_revision
                && record.execution_context_key == execution_context_key
                && !record.rerun_guidance_superseded
        })
        .map(|record| LearnedTopologyGuidance {
            approved_plan_revision: plan_revision,
            execution_context_key: record.execution_context_key.clone(),
            primary_reason_class: record.primary_reason_class.as_str().to_owned(),
        })
}
