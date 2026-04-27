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

fn assert_file_not_contains(path: PathBuf, needle: &str) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    assert!(
        !source.contains(needle),
        "{} should not contain {:?}",
        path.display(),
        needle
    );
}

#[test]
fn skill_docs_route_plan_review_through_independent_fidelity_gate() {
    let root = repo_root();

    assert_file_contains(
        root.join("skills/plan-fidelity-review/SKILL.md"),
        "name: plan-fidelity-review",
    );
    assert_file_contains(
        root.join("skills/plan-fidelity-review/SKILL.md"),
        "independent fresh-context subagent",
    );
    assert_file_contains(
        root.join("skills/plan-fidelity-review/SKILL.md"),
        "Do not call removed workflow helper commands from this skill.",
    );
    assert_file_not_contains(
        root.join("skills/plan-fidelity-review/SKILL.md"),
        "workflow plan-fidelity record",
    );
    assert_file_not_contains(
        root.join("skills/plan-fidelity-review/SKILL.md"),
        "Reviewer Source: cross-model",
    );
    assert_file_contains(
        root.join("skills/plan-fidelity-review/reviewer-prompt.md"),
        "**Review Stage:** featureforge:plan-fidelity-review",
    );
    assert_file_contains(
        root.join("skills/plan-fidelity-review/reviewer-prompt.md"),
        "**Reviewer Source:** fresh-context-subagent",
    );
    assert_file_contains(
        root.join("skills/plan-fidelity-review/reviewer-prompt.md"),
        "**Verified Surfaces:** requirement_index, execution_topology, task_contract, task_determinism, spec_reference_fidelity",
    );
    assert_file_contains(
        root.join("skills/plan-fidelity-review/reviewer-prompt.md"),
        "TASK_DONE_WHEN_NON_DETERMINISTIC",
    );
    assert_file_contains(
        root.join("skills/plan-fidelity-review/SKILL.md"),
        "Review artifacts missing any required verified surface are stale or invalid for the expanded plan-fidelity gate",
    );
    assert_file_not_contains(
        root.join("skills/plan-fidelity-review/reviewer-prompt.md"),
        "Reviewer Source: cross-model",
    );
    assert_file_contains(
        root.join("README.md"),
        "featureforge:brainstorming -> featureforge:plan-ceo-review -> featureforge:writing-plans -> featureforge:plan-fidelity-review -> featureforge:plan-eng-review -> implementation",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "featureforge:brainstorming -> featureforge:plan-ceo-review -> featureforge:writing-plans -> featureforge:plan-fidelity-review -> featureforge:plan-eng-review",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "featureforge:brainstorming -> featureforge:plan-ceo-review -> featureforge:writing-plans -> featureforge:plan-fidelity-review -> featureforge:plan-eng-review",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "plan-ceo-review -> writing-plans -> plan-fidelity-review -> plan-eng-review -> execution.",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "Plan exists, is `Draft`, `Last Reviewed By` is `plan-eng-review`, and the current plan-fidelity review artifact is missing, stale, malformed, non-pass, or non-independent: invoke `featureforge:plan-fidelity-review`.",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "Plan exists, is `Draft`, `Last Reviewed By` is `plan-eng-review`, and has a matching pass plan-fidelity review artifact: invoke `featureforge:plan-eng-review`.",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "Invoke `featureforge:plan-fidelity-review`.",
    );
    assert_file_not_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "dispatch or resume a dedicated independent plan-fidelity reviewer before `plan-eng-review` becomes reachable.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Start engineering review for a structurally parseable draft even when no plan-fidelity review artifact exists yet.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "If the spec is not workflow-valid `CEO Approved` with `**Last Reviewed By:** plan-ceo-review`, stop and direct the agent back to `featureforge:plan-ceo-review`.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Do not look for or require a runtime-owned plan-fidelity receipt. The authoritative fidelity evidence is the parseable review artifact surfaced by workflow routing and `plan contract analyze-plan` as `plan_fidelity_review`.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Engineering approval must also fail closed unless `task_contract_valid`, `task_goal_valid`, `task_context_sufficient`, `task_constraints_valid`, `task_done_when_deterministic`, and `tasks_self_contained` are all `true`.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Do not use legacy task-level `Open Questions` review as the primary approval model after cutover.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "fails to name the shared implementation home when reuse is required",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/accelerated-reviewer-prompt.md"),
        "preserving the normal engineering hard-fail law for `task_contract_valid`, `task_goal_valid`, `task_context_sufficient`, `task_constraints_valid`, `task_done_when_deterministic`, and `tasks_self_contained`",
    );
    assert_file_contains(
        root.join("review/review-accelerator-packet-contract.md"),
        "analyze-plan boolean snapshot for `task_contract_valid`, `task_goal_valid`, `task_context_sufficient`, `task_constraints_valid`, `task_done_when_deterministic`, and `tasks_self_contained`",
    );
}
