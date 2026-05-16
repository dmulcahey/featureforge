# Testing FeatureForge

This document describes the active validation surface for the FeatureForge runtime and skill library.
Treat this file as the canonical validation matrix; release-facing install and overview docs should point here instead of copying partial command lists.

Legacy `tests/codex-runtime/*.sh` harnesses have been removed; use the Rust and Node contract suites below as the active oracle.

## Mandatory Release Gates

Run these commands from the repo root for the release contract surface. These
are mandatory branch/release gates, not optional audit aids:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node scripts/run-codex-runtime-tests.mjs
node --test tests/evals/*.eval.mjs
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

## Focused Runtime Audit Aids

When validating runtime public-surface hardening, generated docs, boundary
tests, and replay churn fixes, use this focused matrix while iterating before
the full no-fail-fast nextest gate:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node scripts/run-codex-runtime-tests.mjs
node --test tests/evals/*.eval.mjs
cargo test --test public_cli_flow_contracts -- --nocapture
cargo test --test public_flow_scan_contracts -- --nocapture
cargo test --test public_replay_churn -- --nocapture
cargo test --test runtime_behavior_golden -- --nocapture
cargo test --test runtime_module_boundaries -- --nocapture
cargo test --test rust_source_scan_contracts -- --nocapture
cargo test --test liveness_model_checker -- --nocapture
cargo test --test packet_and_schema -- --nocapture
cargo test --test workflow_shell_smoke -- --nocapture
cargo test --test plan_execution -- --nocapture
cargo test --test plan_execution_final_review -- --nocapture
cargo test --test workflow_runtime -- --nocapture
cargo test --test workflow_runtime_final_review -- --nocapture
cargo clippy --all-targets --all-features -- -D warnings
```

Use that matrix to prove the intended surfaces directly during audit or
remediation work. It does not replace the mandatory branch gate: after focused
validation passes, still run
`cargo nextest run --all-targets --all-features --no-fail-fast`.
`cargo test --test liveness_model_checker` in this matrix is internal semantic
route-convergence coverage with a targeted compiled-CLI parity edge. Do not cite
it as shipped-runtime public-flow proof; use
`scripts/run-public-runtime-flow-tests.sh`, public shell smoke suites, and route
goldens for that boundary. `public_flow_scan_contracts` is static scanner
self-test coverage; run it in focused validation, but do not count it as
production public-flow proof.

## Installed Control Plane Isolation Gate

Changes that touch runtime provenance, workflow routing, generated skills,
workspace-runtime mutation guards, shell-smoke helpers, evidence linting, or
installed-control-plane docs must run the installed-control-plane isolation gate:

```bash
scripts/verify-installed-control-plane-isolation.sh
```

The script runs the durable gate in this order:

1. focused runtime-boundary and workflow smoke suites
2. generated skill-doc freshness and prompt contracts
3. workspace runtime evidence linting
4. strict Clippy
5. the full no-fail-fast nextest branch gate

This gate keeps the targeted public contracts and the full Rust suite together.
It must keep evidence linting in the normal verification set so recorded
evidence and review artifacts cannot regress to workspace-runtime live
mutations.

### Workspace Runtime State Isolation

Shell-smoke and fixture tests use the workspace-compiled binary as a test
subject only. They must set `FEATUREFORGE_STATE_DIR` to an isolated temp or
fixture state path.

- Test runtime: workspace binary (`target/debug/featureforge` or equivalent) +
  temp/fixture `FEATUREFORGE_STATE_DIR`
- Live control plane: installed binary (`~/.featureforge/install/bin/featureforge`) +
  live `~/.featureforge` state

The shared shell helpers enforce this boundary with
`assert_workspace_runtime_uses_temp_state`. Live-state execution with workspace
binaries is blocked by default in shell-smoke paths and only permitted for
explicit guard-coverage tests that opt in to the live-state assertion bypass.

Workspace binaries must not run live workflow mutations against
`~/.featureforge`. The runtime guard blocks live mutation commands from
workspace-local `./bin/featureforge`, `target/debug/featureforge`, and
`cargo run -- ...` unless
`FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1` is explicitly set. That
override is intentionally auditable and should almost never be used; if it is
approved for self-hosting recovery, record it in execution evidence and review
provenance.

Use the installed diagnostic when checking a session boundary:

```bash
~/.featureforge/install/bin/featureforge doctor self-hosting --json
```

## Public And Internal Runtime Gates

Public-flow proof and internal runtime compatibility are separate gates. Record
their results separately in release checklists and CI summaries:

```bash
scripts/run-public-runtime-flow-tests.sh
scripts/run-internal-runtime-compatibility-tests.sh
```

The public-flow gate is a classified surface. The classification lives in
`tests/support/public_flow_scan.rs` and is checked against
`scripts/run-public-runtime-flow-tests.sh` by `public_flow_scan_contracts`.
The script includes executable public-flow proof suites such as
`public_replay_churn`, `workflow_shell_smoke`,
and `workflow_runtime_final_review`;
mixed public-flow plus internal-semantic suites such as
`workflow_entry_shell_smoke`, `workflow_runtime`, `plan_execution`,
`contracts_execution_runtime_boundaries`, and `execution_query`;
focused public contracts such as `runtime_behavior_golden`,
`plan_execution_final_review`, `plan_execution_topology`, and
`execution_harness_state`; and static guards such as
`public_cli_flow_contracts`. Mixed suites are scanner-protected because their
public sections still prove shipped behavior, but their internal semantic
sections must not be cited as pure public-flow proof. Static guards are
public-flow protection, not end-to-end compiled-CLI transition proof by
themselves.

`public_flow_scan_contracts` is the scanner self-test suite for injected
hidden-helper, token-only follow-up, script/classification drift, and
display-command regressions; run it through
`cargo test --test public_flow_scan_contracts -- --nocapture` as focused/static
validation for the gate, not production public-flow proof. `runtime_behavior_golden`
is focused public contract coverage, not full compiled-CLI transition proof for
every row. Most rows use the in-process public argv/parser contract runner so
they can pin route DTO shape without subprocess churn; rows that explicitly
exercise environment injection still cross the compiled CLI boundary. The
golden serializes only the external route-contract DTO fields:
`phase`, `phase_detail`, `next_action`, `review_state_status`, typed
argv/template surfaces, `required_inputs`, reason/blocking codes, and the
minimal route context needed by assertions. Some late-stage rows use synthetic
fixture setup to reach long-lived workflow states; those rows pin public output
contracts rather than production state construction. `bootstrap_smoke` is the
packaged-artifact smoke layer: on host-compatible checked-in runtimes it probes
`bin/featureforge` and the matching prebuilt with one representative typed-route
status query, while cargo-built public-flow tests remain the behavioral source
of truth. On platforms without a matching checked-in runtime, that test records
the packaging boundary and relies on cargo-built public-route proof.
The goldens intentionally omit display-only `recommended_command`, projection
payloads, and `workflow_status` for execution-route scenarios.
`public_replay_churn` is synthetic historical fixture setup plus public
recovery proof: it may seed impossible legacy states explicitly, but recovery
assertions must run through the compiled public CLI and must not be counted as
public setup proof. It also includes public aggregate `advance-late-stage`
coverage for final-review dispatch and final-review outcome progression, so
late-stage goldens are no longer the only proof that those states are reachable.

The internal runtime compatibility gate runs tests named
`internal_only_compatibility*`. It preserves low-level direct-helper coverage
for legacy and boundary compatibility, but do not count it as public-flow or
public UX proof. Internal helper results may support compatibility confidence;
they cannot replace the public-flow gate above.

`tests/plan_execution_final_review.rs` is scanner-protected and included in the
public-flow gate as focused final-review contract coverage. The same file also
contains receipt parser and validator unit coverage, so do not relabel it as
dedicated end-to-end transition proof.

For final runtime cutover checks that touched execution query or workflow-entry
coverage, extend the matrix with:

```bash
cargo test --test workflow_entry_shell_smoke -- --nocapture
cargo test --test execution_harness_state -- --nocapture
cargo test --test execution_query -- --nocapture
```

## Prompt Surface Budget Gate

Generated top-level skill prompts are budgeted separately from companion
references. The active cutover baseline was 7,191 generated top-level
`skills/*/SKILL.md` lines; the active enforce-mode cap lives in
`skills/skill-doc-budgets.json`. The budget test prints the current total and
per-skill line counts.

The budget gate must stay in enforce mode for release work:

```bash
node scripts/gen-skill-docs.mjs --check
node --test tests/codex-runtime/skill-doc-budget.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs
```

Release checklists must record prompt-surface failures in two separate lines:

- Prompt budget enforcement: `tests/codex-runtime/skill-doc-budget.test.mjs`
  fails when generated top-level skill docs or `skills/skill-doc-budgets.json`
  exceed the approved manifest budget.
- Mandatory-law retention: `tests/codex-runtime/skill-doc-contracts.test.mjs`
  fails when compaction removes required workflow routing law, approval law,
  protected-branch repo-safety law, hidden-helper bans, fail-closed stop rules,
  reviewer-recursion prohibitions, or typed workflow-route stop rules from
  top-level skill docs.

Any change to `skills/skill-doc-budgets.json`, including line limits, enforce
mode, or the set of budgeted skills, requires explicit prompt-budget review in
the release notes or review record. If a top-level generated skill needs more
lines, that review note must explain why the content must remain top-level, why
it cannot move to a companion reference, and whether any existing top-level
prose was removed to make room. Do not treat manifest changes as routine
test-fixture updates.

Do not lower prompt budgets by moving mandatory workflow routing law, approval
law, protected-branch repo-safety law, hidden-helper bans, fail-closed stop
rules, reviewer-recursion prohibitions, or typed workflow-route stop posture
entirely into companion references. Companion references should carry detailed
field lists, operator-rerun binding examples, and rationale; active top-level skills must keep
terminal gates and stop rules directly visible. For route-owning skills, that
means compact top-level operator JSON law plus a link to
`references/operator-route-authority.md`, while detailed argv rebinding and
operator-mediated template materialization stay in the reference.

When content moves into companion references, keep those references discoverable
from the generated top-level skill docs and included in the packaged skill
surface.

## Runtime-Safety Audit Archive

Superseded runtime-safety audit-loop artifacts live behind one active index:
`docs/featureforge/archive/runtime-safety-audit-history/README.md`.
Keep the current remediation plan under `docs/featureforge/plans/`, move
superseded loop plans/reports/reference notes into that archive after a later
loop replaces them, and link the index from active docs instead of individual
historical loop files.

The archive index records the retention rule, the current active runtime-safety
plan, and the archived report/plan/reference counts. If a cleanup pass wants to
delete or further relocate archived files, it must first run the active-reference
check documented in the index and list exact referenced/unreferenced files.

## Performance Budget

The mandatory release matrix above is the canonical branch gate. Its Rust
component is the full no-fail-fast nextest suite; the checked-in nextest profile
already keeps live output focused on failures and final slow-test data.

The default nextest profile is intentionally capped in
`.config/nextest.toml` with `test-threads = 64`. The runtime suites spawn
isolated git repositories and compiled CLI processes; current local measurement
on this machine showed 32 workers leaving long runtime/golden tests queued too
late, while 64 kept the warm full-suite test phase inside the health budget.
Keep the checked-in cap high enough to preserve the clean-run budget without
weakening coverage, but recheck full-suite time before changing it again.

Expensive workflow fixture templates are cached outside `target/` under
`.featureforge/test-cache/test-fixtures/` by default, with
`FEATUREFORGE_TEST_FIXTURE_CACHE_ROOT` available for local override. The cache is
keyed by explicit fixture input versions plus source/fixture content, not binary
mtime or `current_exe()` metadata, so `cargo clean` does not turn every required
clean rerun into a cold fixture rebuild. If fixture setup semantics change,
bump the caller-supplied fixture input version in the affected test helper.

Treat roughly 4 to 5 minutes on a clean local run as the preferred health
target for the full nextest branch gate. The hard remediation trigger is the
user-approved 10-minute stop rule: before starting a full nextest cycle, first
confirm no `cargo nextest`, `cargo-nextest`, or `nextest run` process is already
running; if a full nextest run crosses 10 minutes, stop it, run `cargo clean`,
rerun the same full nextest command, and profile or fix the introduced
contention if the clean rerun still crosses the health budget. For warm local
iteration, roughly 3 to 4 minutes remains the target.

`cargo test` remains useful for focused profiling and compatibility checks, but
it is not the release branch-gate latency authority when nextest is the command
used for full verification.

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
- generated top-level skill prompt budgets in enforce mode
- active docs/skills/agent prompt forbidden-vocabulary scans
- mandatory-law retention, companion-reference packaging, and prompt-scoped reviewer recursion checks
- active docs and archive layout fixtures
- workflow-fixture invariants
- routing and eval-document contract assertions

Run this layer through `node scripts/run-codex-runtime-tests.mjs` in release
and branch gates. The wrapper runs `node --test tests/codex-runtime/*.test.mjs`
with a fixed timeout and fails closed if the grouped Node process prints a
green TAP summary but does not exit. The raw command should still exit with
status 0 in local and CI shells, but the release checklist uses the wrapper so
open handles cannot be mistaken for success.

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

Rust tests own compiled CLI behavior, public/private runtime boundary checks,
Rust module import boundaries, and route-golden DTO/schema parity. They should
not duplicate active markdown prompt scans that already live in the Node
contract layer. Add Rust source scanners only when the boundary is Rust-specific
or the test proves public CLI behavior through the shipped binary.

Keep static Rust assertions tied to durable ownership boundaries. Public-flow
tests should prefer compiled CLI behavior, public route goldens, and JSON schema
semantics over private helper names, struct field ordering, or prose snippets.
`runtime_module_boundaries` should assert import direction, facade/orchestrator
boundaries, centralized vocabulary/DTO ownership, and public projection behavior.
Avoid line caps, child-module name pins, and private helper-name pins unless the
name itself is a boundary-owner API consumed across modules. `public_flow_scan_contracts`
and `rust_source_scan_contracts` are fixture suites for scanners that protect
those boundaries; they are not a general architecture-spec language.
Prefer parser-backed `rust_source_scan` helpers for new Rust-source contracts;
extend the line-oriented public-flow scanner only for concrete public CLI
regressions that cannot be expressed through AST or call-path checks.

Before adding any new runtime-boundary scanner assertion, name the audited
failure class or stable public/import-boundary contract it protects. If the rule
only preserves a private helper layout, a child-module name, a line-count target,
or an incidental implementation shape, do not add the scanner. Delete duplicated
decisioning or test the public route/import boundary instead. Large-module
documentation is visibility debt tracking, not proof that a module is
semantically safe.

### Workflow Status Snapshot

workflow-status snapshot coverage for the ambiguous-spec route lives in `tests/workflow_runtime.rs` and is backed by `tests/fixtures/differential/workflow-status.json`. Treat any mismatch as a contract change that requires explicit fixture review.

### Eval Docs

`tests/evals/README.md` describes the active higher-level eval surfaces:

- the doc-driven `using-featureforge` routing gate
- the doc-driven Search Before Building gate
- opt-in Node-based `.eval.mjs` tests where a local judge run is still useful

## Change-Scoped Guidance

Editing skill templates or generated skill docs:

Run the mandatory release gates. While iterating, use the prompt surface budget
gate and `node scripts/run-codex-runtime-tests.mjs` as focused checks before the
full branch gate.

Editing brainstorm-server runtime scripts or launch wrappers:

```bash
npm --prefix tests/brainstorm-server test
```

Then run the mandatory release gates before release or task completion.

Editing reviewer sources or generated reviewer docs:

Run `node scripts/gen-agent-docs.mjs --check` while iterating, then run the
mandatory release gates.

Editing workflow routing, runtime docs, or execution contracts:

Run the mandatory release gates. Use the focused runtime audit aids above only
to close a known failure before the full branch gate.

## Runtime Churn Cutover Validation

Runtime churn fixes must prove that public routing advances, reports a precise
diagnostic, or returns `already_current` without mutating approved files or
repo-local projection exports. Use targeted iteration while repairing a known
failure. For final cutover proof, run the mandatory release gates plus these
runtime-churn extras:

```bash
node scripts/verify-source-archive.mjs
scripts/run-public-runtime-flow-tests.sh
scripts/run-internal-runtime-compatibility-tests.sh
node scripts/lint-workspace-runtime-evidence.mjs
cargo test --test liveness_model_checker
```

In that cutover list, `cargo test --test liveness_model_checker` is the
internal semantic convergence check. It must remain paired with the public
runtime flow script and the mandatory full nextest gate before any
shipped-runtime claim.

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
cargo test --test public_cli_flow_contracts -- public_test_files_do_not_use_internal_helpers_or_hidden_commands
```

The public replay suite (`tests/public_replay_churn.rs`) is part of the
targeted runtime matrix and the full nextest suite. It must continue to run
through the compiled public CLI only, reject hidden command/flag use in the
test wrapper itself, and preserve command-budget assertions for known churn
dead ends.

Synthetic replay setup boundaries are guarded by
`public_test_files_do_not_use_internal_helpers_or_hidden_commands`: protected
public-flow files may call event-log `_for_tests` APIs only from registered
fixture files and only inside helpers named with the explicit `synthetic_`
prefix. The scanner maps that marker to the `synthetic_fixture_setup` category;
do not add per-helper exceptions for ordinary setup code. Add a new fixture file
to the category only when the state cannot be produced by shipped public
commands and the test body validates public recovery after setup.

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
The runtime-executed representative subset must include the critical stuck-state
families that historically caused loops: current/stale overlap, cycle-break,
targetless stale, downstream stale, downstream interruption, and downstream
stale plus interruption. Remaining production-loop rows may be fixture-only
model coverage when they are explicitly named as such.

Normal `begin`, `complete`, `reopen`, `transfer`, `close-current-task`,
`repair-review-state`, `advance-late-stage`, `workflow operator`, and
`plan execution status` commands must leave approved plan/evidence/review files
and repo-local projection exports untouched. Runtime read models live under the
state directory. Diagnostic materialization is state-dir-only by default:

```bash
$_FEATUREFORGE_BIN plan execution materialize-projections --plan <approved-plan-path> --scope execution|late-stage|all
```

Repo-local human-readable projection exports are Git-visible and require an
explicit confirmed export:

```bash
$_FEATUREFORGE_BIN plan execution materialize-projections --plan <approved-plan-path> --scope execution|late-stage|all --repo-export --confirm-repo-export
```

Materialization is never required for normal progress. Confirmed repo exports
write projection-only files without modifying approved plan or evidence files,
and must not be recommended by operator routing as required progress.

Historical final-remediation plans used targeted Rust subsets while closing
specific failures. For branch proof, task-completion gates, plan-task review
loops, and pre-merge verification, use the mandatory release gates instead.

Targeted `cargo nextest run --test ...` commands are local debugging tools only. Do not use them as the documented final gate.

Editing runtime strategy-checkpoint, topology recommendation, or final-review deviation contracts:

Run the mandatory release gates. Use targeted workflow/runtime shards only for
local failure isolation.

Editing install or update surfaces:

Run the mandatory release gates and the installed-control-plane isolation gate
when provenance, routing, generated skills, or workspace-runtime isolation can
be affected.

Editing packaging or prebuilt artifact refresh flows:

Run the mandatory release gates. When checked-in prebuilts are part of the
change, also refresh and verify them explicitly.

Prebuilt refresh commands:

```bash
FEATUREFORGE_PREBUILT_TARGET=darwin-arm64 scripts/refresh-prebuilt-runtime.sh
PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc FEATUREFORGE_PREBUILT_TARGET=windows-x64 FEATUREFORGE_PREBUILT_RUST_TARGET=x86_64-pc-windows-gnu scripts/refresh-prebuilt-runtime.sh
node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .
```

The full provenance verifier always validates the manifest, source fingerprint,
binary hash/checksum provenance, and denied-string audit. Public help execution
runs for the manifest target matching the host, or for the explicit `--target`
when it matches the host. It also probes the root checked-in `bin/featureforge`
surface when that root binary target matches the host. On incompatible targets
the verifier runs `file`, emits a structured help-skip reason, and continues if
the non-execution checks are clean.

If Homebrew `cargo`/`rustc` shadow rustup-managed toolchains on `PATH`, put the rustup toolchain shims first before running the Windows GNU refresh command so the installed `x86_64-pc-windows-gnu` standard library can be found. The GNU cross-build also expects `x86_64-w64-mingw32-gcc` to be available on `PATH`.

Then rerun the mandatory release gates.

## Repo Fixtures

Keep workflow fixtures under `tests/codex-runtime/fixtures/workflow-artifacts/`. They are the stable contract inputs for route-time header parsing and approved-plan linkage tests.
