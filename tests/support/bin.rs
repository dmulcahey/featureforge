use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[allow(dead_code)]
static COMPILED_FEATUREFORGE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[allow(dead_code)]
pub fn compiled_featureforge_path() -> &'static Path {
    COMPILED_FEATUREFORGE_PATH
        .get_or_init(|| PathBuf::from(env!("CARGO_BIN_EXE_featureforge")))
        .as_path()
}
