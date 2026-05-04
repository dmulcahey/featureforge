use std::path::{Path, PathBuf};

use crate::diagnostics::JsonFailure;
use crate::git::{derive_repo_slug, discover_repo_context};
use crate::paths::{branch_storage_key, featureforge_state_dir};

#[derive(Debug, Clone)]
pub struct ExecutionRuntime {
    pub repo_root: PathBuf,
    pub git_dir: PathBuf,
    pub branch_name: String,
    pub repo_slug: String,
    pub safe_branch: String,
    pub state_dir: PathBuf,
}

impl ExecutionRuntime {
    pub fn discover(current_dir: &Path) -> Result<Self, JsonFailure> {
        let context = discover_repo_context(current_dir).map_err(JsonFailure::from)?;
        let identity = context.identity;

        Ok(Self {
            repo_root: identity.repo_root.clone(),
            git_dir: context.git_dir,
            branch_name: identity.branch_name.clone(),
            repo_slug: derive_repo_slug(&identity.repo_root, identity.remote_url.as_deref()),
            safe_branch: branch_storage_key(&identity.branch_name),
            state_dir: state_dir(),
        })
    }
}

pub fn state_dir() -> PathBuf {
    featureforge_state_dir()
}
