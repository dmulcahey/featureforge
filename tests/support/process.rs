use featureforge::expect_ext::ExpectValueExt as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

#[allow(dead_code)]
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn run(mut command: Command, context: &str) -> Output {
    configure_git_command_env(&mut command);
    command
        .output()
        .unwrap_or_else(|error| featureforge::abort!("{context} should run: {error}"))
}

fn configure_git_command_env(command: &mut Command) {
    let program = command.get_program();
    if !is_git_program(program) {
        return;
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env("GIT_CONFIG_GLOBAL", hermetic_git_global_config_path());
}

fn is_git_program(program: &std::ffi::OsStr) -> bool {
    let Some(file_name) = Path::new(program)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
    else {
        return false;
    };
    matches!(file_name, "git" | "git.exe")
}

fn hermetic_git_global_config_path() -> &'static Path {
    static GIT_GLOBAL_CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
    GIT_GLOBAL_CONFIG_PATH
        .get_or_init(|| {
            let path = std::env::temp_dir().join(format!(
                "featureforge-test-global-gitconfig-{}",
                std::process::id()
            ));
            let tmp_path = path.with_extension("tmp");
            let contents = "[user]\n\tname = FeatureForge Test\n\temail = featureforge-tests@example.com\n[init]\n\tdefaultBranch = main\n";
            fs::write(&tmp_path, contents)
                .expect_or_abort("test process helper should write hermetic git global config");
            fs::rename(&tmp_path, &path)
                .expect_or_abort("test process helper should atomically install hermetic git config");
            path
        })
        .as_path()
}

#[allow(dead_code)]
pub fn run_checked(command: Command, context: &str) -> Output {
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
