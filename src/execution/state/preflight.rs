use super::{
    ExecutionContext, FailureClass, GateResult, GateState, JsonFailure, NoteState,
    PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT, Path, PreflightWriteAuthorityState, RepoSafetyCheckArgs,
    RepoSafetyIntentArg, RepoSafetyRuntime, RepoSafetyWriteTargetArg, RunIdentitySnapshot,
    WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND, active_step, authoritative_run_identity_present,
    ensure_preflight_authoritative_bootstrap,
    ensure_preflight_authoritative_bootstrap_with_existing_authority,
    load_execution_read_scope_for_mutation, persist_preflight_acceptance,
    preflight_acceptance_for_context, preflight_requires_authoritative_handoff,
    preflight_requires_authoritative_mutation_recovery, preflight_write_authority_state,
    repo_has_non_runtime_projection_tracked_changes, repo_has_unresolved_index_entries,
    repo_head_detached, repo_safety_preflight_message, repo_safety_preflight_remediation,
    repo_safety_stage,
};
use crate::execution::command_eligibility::PublicCommandKind;

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
    command_kind: PublicCommandKind,
) -> Result<PublicIntentPreflightReadiness, JsonFailure> {
    let command_name = command_kind.public_mutation_token();
    if authoritative_run_identity_present(context)? {
        return Ok(PublicIntentPreflightReadiness::AlreadyReady);
    }
    if preflight_acceptance_for_context(context)?.is_some() {
        if command_kind == PublicCommandKind::Begin {
            return Ok(PublicIntentPreflightReadiness::AllowedNeedsPersistence);
        }
        return Err(public_intent_preflight_requires_begin_error(
            context,
            command_kind,
        ));
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
                "{command_name} is blocked because the reduced runtime state did not expose an execution preflight gate. Re-query {}; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}.",
                workflow_operator_preflight_recovery_route(context)
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
    command_kind: PublicCommandKind,
) -> Result<(), JsonFailure> {
    public_intent_preflight_readiness(context, command_kind).map(|_| ())
}

pub fn public_intent_preflight_persistence_required(
    context: &ExecutionContext,
    command_kind: PublicCommandKind,
) -> Result<bool, JsonFailure> {
    Ok(matches!(
        public_intent_preflight_readiness(context, command_kind)?,
        PublicIntentPreflightReadiness::AllowedNeedsPersistence
    ))
}

fn ensure_public_intent_preflight_bootstrap_is_safe(
    context: &ExecutionContext,
    command_kind: PublicCommandKind,
) -> Result<(), JsonFailure> {
    if command_kind == PublicCommandKind::Begin {
        return Ok(());
    }
    let command_name = command_kind.public_mutation_token();
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
                "{command_name} cannot bootstrap execution preflight while Task {} Step {} is {note_state}. Re-query {}; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}.",
                step.task_number,
                step.step_number,
                workflow_operator_preflight_recovery_route(context)
            ),
        ));
    }
    Ok(())
}

fn workflow_operator_preflight_recovery_route(context: &ExecutionContext) -> String {
    format!(
        "`{WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND}` for `{}`",
        context.plan_rel
    )
}

fn public_intent_preflight_requires_begin_error(
    context: &ExecutionContext,
    command_kind: PublicCommandKind,
) -> JsonFailure {
    let command_name = command_kind.public_mutation_token();
    JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        format!(
            "{command_name} requires execution preflight and run identity established by begin before it can mutate runtime state. Re-query {}; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}.",
            workflow_operator_preflight_recovery_route(context)
        ),
    )
}

fn persist_allowed_public_intent_preflight(
    context: &ExecutionContext,
    command_kind: PublicCommandKind,
    use_existing_authority: bool,
) -> Result<(), JsonFailure> {
    if authoritative_run_identity_present(context)? {
        return Ok(());
    }
    if command_kind != PublicCommandKind::Begin {
        return Err(public_intent_preflight_requires_begin_error(
            context,
            command_kind,
        ));
    }
    if let Some(acceptance) = preflight_acceptance_for_context(context)? {
        ensure_public_intent_preflight_bootstrap_is_safe(context, command_kind)?;
        let run_identity = RunIdentitySnapshot {
            execution_run_id: acceptance.execution_run_id.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
        };
        return if use_existing_authority {
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
        };
    }
    ensure_public_intent_preflight_bootstrap_is_safe(context, command_kind)?;
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
    command_kind: PublicCommandKind,
) -> Result<(), JsonFailure> {
    match public_intent_preflight_readiness(context, command_kind)? {
        PublicIntentPreflightReadiness::AlreadyReady => Ok(()),
        PublicIntentPreflightReadiness::AllowedNeedsPersistence => {
            if command_kind == PublicCommandKind::Begin {
                return persist_allowed_public_intent_preflight(context, command_kind, false);
            }
            Err(public_intent_preflight_requires_begin_error(
                context,
                command_kind,
            ))
        }
    }
}

pub fn validate_public_begin_preflight_allowed(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, PublicCommandKind::Begin)
}

pub fn public_begin_preflight_persistence_required(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    public_intent_preflight_persistence_required(context, PublicCommandKind::Begin)
}

pub fn persist_allowed_public_begin_preflight(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    persist_allowed_public_intent_preflight(context, PublicCommandKind::Begin, true)
}

pub fn ensure_public_begin_preflight_ready(context: &ExecutionContext) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, PublicCommandKind::Begin)?;
    if authoritative_run_identity_present(context)? {
        return Ok(());
    }
    persist_allowed_public_intent_preflight(context, PublicCommandKind::Begin, false)
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
	            "Follow workflow operator guidance to publish the required handoff through the public workflow route, then retry preflight.",
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
            format!(
                "Stop and report this runtime diagnostic unless workflow operator JSON already exposes a typed public route. Run `{WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND}` for `{}` only to confirm that route; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}; do not manually repair authoritative artifacts.",
                context.plan_rel
            ),
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
