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
                    .or_else(|| repo_relative_projection_suffix(source))
                    .unwrap_or_else(|| source.to_owned());
                *path = Value::String(normalized);
            }
        }
    }
}

pub fn normalize_runtime_provenance_paths_for_parity(value: &mut Value) {
    let Some(runtime_provenance) = value
        .get_mut("runtime_provenance")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for field in [
        "repo_root",
        "state_dir",
        "installed_skill_root",
        "workspace_skill_root",
    ] {
        if runtime_provenance.contains_key(field) {
            runtime_provenance.insert(field.to_owned(), Value::String(String::from("<PATH>")));
        }
    }
    let Some(skill_discovery) = runtime_provenance
        .get_mut("skill_discovery")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for field in ["installed_skill_root", "workspace_skill_root"] {
        if skill_discovery.contains_key(field) {
            skill_discovery.insert(field.to_owned(), Value::String(String::from("<PATH>")));
        }
    }
    if let Some(active_roots) = skill_discovery
        .get_mut("active_roots")
        .and_then(Value::as_array_mut)
    {
        for root in active_roots {
            let Some(root_object) = root.as_object_mut() else {
                continue;
            };
            for field in ["configured_path", "resolved_path"] {
                if root_object.contains_key(field) {
                    root_object.insert(field.to_owned(), Value::String(String::from("<PATH>")));
                }
            }
        }
    }
}

fn repo_relative_projection_suffix(path: &str) -> Option<String> {
    if path.starts_with("docs/featureforge/") {
        return Some(path.to_owned());
    }
    path.split_once("/docs/featureforge/")
        .map(|(_, suffix)| format!("docs/featureforge/{suffix}"))
}
