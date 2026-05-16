#[path = "support/plan_fidelity.rs"]
mod plan_fidelity_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/repo_template.rs"]
mod repo_template_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const ROUTE_SMOKE_PLAN_REL: &str =
    "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

fn featureforge_help_binaries() -> Vec<PathBuf> {
    let mut binaries = vec![PathBuf::from(env!("CARGO_BIN_EXE_featureforge"))];
    #[cfg(target_os = "macos")]
    {
        let repo_binary = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin/featureforge");
        assert!(
            repo_binary.is_file(),
            "expected repo-root featureforge binary at {} for artifact help parity checks",
            repo_binary.display()
        );
        binaries.push(repo_binary);
    }
    binaries
}

fn removed_workflow_record_pivot_subcommand() -> String {
    ["record", "-pivot"].concat()
}

fn workflow_fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/codex-runtime/fixtures/workflow-artifacts")
        .join(relative)
}

fn init_route_smoke_repo() -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("route smoke repo tempdir should exist");
    let state_dir = TempDir::new().expect("route smoke state tempdir should exist");
    repo_template_support::populate_repo_from_template(repo_dir.path());
    (repo_dir, state_dir)
}

fn copy_workflow_fixture(relative: &str, dest: &Path) {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .expect("route smoke fixture destination parent should be creatable");
    }
    fs::copy(workflow_fixture_path(relative), dest)
        .expect("route smoke workflow fixture should copy");
}

fn install_route_smoke_artifacts(repo: &Path) {
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_rel = ROUTE_SMOKE_PLAN_REL;

    copy_workflow_fixture(
        "specs/2026-03-22-runtime-integration-hardening-design.md",
        &repo.join(spec_rel),
    );

    let plan_source = fs::read_to_string(workflow_fixture_path(
        "plans/2026-03-22-runtime-integration-hardening.md",
    ))
    .expect("route smoke plan fixture should load");
    let adjusted_plan = plan_source.replace(
        "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
        spec_rel,
    );
    let plan_path = repo.join(plan_rel);
    fs::create_dir_all(
        plan_path
            .parent()
            .expect("route smoke plan path should have a parent"),
    )
    .expect("route smoke plan parent should be creatable");
    fs::write(&plan_path, adjusted_plan).expect("route smoke plan fixture should write");

    plan_fidelity_support::write_current_pass_plan_fidelity_review_artifact_for_plan(
        repo, plan_rel,
    );
}

fn host_checked_in_runtime_route_binaries() -> Vec<(&'static str, PathBuf)> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut binaries = Vec::new();

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        binaries.push((
            "repo-root checked-in runtime",
            repo_root.join("bin/featureforge"),
        ));
        binaries.push((
            "darwin-arm64 prebuilt runtime",
            repo_root.join("bin/prebuilt/darwin-arm64/featureforge"),
        ));
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        binaries.push((
            "windows-x64 prebuilt runtime",
            repo_root.join("bin/prebuilt/windows-x64/featureforge.exe"),
        ));
        binaries.push((
            "repo-root checked-in runtime",
            repo_root.join("bin/featureforge.exe"),
        ));
    }

    binaries
        .into_iter()
        .filter(|(_, binary)| binary.is_file())
        .collect()
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    process_support::run_checked(checkout, "git checkout route-smoke branch");
}

#[test]
fn featureforge_help_and_version_exist() {
    let mut help = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    let help_output = help
        .arg("--help")
        .output()
        .expect("help command should run");
    assert!(
        help_output.status.success(),
        "expected --help to succeed, got {:?}",
        help_output.status
    );
    let help_stdout = String::from_utf8(help_output.stdout).expect("help stdout should be utf-8");
    assert!(
        help_stdout.contains("featureforge"),
        "expected help output to mention the featureforge binary name, got:\n{help_stdout}"
    );

    let mut version = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    let version_output = version
        .arg("--version")
        .output()
        .expect("version command should run");
    assert!(
        version_output.status.success(),
        "expected --version to succeed, got {:?}",
        version_output.status
    );
    let version_stdout =
        String::from_utf8(version_output.stdout).expect("version stdout should be utf-8");
    assert!(
        version_stdout.starts_with(&format!("featureforge {}", env!("CARGO_PKG_VERSION"))),
        "expected version output to start with 'featureforge {}', got:\n{version_stdout}",
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn checked_in_runtime_route_smoke_exposes_typed_public_argv_when_host_compatible() {
    let route_binaries = host_checked_in_runtime_route_binaries();
    if route_binaries.is_empty() {
        let cargo_binary = PathBuf::from(env!("CARGO_BIN_EXE_featureforge"));
        assert!(
            cargo_binary.is_file(),
            "no host-compatible checked-in runtime route smoke is packaged for {}-{}, and cargo-built public route coverage binary is unavailable at {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            cargo_binary.display()
        );
        eprintln!(
            "skipping checked-in runtime route smoke: no host-compatible checked-in runtime is packaged for {}-{}; cargo-built public route tests remain behavioral coverage",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        return;
    }

    let (repo_dir, state_dir) = init_route_smoke_repo();
    let home_dir = TempDir::new().expect("route smoke HOME tempdir should exist");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_route_smoke_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "checked-in-runtime-route-smoke");

    for (label, binary) in route_binaries {
        let output = Command::new(&binary)
            .current_dir(repo)
            .env("FEATUREFORGE_STATE_DIR", state)
            .env("HOME", home_dir.path())
            .args([
                "plan",
                "execution",
                "status",
                "--plan",
                ROUTE_SMOKE_PLAN_REL,
            ])
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "{label} should run public route smoke from {}: {error}",
                    binary.display()
                )
            });
        assert!(
            output.status.success(),
            "{label} public route smoke should succeed for {}, got {:?}\nstdout:\n{}\nstderr:\n{}",
            binary.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let status: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{label} public route smoke should emit JSON: {error}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert_eq!(
            status["state_kind"], "actionable_public_command",
            "{label} should expose an actionable route, got {status}"
        );
        assert_eq!(
            status["recommended_public_command_argv"],
            serde_json::json!([
                "featureforge",
                "plan",
                "execution",
                "begin",
                "--plan",
                ROUTE_SMOKE_PLAN_REL,
                "--task",
                "1",
                "--step",
                "1",
                "--execution-mode",
                "featureforge:executing-plans",
                "--expect-execution-fingerprint",
                status["execution_fingerprint"]
                    .as_str()
                    .expect("route smoke status should expose execution_fingerprint"),
            ]),
            "{label} should expose typed begin argv through checked-in runtime output: {status}"
        );
        assert!(
            status["recommended_public_command_template"].is_null(),
            "{label} should emit executable argv directly for this route, not a template: {status}"
        );
    }
}

#[test]
fn repo_root_exposes_featureforge_binary_contract() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let unix_binary = repo_root.join("bin/featureforge");
    let windows_binary = repo_root.join("bin/featureforge.exe");
    assert!(
        unix_binary.is_file() || windows_binary.is_file(),
        "expected repo root to expose a real featureforge binary at {} or {}",
        unix_binary.display(),
        windows_binary.display()
    );
}

#[test]
fn workflow_record_pivot_help_is_removed_from_public_surface() {
    for binary in featureforge_help_binaries() {
        let removed_subcommand = removed_workflow_record_pivot_subcommand();
        let output = Command::new(&binary)
            .args(["workflow", removed_subcommand.as_str(), "--help"])
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "workflow {removed_subcommand} --help should execute for binary {}: {error}",
                    binary.display()
                )
            });
        assert!(
            !output.status.success(),
            "workflow {removed_subcommand} --help should be rejected for binary {}, got {:?}",
            binary.display(),
            output.status
        );
        let stderr = String::from_utf8(output.stderr).unwrap_or_else(|error| {
            panic!("workflow {removed_subcommand} help stderr should be utf-8: {error}")
        });
        assert!(
            stderr.contains(&format!("unrecognized subcommand '{removed_subcommand}'")),
            "workflow {removed_subcommand} --help should fail with unknown-subcommand for binary {}, got:\n{stderr}",
            binary.display()
        );
    }
}
