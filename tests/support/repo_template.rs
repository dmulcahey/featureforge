use featureforge::expect_ext::ExpectValueExt as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[path = "git.rs"]
mod git_support;

static TEMPLATE_REPO_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn initialize_template_repo(repo: &Path) {
    git_support::init_repo_with_initial_commit(repo, "# fixture\n", "init");
}

fn template_repo_root() -> &'static Path {
    TEMPLATE_REPO_ROOT
        .get_or_init(|| {
            let tempdir = tempfile::Builder::new()
                .prefix("featureforge-test-repo-template-")
                .tempdir()
                .expect_or_abort("template tempdir should exist");
            let path = tempdir.path().to_path_buf();
            std::mem::forget(tempdir);
            initialize_template_repo(&path);
            path
        })
        .as_path()
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect_or_abort("destination directory should be creatable");
    for entry in fs::read_dir(source).expect_or_abort("source directory should be readable") {
        let entry = entry.expect_or_abort("directory entry should be readable");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .expect_or_abort("directory entry type should be readable");
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path);
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).unwrap_or_else(|error| {
                featureforge::abort!("failed to copy {source_path:?}: {error}")
            });
        }
    }
}

pub fn populate_repo_from_template(destination: &Path) {
    if !destination.exists() {
        fs::create_dir_all(destination).expect_or_abort("destination should be creatable");
    }
    let mut entries =
        fs::read_dir(destination).expect_or_abort("destination directory should be readable");
    assert!(
        entries.next().is_none(),
        "destination repository path should be empty before template copy: {}",
        destination.display()
    );
    copy_dir_recursive(template_repo_root(), destination);
}
