use super::common::*;
use crate::execution::authority::write_authoritative_unit_review_receipt_artifact;
use crate::execution::context::validate_state_dir_evidence_projection_before_materialization;
use crate::execution::projection_renderer::{
    ProjectionWriteMode, materialize_late_stage_projection_artifacts,
    write_execution_projection_read_models,
};
use crate::execution::transitions::materialize_authoritative_transition_state_projection;
use crate::paths::harness_authoritative_artifact_path;

pub fn materialize_projections(
    runtime: &ExecutionRuntime,
    args: &MaterializeProjectionsArgs,
) -> Result<MaterializeProjectionsOutput, JsonFailure> {
    let context = load_execution_context_for_exact_plan(runtime, &args.plan)?;
    let write_authority = claim_step_write_authority(runtime)?;
    let repo_export_requested = args.repo_export || args.tracked;
    if repo_export_requested
        && !args.confirm_repo_export
        && std::env::var("FEATUREFORGE_ALLOW_REPO_PROJECTION_EXPORT").as_deref() != Ok("1")
    {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "materialize-projections repo export requires --confirm-repo-export or FEATUREFORGE_ALLOW_REPO_PROJECTION_EXPORT=1.",
        ));
    }
    let mode = if repo_export_requested {
        ProjectionWriteMode::ProjectionExport
    } else {
        ProjectionWriteMode::StateDirOnly
    };
    let state_dir_materialization = mode == ProjectionWriteMode::StateDirOnly;
    let mut written_paths = Vec::new();
    if state_dir_materialization
        && let Some(path) = materialize_authoritative_transition_state_projection(runtime)?
    {
        written_paths.push(path.to_string_lossy().into_owned());
    }
    let authoritative_state = load_authoritative_transition_state(&context)?;
    if matches!(
        args.scope,
        MaterializeProjectionScopeArg::Execution | MaterializeProjectionScopeArg::All
    ) {
        validate_state_dir_evidence_projection_before_materialization(&context)?;
        let rendered = render_execution_projections(&context);
        written_paths.extend(write_execution_projection_read_models(
            &context, &rendered, mode,
        )?);
    }
    if matches!(
        args.scope,
        MaterializeProjectionScopeArg::LateStage | MaterializeProjectionScopeArg::All
    ) && let Some(authoritative_state) = authoritative_state.as_ref()
    {
        written_paths.extend(materialize_late_stage_projection_artifacts(
            runtime,
            &context,
            authoritative_state,
            mode,
        )?);
    }
    drop(write_authority);
    if state_dir_materialization
        && matches!(
            args.scope,
            MaterializeProjectionScopeArg::Execution | MaterializeProjectionScopeArg::All
        )
        && let Some(authoritative_state) = authoritative_state.as_ref()
    {
        written_paths.extend(materialize_task_review_receipts(
            runtime,
            &context,
            authoritative_state,
        )?);
    }
    Ok(MaterializeProjectionsOutput {
        action: String::from("materialized"),
        projection_mode: mode.as_str().to_owned(),
        written_paths,
        runtime_truth_changed: false,
        trace_summary: match mode {
            ProjectionWriteMode::ProjectionExport if args.tracked => String::from(
                "Materialized projection export files from authoritative runtime state; `--tracked` is a deprecated alias for --repo-export and approved plan/evidence files were not modified.",
            ),
            ProjectionWriteMode::ProjectionExport => String::from(
                "Materialized projection export files from authoritative runtime state; approved plan/evidence files were not modified.",
            ),
            ProjectionWriteMode::StateDirOnly => String::from(
                "Materialized state-dir projection files from authoritative runtime state.",
            ),
            ProjectionWriteMode::Disabled => {
                String::from("Projection materialization was disabled; no files were written.")
            }
        },
    })
}

fn materialize_task_review_receipts(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<Vec<String>, JsonFailure> {
    let Some(default_execution_run_id) = authoritative_state.execution_run_id_opt() else {
        return Ok(Vec::new());
    };
    let current_positive_records = authoritative_state
        .current_task_closure_results()
        .into_values()
        .filter(|record| record.review_result == "pass" && record.verification_result == "pass")
        .collect::<Vec<_>>();
    if current_positive_records.is_empty() {
        return Ok(Vec::new());
    }
    let Some(strategy_checkpoint_fingerprint) =
        authoritative_state.last_strategy_checkpoint_fingerprint()
    else {
        return Ok(Vec::new());
    };
    let active_contract_fingerprint = authoritative_state
        .evidence_provenance()
        .source_contract_fingerprint;
    let mut written_paths = Vec::new();
    for record in current_positive_records {
        written_paths.extend(materialize_task_review_receipts_for_task(
            runtime,
            context,
            default_execution_run_id.as_str(),
            &strategy_checkpoint_fingerprint,
            active_contract_fingerprint.as_deref(),
            record.task,
        )?);
    }
    Ok(written_paths)
}

fn materialize_task_review_receipts_for_task(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    execution_run_id: &str,
    strategy_checkpoint_fingerprint: &str,
    active_contract_fingerprint: Option<&str>,
    task: u32,
) -> Result<Vec<String>, JsonFailure> {
    let mut written_paths = Vec::new();
    let task_steps = context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == task)
        .map(|step_state| step_state.step_number)
        .collect::<Vec<_>>();
    for step in task_steps {
        if let Some(path) = materialize_unit_review_receipt_for_step(
            runtime,
            context,
            execution_run_id,
            strategy_checkpoint_fingerprint,
            active_contract_fingerprint,
            task,
            step,
        )? {
            written_paths.push(path.to_string_lossy().into_owned());
        }
    }
    if let Some(path) = materialize_task_verification_receipt_for_task(
        runtime,
        context,
        execution_run_id,
        strategy_checkpoint_fingerprint,
        task,
    )? {
        written_paths.push(path.to_string_lossy().into_owned());
    }
    Ok(written_paths)
}

fn materialize_unit_review_receipt_for_step(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    execution_run_id: &str,
    strategy_checkpoint_fingerprint: &str,
    active_contract_fingerprint: Option<&str>,
    task: u32,
    step: u32,
) -> Result<Option<PathBuf>, JsonFailure> {
    let Some(attempt) = latest_attempt_for_step(&context.evidence, task, step) else {
        return Ok(None);
    };
    if attempt.status != "Completed" {
        return Ok(None);
    }
    let Some(packet_fingerprint) = attempt
        .packet_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(reviewed_checkpoint_sha) = attempt
        .head_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let execution_unit_id = format!("task-{task}-step-{step}");
    let reviewer_source =
        existing_unit_review_reviewer_source(runtime, execution_run_id, &execution_unit_id)
            .unwrap_or_else(|| String::from("fresh-context-subagent"));
    let generated_at = Timestamp::now().to_string();
    let unsigned_source = if let Some(active_contract_fingerprint) = active_contract_fingerprint {
        let approved_unit_contract_fingerprint = approved_unit_contract_fingerprint_for_review(
            active_contract_fingerprint,
            packet_fingerprint,
            &execution_unit_id,
        );
        let execution_context_key = current_worktree_lease_execution_context_key(
            execution_run_id,
            &execution_unit_id,
            &context.plan_rel,
            context.plan_document.plan_revision,
            &context.runtime.branch_name,
            reviewed_checkpoint_sha,
        );
        let lease_fingerprint = serial_unit_review_lease_fingerprint(
            execution_run_id,
            &execution_unit_id,
            &execution_context_key,
            reviewed_checkpoint_sha,
            packet_fingerprint,
            &approved_unit_contract_fingerprint,
        );
        let Some(reconcile_result_proof_fingerprint) =
            reconcile_result_proof_fingerprint_for_review(
                &context.runtime.repo_root,
                reviewed_checkpoint_sha,
            )
        else {
            return Ok(None);
        };
        let reviewed_worktree = fs::canonicalize(&context.runtime.repo_root)
            .unwrap_or_else(|_| context.runtime.repo_root.clone());
        format!(
            "# Unit Review Result\n**Review Stage:** featureforge:unit-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {reviewer_source}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Source Plan:** {}\n**Source Plan Revision:** {}\n**Execution Run ID:** {execution_run_id}\n**Execution Unit ID:** {execution_unit_id}\n**Lease Fingerprint:** {lease_fingerprint}\n**Execution Context Key:** {execution_context_key}\n**Approved Task Packet Fingerprint:** {packet_fingerprint}\n**Approved Unit Contract Fingerprint:** {approved_unit_contract_fingerprint}\n**Reconciled Result SHA:** {reviewed_checkpoint_sha}\n**Reconcile Result Proof Fingerprint:** {reconcile_result_proof_fingerprint}\n**Reconcile Mode:** identity_preserving\n**Reviewed Worktree:** {}\n**Reviewed Checkpoint SHA:** {reviewed_checkpoint_sha}\n**Result:** pass\n**Generated By:** featureforge:unit-review\n**Generated At:** {generated_at}\n",
            context.plan_rel,
            context.plan_document.plan_revision,
            reviewed_worktree.display(),
        )
    } else {
        format!(
            "# Unit Review Result\n**Review Stage:** featureforge:unit-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {reviewer_source}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Source Plan:** {}\n**Source Plan Revision:** {}\n**Execution Run ID:** {execution_run_id}\n**Execution Unit ID:** {execution_unit_id}\n**Reviewed Checkpoint SHA:** {reviewed_checkpoint_sha}\n**Approved Task Packet Fingerprint:** {packet_fingerprint}\n**Result:** pass\n**Generated By:** featureforge:unit-review\n**Generated At:** {generated_at}\n",
            context.plan_rel, context.plan_document.plan_revision,
        )
    };
    let receipt_fingerprint = canonical_unit_review_receipt_fingerprint(&unsigned_source);
    let source = format!(
        "# Unit Review Result\n**Receipt Fingerprint:** {receipt_fingerprint}\n{}",
        unsigned_source.trim_start_matches("# Unit Review Result\n")
    );

    write_authoritative_unit_review_receipt_artifact(
        runtime,
        execution_run_id,
        &execution_unit_id,
        &source,
    )
    .map(Some)
}

fn existing_unit_review_reviewer_source(
    runtime: &ExecutionRuntime,
    execution_run_id: &str,
    execution_unit_id: &str,
) -> Option<String> {
    let receipt_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("unit-review-{execution_run_id}-{execution_unit_id}.md"),
    );
    let source = fs::read_to_string(receipt_path).ok()?;
    source.lines().find_map(|line| {
        line.trim()
            .strip_prefix("**Reviewer Source:**")
            .map(str::trim)
            .filter(|value| matches!(*value, "fresh-context-subagent" | "cross-model"))
            .map(ToOwned::to_owned)
    })
}

fn materialize_task_verification_receipt_for_task(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    execution_run_id: &str,
    strategy_checkpoint_fingerprint: &str,
    task: u32,
) -> Result<Option<PathBuf>, JsonFailure> {
    let task_steps = context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == task)
        .collect::<Vec<_>>();
    if task_steps.is_empty() {
        return Ok(None);
    }

    let mut verification_commands = Vec::new();
    let mut verification_results = Vec::new();
    for step_state in task_steps {
        if !step_state.checked {
            return Ok(None);
        }
        let Some(attempt) =
            latest_attempt_for_step(&context.evidence, task, step_state.step_number)
        else {
            return Ok(None);
        };
        if attempt.status != "Completed" {
            return Ok(None);
        }
        if let Some(verify_command) = attempt
            .verify_command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            verification_commands.push(verify_command.to_owned());
        }
        let verification_summary = attempt.verification_summary.trim();
        if !verification_summary.is_empty() {
            verification_results.push(verification_summary.to_owned());
        }
    }

    if verification_results.is_empty() {
        return Ok(None);
    }
    if verification_commands.is_empty() {
        verification_commands.push(String::from("manual verification recorded"));
    }

    let receipt_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("task-verification-{execution_run_id}-task-{task}.md"),
    );
    let generated_at = Timestamp::now().to_string();
    let source = format!(
        "# Task Verification Result\n**Source Plan:** {}\n**Source Plan Revision:** {}\n**Execution Run ID:** {execution_run_id}\n**Task Number:** {task}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Verification Commands:** {}\n**Verification Results:** {}\n**Result:** pass\n**Generated By:** featureforge:verification-before-completion\n**Generated At:** {generated_at}\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        verification_commands.join(" && "),
        verification_results.join(" | "),
    );
    write_atomic(&receipt_path, &source)?;
    Ok(Some(receipt_path))
}

fn canonical_unit_review_receipt_fingerprint(source: &str) -> String {
    let filtered = source
        .lines()
        .filter(|line| !line.trim().starts_with("**Receipt Fingerprint:**"))
        .collect::<Vec<_>>()
        .join("\n");
    sha256_hex(filtered.as_bytes())
}

fn current_worktree_lease_execution_context_key(
    execution_run_id: &str,
    execution_unit_id: &str,
    source_plan_path: &str,
    source_plan_revision: u32,
    authoritative_integration_branch: &str,
    reviewed_checkpoint_commit_sha: &str,
) -> String {
    sha256_hex(
        format!(
            "run={execution_run_id}\nunit={execution_unit_id}\nplan={source_plan_path}\nplan_revision={source_plan_revision}\nbranch={authoritative_integration_branch}\nreviewed_checkpoint={reviewed_checkpoint_commit_sha}\n"
        )
        .as_bytes(),
    )
}

fn serial_unit_review_lease_fingerprint(
    execution_run_id: &str,
    execution_unit_id: &str,
    execution_context_key: &str,
    reviewed_checkpoint_commit_sha: &str,
    approved_task_packet_fingerprint: &str,
    approved_unit_contract_fingerprint: &str,
) -> String {
    sha256_hex(
        format!(
            "serial-unit-review:{execution_run_id}:{execution_unit_id}:{execution_context_key}:{reviewed_checkpoint_commit_sha}:{approved_task_packet_fingerprint}:{approved_unit_contract_fingerprint}"
        )
        .as_bytes(),
    )
}

fn approved_unit_contract_fingerprint_for_review(
    active_contract_fingerprint: &str,
    approved_task_packet_fingerprint: &str,
    execution_unit_id: &str,
) -> String {
    sha256_hex(
        format!(
            "approved-unit-contract:{active_contract_fingerprint}:{approved_task_packet_fingerprint}:{execution_unit_id}"
        )
        .as_bytes(),
    )
}
