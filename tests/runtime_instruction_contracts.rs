#[path = "support/generated_docs.rs"]
mod generated_docs_support;
#[path = "support/git.rs"]
mod git_support;
#[path = "support/install.rs"]
mod install_support;
#[path = "support/process.rs"]
mod process_support;

use assert_cmd::Command as AssertCommand;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

use install_support::canonical_install_bin;
use process_support::{repo_root, run, run_checked};

fn read_utf8(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref())
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.as_ref().display()))
}

fn assert_contains(content: &str, needle: &str, label: &str) {
    assert!(
        content.contains(needle),
        "{label} should contain {:?}",
        needle
    );
}

fn assert_not_contains(content: &str, needle: &str, label: &str) {
    assert!(
        !content.contains(needle),
        "{label} should not contain {:?}",
        needle
    );
}

fn assert_forbids_direct_helper_command_mutation(content: &str, command: &str, label: &str) {
    let quoted = format!("`{command}`");
    let lines = content.lines().collect::<Vec<_>>();
    let mut windows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains(&quoted) {
            continue;
        }
        let start = index.saturating_sub(3);
        let end = (index + 3).min(lines.len().saturating_sub(1));
        windows.push(lines[start..=end].join(" "));
    }
    assert!(
        !windows.is_empty(),
        "{label} should explicitly mention {quoted} in helper-boundary guidance"
    );
    let has_boundary = windows.iter().any(|window| {
        let normalized = window.to_ascii_lowercase();
        let has_prohibition = [
            "must not",
            "do not",
            "never",
            "should not",
            "cannot",
            "can't",
        ]
        .iter()
        .any(|needle| normalized.contains(needle));
        let has_direct_action = ["invoke", "call", "run", "execute", "direct"]
            .iter()
            .any(|needle| normalized.contains(needle));
        let has_owner_actor = [
            "coordinator",
            "controller",
            "helper",
            "runtime",
            "harness",
            "gate",
        ]
        .iter()
        .any(|needle| normalized.contains(needle));
        let has_owner_verb = [
            "owns",
            "owned",
            "authoritative",
            "handles",
            "applies",
            "executes",
            "invokes",
            "calls",
            "runs",
            "governs",
        ]
        .iter()
        .any(|needle| normalized.contains(needle));
        (has_prohibition && has_direct_action) || (has_owner_actor && has_owner_verb)
    });
    assert!(
        has_boundary,
        "{label} should keep {quoted} in coordinator/helper-owned authoritative mutation boundaries"
    );
}

fn contains_any_casefold(content: &str, needles: &[&str]) -> bool {
    let normalized = content.to_ascii_lowercase();
    needles.iter().any(|needle| normalized.contains(needle))
}

fn assert_separates_candidate_artifacts_from_authoritative_mutations(content: &str, label: &str) {
    let has_candidate_surface = contains_any_casefold(
        content,
        &[
            "candidate",
            "task packet",
            "task-packet",
            "packet context",
            "handoff",
            "coverage matrix",
        ],
    );
    let has_authoritative_surface = contains_any_casefold(
        content,
        &[
            "authoritative",
            "helper-owned",
            "coordinator-owned",
            "execution state",
            "execution evidence",
            "review gate",
            "finish-gate",
            concat!("gate", "-review"),
        ],
    );
    let has_boundary_language = contains_any_casefold(
        content,
        &[
            "must not",
            "do not",
            "never",
            "may not",
            "only",
            "owns",
            "owned",
            "instead of",
            "fail closed",
        ],
    );
    assert!(
        has_candidate_surface && has_authoritative_surface && has_boundary_language,
        "{label} should distinguish candidate/planning artifacts from authoritative runtime state mutation boundaries"
    );
}

fn assert_downstream_material_stays_gate_and_harness_aware(content: &str, label: &str) {
    let has_gate_awareness = contains_any_casefold(
        content,
        &[
            concat!("gate", "-review"),
            "review gate",
            "finish-gate",
            concat!("gate", "-finish"),
            "fail closed",
        ],
    );
    let has_harness_awareness = contains_any_casefold(
        content,
        &[
            "execution evidence",
            "task-packet",
            "coverage matrix",
            "source plan",
            "source test plan",
            "workflow-routed",
            "artifact",
        ],
    );
    assert!(
        has_gate_awareness && has_harness_awareness,
        "{label} should stay downstream-gate-aware and harness-aware for review/QA handoffs"
    );
}

fn assert_no_runtime_fallback_execution(content: &str, label: &str) {
    // Intentional invariant: skill installs package the runtime binary on
    // purpose. Runtime-root resolution is for locating adjacent files from the
    // same install, not for switching command execution to another launcher.
    // NEVER relax these checks without an explicit product decision.
    for needle in [
        "$_REPO_ROOT/bin/featureforge",
        "$_REPO_ROOT/bin/featureforge.exe",
        "${_FEATUREFORGE_BIN:-featureforge}",
        "command -v featureforge",
    ] {
        assert_not_contains(content, needle, label);
    }
    for line in content.lines().map(str::trim_start) {
        assert!(
            !line.starts_with("\"$_FEATUREFORGE_ROOT/bin/featureforge\""),
            "{label} should not execute runtime commands through $_FEATUREFORGE_ROOT/bin/featureforge"
        );
        assert!(
            !line.starts_with("\"$INSTALL_DIR/bin/featureforge\""),
            "{label} should not execute runtime commands through $INSTALL_DIR/bin/featureforge"
        );
        assert!(
            !line.starts_with("\"$_FEATUREFORGE_ROOT/bin/featureforge.exe\""),
            "{label} should not execute runtime commands through $_FEATUREFORGE_ROOT/bin/featureforge.exe"
        );
        assert!(
            !line.starts_with("\"$INSTALL_DIR/bin/featureforge.exe\""),
            "{label} should not execute runtime commands through $INSTALL_DIR/bin/featureforge.exe"
        );
        assert!(
            !line.starts_with("FEATUREFORGE_RUNTIME_BIN=\"$_FEATUREFORGE_ROOT/bin/featureforge\""),
            "{label} should not assign FEATUREFORGE_RUNTIME_BIN from $_FEATUREFORGE_ROOT"
        );
        assert!(
            !line.starts_with("FEATUREFORGE_RUNTIME_BIN=\"$INSTALL_DIR/bin/featureforge\""),
            "{label} should not assign FEATUREFORGE_RUNTIME_BIN from INSTALL_DIR"
        );
        assert!(
            !line.starts_with(
                "FEATUREFORGE_RUNTIME_BIN=\"$_FEATUREFORGE_ROOT/bin/featureforge.exe\""
            ),
            "{label} should not assign FEATUREFORGE_RUNTIME_BIN from $_FEATUREFORGE_ROOT/bin/featureforge.exe"
        );
        assert!(
            !line.starts_with("FEATUREFORGE_RUNTIME_BIN=\"$INSTALL_DIR/bin/featureforge.exe\""),
            "{label} should not assign FEATUREFORGE_RUNTIME_BIN from INSTALL_DIR/bin/featureforge.exe"
        );
    }
}

fn assert_file_contains(path: impl AsRef<Path>, needle: &str) {
    let path_ref = path.as_ref();
    let content = read_utf8(path_ref);
    assert_contains(&content, needle, &path_ref.display().to_string());
}

fn assert_file_contains_in_order(path: impl AsRef<Path>, needles: &[&str]) {
    let path_ref = path.as_ref();
    let content = read_utf8(path_ref);
    let mut previous_index = 0usize;
    for needle in needles {
        let relative_index = content[previous_index..]
            .find(needle)
            .unwrap_or_else(|| panic!("{} should contain {needle:?}", path_ref.display()));
        previous_index += relative_index + needle.len();
    }
}

fn assert_file_not_contains(path: impl AsRef<Path>, needle: &str) {
    let path_ref = path.as_ref();
    let content = read_utf8(path_ref);
    assert_not_contains(&content, needle, &path_ref.display().to_string());
}

fn source_tree_declares_test(root: &Path, test_name: &str) -> bool {
    source_tree_declares_test_in_dir(&root.join("src"), test_name)
        || source_tree_declares_test_in_dir(&root.join("tests"), test_name)
}

fn source_tree_declares_test_in_dir(dir: &Path, test_name: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if source_tree_declares_test_in_dir(&path, test_name) {
                return true;
            }
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        if declares_rust_test_function(&read_utf8(&path), test_name) {
            return true;
        }
    }
    false
}

fn declares_rust_test_function(content: &str, test_name: &str) -> bool {
    let declaration = format!("fn {test_name}(");
    let mut saw_test_attr = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line == "#[test]"
            || line.starts_with("#[tokio::test")
            || line.starts_with("#[rstest")
            || line.starts_with("#[case]")
        {
            saw_test_attr = true;
            continue;
        }
        if saw_test_attr && line.starts_with(&declaration) {
            return true;
        }
        if !line.is_empty() && !line.starts_with("#[") {
            saw_test_attr = false;
        }
    }
    false
}

#[test]
fn targeted_test_scanner_requires_exact_function_identifier() {
    assert!(declares_rust_test_function(
        "#[test]\nfn runtime_provenance_classifies_installed_runtime() {}\n",
        "runtime_provenance_classifies_installed_runtime",
    ));
    assert!(!declares_rust_test_function(
        "#[test]\nfn runtime_provenance_classifies_installed_runtime_extra() {}\n",
        "runtime_provenance_classifies_installed_runtime",
    ));
}

fn extract_workspace_runtime_guard_commands(content: &str) -> Vec<String> {
    let lines = content.lines().collect::<Vec<_>>();
    let mut commands = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains("deny_workspace_runtime_live_mutation") {
            continue;
        }
        let scan_end = (index + 8).min(lines.len());
        for candidate in &lines[index..scan_end] {
            let Some(command) = extract_first_rust_string_literal(candidate) else {
                continue;
            };
            if command.starts_with("plan contract ")
                || command.starts_with("plan execution ")
                || command.starts_with("repo-safety ")
            {
                commands.push(command);
                break;
            }
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn extract_first_rust_string_literal(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let tail = &line[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn extract_js_string_array(content: &str, name: &str) -> Vec<String> {
    let start_marker = format!("const {name} = [");
    let start = content
        .find(&start_marker)
        .unwrap_or_else(|| panic!("expected JavaScript array declaration for {name}"));
    let tail = &content[start + start_marker.len()..];
    let body = tail
        .split_once("];")
        .unwrap_or_else(|| panic!("expected JavaScript array declaration for {name} to close"))
        .0;
    let mut values = body
        .lines()
        .filter_map(extract_single_quoted_js_string)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn extract_single_quoted_js_string(line: &str) -> Option<String> {
    let stripped = line.trim().trim_end_matches(',').strip_prefix('\'')?;
    let end = stripped.find('\'')?;
    Some(stripped[..end].to_string())
}

fn hidden_text(parts: &[&str]) -> String {
    parts.concat()
}

fn assert_generated_skill_docs_current(root: &Path) {
    generated_docs_support::assert_generated_skill_docs_current(root);
}

fn assert_generated_agent_docs_current(root: &Path) {
    generated_docs_support::assert_generated_agent_docs_current(root);
}

fn assert_description_contains(path: impl AsRef<Path>, needle: &str) {
    let path_ref = path.as_ref();
    let content = read_utf8(path_ref);
    let first_lines = content.lines().take(6).collect::<Vec<_>>().join("\n");
    assert_contains(&first_lines, needle, &path_ref.display().to_string());
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LateStageRuntimeRow {
    release: String,
    review: String,
    qa: String,
    phase: String,
    reason_family: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LateStageReferenceRow {
    release: String,
    review: String,
    qa: String,
    phase: String,
    next_action: String,
    recommended_skill: String,
    reason_family: String,
}

fn parse_runtime_gate_state(block: &str, field: &str) -> String {
    let needle = format!("{field}: GateState::");
    let start = block
        .find(&needle)
        .unwrap_or_else(|| panic!("runtime precedence row should contain {needle:?}: {block}"));
    let rest = &block[start + needle.len()..];
    if rest.starts_with("Blocked") {
        String::from("blocked")
    } else if rest.starts_with("Ready") {
        String::from("ready")
    } else {
        panic!("runtime precedence row should use Ready/Blocked for {field}: {block}");
    }
}

fn parse_runtime_quoted_field(block: &str, field: &str) -> String {
    let needle = format!("{field}: \"");
    let start = block
        .find(&needle)
        .unwrap_or_else(|| panic!("runtime precedence row should contain {needle:?}: {block}"));
    let rest = &block[start + needle.len()..];
    let end = rest.find('"').unwrap_or_else(|| {
        panic!("runtime precedence row should close quoted field {field:?}: {block}")
    });
    rest[..end].to_owned()
}

fn parse_runtime_phase_field(block: &str) -> String {
    let needle = "phase:";
    let start = block
        .find(needle)
        .unwrap_or_else(|| panic!("runtime precedence row should contain {needle:?}: {block}"));
    let rest = block[start + needle.len()..].trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"').unwrap_or_else(|| {
            panic!("runtime precedence row should close quoted phase field: {block}")
        });
        return stripped[..end].to_owned();
    }

    for (source_token, phase) in [
        (
            "crate::execution::phase::PHASE_DOCUMENT_RELEASE_PENDING",
            "document_release_pending",
        ),
        (
            "crate::execution::phase::PHASE_FINAL_REVIEW_PENDING",
            "final_review_pending",
        ),
        ("crate::execution::phase::PHASE_QA_PENDING", "qa_pending"),
        (
            "crate::execution::phase::PHASE_READY_FOR_BRANCH_COMPLETION",
            "ready_for_branch_completion",
        ),
    ] {
        if rest.starts_with(source_token) {
            return phase.to_owned();
        }
    }

    panic!(
        "runtime precedence row should use a known phase literal or shared phase constant: {block}"
    );
}

fn parse_runtime_late_stage_rows(source: &str) -> Vec<LateStageRuntimeRow> {
    let table_start = source
        .find("const PRECEDENCE_ROWS")
        .unwrap_or_else(|| panic!("runtime precedence source should define PRECEDENCE_ROWS"));
    let table_source = &source[table_start..];
    let table_end = table_source.find("];").unwrap_or_else(|| {
        panic!("runtime precedence source should close PRECEDENCE_ROWS with ];")
    });
    let table_source = &table_source[..table_end];
    table_source
        .split("LateStageRow {")
        .skip(1)
        .map(|chunk| chunk.split("},").next().unwrap_or(chunk))
        .map(|block| LateStageRuntimeRow {
            release: parse_runtime_gate_state(block, "release"),
            review: parse_runtime_gate_state(block, "review"),
            qa: parse_runtime_gate_state(block, "qa"),
            phase: parse_runtime_phase_field(block),
            reason_family: parse_runtime_quoted_field(block, "reason_family"),
        })
        .collect()
}

fn strip_inline_code(value: &str) -> String {
    value.trim().trim_matches('`').to_owned()
}

fn parse_reference_late_stage_rows(source: &str) -> Vec<LateStageReferenceRow> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("| blocked") || line.starts_with("| ready"))
        .map(|line| {
            let columns = line
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>();
            assert_eq!(
                columns.len(),
                7,
                "late-stage reference rows should have seven columns: {line}"
            );
            LateStageReferenceRow {
                release: columns[0].to_owned(),
                review: columns[1].to_owned(),
                qa: columns[2].to_owned(),
                phase: strip_inline_code(columns[3]),
                next_action: strip_inline_code(columns[4]),
                recommended_skill: strip_inline_code(columns[5]),
                reason_family: strip_inline_code(columns[6]),
            }
        })
        .collect()
}

fn expected_phase_action_and_skill(phase: &str) -> (&'static str, &'static str) {
    match phase {
        "document_release_pending" => (
            "derived from phase_detail: advance late stage (branch-closure refresh lane); resolve release blocker",
            "featureforge:document-release",
        ),
        "final_review_pending" => (
            "derived from phase_detail: request final review; wait for external review result; advance late stage",
            "featureforge:requesting-code-review",
        ),
        "qa_pending" => (
            "derived from phase_detail: run QA; refresh test plan",
            "featureforge:qa-only",
        ),
        "ready_for_branch_completion" => (
            "derived from phase_detail: finish branch",
            "featureforge:finishing-a-development-branch",
        ),
        _ => panic!("unexpected late-stage phase in precedence row: {phase}"),
    }
}

fn expected_phase_source_token(phase: &str) -> &'static str {
    match phase {
        "document_release_pending" => "phase::PHASE_DOCUMENT_RELEASE_PENDING",
        "final_review_pending" => "phase::PHASE_FINAL_REVIEW_PENDING",
        "qa_pending" => "phase::PHASE_QA_PENDING",
        "ready_for_branch_completion" => "phase::PHASE_READY_FOR_BRANCH_COMPLETION",
        _ => panic!("unexpected late-stage phase in precedence row: {phase}"),
    }
}

fn generated_skill_doc_paths() -> Vec<PathBuf> {
    fs::read_dir(repo_root().join("skills"))
        .expect("skills dir should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("SKILL.md").is_file() && path.join("SKILL.md.tmpl").is_file())
        .map(|path| path.join("SKILL.md"))
        .collect()
}

fn active_prompt_doc_paths() -> Vec<PathBuf> {
    fn collect(root: &Path, dir: &Path, paths: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()))
            .filter_map(Result::ok)
        {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .expect("active prompt path should be repo-relative")
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                if [
                    ".git",
                    "target",
                    "node_modules",
                    "docs/archive",
                    "docs/featureforge/archive",
                    "docs/project_notes",
                ]
                .iter()
                .any(|prefix| rel == *prefix || rel.starts_with(&format!("{prefix}/")))
                {
                    continue;
                }
                collect(root, &path, paths);
            } else if entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
                let is_text_prompt = rel.ends_with(".md")
                    || rel.ends_with(".md.tmpl")
                    || (rel.starts_with(".codex/agents/") && rel.ends_with(".toml"));
                if is_text_prompt && rel != "docs/testing.md" {
                    paths.push(path);
                }
            }
        }
    }

    let root = repo_root();
    let mut paths = Vec::new();
    for rel_root in [
        "README.md",
        "AGENTS.md",
        "docs",
        "qa",
        "references",
        "review",
        "skills",
        ".codex/agents",
    ] {
        let path = root.join(rel_root);
        if path.is_file() {
            paths.push(path);
        } else if path.is_dir() {
            collect(&root, &path, &mut paths);
        }
    }
    paths
}

fn extract_bash_block(content: &str, heading: &str) -> String {
    let mut in_heading = false;
    let mut in_block = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if !in_heading {
            if line == heading {
                in_heading = true;
            }
            continue;
        }
        if !in_block {
            if line == "```bash" {
                in_block = true;
            }
            continue;
        }
        if line == "```" {
            break;
        }
        lines.push(line);
    }

    assert!(
        !lines.is_empty(),
        "expected bash block under heading {heading}"
    );
    lines.join("\n")
}

fn write_executable(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("executable parent dir should exist");
    }
    fs::write(path, body).expect("executable should be writable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("executable should stay executable");
    }
}

fn write_utf8(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("file parent dir should exist");
    }
    fs::write(path, body).expect("file should be writable");
}

fn sha256_hex(contents: &str) -> String {
    format!("{:x}", Sha256::digest(contents.as_bytes()))
}

fn write_minimal_prebuilt_source(root: &Path, source_marker: &str) {
    write_utf8(
        &root.join("Cargo.toml"),
        "[package]\nname = \"prebuilt-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write_utf8(&root.join("VERSION"), "1.0.0\n");
    write_utf8(
        &root.join("src/main.rs"),
        &format!("fn main() {{ println!(\"{source_marker}\"); }}\n"),
    );
}

fn write_prebuilt_fixture_binary(
    root: &Path,
    binary_rel: &str,
    checksum_rel: &str,
    binary_name: &str,
    body: &str,
) {
    write_executable(&root.join(binary_rel), body);
    write_utf8(
        &root.join(checksum_rel),
        &format!("{}  {binary_name}\n", sha256_hex(body)),
    );
}

fn update_prebuilt_fixture_manifest(
    root: &Path,
    target: &str,
    binary_rel: &str,
    checksum_rel: &str,
) {
    let mut command = Command::new("node");
    command
        .arg(repo_root().join("scripts/prebuilt-runtime-provenance.mjs"))
        .arg("update")
        .arg("--target")
        .arg(target)
        .arg("--binary-path")
        .arg(binary_rel)
        .arg("--checksum-path")
        .arg(checksum_rel)
        .arg("--version")
        .arg("1.0.0")
        .arg("--repo-root")
        .arg(root);
    run_checked(command, "prebuilt fixture provenance update");
}

fn verify_prebuilt_fixture(root: &Path) -> std::process::Output {
    let mut command = Command::new("node");
    command
        .arg(repo_root().join("scripts/prebuilt-runtime-provenance.mjs"))
        .arg("verify")
        .arg("--skip-help")
        .arg("--repo-root")
        .arg(root);
    run(command, "prebuilt fixture provenance verify")
}

fn verify_prebuilt_fixture_with_host_target(
    root: &Path,
    host_target: &str,
    extra_env: &[(&str, &Path)],
) -> std::process::Output {
    verify_prebuilt_fixture_with_host_target_and_args(root, host_target, &[], extra_env)
}

fn verify_prebuilt_fixture_with_host_target_and_args(
    root: &Path,
    host_target: &str,
    extra_args: &[&str],
    extra_env: &[(&str, &Path)],
) -> std::process::Output {
    let mut command = Command::new("node");
    command
        .arg(repo_root().join("scripts/prebuilt-runtime-provenance.mjs"))
        .arg("verify")
        .args(extra_args)
        .arg("--repo-root")
        .arg(root)
        .env("FEATUREFORGE_PREBUILT_HOST_TARGET", host_target);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, "prebuilt fixture provenance verify")
}

fn run_workspace_runtime_evidence_lint(
    lint_repo_root: &Path,
    scan_paths: &[&Path],
) -> std::process::Output {
    let mut command = Command::new("node");
    command
        .arg(repo_root().join("scripts/lint-workspace-runtime-evidence.mjs"))
        .arg("--repo-root")
        .arg(lint_repo_root);
    for scan_path in scan_paths {
        command.arg("--path").arg(scan_path);
    }
    run(command, "workspace runtime evidence lint")
}

fn write_complete_prebuilt_fixture(root: &Path, darwin_body: &str, windows_body: &str) {
    let darwin_rel = "bin/prebuilt/darwin-arm64/featureforge";
    let darwin_checksum_rel = "bin/prebuilt/darwin-arm64/featureforge.sha256";
    let windows_rel = "bin/prebuilt/windows-x64/featureforge.exe";
    let windows_checksum_rel = "bin/prebuilt/windows-x64/featureforge.exe.sha256";

    write_minimal_prebuilt_source(root, "source-v1");
    write_prebuilt_fixture_binary(
        root,
        darwin_rel,
        darwin_checksum_rel,
        "featureforge",
        darwin_body,
    );
    write_prebuilt_fixture_binary(
        root,
        windows_rel,
        windows_checksum_rel,
        "featureforge.exe",
        windows_body,
    );
    write_executable(&root.join("bin/featureforge"), darwin_body);
    update_prebuilt_fixture_manifest(root, "darwin-arm64", darwin_rel, darwin_checksum_rel);
    update_prebuilt_fixture_manifest(root, "windows-x64", windows_rel, windows_checksum_rel);
}

fn write_poison_runtime_launcher(root: &Path, marker: &str) {
    let poison_body = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' '{marker}' >> \"$FEATUREFORGE_TEST_LOG\"\nexit 86\n"
    );
    for relative in ["bin/featureforge", "bin/featureforge.exe"] {
        write_executable(&root.join(relative), &poison_body);
    }
}

fn write_logging_packaged_runtime(
    packaged_bin: &Path,
    resolved_runtime_root: &Path,
    log_path: &Path,
) {
    let resolved_runtime_root = resolved_runtime_root
        .canonicalize()
        .unwrap_or_else(|_| resolved_runtime_root.to_path_buf());
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).expect("log parent dir should exist");
    }
    write_executable(
        packaged_bin,
        &format!(
            "#!/usr/bin/env bash\n: \"${{FEATUREFORGE_TEST_LOG:?}}\"\ncase \"${{1:-}}:${{2:-}}:${{3:-}}:${{4:-}}\" in\n  repo:runtime-root:--path:*)\n    printf '%s\\n' 'PACKAGED:repo-runtime-root' >> \"$FEATUREFORGE_TEST_LOG\"\n    printf '%s\\n' '{}'\n    exit 0\n    ;;\n  update-check:::)\n    printf '%s\\n' 'PACKAGED:update-check' >> \"$FEATUREFORGE_TEST_LOG\"\n    printf 'UPGRADE_AVAILABLE 1.0.0 1.1.0\\n'\n    exit 0\n    ;;\n  config:get:featureforge_contributor:*)\n    printf '%s\\n' 'PACKAGED:config-get' >> \"$FEATUREFORGE_TEST_LOG\"\n    printf 'false\\n'\n    exit 0\n    ;;\n  *)\n    printf '%s\\n' \"PACKAGED:UNEXPECTED:${{1:-}}:${{2:-}}:${{3:-}}:${{4:-}}\" >> \"$FEATUREFORGE_TEST_LOG\"\n    exit 0\n    ;;\nesac\n",
            resolved_runtime_root.display()
        ),
    );
}

fn make_runtime_root(dir: &Path) {
    fs::create_dir_all(dir.join("bin")).expect("runtime bin dir should exist");
    fs::write(
        dir.join("bin/featureforge"),
        "#!/usr/bin/env bash\ncase \"${1:-}\" in\n  repo)\n    if [ \"${2:-}\" = \"runtime-root\" ] && [ \"${3:-}\" = \"--json\" ]; then\n      printf '{\"resolved\":true,\"root\":\"%s\",\"source\":\"featureforge_dir_env\",\"validation\":{\"has_version\":true,\"has_binary\":true,\"upgrade_eligible\":true}}\\n' \"$(pwd -P)\"\n      exit 0\n    fi\n    if [ \"${2:-}\" = \"runtime-root\" ] && [ \"${3:-}\" = \"--path\" ]; then\n      printf '%s\\n' \"$(pwd -P)\"\n      exit 0\n    fi\n    exit 0\n    ;;\n  update-check)\n    exit 0\n    ;;\n  config)\n    exit 0\n    ;;\n  *)\n    exit 0\n    ;;\nesac\n",
    )
    .expect("runtime launcher should be writable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            dir.join("bin/featureforge"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("runtime launcher should be executable");
    }
    fs::write(dir.join("VERSION"), "1.0.0\n").expect("VERSION should be writable");
}

fn make_runtime_repo(dir: &Path) {
    git_support::init_repo_with_initial_commit(dir, "# runtime repo\n", "init");
    make_runtime_root(dir);
}

#[test]
fn repo_checkout_ships_the_canonical_runtime_launcher() {
    let launcher = if cfg!(windows) {
        repo_root().join("bin/featureforge.exe")
    } else {
        repo_root().join("bin/featureforge")
    };
    assert!(
        launcher.is_file(),
        "repo checkout should expose the real featureforge binary as the canonical repo-local launcher"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&launcher)
            .expect("repo-local launcher should be stat-able")
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "repo-local launcher should be executable on unix hosts"
        );
    }
}

#[test]
fn repo_checkout_canonical_launcher_runs_without_recursive_fallback() {
    let launcher = if cfg!(windows) {
        repo_root().join("bin/featureforge.exe")
    } else {
        repo_root().join("bin/featureforge")
    };
    let output = AssertCommand::new(launcher)
        .current_dir(repo_root())
        .timeout(Duration::from_secs(2))
        .arg("--version")
        .unwrap();

    assert!(
        output.status.success(),
        "repo-local launcher should resolve to a real runtime binary instead of recursing through compat wrappers\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("featureforge") && stdout.contains(env!("CARGO_PKG_VERSION")),
        "repo-local featureforge binary should print the current runtime version, got:\n{stdout}"
    );
}

#[test]
fn repo_checkout_canonical_launcher_supports_runtime_root_helper_contract() {
    let launcher = if cfg!(windows) {
        repo_root().join("bin/featureforge.exe")
    } else {
        repo_root().join("bin/featureforge")
    };
    let output = AssertCommand::new(launcher)
        .current_dir(repo_root())
        .timeout(Duration::from_secs(2))
        .args(["repo", "runtime-root", "--json"])
        .unwrap();

    assert!(
        output.status.success(),
        "repo-local launcher should support repo runtime-root --json\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("runtime-root stdout should be utf-8");
    assert_contains(
        &stdout,
        "\"resolved\":true",
        "bin/featureforge repo runtime-root --json",
    );
    assert_contains(
        &stdout,
        &format!("\"root\":\"{}\"", repo_root().display()),
        "bin/featureforge repo runtime-root --json",
    );
}

#[test]
fn repo_checkout_canonical_launcher_supports_runtime_root_path_contract() {
    let launcher = if cfg!(windows) {
        repo_root().join("bin/featureforge.exe")
    } else {
        repo_root().join("bin/featureforge")
    };
    let output = AssertCommand::new(launcher)
        .current_dir(repo_root())
        .timeout(Duration::from_secs(2))
        .args(["repo", "runtime-root", "--path"])
        .unwrap();

    assert!(
        output.status.success(),
        "repo-local launcher should support repo runtime-root --path\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout =
        String::from_utf8(output.stdout).expect("runtime-root --path stdout should be utf-8");
    assert_eq!(
        stdout.trim_end(),
        repo_root().to_string_lossy(),
        "bin/featureforge repo runtime-root --path should print the resolved root directly"
    );
}

#[test]
fn repo_checkout_canonical_launcher_avoids_non_binary_repo_fallbacks() {
    let root = repo_root();
    let top_level_bin_files = fs::read_dir(root.join("bin"))
        .expect("bin dir should be readable")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            path.is_file()
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        top_level_bin_files,
        vec![String::from("featureforge")],
        "repo checkout should expose only the standalone featureforge binary at bin/"
    );
    for relative in ["commands", "compat/bash", "compat/powershell"] {
        let dir = root.join(relative);
        if !dir.exists() {
            continue;
        }
        assert!(
            fs::read_dir(&dir)
                .expect("compat/commands dir should be readable")
                .next()
                .is_none(),
            "{relative} should be empty in the standalone runtime"
        );
    }
}

#[test]
fn repo_checkout_canonical_launcher_uses_manifest_selected_binary_path() {
    let root = repo_root();
    let manifest = read_utf8(root.join("bin/prebuilt/manifest.json"));
    for needle in [
        &format!("\"runtime_revision\": \"{}\"", env!("CARGO_PKG_VERSION")),
        "\"source_fingerprint\": \"sha256:",
        "\"source_fingerprint_algorithm\": \"sha256\"",
        "\"source_fingerprint_path_count\":",
        "bin/prebuilt/darwin-arm64/featureforge",
        "bin/prebuilt/darwin-arm64/featureforge.sha256",
        "bin/prebuilt/windows-x64/featureforge.exe",
        "bin/prebuilt/windows-x64/featureforge.exe.sha256",
    ] {
        assert_contains(&manifest, needle, "bin/prebuilt/manifest.json");
    }
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest).expect("manifest json should parse");
    let targets = manifest_json["targets"]
        .as_object()
        .expect("manifest targets should be an object");
    for entry in targets.values() {
        let runtime_path = entry["binary_path"]
            .as_str()
            .expect("manifest binary path should be a string");
        let checksum_path = entry["checksum_path"]
            .as_str()
            .expect("manifest checksum path should be a string");
        let binary_sha256 = entry["binary_sha256"]
            .as_str()
            .expect("manifest binary sha256 should be a string");
        let source_fingerprint = entry["source_fingerprint"]
            .as_str()
            .expect("manifest target source fingerprint should be a string");
        let source_fingerprint_algorithm = entry["source_fingerprint_algorithm"]
            .as_str()
            .expect("manifest target source fingerprint algorithm should be a string");
        let source_fingerprint_path_count = entry["source_fingerprint_path_count"]
            .as_u64()
            .expect("manifest target source fingerprint path count should be an integer");
        assert_contains(runtime_path, "featureforge", "bin/prebuilt/manifest.json");
        assert_contains(checksum_path, "featureforge", "bin/prebuilt/manifest.json");
        assert!(
            binary_sha256.starts_with("sha256:"),
            "manifest binary sha should be algorithm-qualified: {binary_sha256}"
        );
        assert!(
            source_fingerprint.starts_with("sha256:"),
            "manifest target source fingerprint should be algorithm-qualified: {source_fingerprint}"
        );
        assert_eq!(
            source_fingerprint_algorithm, "sha256",
            "manifest target source fingerprint algorithm should be sha256"
        );
        assert!(
            source_fingerprint_path_count > 0,
            "manifest target source fingerprint should cover the runtime source tree"
        );
    }
    for relative in [
        "bin/prebuilt/darwin-arm64/featureforge",
        "bin/prebuilt/darwin-arm64/featureforge.sha256",
        "bin/prebuilt/windows-x64/featureforge.exe",
        "bin/prebuilt/windows-x64/featureforge.exe.sha256",
    ] {
        assert!(
            root.join(relative).is_file(),
            "renamed prebuilt runtime asset should exist: {relative}"
        );
    }
    assert_eq!(
        fs::read(root.join("bin/featureforge")).expect("root runtime should be readable"),
        fs::read(root.join("bin/prebuilt/darwin-arm64/featureforge"))
            .expect("darwin prebuilt runtime should be readable"),
        "root shipped runtime must be hash-identical to darwin-arm64 prebuilt runtime"
    );
}

#[test]
fn prebuilt_runtime_provenance_rejects_partially_refreshed_targets() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let darwin_rel = "bin/prebuilt/darwin-arm64/featureforge";
    let darwin_checksum_rel = "bin/prebuilt/darwin-arm64/featureforge.sha256";
    let windows_rel = "bin/prebuilt/windows-x64/featureforge.exe";
    let windows_checksum_rel = "bin/prebuilt/windows-x64/featureforge.exe.sha256";

    write_minimal_prebuilt_source(root, "source-v1");
    let darwin_v1 = "#!/usr/bin/env bash\nprintf 'darwin v1\\n'\n";
    write_prebuilt_fixture_binary(
        root,
        darwin_rel,
        darwin_checksum_rel,
        "featureforge",
        darwin_v1,
    );
    write_prebuilt_fixture_binary(
        root,
        windows_rel,
        windows_checksum_rel,
        "featureforge.exe",
        "windows v1\n",
    );
    write_executable(&root.join("bin/featureforge"), darwin_v1);
    update_prebuilt_fixture_manifest(root, "darwin-arm64", darwin_rel, darwin_checksum_rel);
    update_prebuilt_fixture_manifest(root, "windows-x64", windows_rel, windows_checksum_rel);
    let clean_verify = verify_prebuilt_fixture(root);
    assert!(
        clean_verify.status.success(),
        "fresh fixture should verify before stale-target regression\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&clean_verify.stdout),
        String::from_utf8_lossy(&clean_verify.stderr)
    );

    write_minimal_prebuilt_source(root, "source-v2");
    let darwin_v2 = "#!/usr/bin/env bash\nprintf 'darwin v2\\n'\n";
    write_prebuilt_fixture_binary(
        root,
        darwin_rel,
        darwin_checksum_rel,
        "featureforge",
        darwin_v2,
    );
    write_executable(&root.join("bin/featureforge"), darwin_v2);
    update_prebuilt_fixture_manifest(root, "darwin-arm64", darwin_rel, darwin_checksum_rel);

    let stale_verify = verify_prebuilt_fixture(root);
    assert!(
        !stale_verify.status.success(),
        "verification should reject a target not refreshed for the current source fingerprint\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stale_verify.stdout),
        String::from_utf8_lossy(&stale_verify.stderr)
    );
    let stderr = String::from_utf8_lossy(&stale_verify.stderr);
    assert_contains(
        &stderr,
        "bin/prebuilt/windows-x64/featureforge.exe: manifest source_fingerprint",
        "prebuilt provenance stale target failure",
    );
}

#[test]
fn prebuilt_runtime_provenance_rejects_root_binary_drift() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let darwin_rel = "bin/prebuilt/darwin-arm64/featureforge";
    let darwin_checksum_rel = "bin/prebuilt/darwin-arm64/featureforge.sha256";
    let windows_rel = "bin/prebuilt/windows-x64/featureforge.exe";
    let windows_checksum_rel = "bin/prebuilt/windows-x64/featureforge.exe.sha256";

    write_minimal_prebuilt_source(root, "source-v1");
    let darwin = "#!/usr/bin/env bash\nprintf 'darwin runtime\\n'\n";
    write_prebuilt_fixture_binary(
        root,
        darwin_rel,
        darwin_checksum_rel,
        "featureforge",
        darwin,
    );
    write_prebuilt_fixture_binary(
        root,
        windows_rel,
        windows_checksum_rel,
        "featureforge.exe",
        "windows runtime\n",
    );
    write_executable(&root.join("bin/featureforge"), darwin);
    update_prebuilt_fixture_manifest(root, "darwin-arm64", darwin_rel, darwin_checksum_rel);
    update_prebuilt_fixture_manifest(root, "windows-x64", windows_rel, windows_checksum_rel);

    write_executable(
        &root.join("bin/featureforge"),
        "#!/usr/bin/env bash\nprintf 'root drift without denied strings\\n'\n",
    );

    let output = verify_prebuilt_fixture(root);
    assert!(
        !output.status.success(),
        "verification should reject root binary drift even when string audit is clean\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "bin/featureforge: root shipped runtime",
        "prebuilt provenance root drift failure",
    );
}

#[test]
fn prebuilt_runtime_provenance_runs_help_on_matching_host_target() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let help_log = root.join("help.log");
    let darwin_body =
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"$FEATUREFORGE_TEST_LOG\"\nexit 0\n";
    write_complete_prebuilt_fixture(root, darwin_body, "windows runtime\n");

    let output = verify_prebuilt_fixture_with_host_target(
        root,
        "darwin-arm64",
        &[("FEATUREFORGE_TEST_LOG", help_log.as_path())],
    );
    assert!(
        output.status.success(),
        "same-target prebuilt verification should run help successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let help_invocations = read_utf8(&help_log);
    assert_contains(
        &help_invocations,
        "--help",
        "same-target prebuilt verification",
    );
    assert_contains(
        &help_invocations,
        "plan execution --help",
        "same-target prebuilt verification",
    );
    assert_contains(
        &help_invocations,
        "workflow --help",
        "same-target prebuilt verification",
    );
    assert_not_contains(
        &String::from_utf8_lossy(&output.stdout),
        "prebuilt_runtime_help_skipped",
        "same-target prebuilt verification",
    );
}

#[test]
fn prebuilt_runtime_provenance_rejects_same_platform_help_failures() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let darwin_body = "#!/usr/bin/env bash\nprintf 'help failed\\n' >&2\nexit 17\n";
    write_complete_prebuilt_fixture(root, darwin_body, "windows runtime\n");

    let output = verify_prebuilt_fixture_with_host_target(root, "darwin-arm64", &[]);
    assert!(
        !output.status.success(),
        "same-target prebuilt verification should fail when help fails\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "bin/featureforge --help failed",
        "same-target prebuilt help failure",
    );
}

#[test]
fn prebuilt_runtime_provenance_runs_matching_manifest_target_help_after_root_skip() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let help_log = root.join("help.log");
    let darwin_body = "#!/usr/bin/env bash\nprintf 'unexpected execution\\n' >> \"$FEATUREFORGE_TEST_LOG\"\nexit 86\n";
    let windows_body = "#!/usr/bin/env bash\nprintf '%s\\n' \"windows:$*\" >> \"$FEATUREFORGE_TEST_LOG\"\nexit 0\n";
    write_complete_prebuilt_fixture(root, darwin_body, windows_body);

    let output = verify_prebuilt_fixture_with_host_target(
        root,
        "windows-x64",
        &[("FEATUREFORGE_TEST_LOG", help_log.as_path())],
    );
    assert!(
        output.status.success(),
        "incompatible-target prebuilt verification should skip help after clean audits\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let help_invocations = read_utf8(&help_log);
    assert_contains(
        &help_invocations,
        "windows:--help",
        "matching manifest-target prebuilt verification",
    );
    assert_contains(
        &help_invocations,
        "windows:plan execution --help",
        "matching manifest-target prebuilt verification",
    );
    assert_contains(
        &help_invocations,
        "windows:workflow --help",
        "matching manifest-target prebuilt verification",
    );
    assert_not_contains(
        &help_invocations,
        "unexpected execution",
        "matching manifest-target prebuilt verification",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains(
        &stdout,
        "prebuilt_runtime_help_skipped",
        "incompatible-target prebuilt verification",
    );
    assert_contains(
        &stdout,
        "\"binary_target\":\"darwin-arm64\"",
        "incompatible-target prebuilt verification",
    );
    assert_contains(
        &stdout,
        "\"host_target\":\"windows-x64\"",
        "incompatible-target prebuilt verification",
    );
}

#[test]
fn prebuilt_runtime_provenance_target_filter_runs_matching_target_help() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let help_log = root.join("help.log");
    let darwin_body = "#!/usr/bin/env bash\nprintf 'unexpected root execution\\n' >> \"$FEATUREFORGE_TEST_LOG\"\nexit 86\n";
    let windows_body = "#!/usr/bin/env bash\nprintf '%s\\n' \"windows-target:$*\" >> \"$FEATUREFORGE_TEST_LOG\"\nexit 0\n";
    write_complete_prebuilt_fixture(root, darwin_body, windows_body);

    let output = verify_prebuilt_fixture_with_host_target_and_args(
        root,
        "windows-x64",
        &["--target", "windows-x64"],
        &[("FEATUREFORGE_TEST_LOG", help_log.as_path())],
    );
    assert!(
        output.status.success(),
        "target-filtered prebuilt verification should run matching target help successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let help_invocations = read_utf8(&help_log);
    assert_contains(
        &help_invocations,
        "windows-target:--help",
        "target-filtered prebuilt verification",
    );
    assert_contains(
        &help_invocations,
        "windows-target:plan execution --help",
        "target-filtered prebuilt verification",
    );
    assert_contains(
        &help_invocations,
        "windows-target:workflow --help",
        "target-filtered prebuilt verification",
    );
    assert_not_contains(
        &String::from_utf8_lossy(&output.stdout),
        "prebuilt_runtime_help_skipped",
        "target-filtered prebuilt verification",
    );
}

#[test]
fn prebuilt_runtime_provenance_rejects_matching_manifest_target_help_failures() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let darwin_body = "#!/usr/bin/env bash\nexit 0\n";
    let windows_body = "#!/usr/bin/env bash\nprintf 'windows help failed\\n' >&2\nexit 17\n";
    write_complete_prebuilt_fixture(root, darwin_body, windows_body);

    let output = verify_prebuilt_fixture_with_host_target(root, "windows-x64", &[]);
    assert!(
        !output.status.success(),
        "matching target prebuilt verification should fail when target help fails\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "bin/prebuilt/windows-x64/featureforge.exe --help failed",
        "matching target prebuilt help failure",
    );
}

#[test]
fn prebuilt_runtime_provenance_rejects_denied_strings_even_when_help_is_incompatible() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    let darwin_body =
        "#!/usr/bin/env bash\n# record-review-dispatch must fail the binary audit\nexit 0\n";
    write_complete_prebuilt_fixture(root, darwin_body, "windows runtime\n");

    let output = verify_prebuilt_fixture_with_host_target(root, "windows-x64", &[]);
    assert!(
        !output.status.success(),
        "incompatible-target prebuilt verification should still fail denied-string audits\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "contains denied public/control-plane string",
        "incompatible-target denied-string audit",
    );
}

#[test]
fn prebuilt_runtime_provenance_rejects_hash_mismatches_even_when_help_is_incompatible() {
    let temp = TempDir::new().expect("prebuilt fixture root should exist");
    let root = temp.path();
    write_complete_prebuilt_fixture(
        root,
        "#!/usr/bin/env bash\nprintf 'darwin runtime\\n'\n",
        "windows runtime\n",
    );
    write_executable(
        &root.join("bin/featureforge"),
        "#!/usr/bin/env bash\nprintf 'root drift without denied strings\\n'\n",
    );

    let output = verify_prebuilt_fixture_with_host_target(root, "windows-x64", &[]);
    assert!(
        !output.status.success(),
        "incompatible-target prebuilt verification should still fail root hash drift\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "bin/featureforge: root shipped runtime hash",
        "incompatible-target root hash audit",
    );
}

#[test]
fn shipped_runtime_docs_never_reintroduce_runtime_binary_fallbacks() {
    for path in generated_skill_doc_paths() {
        let label = path.display().to_string();
        let content = read_utf8(&path);
        assert_no_runtime_fallback_execution(&content, &label);
    }

    let upgrade_skill = repo_root().join("featureforge-upgrade/SKILL.md");
    let upgrade_content = read_utf8(&upgrade_skill);
    assert_no_runtime_fallback_execution(&upgrade_content, &upgrade_skill.display().to_string());
}

#[test]
fn future_process_explained_uses_current_execution_command_shapes() {
    let label = "future-process-explained execution command examples";
    let content =
        read_utf8(repo_root().join("docs/archive/featureforge/specs/future-process-explained.md"));

    assert_contains(
        &content,
        "featureforge plan execution begin --plan docs/featureforge/plans/<plan>.md --task <n> --step <step-id> --execution-mode <mode> --expect-execution-fingerprint <fingerprint>",
        label,
    );
    assert_contains(
        &content,
        "featureforge plan execution complete --plan docs/featureforge/plans/<plan>.md --task <n> --step <step-id> --source <source> --claim <claim> --manual-verify-summary <summary> --expect-execution-fingerprint <fingerprint>",
        label,
    );
    assert_contains(
        &content,
        "featureforge plan execution reopen --plan docs/featureforge/plans/<plan>.md --task <n> --step <step-id> --source <source> --reason <reason> --expect-execution-fingerprint <fingerprint>",
        label,
    );
    assert_contains(
        &content,
        "featureforge plan execution transfer --plan docs/featureforge/plans/<plan>.md --scope <task|branch> --to <owner> --reason <reason>",
        label,
    );

    assert_not_contains(
        &content,
        "featureforge plan execution begin --plan docs/featureforge/plans/<plan>.md --task <n>\n",
        label,
    );
    assert_not_contains(
        &content,
        "featureforge plan execution reopen --plan docs/featureforge/plans/<plan>.md --task <n>\n",
        label,
    );
    assert_not_contains(
        &content,
        "featureforge plan execution note --plan docs/featureforge/plans/<plan>.md",
        label,
    );
}

#[test]
fn repo_checkout_canonical_launcher_rejects_stale_prebuilt_checksum() {
    let root = repo_root();
    let darwin_checksum = read_utf8(root.join("bin/prebuilt/darwin-arm64/featureforge.sha256"));
    let windows_checksum = read_utf8(root.join("bin/prebuilt/windows-x64/featureforge.exe.sha256"));
    assert_contains(
        &darwin_checksum,
        "  featureforge",
        "bin/prebuilt/darwin-arm64/featureforge.sha256",
    );
    assert_contains(
        &windows_checksum,
        "  featureforge.exe",
        "bin/prebuilt/windows-x64/featureforge.exe.sha256",
    );
}

#[test]
fn repo_checkout_powershell_launcher_uses_manifest_selected_binary_path() {
    let root = repo_root();
    let powershell_files = fs::read_dir(root.join("bin"))
        .expect("bin dir should be readable")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.ends_with(".ps1").then_some(name)
        })
        .collect::<Vec<_>>();
    assert!(
        powershell_files.is_empty(),
        "standalone runtime should not ship PowerShell wrapper surfaces: {powershell_files:?}"
    );
    let compat_powershell = root.join("compat/powershell");
    if compat_powershell.exists() {
        assert!(
            fs::read_dir(&compat_powershell)
                .expect("compat/powershell should be readable")
                .next()
                .is_none(),
            "compat/powershell should be empty in the standalone runtime"
        );
    }
}

#[test]
fn repo_checkout_powershell_launcher_rejects_stale_prebuilt_checksum() {
    let root = repo_root();
    let shell_helper_files = fs::read_dir(root.join("bin"))
        .expect("bin dir should be readable")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.ends_with("runtime-common.sh") || name.ends_with("pwsh-common.ps1"))
                .then_some(name)
        })
        .collect::<Vec<_>>();
    assert!(
        shell_helper_files.is_empty(),
        "standalone runtime should not ship shell helper files: {shell_helper_files:?}"
    );
}

#[test]
fn repo_checkout_powershell_launcher_preserves_native_exit_code_with_psnative_preference() {
    let root = repo_root();
    let top_level_bin_files = fs::read_dir(root.join("bin"))
        .expect("bin dir should be readable")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            path.is_file()
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        top_level_bin_files,
        vec![String::from("featureforge")],
        "native exit-code handling should rely on the standalone featureforge binary only"
    );
}

#[test]
fn runtime_instruction_docs_point_at_rust_as_the_primary_oracle() {
    let readme = repo_root().join("README.md");
    let docs_testing = repo_root().join("docs/testing.md");

    let readme_content = read_utf8(&readme);
    let docs_testing_content = read_utf8(&docs_testing);

    assert_contains(
        &readme_content,
        "cargo nextest run --all-targets --all-features --no-fail-fast",
        "README.md",
    );
    assert_contains(&readme_content, "more than 1100 tests", "README.md");
    assert_contains(&readme_content, "--no-fail-fast", "README.md");
    assert_not_contains(
        &readme_content,
        "bash tests/differential/run_legacy_vs_rust.sh",
        "README.md",
    );
    assert_not_contains(
        &readme_content,
        "bash tests/codex-runtime/test-runtime-instructions.sh",
        "README.md",
    );
    assert_not_contains(
        &readme_content,
        "bash tests/codex-runtime/test-workflow-sequencing.sh",
        "README.md",
    );
    assert_not_contains(
        &readme_content,
        "bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh",
        "README.md",
    );
    assert_not_contains(
        &readme_content,
        "bash tests/codex-runtime/test-upgrade-skill.sh",
        "README.md",
    );

    assert_contains(
        &docs_testing_content,
        "cargo nextest run --all-targets --all-features --no-fail-fast",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "more than 1100 tests",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "cargo clippy --all-targets --all-features -- -D warnings",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "captures the full failure set",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "node scripts/gen-agent-docs.mjs --check",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "node scripts/lint-workspace-runtime-evidence.mjs",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "workflow-status snapshot coverage for the ambiguous-spec route lives in `tests/workflow_runtime.rs`",
        "docs/testing.md",
    );
    for legacy_command in [
        "bash tests/differential/run_legacy_vs_rust.sh",
        "bash tests/codex-runtime/test-runtime-instructions.sh",
        "bash tests/codex-runtime/test-workflow-enhancements.sh",
        "bash tests/codex-runtime/test-workflow-sequencing.sh",
        "bash tests/codex-runtime/test-using-featureforge-bypass.sh",
        "bash tests/codex-runtime/test-session-entry-gate.sh",
        "bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh",
        "bash tests/codex-runtime/test-upgrade-skill.sh",
    ] {
        assert_not_contains(&docs_testing_content, legacy_command, "docs/testing.md");
    }
    assert_contains(
        &docs_testing_content,
        "Legacy `tests/codex-runtime/*.sh` harnesses have been removed",
        "docs/testing.md",
    );
}

#[test]
fn runtime_instruction_docs_keep_runtime_state_authoritative_and_publish_full_nextest_gate() {
    let root = repo_root();
    let readme_content = read_utf8(root.join("README.md"));
    let subagent_skill = read_utf8(root.join("skills/subagent-driven-development/SKILL.md"));
    let executing_plans_skill = read_utf8(root.join("skills/executing-plans/SKILL.md"));
    let docs_testing_content = read_utf8(root.join("docs/testing.md"));

    assert_contains(
        &readme_content,
        "normal runtime commands render current read models under the runtime state directory; explicit materialization writes repo-local human-readable exports under `docs/featureforge/projections/` instead of mutating approved plan or evidence files",
        "README.md",
    );
    assert_not_contains(
        &readme_content,
        "execution progress truth for operators lives in the approved plan checklist",
        "README.md",
    );
    assert_contains(
        &subagent_skill,
        "Runtime read models are rendered under the state directory during normal execution. Repo-local projection files under `docs/featureforge/projections/` are optional human-readable exports; do not create or maintain a separate ad hoc task tracker outside workflow/operator and status.",
        "skills/subagent-driven-development/SKILL.md",
    );
    assert_contains(
        &subagent_skill,
        "Use `$_FEATUREFORGE_BIN plan execution materialize-projections --plan <approved-plan-path>` for state-dir-only diagnostic projection refreshes. If the user explicitly needs repo-local human-readable projection exports, add `--repo-export --confirm-repo-export`; approved plan and evidence files are not modified, and materialization is never required for normal progress.",
        "skills/subagent-driven-development/SKILL.md",
    );
    assert_not_contains(
        &subagent_skill,
        "The approved plan checklist is the execution progress record; do not create or maintain a separate authoritative task tracker.",
        "skills/subagent-driven-development/SKILL.md",
    );
    assert_contains(
        &executing_plans_skill,
        "Runtime read models are rendered under the state directory during normal execution. Repo-local projection files under `docs/featureforge/projections/` are optional human-readable exports; do not create or maintain a separate ad hoc task tracker outside workflow/operator and status.",
        "skills/executing-plans/SKILL.md",
    );
    assert_contains(
        &executing_plans_skill,
        "Use `$_FEATUREFORGE_BIN plan execution materialize-projections --plan <approved-plan-path>` for state-dir-only diagnostic projection refreshes. If the user explicitly needs repo-local human-readable projection exports, add `--repo-export --confirm-repo-export`; approved plan and evidence files are not modified, and materialization is never required for normal progress.",
        "skills/executing-plans/SKILL.md",
    );
    assert_not_contains(
        &executing_plans_skill,
        "The approved plan checklist is the execution progress record; do not create or maintain a separate authoritative task tracker.",
        "skills/executing-plans/SKILL.md",
    );
    assert_not_contains(
        &executing_plans_skill,
        "use the approved plan checklist as the execution progress record.",
        "skills/executing-plans/SKILL.md",
    );
    assert_not_contains(
        &executing_plans_skill,
        "Use the approved plan checklist as the visible progress record for the task's steps.",
        "skills/executing-plans/SKILL.md",
    );
    assert_not_contains(
        &subagent_skill,
        "[Use the approved plan as the execution-progress record]",
        "skills/subagent-driven-development/SKILL.md",
    );
    assert_contains(
        &docs_testing_content,
        "For branch proof, task-completion gates, plan-task review loops, and pre-merge verification, use the full Rust nextest suite instead:",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "cargo nextest run --all-targets --all-features --no-fail-fast",
        "docs/testing.md",
    );
    assert_contains(
        &docs_testing_content,
        "Targeted `cargo nextest run --test ...` commands are local debugging tools only. Do not use them as the documented final gate.",
        "docs/testing.md",
    );
    assert_not_contains(
        &docs_testing_content,
        "task closure happy path `<= 2` runtime-management commands",
        "docs/testing.md",
    );
    assert_not_contains(
        &docs_testing_content,
        "internal-dispatch task closure `<= 2` runtime-management commands",
        "docs/testing.md",
    );
    assert_not_contains(
        &docs_testing_content,
        "rebase / resume stale-boundary recovery `<= 3` runtime-management commands before implementation resumes",
        "docs/testing.md",
    );
    assert_not_contains(
        &docs_testing_content,
        "stale release refresh `<= 3` runtime-management commands before the next real review step",
        "docs/testing.md",
    );
    assert_not_contains(
        &docs_testing_content,
        "Run the FS-13 fixture and confirm the runtime surfaces the earliest stale boundary without any manual edit to `**Execution Note:**` lines.",
        "docs/testing.md",
    );
}

#[test]
fn runtime_docs_and_fixtures_do_not_depend_on_the_removed_differential_shell_harness() {
    let root = repo_root();

    assert!(
        !root
            .join("tests/differential/run_legacy_vs_rust.sh")
            .exists(),
        "tests/differential/run_legacy_vs_rust.sh should be removed once the snapshot lives in workflow_runtime.rs"
    );
    assert!(
        !root.join("tests/differential/README.md").exists(),
        "tests/differential/README.md should be removed once the shell harness is gone"
    );

    assert_file_not_contains(root.join("README.md"), "run_legacy_vs_rust.sh");
    assert_file_not_contains(root.join("docs/testing.md"), "run_legacy_vs_rust.sh");
    assert_file_not_contains(
        root.join("docs/test-suite-enhancement-plan.md"),
        "tests/differential/",
    );
    assert_file_contains(
        root.join("docs/testing.md"),
        "workflow-status snapshot coverage for the ambiguous-spec route lives in `tests/workflow_runtime.rs`",
    );
    assert!(
        root.join("tests/fixtures/differential/workflow-status.json")
            .is_file(),
        "the checked-in workflow-status snapshot fixture should remain available"
    );
}

#[test]
fn runtime_instruction_surface_contracts_and_generation_checks_hold() {
    let root = repo_root();

    for required in [
        "README.md",
        ".codex/INSTALL.md",
        ".copilot/INSTALL.md",
        "docs/testing.md",
        "review/checklist.md",
        "qa/references/issue-taxonomy.md",
        "qa/templates/qa-report-template.md",
        "scripts/lint-workspace-runtime-evidence.mjs",
        "tests/runtime_instruction_contracts.rs",
        "tests/using_featureforge_skill.rs",
        "tests/powershell_wrapper_resolution.rs",
        "tests/upgrade_skill.rs",
    ] {
        assert!(
            root.join(required).is_file(),
            "{} should exist",
            root.join(required).display()
        );
    }

    for retired in [
        ".claude-plugin",
        ".cursor-plugin",
        ".opencode/INSTALL.md",
        "docs/README.opencode.md",
        "docs/windows/polyglot-hooks.md",
        "hooks",
        "tests/explicit-skill-requests",
        "tests/skill-triggering",
        "tests/claude-code",
        "tests/opencode",
        "tests/subagent-driven-dev",
    ] {
        assert!(
            !root.join(retired).exists(),
            "{} should remain absent",
            root.join(retired).display()
        );
    }

    let active_doc_files = [
        "README.md",
        ".codex/INSTALL.md",
        ".copilot/INSTALL.md",
        "docs/README.codex.md",
        "docs/README.copilot.md",
        "skills/plan-ceo-review/SKILL.md",
        "skills/plan-eng-review/SKILL.md",
        "skills/using-featureforge/SKILL.md",
        "skills/using-git-worktrees/SKILL.md",
        "skills/subagent-driven-development/SKILL.md",
        "skills/dispatching-parallel-agents/SKILL.md",
        "skills/using-featureforge/references/codex-tools.md",
    ];
    let banned_terms = [
        "cursor",
        "opencode",
        "Skill tool",
        "Task tool",
        "TodoWrite",
        ".claude/",
    ];
    for file in active_doc_files {
        let content = read_utf8(root.join(file));
        for term in banned_terms {
            let allowed = content.contains(
                "Legacy Claude, Cursor, and OpenCode-specific loading flows are intentionally unsupported in this runtime package.",
            ) || content.contains(
                "Legacy prompt docs such as `CLAUDE.md` are intentionally unsupported in this runtime workflow.",
            );
            if !allowed {
                assert_not_contains(&content.to_lowercase(), &term.to_lowercase(), file);
            }
        }
    }

    let mut windows_docs_check = Command::new("rg");
    windows_docs_check
        .args([
            "-nP",
            r"bin\\featureforge-(migrate-install|config|update-check)(?!\.ps1)",
            "README.md",
            ".codex/INSTALL.md",
            ".copilot/INSTALL.md",
            "docs/README.codex.md",
            "docs/README.copilot.md",
        ])
        .current_dir(&root);
    let windows_docs_output = run(windows_docs_check, "windows helper doc contract");
    assert!(
        !windows_docs_output.status.success(),
        "windows-facing docs should not reference bare bash helper paths\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&windows_docs_output.stdout),
        String::from_utf8_lossy(&windows_docs_output.stderr)
    );

    for (path, canonical_commands, retired_forms) in [
        (
            ".codex/INSTALL.md",
            [
                "~/.featureforge/install/bin/featureforge config set featureforge_contributor true",
                "~/.featureforge/install/bin/featureforge config set update_check true",
                "featureforge.exe",
                "for `update-check` automatically",
            ],
            [
                "~/.featureforge/install/bin/featureforge install migrate",
                "~/.featureforge/install/bin/featureforge-migrate-install",
                "~/.featureforge/install/bin/featureforge-config",
                "~/.featureforge/install/bin/featureforge-update-check",
                "PendingMigration",
            ],
        ),
        (
            ".copilot/INSTALL.md",
            [
                "~/.featureforge/install/bin/featureforge config set featureforge_contributor true",
                "~/.featureforge/install/bin/featureforge config set update_check true",
                "featureforge.exe",
                "for `update-check` automatically",
            ],
            [
                "~/.featureforge/install/bin/featureforge install migrate",
                "~/.featureforge/install/bin/featureforge-migrate-install",
                "~/.featureforge/install/bin/featureforge-config",
                "~/.featureforge/install/bin/featureforge-update-check",
                "PendingMigration",
            ],
        ),
    ] {
        let content = read_utf8(root.join(path));
        for command in canonical_commands {
            assert_contains(&content, command, path);
        }
        for retired in retired_forms {
            assert_not_contains(&content, retired, path);
        }
    }

    let runtime_version = read_utf8(root.join("VERSION")).trim().to_owned();
    let manifest = read_utf8(root.join("bin/prebuilt/manifest.json"));
    assert_contains(
        &manifest,
        &format!("\"runtime_revision\": \"{runtime_version}\""),
        "bin/prebuilt/manifest.json",
    );

    assert_generated_skill_docs_current(&root);
    assert_generated_agent_docs_current(&root);

    assert_file_contains(
        root.join("README.md"),
        "`using-featureforge` is the human-readable entry router that consults `$_FEATUREFORGE_BIN workflow` directly from repo-visible artifacts.",
    );
    assert_file_not_contains(root.join("README.md"), "featureforge session-entry");
    assert_file_not_contains(
        root.join("README.md"),
        "FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY",
    );
    assert_file_contains(root.join("README.md"), "$_FEATUREFORGE_BIN repo-safety");
    assert_file_contains(root.join("README.md"), "$_FEATUREFORGE_BIN plan contract");
    assert_file_contains(root.join("README.md"), "protected branches");
    assert_file_contains(root.join("README.md"), "Seven layers matter:");
    assert_file_contains(
        root.join("AGENTS.md"),
        "`docs/project_notes/` is supportive memory only; approved specs, plans, execution evidence, review artifacts, runtime state, and active repo instructions remain authoritative.",
    );
    assert_file_contains(
        root.join("AGENTS.md"),
        "Before inventing a new cross-cutting approach, check `docs/project_notes/decisions.md` for prior decisions and follow the authoritative source it links.",
    );
    assert_file_contains(
        root.join("AGENTS.md"),
        "When debugging recurring failures, check `docs/project_notes/bugs.md` for previously recorded root causes, fixes, and prevention notes.",
    );
    assert_file_contains(
        root.join("AGENTS.md"),
        "Never store credentials, secrets, or secret-shaped values in `docs/project_notes/`.",
    );
    assert_file_contains(
        root.join("AGENTS.md"),
        "Use `featureforge:project-memory` when setting up or making structured updates to repo-visible project memory.",
    );
    assert_file_contains(
        root.join("README.md"),
        "`featureforge:project-memory` is an optional support skill for maintaining `docs/project_notes/*`.",
    );
    assert_file_contains(
        root.join("README.md"),
        "It is not a workflow stage, approval gate, or mandatory part of the default planning/execution stack.",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "Accelerated review is an opt-in branch inside `plan-ceo-review` and `plan-eng-review`, not a separate workflow stage.",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "`using-featureforge` is the human-readable entry router that consults `$_FEATUREFORGE_BIN workflow` directly from repo-visible artifacts.",
    );
    assert_file_not_contains(
        root.join("docs/README.codex.md"),
        "featureforge session-entry",
    );
    assert_file_not_contains(
        root.join("docs/README.codex.md"),
        "FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "run the packaged install binary under `~/.featureforge/install/bin/` (`featureforge` on Unix, `featureforge.exe` on Windows)",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "Accelerated review is an opt-in branch inside `plan-ceo-review` and `plan-eng-review`, not a separate workflow stage.",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "`using-featureforge` is the human-readable entry router that consults `$_FEATUREFORGE_BIN workflow` directly from repo-visible artifacts.",
    );
    assert_file_not_contains(
        root.join("docs/README.copilot.md"),
        "featureforge session-entry",
    );
    assert_file_not_contains(
        root.join("docs/README.copilot.md"),
        "FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "run the packaged install binary under `~/.featureforge/install/bin/` (`featureforge` on Unix, `featureforge.exe` on Windows)",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "`featureforge:project-memory` is an opt-in supportive memory skill for `docs/project_notes/*`; use it only for explicit memory-oriented requests or later follow-up updates, not as a default workflow stage or gate",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "`featureforge:project-memory` is an opt-in supportive memory skill for `docs/project_notes/*`; use it only for explicit memory-oriented requests or later follow-up updates, not as a default workflow stage or gate",
    );
    assert_file_contains(
        root.join("README.md"),
        "node scripts/gen-agent-docs.mjs --check",
    );
    assert_file_contains(
        root.join("docs/testing.md"),
        "direct workflow routing without session-entry prerequisites",
    );
}

#[test]
fn cutover_script_keeps_the_legacy_root_content_scan_repo_bounded_and_single_pass() {
    let script = read_utf8(repo_root().join("scripts/check-featureforge-cutover.sh"));

    // Intentional performance and plan-delivery contract: the cutover gate
    // must classify active versus archived content from one repo-wide scan, not
    // drift back into one `rg` subprocess per tracked file as the repo grows.
    assert_contains(
        &script,
        "while IFS= read -r hit; do",
        "scripts/check-featureforge-cutover.sh",
    );
    assert_contains(
        &script,
        "done < <(grep -nH -E \"$LEGACY_ROOT_REGEX\" -- \"${surface_files[@]}\" || true)",
        "scripts/check-featureforge-cutover.sh",
    );
    assert_not_contains(
        &script,
        "done < <(rg -n -H -I \"$LEGACY_ROOT_REGEX\" \"$file\" || true)",
        "scripts/check-featureforge-cutover.sh",
    );
}

#[test]
fn cutover_script_runs_prebuilt_runtime_provenance_gate() {
    let script = read_utf8(repo_root().join("scripts/check-featureforge-cutover.sh"));

    assert_contains(
        &script,
        "scripts/prebuilt-runtime-provenance.mjs",
        "scripts/check-featureforge-cutover.sh",
    );
    assert_contains(
        &script,
        "verify --repo-root",
        "scripts/check-featureforge-cutover.sh",
    );
}

#[test]
fn cutover_script_runs_workspace_runtime_evidence_lint_gate() {
    let script = read_utf8(repo_root().join("scripts/check-featureforge-cutover.sh"));

    assert_contains(
        &script,
        "scripts/lint-workspace-runtime-evidence.mjs",
        "scripts/check-featureforge-cutover.sh",
    );
    assert_contains(
        &script,
        "workspace-runtime evidence lint failed",
        "scripts/check-featureforge-cutover.sh",
    );
}

#[test]
fn evidence_lint_rejects_workspace_runtime_live_mutation() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    let forbidden_commands = [
        "./bin/featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
        "/Users/example/development/featureforge/bin/featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
        "./target/debug/featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
        "cargo run -- plan execution repair-review-state --plan docs/featureforge/plans/example.md",
        "./bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "/Users/example/development/featureforge/target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "./target/release/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "cargo run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "cargo -q run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "cargo --quiet run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "cargo +stable run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "cargo r -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "./target/debug/featureforge plan contract build-task-packet --plan docs/featureforge/plans/example.md --task 1 --persist yes",
        "cargo run -- plan contract build-task-packet --plan docs/featureforge/plans/example.md --task 1 --persist=yes",
        "cargo run -- plan execution advance-late-stage --plan docs/featureforge/plans/example.md --reviewer-source fresh-context-subagent --reviewer-id 019df56c-0fb2-75f1-866d-97921b961cb5 --result pass --summary-file docs/featureforge/execution-evidence/final-review-summary.md",
        "cargo run -p featureforge -- plan execution materialize-projections --plan docs/featureforge/plans/example.md",
        "cargo run -- repo-safety approve --stage featureforge:project-memory --task-id memory-update --reason explicit-approval --path docs/project_notes/decisions.md --write-target repo-file-write",
    ];
    for (index, command) in forbidden_commands.iter().enumerate() {
        write_utf8(
            &temp.path().join(format!(
                "docs/featureforge/execution-evidence/live-mutation-{index}.md"
            )),
            &format!("Recorded command:\n{command}\n"),
        );
    }

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should fail for live mutation commands"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "workspace-runtime evidence lint failed:",
        "workspace-runtime evidence lint stderr",
    );
    assert_contains(
        &stderr,
        "docs/featureforge/execution-evidence/live-mutation-0.md",
        "workspace-runtime evidence lint stderr",
    );
    for command in [
        "./bin/featureforge plan execution repair-review-state",
        "./target/debug/featureforge plan execution repair-review-state",
        "cargo run -- plan execution repair-review-state",
        "./bin/featureforge plan execution close-current-task",
        "./target/debug/featureforge plan execution close-current-task",
        "./target/release/featureforge plan execution close-current-task",
        "cargo run -- plan execution close-current-task",
        "./target/debug/featureforge plan contract build-task-packet",
        "cargo run -- plan contract build-task-packet",
        "cargo run -- plan execution advance-late-stage",
        "cargo run -- plan execution materialize-projections",
        "cargo run -- repo-safety approve",
    ] {
        assert_contains(&stderr, command, "workspace-runtime evidence lint stderr");
    }
}

#[test]
fn workspace_runtime_evidence_lint_covers_runtime_guarded_live_mutations() {
    let root = repo_root();
    let cli_runtime = read_utf8(root.join("src/lib.rs"));
    let evidence_lint = read_utf8(root.join("scripts/lint-workspace-runtime-evidence.mjs"));
    let guarded_commands = extract_workspace_runtime_guard_commands(&cli_runtime);
    let lint_suffixes = extract_js_string_array(&evidence_lint, "LIVE_WORKFLOW_COMMAND_SUFFIXES");

    assert!(
        !guarded_commands.is_empty(),
        "src/lib.rs should declare workspace-runtime live-mutation guards"
    );
    for command in guarded_commands {
        assert!(
            lint_suffixes.contains(&command),
            "workspace-runtime evidence lint should cover runtime-guarded live mutation command: {command}"
        );
    }
}

#[test]
fn evidence_lint_rejects_workspace_runtime_live_workflow_routing_commands() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    let forbidden_commands = [
        "./target/debug/featureforge workflow operator --plan docs/featureforge/plans/example.md --json",
        "cargo run -- workflow doctor --json",
        "./target/debug/featureforge workflow status --json",
        "./bin/featureforge plan execution status --json",
    ];
    for (index, command) in forbidden_commands.iter().enumerate() {
        write_utf8(
            &temp.path().join(format!(
                "docs/featureforge/handoffs/live-routing-{index}.md"
            )),
            &format!("Recorded live workflow command:\n{command}\n"),
        );
    }

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should fail for live workflow routing commands"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for command in [
        "./target/debug/featureforge workflow operator",
        "cargo run -- workflow doctor",
        "./target/debug/featureforge workflow status",
        "./bin/featureforge plan execution status",
    ] {
        assert_contains(&stderr, command, "workspace-runtime evidence lint stderr");
    }
}

#[test]
fn evidence_lint_allows_workspace_runtime_live_workflow_routing_with_temp_state() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/handoffs/temp-state-routing-safe.md"),
        "Fixture-only temp-state workflow command:\nFEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge workflow operator --plan docs/featureforge/plans/example.md --json\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        output.status.success(),
        "workspace-runtime evidence lint should allow temp-state workflow routing examples\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evidence_lint_allows_workspace_runtime_fixture_temp_state_examples() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp.path().join(".featureforge/reviews/temp-state-safe.md"),
        "Fixture-only temp-state execution:\nFEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        output.status.success(),
        "workspace-runtime evidence lint should allow fixture/temp-state examples\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evidence_lint_allows_non_persisted_workspace_task_packet_examples() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/read-only-task-packet.md"),
        "Read-only task-packet inspection:\n./target/debug/featureforge plan contract build-task-packet --plan docs/featureforge/plans/example.md --task 1 --persist no\ncargo run -- plan contract build-task-packet --plan docs/featureforge/plans/example.md --task 1\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        output.status.success(),
        "workspace-runtime evidence lint should allow non-persisted task-packet examples\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evidence_lint_allows_persisted_workspace_task_packet_with_temp_state() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/temp-task-packet.md"),
        "Fixture-only temp-state task-packet cache execution:\nFEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan contract build-task-packet --plan docs/featureforge/plans/example.md --task 1 --persist yes\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        output.status.success(),
        "workspace-runtime evidence lint should allow persisted task-packet examples with temp state\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evidence_lint_allows_literal_temp_and_fixture_state_values() {
    let cases = [
        (
            "literal-tmp-inline.md",
            "FEATUREFORGE_STATE_DIR=\"/tmp/featureforge-fixture-state\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        ),
        (
            "literal-fixture-inline.md",
            "FEATUREFORGE_STATE_DIR=\"tests/fixtures/temp-state\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        ),
        (
            "literal-tmp-exported.md",
            "export FEATUREFORGE_STATE_DIR=\"/private/tmp/featureforge-fixture-state\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        ),
        (
            "literal-fixture-exported.md",
            "export FEATUREFORGE_STATE_DIR=\"tests/fixtures/runtime-state\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        ),
    ];

    for (file_name, command) in cases {
        let temp = TempDir::new().expect("lint fixture root should exist");
        write_utf8(
            &temp
                .path()
                .join(format!(".featureforge/reviews/{file_name}")),
            &format!("Fixture-only temp-state execution:\n{command}\n"),
        );

        let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
        assert!(
            output.status.success(),
            "workspace-runtime evidence lint should allow literal temp/fixture state value {command}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn evidence_lint_allows_exported_temp_state_for_workspace_runtime_examples() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/exported-temp-state-safe.md"),
        "Fixture-only temp-state execution:\nexport FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        output.status.success(),
        "workspace-runtime evidence lint should allow exported temp-state examples\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evidence_lint_rejects_unexported_split_temp_state_assignment() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    let cases = [
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" && ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"; ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" || ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" echo \"$(./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass)\"",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" echo `./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass`",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" RESULT=$(./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass)",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" true | ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" sleep 1 & ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
    ];
    for (index, command) in cases.iter().enumerate() {
        write_utf8(
            &temp.path().join(format!(
                ".featureforge/reviews/unexported-temp-state-unsafe-{index}.md"
            )),
            &format!("Fixture-only temp-state execution:\n{command}\n"),
        );
    }

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should reject split unexported temp-state assignments"
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "./target/debug/featureforge plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_exported_temp_state_shell_boundaries() {
    let cases = [
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" | ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" & ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" || ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" && ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"; ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\ntrue | ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\nsleep 1 & ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\nfalse || ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\necho \"$(./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass)\"",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\necho `./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass`",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\nRESULT=$(./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass)",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass | tee out.log",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass &",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass && echo done",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass || echo failed",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass; echo done",
    ];

    for (index, command) in cases.iter().enumerate() {
        let temp = TempDir::new().expect("lint fixture root should exist");
        write_utf8(
            &temp.path().join(format!(
                ".featureforge/reviews/exported-temp-state-unsafe-{index}.md"
            )),
            &format!("Fixture-only temp-state execution:\n{command}\n"),
        );

        let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
        assert!(
            !output.status.success(),
            "workspace-runtime evidence lint should reject exported temp-state shell boundary {command}"
        );
        assert_contains(
            &String::from_utf8_lossy(&output.stderr),
            "./target/debug/featureforge plan execution close-current-task",
            "workspace-runtime evidence lint stderr",
        );
    }
}

#[test]
fn evidence_lint_rejects_safe_state_rhs_substitution_and_suffix_boundaries() {
    let cases = [
        "FEATUREFORGE_STATE_DIR=\"$(echo fixture-state)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d $(echo nested))\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(echo fixture-state)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"$(mktemp -d $(echo nested))\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass | tee out.log",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass &",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass && echo done",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass || echo failed",
        "FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass; echo done",
    ];

    for (index, command) in cases.iter().enumerate() {
        let temp = TempDir::new().expect("lint fixture root should exist");
        write_utf8(
            &temp.path().join(format!(
                ".featureforge/reviews/state-rhs-or-suffix-unsafe-{index}.md"
            )),
            &format!("Fixture-only temp-state execution:\n{command}\n"),
        );

        let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
        assert!(
            !output.status.success(),
            "workspace-runtime evidence lint should reject unsafe state RHS or suffix boundary {command}"
        );
        assert_contains(
            &String::from_utf8_lossy(&output.stderr),
            "./target/debug/featureforge plan execution close-current-task",
            "workspace-runtime evidence lint stderr",
        );
    }
}

#[test]
fn evidence_lint_rejects_malformed_or_wrapped_safe_state_assignments() {
    let cases = [
        "export FEATUREFORGE_STATE_DIR = /tmp/featureforge-fixture-state\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR = /tmp/featureforge-fixture-state ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"/tmp/featureforge-fixture-state\" env -u FEATUREFORGE_STATE_DIR ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"/tmp/featureforge-fixture-state\" sudo ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR=\"/tmp/featureforge-fixture-state\" sh -c './target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass'",
        "export FEATUREFORGE_STATE_DIR=\"/tmp/featureforge-fixture-state\"\nenv -u FEATUREFORGE_STATE_DIR ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"/tmp/featureforge-fixture-state\"\nsudo ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export FEATUREFORGE_STATE_DIR=\"/tmp/featureforge-fixture-state\"\nsh -c './target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass'",
    ];

    for (index, command) in cases.iter().enumerate() {
        let temp = TempDir::new().expect("lint fixture root should exist");
        write_utf8(
            &temp.path().join(format!(
                ".featureforge/reviews/state-assignment-bypass-{index}.md"
            )),
            &format!("Fixture-only temp-state execution:\n{command}\n"),
        );

        let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
        assert!(
            !output.status.success(),
            "workspace-runtime evidence lint should reject malformed or wrapped state assignment {command}"
        );
        assert_contains(
            &String::from_utf8_lossy(&output.stderr),
            "./target/debug/featureforge plan execution close-current-task",
            "workspace-runtime evidence lint stderr",
        );
    }
}

#[test]
fn evidence_lint_rejects_post_command_temp_state_assignment() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    let cases = [
        "./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass; FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"",
        "./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass; export FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"",
    ];
    for (index, command) in cases.iter().enumerate() {
        write_utf8(
            &temp.path().join(format!(
                ".featureforge/reviews/post-command-temp-state-{index}.md"
            )),
            &format!("Fixture-only temp-state execution:\n{command}\n"),
        );
    }

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should reject post-command temp-state assignments"
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "./target/debug/featureforge plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_fake_temp_state_isolation() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    let cases = [
        "NOT_FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "FEATUREFORGE_STATE_DIR_BACKUP=\"$(mktemp -d)\" ./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "export NOT_FEATUREFORGE_STATE_DIR=\"$(mktemp -d)\"\n./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        "./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass --state-dir \"$(mktemp -d)\"",
    ];
    for (index, command) in cases.iter().enumerate() {
        write_utf8(
            &temp
                .path()
                .join(format!(".featureforge/reviews/fake-isolation-{index}.md")),
            &format!("Fixture-only temp-state execution:\n{command}\n"),
        );
    }

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should reject fake temp-state isolation"
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "./target/debug/featureforge plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_workspace_runtime_commands_with_live_state_markers() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/live-state-marker.md"),
        "Fixture-only temp-state execution (invalid override case):\nFEATUREFORGE_STATE_DIR=\"${HOME}/.featureforge\" cargo run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should fail when temp/fixture context is mixed with live-state markers"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "cargo run -- plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
    assert_contains(
        &stderr,
        "mixed with live ~/.featureforge state markers",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_scans_docs_featureforge_reviews_root() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp.path().join("docs/featureforge/reviews/review.md"),
        "Unsafe review artifact command:\n./bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should scan docs/featureforge/reviews by default"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "docs/featureforge/reviews/review.md",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_workspace_runtime_variant_launch_forms() {
    let cases = [
        (
            "cargo -q run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "cargo run -- plan execution close-current-task",
        ),
        (
            "cargo --quiet run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "cargo run -- plan execution close-current-task",
        ),
        (
            "cargo +stable run -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "cargo run -- plan execution close-current-task",
        ),
        (
            "cargo r -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "cargo run -- plan execution close-current-task",
        ),
        (
            "cargo run -q plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "cargo run -- plan execution close-current-task",
        ),
        (
            "cargo -q run plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "cargo run -- plan execution close-current-task",
        ),
        (
            "./target/release/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/release/featureforge plan execution close-current-task",
        ),
        (
            "/Users/example/development/featureforge/target/release/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/release/featureforge plan execution close-current-task",
        ),
        (
            "../featureforge/target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/debug/featureforge plan execution close-current-task",
        ),
        (
            "/Users/example/dev/renamed-worktree/target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/debug/featureforge plan execution close-current-task",
        ),
        (
            "/Users/example/dev/renamed-worktree/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "../renamed-worktree/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "renamed-worktree/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "$_REPO_ROOT/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "${REPO_ROOT}/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "$ROOT_DIR/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "${ROOT_DIR}/bin/featureforge workflow status --json",
            "./bin/featureforge workflow status",
        ),
        (
            "$WORKTREE_ROOT/bin/featureforge workflow status --json",
            "./bin/featureforge workflow status",
        ),
        (
            "~/dev/renamed-worktree/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "renamed-worktree/target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/debug/featureforge plan execution close-current-task",
        ),
        (
            "$ROOT_DIR/target/debug/featureforge workflow status --json",
            "./target/debug/featureforge workflow status",
        ),
        (
            "${ROOT_DIR}/target/release/featureforge workflow status --json",
            "./target/release/featureforge workflow status",
        ),
        (
            "~/dev/renamed-worktree/target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/debug/featureforge plan execution close-current-task",
        ),
        (
            "RESULT=$(./bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass)",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "true;./bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./bin/featureforge plan execution close-current-task",
        ),
        (
            "true&&./target/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/debug/featureforge plan execution close-current-task",
        ),
        (
            "/Users/example/dev/renamed-worktree/target/aarch64-apple-darwin/debug/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/<triple>/debug/featureforge plan execution close-current-task",
        ),
        (
            "$WORKTREE_ROOT/target/aarch64-apple-darwin/debug/featureforge workflow status --json",
            "./target/<triple>/debug/featureforge workflow status",
        ),
        (
            "/Users/example/dev/renamed-worktree/target/x86_64-unknown-linux-gnu/release/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
            "./target/<triple>/release/featureforge plan execution close-current-task",
        ),
        (
            "~/dev/renamed-worktree/target/x86_64-unknown-linux-gnu/release/featureforge workflow status --json",
            "./target/<triple>/release/featureforge workflow status",
        ),
    ];

    for (index, (command, expected_marker)) in cases.iter().enumerate() {
        let temp = TempDir::new().expect("lint fixture root should exist");
        write_utf8(
            &temp.path().join(format!(
                "docs/featureforge/execution-evidence/variant-launch-{index}.md"
            )),
            &format!("Unsafe live mutation variant:\n{command}\n"),
        );

        let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
        assert!(
            !output.status.success(),
            "workspace-runtime evidence lint should reject launch variant {command}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_contains(
            &stderr,
            expected_marker,
            "workspace-runtime evidence lint stderr",
        );
    }

    let temp = TempDir::new().expect("lint fixture root should exist");
    let repo_root_bin = temp.path().join("bin/featureforge");
    let command = format!(
        "{} plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass",
        repo_root_bin.display()
    );
    write_utf8(
        &temp
            .path()
            .join("docs/featureforge/execution-evidence/repo-root-bin.md"),
        &format!("Unsafe live mutation variant:\n{command}\n"),
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should reject repo-root bin launch variant {command}"
    );
    assert_contains(
        &String::from_utf8_lossy(&output.stderr),
        "./bin/featureforge plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_allows_installed_runtime_live_mutation() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/installed-runtime-live-command.md"),
        "Installed control-plane commands:\n/Users/example/.featureforge/install/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n~/.featureforge/install/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n${FEATUREFORGE_INSTALL_ROOT}/bin/featureforge workflow status --json\n$INSTALL_ROOT/bin/featureforge workflow status --json\n${INSTALLED_ROOT}/bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        output.status.success(),
        "workspace-runtime evidence lint should not flag installed runtime live mutation commands\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evidence_lint_rejects_cargo_run_equals_option_live_mutation() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/cargo-equals-unsafe.md"),
        "Unsafe live mutation command:\ncargo run --package=featureforge -- plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should reject cargo run --flag=value live mutation forms"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "cargo run -- plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_test_only_workspace_runtime_live_mutation_without_temp_state() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/test-only-unsafe.md"),
        "Test-only example:\n./bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should fail for test-only wording without temp/fixture isolation context"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "missing nearby fixture/temp-state isolation context",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_negated_temp_state_prose_without_isolation() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/negated-temp-state.md"),
        "Unsafe example without temp-state isolation:\n./bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should reject negated temp-state prose without explicit isolation"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "missing nearby fixture/temp-state isolation context",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_workspace_runtime_wrapped_live_mutation_commands() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join("docs/featureforge/projections/wrapped-unsafe.md"),
        "Wrapped unsafe commands:\n./target/debug/featureforge plan execution \\\nrepair-review-state --plan docs/featureforge/plans/example.md\n./bin/featureforge plan execution \\\nclose-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should fail for wrapped multi-line live mutation commands"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "./target/debug/featureforge plan execution repair-review-state",
        "workspace-runtime evidence lint stderr",
    );
    assert_contains(
        &stderr,
        "./bin/featureforge plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_workspace_runtime_long_wrapped_live_mutation_commands() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join("docs/featureforge/execution-evidence/long-wrapped-unsafe.md"),
        "Long wrapped unsafe command:\n./bin/featureforge \\\nplan \\\nexecution \\\nclose-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should fail for long wrapped live mutation commands"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "./bin/featureforge plan execution close-current-task",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn evidence_lint_rejects_tmp_prefix_sibling_state_dir_paths() {
    let temp = TempDir::new().expect("lint fixture root should exist");
    write_utf8(
        &temp
            .path()
            .join(".featureforge/reviews/tmp-prefix-sibling.md"),
        "Fixture-only temp-state execution:\nFEATUREFORGE_STATE_DIR=\"/tmp-featureforge-liveish\" ./bin/featureforge plan execution close-current-task --plan docs/featureforge/plans/example.md --task 1 --review-result pass --verification-result pass\n",
    );

    let output = run_workspace_runtime_evidence_lint(temp.path(), &[]);
    assert!(
        !output.status.success(),
        "workspace-runtime evidence lint should reject /tmp-* sibling paths that are not under the temp root"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(
        &stderr,
        "missing nearby fixture/temp-state isolation context",
        "workspace-runtime evidence lint stderr",
    );
}

#[test]
fn copilot_install_docs_use_the_skills_root_as_the_discovery_link() {
    let root = repo_root();

    let readme = read_utf8(root.join("README.md"));
    assert_contains(
        &readme,
        "`~/.copilot/skills -> ~/.featureforge/install/skills`",
        "README.md",
    );
    assert_not_contains(
        &readme,
        "`~/.copilot/skills/featureforge -> ~/.featureforge/install/skills`",
        "README.md",
    );

    let copilot_overview = read_utf8(root.join("docs/README.copilot.md"));
    assert_contains(
        &copilot_overview,
        "`~/.copilot/skills -> ~/.featureforge/install/skills`",
        "docs/README.copilot.md",
    );
    assert_contains(
        &copilot_overview,
        "`ls -la ~/.copilot/skills`",
        "docs/README.copilot.md",
    );
    assert_not_contains(
        &copilot_overview,
        "~/.copilot/skills/featureforge",
        "docs/README.copilot.md",
    );

    let install_doc = read_utf8(root.join(".copilot/INSTALL.md"));
    for expected in [
        "mkdir -p ~/.copilot",
        "ln -s ~/.featureforge/install/skills ~/.copilot/skills",
        "ls -la ~/.copilot/skills",
        "rm ~/.copilot/skills",
        "Get-Item \"$env:USERPROFILE\\.copilot\\skills\"",
        "Remove-Item \"$env:USERPROFILE\\.copilot\\skills\"",
        "cmd /c mklink /J \"$env:USERPROFILE\\.copilot\\skills\" \"$env:USERPROFILE\\.featureforge\\install\\skills\"",
    ] {
        assert_contains(&install_doc, expected, ".copilot/INSTALL.md");
    }
    for retired in [
        "~/.copilot/skills/featureforge",
        "$env:USERPROFILE\\.copilot\\skills\\featureforge",
        "mkdir -p ~/.copilot/skills",
        "New-Item -ItemType Directory -Force -Path \"$env:USERPROFILE\\.copilot\\skills\"",
    ] {
        assert_not_contains(&install_doc, retired, ".copilot/INSTALL.md");
    }
}

#[test]
fn workflow_enhancement_contracts_are_documented_consistently() {
    let root = repo_root();

    for (file, patterns) in [
        (
            "review/checklist.md",
            vec![
                "Pre-Landing Review Checklist",
                "SQL & Data Safety",
                "Enum & Value Completeness",
                "Documentation Staleness",
                "TODO Cross-Reference",
                "Built-in Before Bespoke / Known Pattern Footguns",
                "Spec / Plan Delivery Content",
                "Release Readiness",
            ],
        ),
        (
            "skills/requesting-code-review/code-reviewer.md",
            vec![
                "{BASE_BRANCH}",
                "built-in-before-bespoke",
                "known pattern footguns",
                "completed task packets",
                "missing tests for `VERIFY-*` requirements",
                "official documentation",
                "issue trackers or maintainer guidance",
                "primary-source technical references",
                "file:line",
            ],
        ),
        (
            "skills/qa-only/SKILL.md",
            vec![
                "playwright",
                "diff-aware",
                "Health Score",
                "qa-report",
                "Known ecosystem issue lookup (optional)",
                "label the result as a hypothesis, not a fix",
                "do not block the report if search is unavailable",
                "# QA Result",
                "featureforge-{safe-branch}-test-outcome-{datetime}.md",
                "**Base Branch:** main",
                "**Current Reviewed Branch State ID:** git_tree:abc1234",
                "**Branch Closure ID:** branch-release-closure",
                "**Generated By:** featureforge/qa",
                "do not hand-write the structured finish-gate artifact",
            ],
        ),
        (
            "skills/document-release/SKILL.md",
            vec![
                "CHANGELOG",
                "NEVER CLOBBER CHANGELOG ENTRIES",
                "discoverability",
                "RELEASE-NOTES.md",
                "release-readiness",
                "rollout notes",
                "rollback notes",
                "known risks or operator-facing caveats",
                "# Release Readiness Result",
                "featureforge-{safe-branch}-release-readiness-{datetime}.md",
                "**Current Reviewed Branch State ID:** git_tree:abc1234",
                "**Branch Closure ID:** branch-release-closure",
                "**Result:** pass",
                "Allowed `**Result:**` values:",
                "- `pass`",
                "- `blocked`",
                "Artifact `pass` is the runtime-rendered form of CLI input `--result ready`.",
                "Do not hand-write or edit this artifact.",
                "workflow-routed release-readiness must be recorded through runtime-owned commands, not inferred from the companion markdown artifact alone.",
                "If `recommended_public_command_argv` is present, invoke it exactly. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic; otherwise satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun workflow/operator.",
                "If workflow/operator reports `phase_detail=branch_closure_recording_required_for_release_readiness`, use input shape `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path>` with the concrete plan and rerun workflow/operator.",
                "When workflow/operator reports `phase_detail=release_readiness_recording_ready`, use input shape `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> --result ready|blocked --summary-file <release-summary>` only after substituting concrete values.",
                "renders `**Result:** pass|blocked` in the derived companion artifact",
            ],
        ),
        (
            "skills/finishing-a-development-branch/SKILL.md",
            vec![
                "featureforge:requesting-code-review",
                "featureforge:qa-only",
                "featureforge:document-release",
                "Conditional Pre-Landing QA Gate",
                "Required release-readiness pass for workflow-routed work before completion",
                "$_FEATUREFORGE_BIN repo-safety check --intent write",
                "For workflow-routed terminal completion, do not run the terminal review gate in this step. Run it only after `featureforge:document-release` and before any runtime-routed `featureforge:qa-only` handoff.",
                "If the current work is governed by an approved FeatureForge plan, after `featureforge:document-release` and the terminal `featureforge:requesting-code-review` gate are current, rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` and follow the exact `phase_detail`-driven next finish command before presenting completion options.",
                "If the current work is not governed by an approved FeatureForge plan, skip this helper-owned finish gate and continue with the normal completion flow.",
            ],
        ),
    ] {
        for pattern in patterns {
            assert_file_contains(root.join(file), pattern);
        }
    }

    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "|| echo main",
    );
    assert_file_contains(
        root.join("skills/document-release/SKILL.md"),
        "For workflow-routed work, get `BASE_BRANCH` from `$_FEATUREFORGE_BIN workflow operator --json` (`base_branch`) for the concrete approved plan path; any `<approved-plan-path>` command text here is input shape, not exact argv.",
    );
    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "Run `featureforge workflow operator --plan <approved-plan-path>` to confirm the current `phase_detail` before recording release-readiness.",
    );
    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "run `featureforge plan execution advance-late-stage --plan <approved-plan-path> --result ready|blocked --summary-file <release-summary>`",
    );
    assert_file_contains(
        root.join("skills/document-release/SKILL.md"),
        "Do not use PR metadata or repo default-branch APIs as a fallback",
    );
    assert_file_not_contains(root.join("skills/document-release/SKILL.md"), "origin/HEAD");
    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "branch.<current>.gh-merge-base",
    );
    assert_file_contains(
        root.join("skills/document-release/SKILL.md"),
        "`featureforge:document-release` does not replace checkpoint reviews and does not own review-dispatch minting. Keep command-boundary semantics explicit: low-level compatibility/debug commands stay out of the normal-path flow.",
    );
    assert_file_not_contains(root.join("skills/document-release/SKILL.md"), "gh pr view");
    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "defaultBranchRef",
    );
    assert_file_not_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "If Step 1.9 already routed through `featureforge:document-release`",
    );
    assert_file_contains(
        root.join("tests/evals/using-featureforge-routing.scenarios.md"),
        "branch-completion language still routes to `requesting-code-review` when no fresh final review artifact exists",
    );
    assert_file_contains(
        root.join("tests/evals/using-featureforge-routing.orchestrator.md"),
        "Use the real repo-versioned `using-featureforge` entry contract and skill/runtime surfaces from the branch under test",
    );
    assert_file_contains(
        root.join("tests/evals/using-featureforge-routing.orchestrator.md"),
        "Pass the absolute branch-under-test repo root into both runner and judge prompts.",
    );
    assert_file_contains(
        root.join("tests/evals/using-featureforge-routing.orchestrator.md"),
        "invoke `<BRANCH_UNDER_TEST_ROOT>/bin/featureforge` explicitly",
    );
    assert_file_contains(
        root.join("tests/evals/using-featureforge-routing.runner.md"),
        "Use the real repo-versioned `using-featureforge` entry contract and skill/runtime surfaces from the branch under test",
    );
    assert_file_contains(
        root.join("tests/evals/using-featureforge-routing.runner.md"),
        "The controller must pass `BRANCH_UNDER_TEST_ROOT` as an absolute path.",
    );
    assert_file_contains(
        root.join("tests/evals/using-featureforge-routing.runner.md"),
        "Do not rely on temp-fixture runtime-root autodetection or any home-install fallback.",
    );
    assert_file_not_contains(
        root.join("tests/evals/using-featureforge-routing.scenarios.md"),
        "| P3 |",
    );
}

#[test]
fn workflow_sequencing_contracts_and_fixtures_are_documented_consistently() {
    let root = repo_root();

    assert_description_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "exploring a feature idea, behavior change, or architecture direction",
    );
    assert_file_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "Use that repo-relative spec path consistently in later review and workflow/operator commands; do not route through compatibility-only `workflow expect` or `workflow sync` helpers.",
    );
    assert_file_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "After the spec is written or updated, continue using the same repo-relative spec path in downstream review and workflow/operator commands.",
    );
    assert_file_not_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" workflow expect --artifact spec --path",
    );
    assert_file_not_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" workflow sync --artifact spec --path",
    );
    assert_file_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "Landscape Awareness",
    );
    assert_file_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "### Decision impact",
    );
    assert_file_contains(
        root.join("skills/brainstorming/SKILL.md"),
        "$_FEATUREFORGE_BIN repo-safety check --intent write",
    );
    assert_file_contains(
        root.join("skills/brainstorming/visual-companion.md"),
        "may stay attached to the terminal instead of returning immediately",
    );
    assert_file_contains(
        root.join("skills/brainstorming/visual-companion.md"),
        "capture the first `server-started` JSON line for `screen_dir`",
    );
    assert_file_contains(
        root.join("skills/brainstorming/visual-companion.md"),
        "install Git Bash or point `FEATUREFORGE_BASH_PATH` at a compatible `bash`",
    );

    assert_description_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "deciding which skill or workflow stage applies",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "If `$_FEATUREFORGE_BIN` is available and an approved plan path is known, call `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` directly for routing. If no approved plan path is known, resolve the plan path through the normal planning/review handoff rather than calling removed workflow status surfaces.",
    );
    assert_file_not_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "Only after the bypass gate resolves to `enabled` for the current session key",
    );
    assert_file_not_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "treat `execution_started` as an executor-resume signal only when workflow/operator reports `phase` `executing`",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "If workflow/operator reports a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv` instead of resuming `featureforge:subagent-driven-development` or `featureforge:executing-plans` just because `execution_started` is `yes`.",
    );
    assert_file_not_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "review_blocked",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "Treat workflow/operator `phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`, and `required_inputs` as the authoritative public routing contract.",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "Treat human-readable projection artifacts and companion markdown as derived output, not routing authority.",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "Hidden compatibility/debug command entrypoints are removed from the public CLI; keep normal progression on public commands only.",
    );
    assert_file_not_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "featureforge plan execution recommend --plan <approved-plan-path> --isolated-agents <available|unavailable> --session-intent <stay|separate|unknown> --workspace-prepared <yes|no|unknown>",
    );
    assert_file_not_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "featureforge session-entry resolve --message-file <path>",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "plan-ceo-review -> writing-plans -> plan-eng-review; plan-fidelity-review runs only after engineering-review edits are complete, then plan-eng-review performs final approval before execution.",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "Do not re-derive `phase`, `phase_detail`, readiness, or late-stage precedence from markdown headers.",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "If helper routing still cannot be recovered, fail closed to the earlier safe stage (`featureforge:brainstorming`) or remain in the current execution flow; do not route directly into implementation or late-stage recording from fallback logic.",
    );
    assert_file_not_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "newest relevant artifacts",
    );

    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "Use that repo-relative plan path consistently in later review and workflow/operator commands; do not route through compatibility-only `workflow expect` or `workflow sync` helpers.",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "Keep using the same repo-relative plan path in downstream review and workflow/operator handoffs.",
    );
    assert_file_not_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" workflow expect --artifact plan --path",
    );
    assert_file_not_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" workflow sync --artifact plan --path",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" plan contract lint \\",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "## CEO Review Summary",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "additive context only",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "Invoke `featureforge:plan-eng-review` for the first engineering review pass.",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "plan-fidelity runs only after engineering-review edits are complete",
    );
    assert_file_not_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "runtime-owned receipt recording",
    );
    assert_file_not_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "receipt records",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "**Last Reviewed By:** plan-ceo-review",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "**QA Requirement:** required | not-required",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "`QA Requirement` is a plan-level finish-gating decision",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "atomic, binary, objectively reviewable, reviewable without interpretation drift",
    );
    assert_file_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "Legacy task fields such as `Task Outcome`, `Plan Constraints`, or task-level `Open Questions`",
    );
    assert_file_not_contains(
        root.join("skills/writing-plans/SKILL.md"),
        "**Open Questions:**",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "For the final cross-task review gate in workflow-routed work",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "For non-terminal checkpoint/task-boundary review, keep command-boundary semantics explicit: low-level compatibility/debug dispatch commands are not normal intent-level progression.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` before starting execution.",
    );
    assert_file_not_contains(
        root.join("skills/executing-plans/SKILL.md"),
        &hidden_text(&[
            "Run `featureforge workflow ",
            "pre",
            "flight --plan <approved-plan-path>` before starting execution.",
        ]),
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "After the implementation steps for a task are complete, enforce the mandatory task-boundary closure loop before beginning the next task:",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "Task `N+1` may begin only after Task `N` has a current positive task-closure record",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "dedicated-independent review loops plus verification are required inputs to `close-current-task`",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "workflow/operator must route normal task-boundary closure through `task_closure_recording_ready` / `close-current-task`, not `task_review_dispatch_required`; if a task-review dispatch phase appears, treat it as a runtime diagnostic bug instead of manual low-level command choreography",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "After all tasks complete and verified:",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "featureforge:document-release",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "featureforge:requesting-code-review",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; when `recommended_public_command_argv` is absent, treat the closure command shape as an input contract and provide concrete review/verification values through `required_inputs` before rerunning workflow/operator",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        concat!(
            "When workflow/operator reports `review_state_status` as stale or missing closure context, do not invent a repair command. If `recommended_public_command",
            "_argv` is present, invoke it exactly. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic. Otherwise satisfy `required_inputs` or run `$_FEATUREFORGE_BIN plan execution repair-review-state --plan ",
            "<approved-plan-path>` only when the non-diagnostic route owns that repair lane."
        ),
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "After `repair-review-state`, MUST follow that command's returned `recommended_public_command_argv` when present before any additional recording commands. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic; otherwise satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun the route owner.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "Hidden compatibility/debug command entrypoints are removed from the public CLI; normal routing must use public commands only.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "MUST NOT manually edit runtime-owned execution records.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "MUST NOT manually edit `**Execution Note:**` lines to recover runtime state.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "MUST NOT manually edit derived markdown projection artifacts.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "`task_closure_recording_ready` requires `recording_context.task_number`.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "`release_readiness_recording_ready` and `release_blocker_resolution_required` require `recording_context.branch_closure_id`.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "`final_review_recording_ready` requires `recording_context.branch_closure_id`.",
    );
    assert_file_contains_in_order(
        root.join("skills/executing-plans/SKILL.md"),
        &[
            "after review is green, run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`",
            "rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; when `recommended_public_command_argv` is absent, treat the closure command shape as an input contract and provide concrete review/verification values through `required_inputs` before rerunning workflow/operator",
            "no exceptions: only after close-current-task succeeds may Task `N+1` begin",
        ],
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "does not require per-dispatch user-consent prompts.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "`review_remediation`: required after actionable independent-review findings and before remediation starts. Runtime records it automatically when reviewable dispatch lineage enters remediation and when remediation reopens execution work.",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "For FeatureForge-on-FeatureForge execution, every execution-evidence update must include a runtime provenance section",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "installed runtime path used for live workflow routing",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "installed runtime hash used for live workflow routing",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "state dir used for live workflow commands",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "workspace runtime hash used for tests/fixtures (or `none` when no workspace runtime was used)",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "explicit confirmation that workspace runtime did not mutate live workflow state (or the explicit approved override record when it did)",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "After each task in subagent-driven development",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` before dispatching implementation subagents.",
    );
    assert_file_not_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        &hidden_text(&[
            "Run `featureforge workflow ",
            "pre",
            "flight --plan <approved-plan-path>` before dispatching implementation subagents.",
        ]),
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "Those per-task review loops satisfy the \"review early\" rule during execution",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "\"More tasks remain?\" -> \"Use featureforge:document-release for release-readiness before terminal review\" [label=\"no\"];",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "\"Use featureforge:document-release for release-readiness before terminal review\" -> \"Use featureforge:requesting-code-review for final review gate\";",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "Rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; when `recommended_public_command_argv` is absent, treat the closure command shape as an input contract and provide concrete review/verification values through `required_inputs` before rerunning workflow/operator.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "Task `N+1` may begin only after Task `N` has a current positive task-closure record",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "review loops and verification are required inputs to `close-current-task`",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        concat!(
            "When workflow/operator reports `review_state_status` as stale or missing closure context, do not invent a repair command. If `recommended_public_command",
            "_argv` is present, invoke it exactly. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic. Otherwise satisfy `required_inputs` or run `$_FEATUREFORGE_BIN plan execution repair-review-state --plan ",
            "<approved-plan-path>` only when the non-diagnostic route owns that repair lane."
        ),
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "After `repair-review-state`, MUST follow that command's returned `recommended_public_command_argv` when present before any additional recording commands. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic; otherwise satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun the route owner.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "Hidden compatibility/debug command entrypoints are removed from the public CLI; normal routing must use public commands only.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "MUST NOT manually edit runtime-owned execution records.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "MUST NOT manually edit derived markdown projection artifacts.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "`task_closure_recording_ready` requires `recording_context.task_number`.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "`release_readiness_recording_ready` and `release_blocker_resolution_required` require `recording_context.branch_closure_id`.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "`final_review_recording_ready` requires `recording_context.branch_closure_id`.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "`review_remediation`: required after actionable independent-review findings and before remediation starts. Runtime records it automatically when reviewable dispatch lineage enters remediation and when remediation reopens execution work.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "After review is green, run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`.",
    );
    assert_file_contains_in_order(
        root.join("skills/subagent-driven-development/SKILL.md"),
        &[
            "After review is green, run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`.",
            "Rerun `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; when `recommended_public_command_argv` is absent, treat the closure command shape as an input contract and provide concrete review/verification values through `required_inputs` before rerunning workflow/operator.",
            "No exceptions: only after close-current-task succeeds may you dispatch Task `N+1`.",
        ],
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "Workflow/operator must not report `task_review_dispatch_required` for normal task-boundary closure; task closure routes through `close-current-task`. If workflow/operator reports `final_review_dispatch_required`, keep routing through workflow/operator plus intent-level commands and do not expand the loop into low-level dispatch-lineage management.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "does not require per-dispatch user-consent prompts.",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "For FeatureForge-on-FeatureForge execution, every execution-evidence update must include a runtime provenance section",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "installed runtime path used for live workflow routing",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "installed runtime hash used for live workflow routing",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "state dir used for live workflow commands",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "workspace runtime hash used for tests/fixtures (or `none` when no workspace runtime was used)",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "explicit confirmation that workspace runtime did not mutate live workflow state (or the explicit approved override record when it did)",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "If the current work is not governed by an approved FeatureForge plan, skip this helper-owned finish gate and continue with the normal completion flow.",
    );
    assert_file_not_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "after `featureforge:document-release` and any required `featureforge:qa-only` handoff are current",
    );
    assert_file_not_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "after `featureforge:document-release` and any required QA handoff",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "For workflow-routed terminal completion, do not run the terminal review gate in this step. Run it only after `featureforge:document-release` and before any runtime-routed `featureforge:qa-only` handoff.",
    );
    assert_file_contains_in_order(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        &[
            "featureforge:document-release",
            "terminal `featureforge:requesting-code-review`",
            "any required `featureforge:qa-only` handoff",
            "`advance-late-stage` only when operator reports `phase_detail=qa_recording_required`",
            "follow its next finish command",
        ],
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "For plan-routed completion, use the exact `base_branch` from `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` instead of redetecting the target branch.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "The Step 2 `<base-branch>` value stays authoritative for Options A, B, and D. Do not redetect it later in the branch-finishing flow.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "Use the exact `<base-branch>` resolved in Step 2. Do not redetect it during PR creation.",
    );
    assert_file_not_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "If a fresh release-readiness artifact is already present, its `**Base Branch:**` header must match that runtime-owned `base_branch`; if it is missing or blank, stop and return to `featureforge:document-release`.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "If approved-plan `QA Requirement` is missing or invalid when deciding whether QA applies, stop and reroute through `$_FEATUREFORGE_BIN plan execution repair-review-state --plan <path>`; do not guess from test-plan prose.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "If workflow/operator reports `test_plan_refresh_required`, hand control back to `featureforge:plan-eng-review` to regenerate the current-branch test-plan artifact before QA or branch completion.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "If approved-plan `QA Requirement` is `required` and no current-branch test-plan artifact exists for workflow-routed work, stop and regenerate it before invoking `featureforge:qa-only` or late-stage completion commands.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "If the current work is governed by an approved FeatureForge plan, treat the approved plan's normalized `**QA Requirement:** required|not-required` metadata as authoritative for workflow-routed finish gating.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "Treat the current-branch test-plan artifact as a QA scope/provenance input only when its `Source Plan`, `Source Plan Revision`, and `Head SHA` match the exact approved plan path, revision, and current branch HEAD from the workflow context.",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "Match current-branch artifacts by their `**Branch:**` header, not by a filename substring glob, so `my-feature` cannot masquerade as `feature`.",
    );
    assert_file_not_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "*-\"$BRANCH\"-test-plan-*",
    );
    assert_file_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "gh pr create --base \"<base-branch>\"",
    );
    assert_file_not_contains(
        root.join("skills/finishing-a-development-branch/SKILL.md"),
        "gh pr view --json baseRefName",
    );
    assert_file_contains(
        root.join("docs/archive/featureforge/specs")
            .join("2026-04-01-gate-diagnostics-and-runtime-semantics.md"),
        "harness_phase",
    );
    assert_file_not_contains(
        root.join("docs/archive/featureforge/specs")
            .join("2026-04-01-gate-diagnostics-and-runtime-semantics.md"),
        "verbose_available",
    );
    assert_file_contains(
        root.join("docs/featureforge/reference")
            .join("2026-04-01-review-state-reference.md"),
        "same routing decision as workflow/operator for `harness_phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`, `required_inputs`, `recommended_command`, `blocking_scope`, `blocking_reason_codes`, and `external_wait_state`",
    );
    assert_file_contains(
        root.join("docs/featureforge/reference")
            .join("2026-04-01-review-state-reference.md"),
        "`blocking_scope`",
    );
    assert_file_contains(
        root.join("docs/featureforge/reference")
            .join("2026-04-01-review-state-reference.md"),
        "`blocking_reason_codes`",
    );
    assert_file_contains(
        root.join("docs/featureforge/reference")
            .join("2026-04-01-review-state-reference.md"),
        "`external_wait_state`",
    );
    assert_file_contains(
        root.join("docs/featureforge/reference")
            .join("2026-04-01-review-state-reference.md"),
        "compare `status --plan <path> --external-review-result-ready` to `workflow operator --plan <path> --external-review-result-ready`",
    );
    assert_file_not_contains(
        root.join("docs/featureforge/reference")
            .join("2026-04-01-review-state-reference.md"),
        "compare `workflow doctor --plan <path> --external-review-result-ready`",
    );

    assert_file_not_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "featureforge plan execution recommend --plan <approved-plan-path>",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Present the runtime-selected execution owner skill as the default path with the approved plan path.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Do not look for or require a runtime-owned plan-fidelity projection file. The authoritative fidelity evidence is the parseable review artifact surfaced by workflow routing and `plan contract analyze-plan` as `plan_fidelity_review`.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Engineering approval must fail closed unless `contract_state == valid` and `packet_buildable_tasks == task_count`.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        concat!(
            "**The terminal state is presenting the execution pre",
            "flight handoff with the approved plan path.**"
        ),
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "plan-eng-review also owns the late refresh-test-plan lane when approved-plan `QA Requirement` is `required` and finish readiness reports `test_plan_artifact_missing`, `test_plan_artifact_malformed`, `test_plan_artifact_stale`, `test_plan_artifact_authoritative_provenance_invalid`, or `test_plan_artifact_generator_mismatch` for the current approved plan revision.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "**QA Requirement:** required | not-required",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "**Head SHA:** {current-head}",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "This field scopes the QA artifact for testers; it is not the authoritative finish-gate policy source.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Set `**Head SHA:**` to the current `git rev-parse HEAD` for the branch state that this test-plan artifact covers.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        concat!(
            "In that late-stage lane, the terminal state is returning to the finish-gate flow with a regenerated current-branch test-plan artifact, not reopening execution pre",
            "flight."
        ),
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        concat!(
            "Before presenting the final execution pre",
            "flight handoff, if `$_FEATUREFORGE_BIN` is available, call `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`."
        ),
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        concat!(
            "If workflow/operator returns `phase` `executing`, present the normal execution pre",
            "flight handoff below."
        ),
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        concat!(
            "If workflow/operator returns a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv` when present instead of reopening execution pre",
            "flight; when argv is absent, satisfy typed `required_inputs` or the prerequisite named by `next_action`, then rerun workflow/operator."
        ),
    );
    assert_file_not_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "review_blocked",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "SELECTIVE EXPANSION",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "Section 11: Design & UX Review",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "## CEO Review Summary",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "Label the source as `cross-model` only when the outside voice definitely uses a different model/provider than the main reviewer.",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "If model provenance is the same, unknown, or only a fresh-context rerun of the same reviewer family, label the source as `fresh-context-subagent`.",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "A `CEO Approved` spec must end with `**Last Reviewed By:** plan-ceo-review`.",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "If the transport truncates or summarizes the outside-voice output, disclose that limitation plainly in review prose instead of overstating independence.",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "After each spec edit (including final approval edits), keep using the same repo-relative spec path in later workflow/operator and writing-plans handoffs; do not route through compatibility-only `workflow sync`.",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "note `UI_SCOPE` for Section 11",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "Present each expansion opportunity as its own individual interactive user question.",
    );
    assert_file_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "Do not use PR metadata or repo default-branch APIs as a fallback; keep the system audit locally derivable from repository state.",
    );
    assert_file_not_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "gh pr view --json baseRefName",
    );
    assert_file_not_contains(
        root.join("skills/plan-ceo-review/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" workflow sync --artifact spec --path",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "coverage graph",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "An `Engineering Approved` plan must end with `**Last Reviewed By:** plan-eng-review`.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "## Key Interactions",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "## Edge Cases",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "## Critical Paths",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "## E2E Test Decision Matrix",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "REGRESSION RULE",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "loading, empty, error, success, partial, navigation, responsive, and accessibility-critical states",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "compatibility, retry/timeout semantics, replay or backfill behavior, and rollback or migration verification",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "Label the source as `cross-model` only when the outside voice definitely uses a different model/provider than the main reviewer.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "If model provenance is the same, unknown, or only a fresh-context rerun of the same reviewer family, label the source as `fresh-context-subagent`.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "If the transport truncates or summarizes the outside-voice output, disclose that limitation plainly in review prose instead of overstating independence.",
    );
    assert_file_contains(
        root.join("skills/plan-eng-review/SKILL.md"),
        "## Engineering Review Summary",
    );
    assert_file_contains(
        root.join("skills/qa-only/SKILL.md"),
        "## Engineering Review Summary",
    );
    assert_file_contains(
        root.join("skills/qa-only/SKILL.md"),
        "additive context only",
    );
    assert_file_contains(
        root.join("skills/qa-only/SKILL.md"),
        "## E2E Test Decision Matrix",
    );
    assert_file_contains(
        root.join("skills/qa-only/SKILL.md"),
        "Do not use PR metadata or repo default-branch APIs as a fallback; keep diff-aware scoping locally derivable from repository state.",
    );
    assert_file_contains(
        root.join("skills/qa-only/SKILL.md"),
        "If no URL is provided, run `diff-aware` mode with an explicitly provided `BASE_BRANCH`:",
    );
    assert_file_contains(
        root.join("skills/qa-only/SKILL.md"),
        "Match current-branch artifacts by their `**Branch:**` header, not by a filename substring glob, so `my-feature` cannot masquerade as `feature`.",
    );
    assert_file_not_contains(
        root.join("skills/qa-only/SKILL.md"),
        "*-\"$BRANCH\"-test-plan-*",
    );
    assert_file_not_contains(
        root.join("skills/qa-only/SKILL.md"),
        "gh pr view --json baseRefName",
    );
    assert_file_not_contains(
        root.join("skills/qa-only/SKILL.md"),
        "`diff-aware` inference from the current branch",
    );
    assert_file_not_contains(
        root.join("skills/qa-only/SKILL.md"),
        "automatically enter `diff-aware` mode",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Review at the right checkpoints, then fail closed on the final whole-diff gate.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` before dispatching the reviewer.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "If workflow/operator fails, stop and return to the current execution flow; do not guess the public late-stage route from raw execution state.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "plan contract analyze-plan --spec \"$SOURCE_SPEC_PATH\" --plan \"$APPROVED_PLAN_PATH\" --format json",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Do not use PR metadata or repo default-branch APIs as a fallback. For workflow-routed review, require `BASE_BRANCH` from `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` (`base_branch`). For non-plan-routed review, require an explicitly provided `BASE_BRANCH`.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Keep review artifacts runtime-owned:",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "project-scoped code-review companion artifact",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "{user}-{safe-branch}-code-review-{datetime}.md",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "**Generated By:** featureforge:requesting-code-review",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "derived companion for reviewer provenance and audit traceability",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "gh pr view --json baseRefName",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "git log --oneline | grep \"Task 1\"",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "git rev-parse HEAD~1",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "CONTRACT_STATE=$(printf '%s\\n' \"$ANALYZE_JSON\" | node -e 'const fs = require(\"fs\"); const parsed = JSON.parse(fs.readFileSync(0, \"utf8\")); process.stdout.write(parsed.contract_state || \"\")')",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "if [ \"$CONTRACT_STATE\" != \"valid\" ] || [ \"$PACKET_BUILDABLE_TASKS\" != \"$TASK_COUNT\" ]; then",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "When diagnostic status is required, if any of `active_task`, `blocking_task`, or `resume_task` is non-null, stop and return to the current execution flow; final review is only valid when all three are `null`.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "treat workflow/operator as authoritative for the public late-stage route; status is diagnostic only.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "For terminal whole-diff review, only request a fresh external final review when workflow/operator reports `phase=final_review_pending` with `phase_detail=final_review_dispatch_required`.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Pass the exact approved plan path into the reviewer context. When runtime-owned execution evidence or task-packet context is already available from the current workflow handoff, pass it through as supplemental context; do not make the public flow harvest it manually.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Required review-dispatch provenance for FeatureForge-on-FeatureForge work:",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "base branch, base SHA, head SHA, working-tree diff hash, installed runtime path/hash used for live routing, workspace runtime hash used for tests (if any), live state dir, active FeatureForge skill source/roots, installed skill root, and workspace skill root.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Reviewers must fail review when live workflow mutation used workspace runtime without an explicit approved override record.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Reviewers must fail review when active FeatureForge skills resolve from the workspace skill root instead of the installed skill root without an explicit approved self-hosting exception.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "Reviewers must fail review when FeatureForge-on-FeatureForge provenance is missing or incomplete.",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "REVIEW_DISPATCH_JSON=",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "REVIEW_DISPATCH_ACTION=",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "DISPATCH_ID=",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "REVIEW_DISPATCH_ALLOWED=",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "RECORDING_READY_JSON=$(\"$_FEATUREFORGE_BIN\" workflow operator --plan \"$APPROVED_PLAN_PATH\" --external-review-result-ready --json)",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "if [ \"$RECORDING_PHASE_DETAIL\" != \"final_review_recording_ready\" ]; then",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "# Stop here: dispatch the dedicated fresh-context reviewer, wait for its result, then set REVIEW_RESULT=pass|fail and SUMMARY_FILE=<actual-final-review-summary>.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" plan execution advance-late-stage --plan \"$APPROVED_PLAN_PATH\" --reviewer-source fresh-context-subagent --reviewer-id <actual-reviewer-id> --result \"$REVIEW_RESULT\" --summary-file \"$SUMMARY_FILE\"",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "\"$_FEATUREFORGE_BIN\" plan execution advance-late-stage --plan \"$APPROVED_PLAN_PATH\" --reviewer-source fresh-context-subagent --reviewer-id 019d3550-c932-7bb2-9903-33f68d7c30ca --result pass --summary-file review-summary.md",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "STATUS_JSON=$(\"$_FEATUREFORGE_BIN\" plan execution status --plan \"$APPROVED_PLAN_PATH\")",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "TASK_PACKET_CONTEXT_TASK_1=",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "treat `execution_started` as an executor-resume signal only when workflow/operator reports `phase` `executing`",
    );
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "If workflow/operator reports a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv` instead of resuming `featureforge:subagent-driven-development` or `featureforge:executing-plans` just because `execution_started` is `yes`.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "# Code Review Briefing Template",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "This file is the skill-local reviewer briefing template, not the generated agent system prompt.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Approved plan path:** {APPROVED_PLAN_PATH}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Execution evidence path:** {EXECUTION_EVIDENCE_PATH}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Working-tree diff hash:** {WORKING_TREE_DIFF_HASH}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Installed runtime path (live routing):** {INSTALLED_RUNTIME_PATH}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Installed runtime hash (live routing):** {INSTALLED_RUNTIME_HASH}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Workspace runtime hash (tests/fixtures):** {WORKSPACE_RUNTIME_HASH}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Live state dir:** {LIVE_STATE_DIR}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Active FeatureForge skill source:** {ACTIVE_FEATUREFORGE_SKILL_SOURCE}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Active FeatureForge skill roots:** {ACTIVE_FEATUREFORGE_SKILL_ROOTS}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Installed skill root:** {INSTALLED_SKILL_ROOT}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Workspace skill root:** {WORKSPACE_SKILL_ROOT}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "**Workspace-runtime live-mutation confirmation:** {WORKSPACE_RUNTIME_LIVE_MUTATION_CONFIRMATION}",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Fail review if any live workflow mutation used workspace runtime without an explicit approved override record.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Fail review if active FeatureForge skills resolve from the workspace skill root instead of the installed skill root without an explicit approved self-hosting exception.",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "Use caller-provided base-branch context and release-lineage routing.",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "Require caller-provided base branch, base SHA, head SHA, plan path if plan-routed, and any runtime context the caller wants considered",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "Do not derive, repair, or reconstruct missing workflow context locally; stop as blocked if the required review range or plan-routed context was not provided",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "When runtime-owned execution evidence, completed task-packet context, or coverage-matrix excerpts are included in the handoff, read them too and use them as supplemental plan-routed review context",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "Treat provided-but-stale or unreadable execution evidence as a blocking issue for plan-routed final review, but do not require the public flow to harvest supplemental evidence or task-packet context manually when the handoff omitted it",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.md"),
        "Require caller-provided base branch, base SHA, head SHA, plan path if plan-routed, and any runtime context the caller wants considered",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.md"),
        "Do not derive, repair, or reconstruct missing workflow context locally; stop as blocked if the required review range or plan-routed context was not provided",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.md"),
        "When runtime-owned execution evidence, completed task-packet context, or coverage-matrix excerpts are included in the handoff, read them too and use them as supplemental plan-routed review context",
    );
    assert_file_contains(
        root.join("agents/code-reviewer.md"),
        "Treat provided-but-stale or unreadable execution evidence as a blocking issue for plan-routed final review, but do not require the public flow to harvest supplemental evidence or task-packet context manually when the handoff omitted it",
    );
    assert_file_not_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "origin/HEAD",
    );
    assert_file_not_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "branch.<current>.gh-merge-base",
    );
    assert_file_not_contains(root.join("agents/code-reviewer.md"), "origin/HEAD");
    assert_file_not_contains(
        root.join("agents/code-reviewer.md"),
        "branch.<current>.gh-merge-base",
    );
    assert_file_not_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "Treat missing or stale execution evidence as a blocking issue for plan-routed final review",
    );
    assert_file_not_contains(
        root.join("agents/code-reviewer.md"),
        "Treat missing or stale execution evidence as a blocking issue for plan-routed final review",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "gh pr view --json baseRefName",
    );

    assert_file_contains(
        root.join("README.md"),
        "brainstorming -> plan-ceo-review -> writing-plans -> plan-eng-review`; `plan-fidelity-review` runs only after engineering-review edits are complete, then `plan-eng-review` performs final approval before implementation.",
    );
    assert_file_contains(
        root.join("README.md"),
        concat!("execution pre", "flight boundary for the approved plan"),
    );
    assert_file_contains(
        root.join("README.md"),
        "The public execution surface is `begin`, `complete`, `reopen`, `transfer`, `close-current-task`, `repair-review-state`, and `advance-late-stage`.",
    );
    assert_file_contains(
        root.join("README.md"),
        "Completion then flows through (runtime-owned late-stage sequencing keeps `featureforge:document-release` ahead of terminal `featureforge:requesting-code-review`):",
    );
    assert_file_contains(
        root.join("README.md"),
        "compatibility/debug command boundaries (`gate-*`, low-level `record-*`) must not be required in the normal path",
    );
    assert_file_not_contains(
        root.join("README.md"),
        &hidden_text(&["plan execution ", "rebuild", "-evidence"]),
    );
    assert_file_not_contains(
        root.join("README.md"),
        &hidden_text(&[
            "`featureforge plan execution ",
            "rebuild",
            "-evidence --plan <approved-plan-path>` replays rebuildable execution-evidence targets from the current approved plan and refreshes helper-owned closure receipts against the current runtime state.",
        ]),
    );
    assert_file_not_contains(
        root.join("README.md"),
        "the broader public execution surface also includes commands such as `note`, `complete`, `reopen`, `transfer`, and compatibility/diagnostic helpers when the route or workflow boundary requires them.",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` is the normal routing surface after handoff; use `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` only for deeper diagnostics",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "for workflow-routed terminal sequencing, run `featureforge:document-release` before terminal `featureforge:requesting-code-review`, then continue to `featureforge:qa-only` (when required) and `featureforge:finishing-a-development-branch`",
    );
    assert_file_contains(
        root.join("docs/README.codex.md"),
        "compatibility/debug command boundaries (low-level `record-*` and related compatibility commands) must not be required in the normal path; normal progression stays on `workflow operator`, `close-current-task`, and `advance-late-stage`",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` is the normal routing surface after handoff; use `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` only for deeper diagnostics",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "for workflow-routed terminal sequencing, run `featureforge:document-release` before terminal `featureforge:requesting-code-review`, then continue to `featureforge:qa-only` (when required) and `featureforge:finishing-a-development-branch`",
    );
    assert_file_contains(
        root.join("docs/README.copilot.md"),
        "compatibility/debug command boundaries (low-level `record-*` and related compatibility commands) must not be required in the normal path; normal progression stays on `workflow operator`, `close-current-task`, and `advance-late-stage`",
    );
    assert_file_not_contains(
        root.join("schemas/workflow-operator.schema.json"),
        "\"record branch closure\"",
    );
    assert_file_contains(
        root.join("schemas/workflow-operator.schema.json"),
        "\"advance late stage\"",
    );
    assert_file_not_contains(
        root.join("schemas/workflow-handoff.schema.json"),
        "\"record branch closure\"",
    );
    assert_file_contains(
        root.join("schemas/workflow-handoff.schema.json"),
        "\"advance late stage\"",
    );
    assert_file_not_contains(
        root.join("schemas/plan-execution-status.schema.json"),
        "\"record branch closure\"",
    );
    assert_file_contains(
        root.join("schemas/plan-execution-status.schema.json"),
        "\"advance late stage\"",
    );
    assert_file_not_contains(
        root.join("README.md"),
        &hidden_text(&["featureforge plan execution ", "gate", "-review-dispatch"]),
    );
    assert_file_not_contains(
        root.join("RELEASE-NOTES.md"),
        &hidden_text(&["featureforge plan execution ", "gate", "-review-dispatch"]),
    );
    assert_file_contains(root.join("RELEASE-NOTES.md"), "windows prebuilt artifacts");
    assert_file_contains(
        root.join("RELEASE-NOTES.md"),
        "same runtime-owned routing decision instead of allowing diagnostic/status drift",
    );
    assert_file_contains(
        root.join("RELEASE-NOTES.md"),
        "projection-only regeneration that fails closed with append-only/manual-repair blockers instead of rewriting authoritative proof in place",
    );
    assert_file_contains(
        root.join("RELEASE-NOTES.md"),
        "`plan execution status --json`: align the diagnostic surface with the runtime-owned route by exposing the same `harness_phase`, `next_action`, `recommended_public_command_argv`, display-only `recommended_command`, and late-stage `recording_context` fields while keeping status diagnostic-only",
    );
    assert_file_contains(
        root.join("review/late-stage-precedence-reference.md"),
        "Legacy finish-gate compatibility commands are compatibility/debug boundaries, not normal-path commands.",
    );
    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "- `needs-user-input`",
    );
    assert_file_not_contains(
        root.join("skills/qa-only/SKILL.md"),
        "**Generated By:** featureforge:qa-only",
    );
    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "also write a project-scoped release-readiness companion artifact",
    );
    assert_file_not_contains(
        root.join("skills/document-release/SKILL.md"),
        "before writing the release-readiness companion artifact",
    );
    assert_file_not_contains(
        root.join("skills/qa-only/SKILL.md"),
        "also write a project-scoped outcome artifact",
    );
    assert_file_not_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "`needs-user-input`",
    );
    assert_file_not_contains(
        root.join("agents/code-reviewer.instructions.md"),
        "`needs-user-input`",
    );
    assert_file_not_contains(root.join("agents/code-reviewer.md"), "`needs-user-input`");
    assert_file_contains(
        root.join("review/late-stage-precedence-reference.md"),
        "low-level `record-*` commands are compatibility/debug boundaries and must not be required by normal-path guidance.",
    );
    assert_file_contains(
        root.join("review/late-stage-precedence-reference.md"),
        "For workflow-routed terminal sequencing, run `document-release` before terminal `requesting-code-review`.",
    );
    let readme = read_utf8(root.join("README.md"));
    let completion_start = readme
        .find("Completion then flows through")
        .expect("README completion flow heading should be present");
    let completion_end = readme[completion_start..]
        .find("## Project Memory")
        .map(|offset| completion_start + offset)
        .expect("README completion flow should end before project memory section");
    let completion_block = &readme[completion_start..completion_end];
    let release_index = completion_block
        .find("featureforge:document-release")
        .expect("completion flow should mention document-release");
    let review_index = completion_block
        .find("featureforge:requesting-code-review")
        .expect("completion flow should mention requesting-code-review");
    assert!(
        release_index < review_index,
        "README completion flow should list document-release before requesting-code-review"
    );
    assert_file_contains(
        root.join("docs/test-suite-enhancement-plan.md"),
        "The active deterministic suite and recommended commands now live in `docs/testing.md`.",
    );
    assert_file_contains(
        root.join("docs/test-suite-enhancement-plan.md"),
        "cargo nextest run --all-targets --all-features --no-fail-fast",
    );
    assert_file_contains(
        root.join("docs/test-suite-enhancement-plan.md"),
        "Targeted `cargo nextest run --test ...` commands are iteration aids only, not branch-proof verification.",
    );
    assert_file_not_contains(
        root.join("docs/test-suite-enhancement-plan.md"),
        "bash tests/codex-runtime/test-runtime-instructions.sh",
    );
    assert_file_not_contains(
        root.join("docs/test-suite-enhancement-plan.md"),
        "bash tests/codex-runtime/test-workflow-sequencing.sh",
    );

    let fixture_root = root.join("tests/codex-runtime/fixtures/workflow-artifacts");
    for spec in [
        "specs/2026-01-22-document-review-system-design.md",
        "specs/2026-01-22-document-review-system-design-v2.md",
        "specs/2026-02-19-visual-brainstorming-refactor-design.md",
        "specs/2026-03-11-zero-dep-brainstorm-server-design.md",
    ] {
        assert_file_contains(fixture_root.join(spec), "**Workflow State:** CEO Approved");
        assert_file_contains(fixture_root.join(spec), "**Spec Revision:** 1");
        assert_file_contains(
            fixture_root.join(spec),
            "**Last Reviewed By:** plan-ceo-review",
        );
    }
    for plan in [
        "plans/2026-01-22-document-review-system.md",
        "plans/2026-02-19-visual-brainstorming-refactor.md",
        "plans/2026-03-11-zero-dep-brainstorm-server.md",
    ] {
        assert_file_contains(
            fixture_root.join(plan),
            "**Workflow State:** Engineering Approved",
        );
        assert_file_contains(fixture_root.join(plan), "**Source Spec:**");
        assert_file_contains(fixture_root.join(plan), "**Source Spec Revision:** 1");
        assert_file_contains(
            fixture_root.join(plan),
            "**Last Reviewed By:** plan-eng-review",
        );
    }
    assert_file_contains(
        fixture_root.join("README.md"),
        "Requirement Index and Requirement Coverage Matrix structure",
    );
    assert_file_contains(
        fixture_root.join("README.md"),
        "canonical `## Task N:` plus parseable `**Files:**` blocks",
    );
}

#[test]
fn late_stage_precedence_reference_rows_match_runtime_rows_and_operator_phase_mappings() {
    let root = repo_root();
    let runtime_precedence = read_utf8(root.join("src/workflow/late_stage_precedence.rs"));
    let operator = read_utf8(root.join("src/workflow/operator.rs"));
    let reference = read_utf8(root.join("review/late-stage-precedence-reference.md"));

    let runtime_rows = parse_runtime_late_stage_rows(&runtime_precedence);
    let reference_rows = parse_reference_late_stage_rows(&reference);
    assert_eq!(
        runtime_rows.len(),
        8,
        "runtime PRECEDENCE_ROWS should define exactly eight late-stage rows"
    );
    assert_eq!(
        reference_rows.len(),
        runtime_rows.len(),
        "late-stage reference table should mirror runtime row count"
    );

    let normalized_operator = operator
        .chars()
        .filter(|char| !char.is_whitespace())
        .collect::<String>();

    for (runtime_row, reference_row) in runtime_rows.iter().zip(reference_rows.iter()) {
        assert_eq!(
            reference_row.release, runtime_row.release,
            "late-stage reference release gate should match runtime row: {runtime_row:?}"
        );
        assert_eq!(
            reference_row.review, runtime_row.review,
            "late-stage reference review gate should match runtime row: {runtime_row:?}"
        );
        assert_eq!(
            reference_row.qa, runtime_row.qa,
            "late-stage reference QA gate should match runtime row: {runtime_row:?}"
        );
        assert_eq!(
            reference_row.phase, runtime_row.phase,
            "late-stage reference phase should match runtime row: {runtime_row:?}"
        );
        assert_eq!(
            reference_row.reason_family, runtime_row.reason_family,
            "late-stage reference reason family should match runtime row: {runtime_row:?}"
        );

        let (expected_action, expected_skill) =
            expected_phase_action_and_skill(reference_row.phase.as_str());
        assert_eq!(
            reference_row.next_action, expected_action,
            "late-stage reference next action should match runtime phase mapping for {}",
            reference_row.phase
        );
        for internal_action_token in [
            "advance_late_stage",
            "dispatch_final_review",
            "run_qa",
            "run_finish_review_gate",
            "run_finish_completion_gate",
        ] {
            assert!(
                !reference_row.next_action.contains(internal_action_token),
                "late-stage reference next action should use public wording instead of internal token {:?} for {}",
                internal_action_token,
                reference_row.phase
            );
        }
        assert_eq!(
            reference_row.recommended_skill, expected_skill,
            "late-stage reference recommended skill should match runtime phase mapping for {}",
            reference_row.phase
        );

        assert!(
            normalized_operator
                .contains("fnnext_action_for_context(context:&OperatorContext)->&str{&context.operator_next_action}"),
            "workflow/operator should surface query-derived next_action directly",
        );
        let expected_phase_token = expected_phase_source_token(reference_row.phase.as_str());
        let direct_mapping = format!("{expected_phase_token}=>(String::from(\"{expected_skill}\")");
        let block_mapping =
            format!("{expected_phase_token}=>{{(String::from(\"{expected_skill}\")");
        assert!(
            normalized_operator.contains(&direct_mapping)
                || normalized_operator.contains(&block_mapping),
            "operator recommended-skill mapping should include {} -> {}",
            reference_row.phase,
            expected_skill
        );
    }
}

#[test]
fn removed_session_entry_gate_contracts_stay_absent_from_active_runtime_and_eval_surfaces() {
    let root = repo_root();

    for relative in [
        "skills/using-featureforge/SKILL.md",
        "skills/dispatching-parallel-agents/SKILL.md",
        "skills/subagent-driven-development/SKILL.md",
        "tests/evals/README.md",
        "tests/evals/using-featureforge-routing.orchestrator.md",
        "tests/evals/using-featureforge-routing.runner.md",
        "tests/evals/using-featureforge-routing.judge.md",
        "tests/evals/using-featureforge-routing.scenarios.md",
        "tests/workflow_entry_shell_smoke.rs",
        "tests/workflow_shell_smoke.rs",
        "src/lib.rs",
    ] {
        let path = root.join(relative);
        assert_file_not_contains(path.clone(), "FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY");
        assert_file_not_contains(path.clone(), "FEATUREFORGE_SPAWNED_SUBAGENT");
        assert_file_not_contains(path.clone(), "FEATUREFORGE_SPAWNED_SUBAGENT_OPT_IN");
        assert_file_not_contains(path.clone(), "featureforge session-entry resolve");
        assert_file_not_contains(path, "featureforge-session-entry");
    }

    assert_file_not_contains(
        root.join("tests/evals/README.md"),
        "starts after the first-turn bypass decision has already been resolved to `enabled`",
    );
    assert_file_not_contains(
        root.join("tests/evals/using-featureforge-routing.runner.md"),
        "pre-seed the synthetic session decision to `enabled`",
    );
    assert_file_not_contains(
        root.join("tests/evals/using-featureforge-routing.scenarios.md"),
        "pre-seeds the synthetic session decision to `enabled`",
    );
    assert_file_not_contains(
        root.join("tests/evals/using-featureforge-routing.orchestrator.md"),
        "Pre-seed the runner's real session decision path to `enabled`",
    );
    assert_file_not_contains(
        root.join("tests/evals/using-featureforge-routing.orchestrator.md"),
        "the runner-derived session decision path used for the pre-seeded `enabled` state",
    );
    assert_file_not_contains(
        root.join("tests/evals/using-featureforge-routing.judge.md"),
        "whether the fixture pre-seeded the synthetic session decision to `enabled`",
    );
    assert_file_not_contains(
        root.join("tests/evals/using-featureforge-routing.judge.md"),
        "whether that pre-seeded state used the runner-derived decision path rather than a guessed `$PPID`",
    );
    assert!(
        !root.join("src/cli/session_entry.rs").exists(),
        "src/cli/session_entry.rs should stay absent from the active source tree"
    );
}

#[test]
fn using_featureforge_preamble_uses_only_the_packaged_runtime_binary() {
    let content = read_utf8(repo_root().join("skills/using-featureforge/SKILL.md"));
    let preamble = extract_bash_block(&content, "## Preamble (run first)");
    let tmp_root = TempDir::new().expect("temp root should exist");

    assert_no_runtime_fallback_execution(&preamble, "using-featureforge preamble");

    let shared_home = tmp_root.path().join("shared-home");
    fs::create_dir_all(&shared_home).expect("shared home should exist");
    let packaged_runtime = tmp_root.path().join("packaged-runtime");
    fs::create_dir_all(&packaged_runtime).expect("packaged runtime should exist");
    make_runtime_repo(&packaged_runtime);
    let packaged_bin = canonical_install_bin(&shared_home);
    fs::create_dir_all(
        packaged_bin
            .parent()
            .expect("packaged install binary should have a parent"),
    )
    .expect("packaged install parent should exist");
    let expected_runtime_root =
        fs::canonicalize(&packaged_runtime).expect("packaged runtime should canonicalize");
    fs::write(
        &packaged_bin,
        format!(
            "#!/usr/bin/env bash\nif [ \"${{1:-}}\" = \"repo\" ] && [ \"${{2:-}}\" = \"runtime-root\" ] && [ \"${{3:-}}\" = \"--path\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0\n",
            expected_runtime_root.display()
        ),
    )
    .expect("packaged runtime binary should be writable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&packaged_bin, fs::Permissions::from_mode(0o755))
            .expect("packaged runtime binary should stay executable");
    }

    let repo_candidate = tmp_root.path().join("repo-candidate");
    fs::create_dir_all(&repo_candidate).expect("repo candidate should exist");
    make_runtime_repo(&repo_candidate);

    let mut packaged_command = Command::new("bash");
    packaged_command
        .arg("-lc")
        .arg(format!(
            "{preamble}\nprintf \"FEATUREFORGE_ROOT=%s\\n\" \"$_FEATUREFORGE_ROOT\"\n"
        ))
        .current_dir(&repo_candidate)
        .env("HOME", &shared_home);
    let packaged = run_checked(packaged_command, "run packaged using-featureforge preamble");
    let packaged_stdout =
        String::from_utf8(packaged.stdout).expect("preamble output should be utf8");
    assert_contains(
        &packaged_stdout,
        &format!("FEATUREFORGE_ROOT={}", expected_runtime_root.display()),
        "using-featureforge packaged output",
    );

    let non_runtime_repo = tmp_root.path().join("non-runtime-repo");
    fs::create_dir_all(&non_runtime_repo).expect("non-runtime repo should exist");
    git_support::init_repo_with_initial_commit(&non_runtime_repo, "# non-runtime repo\n", "init");
    let missing_packaged_home = tmp_root.path().join("missing-packaged-home");
    fs::create_dir_all(&missing_packaged_home).expect("missing packaged home should exist");

    let mut no_fallback_command = Command::new("bash");
    no_fallback_command
        .arg("-lc")
        .arg(format!(
            "{preamble}\nprintf \"FEATUREFORGE_ROOT=%s\\n\" \"$_FEATUREFORGE_ROOT\"\n"
        ))
        .current_dir(&non_runtime_repo)
        .env("HOME", &missing_packaged_home);
    let no_fallback = run_checked(
        no_fallback_command,
        "run using-featureforge preamble without packaged binary",
    );
    let no_fallback_stdout =
        String::from_utf8(no_fallback.stdout).expect("no-fallback output should be utf8");
    assert_contains(
        &no_fallback_stdout,
        "FEATUREFORGE_ROOT=",
        "using-featureforge no-fallback output",
    );
    assert_not_contains(
        &no_fallback_stdout,
        &expected_runtime_root.display().to_string(),
        "using-featureforge no-fallback output",
    );
    assert_not_contains(
        &no_fallback_stdout,
        &non_runtime_repo.display().to_string(),
        "using-featureforge no-fallback output",
    );
}

#[test]
fn generated_skill_preamble_never_executes_repo_or_root_selected_launchers() {
    let content = read_utf8(repo_root().join("skills/brainstorming/SKILL.md"));
    let preamble = extract_bash_block(&content, "## Preamble (run first)");
    let tmp_root = TempDir::new().expect("temp root should exist");
    let home_dir = tmp_root.path().join("home");
    let state_dir = tmp_root.path().join("state");
    let repo_candidate = tmp_root.path().join("repo-candidate");
    let resolved_runtime_root = tmp_root.path().join("resolved-runtime-root");
    let packaged_log = tmp_root.path().join("packaged.log");

    fs::create_dir_all(&home_dir).expect("home dir should exist");
    fs::create_dir_all(&state_dir).expect("state dir should exist");
    fs::create_dir_all(&repo_candidate).expect("repo candidate should exist");
    fs::create_dir_all(&resolved_runtime_root).expect("resolved runtime root should exist");

    git_support::init_repo_with_initial_commit(&repo_candidate, "# repo candidate\n", "init");

    write_logging_packaged_runtime(
        &canonical_install_bin(&home_dir),
        &resolved_runtime_root,
        &packaged_log,
    );
    write_poison_runtime_launcher(&repo_candidate, "POISON_REPO");
    write_poison_runtime_launcher(&resolved_runtime_root, "POISON_ROOT");

    let mut command = Command::new("bash");
    command
        .arg("-lc")
        .arg(preamble)
        .current_dir(&repo_candidate)
        .env("HOME", &home_dir)
        .env("FEATUREFORGE_STATE_DIR", &state_dir)
        .env("FEATUREFORGE_TEST_LOG", &packaged_log);
    let output = run_checked(
        command,
        "run generated skill preamble with poisoned fallback launchers",
    );
    let stdout = String::from_utf8(output.stdout).expect("preamble stdout should be utf8");
    let log = read_utf8(&packaged_log);

    // Intentional invariant: skill installs package the runtime binary on
    // purpose. Repo-local binaries and binaries discovered from the resolved
    // runtime root are companion-file locations only. They must NEVER become
    // command execution fallbacks unless product direction changes explicitly.
    assert_eq!(
        stdout.trim_end(),
        "",
        "generated skill preamble should stay quiet"
    );
    assert_contains(
        &log,
        "PACKAGED:repo-runtime-root",
        "packaged runtime command log",
    );
    assert_not_contains(
        &log,
        "PACKAGED:update-check",
        "packaged runtime command log",
    );
    assert_not_contains(&log, "PACKAGED:config-get", "packaged runtime command log");
    assert_not_contains(&log, "POISON_REPO", "packaged runtime command log");
    assert_not_contains(&log, "POISON_ROOT", "packaged runtime command log");
}

#[test]
fn workflow_execution_skill_docs_enforce_installed_control_plane_routing() {
    assert_generated_skills_use_installed_runtime_for_live_routes();
}

#[test]
fn generated_skills_use_installed_runtime_for_live_routes() {
    assert_generated_skills_use_installed_runtime_for_live_routes();
}

fn assert_generated_skills_use_installed_runtime_for_live_routes() {
    let root = repo_root();
    for relative in [
        "skills/using-featureforge/SKILL.md",
        "skills/executing-plans/SKILL.md",
        "skills/subagent-driven-development/SKILL.md",
        "skills/requesting-code-review/SKILL.md",
        "skills/finishing-a-development-branch/SKILL.md",
    ] {
        let path = root.join(relative);
        let label = path.display().to_string();
        let content = read_utf8(&path);
        assert_contains(&content, "## Installed Control Plane", &label);
        assert_contains(
            &content,
            "use only `$_FEATUREFORGE_BIN` for live workflow control-plane commands",
            &label,
        );
        assert_contains(
            &content,
            "do not route live workflow commands through `./bin/featureforge`",
            &label,
        );
        assert_contains(
            &content,
            "do not route live workflow commands through `target/debug/featureforge`",
            &label,
        );
        assert_contains(
            &content,
            "do not route live workflow commands through `cargo run`",
            &label,
        );
        assert_contains(
            &content,
            "If `recommended_public_command_argv[0] == \"featureforge\"`, execute through the installed runtime by replacing argv[0] with `$_FEATUREFORGE_BIN`",
            &label,
        );
        for forbidden in [
            "featureforge workflow",
            "featureforge plan execution",
            "featureforge plan contract",
            "featureforge repo-safety",
            "`workflow status",
            "`workflow operator --",
        ] {
            assert_not_contains(&content, forbidden, &label);
        }
    }
}

#[test]
fn recommended_public_command_argv_is_rebound_to_installed_binary() {
    let root = repo_root();
    for relative in [
        "README.md",
        "docs/README.codex.md",
        "docs/README.copilot.md",
        "docs/runtime-architecture.md",
        "docs/featureforge/reference/2026-04-01-review-state-reference.md",
    ] {
        let path = root.join(relative);
        let content = read_utf8(&path);
        assert_contains(&content, "featureforge", &path.display().to_string());
        assert_contains(
            &content,
            "~/.featureforge/install/bin/featureforge",
            &path.display().to_string(),
        );
    }
    for relative in [
        "skills/using-featureforge/SKILL.md",
        "skills/executing-plans/SKILL.md",
        "skills/subagent-driven-development/SKILL.md",
        "skills/requesting-code-review/SKILL.md",
        "skills/finishing-a-development-branch/SKILL.md",
    ] {
        let path = root.join(relative);
        assert_file_contains(
            &path,
            "If `recommended_public_command_argv[0] == \"featureforge\"`, execute through the installed runtime by replacing argv[0] with `$_FEATUREFORGE_BIN`",
        );
    }
    assert_file_contains(
        root.join("skills/using-featureforge/SKILL.md"),
        "If `recommended_public_command_argv[0] == \"featureforge\"`, execute through the installed runtime by replacing argv[0] with `$_FEATUREFORGE_BIN`",
    );
    assert_file_contains(
        root.join("docs/runtime-architecture.md"),
        "if argv[0] is `featureforge`, execute `~/.featureforge/install/bin/featureforge` with argv[1..] unchanged",
    );
}

#[test]
fn operator_docs_document_installed_control_plane_isolation() {
    let root = repo_root();

    for relative in [
        "README.md",
        "docs/README.codex.md",
        "docs/README.copilot.md",
        "docs/runtime-architecture.md",
    ] {
        let path = root.join(relative);
        assert_file_contains(&path, "Installed Control Plane");
        assert_file_contains(&path, "~/.featureforge/install/bin/featureforge");
        assert_file_contains(&path, "~/.featureforge/install/skills");
        assert_file_contains(&path, "workspace");
        assert_file_contains(
            &path,
            "FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1",
        );
        assert_file_contains(&path, "featureforge doctor self-hosting --json");
    }

    assert_file_contains(
        root.join("README.md"),
        "Workspace-local runtimes must not mutate live workflow state",
    );
    assert_file_contains(
        root.join("README.md"),
        "`FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1` is intentionally explicit",
    );
    assert_file_contains(
        root.join("docs/testing.md"),
        "Workspace binaries must not run live workflow mutations",
    );
    assert_file_contains(
        root.join("docs/testing.md"),
        "`FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1` is explicitly set",
    );
    assert_file_contains(
        root.join("docs/featureforge/reference/2026-04-01-review-state-reference.md"),
        "installed-control-plane rebinding (`featureforge` argv[0] executes as `~/.featureforge/install/bin/featureforge`)",
    );
    assert_file_contains(
        root.join("docs/featureforge/reference/2026-04-01-review-state-reference.md"),
        "as `./bin/featureforge`, `target/debug/featureforge`, or `cargo run -- ...`",
    );
}

#[test]
fn installed_control_plane_verification_gate_includes_required_commands() {
    let root = repo_root();
    let script = root.join("scripts/verify-installed-control-plane-isolation.sh");
    let script_content = read_utf8(&script);
    let docs_testing = read_utf8(root.join("docs/testing.md"));

    assert!(script.is_file(), "{} should exist", script.display());
    for required in [
        "cargo fmt --check",
        "cargo test --test runtime_module_boundaries -- --nocapture",
        "cargo test --test runtime_instruction_contracts -- --nocapture",
        "cargo test --test workflow_runtime -- --nocapture",
        "cargo test --test workflow_shell_smoke -- --nocapture",
        "cargo test --test workflow_entry_shell_smoke -- --nocapture",
        "node scripts/gen-skill-docs.mjs --check",
        "node --test tests/codex-runtime/skill-doc-contracts.test.mjs",
        "node scripts/lint-workspace-runtime-evidence.mjs",
        "cargo clippy --all-targets --all-features -- -D warnings",
        "cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail",
    ] {
        assert_contains(
            &script_content,
            required,
            "installed control-plane gate script",
        );
        assert_contains(&docs_testing, required, "docs/testing.md");
    }

    for targeted_test in [
        "runtime_provenance_classifies_installed_runtime",
        "runtime_provenance_classifies_workspace_runtime",
        "workspace_runtime_blocks_live_repair_review_state",
        "workspace_runtime_blocks_live_close_current_task",
        "workspace_runtime_allows_fixture_repair_review_state_with_temp_state",
        "generated_skills_use_installed_runtime_for_live_routes",
        "recommended_public_command_argv_is_rebound_to_installed_binary",
        "evidence_lint_rejects_workspace_runtime_live_mutation",
        "self_hosting_diagnostic_reports_installed_and_workspace_hashes",
    ] {
        assert!(
            source_tree_declares_test(&root, targeted_test),
            "installed control-plane targeted test should exist: {targeted_test}"
        );
    }

    assert_contains(
        &docs_testing,
        "evidence linting in the normal verification set",
        "docs/testing.md",
    );
    assert_file_contains(
        root.join("scripts/verify-source-archive.mjs"),
        "scripts/verify-installed-control-plane-isolation.sh",
    );
}

#[test]
fn setup_docs_verify_discovery_links_point_to_install() {
    let root = repo_root();
    for relative in [
        ".codex/INSTALL.md",
        ".copilot/INSTALL.md",
        "docs/README.codex.md",
        "docs/README.copilot.md",
    ] {
        let path = root.join(relative);
        assert_file_contains(&path, "readlink");
        assert_file_contains(&path, "~/.featureforge/install/skills");
        assert_file_contains(&path, "featureforge doctor self-hosting --json");
        assert_file_contains(&path, "not to a workspace-local `<repo>/skills` directory");
    }
}

#[test]
fn execution_skill_docs_keep_candidate_artifacts_and_authoritative_mutations_separated() {
    let executing_plans = read_utf8(repo_root().join("skills/executing-plans/SKILL.md"));
    let subagent_skill = read_utf8(repo_root().join("skills/subagent-driven-development/SKILL.md"));
    let implementer_prompt =
        read_utf8(repo_root().join("skills/subagent-driven-development/implementer-prompt.md"));
    let review_skill = read_utf8(repo_root().join("skills/requesting-code-review/SKILL.md"));
    let qa_skill = read_utf8(repo_root().join("skills/qa-only/SKILL.md"));

    for (content, label) in [
        (&executing_plans, "skills/executing-plans/SKILL.md"),
        (
            &subagent_skill,
            "skills/subagent-driven-development/SKILL.md",
        ),
        (
            &implementer_prompt,
            "skills/subagent-driven-development/implementer-prompt.md",
        ),
    ] {
        for forbidden_direct_command in [
            "record-contract",
            "record-evaluation",
            "record-handoff",
            "begin",
            "note",
            "complete",
            "reopen",
            "transfer",
        ] {
            assert_forbids_direct_helper_command_mutation(content, forbidden_direct_command, label);
        }
    }

    assert_separates_candidate_artifacts_from_authoritative_mutations(
        &executing_plans,
        "skills/executing-plans/SKILL.md",
    );
    assert_separates_candidate_artifacts_from_authoritative_mutations(
        &subagent_skill,
        "skills/subagent-driven-development/SKILL.md",
    );
    assert_separates_candidate_artifacts_from_authoritative_mutations(
        &implementer_prompt,
        "skills/subagent-driven-development/implementer-prompt.md",
    );
    assert_downstream_material_stays_gate_and_harness_aware(
        &review_skill,
        "skills/requesting-code-review/SKILL.md",
    );
    assert_downstream_material_stays_gate_and_harness_aware(&qa_skill, "skills/qa-only/SKILL.md");
}

#[test]
fn active_prompt_docs_do_not_teach_hidden_helper_command_names() {
    let forbidden_terms = [
        hidden_text(&["record", "-review-dispatch"]),
        hidden_text(&["gate", "-review"]),
        hidden_text(&["gate", "-finish"]),
        hidden_text(&["rebuild", "-evidence"]),
        hidden_text(&["record", "-branch-closure"]),
        hidden_text(&["record", "-release-readiness"]),
        hidden_text(&["record", "-final-review"]),
        hidden_text(&["record", "-qa"]),
        hidden_text(&["workflow plan", "-fidelity record"]),
        hidden_text(&["plan", "_fidelity_receipt"]),
        hidden_text(&["plan", "-fidelity-receipt"]),
        hidden_text(&["Plan", "-fidelity receipt"]),
    ];
    let mut violations = Vec::new();
    let root = repo_root();

    for path in active_prompt_doc_paths() {
        let content = read_utf8(&path);
        for forbidden_term in &forbidden_terms {
            if content.contains(forbidden_term) {
                let rel = path
                    .strip_prefix(&root)
                    .expect("active prompt path should be repo-relative");
                violations.push(format!("{}: {forbidden_term}", rel.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "active prompt/docs should not teach hidden helper command names:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_remediation_inventory_is_visible_to_instruction_contract_tests() {
    let inventory = read_utf8(repo_root().join("tests/fixtures/runtime-remediation/README.md"));
    assert_contains(
        &inventory,
        "## Detailed Failure Shapes (Mandatory)",
        "tests/fixtures/runtime-remediation/README.md",
    );
    for scenario in [
        "FS-01", "FS-02", "FS-03", "FS-04", "FS-05", "FS-06", "FS-07", "FS-08", "FS-09", "FS-10",
        "FS-11", "FS-12", "FS-13", "FS-14", "FS-15", "FS-16",
    ] {
        assert_contains(
            &inventory,
            scenario,
            "tests/fixtures/runtime-remediation/README.md",
        );
    }
    for detail_anchor in [
        "branch-closure mutation says repair is required",
        "helper-backed tests pass but compiled CLI behavior differs",
        "status points to the right blocker, operator still recommends execution reentry / begin",
        "rebased consumer-style fixture with forward reentry overlay pointing at Task 3",
        "authoritative state contains `run_identity.execution_run_id`",
        "completed task with no current task closure baseline",
        "remove or stale receipt projections without changing the reviewed state that closure binds to",
    ] {
        assert_contains(
            &inventory,
            detail_anchor,
            "tests/fixtures/runtime-remediation/README.md",
        );
    }
    for anchor in [
        "tests/workflow_shell_smoke.rs::runtime_remediation_fs01_compiled_cli_repair_and_branch_closure_do_not_disagree",
        "tests/plan_execution.rs::record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation",
        "tests/plan_execution.rs::record_final_review_rejects_unapproved_reviewer_source_before_mutation",
        "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift",
        "tests/workflow_runtime.rs::runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine",
        "tests/workflow_shell_smoke.rs::fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers",
        concat!(
            "tests/plan_execution.rs::runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_pre",
            "flight"
        ),
        "tests/plan_execution.rs::runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state",
        "tests/workflow_runtime.rs::runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task",
        "tests/plan_execution.rs::runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch",
        "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task",
        "tests/plan_execution.rs::runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts",
        "task_close_internal_dispatch_runtime_management_budget_is_capped",
    ] {
        assert_contains(
            &inventory,
            anchor,
            "tests/fixtures/runtime-remediation/README.md",
        );
    }
}
