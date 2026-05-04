#![allow(dead_code)]

use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub const DARWIN_ARM64_TARGET: &str = "darwin-arm64";
pub const WINDOWS_X64_TARGET: &str = "windows-x64";
pub const DARWIN_ARM64_BINARY_NAME: &str = "featureforge";
pub const WINDOWS_X64_BINARY_NAME: &str = "featureforge.exe";
pub const DARWIN_ARM64_BINARY_REL: &str = "bin/prebuilt/darwin-arm64/featureforge";
pub const DARWIN_ARM64_CHECKSUM_REL: &str = "bin/prebuilt/darwin-arm64/featureforge.sha256";
pub const WINDOWS_X64_BINARY_REL: &str = "bin/prebuilt/windows-x64/featureforge.exe";
pub const WINDOWS_X64_CHECKSUM_REL: &str = "bin/prebuilt/windows-x64/featureforge.exe.sha256";

pub struct PrebuiltManifestEntry<'a> {
    pub target: &'a str,
    pub binary_path: &'a str,
    pub checksum_path: &'a str,
    pub binary_sha256: Option<String>,
    pub source_fingerprint: Option<String>,
    pub source_fingerprint_algorithm: Option<&'a str>,
    pub source_fingerprint_path_count: Option<usize>,
}

pub fn canonical_prebuilt_entries() -> [PrebuiltManifestEntry<'static>; 2] {
    [
        PrebuiltManifestEntry {
            target: DARWIN_ARM64_TARGET,
            binary_path: DARWIN_ARM64_BINARY_REL,
            checksum_path: DARWIN_ARM64_CHECKSUM_REL,
            binary_sha256: None,
            source_fingerprint: None,
            source_fingerprint_algorithm: None,
            source_fingerprint_path_count: None,
        },
        PrebuiltManifestEntry {
            target: WINDOWS_X64_TARGET,
            binary_path: WINDOWS_X64_BINARY_REL,
            checksum_path: WINDOWS_X64_CHECKSUM_REL,
            binary_sha256: None,
            source_fingerprint: None,
            source_fingerprint_algorithm: None,
            source_fingerprint_path_count: None,
        },
    ]
}

pub fn sha256_checksum_line(binary_name: &str, contents: &str) -> String {
    let checksum = format!("{:x}", Sha256::digest(contents.as_bytes()));
    format!("{checksum}  {binary_name}\n")
}

fn sha256_manifest_digest(contents: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(contents.as_bytes()))
}

pub fn write_prebuilt_artifact(
    root: &Path,
    binary_rel: &str,
    checksum_rel: &str,
    binary_contents: &str,
    checksum_contents: &str,
) {
    let binary_path = root.join(binary_rel);
    if let Some(parent) = binary_path.parent() {
        fs::create_dir_all(parent).expect("binary parent should be creatable");
    }
    fs::write(&binary_path, binary_contents).expect("prebuilt runtime should be writable");
    make_executable(&binary_path);

    let checksum_path = root.join(checksum_rel);
    if let Some(parent) = checksum_path.parent() {
        fs::create_dir_all(parent).expect("checksum parent should be creatable");
    }
    fs::write(&checksum_path, checksum_contents).expect("checksum should be writable");
}

pub fn write_prebuilt_manifest(
    root: &Path,
    runtime_revision: &str,
    entries: &[PrebuiltManifestEntry<'_>],
) {
    let manifest_path = root.join("bin/prebuilt/manifest.json");
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent).expect("manifest parent should be creatable");
    }

    let mut manifest_targets = serde_json::Map::new();
    for entry in entries {
        let mut target = serde_json::Map::new();
        target.insert(String::from("binary_path"), json!(entry.binary_path));
        target.insert(String::from("checksum_path"), json!(entry.checksum_path));
        if let Some(binary_sha256) = entry.binary_sha256.as_deref() {
            target.insert(String::from("binary_sha256"), json!(binary_sha256));
        }
        if let Some(source_fingerprint) = entry.source_fingerprint.as_deref() {
            target.insert(
                String::from("source_fingerprint"),
                json!(source_fingerprint),
            );
        }
        if let Some(source_fingerprint_algorithm) = entry.source_fingerprint_algorithm {
            target.insert(
                String::from("source_fingerprint_algorithm"),
                json!(source_fingerprint_algorithm),
            );
        }
        if let Some(source_fingerprint_path_count) = entry.source_fingerprint_path_count {
            target.insert(
                String::from("source_fingerprint_path_count"),
                json!(source_fingerprint_path_count),
            );
        }
        manifest_targets.insert(entry.target.to_owned(), serde_json::Value::Object(target));
    }

    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&json!({
            "runtime_revision": runtime_revision,
            "targets": manifest_targets,
        }))
        .expect("manifest should serialize"),
    )
    .expect("manifest should be writable");
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("path should be executable");
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

pub fn write_canonical_prebuilt_layout(
    root: &Path,
    runtime_revision: &str,
    darwin_binary_contents: &str,
    windows_binary_contents: &str,
) {
    write_prebuilt_artifact(
        root,
        DARWIN_ARM64_BINARY_REL,
        DARWIN_ARM64_CHECKSUM_REL,
        darwin_binary_contents,
        &sha256_checksum_line(DARWIN_ARM64_BINARY_NAME, darwin_binary_contents),
    );
    write_prebuilt_artifact(
        root,
        WINDOWS_X64_BINARY_REL,
        WINDOWS_X64_CHECKSUM_REL,
        windows_binary_contents,
        &sha256_checksum_line(WINDOWS_X64_BINARY_NAME, windows_binary_contents),
    );

    let entries = canonical_prebuilt_entries();
    let entries = [
        PrebuiltManifestEntry {
            target: entries[0].target,
            binary_path: entries[0].binary_path,
            checksum_path: entries[0].checksum_path,
            binary_sha256: Some(sha256_manifest_digest(darwin_binary_contents)),
            source_fingerprint: None,
            source_fingerprint_algorithm: None,
            source_fingerprint_path_count: None,
        },
        PrebuiltManifestEntry {
            target: entries[1].target,
            binary_path: entries[1].binary_path,
            checksum_path: entries[1].checksum_path,
            binary_sha256: Some(sha256_manifest_digest(windows_binary_contents)),
            source_fingerprint: None,
            source_fingerprint_algorithm: None,
            source_fingerprint_path_count: None,
        },
    ];
    write_prebuilt_manifest(root, runtime_revision, &entries);
}
