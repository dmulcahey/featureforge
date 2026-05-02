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
