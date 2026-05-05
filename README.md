# FeatureForge

FeatureForge is a workflow system for coding agents. It combines a small Rust runtime with a checked-in skill library so planning, execution, review, and finish gates stay grounded in repo-visible artifacts instead of free-form prompt drift.

The active runtime package in this repository targets Codex and GitHub Copilot local installs.

## Provenance

FeatureForge began from upstream Superpowers: <https://github.com/obra/superpowers>

This repository keeps the workflow-first core and extends it with additional review, execution, and runtime patterns adapted from gstack: <https://github.com/garrytan/gstack>

## How It Works

Seven layers matter:

- `using-featureforge` is the human-readable entry router that consults `$_FEATUREFORGE_BIN workflow` directly from repo-visible artifacts.
- generated skill preambles always invoke the packaged install binary under `~/.featureforge/install/bin/` (`featureforge` on Unix, `featureforge.exe` on Windows), and that runtime resolves the active root through `featureforge repo runtime-root --path` before update checks or contributor-mode lookups.
- `$_FEATUREFORGE_BIN workflow` owns product-work routing up to `implementation_ready`.
- `$_FEATUREFORGE_BIN workflow doctor --plan <approved-plan-path> --json` is the first orientation/diagnosis surface after handoff; `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` remains the authoritative routing surface, and `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` is for deeper diagnostics.
- `$_FEATUREFORGE_BIN repo-safety` owns protected branches and repo-write guarantees.
- `$_FEATUREFORGE_BIN plan contract` owns semantic traceability between approved specs, approved plans, and derived task packets.
- `$_FEATUREFORGE_BIN plan execution` owns execution state after an approved plan is handed off.

Execution authority is event-only:

- for this repository's shipped work packages, approved specs and plans are preserved under `docs/archive/featureforge/specs/*.md` and `docs/archive/featureforge/plans/*.md`
- for new FeatureForge-managed project work, approved specs and plans still live under `docs/featureforge/specs/*.md` and `docs/featureforge/plans/*.md`
- normal runtime commands render current read models under the runtime state directory; explicit materialization writes repo-local human-readable exports under `docs/featureforge/projections/` instead of mutating approved plan or evidence files, and materialization is never required for normal progress
- once plan execution starts, branch execution truth is the append-only event log under the harness branch root (`execution-harness/events.jsonl`)
- `state.json`, approved-plan checklist marks, execution evidence, release/readiness/review/QA markdown, and strategy displays are deterministic projections/read models
- deleting, exporting, or regenerating those projections must not change operator routing, status, review-state repair, or mutator legality
- use `$_FEATUREFORGE_BIN plan execution materialize-projections --plan <approved-plan-path> --scope execution|late-stage|all` for state-dir-only diagnostic projection refreshes; add `--repo-export --confirm-repo-export` only when a repo-local human-readable projection export is explicitly needed; approved plan and evidence files are not modified, and materialization is never required for normal progress
- runtime-owned reviewed-closure, milestone, dispatch-lineage, and strategy facts are reduced from the event log for routing and gates
- branch-scoped local projections live under `~/.featureforge/projects/<repo-slug>/<user>-<safe-branch>-workflow-state.json`

## Installation

FeatureForge uses a single shared checkout for its supported runtime surfaces. Codex and GitHub Copilot local installs both point at `~/.featureforge/install`; only the discovery links differ.

Shared layout:

- `~/.featureforge/install` for the canonical checkout
- `~/.agents/skills/featureforge -> ~/.featureforge/install/skills`
- `~/.copilot/skills -> ~/.featureforge/install/skills`
- `~/.codex/agents/code-reviewer.toml -> ~/.featureforge/install/.codex/agents/code-reviewer.toml`
- `~/.copilot/agents/code-reviewer.agent.md -> ~/.featureforge/install/agents/code-reviewer.md`

## Installed Control Plane

Live workflow execution uses the installed control plane only:

- installed runtime: `~/.featureforge/install/bin/featureforge` (or `featureforge.exe` on Windows)
- installed skills: `~/.featureforge/install/skills`
- active FeatureForge skill discovery roots must resolve to the installed skills directory
- `<repo>/skills/*` in this checkout are generated product artifacts under test, not active discovery roots

Workspace-local runtimes are test subjects only. `./bin/featureforge`,
`target/debug/featureforge`, and `cargo run -- ...` may be used for fixture and
smoke tests only when `FEATUREFORGE_STATE_DIR` points at isolated temp or
fixture state.

Workspace-local runtimes must not mutate live workflow state, review state,
execution state, projections, workflow artifacts, or event logs under
`~/.featureforge`. The runtime fails closed for live mutating commands when the
invoked binary is workspace-local. The override
`FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1` is intentionally explicit
and should almost never be used; any approved use must be visible in execution
evidence and review provenance.

Inspect the active boundary with:

```bash
~/.featureforge/install/bin/featureforge doctor self-hosting --json
```

Runtime diagnostics also expose runtime provenance and skill-root provenance
under `runtime_provenance.skill_discovery` so workspace-root drift can be
detected.

Detailed install docs:

- Codex: [docs/README.codex.md](docs/README.codex.md)
- GitHub Copilot: [docs/README.copilot.md](docs/README.copilot.md)
- Checked-in install instructions: [.codex/INSTALL.md](.codex/INSTALL.md) and [.copilot/INSTALL.md](.copilot/INSTALL.md)

## Runtime State

Runtime state lives in `~/.featureforge/`.

- preferences: `~/.featureforge/config/config.yaml`
- session markers: `~/.featureforge/sessions/`
- contributor field reports: `~/.featureforge/contributor-logs/`
- project-scoped artifacts and workflow manifests: `~/.featureforge/projects/`

The repo-local default config for this checkout lives at `.featureforge/config.yaml`.

## Workflow

Default pipeline:

`featureforge:brainstorming -> featureforge:plan-ceo-review -> featureforge:writing-plans -> featureforge:plan-eng-review`; `featureforge:plan-fidelity-review` runs only after engineering-review edits are complete, then `featureforge:plan-eng-review` performs final approval before implementation.

Planning chain in plain language:

`brainstorming -> plan-ceo-review -> writing-plans -> plan-eng-review`; `plan-fidelity-review` runs only after engineering-review edits are complete, then `plan-eng-review` performs final approval before implementation.

The generated `using-featureforge` skill calls `$_FEATUREFORGE_BIN workflow doctor --plan <approved-plan-path> --json` first when an approved plan path is already known, then calls `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` for authoritative routing. If no approved plan path is known, resolve it through the normal planning/review handoff before invoking doctor/operator.

Execution starts from an engineering-approved plan and the exact approved plan path.
Use `$_FEATUREFORGE_BIN workflow doctor --plan <approved-plan-path>` for the compact human dashboard and `$_FEATUREFORGE_BIN workflow doctor --plan <approved-plan-path> --json` for headless diagnostics; use `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` as the normal routing authority, then follow the recommended intent-level argv vector for the current phase. The public execution surface is `begin`, `complete`, `reopen`, `transfer`, `close-current-task`, `repair-review-state`, and `advance-late-stage`. Late-stage public JSON reports `intent=advance_late_stage` plus a semantic `operation`; do not infer or invoke lower-level recording primitives from output fields.

When workflow/operator reports stale or missing closure context, do not invent a repair command. If `recommended_public_command_argv` is present, invoke it exactly except for installed-control-plane rebinding: when argv[0] is `featureforge`, execute via `~/.featureforge/install/bin/featureforge` while preserving argv[1..]. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic. Otherwise satisfy `required_inputs` or run `$_FEATUREFORGE_BIN plan execution repair-review-state --plan <approved-plan-path>` only when the non-diagnostic route owns that repair lane.

After `repair-review-state`, treat that command's own `recommended_public_command_argv` as the immediate reroute when present and complete that follow-up before running any extra command. If argv is absent and `next_action` is `runtime diagnostic required`, stop on the diagnostic; otherwise satisfy the typed `required_inputs` or prerequisite named by `next_action`, then rerun the command that owns the route. `recommended_command` is display-only compatibility text; do not parse it for invocation. Use `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` only when you need additional diagnostic detail.
Do not manually edit `**Execution Note:**` lines to recover runtime state; execution-note markdown is projection-only.
Do not repair runtime routing by editing tracked plan, evidence, review, readiness, QA, or strategy projection files. They are export artifacts; the event log and reducer-owned state are authoritative.

`$_FEATUREFORGE_BIN plan execution` is the execution preflight boundary for the approved plan.

Task closure is enforced at task boundaries, not only at the end of the full plan:

- Task `N+1` may begin only after Task `N` has a current positive task-closure record.
- dedicated-independent review loops and verification are inputs to `close-current-task`; they are not separate begin-time authority once a current positive closure exists
- after implementation steps complete and review plus verification are ready, run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready` and use `close-current-task` as the authoritative task-closure command
- if workflow/operator reports `task_review_dispatch_required` or `final_review_dispatch_required`, keep the normal path on workflow/operator plus the intent-level commands; do not route the normal path through low-level dispatch primitives
- compatibility/debug command boundaries (`gate-*`, low-level `record-*`) must not be required in the normal path
- task-boundary remediation churn is capped with runtime-owned `cycle_break` handling on repeated loops
- after review passes, task verification is required before the task can close and before next-task advancement
- `repair-review-state` returns one exact next command as `recommended_public_command_argv` when all inputs are already bound; follow that argv directly except for installed-control-plane rebinding (`featureforge` argv[0] must execute as `~/.featureforge/install/bin/featureforge`), stop when argv is absent with `next_action=runtime diagnostic required`, otherwise satisfy `required_inputs` and rerun the route owner
- once approved-plan execution has started, execution-phase implementation/review subagent dispatch is authorized without per-dispatch user-consent prompts

Completion then flows through (runtime-owned late-stage sequencing keeps `featureforge:document-release` ahead of terminal `featureforge:requesting-code-review`):

- `featureforge:document-release`
- `featureforge:requesting-code-review` (terminal final review gate after document release)
- `featureforge:qa-only` only when authoritative `QA Requirement` routing for the current plan requires it
- `featureforge:finishing-a-development-branch`

## Project Memory

`featureforge:project-memory` is an optional support skill for maintaining `docs/project_notes/*`.

- It records supportive memory only and never outranks approved specs, approved plans, execution evidence, review artifacts, or runtime state.
- It is not a workflow stage, approval gate, or mandatory part of the default planning/execution stack.
- Use it for explicit memory-oriented requests or later follow-up memory updates, not as a substitute for the active workflow owner.

### Runtime Strategy Checkpoints

Execution strategy checkpoints are runtime-owned execution state, not planning-stage transitions.

- `initial_dispatch` is required before repo-writing execution dispatch
- `review_remediation` is recorded automatically when reviewable dispatch lineage enters remediation and when remediation reopens execution work
- `cycle_break` is recorded automatically when the same task reaches three reviewable dispatch/remediation cycles

The approved plan path/revision remains fixed during execution. Runtime strategy may adjust topology, lane/worktree allocation, and remediation order without sending the workflow back to planning stages.

The runtime records checkpoint history in the authoritative event log and renders `strategy_checkpoints` into projection state for `plan execution status`. Runtime-owned review projections are trace artifacts only; task advancement is governed by current positive task-closure records and reducer-owned state.

Use `$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` to inspect:

- `strategy_state`
- `strategy_checkpoint_kind`
- `last_strategy_checkpoint_fingerprint`
- `strategy_reset_required`

Reviewers should treat this strategy-checkpoint layer as intentional runtime contract hardening. Do not remove it as "out of plan" cleanup when the implementation and tests prove runtime-owned enforcement behavior.

## Repository Layout

- `skills/` holds the checked-in public skills and their templates
- `agents/` holds generated reviewer artifacts and reviewer source material
- `review/` holds shared review references
- `docs/featureforge/` holds reference docs and workflow support material for this package
- `docs/archive/` holds preserved historical project artifacts, including the shipped approved specs, plans, and execution evidence for this repo
- `tests/codex-runtime/fixtures/workflow-artifacts/` holds stable workflow-fixture inputs used by routing and contract tests

## Development

Regenerate generated docs after editing templates or reviewer sources:

```bash
node scripts/gen-skill-docs.mjs
node scripts/gen-agent-docs.mjs
```

The canonical deterministic validation matrix and change-scoped commands live in [docs/testing.md](docs/testing.md).

Core validation:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node scripts/run-codex-runtime-tests.mjs
node --test tests/evals/review-accelerator-contract.eval.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets --all-features --no-fail-fast
```

Installed-control-plane isolation changes use the dedicated gate:

```bash
scripts/verify-installed-control-plane-isolation.sh
```

Runtime release checks keep public-flow proof separate from internal
compatibility coverage:

```bash
scripts/run-public-runtime-flow-tests.sh
scripts/run-internal-runtime-compatibility-tests.sh
```

The Rust verification command is intentionally the full nextest suite. It covers
more than 1100 tests; use targeted `cargo nextest run --test ...` commands only
while iterating on a known failure, then rerun the full command before claiming a
task or branch is green. Keep `--no-fail-fast` so the run reports the complete
failure set.

Full Rust suite through the optional sharded helper, for explicit local
performance investigations or when a CI/job specifically asks for it:

```bash
scripts/run-rust-tests-sharded.sh 8
```

Refresh checked-in prebuilt binaries (release-facing artifacts) when runtime packaging or binary surfaces change:

```bash
FEATUREFORGE_PREBUILT_TARGET=darwin-arm64 scripts/refresh-prebuilt-runtime.sh
PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc FEATUREFORGE_PREBUILT_TARGET=windows-x64 FEATUREFORGE_PREBUILT_RUST_TARGET=x86_64-pc-windows-gnu scripts/refresh-prebuilt-runtime.sh
node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .
```

Full prebuilt verification always checks the manifest, source fingerprint,
binary hashes, and denied public/control-plane strings. It executes
`--help`, `plan execution --help`, and `workflow --help` for the manifest target
matching the current host, or for the explicit `--target` when it matches the
host. It also probes the root checked-in `bin/featureforge` surface when that
root binary target matches the host. Incompatible target binaries are inspected
with `file` and reported with a structured help-skip reason instead.

If Homebrew `cargo`/`rustc` shadow rustup-managed toolchains on your `PATH`, make sure the rustup toolchain shims are ahead of Homebrew Rust before running the Windows GNU refresh command so the installed `x86_64-pc-windows-gnu` standard library is visible. The GNU cross-build also expects `x86_64-w64-mingw32-gcc` to be available on `PATH`.

## Updating

Update the shared checkout used by supported local installs:

```bash
git -C ~/.featureforge/install pull
```

If your platform copies the reviewer artifact instead of symlinking it, refresh that copied file after updating.

## Support

Open an issue in the repository that hosts this checkout, or start with the checked-in install docs and [docs/testing.md](docs/testing.md).
