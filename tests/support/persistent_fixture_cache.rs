use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

const CACHE_SCHEMA_VERSION: &str = "schema-v3";

#[derive(Clone)]
pub struct CachedRepoStateTemplate {
    pub repo_root: PathBuf,
    pub state_root: PathBuf,
}

pub fn cached_repo_state_template_from_source(
    namespace: &str,
    cache_key: &str,
    input_version: &str,
    build_source: impl FnOnce() -> CachedRepoStateTemplate,
) -> CachedRepoStateTemplate {
    let cache_dir = cache_root()
        .join(sanitize_component(namespace))
        .join(sanitize_component(input_version))
        .join(sanitize_component(cache_key));
    let ready_path = cache_dir.join("READY");
    if ready_path.is_file() {
        return cached_template(&cache_dir);
    }

    fs::create_dir_all(&cache_dir).unwrap_or_else(|error| {
        panic!(
            "persistent fixture cache directory should be creatable at {}: {error}",
            cache_dir.display()
        )
    });
    let lock_dir = cache_dir.join("LOCK");
    let mut build_source = Some(build_source);
    let started = Instant::now();
    loop {
        if ready_path.is_file() {
            return cached_template(&cache_dir);
        }
        match fs::create_dir(&lock_dir) {
            Ok(()) => {
                let _guard = CacheLock {
                    lock_dir: lock_dir.clone(),
                };
                if ready_path.is_file() {
                    return cached_template(&cache_dir);
                }
                let source = build_source
                    .take()
                    .expect("persistent fixture source builder should run at most once")(
                );
                publish_cache_template(&cache_dir, &ready_path, &source);
                return cached_template(&cache_dir);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                assert!(
                    started.elapsed() <= Duration::from_secs(180),
                    "timed out waiting for persistent fixture cache lock at {}",
                    lock_dir.display()
                );
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                panic!(
                    "persistent fixture cache lock should be creatable at {}: {error}",
                    lock_dir.display()
                );
            }
        }
    }
}

fn cache_root() -> PathBuf {
    fixture_cache_root_base()
        .join("test-fixtures")
        .join(CACHE_SCHEMA_VERSION)
        .join(workspace_fixture_source_stamp())
}

fn fixture_cache_root_base() -> PathBuf {
    std::env::var_os("FEATUREFORGE_TEST_FIXTURE_CACHE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(".featureforge")
                .join("test-cache")
        })
}

fn workspace_fixture_source_stamp() -> String {
    static STAMP: OnceLock<String> = OnceLock::new();
    STAMP
        .get_or_init(|| {
            let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let mut hasher = StableFnv64::default();
            for rel in fixture_source_roots() {
                hash_fixture_source_root(&manifest_dir, rel, &mut hasher);
            }
            format!("source-{:016x}", hasher.finish())
        })
        .clone()
}

fn fixture_source_roots() -> &'static [&'static str] {
    &[
        "Cargo.lock",
        "Cargo.toml",
        "src",
        "tests/support/persistent_fixture_cache.rs",
        "tests/workflow_runtime.rs",
        "tests/internal_workflow_runtime.rs",
        "tests/workflow_shell_smoke.rs",
        "tests/internal_workflow_shell_smoke.rs",
        "tests/fixtures",
        "tests/codex-runtime/fixtures/workflow-artifacts",
    ]
}

fn hash_fixture_source_root(manifest_dir: &Path, rel: &str, hasher: &mut StableFnv64) {
    let path = manifest_dir.join(rel);
    if path.is_file() {
        hash_fixture_source_file(manifest_dir, &path, hasher);
        return;
    }
    if !path.is_dir() {
        hasher.write_str(rel);
        hasher.write_u8(0);
        return;
    }
    for file in fixture_source_files(&path) {
        hash_fixture_source_file(manifest_dir, &file, hasher);
    }
}

fn fixture_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_fixture_source_files(root, &mut files);
    files.sort();
    files
}

fn collect_fixture_source_files(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_fixture_source_files(&entry_path, files);
        } else if file_type.is_file() && fixture_source_file_is_cache_input(&entry_path) {
            files.push(entry_path);
        }
    }
}

fn fixture_source_file_is_cache_input(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension,
                "lock" | "toml" | "rs" | "md" | "json" | "jsonl" | "txt"
            )
        })
}

fn hash_fixture_source_file(manifest_dir: &Path, path: &Path, hasher: &mut StableFnv64) {
    let rel = path.strip_prefix(manifest_dir).unwrap_or(path);
    hasher.write_bytes(rel.to_string_lossy().as_bytes());
    hasher.write_u8(0);
    match fs::read(path) {
        Ok(bytes) => {
            hasher.write_bytes(&bytes);
            hasher.write_u8(0xff);
        }
        Err(error) => {
            hasher.write_bytes(error.to_string().as_bytes());
            hasher.write_u8(0xee);
        }
    }
}

#[derive(Default)]
struct StableFnv64 {
    value: u64,
}

impl StableFnv64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn write_bytes(&mut self, bytes: &[u8]) {
        if self.value == 0 {
            self.value = Self::OFFSET;
        }
        for byte in bytes {
            self.value ^= u64::from(*byte);
            self.value = self.value.wrapping_mul(Self::PRIME);
        }
    }

    fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    fn write_u8(&mut self, value: u8) {
        self.write_bytes(&[value]);
    }

    fn finish(&self) -> u64 {
        if self.value == 0 {
            Self::OFFSET
        } else {
            self.value
        }
    }
}

fn sanitize_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }
    if sanitized.is_empty() {
        String::from("empty")
    } else {
        sanitized
    }
}

fn cached_template(cache_dir: &Path) -> CachedRepoStateTemplate {
    CachedRepoStateTemplate {
        repo_root: cache_dir.join("repo"),
        state_root: cache_dir.join("state"),
    }
}

fn publish_cache_template(cache_dir: &Path, ready_path: &Path, source: &CachedRepoStateTemplate) {
    let staging_dir = cache_dir.join(format!("staging-{}", std::process::id()));
    remove_dir_if_exists(&staging_dir);
    fs::create_dir_all(&staging_dir).unwrap_or_else(|error| {
        panic!(
            "persistent fixture cache staging directory should be creatable at {}: {error}",
            staging_dir.display()
        )
    });
    let staging_repo = staging_dir.join("repo");
    let staging_state = staging_dir.join("state");
    copy_dir_recursive(&source.repo_root, &staging_repo);
    copy_dir_recursive(&source.state_root, &staging_state);

    let cache_repo = cache_dir.join("repo");
    let cache_state = cache_dir.join("state");
    remove_dir_if_exists(&cache_repo);
    remove_dir_if_exists(&cache_state);
    fs::rename(&staging_repo, &cache_repo).unwrap_or_else(|error| {
        panic!(
            "persistent fixture cache repo should move into place at {}: {error}",
            cache_repo.display()
        )
    });
    fs::rename(&staging_state, &cache_state).unwrap_or_else(|error| {
        panic!(
            "persistent fixture cache state should move into place at {}: {error}",
            cache_state.display()
        )
    });
    remove_dir_if_exists(&staging_dir);
    fs::write(ready_path, "ready\n").unwrap_or_else(|error| {
        panic!(
            "persistent fixture cache readiness marker should be writable at {}: {error}",
            ready_path.display()
        )
    });
}

fn remove_dir_if_exists(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => panic!("failed to remove {}: {error}", path.display()),
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    if clone_dir_contents(source, destination).is_ok() {
        return;
    }
    copy_dir_recursive_fallback(source, destination);
}

#[cfg(target_os = "macos")]
fn clone_dir_contents(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| {
        format!(
            "failed to create destination directory `{}`: {error}",
            destination.display()
        )
    })?;
    let source_contents = source.join(".");
    let output = Command::new("cp")
        .arg("-cR")
        .arg(&source_contents)
        .arg(destination)
        .output()
        .map_err(|error| format!("failed to run cp -cR for `{}`: {error}", source.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "cp -cR failed for `{}` -> `{}`: {}",
            source.display(),
            destination.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn clone_dir_contents(_source: &Path, _destination: &Path) -> Result<(), String> {
    Err(String::from("clone copy is unavailable on this platform"))
}

fn copy_dir_recursive_fallback(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("destination directory should be creatable");
    for entry in fs::read_dir(source).expect("source directory should be readable") {
        let entry = entry.expect("source entry should be readable");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .expect("source entry type should be readable");
        if file_type.is_dir() {
            copy_dir_recursive_fallback(&source_path, &destination_path);
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)
                .unwrap_or_else(|error| panic!("failed to copy {:?}: {error}", source_path));
        }
    }
}

struct CacheLock {
    lock_dir: PathBuf,
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.lock_dir);
    }
}
