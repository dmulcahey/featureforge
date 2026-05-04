use super::*;

pub(in crate::execution::commands) fn step_index(
    context: &ExecutionContext,
    task: u32,
    step: u32,
) -> Option<usize> {
    context
        .steps
        .iter()
        .position(|candidate| candidate.task_number == task && candidate.step_number == step)
}

pub(in crate::execution::commands) fn truncate_summary(summary: &str) -> String {
    if summary.chars().count() <= 120 {
        return summary.to_owned();
    }
    let truncated = summary.chars().take(117).collect::<String>();
    format!("{truncated}...")
}

pub(in crate::execution::commands) fn canonicalize_files(
    files: &[String],
) -> Result<Vec<String>, JsonFailure> {
    let mut normalized = files
        .iter()
        .map(|path| {
            let path = normalize_repo_relative_path(path).map_err(|_| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "Evidence file paths must be normalized repo-relative paths inside the repo root.",
                )
            })?;
            Ok(path)
        })
        .collect::<Result<Vec<_>, JsonFailure>>()?;
    normalized.sort();
    normalized.dedup();
    Ok(if normalized.is_empty() {
        vec![String::from(NO_REPO_FILES_MARKER)]
    } else {
        normalized
    })
}

pub(in crate::execution::commands) fn canonicalize_repo_visible_paths(
    repo_root: &Path,
    files: &[String],
) -> Result<Vec<String>, JsonFailure> {
    let missing = files
        .iter()
        .filter(|path| !repo_root.join(path).exists())
        .cloned()
        .collect::<BTreeSet<_>>();
    if missing.is_empty() {
        return Ok(files.to_vec());
    }

    let rename_map = rename_backed_paths(repo_root, &missing)?;
    let mut canonical = files
        .iter()
        .map(|path| {
            rename_map
                .get(path)
                .cloned()
                .unwrap_or_else(|| path.clone())
        })
        .collect::<Vec<_>>();
    canonical.sort();
    canonical.dedup();
    Ok(canonical)
}

pub(in crate::execution::commands) fn rename_backed_paths(
    repo_root: &Path,
    missing: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not discover the repository while canonicalizing rename-backed file paths: {error}"
            ),
        )
    })?;
    let head_tree = repo.head_tree_id_or_empty().map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not determine the HEAD tree while canonicalizing rename-backed file paths: {error}"
            ),
        )
    })?;
    let index = repo.index_or_empty().map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not open the repository index while canonicalizing rename-backed file paths: {error}"
            ),
        )
    })?;

    let mut paths = BTreeMap::new();
    repo.tree_index_status(
        head_tree.detach().as_ref(),
        &index,
        None,
        gix::status::tree_index::TrackRenames::AsConfigured,
        |change, _, _| {
            if let gix::diff::index::ChangeRef::Rewrite {
                source_location,
                location,
                copy,
                ..
            } = change
                && !copy
            {
                let source = String::from_utf8_lossy(source_location.as_ref()).into_owned();
                if missing.contains(&source) {
                    let destination = String::from_utf8_lossy(location.as_ref()).into_owned();
                    paths.insert(source, destination);
                    if paths.len() == missing.len() {
                        return Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Break(()));
                    }
                }
            }
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
        },
    )
    .map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not canonicalize rename-backed file paths from the current change set: {error}"
            ),
        )
    })?;
    Ok(paths)
}

pub(in crate::execution::commands) fn default_files_for_task(
    context: &ExecutionContext,
    task_number: u32,
) -> Vec<String> {
    let Some(task) = context.tasks_by_number.get(&task_number) else {
        return vec![String::from(NO_REPO_FILES_MARKER)];
    };
    let mut files = task
        .files
        .iter()
        .map(|entry| entry.path.clone())
        .filter(|path| context.runtime.repo_root.join(path).exists())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    if files.is_empty() {
        vec![String::from(NO_REPO_FILES_MARKER)]
    } else {
        files
    }
}

pub(in crate::execution::commands) fn next_attempt_number(
    evidence: &ExecutionEvidence,
    task: u32,
    step: u32,
) -> u32 {
    evidence
        .attempts
        .iter()
        .filter(|attempt| attempt.task_number == task && attempt.step_number == step)
        .map(|attempt| attempt.attempt_number)
        .max()
        .unwrap_or(0)
        + 1
}

pub(in crate::execution::commands) fn record_execution_projection_fingerprints(
    authoritative_state: Option<&mut AuthoritativeTransitionState>,
    rendered: &RenderedExecutionProjections,
) -> Result<(), JsonFailure> {
    if let Some(authoritative_state) = authoritative_state {
        authoritative_state.set_execution_projection_fingerprints(
            &sha256_hex(rendered.plan.as_bytes()),
            &sha256_hex(rendered.evidence.as_bytes()),
        )?;
    }
    Ok(())
}

pub(in crate::execution::commands) fn invalidate_latest_completed_attempt(
    context: &mut ExecutionContext,
    task: u32,
    step: u32,
    reason: &str,
) -> Result<(), JsonFailure> {
    let attempt_index =
        context
            .evidence
            .attempts
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, attempt)| {
                (attempt.task_number == task
                    && attempt.step_number == step
                    && attempt.status == "Completed")
                    .then_some(index)
            });
    let Some(attempt_index) = attempt_index else {
        return Ok(());
    };
    context.evidence.attempts[attempt_index].status = String::from("Invalidated");
    context.evidence.attempts[attempt_index].recorded_at = Timestamp::now().to_string();
    context.evidence.attempts[attempt_index].invalidation_reason = reason.to_owned();
    Ok(())
}

pub(in crate::execution::commands) fn persist_authoritative_state_with_rollback(
    authoritative_state: &AuthoritativeTransitionState,
    command: &str,
    plan_path: &Path,
    original_plan: &str,
    evidence_path: &Path,
    _original_evidence: Option<&str>,
    failpoint: &str,
) -> Result<(), JsonFailure> {
    let rollback = AuthoritativePersistRollback {
        plan_path,
        original_plan,
        evidence_path,
        failpoint,
    };
    persist_authoritative_state_with_step_hint_and_rollback(
        authoritative_state,
        command,
        None,
        rollback,
    )
}

pub(in crate::execution::commands) fn persist_authoritative_state_without_rollback(
    authoritative_state: &AuthoritativeTransitionState,
    command: &str,
) -> Result<(), JsonFailure> {
    authoritative_state.persist_if_dirty_with_failpoint_and_command(None, command)
}

pub(in crate::execution::commands) struct AuthoritativePersistRollback<'a> {
    pub(in crate::execution::commands) plan_path: &'a Path,
    pub(in crate::execution::commands) original_plan: &'a str,
    pub(in crate::execution::commands) evidence_path: &'a Path,
    pub(in crate::execution::commands) failpoint: &'a str,
}

pub(in crate::execution::commands) fn persist_authoritative_state_with_step_hint_and_rollback(
    authoritative_state: &AuthoritativeTransitionState,
    command: &str,
    step_hint: Option<(u32, u32)>,
    rollback: AuthoritativePersistRollback<'_>,
) -> Result<(), JsonFailure> {
    let original_evidence = rollback_evidence_source(rollback.evidence_path)?;
    let outcome = match authoritative_state
        .persist_if_dirty_with_failpoint_command_outcome_and_step_hint(
            Some(rollback.failpoint),
            command,
            step_hint,
        ) {
        Ok(outcome) => outcome,
        Err(error) => {
            restore_plan_and_evidence(
                rollback.plan_path,
                rollback.original_plan,
                rollback.evidence_path,
                original_evidence.as_deref(),
            );
            return Err(error);
        }
    };
    if let Some(error) = outcome.projection_refresh_failure {
        if !outcome.authoritative_event_committed {
            restore_plan_and_evidence(
                rollback.plan_path,
                rollback.original_plan,
                rollback.evidence_path,
                original_evidence.as_deref(),
            );
        }
        return Err(error);
    }
    Ok(())
}

pub(in crate::execution::commands) fn rollback_evidence_source(
    evidence_path: &Path,
) -> Result<Option<String>, JsonFailure> {
    match fs::read_to_string(evidence_path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Could not read tracked execution evidence before authoritative mutation rollback setup: {error}"
            ),
        )),
    }
}

pub(in crate::execution::commands) fn restore_plan_and_evidence(
    plan_path: &Path,
    original_plan: &str,
    evidence_path: &Path,
    original_evidence: Option<&str>,
) {
    let _ = fs::write(plan_path, original_plan);
    match original_evidence {
        Some(source) => {
            let _ = fs::write(evidence_path, source);
        }
        None => {
            let _ = fs::remove_file(evidence_path);
        }
    }
}

pub(in crate::execution::commands) fn maybe_trigger_failpoint(
    name: &str,
) -> Result<(), JsonFailure> {
    if std::env::var("FEATUREFORGE_PLAN_EXECUTION_TEST_FAILPOINT")
        .ok()
        .as_deref()
        == Some(name)
    {
        return Err(JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Injected plan execution failpoint: {name}"),
        ));
    }
    Ok(())
}

pub(in crate::execution::commands) fn write_atomic(
    path: &Path,
    contents: &str,
) -> Result<(), JsonFailure> {
    write_atomic_file(path, contents).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not persist {}: {error}", path.display()),
        )
    })
}

pub(in crate::execution::commands) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}
