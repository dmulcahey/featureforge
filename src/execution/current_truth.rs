use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::contracts::headers::parse_required_header as parse_plan_header;
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::handoff::{
    WorkflowTransferRecordIdentity, current_workflow_transfer_record_exists,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::observability::REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED;
use crate::execution::state::{
    ExecutionContext, GateResult, NO_REPO_FILES_MARKER, PlanExecutionStatus,
    branch_closure_record_matches_plan_exemption, resolve_branch_closure_reviewed_tree_sha,
    resolve_task_closure_reviewed_tree_sha, still_current_task_closure_records,
    validated_current_branch_closure_identity,
};
use crate::execution::transitions::load_authoritative_transition_state;
use crate::execution::transitions::{AuthoritativeTransitionState, CurrentTaskClosureRecord};
use crate::git::{discover_repository, sha256_hex};
use crate::workflow::pivot::{
    WorkflowPivotRecordIdentity, current_workflow_pivot_record_exists, pivot_decision_reason_codes,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CurrentLateStageBranchBindings {
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub current_release_readiness_result: Option<String>,
    pub current_final_review_branch_closure_id: Option<String>,
    pub current_final_review_result: Option<String>,
    pub current_qa_branch_closure_id: Option<String>,
    pub current_qa_result: Option<String>,
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

fn deterministic_record_id(prefix: &str, parts: &[&str]) -> String {
    let mut payload = String::from(prefix);
    for part in parts {
        payload.push('\n');
        payload.push_str(part);
    }
    let digest = sha256_hex(payload.as_bytes());
    format!("{prefix}-{}", &digest[..16])
}

pub(crate) fn branch_contract_identity(
    plan_rel: &str,
    plan_revision: u32,
    repo_slug: &str,
    branch_name: &str,
    base_branch: &str,
) -> String {
    deterministic_record_id(
        "branch-contract",
        &[
            plan_rel,
            &plan_revision.to_string(),
            repo_slug,
            branch_name,
            base_branch,
        ],
    )
}

pub(crate) fn task_closure_contributes_to_branch_surface(
    current_record: &CurrentTaskClosureRecord,
) -> bool {
    current_record
        .effective_reviewed_surface_paths
        .iter()
        .any(|surface_path| surface_path != NO_REPO_FILES_MARKER)
}

pub(crate) fn branch_source_task_closure_ids(
    current_records: &[CurrentTaskClosureRecord],
    late_stage_surface: Option<&[String]>,
) -> Vec<String> {
    let mut source_task_closure_ids = current_records
        .iter()
        .filter(|record| {
            if !task_closure_contributes_to_branch_surface(record) {
                return false;
            }
            let Some(late_stage_surface) = late_stage_surface else {
                return true;
            };
            record
                .effective_reviewed_surface_paths
                .iter()
                .filter(|surface_path| surface_path.as_str() != NO_REPO_FILES_MARKER)
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
    // This helper intentionally uses `git add -u` plus `git write-tree` against a copied index
    // because the reviewed-state contract is defined in terms of exact Git tracked-tree identity.
    // The current gix path we use elsewhere does not yet provide a drop-in equivalent for
    // "stage tracked worktree deltas into a temp index and emit the same tree object ID the git
    // CLI would produce", including index semantics relied on by the runtime/test contracts.
    // Keep this boundary memoized at the ExecutionContext level and prefer in-process gix reads
    // around it so status/operator paths do not repeat the subprocess cost inside one command.
    if !repo_has_tracked_worktree_deltas_for_review_state(&repo)? {
        return git_write_tree(repo_root, None);
    }
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

fn repo_has_tracked_worktree_deltas_for_review_state(
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
                    "Could not determine tracked worktree changes for reviewed-state identity: {error}"
                ),
            )
        })?;
    for item in &mut status_iter {
        let item = item.map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not determine tracked worktree changes for reviewed-state identity: {error}"
                ),
            )
        })?;
        if let gix::status::Item::IndexWorktree(change) = item
            && change.summary().is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
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

type TrackedPathDiffCache = RwLock<BTreeMap<(PathBuf, String, String), Vec<String>>>;

pub(crate) fn tracked_paths_changed_between(
    repo_root: &Path,
    baseline_tree_sha: &str,
    current_tree_sha: &str,
) -> Result<Vec<String>, JsonFailure> {
    static TRACKED_PATH_DIFF_CACHE: OnceLock<TrackedPathDiffCache> = OnceLock::new();

    let cache_key = (
        repo_root.to_path_buf(),
        baseline_tree_sha.to_owned(),
        current_tree_sha.to_owned(),
    );
    if let Some(cached) = TRACKED_PATH_DIFF_CACHE
        .get_or_init(|| RwLock::new(BTreeMap::new()))
        .read()
        .expect("tracked path diff cache lock should not be poisoned")
        .get(&cache_key)
        .cloned()
    {
        return Ok(cached);
    }

    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not discover the repository while diffing reviewed-state trees: {error}"
            ),
        )
    })?;
    let baseline_tree = tree_from_sha(&repo, baseline_tree_sha)?;
    let current_tree = tree_from_sha(&repo, current_tree_sha)?;
    let mut options = gix::diff::Options::default();
    options.track_path();
    options.track_rewrites(None);
    let mut changed_paths = repo
        .diff_tree_to_tree(Some(&baseline_tree), Some(&current_tree), Some(options))
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!(
                    "Could not diff reviewed-state trees for branch reclosure validation: {error}"
                ),
            )
        })?
        .into_iter()
        .map(|change| change.location().to_string())
        .collect::<Vec<_>>();
    changed_paths.sort();
    changed_paths.dedup();
    TRACKED_PATH_DIFF_CACHE
        .get_or_init(|| RwLock::new(BTreeMap::new()))
        .write()
        .expect("tracked path diff cache lock should not be poisoned")
        .insert(cache_key, changed_paths.clone());
    Ok(changed_paths)
}

pub(crate) fn current_branch_closure_reviewed_tree_sha(
    context: &ExecutionContext,
) -> Option<String> {
    let identity = validated_current_branch_closure_identity(context)?;
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

pub(crate) fn current_branch_closure_baseline_tree_sha(
    context: &ExecutionContext,
) -> Option<String> {
    let branch_tree_sha = current_branch_closure_reviewed_tree_sha(context)?;
    let current_tree_sha = context.current_tracked_tree_sha().ok()?;
    (current_tree_sha == branch_tree_sha).then_some(branch_tree_sha)
}

fn current_branch_closure_allows_empty_lineage_late_stage_rerecord(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let Some(identity) = validated_current_branch_closure_identity(context) else {
        return Ok(false);
    };
    let Some(authoritative_state) = load_authoritative_transition_state(context)? else {
        return Ok(false);
    };
    let Some(record) = authoritative_state.branch_closure_record(&identity.branch_closure_id)
    else {
        return Ok(false);
    };
    Ok(
        record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption"
            && record.source_task_closure_ids.is_empty()
            && branch_closure_record_matches_plan_exemption(context, &record),
    )
}

pub(crate) fn tracked_paths_changed_since_record_branch_closure_baseline(
    context: &ExecutionContext,
) -> Result<Vec<String>, JsonFailure> {
    if let Some(branch_tree_sha) = current_branch_closure_reviewed_tree_sha(context) {
        let current_tree_sha = context.current_tracked_tree_sha()?;
        if current_tree_sha == branch_tree_sha {
            return Ok(Vec::new());
        }
        return tracked_paths_changed_between(
            &context.runtime.repo_root,
            &branch_tree_sha,
            &current_tree_sha,
        );
    }
    let current_records = current_branch_task_closure_records(context)?;
    if !current_records.is_empty() {
        return tracked_paths_changed_since_task_closure_records_baseline(
            context,
            &current_records,
        );
    }
    Ok(Vec::new())
}

pub(crate) fn branch_closure_rerecording_supported(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let current_records = current_branch_task_closure_records(context)?;
    let changed_paths = tracked_paths_changed_since_record_branch_closure_baseline(context)?;
    if current_records.is_empty() {
        if !current_branch_closure_allows_empty_lineage_late_stage_rerecord(context)? {
            return Ok(false);
        }
        if changed_paths.is_empty() {
            return Ok(true);
        }
        let late_stage_surface = normalized_late_stage_surface(&context.plan_source)?;
        return Ok(!late_stage_surface.is_empty()
            && changed_paths
                .iter()
                .all(|path| path_matches_late_stage_surface(path, &late_stage_surface)));
    }
    if changed_paths.is_empty() {
        return Ok(true);
    }
    let late_stage_surface = normalized_late_stage_surface(&context.plan_source)?;
    Ok(!late_stage_surface.is_empty()
        && changed_paths
            .iter()
            .all(|path| path_matches_late_stage_surface(path, &late_stage_surface)))
}

fn current_branch_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    if load_authoritative_transition_state(context)?.is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "record-branch-closure requires authoritative current task-closure state.",
        ));
    }
    Ok(still_current_task_closure_records(context)?
        .into_iter()
        .filter(task_closure_contributes_to_branch_surface)
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeEntryIdentity {
    mode: String,
    object_id: String,
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
    let current_entries = cached_tree_entries_for_tree_sha(
        &mut tree_entries_cache,
        &context.runtime.repo_root,
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
        let tree_entries = cached_tree_entries_for_tree_sha(
            &mut tree_entries_cache,
            &context.runtime.repo_root,
            &tree_sha,
        )?;
        all_paths.extend(tree_entries.keys().cloned());
        closure_tree_entries.push((current_record, tree_entries));
    }

    let mut changed_paths = Vec::new();
    for path in all_paths {
        let current_entry = current_entries.get(&path);
        let covering_entries = closure_tree_entries
            .iter()
            .filter(|(record, _)| task_closure_record_covers_path(record, &path))
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

    Ok(changed_paths)
}

fn cached_tree_entries_for_tree_sha(
    cache: &mut BTreeMap<String, BTreeMap<String, TreeEntryIdentity>>,
    repo_root: &Path,
    tree_sha: &str,
) -> Result<BTreeMap<String, TreeEntryIdentity>, JsonFailure> {
    if let Some(entries) = cache.get(tree_sha) {
        return Ok(entries.clone());
    }
    let entries = tree_entries_for_tree_sha(repo_root, tree_sha)?;
    cache.insert(tree_sha.to_owned(), entries.clone());
    Ok(entries)
}

fn tree_entries_for_tree_sha(
    repo_root: &Path,
    tree_sha: &str,
) -> Result<BTreeMap<String, TreeEntryIdentity>, JsonFailure> {
    static TREE_ENTRIES_CACHE: OnceLock<
        Mutex<BTreeMap<String, BTreeMap<String, TreeEntryIdentity>>>,
    > = OnceLock::new();

    if let Some(cached) = TREE_ENTRIES_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("tree entries cache lock should not be poisoned")
        .get(tree_sha)
        .cloned()
    {
        return Ok(cached);
    }

    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!(
                "Could not discover the repository while inspecting reviewed-state tree {tree_sha}: {error}"
            ),
        )
    })?;
    let tree = tree_from_sha(&repo, tree_sha)?;
    let mut entries = BTreeMap::new();
    for entry in tree.traverse().breadthfirst.files().map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not inspect reviewed-state tree {tree_sha}: {error}"),
        )
    })? {
        if entry.mode.is_tree() {
            continue;
        }
        let raw_path: &[u8] = entry.filepath.as_ref();
        let path = String::from_utf8(raw_path.to_vec()).map_err(|_| {
            JsonFailure::new(
                FailureClass::BranchDetectionFailed,
                format!("Reviewed-state tree {tree_sha} contained a non-utf8 repo path."),
            )
        })?;
        entries.insert(
            path,
            TreeEntryIdentity {
                mode: entry.mode.kind().as_octal_str().to_string(),
                object_id: entry.oid.to_string(),
            },
        );
    }
    TREE_ENTRIES_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("tree entries cache lock should not be poisoned")
        .insert(tree_sha.to_owned(), entries.clone());
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

fn tree_from_sha<'repo>(
    repo: &'repo gix::Repository,
    tree_sha: &str,
) -> Result<gix::Tree<'repo>, JsonFailure> {
    let object_id = gix::hash::ObjectId::from_hex(tree_sha.as_bytes()).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not parse reviewed-state tree id `{tree_sha}`: {error}"),
        )
    })?;
    repo.find_tree(object_id).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not load reviewed-state tree {tree_sha}: {error}"),
        )
    })
}

fn task_closure_record_covers_path(current_record: &CurrentTaskClosureRecord, path: &str) -> bool {
    current_record
        .effective_reviewed_surface_paths
        .iter()
        .any(|surface_path| {
            path_matches_late_stage_surface(path, std::slice::from_ref(surface_path))
        })
}

fn require_unambiguous_task_closure_path_entry(
    path: &str,
    entries: &[Option<&TreeEntryIdentity>],
) -> Result<Option<TreeEntryIdentity>, JsonFailure> {
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

fn consensus_tree_entry<'a>(
    entries: &[Option<&'a TreeEntryIdentity>],
) -> Option<Option<&'a TreeEntryIdentity>> {
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
    "review_artifact_authoritative_provenance_invalid",
    "final_review_state_missing",
    "final_review_state_stale",
    "final_review_state_not_fresh",
    "review_receipt_reviewer_fingerprint_invalid",
    "review_receipt_reviewer_fingerprint_mismatch",
];

const LATE_STAGE_QA_BLOCK_REASON_CODES: &[&str] = &[
    "qa_artifact_authoritative_provenance_invalid",
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
                | "task_review_not_independent"
                | "task_review_receipt_malformed"
                | "prior_task_verification_missing"
                | "prior_task_verification_missing_legacy"
                | "task_verification_receipt_malformed"
                | "prior_task_review_dispatch_missing"
                | "prior_task_review_dispatch_stale"
                | "prior_task_current_closure_stale"
                | "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "task_cycle_break_active"
        )
    })
}

pub(crate) fn task_review_dispatch_task(status: &PlanExecutionStatus) -> Option<u32> {
    let blocking_task = status.blocking_task?;
    let reason_code = task_boundary_block_reason_code(status)?;
    if reason_code == "prior_task_review_dispatch_missing" {
        Some(blocking_task)
    } else {
        None
    }
}

pub(crate) fn task_review_result_pending_task(
    status: &PlanExecutionStatus,
    dispatch_id: Option<&str>,
) -> Option<u32> {
    if status.blocking_step.is_some() {
        return None;
    }
    let blocking_task = status.blocking_task?;
    let reason_code = task_boundary_block_reason_code(status)?;
    let dispatch_id = dispatch_id?.trim();
    if dispatch_id.is_empty() {
        return None;
    }
    matches!(
        reason_code,
        "prior_task_review_not_green"
            | "task_review_not_independent"
            | "task_review_receipt_malformed"
            | "prior_task_verification_missing"
            | "prior_task_verification_missing_legacy"
            | "task_verification_receipt_malformed"
    )
    .then_some(blocking_task)
}

pub(crate) fn finish_requires_test_plan_refresh(gate_finish: Option<&GateResult>) -> bool {
    gate_has_any_reason(
        gate_finish,
        &[
            "test_plan_artifact_missing",
            "test_plan_artifact_malformed",
            "test_plan_artifact_stale",
            "test_plan_artifact_authoritative_provenance_invalid",
            "test_plan_artifact_generator_mismatch",
        ],
    )
}

pub(crate) fn public_late_stage_rederivation_basis_present(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_some()
        || status.finish_review_gate_pass_branch_closure_id.is_some()
        || status.current_release_readiness_state.is_some()
        || status.current_final_review_branch_closure_id.is_some()
        || status.current_final_review_result.is_some()
        || status.current_qa_branch_closure_id.is_some()
        || status.current_qa_result.is_some()
}

pub(crate) fn public_late_stage_stale_unreviewed(
    status: &PlanExecutionStatus,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    public_late_stage_rederivation_basis_present(status)
        && late_stage_stale_unreviewed(gate_review, gate_finish)
}

pub(crate) fn current_branch_closure_has_tracked_drift(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<bool, JsonFailure> {
    let Some(baseline_tree_sha) =
        authoritative_state.and_then(|_| current_branch_closure_reviewed_tree_sha(context))
    else {
        return Ok(false);
    };
    Ok(context.current_tracked_tree_sha()? != baseline_tree_sha)
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
        && status
            .current_branch_reviewed_state_id
            .as_deref()
            .is_some_and(|reviewed_state_id| reviewed_state_id != status.workspace_state_id)
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

    gate_review
        .is_some_and(|gate| gate.failure_class == FailureClass::StaleExecutionEvidence.as_str())
        || gate_has_any_reason(gate_review, LATE_STAGE_STALE_REASON_CODES)
        || gate_has_any_reason(gate_finish, LATE_STAGE_STALE_REASON_CODES)
}

pub(crate) fn late_stage_stale_unreviewed_closure_ids(
    status: &PlanExecutionStatus,
    overlay_current_branch_closure_id: Option<&str>,
) -> Vec<String> {
    let mut closures = Vec::new();
    for closure_id in [
        status.current_branch_closure_id.as_deref(),
        overlay_current_branch_closure_id,
        status.finish_review_gate_pass_branch_closure_id.as_deref(),
        status.current_final_review_branch_closure_id.as_deref(),
        status.current_qa_branch_closure_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let closure_id = closure_id.trim();
        if closure_id.is_empty() || closures.iter().any(|existing| existing == closure_id) {
            continue;
        }
        closures.push(closure_id.to_owned());
    }
    if closures.is_empty() {
        for closure in &status.current_task_closures {
            let closure_id = closure.closure_record_id.trim();
            if closure_id.is_empty() || closures.iter().any(|existing| existing == closure_id) {
                continue;
            }
            closures.push(closure_id.to_owned());
        }
    }
    closures
}

pub(crate) fn repair_review_state_branch_reroute_active(
    repair_follow_up: Option<&str>,
    task_scope_repair_precedence_active: bool,
    branch_reroute_still_valid: bool,
) -> bool {
    repair_follow_up == Some("record_branch_closure")
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
            && !(persisted_follow_up == Some("record_branch_closure")
                && branch_reroute_still_valid
                && matches!(
                    live_review_state_status,
                    Some("stale_unreviewed" | "missing_current_closure")
                )))
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
    if current_branch_record.reviewed_state_id != current_branch_reviewed_state_id {
        return CurrentLateStageBranchBindings::default();
    }
    let milestone_matches_current_branch =
        |source_plan_path: &str,
         source_plan_revision: u32,
         repo_slug: &str,
         branch_name: &str,
         base_branch: &str,
         reviewed_state_id: &str| {
            source_plan_path == current_branch_record.source_plan_path
                && source_plan_revision == current_branch_record.source_plan_revision
                && repo_slug == current_branch_record.repo_slug
                && branch_name == current_branch_record.branch_name
                && base_branch == current_branch_record.base_branch
                && reviewed_state_id == current_branch_record.reviewed_state_id
        };

    let finish_review_gate_pass_branch_closure_id = authoritative_state
        .finish_review_gate_pass_branch_closure_id()
        .filter(|branch_closure_id| branch_closure_id == current_branch_closure_id);

    let current_release_readiness_result = authoritative_state
        .current_release_readiness_record()
        .and_then(|record| {
            (record.branch_closure_id == current_branch_closure_id
                && milestone_matches_current_branch(
                    &record.source_plan_path,
                    record.source_plan_revision,
                    &record.repo_slug,
                    &record.branch_name,
                    &record.base_branch,
                    &record.reviewed_state_id,
                ))
            .then_some(record.result)
        });

    let (current_final_review_branch_closure_id, current_final_review_result) = authoritative_state
        .current_final_review_record()
        .and_then(|record| {
            (record.branch_closure_id == current_branch_closure_id
                && milestone_matches_current_branch(
                    &record.source_plan_path,
                    record.source_plan_revision,
                    &record.repo_slug,
                    &record.branch_name,
                    &record.base_branch,
                    &record.reviewed_state_id,
                ))
            .then_some((Some(record.branch_closure_id), Some(record.result)))
        })
        .unwrap_or((None, None));

    let (current_qa_branch_closure_id, current_qa_result) = authoritative_state
        .current_browser_qa_record()
        .and_then(|record| {
            (record.branch_closure_id == current_branch_closure_id
                && milestone_matches_current_branch(
                    &record.source_plan_path,
                    record.source_plan_revision,
                    &record.repo_slug,
                    &record.branch_name,
                    &record.base_branch,
                    &record.reviewed_state_id,
                ))
            .then_some((Some(record.branch_closure_id), Some(record.result)))
        })
        .unwrap_or((None, None));

    CurrentLateStageBranchBindings {
        finish_review_gate_pass_branch_closure_id,
        current_release_readiness_result,
        current_final_review_branch_closure_id,
        current_final_review_result,
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
        "review_artifact_authoritative_provenance_invalid",
        "review_receipt_reviewer_fingerprint_invalid",
        "review_receipt_reviewer_fingerprint_mismatch",
    ];

    let review_artifact_gate_blocked =
        gate_finish.is_some_and(|gate| gate.failure_class == "ReviewArtifactNotFresh");
    !(review_artifact_gate_blocked
        || gate_has_any_reason(gate_review, FINAL_REVIEW_DISPATCH_INVALIDATION_REASON_CODES)
        || gate_has_any_reason(gate_finish, FINAL_REVIEW_DISPATCH_INVALIDATION_REASON_CODES))
}

pub(crate) fn resolve_public_follow_up_override(
    raw_pivot_required: bool,
    raw_handoff_required: bool,
) -> String {
    if raw_pivot_required {
        String::from("record_pivot")
    } else if raw_handoff_required {
        String::from("record_handoff")
    } else {
        String::from("none")
    }
}

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

pub(crate) fn resolve_follow_up_override(inputs: FollowUpOverrideInputs<'_>) -> String {
    let mut raw_pivot_required = inputs.workflow_phase == Some("pivot_required")
        || inputs.harness_phase == Some(HarnessPhase::PivotRequired)
        || inputs.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "blocked_on_plan_revision" | "qa_requirement_missing_or_invalid"
            )
        });
    let mut raw_handoff_required = inputs.workflow_phase == Some("handoff_required")
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
    missing_derived_overlays.iter().any(|field| {
        matches!(
            field.as_str(),
            "current_task_closure_records" | "task_closure_negative_result_records"
        )
    })
}

pub(crate) fn task_scope_overlay_restore_required(
    missing_derived_overlays: &[String],
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    missing_derived_task_scope_overlays(missing_derived_overlays)
        || authoritative_state.is_some_and(|state| {
            state.current_task_closure_overlay_needs_restore()
                || state.task_closure_negative_result_overlay_needs_restore()
        })
}

pub(crate) fn current_task_review_dispatch_id(
    blocking_task: Option<u32>,
    current_lineage_fingerprint: Option<&str>,
    current_reviewed_state_id: Option<&str>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Option<String> {
    let overlay = overlay?;
    let task_number = blocking_task?;
    let current_lineage_fingerprint = current_lineage_fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let current_reviewed_state_id = current_reviewed_state_id
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
    (record
        .task_completion_lineage_fingerprint
        .as_deref()
        .map(str::trim)
        == Some(current_lineage_fingerprint)
        && record.reviewed_state_id.as_deref().map(str::trim) == Some(current_reviewed_state_id))
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
    let lineage_reviewed_state_id = lineage.reviewed_state_id.as_deref()?.trim();
    if lineage_dispatch_id.is_empty() || lineage_reviewed_state_id.is_empty() {
        return None;
    }
    (negative_result.dispatch_id == lineage_dispatch_id
        && negative_result.reviewed_state_id == lineage_reviewed_state_id)
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
    if matches!(workflow_phase, "handoff_required" | "pivot_required") {
        return false;
    }

    if task_negative_result_present {
        return true;
    }

    let final_review_failed = current_final_review_result == Some("fail")
        && current_final_review_branch_closure_id
            .zip(current_branch_closure_id)
            .is_some_and(|(recorded, current)| recorded == current);
    let qa_failed = current_qa_result == Some("fail")
        && current_qa_branch_closure_id
            .zip(current_branch_closure_id)
            .is_some_and(|(recorded, current)| recorded == current);

    final_review_failed || qa_failed
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
