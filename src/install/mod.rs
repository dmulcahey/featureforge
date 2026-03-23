use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::install::InstallMigrateArgs;
use crate::config;
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::repo_safety;

#[derive(Debug, Clone)]
struct InstallPaths {
    state_dir: PathBuf,
    shared_root: PathBuf,
    codex_root: PathBuf,
    copilot_root: PathBuf,
    source_repo: PathBuf,
    stamp: String,
}

pub fn migrate(_: &InstallMigrateArgs) -> Result<String, DiagnosticError> {
    let paths = discover_paths()?;
    let mut lines = Vec::new();

    let legacy_roots = legacy_roots(&paths);
    let selected_root = if is_valid_install(&paths.shared_root) {
        lines.push(format!(
            "Using existing shared install at {}",
            paths.shared_root.display()
        ));
        paths.shared_root.clone()
    } else {
        select_or_create_shared_root(&paths, &legacy_roots, &mut lines)?
    };

    for legacy_root in [&paths.codex_root, &paths.copilot_root] {
        rewire_legacy_root(legacy_root, &selected_root, &paths.stamp, &mut lines)?;
    }

    migrate_non_rebuildable_state(&paths.state_dir, &mut lines)?;
    lines.push(format!(
        "Shared install ready at {}",
        selected_root.display()
    ));
    lines.extend(print_next_steps(&selected_root));

    Ok(format!("{}\n", lines.join("\n")))
}

fn discover_paths() -> Result<InstallPaths, DiagnosticError> {
    let home_dir = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let state_dir = env::var_os("SUPERPOWERS_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir.join(".superpowers"));
    let shared_root = env::var_os("SUPERPOWERS_SHARED_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.join("install"));
    let codex_root = env::var_os("SUPERPOWERS_CODEX_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir.join(".codex").join("superpowers"));
    let copilot_root = env::var_os("SUPERPOWERS_COPILOT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir.join(".copilot").join("superpowers"));
    let source_repo = resolve_local_source_repo(
        env::var("SUPERPOWERS_REPO_URL")
            .unwrap_or_else(|_| String::from("https://github.com/dmulcahey/superpowers.git")),
    )?;
    let stamp = env::var("SUPERPOWERS_MIGRATE_STAMP").unwrap_or_else(|_| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{now}")
    });

    Ok(InstallPaths {
        state_dir,
        shared_root,
        codex_root,
        copilot_root,
        source_repo,
        stamp,
    })
}

fn resolve_local_source_repo(raw: String) -> Result<PathBuf, DiagnosticError> {
    let candidate = if let Some(path) = raw.strip_prefix("file://") {
        PathBuf::from(path)
    } else {
        PathBuf::from(raw)
    };
    if candidate.exists() {
        Ok(candidate)
    } else {
        Err(DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            "Install migration requires a local source repo path or file:// URL for this cutover.",
        ))
    }
}

fn legacy_roots(paths: &InstallPaths) -> Vec<PathBuf> {
    [&paths.codex_root, &paths.copilot_root]
        .into_iter()
        .filter(|path| path.exists() || path.is_symlink())
        .cloned()
        .collect()
}

fn select_or_create_shared_root(
    paths: &InstallPaths,
    legacy_roots: &[PathBuf],
    lines: &mut Vec<String>,
) -> Result<PathBuf, DiagnosticError> {
    let valid_legacy_roots = legacy_roots
        .iter()
        .filter(|path| is_valid_install(path))
        .cloned()
        .collect::<Vec<_>>();

    let selected_root = match valid_legacy_roots.as_slice() {
        [] => {
            if paths.shared_root.exists() || paths.shared_root.is_symlink() {
                let backup_path = backup_path(&paths.shared_root, &paths.stamp);
                move_path(&paths.shared_root, &backup_path)?;
                lines.push(format!(
                    "Backed up invalid shared install at {} -> {}",
                    paths.shared_root.display(),
                    backup_path.display()
                ));
            }
            clone_local_source(&paths.source_repo, &paths.shared_root)?;
            lines.push(format!(
                "Cloned shared install to {}",
                paths.shared_root.display()
            ));
            paths.shared_root.clone()
        }
        [root] => adopt_legacy_root(root, &paths.shared_root, &paths.stamp, lines)?,
        [left, right] => {
            let left_ts = head_commit_timestamp(left)?;
            let right_ts = head_commit_timestamp(right)?;
            if left_ts == right_ts {
                return Err(DiagnosticError::new(
                    FailureClass::InstallMigrationFailed,
                    "Found multiple legacy installs with ambiguous recency; manual reconciliation required.",
                ));
            }
            let selected = if left_ts > right_ts { left } else { right };
            adopt_legacy_root(selected, &paths.shared_root, &paths.stamp, lines)?
        }
        _ => unreachable!(),
    };

    Ok(selected_root)
}

fn adopt_legacy_root(
    selected_root: &Path,
    shared_root: &Path,
    stamp: &str,
    lines: &mut Vec<String>,
) -> Result<PathBuf, DiagnosticError> {
    if shared_root.exists() || shared_root.is_symlink() {
        if !same_install(shared_root, selected_root) {
            let backup_path = backup_path(shared_root, stamp);
            move_path(shared_root, &backup_path)?;
            lines.push(format!(
                "Backed up existing shared path at {} -> {}",
                shared_root.display(),
                backup_path.display()
            ));
        }
    }

    if !same_install(selected_root, shared_root) {
        if let Some(parent) = shared_root.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                DiagnosticError::new(
                    FailureClass::InstallMigrationFailed,
                    format!(
                        "Could not create shared install directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
        fs::rename(selected_root, shared_root).map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!(
                    "Could not move legacy install {} into {}: {error}",
                    selected_root.display(),
                    shared_root.display()
                ),
            )
        })?;
        lines.push(format!(
            "Moved {} -> {}",
            selected_root.display(),
            shared_root.display()
        ));
        link_directory(selected_root, shared_root)?;
        lines.push(format!(
            "Rewired {} -> {}",
            selected_root.display(),
            shared_root.display()
        ));
    }

    Ok(shared_root.to_path_buf())
}

fn rewire_legacy_root(
    legacy_root: &Path,
    target_root: &Path,
    stamp: &str,
    lines: &mut Vec<String>,
) -> Result<(), DiagnosticError> {
    if !legacy_root.exists() && !legacy_root.is_symlink() {
        return Ok(());
    }
    if same_install(legacy_root, target_root) {
        return Ok(());
    }

    let backup_path = backup_path(legacy_root, stamp);
    move_path(legacy_root, &backup_path)?;
    lines.push(format!(
        "Backed up legacy install at {} -> {}",
        legacy_root.display(),
        backup_path.display()
    ));
    link_directory(legacy_root, target_root)?;
    lines.push(format!(
        "Rewired {} -> {}",
        legacy_root.display(),
        target_root.display()
    ));
    Ok(())
}

fn migrate_non_rebuildable_state(
    state_dir: &Path,
    lines: &mut Vec<String>,
) -> Result<(), DiagnosticError> {
    let config_report = config::migrate_explicit(state_dir)?;
    if config_report.migrated {
        lines.push(format!(
            "Migrated config {} -> {}",
            state_dir.join(config::LEGACY_CONFIG_FILE).display(),
            config_report.canonical_path.display()
        ));
        if config_report.backup_created {
            lines.push(format!(
                "Backed up legacy config at {} -> {}",
                state_dir.join(config::LEGACY_CONFIG_FILE).display(),
                config_report.backup_path.display()
            ));
        }
    }

    let approval_report = repo_safety::migrate_legacy_approvals(state_dir)?;
    for (backup_path, canonical_path) in approval_report.migrated {
        lines.push(format!(
            "Migrated repo-safety approval {} -> {}",
            backup_path.display(),
            canonical_path.display()
        ));
    }
    for backup_path in approval_report.invalidated_backups {
        lines.push(format!(
            "Invalidated unreadable legacy repo-safety approval at {}; fresh approval required.",
            backup_path.display()
        ));
    }

    Ok(())
}

fn print_next_steps(install_root: &Path) -> Vec<String> {
    vec![
        format!(
            "Codex next step: create or refresh ~/.agents/skills/superpowers -> {}/skills",
            install_root.display()
        ),
        format!(
            "Codex next step: create or refresh ~/.codex/agents/code-reviewer.toml from {}/.codex/agents/code-reviewer.toml (copy on Windows; symlink on Unix-like installs)",
            install_root.display()
        ),
        format!(
            "GitHub Copilot next step: create or refresh ~/.copilot/skills/superpowers -> {}/skills",
            install_root.display()
        ),
        format!(
            "GitHub Copilot next step: create or refresh ~/.copilot/agents/code-reviewer.agent.md from {}/agents/code-reviewer.md (copy on Windows; symlink on Unix-like installs)",
            install_root.display()
        ),
    ]
}

fn is_valid_install(path: &Path) -> bool {
    path.join("bin/superpowers-update-check").is_file()
        && path.join("bin/superpowers-config").is_file()
        && path.join("agents/code-reviewer.md").is_file()
        && path.join(".codex/agents/code-reviewer.toml").is_file()
        && path.join("VERSION").is_file()
        && gix::discover(path).is_ok()
}

fn head_commit_timestamp(path: &Path) -> Result<i64, DiagnosticError> {
    let repo = gix::discover(path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!(
                "Could not inspect legacy install {}: {error}",
                path.display()
            ),
        )
    })?;
    let commit = repo.head_commit().map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!(
                "Could not resolve the legacy install head commit {}: {error}",
                path.display()
            ),
        )
    })?;
    Ok(commit
        .time()
        .map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!(
                    "Could not read the legacy install commit time {}: {error}",
                    path.display()
                ),
            )
        })?
        .seconds)
}

fn clone_local_source(source_repo: &Path, destination: &Path) -> Result<(), DiagnosticError> {
    if destination.exists() || destination.is_symlink() {
        remove_path(destination)?;
    }
    copy_dir_recursive(source_repo, destination)
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), DiagnosticError> {
    fs::create_dir_all(destination).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!(
                "Could not create install destination {}: {error}",
                destination.display()
            ),
        )
    })?;

    for entry in fs::read_dir(source).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!(
                "Could not read source install {}: {error}",
                source.display()
            ),
        )
    })? {
        let entry = entry.map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!(
                    "Could not read source install entry from {}: {error}",
                    source.display()
                ),
            )
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry.metadata().map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!(
                    "Could not inspect source install entry {}: {error}",
                    source_path.display()
                ),
            )
        })?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    DiagnosticError::new(
                        FailureClass::InstallMigrationFailed,
                        format!(
                            "Could not create copied install directory {}: {error}",
                            parent.display()
                        ),
                    )
                })?;
            }
            fs::copy(&source_path, &destination_path).map_err(|error| {
                DiagnosticError::new(
                    FailureClass::InstallMigrationFailed,
                    format!(
                        "Could not copy install entry {} to {}: {error}",
                        source_path.display(),
                        destination_path.display()
                    ),
                )
            })?;
        }
    }

    Ok(())
}

fn same_install(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left);
    let right = fs::canonicalize(right);
    matches!((left, right), (Ok(left), Ok(right)) if left == right)
}

fn backup_path(path: &Path, stamp: &str) -> PathBuf {
    PathBuf::from(format!("{}.backup-{stamp}", path.display()))
}

fn move_path(source: &Path, destination: &Path) -> Result<(), DiagnosticError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!(
                    "Could not create backup directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    fs::rename(source, destination).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!(
                "Could not move {} to {}: {error}",
                source.display(),
                destination.display()
            ),
        )
    })
}

fn remove_path(path: &Path) -> Result<(), DiagnosticError> {
    if !path.exists() && !path.is_symlink() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!("Could not inspect path {}: {error}", path.display()),
        )
    })?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!("Could not remove directory {}: {error}", path.display()),
            )
        })?;
    } else {
        fs::remove_file(path).map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!("Could not remove file {}: {error}", path.display()),
            )
        })?;
    }
    Ok(())
}

fn link_directory(link_path: &Path, target_path: &Path) -> Result<(), DiagnosticError> {
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            DiagnosticError::new(
                FailureClass::InstallMigrationFailed,
                format!(
                    "Could not create link parent directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    remove_path(link_path)?;
    create_directory_link(target_path, link_path)
}

#[cfg(unix)]
fn create_directory_link(target_path: &Path, link_path: &Path) -> Result<(), DiagnosticError> {
    std::os::unix::fs::symlink(target_path, link_path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!(
                "Could not create symlink {} -> {}: {error}",
                link_path.display(),
                target_path.display()
            ),
        )
    })
}

#[cfg(windows)]
fn create_directory_link(target_path: &Path, link_path: &Path) -> Result<(), DiagnosticError> {
    std::os::windows::fs::symlink_dir(target_path, link_path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstallMigrationFailed,
            format!(
                "Could not create directory link {} -> {}: {error}",
                link_path.display(),
                target_path.display()
            ),
        )
    })
}
