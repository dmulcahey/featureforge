use std::fs;
use std::path::Path;

pub fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    fs::write(path, contents).expect("file should be writable");
    if is_execution_harness_state_path(path) {
        let _ = fs::remove_file(path.with_file_name("events.jsonl"));
        let _ = fs::remove_file(path.with_file_name("events.lock"));
        let _ = fs::remove_file(path.with_file_name("state.legacy.json"));
    }
}

fn is_execution_harness_state_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "state.json")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "execution-harness")
}
