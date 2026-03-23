use std::path::{Path, PathBuf};

use crate::diagnostics::{DiagnosticError, FailureClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIdentity {
    pub repo_root: PathBuf,
    pub remote_url: Option<String>,
    pub branch_name: String,
}

pub fn discover_repo_identity(start_dir: &Path) -> Result<RepositoryIdentity, DiagnosticError> {
    let repo = gix::discover(start_dir).map_err(|err| {
        DiagnosticError::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not discover the current repository: {err}"),
        )
    })?;
    let head = repo.head().map_err(|err| {
        DiagnosticError::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not determine the current branch: {err}"),
        )
    })?;

    let repo_root = repo
        .workdir()
        .map_or_else(|| repo.path().to_path_buf(), Path::to_path_buf);
    let branch_name = if head.is_detached() {
        String::from("current")
    } else {
        head.referent_name()
            .map(|name| name.shorten().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| String::from("current"))
    };
    let remote_url = repo
        .find_remote("origin")
        .ok()
        .and_then(|remote| remote.url(gix::remote::Direction::Fetch).map(|url| url.to_string()));

    Ok(RepositoryIdentity {
        repo_root,
        remote_url,
        branch_name,
    })
}
