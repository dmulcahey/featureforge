# Testing FeatureForge

This document describes the active validation surface for the FeatureForge runtime and skill library.
Treat this file as the canonical validation matrix; release-facing install and overview docs should point here instead of copying partial command lists.

Legacy `tests/codex-runtime/*.sh` harnesses have been removed; use the Rust and Node contract suites below as the active oracle.

## Fast Validation

Run these commands from the repo root for the core contract surface:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
npm --prefix tests/brainstorm-server test
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test contracts_spec_plan --test packet_and_schema --test contracts_execution_runtime_boundaries --test runtime_instruction_contracts --test using_featureforge_skill --test runtime_instruction_plan_review_contracts --test session_config_slug --test repo_safety --test runtime_root_cli --test update_and_install --test workflow_runtime --test workflow_shell_smoke --test plan_execution --test powershell_wrapper_resolution --test upgrade_skill
cargo test --test liveness_model_checker
```
## Performance Budget

`cargo test` with no extra args is the canonical full-suite latency budget for this repository. Treat roughly 3 to 4 minutes on a warm local build as the target. If a warm local run regresses past about 240 seconds, stop and profile before merging instead of normalizing the slowdown away.

For day-to-day full-suite runs, prefer the sharded runner below. It compiles once, then runs isolated nextest shards in parallel from one archive, which removes parallel `cargo` lock contention and prevents shard-to-shard tempdir interference.

```bash
scripts/run-rust-tests-sharded.sh
# explicit shard count
scripts/run-rust-tests-sharded.sh 8
# isolate more aggressively (lower per-shard contention)
FEATUREFORGE_SHARD_THREADS=1 scripts/run-rust-tests-sharded.sh 8
# run a focused subset with nextest-compatible filters
scripts/run-rust-tests-sharded.sh 6 -- runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine
```

The runner writes logs and per-shard temp sandboxes under `${TMPDIR:-/tmp}/featureforge-nextest-sharded/`.

When the suite slows down:

- do not remove tests or weaken assertions to recover time
- prefer in-process semantic test helpers over binary subprocesses when stdout/stderr framing and shell behavior are not the contract under test
- prefer shared runtime helpers and memoized immutable reads over repeated repo discovery, repeated state reloads, or repeated tree/head lookups
- prefer `gix` or equivalent high-performance libraries over ad hoc `git` subprocesses when semantics can be preserved
- when a test helper synthesizes CLI output in-process, preserve CLI bytes exactly: exit code semantics, stdout/stderr routing, trailing newlines, JSON field order, and explicit state-dir inputs should match the real binary
- if a test or helper must keep a subprocess boundary for contract coverage, leave a code comment explaining why that divergence is intentional

Profile the plain suite first:

```bash
time -p cargo test --quiet
# macOS detailed memory/context stats:
/usr/bin/time -lp cargo test --quiet
```

Then profile the largest binaries individually to find the regression source before changing code. The usual hot set is `workflow_shell_smoke`, `plan_execution`, `workflow_runtime`, and `workflow_runtime_final_review`:

```bash
time -p cargo test --quiet --test workflow_shell_smoke
time -p cargo test --quiet --test workflow_runtime
time -p cargo test --quiet --test workflow_runtime_final_review
time -p cargo test --quiet --test plan_execution
# macOS detailed memory/context stats:
/usr/bin/time -lp cargo test --quiet --test workflow_shell_smoke
/usr/bin/time -lp cargo test --quiet --test workflow_runtime
/usr/bin/time -lp cargo test --quiet --test workflow_runtime_final_review
/usr/bin/time -lp cargo test --quiet --test plan_execution
```

## What Each Layer Covers

### Node Contract Tests

`tests/codex-runtime/*.test.mjs` covers:

- generated skill-doc structure and freshness
- explicit skill-doc generation contracts (`gen-skill-docs.unit`, `skill-doc-contracts`, `skill-doc-generation`)
- active docs and archive layout fixtures
- workflow-fixture invariants
- routing and eval-document contract assertions

`tests/brainstorm-server` `npm test` covers:

- brainstorm server HTTP/WebSocket behavior
- launch-wrapper smoke for `start-server`/`stop-server` shell and PowerShell entrypoints

### Rust Runtime Tests

The main Rust suites cover:

- workflow artifact resolution and failure contracts
- packet/schema and workflow routing-boundary contracts (`packet_and_schema`, `contracts_execution_runtime_boundaries`)
- `using-featureforge` and direct workflow routing without session-entry prerequisites, including regression coverage for inert legacy gate files and env inputs
- repo-safety and protected-branch write guarantees
- install, state, and update-check runtime behavior
- public workflow CLI behavior
- execution state transitions and plan linkage

### Workflow Status Snapshot

workflow-status snapshot coverage for the ambiguous-spec route lives in `tests/workflow_runtime.rs` and is backed by `tests/fixtures/differential/workflow-status.json`. Treat any mismatch as a contract change that requires explicit fixture review.

### Eval Docs

`tests/evals/README.md` describes the active higher-level eval surfaces:

- the doc-driven `using-featureforge` routing gate
- the doc-driven Search Before Building gate
- opt-in Node-based `.eval.mjs` tests where a local judge run is still useful

## Change-Scoped Guidance

Editing skill templates or generated skill docs:

```bash
node scripts/gen-skill-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
```

Editing brainstorm-server runtime scripts or launch wrappers:

```bash
npm --prefix tests/brainstorm-server test
```

Editing reviewer sources or generated reviewer docs:

```bash
node scripts/gen-agent-docs.mjs --check
```

Editing workflow routing, runtime docs, or execution contracts:

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test contracts_spec_plan --test packet_and_schema --test contracts_execution_runtime_boundaries --test runtime_instruction_contracts --test using_featureforge_skill --test runtime_instruction_plan_review_contracts --test workflow_runtime --test workflow_shell_smoke --test plan_execution
```

Final-remediation verification also includes the Step 12 command sequence from the task-boundary gap-closure plan:

```bash
node scripts/gen-skill-docs.mjs
node scripts/gen-agent-docs.mjs
node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs
cargo nextest run --test workflow_runtime --test workflow_shell_smoke --test plan_execution --test contracts_execution_runtime_boundaries --test runtime_instruction_contracts --test runtime_instruction_plan_review_contracts
```

If the repo's normal verification flow also expects these and they are available locally, then run:

```bash
cargo nextest run --test using_featureforge_skill --test runtime_root_cli --test upgrade_skill
```

Then complete the remaining manual Step 12 checks from the plan:

- 12.4 Confirm the compiled-CLI smoke output keeps these ceilings:
  - task closure happy path `<= 2` runtime-management commands
  - internal-dispatch task closure `<= 2` runtime-management commands
  - rebase / resume stale-boundary recovery `<= 3` runtime-management commands before implementation resumes
  - stale release refresh `<= 3` runtime-management commands before the next real review step
- 12.5 Search the fixed recovery / budget paths and confirm the normal path recommends only public execution commands (`begin`, `complete`, `reopen`, `transfer`, `close-current-task`, `repair-review-state`, `advance-late-stage`).
- 12.6 Run the FS-13 fixture and confirm the runtime surfaces the earliest stale boundary without any manual edit to `**Execution Note:**` lines.

Editing runtime strategy-checkpoint, topology recommendation, or final-review deviation contracts:

```bash
cargo nextest run --test plan_execution_topology --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_leases --test execution_harness_state
```

Editing install or update surfaces:

```bash
cargo nextest run --test session_config_slug --test update_and_install --test upgrade_skill
```

Editing packaging or prebuilt artifact refresh flows:

```bash
cargo nextest run --test powershell_wrapper_resolution --test workflow_shell_smoke --test workflow_runtime
```

When checked-in prebuilt artifacts are part of the change, refresh and verify them explicitly:

```bash
FEATUREFORGE_PREBUILT_TARGET=darwin-arm64 scripts/refresh-prebuilt-runtime.sh
PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc FEATUREFORGE_PREBUILT_TARGET=windows-x64 FEATUREFORGE_PREBUILT_RUST_TARGET=x86_64-pc-windows-gnu scripts/refresh-prebuilt-runtime.sh
cp target/aarch64-apple-darwin/release/featureforge bin/featureforge
chmod +x bin/featureforge
```

If Homebrew `cargo`/`rustc` shadow rustup-managed toolchains on `PATH`, put the rustup toolchain shims first before running the Windows GNU refresh command so the installed `x86_64-pc-windows-gnu` standard library can be found. The GNU cross-build also expects `x86_64-w64-mingw32-gcc` to be available on `PATH`.

Then rerun:

```bash
cargo nextest run --test powershell_wrapper_resolution --test workflow_shell_smoke --test workflow_runtime
```

## Repo Fixtures

Keep workflow fixtures under `tests/codex-runtime/fixtures/workflow-artifacts/`. They are the stable contract inputs for route-time header parsing and approved-plan linkage tests.
