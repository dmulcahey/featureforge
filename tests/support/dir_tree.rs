use std::fs;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

pub fn copy_dir_recursive(source: &Path, destination: &Path) {
    if clone_dir_contents(source, destination).is_ok() {
        return;
    }
    copy_dir_recursive_fallback(source, destination);
}

#[cfg(target_os = "macos")]
fn clone_dir_contents(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| {
        format!(
            "failed to create destination directory `{}`: {error}",
            destination.display()
        )
    })?;
    let source_contents = source.join(".");
    let output = Command::new("cp")
        .arg("-cR")
        .arg(&source_contents)
        .arg(destination)
        .output()
        .map_err(|error| format!("failed to run cp -cR for `{}`: {error}", source.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "cp -cR failed for `{}` -> `{}`: {}",
            source.display(),
            destination.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn clone_dir_contents(_source: &Path, _destination: &Path) -> Result<(), String> {
    Err(String::from("clone copy is unavailable on this platform"))
}

fn copy_dir_recursive_fallback(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("destination directory should be creatable");
    for entry in fs::read_dir(source).expect("source directory should be readable") {
        let entry = entry.expect("source entry should be readable");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .expect("source entry type should be readable");
        if file_type.is_dir() {
            copy_dir_recursive_fallback(&source_path, &destination_path);
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)
                .unwrap_or_else(|error| panic!("failed to copy {:?}: {error}", source_path));
        }
    }
}
