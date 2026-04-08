use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const TEST_GIT_USER_NAME: &str = "FeatureForge Test";
const TEST_GIT_USER_EMAIL: &str = "featureforge-tests@example.com";

static TEMPLATE_REPO_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn run_checked(mut command: Command, context: &str) {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"));
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn initialize_template_repo(repo: &Path) {
    run_checked(
        {
            let mut command = Command::new("git");
            command.arg("init").current_dir(repo);
            command
        },
        "git init template repo",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["config", "user.name", TEST_GIT_USER_NAME])
                .current_dir(repo);
            command
        },
        "git config user.name template repo",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["config", "user.email", TEST_GIT_USER_EMAIL])
                .current_dir(repo);
            command
        },
        "git config user.email template repo",
    );
    fs::write(repo.join("README.md"), "# fixture\n")
        .expect("template repository README should be writable");
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["add", "README.md"]).current_dir(repo);
            command
        },
        "git add README in template repo",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["commit", "-m", "init"]).current_dir(repo);
            command
        },
        "git commit init in template repo",
    );
}

fn template_repo_root() -> &'static Path {
    TEMPLATE_REPO_ROOT
        .get_or_init(|| {
            let tempdir = tempfile::Builder::new()
                .prefix("featureforge-test-repo-template-")
                .tempdir()
                .expect("template tempdir should exist");
            let path = tempdir.path().to_path_buf();
            std::mem::forget(tempdir);
            initialize_template_repo(&path);
            path
        })
        .as_path()
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("destination directory should be creatable");
    for entry in fs::read_dir(source).expect("source directory should be readable") {
        let entry = entry.expect("directory entry should be readable");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .expect("directory entry type should be readable");
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path);
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)
                .unwrap_or_else(|error| panic!("failed to copy {:?}: {error}", source_path));
        }
    }
}

pub fn populate_repo_from_template(destination: &Path) {
    if !destination.exists() {
        fs::create_dir_all(destination).expect("destination should be creatable");
    }
    let mut entries = fs::read_dir(destination).expect("destination directory should be readable");
    assert!(
        entries.next().is_none(),
        "destination repository path should be empty before template copy: {}",
        destination.display()
    );
    copy_dir_recursive(template_repo_root(), destination);
}
