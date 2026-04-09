use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static COMPILED_FEATUREFORGE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn compiled_featureforge_path() -> &'static Path {
    COMPILED_FEATUREFORGE_PATH
        .get_or_init(|| PathBuf::from(env!("CARGO_BIN_EXE_featureforge")))
        .as_path()
}
