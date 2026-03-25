#[path = "support/executable.rs"]
mod executable_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/prebuilt.rs"]
mod prebuilt_support;
#[path = "support/process.rs"]
mod process_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

use executable_support::make_executable;
use files_support::write_file;
use prebuilt_support::{
    DARWIN_ARM64_BINARY_REL, DARWIN_ARM64_CHECKSUM_REL, WINDOWS_X64_BINARY_REL,
    WINDOWS_X64_CHECKSUM_REL, write_canonical_prebuilt_layout,
};
use process_support::{repo_root, run};

fn skill_doc_path() -> PathBuf {
    repo_root().join("featureforge-upgrade/SKILL.md")
}

fn read_skill_doc() -> String {
    fs::read_to_string(skill_doc_path()).expect("featureforge-upgrade skill doc should be readable")
}

fn assert_contains(content: &str, needle: &str, label: &str) {
    assert!(
        content.contains(needle),
        "{label} should contain {needle:?}"
    );
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
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

fn run_bash_block(
    cwd: &Path,
    home_dir: &Path,
    script: &str,
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    let mut command = Command::new("bash");
    command.arg("-euo").arg("pipefail").arg("-c").arg(script);
    command.current_dir(cwd);
    command.env("HOME", home_dir);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, context)
}

fn make_valid_install(dir: &Path, git_mode: &str) {
    fs::create_dir_all(dir.join("bin")).expect("install bin dir should exist");
    write_file(&dir.join("bin/featureforge"), "");
    make_executable(&dir.join("bin/featureforge"));
    write_file(&dir.join("VERSION"), "1.0.0\n");
    write_canonical_prebuilt_layout(
        dir,
        "1.0.0",
        "#!/usr/bin/env bash\nprintf 'darwin runtime\\n'\n",
        "windows runtime\n",
    );
    match git_mode {
        "dir" => {
            fs::create_dir_all(dir.join(".git")).expect(".git dir should exist");
        }
        "file" => {
            write_file(&dir.join(".git"), "gitdir: /tmp/fake-worktree\n");
        }
        _ => panic!("unexpected git mode {git_mode}"),
    }
}

fn write_mock_featureforge(bin_dir: &Path, script_body: &str) {
    fs::create_dir_all(bin_dir).expect("mock featureforge bin dir should exist");
    let helper_path = bin_dir.join("featureforge");
    write_file(&helper_path, script_body);
    make_executable(&helper_path);
}

fn path_with_mock_featureforge(bin_dir: &Path) -> String {
    match std::env::var("PATH") {
        Ok(existing) if !existing.is_empty() => format!("{}:{existing}", bin_dir.display()),
        _ => bin_dir.display().to_string(),
    }
}

fn json_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn resolved_runtime_root_json(root: &Path) -> String {
    format!(
        "{{\"resolved\":true,\"root\":\"{}\",\"source\":\"featureforge_dir\",\"validation\":{{\"has_version\":true,\"has_binary\":true,\"upgrade_eligible\":true}}}}",
        json_string(&root.to_string_lossy())
    )
}

#[test]
fn upgrade_skill_contract_tracks_doc_patterns_and_install_root_resolution() {
    let skill_doc = read_skill_doc();
    for pattern in [
        "featureforge repo runtime-root --json",
        "bin/featureforge",
        "FEATUREFORGE_BIN=\"$INSTALL_DIR/bin/featureforge\"",
        "VERSION",
        "ERROR: featureforge runtime-root helper unavailable",
        "ERROR: featureforge runtime root unavailable",
        "ERROR: featureforge runtime-root helper returned unreadable JSON",
        "Read `$INSTALL_DIR/RELEASE-NOTES.md`.",
        "git stash push --include-untracked",
        "git stash pop",
        "ERROR: featureforge upgrade failed during git pull",
        "Run $FEATUREFORGE_BIN config set update_check true to re-enable.",
        "REMOTE_URL=\"${FEATUREFORGE_REMOTE_URL:-https://raw.githubusercontent.com/dmulcahey/featureforge/main/VERSION}\"",
        "REMOTE_STATUS=",
        "VERSION_RELATION=",
        "If `REMOTE_STATUS=unavailable` and this skill was invoked directly, stop before Step 3.",
        "FeatureForge couldn't verify the latest version right now.",
        "If `VERSION_RELATION=equal`, tell the user: `You're already on the latest known version (v$LOCAL_VERSION).`",
        "If `VERSION_RELATION=local_ahead`, tell the user: `Your local FeatureForge install (v$LOCAL_VERSION) is newer than the fetched remote version (v$REMOTE_VERSION).`",
        "If this skill was invoked from an `UPGRADE_AVAILABLE` handoff",
        "You're already on the latest known version (v$LOCAL_VERSION).",
    ] {
        assert_contains(&skill_doc, pattern, "featureforge-upgrade/SKILL.md");
    }
    assert!(
        !skill_doc.contains("featureforge-update-check"),
        "featureforge-upgrade/SKILL.md should not reference removed helper binaries"
    );
    assert!(
        !skill_doc.contains("featureforge-config"),
        "featureforge-upgrade/SKILL.md should not reference removed helper binaries"
    );

    let step_one = extract_bash_block(&skill_doc, "### Step 1: Resolve install root");

    let tmp_root = TempDir::new().expect("temp root should exist");
    let home_dir = tmp_root.path().join("home");
    fs::create_dir_all(&home_dir).expect("home should exist");
    let current_root = tmp_root.path().join("current-project");
    fs::create_dir_all(&current_root).expect("project root should exist");
    let helper_bin = tmp_root.path().join("mock-bin");
    let helper_path = path_with_mock_featureforge(&helper_bin);
    let resolved_install = tmp_root.path().join("resolved-install");
    fs::create_dir_all(&resolved_install).expect("resolved install dir should exist");
    write_mock_featureforge(
        &helper_bin,
        &format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' '{}'\n",
            resolved_runtime_root_json(&resolved_install)
        ),
    );

    let active_output = run_bash_block(
        &current_root,
        &home_dir,
        &step_one,
        &[("PATH", helper_path.as_str())],
        "upgrade skill step 1 resolved helper",
    );
    let active_stdout = String::from_utf8_lossy(&active_output.stdout);
    assert_contains(
        &active_stdout,
        &format!("INSTALL_DIR={}", resolved_install.display()),
        "upgrade skill step 1 resolved helper",
    );

    let renamed_root = tmp_root.path().join("custom-runtime-name");
    fs::create_dir_all(&renamed_root).expect("renamed root should exist");
    write_mock_featureforge(
        &helper_bin,
        &format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' '{}'\n",
            resolved_runtime_root_json(
                &renamed_root
                    .canonicalize()
                    .expect("renamed root canonicalize")
            )
        ),
    );
    let renamed_output = run_bash_block(
        &current_root,
        &home_dir,
        &step_one,
        &[("PATH", helper_path.as_str())],
        "upgrade skill step 1 arbitrary resolved path",
    );
    let renamed_stdout = String::from_utf8_lossy(&renamed_output.stdout);
    assert_contains(
        &renamed_stdout,
        &format!(
            "INSTALL_DIR={}",
            renamed_root
                .canonicalize()
                .expect("renamed root canonicalize")
                .display()
        ),
        "upgrade skill step 1 arbitrary resolved path",
    );

    write_mock_featureforge(
        &helper_bin,
        "#!/usr/bin/env bash\nprintf '%s\\n' '{\"resolved\":false,\"root\":null,\"source\":\"unresolved\",\"validation\":{\"has_version\":false,\"has_binary\":false,\"upgrade_eligible\":false}}'\n",
    );
    let unresolved_output = run_bash_block(
        &current_root,
        &home_dir,
        &step_one,
        &[("PATH", helper_path.as_str())],
        "upgrade skill step 1 unresolved helper",
    );
    assert!(
        !unresolved_output.status.success(),
        "unresolved helper output should fail closed"
    );
    assert_contains(
        &combined_output(&unresolved_output),
        "ERROR: featureforge runtime root unavailable",
        "upgrade skill step 1 unresolved helper",
    );

    write_mock_featureforge(
        &helper_bin,
        "#!/usr/bin/env bash\nprintf '%s\\n' '{\"resolved\":true,\"source\":\"featureforge_dir\"}'\n",
    );
    let malformed_output = run_bash_block(
        &current_root,
        &home_dir,
        &step_one,
        &[("PATH", helper_path.as_str())],
        "upgrade skill step 1 malformed helper",
    );
    assert!(
        !malformed_output.status.success(),
        "malformed helper output should fail closed"
    );
    assert_contains(
        &combined_output(&malformed_output),
        "ERROR: featureforge runtime-root helper returned unreadable JSON",
        "upgrade skill step 1 malformed helper",
    );

    fs::remove_file(helper_bin.join("featureforge")).expect("mock helper should remove");
    let unavailable_output = run_bash_block(
        &current_root,
        &home_dir,
        &step_one,
        &[("PATH", helper_path.as_str())],
        "upgrade skill step 1 unavailable helper",
    );
    assert!(
        !unavailable_output.status.success(),
        "missing helper should fail closed"
    );
    assert_contains(
        &combined_output(&unavailable_output),
        "ERROR: featureforge runtime-root helper unavailable",
        "upgrade skill step 1 unavailable helper",
    );
}

#[test]
fn valid_install_fixture_includes_checked_in_prebuilt_layout() {
    let install_root = TempDir::new().expect("install root tempdir should exist");
    make_valid_install(install_root.path(), "dir");

    for relative in [
        DARWIN_ARM64_BINARY_REL,
        DARWIN_ARM64_CHECKSUM_REL,
        WINDOWS_X64_BINARY_REL,
        WINDOWS_X64_CHECKSUM_REL,
        "bin/prebuilt/manifest.json",
    ] {
        assert!(
            install_root.path().join(relative).is_file(),
            "valid install fixture should include {relative}"
        );
    }
}

#[test]
fn upgrade_skill_version_resolution_matches_shell_contract() {
    let skill_doc = read_skill_doc();
    let step_two = extract_bash_block(
        &skill_doc,
        "### Step 2: Resolve versions and auto-upgrade preference",
    );

    let version_root = TempDir::new().expect("version root should exist");

    let behind_install = version_root.path().join("behind");
    make_valid_install(&behind_install, "dir");
    write_file(&behind_install.join("VERSION"), "5.1.2\n");
    let behind_remote = version_root.path().join("behind-remote");
    write_file(&behind_remote, "5.1.10\n");
    let behind_remote_url = format!("file://{}", behind_remote.display());
    let behind_output = run_bash_block(
        &behind_install,
        version_root.path(),
        &step_two,
        &[
            ("INSTALL_DIR", behind_install.to_string_lossy().as_ref()),
            ("FEATUREFORGE_REMOTE_URL", behind_remote_url.as_str()),
        ],
        "upgrade skill step 2 upgrade relation",
    );
    let behind_stdout = String::from_utf8_lossy(&behind_output.stdout);
    assert_contains(&behind_stdout, "LOCAL_VERSION=5.1.2", "upgrade relation");
    assert_contains(&behind_stdout, "REMOTE_VERSION=5.1.10", "upgrade relation");
    assert_contains(
        &behind_stdout,
        "VERSION_RELATION=upgrade",
        "upgrade relation",
    );

    let equal_install = version_root.path().join("equal");
    make_valid_install(&equal_install, "dir");
    write_file(&equal_install.join("VERSION"), "5.1.0\n");
    let equal_remote = version_root.path().join("equal-remote");
    write_file(&equal_remote, "5.1\n");
    let equal_remote_url = format!("file://{}", equal_remote.display());
    let equal_output = run_bash_block(
        &equal_install,
        version_root.path(),
        &step_two,
        &[
            ("INSTALL_DIR", equal_install.to_string_lossy().as_ref()),
            ("FEATUREFORGE_REMOTE_URL", equal_remote_url.as_str()),
        ],
        "upgrade skill step 2 equal relation",
    );
    let equal_stdout = String::from_utf8_lossy(&equal_output.stdout);
    assert_contains(&equal_stdout, "LOCAL_VERSION=5.1.0", "equal relation");
    assert_contains(&equal_stdout, "REMOTE_VERSION=5.1", "equal relation");
    assert_contains(&equal_stdout, "VERSION_RELATION=equal", "equal relation");

    let ahead_install = version_root.path().join("ahead");
    make_valid_install(&ahead_install, "dir");
    write_file(&ahead_install.join("VERSION"), "5.2.0\n");
    let ahead_remote = version_root.path().join("ahead-remote");
    write_file(&ahead_remote, "5.1.9\n");
    let ahead_remote_url = format!("file://{}", ahead_remote.display());
    let ahead_output = run_bash_block(
        &ahead_install,
        version_root.path(),
        &step_two,
        &[
            ("INSTALL_DIR", ahead_install.to_string_lossy().as_ref()),
            ("FEATUREFORGE_REMOTE_URL", ahead_remote_url.as_str()),
        ],
        "upgrade skill step 2 local ahead relation",
    );
    let ahead_stdout = String::from_utf8_lossy(&ahead_output.stdout);
    assert_contains(&ahead_stdout, "LOCAL_VERSION=5.2.0", "local ahead relation");
    assert_contains(
        &ahead_stdout,
        "REMOTE_VERSION=5.1.9",
        "local ahead relation",
    );
    assert_contains(
        &ahead_stdout,
        "VERSION_RELATION=local_ahead",
        "local ahead relation",
    );

    let unavailable_install = version_root.path().join("unavailable");
    make_valid_install(&unavailable_install, "dir");
    write_file(&unavailable_install.join("VERSION"), "5.1.0\n");
    let unavailable_remote_url = format!(
        "file://{}",
        version_root.path().join("does-not-exist").display()
    );
    let unavailable_output = run_bash_block(
        &unavailable_install,
        version_root.path(),
        &step_two,
        &[
            (
                "INSTALL_DIR",
                unavailable_install.to_string_lossy().as_ref(),
            ),
            ("FEATUREFORGE_REMOTE_URL", unavailable_remote_url.as_str()),
        ],
        "upgrade skill step 2 unavailable relation",
    );
    let unavailable_stdout = String::from_utf8_lossy(&unavailable_output.stdout);
    assert_contains(
        &unavailable_stdout,
        "LOCAL_VERSION=5.1.0",
        "remote unavailable",
    );
    assert_contains(&unavailable_stdout, "REMOTE_VERSION=", "remote unavailable");
    assert_contains(
        &unavailable_stdout,
        "REMOTE_STATUS=unavailable",
        "remote unavailable",
    );
    assert_contains(
        &unavailable_stdout,
        "VERSION_RELATION=unknown",
        "remote unavailable",
    );
}
