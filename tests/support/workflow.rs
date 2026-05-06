#[path = "plan_fidelity.rs"]
mod plan_fidelity_support;
#[path = "repo_template.rs"]
mod repo_template_support;

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use crate::process_support::repo_root;
pub use plan_fidelity_support::{
    write_current_pass_plan_fidelity_review_artifact,
    write_current_pass_plan_fidelity_review_artifact_for_plan,
};
use serde_json::Value;

pub fn workflow_fixture_root() -> PathBuf {
    repo_root().join("tests/codex-runtime/fixtures/workflow-artifacts")
}

pub fn workflow_fixture_path(relative: &str) -> PathBuf {
    workflow_fixture_root().join(relative)
}

pub fn harness_fixture_path(file_name: &str) -> PathBuf {
    workflow_fixture_path("harness").join(file_name)
}

pub fn copy_workflow_fixture(relative: &str, dest: &Path) {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .expect("fixture destination parent directory should be creatable");
    }
    fs::copy(workflow_fixture_path(relative), dest).expect("workflow fixture should copy");
}

pub fn copy_harness_fixture(file_name: &str, dest: &Path) {
    copy_workflow_fixture(&format!("harness/{file_name}"), dest);
}

pub fn read_harness_fixture_text(file_name: &str) -> String {
    fs::read_to_string(harness_fixture_path(file_name)).expect("harness fixture should load")
}

pub fn read_harness_json_fixture(file_name: &str) -> Value {
    serde_json::from_str(&read_harness_fixture_text(file_name))
        .expect("harness fixture should contain valid json")
}

pub fn init_repo(_name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    repo_template_support::populate_repo_from_template(repo_dir.path());

    (repo_dir, state_dir)
}

pub fn install_full_contract_ready_artifacts(repo: &Path) {
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let spec_path = repo.join(spec_rel);
    let plan_path = repo.join(plan_rel);

    copy_workflow_fixture(
        "specs/2026-03-22-runtime-integration-hardening-design.md",
        &spec_path,
    );

    let plan_source = fs::read_to_string(workflow_fixture_path(
        "plans/2026-03-22-runtime-integration-hardening.md",
    ))
    .expect("full-contract ready plan fixture should load");
    let adjusted_plan = plan_source.replace(
        "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
        spec_rel,
    );
    fs::create_dir_all(
        plan_path
            .parent()
            .expect("plan fixture should have a parent directory"),
    )
    .expect("plan directory should be creatable");
    fs::write(&plan_path, adjusted_plan).expect("full-contract ready plan fixture should write");
    write_current_pass_plan_fidelity_review_artifact(
        repo,
        ".featureforge/reviews/runtime-integration-hardening-plan-fidelity.md",
        plan_rel,
        spec_rel,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_fixture_path_joins_relative_path() {
        let relative = "specs/2026-03-22-runtime-integration-hardening-design.md";
        assert_eq!(
            workflow_fixture_path(relative),
            workflow_fixture_root().join(relative)
        );
    }

    #[test]
    fn harness_fixture_path_targets_harness_subdirectory() {
        let file_name = "valid-execution-contract.md";
        assert_eq!(
            harness_fixture_path(file_name),
            workflow_fixture_root().join("harness").join(file_name)
        );
    }

    #[test]
    fn copy_workflow_fixture_copies_fixture_contents() {
        let tmp = TempDir::new().expect("tempdir should exist");
        let relative = "specs/2026-03-22-runtime-integration-hardening-design.md";
        let dest = tmp.path().join("copied.md");

        copy_workflow_fixture(relative, &dest);

        let expected = fs::read_to_string(workflow_fixture_path(relative))
            .expect("workflow fixture source should read");
        let copied = fs::read_to_string(&dest).expect("copied workflow fixture should read");
        assert_eq!(copied, expected);
    }

    #[test]
    fn copy_harness_fixture_copies_fixture_contents() {
        let tmp = TempDir::new().expect("tempdir should exist");
        let file_name = "valid-execution-contract.md";
        let dest = tmp.path().join("copied-harness.md");

        copy_harness_fixture(file_name, &dest);

        let expected = fs::read_to_string(harness_fixture_path(file_name))
            .expect("harness fixture source should read");
        let copied = fs::read_to_string(&dest).expect("copied harness fixture should read");
        assert_eq!(copied, expected);
    }

    #[test]
    fn read_harness_fixture_text_returns_fixture_contents() {
        let file_name = "valid-evaluation-report.md";
        let expected = fs::read_to_string(harness_fixture_path(file_name))
            .expect("harness fixture source should read");

        assert_eq!(read_harness_fixture_text(file_name), expected);
    }

    #[test]
    fn read_harness_json_fixture_rejects_non_json_fixtures() {
        let failure = std::panic::catch_unwind(|| {
            let _ = read_harness_json_fixture("valid-execution-contract.md");
        })
        .expect_err("non-json harness fixtures should panic when read as JSON");
        let message = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            message.contains("harness fixture should contain valid json"),
            "unexpected panic message: {message}"
        );
    }

    #[test]
    fn install_full_contract_ready_artifacts_installs_expected_files() {
        let (repo_dir, _state_dir) = init_repo("full-contract-ready");
        let repo = repo_dir.path();

        install_full_contract_ready_artifacts(repo);

        let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
        let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
        let review_rel = ".featureforge/reviews/runtime-integration-hardening-plan-fidelity.md";

        let spec_path = repo.join(spec_rel);
        let plan_path = repo.join(plan_rel);
        let review_path = repo.join(review_rel);

        assert!(spec_path.exists(), "spec fixture should be copied");
        assert!(plan_path.exists(), "plan fixture should be copied");
        write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
        assert!(
            review_path.exists(),
            "plan-fidelity fixture should be written"
        );

        let plan_contents =
            fs::read_to_string(plan_path).expect("installed plan fixture should be readable");
        assert!(
            plan_contents.contains(spec_rel),
            "plan fixture should reference repo-relative spec path"
        );
    }
}
