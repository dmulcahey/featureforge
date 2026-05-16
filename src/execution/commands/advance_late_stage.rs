use super::common::*;
use crate::execution::branch_closure_provenance::{
    BRANCH_CLOSURE_PROVENANCE_LATE_STAGE_SURFACE_EXEMPTION,
    branch_closure_provenance_is_late_stage_surface_exemption,
};
use crate::execution::command_eligibility::negative_result_follow_up;

pub fn record_branch_closure(
    runtime: &ExecutionRuntime,
    args: &RecordBranchClosureArgs,
) -> Result<RecordBranchClosureOutput, JsonFailure> {
    record_branch_closure_for_command(
        runtime,
        args,
        EventCommandOwner::InternalRecordBranchClosure,
    )
}

fn record_branch_closure_for_command(
    runtime: &ExecutionRuntime,
    args: &RecordBranchClosureArgs,
    command_owner: EventCommandOwner,
) -> Result<RecordBranchClosureOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let operator = current_workflow_operator(runtime, &args.plan, false)?;
    let overlay = load_status_authoritative_overlay_checked(&context)?;
    let mut reviewed_state = current_branch_reviewed_state(&context)?;
    if let Some(blocked_output) =
        blocked_branch_closure_output_for_invalid_current_task_closure(&context, &operator)?
    {
        return Ok(blocked_output);
    }
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage branch-closure recording requires authoritative harness state.",
        ));
    };
    if let Some(output) = branch_closure_already_current_empty_lineage_exemption_output(
        &context,
        authoritative_state,
        &reviewed_state,
        command_owner,
    )? {
        return Ok(output);
    }
    let rerecording_assessment = branch_closure_rerecording_assessment(&context)?;
    let changed_paths = rerecording_assessment.changed_paths.clone();
    let supported_late_stage_rerecording =
        !changed_paths.is_empty() && rerecording_assessment.supported;
    let repair_follow_up_branch_closure_target_id = authoritative_state
        .review_state_repair_follow_up_record()
        .and_then(|record| {
            (matches!(
                record.kind,
                RepairFollowUpKind::RecordBranchClosure | RepairFollowUpKind::AdvanceLateStage
            ) && record.target_scope
                == crate::execution::follow_up::RepairTargetScope::BranchClosure)
                .then_some(record.target_record_id)
                .flatten()
        })
        .filter(|target_id| {
            authoritative_state
                .branch_closure_record_for_repair_follow_up(target_id)
                .is_some_and(|record| {
                    branch_closure_record_is_empty_lineage_late_stage_exemption_baseline(
                        &record, &context,
                    )
                })
        });
    let repair_follow_up_allows_branch_closure =
        repair_follow_up_branch_closure_target_id.is_some();
    let branch_closure_recording_ready =
        advance_late_stage_operator_routes_branch_closure(&operator)
            || supported_late_stage_rerecording
            || repair_follow_up_allows_branch_closure;
    if !branch_closure_recording_ready {
        if operator_requires_review_state_repair(&operator) {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                Some(String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
                )),
            );
            return Ok(RecordBranchClosureOutput {
                action: String::from("blocked"),
                branch_closure_id: None,
                code: None,
                recommended_command: recovery.recommended_command,
                recommended_public_command_argv: recovery.recommended_public_command_argv,
                recommended_public_command_template: recovery.recommended_public_command_template,
                required_inputs: recovery.required_inputs,
                rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: recovery.required_follow_up,
                trace_summary: String::from(
                    "advance-late-stage branch-closure recording failed closed because workflow/operator requires review-state repair before branch-closure recording can proceed.",
                ),
            });
        } else if operator.review_state_status == "clean" {
            return Ok(shared_out_of_phase_record_branch_closure_output(
                &args.plan,
                current_authoritative_branch_closure_id(&context)?,
                "advance-late-stage branch-closure recording failed closed because the current phase must be re-derived through workflow/operator before branch-closure recording can proceed.",
            ));
        } else {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                branch_closure_required_follow_up(&operator),
            );
            return Ok(RecordBranchClosureOutput {
                action: String::from("blocked"),
                branch_closure_id: None,
                code: None,
                recommended_command: recovery.recommended_command,
                recommended_public_command_argv: recovery.recommended_public_command_argv,
                recommended_public_command_template: recovery.recommended_public_command_template,
                required_inputs: recovery.required_inputs,
                rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: recovery.required_follow_up,
                trace_summary: String::from(
                    "advance-late-stage branch-closure recording failed closed because workflow/operator did not expose branch_closure_recording_required_for_release_readiness.",
                ),
            });
        }
    }
    if !changed_paths.is_empty() {
        if !rerecording_assessment.supported {
            let trace_summary = match rerecording_assessment.unsupported_reason {
                Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => {
                    "advance-late-stage branch-closure recording failed closed because no still-current task-closure baseline remains for authoritative branch re-recording."
                }
                Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => {
                    "advance-late-stage branch-closure recording failed closed because the approved plan does not declare Late-Stage Surface metadata, so post-closure repo drift cannot be classified as trusted late-stage-only."
                }
                Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
                    "advance-late-stage branch-closure recording failed closed because branch drift escaped the trusted Late-Stage Surface."
                }
            };
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                Some(String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
                )),
            );
            return Ok(RecordBranchClosureOutput {
                action: String::from("blocked"),
                branch_closure_id: None,
                code: None,
                recommended_command: recovery.recommended_command,
                recommended_public_command_argv: recovery.recommended_public_command_argv,
                recommended_public_command_template: recovery.recommended_public_command_template,
                required_inputs: recovery.required_inputs,
                rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                superseded_branch_closure_ids: Vec::new(),
                required_follow_up: recovery.required_follow_up,
                trace_summary: trace_summary.to_owned(),
            });
        }
        let late_stage_surface = rerecording_assessment.late_stage_surface.as_slice();
        reviewed_state.provenance_basis =
            String::from(BRANCH_CLOSURE_PROVENANCE_LATE_STAGE_SURFACE_EXEMPTION);
        reviewed_state.source_task_closure_ids = shared_branch_source_task_closure_ids(
            &context,
            &current_branch_task_closure_records(&context)?,
            Some(late_stage_surface),
        );
        if reviewed_state.source_task_closure_ids.is_empty() {
            reviewed_state.effective_reviewed_branch_surface =
                late_stage_surface_only_branch_surface(&changed_paths);
        }
    }
    if let Some(output) = branch_closure_already_current_output(
        &context,
        authoritative_state,
        &reviewed_state,
        command_owner,
    )? {
        return Ok(output);
    }
    if repair_follow_up_allows_branch_closure && reviewed_state.source_task_closure_ids.is_empty() {
        reviewed_state.provenance_basis =
            String::from(BRANCH_CLOSURE_PROVENANCE_LATE_STAGE_SURFACE_EXEMPTION);
        if reviewed_state.effective_reviewed_branch_surface.is_empty() {
            reviewed_state.effective_reviewed_branch_surface =
                late_stage_surface_only_branch_surface(&changed_paths);
        }
    }
    if reviewed_state.source_task_closure_ids.is_empty()
        && !branch_closure_provenance_is_late_stage_surface_exemption(
            &reviewed_state.provenance_basis,
        )
    {
        let recovery = public_recovery_contract_for_follow_up(
            &args.plan,
            Some(&operator),
            Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE,
            )),
        );
        return Ok(RecordBranchClosureOutput {
            action: String::from("blocked"),
            branch_closure_id: None,
            code: None,
            recommended_command: recovery.recommended_command,
            recommended_public_command_argv: recovery.recommended_public_command_argv,
            recommended_public_command_template: recovery.recommended_public_command_template,
            required_inputs: recovery.required_inputs,
            rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
            superseded_branch_closure_ids: Vec::new(),
            required_follow_up: recovery.required_follow_up,
            trace_summary: String::from(
                "advance-late-stage branch-closure recording failed closed because no authoritative still-current task-closure provenance remains for the requested branch surface.",
            ),
        });
    }
    let base_branch_closure_id = deterministic_branch_closure_record_id(&context, &reviewed_state);
    let branch_closure_id =
        authoritative_state.next_available_branch_closure_record_id(&base_branch_closure_id);
    let mut superseded_branch_closure_ids =
        superseded_branch_closure_ids_from_previous_current(overlay.as_ref(), &branch_closure_id);
    if let Some(target_id) = repair_follow_up_branch_closure_target_id
        && target_id != branch_closure_id
        && !superseded_branch_closure_ids
            .iter()
            .any(|existing| existing == &target_id)
    {
        superseded_branch_closure_ids.push(target_id);
    }
    let branch_closure_source = render_branch_closure_artifact(
        &context,
        &branch_closure_id,
        BranchClosureProjectionInput {
            contract_identity: &reviewed_state.contract_identity,
            base_branch: &reviewed_state.base_branch,
            reviewed_state_id: &reviewed_state.reviewed_state_id,
            effective_reviewed_branch_surface: &reviewed_state.effective_reviewed_branch_surface,
            source_task_closure_ids: &reviewed_state.source_task_closure_ids,
            provenance_basis: &reviewed_state.provenance_basis,
            superseded_branch_closure_ids: &superseded_branch_closure_ids,
        },
    )?;
    let branch_closure_fingerprint = sha256_hex(branch_closure_source.as_bytes());
    record_current_branch_closure_for_command(
        authoritative_state,
        BranchClosureWrite {
            branch_closure_id: &branch_closure_id,
            source_plan_path: &context.plan_rel,
            source_plan_revision: context.plan_document.plan_revision,
            repo_slug: &context.runtime.repo_slug,
            branch_name: &context.runtime.branch_name,
            base_branch: &reviewed_state.base_branch,
            reviewed_state_id: &reviewed_state.reviewed_state_id,
            semantic_reviewed_state_id: Some(&reviewed_state.semantic_reviewed_state_id),
            contract_identity: &reviewed_state.contract_identity,
            effective_reviewed_branch_surface: &reviewed_state.effective_reviewed_branch_surface,
            source_task_closure_ids: &reviewed_state.source_task_closure_ids,
            provenance_basis: &reviewed_state.provenance_basis,
            closure_status: "current",
            superseded_branch_closure_ids: &superseded_branch_closure_ids,
            branch_closure_fingerprint: Some(&branch_closure_fingerprint),
        },
        command_owner,
    )?;
    let published_branch_closure_fingerprint =
        publish_authoritative_artifact(runtime, "branch-closure", &branch_closure_source)?;
    debug_assert_eq!(
        published_branch_closure_fingerprint,
        branch_closure_fingerprint
    );
    Ok(RecordBranchClosureOutput {
        action: String::from("recorded"),
        branch_closure_id: Some(branch_closure_id),
        code: None,
        recommended_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        rederive_via_workflow_operator: None,
        superseded_branch_closure_ids,
        required_follow_up: None,
        trace_summary: String::from(
            "Recorded a current branch closure for the still-current reviewed branch state.",
        ),
    })
}

pub fn advance_late_stage(
    runtime: &ExecutionRuntime,
    args: &AdvanceLateStageArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    require_advance_late_stage_public_flags(args)?;
    advance_late_stage_impl(runtime, args)
}

pub fn record_release_readiness(
    runtime: &ExecutionRuntime,
    args: &RecordReleaseReadinessArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let current_branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
    let Some(current_branch_closure_id) = current_branch_closure_id else {
        let params = AdvanceLateStageOutputContext {
            stage_path: "release_readiness",
            operation: "record_release_readiness_outcome",
            branch_closure_id: Some(args.branch_closure_id.clone()),
            dispatch_id: None,
            result: args.result.as_str(),
            external_review_result_ready: false,
            trace_summary: "advance-late-stage release-readiness recording failed closed because no authoritative current branch closure is available.",
        };
        if let Ok(operator) = current_workflow_operator(runtime, &args.plan, false) {
            return Ok(release_readiness_follow_up_or_requery_output(
                &operator, &args.plan, params,
            ));
        }
        return Ok(shared_out_of_phase_advance_late_stage_output(
            &args.plan, params,
        ));
    };
    if current_branch_closure_id != args.branch_closure_id {
        return Ok(shared_out_of_phase_advance_late_stage_output(
            &args.plan,
            AdvanceLateStageOutputContext {
                stage_path: "release_readiness",
                operation: "record_release_readiness_outcome",
                branch_closure_id: Some(args.branch_closure_id.clone()),
                dispatch_id: None,
                result: args.result.as_str(),
                external_review_result_ready: false,
                trace_summary: "advance-late-stage release-readiness recording failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
            },
        ));
    }
    let result = match args.result {
        crate::execution::internal_args::ReleaseReadinessOutcomeArg::Ready => {
            AdvanceLateStageResultArg::Ready
        }
        crate::execution::internal_args::ReleaseReadinessOutcomeArg::Blocked => {
            AdvanceLateStageResultArg::Blocked
        }
    };
    advance_late_stage_impl_for_command(
        runtime,
        &AdvanceLateStageArgs {
            plan: args.plan.clone(),
            dispatch_id: None,
            branch_closure_id: None,
            reviewer_source: None,
            reviewer_id: None,
            result: Some(result),
            summary_file: Some(args.summary_file.clone()),
        },
        EventCommandOwner::InternalRecordReleaseReadiness,
    )
}

pub fn record_final_review(
    runtime: &ExecutionRuntime,
    args: &RecordFinalReviewArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let current_branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
    if current_branch_closure_id.as_deref() != Some(args.branch_closure_id.as_str()) {
        return Ok(shared_out_of_phase_advance_late_stage_output(
            &args.plan,
            AdvanceLateStageOutputContext {
                stage_path: "final_review",
                operation: "record_final_review_outcome",
                branch_closure_id: Some(args.branch_closure_id.clone()),
                dispatch_id: Some(args.dispatch_id.clone()),
                result: args.result.as_str(),
                external_review_result_ready: true,
                trace_summary: "advance-late-stage final-review recording failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
            },
        ));
    }
    let result = match args.result {
        ReviewOutcomeArg::Pass => AdvanceLateStageResultArg::Pass,
        ReviewOutcomeArg::Fail => AdvanceLateStageResultArg::Fail,
    };
    advance_late_stage_impl_for_command(
        runtime,
        &AdvanceLateStageArgs {
            plan: args.plan.clone(),
            dispatch_id: Some(args.dispatch_id.clone()),
            branch_closure_id: None,
            reviewer_source: Some(args.reviewer_source.clone()),
            reviewer_id: Some(args.reviewer_id.clone()),
            result: Some(result),
            summary_file: Some(args.summary_file.clone()),
        },
        EventCommandOwner::InternalRecordFinalReview,
    )
}

pub fn record_qa(
    runtime: &ExecutionRuntime,
    args: &RecordQaArgs,
) -> Result<RecordQaOutput, JsonFailure> {
    record_qa_for_command(runtime, args, EventCommandOwner::InternalRecordQa)
}

fn record_qa_for_command(
    runtime: &ExecutionRuntime,
    args: &RecordQaArgs,
    command_owner: EventCommandOwner,
) -> Result<RecordQaOutput, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    require_preflight_acceptance(&context)?;
    let current_branch_closure = current_authoritative_branch_closure_binding_optional(&context)?;
    let branch_closure_id =
        current_authoritative_branch_closure_id_optional(&context)?.unwrap_or_default();
    let (operator, runtime_state) =
        current_workflow_operator_with_runtime_state(runtime, &args.plan, false)?;
    let workflow_operator_requery =
        || workflow_operator_requery_optional_surfaces(&args.plan, false);
    let required_follow_up = blocked_follow_up_for_operator(&operator);
    let qa_refresh_reroute_active =
        shared_finish_requires_test_plan_refresh(runtime_state.gate_snapshot.gate_finish.as_ref())
            || (operator.phase == crate::execution::phase::PHASE_QA_PENDING
                && operator.phase_detail
                    == crate::execution::phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED);
    if qa_refresh_reroute_active {
        let (recommended_command, recommended_public_command_argv) = workflow_operator_requery();
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "advance-late-stage QA recording failed closed because workflow/operator requires a fresh current-branch test plan before QA recording can proceed.",
            ),
        });
    }
    if required_follow_up.as_deref()
        == Some(crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE)
        && operator.phase == crate::execution::phase::PHASE_EXECUTING
        && operator.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && operator.review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && operator.current_branch_closure_id.is_none()
    {
        let (recommended_command, recommended_public_command_argv) = workflow_operator_requery();
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "advance-late-stage QA recording failed closed because workflow/operator must be requeried before QA recording can proceed.",
            ),
        });
    }
    if required_follow_up.as_deref()
        == Some(crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE)
        && operator.review_state_status == "clean"
    {
        let (recommended_command, recommended_public_command_argv) = workflow_operator_requery();
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: Some(true),
            required_follow_up: None,
            trace_summary: String::from(
                "advance-late-stage QA recording failed closed because workflow/operator must be requeried before QA recording can proceed.",
            ),
        });
    }
    if operator.review_state_status != "clean" {
        if operator.review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            || required_follow_up.as_deref()
                != Some(crate::execution::review_route_tokens::FOLLOW_UP_REPAIR_REVIEW_STATE)
        {
            let (recommended_command, recommended_public_command_argv) =
                workflow_operator_requery();
            return Ok(RecordQaOutput {
                action: String::from("blocked"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                code: Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
                recommended_command,
                recommended_public_command_argv,
                recommended_public_command_template: None,
                required_inputs: Vec::new(),
                rederive_via_workflow_operator: Some(true),
                required_follow_up: None,
                trace_summary: String::from(
                    "advance-late-stage QA recording failed closed because the current phase must be re-derived through workflow/operator before QA recording can proceed.",
                ),
            });
        }
        let recovery =
            public_recovery_contract_for_follow_up(&args.plan, Some(&operator), required_follow_up);
        return Ok(RecordQaOutput {
            action: String::from("blocked"),
            branch_closure_id,
            result: args.result.as_str().to_owned(),
            code: None,
            recommended_command: recovery.recommended_command,
            recommended_public_command_argv: recovery.recommended_public_command_argv,
            recommended_public_command_template: recovery.recommended_public_command_template,
            required_inputs: recovery.required_inputs,
            rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
            required_follow_up: recovery.required_follow_up,
            trace_summary: String::from(
                "advance-late-stage QA recording failed closed because workflow/operator did not expose qa_recording_required for the current branch closure.",
            ),
        });
    }
    let qa_override_out_of_phase = late_stage_negative_result_override_active(&operator);
    if !advance_late_stage_operator_routes_qa(&operator) {
        let allow_fail_recording_while_override_out_of_phase =
            args.result == ReviewOutcomeArg::Fail && qa_override_out_of_phase;
        if !allow_fail_recording_while_override_out_of_phase
            && equivalent_current_browser_qa_rerun_allowed(&operator, args.result.as_str())
            && let Some(current_branch_closure) = current_branch_closure.as_ref()
            && let Some(mut output) = equivalent_current_browser_qa_rerun(
                &context,
                current_branch_closure,
                &runtime_state.gate_snapshot,
                args.result.as_str(),
                &args.summary_file,
                (args.result == ReviewOutcomeArg::Fail)
                    .then(|| negative_result_follow_up(&operator))
                    .flatten(),
            )?
        {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                output.required_follow_up.take(),
            );
            output.recommended_command = recovery.recommended_command;
            output.recommended_public_command_argv = recovery.recommended_public_command_argv;
            output.recommended_public_command_template =
                recovery.recommended_public_command_template;
            output.required_inputs = recovery.required_inputs;
            output.rederive_via_workflow_operator = recovery.rederive_via_workflow_operator;
            output.required_follow_up = recovery.required_follow_up;
            return Ok(output);
        }
        if !allow_fail_recording_while_override_out_of_phase {
            let (recommended_command, recommended_public_command_argv) =
                workflow_operator_requery();
            return Ok(RecordQaOutput {
                action: String::from("blocked"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                code: Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
                recommended_command,
                recommended_public_command_argv,
                recommended_public_command_template: None,
                required_inputs: Vec::new(),
                rederive_via_workflow_operator: Some(true),
                required_follow_up: None,
                trace_summary: String::from(
                    "advance-late-stage QA recording failed closed because the current phase is out of band for QA recording; reroute through workflow/operator.",
                ),
            });
        }
    }
    let current_branch_closure =
        authoritative_current_branch_closure_binding(&context, "advance-late-stage QA recording")?;
    let branch_closure_id = current_branch_closure.branch_closure_id.clone();
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage QA recording requires authoritative harness state.",
        ));
    };
    let provided_summary_hash = optional_summary_hash(&args.summary_file);
    if let (Some(current_qa_branch_closure_id), Some(current_result), Some(current_summary_hash)) = (
        authoritative_state.current_qa_branch_closure_id(),
        authoritative_state.current_qa_result(),
        authoritative_state.current_qa_summary_hash(),
    ) && current_qa_branch_closure_id == branch_closure_id
    {
        let equivalent_current_result = provided_summary_hash.as_deref()
            == Some(current_summary_hash)
            && current_result == args.result.as_str();
        if provided_summary_hash.is_some() && !equivalent_current_result {
            return Ok(RecordQaOutput {
                action: String::from("blocked"),
                branch_closure_id,
                result: args.result.as_str().to_owned(),
                code: None,
                recommended_command: None,
                recommended_public_command_argv: None,
                recommended_public_command_template: None,
                required_inputs: Vec::new(),
                rederive_via_workflow_operator: None,
                required_follow_up: None,
                trace_summary: String::from(
                    "advance-late-stage QA recording failed closed because the current branch closure already has a conflicting recorded browser QA outcome.",
                ),
            });
        }
    }
    let final_review_record_id = authoritative_state
        .current_final_review_record_id()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage QA recording requires a current final-review record id.",
            )
        })?;
    let test_plan_path = if let Some(path) = current_authoritative_test_plan_path_from_qa_record(
        runtime,
        authoritative_state,
        &branch_closure_id,
        &final_review_record_id,
    )? {
        Some(path)
    } else {
        match current_test_plan_artifact_path(&context) {
            Ok(path) => Some(path),
            Err(error)
                if error.error_class == FailureClass::ExecutionStateNotReady.as_str()
                    || error.error_class == FailureClass::QaArtifactNotFresh.as_str() =>
            {
                None
            }
            Err(error) => return Err(error),
        }
    };
    let summary = read_nonempty_summary_file(&args.summary_file, "summary")?;
    let summary_hash = qa_summary_hash(&summary);
    let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
    let semantic_reviewed_state_id = current_branch_closure.semantic_reviewed_state_id.clone();
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::QaArtifactNotFresh,
            "advance-late-stage QA recording requires a resolvable base branch.",
        )
    })?;
    let qa_source = render_qa_artifact(
        runtime,
        &context,
        QaProjectionInput {
            branch_closure_id: &branch_closure_id,
            reviewed_state_id: &reviewed_state_id,
            result: args.result.as_str(),
            summary: &summary,
            base_branch: &base_branch,
            test_plan_path: test_plan_path.as_deref(),
        },
    )?;
    let authoritative_test_plan_write = if let Some(test_plan_path) = test_plan_path.as_deref() {
        let authoritative_test_plan_source =
            fs::read_to_string(test_plan_path).map_err(|error| {
                JsonFailure::new(
                    FailureClass::EvidenceWriteFailed,
                    format!(
                        "Could not read current test-plan artifact {}: {error}",
                        test_plan_path.display()
                    ),
                )
            })?;
        let authoritative_test_plan_fingerprint =
            sha256_hex(authoritative_test_plan_source.as_bytes());
        let authoritative_test_plan_path = harness_authoritative_artifact_path(
            &runtime.state_dir,
            &runtime.repo_slug,
            &runtime.branch_name,
            &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
        );
        let needs_publish = authoritative_test_plan_path != test_plan_path;
        Some((
            authoritative_test_plan_path,
            authoritative_test_plan_source,
            authoritative_test_plan_fingerprint,
            needs_publish,
        ))
    } else {
        None
    };
    let source_test_plan_fingerprint = authoritative_test_plan_write
        .as_ref()
        .map(|(_, _, fingerprint, _)| fingerprint.clone());
    let source_test_plan_projection_available = source_test_plan_fingerprint.is_some();
    let authoritative_qa_source = if let Some((authoritative_test_plan_path, _, _, _)) =
        authoritative_test_plan_write.as_ref()
    {
        rewrite_rebuild_source_test_plan_header(&qa_source, authoritative_test_plan_path)
    } else {
        qa_source.clone()
    };
    let qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    let authoritative_qa_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("browser-qa-{qa_fingerprint}.md"),
    );
    record_browser_qa_for_command(
        authoritative_state,
        BrowserQaWrite {
            branch_closure_id: &branch_closure_id,
            final_review_record_id: &final_review_record_id,
            source_plan_path: &context.plan_rel,
            source_plan_revision: context.plan_document.plan_revision,
            repo_slug: &runtime.repo_slug,
            branch_name: &context.runtime.branch_name,
            base_branch: &base_branch,
            reviewed_state_id: &reviewed_state_id,
            semantic_reviewed_state_id: semantic_reviewed_state_id.as_deref(),
            result: args.result.as_str(),
            browser_qa_fingerprint: Some(qa_fingerprint.as_str()),
            source_test_plan_fingerprint: source_test_plan_fingerprint.as_deref(),
            summary: &summary,
            summary_hash: &summary_hash,
            generated_by_identity: "featureforge/qa",
        },
        command_owner,
    )?;
    if let Some((authoritative_test_plan_path, authoritative_test_plan_source, _, true)) =
        authoritative_test_plan_write
    {
        write_atomic_file(
            &authoritative_test_plan_path,
            &authoritative_test_plan_source,
        )
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!(
                    "Could not write current test-plan artifact {}: {error}",
                    authoritative_test_plan_path.display()
                ),
            )
        })?;
    }
    write_atomic_file(&authoritative_qa_path, &authoritative_qa_source).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not write browser QA artifact {}: {error}",
                authoritative_qa_path.display()
            ),
        )
    })?;
    let (
        code,
        recommended_command,
        recommended_public_command_argv,
        recommended_public_command_template,
        required_inputs,
        rederive_via_workflow_operator,
        required_follow_up,
        trace_summary,
    ) = if args.result == ReviewOutcomeArg::Fail && qa_override_out_of_phase {
        let (recommended_command, recommended_public_command_argv) = workflow_operator_requery();
        (
            Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
            recommended_command,
            recommended_public_command_argv,
            None,
            Vec::new(),
            Some(true),
            None,
            String::from(
                "Recorded browser QA evidence for the current branch closure; workflow/operator must be requeried to continue from the active override lane.",
            ),
        )
    } else {
        let required_follow_up = (args.result == ReviewOutcomeArg::Fail)
            .then(|| {
                negative_result_required_follow_up(
                    runtime,
                    &args.plan,
                    &operator,
                    Some(authoritative_state),
                )
            })
            .flatten();
        let recovery =
            public_recovery_contract_for_follow_up(&args.plan, Some(&operator), required_follow_up);
        (
            None,
            recovery.recommended_command,
            recovery.recommended_public_command_argv,
            recovery.recommended_public_command_template,
            recovery.required_inputs,
            recovery.rederive_via_workflow_operator,
            recovery.required_follow_up,
            if source_test_plan_projection_available {
                String::from(
                    "Recorded browser QA evidence for the current branch closure and current test-plan projection.",
                )
            } else {
                String::from(
                    "Recorded browser QA evidence for the current branch closure; source test-plan projection was unavailable and remains diagnostic-only.",
                )
            },
        )
    };
    Ok(RecordQaOutput {
        action: String::from("recorded"),
        branch_closure_id,
        result: args.result.as_str().to_owned(),
        code,
        recommended_command,
        recommended_public_command_argv,
        recommended_public_command_template,
        required_inputs,
        rederive_via_workflow_operator,
        required_follow_up,
        trace_summary,
    })
}

fn advance_late_stage_args_are_intent_only(args: &AdvanceLateStageArgs) -> bool {
    args.dispatch_id.is_none()
        && args.branch_closure_id.is_none()
        && args.reviewer_source.is_none()
        && args.reviewer_id.is_none()
        && args.result.is_none()
        && args.summary_file.is_none()
}

fn final_review_dispatch_public_route_ready(operator: &ExecutionRoutingState) -> bool {
    advance_late_stage_operator_routes_final_review_dispatch(operator)
        && operator.current_branch_closure_id.is_some()
        && operator.current_release_readiness_result.as_deref() == Some("ready")
}

fn final_review_recording_can_bootstrap_dispatch(
    operator: &ExecutionRoutingState,
    branch_closure_id: Option<&str>,
) -> bool {
    final_review_dispatch_public_route_ready(operator)
        && operator
            .current_branch_closure_id
            .as_deref()
            .zip(branch_closure_id)
            .is_some_and(|(operator_branch, current_branch)| operator_branch == current_branch)
}

fn gate_requery_output(
    plan: &Path,
    stage_path: &str,
    operation: &str,
    branch_closure_id: Option<String>,
    result: &str,
    trace_summary: &str,
) -> AdvanceLateStageOutput {
    let (recommended_command, recommended_public_command_argv) =
        workflow_operator_requery_optional_surfaces(plan, false);
    AdvanceLateStageOutput {
        action: String::from("blocked"),
        stage_path: String::from(stage_path),
        intent: String::from(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE),
        operation: String::from(operation),
        branch_closure_id,
        dispatch_id: None,
        result: String::from(result),
        code: Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
        recommended_command,
        recommended_public_command_argv,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        rederive_via_workflow_operator: Some(true),
        required_follow_up: None,
        trace_summary: String::from(trace_summary),
    }
}

fn current_finish_review_gate_checkpoint_from_authoritative_state(
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<String> {
    authoritative_state
        .and_then(AuthoritativeTransitionState::finish_review_gate_pass_branch_closure_id)
}

fn current_branch_closure_id_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<String> {
    usable_current_branch_closure_identity_from_authoritative_state(context, authoritative_state)
        .map(|identity| identity.branch_closure_id)
}

fn current_final_review_record_dispatch_id_for_equivalent_rerun(
    context: &ExecutionContext,
    branch_closure_id: Option<&str>,
) -> Result<Option<String>, JsonFailure> {
    let Some(branch_closure_id) = branch_closure_id else {
        return Ok(None);
    };
    Ok(load_authoritative_transition_state(context)?
        .as_ref()
        .and_then(|state| {
            (state.current_final_review_branch_closure_id() == Some(branch_closure_id))
                .then(|| state.current_final_review_dispatch_id())
                .flatten()
        })
        .map(str::to_owned))
}

fn equivalent_release_readiness_rerun_for_advance_late_stage_args(
    context: &ExecutionContext,
    current_branch_closure: Option<&CurrentBranchClosureBinding>,
    args: &AdvanceLateStageArgs,
) -> Result<Option<AdvanceLateStageOutput>, JsonFailure> {
    let Some(current_branch_closure) = current_branch_closure else {
        return Ok(None);
    };
    let Some(summary_file) = args.summary_file.as_deref() else {
        return Ok(None);
    };
    let Some(result) = args.result.and_then(|result| match result {
        AdvanceLateStageResultArg::Ready => Some("ready"),
        AdvanceLateStageResultArg::Blocked => Some("blocked"),
        AdvanceLateStageResultArg::Pass | AdvanceLateStageResultArg::Fail => None,
    }) else {
        return Ok(None);
    };
    equivalent_current_release_readiness_rerun(
        context,
        current_branch_closure,
        "release_readiness",
        "record_release_readiness_outcome",
        result,
        summary_file,
    )
}

fn equivalent_branch_closure_already_current_rerun_for_intent_only_advance_late_stage(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    args: &AdvanceLateStageArgs,
    operator: &ExecutionRoutingState,
    command_owner: EventCommandOwner,
) -> Result<Option<AdvanceLateStageOutput>, JsonFailure> {
    if !advance_late_stage_args_are_intent_only(args)
        || !advance_late_stage_operator_routes_release_readiness(operator)
    {
        return Ok(None);
    }

    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(None);
    };
    let reviewed_state = current_branch_reviewed_state(context)?;
    let Some(mut output) = branch_closure_already_current_output(
        context,
        authoritative_state,
        &reviewed_state,
        command_owner,
    )?
    else {
        return Ok(None);
    };

    let recovery = public_recovery_contract_for_follow_up(
        &args.plan,
        Some(operator),
        Some(FollowUpKind::AdvanceLateStage.public_token().to_owned()),
    );
    output.recommended_command = recovery.recommended_command;
    output.recommended_public_command_argv = recovery.recommended_public_command_argv;
    output.recommended_public_command_template = recovery.recommended_public_command_template;
    output.required_inputs = recovery.required_inputs;
    output.rederive_via_workflow_operator = recovery.rederive_via_workflow_operator;
    output.required_follow_up = recovery.required_follow_up;

    Ok(Some(AdvanceLateStageOutput {
        action: output.action,
        stage_path: String::from("branch_closure"),
        intent: String::from(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE),
        operation: String::from("capture_branch_closure_state"),
        branch_closure_id: output.branch_closure_id,
        dispatch_id: None,
        result: String::from("recorded"),
        code: output.code,
        recommended_command: output.recommended_command,
        recommended_public_command_argv: output.recommended_public_command_argv,
        recommended_public_command_template: output.recommended_public_command_template,
        required_inputs: output.required_inputs,
        rederive_via_workflow_operator: output.rederive_via_workflow_operator,
        required_follow_up: output.required_follow_up,
        trace_summary: output.trace_summary,
    }))
}

pub(super) fn advance_late_stage_impl(
    runtime: &ExecutionRuntime,
    args: &AdvanceLateStageArgs,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    advance_late_stage_impl_for_command(runtime, args, EventCommandOwner::PublicAdvanceLateStage)
}

fn advance_late_stage_impl_for_command(
    runtime: &ExecutionRuntime,
    args: &AdvanceLateStageArgs,
    command_owner: EventCommandOwner,
) -> Result<AdvanceLateStageOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    ensure_public_intent_preflight_ready(&context, PublicCommandKind::AdvanceLateStage)?;
    require_preflight_acceptance(&context)?;
    let supplied_result_label = advance_late_stage_result_label(args.result);
    let current_branch_closure = current_authoritative_branch_closure_binding_optional(&context)?;
    let branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
    let operator_without_external_review = current_workflow_operator(runtime, &args.plan, false);
    let final_review_recording_requested = args.dispatch_id.is_some()
        || args.reviewer_source.is_some()
        || args.reviewer_id.is_some()
        || (matches!(
            args.result,
            Some(AdvanceLateStageResultArg::Pass | AdvanceLateStageResultArg::Fail)
        ) && operator_without_external_review
            .as_ref()
            .ok()
            .is_some_and(advance_late_stage_operator_accepts_final_review_inputs));
    let mut status = status_with_shared_routing_or_context_with_external_review(
        runtime,
        &args.plan,
        &context,
        final_review_recording_requested,
    )?;
    let public_mutation_mode = advance_late_stage_public_mutation_mode(args, &status);
    if advance_late_stage_args_are_intent_only(args)
        && let Ok(operator) = operator_without_external_review.as_ref()
        && final_review_dispatch_public_route_ready(operator)
    {
        require_advance_late_stage_public_mutation(&status, public_mutation_mode)?;
        let prior_dispatch_id = current_review_dispatch_id_candidate(
            &context,
            ReviewDispatchScopeArg::FinalReview,
            None,
            None,
        )?;
        let dispatch_id = ensure_current_review_dispatch_id_for_command(
            &context,
            ReviewDispatchScopeArg::FinalReview,
            None,
            None,
            command_owner,
        )?;
        let recovery = public_recovery_contract_for_follow_up(
            &args.plan,
            Some(operator),
            Some(
                FollowUpKind::RequestExternalReview
                    .public_token()
                    .to_owned(),
            ),
        );
        return Ok(AdvanceLateStageOutput {
            action: if prior_dispatch_id.as_deref() == Some(dispatch_id.as_str()) {
                String::from("already_current")
            } else {
                String::from("recorded")
            },
            stage_path: String::from("final_review"),
            intent: String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            ),
            operation: String::from("record_final_review_dispatch"),
            branch_closure_id: branch_closure_id.clone(),
            dispatch_id: Some(dispatch_id),
            result: String::from("dispatch_recorded"),
            code: None,
            recommended_command: recovery.recommended_command,
            recommended_public_command_argv: recovery.recommended_public_command_argv,
            recommended_public_command_template: recovery.recommended_public_command_template,
            required_inputs: recovery.required_inputs,
            rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
            required_follow_up: recovery.required_follow_up,
            trace_summary: String::from(
                "Recorded current runtime-owned final-review state through the public advance-late-stage route.",
            ),
        });
    }
    if final_review_recording_requested {
        if args.branch_closure_id.is_some() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "final_review_branch_closure_id_invalid: final-review advance-late-stage does not accept a branch-closure-id compatibility flag; use the workflow/operator recording_context branch_closure_id.",
            ));
        }
        let reviewer_source = args
            .reviewer_source
            .as_deref()
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "reviewer_source_required: final-review advance-late-stage requires --reviewer-source.",
                )
            })?;
        if !shared_reviewer_source_is_valid(reviewer_source) {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "reviewer_source_invalid: final-review advance-late-stage requires --reviewer-source fresh-context-subagent|cross-model|human-independent-reviewer.",
            ));
        }
        let reviewer_id = args.reviewer_id.as_deref().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "reviewer_id_required: final-review advance-late-stage requires --reviewer-id.",
            )
        })?;
        let result = match args.result {
            Some(AdvanceLateStageResultArg::Pass) => "pass",
            Some(AdvanceLateStageResultArg::Fail) => "fail",
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "final_review_result_invalid: final-review advance-late-stage requires --result pass or fail.",
                ));
            }
        };
        let summary_file = require_advance_late_stage_summary_file(args, "final-review")?;
        let final_review_recording_ready = |operator: &ExecutionRoutingState| {
            advance_late_stage_operator_routes_final_review(operator)
                && operator
                    .recording_context
                    .as_ref()
                    .and_then(|context| context.branch_closure_id.as_deref())
                    == branch_closure_id.as_deref()
        };
        let mut candidate_dispatch_id = current_review_dispatch_id_candidate(
            &context,
            ReviewDispatchScopeArg::FinalReview,
            None,
            args.dispatch_id.as_deref(),
        )?;
        if candidate_dispatch_id.is_none() {
            candidate_dispatch_id = current_final_review_record_dispatch_id_for_equivalent_rerun(
                &context,
                branch_closure_id.as_deref(),
            )?;
        }
        let mut operator = match current_workflow_operator(runtime, &args.plan, true) {
            Ok(operator) => operator,
            Err(error) if error.error_class == FailureClass::InstructionParseFailed.as_str() => {
                return Ok(shared_out_of_phase_advance_late_stage_output(
                    &args.plan,
                    AdvanceLateStageOutputContext {
                        stage_path: "final_review",
                        operation: "record_final_review_outcome",
                        branch_closure_id: branch_closure_id.clone(),
                        dispatch_id: None,
                        result,
                        external_review_result_ready: true,
                        trace_summary: "advance-late-stage final-review recording failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                    },
                ));
            }
            Err(error) => return Err(error),
        };
        if candidate_dispatch_id.is_none()
            && final_review_recording_can_bootstrap_dispatch(
                &operator,
                branch_closure_id.as_deref(),
            )
        {
            require_advance_late_stage_public_mutation(&status, public_mutation_mode)?;
            let dispatch_id = ensure_current_review_dispatch_id_for_command(
                &context,
                ReviewDispatchScopeArg::FinalReview,
                None,
                None,
                command_owner,
            )?;
            candidate_dispatch_id = Some(dispatch_id);
            let refreshed_context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
            status = status_with_shared_routing_or_context_with_external_review(
                runtime,
                &args.plan,
                &refreshed_context,
                true,
            )?;
            operator = match current_workflow_operator(runtime, &args.plan, true) {
                Ok(operator) => operator,
                Err(error)
                    if error.error_class == FailureClass::InstructionParseFailed.as_str() =>
                {
                    return Ok(shared_out_of_phase_advance_late_stage_output(
                        &args.plan,
                        AdvanceLateStageOutputContext {
                            stage_path: "final_review",
                            operation: "record_final_review_outcome",
                            branch_closure_id: branch_closure_id.clone(),
                            dispatch_id: candidate_dispatch_id.clone(),
                            result,
                            external_review_result_ready: true,
                            trace_summary: "advance-late-stage final-review recording failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                        },
                    ));
                }
                Err(error) => return Err(error),
            };
        }
        let final_review_override_out_of_phase =
            late_stage_negative_result_override_active(&operator);
        let dispatch_current_before_record = candidate_dispatch_id
            .as_deref()
            .map(|dispatch_id| {
                ensure_final_review_dispatch_id_matches(&context, dispatch_id).is_ok()
            })
            .unwrap_or(false);
        if operator.review_state_status == "clean"
            && !final_review_override_out_of_phase
            && dispatch_current_before_record
            && let Some(current_branch_closure) = current_branch_closure.as_ref()
            && let Some(output) = {
                let required_follow_up = (result == "fail")
                    .then(|| negative_result_follow_up(&operator))
                    .flatten();
                equivalent_current_final_review_rerun(
                    &context,
                    current_branch_closure,
                    EquivalentFinalReviewRerunParams {
                        stage_path: "final_review",
                        operation: "record_final_review_outcome",
                        dispatch_id: candidate_dispatch_id
                            .as_deref()
                            .expect("candidate dispatch id should exist when marked current"),
                        reviewer_source,
                        reviewer_id,
                        result,
                        summary_file,
                        required_follow_up: required_follow_up.clone(),
                    },
                )?
                .map(|output| (output, required_follow_up))
            }
            .map(|(mut output, required_follow_up)| {
                let recovery = public_recovery_contract_for_follow_up(
                    &args.plan,
                    Some(&operator),
                    required_follow_up,
                );
                output.recommended_command = recovery.recommended_command;
                output.recommended_public_command_argv = recovery.recommended_public_command_argv;
                output.recommended_public_command_template =
                    recovery.recommended_public_command_template;
                output.required_inputs = recovery.required_inputs;
                output.rederive_via_workflow_operator = recovery.rederive_via_workflow_operator;
                output.required_follow_up = recovery.required_follow_up;
                output
            })
        {
            return Ok(output);
        }
        let allow_fail_recording_while_override_out_of_phase = result == "fail"
            && final_review_override_out_of_phase
            && dispatch_current_before_record;
        if !final_review_recording_ready(&operator)
            && !allow_fail_recording_while_override_out_of_phase
        {
            return Ok(advance_late_stage_follow_up_or_requery_output(
                &operator,
                &args.plan,
                false,
                AdvanceLateStageOutputContext {
                    stage_path: "final_review",
                    operation: "record_final_review_outcome",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: None,
                    result,
                    external_review_result_ready: true,
                    trace_summary: "advance-late-stage final-review recording failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                },
            ));
        }
        require_advance_late_stage_final_review_public_mutation(&status)?;
        let summary = read_nonempty_summary_file(summary_file, "summary")?;
        let normalized_summary_hash = summary_hash(&summary);
        let dispatch_id = if let Some(dispatch_id) = candidate_dispatch_id {
            dispatch_id
        } else {
            ensure_current_review_dispatch_id_for_command(
                &context,
                ReviewDispatchScopeArg::FinalReview,
                None,
                args.dispatch_id.as_deref(),
                command_owner,
            )?
        };
        let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
        let current_branch_closure =
            current_authoritative_branch_closure_binding_optional(&context)?;
        let branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
        let dispatch_current =
            ensure_final_review_dispatch_id_matches(&context, &dispatch_id).is_ok();
        let operator = match current_workflow_operator(runtime, &args.plan, true) {
            Ok(operator) => operator,
            Err(error) if error.error_class == FailureClass::InstructionParseFailed.as_str() => {
                return Ok(shared_out_of_phase_advance_late_stage_output(
                    &args.plan,
                    AdvanceLateStageOutputContext {
                        stage_path: "final_review",
                        operation: "record_final_review_outcome",
                        branch_closure_id: branch_closure_id.clone(),
                        dispatch_id: Some(dispatch_id.clone()),
                        result,
                        external_review_result_ready: true,
                        trace_summary: "advance-late-stage final-review recording failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                    },
                ));
            }
            Err(error) => return Err(error),
        };
        let final_review_override_out_of_phase =
            late_stage_negative_result_override_active(&operator);
        let allow_fail_recording_while_override_out_of_phase =
            result == "fail" && final_review_override_out_of_phase && dispatch_current;
        if (!final_review_recording_ready(&operator)
            && !allow_fail_recording_while_override_out_of_phase)
            || !dispatch_current
        {
            if operator.review_state_status == "clean"
                && !final_review_override_out_of_phase
                && let Some(current_branch_closure) = current_branch_closure.as_ref()
                && let Some(output) = {
                    let required_follow_up = (result == "fail")
                        .then(|| negative_result_follow_up(&operator))
                        .flatten();
                    equivalent_current_final_review_rerun(
                        &context,
                        current_branch_closure,
                        EquivalentFinalReviewRerunParams {
                            stage_path: "final_review",
                            operation: "record_final_review_outcome",
                            dispatch_id: &dispatch_id,
                            reviewer_source,
                            reviewer_id,
                            result,
                            summary_file,
                            required_follow_up: required_follow_up.clone(),
                        },
                    )?
                    .map(|output| (output, required_follow_up))
                }
                .map(|(mut output, required_follow_up)| {
                    let recovery = public_recovery_contract_for_follow_up(
                        &args.plan,
                        Some(&operator),
                        required_follow_up,
                    );
                    output.recommended_command = recovery.recommended_command;
                    output.recommended_public_command_argv =
                        recovery.recommended_public_command_argv;
                    output.recommended_public_command_template =
                        recovery.recommended_public_command_template;
                    output.required_inputs = recovery.required_inputs;
                    output.rederive_via_workflow_operator = recovery.rederive_via_workflow_operator;
                    output.required_follow_up = recovery.required_follow_up;
                    output
                })
            {
                return Ok(output);
            }
            return Ok(advance_late_stage_follow_up_or_requery_output(
                &operator,
                &args.plan,
                dispatch_current,
                AdvanceLateStageOutputContext {
                    stage_path: "final_review",
                    operation: "record_final_review_outcome",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: Some(dispatch_id.clone()),
                    result,
                    external_review_result_ready: true,
                    trace_summary: "advance-late-stage final-review recording failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                },
            ));
        }
        let current_branch_closure = authoritative_current_branch_closure_binding(
            &context,
            "advance-late-stage final-review recording",
        )?;
        let branch_closure_id = current_branch_closure.branch_closure_id.clone();
        ensure_final_review_dispatch_id_matches(&context, &dispatch_id)?;
        let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
        let semantic_reviewed_state_id = current_branch_closure.semantic_reviewed_state_id.clone();
        let browser_qa_required = current_plan_requires_browser_qa(&context);
        let _write_authority = claim_step_write_authority(runtime)?;
        let mut authoritative_state = load_authoritative_transition_state(&context)?;
        let Some(authoritative_state) = authoritative_state.as_mut() else {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "advance-late-stage requires authoritative harness state.",
            ));
        };
        let final_review_evidence = resolve_final_review_evidence(&context)?;
        if let (
            Some(current_branch_closure_id),
            Some(current_dispatch_id),
            Some(current_reviewer_source),
            Some(current_reviewer_id),
            Some(current_result),
            Some(current_summary_hash),
        ) = (
            authoritative_state.current_final_review_branch_closure_id(),
            authoritative_state.current_final_review_dispatch_id(),
            authoritative_state.current_final_review_reviewer_source(),
            authoritative_state.current_final_review_reviewer_id(),
            authoritative_state.current_final_review_result(),
            authoritative_state.current_final_review_summary_hash(),
        ) && current_branch_closure_id == branch_closure_id
            && current_dispatch_id == dispatch_id
        {
            let equivalent_current_result = current_reviewer_source == reviewer_source
                && current_reviewer_id == reviewer_id
                && current_result == result
                && current_summary_hash == normalized_summary_hash;
            if equivalent_current_result {
                if current_final_review_record_is_still_authoritative(
                    &context,
                    authoritative_state,
                    CurrentFinalReviewAuthorityCheck {
                        branch_closure_id: &branch_closure_id,
                        dispatch_id: &dispatch_id,
                        reviewer_source,
                        reviewer_id,
                        result,
                        normalized_summary_hash: &normalized_summary_hash,
                    },
                )? {
                    if result == "fail" && final_review_override_out_of_phase {
                        return Ok(shared_out_of_phase_advance_late_stage_output(
                            &args.plan,
                            AdvanceLateStageOutputContext {
                                stage_path: "final_review",
                                operation: "record_final_review_outcome",
                                branch_closure_id: Some(branch_closure_id),
                                dispatch_id: Some(dispatch_id.clone()),
                                result,
                                external_review_result_ready: true,
                                trace_summary: "advance-late-stage final-review recording failed closed because the current phase must be re-derived through workflow/operator before final-review recording can proceed.",
                            },
                        ));
                    }
                    let required_follow_up = (result == "fail")
                        .then(|| {
                            negative_result_required_follow_up(
                                runtime,
                                &args.plan,
                                &operator,
                                Some(authoritative_state),
                            )
                        })
                        .flatten();
                    let recovery = public_recovery_contract_for_follow_up(
                        &args.plan,
                        Some(&operator),
                        required_follow_up,
                    );
                    return Ok(AdvanceLateStageOutput {
                        action: String::from("already_current"),
                        stage_path: String::from("final_review"),
                        intent: String::from(
                            crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
                        ),
                        operation: String::from("record_final_review_outcome"),
                        branch_closure_id: Some(branch_closure_id),
                        dispatch_id: Some(dispatch_id.clone()),
                        result: result.to_owned(),
                        code: None,
                        recommended_command: recovery.recommended_command,
                        recommended_public_command_argv: recovery.recommended_public_command_argv,
                        recommended_public_command_template: recovery
                            .recommended_public_command_template,
                        required_inputs: recovery.required_inputs,
                        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                        required_follow_up: recovery.required_follow_up,
                        trace_summary: String::from(
                            "Current branch closure already has an equivalent recorded final-review outcome.",
                        ),
                    });
                }
                return Ok(shared_out_of_phase_advance_late_stage_output(
                    &args.plan,
                    AdvanceLateStageOutputContext {
                        stage_path: "final_review",
                        operation: "record_final_review_outcome",
                        branch_closure_id: Some(branch_closure_id.clone()),
                        dispatch_id: Some(dispatch_id.clone()),
                        result,
                        external_review_result_ready: true,
                        trace_summary: "advance-late-stage final-review recording failed closed because the current final-review record is no longer authoritative and workflow/operator must re-derive the next safe step.",
                    },
                ));
            } else {
                return Ok(AdvanceLateStageOutput {
                    action: String::from("blocked"),
                    stage_path: String::from("final_review"),
                    intent: String::from(
                        crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
                    ),
                    operation: String::from("record_final_review_outcome"),
                    branch_closure_id: Some(branch_closure_id),
                    dispatch_id: Some(dispatch_id.clone()),
                    result: result.to_owned(),
                    code: None,
                    recommended_command: None,
                    recommended_public_command_argv: None,
                    recommended_public_command_template: None,
                    required_inputs: Vec::new(),
                    rederive_via_workflow_operator: None,
                    required_follow_up: None,
                    trace_summary: String::from(
                        "advance-late-stage final-review recording failed closed because the current branch closure already has a conflicting recorded final-review outcome for this reviewed state.",
                    ),
                });
            }
        }
        let rendered_final_review = render_final_review_artifacts(
            runtime,
            &context,
            &branch_closure_id,
            &reviewed_state_id,
            &final_review_evidence.base_branch,
            FinalReviewProjectionInput {
                dispatch_id: &dispatch_id,
                reviewer_source,
                reviewer_id,
                result,
                deviations_required: final_review_evidence.deviations_required,
                summary: &summary,
            },
        )?;
        let final_review_source = rendered_final_review.final_review_source;
        let final_review_fingerprint = sha256_hex(final_review_source.as_bytes());
        let release_readiness_record_id = authoritative_state
            .current_release_readiness_record_id()
            .ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    "advance-late-stage final-review recording requires a current release-readiness record id.",
                )
            })?;
        record_final_review_for_command(
            authoritative_state,
            FinalReviewWrite {
                branch_closure_id: &branch_closure_id,
                release_readiness_record_id: &release_readiness_record_id,
                dispatch_id: &dispatch_id,
                reviewer_source,
                reviewer_id,
                result,
                final_review_fingerprint: Some(final_review_fingerprint.as_str()),
                deviations_required: Some(final_review_evidence.deviations_required),
                browser_qa_required,
                source_plan_path: &context.plan_rel,
                source_plan_revision: context.plan_document.plan_revision,
                repo_slug: &context.runtime.repo_slug,
                branch_name: &context.runtime.branch_name,
                base_branch: &final_review_evidence.base_branch,
                reviewed_state_id: &reviewed_state_id,
                semantic_reviewed_state_id: semantic_reviewed_state_id.as_deref(),
                summary: &summary,
                summary_hash: &normalized_summary_hash,
            },
            command_owner,
        )?;
        let published =
            publish_authoritative_artifact(runtime, "final-review", &final_review_source)?;
        debug_assert_eq!(published, final_review_fingerprint);
        let (
            code,
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
            rederive_via_workflow_operator,
            required_follow_up,
            trace_summary,
        ) = if result == "fail" && final_review_override_out_of_phase {
            let (recommended_command, recommended_public_command_argv) =
                workflow_operator_requery_optional_surfaces(&args.plan, true);
            (
                Some(String::from(OUT_OF_PHASE_REQUERY_REQUIRED_CODE)),
                recommended_command,
                recommended_public_command_argv,
                None,
                Vec::new(),
                Some(true),
                None,
                String::from(
                    "Recorded final-review evidence for the current runtime-owned final-review state; workflow/operator must be requeried to continue from the active override lane.",
                ),
            )
        } else {
            let required_follow_up = (result == "fail")
                .then(|| {
                    negative_result_required_follow_up(
                        runtime,
                        &args.plan,
                        &operator,
                        Some(authoritative_state),
                    )
                })
                .flatten();
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                required_follow_up,
            );
            (
                None,
                recovery.recommended_command,
                recovery.recommended_public_command_argv,
                recovery.recommended_public_command_template,
                recovery.required_inputs,
                recovery.rederive_via_workflow_operator,
                recovery.required_follow_up,
                String::from(
                    "Validated runtime-owned final-review state and recorded final-review evidence from authoritative late-stage state.",
                ),
            )
        };
        return Ok(AdvanceLateStageOutput {
            action: String::from("recorded"),
            stage_path: String::from("final_review"),
            intent: String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            ),
            operation: String::from("record_final_review_outcome"),
            branch_closure_id: Some(branch_closure_id),
            dispatch_id: Some(dispatch_id.clone()),
            result: result.to_owned(),
            code,
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template,
            required_inputs,
            rederive_via_workflow_operator,
            required_follow_up,
            trace_summary,
        });
    }

    if args.branch_closure_id.is_some()
        || args.reviewer_source.is_some()
        || args.reviewer_id.is_some()
    {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "release_readiness_argument_mismatch: release-readiness advance-late-stage does not accept final-review-only arguments.",
        ));
    }
    let operator = match operator_without_external_review {
        Ok(operator) => operator,
        Err(error) if error.error_class == FailureClass::InstructionParseFailed.as_str() => {
            return Ok(shared_out_of_phase_advance_late_stage_output(
                &args.plan,
                AdvanceLateStageOutputContext {
                    stage_path: "release_readiness",
                    operation: "record_release_readiness_outcome",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: None,
                    result: supplied_result_label,
                    external_review_result_ready: false,
                    trace_summary: "advance-late-stage release-readiness recording failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
                },
            ));
        }
        Err(error) => return Err(error),
    };
    if let Some(output) =
        equivalent_branch_closure_already_current_rerun_for_intent_only_advance_late_stage(
            runtime,
            &context,
            args,
            &operator,
            command_owner,
        )?
    {
        return Ok(output);
    }
    if advance_late_stage_operator_routes_finish_review(&operator) {
        if !advance_late_stage_args_are_intent_only(args) {
            if let Some(output) = equivalent_release_readiness_rerun_for_advance_late_stage_args(
                &context,
                current_branch_closure.as_ref(),
                args,
            )? {
                return Ok(output);
            }
            return Ok(gate_requery_output(
                &args.plan,
                "finish_review",
                "record_finish_review_gate_checkpoint",
                branch_closure_id.clone(),
                supplied_result_label,
                "advance-late-stage finish-review checkpointing failed closed because the routed finish-review operation accepts only intent-only advance-late-stage; reroute through workflow/operator.",
            ));
        }
        require_advance_late_stage_public_mutation(&status, public_mutation_mode)?;
        let _write_authority = claim_step_write_authority(runtime)?;
        let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
        let mut authoritative_state = load_authoritative_transition_state(&context);
        let branch_closure_id = current_branch_closure_id_from_authoritative_state(
            &context,
            authoritative_state
                .as_ref()
                .map_err(|error| error.clone())?
                .as_ref(),
        );
        let gate = gate_review_from_context_with_authoritative_state(
            &context,
            authoritative_state.as_ref().map(|state| state.as_ref()),
            true,
        );
        if !gate.allowed {
            return Ok(gate_requery_output(
                &args.plan,
                "finish_review",
                "record_finish_review_gate_checkpoint",
                branch_closure_id,
                "blocked",
                "advance-late-stage finish-review checkpointing failed closed because finish-review gate validation is not currently allowed.",
            ));
        }
        let checkpoint_before = current_finish_review_gate_checkpoint_from_authoritative_state(
            authoritative_state
                .as_ref()
                .map_err(|error| error.clone())?
                .as_ref(),
        );
        persist_finish_review_gate_pass_checkpoint_for_command_with_authoritative_state(
            &context,
            command_owner.as_str(),
            &mut authoritative_state,
        )?;
        let checkpoint_after = current_finish_review_gate_checkpoint_from_authoritative_state(
            authoritative_state
                .as_ref()
                .map_err(|error| error.clone())?
                .as_ref(),
        );
        let Some(branch_closure_id) = branch_closure_id else {
            return Ok(gate_requery_output(
                &args.plan,
                "finish_review",
                "record_finish_review_gate_checkpoint",
                None,
                "blocked",
                "advance-late-stage finish-review checkpointing failed closed because no current branch closure is available.",
            ));
        };
        if checkpoint_after.as_deref() != Some(branch_closure_id.as_str()) {
            return Ok(gate_requery_output(
                &args.plan,
                "finish_review",
                "record_finish_review_gate_checkpoint",
                Some(branch_closure_id),
                "blocked",
                "advance-late-stage finish-review checkpointing failed closed because the persisted checkpoint did not bind to the current branch closure.",
            ));
        }
        return Ok(AdvanceLateStageOutput {
            action: if checkpoint_before.as_deref() == checkpoint_after.as_deref() {
                String::from("already_current")
            } else {
                String::from("recorded")
            },
            stage_path: String::from("finish_review"),
            intent: String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            ),
            operation: String::from("record_finish_review_gate_checkpoint"),
            branch_closure_id: Some(branch_closure_id),
            dispatch_id: None,
            result: String::from("pass"),
            code: None,
            recommended_command: None,
            recommended_public_command_argv: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
            required_follow_up: None,
            trace_summary: String::from(
                "Recorded finish-review checkpoint for the current branch closure through the public advance-late-stage route.",
            ),
        });
    }
    if advance_late_stage_operator_routes_finish_completion(&operator) {
        if !advance_late_stage_args_are_intent_only(args) {
            if let Some(output) = equivalent_release_readiness_rerun_for_advance_late_stage_args(
                &context,
                current_branch_closure.as_ref(),
                args,
            )? {
                return Ok(output);
            }
            return Ok(gate_requery_output(
                &args.plan,
                "finish_completion",
                "validate_finish_completion",
                branch_closure_id.clone(),
                supplied_result_label,
                "advance-late-stage finish completion failed closed because the routed finish-completion operation accepts only intent-only advance-late-stage; reroute through workflow/operator.",
            ));
        }
        require_advance_late_stage_public_mutation(&status, public_mutation_mode)?;
        let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
        let branch_closure_id = current_authoritative_branch_closure_id_optional(&context)?;
        let gate = gate_finish_from_context(&context);
        if !gate.allowed {
            return Ok(gate_requery_output(
                &args.plan,
                "finish_completion",
                "validate_finish_completion",
                branch_closure_id,
                "blocked",
                "advance-late-stage finish completion failed closed because finish-gate validation is not currently allowed.",
            ));
        }
        return Ok(AdvanceLateStageOutput {
            action: String::from("completed"),
            stage_path: String::from("finish_completion"),
            intent: String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            ),
            operation: String::from("validate_finish_completion"),
            branch_closure_id,
            dispatch_id: None,
            result: String::from("pass"),
            code: None,
            recommended_command: None,
            recommended_public_command_argv: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
            required_follow_up: None,
            trace_summary: String::from(
                "Validated finish completion through the public advance-late-stage route.",
            ),
        });
    }
    if advance_late_stage_operator_routes_branch_closure(&operator) {
        if args.result.is_some() || args.summary_file.is_some() || args.dispatch_id.is_some() {
            return Ok(shared_out_of_phase_advance_late_stage_output(
                &args.plan,
                AdvanceLateStageOutputContext {
                    stage_path: "release_readiness",
                    operation: "record_release_readiness_outcome",
                    branch_closure_id: branch_closure_id.clone(),
                    dispatch_id: None,
                    result: supplied_result_label,
                    external_review_result_ready: false,
                    trace_summary: "advance-late-stage release-readiness recording failed closed because branch-closure recording is required before release-readiness arguments are valid.",
                },
            ));
        }
        require_advance_late_stage_public_mutation(&status, public_mutation_mode)?;
        let output = record_branch_closure_for_command(
            runtime,
            &RecordBranchClosureArgs {
                plan: args.plan.clone(),
            },
            command_owner,
        )?;
        return Ok(AdvanceLateStageOutput {
            action: output.action,
            stage_path: String::from("branch_closure"),
            intent: String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            ),
            operation: String::from("capture_branch_closure_state"),
            branch_closure_id: output.branch_closure_id,
            dispatch_id: None,
            result: String::from("recorded"),
            code: output.code,
            recommended_command: output.recommended_command,
            recommended_public_command_argv: output.recommended_public_command_argv,
            recommended_public_command_template: output.recommended_public_command_template,
            required_inputs: output.required_inputs,
            rederive_via_workflow_operator: output.rederive_via_workflow_operator,
            required_follow_up: output.required_follow_up,
            trace_summary: output.trace_summary,
        });
    }
    if advance_late_stage_operator_routes_qa(&operator) {
        let result = match args.result {
            Some(AdvanceLateStageResultArg::Pass) => ReviewOutcomeArg::Pass,
            Some(AdvanceLateStageResultArg::Fail) => ReviewOutcomeArg::Fail,
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "qa_result_invalid: QA advance-late-stage requires --result pass or fail.",
                ));
            }
        };
        let summary_file = require_advance_late_stage_summary_file(args, "QA")?;
        require_advance_late_stage_public_mutation(&status, public_mutation_mode)?;
        let output = record_qa_for_command(
            runtime,
            &RecordQaArgs {
                plan: args.plan.clone(),
                result,
                summary_file: summary_file.to_path_buf(),
            },
            command_owner,
        )?;
        return Ok(AdvanceLateStageOutput {
            action: output.action,
            stage_path: String::from("browser_qa"),
            intent: String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
            ),
            operation: String::from("record_qa_outcome"),
            branch_closure_id: Some(output.branch_closure_id),
            dispatch_id: None,
            result: output.result,
            code: output.code,
            recommended_command: output.recommended_command,
            recommended_public_command_argv: output.recommended_public_command_argv,
            recommended_public_command_template: output.recommended_public_command_template,
            required_inputs: output.required_inputs,
            rederive_via_workflow_operator: output.rederive_via_workflow_operator,
            required_follow_up: output.required_follow_up,
            trace_summary: output.trace_summary,
        });
    }
    let result = match args.result {
        Some(AdvanceLateStageResultArg::Ready) => "ready",
        Some(AdvanceLateStageResultArg::Blocked) => "blocked",
        _ => {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "release_readiness_result_invalid: release-readiness advance-late-stage requires --result ready or blocked.",
            ));
        }
    };
    let summary_file = require_advance_late_stage_summary_file(args, "release-readiness")?;
    let release_route_ready = advance_late_stage_operator_routes_release_readiness(&operator);
    if !release_route_ready {
        if operator.review_state_status == "clean"
            && let Some(current_branch_closure) = current_branch_closure.as_ref()
            && let Some(mut output) = equivalent_current_release_readiness_rerun(
                &context,
                current_branch_closure,
                "release_readiness",
                "record_release_readiness_outcome",
                result,
                summary_file,
            )?
        {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                output.required_follow_up.take(),
            );
            output.recommended_command = recovery.recommended_command;
            output.recommended_public_command_argv = recovery.recommended_public_command_argv;
            output.recommended_public_command_template =
                recovery.recommended_public_command_template;
            output.required_inputs = recovery.required_inputs;
            output.rederive_via_workflow_operator = recovery.rederive_via_workflow_operator;
            output.required_follow_up = recovery.required_follow_up;
            return Ok(output);
        }
        if current_branch_closure.is_none() {
            require_public_mutation(
                &status,
                PublicMutationRequest::advance_late_stage(public_mutation_mode),
                FailureClass::ExecutionStateNotReady,
            )?;
        }
        return Ok(release_readiness_follow_up_or_requery_output(
            &operator,
            &args.plan,
            AdvanceLateStageOutputContext {
                stage_path: "release_readiness",
                operation: "record_release_readiness_outcome",
                branch_closure_id: branch_closure_id.clone(),
                dispatch_id: None,
                result,
                external_review_result_ready: false,
                trace_summary: "advance-late-stage release-readiness recording failed closed because the current phase must be re-derived through workflow/operator before release-readiness recording can proceed.",
            },
        ));
    }
    require_advance_late_stage_public_mutation(&status, public_mutation_mode)?;
    let summary = read_nonempty_summary_file(summary_file, "summary")?;
    let normalized_summary_hash = summary_hash(&summary);
    let current_branch_closure = authoritative_current_branch_closure_binding(
        &context,
        "advance-late-stage release-readiness recording",
    )?;
    let branch_closure_id = current_branch_closure.branch_closure_id.clone();
    let reviewed_state_id = current_branch_closure.reviewed_state_id.clone();
    let semantic_reviewed_state_id = current_branch_closure.semantic_reviewed_state_id.clone();
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(&context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage requires authoritative harness state.",
        ));
    };
    if let Some(current_record) = authoritative_state
        .current_release_readiness_record()
        .filter(|record| record.branch_closure_id == branch_closure_id)
    {
        if current_record.result == result && current_record.summary_hash == normalized_summary_hash
        {
            let recovery = public_recovery_contract_for_follow_up(
                &args.plan,
                Some(&operator),
                (result == "blocked").then(|| {
                    FollowUpKind::ResolveReleaseBlocker
                        .public_token()
                        .to_owned()
                }),
            );
            return Ok(AdvanceLateStageOutput {
                action: String::from("already_current"),
                stage_path: String::from("release_readiness"),
                intent: String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
                ),
                operation: String::from("record_release_readiness_outcome"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: None,
                result: result.to_owned(),
                code: None,
                recommended_command: recovery.recommended_command,
                recommended_public_command_argv: recovery.recommended_public_command_argv,
                recommended_public_command_template: recovery.recommended_public_command_template,
                required_inputs: recovery.required_inputs,
                rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
                required_follow_up: recovery.required_follow_up,
                trace_summary: String::from(
                    "Current branch closure already has an equivalent recorded release-readiness outcome.",
                ),
            });
        }
        if current_record.result != "blocked" {
            return Ok(AdvanceLateStageOutput {
                action: String::from("blocked"),
                stage_path: String::from("release_readiness"),
                intent: String::from(
                    crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE,
                ),
                operation: String::from("record_release_readiness_outcome"),
                branch_closure_id: Some(branch_closure_id),
                dispatch_id: None,
                result: result.to_owned(),
                code: None,
                recommended_command: None,
                recommended_public_command_argv: None,
                recommended_public_command_template: None,
                required_inputs: Vec::new(),
                rederive_via_workflow_operator: None,
                required_follow_up: None,
                trace_summary: String::from(
                    "advance-late-stage release-readiness recording failed closed because the current branch closure already has a conflicting recorded release-readiness outcome.",
                ),
            });
        }
    }
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ReleaseArtifactNotFresh,
            "advance-late-stage release-readiness recording requires a resolvable base branch.",
        )
    })?;
    let release_source = render_release_readiness_artifact(
        &context,
        &branch_closure_id,
        &reviewed_state_id,
        &base_branch,
        result,
        &summary,
    )?;
    let release_fingerprint = if result == "ready" {
        Some(sha256_hex(release_source.as_bytes()))
    } else {
        None
    };
    record_release_readiness_for_command(
        authoritative_state,
        ReleaseReadinessWrite {
            branch_closure_id: &branch_closure_id,
            source_plan_path: &context.plan_rel,
            source_plan_revision: context.plan_document.plan_revision,
            repo_slug: &runtime.repo_slug,
            branch_name: &context.runtime.branch_name,
            base_branch: &base_branch,
            reviewed_state_id: &reviewed_state_id,
            semantic_reviewed_state_id: semantic_reviewed_state_id.as_deref(),
            result,
            release_docs_fingerprint: release_fingerprint.as_deref(),
            summary: &summary,
            summary_hash: &normalized_summary_hash,
            generated_by_identity: "featureforge/release-readiness",
        },
        command_owner,
    )?;
    if let Some(release_docs_fingerprint) = release_fingerprint.as_deref() {
        let published = publish_authoritative_artifact(runtime, "release-docs", &release_source)?;
        debug_assert_eq!(published, release_docs_fingerprint);
    }
    let recovery = public_recovery_contract_for_follow_up(
        &args.plan,
        Some(&operator),
        (result == "blocked").then(|| {
            FollowUpKind::ResolveReleaseBlocker
                .public_token()
                .to_owned()
        }),
    );
    Ok(AdvanceLateStageOutput {
        action: String::from("recorded"),
        stage_path: String::from("release_readiness"),
        intent: String::from(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE),
        operation: String::from("record_release_readiness_outcome"),
        branch_closure_id: Some(branch_closure_id),
        dispatch_id: None,
        result: result.to_owned(),
        code: None,
        recommended_command: recovery.recommended_command,
        recommended_public_command_argv: recovery.recommended_public_command_argv,
        recommended_public_command_template: recovery.recommended_public_command_template,
        required_inputs: recovery.required_inputs,
        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
        required_follow_up: recovery.required_follow_up,
        trace_summary: String::from(
            "Recorded release-readiness late-stage evidence for the current branch closure.",
        ),
    })
}
