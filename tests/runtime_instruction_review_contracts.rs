use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn assert_file_contains(path: PathBuf, needle: &str) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    assert!(
        source.contains(needle),
        "{} should contain {:?}",
        path.display(),
        needle
    );
}

#[test]
fn review_skill_docs_keep_final_review_dedicated_and_gate_aware() {
    let root = repo_root();

    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "final cross-task review gate",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "featureforge:requesting-code-review",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "dedicated fresh-context reviewer independent of the implementation context",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Reviewer Provenance:** dedicated-independent",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Reviewer Source:** fresh-context-subagent",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Reviewer ID:** 019d3550-c932-7bb2-9903-33f68d7c30ca",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Reviewer Artifact Path:** `$_SP_STATE_DIR/projects/$SLUG/{user}-{safe-branch}-independent-review-{datetime}.md`",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Reviewer Artifact Fingerprint:**",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Recorded Execution Deviations:** none",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Deviation Review Verdict:** not_required",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Branch:** feature/foo",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Repo:** featureforge",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Strategy Checkpoint Fingerprint:**",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "approved plan",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "dedicated independent reviewer",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "explicitly inspect them and state whether those deviations pass final review",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Dedicated Reviewer Receipt Contract",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "include structured receipt-ready metadata in your response",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Source Plan`, `Source Plan Revision`, `Strategy Checkpoint Fingerprint`, `Branch`, `Repo`, `Base Branch`, `Head SHA`",
    );
}

#[test]
fn late_stage_skill_docs_describe_scope_check_distribution_and_versioning_contracts() {
    let root = repo_root();

    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Scope Check",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "DRIFT_DETECTED",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "REQUIREMENTS_MISSING",
    );
    assert_file_contains(
        root.join("skills/document-release/SKILL.md"),
        "Versioning Decision",
    );
    assert_file_contains(
        root.join("skills/document-release/SKILL.md"),
        "publishability",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "distribution",
    );
    assert_file_contains(
        root.join("skills/systematic-debugging/SKILL.md"),
        "debug report",
    );
    assert_file_contains(
        root.join("skills/receiving-code-review/SKILL.md"),
        "mechanical low-risk",
    );
    assert_file_contains(
        root.join("skills/verification-before-completion/SKILL.md"),
        "scope-check",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "dynamic required gates",
    );
}

#[test]
fn late_stage_skill_docs_describe_security_review_gate_contract() {
    let root = repo_root();

    assert_file_contains(
        root.join("skills/security-review/SKILL.md"),
        "post-implementation",
    );
    assert_file_contains(
        root.join("skills/security-review/SKILL.md"),
        "runtime-owned",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "security-review",
    );
    assert_file_contains(
        root.join("skills/security-review/SKILL.md"),
        "Artifact Kind: security-review",
    );
    assert_file_contains(
        root.join("skills/security-review/SKILL.md"),
        "Schema Version: 1",
    );
    assert_file_contains(
        root.join("skills/security-review/SKILL.md"),
        "Artifact Provenance: runtime-owned",
    );
    assert_file_contains(
        root.join("skills/security-review/SKILL.md"),
        "Retention Policy: featureforge:authoritative-runtime-artifact",
    );
    assert_file_contains(
        root.join("skills/security-review/SKILL.md"),
        "Execution Diff Fingerprint",
    );
    assert_file_contains(
        root.join("skills/plan-design-review/SKILL.md"),
        "Artifact Kind: plan-design-review",
    );
    assert_file_contains(
        root.join("skills/plan-design-review/SKILL.md"),
        "Schema Version: 1",
    );
    assert_file_contains(
        root.join("skills/plan-design-review/SKILL.md"),
        "Artifact Provenance: runtime-owned",
    );
    assert_file_contains(
        root.join("skills/plan-design-review/SKILL.md"),
        "Retention Policy: featureforge:authoritative-runtime-artifact",
    );
    assert_file_contains(
        root.join("skills/plan-design-review/SKILL.md"),
        "Generated At",
    );
}
