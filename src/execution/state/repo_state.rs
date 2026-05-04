use super::*;

impl ExecutionContext {
    pub(crate) fn current_tracked_tree_sha(&self) -> Result<String, JsonFailure> {
        match self
            .tracked_tree_sha_cache
            .get_or_init(|| current_repo_tracked_tree_sha(&self.runtime.repo_root))
        {
            Ok(tree_sha) => Ok(tree_sha.clone()),
            Err(error) => Err(error.clone()),
        }
    }

    pub(crate) fn repo_has_tracked_worktree_changes_excluding_execution_evidence(
        &self,
    ) -> Result<bool, JsonFailure> {
        match self
            .tracked_worktree_changes_excluding_execution_evidence_cache
            .get_or_init(|| {
                compute_repo_has_tracked_worktree_changes_excluding_execution_evidence(self)
            }) {
            Ok(has_changes) => Ok(*has_changes),
            Err(error) => Err(error.clone()),
        }
    }

    pub(crate) fn cached_reviewed_tree_sha(
        &self,
        reviewed_state_id: &str,
        resolver: impl FnOnce(&Path, &str) -> Result<String, JsonFailure>,
    ) -> Result<String, JsonFailure> {
        if let Some(cached) = self
            .reviewed_tree_sha_cache
            .borrow()
            .get(reviewed_state_id)
            .cloned()
        {
            return Ok(cached);
        }
        let resolved = resolver(&self.runtime.repo_root, reviewed_state_id)?;
        self.reviewed_tree_sha_cache
            .borrow_mut()
            .insert(reviewed_state_id.to_owned(), resolved.clone());
        Ok(resolved)
    }

    pub(crate) fn current_head_sha(&self) -> Result<String, JsonFailure> {
        match self
            .head_sha_cache
            .get_or_init(|| current_head_sha(&self.runtime.repo_root))
        {
            Ok(head_sha) => Ok(head_sha.clone()),
            Err(error) => Err(error.clone()),
        }
    }

    pub(crate) fn current_release_base_branch(&self) -> Option<String> {
        self.release_base_branch_cache
            .get_or_init(|| {
                resolve_release_base_branch(&self.runtime.git_dir, &self.runtime.branch_name)
            })
            .clone()
    }
}

pub fn current_head_sha(repo_root: &Path) -> Result<String, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let head = repo.head_id().map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not determine the current HEAD commit: {error}"),
        )
    })?;
    Ok(head.detach().to_string())
}

pub fn current_tracked_tree_sha(repo_root: &Path) -> Result<String, JsonFailure> {
    current_repo_tracked_tree_sha(repo_root)
}

pub(crate) fn repo_has_non_runtime_projection_tracked_changes(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let mut status_iter = repo
        .status(gix::progress::Discard)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Could not prepare tracked worktree status: {error}"),
            )
        })?
        .untracked_files(gix::status::UntrackedFiles::None)
        .index_worktree_rewrites(None)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Could not determine tracked worktree status: {error}"),
            )
        })?;
    for item in &mut status_iter {
        let item = item.map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Could not inspect tracked worktree change: {error}"),
            )
        })?;
        let path = item.location().to_string();
        if is_runtime_owned_execution_control_plane_path(context, &path) {
            continue;
        }
        if path == context.plan_rel {
            if approved_plan_change_is_clean_for_preflight(context, &path)? {
                continue;
            }
            return Ok(Some(String::from("approved_plan_semantic_drift")));
        }
        return Ok(Some(String::from("tracked_worktree_dirty")));
    }
    Ok(None)
}

fn approved_plan_change_is_projection_only(
    context: &ExecutionContext,
    path: &str,
) -> Result<bool, JsonFailure> {
    approved_plan_sources_match_after_normalization(
        context,
        path,
        normalized_plan_source_for_semantic_identity,
    )
}

fn approved_plan_change_is_clean_for_preflight(
    context: &ExecutionContext,
    path: &str,
) -> Result<bool, JsonFailure> {
    approved_plan_sources_match_after_normalization(
        context,
        path,
        normalized_plan_source_for_approved_plan_preflight,
    )
}

fn approved_plan_sources_match_after_normalization(
    context: &ExecutionContext,
    path: &str,
    normalize: fn(&str) -> String,
) -> Result<bool, JsonFailure> {
    let Some(head_source) = head_blob_source_for_path(&context.runtime.repo_root, path)? else {
        return Ok(false);
    };
    let Some(index_source) = index_blob_source_for_path(&context.runtime.repo_root, path)? else {
        return Ok(false);
    };
    let worktree_source =
        fs::read_to_string(context.runtime.repo_root.join(path)).map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not read approved plan {path} while checking semantic drift: {error}"
                ),
            )
        })?;
    let normalized_head = normalize(&head_source);
    Ok(normalized_head == normalize(&index_source)
        && normalized_head == normalize(&worktree_source))
}

fn head_blob_source_for_path(repo_root: &Path, path: &str) -> Result<Option<String>, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover repository while reading HEAD content for {path}: {error}"),
        )
    })?;
    let tree = repo.head_tree().map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not read HEAD tree while checking semantic drift for {path}: {error}"),
        )
    })?;
    let Some(entry) = tree.lookup_entry_by_path(path).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not inspect HEAD tree path {path}: {error}"),
        )
    })?
    else {
        return Ok(None);
    };
    let blob = repo.find_blob(entry.object_id()).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not load HEAD blob for {path}: {error}"),
        )
    })?;
    std::str::from_utf8(&blob.data)
        .map(|content| Some(content.to_owned()))
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("HEAD content for {path} was not valid UTF-8: {error}"),
            )
        })
}

fn index_blob_source_for_path(repo_root: &Path, path: &str) -> Result<Option<String>, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!(
                "Could not discover repository while reading index content for {path}: {error}"
            ),
        )
    })?;
    let index = repo.open_index().map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!(
                "Could not open repository index while checking semantic drift for {path}: {error}"
            ),
        )
    })?;
    let Some(entry) = index.entry_by_path(path.as_bytes().as_bstr()) else {
        return Ok(None);
    };
    if entry.stage_raw() != 0 {
        return Ok(None);
    }
    let blob = repo.find_blob(entry.id).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not load index blob for {path}: {error}"),
        )
    })?;
    std::str::from_utf8(&blob.data)
        .map(|content| Some(content.to_owned()))
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Index content for {path} was not valid UTF-8: {error}"),
            )
        })
}

fn compute_repo_has_tracked_worktree_changes_excluding_execution_evidence(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let mut status_iter = repo
        .status(gix::progress::Discard)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not prepare tracked worktree status while filtering execution evidence: {error}"
                ),
            )
        })?
        .untracked_files(gix::status::UntrackedFiles::None)
        .index_worktree_rewrites(None)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not determine whether tracked worktree changes remain outside execution evidence: {error}"
                ),
            )
        })?;
    for item in &mut status_iter {
        let item = item.map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not determine whether tracked worktree changes remain outside execution evidence: {error}"
                ),
            )
        })?;
        let path = item.location().to_string();
        if is_runtime_owned_execution_control_plane_path(context, &path) {
            continue;
        }
        if path == context.plan_rel && approved_plan_change_is_projection_only(context, &path)? {
            continue;
        }
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

pub(crate) fn repo_head_detached(context: &ExecutionContext) -> Result<bool, HeadError> {
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| HeadError {
        message: format!("Could not discover the current repository: {error}"),
    })?;
    let head = repo.head().map_err(|error| HeadError {
        message: format!("Could not determine the current branch: {error}"),
    })?;
    Ok(head.is_detached())
}

#[derive(Debug)]
pub(crate) struct HeadError {
    pub(crate) message: String,
}

pub(crate) fn repo_safety_stage(context: &ExecutionContext) -> String {
    match context.plan_document.execution_mode.as_str() {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
            context.plan_document.execution_mode.clone()
        }
        _ => String::from("featureforge:execution-preflight"),
    }
}

pub(crate) fn repo_safety_preflight_message(
    result: &crate::repo_safety::RepoSafetyResult,
) -> String {
    match result.failure_class.as_str() {
        "ProtectedBranchDetected" => format!(
            "Execution preflight cannot continue on protected branch {} without explicit approval.",
            result.branch
        ),
        "ApprovalScopeMismatch" => String::from(
            "Execution preflight repo-safety approval does not match the current scope.",
        ),
        "ApprovalFingerprintMismatch" => String::from(
            "Execution preflight repo-safety approval does not match the current branch or write scope.",
        ),
        _ => String::from("Execution preflight is blocked by repo-safety policy."),
    }
}

pub(crate) fn repo_safety_preflight_remediation(
    result: &crate::repo_safety::RepoSafetyResult,
) -> String {
    if !result.suggested_next_skill.is_empty() {
        format!(
            "Use {} or explicitly approve the protected-branch execution scope before continuing.",
            result.suggested_next_skill
        )
    } else {
        String::from("Resolve the repo-safety blocker before continuing execution.")
    }
}

pub(crate) fn repo_has_unresolved_index_entries(repo_root: &Path) -> Result<bool, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let index = repo.open_index().map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not open the repository index: {error}"),
        )
    })?;
    Ok(index
        .entries()
        .iter()
        .any(|entry| entry.stage() != gix::index::entry::Stage::Unconflicted))
}
