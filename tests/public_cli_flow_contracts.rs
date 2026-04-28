use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_repo_file(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"))
}

#[test]
fn internal_plan_execution_helpers_are_explicitly_quarantined() {
    let source = read_repo_file("tests/support/plan_execution_direct.rs");

    assert!(
        source.starts_with(
            "//! Internal reducer/unit-test helpers only.\n//! These helpers may exercise hidden or removed runtime machinery.\n//! They must not be used by tests that claim public CLI, operator, budget, liveness, or session-replay behavior."
        ),
        "plan_execution_direct.rs must start with the internal-only quarantine contract"
    );
    assert!(
        !source.contains("pub fn internal_test_"),
        "plan_execution_direct.rs must not expose ambiguous internal_test_* helpers"
    );
    assert!(
        source.contains("pub fn internal_only_"),
        "plan_execution_direct.rs should keep internal helpers visibly prefixed"
    );
}

#[test]
fn public_cli_json_helper_uses_the_compiled_binary_only() {
    let source = read_repo_file("tests/support/featureforge.rs");
    let helper_start = source
        .find("pub fn run_public_featureforge_cli_json")
        .expect("public CLI JSON helper should exist");
    let helper_body = &source[helper_start
        ..source[helper_start..]
            .find("\n}\n\n")
            .map(|offset| helper_start + offset + 3)
            .expect("public CLI JSON helper body should be bounded")];

    assert!(
        helper_body.contains("run_rust_featureforge_with_env_control_real_cli"),
        "public CLI helper must invoke the compiled featureforge binary"
    );
    assert!(
        !helper_body.contains("try_direct_featureforge_output")
            && !helper_body.contains("try_run_plan_execution_output_direct"),
        "public CLI helper must not fall back to in-process direct helpers"
    );
}

#[test]
fn public_flow_named_tests_do_not_invoke_hidden_command_literals() {
    let public_name_tokens = [
        "real_cli",
        "public",
        "happy_path",
        "budget",
        "replay",
        "parity",
    ];
    let internal_name_tokens = [
        "internal_only",
        "reject",
        "not_public",
        "removed",
        "hidden",
        "compatibility",
        "shim",
        "direct",
        "helper",
        "fixture",
        "help",
    ];
    let denied_literals = [
        "preflight",
        "gate-review",
        "gate-finish",
        "record-review-dispatch",
        "record-branch-closure",
        "record-release-readiness",
        "record-final-review",
        "record-qa",
        "rebuild-evidence",
        "explain-review-state",
        "reconcile-review-state",
        "--dispatch-id",
        "--branch-closure-id",
        "FEATUREFORGE_ALLOW_INTERNAL_EXECUTION_FLAGS",
    ];
    let files = [
        "tests/workflow_shell_smoke.rs",
        "tests/workflow_runtime.rs",
        "tests/plan_execution.rs",
        "tests/liveness_model_checker.rs",
        "tests/public_replay_churn.rs",
    ];

    let mut violations = Vec::new();
    for rel in files {
        let source = read_repo_file(rel);
        let mut pending_test = false;
        let mut current_test: Option<String> = None;
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed == "#[test]" {
                pending_test = true;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("fn ") {
                let name = rest
                    .split_once('(')
                    .map(|(name, _)| name)
                    .unwrap_or(rest)
                    .to_owned();
                current_test = pending_test.then_some(name);
                pending_test = false;
            }
            let Some(test_name) = &current_test else {
                continue;
            };
            if !public_name_tokens
                .iter()
                .any(|token| test_name.contains(token))
                || internal_name_tokens
                    .iter()
                    .any(|token| test_name.contains(token))
            {
                continue;
            }
            for literal in denied_literals {
                if trimmed == format!("\"{literal}\",")
                    || trimmed == format!("\"{literal}\"")
                    || trimmed.starts_with(&format!("&[\"{literal}\""))
                    || trimmed.starts_with(&format!("[\"{literal}\""))
                {
                    violations.push(format!(
                        "{rel}:{}:{test_name} invokes hidden command or flag literal `{literal}`",
                        index + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "public-flow named tests must use compiled public commands only:\n{}",
        violations.join("\n")
    );
}

#[test]
fn ambiguous_internal_test_prefix_does_not_reenter_rust_tests() {
    let mut offenders = Vec::new();
    for entry in
        fs::read_dir(repo_root().join("tests")).expect("tests directory should be readable")
    {
        let entry = entry.expect("tests directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            for nested in fs::read_dir(&path).expect("nested tests directory should be readable") {
                let nested = nested.expect("nested tests directory entry should be readable");
                collect_internal_test_prefix_offenders(&nested.path(), &mut offenders);
            }
        } else {
            collect_internal_test_prefix_offenders(&path, &mut offenders);
        }
    }

    assert!(
        offenders.is_empty(),
        "ambiguous internal_test_* helpers must stay renamed to internal_only_*:\n{}",
        offenders.join("\n")
    );
}

fn collect_internal_test_prefix_offenders(path: &Path, offenders: &mut Vec<String>) {
    if path.file_name().and_then(|name| name.to_str()) == Some("public_cli_flow_contracts.rs") {
        return;
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return;
    }
    let source = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    if source.contains("internal_test_") {
        offenders.push(path.display().to_string());
    }
}
