#[path = "support/executable.rs"]
mod executable_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/prebuilt.rs"]
mod prebuilt_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use assert_cmd::cargo::CommandCargoExt;
use executable_support::make_executable;
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use files_support::write_file;
use prebuilt_support::write_canonical_prebuilt_layout;
use process_support::run;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;
use workflow_support::{init_repo, workflow_fixture_root};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn inject_current_topology_sections(plan_source: &str) -> String {
    const INSERT_AFTER: &str = "## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-004 -> Task 1\n- VERIFY-001 -> Task 1\n";
    const TOPOLOGY_BLOCK: &str = "\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n";
    const QA_HEADER_AFTER: &str = "**Last Reviewed By:** plan-eng-review\n";
    const QA_HEADER_BLOCK: &str = "**Last Reviewed By:** plan-eng-review\n**QA Requirement:** not-required\n";

    let mut adjusted = if plan_source.contains("## Execution Strategy")
        && plan_source.contains("## Dependency Diagram")
    {
        plan_source.to_owned()
    } else {
        plan_source.replacen(INSERT_AFTER, &format!("{INSERT_AFTER}{TOPOLOGY_BLOCK}"), 1)
    };

    if !adjusted.contains("**QA Requirement:**") {
        adjusted = adjusted.replacen(QA_HEADER_AFTER, QA_HEADER_BLOCK, 1);
    }

    adjusted
}

fn rewrite_plan_qa_requirement(repo: &Path, plan_rel: &str, qa_requirement: Option<&str>) {
    let plan_path = repo.join(plan_rel);
    let mut plan_source =
        fs::read_to_string(&plan_path).expect("workflow shell-smoke plan should be readable");
    let current_header_line = plan_source
        .lines()
        .find(|line| line.starts_with("**QA Requirement:**"))
        .map(str::to_owned);
    match (current_header_line, qa_requirement) {
        (Some(current_header_line), Some(qa_requirement)) => {
            plan_source = plan_source.replace(
                &current_header_line,
                &format!("**QA Requirement:** {qa_requirement}"),
            );
        }
        (Some(current_header_line), None) => {
            plan_source = plan_source.replace(&format!("{current_header_line}\n"), "");
        }
        (None, Some(qa_requirement)) => {
            plan_source = plan_source.replacen(
                "**Last Reviewed By:** plan-eng-review\n",
                &format!(
                    "**Last Reviewed By:** plan-eng-review\n**QA Requirement:** {qa_requirement}\n"
                ),
                1,
            );
        }
        (None, None) => {}
    }
    write_file(&plan_path, &plan_source);
}

fn install_full_contract_ready_artifacts(repo: &Path) {
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let fixture_root = workflow_fixture_root();
    let spec_path = repo.join(spec_rel);
    let plan_path = repo.join(plan_rel);

    if let Some(parent) = spec_path.parent() {
        fs::create_dir_all(parent).expect("spec fixture parent should be creatable");
    }
    fs::copy(
        fixture_root.join("specs/2026-03-22-runtime-integration-hardening-design.md"),
        &spec_path,
    )
    .expect("spec fixture should copy");

    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent).expect("plan fixture parent should be creatable");
    }
    let plan_source =
        fs::read_to_string(fixture_root.join("plans/2026-03-22-runtime-integration-hardening.md"))
            .expect("ready plan fixture should read");
    let adjusted_plan = inject_current_topology_sections(&plan_source).replace(
        "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
        spec_rel,
    );
    fs::write(&plan_path, adjusted_plan).expect("ready plan fixture should write");
}

fn install_ready_artifacts(repo: &Path) {
    install_full_contract_ready_artifacts(repo);
}

fn run_featureforge(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge cargo binary should be available");
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    run(command, context)
}

fn run_featureforge_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge cargo binary should be available");
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, context)
}

fn run_featureforge_with_env_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Value {
    let output = run_featureforge_with_env(repo, state_dir, args, extra_env, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn run_checked(command: Command, context: &str) -> Output {
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn write_repo_file(repo: &Path, relative: &str, content: &str) {
    let path = repo.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("repo file parent should be creatable");
    }
    write_file(&path, content);
}

fn run_plan_execution_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge cargo binary should be available");
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "execution"])
        .args(args);
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn current_branch_name(repo: &Path) -> String {
    let mut command = Command::new("git");
    command
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo);
    let output = run_checked(command, "git rev-parse --abbrev-ref HEAD");
    String::from_utf8(output.stdout)
        .expect("branch output should be utf-8")
        .trim()
        .to_owned()
}

fn expected_release_base_branch(repo: &Path) -> String {
    const COMMON_BASE_BRANCHES: [&str; 5] = ["main", "master", "develop", "dev", "trunk"];

    let current_branch = current_branch_name(repo);
    if COMMON_BASE_BRANCHES.contains(&current_branch.as_str()) {
        return current_branch;
    }

    let output = run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
                .current_dir(repo);
            command
        },
        "git for-each-ref refs/heads for expected base branch",
    );
    let branches = String::from_utf8(output.stdout)
        .expect("branch listing output should be utf-8")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for candidate in COMMON_BASE_BRANCHES {
        if branches.contains(candidate) {
            return candidate.to_owned();
        }
    }
    current_branch
}

fn current_head_sha(repo: &Path) -> String {
    let mut command = Command::new("git");
    command.args(["rev-parse", "HEAD"]).current_dir(repo);
    let output = run_checked(command, "git rev-parse HEAD");
    String::from_utf8(output.stdout)
        .expect("head output should be utf-8")
        .trim()
        .to_owned()
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn repo_slug(repo: &Path, state_dir: &Path) -> String {
    let output = run_featureforge(repo, state_dir, &["repo", "slug"], "featureforge repo slug");
    assert!(
        output.status.success(),
        "featureforge repo slug should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("repo slug output should be utf-8")
        .lines()
        .find_map(|line| line.strip_prefix("SLUG="))
        .unwrap_or_else(|| panic!("repo slug output should include SLUG=..., got missing slug"))
        .to_owned()
}

fn project_artifact_dir(repo: &Path, state_dir: &Path) -> PathBuf {
    state_dir.join("projects").join(repo_slug(repo, state_dir))
}

fn write_branch_test_plan_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    browser_required: &str,
) {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    write_file(
        &path,
        &format!(
            "# Test Plan\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-24T12:00:00Z\n\n## Affected Pages / Routes\n- none\n\n## Key Interactions\n- shell smoke parity fixtures\n\n## Edge Cases\n- downstream phase routing coverage\n\n## Critical Paths\n- downstream routing should stay harness-aware.\n",
            repo_slug(repo, state_dir)
        ),
    );
}

fn remove_branch_test_plan_artifact(repo: &Path, state_dir: &Path) {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    if path.exists() {
        fs::remove_file(&path).expect("branch test-plan artifact should be removable");
    }
}

fn write_branch_review_artifact(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let strategy_checkpoint_fingerprint = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "plan execution status for shell-smoke review artifact fixture",
    )["last_strategy_checkpoint_fingerprint"]
        .as_str()
        .expect("shell-smoke review artifact fixture should expose strategy checkpoint fingerprint")
        .to_owned();
    let reviewer_artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-independent-review-20260324-120950.md"
    ));
    let reviewer_artifact_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
        repo_slug(repo, state_dir),
        current_head_sha(repo)
    );
    write_file(&reviewer_artifact_path, &reviewer_artifact_source);
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ));
    write_file(
        &path,
        &format!(
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:10:00Z\n\n## Summary\n- shell smoke parity fixture.\n",
            reviewer_artifact_path.display(),
            repo_slug(repo, state_dir),
            current_head_sha(repo)
        ),
    );
}

fn write_branch_release_artifact(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-release-readiness-20260324-121500.md"
    ));
    write_file(
        &path,
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-24T12:15:00Z\n\n## Summary\n- shell smoke parity fixture.\n",
            repo_slug(repo, state_dir),
            current_head_sha(repo)
        ),
    );
    publish_authoritative_release_truth(repo, state_dir, &path);
}

fn mark_current_branch_closure_release_ready(repo: &Path, state_dir: &Path, branch_closure_id: &str) {
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("current_branch_closure_id", Value::from(branch_closure_id)),
            ("current_release_readiness_result", Value::from("ready")),
        ],
    );
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    run_checked(checkout, "git checkout preflight acceptance branch");
}

fn complete_workflow_fixture_execution_with_qa_requirement(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    qa_requirement: Option<&str>,
    remove_qa_requirement: bool,
) {
    install_full_contract_ready_artifacts(repo);
    if remove_qa_requirement {
        rewrite_plan_qa_requirement(repo, plan_rel, None);
    } else if let Some(qa_requirement) = qa_requirement {
        rewrite_plan_qa_requirement(repo, plan_rel, Some(qa_requirement));
    }
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "workflow-shell-smoke-fixture");
    let status = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "plan execution status for shell-smoke parity fixture",
    );
    let preflight = run_plan_execution_json(
        repo,
        state_dir,
        &["preflight", "--plan", plan_rel],
        "plan execution preflight for shell-smoke parity fixture",
    );
    assert_eq!(preflight["allowed"], true);
    let begin = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "plan execution begin for shell-smoke parity fixture",
    );
    let begin_fingerprint = begin["execution_fingerprint"]
        .as_str()
        .expect("begin should expose execution_fingerprint")
        .to_owned();
    let complete_args = vec![
        "complete",
        "--plan",
        plan_rel,
        "--task",
        "1",
        "--step",
        "1",
        "--source",
        "featureforge:executing-plans",
        "--claim",
        "Completed shell smoke parity fixture task.",
        "--manual-verify-summary",
        "Verified by shell smoke parity setup.",
        "--file",
        "tests/workflow_shell_smoke.rs",
        "--expect-execution-fingerprint",
        begin_fingerprint.as_str(),
    ];
    let _ = run_plan_execution_json(
        repo,
        state_dir,
        &complete_args,
        "plan execution complete for shell-smoke parity fixture",
    );
}

fn complete_workflow_fixture_execution(repo: &Path, state_dir: &Path, plan_rel: &str) {
    complete_workflow_fixture_execution_with_qa_requirement(repo, state_dir, plan_rel, None, false);
}

fn append_tracked_repo_line(repo: &Path, rel_path: &str, line: &str) {
    let path = repo.join(rel_path);
    let mut source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("tracked fixture file {} should be readable: {error}", path.display()));
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(line);
    source.push('\n');
    write_file(&path, &source);
}

fn upsert_plan_header(repo: &Path, plan_rel: &str, header: &str, value: &str) {
    let plan_path = repo.join(plan_rel);
    let source = fs::read_to_string(&plan_path)
        .unwrap_or_else(|error| panic!("plan fixture {} should be readable: {error}", plan_path.display()));
    let header_prefix = format!("**{header}:** ");
    let replacement = format!("{header_prefix}{value}");
    if source.contains(&header_prefix) {
        let rewritten = source
            .lines()
            .map(|line| {
                if line.starts_with(&header_prefix) {
                    replacement.clone()
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        write_file(&plan_path, &rewritten);
        return;
    }
    let inserted = if source.contains("**QA Requirement:**") {
        source.replacen(
            "**QA Requirement:** not-required\n",
            &format!("**QA Requirement:** not-required\n{replacement}\n"),
            1,
        )
    } else {
        source.replacen(
            "**Last Reviewed By:** plan-eng-review\n",
            &format!("**Last Reviewed By:** plan-eng-review\n{replacement}\n"),
            1,
        )
    };
    write_file(&plan_path, &inserted);
}

fn update_authoritative_harness_state(
    repo: &Path,
    state_dir: &Path,
    updates: &[(&str, Value)],
) {
    let state_path = harness_state_path(state_dir, &repo_slug(repo, state_dir), &current_branch_name(repo));
    let mut payload: Value = match fs::read_to_string(&state_path) {
        Ok(source) => serde_json::from_str(&source)
            .expect("authoritative shell-smoke harness state should remain valid json"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Object(serde_json::Map::new()),
        Err(error) => panic!("authoritative shell-smoke harness state should be readable: {error}"),
    };
    let object = payload
        .as_object_mut()
        .expect("authoritative shell-smoke harness state should remain an object");
    for (key, value) in updates {
        object.insert((*key).to_string(), value.clone());
    }
    let explicit_reviewed_state_update = updates
        .iter()
        .any(|(key, _)| *key == "current_branch_closure_reviewed_state_id");
    if !explicit_reviewed_state_update {
        for (key, value) in updates {
            if *key == "current_branch_closure_id" {
                let reviewed_state_value = value
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(|_| Value::from(current_tracked_tree_id(repo)))
                    .unwrap_or(Value::Null);
                object.insert(
                    String::from("current_branch_closure_reviewed_state_id"),
                    reviewed_state_value,
                );
            }
        }
    }
    write_file(
        &state_path,
        &serde_json::to_string(&payload).expect("authoritative shell-smoke harness state should serialize"),
    );
}

fn authoritative_harness_state(repo: &Path, state_dir: &Path) -> Value {
    let state_path = harness_state_path(state_dir, &repo_slug(repo, state_dir), &current_branch_name(repo));
    serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("authoritative shell-smoke harness state should be readable"),
    )
    .expect("authoritative shell-smoke harness state should remain valid json")
}

fn current_tracked_tree_id(repo: &Path) -> String {
    let index_path_output = run(
        {
            let mut command = Command::new("git");
            command.args(["rev-parse", "--git-path", "index"]).current_dir(repo);
            command
        },
        "git rev-parse --git-path index",
    );
    let index_path_text = String::from_utf8_lossy(&index_path_output.stdout)
        .trim()
        .to_owned();
    let index_path = PathBuf::from(&index_path_text);
    let index_path = if index_path.is_absolute() {
        index_path
    } else {
        repo.join(index_path)
    };
    let temp_index_path = repo.join(".git").join(format!(
        "workflow-shell-smoke-reviewed-state-{}.index",
        std::process::id()
    ));
    fs::copy(&index_path, &temp_index_path)
        .expect("tracked tree helper should copy git index");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .current_dir(repo)
                .env("GIT_INDEX_FILE", &temp_index_path)
                .args(["add", "-u", "."]);
            command
        },
        "git add -u for tracked tree helper",
    );
    let write_tree_output = run_checked(
        {
            let mut command = Command::new("git");
            command
                .current_dir(repo)
                .env("GIT_INDEX_FILE", &temp_index_path)
                .args(["write-tree"]);
            command
        },
        "git write-tree for tracked tree helper",
    );
    fs::remove_file(&temp_index_path).expect("tracked tree helper should clean up temp index");
    let tree_sha = String::from_utf8(write_tree_output.stdout)
        .expect("tracked tree output should be utf-8")
        .trim()
        .to_owned();
    format!("git_tree:{tree_sha}")
}

fn publish_authoritative_final_review_truth(repo: &Path, state_dir: &Path, review_path: &Path) {
    let branch = current_branch_name(repo);
    let review_source = fs::read_to_string(review_path)
        .expect("shell-smoke review artifact should be readable for authoritative publication");
    let review_fingerprint = sha256_hex(review_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state_dir,
            &repo_slug(repo, state_dir),
            &branch,
            &format!("final-review-{review_fingerprint}.md"),
        ),
        &review_source,
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("not_required")),
            ("release_docs_state", Value::from("not_required")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from(review_fingerprint),
            ),
        ],
    );
}

fn publish_authoritative_release_truth(repo: &Path, state_dir: &Path, release_path: &Path) {
    let branch = current_branch_name(repo);
    let release_source = fs::read_to_string(release_path)
        .expect("shell-smoke release artifact should be readable for authoritative publication");
    let release_fingerprint = sha256_hex(release_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state_dir,
            &repo_slug(repo, state_dir),
            &branch,
            &format!("release-docs-{release_fingerprint}.md"),
        ),
        &release_source,
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("release_docs_state", Value::from("fresh")),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from(release_fingerprint),
            ),
        ],
    );
}

fn write_dispatched_branch_review_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    write_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    mark_current_branch_closure_release_ready(repo, state_dir, "branch-release-closure");
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let initial_review_path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-code-review-20260324-121000.md"));
    publish_authoritative_final_review_truth(repo, state_dir, &initial_review_path);
    let gate_review = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution gate-review dispatch for shell-smoke review fixture",
    );
    assert_eq!(
        gate_review["allowed"],
        Value::Bool(true),
        "shell-smoke review fixture should prime a passing gate-review dispatch before minting a final-review artifact: {gate_review:?}"
    );
    write_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    let review_path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-code-review-20260324-121000.md"));
    publish_authoritative_final_review_truth(repo, state_dir, &review_path);
}

fn install_cutover_check_baseline(repo: &Path) {
    write_repo_file(
        repo,
        "bin/featureforge",
        "#!/usr/bin/env bash\nprintf 'featureforge test runtime\\n'\n",
    );
    make_executable(&repo.join("bin/featureforge"));
    write_canonical_prebuilt_layout(
        repo,
        "1.0.0",
        "#!/usr/bin/env bash\nprintf 'darwin runtime\\n'\n",
        "windows runtime\n",
    );
}

fn git_add_all(repo: &Path) {
    let mut command = Command::new("git");
    command.args(["add", "."]).current_dir(repo);
    run_checked(command, "git add for cutover repo");
}

fn run_cutover_check(repo: &Path) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts/check-featureforge-cutover.sh"))
        .current_dir(repo)
        .env("FEATUREFORGE_CUTOVER_REPO_ROOT", repo);
    run(command, "featureforge cutover check")
}

fn run_cutover_check_with_env(repo: &Path, extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts/check-featureforge-cutover.sh"))
        .current_dir(repo)
        .env("FEATUREFORGE_CUTOVER_REPO_ROOT", repo);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, "featureforge cutover check with env")
}

#[test]
fn standalone_binary_has_no_separate_workflow_wrapper_files() {
    let bin_dir = repo_root().join("bin");
    let workflow_entries = fs::read_dir(&bin_dir)
        .expect("bin dir should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "featureforge" && name.contains("workflow"))
        .collect::<Vec<_>>();
    assert!(
        workflow_entries.is_empty(),
        "workflow wrapper files should not exist alongside the standalone featureforge binary: {workflow_entries:?}"
    );
}

#[test]
fn workflow_help_outside_repo_mentions_the_public_surfaces() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should exist");
    let output = run_featureforge(
        outside_repo.path(),
        outside_repo.path(),
        &["workflow", "help"],
        "workflow help outside repo",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: featureforge workflow <COMMAND>"));
    assert!(stdout.contains("Commands:"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("help"));
}

#[test]
fn workflow_status_summary_matches_json_semantics_for_ready_plans() {
    let (repo_dir, state_dir) = init_repo("workflow-summary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_ready_artifacts(repo);

    let json_output = run_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "workflow status json",
    );
    let json_stdout = String::from_utf8_lossy(&json_output.stdout);
    assert!(json_stdout.contains("\"schema_version\":3"));
    assert!(json_stdout.contains("\"status\":\"implementation_ready\""));
    assert!(json_stdout.contains("\"next_skill\":\"\""));

    let summary_output = run_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh", "--summary"],
        "workflow status summary",
    );
    let summary_stdout = String::from_utf8_lossy(&summary_output.stdout);
    assert!(!summary_stdout.contains("{\"status\""));
    assert!(summary_stdout.contains("status=implementation_ready"));
    assert!(summary_stdout.contains("next=execution_preflight"));
    assert!(summary_stdout.contains(
        "spec=docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    ));
    assert!(
        summary_stdout
            .contains("plan=docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md")
    );
}

#[test]
fn workflow_operator_commands_work_for_ready_plan() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let next_output = run_featureforge(
        repo,
        state,
        &["workflow", "next"],
        "workflow next",
    );
    let next_stdout = String::from_utf8_lossy(&next_output.stdout);
    assert!(next_stdout.contains("Next safe step:"));
    assert!(next_stdout.contains(
        "Return to execution preflight for the approved plan: docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md"
    ));
    assert!(!next_stdout.contains("session-entry"));

    let artifacts_output = run_featureforge(
        repo,
        state,
        &["workflow", "artifacts"],
        "workflow artifacts",
    );
    let artifacts_stdout = String::from_utf8_lossy(&artifacts_output.stdout);
    assert!(artifacts_stdout.contains("Workflow artifacts"));
    assert!(artifacts_stdout.contains(
        "Spec: docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    ));
    assert!(
        artifacts_stdout
            .contains("Plan: docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md")
    );

    let explain_output = run_featureforge(
        repo,
        state,
        &["workflow", "explain"],
        "workflow explain",
    );
    let explain_stdout = String::from_utf8_lossy(&explain_output.stdout);
    assert!(explain_stdout.contains("Why FeatureForge chose this state"));
    assert!(explain_stdout.contains("What to do:"));
    assert!(!explain_stdout.contains("session-entry"));
}

#[test]
fn workflow_operator_routes_active_execution_to_exact_step_command() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-execution-command-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-operator-execution-command-context");

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status for workflow operator active execution routing",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", plan_rel],
        "preflight for workflow operator active execution routing",
    );
    assert_eq!(preflight["allowed"], true);
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for begin"),
        ],
        "begin should establish an active step for workflow operator routing",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for active execution routing",
    );

    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_in_progress");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "continue execution");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution complete --plan {plan_rel} --task 1 --step 1 --source featureforge:executing-plans --claim <claim> --manual-verify-summary <summary> --expect-execution-fingerprint {}",
            begin["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for operator command")
        ))
    );
    assert_eq!(
        operator_json["execution_command_context"],
        serde_json::json!({
            "command_kind": "complete",
            "task_number": 1,
            "step_id": 1
        })
    );
}

#[test]
fn workflow_operator_routes_blocked_execution_to_resume_same_step() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-blocked-step-command-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-operator-blocked-step-command-context");

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status for workflow operator blocked execution routing",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", plan_rel],
        "preflight for workflow operator blocked execution routing",
    );
    assert_eq!(preflight["allowed"], true);
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for blocked begin"),
        ],
        "begin should establish an active step before it becomes blocked",
    );
    let blocked = run_plan_execution_json(
        repo,
        state,
        &[
            "note",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--state",
            "blocked",
            "--message",
            "Waiting for dependency",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for blocked note"),
        ],
        "note blocked should establish a blocked step for workflow operator routing",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for blocked execution routing",
    );

    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_in_progress");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "continue execution");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution begin --plan {plan_rel} --task 1 --step 1 --expect-execution-fingerprint {}",
            blocked["execution_fingerprint"]
                .as_str()
                .expect("blocked note should expose execution fingerprint for operator command")
        ))
    );
    assert_eq!(
        operator_json["execution_command_context"],
        serde_json::json!({
            "command_kind": "begin",
            "task_number": 1,
            "step_id": 1
        })
    );

    let resumed = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--expect-execution-fingerprint",
            blocked["execution_fingerprint"]
                .as_str()
                .expect("blocked note should expose execution fingerprint for resume begin"),
        ],
        "begin should resume the same blocked step",
    );
    assert_eq!(resumed["active_task"], Value::from(1));
    assert_eq!(resumed["active_step"], Value::from(1));
    assert_eq!(resumed["blocking_task"], Value::Null);
    assert_eq!(resumed["blocking_step"], Value::Null);
}

#[derive(Clone, Copy)]
struct LateStageCase {
    name: &'static str,
    expected_phase: &'static str,
    expected_next_action: &'static str,
    setup: fn(&Path, &Path, &str, &str),
}

fn seed_current_task_closure_state(repo: &Path, state_dir: &Path, plan_rel: &str) {
    let closure_record_id = format!(
        "task-closure-{}",
        sha256_hex(format!("{plan_rel}:task-1").as_bytes())
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "fixture-task-dispatch",
                    "closure_record_id": closure_record_id,
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": format!(
                        "task-contract-{}",
                        sha256_hex(format!("{plan_rel}:task-1").as_bytes())
                    ),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"Fixture task review passed."),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(
                        b"Fixture task verification passed for the current reviewed state."
                    ),
                }
            }),
        )],
    );
}

fn setup_qa_pending_case(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    complete_workflow_fixture_execution_with_qa_requirement(
        repo,
        state_dir,
        plan_rel,
        Some("required"),
        false,
    );
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );
}

fn setup_document_release_pending_case(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    complete_workflow_fixture_execution(repo, state_dir, plan_rel);
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );
}

fn setup_ready_for_finish_case(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    complete_workflow_fixture_execution(repo, state_dir, plan_rel);
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
}

fn setup_ready_for_finish_case_with_qa_requirement(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
    qa_requirement: Option<&str>,
    remove_qa_requirement: bool,
) {
    complete_workflow_fixture_execution_with_qa_requirement(
        repo,
        state_dir,
        plan_rel,
        qa_requirement,
        remove_qa_requirement,
    );
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
}

fn setup_task_boundary_blocked_case(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    _base_branch: &str,
) {
    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(plan_rel),
        r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Task Outcome:** Task 1 execution reaches a boundary gate before Task 2 starts.
**Plan Constraints:**
- Keep fixture inputs deterministic.
**Open Questions:** none

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Task Outcome:** Task 2 should remain blocked until Task 1 closure requirements are met.
**Plan Constraints:**
- Preserve deterministic task-boundary diagnostics.
**Open Questions:** none

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Start the follow-on task**
"#,
    );
    prepare_preflight_acceptance_workspace(repo, "workflow-shell-smoke-task-boundary-blocked");

    let status_before_begin = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "status before task-boundary blocked shell-smoke fixture execution",
    );
    let preflight = run_plan_execution_json(
        repo,
        state_dir,
        &["preflight", "--plan", plan_rel],
        "preflight for task-boundary blocked shell-smoke fixture execution",
    );
    assert_eq!(preflight["allowed"], true);

    let begin_task1_step1 = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before begin"),
        ],
        "begin task 1 step 1 for task-boundary blocked shell-smoke fixture",
    );
    let complete_task1_step1 = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for task-boundary blocked shell-smoke fixture.",
            "--manual-verify-summary",
            "Verified by shell-smoke task-boundary fixture setup.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for task-boundary blocked shell-smoke fixture",
    );
    let begin_task1_step2 = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("complete should expose execution fingerprint for next begin"),
        ],
        "begin task 1 step 2 for task-boundary blocked shell-smoke fixture",
    );
    run_plan_execution_json(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 2 for task-boundary blocked shell-smoke fixture.",
            "--manual-verify-summary",
            "Verified by shell-smoke task-boundary fixture setup.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for task-boundary blocked shell-smoke fixture",
    );
}

#[test]
fn workflow_phase_text_and_json_surfaces_match_harness_downstream_freshness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let cases = [
        LateStageCase {
            name: "qa-pending",
            expected_phase: "qa_pending",
            expected_next_action: "run_qa_only",
            setup: setup_qa_pending_case,
        },
        LateStageCase {
            name: "document-release-pending",
            expected_phase: "document_release_pending",
            expected_next_action: "run_document_release",
            setup: setup_document_release_pending_case,
        },
        LateStageCase {
            name: "ready-for-branch-completion",
            expected_phase: "ready_for_branch_completion",
            expected_next_action: "finish_branch",
            setup: setup_ready_for_finish_case,
        },
        LateStageCase {
            name: "task-boundary-blocked",
            expected_phase: "repairing",
            expected_next_action: "return_to_execution",
            setup: setup_task_boundary_blocked_case,
        },
    ];

    for case in cases {
        let (repo_dir, state_dir) = init_repo(&format!("workflow-phase-next-parity-{}", case.name));
        let repo = repo_dir.path();
        let state = state_dir.path();
        let base_branch = expected_release_base_branch(repo);
        (case.setup)(repo, state, plan_rel, &base_branch);

        let phase_json = run_featureforge_with_env_json(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[],
            "workflow phase json for shell-smoke late-stage parity",
        );
        let doctor_json = run_featureforge_with_env_json(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[],
            "workflow doctor json for shell-smoke late-stage parity",
        );
        let phase_text_output = run_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase"],
            &[],
            "workflow phase text for shell-smoke late-stage parity",
        );
        assert!(
            phase_text_output.status.success(),
            "workflow phase text should succeed for case {}, got {:?}",
            case.name,
            phase_text_output.status
        );
        let phase_text = String::from_utf8_lossy(&phase_text_output.stdout);
        let next_output = run_featureforge_with_env(
            repo,
            state,
            &["workflow", "next"],
            &[],
            "workflow next text for shell-smoke late-stage parity",
        );
        assert!(
            next_output.status.success(),
            "workflow next text should succeed for case {}, got {:?}",
            case.name,
            next_output.status
        );
        let next_text = String::from_utf8_lossy(&next_output.stdout);

        assert_eq!(phase_json["phase"], case.expected_phase);
        assert_eq!(phase_json["next_action"], case.expected_next_action);
        assert!(phase_text.contains(&format!("Workflow phase: {}", case.expected_phase)));
        assert!(phase_text.contains(&format!("Next action: {}", case.expected_next_action)));
        assert!(next_text.contains(&format!("Next action: {}", case.expected_next_action)));

        let next_step = phase_text
            .lines()
            .find_map(|line| line.strip_prefix("Next: "))
            .unwrap_or_else(|| {
                panic!(
                    "workflow phase text should expose Next line for case {}",
                    case.name
                )
            });
        assert!(
            next_text.contains(next_step),
            "workflow next text should mirror the same Next step from workflow phase text for case {}",
            case.name
        );
        assert_eq!(
            phase_json["next_step"],
            Value::from(next_step),
            "workflow phase json should mirror the same Next step from workflow phase text for case {}",
            case.name
        );

        for field in [
            "final_review_state",
            "browser_qa_state",
            "release_docs_state",
            "last_final_review_artifact_fingerprint",
            "last_browser_qa_artifact_fingerprint",
            "last_release_docs_artifact_fingerprint",
        ] {
            assert!(
                doctor_json["execution_status"].get(field).is_some(),
                "workflow doctor json should keep downstream freshness metadata field `{field}` for case {}",
                case.name
            );
        }
    }
}

#[test]
fn workflow_operator_routes_task_boundary_to_record_review_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-boundary-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for task-boundary dispatch routing",
    );

    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(operator_json["phase_detail"], "task_review_dispatch_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "dispatch review");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-review-dispatch --plan {plan_rel} --scope task --task 1"
        ))
    );
}

#[test]
fn workflow_operator_routes_ready_branch_completion_to_gate_finish_after_review_gate_passes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-finish-review-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::from("branch-closure-ready")),
            (
                "finish_review_gate_pass_branch_closure_id",
                Value::from("branch-closure-ready"),
            ),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for ready branch completion routing",
    );

    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["phase_detail"], "finish_completion_gate_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["finish_review_gate_pass_branch_closure_id"],
        "branch-closure-ready"
    );
    assert_eq!(operator_json["next_action"], "run finish completion gate");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!("featureforge plan execution gate-finish --plan {plan_rel}"))
    );
}

#[test]
fn workflow_operator_requires_persisted_gate_review_checkpoint_before_gate_finish() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-finish-review-checkpoint-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-closure-ready"))],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json without persisted gate-review checkpoint",
    );

    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["phase_detail"], "finish_review_gate_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["finish_review_gate_pass_branch_closure_id"],
        Value::Null
    );
    assert_eq!(operator_json["next_action"], "run finish review gate");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!("featureforge plan execution gate-review --plan {plan_rel}"))
    );
}

#[test]
fn plan_execution_gate_review_records_finish_review_gate_pass_checkpoint() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-gate-review-records-finish-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-closure-ready"))],
    );

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should succeed and persist the finish-review gate pass checkpoint",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(true));

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after gate-review"),
    )
    .expect("authoritative state should remain valid json after gate-review");
    assert_eq!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"],
        Value::from("branch-closure-ready")
    );
}

#[test]
fn plan_execution_explain_review_state_does_not_record_finish_review_gate_pass_checkpoint() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-explain-review-state-does-not-record-finish-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-closure-ready"))],
    );

    let _ = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should stay read-only and not persist the finish-review gate pass checkpoint",
    );

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should remain readable after explain-review-state"),
    )
    .expect("authoritative state should remain valid json after explain-review-state");
    assert!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"].is_null(),
        "explain-review-state must not persist the finish-review gate pass checkpoint: {authoritative_state_after}",
    );
}

#[test]
fn workflow_operator_waits_for_task_review_result_after_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-review-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for workflow operator pending fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for task review result pending",
    );

    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(operator_json["phase_detail"], "task_review_result_pending");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "wait for external review result");
    assert!(operator_json.get("recommended_command").is_none());
}

#[test]
fn workflow_operator_routes_task_review_result_ready_to_close_current_task() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-review-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for workflow operator ready fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for task review result ready",
    );

    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("task closure recording ready should expose dispatch_id");
    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(operator_json["phase_detail"], "task_closure_recording_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "close current task");
    assert_eq!(operator_json["recording_context"]["task_number"], Value::from(1));
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution close-current-task --plan {plan_rel} --task 1 --dispatch-id {dispatch_id} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
        ))
    );
}

#[test]
fn plan_execution_record_review_dispatch_exposes_dispatch_id() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-review-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let dispatch_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should expose dispatch contract fields",
    );

    assert_eq!(dispatch_json["allowed"], Value::Bool(true));
    assert_eq!(dispatch_json["action"], "recorded");
    assert_eq!(dispatch_json["scope"], "task");
    assert!(dispatch_json["dispatch_id"].as_str().is_some());

    let rerun_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch rerun should remain idempotent",
    );
    assert_eq!(rerun_json["allowed"], Value::Bool(true));
    assert_eq!(rerun_json["action"], "already_current");
    assert_eq!(rerun_json["dispatch_id"], dispatch_json["dispatch_id"]);
}

#[test]
fn plan_execution_close_current_task_records_task_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for close-current-task fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("task closure fixture authoritative state should read"),
    )
    .expect("task closure fixture authoritative state should remain valid json");
    let dispatch_id = authoritative_state["strategy_review_dispatch_lineage"]["task-1"]["dispatch_id"]
        .as_str()
        .expect("task closure fixture should expose dispatch_id")
        .to_owned();

    let review_summary_path = repo.join("task-1-review-summary.md");
    let verification_summary_path = repo.join("task-1-verification-summary.md");
    write_file(&review_summary_path, "Task 1 independent review passed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task command should succeed",
    );

    assert_eq!(close_json["action"], "recorded");
    assert_eq!(close_json["task_number"], 1);
    assert_eq!(close_json["dispatch_validation_action"], "validated");
    assert_eq!(close_json["closure_action"], "recorded");
    assert_eq!(close_json["task_closure_status"], "current");
    assert_eq!(close_json["superseded_task_closure_ids"], Value::from(Vec::<String>::new()));
    assert!(close_json["closure_record_id"].as_str().is_some());
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("task closure authoritative state should be readable"),
    )
    .expect("task closure authoritative state should remain valid json");
    let current_record = &authoritative_state["current_task_closure_records"]["task-1"];
    assert!(
        current_record["reviewed_state_id"]
            .as_str()
            .unwrap_or_default()
            .starts_with("git_tree:")
    );
    assert_eq!(
        current_record["effective_reviewed_surface_paths"],
        Value::from(vec![String::from("tests/workflow_shell_smoke.rs")])
    );

    let rerun_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task rerun should be idempotent",
    );
    assert_eq!(rerun_json["action"], "already_current", "json: {rerun_json:?}");
    assert_eq!(rerun_json["closure_action"], "already_current");

    write_file(
        &review_summary_path,
        "Task 1 independent review passed with conflicting summary content.\n",
    );
    let conflicting_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task conflicting rerun should fail closed",
    );
    assert_eq!(conflicting_json["action"], "blocked");
    assert_eq!(conflicting_json["closure_action"], "blocked");
}

#[test]
fn plan_execution_close_current_task_requires_fresh_reviewed_state_after_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-stale-after-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for stale close-current-task fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("stale close-current-task fixture authoritative state should read"),
    )
    .expect("stale close-current-task fixture authoritative state should remain valid json");
    let dispatch_id = authoritative_state["strategy_review_dispatch_lineage"]["task-1"]["dispatch_id"]
        .as_str()
        .expect("stale close-current-task fixture should expose dispatch_id")
        .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "tracked drift after task review dispatch",
    );

    let review_summary_path = repo.join("task-1-review-summary.md");
    let verification_summary_path = repo.join("task-1-verification-summary.md");
    write_file(&review_summary_path, "Task 1 independent review passed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task should fail closed after post-dispatch tracked drift",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");
}

#[test]
fn plan_execution_close_current_task_requires_dispatch_reviewed_state_binding() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-close-current-task-missing-dispatch-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for missing reviewed-state binding fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("missing reviewed-state binding fixture should expose dispatch id")
        .to_owned();
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let mut authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("missing reviewed-state binding fixture authoritative state should read"),
    )
    .expect("missing reviewed-state binding fixture authoritative state should remain valid json");
    authoritative_state["strategy_review_dispatch_lineage"]["task-1"]
        .as_object_mut()
        .expect("dispatch lineage should remain an object")
        .remove("reviewed_state_id");
    write_file(
        &state_path,
        &serde_json::to_string(&authoritative_state)
            .expect("missing reviewed-state binding fixture state should serialize"),
    );

    let review_summary_path = repo.join("task-1-failed-review-summary.md");
    write_file(&review_summary_path, "Task 1 review found a blocker.\n");
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should fail closed when dispatch lineage loses reviewed-state binding",
    );

    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["dispatch_validation_action"], "blocked");
    assert_eq!(close_json["required_follow_up"], "record_review_dispatch");
}

#[test]
fn plan_execution_close_current_task_records_failed_task_outcomes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-failures");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for failing close-current-task fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("failing task closure fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-failed-review-summary.md");
    let verification_summary_path = repo.join("task-1-failed-verification-summary.md");
    write_file(&review_summary_path, "Task 1 review passed but verification failed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification found a blocker in the current reviewed state.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "fail",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("failed verification summary path should be utf-8"),
        ],
        "close-current-task should record failed task outcomes",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");

    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("failed task closure authoritative state should be readable"),
    )
    .expect("failed task closure authoritative state should remain valid json");
    let record = &authoritative_state["task_closure_negative_result_records"]["task-1"];
    assert_eq!(record["dispatch_id"], Value::from(dispatch_id.clone()));
    assert_eq!(record["closure_record_id"], Value::Null);
    assert_eq!(record["review_result"], "pass");
    assert_eq!(record["verification_result"], "fail");
    assert!(record["reviewed_state_id"].as_str().is_some());
    assert!(record["contract_identity"].as_str().is_some());
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-1"],
        Value::Null,
        "failed task closure must not create current task closure truth"
    );
}

#[test]
fn plan_execution_close_current_task_records_failed_review_outcomes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-review-fail");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for failing review close-current-task fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("failing review task closure fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-review-failed-summary.md");
    write_file(&review_summary_path, "Task 1 review found blocking issues.\n");

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should record failed review outcomes",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");

    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("failed review authoritative state should be readable"),
    )
    .expect("failed review authoritative state should remain valid json");
    let record = &authoritative_state["task_closure_negative_result_records"]["task-1"];
    assert_eq!(record["dispatch_id"], Value::from(dispatch_id.clone()));
    assert_eq!(record["closure_record_id"], Value::Null);
    assert_eq!(record["review_result"], "fail");
    assert_eq!(record["verification_result"], "not-run");
    assert!(record["reviewed_state_id"].as_str().is_some());
    assert!(record["contract_identity"].as_str().is_some());

    let rerun_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task negative rerun should fail closed",
    );
    assert_eq!(rerun_json["action"], "blocked");
    assert_eq!(rerun_json["closure_action"], "blocked");
    assert_eq!(rerun_json["task_closure_status"], "not_current");

    let verification_summary_path = repo.join("task-1-review-failed-verification-summary.md");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );
    let conflicting_pass_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task negative then pass rerun should fail closed",
    );
    assert_eq!(conflicting_pass_json["action"], "blocked");
    assert_eq!(conflicting_pass_json["closure_action"], "blocked");
    assert_eq!(conflicting_pass_json["task_closure_status"], "not_current");
}

#[test]
fn plan_execution_close_current_task_supersedes_overlapping_prior_task_closures() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-supersedes-overlap");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let task1_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should expose task 1 dispatch contract fields",
    );
    let task1_dispatch_id = task1_dispatch["dispatch_id"]
        .as_str()
        .expect("task 1 dispatch should expose dispatch_id")
        .to_owned();
    let task1_review_summary_path = repo.join("task-1-supersession-review-summary.md");
    let task1_verification_summary_path = repo.join("task-1-supersession-verification-summary.md");
    write_file(
        &task1_review_summary_path,
        "Task 1 independent review passed before overlapping task 2 work.\n",
    );
    write_file(
        &task1_verification_summary_path,
        "Task 1 verification passed before overlapping task 2 work.\n",
    );
    let task1_close = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &task1_dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            task1_review_summary_path
                .to_str()
                .expect("task 1 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task1_verification_summary_path
                .to_str()
                .expect("task 1 verification summary path should be utf-8"),
        ],
        "close-current-task should record task 1 closure before supersession",
    );
    let task1_closure_record_id = task1_close["closure_record_id"]
        .as_str()
        .expect("task 1 close should expose closure record id")
        .to_owned();

    let status_after_task1 = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution fingerprint after task 1 closure",
    );
    let begin_task2 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_task1["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for task 2 begin"),
        ],
        "begin task 2 should succeed once task 1 closure is current",
    );
    let _complete_task2 = run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 2 overlapping work for supersession coverage.",
            "--manual-verify-summary",
            "Verified by supersession shell-smoke coverage.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task2["execution_fingerprint"]
                .as_str()
                .expect("begin task 2 should expose execution fingerprint for complete"),
        ],
        "complete task 2 should succeed for supersession coverage",
    );

    let task2_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        "record-review-dispatch should expose task 2 dispatch contract fields",
    );
    let task2_dispatch_id = task2_dispatch["dispatch_id"]
        .as_str()
        .expect("task 2 dispatch should expose dispatch_id")
        .to_owned();
    let task2_review_summary_path = repo.join("task-2-supersession-review-summary.md");
    let task2_verification_summary_path = repo.join("task-2-supersession-verification-summary.md");
    write_file(
        &task2_review_summary_path,
        "Task 2 independent review passed after overlapping Task 1 surface.\n",
    );
    write_file(
        &task2_verification_summary_path,
        "Task 2 verification passed after overlapping Task 1 surface.\n",
    );
    let task2_close = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--dispatch-id",
            &task2_dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            task2_review_summary_path
                .to_str()
                .expect("task 2 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task2_verification_summary_path
                .to_str()
                .expect("task 2 verification summary path should be utf-8"),
        ],
        "close-current-task should supersede overlapping task 1 closure",
    );
    let task2_closure_record_id = task2_close["closure_record_id"]
        .as_str()
        .expect("task 2 close should expose closure record id")
        .to_owned();
    assert_eq!(task2_close["action"], "recorded");
    assert_eq!(
        task2_close["superseded_task_closure_ids"],
        Value::from(vec![task1_closure_record_id.clone()])
    );

    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("supersession authoritative state should be readable"),
    )
    .expect("supersession authoritative state should remain valid json");
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-1"],
        Value::Null,
        "overlapping later task closure should remove task 1 from the current task-closure set"
    );
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-2"]["closure_record_id"],
        Value::from(task2_closure_record_id.clone())
    );

    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should use only the effective current task-closure set",
    );
    assert_eq!(branch_closure["action"], "recorded");
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("record-branch-closure should expose branch closure id")
        .to_owned();
    let explain = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should expose superseded task closures after supersession",
    );
    assert!(
        explain["superseded_closures"].as_array().is_some_and(|closures| closures
            .iter()
            .any(|closure| closure == &Value::from(task1_closure_record_id.clone()))),
        "json: {explain:?}"
    );
    let branch_record_source = fs::read_to_string(
        project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md")),
    )
    .expect("branch closure artifact should be readable after task supersession");
    assert!(
        branch_record_source.contains(&task2_closure_record_id),
        "branch closure should keep the still-current task 2 lineage: {branch_record_source}"
    );
    assert!(
        !branch_record_source.contains(&task1_closure_record_id),
        "branch closure should exclude superseded task 1 lineage: {branch_record_source}"
    );
}

#[test]
fn workflow_operator_waits_for_final_review_result_after_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for workflow operator pending fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let final_review_rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch rerun should remain idempotent",
    );
    assert_eq!(final_review_rerun["allowed"], Value::Bool(true));
    assert_eq!(final_review_rerun["action"], "already_current");
    assert_eq!(final_review_rerun["dispatch_id"], dispatch["dispatch_id"]);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for final review result pending",
    );

    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(operator_json["phase_detail"], "final_review_outcome_pending");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "wait for external review result");
    assert!(operator_json.get("recommended_command").is_none());
}

#[test]
fn workflow_operator_routes_final_review_result_ready_to_advance_late_stage() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for workflow operator ready fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for final review result ready",
    );

    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review recording ready should expose dispatch_id");
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(operator_json["phase_detail"], "final_review_recording_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --dispatch-id {dispatch_id} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_operator_reroutes_dispatched_final_review_without_release_ready() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-missing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before release-readiness reroute",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_release_readiness_result", Value::Null)],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should reroute dispatched final review without release readiness",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(operator_json["phase_detail"], "release_readiness_recording_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_operator_reroutes_dispatched_final_review_blocked_release_ready_to_resolution() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before blocked release-readiness reroute",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_release_readiness_result", Value::from("blocked"))],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should reroute blocked final review back to release blocker resolution",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_blocker_resolution_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "resolve release blocker");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_operator_requires_fresh_final_review_dispatch_after_branch_closure_changes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-dispatch-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for stale-dispatch fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure-2"))],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should reject stale final-review dispatch lineage",
    );

    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(operator_json["phase_detail"], "final_review_dispatch_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-review-dispatch --plan {plan_rel} --scope final-review"
        ))
    );
}

#[test]
fn plan_execution_final_review_dispatch_requires_release_readiness_ready() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-dispatch-requires-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );
    let state_before = authoritative_harness_state(repo, state);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch without release readiness ready",
    );

    assert_eq!(dispatch["allowed"], Value::Bool(false));
    assert_eq!(dispatch["action"], Value::from("blocked"));
    assert_eq!(
        dispatch["reason_codes"],
        Value::from(vec![String::from("release_readiness_recording_ready")])
    );

    let state_after = authoritative_harness_state(repo, state);
    assert_eq!(
        state_after["strategy_checkpoints"],
        state_before["strategy_checkpoints"],
        "blocked final-review dispatch should not append strategy checkpoints before release readiness is ready: {state_after}"
    );
    assert!(
        state_after["final_review_dispatch_lineage"].is_null(),
        "blocked final-review dispatch should not persist final-review lineage before release readiness is ready: {state_after}"
    );
}

#[test]
fn workflow_operator_routes_final_review_pending_without_current_closure_to_record_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-missing-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for missing-closure fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute final-review missing-closure state",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(operator_json["review_state_status"], "missing_current_closure");
    assert_eq!(operator_json["next_action"], "record branch closure");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_advance_late_stage_records_final_review() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-record");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for advance-late-stage fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for final review recording fixture",
    );
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review recording fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review command should succeed",
    );

    assert_eq!(review_json["action"], "recorded");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(review_json["delegated_primitive"], "record-final-review");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after final review recording",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
}

#[test]
fn plan_execution_advance_late_stage_final_review_blocks_without_release_ready() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-missing-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before clearing release readiness",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final review dispatch should expose dispatch_id")
        .to_owned();
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_release_readiness_result", Value::Null)],
    );

    let summary_path = repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should fail closed without release readiness ready",
    );

    assert_eq!(review_json["action"], "blocked");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(review_json["delegated_primitive"], "record-final-review");
    assert!(review_json["required_follow_up"].is_null(), "json: {review_json}");
    assert!(
        review_json["trace_summary"]
            .as_str()
            .is_some_and(|value| value.contains("release-readiness result `ready`")),
        "json: {review_json}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(authoritative_state["current_final_review_result"].is_null());
    assert!(authoritative_state["final_review_state"].is_null());
}

#[test]
fn plan_execution_advance_late_stage_final_review_blocked_release_ready_requires_resolution() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-blocked-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before blocking release readiness",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final review dispatch should expose dispatch_id")
        .to_owned();
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_release_readiness_result", Value::from("blocked"))],
    );

    let summary_path = repo.join("final-review-blocked-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should require blocker resolution when release readiness is blocked",
    );

    assert_eq!(review_json["action"], "blocked");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(review_json["delegated_primitive"], "record-final-review");
    assert_eq!(
        review_json["required_follow_up"],
        Value::from("resolve_release_blocker")
    );
    assert!(
        review_json["trace_summary"]
            .as_str()
            .is_some_and(|value| value.contains("release-readiness result `ready`")),
        "json: {review_json}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(authoritative_state["current_final_review_result"].is_null());
    assert!(authoritative_state["final_review_state"].is_null());
}

#[test]
fn plan_execution_advance_late_stage_final_review_rerun_is_idempotent_and_conflicts_fail_closed() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-idempotency");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for idempotency fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final-review idempotency fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "first final-review recording should succeed",
    );
    assert_eq!(first["action"], "recorded");
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should read after final-review recording"),
    )
    .expect("authoritative state should remain valid json after final-review recording");
    assert_eq!(
        authoritative_state["current_final_review_dispatch_id"],
        Value::from(dispatch_id.clone())
    );
    assert_eq!(
        authoritative_state["current_final_review_reviewer_source"],
        Value::from("fresh-context-subagent")
    );
    assert_eq!(
        authoritative_state["current_final_review_reviewer_id"],
        Value::from("reviewer-fixture-001")
    );
    assert_eq!(authoritative_state["current_final_review_result"], Value::from("pass"));
    assert_eq!(
        authoritative_state["current_final_review_summary_hash"],
        Value::from(sha256_hex(b"Independent final review passed."))
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_release_readiness_result", Value::Null)],
    );
    let degraded_rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent final-review rerun should fail closed after release readiness degrades",
    );
    assert_eq!(degraded_rerun["action"], "blocked");
    assert!(
        degraded_rerun["trace_summary"]
            .as_str()
            .is_some_and(|value| value.contains("release-readiness result `ready`")),
        "json: {degraded_rerun}"
    );

    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent final-review rerun should be idempotent",
    );
    assert_eq!(second["action"], "already_current");

    let conflicting = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-999",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "conflicting final-review rerun should fail closed while review state is still clean",
    );
    assert_eq!(conflicting["action"], "blocked");
    assert!(conflicting["required_follow_up"].is_null(), "json: {conflicting}");
    assert!(
        conflicting["trace_summary"].as_str().is_some_and(|value| value.contains(
            "conflicting recorded final-review outcome for this dispatch lineage"
        )),
        "json: {conflicting}"
    );

    append_tracked_repo_line(
        repo,
        "README.md",
        "final-review stale-unreviewed regression coverage",
    );
    let stale_operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after final-review stale drift",
    );
    assert_eq!(stale_operator_json["review_state_status"], "stale_unreviewed");
    let stale_rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "stale final-review rerun should fail closed",
    );
    assert_eq!(stale_rerun["action"], "blocked");
    assert_eq!(stale_rerun["required_follow_up"], "repair_review_state");
}

#[test]
fn plan_execution_advance_late_stage_final_review_fail_rerun_requires_review_state_repair() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-fail-rerun");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for failing rerun fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final-review fail rerun fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-fail-summary.md");
    write_file(&summary_path, "Independent final review found a release blocker.\n");
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-fail",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "first failing final-review recording should return remediation follow-up",
    );
    assert_eq!(first["action"], "recorded");
    assert_eq!(first["result"], "fail");
    assert_eq!(first["required_follow_up"], "execution_reentry");

    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-fail",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent failing final-review rerun should fail closed behind review-state repair",
    );
    assert_eq!(second["action"], "blocked", "json: {second}");
    assert_eq!(second["result"], "fail");
    assert_eq!(second["required_follow_up"], "repair_review_state", "json: {second}");
    assert!(
        second["trace_summary"]
            .as_str()
            .is_some_and(|value| value.contains("did not expose final_review_recording_ready")),
        "json: {second}"
    );
}

#[test]
fn plan_execution_advance_late_stage_accepts_human_independent_reviewer() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-human-reviewer");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for human reviewer fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for human-independent-reviewer final review",
    );
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review recording fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-human-summary.md");
    write_file(&summary_path, "Independent human final review passed.\n");
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "human-independent-reviewer",
            "--reviewer-id",
            "human-reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should accept human-independent-reviewer",
    );
    assert_eq!(review_json["action"], "recorded");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after human reviewer final review recording",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
}

#[test]
fn workflow_operator_routes_document_release_pending_to_advance_late_stage_after_branch_closure_exists()
{
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-release-readiness-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for release-readiness-ready routing",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(operator_json["phase_detail"], "release_readiness_recording_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_record_pivot_writes_project_artifact() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let pivot_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            plan_rel,
            "--reason",
            "plan revision superseded current execution",
            "--json",
        ],
        &[],
        "workflow record-pivot json",
    );

    assert_eq!(pivot_json["action"], "recorded");
    assert_eq!(pivot_json["plan_path"], plan_rel);
    assert_eq!(
        pivot_json["reason"],
        "plan revision superseded current execution"
    );
    let record_path = pivot_json["record_path"]
        .as_str()
        .expect("workflow record-pivot should emit record_path");
    let record_source = fs::read_to_string(record_path)
        .expect("workflow record-pivot artifact should be readable");
    assert!(record_source.contains("# Workflow Pivot Record"));
    assert!(record_source.contains(&format!("**Source Plan:** `{plan_rel}`")));
    assert!(record_source.contains("**Reason:** plan revision superseded current execution"));
    assert!(record_source.contains("blocked_on_plan_revision"));
    assert!(record_source.contains("follow_up_override_record_pivot"));
    assert!(Path::new(record_path).starts_with(project_artifact_dir(repo, state)));

    let idempotent_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            plan_rel,
            "--reason",
            "plan revision superseded current execution",
            "--json",
        ],
        &[],
        "workflow record-pivot idempotent json",
    );
    assert_eq!(idempotent_json["action"], "already_current");
    assert_eq!(idempotent_json["record_path"], Value::from(record_path));
}

#[test]
fn workflow_record_pivot_blocks_out_of_phase() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let pivot_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            plan_rel,
            "--reason",
            "plan revision superseded current execution",
            "--json",
        ],
        &[],
        "workflow record-pivot blocked json",
    );

    assert_eq!(pivot_json["action"], "blocked");
    assert_eq!(pivot_json["record_path"], Value::Null);
    assert!(
        pivot_json["remediation"]
            .as_str()
            .is_some_and(|text| text.contains("pivot_required")),
        "{pivot_json:?}"
    );
}

#[test]
fn workflow_record_pivot_preserves_missing_qa_requirement_reason_code() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-missing-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        None,
        true,
    );
    remove_branch_test_plan_artifact(repo, state);

    let pivot_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            plan_rel,
            "--reason",
            "qa requirement metadata was missing",
            "--json",
        ],
        &[],
        "workflow record-pivot json for missing QA Requirement",
    );

    assert_eq!(pivot_json["action"], "recorded");
    let record_path = pivot_json["record_path"]
        .as_str()
        .expect("workflow record-pivot should emit record_path");
    let record_source = fs::read_to_string(record_path)
        .expect("workflow record-pivot artifact should be readable");
    assert!(record_source.contains("qa_requirement_missing_or_invalid"));
    assert!(record_source.contains("follow_up_override_record_pivot"));
}

#[test]
fn workflow_operator_routes_document_release_pending_to_record_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-document-release-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for document release pending",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(operator_json["review_state_status"], "missing_current_closure");
    assert_eq!(operator_json["next_action"], "record branch closure");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_record_qa_missing_current_closure_returns_blocked_follow_up() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-missing-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("missing-closure-qa-summary.md");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should block through workflow/operator when branch closure is missing",
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(qa_json["branch_closure_id"], "");
    assert_eq!(qa_json["required_follow_up"], "record_branch_closure");
    assert!(
        qa_json["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("workflow/operator"),
        "json: {qa_json:?}"
    );
}

#[test]
fn plan_execution_record_branch_closure_records_current_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure command should succeed",
    );
    let branch_closure_id = branch_closure_json["branch_closure_id"]
        .as_str()
        .expect("record-branch-closure should expose branch_closure_id");
    assert_eq!(branch_closure_json["action"], "recorded");
    let record_path = project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md"));
    let record_source =
        fs::read_to_string(&record_path).expect("branch-closure artifact should be readable");
    assert!(record_source.contains("**Contract Identity:** "));
    assert!(record_source.contains("**Current Reviewed State ID:** git_tree:"));
    assert!(record_source.contains("**Effective Reviewed Branch Surface:** repo_tracked_content"));
    assert!(record_source.contains("**Source Task Closure IDs:** task-closure-"));
    assert!(record_source.contains("**Provenance Basis:** task_closure_lineage"));
    assert!(record_source.contains("**Closure Status:** current"));
    assert!(record_source.contains("**Superseded Branch Closure IDs:** "));
    assert!(record_source.contains(&format!("**Branch Closure ID:** {branch_closure_id}")));

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after branch closure recording",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(operator_json["phase_detail"], "release_readiness_recording_ready");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        Value::from(branch_closure_id)
    );
}

#[test]
fn plan_execution_record_branch_closure_uses_recorded_task_closure_provenance() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-branch-closure-real-task-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for real branch provenance fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("real provenance fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("real-provenance-review-summary.md");
    let verification_summary_path = repo.join("real-provenance-verification-summary.md");
    write_file(&review_summary_path, "Task 1 independent review passed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("real provenance review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("real provenance verification summary path should be utf-8"),
        ],
        "close-current-task should succeed for real branch provenance fixture",
    );
    let closure_record_id = close_json["closure_record_id"]
        .as_str()
        .expect("real provenance fixture should expose closure record id")
        .to_owned();
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": dispatch_id.clone(),
                    "closure_record_id": closure_record_id.clone(),
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": format!(
                        "task-contract-{}",
                        sha256_hex(format!("{plan_rel}:task-1").as_bytes())
                    ),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"Task 1 independent review passed."),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(
                        b"Task 1 verification passed against the current reviewed state."
                    ),
                },
                "task-2": {
                    "dispatch_id": "fixture-task-dispatch-2",
                    "closure_record_id": "task-closure-synthetic-2",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": format!(
                        "task-contract-{}",
                        sha256_hex(format!("{plan_rel}:task-2").as_bytes())
                    ),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"Fixture task 2 review passed."),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(
                        b"Fixture task 2 verification passed for the current reviewed state."
                    ),
                }
            }),
        )],
    );

    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should use recorded task closure provenance",
    );
    let branch_closure_id = record_json["branch_closure_id"]
        .as_str()
        .expect("real provenance branch closure should expose an id")
        .to_owned();
    let record_source = fs::read_to_string(
        project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md")),
    )
    .expect("real provenance branch closure artifact should be readable");
    assert!(record_source.contains("task-closure-synthetic-2"));
    assert!(
        record_source.contains(&closure_record_id),
        "branch closure should reference the recorded task closure id: {record_source}"
    );
}

#[test]
fn plan_execution_record_branch_closure_re_records_when_reviewed_state_changes_at_same_head() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed",
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect("first record-branch-closure should expose branch_closure_id")
        .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure reviewed-state regression coverage",
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "second record-branch-closure should re-record after reviewed-state drift",
    );
    let second_branch_closure_id = second_branch_closure["branch_closure_id"]
        .as_str()
        .expect("second record-branch-closure should expose branch_closure_id");

    assert_eq!(second_branch_closure["action"], "recorded");
    assert_ne!(second_branch_closure_id, first_branch_closure_id);
    assert_eq!(
        second_branch_closure["superseded_branch_closure_ids"],
        Value::from(vec![first_branch_closure_id.clone()])
    );
    let explain = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should expose superseded branch closures after re-record",
    );
    assert!(
        explain["superseded_closures"].as_array().is_some_and(|closures| closures
            .iter()
            .any(|closure| closure == &Value::from(first_branch_closure_id.clone()))),
        "json: {explain:?}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after reviewed-state branch closure refresh",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(operator_json["phase_detail"], "release_readiness_recording_ready");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        Value::from(second_branch_closure_id)
    );
    let second_record_path =
        project_artifact_dir(repo, state).join(format!("branch-closure-{second_branch_closure_id}.md"));
    let second_record_source =
        fs::read_to_string(&second_record_path).expect("re-recorded branch-closure artifact should read");
    assert!(second_record_source.contains(
        "**Provenance Basis:** task_closure_lineage_plus_late_stage_surface_exemption"
    ));
    assert!(second_record_source.contains("**Source Task Closure IDs:** task-closure-"));
}

#[test]
fn plan_execution_record_branch_closure_allows_empty_source_task_closure_ids_for_late_stage_only_recreation() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-empty-late-stage-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed before late-stage-only recreation",
    );
    assert_eq!(first_branch_closure["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_task_closure_records", serde_json::json!({}))],
    );
    append_tracked_repo_line(
        repo,
        "README.md",
        "late-stage-only branch recreation without task closure provenance",
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should allow empty task provenance for late-stage-only recreation",
    );
    let second_branch_closure_id = second_branch_closure["branch_closure_id"]
        .as_str()
        .expect("late-stage-only recreation should expose a branch closure id")
        .to_owned();

    assert_eq!(second_branch_closure["action"], "recorded");
    let second_record_path =
        project_artifact_dir(repo, state).join(format!("branch-closure-{second_branch_closure_id}.md"));
    let second_record_source =
        fs::read_to_string(&second_record_path).expect("late-stage-only branch-closure artifact should read");
    assert!(second_record_source.contains(
        "**Provenance Basis:** task_closure_lineage_plus_late_stage_surface_exemption"
    ));
    assert!(second_record_source.contains(
        "**Effective Reviewed Branch Surface:** late_stage_surface_only:README.md"
    ));
    assert!(second_record_source.contains("**Source Task Closure IDs:** none"));
}

#[test]
fn plan_execution_record_branch_closure_blocks_first_entry_drift_outside_late_stage_surface() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-first-entry-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(
        repo,
        plan_rel,
        "Late-Stage Surface",
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md",
    );
    let baseline_tree_id = current_tracked_tree_id(repo);

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure first-entry drift outside trusted late-stage surface",
    );
    let drifted_tree_id = current_tracked_tree_id(repo);
    assert_ne!(baseline_tree_id, drifted_tree_id);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed on first late-stage entry when drift escapes the task-closure baseline",
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
}

#[test]
fn plan_execution_record_branch_closure_prefers_current_task_closure_set_baseline() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-current-task-set-baseline");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);

    write_repo_file(repo, "README.md", "task 2 still-current reviewed state\n");
    let task2_reviewed_state_id = current_tracked_tree_id(repo);
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "task 1 reopened reviewed state that should remain branch-current\n",
    );
    let task1_reviewed_state_id = current_tracked_tree_id(repo);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "reviewed_state_id": task1_reviewed_state_id,
                    "contract_identity": "task-1-contract",
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                },
                "task-2": {
                    "dispatch_id": "task-2-current-dispatch",
                    "closure_record_id": "task-2-current-closure",
                    "reviewed_state_id": task2_reviewed_state_id,
                    "contract_identity": "task-2-contract",
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 2 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 2 current verification"),
                }
            }),
        )],
    );

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should use the authoritative current task-closure set baseline",
    );

    assert_eq!(branch_closure["action"], "recorded");
    assert!(branch_closure["branch_closure_id"].is_string());
}

#[test]
fn plan_execution_record_branch_closure_re_records_when_contract_identity_changes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-contract-identity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed",
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect("first record-branch-closure should expose branch_closure_id")
        .to_owned();

    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["branch", "release-alt"]).current_dir(repo);
            command
        },
        "git branch release-alt for contract-identity regression",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args([
                    "config",
                    &format!("branch.{}.gh-merge-base", current_branch_name(repo)),
                    "release-alt",
                ])
                .current_dir(repo);
            command
        },
        "git config gh-merge-base for contract-identity regression",
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should re-record when branch contract identity changes",
    );
    let second_branch_closure_id = second_branch_closure["branch_closure_id"]
        .as_str()
        .expect("second record-branch-closure should expose branch_closure_id");

    assert_eq!(second_branch_closure["action"], "recorded");
    assert_ne!(second_branch_closure_id, first_branch_closure_id);
    assert_eq!(
        second_branch_closure["superseded_branch_closure_ids"],
        Value::from(vec![first_branch_closure_id])
    );
    let second_record_path =
        project_artifact_dir(repo, state).join(format!("branch-closure-{second_branch_closure_id}.md"));
    let second_record_source =
        fs::read_to_string(&second_record_path).expect("re-recorded branch-closure artifact should read");
    assert!(second_record_source.contains("**Contract Identity:** "));
}

#[test]
fn plan_execution_record_branch_closure_blocks_re_record_when_drift_escapes_late_stage_surface() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-blocks-untrusted-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed",
    );
    assert_eq!(first_branch_closure["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure drift outside trusted late-stage surface",
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "second record-branch-closure should fail closed when drift escapes Late-Stage Surface",
    );
    assert_eq!(second_branch_closure["action"], "blocked");
    assert_eq!(second_branch_closure["required_follow_up"], "repair_review_state");
}

#[test]
fn plan_execution_record_branch_closure_clears_stale_release_readiness_binding() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-clears-release");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_release_readiness_result", Value::from("blocked")),
            ("release_docs_state", Value::from("fresh")),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from("stale-release-docs-fingerprint"),
            ),
            ("final_review_state", Value::from("fresh")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from("stale-final-review-fingerprint"),
            ),
            ("browser_qa_state", Value::from("fresh")),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from("stale-browser-qa-fingerprint"),
            ),
            ("current_qa_branch_closure_id", Value::from("old-branch-closure")),
            ("current_qa_result", Value::from("pass")),
            ("current_qa_summary_hash", Value::from("stale-qa-summary-hash")),
        ],
    );

    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should clear stale release-readiness binding",
    );

    let branch_closure_id = branch_closure_json["branch_closure_id"]
        .as_str()
        .expect("record-branch-closure should expose branch_closure_id");
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after branch closure recording"),
    )
    .expect("authoritative state should remain valid json");
    assert_eq!(
        authoritative_state["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        authoritative_state["current_release_readiness_result"],
        Value::Null
    );
    assert_eq!(authoritative_state["release_docs_state"], Value::Null);
    assert_eq!(
        authoritative_state["last_release_docs_artifact_fingerprint"],
        Value::Null
    );
    assert_eq!(authoritative_state["final_review_state"], Value::Null);
    assert_eq!(
        authoritative_state["last_final_review_artifact_fingerprint"],
        Value::Null
    );
    assert_eq!(authoritative_state["browser_qa_state"], Value::Null);
    assert_eq!(
        authoritative_state["last_browser_qa_artifact_fingerprint"],
        Value::Null
    );
    assert_eq!(authoritative_state["current_qa_branch_closure_id"], Value::Null);
    assert_eq!(authoritative_state["current_qa_result"], Value::Null);
    assert_eq!(authoritative_state["current_qa_summary_hash"], Value::Null);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after clearing stale release-readiness binding",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(operator_json["phase_detail"], "release_readiness_recording_ready");
    assert_eq!(operator_json["next_action"], "advance late stage");
}

#[test]
fn plan_execution_advance_late_stage_records_release_readiness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-release-readiness-record");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure for release-readiness fixture",
    );
    assert_eq!(branch_closure_json["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(&summary_path, "Release readiness is green for the current branch closure.\n");
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage release-readiness command should succeed",
    );

    assert_eq!(release_json["action"], "recorded");
    assert_eq!(release_json["stage_path"], "release_readiness");
    assert_eq!(
        release_json["delegated_primitive"],
        "record-release-readiness"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after release-readiness recording",
    );
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(operator_json["phase_detail"], "final_review_dispatch_required");
}

#[test]
fn plan_execution_advance_late_stage_release_readiness_rerun_is_idempotent_and_conflicts_fail_closed() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-release-readiness-idempotency");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure for release-readiness idempotency fixture",
    );
    assert_eq!(branch_closure_json["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(&summary_path, "Release readiness is green for the current branch closure.\n");
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "first release-readiness recording should succeed",
    );
    assert_eq!(first["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("release_docs_state", Value::Null),
            ("last_release_docs_artifact_fingerprint", Value::Null),
        ],
    );
    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent release-readiness rerun should be idempotent",
    );
    assert_eq!(second["action"], "already_current");

    write_file(
        &summary_path,
        "Release readiness summary changed in structure.\nStill the same words.\n",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("release_docs_state", Value::Null),
            ("last_release_docs_artifact_fingerprint", Value::Null),
        ],
    );
    let conflicting = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "conflicting release-readiness rerun should fail closed",
    );
    assert_eq!(conflicting["action"], "blocked");
}

#[test]
fn workflow_operator_routes_blocked_release_readiness_to_resolution() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-release-blocker-resolution");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::from("branch-release-closure")),
            ("current_release_readiness_result", Value::from("blocked")),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for blocked release readiness",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_blocker_resolution_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "resolve release blocker");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_operator_routes_qa_pending_to_record_qa() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-qa-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for qa pending",
    );

    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "qa_recording_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["qa_requirement"], "required");
    assert_eq!(operator_json["next_action"], "run QA");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-qa --plan {plan_rel} --result pass|fail --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_operator_routes_test_plan_refresh_lane_to_plan_eng_review() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-test-plan-refresh");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    let safe_branch = branch_storage_key(&current_branch_name(repo));
    let test_plan_path = project_artifact_dir(repo, state)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    let test_plan_source =
        fs::read_to_string(&test_plan_path).expect("test-plan fixture should be readable");
    write_file(
        &test_plan_path,
        &test_plan_source.replace(
            "**Generated By:** featureforge:plan-eng-review",
            "**Generated By:** manual-test-plan-edit",
        ),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for test-plan refresh lane",
    );

    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "test_plan_refresh_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["qa_requirement"], "required");
    assert_eq!(operator_json["next_action"], "refresh test plan");
    assert_eq!(operator_json["recommended_command"], Value::Null);
}

#[test]
fn workflow_operator_routes_missing_qa_requirement_to_pivot_required() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        None,
        true,
    );
    remove_branch_test_plan_artifact(repo, state);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for missing QA Requirement",
    );

    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(operator_json["phase_detail"], "planning_reentry_required");
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow record-pivot --plan {plan_rel} --reason <reason>"
        ))
    );
}

#[test]
fn workflow_operator_routes_invalid_qa_requirement_to_pivot_required() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-invalid-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        Some("sometimes"),
        false,
    );
    remove_branch_test_plan_artifact(repo, state);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for invalid QA Requirement",
    );

    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(operator_json["phase_detail"], "planning_reentry_required");
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow record-pivot --plan {plan_rel} --reason <reason>"
        ))
    );
}

#[test]
fn workflow_operator_normalizes_mixed_case_qa_requirement() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-mixed-case-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        Some("  Not-Required  "),
        false,
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for mixed-case QA Requirement",
    );

    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["qa_requirement"], "not-required");
}

#[test]
fn gate_finish_allows_not_required_qa_without_current_test_plan_artifact() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("gate-finish-no-test-plan-when-qa-not-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json when QA is not required and test plan is absent",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should allow branch completion when QA is not required",
    );
    assert_eq!(gate_finish["allowed"], true);
}

#[test]
fn workflow_operator_routes_qa_pending_without_current_closure_to_record_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-qa-pending-missing-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute qa-pending missing-closure state",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(operator_json["review_state_status"], "missing_current_closure");
    assert_eq!(operator_json["next_action"], "record branch closure");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_record_qa_records_browser_qa_result() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, "Browser QA passed against the approved test plan.\n");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa command should succeed",
    );

    assert_eq!(qa_json["action"], "recorded");
    assert_eq!(qa_json["result"], "pass");
    assert_eq!(qa_json["branch_closure_id"], "branch-release-closure");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after QA recording",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
}

#[test]
fn plan_execution_record_qa_fail_returns_execution_reentry_follow_up() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-fail");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );

    let summary_path = repo.join("qa-fail-summary.md");
    write_file(&summary_path, "Browser QA found a blocker in the release flow.\n");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa fail command should return authoritative follow-up",
    );

    assert_eq!(qa_json["action"], "recorded");
    assert_eq!(qa_json["result"], "fail");
    assert_eq!(qa_json["required_follow_up"], "execution_reentry");
}

#[test]
fn plan_execution_record_qa_same_state_rerun_is_idempotent_and_conflicts_fail_closed() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-idempotent-rerun");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    let summary = "Browser QA found a blocker in the release flow.\n";
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::from("branch-release-closure")),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("fail")),
            (
                "current_qa_summary_hash",
                Value::from(sha256_hex(b"Browser QA found a blocker in the release flow.")),
            ),
        ],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, summary);

    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "seeded same-state failing record-qa rerun should be idempotent while QA is still current",
    );
    assert_eq!(second["action"], "already_current");
    assert_eq!(second["result"], "fail");
    assert_eq!(second["required_follow_up"], "execution_reentry");

    let conflict = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "conflicting same-state record-qa rerun should fail closed",
    );
    assert_eq!(conflict["action"], "blocked");
    assert!(
        conflict["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("conflicting recorded browser QA outcome"),
        "json: {conflict:?}"
    );
}

#[test]
fn plan_execution_record_qa_out_of_phase_refresh_lane_skips_summary_validation() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-refresh-summary-order");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );

    let missing_summary_path = repo.join("missing-qa-summary.md");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            missing_summary_path
                .to_str()
                .expect("summary path should be utf-8"),
        ],
        "out-of-phase record-qa should block before summary validation",
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(qa_json["required_follow_up"], Value::Null);
    assert!(
        qa_json["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("reroute through workflow/operator"),
        "json: {qa_json:?}"
    );
}

#[test]
fn plan_execution_record_qa_same_state_rerun_does_not_bypass_refresh_required_lane() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-refresh-lane-rerun");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    let summary = "Browser QA passed for the release flow.\n";
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::from("branch-release-closure")),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("pass")),
            (
                "current_qa_summary_hash",
                Value::from(sha256_hex(b"Browser QA passed for the release flow.")),
            ),
        ],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, summary);

    let safe_branch = branch_storage_key(&current_branch_name(repo));
    let test_plan_path = project_artifact_dir(repo, state)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    let test_plan_source =
        fs::read_to_string(&test_plan_path).expect("test-plan fixture should be readable");
    write_file(
        &test_plan_path,
        &test_plan_source.replace(
            "**Generated By:** featureforge:plan-eng-review",
            "**Generated By:** manual-test-plan-edit",
        ),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after shifting to the refresh-required lane",
    );
    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "test_plan_refresh_required");

    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "same-state rerun should not bypass refresh-required lane",
    );
    assert_eq!(second["action"], "blocked");
    assert_eq!(second["required_follow_up"], Value::Null);
    assert!(
        second["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("reroute through workflow/operator"),
        "operator: {operator_json:?}; json: {second:?}"
    );
}

#[test]
fn plan_execution_record_qa_same_summary_on_new_branch_closure_records_again() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-new-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure-a"))],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, "Browser QA found a blocker in the release flow.\n");
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "initial record-qa invocation for closure A should record",
    );
    assert_eq!(first["action"], "recorded");
    assert_eq!(first["branch_closure_id"], "branch-release-closure-a");
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let first_project_artifacts: Vec<PathBuf> = fs::read_dir(project_artifact_dir(repo, state))
        .expect("project artifact dir should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("-test-outcome-"))
        })
        .collect();
    let first_project_artifact_count = first_project_artifacts.len();
    assert!(
        first_project_artifact_count > 0,
        "closure A should append a QA project artifact"
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure-b"))],
    );
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after switching to a new branch closure",
    );
    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "same summary on a new branch closure should record a new QA outcome",
    );

    assert_ne!(second["action"], "already_current");
    assert_eq!(second["branch_closure_id"], "branch-release-closure-b");
    assert!(
        !second["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("conflicting recorded browser QA outcome"),
        "json: {second:?}"
    );
    if operator_json["phase"] == "qa_pending" && operator_json["phase_detail"] == "qa_recording_required"
    {
        assert_eq!(second["action"], "recorded");
        let second_authoritative_state: Value = serde_json::from_str(
            &fs::read_to_string(&authoritative_state_path)
                .expect("qa authoritative state should read after closure B"),
        )
        .expect("qa authoritative state should remain valid json after closure B");
        assert_eq!(
            second_authoritative_state["current_qa_branch_closure_id"],
            Value::from("branch-release-closure-b")
        );
        let second_project_artifacts: Vec<PathBuf> = fs::read_dir(project_artifact_dir(repo, state))
            .expect("project artifact dir should be readable after closure B")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains("-test-outcome-"))
            })
            .collect();
        for artifact in &first_project_artifacts {
            assert!(
                artifact.exists(),
                "closure A QA artifact should remain after closure B records: {}",
                artifact.display()
            );
            assert!(
                second_project_artifacts.iter().any(|candidate| candidate == artifact),
                "closure A QA artifact should remain listed after closure B records: {}",
                artifact.display()
            );
        }
        let second_project_artifact_count = second_project_artifacts.len();
        assert!(
            second_project_artifact_count > first_project_artifact_count,
            "a new QA artifact should be appended for closure B"
        );
        assert_eq!(
            second_authoritative_state["current_qa_summary_hash"],
            Value::from(sha256_hex(b"Browser QA found a blocker in the release flow."))
        );
    } else {
        assert_eq!(second["action"], "blocked");
        let blocked_authoritative_state: Value = serde_json::from_str(
            &fs::read_to_string(&authoritative_state_path)
                .expect("qa authoritative state should read after blocked closure B attempt"),
        )
        .expect("qa authoritative state should remain valid json after blocked closure B attempt");
        assert_eq!(
            blocked_authoritative_state["current_qa_branch_closure_id"],
            Value::from("branch-release-closure-a")
        );
        for artifact in &first_project_artifacts {
            assert!(
                artifact.exists(),
                "closure A QA artifact should remain after blocked closure B record: {}",
                artifact.display()
            );
        }
        assert!(
            second["required_follow_up"] == "repair_review_state"
                || second["required_follow_up"] == Value::Null,
            "operator: {operator_json:?}; json: {second:?}"
        );
    }
}

#[test]
fn plan_execution_record_qa_stale_unreviewed_requires_repair_review_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-stale-unreviewed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, "Browser QA passed against the approved test plan.\n");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should record before stale-unreviewed repo changes",
    );
    assert_eq!(qa_json["action"], "recorded");

    let readme_path = repo.join("README.md");
    let original_readme = fs::read_to_string(&readme_path).expect("README.md should exist");
    write_file(
        &readme_path,
        &format!("{original_readme}\npost-qa tracked change\n"),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after post-QA tracked repo change",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should surface the exact stale-unreviewed reroute",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "execution_reentry");
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );

    let blocked = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should fail closed when QA is stale-unreviewed",
    );
    assert_eq!(blocked["action"], "blocked");
    assert_eq!(blocked["required_follow_up"], "repair_review_state");
}

#[test]
fn plan_execution_repair_review_state_reroutes_late_stage_surface_only_drift_to_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-late-stage-reroute");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a real current branch closure before late-stage reroute coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    let summary_path = repo.join("release-readiness-late-stage-reroute.md");
    write_file(
        &summary_path,
        "Release readiness passed before trusted late-stage-only drift.\n",
    );
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before trusted late-stage drift coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    append_tracked_repo_line(repo, "README.md", "late-stage-only branch drift");
    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should reroute trusted late-stage-only drift to branch closure re-recording",
    );

    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(
        repair_json["required_follow_up"],
        "record_branch_closure",
        "json: {repair_json:?}"
    );
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_repair_review_state_routes_escaped_drift_to_execution_reentry() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-escaped-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", plan_rel);
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a real current branch closure before escaped-drift coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    let summary_path = repo.join("release-readiness-escaped-drift.md");
    write_file(
        &summary_path,
        "Release readiness passed before escaped branch drift.\n",
    );
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before escaped-drift coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "escaped branch drift outside trusted late-stage surface",
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route escaped late-stage drift back to execution reentry",
    );

    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "execution_reentry");
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
}

#[test]
fn plan_execution_reconcile_review_state_restores_missing_branch_closure_overlay() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-restores-branch-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before reconcile coverage",
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_before: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable before overlay corruption"),
    )
    .expect("authoritative state should remain valid json before overlay corruption");
    let expected_reviewed_state_id = authoritative_state_before["current_branch_closure_reviewed_state_id"]
        .as_str()
        .expect("branch closure should seed reviewed state overlay")
        .to_owned();
    let expected_contract_identity = authoritative_state_before["current_branch_closure_contract_identity"]
        .as_str()
        .expect("branch closure should seed contract identity overlay")
        .to_owned();

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should rebuild missing current branch closure overlays",
    );

    assert_eq!(reconcile["action"], "reconciled");
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after reconcile"),
    )
    .expect("authoritative state should remain valid json after reconcile");
    assert_eq!(
        authoritative_state_after["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_reviewed_state_id"],
        Value::from(expected_reviewed_state_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_contract_identity"],
        Value::from(expected_contract_identity)
    );
}

#[test]
fn plan_execution_reconcile_review_state_restores_branch_overlay_without_branch_closure_markdown() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-restores-authoritative-branch-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before authoritative overlay restore coverage",
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_before: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable before overlay corruption"),
    )
    .expect("authoritative state should remain valid json before overlay corruption");
    let expected_reviewed_state_id = authoritative_state_before["current_branch_closure_reviewed_state_id"]
        .as_str()
        .expect("branch closure should seed reviewed state overlay")
        .to_owned();
    let expected_contract_identity = authoritative_state_before["current_branch_closure_contract_identity"]
        .as_str()
        .expect("branch closure should seed contract identity overlay")
        .to_owned();

    let branch_closure_path =
        project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md"));
    fs::remove_file(&branch_closure_path)
        .expect("authoritative overlay restore test should remove the rendered branch-closure artifact");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should rebuild missing current branch closure overlays from authoritative state",
    );

    assert_eq!(reconcile["action"], "reconciled");
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after reconcile"),
    )
    .expect("authoritative state should remain valid json after reconcile");
    assert_eq!(
        authoritative_state_after["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_reviewed_state_id"],
        Value::from(expected_reviewed_state_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_contract_identity"],
        Value::from(expected_contract_identity)
    );
}

#[test]
fn plan_execution_reconcile_review_state_preserves_release_readiness_while_restoring_overlay() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-preserves-release-readiness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before reconcile preservation coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(&summary_path, "Release readiness is still current.\n");
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release-readiness before reconcile preservation coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should restore missing overlays without clearing release-readiness",
    );
    assert_eq!(reconcile["action"], "reconciled");

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after reconcile"),
    )
    .expect("authoritative state should remain valid json after reconcile");
    assert_eq!(
        authoritative_state_after["current_release_readiness_result"],
        Value::from("ready")
    );
    assert_eq!(
        authoritative_state_after["release_docs_state"],
        Value::from("fresh")
    );
}

#[test]
fn plan_execution_repair_review_state_reports_reconciled_after_overlay_restore() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-reconciles-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before repair reconcile coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should report reconciled after restoring derivable overlays",
    );

    assert_eq!(repair["action"], "reconciled");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert_eq!(
        repair["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
}

#[test]
fn plan_execution_repair_review_state_restores_overlay_from_authoritative_branch_record() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-blocks-unrestorable-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before unrestorable repair coverage",
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id");
    write_file(
        &project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md")),
        "# Branch Closure\n\ncorrupted fixture without derivable overlay headers\n",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should restore missing overlays from the authoritative branch record even if markdown is corrupted",
    );

    assert_eq!(repair["action"], "reconciled");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert_eq!(
        repair["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    assert_eq!(
        repair["missing_derived_overlays"],
        Value::from(Vec::<String>::new())
    );
}

#[test]
fn plan_execution_reconcile_review_state_restores_missing_branch_overlay_while_stale() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-restores-stale-branch-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before stale reconcile coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    append_tracked_repo_line(
        repo,
        "README.md",
        "stale reconcile overlay restoration coverage",
    );

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_before: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable before stale overlay corruption"),
    )
    .expect("authoritative state should remain valid json before stale overlay corruption");
    let expected_reviewed_state_id = authoritative_state_before["current_branch_closure_reviewed_state_id"]
        .as_str()
        .expect("branch closure should seed reviewed state overlay before stale corruption")
        .to_owned();
    let expected_contract_identity = authoritative_state_before["current_branch_closure_contract_identity"]
        .as_str()
        .expect("branch closure should seed contract identity overlay before stale corruption")
        .to_owned();

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should restore derivable branch overlays even when the branch state is stale",
    );

    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after stale reconcile"),
    )
    .expect("authoritative state should remain valid json after stale reconcile");
    assert_eq!(
        authoritative_state_after["current_branch_closure_reviewed_state_id"],
        Value::from(expected_reviewed_state_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_contract_identity"],
        Value::from(expected_contract_identity)
    );
}

#[test]
fn plan_execution_reconcile_review_state_stale_only_does_not_claim_restore() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-stale-only");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before stale-only reconcile coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    append_tracked_repo_line(repo, "README.md", "stale reconcile without overlay corruption");

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should not claim overlay restoration when no derived overlays were missing",
    );

    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(reconcile["actions_performed"], Value::from(Vec::<String>::new()));
    assert_eq!(
        reconcile["trace_summary"],
        Value::from(
            "Reviewed state is stale_unreviewed; no derivable overlays required reconciliation.",
        )
    );
}

#[test]
fn plan_execution_record_qa_blocks_when_test_plan_refresh_is_required() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-refresh-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_branch_closure_id", Value::from("branch-release-closure"))],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, "Browser QA passed after manual verification.\n");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa command should fail closed when test-plan refresh is required",
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(qa_json["result"], "pass");
    assert_eq!(qa_json["required_follow_up"], Value::Null);
    assert!(
        qa_json["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("reroute through workflow/operator"),
        "json: {qa_json:?}"
    );
}

#[test]
fn workflow_operator_routes_pivot_required_to_record_pivot() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-pivot-plan-block");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for pivot-required plan-revision block",
    );

    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(operator_json["phase_detail"], "planning_reentry_required");
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow record-pivot --plan {plan_rel} --reason <reason>"
        ))
    );
}

fn display_json_array(value: &Value) -> String {
    value
        .as_array()
        .map(|items| {
            if items.is_empty() {
                String::from("none")
            } else {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        })
        .unwrap_or_else(|| String::from("none"))
}

fn display_json_optional_str(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| String::from("none"))
}

#[test]
fn workflow_handoff_and_doctor_text_and_json_surfaces_match_harness_evaluator_and_reason_metadata()
{
    let (repo_dir, state_dir) = init_repo("workflow-doctor-handoff-metadata-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let doctor_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "doctor", "--json"],
        &[],
        "workflow doctor json for shell-smoke metadata parity",
    );
    let handoff_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "handoff", "--json"],
        &[],
        "workflow handoff json for shell-smoke metadata parity",
    );
    let doctor_text_output = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "doctor"],
        &[],
        "workflow doctor text for shell-smoke metadata parity",
    );
    assert!(doctor_text_output.status.success());
    let doctor_text = String::from_utf8_lossy(&doctor_text_output.stdout);
    let handoff_text_output = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "handoff"],
        &[],
        "workflow handoff text for shell-smoke metadata parity",
    );
    assert!(handoff_text_output.status.success());
    let handoff_text = String::from_utf8_lossy(&handoff_text_output.stdout);

    let execution_status = doctor_json["execution_status"]
        .as_object()
        .expect("workflow doctor json should expose execution_status object");
    let write_authority_state = execution_status
        .get("write_authority_state")
        .and_then(Value::as_str)
        .expect("workflow doctor json should expose write_authority_state");
    let write_authority_holder =
        display_json_optional_str(execution_status.get("write_authority_holder"));
    let write_authority_worktree =
        display_json_optional_str(execution_status.get("write_authority_worktree"));
    let reason_codes = display_json_array(
        execution_status
            .get("reason_codes")
            .expect("workflow doctor json should expose reason_codes"),
    );
    let required_evaluators = display_json_array(
        execution_status
            .get("required_evaluator_kinds")
            .expect("workflow doctor json should expose required_evaluator_kinds"),
    );
    let completed_evaluators = display_json_array(
        execution_status
            .get("completed_evaluator_kinds")
            .expect("workflow doctor json should expose completed_evaluator_kinds"),
    );
    let pending_evaluators = display_json_array(
        execution_status
            .get("pending_evaluator_kinds")
            .expect("workflow doctor json should expose pending_evaluator_kinds"),
    );
    let non_passing_evaluators = display_json_array(
        execution_status
            .get("non_passing_evaluator_kinds")
            .expect("workflow doctor json should expose non_passing_evaluator_kinds"),
    );
    let last_evaluator =
        display_json_optional_str(execution_status.get("last_evaluation_evaluator_kind"));
    let finish_reason_codes = display_json_array(
        doctor_json["gate_finish"]
            .get("reason_codes")
            .expect("workflow doctor json should expose gate_finish reason_codes"),
    );

    assert!(doctor_text.contains(&format!(
        "Phase: {}",
        doctor_json["phase"]
            .as_str()
            .expect("workflow doctor json should expose phase"),
    )));
    assert!(doctor_text.contains(&format!(
        "Next action: {}",
        doctor_json["next_action"]
            .as_str()
            .expect("workflow doctor json should expose next_action"),
    )));
    assert!(doctor_text.contains(&format!("Execution reason codes: {reason_codes}")));
    assert!(doctor_text.contains(&format!("Evaluator required kinds: {required_evaluators}")));
    assert!(doctor_text.contains(&format!(
        "Evaluator completed kinds: {completed_evaluators}"
    )));
    assert!(doctor_text.contains(&format!("Evaluator pending kinds: {pending_evaluators}")));
    assert!(doctor_text.contains(&format!(
        "Evaluator non-passing kinds: {non_passing_evaluators}"
    )));
    assert!(doctor_text.contains(&format!("Evaluator last kind: {last_evaluator}")));
    assert!(doctor_text.contains(&format!("Write authority state: {write_authority_state}")));
    assert!(doctor_text.contains(&format!("Write authority holder: {write_authority_holder}")));
    assert!(doctor_text.contains(&format!(
        "Write authority worktree: {write_authority_worktree}"
    )));
    assert!(doctor_text.contains(&format!("Finish gate reason codes: {finish_reason_codes}")));

    assert!(handoff_text.contains(&format!(
        "Phase: {}",
        handoff_json["phase"]
            .as_str()
            .expect("workflow handoff json should expose phase"),
    )));
    assert!(handoff_text.contains(&format!(
        "Next action: {}",
        handoff_json["next_action"]
            .as_str()
            .expect("workflow handoff json should expose next_action"),
    )));
    assert!(handoff_text.contains(&format!("Execution reason codes: {reason_codes}")));
    assert!(handoff_text.contains(&format!("Evaluator required kinds: {required_evaluators}")));
    assert!(handoff_text.contains(&format!("Write authority state: {write_authority_state}")));
    assert!(handoff_text.contains(&format!("Write authority holder: {write_authority_holder}")));
    assert!(handoff_text.contains(&format!(
        "Write authority worktree: {write_authority_worktree}"
    )));
    assert!(handoff_text.contains(&format!(
        "Reason: {}",
        handoff_json["recommendation_reason"]
            .as_str()
            .expect("workflow handoff json should expose recommendation_reason")
    )));
}

#[test]
fn workflow_phase_doctor_handoff_json_parity_for_pivot_required_plan_revision_block() {
    let (repo_dir, state_dir) = init_repo("workflow-shell-smoke-pivot-plan-block");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let phase_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "phase", "--json"],
        &[],
        "workflow phase json for shell-smoke pivot plan-block parity",
    );
    let doctor_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "doctor", "--json"],
        &[],
        "workflow doctor json for shell-smoke pivot plan-block parity",
    );
    let handoff_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "handoff", "--json"],
        &[],
        "workflow handoff json for shell-smoke pivot plan-block parity",
    );

    assert_eq!(phase_json["phase"], "pivot_required");
    assert_eq!(doctor_json["phase"], phase_json["phase"]);
    assert_eq!(handoff_json["phase"], phase_json["phase"]);
    assert_eq!(phase_json["next_action"], "plan_update");
    assert_eq!(doctor_json["next_action"], phase_json["next_action"]);
    assert_eq!(handoff_json["next_action"], phase_json["next_action"]);
}

#[test]
fn featureforge_cutover_gate_rejects_active_legacy_root_content() {
    let (repo_dir, _state_dir) = init_repo("cutover-active-content");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "featureforge-upgrade/SKILL.md",
        "Do not use ~/.codex/featureforge for active FeatureForge installs.\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on active legacy-root content\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active content references:"));
    assert!(stderr.contains("featureforge-upgrade/SKILL.md:1"));
}

#[test]
fn featureforge_cutover_gate_rejects_punctuation_delimited_legacy_root_content() {
    let (repo_dir, _state_dir) = init_repo("cutover-punctuation-content");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "docs/runtime.md",
        "Retired paths like (~/.codex/featureforge) or ~/.copilot/featureforge, must stay blocked.\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on punctuation-delimited legacy-root content\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active content references:"));
    assert!(stderr.contains("docs/runtime.md:1"));
}

#[test]
fn featureforge_cutover_gate_scans_repo_wide_tracked_files() {
    let (repo_dir, _state_dir) = init_repo("cutover-repo-bounded");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "src/reintroduced.rs",
        "legacy = \"~/.codex/featureforge/runtime\"\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on legacy-root content anywhere in tracked active files\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active content references:"));
    assert!(stderr.contains("src/reintroduced.rs:"));
}

#[test]
fn featureforge_cutover_gate_rejects_active_legacy_root_paths() {
    let (repo_dir, _state_dir) = init_repo("cutover-active-path");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        ".codex/featureforge/INSTALL.md",
        "retired path should be blocked\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on active legacy-root paths\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active path names:"));
    assert!(stderr.contains(".codex/featureforge/INSTALL.md"));
}

#[test]
fn featureforge_cutover_gate_allows_archived_legacy_root_history() {
    let (repo_dir, _state_dir) = init_repo("cutover-archive-allowed");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "docs/archive/featureforge/legacy-root-history.md",
        "Historical notes may mention ~/.codex/featureforge and ~/.copilot/featureforge.\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        output.status.success(),
        "cutover check should ignore docs/archive legacy-root history\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "featureforge cutover checks passed"
    );
}

#[test]
fn featureforge_cutover_gate_uses_one_repo_wide_content_scan() {
    let (repo_dir, _state_dir) = init_repo("cutover-single-pass");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(repo, "src/one.rs", "const ONE: &str = \"clean\";\n");
    write_repo_file(repo, "src/two.rs", "const TWO: &str = \"clean\";\n");
    write_repo_file(repo, "docs/guide.md", "still clean\n");
    git_add_all(repo);

    let wrapper_root = TempDir::new().expect("wrapper tempdir should exist");
    let wrapper_bin = wrapper_root.path().join("bin");
    fs::create_dir_all(&wrapper_bin).expect("wrapper bin dir should exist");
    let grep_log = wrapper_root.path().join("grep.log");
    let grep_path = wrapper_bin.join("grep");
    let real_grep = Command::new("sh")
        .arg("-c")
        .arg("command -v grep")
        .output()
        .expect("real grep path should resolve");
    let real_grep = String::from_utf8_lossy(&real_grep.stdout).trim().to_owned();
    assert!(!real_grep.is_empty(), "real grep path should not be empty");
    write_repo_file(
        wrapper_root.path(),
        "bin/grep",
        &format!(
            "#!/usr/bin/env bash\nprintf 'grep %s\\n' \"$*\" >> \"{}\"\nexec \"{}\" \"$@\"\n",
            grep_log.display(),
            real_grep
        ),
    );
    make_executable(&grep_path);

    let existing_path = std::env::var("PATH").expect("PATH should exist");
    let wrapper_path = format!("{}:{}", wrapper_bin.display(), existing_path);
    let output = run_cutover_check_with_env(repo, &[("PATH", wrapper_path.as_str())]);
    assert!(
        output.status.success(),
        "cutover check should stay green under rg instrumentation\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let grep_invocations = fs::read_to_string(&grep_log).expect("grep log should exist");
    let content_scan_lines = grep_invocations
        .lines()
        .filter(|line| line.contains("grep -nH -E "))
        .collect::<Vec<_>>();
    assert_eq!(
        content_scan_lines.len(),
        1,
        "cutover content scanning should stay repo-bounded and single-pass instead of spawning one scan per tracked file: {content_scan_lines:?}"
    );
}
