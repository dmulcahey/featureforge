use super::common::*;

pub fn complete(
    runtime: &ExecutionRuntime,
    args: &CompleteArgs,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let request = normalize_complete_request(args)?;
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut context = load_execution_context_for_mutation(runtime, &args.plan)?;
    validate_expected_fingerprint(&context, &request.expect_execution_fingerprint)?;
    normalize_or_seed_source(&request.source, &mut context.plan_document.execution_mode)?;
    let mut authoritative_state =
        Some(load_or_initialize_authoritative_transition_state(&context)?);
    enforce_authoritative_phase(authoritative_state.as_ref(), StepCommand::Complete)?;
    enforce_active_contract_scope(
        authoritative_state.as_ref(),
        StepCommand::Complete,
        request.task,
        request.step,
    )?;
    let complete_status = public_status_from_context_with_shared_routing(runtime, &context, false)?;
    require_public_mutation(
        &complete_status,
        PublicMutationRequest {
            kind: PublicMutationKind::Complete,
            task: Some(request.task),
            step: Some(request.step),
            expect_execution_fingerprint: Some(request.expect_execution_fingerprint.clone()),
            transfer_mode: None,
            transfer_scope: None,
            command_name: "complete",
        },
        FailureClass::ExecutionStateNotReady,
    )?;
    let provenance = authoritative_state
        .as_ref()
        .map(|state| state.evidence_provenance())
        .unwrap_or_default();

    let step_index = step_index(&context, request.task, request.step).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "Requested task/step does not exist in the approved plan.",
        )
    })?;
    if context.steps[step_index].note_state != Some(crate::execution::state::NoteState::Active) {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "complete may target only the current active step.",
        ));
    }
    if context.steps[step_index].checked {
        return Err(JsonFailure::new(
            FailureClass::InvalidStepTransition,
            "complete may not directly refresh an already checked step.",
        ));
    }

    let files = if request.files.is_empty() {
        default_files_for_task(&context, request.task)
    } else {
        canonicalize_files(&request.files)?
    };
    let files = canonicalize_repo_visible_paths(&context.runtime.repo_root, &files)?;
    let file_proofs = files
        .iter()
        .map(|path| FileProof {
            path: path.clone(),
            proof: current_file_proof(&context.runtime.repo_root, path),
        })
        .collect::<Vec<_>>();

    context.steps[step_index].checked = true;
    context.steps[step_index].note_state = None;
    context.steps[step_index].note_summary.clear();
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.clear_open_step_state()?;
    }

    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let packet_fingerprint = task_packet_fingerprint(
        &context,
        &source_spec_fingerprint,
        request.task,
        request.step,
    )
    .ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "complete could not determine semantic task contract identity for task {}.",
                request.task
            ),
        )
    })?;
    let recorded_at = Timestamp::now().to_string();
    let head_sha = context.current_head_sha()?;
    let new_attempt = EvidenceAttempt {
        task_number: request.task,
        step_number: request.step,
        attempt_number: next_attempt_number(&context.evidence, request.task, request.step),
        status: String::from("Completed"),
        recorded_at,
        execution_source: request.source.clone(),
        claim: request.claim,
        files: files.clone(),
        file_proofs,
        verify_command: request.verify_command,
        verification_summary: request.verification_summary,
        invalidation_reason: String::from("N/A"),
        packet_fingerprint: Some(packet_fingerprint),
        head_sha: Some(head_sha.clone()),
        base_sha: Some(head_sha),
        source_contract_path: provenance.source_contract_path,
        source_contract_fingerprint: provenance.source_contract_fingerprint,
        source_evaluation_report_fingerprint: provenance.source_evaluation_report_fingerprint,
        evaluator_verdict: provenance.evaluator_verdict,
        failing_criterion_ids: provenance.failing_criterion_ids,
        source_handoff_fingerprint: provenance.source_handoff_fingerprint,
        repo_state_baseline_head_sha: provenance.repo_state_baseline_head_sha,
        repo_state_baseline_worktree_fingerprint: provenance
            .repo_state_baseline_worktree_fingerprint,
    };

    context.evidence.attempts.push(new_attempt);
    context.evidence.format = crate::execution::state::EvidenceFormat::V2;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.record_execution_evidence_attempts(&context)?;
    }

    let rendered = render_execution_projections(&context);
    record_execution_projection_fingerprints(authoritative_state.as_mut(), &rendered)?;
    let _ = consume_execution_reentry_repair_follow_up(authoritative_state.as_mut())?;
    if let Some(authoritative_state) = authoritative_state.as_ref() {
        persist_authoritative_state_with_step_hint_and_rollback(
            authoritative_state,
            "complete",
            Some((request.task, request.step)),
            AuthoritativePersistRollback {
                plan_path: &context.plan_abs,
                original_plan: &context.plan_source,
                evidence_path: &context.evidence_abs,
                failpoint: "complete_after_plan_and_evidence_write_before_authoritative_state_publish",
            },
        )?;
    }
    maybe_trigger_failpoint("complete_after_plan_write")?;
    let reloaded = load_execution_context_for_mutation(runtime, &args.plan)?;
    status_with_shared_routing_or_context(runtime, &args.plan, &reloaded)
}
