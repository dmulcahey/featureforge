use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::execution::leases::load_status_authoritative_overlay;
use crate::execution::state::ExecutionContext;
use crate::paths::harness_authoritative_artifacts_dir;

#[derive(Debug, Default, Clone)]
pub(crate) struct ArtifactDocument {
    pub(crate) title: Option<String>,
    pub(crate) headers: BTreeMap<String, String>,
}

pub(crate) fn parse_artifact_document(path: &Path) -> ArtifactDocument {
    let Ok(source) = fs::read_to_string(path) else {
        return ArtifactDocument::default();
    };
    ArtifactDocument {
        title: source
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned),
        headers: parse_headers(&source),
    }
}

pub(crate) fn resolve_release_base_branch(git_dir: &Path, current_branch: &str) -> Option<String> {
    const COMMON_BASE_BRANCHES: &[&str] = &["main", "master", "develop", "dev", "trunk"];

    if COMMON_BASE_BRANCHES.contains(&current_branch) {
        return Some(current_branch.to_owned());
    }

    if let Some(branch) = branch_merge_base_from_config(git_dir, current_branch) {
        return Some(branch);
    }
    if let Some(branch) = origin_head_branch(git_dir) {
        return Some(branch);
    }

    let branches = local_head_branches(&git_dir.join("refs/heads"));
    for candidate in COMMON_BASE_BRANCHES {
        if branches.iter().any(|branch| branch == candidate) {
            return Some((*candidate).to_owned());
        }
    }

    let mut non_current = branches
        .into_iter()
        .filter(|branch| branch != current_branch)
        .collect::<Vec<_>>();
    non_current.sort();
    non_current.dedup();
    if non_current.len() == 1 {
        return non_current.pop();
    }
    None
}

pub(crate) fn latest_branch_artifact_path(
    artifact_dir: &Path,
    branch_name: &str,
    kind: &str,
) -> Option<PathBuf> {
    let entries = fs::read_dir(artifact_dir).ok()?;
    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("md"))
        .filter(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| {
                    name.strip_suffix(".md")
                        .and_then(|stem| stem.rsplit_once(&format!("-{kind}-")))
                        .is_some_and(|(_, timestamp)| !timestamp.is_empty())
                })
        })
        .filter(|path| {
            parse_artifact_document(path)
                .headers
                .get("Branch")
                .is_some_and(|value| value == branch_name)
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

pub(crate) fn authoritative_final_review_artifact_path(
    context: &ExecutionContext,
) -> Option<PathBuf> {
    let overlay = load_status_authoritative_overlay(context)?;
    authoritative_fingerprinted_artifact_path(
        context,
        overlay.final_review_state.as_deref(),
        overlay.last_final_review_artifact_fingerprint.as_deref(),
        "final-review",
    )
}

pub(crate) fn authoritative_browser_qa_artifact_path(
    context: &ExecutionContext,
) -> Option<PathBuf> {
    let overlay = load_status_authoritative_overlay(context)?;
    authoritative_fingerprinted_artifact_path(
        context,
        overlay.browser_qa_state.as_deref(),
        overlay.last_browser_qa_artifact_fingerprint.as_deref(),
        "browser-qa",
    )
}

pub(crate) fn authoritative_release_docs_artifact_path(
    context: &ExecutionContext,
) -> Option<PathBuf> {
    let overlay = load_status_authoritative_overlay(context)?;
    authoritative_fingerprinted_artifact_path(
        context,
        overlay.release_docs_state.as_deref(),
        overlay.last_release_docs_artifact_fingerprint.as_deref(),
        "release-docs",
    )
}

pub(crate) fn authoritative_test_plan_artifact_path_from_qa(
    qa_artifact_path: &Path,
) -> Option<PathBuf> {
    let qa = parse_artifact_document(qa_artifact_path);
    let source_test_plan = qa
        .headers
        .get("Source Test Plan")
        .map(|value| strip_backticks(value))?;
    let source_test_plan = source_test_plan.trim();
    if source_test_plan.is_empty() {
        return None;
    }

    let source_test_plan_path = PathBuf::from(source_test_plan);
    let resolved_path = if source_test_plan_path.is_absolute() {
        source_test_plan_path
    } else {
        qa_artifact_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(source_test_plan_path)
    };
    resolved_path.is_file().then_some(resolved_path)
}

fn parse_headers(source: &str) -> BTreeMap<String, String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("**")?;
            let (key, value) = rest.split_once(":** ")?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn branch_merge_base_from_config(git_dir: &Path, current_branch: &str) -> Option<String> {
    let source = fs::read_to_string(git_dir.join("config")).ok()?;
    let target_section = format!(r#"[branch "{current_branch}"]"#);
    let mut in_target_section = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_target_section = trimmed == target_section;
            continue;
        }
        if !in_target_section
            || trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with(';')
        {
            continue;
        }
        let (key, value) = trimmed.split_once('=')?;
        if key.trim() == "gh-merge-base" {
            let normalized = value.trim();
            if !normalized.is_empty() {
                return Some(normalized.to_owned());
            }
        }
    }

    None
}

fn origin_head_branch(git_dir: &Path) -> Option<String> {
    let source = fs::read_to_string(git_dir.join("refs/remotes/origin/HEAD")).ok()?;
    let reference = source.trim().strip_prefix("ref: ")?;
    let branch = reference.strip_prefix("refs/remotes/origin/")?.trim();
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_owned())
    }
}

fn local_head_branches(heads_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(heads_dir) else {
        return Vec::new();
    };
    let mut branches = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            branches.extend(local_head_branches(&path).into_iter().filter_map(|branch| {
                path.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .map(|prefix| format!("{prefix}/{branch}"))
            }));
            continue;
        }
        if let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) {
            branches.push(name.to_owned());
        }
    }
    branches
}

fn authoritative_fingerprinted_artifact_path(
    context: &ExecutionContext,
    freshness_state: Option<&str>,
    fingerprint: Option<&str>,
    artifact_prefix: &str,
) -> Option<PathBuf> {
    let freshness_state = normalize_optional_overlay_value(freshness_state)?;
    if freshness_state != "fresh" {
        return None;
    }

    let fingerprint = fingerprint.map(str::trim).filter(|value| !value.is_empty())?;
    if fingerprint.len() != 64 || !fingerprint.chars().all(|value| value.is_ascii_hexdigit()) {
        return None;
    }

    let path = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    )
    .join(format!("{artifact_prefix}-{fingerprint}.md"));
    path.is_file().then_some(path)
}

fn normalize_optional_overlay_value(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "unknown")
}

fn strip_backticks(value: &str) -> String {
    value.trim_matches('`').to_owned()
}
