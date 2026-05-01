use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceCounts {
    pub planned: u32,
    pub rebuilt: u32,
    pub manual: u32,
    pub failed: u32,
    pub noop: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceFilter {
    pub all: bool,
    pub tasks: Vec<u32>,
    pub steps: Vec<String>,
    pub include_open: bool,
    pub skip_manual_fallback: bool,
    pub continue_on_error: bool,
    pub max_jobs: u32,
    pub no_output: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceTarget {
    pub task_id: u32,
    pub step_id: u32,
    pub target_kind: String,
    pub pre_invalidation_reason: String,
    pub status: String,
    pub verify_mode: String,
    pub verify_command: Option<String>,
    pub attempt_id_before: Option<String>,
    pub attempt_id_after: Option<String>,
    pub verification_hash: Option<String>,
    pub error: Option<String>,
    pub failure_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceOutput {
    pub session_root: String,
    pub dry_run: bool,
    pub filter: RebuildEvidenceFilter,
    pub scope: String,
    pub counts: RebuildEvidenceCounts,
    pub duration_ms: u64,
    pub targets: Vec<RebuildEvidenceTarget>,
    #[serde(skip_serializing)]
    pub exit_code: u8,
}

impl RebuildEvidenceOutput {
    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub fn render_text(&self) -> String {
        let mut lines = Vec::with_capacity(self.targets.len() + 1);
        lines.push(format!(
            "summary scope={} dry_run={} planned={} rebuilt={} manual={} failed={} noop={}",
            render_text_value(&self.scope),
            self.dry_run,
            self.counts.planned,
            self.counts.rebuilt,
            self.counts.manual,
            self.counts.failed,
            self.counts.noop,
        ));
        for target in &self.targets {
            lines.push(format!(
                "target task_id={} step_id={} status={} target_kind={} pre_invalidation_reason={} verify_mode={} verify_command={} attempt_id_before={} attempt_id_after={} verification_hash={} error={} failure_class={}",
                target.task_id,
                target.step_id,
                render_text_value(&target.status),
                render_text_value(&target.target_kind),
                render_text_value(&target.pre_invalidation_reason),
                render_text_value(&target.verify_mode),
                render_optional_text_value(target.verify_command.as_deref()),
                render_optional_text_value(target.attempt_id_before.as_deref()),
                render_optional_text_value(target.attempt_id_after.as_deref()),
                render_optional_text_value(target.verification_hash.as_deref()),
                render_optional_text_value(target.error.as_deref()),
                render_optional_text_value(target.failure_class.as_deref()),
            ));
        }
        lines.join("\n") + "\n"
    }
}

fn render_text_value(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::from("\"<serialization-error>\""))
}

fn render_optional_text_value(value: Option<&str>) -> String {
    value
        .map(render_text_value)
        .unwrap_or_else(|| String::from("null"))
}

pub fn normalize_rebuild_evidence_request(
    args: &RebuildEvidenceArgs,
) -> Result<RebuildEvidenceRequest, JsonFailure> {
    let mut parsed_steps = Vec::with_capacity(args.steps.len());
    for raw in &args.steps {
        let (task, step) = raw.split_once(':').ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "--step must use task:step selectors such as 1:2.",
            )
        })?;
        let task = task.parse::<u32>().map_err(|_| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "--step must use numeric task:step selectors such as 1:2.",
            )
        })?;
        let step = step.parse::<u32>().map_err(|_| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "--step must use numeric task:step selectors such as 1:2.",
            )
        })?;
        parsed_steps.push((task, step));
    }

    Ok(RebuildEvidenceRequest {
        plan: args.plan.clone(),
        all: args.all || (args.tasks.is_empty() && args.steps.is_empty()),
        tasks: args.tasks.clone(),
        steps: parsed_steps,
        raw_steps: args.steps.clone(),
        include_open: args.include_open,
        skip_manual_fallback: args.skip_manual_fallback,
        continue_on_error: args.continue_on_error,
        dry_run: args.dry_run,
        max_jobs: args.max_jobs,
        no_output: args.no_output,
        json: args.json,
    })
}

struct RebuildCandidateScan {
    session_provenance_reason: Option<String>,
    source_spec_fingerprint: String,
    latest_attempts: BTreeMap<(u32, u32), usize>,
    latest_completed: BTreeMap<(u32, u32), usize>,
    latest_file_proofs: BTreeMap<String, usize>,
}

fn prepare_rebuild_candidate_scan(context: &ExecutionContext) -> RebuildCandidateScan {
    let contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let session_provenance_reason = if context.evidence.plan_fingerprint.as_deref()
        != Some(contract_plan_fingerprint.as_str())
    {
        Some(String::from("plan_fingerprint_mismatch"))
    } else if context.evidence.source_spec_fingerprint.as_deref()
        != Some(source_spec_fingerprint.as_str())
    {
        Some(String::from("source_spec_fingerprint_mismatch"))
    } else {
        None
    };
    let latest_attempts = latest_attempt_indices_by_step(&context.evidence);
    let latest_completed = latest_completed_attempts_by_step(&context.evidence);
    let latest_file_proofs =
        latest_completed_attempts_by_file(&context.evidence, &latest_completed);

    RebuildCandidateScan {
        session_provenance_reason,
        source_spec_fingerprint,
        latest_attempts,
        latest_completed,
        latest_file_proofs,
    }
}

fn rebuild_candidate_for_step(
    context: &ExecutionContext,
    scan: &RebuildCandidateScan,
    step: &PlanStepState,
    include_open: bool,
) -> Option<RebuildEvidenceCandidate> {
    let step_key = (step.task_number, step.step_number);
    let latest_attempt = scan
        .latest_attempts
        .get(&step_key)
        .map(|index| &context.evidence.attempts[*index]);
    let latest_completed_index = scan.latest_completed.get(&step_key).copied();
    let latest_completed_attempt =
        latest_completed_index.map(|index| &context.evidence.attempts[index]);

    let mut pre_invalidation_reason = None;
    let mut target_kind = String::new();
    let mut needs_reopen = false;

    if step.checked
        && let Some(reason) = scan.session_provenance_reason.as_ref()
        && latest_completed_attempt.is_some()
    {
        pre_invalidation_reason = Some(reason.clone());
        target_kind = String::from("stale_completed_attempt");
        needs_reopen = true;
    }

    if let Some(attempt) = latest_attempt
        && attempt.status == "Invalidated"
        && attempt.invalidation_reason != "N/A"
    {
        pre_invalidation_reason = Some(attempt.invalidation_reason.clone());
        target_kind = String::from("invalidated_attempt");
        needs_reopen = step.checked;
    }

    if pre_invalidation_reason.is_none()
        && step.checked
        && let Some(attempt) = latest_completed_attempt
    {
        let expected_packet = task_packet_fingerprint(
            context,
            &scan.source_spec_fingerprint,
            step.task_number,
            step.step_number,
        )?;
        if attempt.packet_fingerprint.as_deref() != Some(expected_packet.as_str()) {
            pre_invalidation_reason = Some(String::from("packet_fingerprint_mismatch"));
            target_kind = String::from("stale_completed_attempt");
            needs_reopen = true;
        } else {
            for proof in &attempt.file_proofs {
                if proof.path == NO_REPO_FILES_MARKER
                    || proof.path == context.plan_rel
                    || proof.path == context.evidence_rel
                {
                    continue;
                }
                if scan
                    .latest_file_proofs
                    .get(&proof.path)
                    .is_some_and(|latest_index| {
                        latest_completed_index
                            .is_some_and(|attempt_index| *latest_index != attempt_index)
                    })
                {
                    continue;
                }
                match current_file_proof_checked(&context.runtime.repo_root, &proof.path) {
                    Ok(current_proof) => {
                        if current_proof != proof.proof {
                            pre_invalidation_reason = Some(String::from("files_proven_drifted"));
                            target_kind = String::from("stale_completed_attempt");
                            needs_reopen = true;
                            break;
                        }
                    }
                    Err(error) => {
                        pre_invalidation_reason = Some(format!(
                            "artifact_read_error: could not read {} ({error})",
                            proof.path
                        ));
                        target_kind = String::from("artifact_read_error");
                        needs_reopen = false;
                        break;
                    }
                }
            }
        }
    }

    if pre_invalidation_reason.is_none()
        && include_open
        && !step.checked
        && (step.note_state.is_some() || latest_attempt.is_some())
    {
        pre_invalidation_reason = Some(String::from("open_step_requested"));
        target_kind = String::from("open_step");
    }

    let pre_invalidation_reason = pre_invalidation_reason?;
    let attempt = latest_attempt.or(latest_completed_attempt);
    let verify_command = attempt.and_then(|candidate| candidate.verify_command.clone());
    let verify_mode = if verify_command.is_some() {
        String::from("command")
    } else {
        String::from("manual")
    };
    let claim = attempt
        .map(|candidate| candidate.claim.clone())
        .unwrap_or_else(|| {
            format!(
                "Rebuilt evidence for Task {} Step {}.",
                step.task_number, step.step_number
            )
        });
    let files = attempt
        .map(|candidate| candidate.files.clone())
        .unwrap_or_default();
    let attempt_number = attempt.map(|candidate| candidate.attempt_number);
    let artifact_epoch = attempt.map(|candidate| candidate.recorded_at.clone());

    Some(RebuildEvidenceCandidate {
        task: step.task_number,
        step: step.step_number,
        order_key: (step.task_number, step.step_number),
        target_kind,
        pre_invalidation_reason,
        verify_command,
        verify_mode,
        claim,
        files,
        attempt_number,
        artifact_epoch,
        needs_reopen,
    })
}

pub fn discover_rebuild_candidates(
    context: &ExecutionContext,
    request: &RebuildEvidenceRequest,
) -> Result<Vec<RebuildEvidenceCandidate>, JsonFailure> {
    let task_filter = request.tasks.iter().copied().collect::<BTreeSet<_>>();
    let step_filter = request.steps.iter().copied().collect::<BTreeSet<_>>();

    let matching_steps = context
        .steps
        .iter()
        .filter(|step| {
            (task_filter.is_empty() || task_filter.contains(&step.task_number))
                && (step_filter.is_empty()
                    || step_filter.contains(&(step.task_number, step.step_number)))
        })
        .collect::<Vec<_>>();
    if (!request.tasks.is_empty() || !request.steps.is_empty()) && matching_steps.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "scope_no_matches: no approved plan steps matched the requested filters.",
        ));
    }

    let scan = prepare_rebuild_candidate_scan(context);
    let mut candidates = Vec::new();

    for step in matching_steps {
        if let Some(candidate) =
            rebuild_candidate_for_step(context, &scan, step, request.include_open)
        {
            candidates.push(candidate);
        }
    }

    candidates.sort_by_key(|candidate| candidate.order_key);

    Ok(candidates)
}

pub fn validate_v2_evidence_provenance(context: &ExecutionContext, gate: &mut GateState) {
    let contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let latest_attempts = latest_completed_attempts_by_step(&context.evidence);
    let latest_file_proofs = latest_completed_attempts_by_file(&context.evidence, &latest_attempts);

    if context.evidence.plan_fingerprint.as_deref() != Some(contract_plan_fingerprint.as_str()) {
        gate.fail(
            FailureClass::StaleExecutionEvidence,
            "plan_fingerprint_mismatch",
            "Execution evidence plan fingerprint no longer matches the approved plan source.",
            "Rebuild the execution evidence for the current approved plan revision.",
        );
    }
    if context.evidence.source_spec_fingerprint.as_deref() != Some(source_spec_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::StaleExecutionEvidence,
            "source_spec_fingerprint_mismatch",
            "Execution evidence source spec fingerprint no longer matches the approved source spec.",
            "Rebuild the execution evidence for the current approved spec revision.",
        );
    }

    for step in context.steps.iter().filter(|step| step.checked) {
        let Some(attempt_index) = latest_attempts
            .get(&(step.task_number, step.step_number))
            .copied()
        else {
            continue;
        };
        let attempt = &context.evidence.attempts[attempt_index];
        let expected_packet = task_packet_fingerprint(
            context,
            &source_spec_fingerprint,
            step.task_number,
            step.step_number,
        );
        if attempt.packet_fingerprint.as_deref() != expected_packet.as_deref() {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "packet_fingerprint_mismatch",
                format!(
                    "Task {} Step {} evidence packet provenance no longer matches the current approved plan/spec pair.",
                    step.task_number, step.step_number
                ),
                "Rebuild the packet and reopen the affected step.",
            );
        }
        for proof in &attempt.file_proofs {
            if proof.path == NO_REPO_FILES_MARKER
                || proof.path == context.plan_rel
                || proof.path == context.evidence_rel
            {
                continue;
            }
            if latest_file_proofs
                .get(&proof.path)
                .is_some_and(|latest_index| *latest_index != attempt_index)
            {
                continue;
            }
            let current = current_file_proof(&context.runtime.repo_root, &proof.path);
            if current != proof.proof {
                gate.fail(
                    FailureClass::MissedReopenRequired,
                    "files_proven_drifted",
                    format!(
                        "Task {} Step {} proved file '{}' no longer matches its recorded fingerprint.",
                        step.task_number, step.step_number, proof.path
                    ),
                    "Reopen the step and rebuild its evidence.",
                );
            }
        }
    }
}
