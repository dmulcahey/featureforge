use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::git::RepositoryIdentity;
use crate::paths::normalize_identifier_token;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkflowManifest {
    pub version: u32,
    pub repo_root: String,
    pub branch: String,
    pub expected_spec_path: String,
    pub expected_plan_path: String,
    pub status: String,
    pub next_skill: String,
    pub reason: String,
    pub note: String,
    pub updated_at: String,
}

pub fn manifest_path(identity: &RepositoryIdentity, state_dir: &Path) -> PathBuf {
    let slug = derive_repo_slug(identity);
    let safe_branch = normalize_identifier_token(&identity.branch_name);
    let user_name = env::var("USER").unwrap_or_else(|_| String::from("user"));
    state_dir
        .join("projects")
        .join(slug)
        .join(format!("{user_name}-{safe_branch}-workflow-state.json"))
}

pub fn load_manifest(path: &Path) -> Option<WorkflowManifest> {
    let source = fs::read_to_string(path).ok()?;
    serde_json::from_str(&source).ok()
}

pub fn save_manifest(path: &Path, manifest: &WorkflowManifest) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_string(manifest)
        .expect("workflow manifest serialization should stay valid json");
    fs::write(path, payload)
}

fn derive_repo_slug(identity: &RepositoryIdentity) -> String {
    if let Some(remote) = identity.remote_url.as_deref() {
        let normalized = remote.trim_end_matches(".git").replace(':', "/");
        let parts = normalized
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if let [.., owner, repo] = parts.as_slice() {
            return format!("{owner}-{repo}");
        }
    }

    let repo_name = identity
        .repo_root
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("repo");
    let digest = Sha256::digest(identity.repo_root.to_string_lossy().as_bytes());
    let suffix = format!("{digest:x}");
    format!("{repo_name}-{}", &suffix[..12])
}
