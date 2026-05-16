//! INTERNAL_RUNTIME_HELPER_TEST: this file intentionally exercises unavailable runtime internals.
//! Liveness-only semantic support for executing exact public argv through the public CLI parser
//! without paying one subprocess per synthetic graph edge.
//!
//! This is not shipped-runtime proof: end-to-end public-flow tests must use the compiled CLI.

use std::path::Path;
use std::process::Output;

#[path = "public_runtime_contract_runner.rs"]
mod public_runtime_contract_runner;

pub fn run_semantic_public_argv_in_process<'a>(
    repo: &Path,
    state_dir: &Path,
    args: impl IntoIterator<Item = &'a str>,
    context: &str,
) -> Output {
    public_runtime_contract_runner::run_public_runtime_contract_in_process(
        repo, state_dir, args, context,
    )
}
