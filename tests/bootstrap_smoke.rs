use std::path::PathBuf;

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
fn internal_only_compatibility_plan_execution_help_surface_hides_low_level_compatibility_commands()
{
    for binary in featureforge_help_binaries() {
        let output = std::process::Command::new(&binary)
            .args(["plan", "execution", "--help"])
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "plan execution --help should run for binary {}: {error}",
                    binary.display()
                )
            });
        assert!(
            output.status.success(),
            "expected plan execution --help to succeed for binary {}, got {:?}",
            binary.display(),
            output.status
        );
        let stdout =
            String::from_utf8(output.stdout).expect("plan execution help stdout should be utf-8");
        for command in [
            "status",
            "begin",
            "complete",
            "reopen",
            "transfer",
            "close-current-task",
            "repair-review-state",
            "advance-late-stage",
        ] {
            assert!(
                stdout.contains(command),
                "plan execution --help should include `{command}` for binary {}, got:\n{stdout}",
                binary.display()
            );
        }
        for description in [
            "Diagnostic routing status query.",
            "Intent-level review-state repair command.",
            "Intent-level task-closure command.",
            "Intent-level late-stage progression command.",
            "Execution step start recorder.",
            "Execution step completion recorder.",
            "Execution task reopen recorder.",
            "Execution handoff transfer recorder.",
        ] {
            assert!(
                stdout.contains(description),
                "plan execution --help should include description `{description}` for binary {}, got:\n{stdout}",
                binary.display()
            );
        }
        for compatibility_only in [
            "recommend",
            concat!("pre", "flight"),
            concat!("record", "-review-dispatch"),
            concat!("record", "-branch-closure"),
            concat!("record", "-release-readiness"),
            concat!("record", "-final-review"),
            concat!("record", "-qa"),
            concat!("rebuild", "-evidence"),
            concat!("explain", "-review-state"),
            concat!("reconcile", "-review-state"),
            "gate-contract",
            "gate-evaluator",
            "gate-handoff",
            concat!("gate", "-review"),
            concat!("gate", "-finish"),
        ] {
            assert!(
                !stdout.contains(compatibility_only),
                "plan execution --help should not expose compatibility-only `{compatibility_only}` for binary {}, got:\n{stdout}",
                binary.display()
            );
        }
    }
}

#[test]
fn internal_only_compatibility_normal_path_command_help_hides_dispatch_id_plumbing() {
    for binary in featureforge_help_binaries() {
        for command in ["close-current-task", "advance-late-stage"] {
            let output = std::process::Command::new(&binary)
                .args(["plan", "execution", command, "--help"])
                .output()
                .unwrap_or_else(|error| {
                    panic!(
                        "plan execution {command} --help should run for binary {}: {error}",
                        binary.display()
                    )
                });
            assert!(
                output.status.success(),
                "expected plan execution {command} --help to succeed for binary {}, got {:?}",
                binary.display(),
                output.status
            );
            let stdout = String::from_utf8(output.stdout)
                .expect("normal-path command help stdout should be utf-8");
            assert!(
                !stdout.contains(concat!("--dispatch", "-id")),
                "plan execution {command} --help should not expose {} plumbing for binary {}, got:\n{stdout}",
                concat!("--dispatch", "-id"),
                binary.display()
            );
        }
    }
}

#[test]
fn internal_only_compatibility_workflow_help_surface_hides_compatibility_only_commands() {
    for binary in featureforge_help_binaries() {
        let output = std::process::Command::new(&binary)
            .args(["workflow", "--help"])
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "workflow --help should run for binary {}: {error}",
                    binary.display()
                )
            });
        assert!(
            output.status.success(),
            "expected workflow --help to succeed for binary {}, got {:?}",
            binary.display(),
            output.status
        );
        let stdout =
            String::from_utf8(output.stdout).expect("workflow help stdout should be utf-8");
        let command = "operator";
        assert!(
            stdout
                .lines()
                .any(|line| line.trim_start().starts_with(command)),
            "workflow --help should expose `{command}` for binary {}, got:\n{stdout}",
            binary.display()
        );
        for compatibility_only in [
            "resolve",
            "expect",
            "sync",
            "next",
            "artifacts",
            "explain",
            "phase",
            "doctor",
            "handoff",
            concat!("pre", "flight"),
            "gate",
        ] {
            assert!(
                !stdout
                    .lines()
                    .any(|line| line.trim_start().starts_with(compatibility_only)),
                "workflow --help should not expose compatibility-only `{compatibility_only}` for binary {}, got:\n{stdout}",
                binary.display()
            );
        }
    }
}

#[test]
fn workflow_record_pivot_help_is_removed_from_public_surface() {
    for binary in featureforge_help_binaries() {
        let output = std::process::Command::new(&binary)
            .args(["workflow", "record-pivot", "--help"])
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "workflow record-pivot --help should execute for binary {}: {error}",
                    binary.display()
                )
            });
        assert!(
            !output.status.success(),
            "workflow record-pivot --help should be rejected for binary {}, got {:?}",
            binary.display(),
            output.status
        );
        let stderr = String::from_utf8(output.stderr)
            .expect("workflow record-pivot help stderr should be utf-8");
        assert!(
            stderr.contains("unrecognized subcommand 'record-pivot'"),
            "workflow record-pivot --help should fail with unknown-subcommand for binary {}, got:\n{stderr}",
            binary.display()
        );
    }
}

#[test]
fn internal_only_compatibility_command_help_marks_non_normal_flow_usage() {
    for binary in featureforge_help_binaries() {
        for compatibility_command in [
            concat!("record", "-review-dispatch"),
            concat!("record", "-branch-closure"),
            concat!("record", "-release-readiness"),
            concat!("record", "-final-review"),
            concat!("record", "-qa"),
            concat!("rebuild", "-evidence"),
        ] {
            let output = std::process::Command::new(&binary)
                .args(["plan", "execution", compatibility_command, "--help"])
                .output()
                .unwrap_or_else(|error| {
                    panic!(
                        "plan execution {compatibility_command} --help should run for binary {}: {error}",
                        binary.display()
                    )
                });
            let stdout = String::from_utf8(output.stdout)
                .expect("compatibility command stdout should be utf-8");
            let stderr = String::from_utf8(output.stderr)
                .expect("compatibility command stderr should be utf-8");
            let combined = format!("{stdout}\n{stderr}");
            if output.status.success() {
                assert!(
                    stdout.contains("Compatibility/debug"),
                    "legacy compatibility command `{compatibility_command}` should remain explicitly marked as non-normal flow for binary {}, got:\n{stdout}",
                    binary.display()
                );
            } else {
                assert!(
                    combined.contains("unrecognized subcommand"),
                    "removed compatibility command `{compatibility_command}` should fail as unknown for binary {}, got:\n{combined}",
                    binary.display()
                );
            }
        }
    }
}
