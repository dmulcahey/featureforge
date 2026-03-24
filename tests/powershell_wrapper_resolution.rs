use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_utf8(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref()).unwrap_or_else(|error| {
        panic!("{} should be readable: {error}", path.as_ref().display())
    })
}

#[test]
fn legacy_powershell_wrapper_entrypoints_are_removed() {
    let root = repo_root();
    for relative in [
        "bin/superpowers.ps1",
        "bin/superpowers-config.ps1",
        "bin/superpowers-migrate-install.ps1",
        "bin/superpowers-plan-contract.ps1",
        "bin/superpowers-plan-execution.ps1",
        "bin/superpowers-repo-safety.ps1",
        "bin/superpowers-session-entry.ps1",
        "bin/superpowers-update-check.ps1",
        "bin/superpowers-workflow-status.ps1",
        "bin/superpowers-workflow.ps1",
        "compat/powershell/superpowers.ps1",
    ] {
        assert!(
            !root.join(relative).exists(),
            "legacy PowerShell wrapper should be removed: {relative}"
        );
    }
}

#[test]
fn canonical_prebuilt_manifest_and_assets_use_featureforge_names() {
    let root = repo_root();
    let manifest = read_utf8(root.join("bin/prebuilt/manifest.json"));
    for needle in [
        "bin/prebuilt/darwin-arm64/featureforge",
        "bin/prebuilt/darwin-arm64/featureforge.sha256",
        "bin/prebuilt/windows-x64/featureforge.exe",
        "bin/prebuilt/windows-x64/featureforge.exe.sha256",
    ] {
        assert!(
            manifest.contains(needle),
            "bin/prebuilt/manifest.json should contain {needle:?}"
        );
    }
    for retired in [
        "bin/prebuilt/darwin-arm64/superpowers",
        "bin/prebuilt/darwin-arm64/superpowers.sha256",
        "bin/prebuilt/windows-x64/superpowers.exe",
        "bin/prebuilt/windows-x64/superpowers.exe.sha256",
    ] {
        assert!(
            !manifest.contains(retired),
            "bin/prebuilt/manifest.json should not contain {retired:?}"
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
}
