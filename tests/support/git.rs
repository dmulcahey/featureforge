use featureforge::expect_ext::ExpectValueExt as _;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::OnceLock;

const TEST_GIT_USER_NAME: &str = "FeatureForge Test";
const TEST_GIT_USER_EMAIL: &str = "featureforge-tests@example.com";

fn run_git(repo: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new("git");
    command.current_dir(repo).args(args);
    configure_hermetic_git_env(&mut command);
    run_checked(command, context)
}

fn run_git_with_env(repo: &Path, args: &[&str], envs: &[(&str, &str)], context: &str) -> Output {
    let mut command = Command::new("git");
    command.current_dir(repo).args(args);
    configure_hermetic_git_env(&mut command);
    for (key, value) in envs {
        command.env(key, value);
    }
    run_checked(command, context)
}

fn init_repo(repo: &Path, context: &str) {
    gix::init(repo).unwrap_or_else(|error| {
        featureforge::abort!("{context} should initialize a git repo: {error}")
    });
}

fn add(repo: &Path, paths: &[&str], context: &str) -> Output {
    let mut args = vec!["add"];
    args.extend(paths.iter().copied());
    run_git(repo, &args, context)
}

fn commit(repo: &Path, message: &str, context: &str) -> Output {
    run_git_with_env(
        repo,
        &["commit", "--no-gpg-sign", "--no-verify", "-m", message],
        &[
            ("GIT_AUTHOR_NAME", TEST_GIT_USER_NAME),
            ("GIT_AUTHOR_EMAIL", TEST_GIT_USER_EMAIL),
            ("GIT_COMMITTER_NAME", TEST_GIT_USER_NAME),
            ("GIT_COMMITTER_EMAIL", TEST_GIT_USER_EMAIL),
        ],
        context,
    )
}

fn configure_hermetic_git_env(command: &mut Command) {
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", hermetic_git_global_config_path())
        .env_remove("GIT_CONFIG_SYSTEM");
}

fn hermetic_git_global_config_path() -> &'static Path {
    static GIT_GLOBAL_CONFIG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
    GIT_GLOBAL_CONFIG_PATH
        .get_or_init(|| {
            let path = std::env::temp_dir().join(format!(
                "featureforge-test-global-gitconfig-{}",
                std::process::id()
            ));
            let tmp_path = path.with_extension("tmp");
            let contents = "[user]\n\tname = FeatureForge Test\n\temail = featureforge-tests@example.com\n[init]\n\tdefaultBranch = main\n";
            fs::write(&tmp_path, contents)
                .expect_or_abort("git helper should write hermetic global git config");
            fs::rename(&tmp_path, &path)
                .expect_or_abort("git helper should atomically install hermetic git config");
            path
        })
        .as_path()
}

pub fn init_repo_with_initial_commit(repo: &Path, readme_contents: &str, commit_message: &str) {
    fs::create_dir_all(repo).expect_or_abort("repo directory should be creatable");
    init_repo(repo, "git init");
    fs::write(repo.join("README.md"), readme_contents).expect_or_abort("README should be writable");
    add(repo, &["README.md"], "git add README");
    commit(repo, commit_message, "git commit init");
}

fn run(mut command: Command, context: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| featureforge::abort!("{context} should run: {error}"))
}

fn run_checked(command: Command, context: &str) -> Output {
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
