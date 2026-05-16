#[path = "support/git.rs"]
mod git_support;
#[path = "support/install.rs"]
mod install_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use assert_cmd::Command as AssertCommand;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

use install_support::canonical_install_bin;
use process_support::{repo_root, run, run_checked};
use rust_source_scan::source_tree_declares_test;

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

#[test]
fn public_runtime_diagnostics_do_not_hardcode_repair_review_state_recovery() {
    let root = repo_root();
    let route_guidance = read_utf8(root.join("src/execution/public_route_guidance.rs"));
    for needle in [
        "recommended_public_command_argv",
        "recommended_public_command_template",
        "recommended_public_command_template.input_bindings",
        "required_inputs",
    ] {
        assert_contains(
            &route_guidance,
            needle,
            "src/execution/public_route_guidance.rs",
        );
    }

    for relative_path in [
        "src/execution/status_support.rs",
        "src/execution/state/preflight.rs",
    ] {
        let path = root.join(relative_path);
        let content = read_utf8(&path);
        let label = path.display().to_string();
        assert_not_contains(
            &content,
            "featureforge plan execution repair-review-state --plan",
            &label,
        );
        assert_not_contains(
            &content,
            "repair_review_state_preflight_recovery_command",
            &label,
        );
    }

    let preflight = read_utf8(root.join("src/execution/state/preflight.rs"));
    assert_contains(
        &preflight,
        "PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT",
        "src/execution/state/preflight.rs",
    );
    for duplicated_field_law in [
        "follow its typed `recommended_public_command_argv`",
        "recommended_public_command_template` to recover a public route",
    ] {
        assert_not_contains(
            &preflight,
            duplicated_field_law,
            "src/execution/state/preflight.rs",
        );
    }
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
fn late_stage_precedence_reference_delegates_rows_to_runtime_without_row_oracle() {
    let root = repo_root();
    let runtime_precedence = read_utf8(root.join("src/execution/late_stage_precedence.rs"));
    let reference = read_utf8(root.join("review/late-stage-precedence-reference.md"));

    assert!(
        runtime_precedence.contains("const PRECEDENCE_ROWS")
            && runtime_precedence.contains("pub fn resolve("),
        "runtime should own late-stage precedence rows and resolution"
    );

    assert!(
        reference.contains("Do not maintain a second phase matrix"),
        "late-stage reference should delegate row authority instead of mirroring runtime rows"
    );
    assert!(
        reference.contains("src/execution/late_stage_precedence.rs")
            && reference.contains(
                "$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json"
            )
            && reference.contains("references/operator-route-authority.md"),
        "late-stage reference should point to runtime/operator/route authority surfaces"
    );
    assert!(
        !reference.contains("| Release Gate | Review Gate | QA Gate | Phase |"),
        "late-stage reference must not maintain a duplicated precedence table"
    );
    for required in [
        "Legacy finish-gate compatibility commands are compatibility/debug boundaries",
        "Low-level `record-*` commands are compatibility/debug boundaries",
        "execute that selected",
        "typed route or selected handoff lane",
        "Do not use this reference to run a",
        "memorized chain",
        "Do not infer branch closure, release readiness, final review, QA, or finish",
    ] {
        assert!(
            reference.contains(required),
            "late-stage reference should retain command-boundary guidance: {required}"
        );
    }

    for internal_action_token in [
        "advance_late_stage",
        "dispatch_final_review",
        "run_qa",
        "run_finish_review_gate",
        "run_finish_completion_gate",
    ] {
        assert!(
            !reference.contains(internal_action_token),
            "late-stage reference should avoid internal action token {internal_action_token:?}"
        );
    }
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
fn installed_control_plane_verification_gate_includes_required_commands() {
    let root = repo_root();
    let script = root.join("scripts/verify-installed-control-plane-isolation.sh");
    let script_content = read_utf8(&script);

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
    }

    for targeted_test in [
        "runtime_provenance_classifies_installed_runtime",
        "runtime_provenance_classifies_workspace_runtime",
        "workspace_runtime_blocks_live_repair_review_state",
        "workspace_runtime_blocks_live_close_current_task",
        "workspace_runtime_allows_fixture_repair_review_state_with_temp_state",
        "evidence_lint_rejects_workspace_runtime_live_mutation",
        "self_hosting_diagnostic_reports_installed_and_workspace_hashes",
    ] {
        assert!(
            source_tree_declares_test(&root, targeted_test),
            "installed control-plane targeted test should exist: {targeted_test}"
        );
    }

    assert_file_contains(
        root.join("scripts/verify-source-archive.mjs"),
        "scripts/verify-installed-control-plane-isolation.sh",
    );
}

#[test]
fn runtime_diagnostic_failure_guidance_is_stop_only() {
    let root = repo_root();
    let command_eligibility = read_utf8(root.join("src/execution/command_eligibility.rs"));
    assert_contains(
        &command_eligibility,
        "stop on runtime diagnostic; do not retry mutation",
        "src/execution/command_eligibility.rs",
    );
    assert_not_contains(
        &command_eligibility,
        "before retrying mutation",
        "src/execution/command_eligibility.rs",
    );

    let mutation_guards = read_utf8(root.join("src/execution/commands/common/mutation_guards.rs"));
    let route_guidance = read_utf8(root.join("src/execution/public_route_guidance.rs"));
    assert_contains(
        &mutation_guards,
        "projection rebuild reports stale projection candidates without mutating runtime truth",
        "src/execution/commands/common/mutation_guards.rs",
    );
    assert_contains(
        &mutation_guards,
        "OPERATOR_ROUTE_AUTHORITY_REFERENCE",
        "src/execution/commands/common/mutation_guards.rs",
    );
    assert_contains(
        &route_guidance,
        "follow the shared route law in references/operator-route-authority.md",
        "src/execution/public_route_guidance.rs",
    );
    assert_not_contains(
        &mutation_guards,
        "run materialize-projections for explicit projection materialization or replay stale execution with reopen/begin/complete",
        "src/execution/commands/common/mutation_guards.rs",
    );
}
