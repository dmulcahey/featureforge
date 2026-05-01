use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::contracts::headers::parse_required_header as parse_plan_header;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::closure_graph::{
    AuthoritativeClosureGraph, ClosureGraphSignals, reason_code_indicates_stale_unreviewed,
};
use crate::execution::follow_up::{
    RepairFollowUpKind, RepairFollowUpRecord, RepairTargetScope, execution_step_repair_target_id,
    normalize_persisted_repair_follow_up_token,
};
#[cfg(test)]
use crate::execution::handoff::{
    WorkflowTransferRecordIdentity, current_workflow_transfer_record_exists,
};
use crate::execution::harness::{DownstreamFreshnessState, HarnessPhase};
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::observability::{
    REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED, REASON_CODE_STALE_PROVENANCE,
};
use crate::execution::projection_renderer::is_projection_export_path;
use crate::execution::reducer::{RuntimeGateSnapshot, RuntimeState};
use crate::execution::semantic_identity::{
    branch_definition_identity_for_context, semantic_paths_changed_between_raw_trees,
    semantic_tree_entries_for_raw_tree, semantic_workspace_snapshot,
};
use crate::execution::state::{
    ExecutionContext, GateResult, NO_REPO_FILES_MARKER, PlanExecutionStatus,
    branch_closure_record_matches_plan_exemption, resolve_branch_closure_reviewed_tree_sha,
    resolve_task_closure_reviewed_tree_sha, still_current_task_closure_records,
    validated_current_branch_closure_identity,
};
use crate::execution::transitions::load_authoritative_transition_state;
use crate::execution::transitions::{AuthoritativeTransitionState, CurrentTaskClosureRecord};
use crate::git::{discover_repository, sha256_hex};
#[cfg(test)]
use crate::workflow::pivot::{
    WorkflowPivotRecordIdentity, current_workflow_pivot_record_exists, pivot_decision_reason_codes,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct CurrentLateStageBranchBindings {
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub current_release_readiness_record_id: Option<String>,
    pub current_release_readiness_result: Option<String>,
    pub current_final_review_record_id: Option<String>,
    pub current_final_review_branch_closure_id: Option<String>,
    pub current_final_review_result: Option<String>,
    pub current_qa_record_id: Option<String>,
    pub current_qa_branch_closure_id: Option<String>,
    pub current_qa_result: Option<String>,
}

const PUBLIC_TASK_BOUNDARY_REASON_CODES: &[&str] = &[
    "prior_task_current_closure_missing",
    "prior_task_current_closure_stale",
    "prior_task_current_closure_invalid",
    "prior_task_current_closure_reviewed_state_malformed",
    "task_cycle_break_active",
    "current_task_closure_overlay_restore_required",
    "prior_task_review_not_green",
    "task_closure_baseline_repair_candidate",
    crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
];

const TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES: &[&str] = &[
    "prior_task_review_dispatch_missing",
    "prior_task_review_dispatch_stale",
    "prior_task_verification_missing",
    "prior_task_verification_missing_legacy",
    "task_review_not_independent",
    "task_review_artifact_malformed",
    "task_verification_summary_malformed",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PublicTaskBoundaryDecision {
    pub task: Option<u32>,
    pub state: PublicTaskBoundaryState,
    pub public_reason_codes: Vec<String>,
    pub diagnostic_reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PublicTaskBoundaryState {
    Clean,
    CurrentClosureMissing,
    CurrentClosureStale,
    NegativeReviewCurrent,
    CycleBreakActive,
    OverlayRestoreRequired,
    TaskClosureRecordingReady,
    ExecutionReentryRequired,
}

pub(crate) fn public_task_boundary_reason_code(reason_code: &str) -> bool {
    PUBLIC_TASK_BOUNDARY_REASON_CODES.contains(&reason_code)
}

pub(crate) fn task_boundary_projection_diagnostic_reason_code(reason_code: &str) -> bool {
    TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES.contains(&reason_code)
}

pub(crate) fn public_task_boundary_decision(
    status: &PlanExecutionStatus,
) -> PublicTaskBoundaryDecision {
    let task_scope = status.blocking_step.is_none()
        && (status.blocking_task.is_some()
            || status.phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            || status.reason_codes.iter().any(|reason_code| {
                public_task_boundary_reason_code(reason_code)
                    || task_boundary_projection_diagnostic_reason_code(reason_code)
            }));
    if !task_scope {
        return PublicTaskBoundaryDecision {
            task: None,
            state: PublicTaskBoundaryState::Clean,
            public_reason_codes: Vec::new(),
            diagnostic_reason_codes: Vec::new(),
        };
    }

    let public_reason_codes =
        reason_codes_matching(&status.reason_codes, public_task_boundary_reason_code);
    let diagnostic_reason_codes = reason_codes_matching(
        &status.reason_codes,
        task_boundary_projection_diagnostic_reason_code,
    );
    let state = if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
    {
        PublicTaskBoundaryState::CycleBreakActive
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "current_task_closure_overlay_restore_required")
    {
        PublicTaskBoundaryState::OverlayRestoreRequired
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_review_not_green")
    {
        PublicTaskBoundaryState::NegativeReviewCurrent
    } else if public_reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
        )
    }) {
        PublicTaskBoundaryState::ExecutionReentryRequired
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_current_closure_stale")
    {
        PublicTaskBoundaryState::CurrentClosureStale
    } else if public_reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_current_closure_missing")
    {
        PublicTaskBoundaryState::CurrentClosureMissing
    } else if status.phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        || public_reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "task_closure_baseline_repair_candidate"
                    | crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            )
        })
    {
        PublicTaskBoundaryState::TaskClosureRecordingReady
    } else {
        PublicTaskBoundaryState::Clean
    };

    PublicTaskBoundaryDecision {
        task: status.blocking_task,
        state,
        public_reason_codes,
        diagnostic_reason_codes,
    }
}

fn reason_codes_matching(reason_codes: &[String], predicate: fn(&str) -> bool) -> Vec<String> {
    let mut matched = Vec::new();
    for reason_code in reason_codes {
        if predicate(reason_code) && !matched.iter().any(|existing| existing == reason_code) {
            matched.push(reason_code.clone());
        }
    }
    matched
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewStateRepairReroute {
    None,
    ExecutionReentry,
    RecordBranchClosure,
}

pub(crate) fn normalize_summary_content(value: &str) -> String {
    let normalized_newlines = value.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed_lines = normalized_newlines
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>();
    let start = trimmed_lines
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(trimmed_lines.len());
    let end = trimmed_lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map(|index| index + 1)
        .unwrap_or(start);
    trimmed_lines[start..end].join("\n")
}

pub(crate) fn summary_hash(value: &str) -> String {
    sha256_hex(normalize_summary_content(value).as_bytes())
}

pub(crate) fn reviewer_source_is_valid(value: &str) -> bool {
    matches!(
        value,
        "fresh-context-subagent" | "cross-model" | "human-independent-reviewer"
    )
}

pub(crate) fn task_closure_contributes_to_branch_surface(
    context: &ExecutionContext,
    current_record: &CurrentTaskClosureRecord,
) -> bool {
    current_record
        .effective_reviewed_surface_paths
        .iter()
        .filter(|surface_path| surface_path.as_str() != NO_REPO_FILES_MARKER)
        .any(|surface_path| !is_runtime_owned_execution_control_plane_path(context, surface_path))
}

pub(crate) fn branch_source_task_closure_ids(
    context: &ExecutionContext,
    current_records: &[CurrentTaskClosureRecord],
    late_stage_surface: Option<&[String]>,
) -> Vec<String> {
    let mut source_task_closure_ids = current_records
        .iter()
        .filter(|record| {
            if !task_closure_contributes_to_branch_surface(context, record) {
                return false;
            }
            let Some(late_stage_surface) = late_stage_surface else {
                return true;
            };
            record
                .effective_reviewed_surface_paths
                .iter()
                .filter(|surface_path| {
                    surface_path.as_str() != NO_REPO_FILES_MARKER
                        && !is_runtime_owned_execution_control_plane_path(context, surface_path)
                })
                .any(|surface_path| {
                    !path_matches_late_stage_surface(surface_path, late_stage_surface)
                })
        })
        .map(|record| record.closure_record_id.clone())
        .collect::<Vec<_>>();
    source_task_closure_ids.sort();
    source_task_closure_ids.dedup();
    source_task_closure_ids
}

fn normalized_runtime_control_plane_path(path: &str) -> Option<String> {
    let mut normalized = path.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_owned();
    }
    (!normalized.is_empty()).then_some(normalized)
}

fn is_runtime_owned_runtime_output_path(path: &str) -> bool {
    path.starts_with("docs/archive/featureforge/execution-evidence/")
        || is_projection_export_path(path)
        || path.starts_with("docs/featureforge/reviews/")
}

pub(crate) fn is_runtime_owned_execution_control_plane_path(
    _context: &ExecutionContext,
    path: &str,
) -> bool {
    let Some(normalized_path) = normalized_runtime_control_plane_path(path) else {
        return false;
    };
    normalized_path.starts_with("docs/featureforge/execution-evidence/")
        || is_runtime_owned_runtime_output_path(&normalized_path)
}

pub(crate) fn normalized_late_stage_surface(plan_source: &str) -> Result<Vec<String>, JsonFailure> {
    let Some(raw_value) = parse_plan_header(plan_source, "Late-Stage Surface") else {
        return Ok(Vec::new());
    };
    raw_value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(normalize_late_stage_surface_entry)
        .collect()
}

pub(crate) fn render_late_stage_surface_only_branch_surface(surface_entries: &[String]) -> String {
    let mut normalized = surface_entries.to_vec();
    normalized.sort();
    normalized.dedup();
    format!("late_stage_surface_only:{}", normalized.join(","))
}

pub(crate) fn parse_late_stage_surface_only_branch_surface(value: &str) -> Option<Vec<String>> {
    let raw_entries = value.strip_prefix("late_stage_surface_only:")?;
    let mut normalized = raw_entries
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(normalize_late_stage_surface_entry)
        .collect::<Result<Vec<_>, JsonFailure>>()
        .ok()?;
    normalized.sort();
    normalized.dedup();
    Some(normalized)
}

pub(crate) fn normalize_late_stage_surface_entry(entry: &str) -> Result<String, JsonFailure> {
    let mut normalized = entry.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_owned();
    }
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
            && normalized
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
            && normalized
                .as_bytes()
                .get(2)
                .is_some_and(|slash| *slash == b'/')
        || normalized.split('/').any(|segment| segment == "..")
        || normalized.contains('*')
        || normalized.contains('?')
        || normalized.contains('[')
        || normalized.contains(']')
        || normalized.contains('{')
        || normalized.contains('}')
    {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("late_stage_surface_invalid: unsupported Late-Stage Surface entry `{entry}`."),
        ));
    }
    Ok(normalized)
}

pub(crate) fn path_matches_late_stage_surface(path: &str, surface_entries: &[String]) -> bool {
    surface_entries.iter().any(|entry| {
        if let Some(prefix) = entry.strip_suffix('/') {
            path == prefix || path.starts_with(&format!("{prefix}/"))
        } else {
            path == entry
        }
    })
}

pub(crate) fn current_repo_tracked_tree_sha(repo_root: &Path) -> Result<String, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not discover the repository for reviewed-state identity: {error}"),
        )
    })?;
    if !repo_has_tracked_changes_for_reviewed_state(&repo)? {
        return head_tree_sha_for_reviewed_state(&repo);
    }
    // This helper intentionally uses `git add -u` plus `git write-tree` against a copied index
    // only when tracked worktree changes exist. Clean worktrees use the gix HEAD-tree fast path
    // above because it is semantically identical without crossing a subprocess boundary.
    let index_path = repo
        .open_index()
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!("Could not open the repository index for reviewed-state identity: {error}"),
            )
        })?
        .path()
        .to_path_buf();
    let temp_index_path = reserve_unique_reviewed_state_index_path()?;
    let _temp_index_guard = ReservedTempPath::new(temp_index_path.clone());
    fs::copy(&index_path, &temp_index_path).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not copy git index for reviewed-state identity from {}: {error}",
                index_path.display()
            ),
        )
    })?;
    let add_status = Command::new("git")
        .current_dir(repo_root)
        .env("GIT_INDEX_FILE", &temp_index_path)
        .args(["add", "-u", "."])
        .status()
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not stage tracked worktree content for reviewed-state identity: {error}"
                ),
            )
        })?;
    if !add_status.success() {
        return Err(JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            "Could not stage tracked worktree content for reviewed-state identity.",
        ));
    }
    git_write_tree(repo_root, Some(&temp_index_path))
}

fn repo_has_tracked_changes_for_reviewed_state(
    repo: &gix::Repository,
) -> Result<bool, JsonFailure> {
    let mut status_iter = repo
        .status(gix::progress::Discard)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not prepare tracked worktree status for reviewed-state identity: {error}"
                ),
            )
        })?
        .untracked_files(gix::status::UntrackedFiles::None)
        .index_worktree_rewrites(None)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not inspect tracked worktree status for reviewed-state identity: {error}"
                ),
            )
        })?;
    for item in &mut status_iter {
        let item = item.map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not inspect tracked worktree status for reviewed-state identity: {error}"
                ),
            )
        })?;
        match item {
            gix::status::Item::TreeIndex(_) => return Ok(true),
            gix::status::Item::IndexWorktree(change) if change.summary().is_some() => {
                return Ok(true);
            }
            gix::status::Item::IndexWorktree(_) => {}
        }
    }
    Ok(false)
}

fn head_tree_sha_for_reviewed_state(repo: &gix::Repository) -> Result<String, JsonFailure> {
    let head = repo.head_id().map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not resolve HEAD for reviewed-state identity: {error}"),
        )
    })?;
    let commit = repo.find_commit(head.detach()).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not load HEAD commit for reviewed-state identity: {error}"),
        )
    })?;
    let tree_id = commit.tree_id().map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not resolve HEAD tree for reviewed-state identity: {error}"),
        )
    })?;
    Ok(tree_id.to_string())
}

fn git_write_tree(repo_root: &Path, index_override: Option<&Path>) -> Result<String, JsonFailure> {
    let mut command = Command::new("git");
    command.current_dir(repo_root).args(["write-tree"]);
    if let Some(index_override) = index_override {
        command.env("GIT_INDEX_FILE", index_override);
    }
    let write_tree_output = command.output().map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not compute the reviewed-state tree identity: {error}"),
        )
    })?;
    if !write_tree_output.status.success() {
        let stderr = String::from_utf8_lossy(&write_tree_output.stderr);
        return Err(JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not compute the reviewed-state tree identity: {}",
                stderr.trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&write_tree_output.stdout)
        .trim()
        .to_owned())
}

struct ReservedTempPath {
    path: PathBuf,
}

impl ReservedTempPath {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for ReservedTempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn reserve_unique_reviewed_state_index_path() -> Result<PathBuf, JsonFailure> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    for _ in 0..32 {
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let candidate = std::env::temp_dir().join(format!(
            "featureforge-reviewed-state-{}-{timestamp}-{counter}.index",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                drop(file);
                return Ok(candidate);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(JsonFailure::new(
                    FailureClass::BranchDetectionFailed,
                    format!(
                        "Could not allocate temp git index for reviewed-state identity: {error}"
                    ),
                ));
            }
        }
    }
    Err(JsonFailure::new(
        FailureClass::BranchDetectionFailed,
        "Could not allocate a unique temp git index for reviewed-state identity after repeated attempts.",
    ))
}

pub(crate) fn current_branch_closure_reviewed_tree_sha(
    context: &ExecutionContext,
) -> Option<String> {
    let identity = branch_closure_identity_for_rerecording(context)?;
    context
        .cached_reviewed_tree_sha(
            &identity.reviewed_state_id,
            |repo_root, reviewed_state_id| {
                resolve_branch_closure_reviewed_tree_sha(
                    repo_root,
                    &identity.branch_closure_id,
                    reviewed_state_id,
                )
            },
        )
        .ok()
}

pub(crate) fn tracked_paths_changed_since_record_branch_closure_baseline(
    context: &ExecutionContext,
) -> Result<Vec<String>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    tracked_paths_changed_since_record_branch_closure_baseline_with_authority(
        context,
        authoritative_state.as_ref(),
    )
}

fn tracked_paths_changed_since_record_branch_closure_baseline_with_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Vec<String>, JsonFailure> {
    if current_branch_closure_has_semantic_reviewed_state_id(authoritative_state, context)
        && !current_branch_closure_has_tracked_drift(context, authoritative_state)?
    {
        return Ok(Vec::new());
    }
    if let Some(branch_tree_sha) = current_branch_closure_reviewed_tree_sha(context) {
        let current_tree_sha = context.current_tracked_tree_sha()?;
        let semantic_changed_paths =
            semantic_paths_changed_between_raw_trees(context, &branch_tree_sha, &current_tree_sha)?;
        if semantic_changed_paths.is_empty() {
            return Ok(semantic_branch_rerecording_drift_paths(
                context,
                authoritative_state,
            ));
        }
        return Ok(semantic_changed_paths);
    }
    let current_records =
        current_branch_task_closure_records_with_authority(context, authoritative_state)?;
    if !current_records.is_empty() {
        return tracked_paths_changed_since_task_closure_records_baseline(
            context,
            &current_records,
        );
    }
    Ok(Vec::new())
}

fn current_branch_closure_allows_repaired_empty_lineage_late_stage_rerecord(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    let Some(authoritative_state) = authoritative_state else {
        return false;
    };
    let Some(identity) = branch_closure_identity_for_rerecording(context) else {
        return false;
    };
    let Some(follow_up) = authoritative_state.review_state_repair_follow_up_record() else {
        return false;
    };
    if follow_up.kind != RepairFollowUpKind::RecordBranchClosure
        || follow_up.target_scope != RepairTargetScope::BranchClosure
        || follow_up.target_record_id.as_deref() != Some(identity.branch_closure_id.as_str())
    {
        return false;
    }
    let semantic_workspace_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    if !repair_follow_up_semantic_workspace_matches(
        &follow_up,
        semantic_workspace_state_id.as_deref(),
    ) || !repair_follow_up_exact_target_still_bound(
        &follow_up,
        authoritative_state,
        Some(identity.branch_closure_id.as_str()),
    ) {
        return false;
    }
    authoritative_state
        .branch_closure_record(&identity.branch_closure_id)
        .is_some_and(|record| {
            record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption"
                && record.source_task_closure_ids.is_empty()
                && branch_closure_record_matches_plan_exemption(context, &record)
        })
}

fn current_branch_closure_has_semantic_reviewed_state_id(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    context: &ExecutionContext,
) -> bool {
    authoritative_state
        .and_then(|state| {
            branch_closure_identity_for_rerecording(context)
                .and_then(|identity| state.branch_closure_record(&identity.branch_closure_id))
        })
        .is_some_and(|record| {
            record
                .semantic_reviewed_state_id
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
        })
}

fn semantic_branch_rerecording_drift_paths(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Vec<String> {
    let Some(state) = authoritative_state else {
        return Vec::new();
    };
    let Some(identity) = branch_closure_identity_for_rerecording(context) else {
        return Vec::new();
    };
    let Some(record) = state.branch_closure_record(&identity.branch_closure_id) else {
        return Vec::new();
    };
    let current_branch_contract_identity = branch_definition_identity_for_context(context);
    let canonical_branch_contract_identity = record.contract_identity.starts_with("branch_def:");
    if canonical_branch_contract_identity
        && record.contract_identity != current_branch_contract_identity
    {
        let current_base_branch = context.current_release_base_branch().unwrap_or_default();
        if record.base_branch != current_base_branch {
            return Vec::new();
        }
        return vec![context.plan_rel.clone()];
    }
    Vec::new()
}

fn tracked_worktree_paths_changed_excluding_runtime_control_plane(
    context: &ExecutionContext,
) -> Result<Vec<String>, JsonFailure> {
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not discover the repository while inspecting tracked worktree changes: {error}"
            ),
        )
    })?;
    let mut status_iter = repo
        .status(gix::progress::Discard)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not prepare tracked worktree status while inspecting branch-closure drift: {error}"
                ),
            )
        })?
        .untracked_files(gix::status::UntrackedFiles::None)
        .index_worktree_rewrites(None)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not inspect tracked worktree changes while evaluating branch-closure drift: {error}"
                ),
            )
        })?;
    let mut changed_paths = Vec::new();
    for item in &mut status_iter {
        let item = item.map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not inspect tracked worktree changes while evaluating branch-closure drift: {error}"
                ),
            )
        })?;
        let path = item.location().to_string();
        if is_runtime_owned_execution_control_plane_path(context, &path) {
            continue;
        }
        match item {
            gix::status::Item::TreeIndex(_) => changed_paths.push(path),
            gix::status::Item::IndexWorktree(change) if change.summary().is_some() => {
                changed_paths.push(path)
            }
            gix::status::Item::IndexWorktree(_) => {}
        }
    }
    changed_paths.sort();
    changed_paths.dedup();
    Ok(changed_paths)
}

pub(crate) fn worktree_drift_escapes_late_stage_surface(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let changed_paths = tracked_worktree_paths_changed_excluding_runtime_control_plane(context)?;
    if changed_paths.is_empty() {
        return Ok(false);
    }
    let late_stage_surface = normalized_late_stage_surface(&context.plan_source)?;
    if late_stage_surface.is_empty() {
        return Ok(true);
    }
    Ok(!changed_paths
        .iter()
        .all(|path| path_matches_late_stage_surface(path, &late_stage_surface)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BranchRerecordingUnsupportedReason {
    MissingTaskClosureBaseline,
    LateStageSurfaceNotDeclared,
    DriftEscapesLateStageSurface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchRerecordingAssessment {
    pub changed_paths: Vec<String>,
    pub late_stage_surface: Vec<String>,
    pub drift_confined_to_late_stage_surface: bool,
    pub supported: bool,
    pub unsupported_reason: Option<BranchRerecordingUnsupportedReason>,
}

pub(crate) fn late_stage_missing_task_closure_baseline_bridge_supported(
    assessment: &BranchRerecordingAssessment,
) -> bool {
    assessment.supported
        || (assessment.unsupported_reason
            == Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline)
            && assessment.changed_paths.is_empty())
}

pub(crate) fn branch_closure_rerecording_assessment(
    context: &ExecutionContext,
) -> Result<BranchRerecordingAssessment, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    branch_closure_rerecording_assessment_with_authority(context, authoritative_state.as_ref())
}

pub(crate) fn branch_closure_rerecording_assessment_with_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<BranchRerecordingAssessment, JsonFailure> {
    let current_records =
        current_branch_task_closure_records_with_authority(context, authoritative_state)?;
    let mut changed_paths =
        tracked_paths_changed_since_record_branch_closure_baseline_with_authority(
            context,
            authoritative_state,
        )?;
    let semantic_contract_drift_paths = if changed_paths.is_empty() {
        semantic_branch_rerecording_drift_paths(context, authoritative_state)
    } else {
        Vec::new()
    };
    let semantic_contract_drift = !semantic_contract_drift_paths.is_empty();
    if semantic_contract_drift {
        changed_paths = semantic_contract_drift_paths;
    }
    if changed_paths.is_empty() {
        return Ok(BranchRerecordingAssessment {
            changed_paths,
            late_stage_surface: Vec::new(),
            drift_confined_to_late_stage_surface: false,
            supported: true,
            unsupported_reason: None,
        });
    }
    let authoritative_task_closure_baseline_exists =
        authoritative_state.is_some_and(|state| !state.current_task_closure_results().is_empty());
    if current_records.is_empty()
        && !authoritative_task_closure_baseline_exists
        && !current_branch_closure_allows_repaired_empty_lineage_late_stage_rerecord(
            context,
            authoritative_state,
        )
    {
        return Ok(BranchRerecordingAssessment {
            changed_paths,
            late_stage_surface: Vec::new(),
            drift_confined_to_late_stage_surface: false,
            supported: false,
            unsupported_reason: Some(
                BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline,
            ),
        });
    }
    let late_stage_surface = normalized_late_stage_surface(&context.plan_source)?;
    if late_stage_surface.is_empty() {
        return Ok(BranchRerecordingAssessment {
            changed_paths,
            late_stage_surface,
            drift_confined_to_late_stage_surface: false,
            supported: false,
            unsupported_reason: Some(if semantic_contract_drift {
                BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface
            } else {
                BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared
            }),
        });
    }
    let drift_confined_to_late_stage_surface = changed_paths
        .iter()
        .all(|path| path_matches_late_stage_surface(path, &late_stage_surface));
    let assessment = BranchRerecordingAssessment {
        changed_paths,
        late_stage_surface,
        drift_confined_to_late_stage_surface,
        supported: drift_confined_to_late_stage_surface,
        unsupported_reason: if drift_confined_to_late_stage_surface {
            None
        } else {
            Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface)
        },
    };
    Ok(assessment)
}

fn current_branch_task_closure_records_with_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    if authoritative_state.is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires authoritative current task-closure state.",
        ));
    }
    Ok(still_current_task_closure_records(context)?
        .into_iter()
        .filter(|record| task_closure_contributes_to_branch_surface(context, record))
        .collect())
}

fn tracked_paths_changed_since_task_closure_records_baseline(
    context: &ExecutionContext,
    current_records: &[CurrentTaskClosureRecord],
) -> Result<Vec<String>, JsonFailure> {
    if current_records.is_empty() {
        return Ok(Vec::new());
    }
    let current_tree_sha = context.current_tracked_tree_sha()?;
    let mut tree_entries_cache = BTreeMap::new();
    let current_entries = cached_semantic_tree_entries_for_tree_sha(
        context,
        &mut tree_entries_cache,
        &current_tree_sha,
    )?;
    let mut closure_tree_entries = Vec::with_capacity(current_records.len());
    let mut all_paths = current_entries.keys().cloned().collect::<BTreeSet<_>>();
    for current_record in current_records {
        if current_record.contract_identity.trim().is_empty() {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "record-branch-closure requires task-closure contract identity for the current task-closure baseline.",
            ));
        }
        let tree_sha = current_task_closure_reviewed_tree_sha(context, current_record)?;
        let tree_entries =
            cached_semantic_tree_entries_for_tree_sha(context, &mut tree_entries_cache, &tree_sha)?;
        all_paths.extend(tree_entries.keys().cloned());
        closure_tree_entries.push((current_record, tree_entries));
    }

    let mut changed_paths = Vec::new();
    for path in all_paths {
        let current_entry = current_entries.get(&path);
        let covering_entries = closure_tree_entries
            .iter()
            .filter(|(record, _)| task_closure_record_covers_path(context, record, &path))
            .map(|(_, entries)| entries.get(&path))
            .collect::<Vec<_>>();
        if !covering_entries.is_empty() {
            let expected_entry =
                require_unambiguous_task_closure_path_entry(&path, &covering_entries)?;
            if current_entry.cloned() != expected_entry {
                changed_paths.push(path);
            }
            continue;
        }

        let baseline_entries = closure_tree_entries
            .iter()
            .map(|(_, entries)| entries.get(&path))
            .collect::<Vec<_>>();
        let Some(expected_entry) = consensus_tree_entry(&baseline_entries) else {
            changed_paths.push(path);
            continue;
        };
        if current_entry != expected_entry {
            changed_paths.push(path);
        }
    }

    Ok(changed_paths
        .into_iter()
        .filter(|path| !is_runtime_owned_execution_control_plane_path(context, path))
        .collect())
}

fn cached_semantic_tree_entries_for_tree_sha(
    context: &ExecutionContext,
    cache: &mut BTreeMap<String, BTreeMap<String, String>>,
    tree_sha: &str,
) -> Result<BTreeMap<String, String>, JsonFailure> {
    if let Some(entries) = cache.get(tree_sha) {
        return Ok(entries.clone());
    }
    let entries = semantic_tree_entries_for_raw_tree(context, tree_sha)?;
    cache.insert(tree_sha.to_owned(), entries.clone());
    Ok(entries)
}

fn current_task_closure_reviewed_tree_sha(
    context: &ExecutionContext,
    current_record: &CurrentTaskClosureRecord,
) -> Result<String, JsonFailure> {
    context.cached_reviewed_tree_sha(
        &current_record.reviewed_state_id,
        |repo_root, reviewed_state_id| {
            resolve_task_closure_reviewed_tree_sha(
                repo_root,
                current_record.task,
                reviewed_state_id,
            )
        },
    )
}

fn task_closure_record_covers_path(
    context: &ExecutionContext,
    current_record: &CurrentTaskClosureRecord,
    path: &str,
) -> bool {
    if is_runtime_owned_execution_control_plane_path(context, path) {
        return false;
    }
    current_record
        .effective_reviewed_surface_paths
        .iter()
        .filter(|surface_path| {
            !is_runtime_owned_execution_control_plane_path(context, surface_path)
        })
        .any(|surface_path| {
            path_matches_late_stage_surface(path, std::slice::from_ref(surface_path))
        })
}

fn require_unambiguous_task_closure_path_entry(
    path: &str,
    entries: &[Option<&String>],
) -> Result<Option<String>, JsonFailure> {
    let Some(expected_entry) = consensus_tree_entry(entries) else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "record-branch-closure requires one unambiguous current task-closure reviewed-state baseline for path `{path}`."
            ),
        ));
    };
    Ok(expected_entry.cloned())
}

fn consensus_tree_entry<'a>(entries: &[Option<&'a String>]) -> Option<Option<&'a String>> {
    let first = entries.first().copied()?;
    entries
        .iter()
        .copied()
        .all(|entry| entry == first)
        .then_some(first)
}

pub(crate) fn gate_has_any_reason(gate: Option<&GateResult>, reason_codes: &[&str]) -> bool {
    gate.is_some_and(|gate| {
        gate.reason_codes
            .iter()
            .any(|code| reason_codes.iter().any(|expected| code == expected))
    })
}

const LATE_STAGE_RELEASE_BLOCK_REASON_CODES: &[&str] = &[
    "release_artifact_authoritative_provenance_invalid",
    "release_artifact_malformed",
    "release_docs_state_missing",
    "release_docs_state_stale",
    "release_docs_state_not_fresh",
];

const LATE_STAGE_RELEASE_TRUTH_BLOCK_REASON_CODES: &[&str] = &[
    "release_docs_state_missing",
    "release_docs_state_stale",
    "release_docs_state_not_fresh",
];

const LATE_STAGE_REVIEW_BLOCK_REASON_CODES: &[&str] = &[
    "review_artifact_malformed",
    "final_review_state_missing",
    "final_review_state_stale",
    "final_review_state_not_fresh",
];

const LATE_STAGE_QA_BLOCK_REASON_CODES: &[&str] = &[
    "qa_artifact_authoritative_provenance_invalid",
    "qa_artifact_malformed",
    "test_plan_artifact_authoritative_provenance_invalid",
    "browser_qa_state_missing",
    "browser_qa_state_stale",
    "browser_qa_state_not_fresh",
];

pub(crate) fn late_stage_release_blocked(gate_finish: Option<&GateResult>) -> bool {
    gate_finish.is_some_and(|gate| gate.failure_class == "ReleaseArtifactNotFresh")
        || gate_has_any_reason(gate_finish, LATE_STAGE_RELEASE_BLOCK_REASON_CODES)
}

pub(crate) fn late_stage_release_truth_blocked(gate_review: Option<&GateResult>) -> bool {
    gate_has_any_reason(gate_review, LATE_STAGE_RELEASE_TRUTH_BLOCK_REASON_CODES)
}

pub(crate) fn late_stage_review_blocked(gate_finish: Option<&GateResult>) -> bool {
    gate_finish.is_some_and(|gate| gate.failure_class == "ReviewArtifactNotFresh")
        || gate_has_any_reason(gate_finish, LATE_STAGE_REVIEW_BLOCK_REASON_CODES)
}

pub(crate) fn late_stage_review_truth_blocked(gate_review: Option<&GateResult>) -> bool {
    gate_has_any_reason(gate_review, LATE_STAGE_REVIEW_BLOCK_REASON_CODES)
}

pub(crate) fn late_stage_qa_blocked(gate_finish: Option<&GateResult>) -> bool {
    gate_finish.is_some_and(|gate| gate.failure_class == "QaArtifactNotFresh")
        || gate_has_any_reason(gate_finish, LATE_STAGE_QA_BLOCK_REASON_CODES)
}

pub(crate) fn task_boundary_block_reason_code(status: &PlanExecutionStatus) -> Option<&str> {
    if status.blocking_task.is_none() || status.blocking_step.is_some() {
        return None;
    }
    status.reason_codes.iter().map(String::as_str).find(|code| {
        matches!(
            *code,
            "prior_task_review_not_green"
                | "prior_task_current_closure_stale"
                | "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "task_cycle_break_active"
                | "current_task_closure_overlay_restore_required"
        )
    })
}

pub(crate) fn task_review_result_requires_verification_reason_codes<'a>(
    reason_codes: impl IntoIterator<Item = &'a str>,
) -> bool {
    const TASK_VERIFICATION_REASON_CODES: &[&str] = &[
        "prior_task_verification_missing",
        "prior_task_verification_missing_legacy",
        "task_verification_summary_malformed",
    ];
    reason_codes
        .into_iter()
        .any(|reason_code| TASK_VERIFICATION_REASON_CODES.contains(&reason_code))
}

#[cfg(test)]
pub(crate) fn task_review_dispatch_task(_status: &PlanExecutionStatus) -> Option<u32> {
    // Public task-boundary routing is closure-state driven. Missing or stale
    // dispatch projection lineage is diagnostic-only; close-current-task owns
    // recording or regenerating the derived projection metadata.
    None
}

pub(crate) fn task_review_result_pending_task(
    _status: &PlanExecutionStatus,
    _dispatch_id: Option<&str>,
) -> Option<u32> {
    // Public task closure is now owned by close-current-task. Missing review or
    // verification projection artifacts remain diagnostics, but they no longer
    // route users into a separate task-review-result waiting lane.
    None
}

pub(crate) fn finish_requires_test_plan_refresh(gate_finish: Option<&GateResult>) -> bool {
    gate_finish.is_some_and(|gate| {
        gate.reason_codes
            .iter()
            .any(|reason_code| reason_code_requires_test_plan_refresh(reason_code))
    })
}

pub(crate) fn reason_code_requires_test_plan_refresh(reason_code: &str) -> bool {
    matches!(
        reason_code,
        "test_plan_artifact_missing"
            | "test_plan_artifact_malformed"
            | "test_plan_artifact_stale"
            | "test_plan_artifact_authoritative_provenance_invalid"
            | "test_plan_artifact_generator_mismatch"
            | "test_plan_generator_mismatch"
            | "test_plan_authoritative_fingerprint_mismatch"
            | "qa_source_test_plan_mismatch"
    )
}

pub(crate) fn public_late_stage_rederivation_basis_present(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_some()
        || status.finish_review_gate_pass_branch_closure_id.is_some()
        || status.current_release_readiness_state.is_some()
        || !matches!(
            status.release_docs_state,
            DownstreamFreshnessState::NotRequired
        )
        || status.current_final_review_branch_closure_id.is_some()
        || status.current_final_review_result.is_some()
        || !matches!(
            status.final_review_state,
            DownstreamFreshnessState::NotRequired
        )
        || status.current_qa_branch_closure_id.is_some()
        || status.current_qa_result.is_some()
        || !matches!(
            status.browser_qa_state,
            DownstreamFreshnessState::NotRequired
        )
}

pub(crate) fn late_stage_projection_targets_present(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_some()
        || status.finish_review_gate_pass_branch_closure_id.is_some()
        || status.current_final_review_branch_closure_id.is_some()
        || status.current_qa_branch_closure_id.is_some()
}

pub(crate) fn public_late_stage_stale_unreviewed(
    status: &PlanExecutionStatus,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    late_stage_projection_targets_present(status)
        && public_late_stage_rederivation_basis_present(status)
        && late_stage_stale_unreviewed(gate_review, gate_finish)
}

pub(crate) fn late_stage_missing_current_closure_stale_provenance_present(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Result<bool, JsonFailure> {
    if branch_closure_refresh_missing_current_closure(status) {
        return Ok(true);
    }
    if status.current_branch_closure_id.is_none() {
        let authoritative_state = load_authoritative_transition_state(context)?;
        if actionable_late_stage_stale_provenance_without_current_branch_closure(
            context,
            authoritative_state.as_ref(),
            status,
        ) {
            return Ok(true);
        }
    }
    if !status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
    {
        return Ok(false);
    }
    Ok(!tracked_paths_changed_since_record_branch_closure_baseline(context)?.is_empty())
}

fn actionable_late_stage_stale_provenance_without_current_branch_closure(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    status: &PlanExecutionStatus,
) -> bool {
    let Some(state) = authoritative_state else {
        return false;
    };
    if resolve_actionable_repair_follow_up_for_status(context, status, Some(state)).is_some_and(
        |follow_up| {
            follow_up.kind == RepairFollowUpKind::RecordBranchClosure
                && follow_up.target_scope == RepairTargetScope::BranchClosure
        },
    ) {
        return true;
    }
    let referenced_current_branch_closure_id_present = state
        .current_release_readiness_record()
        .as_ref()
        .map(|record| record.branch_closure_id.as_str())
        .into_iter()
        .chain(
            state
                .current_final_review_record()
                .as_ref()
                .map(|record| record.branch_closure_id.as_str()),
        )
        .chain(
            state
                .current_browser_qa_record()
                .as_ref()
                .map(|record| record.branch_closure_id.as_str()),
        )
        .any(|closure_id| !closure_id.trim().is_empty());
    referenced_current_branch_closure_id_present
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
}

pub(crate) fn current_branch_closure_has_tracked_drift(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<bool, JsonFailure> {
    if let Some(state) = authoritative_state
        && let Some(identity) = state.bound_current_branch_closure_identity()
        && let Some(record) = state.branch_closure_record(&identity.branch_closure_id)
    {
        let Some(recorded_semantic_reviewed_state_id) = record
            .semantic_reviewed_state_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            let Ok(branch_tree_sha) = resolve_branch_closure_reviewed_tree_sha(
                &context.runtime.repo_root,
                &identity.branch_closure_id,
                &record.reviewed_state_id,
            ) else {
                return Ok(false);
            };
            let current_tree_sha = context.current_tracked_tree_sha()?;
            return Ok(!semantic_paths_changed_between_raw_trees(
                context,
                &branch_tree_sha,
                &current_tree_sha,
            )?
            .is_empty());
        };
        let current_semantic_reviewed_state_id =
            semantic_workspace_snapshot(context)?.semantic_workspace_tree_id;
        return Ok(current_semantic_reviewed_state_id != recorded_semantic_reviewed_state_id);
    }
    Ok(false)
}

fn branch_closure_identity_for_rerecording(
    context: &ExecutionContext,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    validated_current_branch_closure_identity(context).or_else(|| {
        load_authoritative_transition_state(context)
            .ok()
            .flatten()
            .and_then(|state| state.bound_current_branch_closure_identity())
    })
}

pub(crate) fn public_review_state_stale_unreviewed_for_reroute(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    status: &PlanExecutionStatus,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> Result<bool, JsonFailure> {
    Ok(
        public_late_stage_stale_unreviewed(status, gate_review, gate_finish)
            || current_branch_closure_has_tracked_drift(context, authoritative_state)?,
    )
}

pub(crate) fn branch_closure_refresh_missing_current_closure(status: &PlanExecutionStatus) -> bool {
    status.current_release_readiness_state.is_none()
        && status.current_branch_closure_id.is_some()
        && status.current_branch_meaningful_drift
}

pub(crate) struct CurrentTruthSnapshot<'a> {
    pub(crate) authoritative_state: Option<&'a AuthoritativeTransitionState>,
    pub(crate) source_route_decision_hash: Option<&'a str>,
}

impl<'a> CurrentTruthSnapshot<'a> {
    pub(crate) fn from_authoritative_state(
        authoritative_state: Option<&'a AuthoritativeTransitionState>,
    ) -> Self {
        Self {
            authoritative_state,
            source_route_decision_hash: None,
        }
    }

    pub(crate) fn with_source_route_decision_hash(
        mut self,
        source_route_decision_hash: Option<&'a str>,
    ) -> Self {
        self.source_route_decision_hash = source_route_decision_hash;
        self
    }
}

pub(crate) fn resolve_actionable_repair_follow_up(
    state: &RuntimeState,
    current: &CurrentTruthSnapshot<'_>,
) -> Option<RepairFollowUpRecord> {
    resolve_actionable_repair_follow_up_with_status(
        current.authoritative_state,
        &state.status,
        Some(&state.gate_snapshot),
        Some(state.semantic_workspace.semantic_workspace_tree_id.as_str()),
        current.source_route_decision_hash,
    )
}

pub(crate) fn resolve_actionable_repair_follow_up_for_status(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<RepairFollowUpRecord> {
    resolve_actionable_repair_follow_up_for_status_with_source_hash(
        context,
        status,
        authoritative_state,
        None,
    )
}

pub(crate) fn resolve_actionable_repair_follow_up_for_status_with_source_hash(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    source_route_decision_hash: Option<&str>,
) -> Option<RepairFollowUpRecord> {
    let semantic_workspace_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    resolve_actionable_repair_follow_up_with_status(
        authoritative_state,
        status,
        None,
        semantic_workspace_state_id.as_deref(),
        source_route_decision_hash,
    )
}

fn resolve_actionable_repair_follow_up_with_status(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    status: &PlanExecutionStatus,
    gate_snapshot: Option<&RuntimeGateSnapshot>,
    semantic_workspace_state_id: Option<&str>,
    source_route_decision_hash: Option<&str>,
) -> Option<RepairFollowUpRecord> {
    let authoritative_state = authoritative_state?;
    let record = authoritative_state
        .review_state_repair_follow_up_record()
        .or_else(|| {
            legacy_repair_follow_up_record(authoritative_state, semantic_workspace_state_id)
        })?;
    if !repair_follow_up_semantic_workspace_matches(&record, semantic_workspace_state_id) {
        return None;
    }
    let exact_target_still_bound = repair_follow_up_exact_target_still_bound(
        &record,
        authoritative_state,
        status.current_branch_closure_id.as_deref(),
    );
    if record.expires_on_plan_fingerprint_change
        && !exact_target_still_bound
        && record
            .source_route_decision_hash
            .as_deref()
            .zip(source_route_decision_hash)
            .is_some_and(|(stored, current)| stored != current)
    {
        return None;
    }
    repair_follow_up_target_still_bound(&record, gate_snapshot, exact_target_still_bound)
        .then_some(record)
}

fn repair_follow_up_semantic_workspace_matches(
    record: &RepairFollowUpRecord,
    semantic_workspace_state_id: Option<&str>,
) -> bool {
    record
        .semantic_workspace_state_id
        .as_deref()
        .zip(semantic_workspace_state_id)
        .is_none_or(|(record_semantic, current_semantic)| record_semantic == current_semantic)
}

fn legacy_repair_follow_up_record(
    authoritative_state: &AuthoritativeTransitionState,
    semantic_workspace_state_id: Option<&str>,
) -> Option<RepairFollowUpRecord> {
    let raw_token = authoritative_state.review_state_repair_follow_up()?;
    let kind = RepairFollowUpKind::from_persisted_token(raw_token)?;
    if !matches!(
        kind,
        RepairFollowUpKind::ExecutionReentry | RepairFollowUpKind::CloseTask
    ) {
        return None;
    }
    let target_task = authoritative_state.review_state_repair_follow_up_task()?;
    let target_step = authoritative_state.review_state_repair_follow_up_step();
    let target_scope = if kind == RepairFollowUpKind::CloseTask || target_step.is_none() {
        RepairTargetScope::TaskClosure
    } else {
        RepairTargetScope::ExecutionStep
    };
    Some(RepairFollowUpRecord {
        kind,
        target_scope,
        target_task: Some(target_task),
        target_step,
        target_record_id: authoritative_state
            .review_state_repair_follow_up_closure_record_id()
            .or_else(|| {
                (target_scope == RepairTargetScope::ExecutionStep)
                    .then(|| format!("task-{target_task}"))
            }),
        semantic_workspace_state_id: semantic_workspace_state_id.map(str::to_owned),
        source_route_decision_hash: None,
        created_sequence: authoritative_state.latest_authoritative_sequence(),
        created_at: None,
        expires_on_plan_fingerprint_change: true,
    })
}

pub(crate) fn legacy_repair_follow_up_unbound(
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    let Some(authoritative_state) = authoritative_state else {
        return false;
    };
    let Some(raw_token) = authoritative_state.review_state_repair_follow_up() else {
        return false;
    };
    let Some(kind) = RepairFollowUpKind::from_persisted_token(raw_token) else {
        return false;
    };
    if matches!(
        kind,
        RepairFollowUpKind::ExecutionReentry | RepairFollowUpKind::CloseTask
    ) {
        return authoritative_state
            .review_state_repair_follow_up_task()
            .is_none();
    }
    true
}

fn repair_follow_up_target_still_bound(
    record: &RepairFollowUpRecord,
    gate_snapshot: Option<&RuntimeGateSnapshot>,
    exact_target_still_bound: bool,
) -> bool {
    if exact_target_still_bound {
        return true;
    }
    match record.target_scope {
        RepairTargetScope::TaskClosure | RepairTargetScope::ExecutionStep => {
            let Some(task) = record.target_task else {
                return false;
            };
            gate_snapshot.is_some_and(|snapshot| {
                snapshot.stale_targets.iter().any(|target| {
                    target.task == Some(task)
                        && target.record_id.as_deref() == record.target_record_id.as_deref()
                })
            })
        }
        RepairTargetScope::BranchClosure => {
            let Some(record_id) = record.target_record_id.as_deref() else {
                return false;
            };
            gate_snapshot.is_some_and(|snapshot| {
                snapshot.stale_targets.iter().any(|target| {
                    target.scope == crate::execution::reducer::AuthoritativeStaleTargetScope::Branch
                        && target.record_id.as_deref() == Some(record_id)
                })
            })
        }
        RepairTargetScope::ReleaseReadiness
        | RepairTargetScope::FinalReview
        | RepairTargetScope::Qa => false,
    }
}

fn repair_follow_up_exact_target_still_bound(
    record: &RepairFollowUpRecord,
    authoritative_state: &AuthoritativeTransitionState,
    current_branch_closure_id: Option<&str>,
) -> bool {
    match record.target_scope {
        RepairTargetScope::TaskClosure => {
            let Some(task) = record.target_task else {
                return false;
            };
            let Some(closure) = authoritative_state.current_task_closure_result(task) else {
                return false;
            };
            if task_closure_is_current_pass_pass(&closure) {
                return false;
            }
            record
                .target_record_id
                .as_deref()
                .is_none_or(|record_id| record_id == closure.closure_record_id)
        }
        RepairTargetScope::ExecutionStep => {
            let Some(task) = record.target_task else {
                return false;
            };
            let execution_step_target_id = record
                .target_step
                .map(|step| execution_step_repair_target_id(task, step));
            if authoritative_state
                .current_task_closure_result(task)
                .as_ref()
                .is_some_and(task_closure_is_current_pass_pass)
            {
                return false;
            }
            record.target_record_id.as_deref().is_some_and(|record_id| {
                Some(record_id) == execution_step_target_id.as_deref()
                    || record_id == format!("task-{task}")
            })
        }
        RepairTargetScope::BranchClosure => {
            let Some(record_id) = record.target_record_id.as_deref() else {
                return false;
            };
            current_branch_closure_id == Some(record_id)
                || authoritative_state
                    .branch_closure_record(record_id)
                    .is_some()
        }
        RepairTargetScope::ReleaseReadiness => {
            record.target_record_id.as_deref().is_some_and(|record_id| {
                authoritative_state
                    .current_release_readiness_record_id()
                    .as_deref()
                    == Some(record_id)
            })
        }
        RepairTargetScope::FinalReview => {
            record.target_record_id.as_deref().is_some_and(|record_id| {
                authoritative_state
                    .current_final_review_record_id()
                    .as_deref()
                    == Some(record_id)
            })
        }
        RepairTargetScope::Qa => record.target_record_id.as_deref().is_some_and(|record_id| {
            authoritative_state.current_qa_record_id().as_deref() == Some(record_id)
        }),
    }
}

fn task_closure_is_current_pass_pass(closure: &CurrentTaskClosureRecord) -> bool {
    closure.review_result == "pass"
        && closure.verification_result == "pass"
        && closure
            .closure_status
            .as_deref()
            .is_none_or(|status| status == "current")
}

pub(crate) fn late_stage_stale_unreviewed(
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    const LATE_STAGE_STALE_REASON_CODES: &[&str] = &[
        "review_artifact_worktree_dirty",
        REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
        "release_docs_state_stale",
        "release_docs_state_not_fresh",
        "final_review_state_stale",
        "final_review_state_not_fresh",
        "browser_qa_state_stale",
        "browser_qa_state_not_fresh",
    ];

    gate_has_any_reason(gate_review, LATE_STAGE_STALE_REASON_CODES)
        || gate_has_any_reason(gate_finish, LATE_STAGE_STALE_REASON_CODES)
}

pub(crate) fn stale_reason_codes_for_late_stage_projection<'a>(
    status: &'a PlanExecutionStatus,
    gate_reason_codes: impl IntoIterator<Item = &'a String>,
) -> Vec<String> {
    let mut reason_codes = Vec::new();
    for reason_code in gate_reason_codes
        .into_iter()
        .chain(status.reason_codes.iter())
    {
        if (reason_code_indicates_stale_unreviewed(reason_code)
            || reason_code == REASON_CODE_STALE_PROVENANCE)
            && !reason_codes.iter().any(|existing| existing == reason_code)
        {
            reason_codes.push(reason_code.clone());
        }
    }
    reason_codes
}

pub(crate) fn repair_review_state_branch_reroute_active(
    repair_follow_up: Option<&str>,
    task_scope_repair_precedence_active: bool,
    branch_reroute_still_valid: bool,
) -> bool {
    normalize_persisted_repair_follow_up_token(repair_follow_up) == Some("advance_late_stage")
        && !task_scope_repair_precedence_active
        && branch_reroute_still_valid
}

pub(crate) fn repair_review_state_execution_reentry_active(
    repair_follow_up: Option<&str>,
    task_scope_repair_precedence_active: bool,
) -> bool {
    repair_follow_up == Some("execution_reentry") && !task_scope_repair_precedence_active
}

pub(crate) fn live_review_state_status_for_reroute(
    stale_unreviewed: bool,
    missing_current_closure: bool,
) -> Option<&'static str> {
    if stale_unreviewed {
        Some("stale_unreviewed")
    } else if missing_current_closure {
        Some("missing_current_closure")
    } else {
        None
    }
}

pub(crate) fn live_review_state_repair_reroute(
    persisted_follow_up: Option<&str>,
    task_scope_repair_precedence_active: bool,
    branch_reroute_still_valid: bool,
    live_review_state_status: Option<&str>,
    branch_closure_refresh_missing_current_closure: bool,
) -> ReviewStateRepairReroute {
    if !matches!(
        live_review_state_status,
        Some("stale_unreviewed" | "missing_current_closure")
    ) {
        return ReviewStateRepairReroute::None;
    }
    if live_review_state_status == Some("missing_current_closure")
        && !task_scope_repair_precedence_active
        && (branch_reroute_still_valid || branch_closure_refresh_missing_current_closure)
    {
        return ReviewStateRepairReroute::RecordBranchClosure;
    }
    review_state_repair_reroute(
        persisted_follow_up,
        task_scope_repair_precedence_active,
        branch_reroute_still_valid,
    )
}

pub(crate) fn live_task_scope_repair_precedence_active(
    task_scope_overlay_restore_required: bool,
    task_scope_structural_reason_present: bool,
    task_scope_stale_reason_present: bool,
    persisted_follow_up: Option<&str>,
    branch_reroute_still_valid: bool,
    live_review_state_status: Option<&str>,
) -> bool {
    task_scope_overlay_restore_required
        || task_scope_structural_reason_present
        || (task_scope_stale_reason_present
            && !(normalize_persisted_repair_follow_up_token(persisted_follow_up)
                == Some("advance_late_stage")
                && branch_reroute_still_valid
                && matches!(
                    live_review_state_status,
                    Some("stale_unreviewed" | "missing_current_closure")
                )))
}

pub(crate) fn release_readiness_result_for_branch_closure(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    branch_closure_id: Option<&str>,
) -> Option<String> {
    let branch_closure_id = branch_closure_id
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let authoritative_state = authoritative_state?;
    authoritative_state
        .current_release_readiness_record_id()
        .and_then(|record_id| authoritative_state.release_readiness_record_by_id(&record_id))
        .filter(|record| record.branch_closure_id == branch_closure_id)
        .map(|record| record.result)
}

pub(crate) fn task_scope_stale_review_state_reason_present(repair_reason: Option<&str>) -> bool {
    matches!(
        repair_reason,
        Some("prior_task_review_dispatch_stale" | "prior_task_current_closure_stale")
    )
}

pub(crate) fn current_late_stage_branch_bindings(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    current_branch_closure_id: Option<&str>,
    current_branch_reviewed_state_id: Option<&str>,
) -> CurrentLateStageBranchBindings {
    let Some(current_branch_closure_id) = current_branch_closure_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CurrentLateStageBranchBindings::default();
    };
    let Some(current_branch_reviewed_state_id) = current_branch_reviewed_state_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CurrentLateStageBranchBindings::default();
    };
    let Some(authoritative_state) = authoritative_state else {
        return CurrentLateStageBranchBindings::default();
    };
    let Some(current_branch_record) =
        authoritative_state.branch_closure_record(current_branch_closure_id)
    else {
        return CurrentLateStageBranchBindings::default();
    };
    let milestone_reviewed_state_matches =
        |record_semantic_reviewed_state_id: Option<&str>, record_reviewed_state_id: &str| {
            match (
                current_branch_record.semantic_reviewed_state_id.as_deref(),
                record_semantic_reviewed_state_id,
            ) {
                (Some(current), Some(record)) => current == record,
                // Legacy imported milestone records may be missing semantic identities even when
                // the branch closure has one. Confine the raw fallback to milestone-to-branch
                // binding; workspace freshness/routing still uses semantic identities.
                (_, None) | (None, Some(_)) => {
                    record_reviewed_state_id == current_branch_reviewed_state_id
                }
            }
        };
    let milestone_matches_current_branch =
        |source_plan_path: &str,
         source_plan_revision: u32,
         repo_slug: &str,
         branch_name: &str,
         base_branch: &str,
         semantic_reviewed_state_id: Option<&str>,
         reviewed_state_id: &str| {
            source_plan_path == current_branch_record.source_plan_path
                && source_plan_revision == current_branch_record.source_plan_revision
                && repo_slug == current_branch_record.repo_slug
                && branch_name == current_branch_record.branch_name
                && base_branch == current_branch_record.base_branch
                && milestone_reviewed_state_matches(semantic_reviewed_state_id, reviewed_state_id)
        };
    let closure_graph = AuthoritativeClosureGraph::from_state(
        Some(authoritative_state),
        &ClosureGraphSignals::from_authoritative_state(
            Some(authoritative_state),
            Some(current_branch_closure_id),
            false,
            false,
            Vec::new(),
        ),
    );

    let finish_review_gate_pass_branch_closure_id = authoritative_state
        .finish_review_gate_pass_branch_closure_id()
        .filter(|branch_closure_id| branch_closure_id == current_branch_closure_id);
    let current_release_readiness_record_id = closure_graph
        .current_release_readiness_record_id()
        .map(str::to_owned);
    let current_release_readiness_record = current_release_readiness_record_id
        .as_deref()
        .and_then(|record_id| authoritative_state.release_readiness_record_by_id(record_id))
        .filter(|record| {
            record.record_status == "current"
                && record.branch_closure_id == current_branch_closure_id
                && milestone_matches_current_branch(
                    &record.source_plan_path,
                    record.source_plan_revision,
                    &record.repo_slug,
                    &record.branch_name,
                    &record.base_branch,
                    record.semantic_reviewed_state_id.as_deref(),
                    &record.reviewed_state_id,
                )
        });
    let current_release_readiness_record_id =
        current_release_readiness_record_id.filter(|_| current_release_readiness_record.is_some());
    let required_release_readiness_record_id = current_release_readiness_record_id.as_deref();
    let current_release_readiness_result = current_release_readiness_record
        .as_ref()
        .map(|record| record.result.clone());

    let current_final_review_record_id = closure_graph
        .current_final_review_record_id()
        .map(str::to_owned);
    let current_final_review_record = current_final_review_record_id
        .as_deref()
        .and_then(|record_id| authoritative_state.final_review_record_by_id(record_id))
        .filter(|record| {
            record.record_status == "current"
                && record.branch_closure_id == current_branch_closure_id
                && required_release_readiness_record_id.is_some()
                && record.release_readiness_record_id.as_deref()
                    == required_release_readiness_record_id
                && milestone_matches_current_branch(
                    &record.source_plan_path,
                    record.source_plan_revision,
                    &record.repo_slug,
                    &record.branch_name,
                    &record.base_branch,
                    record.semantic_reviewed_state_id.as_deref(),
                    &record.reviewed_state_id,
                )
        });
    let current_final_review_record_id =
        current_final_review_record_id.filter(|_| current_final_review_record.is_some());
    let required_final_review_record_id = current_final_review_record_id.as_deref();
    let (current_final_review_branch_closure_id, current_final_review_result) =
        if let Some(record) = current_final_review_record.as_ref() {
            (
                Some(record.branch_closure_id.clone()),
                Some(record.result.clone()),
            )
        } else {
            (None, None)
        };

    let current_qa_record_id = closure_graph
        .current_browser_qa_record_id()
        .map(str::to_owned);
    let current_qa_record = current_qa_record_id
        .as_deref()
        .and_then(|record_id| authoritative_state.browser_qa_record_by_id(record_id))
        .filter(|record| {
            record.record_status == "current"
                && record.branch_closure_id == current_branch_closure_id
                && required_final_review_record_id.is_some()
                && record.final_review_record_id.as_deref() == required_final_review_record_id
                && milestone_matches_current_branch(
                    &record.source_plan_path,
                    record.source_plan_revision,
                    &record.repo_slug,
                    &record.branch_name,
                    &record.base_branch,
                    record.semantic_reviewed_state_id.as_deref(),
                    &record.reviewed_state_id,
                )
        });
    let current_qa_record_id = current_qa_record_id.filter(|_| current_qa_record.is_some());
    let (current_qa_branch_closure_id, current_qa_result) =
        if let Some(record) = current_qa_record.as_ref() {
            (
                Some(record.branch_closure_id.clone()),
                Some(record.result.clone()),
            )
        } else {
            (None, None)
        };

    CurrentLateStageBranchBindings {
        finish_review_gate_pass_branch_closure_id,
        current_release_readiness_record_id,
        current_release_readiness_result,
        current_final_review_record_id,
        current_final_review_branch_closure_id,
        current_final_review_result,
        current_qa_record_id,
        current_qa_branch_closure_id,
        current_qa_result,
    }
}

pub(crate) fn final_review_dispatch_still_current(
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    const FINAL_REVIEW_DISPATCH_INVALIDATION_REASON_CODES: &[&str] = &[
        "review_artifact_malformed",
        "review_artifact_plan_mismatch",
        "review_artifact_release_binding_mismatch",
    ];

    !(gate_has_any_reason(gate_review, FINAL_REVIEW_DISPATCH_INVALIDATION_REASON_CODES)
        || gate_has_any_reason(gate_finish, FINAL_REVIEW_DISPATCH_INVALIDATION_REASON_CODES))
}

#[cfg(test)]
pub(crate) fn resolve_public_follow_up_override(
    raw_pivot_required: bool,
    raw_handoff_required: bool,
) -> String {
    if raw_pivot_required {
        String::from("repair_review_state")
    } else if raw_handoff_required {
        String::from("record_handoff")
    } else {
        String::from("none")
    }
}

#[cfg(test)]
pub(crate) struct FollowUpOverrideInputs<'a> {
    pub(crate) state_dir: &'a Path,
    pub(crate) repo_slug: &'a str,
    pub(crate) safe_branch: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) plan_path: &'a str,
    pub(crate) head_sha: Option<&'a str>,
    pub(crate) workflow_phase: Option<&'a str>,
    pub(crate) harness_phase: Option<HarnessPhase>,
    pub(crate) handoff_required: bool,
    pub(crate) handoff_decision_scope: Option<&'a str>,
    pub(crate) reason_codes: &'a [String],
    pub(crate) qa_requirement: Option<&'a str>,
}

#[cfg(test)]
pub(crate) fn resolve_follow_up_override(inputs: FollowUpOverrideInputs<'_>) -> String {
    let mut raw_pivot_required = inputs.workflow_phase
        == Some(crate::execution::phase::PHASE_PIVOT_REQUIRED)
        || inputs.harness_phase == Some(HarnessPhase::PivotRequired)
        || inputs.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "blocked_on_plan_revision" | "qa_requirement_missing_or_invalid"
            )
        });
    let mut raw_handoff_required = inputs.workflow_phase
        == Some(crate::execution::phase::PHASE_HANDOFF_REQUIRED)
        || inputs.harness_phase == Some(HarnessPhase::HandoffRequired)
        || inputs.handoff_required;

    if raw_pivot_required && current_workflow_pivot_record_exists_for_decision(&inputs) {
        raw_pivot_required = false;
    }
    if raw_handoff_required && current_workflow_transfer_record_exists_for_decision(&inputs) {
        raw_handoff_required = false;
    }

    resolve_public_follow_up_override(raw_pivot_required, raw_handoff_required)
}

pub(crate) fn handoff_decision_scope(
    active_task: Option<u32>,
    blocking_task: Option<u32>,
    resume_task: Option<u32>,
    handoff_required: bool,
    harness_phase: Option<HarnessPhase>,
) -> Option<&'static str> {
    if active_task.is_some() || blocking_task.is_some() || resume_task.is_some() {
        Some("task")
    } else if handoff_required || harness_phase == Some(HarnessPhase::HandoffRequired) {
        Some("branch")
    } else {
        None
    }
}

pub(crate) fn normalized_plan_qa_requirement(value: Option<&str>) -> Option<String> {
    value.and_then(crate::contracts::plan::normalize_plan_qa_requirement)
}

pub(crate) fn missing_derived_task_scope_overlays(missing_derived_overlays: &[String]) -> bool {
    missing_derived_overlays
        .iter()
        .any(|field| matches!(field.as_str(), "current_task_closure_records"))
}

pub(crate) fn missing_derived_branch_scope_overlays(missing_derived_overlays: &[String]) -> bool {
    missing_derived_overlays.iter().any(|field| {
        matches!(
            field.as_str(),
            "current_branch_closure_id"
                | "current_branch_closure_reviewed_state_id"
                | "current_branch_closure_contract_identity"
        )
    })
}

pub(crate) fn task_scope_overlay_restore_required(
    missing_derived_overlays: &[String],
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    missing_derived_task_scope_overlays(missing_derived_overlays)
        || authoritative_state
            .is_some_and(|state| state.current_task_closure_overlay_needs_restore())
}

pub(crate) fn current_task_review_dispatch_id(
    blocking_task: Option<u32>,
    current_lineage_fingerprint: Option<&str>,
    current_semantic_reviewed_state_id: Option<&str>,
    _current_raw_reviewed_state_id: Option<&str>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Option<String> {
    let overlay = overlay?;
    let task_number = blocking_task?;
    let current_lineage_fingerprint = current_lineage_fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let current_semantic_reviewed_state_id = current_semantic_reviewed_state_id
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let record = overlay
        .strategy_review_dispatch_lineage
        .get(&format!("task-{task_number}"))?;
    let dispatch_id = record
        .dispatch_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let recorded_semantic_reviewed_state_id = record
        .semantic_reviewed_state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let reviewed_state_matches =
        current_semantic_reviewed_state_id == recorded_semantic_reviewed_state_id;
    (record
        .task_completion_lineage_fingerprint
        .as_deref()
        .map(str::trim)
        == Some(current_lineage_fingerprint)
        && reviewed_state_matches)
        .then_some(dispatch_id.to_owned())
}

pub(crate) fn current_final_review_dispatch_id(
    usable_current_branch_closure_id: Option<&str>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Option<String> {
    let overlay = overlay?;
    let usable_current_branch_closure_id = usable_current_branch_closure_id?;
    overlay
        .final_review_dispatch_lineage
        .as_ref()
        .and_then(|record| {
            let execution_run_id = record.execution_run_id.as_deref()?;
            if execution_run_id.trim().is_empty() {
                return None;
            }
            let branch_closure_id = record.branch_closure_id.as_deref()?;
            if usable_current_branch_closure_id != branch_closure_id {
                return None;
            }
            record
                .dispatch_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

pub(crate) fn current_task_negative_result_task(
    execution_status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<u32> {
    if execution_status.blocking_step.is_some() {
        return None;
    }
    let task = execution_status.blocking_task?;
    let negative_result = authoritative_state?.task_closure_negative_result(task)?;
    let lineage = overlay?
        .strategy_review_dispatch_lineage
        .get(&format!("task-{task}"))?;
    let lineage_dispatch_id = lineage.dispatch_id.as_deref()?.trim();
    let lineage_reviewed_state_id = lineage
        .semantic_reviewed_state_id
        .as_deref()
        .or(lineage.reviewed_state_id.as_deref())?
        .trim();
    if lineage_dispatch_id.is_empty() || lineage_reviewed_state_id.is_empty() {
        return None;
    }
    let negative_result_reviewed_state_id = negative_result
        .semantic_reviewed_state_id
        .as_deref()
        .unwrap_or(negative_result.reviewed_state_id.as_str());
    (negative_result.dispatch_id == lineage_dispatch_id
        && negative_result_reviewed_state_id == lineage_reviewed_state_id)
        .then_some(task)
}

pub(crate) fn negative_result_requires_execution_reentry(
    task_negative_result_present: bool,
    workflow_phase: &str,
    current_branch_closure_id: Option<&str>,
    current_final_review_branch_closure_id: Option<&str>,
    current_final_review_result: Option<&str>,
    current_qa_branch_closure_id: Option<&str>,
    current_qa_result: Option<&str>,
) -> bool {
    let final_review_failed = current_final_review_result == Some("fail")
        && current_final_review_branch_closure_id
            .zip(current_branch_closure_id)
            .is_some_and(|(recorded, current)| recorded == current);
    let qa_failed = current_qa_result == Some("fail")
        && current_qa_branch_closure_id
            .zip(current_branch_closure_id)
            .is_some_and(|(recorded, current)| recorded == current);

    if final_review_failed || qa_failed {
        return true;
    }

    if matches!(
        workflow_phase,
        crate::execution::phase::PHASE_HANDOFF_REQUIRED
            | crate::execution::phase::PHASE_PIVOT_REQUIRED
    ) {
        return false;
    }

    task_negative_result_present
}

pub(crate) fn qa_requirement_policy_invalid(gate_finish: Option<&GateResult>) -> bool {
    gate_finish.is_some_and(|gate| {
        gate.reason_codes
            .iter()
            .any(|code| code == "qa_requirement_missing_or_invalid")
    })
}

pub(crate) fn review_state_repair_reroute(
    persisted_follow_up: Option<&str>,
    task_scope_repair_precedence_active: bool,
    branch_reroute_still_valid: bool,
) -> ReviewStateRepairReroute {
    if repair_review_state_branch_reroute_active(
        persisted_follow_up,
        task_scope_repair_precedence_active,
        branch_reroute_still_valid,
    ) {
        ReviewStateRepairReroute::RecordBranchClosure
    } else if repair_review_state_execution_reentry_active(
        persisted_follow_up,
        task_scope_repair_precedence_active,
    ) {
        ReviewStateRepairReroute::ExecutionReentry
    } else {
        ReviewStateRepairReroute::None
    }
}

#[cfg(test)]
fn current_workflow_pivot_record_exists_for_decision(inputs: &FollowUpOverrideInputs<'_>) -> bool {
    if inputs.plan_path.trim().is_empty() {
        return false;
    }
    let Some(head_sha) = inputs.head_sha.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    let qa_requirement_missing_or_invalid = !matches!(
        inputs.qa_requirement,
        Some("required") | Some("not-required")
    );
    let decision_reason_codes =
        pivot_decision_reason_codes(inputs.reason_codes, true, qa_requirement_missing_or_invalid);
    current_workflow_pivot_record_exists(
        inputs.state_dir,
        WorkflowPivotRecordIdentity {
            repo_slug: inputs.repo_slug,
            safe_branch: inputs.safe_branch,
            plan_path: inputs.plan_path,
            branch_name: inputs.branch_name,
            head_sha,
            decision_reason_codes: &decision_reason_codes,
        },
    )
}

#[cfg(test)]
fn current_workflow_transfer_record_exists_for_decision(
    inputs: &FollowUpOverrideInputs<'_>,
) -> bool {
    if inputs.plan_path.trim().is_empty() {
        return false;
    }
    let Some(head_sha) = inputs.head_sha.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    current_workflow_transfer_record_exists(
        inputs.state_dir,
        WorkflowTransferRecordIdentity {
            repo_slug: inputs.repo_slug,
            safe_branch: inputs.safe_branch,
            plan_path: inputs.plan_path,
            branch_name: inputs.branch_name,
            head_sha,
            decision_reason_codes: inputs.reason_codes,
            decision_scope: inputs.handoff_decision_scope,
        },
    )
}
