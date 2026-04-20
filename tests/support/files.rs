use featureforge::expect_ext::ExpectValueExt as _;
use std::fs;
use std::path::Path;

pub fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect_or_abort("parent directory should be creatable");
    }
    fs::write(path, contents).expect_or_abort("file should be writable");
}
