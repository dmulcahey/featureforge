use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use sha2::{Digest, Sha256};

use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::paths::branch_storage_key;

type CommitFingerprintCache = RwLock<HashMap<(PathBuf, String), String>>;
type AncestorCache = RwLock<HashMap<(PathBuf, String, String), bool>>;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Runtime struct.
pub struct RepositoryIdentity {
    /// Runtime field.
    pub repo_root: PathBuf,
    /// Runtime field.
    pub remote_url: Option<String>,
    /// Runtime field.
    pub branch_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Runtime struct.
pub struct RepositoryContext {
    /// Runtime field.
    pub identity: RepositoryIdentity,
    /// Runtime field.
    pub git_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Runtime struct.
pub struct SlugIdentity {
    /// Runtime field.
    pub repo_root: PathBuf,
    /// Runtime field.
    pub remote_url: Option<String>,
    /// Runtime field.
    pub branch_name: String,
    /// Runtime field.
    pub repo_slug: String,
    /// Runtime field.
    pub safe_branch: String,
}

fn fallback_slug_identity(start_dir: &Path) -> SlugIdentity {
    let repo_root = canonicalize_repo_root_path(start_dir);
    let branch_name = String::from("current");
    SlugIdentity {
        repo_slug: derive_repo_slug(&repo_root, None),
        safe_branch: String::from("current"),
        repo_root,
        remote_url: None,
        branch_name,
    }
}

fn slug_identity_from_repo_identity(identity: RepositoryIdentity) -> SlugIdentity {
    let safe_branch = branch_storage_key(&identity.branch_name);
    SlugIdentity {
        repo_slug: derive_repo_slug(&identity.repo_root, identity.remote_url.as_deref()),
        safe_branch,
        repo_root: identity.repo_root,
        remote_url: identity.remote_url,
        branch_name: identity.branch_name,
    }
}

#[must_use]
/// Runtime function.
pub fn canonicalize_repo_root_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[must_use]
/// Runtime function.
pub fn canonicalize_repo_root_string(path: &Path) -> String {
    canonicalize_repo_root_path(path)
        .to_string_lossy()
        .into_owned()
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn discover_repository(start_dir: &Path) -> Result<gix::Repository, Box<gix::discover::Error>> {
    gix::discover(start_dir).map_err(Box::new)
}

fn commit_fingerprint_cache() -> &'static CommitFingerprintCache {
    static COMMIT_FINGERPRINT_CACHE: OnceLock<CommitFingerprintCache> = OnceLock::new();
    COMMIT_FINGERPRINT_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn ancestor_cache() -> &'static AncestorCache {
    static ANCESTOR_CACHE: OnceLock<AncestorCache> = OnceLock::new();
    ANCESTOR_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn rwlock_read_recover<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn rwlock_write_recover<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[must_use]
/// Runtime function.
pub fn commit_object_fingerprint(repo_root: &Path, commit_sha: &str) -> Option<String> {
    let key = (repo_root.to_path_buf(), commit_sha.trim().to_owned());
    {
        let cache = rwlock_read_recover(commit_fingerprint_cache());
        if let Some(fingerprint) = cache.get(&key).cloned() {
            return Some(fingerprint);
        }
    }

    let repo = discover_repository(repo_root).ok()?;
    let object_id = gix::hash::ObjectId::from_hex(commit_sha.trim().as_bytes()).ok()?;
    let commit = repo.find_commit(object_id).ok()?;
    let fingerprint = sha256_hex(commit.data.as_slice());
    rwlock_write_recover(commit_fingerprint_cache()).insert(key, fingerprint.clone());
    Some(fingerprint)
}

#[must_use]
/// Runtime function.
pub fn is_ancestor_commit(repo_root: &Path, ancestor: &str, descendant: &str) -> bool {
    let key = (
        repo_root.to_path_buf(),
        ancestor.trim().to_owned(),
        descendant.trim().to_owned(),
    );
    {
        let cache = rwlock_read_recover(ancestor_cache());
        if let Some(cached) = cache.get(&key).copied() {
            return cached;
        }
    }

    let result = discover_repository(repo_root)
        .ok()
        .and_then(|repo| {
            let ancestor_id = gix::hash::ObjectId::from_hex(ancestor.trim().as_bytes()).ok()?;
            let descendant_id = gix::hash::ObjectId::from_hex(descendant.trim().as_bytes()).ok()?;
            let merge_base = repo.merge_base(ancestor_id, descendant_id).ok()?;
            Some(merge_base.detach() == ancestor_id)
        })
        .unwrap_or(false);
    rwlock_write_recover(ancestor_cache()).insert(key, result);
    result
}

#[must_use]
/// Runtime function.
pub fn stored_repo_root_matches_current(stored_repo_root: &str, current_repo_root: &Path) -> bool {
    if stored_repo_root.is_empty() {
        return false;
    }
    let current = canonicalize_repo_root_string(current_repo_root);
    if stored_repo_root == current {
        return true;
    }
    let stored_path = Path::new(stored_repo_root);
    stored_path.is_absolute() && canonicalize_repo_root_string(stored_path) == current
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn discover_repo_identity(start_dir: &Path) -> Result<RepositoryIdentity, DiagnosticError> {
    discover_repo_context(start_dir).map(|context| context.identity)
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn discover_repo_context(start_dir: &Path) -> Result<RepositoryContext, DiagnosticError> {
    let repo = discover_repository(start_dir).map_err(|err| {
        DiagnosticError::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not discover the current repository: {err}"),
        )
    })?;
    repo_context_from_repository(&repo)
}

/// Runtime function.
pub fn discover_slug_identity(start_dir: &Path) -> SlugIdentity {
    discover_repo_identity(start_dir).map_or_else(
        |_| fallback_slug_identity(start_dir),
        slug_identity_from_repo_identity,
    )
}

#[must_use]
/// Runtime function.
pub fn discover_slug_identity_and_head(start_dir: &Path) -> (SlugIdentity, Option<String>) {
    let Ok(repo) = discover_repository(start_dir) else {
        return (fallback_slug_identity(start_dir), None);
    };
    let head_sha = repo.head_id().ok().map(|head| head.detach().to_string());
    let Ok(context) = repo_context_from_repository(&repo) else {
        return (fallback_slug_identity(start_dir), None);
    };
    (slug_identity_from_repo_identity(context.identity), head_sha)
}

fn repo_context_from_repository(
    repo: &gix::Repository,
) -> Result<RepositoryContext, DiagnosticError> {
    let head = repo.head().map_err(|err| {
        DiagnosticError::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not determine the current branch: {err}"),
        )
    })?;

    let repo_root = repo
        .workdir()
        .map_or_else(|| repo.path().to_path_buf(), Path::to_path_buf);
    let repo_root = canonicalize_repo_root_path(&repo_root);
    let branch_name = if head.is_detached() {
        String::from("current")
    } else {
        head.referent_name()
            .map(|name| name.shorten().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| String::from("current"))
    };
    let remote_url = repo.find_remote("origin").ok().and_then(|remote| {
        remote
            .url(gix::remote::Direction::Fetch)
            .map(ToString::to_string)
    });

    Ok(RepositoryContext {
        identity: RepositoryIdentity {
            repo_root,
            remote_url,
            branch_name,
        },
        git_dir: repo.path().to_path_buf(),
    })
}

/// Runtime function.
pub fn derive_repo_slug(repo_root: &Path, remote_url: Option<&str>) -> String {
    if let Some(remote_url) = remote_url
        && let Some(slug) = slug_from_remote(remote_url)
    {
        return slug;
    }

    let repo_name = repo_root
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("repo");
    format!("{repo_name}-{}", hash_repo_root(repo_root))
}

fn slug_from_remote(remote_url: &str) -> Option<String> {
    let trimmed = remote_url.trim_end_matches(".git");
    let mut parts = trimmed
        .split(['/', ':'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let repo = parts.pop()?;
    let owner = parts.pop()?;
    Some(format!("{owner}-{repo}"))
}

#[must_use]
/// Runtime function.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

#[must_use]
/// Runtime function.
pub fn short_sha256_hex(bytes: &[u8], width: usize) -> String {
    sha256_hex(bytes)[..width].to_owned()
}

fn hash_repo_root(repo_root: &Path) -> String {
    short_sha256_hex(repo_root.to_string_lossy().as_bytes(), 12)
}
