use std::fs;
use std::path::Path;

use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::manifest::{
    ManifestLoadResult, load_manifest_read_only, manifest_path,
};

pub fn discover_workflow_plan_rel(runtime: &ExecutionRuntime, context: &str) -> String {
    if let Some(plan) = manifest_expected_plan(runtime).filter(|plan| !plan.trim().is_empty()) {
        return plan;
    }

    let mut plans = Vec::new();
    collect_plan_files(
        &runtime.repo_root,
        &runtime.repo_root.join("docs/featureforge/plans"),
        &mut plans,
    );
    plans.sort();

    let full_contract_plan = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    if plans.iter().any(|plan| plan == full_contract_plan) {
        return full_contract_plan.to_owned();
    }
    plans.into_iter().next().unwrap_or_else(|| {
        panic!("{context}: workflow doctor public CLI requires --plan but no fixture plan exists")
    })
}

fn manifest_expected_plan(runtime: &ExecutionRuntime) -> Option<String> {
    let identity = featureforge::git::discover_repo_identity(&runtime.repo_root).ok()?;
    match load_manifest_read_only(&manifest_path(&identity, &runtime.state_dir)) {
        ManifestLoadResult::Loaded(manifest) => Some(manifest.expected_plan_path),
        ManifestLoadResult::Missing | ManifestLoadResult::Corrupt { .. } => None,
    }
}

fn collect_plan_files(repo_root: &Path, dir: &Path, plans: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_plan_files(repo_root, &path, plans);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let Ok(rel) = path.strip_prefix(repo_root) else {
            continue;
        };
        plans.push(path_to_slash_string(rel));
    }
}

fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => segment.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}
