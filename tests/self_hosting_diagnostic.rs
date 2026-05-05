#[path = "support/process.rs"]
mod process_support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use featureforge::execution::runtime_provenance::featureforge_runtime_binary_name;
use process_support::{assert_workspace_runtime_uses_temp_state, repo_root, run};
use serde_json::Value;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

struct DiagnosticFixture {
    _temp: TempDir,
    home_dir: PathBuf,
    codex_home: PathBuf,
    state_dir: PathBuf,
    installed_runtime_path: PathBuf,
    installed_skill_root: PathBuf,
}

impl DiagnosticFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("self-hosting diagnostic temp dir should exist");
        let home_dir = temp.path().join("home");
        let install_root = home_dir.join(".featureforge").join("install");
        let installed_runtime_path = install_root
            .join("bin")
            .join(featureforge_runtime_binary_name());
        let installed_skill_root = install_root.join("skills");
        let installed_featureforge_skill = installed_skill_root.join("featureforge");
        let codex_home = install_root.clone();
        let state_dir = temp.path().join("featureforge-state");
        fs::create_dir_all(
            installed_runtime_path
                .parent()
                .expect("installed runtime should have a parent"),
        )
        .expect("installed runtime parent should be creatable");
        fs::write(
            &installed_runtime_path,
            b"installed featureforge test runtime\n",
        )
        .expect("installed runtime fixture should be writable");
        make_executable(&installed_runtime_path);
        fs::create_dir_all(&installed_featureforge_skill)
            .expect("installed FeatureForge skill root should be creatable");
        fs::create_dir_all(&state_dir).expect("diagnostic state dir should be creatable");

        Self {
            _temp: temp,
            home_dir,
            codex_home,
            state_dir,
            installed_runtime_path,
            installed_skill_root,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_featureforge"));
        command
            .current_dir(repo_root())
            .env("HOME", &self.home_dir)
            .env("CODEX_HOME", &self.codex_home)
            .env("FEATUREFORGE_STATE_DIR", &self.state_dir)
            .env_remove("FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION")
            .args(args);
        command
    }
}

#[test]
fn self_hosting_diagnostic_reports_installed_and_workspace_hashes() {
    let fixture = DiagnosticFixture::new();
    assert_workspace_runtime_uses_temp_state(
        Some(&repo_root()),
        Some(&fixture.state_dir),
        Some(&fixture.home_dir),
        false,
        "doctor self-hosting json",
    );

    let output = run(
        fixture.command(&["doctor", "self-hosting", "--json"]),
        "doctor self-hosting json",
    );
    assert!(
        output.status.success(),
        "doctor self-hosting --json should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic: Value =
        serde_json::from_slice(&output.stdout).expect("diagnostic should be valid JSON");

    assert_exact_json_fields(
        &diagnostic,
        &[
            "active_skill_root",
            "installed_runtime_hash",
            "installed_runtime_path",
            "invoked_runtime_hash",
            "invoked_runtime_path",
            "is_featureforge_repo",
            "live_mutation_allowed",
            "recommended_remediation",
            "repo_root",
            "runtime_source",
            "skill_source",
            "state_dir",
            "state_dir_kind",
            "warnings",
            "workspace_runtime_hash",
            "workspace_runtime_path",
        ],
    );
    let expected_invoked_runtime_path = canonicalized_path(env!("CARGO_BIN_EXE_featureforge"));
    let expected_state_dir = canonicalized_path(&fixture.state_dir);
    let expected_repo_root = canonicalized_path(repo_root());
    assert_eq!(
        diagnostic["installed_runtime_path"].as_str(),
        Some(fixture.installed_runtime_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        diagnostic["invoked_runtime_path"].as_str(),
        Some(expected_invoked_runtime_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        diagnostic["workspace_runtime_path"].as_str(),
        Some(expected_invoked_runtime_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        diagnostic["state_dir"].as_str(),
        Some(expected_state_dir.to_string_lossy().as_ref())
    );
    assert_eq!(
        diagnostic["repo_root"].as_str(),
        Some(expected_repo_root.to_string_lossy().as_ref())
    );
    assert_hash(&diagnostic["installed_runtime_hash"]);
    assert_hash(&diagnostic["invoked_runtime_hash"]);
    assert_hash(&diagnostic["workspace_runtime_hash"]);
    assert_eq!(diagnostic["runtime_source"], "workspace");
    assert_eq!(diagnostic["skill_source"], "installed");
    assert_eq!(diagnostic["state_dir_kind"], "temp");
    assert_eq!(diagnostic["is_featureforge_repo"], true);
    assert_eq!(diagnostic["live_mutation_allowed"], true);
    let expected_installed_skill_root = fs::canonicalize(&fixture.installed_skill_root)
        .unwrap_or_else(|_| fixture.installed_skill_root.clone());
    assert!(
        diagnostic["active_skill_root"].as_str().is_some_and(
            |root| root.starts_with(expected_installed_skill_root.to_string_lossy().as_ref())
        ),
        "active skill root should resolve under installed root: {diagnostic}"
    );
    assert!(
        diagnostic["warnings"]
            .as_array()
            .expect("warnings should be an array")
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|warning| warning.contains("workspace runtime detected"))),
        "workspace runtime warning should be reported: {diagnostic}"
    );
    assert!(
        diagnostic["recommended_remediation"]
            .as_str()
            .is_some_and(|remediation| remediation.contains("live workflow control-plane")),
        "diagnostic should recommend installed runtime control-plane use: {diagnostic}"
    );
}

#[test]
fn self_hosting_diagnostic_text_output_is_readable() {
    let fixture = DiagnosticFixture::new();
    assert_workspace_runtime_uses_temp_state(
        Some(&repo_root()),
        Some(&fixture.state_dir),
        Some(&fixture.home_dir),
        false,
        "doctor self-hosting text",
    );

    let output = run(
        fixture.command(&["doctor", "self-hosting"]),
        "doctor self-hosting text",
    );
    assert!(
        output.status.success(),
        "doctor self-hosting should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Self-hosting diagnostic"));
    assert!(stdout.contains("runtime_source: Workspace"));
    assert!(stdout.contains("skill_source: Installed"));
    assert!(stdout.contains("live_mutation_allowed: true"));
}

#[test]
fn self_hosting_diagnostic_reports_blocked_live_workspace_mutation_policy() {
    let fixture = DiagnosticFixture::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    command
        .current_dir(repo_root())
        .env("HOME", &fixture.home_dir)
        .env("CODEX_HOME", &fixture.codex_home)
        .env_remove("FEATUREFORGE_STATE_DIR")
        .env_remove("FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION")
        .args(["doctor", "self-hosting", "--json"]);

    let output = run(command, "doctor self-hosting live-state json");
    assert!(
        output.status.success(),
        "doctor self-hosting --json should succeed for read-only live-state diagnostics\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic: Value =
        serde_json::from_slice(&output.stdout).expect("diagnostic should be valid JSON");

    assert_eq!(diagnostic["runtime_source"], "workspace");
    assert_eq!(diagnostic["state_dir_kind"], "live");
    assert_eq!(diagnostic["live_mutation_allowed"], false);
    assert!(
        diagnostic["warnings"]
            .as_array()
            .expect("warnings should be an array")
            .iter()
            .any(|warning| {
                warning.as_str().is_some_and(|warning| {
                    warning.contains("workspace_runtime_live_mutation_blocked")
                })
            }),
        "live workspace diagnostic should report blocked mutation policy: {diagnostic}"
    );
}

fn assert_hash(value: &Value) {
    let Some(hash) = value.as_str() else {
        panic!("expected hash string, got {value}");
    };
    assert!(
        hash.strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == 64 && hex.chars().all(|ch| ch.is_ascii_hexdigit())),
        "hash should be a sha256 digest, got {hash}"
    );
}

fn assert_exact_json_fields(value: &Value, expected: &[&str]) {
    let object = value
        .as_object()
        .expect("diagnostic should be a JSON object");
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "diagnostic JSON field set drifted");
}

fn canonicalized_path(path: impl AsRef<Path>) -> PathBuf {
    fs::canonicalize(path.as_ref()).unwrap_or_else(|_| path.as_ref().to_path_buf())
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)
            .expect("runtime fixture metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .expect("runtime fixture permissions should be writable");
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
