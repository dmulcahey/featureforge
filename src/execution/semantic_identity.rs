use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::contracts::task_contract::{
    RuntimeExecutionNoteProjectionBlock, known_runtime_step_projection_lines,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::current_truth::{
    is_runtime_owned_execution_control_plane_path, normalized_late_stage_surface,
};
use crate::execution::state::{ExecutionContext, parse_step_line};
use crate::git::discover_repository;
use crate::git::sha256_hex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SemanticWorkspaceSnapshot {
    pub(crate) raw_workspace_tree_id: String,
    pub(crate) semantic_workspace_tree_id: String,
    pub(crate) plan_definition_identity: String,
    pub(crate) task_definition_identity: BTreeMap<u32, String>,
    pub(crate) branch_definition_identity: String,
}

pub(crate) fn semantic_workspace_snapshot(
    context: &ExecutionContext,
) -> Result<SemanticWorkspaceSnapshot, JsonFailure> {
    match context
        .semantic_workspace_snapshot_cache
        .get_or_init(|| compute_semantic_workspace_snapshot(context))
    {
        Ok(snapshot) => Ok(snapshot.clone()),
        Err(error) => Err(error.clone()),
    }
}

fn compute_semantic_workspace_snapshot(
    context: &ExecutionContext,
) -> Result<SemanticWorkspaceSnapshot, JsonFailure> {
    let raw_tree_sha = context.current_tracked_tree_sha()?;
    let semantic_tree_sha = compute_semantic_workspace_tree_id(context, &raw_tree_sha)?;
    let plan_definition_identity = compute_plan_definition_identity(context);
    let task_definition_identity = compute_task_definition_identities(context)?;
    let branch_definition_identity =
        compute_branch_definition_identity(context, &plan_definition_identity)?;
    Ok(SemanticWorkspaceSnapshot {
        raw_workspace_tree_id: format!("git_tree:{raw_tree_sha}"),
        semantic_workspace_tree_id: format!("semantic_tree:{semantic_tree_sha}"),
        plan_definition_identity,
        task_definition_identity,
        branch_definition_identity,
    })
}

pub fn task_definition_identity_for_task(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<Option<String>, JsonFailure> {
    Ok(compute_task_definition_identities(context)?
        .get(&task_number)
        .cloned())
}

pub fn branch_definition_identity_for_context(context: &ExecutionContext) -> String {
    try_branch_definition_identity_for_context(context).unwrap_or_else(|error| {
        let material = format!(
            "plan_def={}\nrepo_slug={}\nbranch_name={}\ninvalid_late_stage_surface={}",
            compute_plan_definition_identity(context),
            context.runtime.repo_slug,
            context.runtime.branch_name,
            error.message
        );
        format!("branch_def:{}", sha256_hex(material.as_bytes()))
    })
}

pub(crate) fn try_branch_definition_identity_for_context(
    context: &ExecutionContext,
) -> Result<String, JsonFailure> {
    compute_branch_definition_identity(context, &compute_plan_definition_identity(context))
}

fn compute_semantic_workspace_tree_id(
    context: &ExecutionContext,
    raw_tree_sha: &str,
) -> Result<String, JsonFailure> {
    let semantic_entries = semantic_tree_entries_for_raw_tree(context, raw_tree_sha)?;
    let semantic_material = semantic_entries
        .into_iter()
        .map(|(path, blob_id)| format!("{blob_id}\t{path}"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(sha256_hex(semantic_material.as_bytes()))
}

pub(crate) fn semantic_paths_changed_between_raw_trees(
    context: &ExecutionContext,
    baseline_tree_sha: &str,
    current_tree_sha: &str,
) -> Result<Vec<String>, JsonFailure> {
    let baseline_entries = semantic_tree_entries_for_raw_tree(context, baseline_tree_sha)?;
    let current_entries = semantic_tree_entries_for_raw_tree(context, current_tree_sha)?;
    let all_paths = baseline_entries
        .keys()
        .chain(current_entries.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    Ok(all_paths
        .into_iter()
        .filter(|path| baseline_entries.get(path) != current_entries.get(path))
        .collect())
}

pub(crate) fn semantic_tree_entries_for_raw_tree(
    context: &ExecutionContext,
    raw_tree_sha: &str,
) -> Result<BTreeMap<String, String>, JsonFailure> {
    static SEMANTIC_TREE_ENTRIES_CACHE: OnceLock<
        Mutex<BTreeMap<String, BTreeMap<String, String>>>,
    > = OnceLock::new();
    let cache_key = format!(
        "{}::{}::{}",
        context.runtime.repo_root.display(),
        context.plan_rel,
        raw_tree_sha
    );
    if let Some(cached) = SEMANTIC_TREE_ENTRIES_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("semantic tree entries cache lock should not be poisoned")
        .get(&cache_key)
        .cloned()
    {
        return Ok(cached);
    }
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Could not discover repository for semantic identity: {error}"),
        )
    })?;
    let object_id = gix::hash::ObjectId::from_hex(raw_tree_sha.as_bytes()).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Could not parse tracked tree id `{raw_tree_sha}` for semantic identity: {error}"
            ),
        )
    })?;
    let tree = repo.find_tree(object_id).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Could not load tracked tree {raw_tree_sha} for semantic identity: {error}"),
        )
    })?;
    let normalized_plan_path = normalize_repo_relative_path(&context.plan_rel);
    let mut semantic_entries = BTreeMap::new();
    for entry in tree.traverse().breadthfirst.files().map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Could not traverse tracked tree {raw_tree_sha} for semantic identity: {error}"
            ),
        )
    })? {
        if entry.mode.is_tree() {
            continue;
        }
        let raw_path: &[u8] = entry.filepath.as_ref();
        let path = String::from_utf8(raw_path.to_vec()).map_err(|_| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!("Tracked tree {raw_tree_sha} contained a non-utf8 repo path."),
            )
        })?;
        if !is_runtime_owned_semantic_exclusion_path(context, &path) {
            let normalized_path = normalize_repo_relative_path(&path);
            let blob_identity = if normalized_path == normalized_plan_path {
                semantic_plan_blob_identity(&repo, entry.oid.to_string().as_str(), &path)?
            } else {
                entry.oid.to_string()
            };
            semantic_entries.insert(normalized_path, blob_identity);
        }
    }
    SEMANTIC_TREE_ENTRIES_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("semantic tree entries cache lock should not be poisoned")
        .insert(cache_key, semantic_entries.clone());
    Ok(semantic_entries)
}

fn is_runtime_owned_semantic_exclusion_path(context: &ExecutionContext, path: &str) -> bool {
    is_runtime_owned_execution_control_plane_path(context, path)
}

fn semantic_plan_blob_identity(
    repo: &gix::Repository,
    blob_oid: &str,
    path: &str,
) -> Result<String, JsonFailure> {
    let object_id = gix::hash::ObjectId::from_hex(blob_oid.as_bytes()).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Could not parse plan blob id `{blob_oid}` for semantic identity: {error}"),
        )
    })?;
    let blob = repo.find_blob(object_id).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Could not load plan blob `{blob_oid}` for semantic identity: {error}"),
        )
    })?;
    let plan_source = std::str::from_utf8(blob.data.as_ref()).map_err(|_| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Plan blob for `{path}` contained non-utf8 content."),
        )
    })?;
    Ok(format!(
        "semantic_plan:{}",
        sha256_hex(normalized_plan_source_for_semantic_identity(plan_source).as_bytes())
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanSourceNormalizationMode {
    WorkspaceIdentity,
    ApprovedPlanPreflight,
}

pub(crate) fn normalized_plan_source_for_semantic_identity(plan_source: &str) -> String {
    normalized_plan_source(plan_source, PlanSourceNormalizationMode::WorkspaceIdentity)
}

pub(crate) fn normalized_plan_source_for_approved_plan_preflight(plan_source: &str) -> String {
    normalized_plan_source(
        plan_source,
        PlanSourceNormalizationMode::ApprovedPlanPreflight,
    )
}

fn normalized_plan_source(plan_source: &str, mode: PlanSourceNormalizationMode) -> String {
    // Runtime-injected execution headers and control-plane comments must not turn the whole
    // approved plan file into semantic drift.
    let known_runtime_steps = known_runtime_step_projection_lines(plan_source);
    let mut normalizer = PlanSourceProjectionNormalizer::new(mode, known_runtime_steps);
    for line in plan_source.lines() {
        normalizer.push_line(line);
    }
    let normalized = normalizer.finish();
    normalized
        .trim_end_matches('\n')
        .trim_end_matches(|ch: char| ch.is_ascii_whitespace() && ch != '\n')
        .to_owned()
}

struct PlanSourceProjectionNormalizer {
    mode: PlanSourceNormalizationMode,
    known_runtime_steps: BTreeMap<(u32, u32), String>,
    normalized: Vec<String>,
    current_task: Option<u32>,
    current_task_files_seen: bool,
    in_fenced_block: bool,
    pending_runtime_note_after_step: bool,
    skipping_runtime_note_block: Option<RuntimeExecutionNoteProjectionBlock>,
}

impl PlanSourceProjectionNormalizer {
    fn new(
        mode: PlanSourceNormalizationMode,
        known_runtime_steps: BTreeMap<(u32, u32), String>,
    ) -> Self {
        Self {
            mode,
            known_runtime_steps,
            normalized: Vec::new(),
            current_task: None,
            current_task_files_seen: false,
            in_fenced_block: false,
            pending_runtime_note_after_step: false,
            skipping_runtime_note_block: None,
        }
    }

    fn push_line(&mut self, line: &str) {
        if self.consume_runtime_note_block_line(line) {
            return;
        }
        if self.consume_pending_runtime_note_line(line) {
            return;
        }
        if self.update_task_position(line) {
            self.normalized.push(line.to_owned());
            return;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            self.in_fenced_block = !self.in_fenced_block;
            self.normalized.push(line.to_owned());
            return;
        }
        let runtime_owned_comment = trimmed.starts_with("<!--")
            && trimmed.ends_with("-->")
            && (trimmed.contains("runtime-owned")
                || trimmed.contains("featureforge:")
                || trimmed.contains("codex:"));
        let runtime_owned_execution_metadata = matches!(
            trimmed,
            l if l.starts_with("**Execution Mode:**")
                || l.starts_with("**Chunking Strategy:**")
                || l.starts_with("**Evaluator Policy:**")
                || l.starts_with("**Reset Policy:**")
                || l.starts_with("**Review Stack:**")
        );
        if trimmed.starts_with("**Execution Fingerprint:**")
            || runtime_owned_comment
            || (self.mode == PlanSourceNormalizationMode::WorkspaceIdentity
                && runtime_owned_execution_metadata)
        {
            return;
        }
        if let Some(normalized_step) = self.normalize_runtime_step_projection_line(line) {
            self.pending_runtime_note_after_step = true;
            self.normalized.push(normalized_step);
            return;
        }
        self.normalized.push(line.to_owned());
    }

    fn update_task_position(&mut self, line: &str) -> bool {
        if let Some(rest) = line.strip_prefix("## Task ") {
            self.current_task = rest
                .split(':')
                .next()
                .and_then(|value| value.parse::<u32>().ok());
            self.current_task_files_seen = false;
            self.in_fenced_block = false;
            return true;
        }
        if line.starts_with("## ") {
            self.current_task = None;
            self.current_task_files_seen = false;
            self.in_fenced_block = false;
            return false;
        }
        if line.trim() == "**Files:**" {
            self.current_task_files_seen = true;
        }
        false
    }

    fn consume_pending_runtime_note_line(&mut self, line: &str) -> bool {
        if !self.pending_runtime_note_after_step {
            return false;
        }
        if line.trim().is_empty() {
            return true;
        }
        self.pending_runtime_note_after_step = false;
        if let Some(note_block) = RuntimeExecutionNoteProjectionBlock::start(line) {
            self.skipping_runtime_note_block = Some(note_block);
            return true;
        }
        false
    }

    fn consume_runtime_note_block_line(&mut self, line: &str) -> bool {
        if let Some(note_block) = self.skipping_runtime_note_block {
            if note_block.continues(line) {
                return true;
            }
            self.skipping_runtime_note_block = None;
        }
        false
    }

    fn normalize_runtime_step_projection_line(&self, line: &str) -> Option<String> {
        if self.in_fenced_block || !self.current_task_files_seen {
            return None;
        }
        let task_number = self.current_task?;
        let (_, step_number, title) = parse_step_line(line)?;
        let known_title = self.known_runtime_steps.get(&(task_number, step_number))?;
        if known_title != &title {
            return None;
        }
        Some(format!("- [ ] **Step {step_number}: {title}**"))
    }

    fn finish(self) -> String {
        self.normalized.join("\n")
    }
}

fn compute_plan_definition_identity(context: &ExecutionContext) -> String {
    let normalized_plan_source = normalized_plan_source_for_semantic_identity(&context.plan_source);
    let material = format!(
        "plan_path={}\nsource_spec_path={}\nsource_spec_revision={}\nplan_body_hash={}",
        context.plan_rel,
        context.plan_document.source_spec_path,
        context.plan_document.source_spec_revision,
        sha256_hex(normalized_plan_source.as_bytes())
    );
    format!("plan_def:{}", sha256_hex(material.as_bytes()))
}

fn compute_task_definition_identities(
    context: &ExecutionContext,
) -> Result<BTreeMap<u32, String>, JsonFailure> {
    let mut identities = BTreeMap::new();
    for task in &context.plan_document.tasks {
        let serialized = serde_json::to_string(task).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not serialize task {} for semantic identity: {error}",
                    task.number
                ),
            )
        })?;
        identities.insert(
            task.number,
            format!("task_def:{}", sha256_hex(serialized.as_bytes())),
        );
    }
    Ok(identities)
}

fn compute_branch_definition_identity(
    context: &ExecutionContext,
    plan_definition_identity: &str,
) -> Result<String, JsonFailure> {
    let base_branch = context.current_release_base_branch().unwrap_or_default();
    let mut late_stage_surface = normalized_late_stage_surface(&context.plan_source)?;
    late_stage_surface.sort();
    late_stage_surface.dedup();
    let late_stage_surface = late_stage_surface.join("\n");
    let material = format!(
        "plan_def={plan_definition_identity}\nrepo_slug={}\nbranch_name={}\nbase_branch={base_branch}\nlate_stage_surface={late_stage_surface}",
        context.runtime.repo_slug, context.runtime.branch_name
    );
    Ok(format!("branch_def:{}", sha256_hex(material.as_bytes())))
}

fn normalize_repo_relative_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_owned();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use crate::execution::state::hash_contract_plan;

    use super::normalized_plan_source_for_semantic_identity;

    fn valid_task_source(done_when: &str, step_projection: &str) -> String {
        format!(
            "# Plan\n\
## Task 1: Build\n\
**Spec Coverage:** REQ-1\n\
**Goal:** Build the thing.\n\
**Context:**\n\
- The plan has enough context for deterministic execution.\n\
**Constraints:**\n\
- Preserve semantic identity boundaries.\n\
**Done when:**\n\
- {done_when}\n\
**Files:**\n\
- Modify: `src/lib.rs`\n\
{step_projection}"
        )
    }

    #[test]
    fn normalized_plan_source_ignores_runtime_execution_headers_and_comments() {
        let normalized = normalized_plan_source_for_semantic_identity(
            "# Plan\n\
            **Execution Mode:** featureforge:executing-plans\n\
            **Execution Fingerprint:** abc123\n\
            **Late-Stage Surface:** README.md\n\
            <!-- runtime-owned plan mutation -->\n\
            ## Task 1\n\
            keep this line\n",
        );
        assert_eq!(
            normalized,
            "# Plan\n**Late-Stage Surface:** README.md\n## Task 1\nkeep this line"
        );
    }

    #[test]
    fn normalized_plan_source_ignores_trailing_blank_line_left_by_runtime_comment_append() {
        let normalized = normalized_plan_source_for_semantic_identity(
            "# Plan\n\
            ## Task 1\n\
            keep this line\n\
            \n\
            <!-- runtime-owned plan mutation -->\n",
        );
        assert_eq!(normalized, "# Plan\n## Task 1\nkeep this line");
    }

    #[test]
    fn normalized_plan_source_ignores_runtime_step_projection_marks_and_notes() {
        let normalized = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "- [x] **Step 1: Build the thing**\n\n  **Execution Note:** Active - Runtime-owned note.\n- [ ] **Step 2: Verify the thing**\n",
        ));
        assert_eq!(
            normalized,
            valid_task_source(
                "The implementation is verified.",
                "- [ ] **Step 1: Build the thing**\n- [ ] **Step 2: Verify the thing**"
            )
        );
    }

    #[test]
    fn normalized_plan_source_normalizes_indented_runtime_step_projection_marks() {
        let checked = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "  - [x] **Step 6: Build the thing**\n",
        ));
        let unchecked = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "  - [ ] **Step 6: Build the thing**\n",
        ));
        assert_eq!(checked, unchecked);
        assert_eq!(
            checked,
            valid_task_source(
                "The implementation is verified.",
                "- [ ] **Step 6: Build the thing**"
            )
        );
    }

    #[test]
    fn normalized_plan_source_suppresses_multiline_runtime_execution_note_blocks() {
        let without_note = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "- [ ] **Step 1: Build the thing**\n",
        ));
        let with_wrapped_note = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "- [x] **Step 1: Build the thing**\n\n  **Execution Note:** Active - Runtime-owned note starts here\n    and wraps across an indented continuation line.\n",
        ));
        assert_eq!(with_wrapped_note, without_note);
    }

    #[test]
    fn normalized_plan_source_keeps_task_contract_changes_semantic() {
        let baseline = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "original reviewable condition",
            "- [ ] **Step 1: Build the thing**\n",
        ));
        let changed = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "changed reviewable condition",
            "- [ ] **Step 1: Build the thing**\n",
        ));
        assert_ne!(baseline, changed);
    }

    #[test]
    fn normalized_plan_source_keeps_step_shaped_content_outside_runtime_steps_semantic() {
        let checked = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "- [ ] **Step 1: Build the thing**\n```\n  - [x] **Step 1: Build the thing**\n```\n",
        ));
        let unchecked = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "- [ ] **Step 1: Build the thing**\n```\n  - [ ] **Step 1: Build the thing**\n```\n",
        ));
        assert_ne!(checked, unchecked);
        assert!(checked.contains("  - [x] **Step 1: Build the thing**"));
    }

    #[test]
    fn contract_plan_hash_keeps_step_shaped_content_outside_runtime_steps_semantic() {
        let checked = hash_contract_plan(&valid_task_source(
            "The implementation is verified.",
            "- [ ] **Step 1: Build the thing**\n```\n  - [x] **Step 1: Build the thing**\n```\n",
        ));
        let unchecked = hash_contract_plan(&valid_task_source(
            "The implementation is verified.",
            "- [ ] **Step 1: Build the thing**\n```\n  - [ ] **Step 1: Build the thing**\n```\n",
        ));
        assert_ne!(checked, unchecked);
    }

    #[test]
    fn normalized_plan_source_keeps_non_note_indented_content_after_execution_note_semantic() {
        let baseline = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "- [ ] **Step 1: Build the thing**\n",
        ));
        let changed = normalized_plan_source_for_semantic_identity(&valid_task_source(
            "The implementation is verified.",
            "- [x] **Step 1: Build the thing**\n  **Execution Note:** Active - Runtime-owned note.\n  user-authored post-step content\n",
        ));
        assert_ne!(baseline, changed);
        assert!(changed.contains("user-authored post-step content"));
    }

    #[test]
    fn normalized_plan_source_keeps_non_runtime_comments_semantic() {
        let normalized = normalized_plan_source_for_semantic_identity(
            "# Plan\n\
            ## Task 1\n\
            keep this line\n\
            <!-- manual semantic drift -->\n",
        );
        assert_eq!(
            normalized,
            "# Plan\n## Task 1\nkeep this line\n<!-- manual semantic drift -->"
        );
    }
}
