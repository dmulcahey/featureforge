use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn legacy_powershell_bash_compat_surfaces_are_removed() {
    let root = repo_root();
    for relative in [
        "compat/bash/superpowers",
        "compat/powershell/superpowers.ps1",
        "bin/superpowers-pwsh-common.ps1",
        "bin/superpowers-runtime-common.sh",
    ] {
        assert!(
            !root.join(relative).exists(),
            "legacy PowerShell or bash compat surface should be removed: {relative}"
        );
    }
}
