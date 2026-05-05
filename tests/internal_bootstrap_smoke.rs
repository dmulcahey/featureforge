// Internal compatibility tests extracted from tests/bootstrap_smoke.rs.
// This file intentionally reuses the source fixture scaffolding from the public-facing integration test.

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
        for command in ["status", "doctor", "operator"] {
            assert!(
                stdout
                    .lines()
                    .any(|line| line.trim_start().starts_with(command)),
                "workflow --help should expose `{command}` for binary {}, got:\n{stdout}",
                binary.display()
            );
        }
        for compatibility_only in [
            "resolve",
            "expect",
            "sync",
            "next",
            "artifacts",
            "explain",
            "phase",
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
