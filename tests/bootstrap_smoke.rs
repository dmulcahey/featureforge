use std::path::PathBuf;

#[test]
fn featureforge_help_and_version_exist() {
    let mut help = std::process::Command::new(env!("CARGO_BIN_EXE_featureforge"));
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

    let mut version = std::process::Command::new(env!("CARGO_BIN_EXE_featureforge"));
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
fn plan_execution_help_surface_hides_low_level_compatibility_commands() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_featureforge"))
        .args(["plan", "execution", "--help"])
        .output()
        .expect("plan execution help command should run");
    assert!(
        output.status.success(),
        "expected plan execution --help to succeed, got {:?}",
        output.status
    );
    let stdout =
        String::from_utf8(output.stdout).expect("plan execution help stdout should be utf-8");
    for command in [
        "begin",
        "complete",
        "close-current-task",
        "repair-review-state",
        "advance-late-stage",
    ] {
        assert!(
            stdout.contains(command),
            "plan execution --help should include `{command}`, got:\n{stdout}"
        );
    }
    for compatibility_only in [
        "recommend",
        "preflight",
        "record-review-dispatch",
        "record-branch-closure",
        "record-release-readiness",
        "record-final-review",
        "record-qa",
        "rebuild-evidence",
        "explain-review-state",
        "reconcile-review-state",
        "gate-contract",
        "gate-evaluator",
        "gate-handoff",
        "gate-review",
        "gate-finish",
    ] {
        assert!(
            !stdout.contains(compatibility_only),
            "plan execution --help should not expose compatibility-only `{compatibility_only}`, got:\n{stdout}"
        );
    }
}

#[test]
fn normal_path_command_help_hides_dispatch_id_plumbing() {
    for command in ["close-current-task", "advance-late-stage"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_featureforge"))
            .args(["plan", "execution", command, "--help"])
            .output()
            .unwrap_or_else(|error| panic!("plan execution {command} --help should run: {error}"));
        assert!(
            output.status.success(),
            "expected plan execution {command} --help to succeed, got {:?}",
            output.status
        );
        let stdout = String::from_utf8(output.stdout)
            .expect("normal-path command help stdout should be utf-8");
        assert!(
            !stdout.contains("--dispatch-id"),
            "plan execution {command} --help should not expose --dispatch-id plumbing, got:\n{stdout}"
        );
    }
}

#[test]
fn direct_compatibility_command_help_marks_non_normal_flow_usage() {
    for compatibility_command in [
        "record-review-dispatch",
        "record-branch-closure",
        "record-release-readiness",
        "record-final-review",
        "record-qa",
        "rebuild-evidence",
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_featureforge"))
            .args(["plan", "execution", compatibility_command, "--help"])
            .output()
            .unwrap_or_else(|error| {
                panic!("plan execution {compatibility_command} --help should run: {error}")
            });
        assert!(
            output.status.success(),
            "expected plan execution {compatibility_command} --help to succeed, got {:?}",
            output.status
        );
        let stdout = String::from_utf8(output.stdout)
            .expect("compatibility command help stdout should be utf-8");
        assert!(
            stdout.contains("Compatibility/debug"),
            "direct compatibility command help should explicitly mark non-normal flow usage for `{compatibility_command}`, got:\n{stdout}"
        );
    }
}
