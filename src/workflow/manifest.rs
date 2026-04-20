use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{RepositoryIdentity, derive_repo_slug, stored_repo_root_matches_current};
use crate::paths::{branch_storage_key, write_atomic as write_atomic_file};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
/// Runtime struct.
pub struct WorkflowManifest {
    /// Runtime field.
    pub version: u32,
    /// Runtime field.
    pub repo_root: String,
    /// Runtime field.
    pub branch: String,
    /// Runtime field.
    pub expected_spec_path: String,
    /// Runtime field.
    pub expected_plan_path: String,
    /// Runtime field.
    pub status: String,
    /// Runtime field.
    pub next_skill: String,
    /// Runtime field.
    pub reason: String,
    /// Runtime field.
    pub note: String,
    /// Runtime field.
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Runtime enum.
pub enum ManifestLoadResult {
    /// Runtime enum variant.
    Missing,
    /// Runtime enum variant.
    Loaded(WorkflowManifest),
    /// Runtime enum variant.
    Corrupt {
        /// Runtime field.
        backup_path: PathBuf,
    },
}

const CROSS_SLUG_RECOVERY_LIMIT: usize = 12;

#[must_use]
/// Runtime function.
pub fn manifest_path(identity: &RepositoryIdentity, state_dir: &Path) -> PathBuf {
    let slug = derive_repo_slug(&identity.repo_root, identity.remote_url.as_deref());
    let safe_branch = branch_storage_key(&identity.branch_name);
    let user_name = env::var("USER").unwrap_or_else(|_| String::from("user"));
    state_dir
        .join("projects")
        .join(slug)
        .join(format!("{user_name}-{safe_branch}-workflow-state.json"))
}

#[must_use]
/// Runtime function.
pub fn load_manifest(path: &Path) -> ManifestLoadResult {
    let Ok(source) = fs::read_to_string(path) else {
        return ManifestLoadResult::Missing;
    };
    serde_json::from_str(&source).map_or_else(
        |_| {
            let backup_path = corrupt_backup_path(path);
            let _ = fs::rename(path, &backup_path);
            ManifestLoadResult::Corrupt { backup_path }
        },
        ManifestLoadResult::Loaded,
    )
}

#[must_use]
/// Runtime function.
pub fn load_manifest_read_only(path: &Path) -> ManifestLoadResult {
    let Ok(source) = fs::read_to_string(path) else {
        return ManifestLoadResult::Missing;
    };
    serde_json::from_str(&source).map_or_else(
        |_| ManifestLoadResult::Corrupt {
            backup_path: corrupt_backup_path(path),
        },
        ManifestLoadResult::Loaded,
    )
}

/// Runtime function.
pub fn recover_slug_changed_manifest(
    identity: &RepositoryIdentity,
    state_dir: &Path,
    current_manifest_path: &Path,
) -> Option<WorkflowManifest> {
    recover_slug_changed_manifest_with_loader(
        identity,
        state_dir,
        current_manifest_path,
        load_manifest,
    )
}

/// Runtime function.
pub fn recover_slug_changed_manifest_read_only(
    identity: &RepositoryIdentity,
    state_dir: &Path,
    current_manifest_path: &Path,
) -> Option<WorkflowManifest> {
    recover_slug_changed_manifest_with_loader(
        identity,
        state_dir,
        current_manifest_path,
        load_manifest_read_only,
    )
}

fn recover_slug_changed_manifest_with_loader(
    identity: &RepositoryIdentity,
    state_dir: &Path,
    current_manifest_path: &Path,
    loader: fn(&Path) -> ManifestLoadResult,
) -> Option<WorkflowManifest> {
    let projects_dir = state_dir.join("projects");
    let manifest_name = current_manifest_path.file_name()?;
    let current_project_dir = current_manifest_path.parent();
    let mut candidate_dirs = fs::read_dir(&projects_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| current_project_dir != Some(path.as_path()))
        .collect::<Vec<_>>();
    candidate_dirs.sort();

    for project_dir in candidate_dirs.into_iter().take(CROSS_SLUG_RECOVERY_LIMIT) {
        let candidate_path = project_dir.join(manifest_name);
        let ManifestLoadResult::Loaded(manifest) = loader(&candidate_path) else {
            continue;
        };
        if stored_repo_root_matches_current(&manifest.repo_root, &identity.repo_root)
            && manifest.branch == identity.branch_name
        {
            return Some(manifest);
        }
    }

    None
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn save_manifest(path: &Path, manifest: &WorkflowManifest) -> std::io::Result<()> {
    let payload = serde_json::to_string(manifest).map_err(|error| {
        std::io::Error::other(format!(
            "workflow manifest serialization failed for {}: {error}",
            path.display()
        ))
    })?;
    write_atomic_file(path, payload)
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("workflow-state.json");
    path.with_file_name(format!("{file_name}.corrupt-{stamp}"))
}
