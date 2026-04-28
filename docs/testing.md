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
cargo nextest run --all-targets --all-features --no-fail-fast
```

For task-completion gates, plan-task review loops, and pre-merge verification,
run the full Rust nextest suite. Do not replace it with targeted `--test ...`
subsets when the goal is to prove the branch, because the full suite has more than 1100 tests and targeted shards can hide unrelated failures. Use targeted
commands only while iterating on a known failure, then return to
`cargo nextest run --all-targets --all-features --no-fail-fast` before claiming
the task or branch is green. The `--no-fail-fast` flag is required so the run
captures the full failure set instead of stopping at the first failed binary.
## Performance Budget

`cargo test` with no extra args is the canonical full-suite latency budget for this repository. Treat roughly 3 to 4 minutes on a warm local build as the target. If a warm local run regresses past about 240 seconds, stop and profile before merging instead of normalizing the slowdown away.

Performance and profiling hardening from broader remediation reports is deliberately out of scope for the plan-review hardening cutover. Treat this section as maintenance guidance for test-suite health, not as evidence that benchmark or profiling work was implemented as part of the task-contract migration.

For performance investigations or local iteration where you explicitly want the
same full suite through a sharded runner, use the helper below. It compiles
once, then runs isolated nextest shards in parallel from one archive, which
removes parallel `cargo` lock contention and prevents shard-to-shard tempdir
interference. The branch-verification command remains the plain full nextest
suite above unless a user or CI job explicitly asks for the sharded helper.

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
cargo nextest run --all-targets --all-features --no-fail-fast
```

## Runtime Churn Cutover Validation

Runtime churn fixes must prove that public routing advances, reports a precise
diagnostic, or returns `already_current` without mutating approved files or
repo-local projection exports. Use targeted iteration while repairing a known failure, but the final
cutover proof must include the Rust, Node/doc, and source-archive checks below:

```bash
node scripts/gen-skill-docs.mjs
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
node --test tests/evals/review-accelerator-contract.eval.mjs
npm --prefix tests/brainstorm-server test
node scripts/verify-source-archive.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo test --test liveness_model_checker
cargo nextest run --all-targets --all-features --no-fail-fast
```

While repairing a known runtime-churn failure, the focused Rust shards are the
runtime-instruction shard (`cargo nextest run --test runtime_instruction_contracts --test runtime_instruction_plan_review_contracts --test runtime_instruction_review_contracts`)
and the execution/public replay shard (`cargo nextest run --test workflow_runtime --test workflow_shell_smoke --test plan_execution --test cli_parse_boundary --test public_replay_churn`).
These are iteration aids only; the documented final Rust gate remains the full
no-fail-fast nextest suite above.

Run these source checks as part of the same cutover proof and inspect every
match. The accepted result is limited to historical/internal-only tests,
quarantined direct helpers, generated-doc contract assertions, or explicit
compiled-CLI rejection coverage:

```bash
rg -n "runtime-owned receipt|receipt records|receipt-ready|Dedicated Reviewer Receipt Contract" README.md docs skills agents tests
rg -n "Invoke `featureforge:plan-fidelity-review`\\." skills/writing-plans tests
rg -n "record-review-dispatch|rebuild-evidence|gate-review|gate-finish|record-branch-closure|record-release-readiness|record-final-review|record-qa|preflight" tests
```

The public replay suite (`tests/public_replay_churn.rs`) is part of the
targeted runtime matrix and the full nextest suite. It must continue to run
through the compiled public CLI only, reject hidden command/flag use in the
test wrapper itself, and preserve command-budget assertions for known churn
dead ends.

The source-archive verifier must pass from the repository root or from an
unpacked source archive root. It asserts that clean-archive Node/doc test helper
modules, including `tests/codex-runtime/helpers/markdown-test-helpers.mjs` and
`tests/evals/helpers/eval-observability.mjs`, are present instead of relying on
machine-local files.

The liveness checker must include the FS-01 through FS-08 production-loop
shapes: already-current cycle-break overlays, targetless stale diagnostics,
orphan late-stage records, projection-only dirtiness, summary-hash drift,
downstream stale steps, exact command/resume disagreement, and nested
interruption projections. It must fail on hidden/debug public recommendations
and on public commands that neither improve the runtime-derived progress metric,
expose a different true blocker, emit a deterministic diagnostic, nor resolve an
`already_current` state without stale overlays.

Normal `begin`, `complete`, `reopen`, `transfer`, `close-current-task`,
`repair-review-state`, `advance-late-stage`, `workflow operator`, and
`plan execution status` commands must leave approved plan/evidence/review files
and repo-local projection exports untouched. Runtime read models live under the
state directory. Repo-local human-readable exports are explicit:

```bash
featureforge plan execution materialize-projections --plan <approved-plan-path> --scope execution|late-stage|all
```

Materialization writes repo-local projection exports without modifying approved
plan or evidence files. It is projection-only and must not be recommended by
operator routing as required progress.

Historical final-remediation plans used targeted Rust subsets while closing specific failures. For branch proof, task-completion gates, plan-task review loops, and pre-merge verification, use the full Rust nextest suite instead:

```bash
node scripts/gen-skill-docs.mjs
node scripts/gen-agent-docs.mjs
node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs
node --test tests/evals/review-accelerator-contract.eval.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets --all-features --no-fail-fast
```

Targeted `cargo nextest run --test ...` commands are local debugging tools only. Do not use them as the documented final gate.

Editing runtime strategy-checkpoint, topology recommendation, or final-review deviation contracts:

```bash
cargo nextest run --all-targets --all-features --no-fail-fast
```

Editing install or update surfaces:

```bash
cargo nextest run --all-targets --all-features --no-fail-fast
```

Editing packaging or prebuilt artifact refresh flows:

```bash
cargo nextest run --all-targets --all-features --no-fail-fast
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
cargo nextest run --all-targets --all-features --no-fail-fast
```

## Repo Fixtures

Keep workflow fixtures under `tests/codex-runtime/fixtures/workflow-artifacts/`. They are the stable contract inputs for route-time header parsing and approved-plan linkage tests.
