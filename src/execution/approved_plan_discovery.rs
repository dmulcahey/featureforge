use std::fs;

use crate::execution::state::ExecutionRuntime;

pub(crate) fn infer_unique_engineering_approved_plan_path(
    runtime: &ExecutionRuntime,
) -> Option<String> {
    let mut stack = vec![runtime.repo_root.join("docs/featureforge/plans")];
    let mut candidates = Vec::new();
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file()
                || path.extension().and_then(|value| value.to_str()) != Some("md")
            {
                continue;
            }
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            if !source.contains("**Workflow State:** Engineering Approved") {
                continue;
            }
            let rel = path
                .strip_prefix(&runtime.repo_root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            candidates.push(rel);
            if candidates.len() > 1 {
                return None;
            }
        }
    }
    candidates.pop()
}
