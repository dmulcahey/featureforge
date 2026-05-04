use std::cell::RefCell;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::contracts::plan::{PlanDocument, PlanTask, parse_plan_file};
use crate::contracts::task_contract::{
    RuntimeExecutionNoteProjectionBlock, known_runtime_step_projection_lines,
    parse_task_step_projection_line,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::event_log::load_reduced_authoritative_state;
use crate::execution::final_review::is_canonical_fingerprint;
use crate::execution::projection_renderer::{
    projection_source_matches_fingerprint, read_state_dir_projection,
    render_canonical_evidence_projection_source,
    render_canonical_evidence_projection_source_with_fingerprints,
    state_dir_projection_matches_recorded_output_fingerprint,
};
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::semantic_identity::{
    normalized_plan_source_for_semantic_identity, semantic_workspace_snapshot,
    task_definition_identity_for_task,
};
use crate::execution::transitions::{
    AuthoritativeTransitionState, OpenStepStateRecord,
    authoritative_state_optional_string_field_for_runtime,
    load_authoritative_transition_state_relaxed,
};
use crate::git::{
    canonicalize_repo_root_path, discover_repository, sha256_hex, stored_repo_root_matches_current,
};
use crate::paths::{RepoPath, normalize_repo_relative_path, normalize_whitespace};
use crate::workflow::manifest::{ManifestLoadResult, WorkflowManifest, load_manifest_read_only};
use crate::workflow::markdown_scan::markdown_files_under;

pub const NO_REPO_FILES_MARKER: &str = "__featureforge__/no-repo-files";
const ACTIVE_SPEC_ROOT: &str = "docs/featureforge/specs";
const ACTIVE_PLAN_ROOT: &str = "docs/featureforge/plans";
const ACTIVE_EVIDENCE_ROOT: &str = "docs/featureforge/execution-evidence";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteState {
    Active,
    Blocked,
    Interrupted,
}

impl NoteState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Blocked => "Blocked",
            Self::Interrupted => "Interrupted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanStepState {
    pub task_number: u32,
    pub step_number: u32,
    pub title: String,
    pub checked: bool,
    pub note_state: Option<NoteState>,
    pub note_summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceFormat {
    Empty,
    Legacy,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceSourceOrigin {
    Empty,
    TrackedFile,
    StateDirProjection,
    AuthoritativeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateDirProjectionCurrentness {
    Unbound,
    Current,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileProof {
    pub path: String,
    pub proof: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAttempt {
    pub task_number: u32,
    pub step_number: u32,
    pub attempt_number: u32,
    pub status: String,
    pub recorded_at: String,
    pub execution_source: String,
    pub claim: String,
    pub files: Vec<String>,
    pub file_proofs: Vec<FileProof>,
    pub verify_command: Option<String>,
    pub verification_summary: String,
    pub invalidation_reason: String,
    pub packet_fingerprint: Option<String>,
    pub head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub source_contract_path: Option<String>,
    pub source_contract_fingerprint: Option<String>,
    pub source_evaluation_report_fingerprint: Option<String>,
    pub evaluator_verdict: Option<String>,
    pub failing_criterion_ids: Vec<String>,
    pub source_handoff_fingerprint: Option<String>,
    pub repo_state_baseline_head_sha: Option<String>,
    pub repo_state_baseline_worktree_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionEvidence {
    pub format: EvidenceFormat,
    pub plan_path: String,
    pub plan_revision: u32,
    pub plan_fingerprint: Option<String>,
    pub source_spec_path: String,
    pub source_spec_revision: u32,
    pub source_spec_fingerprint: Option<String>,
    pub attempts: Vec<EvidenceAttempt>,
    pub source: Option<String>,
    pub(crate) source_origin: EvidenceSourceOrigin,
    pub(crate) tracked_progress_present: bool,
    pub(crate) tracked_source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub runtime: ExecutionRuntime,
    pub plan_rel: String,
    pub plan_abs: PathBuf,
    pub plan_document: PlanDocument,
    pub plan_source: String,
    pub steps: Vec<PlanStepState>,
    pub(crate) local_execution_progress_markers_present: bool,
    pub(crate) legacy_open_step_projection_present: bool,
    pub tasks_by_number: BTreeMap<u32, PlanTask>,
    pub evidence_rel: String,
    pub evidence_abs: PathBuf,
    pub evidence: ExecutionEvidence,
    pub(crate) authoritative_evidence_projection_fingerprint: Option<String>,
    pub source_spec_source: String,
    pub source_spec_path: PathBuf,
    pub execution_fingerprint: String,
    pub(crate) tracked_tree_sha_cache: OnceLock<Result<String, JsonFailure>>,
    pub(crate) semantic_workspace_snapshot_cache: OnceLock<
        Result<crate::execution::semantic_identity::SemanticWorkspaceSnapshot, JsonFailure>,
    >,
    pub(crate) reviewed_tree_sha_cache: RefCell<BTreeMap<String, String>>,
    pub(crate) head_sha_cache: OnceLock<Result<String, JsonFailure>>,
    pub(crate) release_base_branch_cache: OnceLock<Option<String>>,
    pub(crate) tracked_worktree_changes_excluding_execution_evidence_cache:
        OnceLock<Result<bool, JsonFailure>>,
}

pub fn load_execution_context(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Reject,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::RequireUnique,
        true,
    )
}

pub fn load_execution_context_for_mutation(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Allow,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        true,
    )
}

pub(crate) fn load_execution_context_for_rebuild(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Allow,
        TrackedEvidenceProjectionPolicy::AllowExplicitImport,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        true,
    )
}

pub(crate) fn load_execution_context_for_exact_plan(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Reject,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        true,
    )
}

pub(crate) fn load_execution_context_without_authority_overlay(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Reject,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        false,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyEvidencePolicy {
    Reject,
    Allow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackedEvidenceProjectionPolicy {
    Ignore,
    AllowExplicitImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApprovedArtifactSelectionPolicy {
    RequireUnique,
    AllowExactPlan,
}

fn load_execution_context_with_policies(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    legacy_evidence_policy: LegacyEvidencePolicy,
    tracked_evidence_policy: TrackedEvidenceProjectionPolicy,
    selection_policy: ApprovedArtifactSelectionPolicy,
    apply_authority_overlay: bool,
) -> Result<ExecutionContext, JsonFailure> {
    let plan_rel = normalize_plan_path(plan_path)?;
    let plan_abs = runtime.repo_root.join(&plan_rel);
    if !plan_abs.is_file() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Approved plan file does not exist.",
        ));
    }

    let mut plan_document = parse_plan_file(&plan_abs).map_err(|error| {
        JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            format!("Approved plan headers are missing or malformed: {error}"),
        )
    })?;
    if plan_document.workflow_state != "Engineering Approved" {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Plan is not Engineering Approved.",
        ));
    }
    match plan_document.execution_mode.as_str() {
        "none" | "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::PlanNotExecutionReady,
                "Execution Mode header is missing, malformed, or out of range.",
            ));
        }
    }
    if plan_document.last_reviewed_by != "plan-eng-review" {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan Last Reviewed By header is missing or malformed.",
        ));
    }
    if plan_document.tasks.iter().any(|task| task.files.is_empty()) {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan tasks require a parseable Files block.",
        ));
    }

    let plan_source = fs::read_to_string(&plan_abs).map_err(|error| {
        JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            format!(
                "Could not read approved plan {}: {error}",
                plan_abs.display()
            ),
        )
    })?;
    let mut parsed_steps = parse_step_state(&plan_source, &plan_document)?;
    let markdown_has_checked_steps = parsed_steps.iter().any(|step| step.checked);
    let markdown_has_open_step_notes = parsed_steps.iter().any(|step| step.note_state.is_some());

    // Legacy markdown execution marks and notes are migration candidates only.
    // They must not remain live read-surface authority once captured.
    clear_open_step_projections_for_steps(&mut parsed_steps);
    for step in &mut parsed_steps {
        step.checked = false;
    }

    let source_spec_path = runtime.repo_root.join(&plan_document.source_spec_path);
    let source_spec_source = fs::read_to_string(&source_spec_path).map_err(|_| {
        JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec does not exist.",
        )
    })?;
    let matching_manifest = matching_workflow_manifest(runtime);
    validate_source_spec(
        &source_spec_source,
        &plan_document.source_spec_path,
        plan_document.source_spec_revision,
        runtime,
        matching_manifest.as_ref(),
        selection_policy,
    )?;
    validate_unique_approved_plan(
        &plan_rel,
        &plan_document.source_spec_path,
        plan_document.source_spec_revision,
        runtime,
        matching_manifest.as_ref(),
        selection_policy,
    )?;

    let evidence_rel = derive_evidence_rel_path(&plan_rel, plan_document.plan_revision);
    let evidence_abs = runtime.repo_root.join(&evidence_rel);
    let plan_header_execution_mode = plan_document.execution_mode.clone();
    let evidence = parse_execution_evidence_projection(ExecutionEvidenceProjectionParseInput {
        runtime,
        evidence_rel: &evidence_rel,
        evidence_abs: &evidence_abs,
        plan_source: &plan_source,
        expected_plan_path: &plan_rel,
        plan_document: &plan_document,
        steps: &parsed_steps,
        expected_spec_path: &plan_document.source_spec_path,
        source_spec_source: &source_spec_source,
        allow_legacy_unbound: legacy_evidence_policy == LegacyEvidencePolicy::Allow,
        allow_tracked_projection: tracked_evidence_policy
            == TrackedEvidenceProjectionPolicy::AllowExplicitImport,
    })?;

    infer_execution_mode_from_evidence(&mut plan_document, &evidence);

    if legacy_evidence_policy == LegacyEvidencePolicy::Reject
        && evidence.format == EvidenceFormat::Legacy
        && !evidence.attempts.is_empty()
    {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Legacy pre-harness execution evidence is no longer accepted; regenerate execution evidence using the harness v2 format.",
        ));
    }

    if plan_document.execution_mode == "none" && !evidence.attempts.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Execution evidence history cannot exist while Execution Mode is none.",
        ));
    }
    let local_execution_progress_markers_present = plan_header_execution_mode != "none"
        || (evidence.source_origin == EvidenceSourceOrigin::TrackedFile
            && !evidence.attempts.is_empty());
    let tasks_by_number = plan_document
        .tasks
        .iter()
        .cloned()
        .map(|task| (task.number, task))
        .collect();
    let mut context = ExecutionContext {
        runtime: runtime.clone(),
        plan_rel,
        plan_abs,
        plan_document,
        plan_source,
        steps: parsed_steps,
        local_execution_progress_markers_present,
        legacy_open_step_projection_present: markdown_has_open_step_notes,
        tasks_by_number,
        evidence_rel,
        evidence_abs,
        evidence,
        authoritative_evidence_projection_fingerprint: None,
        source_spec_source,
        source_spec_path,
        execution_fingerprint: String::new(),
        tracked_tree_sha_cache: OnceLock::new(),
        semantic_workspace_snapshot_cache: OnceLock::new(),
        reviewed_tree_sha_cache: RefCell::new(BTreeMap::new()),
        head_sha_cache: OnceLock::new(),
        release_base_branch_cache: OnceLock::new(),
        tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
    };

    let authoritative_state = if apply_authority_overlay {
        load_authoritative_transition_state_relaxed(&context)?
    } else {
        None
    };
    if apply_authority_overlay {
        overlay_execution_evidence_attempts_from_authority(
            &mut context,
            authoritative_state.as_ref(),
        )?;
        infer_execution_mode_from_evidence(&mut context.plan_document, &context.evidence);
        overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    }
    refresh_execution_fingerprint(&mut context);

    if context.plan_document.execution_mode == "none"
        && (markdown_has_checked_steps || markdown_has_open_step_notes)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Newly approved plan revisions must start execution-clean.",
        ));
    }

    for attempt in &context.evidence.attempts {
        if !context.steps.iter().any(|step| {
            step.task_number == attempt.task_number && step.step_number == attempt.step_number
        }) {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Execution evidence references a task/step that does not exist in the approved plan.",
            ));
        }
        normalize_source(
            &attempt.execution_source,
            &context.plan_document.execution_mode,
        )
        .map_err(|_| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Execution evidence source must match the persisted execution mode.",
            )
        })?;
    }

    Ok(context)
}

pub fn derive_evidence_rel_path(plan_rel: &str, revision: u32) -> String {
    let base = Path::new(plan_rel)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("plan");
    format!("{ACTIVE_EVIDENCE_ROOT}/{base}-r{revision}-evidence.md")
}

pub fn hash_contract_plan(source: &str) -> String {
    let sanitized_steps = parse_contract_render(source);
    let semantic_plan = normalized_plan_source_for_semantic_identity(&sanitized_steps);
    sha256_hex(semantic_plan.as_bytes())
}

pub fn render_contract_plan(source: &str) -> String {
    parse_contract_render(source)
}

pub struct PacketFingerprintInput<'a> {
    pub plan_path: &'a str,
    pub plan_revision: u32,
    pub task_definition_identity: &'a str,
    pub source_spec_path: &'a str,
    pub source_spec_revision: u32,
    pub source_spec_fingerprint: &'a str,
    pub task: u32,
    pub step: u32,
}

pub fn compute_packet_fingerprint(input: PacketFingerprintInput<'_>) -> String {
    let payload = format!(
        "plan_path={plan_path}\nplan_revision={plan_revision}\ntask_definition_identity={task_definition_identity}\nsource_spec_path={source_spec_path}\nsource_spec_revision={source_spec_revision}\nsource_spec_fingerprint={source_spec_fingerprint}\ntask_number={task}\nstep_number={step}\n",
        plan_path = input.plan_path,
        plan_revision = input.plan_revision,
        task_definition_identity = input.task_definition_identity,
        source_spec_path = input.source_spec_path,
        source_spec_revision = input.source_spec_revision,
        source_spec_fingerprint = input.source_spec_fingerprint,
        task = input.task,
        step = input.step,
    );
    sha256_hex(payload.as_bytes())
}

pub fn task_packet_fingerprint(
    context: &ExecutionContext,
    source_spec_fingerprint: &str,
    task: u32,
    step: u32,
) -> Option<String> {
    let task_definition_identity = task_definition_identity_for_task(context, task).ok()??;
    Some(compute_packet_fingerprint(PacketFingerprintInput {
        plan_path: &context.plan_rel,
        plan_revision: context.plan_document.plan_revision,
        task_definition_identity: &task_definition_identity,
        source_spec_path: &context.plan_document.source_spec_path,
        source_spec_revision: context.plan_document.source_spec_revision,
        source_spec_fingerprint,
        task,
        step,
    }))
}

pub(crate) fn has_other_same_branch_worktree(current_runtime: &ExecutionRuntime) -> bool {
    if current_runtime.branch_name == "current" {
        return false;
    }
    same_branch_worktrees(&current_runtime.repo_root)
        .into_iter()
        .filter(|root| root != &current_runtime.repo_root)
        .filter_map(|root| ExecutionRuntime::discover(&root).ok())
        .any(|runtime| {
            runtime.branch_name != "current" && runtime.branch_name == current_runtime.branch_name
        })
}

pub(crate) fn clear_open_step_projections_for_steps(steps: &mut [PlanStepState]) {
    for step in steps {
        step.note_state = None;
        step.note_summary.clear();
    }
}

fn clear_open_step_projections(context: &mut ExecutionContext) {
    clear_open_step_projections_for_steps(&mut context.steps);
}

pub(crate) fn clear_projection_only_execution_progress(context: &mut ExecutionContext) {
    clear_open_step_projections(context);
    context.authoritative_evidence_projection_fingerprint = None;
    if matches!(
        context.evidence.source_origin,
        EvidenceSourceOrigin::StateDirProjection | EvidenceSourceOrigin::AuthoritativeState
    ) {
        let tracked_progress_present = context.evidence.tracked_progress_present;
        let tracked_source = context.evidence.tracked_source.clone();
        let source_origin = if tracked_source.is_some() {
            EvidenceSourceOrigin::TrackedFile
        } else {
            EvidenceSourceOrigin::Empty
        };
        context.evidence = ExecutionEvidence {
            format: EvidenceFormat::Empty,
            plan_path: context.plan_rel.clone(),
            plan_revision: context.plan_document.plan_revision,
            plan_fingerprint: None,
            source_spec_path: context.plan_document.source_spec_path.clone(),
            source_spec_revision: context.plan_document.source_spec_revision,
            source_spec_fingerprint: None,
            attempts: Vec::new(),
            source: tracked_source.clone(),
            source_origin,
            tracked_progress_present,
            tracked_source,
        };
        if !context.local_execution_progress_markers_present {
            context.plan_document.execution_mode = String::from("none");
        }
    }
}

pub(crate) fn infer_execution_mode_from_evidence(
    plan_document: &mut PlanDocument,
    evidence: &ExecutionEvidence,
) {
    if plan_document.execution_mode != "none" {
        return;
    }
    if let Some(execution_source) = evidence
        .attempts
        .iter()
        .rev()
        .map(|attempt| attempt.execution_source.as_str())
        .find(|source| {
            matches!(
                *source,
                "featureforge:executing-plans" | "featureforge:subagent-driven-development"
            )
        })
    {
        plan_document.execution_mode = execution_source.to_owned();
    }
}

pub(crate) fn overlay_step_state_from_authority(
    context: &mut ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<(), JsonFailure> {
    let Some(authoritative_state) = authoritative_state else {
        return Ok(());
    };
    let state_payload = authoritative_state.state_payload_snapshot();
    overlay_task_closure_completed_steps(context, &state_payload)?;
    let Some(completed_steps) = state_payload
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
    else {
        overlay_authoritative_open_step_state_from_state(context, authoritative_state)?;
        return Ok(());
    };
    overlay_event_completed_steps(context, completed_steps)?;
    overlay_authoritative_open_step_state_from_state(context, authoritative_state)
}

pub(crate) fn overlay_execution_evidence_attempts_from_authority(
    context: &mut ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<(), JsonFailure> {
    let Some(authoritative_state) = authoritative_state else {
        return Ok(());
    };
    let state_payload = authoritative_state.state_payload_snapshot();
    context.authoritative_evidence_projection_fingerprint = state_payload
        .get("execution_evidence_projection_fingerprint")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let Some(attempts_value) = state_payload
        .get("execution_evidence_attempts")
        .filter(|value| !value.is_null())
        .cloned()
    else {
        if context.evidence.attempts.is_empty() {
            synthesize_legacy_authoritative_evidence_attempts(context, &state_payload)?;
        }
        return Ok(());
    };
    let attempts =
        serde_json::from_value::<Vec<EvidenceAttempt>>(attempts_value).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!("Authoritative execution evidence attempts are malformed: {error}"),
            )
        })?;
    context.evidence.format = if attempts.is_empty() {
        EvidenceFormat::Empty
    } else {
        EvidenceFormat::V2
    };
    context.evidence.plan_fingerprint = Some(hash_contract_plan(&context.plan_source));
    context.evidence.source_spec_fingerprint =
        Some(sha256_hex(context.source_spec_source.as_bytes()));
    context.evidence.attempts = attempts;
    context.evidence.source = Some(render_canonical_evidence_projection_source(
        &context.plan_rel,
        &context.plan_document,
        &context.plan_source,
        &context.source_spec_source,
        &context.steps,
        &context.evidence,
    ));
    context.evidence.source_origin = EvidenceSourceOrigin::AuthoritativeState;
    Ok(())
}

fn synthesize_legacy_authoritative_evidence_attempts(
    context: &mut ExecutionContext,
    state_payload: &serde_json::Value,
) -> Result<(), JsonFailure> {
    let completed_steps = authoritative_completed_steps_for_evidence(context, state_payload)?;
    if completed_steps.is_empty() {
        return Ok(());
    }
    let execution_source = if context.plan_document.execution_mode == "none" {
        context.plan_document.execution_mode = String::from("featureforge:executing-plans");
        String::from("featureforge:executing-plans")
    } else {
        context.plan_document.execution_mode.clone()
    };
    context.evidence.format = EvidenceFormat::V2;
    let plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let head_sha = state_payload
        .get("repo_state_baseline_head_sha")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            context
                .current_head_sha()
                .unwrap_or_else(|_| String::from("authoritative-event-log"))
        });
    context.evidence.plan_fingerprint = Some(plan_fingerprint);
    context.evidence.source_spec_fingerprint = Some(source_spec_fingerprint.clone());
    context.evidence.attempts = completed_steps
        .into_iter()
        .map(|((task_number, step_number), files)| {
            let files = if files.is_empty() {
                vec![NO_REPO_FILES_MARKER.to_owned()]
            } else {
                files
            };
            let file_proofs = files
                .iter()
                .map(|path| FileProof {
                    path: path.clone(),
                    proof: current_file_proof(&context.runtime.repo_root, path),
                })
                .collect::<Vec<_>>();
            let packet_fingerprint = task_packet_fingerprint(
                context,
                &source_spec_fingerprint,
                task_number,
                step_number,
            );
            EvidenceAttempt {
                task_number,
                step_number,
                attempt_number: 1,
                status: String::from("Completed"),
                recorded_at: String::from("authoritative-event-log"),
                execution_source: execution_source.clone(),
                claim: format!(
                    "Authoritative event log marks Task {task_number} Step {step_number} complete."
                ),
                files,
                file_proofs,
                verify_command: None,
                verification_summary: String::from(
                    "Recovered from authoritative completed-step state.",
                ),
                invalidation_reason: String::from("N/A"),
                packet_fingerprint,
                head_sha: Some(head_sha.clone()),
                base_sha: Some(head_sha.clone()),
                source_contract_path: None,
                source_contract_fingerprint: None,
                source_evaluation_report_fingerprint: None,
                evaluator_verdict: None,
                failing_criterion_ids: Vec::new(),
                source_handoff_fingerprint: None,
                repo_state_baseline_head_sha: None,
                repo_state_baseline_worktree_fingerprint: None,
            }
        })
        .collect();
    context.evidence.source = Some(render_canonical_evidence_projection_source(
        &context.plan_rel,
        &context.plan_document,
        &context.plan_source,
        &context.source_spec_source,
        &context.steps,
        &context.evidence,
    ));
    context.evidence.source_origin = EvidenceSourceOrigin::AuthoritativeState;
    Ok(())
}

fn authoritative_completed_steps_for_evidence(
    context: &ExecutionContext,
    state_payload: &serde_json::Value,
) -> Result<BTreeMap<(u32, u32), Vec<String>>, JsonFailure> {
    let mut completed_steps = BTreeMap::<(u32, u32), Vec<String>>::new();
    if let Some(event_completed_steps) = state_payload
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
    {
        for step in parse_authoritative_completed_steps(event_completed_steps)? {
            completed_steps.entry(step).or_default();
        }
    }
    if let Some(records) = state_payload
        .get("current_task_closure_records")
        .and_then(serde_json::Value::as_object)
    {
        for (record_key, record) in records {
            let Some(task) = task_number_from_task_closure_record(record_key, record) else {
                continue;
            };
            let reviewed_surface_paths = record
                .get("effective_reviewed_surface_paths")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for step in context.steps.iter().filter(|step| step.task_number == task) {
                let entry = completed_steps
                    .entry((step.task_number, step.step_number))
                    .or_default();
                if !reviewed_surface_paths.is_empty() {
                    *entry = reviewed_surface_paths.clone();
                }
            }
        }
    }
    Ok(completed_steps)
}

fn overlay_event_completed_steps(
    context: &mut ExecutionContext,
    completed_steps: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFailure> {
    for (task, step) in parse_authoritative_completed_steps(completed_steps)? {
        let Some(plan_step) = context
            .steps
            .iter_mut()
            .find(|candidate| candidate.task_number == task && candidate.step_number == step)
        else {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative event_completed_steps points to missing Task {task} Step {step}."
                ),
            ));
        };
        plan_step.checked = true;
    }
    Ok(())
}

fn parse_authoritative_completed_steps(
    completed_steps: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<(u32, u32)>, JsonFailure> {
    completed_steps
        .values()
        .map(|entry| {
            let task = entry
                .get("task")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Authoritative event_completed_steps entry is missing a numeric task.",
                    )
                })?;
            let step = entry
                .get("step")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Authoritative event_completed_steps entry is missing a numeric step.",
                    )
                })?;
            Ok((task, step))
        })
        .collect()
}

fn overlay_task_closure_completed_steps(
    context: &mut ExecutionContext,
    state_payload: &serde_json::Value,
) -> Result<(), JsonFailure> {
    let Some(records) = state_payload
        .get("current_task_closure_records")
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(());
    };
    for (record_key, record) in records {
        let Some(task) = task_number_from_task_closure_record(record_key, record) else {
            continue;
        };
        for step in context
            .steps
            .iter_mut()
            .filter(|candidate| candidate.task_number == task)
        {
            step.checked = true;
        }
    }
    Ok(())
}

fn task_number_from_closure_record_key(record_key: &str) -> Option<u32> {
    record_key
        .strip_prefix("task-")
        .unwrap_or(record_key)
        .parse::<u32>()
        .ok()
}

fn task_number_from_task_closure_record(
    record_key: &str,
    record: &serde_json::Value,
) -> Option<u32> {
    record
        .get("task")
        .or_else(|| record.get("task_number"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| task_number_from_closure_record_key(record_key))
}

pub(crate) fn same_branch_worktrees(current_repo_root: &Path) -> Vec<PathBuf> {
    let repo = match discover_repository(current_repo_root) {
        Ok(repo) => repo,
        _ => return Vec::new(),
    };
    let listing_repo = repo.main_repo().unwrap_or(repo);
    let mut entries = listing_repo
        .workdir()
        .map(|work_dir| vec![canonicalize_repo_root_path(work_dir)])
        .unwrap_or_default();
    if let Ok(worktrees) = listing_repo.worktrees() {
        entries.extend(
            worktrees
                .into_iter()
                .filter_map(|worktree| worktree.base().ok())
                .map(|root| canonicalize_repo_root_path(&root)),
        );
    }
    entries.sort();
    entries.dedup();

    entries
}

pub(crate) fn refresh_execution_fingerprint(context: &mut ExecutionContext) {
    let normalized_plan_source = normalized_plan_source_for_semantic_identity(&context.plan_source);
    context.execution_fingerprint = compute_execution_fingerprint(
        &normalized_plan_source,
        context.evidence.source.as_deref(),
        context
            .authoritative_evidence_projection_fingerprint
            .as_deref(),
        &execution_state_fingerprint_source(context),
    );
}

pub fn current_file_proof(repo_root: &Path, path: &str) -> String {
    if path == NO_REPO_FILES_MARKER {
        return String::from("sha256:none");
    }
    let abs = repo_root.join(path);
    match fs::read(&abs) {
        Ok(contents) => format!("sha256:{}", sha256_hex(&contents)),
        Err(_) => String::from("sha256:missing"),
    }
}

pub fn current_file_proof_checked(repo_root: &Path, path: &str) -> Result<String, String> {
    if path == NO_REPO_FILES_MARKER {
        return Ok(String::from("sha256:none"));
    }
    let abs = repo_root.join(path);
    match fs::read(&abs) {
        Ok(contents) => Ok(format!("sha256:{}", sha256_hex(&contents))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::from("sha256:missing")),
        Err(error) => Err(error.to_string()),
    }
}

fn normalize_persisted_file_path(path: &str) -> Result<String, JsonFailure> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Execution evidence must include at least one repo-relative file entry.",
        ));
    }
    normalize_repo_relative_path(trimmed).map_err(JsonFailure::from)
}

pub(crate) fn normalize_plan_path(plan_path: &Path) -> Result<String, JsonFailure> {
    let raw = plan_path.to_string_lossy();
    let normalized = RepoPath::parse(&raw).map_err(|_| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Plan path must be a normalized repo-relative path.",
        )
    })?;
    let required_prefix = format!("{ACTIVE_PLAN_ROOT}/");
    if !normalized.as_str().starts_with(&required_prefix) {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Plan path must live under docs/featureforge/plans/.",
        ));
    }
    Ok(normalized.as_str().to_owned())
}

pub(crate) fn validate_source_spec(
    source: &str,
    expected_path: &str,
    expected_revision: u32,
    runtime: &ExecutionRuntime,
    matching_manifest: Option<&WorkflowManifest>,
    selection_policy: ApprovedArtifactSelectionPolicy,
) -> Result<(), JsonFailure> {
    let headers = parse_headers(source);
    if headers.get("Workflow State") != Some(&String::from("CEO Approved")) {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec is not CEO Approved.",
        ));
    }
    if headers
        .get("Spec Revision")
        .and_then(|value| value.parse::<u32>().ok())
        != Some(expected_revision)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec path or revision is stale.",
        ));
    }
    match headers.get("Last Reviewed By").map(String::as_str) {
        Some("plan-ceo-review") => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::PlanNotExecutionReady,
                "Approved plan source spec Last Reviewed By header is missing or malformed.",
            ));
        }
    }
    let approved_spec_candidates = approved_spec_candidate_paths(&runtime.repo_root);
    let manifest_selected_spec =
        matching_manifest.is_some_and(|manifest| manifest.expected_spec_path == expected_path);
    if approved_spec_candidates.len() > 1
        && !manifest_selected_spec
        && !matches!(
            selection_policy,
            ApprovedArtifactSelectionPolicy::AllowExactPlan
        )
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved spec candidates are ambiguous.",
        ));
    }
    if !approved_spec_candidates
        .iter()
        .any(|candidate| candidate == expected_path)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec path or revision is stale.",
        ));
    }
    Ok(())
}

pub(crate) fn validate_unique_approved_plan(
    expected_plan_path: &str,
    source_spec_path: &str,
    source_spec_revision: u32,
    runtime: &ExecutionRuntime,
    matching_manifest: Option<&WorkflowManifest>,
    selection_policy: ApprovedArtifactSelectionPolicy,
) -> Result<(), JsonFailure> {
    let approved_plan_candidates =
        approved_plan_candidate_paths(&runtime.repo_root, source_spec_path, source_spec_revision);
    let manifest_selected_plan =
        matching_manifest.is_some_and(|manifest| manifest.expected_plan_path == expected_plan_path);
    if approved_plan_candidates.len() > 1
        && !manifest_selected_plan
        && !matches!(
            selection_policy,
            ApprovedArtifactSelectionPolicy::AllowExactPlan
        )
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan candidates are ambiguous.",
        ));
    }
    if !approved_plan_candidates
        .iter()
        .any(|candidate| candidate == expected_plan_path)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan is not the unique current approved plan for its source spec.",
        ));
    }
    Ok(())
}

pub(crate) fn matching_workflow_manifest(runtime: &ExecutionRuntime) -> Option<WorkflowManifest> {
    let user_name = env::var("USER").unwrap_or_else(|_| String::from("user"));
    let manifest_path = runtime
        .state_dir
        .join("projects")
        .join(&runtime.repo_slug)
        .join(format!(
            "{user_name}-{}-workflow-state.json",
            runtime.safe_branch
        ));
    let ManifestLoadResult::Loaded(manifest) = load_manifest_read_only(&manifest_path) else {
        return None;
    };
    if stored_repo_root_matches_current(&manifest.repo_root, &runtime.repo_root)
        && manifest.branch == runtime.branch_name
    {
        Some(manifest)
    } else {
        None
    }
}

pub fn parse_command_verification_summary(summary: &str) -> Option<String> {
    let trimmed = normalize_whitespace(summary);
    let suffix = trimmed.strip_prefix('`')?;
    let (command, _) = suffix.split_once("` -> ")?;
    let command = normalize_whitespace(command);
    (!command.is_empty()).then_some(command)
}

pub fn normalize_source(source: &str, execution_mode: &str) -> Result<(), JsonFailure> {
    match source {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::InvalidExecutionMode,
                "Execution source must be one of the supported execution modes.",
            ));
        }
    }
    if source != execution_mode {
        return Err(JsonFailure::new(
            FailureClass::InvalidExecutionMode,
            "Execution source must exactly match the persisted execution mode for this plan revision.",
        ));
    }
    Ok(())
}

fn parse_headers(source: &str) -> BTreeMap<String, String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("**")?;
            let (key, value) = rest.split_once(":** ")?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn parse_headers_file(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .ok()
        .map(|source| parse_headers(&source))
        .unwrap_or_default()
}

fn approved_spec_candidate_paths(repo_root: &Path) -> Vec<String> {
    let mut candidates = markdown_files_under(&repo_root.join(ACTIVE_SPEC_ROOT))
        .into_iter()
        .filter_map(|path| {
            let headers = parse_headers_file(&path);
            if headers.get("Workflow State").map(String::as_str) != Some("CEO Approved") {
                return None;
            }
            let revision_valid = headers
                .get("Spec Revision")
                .and_then(|value| value.parse::<u32>().ok())
                .is_some();
            let reviewed_by_valid =
                headers.get("Last Reviewed By").map(String::as_str) == Some("plan-ceo-review");
            if !revision_valid || !reviewed_by_valid {
                return None;
            }
            path.strip_prefix(repo_root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
}

fn approved_plan_candidate_paths(
    repo_root: &Path,
    source_spec_path: &str,
    source_spec_revision: u32,
) -> Vec<String> {
    let mut candidates = markdown_files_under(&repo_root.join(ACTIVE_PLAN_ROOT))
        .into_iter()
        .filter_map(|path| {
            let headers = parse_headers_file(&path);
            if headers.get("Workflow State").map(String::as_str) != Some("Engineering Approved") {
                return None;
            }
            let execution_mode_valid = matches!(
                headers.get("Execution Mode").map(String::as_str),
                Some("none")
                    | Some("featureforge:executing-plans")
                    | Some("featureforge:subagent-driven-development")
            );
            let reviewed_by_valid =
                headers.get("Last Reviewed By").map(String::as_str) == Some("plan-eng-review");
            let source_path_matches =
                headers.get("Source Spec") == Some(&format!("`{source_spec_path}`"));
            let source_revision_matches = headers
                .get("Source Spec Revision")
                .and_then(|value| value.parse::<u32>().ok())
                == Some(source_spec_revision);
            let plan_revision_valid = headers
                .get("Plan Revision")
                .and_then(|value| value.parse::<u32>().ok())
                .is_some();
            if !execution_mode_valid
                || !reviewed_by_valid
                || !source_path_matches
                || !source_revision_matches
                || !plan_revision_valid
            {
                return None;
            }
            path.strip_prefix(repo_root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
}

pub(crate) fn parse_step_state(
    source: &str,
    plan_document: &PlanDocument,
) -> Result<Vec<PlanStepState>, JsonFailure> {
    let mut step_titles = BTreeMap::new();
    for task in &plan_document.tasks {
        for step in &task.steps {
            step_titles.insert((task.number, step.number), step.text.clone());
        }
    }

    let lines = source.lines().collect::<Vec<_>>();
    let mut current_task = None::<u32>;
    let mut steps = Vec::new();
    let mut line_index = 0;
    while line_index < lines.len() {
        let line = lines[line_index];
        if let Some(rest) = line.strip_prefix("## Task ") {
            current_task = rest
                .split(':')
                .next()
                .and_then(|value| value.parse::<u32>().ok());
            line_index += 1;
            continue;
        }

        if let Some((checked, step_number, title)) = parse_step_line(line) {
            let task_number = current_task.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::PlanNotExecutionReady,
                    "Plan step headings must live within a task section.",
                )
            })?;
            let canonical_title = step_titles
                .get(&(task_number, step_number))
                .cloned()
                .unwrap_or(title);
            let mut note_state = None;
            let mut note_summary = String::new();
            let mut cursor = line_index + 1;
            while cursor < lines.len() && lines[cursor].is_empty() {
                cursor += 1;
            }
            if cursor < lines.len()
                && let Some((parsed_state, parsed_summary)) = parse_note_line(lines[cursor])
            {
                if parsed_summary.is_empty() {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Execution note summaries may not be blank after whitespace normalization.",
                    ));
                }
                if parsed_summary.chars().count() > 120 {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Execution note summaries may not exceed 120 characters.",
                    ));
                }
                note_state = Some(parsed_state);
                note_summary = parsed_summary;
                let mut duplicate_cursor = cursor + 1;
                while duplicate_cursor < lines.len() && lines[duplicate_cursor].is_empty() {
                    duplicate_cursor += 1;
                }
                if duplicate_cursor < lines.len()
                    && parse_note_line(lines[duplicate_cursor]).is_some()
                {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Plan may have at most one execution note per step.",
                    ));
                }
            }

            steps.push(PlanStepState {
                task_number,
                step_number,
                title: canonical_title,
                checked,
                note_state,
                note_summary,
            });
        }
        line_index += 1;
    }

    Ok(steps)
}

pub(crate) fn parse_step_line(line: &str) -> Option<(bool, u32, String)> {
    let (checked, step) = parse_task_step_projection_line(line).ok()??;
    Some((checked, step.number, step.text))
}

fn parse_note_line(line: &str) -> Option<(NoteState, String)> {
    let rest = line.trim_start().strip_prefix("**Execution Note:** ")?;
    let (state, summary) = rest.split_once(" - ")?;
    let note_state = match state {
        "Active" => NoteState::Active,
        "Blocked" => NoteState::Blocked,
        "Interrupted" => NoteState::Interrupted,
        _ => return None,
    };
    Some((note_state, normalize_whitespace(summary)))
}

fn overlay_authoritative_open_step_state_from_state(
    context: &mut ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<(), JsonFailure> {
    let Some(record) = authoritative_state.current_open_step_state_checked()? else {
        return Ok(());
    };
    if !open_step_record_matches_or_can_share_current_workspace(context, &record) {
        return Ok(());
    }
    if context.plan_document.execution_mode == "none"
        && let Some(execution_mode) = record.execution_mode.as_deref()
    {
        match execution_mode {
            "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
                context.plan_document.execution_mode = execution_mode.to_owned();
            }
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Authoritative current_open_step_state for {} has invalid execution_mode `{execution_mode}`.",
                        context.plan_rel
                    ),
                ));
            }
        }
    }
    apply_authoritative_open_step_record_to_steps(
        &mut context.steps,
        &record,
        &context.plan_rel,
        context.plan_document.plan_revision,
    )
}

fn open_step_record_matches_or_can_share_current_workspace(
    context: &ExecutionContext,
    record: &OpenStepStateRecord,
) -> bool {
    let Some(record_repo_root) = record.repo_root.as_deref() else {
        return true;
    };
    let record_repo_root = canonicalize_repo_root_path(Path::new(record_repo_root));
    if record_repo_root == context.runtime.repo_root {
        return true;
    }
    if context
        .evidence
        .source
        .as_deref()
        .is_some_and(|source| source.contains("### Task "))
    {
        return false;
    }
    let Ok(discovered_runtime) = ExecutionRuntime::discover(&record_repo_root) else {
        return false;
    };
    if context.runtime.branch_name == "current"
        || discovered_runtime.branch_name == "current"
        || discovered_runtime.branch_name != context.runtime.branch_name
    {
        return false;
    }
    let record_runtime = ExecutionRuntime {
        state_dir: context.runtime.state_dir.clone(),
        ..discovered_runtime
    };
    let Ok(record_context) = load_execution_context_without_authority_overlay(
        &record_runtime,
        Path::new(&context.plan_rel),
    ) else {
        return false;
    };
    hash_contract_plan(&record_context.plan_source) == hash_contract_plan(&context.plan_source)
        && context_workspace_state_id(&record_context).ok()
            == context_workspace_state_id(context).ok()
}

fn context_workspace_state_id(context: &ExecutionContext) -> Result<String, JsonFailure> {
    Ok(semantic_workspace_snapshot(context)?.semantic_workspace_tree_id)
}

fn apply_authoritative_open_step_record_to_steps(
    steps: &mut [PlanStepState],
    record: &OpenStepStateRecord,
    plan_rel: &str,
    plan_revision: u32,
) -> Result<(), JsonFailure> {
    if normalize_whitespace(&record.source_plan_path) != plan_rel {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} points to source_plan_path `{}`.",
                record.source_plan_path
            ),
        ));
    }
    if record.source_plan_revision != plan_revision {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} points to source_plan_revision {}, expected {}.",
                record.source_plan_revision, plan_revision
            ),
        ));
    }
    let note_state = match record.note_state.as_str() {
        "Active" => NoteState::Active,
        "Blocked" => NoteState::Blocked,
        "Interrupted" => NoteState::Interrupted,
        _ => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative current_open_step_state for {plan_rel} has invalid note_state `{}`.",
                    record.note_state
                ),
            ));
        }
    };

    let Some(open_index) = steps
        .iter()
        .position(|step| step.task_number == record.task && step.step_number == record.step)
    else {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} points to missing Task {} Step {}.",
                record.task, record.step
            ),
        ));
    };

    let note_summary = normalize_whitespace(&record.note_summary);
    if note_summary.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} has a blank note_summary.",
            ),
        ));
    }
    if note_summary.chars().count() > 120 {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} has note_summary longer than 120 characters.",
            ),
        ));
    }

    for step in steps.iter_mut() {
        step.note_state = None;
        step.note_summary.clear();
    }
    // Projection lag in the plan markdown must not override authoritative
    // runtime open-step truth once the event log has committed it.
    steps[open_index].checked = false;
    steps[open_index].note_state = Some(note_state);
    steps[open_index].note_summary = note_summary;
    Ok(())
}

pub(crate) struct ExecutionEvidenceProjectionParseInput<'a> {
    pub(crate) runtime: &'a ExecutionRuntime,
    pub(crate) evidence_rel: &'a str,
    pub(crate) evidence_abs: &'a Path,
    pub(crate) plan_source: &'a str,
    pub(crate) expected_plan_path: &'a str,
    pub(crate) plan_document: &'a PlanDocument,
    pub(crate) steps: &'a [PlanStepState],
    pub(crate) expected_spec_path: &'a str,
    pub(crate) source_spec_source: &'a str,
    pub(crate) allow_legacy_unbound: bool,
    pub(crate) allow_tracked_projection: bool,
}

pub(crate) fn parse_execution_evidence_projection(
    input: ExecutionEvidenceProjectionParseInput<'_>,
) -> Result<ExecutionEvidence, JsonFailure> {
    let expected_plan_revision = input.plan_document.plan_revision;
    let expected_spec_revision = input.plan_document.source_spec_revision;
    if let Some(source) = read_state_dir_projection(input.runtime, input.evidence_rel)? {
        match state_dir_evidence_projection_currentness_for_source(
            input.runtime,
            input.evidence_rel,
            &source,
        )? {
            StateDirProjectionCurrentness::Current => {
                return parse_evidence_source(
                    source,
                    EvidenceSourceParseInput {
                        expected_plan_path: input.expected_plan_path,
                        expected_plan_revision,
                        expected_spec_path: input.expected_spec_path,
                        expected_spec_revision,
                        source_origin: EvidenceSourceOrigin::StateDirProjection,
                        tracked_progress_present: false,
                        tracked_source: None,
                    },
                );
            }
            StateDirProjectionCurrentness::Stale => {
                match classify_stale_state_dir_evidence_projection(
                    &input,
                    &source,
                    expected_plan_revision,
                    expected_spec_revision,
                ) {
                    Ok(StaleStateDirEvidenceProjection::CurrentEvidence(parsed)) => {
                        return Ok(parsed);
                    }
                    Ok(StaleStateDirEvidenceProjection::RuntimeGeneratedReadModel) => {}
                    Err(_error)
                        if authoritative_state_owns_evidence_history(
                            input.runtime,
                            input.expected_plan_path,
                        )? =>
                    {
                        // State-dir projections are read models. Once event history owns evidence,
                        // stale or tampered read-model files must not become mutation authority.
                    }
                    Err(error) => return Err(error),
                }
            }
            StateDirProjectionCurrentness::Unbound => {
                if let Some(parsed) = parse_self_bound_legacy_state_dir_evidence_projection(
                    &source, &input, false, None,
                )? {
                    return Ok(parsed);
                }
                if input.allow_legacy_unbound {
                    return parse_evidence_source(
                        source,
                        EvidenceSourceParseInput {
                            expected_plan_path: input.expected_plan_path,
                            expected_plan_revision,
                            expected_spec_path: input.expected_spec_path,
                            expected_spec_revision,
                            source_origin: EvidenceSourceOrigin::StateDirProjection,
                            tracked_progress_present: false,
                            tracked_source: None,
                        },
                    );
                }
                if evidence_source_has_progress(&source) {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "State-dir execution evidence projection is not bound to authoritative runtime state.",
                    ));
                }
            }
        }
    }
    if input.allow_tracked_projection
        && let Some(source) = read_tracked_evidence_source(input.evidence_abs)?
    {
        let tracked_source = Some(source.clone());
        return parse_evidence_source(
            source,
            EvidenceSourceParseInput {
                expected_plan_path: input.expected_plan_path,
                expected_plan_revision,
                expected_spec_path: input.expected_spec_path,
                expected_spec_revision,
                source_origin: EvidenceSourceOrigin::TrackedFile,
                tracked_progress_present: true,
                tracked_source,
            },
        );
    }
    // Tracked execution evidence is an optional export in normal operation.
    // The only tracked read retained here is the legacy pre-harness guard: a
    // legacy evidence file with progress is rejected or imported through the
    // existing legacy policy instead of being treated as a current projection.
    if !authoritative_state_owns_evidence_history(input.runtime, input.expected_plan_path)?
        && let Some(source) = read_tracked_legacy_evidence_source(input.evidence_abs)?
    {
        let tracked_source = Some(source.clone());
        return parse_evidence_source(
            source,
            EvidenceSourceParseInput {
                expected_plan_path: input.expected_plan_path,
                expected_plan_revision,
                expected_spec_path: input.expected_spec_path,
                expected_spec_revision,
                source_origin: EvidenceSourceOrigin::TrackedFile,
                tracked_progress_present: true,
                tracked_source,
            },
        );
    }

    Ok(ExecutionEvidence {
        format: EvidenceFormat::Empty,
        plan_path: input.expected_plan_path.to_owned(),
        plan_revision: expected_plan_revision,
        plan_fingerprint: None,
        source_spec_path: input.expected_spec_path.to_owned(),
        source_spec_revision: expected_spec_revision,
        source_spec_fingerprint: None,
        attempts: Vec::new(),
        source: None,
        source_origin: EvidenceSourceOrigin::Empty,
        tracked_progress_present: false,
        tracked_source: None,
    })
}

enum StaleStateDirEvidenceProjection {
    CurrentEvidence(ExecutionEvidence),
    RuntimeGeneratedReadModel,
}

fn classify_stale_state_dir_evidence_projection(
    input: &ExecutionEvidenceProjectionParseInput<'_>,
    source: &str,
    expected_plan_revision: u32,
    expected_spec_revision: u32,
) -> Result<StaleStateDirEvidenceProjection, JsonFailure> {
    if let Some(expected_fingerprint) =
        state_dir_evidence_projection_expected_fingerprint(input.runtime, input.evidence_rel)?
    {
        let parsed = parse_evidence_source(
            source.to_owned(),
            EvidenceSourceParseInput {
                expected_plan_path: input.expected_plan_path,
                expected_plan_revision,
                expected_spec_path: input.expected_spec_path,
                expected_spec_revision,
                source_origin: EvidenceSourceOrigin::StateDirProjection,
                tracked_progress_present: false,
                tracked_source: None,
            },
        )?;
        let canonical_source = render_canonical_evidence_projection_source(
            input.expected_plan_path,
            input.plan_document,
            input.plan_source,
            input.source_spec_source,
            input.steps,
            &parsed,
        );
        if projection_source_matches_fingerprint(&canonical_source, &expected_fingerprint) {
            return Ok(StaleStateDirEvidenceProjection::CurrentEvidence(parsed));
        }
    }
    if state_dir_projection_matches_recorded_output_fingerprint(
        input.runtime,
        input.evidence_rel,
        source,
    )? {
        // This is an older runtime-generated read model. Do not parse it as
        // authority, but allow the authoritative reducer to rebuild the current
        // read model for normal paths.
        return Ok(StaleStateDirEvidenceProjection::RuntimeGeneratedReadModel);
    }
    Err(JsonFailure::new(
        FailureClass::MalformedExecutionState,
        "State-dir execution evidence projection does not match authoritative runtime state.",
    ))
}

pub(crate) fn validate_state_dir_evidence_projection_before_materialization(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    let Some(source) = read_state_dir_projection(&context.runtime, &context.evidence_rel)? else {
        return Ok(());
    };
    if state_dir_evidence_projection_currentness_for_source(
        &context.runtime,
        &context.evidence_rel,
        &source,
    )? != StateDirProjectionCurrentness::Stale
    {
        return Ok(());
    }
    let input = ExecutionEvidenceProjectionParseInput {
        runtime: &context.runtime,
        evidence_rel: &context.evidence_rel,
        evidence_abs: &context.evidence_abs,
        plan_source: &context.plan_source,
        expected_plan_path: &context.plan_rel,
        plan_document: &context.plan_document,
        steps: &context.steps,
        expected_spec_path: &context.plan_document.source_spec_path,
        source_spec_source: &context.source_spec_source,
        allow_legacy_unbound: false,
        allow_tracked_projection: false,
    };
    classify_stale_state_dir_evidence_projection(
        &input,
        &source,
        context.plan_document.plan_revision,
        context.plan_document.source_spec_revision,
    )?;
    Ok(())
}

fn state_dir_evidence_projection_currentness_for_source(
    runtime: &ExecutionRuntime,
    evidence_rel: &str,
    source: &str,
) -> Result<StateDirProjectionCurrentness, JsonFailure> {
    let Some(expected_fingerprint) =
        state_dir_evidence_projection_expected_fingerprint(runtime, evidence_rel)?
    else {
        return Ok(StateDirProjectionCurrentness::Unbound);
    };
    if projection_source_matches_fingerprint(source, &expected_fingerprint) {
        Ok(StateDirProjectionCurrentness::Current)
    } else {
        Ok(StateDirProjectionCurrentness::Stale)
    }
}

fn state_dir_evidence_projection_expected_fingerprint(
    runtime: &ExecutionRuntime,
    _evidence_rel: &str,
) -> Result<Option<String>, JsonFailure> {
    match authoritative_state_optional_string_field_for_runtime(
        runtime,
        "execution_evidence_projection_fingerprint",
    )? {
        Some(Some(fingerprint)) => Ok(Some(fingerprint)),
        Some(None) | None => Ok(None),
    }
}

fn parse_self_bound_legacy_state_dir_evidence_projection(
    source: &str,
    input: &ExecutionEvidenceProjectionParseInput<'_>,
    tracked_progress_present: bool,
    tracked_source: Option<String>,
) -> Result<Option<ExecutionEvidence>, JsonFailure> {
    let headers = parse_headers(source);
    if !headers.contains_key("Plan Fingerprint") {
        return Ok(None);
    }
    let expected_plan_revision = input.plan_document.plan_revision.to_string();
    let expected_spec_revision = input.plan_document.source_spec_revision.to_string();
    let Some(header_plan_fingerprint) = headers.get("Plan Fingerprint").map(String::as_str) else {
        return Ok(None);
    };
    if !is_canonical_fingerprint(header_plan_fingerprint) {
        return Ok(None);
    }
    let expected_source_spec_fingerprint = sha256_hex(input.source_spec_source.as_bytes());
    if headers.get("Plan Path").map(String::as_str) != Some(input.expected_plan_path)
        || headers.get("Plan Revision").map(String::as_str) != Some(expected_plan_revision.as_str())
        || headers.get("Source Spec Path").map(String::as_str) != Some(input.expected_spec_path)
        || headers.get("Source Spec Revision").map(String::as_str)
            != Some(expected_spec_revision.as_str())
        || headers.get("Source Spec Fingerprint").map(String::as_str)
            != Some(expected_source_spec_fingerprint.as_str())
    {
        return Ok(None);
    }
    let parsed = parse_evidence_source(
        source.to_owned(),
        EvidenceSourceParseInput {
            expected_plan_path: input.expected_plan_path,
            expected_plan_revision: input.plan_document.plan_revision,
            expected_spec_path: input.expected_spec_path,
            expected_spec_revision: input.plan_document.source_spec_revision,
            source_origin: EvidenceSourceOrigin::StateDirProjection,
            tracked_progress_present,
            tracked_source,
        },
    )?;
    let canonical_source = render_canonical_evidence_projection_source_with_fingerprints(
        input.expected_plan_path,
        input.plan_document,
        header_plan_fingerprint,
        &expected_source_spec_fingerprint,
        input.steps,
        &parsed,
    );
    let canonical_fingerprint = sha256_hex(canonical_source.as_bytes());
    let source_matches_canonical =
        projection_source_matches_fingerprint(source, &canonical_fingerprint);
    if source_matches_canonical {
        Ok(Some(parsed))
    } else {
        Ok(None)
    }
}

fn evidence_source_has_progress(source: &str) -> bool {
    source.contains("### Task ")
}

fn authoritative_state_owns_evidence_history(
    runtime: &ExecutionRuntime,
    _expected_plan_path: &str,
) -> Result<bool, JsonFailure> {
    let Some(state_payload) = load_reduced_authoritative_state(runtime)? else {
        return Ok(false);
    };
    let evidence_attempts_present = state_payload
        .get("execution_evidence_attempts")
        .is_some_and(|attempts| !attempts.is_null());
    let event_completed_steps_present = state_payload
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|steps| !steps.is_empty());
    let task_closure_records_present = state_payload
        .get("current_task_closure_records")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|records| !records.is_empty());
    let current_open_step_present = state_payload
        .get("current_open_step_state")
        .is_some_and(|record| !record.is_null());
    Ok(evidence_attempts_present
        || event_completed_steps_present
        || task_closure_records_present
        || current_open_step_present)
}

fn read_tracked_legacy_evidence_source(evidence_abs: &Path) -> Result<Option<String>, JsonFailure> {
    let Some(source) = read_tracked_evidence_source(evidence_abs)? else {
        return Ok(None);
    };
    let is_legacy_evidence = !parse_headers(&source).contains_key("Plan Fingerprint");
    Ok(is_legacy_evidence.then_some(source))
}

fn read_tracked_evidence_source(evidence_abs: &Path) -> Result<Option<String>, JsonFailure> {
    let source = match fs::read_to_string(evidence_abs) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not read legacy execution evidence {}: {error}",
                    evidence_abs.display()
                ),
            ));
        }
    };
    Ok(evidence_source_has_progress(&source).then_some(source))
}

struct EvidenceSourceParseInput<'a> {
    expected_plan_path: &'a str,
    expected_plan_revision: u32,
    expected_spec_path: &'a str,
    expected_spec_revision: u32,
    source_origin: EvidenceSourceOrigin,
    tracked_progress_present: bool,
    tracked_source: Option<String>,
}

fn parse_evidence_source(
    source: String,
    input: EvidenceSourceParseInput<'_>,
) -> Result<ExecutionEvidence, JsonFailure> {
    let headers = parse_headers(&source);
    let format = if headers.contains_key("Plan Fingerprint") {
        EvidenceFormat::V2
    } else {
        EvidenceFormat::Legacy
    };
    let attempts = parse_evidence_attempts(&source, format)?;
    if attempts.is_empty() {
        return Ok(ExecutionEvidence {
            format: EvidenceFormat::Empty,
            plan_path: input.expected_plan_path.to_owned(),
            plan_revision: input.expected_plan_revision,
            plan_fingerprint: headers.get("Plan Fingerprint").cloned(),
            source_spec_path: headers
                .get("Source Spec Path")
                .cloned()
                .unwrap_or_else(|| input.expected_spec_path.to_owned()),
            source_spec_revision: headers
                .get("Source Spec Revision")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(input.expected_spec_revision),
            source_spec_fingerprint: headers.get("Source Spec Fingerprint").cloned(),
            attempts,
            source: Some(source),
            source_origin: input.source_origin,
            tracked_progress_present: input.tracked_progress_present,
            tracked_source: input.tracked_source,
        });
    }

    Ok(ExecutionEvidence {
        format,
        plan_path: headers
            .get("Plan Path")
            .cloned()
            .unwrap_or_else(|| input.expected_plan_path.to_owned()),
        plan_revision: headers
            .get("Plan Revision")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(input.expected_plan_revision),
        plan_fingerprint: headers.get("Plan Fingerprint").cloned(),
        source_spec_path: headers
            .get("Source Spec Path")
            .cloned()
            .unwrap_or_else(|| input.expected_spec_path.to_owned()),
        source_spec_revision: headers
            .get("Source Spec Revision")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(input.expected_spec_revision),
        source_spec_fingerprint: headers.get("Source Spec Fingerprint").cloned(),
        attempts,
        source: Some(source),
        source_origin: input.source_origin,
        tracked_progress_present: input.tracked_progress_present,
        tracked_source: input.tracked_source,
    })
}

fn parse_evidence_attempts(
    source: &str,
    format: EvidenceFormat,
) -> Result<Vec<EvidenceAttempt>, JsonFailure> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut attempts = Vec::new();
    let mut next_attempt_by_step = BTreeMap::<(u32, u32), u32>::new();
    let mut line_index = 0;
    let mut current_task = None::<u32>;
    let mut current_step = None::<u32>;

    while line_index < lines.len() {
        let line = lines[line_index];
        if let Some(rest) = line.strip_prefix("### Task ") {
            let (task, step) = rest.split_once(" Step ").ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence step heading is malformed.",
                )
            })?;
            current_task = task.parse::<u32>().ok();
            current_step = step.parse::<u32>().ok();
            line_index += 1;
            continue;
        }

        if let Some(rest) = line.strip_prefix("#### Attempt ") {
            let task_number = current_task.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempt is missing its step heading.",
                )
            })?;
            let step_number = current_step.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempt is missing its step heading.",
                )
            })?;
            let attempt_number = rest.parse::<u32>().map_err(|_| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempt number is malformed.",
                )
            })?;
            let expected_attempt = next_attempt_by_step
                .get(&(task_number, step_number))
                .copied()
                .unwrap_or(1);
            if attempt_number != expected_attempt {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempts must start at 1 and increase sequentially per step.",
                ));
            }
            next_attempt_by_step.insert((task_number, step_number), expected_attempt + 1);

            let mut status = String::new();
            let mut recorded_at = String::new();
            let mut execution_source = String::new();
            let mut claim = String::new();
            let mut files = Vec::new();
            let mut file_proofs = Vec::new();
            let mut verify_command = None;
            let mut verification_summary = String::new();
            let mut invalidation_reason = String::new();
            let mut packet_fingerprint = None;
            let mut head_sha = None;
            let mut base_sha = None;
            let mut source_contract_path = None;
            let mut source_contract_fingerprint = None;
            let mut source_evaluation_report_fingerprint = None;
            let mut evaluator_verdict = None;
            let mut failing_criterion_ids = Vec::new();
            let mut source_handoff_fingerprint = None;
            let mut repo_state_baseline_head_sha = None;
            let mut repo_state_baseline_worktree_fingerprint = None;

            line_index += 1;
            while line_index < lines.len() {
                let line = lines[line_index];
                if line.starts_with("#### Attempt ") || line.starts_with("### Task ") {
                    line_index = line_index.saturating_sub(1);
                    break;
                }

                if let Some(value) = line.strip_prefix("**Status:** ") {
                    status = normalize_whitespace(value);
                } else if let Some(value) = line.strip_prefix("**Recorded At:** ") {
                    recorded_at = value.to_owned();
                } else if let Some(value) = line.strip_prefix("**Execution Source:** ") {
                    execution_source = normalize_whitespace(value);
                } else if let Some(value) = line.strip_prefix("**Packet Fingerprint:** ") {
                    packet_fingerprint = Some(normalize_whitespace(value));
                } else if let Some(value) = line.strip_prefix("**Head SHA:** ") {
                    head_sha = Some(normalize_whitespace(value));
                } else if let Some(value) = line.strip_prefix("**Base SHA:** ") {
                    base_sha = Some(normalize_whitespace(value));
                } else if let Some(value) = line.strip_prefix("**Claim:** ") {
                    claim = normalize_whitespace(value);
                } else if let Some(value) = line.strip_prefix("**Source Contract Path:** ") {
                    source_contract_path = parse_optional_evidence_scalar(value);
                } else if let Some(value) = line.strip_prefix("**Source Contract Fingerprint:** ") {
                    source_contract_fingerprint = parse_optional_evidence_scalar(value);
                } else if let Some(value) =
                    line.strip_prefix("**Source Evaluation Report Fingerprint:** ")
                {
                    source_evaluation_report_fingerprint = parse_optional_evidence_scalar(value);
                } else if let Some(value) = line.strip_prefix("**Evaluator Verdict:** ") {
                    evaluator_verdict = parse_optional_evidence_scalar(value);
                } else if line == "**Failing Criterion IDs:**" {
                    line_index += 1;
                    while line_index < lines.len() {
                        let criterion_line = lines[line_index].trim();
                        if criterion_line.is_empty() {
                            line_index += 1;
                            continue;
                        }
                        if criterion_line == "[]" {
                            line_index += 1;
                            continue;
                        }
                        if criterion_line.starts_with("**")
                            || criterion_line.starts_with("### ")
                            || criterion_line.starts_with("#### ")
                        {
                            line_index = line_index.saturating_sub(1);
                            break;
                        }
                        if let Some(value) = criterion_line.strip_prefix("- ") {
                            if let Some(criterion_id) = parse_optional_evidence_scalar(value) {
                                failing_criterion_ids.push(criterion_id);
                            }
                            line_index += 1;
                            continue;
                        }
                        line_index = line_index.saturating_sub(1);
                        break;
                    }
                } else if let Some(value) = line.strip_prefix("**Source Handoff Fingerprint:** ") {
                    source_handoff_fingerprint = parse_optional_evidence_scalar(value);
                } else if let Some(value) = line.strip_prefix("**Repo State Baseline Head SHA:** ")
                {
                    repo_state_baseline_head_sha = parse_optional_evidence_scalar(value);
                } else if let Some(value) =
                    line.strip_prefix("**Repo State Baseline Worktree Fingerprint:** ")
                {
                    repo_state_baseline_worktree_fingerprint =
                        parse_optional_evidence_scalar(value);
                } else if line == "**Files Proven:**" {
                    line_index += 1;
                    while line_index < lines.len() {
                        let proof_line = lines[line_index];
                        if let Some(proof_entry) = proof_line.strip_prefix("- ") {
                            let (path, proof) = proof_entry.split_once(" | ").ok_or_else(|| {
                                JsonFailure::new(
                                    FailureClass::MalformedExecutionState,
                                    "Execution evidence Files Proven bullets must include a proof suffix.",
                                )
                            })?;
                            let path = normalize_persisted_file_path(path).map_err(|_| {
                                JsonFailure::new(
                                    FailureClass::MalformedExecutionState,
                                    "Execution evidence Files Proven bullets must use canonical repo-relative paths.",
                                )
                            })?;
                            files.push(path.clone());
                            file_proofs.push(FileProof {
                                path,
                                proof: proof.to_owned(),
                            });
                            line_index += 1;
                            continue;
                        }
                        line_index = line_index.saturating_sub(1);
                        break;
                    }
                } else if line == "**Files:**" {
                    line_index += 1;
                    while line_index < lines.len() {
                        let legacy_line = lines[line_index];
                        if let Some(path) = legacy_line.strip_prefix("- ") {
                            let path = normalize_persisted_file_path(path).map_err(|_| {
                                JsonFailure::new(
                                    FailureClass::MalformedExecutionState,
                                    "Execution evidence Files bullets must use canonical repo-relative paths.",
                                )
                            })?;
                            files.push(path.clone());
                            file_proofs.push(FileProof {
                                path,
                                proof: String::from("sha256:unknown"),
                            });
                            line_index += 1;
                            continue;
                        }
                        line_index = line_index.saturating_sub(1);
                        break;
                    }
                } else if let Some(value) = line.strip_prefix("**Verify Command:** ") {
                    verify_command = parse_optional_evidence_scalar(value).or_else(|| {
                        Some(normalize_whitespace(value)).filter(|candidate| !candidate.is_empty())
                    });
                } else if let Some(value) = line.strip_prefix("**Verification Summary:** ") {
                    verification_summary = normalize_whitespace(value);
                } else if line == "**Verification:**" {
                    line_index += 1;
                    if line_index < lines.len()
                        && let Some(value) = lines[line_index].strip_prefix("- ")
                    {
                        verification_summary = normalize_whitespace(value);
                    }
                } else if let Some(value) = line.strip_prefix("**Invalidation Reason:** ") {
                    invalidation_reason = normalize_whitespace(value);
                }

                line_index += 1;
            }

            if !matches!(status.as_str(), "Completed" | "Invalidated") {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence status must be Completed or Invalidated.",
                ));
            }
            if recorded_at.trim().is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence recorded-at timestamps may not be blank.",
                ));
            }
            if execution_source.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence source may not be blank.",
                ));
            }
            if !matches!(
                execution_source.as_str(),
                "featureforge:executing-plans" | "featureforge:subagent-driven-development"
            ) {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence source must be one of the supported execution modes.",
                ));
            }
            if claim.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence claims may not be blank after whitespace normalization.",
                ));
            }
            if files.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence must include at least one repo-relative file entry.",
                ));
            }
            if verification_summary.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence verification summaries may not be blank after whitespace normalization.",
                ));
            }
            if invalidation_reason.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence invalidation reasons may not be blank after whitespace normalization.",
                ));
            }
            if status == "Invalidated" && invalidation_reason == "N/A" {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Invalidated execution evidence must carry a real invalidation reason.",
                ));
            }

            let verify_command = verify_command
                .or_else(|| parse_command_verification_summary(&verification_summary));

            attempts.push(EvidenceAttempt {
                task_number,
                step_number,
                attempt_number,
                status,
                recorded_at,
                execution_source,
                claim,
                files,
                file_proofs,
                verify_command,
                verification_summary,
                invalidation_reason,
                packet_fingerprint,
                head_sha,
                base_sha,
                source_contract_path,
                source_contract_fingerprint,
                source_evaluation_report_fingerprint,
                evaluator_verdict,
                failing_criterion_ids,
                source_handoff_fingerprint,
                repo_state_baseline_head_sha,
                repo_state_baseline_worktree_fingerprint,
            });
        }

        line_index += 1;
    }

    if format == EvidenceFormat::V2 && attempts.is_empty() && source.contains("### Task ") {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Execution evidence v2 attempts could not be parsed.",
        ));
    }
    Ok(attempts)
}

fn parse_optional_evidence_scalar(value: &str) -> Option<String> {
    let normalized = normalize_whitespace(value);
    let trimmed = normalized.trim().trim_matches('`').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn execution_state_fingerprint_source(context: &ExecutionContext) -> String {
    let mut source = String::new();
    source.push_str("execution-mode=");
    source.push_str(&context.plan_document.execution_mode);
    source.push('\n');
    for step in &context.steps {
        source.push_str("step=");
        source.push_str(&step.task_number.to_string());
        source.push('.');
        source.push_str(&step.step_number.to_string());
        source.push_str(";checked=");
        source.push_str(if step.checked { "true" } else { "false" });
        source.push_str(";note=");
        if let Some(note_state) = step.note_state {
            source.push_str(note_state.as_str());
        } else {
            source.push_str("none");
        }
        source.push_str(";summary=");
        source.push_str(&step.note_summary);
        source.push('\n');
    }
    source
}

fn compute_execution_fingerprint(
    plan_source: &str,
    evidence_source: Option<&str>,
    authoritative_evidence_projection_fingerprint: Option<&str>,
    execution_state_source: &str,
) -> String {
    let mut payload = String::from("plan\n");
    payload.push_str(plan_source);
    payload.push_str("\n--evidence--\n");
    if let Some(fingerprint) = authoritative_evidence_projection_fingerprint {
        payload.push_str("authoritative-projection-fingerprint=");
        payload.push_str(fingerprint);
        payload.push('\n');
    } else if let Some(source) = evidence_source {
        if source.contains("### Task ") {
            payload.push_str(source);
        } else {
            payload.push_str("__EMPTY_EVIDENCE__\n");
        }
    } else {
        payload.push_str("__EMPTY_EVIDENCE__\n");
    }
    payload.push_str("\n--execution-state--\n");
    payload.push_str(execution_state_source);
    sha256_hex(payload.as_bytes())
}

fn parse_contract_render(source: &str) -> String {
    let known_runtime_steps = known_runtime_step_projection_lines(source);
    let lines = source.lines().collect::<Vec<_>>();
    let mut rendered = Vec::new();
    let mut current_task = None::<u32>;
    let mut current_task_files_seen = false;
    let mut in_fenced_block = false;
    let mut pending_note_after_step = false;
    let mut suppress_note_block = None::<RuntimeExecutionNoteProjectionBlock>;

    for line in lines {
        if let Some(note_block) = suppress_note_block {
            if note_block.continues(line) {
                continue;
            }
            suppress_note_block = None;
        }
        if pending_note_after_step {
            if line.trim().is_empty() {
                continue;
            }
            pending_note_after_step = false;
            if let Some(note_block) = RuntimeExecutionNoteProjectionBlock::start(line) {
                suppress_note_block = Some(note_block);
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("## Task ") {
            current_task = rest
                .split(':')
                .next()
                .and_then(|value| value.parse::<u32>().ok());
            current_task_files_seen = false;
            in_fenced_block = false;
            rendered.push(line.to_owned());
            continue;
        }
        if line.starts_with("## ") {
            current_task = None;
            current_task_files_seen = false;
            in_fenced_block = false;
        }
        let trimmed = line.trim();
        if trimmed == "**Files:**" {
            current_task_files_seen = true;
        }
        if trimmed.starts_with("```") {
            in_fenced_block = !in_fenced_block;
            rendered.push(line.to_owned());
            continue;
        }
        if line.starts_with("**Execution Mode:** ") {
            rendered.push(String::from("**Execution Mode:** none"));
            continue;
        }
        if let Some((_, step_number, title)) = parse_step_line(line) {
            let known_step = current_task
                .and_then(|task_number| known_runtime_steps.get(&(task_number, step_number)))
                .is_some_and(|known_title| known_title == &title);
            if in_fenced_block || !current_task_files_seen || !known_step {
                rendered.push(line.to_owned());
                continue;
            }
            rendered.push(format!("- [ ] **Step {step_number}: {title}**"));
            pending_note_after_step = true;
            continue;
        }
        rendered.push(line.to_owned());
    }

    format!("{}\n", rendered.join("\n"))
}
