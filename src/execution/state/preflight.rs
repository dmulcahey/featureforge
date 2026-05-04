use super::*;

pub fn validate_expected_fingerprint(
    context: &ExecutionContext,
    expected: &str,
) -> Result<(), JsonFailure> {
    if context.execution_fingerprint != expected {
        return Err(JsonFailure::new(
            FailureClass::StaleMutation,
            "Execution state changed since the last parsed execution fingerprint.",
        ));
    }
    Ok(())
}

pub fn require_preflight_acceptance(context: &ExecutionContext) -> Result<(), JsonFailure> {
    crate::execution::topology::require_preflight_acceptance(context)
}

enum PublicIntentPreflightReadiness {
    AlreadyReady,
    AllowedNeedsPersistence,
}

fn public_intent_preflight_readiness(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<PublicIntentPreflightReadiness, JsonFailure> {
    if authoritative_run_identity_present(context)?
        || preflight_acceptance_for_context(context)?.is_some()
    {
        return Ok(PublicIntentPreflightReadiness::AlreadyReady);
    }

    let read_scope = load_execution_read_scope_for_mutation(
        &context.runtime,
        Path::new(&context.plan_rel),
        true,
    )?;
    let reduced_state = crate::execution::reducer::reduce_execution_read_scope(&read_scope)?;
    let Some(gate) = reduced_state
        .gate_snapshot
        .preflight
        .or(reduced_state.preflight)
    else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "{command_name} is blocked because the reduced runtime state did not expose an execution preflight gate. Run {} to recover a public route.",
                repair_review_state_preflight_recovery_command(context)
            ),
        ));
    };
    if !gate.allowed {
        return Err(JsonFailure::new(
            failure_class_for_gate_result(&gate),
            preflight_gate_failure_message(command_name, &gate),
        ));
    }

    Ok(PublicIntentPreflightReadiness::AllowedNeedsPersistence)
}

pub fn validate_public_intent_preflight_allowed(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<(), JsonFailure> {
    public_intent_preflight_readiness(context, command_name).map(|_| ())
}

pub fn public_intent_preflight_persistence_required(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<bool, JsonFailure> {
    Ok(matches!(
        public_intent_preflight_readiness(context, command_name)?,
        PublicIntentPreflightReadiness::AllowedNeedsPersistence
    ))
}

fn ensure_public_intent_preflight_bootstrap_is_safe(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<(), JsonFailure> {
    if command_name == "begin" {
        return Ok(());
    }
    if let Some(step) = context.steps.iter().find(|step| {
        matches!(
            step.note_state,
            Some(NoteState::Active | NoteState::Blocked | NoteState::Interrupted)
        )
    }) {
        let note_state = step.note_state.map(NoteState::as_str).unwrap_or("unknown");
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "{command_name} cannot bootstrap execution preflight while Task {} Step {} is {note_state}. Run {} to recover a public route.",
                step.task_number,
                step.step_number,
                repair_review_state_preflight_recovery_command(context)
            ),
        ));
    }
    Ok(())
}

fn repair_review_state_preflight_recovery_command(context: &ExecutionContext) -> String {
    format!(
        "featureforge plan execution repair-review-state --plan {}",
        context.plan_rel
    )
}

fn persist_allowed_public_intent_preflight(
    context: &ExecutionContext,
    command_name: &str,
    use_existing_authority: bool,
) -> Result<(), JsonFailure> {
    if authoritative_run_identity_present(context)?
        || preflight_acceptance_for_context(context)?.is_some()
    {
        return Ok(());
    }
    ensure_public_intent_preflight_bootstrap_is_safe(context, command_name)?;
    let acceptance = persist_preflight_acceptance(context)?;
    let run_identity = RunIdentitySnapshot {
        execution_run_id: acceptance.execution_run_id.clone(),
        source_plan_path: context.plan_rel.clone(),
        source_plan_revision: context.plan_document.plan_revision,
    };
    if use_existing_authority {
        ensure_preflight_authoritative_bootstrap_with_existing_authority(
            &context.runtime,
            run_identity,
            acceptance.chunk_id,
        )
    } else {
        ensure_preflight_authoritative_bootstrap(
            &context.runtime,
            run_identity,
            acceptance.chunk_id,
        )
    }
}

pub fn ensure_public_intent_preflight_ready(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, command_name)?;
    persist_allowed_public_intent_preflight(context, command_name, false)
}

pub fn validate_public_begin_preflight_allowed(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, "begin")
}

pub fn public_begin_preflight_persistence_required(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    public_intent_preflight_persistence_required(context, "begin")
}

pub fn persist_allowed_public_begin_preflight(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    persist_allowed_public_intent_preflight(context, "begin", true)
}

pub fn ensure_public_begin_preflight_ready(context: &ExecutionContext) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, "begin")?;
    if authoritative_run_identity_present(context)?
        || preflight_acceptance_for_context(context)?.is_some()
    {
        return Ok(());
    }
    let acceptance = persist_preflight_acceptance(context)?;
    ensure_preflight_authoritative_bootstrap(
        &context.runtime,
        RunIdentitySnapshot {
            execution_run_id: acceptance.execution_run_id.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
        },
        acceptance.chunk_id,
    )
}

fn failure_class_for_gate_result(gate: &GateResult) -> FailureClass {
    match gate.failure_class.as_str() {
        "WorkspaceNotSafe" => FailureClass::WorkspaceNotSafe,
        "MalformedExecutionState" => FailureClass::MalformedExecutionState,
        "ConcurrentWriterConflict" => FailureClass::ConcurrentWriterConflict,
        "PartialAuthoritativeMutation" => FailureClass::PartialAuthoritativeMutation,
        _ => FailureClass::ExecutionStateNotReady,
    }
}

fn preflight_gate_failure_message(command_name: &str, gate: &GateResult) -> String {
    let Some(diagnostic) = gate.diagnostics.first() else {
        return format!("{command_name} is blocked because execution preflight is not allowed.");
    };
    format!(
        "{command_name} is blocked by execution preflight: {} Remediation: {}",
        diagnostic.message, diagnostic.remediation
    )
}

pub fn preflight_from_context(context: &ExecutionContext) -> GateResult {
    let mut gate = GateState::default();
    match preflight_write_authority_state(context) {
        Ok(PreflightWriteAuthorityState::Clear) => {}
        Ok(PreflightWriteAuthorityState::Conflict) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "write_authority_conflict",
            "Execution preflight cannot continue while another runtime writer holds write authority.",
            "Retry once the active writer releases write authority.",
        ),
        Err(error) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "write_authority_unavailable",
            error.message,
            "Restore write-authority lock access before retrying preflight.",
        ),
    }

    match preflight_requires_authoritative_handoff(context) {
        Ok(true) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_handoff_required",
            "Execution preflight cannot continue while authoritative harness state requires handoff.",
            "Publish a valid handoff (or clear handoff_required in authoritative state) before retrying preflight.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_state_unavailable",
            error.message,
            "Restore authoritative harness state readability and validity before retrying preflight.",
        ),
    }
    match preflight_requires_authoritative_mutation_recovery(context) {
        Ok(true) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_mutation_recovery_required",
            "Execution preflight cannot continue while authoritative artifact history is ahead of persisted harness state.",
            "Recover interrupted authoritative mutation state before retrying preflight.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_state_unavailable",
            error.message,
            "Restore authoritative harness state and artifact readability before retrying preflight.",
        ),
    }

    if let Some(step) = active_step(context, NoteState::Active) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "active_step_in_progress",
            format!(
                "Execution preflight cannot continue while Task {} Step {} is already active.",
                step.task_number, step.step_number
            ),
            "Resume or resolve the active step first.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Blocked) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "blocked_step",
            format!(
                "Execution preflight cannot continue while Task {} Step {} is blocked.",
                step.task_number, step.step_number
            ),
            "Resolve the blocked step first.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Interrupted) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "interrupted_work_unresolved",
            format!(
                "Execution preflight cannot continue while Task {} Step {} remains interrupted.",
                step.task_number, step.step_number
            ),
            "Resume or explicitly resolve the interrupted step first.",
        );
    }

    match repo_head_detached(context) {
        Ok(true) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "detached_head",
            "Execution preflight requires a branch-based workspace.",
            "Check out a branch before continuing execution.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "branch_unavailable",
            error.message,
            "Restore repository availability before continuing execution.",
        ),
    }
    match RepoSafetyRuntime::discover(&context.runtime.repo_root) {
        Ok(runtime) => {
            let args = RepoSafetyCheckArgs {
                intent: RepoSafetyIntentArg::Write,
                stage: repo_safety_stage(context),
                task_id: Some(context.plan_rel.clone()),
                paths: vec![context.plan_rel.clone()],
                write_targets: vec![RepoSafetyWriteTargetArg::ExecutionTaskSlice],
            };
            match runtime.check(&args) {
                Ok(result) if result.outcome == "blocked" => gate.fail(
                    FailureClass::WorkspaceNotSafe,
                    &result.reason,
                    repo_safety_preflight_message(&result),
                    repo_safety_preflight_remediation(&result),
                ),
                Ok(_) => {}
                Err(error) => gate.fail(
                    FailureClass::WorkspaceNotSafe,
                    "repo_safety_unavailable",
                    error.message(),
                    "Restore repo-safety availability before continuing execution.",
                ),
            }
        }
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "repo_safety_unavailable",
            error.message(),
            "Restore repo-safety availability before continuing execution.",
        ),
    }
    match repo_has_non_runtime_projection_tracked_changes(context) {
        Ok(Some(reason)) => {
            let (message, remediation) = if reason == "approved_plan_semantic_drift" {
                (
                    "Execution preflight does not allow semantic approved-plan edits.",
                    "Restore, commit, or re-approve semantic approved-plan changes before continuing execution.",
                )
            } else {
                (
                    "Execution preflight does not allow tracked worktree changes.",
                    "Commit or discard tracked worktree changes before continuing execution.",
                )
            };
            gate.fail(
                FailureClass::WorkspaceNotSafe,
                &reason,
                message,
                remediation,
            );
        }
        Ok(None) => {}
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "worktree_state_unavailable",
            error.message,
            "Restore repository status inspection before continuing execution.",
        ),
    }

    if context.runtime.git_dir.join("MERGE_HEAD").exists() {
        gate.fail(
            FailureClass::WorkspaceNotSafe,
            "merge_in_progress",
            "Execution preflight does not allow an in-progress merge.",
            "Resolve or abort the merge before continuing.",
        );
    }
    if context.runtime.git_dir.join("rebase-merge").exists()
        || context.runtime.git_dir.join("rebase-apply").exists()
    {
        gate.fail(
            FailureClass::WorkspaceNotSafe,
            "rebase_in_progress",
            "Execution preflight does not allow an in-progress rebase.",
            "Resolve or abort the rebase before continuing.",
        );
    }
    if context.runtime.git_dir.join("CHERRY_PICK_HEAD").exists() {
        gate.fail(
            FailureClass::WorkspaceNotSafe,
            "cherry_pick_in_progress",
            "Execution preflight does not allow an in-progress cherry-pick.",
            "Resolve or abort the cherry-pick before continuing.",
        );
    }
    match repo_has_unresolved_index_entries(&context.runtime.repo_root) {
        Ok(true) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "unresolved_index_entries",
            "Execution preflight does not allow unresolved index entries.",
            "Resolve index conflicts before continuing.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "index_unavailable",
            error.message,
            "Restore repository index availability before continuing execution.",
        ),
    }

    gate.finish()
}
