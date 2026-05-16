use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_file(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

fn assert_file_contains(path: PathBuf, needle: &str) {
    let source = read_file(&path);
    assert!(
        source.contains(needle),
        "{} should contain {:?}",
        path.display(),
        needle
    );
}

fn assert_file_does_not_contain(path: PathBuf, needle: &str) {
    let source = read_file(&path);
    assert!(
        !source.contains(needle),
        "{} should not contain {:?}",
        path.display(),
        needle
    );
}

fn dispatch_prompt_payload(path: &Path) -> String {
    let source = read_file(path);
    let marker = "  prompt: |\n";
    let start = source.find(marker).unwrap_or_else(|| {
        panic!(
            "{} should contain a dispatch prompt payload",
            path.display()
        )
    }) + marker.len();
    let payload = &source[start..];
    let end = payload.find("\n```").unwrap_or_else(|| {
        panic!(
            "{} dispatch prompt payload should close before fence",
            path.display()
        )
    });
    payload[..end].to_owned()
}

fn assert_text_contains(label: &str, source: &str, needle: &str) {
    assert!(source.contains(needle), "{label} should contain {needle:?}");
}

fn assert_text_does_not_contain(label: &str, source: &str, needle: &str) {
    assert!(
        !source.contains(needle),
        "{label} should not contain {needle:?}"
    );
}

fn normalized_prompt_block(source: &str) -> String {
    source
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn canonical_reviewer_recursion_rule(root: &Path) -> String {
    read_file(&root.join("references/reviewer-recursion-rule.md"))
        .trim()
        .to_owned()
}

fn assert_canonical_reviewer_recursion_rule_is_strong(root: &Path) {
    let source = canonical_reviewer_recursion_rule(root);
    assert_text_contains(
        "canonical reviewer recursion rule",
        &source,
        "# Review-subagent recursion rule",
    );
    assert_text_contains(
        "canonical reviewer recursion rule",
        &source,
        "You are a reviewer.",
    );
    assert_text_contains(
        "canonical reviewer recursion rule",
        &source,
        "You may inspect the provided files, packet, summaries, and context and produce review findings.",
    );
    assert_text_contains(
        "canonical reviewer recursion rule",
        &source,
        "Do not launch, request, or delegate to additional subagents while performing this review.",
    );
    assert_text_contains(
        "canonical reviewer recursion rule",
        &source,
        "Do not delegate this review to another reviewer agent.",
    );
    for skill_name in [
        "`subagent-driven-development`",
        "`requesting-code-review`",
        "`plan-fidelity-review`",
        "`plan-eng-review`",
        "`plan-ceo-review`",
    ] {
        assert_text_contains("canonical reviewer recursion rule", &source, skill_name);
    }
    assert_text_contains(
        "canonical reviewer recursion rule",
        &source,
        "return a blocked review finding that names the missing context instead of spawning another agent.",
    );
}

fn assert_prompt_contains_canonical_recursion_rule(root: &Path, path: &Path) {
    let source = read_file(path);
    let canonical = canonical_reviewer_recursion_rule(root);
    assert!(
        normalized_prompt_block(&source).contains(&normalized_prompt_block(&canonical)),
        "{} should include the canonical reviewer recursion rule",
        path.display()
    );
}

fn assert_prompt_has_no_runtime_environment_guards(path: &Path) {
    assert_file_does_not_contain(
        path.to_path_buf(),
        "FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED",
    );
    assert_file_does_not_contain(path.to_path_buf(), "FEATUREFORGE_REVIEWER_CONTEXT");
    assert_file_does_not_contain(path.to_path_buf(), "ReviewerRuntimeCommandForbidden");
    assert_file_does_not_contain(path.to_path_buf(), "REVIEWER_RUNTIME_COMMANDS_ALLOWED: no");
    assert_file_does_not_contain(path.to_path_buf(), "runtime command guard");
    assert_file_does_not_contain(path.to_path_buf(), "reviewer-mode environment");
}

fn collect_rust_source_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()));
    for entry in entries {
        let entry = entry
            .unwrap_or_else(|error| panic!("{} entry should be readable: {error}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_rust_source_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn runtime_rust_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_source_files(&root.join("src"), &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "{} should contain Rust source files",
        root.join("src").display()
    );
    files
}

fn assert_runtime_sources_do_not_enforce_reviewer_recursion_guards(root: &Path) {
    let forbidden_markers = [
        "FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED",
        "FEATUREFORGE_REVIEWER_CONTEXT",
        "ReviewerRuntimeCommandForbidden",
        "REVIEWER_RUNTIME_ENV_CONTRACT",
        "REVIEWER_RUNTIME_COMMANDS_ALLOWED: no",
        "runtime command guard",
        "reviewer-mode environment",
        "reject_runtime_command_in_reviewer_context",
    ];
    let violations = runtime_rust_source_files(root)
        .into_iter()
        .flat_map(|path| {
            let source = read_file(&path);
            forbidden_markers
                .iter()
                .filter(|marker| source.contains(**marker))
                .map(|marker| format!("{} contains {marker:?}", path.display()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "reviewer recursion must remain prompt-only, not runtime/env enforced: {violations:#?}"
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
        "Keep review artifacts runtime-owned:",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Review Stage: featureforge:requesting-code-review",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Reviewer Provenance: dedicated-independent",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Reviewer Source` and `Reviewer ID`",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Distinct From Stages` including both `featureforge:executing-plans` and `featureforge:subagent-driven-development`",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Recorded Execution Deviations` and `Deviation Review Verdict` aligned to the execution evidence you reviewed",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Generated By: featureforge:requesting-code-review",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Source Plan`, `Source Plan Revision`, `Strategy Checkpoint Fingerprint`, `Branch`, `Repo`, `Base Branch`, `Head SHA`",
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
        "Structured Review Result Metadata",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "review-result metadata for the controller to bind to runtime-owned state",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Do not create, repair, search for, or reference runtime-owned projection files",
    );
    assert_file_does_not_contain(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "receipt-ready metadata",
    );
    assert_file_does_not_contain(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Dedicated Reviewer Receipt Contract",
    );
}

#[test]
fn reviewer_prompts_are_non_recursive_and_runtime_command_free() {
    let root = repo_root();
    assert_canonical_reviewer_recursion_rule_is_strong(&root);

    let reviewer_paths = [
        root.join("agents/code-reviewer.instructions.md"),
        root.join("agents/code-reviewer.md"),
        root.join("skills/requesting-code-review/code-reviewer.md"),
        root.join("skills/plan-fidelity-review/reviewer-prompt.md"),
        root.join("skills/plan-eng-review/accelerated-reviewer-prompt.md"),
        root.join("skills/plan-ceo-review/accelerated-reviewer-prompt.md"),
        root.join("skills/plan-eng-review/outside-voice-prompt.md"),
        root.join("skills/plan-ceo-review/outside-voice-prompt.md"),
        root.join("skills/subagent-driven-development/code-quality-reviewer-prompt.md"),
        root.join("skills/subagent-driven-development/spec-reviewer-prompt.md"),
    ];

    for path in reviewer_paths {
        assert_prompt_contains_canonical_recursion_rule(&root, &path);
        assert_prompt_has_no_runtime_environment_guards(&path);
    }

    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "The reviewer prompt owns the reviewer-only recursion contract.",
    );
    assert_file_does_not_contain(
        root.join("skills/requesting-code-review/SKILL.md"),
        "FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED",
    );
    assert_file_does_not_contain(
        root.join("skills/requesting-code-review/SKILL.md"),
        "## Review-subagent recursion rule",
    );
    assert_file_does_not_contain(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Do not launch, request, or delegate to additional subagents",
    );

    let spec_prompt_path = root.join("skills/subagent-driven-development/spec-reviewer-prompt.md");
    let spec_prompt = read_file(&spec_prompt_path);
    let spec_preamble = spec_prompt.split("```").next().unwrap_or("");
    assert_text_contains(
        "spec reviewer prompt guidance",
        spec_preamble,
        "$_FEATUREFORGE_ROOT/references/reviewer-recursion-rule.md",
    );
    assert_text_does_not_contain(
        "spec reviewer prompt guidance",
        spec_preamble,
        "Do not launch, request, or delegate to additional subagents while performing this review.",
    );

    let spec_payload = dispatch_prompt_payload(
        &root.join("skills/subagent-driven-development/spec-reviewer-prompt.md"),
    );
    assert!(
        normalized_prompt_block(&spec_payload).contains(&normalized_prompt_block(
            &canonical_reviewer_recursion_rule(&root)
        )),
        "spec reviewer dispatch payload should include the canonical reviewer recursion rule"
    );
}

#[test]
fn reviewer_recursion_is_not_enforced_by_runtime_environment_guards() {
    let root = repo_root();

    assert_runtime_sources_do_not_enforce_reviewer_recursion_guards(&root);
}

#[test]
fn generated_codex_reviewer_agent_carries_prompt_scoped_recursion_contract() {
    let root = repo_root();
    let codex_agent = root.join(".codex/agents/code-reviewer.toml");

    assert_canonical_reviewer_recursion_rule_is_strong(&root);
    assert_prompt_contains_canonical_recursion_rule(&root, &codex_agent);
    assert_file_does_not_contain(codex_agent.clone(), "# REVIEWER_RUNTIME_ENV_CONTRACT");
    assert_file_does_not_contain(
        codex_agent.clone(),
        "FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED",
    );
    assert_file_does_not_contain(codex_agent, "REVIEWER_RUNTIME_COMMANDS_ALLOWED: no");
}
