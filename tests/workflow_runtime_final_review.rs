#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/json.rs"]
mod json_support;
#[path = "support/process.rs"]
mod process_support;

use assert_cmd::cargo::CommandCargoExt;
use bin_support::compiled_featureforge_path;
use featureforge::paths::branch_storage_key;
use files_support::write_file;
use json_support::parse_json;
use process_support::{run, run_checked};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-17-example-execution-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-17-example-execution-plan-design.md";

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    run_checked(
        {
            let mut command = Command::new("git");
            command.arg("init").current_dir(repo);
            command
        },
        "git init",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["config", "user.name", "FeatureForge Test"])
                .current_dir(repo);
            command
        },
        "git config user.name",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["config", "user.email", "featureforge-tests@example.com"])
                .current_dir(repo);
            command
        },
        "git config user.email",
    );
    write_file(&repo.join("README.md"), &format!("# {name}\n"));
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["add", "README.md"]).current_dir(repo);
            command
        },
        "git add README",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["commit", "-m", "init"]).current_dir(repo);
            command
        },
        "git commit init",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["checkout", "-B", "fixture-work"]).current_dir(repo);
            command
        },
        "git checkout fixture-work",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["remote", "add", "origin", &format!("git@github.com:example/{name}.git")])
                .current_dir(repo);
            command
        },
        "git remote add origin",
    );

    (repo_dir, state_dir)
}

fn write_approved_spec(repo: &Path) {
    write_file(
        &repo.join(SPEC_REL),
        r#"# Example Execution Plan Design

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Requirement Index

- [REQ-001][behavior] Execution fixtures must support a valid single-task plan path for routing and finish-gate coverage.

## Summary

Fixture spec for focused execution-helper regression coverage.
"#,
    );
}

fn write_single_step_plan(repo: &Path, execution_mode: &str) {
    write_file(
        &repo.join(PLAN_REL),
        &format!(
            r#"# Example Execution Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** {execution_mode}
**Source Spec:** `{SPEC_REL}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1

## Execution Strategy

- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for downstream review routing.

## Dependency Diagram

```text
Task 1
```

## Task 1: Single Step Task

**Spec Coverage:** REQ-001
**Task Outcome:** The workspace is prepared for execution.
**Plan Constraints:**
- Keep the fixture single-step and deterministic.
**Open Questions:** none

**Files:**
- Modify: `docs/example-output.md`

- [ ] **Step 1: Prepare the workspace for execution**
"#
        ),
    );
}

fn mark_all_plan_steps_checked(repo: &Path) {
    let path = repo.join(PLAN_REL);
    let source = fs::read_to_string(&path).expect("plan should be readable");
    fs::write(path, source.replace("- [ ]", "- [x]")).expect("plan should be writable");
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn current_head_sha(repo: &Path) -> String {
    let output = run_checked(
        {
            let mut command = Command::new("git");
            command.args(["rev-parse", "HEAD"]).current_dir(repo);
            command
        },
        "git rev-parse HEAD",
    );
    String::from_utf8(output.stdout)
        .expect("head sha should be utf-8")
        .trim()
        .to_owned()
}

fn branch_name(repo: &Path) -> String {
    let output = run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(repo);
            command
        },
        "git rev-parse branch",
    );
    String::from_utf8(output.stdout)
        .expect("branch should be utf-8")
        .trim()
        .to_owned()
}

fn expected_base_branch(repo: &Path) -> String {
    let current = branch_name(repo);
    let output = run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
                .current_dir(repo);
            command
        },
        "git for-each-ref refs/heads",
    );
    let mut branches = String::from_utf8(output.stdout)
        .expect("branch list should be utf-8")
        .lines()
        .map(str::trim)
        .filter(|branch| !branch.is_empty() && *branch != current)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    branches.sort();
    branches.dedup();
    if branches.len() == 1 {
        return branches.remove(0);
    }
    current
}

fn repo_slug(repo: &Path) -> String {
    let output = run_checked(
        {
            let mut command =
                Command::cargo_bin("featureforge").expect("featureforge binary should exist");
            command.current_dir(repo).args(["repo", "slug"]);
            command
        },
        "featureforge repo slug",
    );
    String::from_utf8(output.stdout)
        .expect("repo slug output should be utf-8")
        .lines()
        .find_map(|line| line.strip_prefix("SLUG="))
        .unwrap_or_else(|| panic!("repo slug output should include SLUG=..."))
        .to_owned()
}

fn project_artifact_dir(repo: &Path, state: &Path) -> PathBuf {
    state.join("projects").join(repo_slug(repo))
}

fn execution_contract_plan_hash(repo: &Path) -> String {
    let source = fs::read_to_string(repo.join(PLAN_REL)).expect("plan should be readable");
    let mut output = Vec::new();
    for line in source.lines() {
        if line.starts_with("**Execution Mode:** ") {
            output.push(String::from("**Execution Mode:** none"));
            continue;
        }
        if line.starts_with("- [x]") {
            output.push(line.replacen("- [x]", "- [ ]", 1));
            continue;
        }
        output.push(line.to_owned());
    }
    sha256_hex(format!("{}\n", output.join("\n")).as_bytes())
}

fn expected_packet_fingerprint(repo: &Path, task: u32, step: u32) -> String {
    let plan_fingerprint = execution_contract_plan_hash(repo);
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable"));
    let payload = format!(
        "plan_path={PLAN_REL}\nplan_revision=1\nplan_fingerprint={plan_fingerprint}\nsource_spec_path={SPEC_REL}\nsource_spec_revision=1\nsource_spec_fingerprint={spec_fingerprint}\ntask_number={task}\nstep_number={step}\n"
    );
    sha256_hex(payload.as_bytes())
}

fn write_single_step_v2_completed_attempt(repo: &Path, packet_fingerprint: &str) {
    let evidence_path =
        repo.join("docs/featureforge/execution-evidence/2026-03-17-example-execution-plan-r1-evidence.md");
    let plan_fingerprint =
        sha256_hex(&fs::read(repo.join(PLAN_REL)).expect("plan should be readable"));
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable"));
    write_file(&repo.join("docs/example-output.md"), "verified output\n");
    let file_digest = sha256_hex(
        &fs::read(repo.join("docs/example-output.md")).expect("output should be readable"),
    );
    let head_sha = current_head_sha(repo);
    write_file(
        &evidence_path,
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {PLAN_REL}\n**Plan Revision:** 1\n**Plan Fingerprint:** {plan_fingerprint}\n**Source Spec Path:** {SPEC_REL}\n**Source Spec Revision:** 1\n**Source Spec Fingerprint:** {spec_fingerprint}\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Task Number:** 1\n**Step Number:** 1\n**Packet Fingerprint:** {packet_fingerprint}\n**Head SHA:** {head_sha}\n**Base SHA:** {head_sha}\n**Claim:** Prepared the workspace for execution.\n**Files Proven:**\n- docs/example-output.md | sha256:{file_digest}\n**Verification Summary:** Manual inspection only: Verified by fixture setup.\n**Invalidation Reason:** N/A\n"
        ),
    );
}

fn write_test_plan_artifact(repo: &Path, state: &Path, browser_required: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state)
        .join(format!("tester-{safe_branch}-test-plan-20260322-170500.md"));
    write_file(
        &artifact_path,
        &format!(
            "# Test Plan\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-22T17:05:00Z\n",
            repo_slug(repo)
        ),
    );
    artifact_path
}

fn write_code_review_artifact(repo: &Path, state: &Path, base_branch: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state)
        .join(format!("tester-{safe_branch}-code-review-20260322-171100.md"));
    write_file(
        &artifact_path,
        &format!(
            "# Code Review Result\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-22T17:11:00Z\n",
            repo_slug(repo)
        ),
    );
    artifact_path
}

fn write_release_readiness_artifact(repo: &Path, state: &Path, base_branch: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state)
        .join(format!("tester-{safe_branch}-release-readiness-20260322-171500.md"));
    write_file(
        &artifact_path,
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-22T17:15:00Z\n",
            repo_slug(repo)
        ),
    );
    artifact_path
}

fn run_featureforge_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    context: &str,
) -> serde_json::Value {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    parse_json(&run(command, context), context)
}

fn enable_session_decision(state: &Path, session_key: &str) {
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    write_file(&decision_path, "enabled\n");
}

fn replace_in_file(path: &Path, from: &str, to: &str) {
    let source = fs::read_to_string(path).expect("file should be readable");
    fs::write(path, source.replace(from, to)).expect("file should be writable");
}

#[test]
fn workflow_phase_routes_missing_final_review_back_to_requesting_code_review() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-final-review");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-runtime-final-review";

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    write_release_readiness_artifact(repo, state, &expected_base_branch(repo));
    enable_session_decision(state, session_key);

    let phase_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "phase", "--json"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow phase for final-review-focused shard",
    );
    let handoff_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "handoff", "--json"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow handoff for final-review-focused shard",
    );
    let gate_finish_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "gate", "finish", "--plan", PLAN_REL, "--json"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow finish gate for final-review-focused shard",
    );

    assert_eq!(phase_json["phase"], "final_review_pending");
    assert_eq!(phase_json["next_action"], "request_code_review");
    assert_eq!(handoff_json["phase"], "final_review_pending");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:requesting-code-review");
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Finish readiness requires a final code-review artifact."
    );
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "ReviewArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "review_artifact_missing");
}

#[test]
fn workflow_phase_routes_stale_review_back_to_requesting_code_review() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-stale-final-review");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-runtime-stale-final-review";

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    enable_session_decision(state, session_key);
    replace_in_file(
        &review_path,
        &format!("**Head SHA:** {}", current_head_sha(repo)),
        "**Head SHA:** 0000000000000000000000000000000000000000",
    );

    let phase_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "phase", "--json"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow phase for stale-review-focused shard",
    );
    let handoff_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "handoff", "--json"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow handoff for stale-review-focused shard",
    );
    let gate_finish_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "gate", "finish", "--plan", PLAN_REL, "--json"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow finish gate for stale-review-focused shard",
    );

    assert_eq!(phase_json["phase"], "final_review_pending");
    assert_eq!(phase_json["next_action"], "request_code_review");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:requesting-code-review");
    assert_eq!(
        handoff_json["recommendation_reason"],
        "The latest code-review artifact does not match the current HEAD."
    );
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "ReviewArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "review_artifact_head_mismatch");
}
