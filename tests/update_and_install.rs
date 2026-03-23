use assert_cmd::cargo::CommandCargoExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn update_check_helper_path() -> PathBuf {
    repo_root().join("bin/superpowers-update-check")
}

fn repo_safety_helper_path() -> PathBuf {
    repo_root().join("bin/superpowers-repo-safety")
}

fn run(mut command: Command, context: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"))
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

fn parse_json(output: &Output, context: &str) -> Value {
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    fs::write(path, contents).expect("file should be writable");
}

fn run_shell_update_check(
    state_dir: &Path,
    install_dir: &Path,
    remote_url: &str,
    args: &[&str],
    context: &str,
) -> Output {
    let mut command = Command::new(update_check_helper_path());
    command
        .env("SUPERPOWERS_STATE_DIR", state_dir)
        .env("SUPERPOWERS_DIR", install_dir)
        .env("SUPERPOWERS_REMOTE_URL", remote_url)
        .args(args);
    run(command, context)
}

fn run_rust_superpowers(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Output {
    let mut command =
        Command::cargo_bin("superpowers").expect("superpowers cargo binary should be available");
    if let Some(repo) = repo {
        command.current_dir(repo);
    }
    if let Some(state_dir) = state_dir {
        command.env("SUPERPOWERS_STATE_DIR", state_dir);
    }
    if let Some(home_dir) = home_dir {
        command.env("HOME", home_dir);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.args(args);
    run(command, context)
}

fn init_repo(name: &str, branch: &str, remote_url: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    let mut git_init = Command::new("git");
    git_init.arg("init").current_dir(repo);
    run_checked(git_init, "git init");

    let mut git_config_name = Command::new("git");
    git_config_name
        .args(["config", "user.name", "Superpowers Test"])
        .current_dir(repo);
    run_checked(git_config_name, "git config user.name");

    let mut git_config_email = Command::new("git");
    git_config_email
        .args(["config", "user.email", "superpowers-tests@example.com"])
        .current_dir(repo);
    run_checked(git_config_email, "git config user.email");

    write_file(&repo.join("README.md"), &format!("# {name}\n"));

    let mut git_add = Command::new("git");
    git_add.args(["add", "README.md"]).current_dir(repo);
    run_checked(git_add, "git add README");

    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", "init"]).current_dir(repo);
    run_checked(git_commit, "git commit init");

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", branch])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout branch");

    let mut git_remote_add = Command::new("git");
    git_remote_add
        .args(["remote", "add", "origin", remote_url])
        .current_dir(repo);
    run_checked(git_remote_add, "git remote add origin");

    (repo_dir, state_dir)
}

fn current_user_name() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| String::from("user"))
}

fn repo_slug_from_remote(remote_url: &str) -> String {
    remote_url
        .trim_end_matches(".git")
        .rsplit('/')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("-")
}

fn task_hash(stage: &str, task_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(stage.as_bytes());
    hasher.update(b"\n");
    hasher.update(task_id.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_owned()
}

fn canonical_approval_path(
    state_dir: &Path,
    remote_url: &str,
    branch: &str,
    stage: &str,
    task_id: &str,
) -> PathBuf {
    state_dir
        .join("repo-safety")
        .join("approvals")
        .join(repo_slug_from_remote(remote_url))
        .join(format!("{}-{}", current_user_name(), branch))
        .join(format!("{}.json", task_hash(stage, task_id)))
}

fn prepare_install_dir(version: &str) -> TempDir {
    let install_dir = TempDir::new().expect("install tempdir should exist");
    let bin_dir = install_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        repo_root().join("bin/superpowers-config"),
        bin_dir.join("superpowers-config"),
    )
    .expect("config helper symlink should be creatable");
    #[cfg(not(unix))]
    fs::copy(
        repo_root().join("bin/superpowers-config"),
        bin_dir.join("superpowers-config"),
    )
    .expect("config helper should copy on non-unix hosts");
    write_file(&install_dir.path().join("VERSION"), &format!("{version}\n"));
    install_dir
}

fn create_source_install_repo(dir: &Path) {
    let mut git_init = Command::new("git");
    git_init.arg("init").current_dir(dir);
    run_checked(git_init, "source git init");

    let mut git_config_name = Command::new("git");
    git_config_name
        .args(["config", "user.name", "Superpowers Test"])
        .current_dir(dir);
    run_checked(git_config_name, "source git config user.name");

    let mut git_config_email = Command::new("git");
    git_config_email
        .args(["config", "user.email", "superpowers-tests@example.com"])
        .current_dir(dir);
    run_checked(git_config_email, "source git config user.email");

    write_file(
        &dir.join("bin/superpowers-update-check"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    write_file(
        &dir.join("bin/superpowers-config"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            dir.join("bin/superpowers-update-check"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("update-check helper should be executable");
        fs::set_permissions(
            dir.join("bin/superpowers-config"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("config helper should be executable");
    }
    write_file(&dir.join("agents/code-reviewer.md"), "# reviewer\n");
    write_file(
        &dir.join(".codex/agents/code-reviewer.toml"),
        "name = \"code-reviewer\"\ndescription = \"reviewer\"\ndeveloper_instructions = \"\"\"review\"\"\"",
    );
    write_file(&dir.join("VERSION"), "1.0.0\n");

    let mut git_add = Command::new("git");
    git_add
        .args([
            "add",
            "VERSION",
            "bin/superpowers-update-check",
            "bin/superpowers-config",
            "agents/code-reviewer.md",
            ".codex/agents/code-reviewer.toml",
        ])
        .current_dir(dir);
    run_checked(git_add, "source git add");

    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", "init"]).current_dir(dir);
    run_checked(git_commit, "source git commit");
}

fn make_legacy_install(dir: &Path, version: &str) {
    fs::create_dir_all(dir).expect("legacy install dir should exist");
    let mut git_init = Command::new("git");
    git_init.arg("init").current_dir(dir);
    run_checked(git_init, "legacy install git init");

    let mut git_config_name = Command::new("git");
    git_config_name
        .args(["config", "user.name", "Superpowers Test"])
        .current_dir(dir);
    run_checked(git_config_name, "legacy install git config user.name");

    let mut git_config_email = Command::new("git");
    git_config_email
        .args(["config", "user.email", "superpowers-tests@example.com"])
        .current_dir(dir);
    run_checked(git_config_email, "legacy install git config user.email");

    write_file(
        &dir.join("bin/superpowers-update-check"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    write_file(
        &dir.join("bin/superpowers-config"),
        "#!/usr/bin/env bash\nexit 0\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            dir.join("bin/superpowers-update-check"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("legacy update-check helper should be executable");
        fs::set_permissions(
            dir.join("bin/superpowers-config"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("legacy config helper should be executable");
    }
    write_file(&dir.join("agents/code-reviewer.md"), "# reviewer\n");
    write_file(
        &dir.join(".codex/agents/code-reviewer.toml"),
        "name = \"code-reviewer\"\ndescription = \"reviewer\"\ndeveloper_instructions = \"\"\"review\"\"\"",
    );
    write_file(&dir.join("VERSION"), &format!("{version}\n"));

    let mut git_add = Command::new("git");
    git_add
        .args([
            "add",
            "VERSION",
            "bin/superpowers-update-check",
            "bin/superpowers-config",
            "agents/code-reviewer.md",
            ".codex/agents/code-reviewer.toml",
        ])
        .current_dir(dir);
    run_checked(git_add, "legacy install git add");

    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", "init"]).current_dir(dir);
    run_checked(git_commit, "legacy install git commit");
}

#[test]
fn canonical_update_check_preserves_status_line_and_writes_canonical_state() {
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let install_dir = prepare_install_dir("5.1.0");
    let remote_file = TempDir::new().expect("remote tempdir should exist");
    let remote_version_path = remote_file.path().join("VERSION");
    write_file(&remote_version_path, "5.2.0\n");
    let remote_url = format!("file://{}", remote_version_path.display());

    let helper_output = run_shell_update_check(
        state_dir.path(),
        install_dir.path(),
        &remote_url,
        &[],
        "helper update-check",
    );
    assert_eq!(
        String::from_utf8_lossy(&helper_output.stdout).trim(),
        "UPGRADE_AVAILABLE 5.1.0 5.2.0"
    );

    let rust_output = run_rust_superpowers(
        None,
        Some(state_dir.path()),
        None,
        &[
            (
                "SUPERPOWERS_DIR",
                install_dir.path().to_string_lossy().as_ref(),
            ),
            ("SUPERPOWERS_REMOTE_URL", &remote_url),
        ],
        &["update-check"],
        "canonical update-check",
    );
    assert!(
        rust_output.status.success(),
        "canonical update-check should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        rust_output.status,
        String::from_utf8_lossy(&rust_output.stdout),
        String::from_utf8_lossy(&rust_output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&rust_output.stdout).trim(),
        "UPGRADE_AVAILABLE 5.1.0 5.2.0"
    );

    let canonical_cache = state_dir.path().join("update-check/last-update-check");
    assert_eq!(
        fs::read_to_string(&canonical_cache).expect("canonical update cache should exist"),
        "UPGRADE_AVAILABLE 5.1.0 5.2.0\n"
    );
    assert!(
        !state_dir.path().join("last-update-check").exists(),
        "canonical update-check should not keep writing the legacy root cache path"
    );
}

#[test]
fn pending_non_rebuildable_state_blocks_mutations_but_allows_read_only_inspection() {
    let remote_url = "https://example.com/acme/pending-migration.git";
    let (repo_dir, state_dir) = init_repo("pending-migration", "main", remote_url);
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_file(&state.join("config.yaml"), "update_check: false\n");

    let config_get = run_rust_superpowers(
        None,
        Some(state),
        None,
        &[],
        &["config", "get", "update_check"],
        "config get with pending migration",
    );
    assert!(
        config_get.status.success(),
        "config get should remain readable during pending migration, got {:?}\nstdout:\n{}\nstderr:\n{}",
        config_get.status,
        String::from_utf8_lossy(&config_get.stdout),
        String::from_utf8_lossy(&config_get.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&config_get.stdout).trim(), "false");
    assert!(
        String::from_utf8_lossy(&config_get.stderr).contains("PendingMigration"),
        "config get should emit an explicit pending-migration warning on stderr"
    );

    let config_set = run_rust_superpowers(
        None,
        Some(state),
        None,
        &[],
        &["config", "set", "update_check", "true"],
        "config set with pending migration",
    );
    assert!(
        !config_set.status.success(),
        "config set should fail closed until install migrate runs"
    );
    assert!(
        String::from_utf8_lossy(&config_set.stderr).contains("PendingMigration"),
        "config set failure should direct the user to install migrate"
    );

    let helper_approve = {
        let mut command = Command::new(repo_safety_helper_path());
        command
            .current_dir(repo)
            .env("SUPERPOWERS_STATE_DIR", state)
            .args([
                "approve",
                "--stage",
                "superpowers:executing-plans",
                "--task-id",
                "task-7",
                "--reason",
                "User explicitly approved this write.",
                "--path",
                "docs/superpowers/specs/example.md",
                "--write-target",
                "execution-task-slice",
            ]);
        run(command, "helper repo-safety approve for pending migration")
    };
    let helper_json = parse_json(
        &helper_approve,
        "helper repo-safety approve for pending migration",
    );
    assert!(
        helper_json["outcome"].as_str().is_some(),
        "helper repo-safety approve should emit an outcome field"
    );

    let repo_check = run_rust_superpowers(
        Some(repo),
        Some(state),
        None,
        &[],
        &[
            "repo-safety",
            "check",
            "--intent",
            "write",
            "--stage",
            "superpowers:executing-plans",
            "--task-id",
            "task-7",
            "--path",
            "docs/superpowers/specs/example.md",
            "--write-target",
            "execution-task-slice",
        ],
        "repo-safety check with pending migration",
    );
    let repo_check_json = parse_json(&repo_check, "repo-safety check with pending migration");
    assert_eq!(
        repo_check_json["outcome"],
        Value::String(String::from("allowed"))
    );
    assert!(
        String::from_utf8_lossy(&repo_check.stderr).contains("PendingMigration"),
        "repo-safety check should warn when legacy approvals still need explicit migration"
    );

    let repo_approve = run_rust_superpowers(
        Some(repo),
        Some(state),
        None,
        &[],
        &[
            "repo-safety",
            "approve",
            "--stage",
            "superpowers:executing-plans",
            "--task-id",
            "task-7b",
            "--reason",
            "Second approval should block until migration.",
            "--path",
            "docs/superpowers/specs/example.md",
            "--write-target",
            "execution-task-slice",
        ],
        "repo-safety approve with pending migration",
    );
    assert!(
        !repo_approve.status.success(),
        "repo-safety approve should fail closed until install migrate rewrites approvals"
    );
    assert!(
        String::from_utf8_lossy(&repo_approve.stderr).contains("PendingMigration"),
        "repo-safety approve failure should name the pending migration gate"
    );
}

#[test]
fn install_migrate_rewrites_config_and_legacy_approvals_with_backup_reporting() {
    let home_dir = TempDir::new().expect("home tempdir should exist");
    let source_repo = home_dir.path().join("source");
    fs::create_dir_all(&source_repo).expect("source repo dir should exist");
    create_source_install_repo(&source_repo);

    let shared_root = home_dir.path().join(".superpowers/install");
    let codex_root = home_dir.path().join(".codex/superpowers");
    let copilot_root = home_dir.path().join(".copilot/superpowers");
    fs::create_dir_all(codex_root.parent().expect("codex parent"))
        .expect("codex parent should exist");
    make_legacy_install(&codex_root, "4.9.0");

    let state_dir = home_dir.path().join(".superpowers");
    write_file(&state_dir.join("config.yaml"), "update_check: false\n");

    let remote_url = "https://example.com/acme/install-migrate.git";
    let (repo_dir, _repo_state_dir) = init_repo("install-migrate", "main", remote_url);
    let repo = repo_dir.path();
    let mut helper_approve = Command::new(repo_safety_helper_path());
    helper_approve
        .current_dir(repo)
        .env("SUPERPOWERS_STATE_DIR", &state_dir)
        .args([
            "approve",
            "--stage",
            "superpowers:executing-plans",
            "--task-id",
            "task-7",
            "--reason",
            "User explicitly approved this write.",
            "--path",
            "docs/superpowers/specs/example.md",
            "--write-target",
            "execution-task-slice",
        ]);
    let helper_output = run_checked(
        helper_approve,
        "helper repo-safety approve for install-migrate fixtures",
    );
    let helper_json = parse_json(
        &helper_output,
        "helper repo-safety approve for install-migrate fixtures",
    );
    assert!(
        helper_json["outcome"].as_str().is_some(),
        "helper repo-safety approve should emit an outcome field"
    );

    let migrate_output = run_rust_superpowers(
        None,
        Some(&state_dir),
        Some(home_dir.path()),
        &[
            (
                "SUPERPOWERS_SHARED_ROOT",
                shared_root.to_string_lossy().as_ref(),
            ),
            (
                "SUPERPOWERS_CODEX_ROOT",
                codex_root.to_string_lossy().as_ref(),
            ),
            (
                "SUPERPOWERS_COPILOT_ROOT",
                copilot_root.to_string_lossy().as_ref(),
            ),
            (
                "SUPERPOWERS_REPO_URL",
                source_repo.to_string_lossy().as_ref(),
            ),
            ("SUPERPOWERS_MIGRATE_STAMP", "20260323-140000"),
        ],
        &["install", "migrate"],
        "install migrate with config and approval migration",
    );
    assert!(
        migrate_output.status.success(),
        "install migrate should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        migrate_output.status,
        String::from_utf8_lossy(&migrate_output.stdout),
        String::from_utf8_lossy(&migrate_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&migrate_output.stdout);
    assert!(
        stdout.contains("Migrated config"),
        "install migrate should report config migration"
    );
    assert!(
        stdout.contains("Migrated repo-safety approval"),
        "install migrate should report migrated approval records"
    );
    assert!(
        stdout.contains("Shared install ready"),
        "install migrate should still report the shared-install result"
    );

    let canonical_config = state_dir.join("config/config.yaml");
    assert_eq!(
        fs::read_to_string(&canonical_config).expect("canonical config should exist"),
        "update_check: false\n"
    );
    assert!(
        state_dir.join("config.yaml.bak").exists(),
        "install migrate should back up the legacy config before rewriting it"
    );
    let canonical_approval = canonical_approval_path(
        &state_dir,
        remote_url,
        "main",
        "superpowers:executing-plans",
        "task-7",
    );
    assert!(
        canonical_approval.exists(),
        "install migrate should rewrite legacy approval state into the canonical subtree"
    );
}
