use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use featureforge::execution::runtime_provenance::{StateDirKind, classify_state_dir_kind_for_path};

#[allow(dead_code)]
pub const WORKSPACE_RUNTIME_LIVE_STATE_TEST_ALLOW_ENV: &str =
    "FEATUREFORGE_TEST_ALLOW_WORKSPACE_RUNTIME_LIVE_STATE";

#[allow(dead_code)]
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn run(mut command: Command, context: &str) -> Output {
    configure_git_command_env(&mut command);
    command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"))
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
                .expect("test process helper should write hermetic git global config");
            fs::rename(&tmp_path, &path)
                .expect("test process helper should atomically install hermetic git config");
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

#[allow(dead_code)]
pub fn assert_workspace_runtime_uses_temp_state(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    allow_live_state: bool,
    context: &str,
) {
    if !is_workspace_runtime_binary() || repo.is_none() {
        return;
    }

    let Some(state_dir) = state_dir else {
        if allow_live_state {
            return;
        }
        panic!(
            "{context}: workspace runtime shell-smoke commands must set FEATUREFORGE_STATE_DIR to an isolated temp state directory"
        );
    };

    if allow_live_state {
        return;
    }

    let state_dir_kind = classify_state_dir_kind_for_path(state_dir, home_dir, true);
    if state_dir_kind == StateDirKind::Live {
        panic!(
            "{context}: workspace runtime shell-smoke commands must not use live ~/.featureforge state: {}",
            state_dir.display()
        );
    }
    if state_dir_kind != StateDirKind::Temp {
        panic!(
            "{context}: workspace runtime shell-smoke commands must use temp/fixture FEATUREFORGE_STATE_DIR, got {}",
            state_dir.display()
        );
    }
}

fn is_workspace_runtime_binary() -> bool {
    let binary = canonicalize_or_self(Path::new(env!("CARGO_BIN_EXE_featureforge")));
    let root = canonicalize_or_self(Path::new(env!("CARGO_MANIFEST_DIR")));
    !binary.as_os_str().is_empty() && !root.as_os_str().is_empty() && binary.starts_with(root)
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
