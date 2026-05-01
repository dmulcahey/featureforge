use super::common::*;

pub fn rebuild_evidence(
    runtime: &ExecutionRuntime,
    args: &RebuildEvidenceArgs,
) -> Result<RebuildEvidenceOutput, JsonFailure> {
    let request = normalize_rebuild_evidence_request(args)?;
    if request.max_jobs > 1 {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "max_jobs_parallel_unsupported: rebuild-evidence currently supports only --max-jobs 1.",
        ));
    }
    let started_at = Instant::now();
    let context = load_execution_context_for_rebuild(runtime, &request.plan)?;
    if context.evidence.source.is_none() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "session_not_found: no execution evidence session exists for the approved plan revision.",
        ));
    }
    let matched_scope_ids = matched_rebuild_scope_ids(&context, &request);
    let candidates = discover_rebuild_candidates(&context, &request)?;
    if (!request.tasks.is_empty() || !request.steps.is_empty()) && candidates.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!(
                "scope_empty: requested scope matched approved plan steps [{}] but none currently require rebuild.",
                matched_scope_ids.join(", ")
            ),
        ));
    }
    let filter = RebuildEvidenceFilter {
        all: request.all,
        tasks: request.tasks.clone(),
        steps: request.raw_steps.clone(),
        include_open: request.include_open,
        skip_manual_fallback: request.skip_manual_fallback,
        continue_on_error: request.continue_on_error,
        max_jobs: request.max_jobs,
        no_output: request.no_output,
        json: request.json,
    };
    let scope = rebuild_scope_label(&request);

    if request.dry_run {
        let targets = candidates
            .iter()
            .map(planned_rebuild_target)
            .collect::<Vec<_>>();
        return Ok(RebuildEvidenceOutput {
            session_root: context.runtime.repo_root.to_string_lossy().into_owned(),
            dry_run: true,
            filter,
            scope,
            counts: RebuildEvidenceCounts {
                planned: targets.len() as u32,
                rebuilt: 0,
                manual: 0,
                failed: 0,
                noop: u32::from(targets.is_empty()),
            },
            duration_ms: started_at.elapsed().as_millis() as u64,
            targets,
            exit_code: 0,
        });
    }

    if candidates.is_empty() {
        return Ok(RebuildEvidenceOutput {
            session_root: context.runtime.repo_root.to_string_lossy().into_owned(),
            dry_run: false,
            filter,
            scope,
            counts: RebuildEvidenceCounts {
                planned: 0,
                rebuilt: 0,
                manual: 0,
                failed: 0,
                noop: 1,
            },
            duration_ms: started_at.elapsed().as_millis() as u64,
            targets: Vec::new(),
            exit_code: 0,
        });
    }

    let mut targets = Vec::with_capacity(candidates.len());
    let mut counts = RebuildEvidenceCounts {
        planned: candidates.len() as u32,
        rebuilt: 0,
        manual: 0,
        failed: 0,
        noop: 0,
    };
    let candidate_batch_is_manual_only = request.skip_manual_fallback && !candidates.is_empty();
    let mut saw_strict_manual_failure = false;
    let mut saw_precondition_failure = false;
    let mut saw_non_precondition_failure = false;

    for (index, candidate) in candidates.iter().enumerate() {
        let target = execute_rebuild_candidate_projection_only(&request, candidate);
        match target.status.as_str() {
            "rebuilt" => counts.rebuilt += 1,
            "manual_required" => counts.manual += 1,
            "failed" => {
                counts.failed += 1;
                match target.failure_class.as_deref() {
                    Some("manual_required") => {
                        saw_strict_manual_failure = true;
                    }
                    Some(failure_class) if is_rebuild_precondition_failure(failure_class) => {
                        saw_precondition_failure = true;
                    }
                    _ => {
                        saw_non_precondition_failure = true;
                    }
                }
            }
            _ => {}
        }
        let should_stop = target.status == "failed"
            && target.failure_class.as_deref() != Some("artifact_read_error")
            && !request.continue_on_error;
        targets.push(target);
        if should_stop || index + 1 == candidates.len() {
            break;
        }
    }

    let strict_manual_only = candidate_batch_is_manual_only
        && saw_strict_manual_failure
        && !saw_precondition_failure
        && !saw_non_precondition_failure;
    let exit_code = if strict_manual_only {
        3
    } else if saw_non_precondition_failure || saw_strict_manual_failure {
        2
    } else if saw_precondition_failure {
        1
    } else {
        0
    };

    Ok(RebuildEvidenceOutput {
        session_root: context.runtime.repo_root.to_string_lossy().into_owned(),
        dry_run: false,
        filter,
        scope,
        counts,
        duration_ms: started_at.elapsed().as_millis() as u64,
        targets,
        exit_code,
    })
}
