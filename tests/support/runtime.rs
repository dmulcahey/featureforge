use featureforge::expect_ext::ExpectValueExt as _;
use std::path::Path;

use featureforge::execution::state::ExecutionRuntime;

pub fn execution_runtime(repo: &Path, state: &Path) -> ExecutionRuntime {
    let mut runtime = ExecutionRuntime::discover(repo)
        .expect_or_abort("git repo should be discoverable for test runtime");
    runtime.state_dir = state.to_path_buf();
    runtime
}
