use super::{
    BTreeSet, ExecutionContext, ExecutionRoutingState, ExecutionRuntime,
    ExecutionTopologyDowngradeRecord, FailureClass, FollowUpAliasContext, FollowUpKind,
    GateContractArgs, GateDiagnostic, GateEvaluatorArgs, GateHandoffArgs, GateResult, GateState,
    IsolatedAgentsArg, JsonFailure, JsonSchema, LearnedTopologyGuidance, NoteState, Path,
    PlanExecutionStatus, RecommendArgs, RecommendOutput, RecordContractArgs, RecordEvaluationArgs,
    RecordHandoffArgs, RecordReviewDispatchArgs, ReviewDispatchScopeArg, RunIdentitySnapshot,
    Serialize, StatusArgs, TaskCurrentClosureStatus, Timestamp, TopologySelectionContext,
    active_step, analyze_documents, apply_public_read_invariants_to_read_scope,
    apply_shared_routing_projection_to_read_scope, authoritative_completed_steps_for_context,
    authoritative_matching_execution_topology_downgrade_records_checked,
    claim_step_write_authority, current_branch_gate_bindings_from_authoritative_state,
    current_review_dispatch_id_from_lineage, current_review_dispatch_id_if_still_current,
    current_task_closure_overlay_restore_required, default_preflight_chunking_strategy,
    default_preflight_evaluator_policy, default_preflight_reset_policy,
    default_preflight_review_stack, direct_gate_follow_up_from_reason_codes,
    ensure_preflight_authoritative_bootstrap,
    finish_review_gate_checkpoint_matches_current_branch_closure,
    gate_finish_from_context_with_authoritative_state, gate_review_base_result,
    gate_review_from_context_internal, gate_review_from_context_with_authoritative_state,
    load_authoritative_transition_state, load_execution_context_for_exact_plan,
    load_execution_read_scope, load_execution_read_scope_for_mutation, normalize_follow_up_alias,
    parse_spec_file,
    persist_finish_review_gate_pass_checkpoint_for_command_with_authoritative_state,
    persist_preflight_acceptance, preflight_acceptance_for_context, preflight_from_context,
    public_typed_operator_route_remediation_for_plan,
    public_workflow_operator_remediation_for_plan, recommend_topology,
    required_follow_up_from_routing, status_workspace_state_id,
    step_completed_by_authoritative_truth, task_boundary_reason_code_from_message,
    task_current_closure_status, tasks_are_independent, verify_completed_step_evidence_projection,
};
use crate::execution::closure_dispatch::{
    ReviewDispatchCycleTarget, review_dispatch_cycle_target, validate_review_dispatch_request,
};
use crate::execution::closure_dispatch_mutation::{
    ReviewDispatchMutationAction, ensure_review_dispatch_authoritative_bootstrap,
    record_review_dispatch_strategy_checkpoint,
};
use crate::execution::command_eligibility::PublicCommandInputRequirement;
use crate::execution::event_command::EventCommandOwner;
use crate::execution::gate_reason_codes::{
    FINISH_REVIEW_GATE_ALREADY_CURRENT, finish_review_gate_already_current_reason_code,
};
use crate::execution::implementation_gate::apply_pre_execution_plan_fidelity_gate;
use crate::execution::public_command_types::RecommendedPublicCommandTemplate;
use crate::execution::review_route_tokens::{
    FOLLOW_UP_GATE_REVIEW, OUT_OF_PHASE_REQUERY_REQUIRED_CODE,
};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_public_command_template: RecommendedPublicCommandTemplate,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_inputs: Vec<PublicCommandInputRequirement>,
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
            true,
        )?;
        apply_pre_execution_plan_fidelity_gate(&read_scope.context, &mut read_scope.status);
        apply_public_read_invariants_to_read_scope(&mut read_scope);
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
                let authoritative_state = load_authoritative_transition_state(&context);
                let gate_preview = gate_review_from_context_with_authoritative_state(
                    &context,
                    authoritative_state.as_ref().map(|state| state.as_ref()),
                    true,
                );
                if let Some(mut gate) = gate_review_command_phase_gate(
                    &context,
                    authoritative_state.as_ref().map(|state| state.as_ref()),
                    &gate_preview,
                ) {
                    gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
                    apply_current_branch_gate_bindings(
                        &context,
                        &mut gate,
                        authoritative_state.as_ref().map(|state| state.as_ref()),
                    )?;
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
                let mut authoritative_state = load_authoritative_transition_state(&context);
                let mut gate = gate_review_from_context_with_authoritative_state(
                    &context,
                    authoritative_state.as_ref().map(|state| state.as_ref()),
                    true,
                );
                if gate.allowed {
                    persist_finish_review_gate_pass_checkpoint_for_command_with_authoritative_state(
                        &context,
                        FOLLOW_UP_GATE_REVIEW,
                        &mut authoritative_state,
                    )?;
                }
                gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
                apply_current_branch_gate_bindings(
                    &context,
                    &mut gate,
                    authoritative_state.as_ref().map(|state| state.as_ref()),
                )?;
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
                    review_dispatch_out_of_phase_gate(&initial_context, error.message),
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
        ensure_review_dispatch_authoritative_bootstrap(
            &initial_context,
            EventCommandOwner::InternalRecordReviewDispatch,
        )?;
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
                    review_dispatch_out_of_phase_gate(&context, error.message),
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
                "Runtime review state was updated but the current dispatch id could not be reloaded. Re-query workflow operator/status and follow its typed public route.",
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
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
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
        let authoritative_state = load_authoritative_transition_state(&context);
        let mut gate = gate_finish_from_context_with_authoritative_state(
            &context,
            authoritative_state.as_ref().map(|state| state.as_ref()),
        );
        gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
        apply_current_branch_gate_bindings(
            &context,
            &mut gate,
            authoritative_state.as_ref().map(|state| state.as_ref()),
        )?;
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

fn apply_current_branch_gate_bindings(
    context: &ExecutionContext,
    gate: &mut GateResult,
    authoritative_state: super::AuthoritativeTransitionStateRef<'_>,
) -> Result<(), JsonFailure> {
    let bindings = match authoritative_state {
        Ok(authoritative_state) => current_branch_gate_bindings_from_authoritative_state(
            context,
            authoritative_state,
            gate.allowed,
        ),
        Err(_) if !gate.allowed => {
            gate.current_branch_reviewed_state_id = None;
            gate.current_branch_closure_id = None;
            gate.finish_review_gate_pass_branch_closure_id = None;
            return Ok(());
        }
        Err(error) => return Err(error.clone()),
    };
    gate.current_branch_reviewed_state_id = bindings.current_branch_reviewed_state_id;
    gate.current_branch_closure_id = bindings.current_branch_closure_id;
    gate.finish_review_gate_pass_branch_closure_id =
        bindings.finish_review_gate_pass_branch_closure_id;
    if gate.allowed && gate.current_branch_closure_id.is_none() {
        gate.current_branch_closure_id = gate.finish_review_gate_pass_branch_closure_id.clone();
    }
    Ok(())
}

fn gate_follow_up_routing_state(
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Option<ExecutionRoutingState> {
    let read_scope = load_execution_read_scope_for_mutation(
        &context.runtime,
        Path::new(&context.plan_rel),
        true,
    )
    .ok()?;
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

fn gate_should_rederive_via_workflow_operator(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> bool {
    if gate
        .reason_codes
        .iter()
        .map(String::as_str)
        .any(finish_review_gate_already_current_reason_code)
    {
        return true;
    }
    gate.allowed
        || specific_gate_direct_recommendation(context, gate, external_review_result_ready)
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

#[derive(Debug, Clone)]
struct SpecificGateRecommendation {
    recommended_public_command_template: RecommendedPublicCommandTemplate,
    required_inputs: Vec<PublicCommandInputRequirement>,
}

impl SpecificGateRecommendation {
    fn from_routing_state(routing: Option<&ExecutionRoutingState>) -> Option<Self> {
        routing
            .and_then(|routing| routing.route_decision.as_ref())
            .and_then(Self::from_route_decision)
    }

    fn from_route_decision(
        route_decision: &crate::execution::route_plan::RouteDecision,
    ) -> Option<Self> {
        let recommended_public_command_template = route_decision.public_command_template();
        let required_inputs = route_decision.required_inputs.clone();
        (recommended_public_command_template.is_some() || !required_inputs.is_empty()).then_some(
            Self {
                recommended_public_command_template,
                required_inputs,
            },
        )
    }
}

fn specific_gate_direct_recommendation(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> Option<SpecificGateRecommendation> {
    let routing = gate_follow_up_routing_state(context, external_review_result_ready);
    let explicit_follow_up =
        specific_gate_reason_is_explicit_direct_follow_up(gate, routing.as_ref());
    if explicit_follow_up.is_some() {
        return SpecificGateRecommendation::from_routing_state(routing.as_ref());
    }

    SpecificGateRecommendation::from_routing_state(routing.as_ref())
}

fn apply_out_of_phase_gate_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    let force_operator_requery = gate
        .reason_codes
        .iter()
        .map(String::as_str)
        .any(finish_review_gate_already_current_reason_code);
    if !force_operator_requery
        && let Some(route_decision) =
            gate_follow_up_routing_state(context, external_review_result_ready)
                .and_then(|routing| routing.route_decision)
    {
        let recommended_public_command_template = route_decision.public_command_template();
        let required_inputs = route_decision.required_inputs;
        if recommended_public_command_template.is_some() || !required_inputs.is_empty() {
            gate.code = None;
            gate.recommended_command = None;
            gate.recommended_public_command_template = recommended_public_command_template;
            gate.required_inputs = required_inputs;
            gate.rederive_via_workflow_operator = None;
            return;
        }
    }
    gate.code = Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE));
    gate.recommended_command = None;
    gate.recommended_public_command_template = None;
    gate.required_inputs = Vec::new();
    gate.rederive_via_workflow_operator = Some(true);
}

fn apply_out_of_phase_requery_contract(
    _context: &ExecutionContext,
    gate: &mut GateResult,
    _external_review_result_ready: bool,
) {
    gate.code = Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE));
    gate.recommended_command = None;
    gate.recommended_public_command_template = None;
    gate.required_inputs = Vec::new();
    gate.rederive_via_workflow_operator = Some(true);
}

fn apply_specific_gate_follow_up_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    gate.recommended_command = None;
    if let Some(SpecificGateRecommendation {
        recommended_public_command_template,
        required_inputs,
    }) = specific_gate_direct_recommendation(context, gate, external_review_result_ready)
    {
        gate.recommended_command = None;
        gate.recommended_public_command_template = recommended_public_command_template;
        gate.required_inputs = required_inputs;
    }
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
        recommended_public_command_template,
        required_inputs,
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
        recommended_public_command_template,
        required_inputs,
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
    let routing = gate_follow_up_routing_state(context, false);
    let direct_follow_up =
        specific_gate_reason_is_explicit_direct_follow_up(&gate, routing.as_ref());
    let task_scope_prior_task_requires_requery = matches!(args.scope, ReviewDispatchScopeArg::Task)
        && gate
            .reason_codes
            .iter()
            .any(|code| code.starts_with("prior_task_"));
    if gate.allowed || direct_follow_up.is_none() || task_scope_prior_task_requires_requery {
        apply_out_of_phase_requery_contract(context, &mut gate, false);
    } else if let Some(SpecificGateRecommendation {
        recommended_public_command_template,
        required_inputs,
    }) = SpecificGateRecommendation::from_routing_state(routing.as_ref())
    {
        gate.recommended_command = None;
        gate.recommended_public_command_template = recommended_public_command_template;
        gate.required_inputs = required_inputs;
    } else {
        apply_out_of_phase_requery_contract(context, &mut gate, false);
    }
    record_review_dispatch_blocked_output(args, gate)
}

fn review_dispatch_scope_label(scope: ReviewDispatchScopeArg) -> String {
    match scope {
        ReviewDispatchScopeArg::Task => String::from("task"),
        ReviewDispatchScopeArg::FinalReview => String::from("final-review"),
    }
}

fn review_dispatch_out_of_phase_gate(context: &ExecutionContext, message: String) -> GateResult {
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::ExecutionStateNotReady,
        "record_review_dispatch_out_of_phase",
        message,
        public_typed_operator_route_remediation_for_plan(
            "Re-query the routed public step for review-dispatch authority.",
            &context.plan_rel,
        ),
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

fn gate_review_command_phase_gate(
    context: &ExecutionContext,
    authoritative_state: super::AuthoritativeTransitionStateRef<'_>,
    gate_review: &GateResult,
) -> Option<GateResult> {
    if !gate_review.allowed {
        return None;
    }
    let checkpoint_current = matches!(
        finish_review_gate_checkpoint_matches_current_branch_closure(context, authoritative_state),
        Ok(true)
    );
    if !checkpoint_current
        || !gate_finish_from_context_with_authoritative_state(context, authoritative_state).allowed
    {
        return None;
    }
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::ExecutionStateNotReady,
        FINISH_REVIEW_GATE_ALREADY_CURRENT,
        "finish-review checkpoint recording is out of phase because the current branch closure already has a fresh persisted checkpoint.",
        public_typed_operator_route_remediation_for_plan(
            "The finish-review checkpoint is already current; re-query the routed public step before continuing.",
            &context.plan_rel,
        ),
    );
    Some(gate.finish())
}

fn recommendation_execution_context_key(context: &ExecutionContext) -> String {
    let base_branch = context
        .current_release_base_branch()
        .unwrap_or_else(|| String::from("unknown"));
    format!("{}@{}", context.runtime.branch_name, base_branch)
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
    let mut gate = GateState::from_result(gate_review_base_result(context, false, Ok(None)));
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
                public_typed_operator_route_remediation_for_plan(
                    "Restore authoritative harness state readability before binding the final-review route.",
                    &context.plan_rel,
                ),
            );
            return gate.finish();
        }
    };
    let branch_bindings = current_branch_gate_bindings_from_authoritative_state(
        context,
        authoritative_state.as_ref(),
        false,
    );
    let Some(current_branch_closure_id) = branch_bindings.current_branch_closure_id.as_deref()
    else {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            "Final-review route is blocked because no current reviewed branch closure exists.",
            public_typed_operator_route_remediation_for_plan(
                "Re-query workflow/operator JSON for the approved plan and follow the typed public late-stage route.",
                &context.plan_rel,
            ),
        );
        return gate.finish();
    };
    if branch_bindings.current_branch_reviewed_state_id.is_none() {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "current_branch_reviewed_state_id_missing",
            "Final-review route is blocked because the current branch-closure reviewed state needs a current public review-state route before late-stage progression can continue.",
            public_typed_operator_route_remediation_for_plan(
                "Re-query workflow/operator JSON for the approved plan and follow the typed public review-state route.",
                &context.plan_rel,
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
            "Final-review route is blocked because the current branch closure still has a blocked release-readiness result.",
            public_typed_operator_route_remediation_for_plan(
                "Re-query workflow/operator JSON for the approved plan and follow the typed public route that resolves release-readiness before final review.",
                &context.plan_rel,
            ),
        );
        return gate.finish();
    }
    if release_readiness_result.as_deref() != Some("ready") {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY,
            "Final-review route is blocked because the current branch closure does not yet have a current release-readiness result `ready`.",
            public_typed_operator_route_remediation_for_plan(
                "Re-query workflow/operator JSON for the approved plan and follow the typed public release-readiness route before final review.",
                &context.plan_rel,
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
    let task_boundary_route_remediation = public_typed_operator_route_remediation_for_plan(
        "Return to workflow/operator JSON for the current task-boundary route before trying task-boundary review again.",
        &context.plan_rel,
    );
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
	                "Task {task_number} does not exist in the approved plan and cannot be used for the current task-boundary review route."
	            ),
	            public_typed_operator_route_remediation_for_plan(
	                "Choose a valid task number from the approved plan before binding the current task-boundary review route.",
	                &context.plan_rel,
	            ),
	        );
        return gate.finish();
    }

    if current_task_closure_overlay_restore_required(context).unwrap_or(false) {
        gate.fail(
	            FailureClass::ExecutionStateNotReady,
	            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED,
	            format!(
	                "Task {task_number} task-boundary review route is blocked because current task-closure overlays are missing for this task."
	            ),
	            public_workflow_operator_remediation_for_plan(&context.plan_rel),
	        );
        return gate.finish();
    }
    let authoritative_completed_steps =
        authoritative_completed_steps_for_review_dispatch(context, task_number, &mut gate);

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
                        "Task {task_number} task-boundary review route is blocked while Step {} remains active.",
                        step.step_number
                    ),
                    task_boundary_route_remediation.clone(),
                ),
                NoteState::Blocked => (
                    "blocked_step",
                    format!(
                        "Task {task_number} task-boundary review route is blocked while Step {} remains blocked.",
                        step.step_number
                    ),
                    task_boundary_route_remediation.clone(),
                ),
                NoteState::Interrupted => (
                    "interrupted_work_unresolved",
                    format!(
                        "Task {task_number} task-boundary review route is blocked while Step {} remains interrupted.",
                        step.step_number
                    ),
                    task_boundary_route_remediation.clone(),
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

    let public_repair_remediation =
        public_workflow_operator_remediation_for_plan(&context.plan_rel);
    for step in task_steps {
        if !step_completed_by_authoritative_truth(step, authoritative_completed_steps.as_ref()) {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "unfinished_task_steps_remaining",
                format!(
                    "Task {task_number} task-boundary review route is blocked while Step {} remains unchecked.",
                    step.step_number
                ),
                task_boundary_route_remediation.clone(),
            );
            continue;
        }
        verify_completed_step_evidence_projection(
            context,
            &mut gate,
            step,
            &public_repair_remediation,
        );
    }

    match task_current_closure_status(context, task_number) {
        Ok(TaskCurrentClosureStatus::Current) => {
            gate.fail(
	                FailureClass::ExecutionStateNotReady,
	                "task_current_closure_already_current",
	                format!(
	                    "Task {task_number} task-boundary review route is out of phase because Task {task_number} already has a current passing task closure for the active approved plan."
	                ),
	                public_workflow_operator_remediation_for_plan(&context.plan_rel),
	            );
        }
        Ok(TaskCurrentClosureStatus::Missing) => {}
        Ok(TaskCurrentClosureStatus::Stale) => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
                format!(
                    "Task {task_number} task-boundary review route is blocked because Task {task_number} current task closure no longer matches the current reviewed workspace state."
                ),
                public_workflow_operator_remediation_for_plan(&context.plan_rel),
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
	                    "Task {task_number} task-boundary review route is blocked because the current task-closure state is not trustworthy: {}",
	                    error.message
	                ),
	                public_workflow_operator_remediation_for_plan(&context.plan_rel),
	            );
        }
    }

    gate.finish()
}

fn authoritative_completed_steps_for_review_dispatch(
    context: &ExecutionContext,
    task_number: u32,
    gate: &mut GateState,
) -> Option<BTreeSet<(u32, u32)>> {
    match authoritative_completed_steps_for_context(context) {
        Ok(Some(completed_steps)) => Some(completed_steps),
        Ok(None) => {
            if context.local_execution_progress_markers_present
                || !context.evidence.attempts.is_empty()
            {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "authoritative_completion_state_missing",
                    format!(
                        "Task {task_number} task-boundary review route requires authoritative event-log completion state; projection-only plan/evidence state is not authoritative."
                    ),
                    public_workflow_operator_remediation_for_plan(&context.plan_rel),
                );
                return Some(BTreeSet::new());
            }
            None
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_completion_state_unavailable",
                format!(
                    "Task {task_number} task-boundary review route could not load authoritative completion state: {}",
                    error.message
                ),
                public_workflow_operator_remediation_for_plan(&context.plan_rel),
            );
            Some(BTreeSet::new())
        }
    }
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
