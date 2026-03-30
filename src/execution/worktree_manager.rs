use std::path::{Path, PathBuf};

use crate::paths::{featureforge_home_dir, normalize_identifier_token};

pub fn recommended_worktree_root_for_repo(repo_root: &Path) -> PathBuf {
    let repo_root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());

    for candidate in [repo_root.join(".worktrees"), repo_root.join("worktrees")] {
        if candidate.exists() {
            return candidate;
        }
    }

    let project_name = repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(normalize_identifier_token)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| String::from("repo"));

    featureforge_home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".config")
        .join("featureforge")
        .join("worktrees")
        .join(project_name)
}