#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

pub fn state_dir_projection_path(status: &Value, repo_relative_path: &str) -> PathBuf {
    let suffix = format!("/{repo_relative_path}");
    status["state_dir_projection_paths"]
        .as_array()
        .and_then(|paths| {
            paths.iter().filter_map(Value::as_str).find(|path| {
                path == &repo_relative_path
                    || path.ends_with(&suffix)
                    || path.ends_with(repo_relative_path)
            })
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "status should expose state-dir projection path for {repo_relative_path}: {status:?}"
            )
        })
}

pub fn read_state_dir_projection(status: &Value, repo_relative_path: &str) -> String {
    let path = state_dir_projection_path(status, repo_relative_path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "state-dir projection {} should be readable: {error}",
            path.display()
        )
    })
}

pub fn write_state_dir_projection(status: &Value, repo_relative_path: &str, source: &str) {
    let path = state_dir_projection_path(status, repo_relative_path);
    fs::write(&path, source).unwrap_or_else(|error| {
        panic!(
            "state-dir projection {} should be writable: {error}",
            path.display()
        )
    });
}

pub fn remove_state_dir_projection(status: &Value, repo_relative_path: &str) {
    let path = state_dir_projection_path(status, repo_relative_path);
    fs::remove_file(&path).unwrap_or_else(|error| {
        panic!(
            "state-dir projection {} should be removable: {error}",
            path.display()
        )
    });
}

pub fn normalize_state_dir_projection_paths_for_parity(value: &mut Value) {
    let tracked_paths = value["tracked_projection_paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(paths) = value["state_dir_projection_paths"].as_array_mut() {
        for path in paths {
            if let Some(source) = path.as_str() {
                let normalized = tracked_paths
                    .iter()
                    .find(|tracked_path| {
                        source == tracked_path.as_str()
                            || source.ends_with(&format!("/{tracked_path}"))
                    })
                    .cloned()
                    .unwrap_or_else(|| source.to_owned());
                *path = Value::String(normalized);
            }
        }
    }
}
