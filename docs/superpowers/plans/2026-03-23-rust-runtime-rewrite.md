# Superpowers Rust Runtime Rewrite Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use `superpowers:subagent-driven-development` when isolated-agent workflows are available in the current platform/session; otherwise use `superpowers:executing-plans`. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/superpowers/specs/2026-03-23-rust-runtime-rewrite-design.md`
**Source Spec Revision:** 3
**Last Reviewed By:** plan-eng-review

**Goal:** Replace the shell-heavy Superpowers runtime with one Rust-backed CLI that preserves markdown authority, current workflow law, and fail-closed behavior while shifting normal installation to checked-in prebuilt binaries under `bin/prebuilt/`.

**Architecture:** Add one root Rust crate with typed modules for CLI dispatch, workflow, contracts, execution, repo safety, session entry, config, update-check, and install migration. Use existing shell and PowerShell helpers only as temporary migration shims while repo-owned callers, tests, and docs move to canonical `superpowers ...` commands, then finish with checked-in macOS arm64 and Windows x64 binaries plus integrity metadata, all keyed by one checked-in `bin/prebuilt/manifest.json` install contract.

**Tech Stack:** Rust stable, `clap`, `serde`, `serde_json`, `schemars`, `gix`, `camino`, `sha2`, `semver`, `reqwest`, `jiff`, `thiserror`, `fs-err`, `tempfile`, `assert_cmd`, `insta`, `proptest`, `criterion`, Cargo, `cargo-nextest`, `cargo-llvm-cov`, `cargo-deny`, `cargo-audit`, existing shell regression tests, Node-based skill-doc and fixture-contract tests

---

## What Already Exists

- `bin/superpowers-plan-contract`, `bin/superpowers-plan-execution`, `bin/superpowers-workflow-status`, `bin/superpowers-repo-safety`, `bin/superpowers-session-entry`, `bin/superpowers-config`, `bin/superpowers-update-check`, `bin/superpowers-slug`, and `bin/superpowers-migrate-install` already encode the runtime contract in shell.
- `tests/codex-runtime/` already contains the most important compatibility corpus for workflow, contract parsing, execution, repo safety, session entry, wrappers, and install behavior.
- `README.md`, `docs/README.codex.md`, `docs/README.copilot.md`, `docs/testing.md`, and skill templates already document the operator-facing runtime surface that the Rust rewrite must preserve or explicitly clean up.
- The approved spec already defines the target repository layout, subsystem boundaries, helper-owned state layout, migration rules, and packaging contract for checked-in prebuilt binaries.

## Existing Capabilities / Built-ins to Reuse

- Reuse the current shell regression suites and fixtures as the first parity gate instead of inventing a brand-new test corpus.
- Reuse the approved spec's crate and module boundaries; do not split the runtime into multiple shipped binaries or helper-specific crates.
- Reuse Cargo-native tooling for linting, tests, coverage, dependency review, and deterministic builds.
- Reuse the existing README and skill-doc contract as the truth source for public command names and behavior conflicts.

## Known Footguns / Constraints

- Do not shell out to `git`; all repository identity and branch logic must move to `gix` or equivalent library APIs.
- Do not rely on archived `serde_yaml`; keep YAML on disk, but choose a maintained parser path during implementation.
- Do not let temporary shims become a second public contract. They are migration-only and must stay thin.
- Do not broaden scope into brainstorm-server, eval harnesses, or unrelated Node tooling; catalog them and verify non-regression only where the runtime boundary touches them.
- Do not assume one Apple Silicon host can produce every checked-in target binary. The macOS arm64 and Windows x64 prebuilt artifacts must be refreshed from matching target hosts or an explicitly proven cross-compile setup.

## Change Surface

- New Rust workspace manifests and source tree at repo root
- Existing shell helpers in `bin/` converted into migration shims or removed from the supported install surface
- New schema files under `schemas/`
- New Rust integration, snapshot, property, and differential verification
- Updates to skill docs, command docs, operator docs, testing docs, and release notes
- New checked-in binaries, install manifest, and integrity metadata under `bin/prebuilt/`

## Planned File Structure

- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.cargo/config.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`
- Create: `src/cli/mod.rs`
- Create: `src/compat/mod.rs`
- Create: `src/compat/argv0.rs`
- Create: `src/diagnostics/mod.rs`
- Create: `src/output/mod.rs`
- Create: `src/paths/mod.rs`
- Create: `src/git/mod.rs`
- Create: `src/instructions/mod.rs`
- Create: `src/workflow/mod.rs`
- Create: `src/workflow/manifest.rs`
- Create: `src/workflow/status.rs`
- Create: `src/contracts/mod.rs`
- Create: `src/contracts/spec.rs`
- Create: `src/contracts/plan.rs`
- Create: `src/contracts/packet.rs`
- Create: `src/contracts/evidence.rs`
- Create: `src/execution/mod.rs`
- Create: `src/execution/state.rs`
- Create: `src/execution/mutate.rs`
- Create: `src/repo_safety/mod.rs`
- Create: `src/session_entry/mod.rs`
- Create: `src/config/mod.rs`
- Create: `src/update_check/mod.rs`
- Create: `src/install/mod.rs`
- Create: `src/cli/workflow.rs`
- Create: `src/cli/plan_execution.rs`
- Create: `src/cli/repo_safety.rs`
- Create: `src/cli/session_entry.rs`
- Create: `src/cli/config.rs`
- Create: `src/cli/slug.rs`
- Create: `src/cli/update_check.rs`
- Create: `src/cli/install.rs`
- Create: `compat/bash/superpowers`
- Create: `compat/powershell/superpowers.ps1`
- Create: `schemas/workflow-status.schema.json`
- Create: `schemas/workflow-resolve.schema.json`
- Create: `schemas/plan-contract-analyze.schema.json`
- Create: `schemas/plan-contract-packet.schema.json`
- Create: `schemas/plan-execution-status.schema.json`
- Create: `schemas/repo-safety-check.schema.json`
- Create: `schemas/session-entry-resolve.schema.json`
- Create: `schemas/update-check.schema.json`
- Create: `tests/bootstrap_smoke.rs`
- Create: `tests/paths_identity.rs`
- Create: `tests/instructions_and_git.rs`
- Create: `tests/contracts_spec_plan.rs`
- Create: `tests/packet_and_schema.rs`
- Create: `tests/workflow_runtime.rs`
- Create: `tests/plan_execution.rs`
- Create: `tests/repo_safety.rs`
- Create: `tests/session_config_slug.rs`
- Create: `tests/update_and_install.rs`
- Create: `tests/differential/README.md`
- Create: `tests/differential/run_legacy_vs_rust.sh`
- Create: `tests/fixtures/differential/workflow-status.json`
- Create: `benches/workflow_status.rs`
- Create: `benches/plan_contract.rs`
- Create: `benches/execution_status.rs`
- Create: `perf-baselines/runtime-hot-paths.json`
- Create: `scripts/check-runtime-benchmarks.sh`
- Create: `scripts/refresh-prebuilt-runtime.sh`
- Create: `scripts/refresh-prebuilt-runtime.ps1`
- Create: `bin/prebuilt/manifest.json`
- Create: `bin/prebuilt/darwin-arm64/superpowers`
- Create: `bin/prebuilt/darwin-arm64/superpowers.sha256`
- Create: `bin/prebuilt/windows-x64/superpowers.exe`
- Create: `bin/prebuilt/windows-x64/superpowers.exe.sha256`
- Modify: `bin/superpowers-plan-contract`
- Modify: `bin/superpowers-plan-contract.ps1`
- Modify: `bin/superpowers-plan-execution`
- Modify: `bin/superpowers-plan-execution.ps1`
- Modify: `bin/superpowers-workflow`
- Modify: `bin/superpowers-workflow.ps1`
- Modify: `bin/superpowers-workflow-status`
- Modify: `bin/superpowers-workflow-status.ps1`
- Modify: `bin/superpowers-repo-safety`
- Modify: `bin/superpowers-repo-safety.ps1`
- Modify: `bin/superpowers-session-entry`
- Modify: `bin/superpowers-session-entry.ps1`
- Modify: `bin/superpowers-config`
- Modify: `bin/superpowers-config.ps1`
- Modify: `bin/superpowers-slug`
- Modify: `bin/superpowers-update-check`
- Modify: `bin/superpowers-update-check.ps1`
- Modify: `bin/superpowers-migrate-install`
- Modify: `bin/superpowers-migrate-install.ps1`
- Modify: `README.md`
- Modify: `docs/README.codex.md`
- Modify: `docs/README.copilot.md`
- Modify: `docs/testing.md`
- Modify: `RELEASE-NOTES.md`
- Modify: `commands/brainstorm.md`
- Modify: `commands/write-plan.md`
- Modify: `commands/execute-plan.md`
- Modify: `skills/brainstorming/SKILL.md.tmpl`
- Modify: `skills/brainstorming/SKILL.md`
- Modify: `skills/using-superpowers/SKILL.md.tmpl`
- Modify: `skills/using-superpowers/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `tests/codex-runtime/test-superpowers-workflow-status.sh`
- Modify: `tests/codex-runtime/test-superpowers-workflow.sh`
- Modify: `tests/codex-runtime/test-superpowers-plan-contract.sh`
- Modify: `tests/codex-runtime/test-superpowers-plan-execution.sh`
- Modify: `tests/codex-runtime/test-superpowers-repo-safety.sh`
- Modify: `tests/codex-runtime/test-superpowers-session-entry.sh`
- Modify: `tests/codex-runtime/test-superpowers-session-entry-gate.sh`
- Modify: `tests/codex-runtime/test-superpowers-config.sh`
- Modify: `tests/codex-runtime/test-superpowers-slug.sh`
- Modify: `tests/codex-runtime/test-superpowers-update-check.sh`
- Modify: `tests/codex-runtime/test-superpowers-migrate-install.sh`
- Modify: `tests/codex-runtime/test-superpowers-upgrade-skill.sh`
- Modify: `tests/codex-runtime/test-runtime-instructions.sh`
- Modify: `tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
- Modify: `tests/codex-runtime/workflow-fixtures.test.mjs`
- Modify: `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-generation.test.mjs`
- Modify: `tests/brainstorm-server/test-launch-wrappers.sh`
- Modify: `tests/codex-runtime/fixtures/plan-contract/valid-spec.md`
- Modify: `tests/codex-runtime/fixtures/plan-contract/valid-plan.md`

## Artifact Ownership / Migration Inventory

- `~/.superpowers/projects/.../workflow-state.json`
  Owner: `src/workflow/manifest.rs`
  Strategy: rebuild or lazily upgrade derived state; back up corrupt files before replacement.
- `~/.superpowers/session-flags/using-superpowers/<ppid>` and `~/.superpowers/session-entry/using-superpowers/<ppid>`
  Owner: `src/session_entry/mod.rs`
  Strategy: read legacy path as migration input, rewrite canonical state into the stable subsystem path.
- `~/.superpowers/config.yaml` and `~/.superpowers/config/config.yaml`
  Owner: `src/config/mod.rs`
  Strategy: explicit migration with backup through `superpowers install migrate`; keep YAML on disk.
- `~/.superpowers/repo-safety/approvals/...`
  Owner: `src/repo_safety/mod.rs`
  Strategy: migrate parseable approvals forward, back up unmigratable ones, fail closed only for unreadable scope.
- `~/.superpowers/update-check/...`
  Owner: `src/update_check/mod.rs`
  Strategy: rebuild cache or lazily upgrade non-destructive derived state.
- `~/.superpowers/install/...`
  Owner: `src/install/mod.rs`
  Strategy: own install validation, backup markers, prebuilt-binary provisioning, and explicit migration reporting.
- Repo-visible specs, plans, packets, and evidence
  Owner: `src/contracts/{spec,plan,packet,evidence}.rs`
  Strategy: preserve exact authoritative paths and headers; never migrate authority into helper-owned state.

## Preconditions

- Work from `dm/rust-rewrite`, not `main`.
- Treat [2026-03-23-rust-runtime-rewrite-design.md](/Users/dmulcahey/development/skills/superpowers/docs/superpowers/specs/2026-03-23-rust-runtime-rewrite-design.md) revision `3` as the exact source contract for this plan.
- Execute every implementation task with `superpowers:test-driven-development`.
- Finish each task with targeted verification before moving on, and use `superpowers:verification-before-completion` before any success claim or merge proposal.
- Keep markdown authoritative throughout; helper-owned state may be migrated or rebuilt, but it may never become approval truth.
- Keep helper-owned local state file-based under `~/.superpowers/`; do not introduce SQLite or a service.
- Keep any migration wrappers thin and temporary; they may not own business logic or JSON mutation.
- Build checked-in prebuilt binaries only for macOS arm64 and Windows x64 in the initial cutover.

## Not In Scope

- Replacing specs, plans, evidence, or approvals with a database or hidden service
- Porting brainstorm-server, eval harnesses, or unrelated Node tooling into Rust
- Turning helper shims into long-term supported installed executables
- Redesigning stable CLI output just because the implementation language changes
- Requiring a local Rust or Cargo build, or a remote artifact fetch, during normal skills installation on supported targets

## Execution Strategy

1. Bootstrap the Rust workspace and strict crate boundary first so every later task lands inside the approved module layout.
2. Move correctness-sensitive primitives into typed Rust modules before porting user-visible commands.
3. Port contract parsing and workflow state before execution, policy, and migration logic so later tasks can reuse shared types instead of duplicating parsing.
4. Port execution and local-state subsystems next, with migration rules built into the command design instead of bolted on afterward.
5. Convert shell helpers into thin migration shims only after canonical Rust subcommands exist and are covered by tests.
6. Cut repo-owned callers, docs, and skill instructions to canonical `superpowers ...` commands only after differential and parity coverage are in place.
7. Refresh and commit supported target binaries last, after the command surface, tests, and docs are stable enough to represent the cutover release.

## Evidence Expectations

- Every runtime port task must leave at least one new Rust test and one targeted parity assertion in the existing shell suite where that shell contract still matters.
- Every machine-readable command family must leave checked-in schemas and snapshot-backed output proof.
- Every helper-owned state migration path must leave backup, rollback, and fail-closed coverage.
- Every temporary shim or wrapper path must leave explicit transport-only proof; no behavior should depend on wrapper-side rewriting.
- Final cutover evidence must show green targeted tests, reviewed differential results, and checked-in prebuilt binaries plus integrity metadata for supported targets.
- The checked-in prebuilt-binary flow must leave one authoritative manifest that maps supported targets to binary path, checksum path, and runtime revision.
- Final cutover evidence must include separate fresh-install proof from a macOS arm64 host and a Windows x64 host, each validating host-target resolution, checksum verification, install into `~/.superpowers/install/bin`, and direct `superpowers` invocation.
- Final cutover evidence must include benchmark results for workflow status, plan-contract parsing, and execution status against checked-in thresholds in `perf-baselines/runtime-hot-paths.json`, with any threshold changes reviewed in the same change.

## Validation Strategy

At minimum, this plan should finish with these passing commands:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run
cargo llvm-cov nextest --workspace --all-features --lcov --output-path target/lcov.info
cargo deny check
cargo audit
node scripts/gen-skill-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
bash tests/codex-runtime/test-superpowers-workflow-status.sh
bash tests/codex-runtime/test-superpowers-workflow.sh
bash tests/codex-runtime/test-superpowers-plan-contract.sh
bash tests/codex-runtime/test-superpowers-plan-execution.sh
bash tests/codex-runtime/test-superpowers-repo-safety.sh
bash tests/codex-runtime/test-superpowers-session-entry.sh
bash tests/codex-runtime/test-superpowers-session-entry-gate.sh
bash tests/codex-runtime/test-superpowers-config.sh
bash tests/codex-runtime/test-superpowers-slug.sh
bash tests/codex-runtime/test-superpowers-update-check.sh
bash tests/codex-runtime/test-superpowers-migrate-install.sh
bash tests/codex-runtime/test-superpowers-upgrade-skill.sh
bash tests/codex-runtime/test-runtime-instructions.sh
bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh
bash tests/brainstorm-server/test-launch-wrappers.sh
bash scripts/check-runtime-benchmarks.sh
# plus fresh-install verification on macOS arm64 and Windows x64 hosts
```

## Documentation Update Expectations

- Update `README.md` to describe the Rust runtime, canonical command tree, checked-in binary install story, and migration-only shim stance.
- Update `docs/README.codex.md` and `docs/README.copilot.md` so operator guidance uses canonical `superpowers ...` commands rather than helper-style names.
- Update `docs/testing.md` with the Cargo-based verification flow, differential harness, benchmark-threshold suite, and supported-target prebuilt-binary refresh steps.
- Update `RELEASE-NOTES.md` with the Rust cutover summary, compatibility exceptions if any, and the checked-in prebuilt-binary packaging contract.
- Regenerate touched `SKILL.md` outputs from their templates after changing command references.

## Rollout Plan

- Land the Rust workspace and shared foundations first without changing the public install surface.
- Land subsystem ports behind canonical Rust subcommands before converting helper scripts into migration shims.
- Switch repo-owned callers, docs, skills, and tests to canonical commands in the same cutover slice that removes helper-style installed executables.
- Refresh and commit supported target binaries only when the parity suite is green and the canonical command surface is final.
- Treat the first Rust release as atomic: code, tests, docs, and checked-in binaries move together.

## Rollback Plan

- Revert the Rust workspace, shim changes, docs, tests, and checked-in binaries together if the cutover proves unstable.
- Restore helper-owned non-rebuildable local state from backups created by `superpowers install migrate`.
- Rebuild derived helper-owned state such as manifests and update caches when that is safer than restoring migrated files.
- Preserve repo-visible markdown artifacts and execution evidence unchanged during rollback.
- Keep differential mismatch artifacts so rollback analysis can explain exactly where the Rust behavior diverged.

## Risks And Mitigations

- Risk: the rewrite silently codifies shell bugs as product law.
  Mitigation: resolve behavior conflicts in the spec-defined order and require differential triage before cutover.
- Risk: helper shims linger and become a hidden second contract.
  Mitigation: keep them transport-only, switch repo-owned callers to canonical commands, and remove them from the installed surface.
- Risk: migration of helper-owned local state causes rollback pain.
  Mitigation: separate rebuildable from non-rebuildable state, back up before destructive rewrites, and verify rollback paths explicitly.
- Risk: checked-in binaries drift from source or the installer and refresh scripts disagree about target layout.
  Mitigation: script the refresh flow, check integrity metadata into git, store target mapping in one checked-in `bin/prebuilt/manifest.json`, and make manifest-driven binary refresh part of final verification.
- Risk: the project balloons into a generalized CLI redesign.
  Mitigation: preserve stable output defaults and treat any intentional output delta as an explicit compatibility exception.
- Risk: performance thresholds become noisy or meaningless if they are not tied to stable fixtures and a checked-in baseline contract.
  Mitigation: benchmark only the approved hot paths on fixed fixtures, store thresholds in `perf-baselines/runtime-hot-paths.json`, and review threshold changes like any other contract change.

## Diagrams

### Delivery Order

```text
Task 1 bootstrap
    |
    v
Task 2 foundations
    |
    v
Task 3 contracts + packets
    |
    v
Task 4 workflow
    |
    v
Task 5 execution
    |
    v
Task 6 policy + local state
    |
    v
Task 7 update-check + install-state migration
    |
    v
Task 8 prebuilt manifest + provisioning
    |
    v
Task 9 migration shims + installed-surface cleanup
    |
    v
Task 10 caller/doc/test cutover + differential harness
    |
    v
Task 11 checked-in binaries + final verification
```

### Runtime Boundary

```text
repo-visible markdown
    |
    +--> contracts::{spec,plan,packet,evidence}
    |
    +--> workflow -------------------+
    |                                |
    +--> execution ------------------+--> cli --> output / diagnostics
    |                                |
helper-owned local state ------------+
```

## Requirement Coverage Matrix

- REQ-001 -> Task 1, Task 9
- REQ-002 -> Task 4, Task 9, Task 10, Task 11
- REQ-003 -> Task 4, Task 5, Task 6, Task 7, Task 8, Task 9
- REQ-004 -> Task 3, Task 4, Task 5, Task 10
- REQ-005 -> Task 2, Task 10
- REQ-006 -> Task 2, Task 3, Task 4, Task 6
- REQ-007 -> Task 2, Task 6
- REQ-008 -> Task 4
- REQ-009 -> Task 4, Task 9
- REQ-010 -> Task 4, Task 10
- REQ-011 -> Task 4, Task 8
- REQ-012 -> Task 2, Task 3, Task 4, Task 5, Task 6, Task 7
- REQ-013 -> Task 3
- REQ-014 -> Task 3, Task 9
- REQ-015 -> Task 3
- REQ-016 -> Task 5, Task 9
- REQ-017 -> Task 5
- REQ-018 -> Task 3, Task 5
- REQ-019 -> Task 6
- REQ-020 -> Task 2, Task 6, Task 9
- REQ-021 -> Task 2, Task 4, Task 6
- REQ-022 -> Task 6
- REQ-023 -> Task 6
- REQ-024 -> Task 6, Task 7
- REQ-025 -> Task 7
- REQ-026 -> Task 7
- REQ-027 -> Task 2, Task 3, Task 4, Task 5, Task 6, Task 7, Task 8
- REQ-028 -> Task 9, Task 11
- REQ-029 -> Task 9, Task 11
- REQ-030 -> Task 10
- REQ-031 -> Task 1, Task 3, Task 4, Task 5, Task 6, Task 7, Task 8, Task 10, Task 11
- REQ-032 -> Task 11
- REQ-033 -> Task 1, Task 11
- REQ-034 -> Task 1, Task 10
- REQ-035 -> Task 2, Task 4, Task 10, Task 11
- REQ-036 -> Task 2, Task 6, Task 9
- REQ-037 -> Task 1
- REQ-038 -> Task 1, Task 2, Task 11
- REQ-039 -> Task 4, Task 9, Task 10
- REQ-040 -> Task 9
- REQ-041 -> Task 1
- REQ-042 -> Task 1
- REQ-043 -> Task 1, Task 4, Task 11
- REQ-044 -> Task 6, Task 7
- REQ-045 -> Task 6, Task 7
- REQ-046 -> Task 2, Task 3, Task 4, Task 5, Task 6, Task 7, Task 10
- REQ-047 -> Task 7
- REQ-048 -> Task 1, Task 7, Task 8, Task 11
- REQ-049 -> Task 7
- REQ-050 -> Task 6, Task 7
- REQ-051 -> Task 6, Task 7
- REQ-052 -> Task 6, Task 7
- REQ-053 -> Task 8, Task 11
- REQ-054 -> Task 8, Task 11
- NONGOAL-001 -> Task 3, Task 4, Task 5, Task 6
- NONGOAL-002 -> Task 1, Task 10
- NONGOAL-003 -> Task 1, Task 9
- NONGOAL-004 -> Task 4, Task 9
- NONGOAL-005 -> Task 8, Task 11
- VERIFY-001 -> Task 9, Task 10, Task 11
- VERIFY-002 -> Task 8, Task 11
- VERIFY-003 -> Task 10, Task 11
- VERIFY-004 -> Task 1, Task 11
- VERIFY-005 -> Task 6, Task 7, Task 11
- VERIFY-006 -> Task 3, Task 4, Task 5, Task 6, Task 7, Task 10
- VERIFY-007 -> Task 7, Task 11
- VERIFY-008 -> Task 7, Task 10
- VERIFY-009 -> Task 6, Task 7, Task 10
- VERIFY-010 -> Task 8, Task 11

## Task 1: Bootstrap the Rust Workspace and Runtime Skeleton

**Spec Coverage:** REQ-001, REQ-031, REQ-033, REQ-034, REQ-037, REQ-038, REQ-041, REQ-042, REQ-043, REQ-048, NONGOAL-002, NONGOAL-003, VERIFY-004
**Task Outcome:** The repo has a compiling Rust crate with the approved top-level module layout, pinned toolchain, baseline Cargo policy, and a first smoke test proving the `superpowers` binary exists as the single runtime entrypoint.
**Plan Constraints:**
- Keep one root crate and one shipped binary.
- Do not start porting shell logic in this task.
- Record adjacent Node-based surfaces for awareness, but do not let them block bootstrap work.
**Open Questions:** none

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.cargo/config.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`
- Create: `src/cli/mod.rs`
- Create: `tests/bootstrap_smoke.rs`

- [ ] **Step 1: Write `tests/bootstrap_smoke.rs` to assert that `superpowers --help` and `superpowers --version` exist and that the binary name is `superpowers`.**
- [ ] **Step 2: Run `cargo test --test bootstrap_smoke` and confirm it fails because the Rust workspace and CLI entrypoint do not exist yet.**
- [ ] **Step 3: Add `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `src/main.rs`, `src/lib.rs`, and `src/cli/mod.rs` with the approved dependency set, strict lint settings, and a minimal compiling CLI shell.**
- [ ] **Step 4: Run `cargo test --test bootstrap_smoke` and confirm it passes with one binary and no helper-per-binary split.**
- [ ] **Step 5: Commit with `git add Cargo.toml rust-toolchain.toml .cargo/config.toml src/main.rs src/lib.rs src/cli/mod.rs tests/bootstrap_smoke.rs && git commit -m "chore: bootstrap rust runtime workspace"`**

## Task 2: Add Shared Primitives, Diagnostics, and Guardrails

**Spec Coverage:** REQ-005, REQ-006, REQ-007, REQ-020, REQ-021, REQ-027, REQ-035, REQ-036, REQ-038, REQ-046
**Task Outcome:** The Rust runtime has typed primitives for repo-path identity, diagnostics, output plumbing, repository identity, instruction discovery, and argv0 compatibility resolution, with fail-closed behavior proven before user-visible command ports begin.
**Plan Constraints:**
- Keep `git` access library-only.
- Preserve repo-relative identity in normalized form.
- Keep diagnostics stable enough to carry current reason and failure classes forward.
**Open Questions:** none

**Files:**
- Create: `src/compat/mod.rs`
- Create: `src/compat/argv0.rs`
- Create: `src/diagnostics/mod.rs`
- Create: `src/output/mod.rs`
- Create: `src/paths/mod.rs`
- Create: `src/git/mod.rs`
- Create: `src/instructions/mod.rs`
- Create: `tests/paths_identity.rs`
- Create: `tests/instructions_and_git.rs`

- [ ] **Step 1: Write `tests/paths_identity.rs` and `tests/instructions_and_git.rs` for path normalization, malformed instruction failure, detached-HEAD handling, and rejection of shell-eval or `git` subprocess shortcuts.**
- [ ] **Step 2: Run `cargo nextest run --test paths_identity --test instructions_and_git` and confirm the new guardrail tests fail before the supporting modules exist.**
- [ ] **Step 3: Implement `src/diagnostics/mod.rs`, `src/output/mod.rs`, `src/paths/mod.rs`, `src/git/mod.rs`, `src/instructions/mod.rs`, `src/compat/mod.rs`, and `src/compat/argv0.rs` with typed failures, normalized repo paths, library-backed repo identity, and fail-closed instruction parsing.**
- [ ] **Step 4: Run `cargo nextest run --test paths_identity --test instructions_and_git` and confirm the primitives pass without invoking `git` or wrapper-owned path rewriting.**
- [ ] **Step 5: Commit with `git add src/compat src/diagnostics src/output src/paths src/git src/instructions tests/paths_identity.rs tests/instructions_and_git.rs && git commit -m "feat: add rust runtime foundations"`**

## Task 3: Port Spec, Plan, Packet, and Evidence Contracts

**Spec Coverage:** REQ-004, REQ-006, REQ-012, REQ-013, REQ-014, REQ-015, REQ-018, REQ-027, REQ-031, REQ-046, NONGOAL-001, VERIFY-006
**Task Outcome:** The Rust runtime can parse strict workflow markdown contracts, validate cross-document relationships, build deterministic task packets, read existing evidence, and emit checked-in schemas for contract-backed machine output.
**Plan Constraints:**
- Preserve exact header parsing and contract-invalid vs malformed distinctions.
- Keep packet provenance deterministic and renderer-backed.
- Do not move artifact authority out of repo-visible markdown.
**Open Questions:** none

**Files:**
- Create: `src/contracts/mod.rs`
- Create: `src/contracts/spec.rs`
- Create: `src/contracts/plan.rs`
- Create: `src/contracts/packet.rs`
- Create: `src/contracts/evidence.rs`
- Create: `schemas/plan-contract-analyze.schema.json`
- Create: `schemas/plan-contract-packet.schema.json`
- Create: `tests/contracts_spec_plan.rs`
- Create: `tests/packet_and_schema.rs`
- Modify: `tests/codex-runtime/test-superpowers-plan-contract.sh`
- Modify: `tests/codex-runtime/fixtures/plan-contract/valid-spec.md`
- Modify: `tests/codex-runtime/fixtures/plan-contract/valid-plan.md`

- [ ] **Step 1: Extend `tests/codex-runtime/test-superpowers-plan-contract.sh`, `tests/contracts_spec_plan.rs`, and `tests/packet_and_schema.rs` to pin exact header parsing, coverage-matrix validation, deterministic packet provenance, schema generation, and legacy evidence readability.**
- [ ] **Step 2: Run `bash tests/codex-runtime/test-superpowers-plan-contract.sh` and `cargo nextest run --test contracts_spec_plan --test packet_and_schema` and confirm the new expectations fail before the Rust contract modules are implemented.**
- [ ] **Step 3: Implement `src/contracts/{spec,plan,packet,evidence}.rs`, wire schema generation into `schemas/plan-contract-analyze.schema.json` and `schemas/plan-contract-packet.schema.json`, and keep output defaults stable unless the spec explicitly permits a difference.**
- [ ] **Step 4: Re-run `bash tests/codex-runtime/test-superpowers-plan-contract.sh` and `cargo nextest run --test contracts_spec_plan --test packet_and_schema` and confirm both shell parity and Rust-native contract tests pass.**
- [ ] **Step 5: Commit with `git add src/contracts schemas/plan-contract-analyze.schema.json schemas/plan-contract-packet.schema.json tests/contracts_spec_plan.rs tests/packet_and_schema.rs tests/codex-runtime/test-superpowers-plan-contract.sh tests/codex-runtime/fixtures/plan-contract/valid-spec.md tests/codex-runtime/fixtures/plan-contract/valid-plan.md && git commit -m "feat: port plan contract parsing to rust"`**

## Task 4: Implement the Workflow Engine and Canonical Workflow CLI

**Spec Coverage:** REQ-002, REQ-003, REQ-004, REQ-006, REQ-008, REQ-009, REQ-010, REQ-011, REQ-012, REQ-021, REQ-027, REQ-035, REQ-039, REQ-043, REQ-046, NONGOAL-001, NONGOAL-004, VERIFY-006
**Task Outcome:** `superpowers workflow status|resolve|expect|sync` is owned by typed Rust workflow code with deterministic artifact ranking, manifest repair, schema-backed output, and canonical CLI routing that legacy workflow names can thinly dispatch into during migration.
**Plan Constraints:**
- Keep manifests derived and helper-owned only.
- Preserve conservative downgrade and explicit ambiguity reporting.
- Keep any temporary legacy workflow shims transport-only.
**Open Questions:** none

**Files:**
- Create: `src/workflow/mod.rs`
- Create: `src/workflow/manifest.rs`
- Create: `src/workflow/status.rs`
- Create: `src/cli/workflow.rs`
- Create: `schemas/workflow-status.schema.json`
- Create: `schemas/workflow-resolve.schema.json`
- Create: `tests/workflow_runtime.rs`
- Modify: `tests/codex-runtime/test-superpowers-workflow-status.sh`
- Modify: `tests/codex-runtime/test-superpowers-workflow.sh`
- Modify: `tests/codex-runtime/workflow-fixtures.test.mjs`

- [ ] **Step 1: Add failing Rust and shell parity coverage for manifest-backed status, deterministic ambiguity handling, `expect`, `sync`, legacy `reason` compatibility fields, and canonical `superpowers workflow ...` dispatch.**
- [ ] **Step 2: Run `bash tests/codex-runtime/test-superpowers-workflow-status.sh`, `bash tests/codex-runtime/test-superpowers-workflow.sh`, and `cargo nextest run --test workflow_runtime` and confirm the new workflow expectations fail before the Rust workflow engine exists.**
- [ ] **Step 3: Implement `src/workflow/{mod,manifest,status}.rs`, `src/cli/workflow.rs`, and schema generation for workflow JSON output, including atomic manifest writes and explicit corrupt-manifest backup behavior.**
- [ ] **Step 4: Re-run `bash tests/codex-runtime/test-superpowers-workflow-status.sh`, `bash tests/codex-runtime/test-superpowers-workflow.sh`, `node --test tests/codex-runtime/workflow-fixtures.test.mjs`, and `cargo nextest run --test workflow_runtime` and confirm workflow parity holds.**
- [ ] **Step 5: Commit with `git add src/workflow src/cli/workflow.rs schemas/workflow-status.schema.json schemas/workflow-resolve.schema.json tests/workflow_runtime.rs tests/codex-runtime/test-superpowers-workflow-status.sh tests/codex-runtime/test-superpowers-workflow.sh tests/codex-runtime/workflow-fixtures.test.mjs && git commit -m "feat: port workflow runtime to rust"`**

## Task 5: Port the Execution Engine and Evidence Mutations

**Spec Coverage:** REQ-003, REQ-004, REQ-016, REQ-017, REQ-018, REQ-027, REQ-031, REQ-046, NONGOAL-001, VERIFY-006
**Task Outcome:** `superpowers plan execution` commands run through a typed execution engine that preserves current invariants, stale-write protection, deterministic evidence rendering, and fail-closed review and finish gates.
**Plan Constraints:**
- Preserve one-active-step and stale-fingerprint protections exactly.
- Keep evidence readable from legacy artifacts before any rewrite into canonical Rust rendering.
- Do not weaken gate-review or gate-finish law to make migration easier.
**Open Questions:** none

**Files:**
- Create: `src/execution/mod.rs`
- Create: `src/execution/state.rs`
- Create: `src/execution/mutate.rs`
- Create: `src/cli/plan_execution.rs`
- Create: `schemas/plan-execution-status.schema.json`
- Create: `tests/plan_execution.rs`
- Modify: `tests/codex-runtime/test-superpowers-plan-execution.sh`

- [ ] **Step 1: Extend `tests/codex-runtime/test-superpowers-plan-execution.sh` and add `tests/plan_execution.rs` for status, recommend, preflight, gate-review, gate-finish, stale fingerprint rejection, and deterministic evidence rewriting.**
- [ ] **Step 2: Run `bash tests/codex-runtime/test-superpowers-plan-execution.sh` and `cargo nextest run --test plan_execution` and confirm the ported execution expectations fail before the Rust execution engine is wired in.**
- [ ] **Step 3: Implement `src/execution/{mod,state,mutate}.rs`, `src/cli/plan_execution.rs`, and `schemas/plan-execution-status.schema.json`, including compare-and-swap mutation protections and legacy evidence read compatibility.**
- [ ] **Step 4: Re-run `bash tests/codex-runtime/test-superpowers-plan-execution.sh` and `cargo nextest run --test plan_execution` and confirm the execution engine matches current invariants and output expectations.**
- [ ] **Step 5: Commit with `git add src/execution src/cli/plan_execution.rs schemas/plan-execution-status.schema.json tests/plan_execution.rs tests/codex-runtime/test-superpowers-plan-execution.sh && git commit -m "feat: port execution engine to rust"`**

## Task 6: Port Repo Safety, Session Entry, Slug, and Config

**Spec Coverage:** REQ-003, REQ-006, REQ-007, REQ-012, REQ-019, REQ-020, REQ-021, REQ-022, REQ-023, REQ-024, REQ-027, REQ-036, REQ-044, REQ-045, REQ-050, REQ-051, REQ-052, NONGOAL-001, VERIFY-005, VERIFY-006, VERIFY-009
**Task Outcome:** Policy-sensitive local runtime subsystems move into typed Rust modules with canonical helper-owned paths under `~/.superpowers/`, migrated approval support, YAML config migration, and fail-closed session-entry behavior.
**Plan Constraints:**
- Keep helper-owned state file-based.
- Migrate non-rebuildable state with backup and explicit reporting.
- Preserve user-visible repo-safety failure classes and slug semantics.
**Open Questions:** none

**Files:**
- Create: `src/repo_safety/mod.rs`
- Create: `src/session_entry/mod.rs`
- Create: `src/config/mod.rs`
- Create: `src/cli/repo_safety.rs`
- Create: `src/cli/session_entry.rs`
- Create: `src/cli/config.rs`
- Create: `src/cli/slug.rs`
- Create: `schemas/repo-safety-check.schema.json`
- Create: `schemas/session-entry-resolve.schema.json`
- Create: `tests/repo_safety.rs`
- Create: `tests/session_config_slug.rs`
- Modify: `tests/codex-runtime/test-superpowers-repo-safety.sh`
- Modify: `tests/codex-runtime/test-superpowers-session-entry.sh`
- Modify: `tests/codex-runtime/test-superpowers-session-entry-gate.sh`
- Modify: `tests/codex-runtime/test-superpowers-config.sh`
- Modify: `tests/codex-runtime/test-superpowers-slug.sh`
- Modify: `tests/codex-runtime/test-runtime-instructions.sh`

- [ ] **Step 1: Add failing shell and Rust tests for protected-branch checks, approval migration, session-entry resolution, legacy-to-canonical config migration, YAML validation, and slug output stability.**
- [ ] **Step 2: Run `bash tests/codex-runtime/test-superpowers-repo-safety.sh`, `bash tests/codex-runtime/test-superpowers-session-entry.sh`, `bash tests/codex-runtime/test-superpowers-session-entry-gate.sh`, `bash tests/codex-runtime/test-superpowers-config.sh`, `bash tests/codex-runtime/test-superpowers-slug.sh`, `bash tests/codex-runtime/test-runtime-instructions.sh`, `cargo nextest run --test repo_safety --test session_config_slug`, and confirm they fail before the subsystem ports land.**
- [ ] **Step 3: Implement `src/repo_safety/mod.rs`, `src/session_entry/mod.rs`, `src/config/mod.rs`, `src/cli/{repo_safety,session_entry,config,slug}.rs`, and schema output for repo-safety and session-entry while preserving file-based helper state and canonical migrated paths.**
- [ ] **Step 4: Re-run the targeted shell suite and `cargo nextest run --test repo_safety --test session_config_slug` and confirm migrated approvals, config backup behavior, slug output, and fail-closed session handling match the approved contract.**
- [ ] **Step 5: Commit with `git add src/repo_safety src/session_entry src/config src/cli/repo_safety.rs src/cli/session_entry.rs src/cli/config.rs src/cli/slug.rs schemas/repo-safety-check.schema.json schemas/session-entry-resolve.schema.json tests/repo_safety.rs tests/session_config_slug.rs tests/codex-runtime/test-superpowers-repo-safety.sh tests/codex-runtime/test-superpowers-session-entry.sh tests/codex-runtime/test-superpowers-session-entry-gate.sh tests/codex-runtime/test-superpowers-config.sh tests/codex-runtime/test-superpowers-slug.sh tests/codex-runtime/test-runtime-instructions.sh && git commit -m "feat: port policy and local state helpers to rust"`**

## Task 7: Port Update Check and Explicit Install-State Migration

**Spec Coverage:** REQ-003, REQ-012, REQ-024, REQ-025, REQ-026, REQ-027, REQ-044, REQ-045, REQ-046, REQ-047, REQ-048, REQ-049, REQ-050, REQ-051, REQ-052, VERIFY-005, VERIFY-007, VERIFY-008, VERIFY-009
**Task Outcome:** `superpowers update-check` and the explicit non-rebuildable-state portions of `superpowers install migrate` are Rust-backed, preserve current cache and status behavior, and enforce the approved migration gates and backup rules for helper-owned local state.
**Plan Constraints:**
- Keep normal installation local-only on supported targets.
- Require explicit migration for non-rebuildable helper-owned state.
- Keep Linux follow-on scope out of the first-release blocking path.
**Open Questions:** none

**Files:**
- Create: `src/update_check/mod.rs`
- Create: `src/install/mod.rs`
- Create: `src/cli/update_check.rs`
- Create: `src/cli/install.rs`
- Create: `schemas/update-check.schema.json`
- Create: `tests/update_and_install.rs`
- Modify: `tests/codex-runtime/test-superpowers-update-check.sh`
- Modify: `tests/codex-runtime/test-superpowers-migrate-install.sh`
- Modify: `tests/codex-runtime/test-superpowers-upgrade-skill.sh`

- [ ] **Step 1: Add failing tests for update-check cache behavior, disabled and snoozed states, install ambiguity handling, explicit migration reporting, pending-migration read-only allowances, blocked mutation paths, and approval/config migration fallback behavior.**
- [ ] **Step 2: Run `bash tests/codex-runtime/test-superpowers-update-check.sh`, `bash tests/codex-runtime/test-superpowers-migrate-install.sh`, `bash tests/codex-runtime/test-superpowers-upgrade-skill.sh`, and `cargo nextest run --test update_and_install` and confirm the new install and update expectations fail before the Rust modules exist.**
- [ ] **Step 3: Implement `src/update_check/mod.rs`, `src/install/mod.rs`, `src/cli/update_check.rs`, `src/cli/install.rs`, and `schemas/update-check.schema.json` so update-check behavior and explicit install-state migration rules are Rust-backed and fail closed where the spec requires.**
- [ ] **Step 4: Re-run the targeted shell suite and `cargo nextest run --test update_and_install` and confirm explicit migration gating, local-only install behavior, and migration reporting all pass without remote artifact fetches.**
- [ ] **Step 5: Commit with `git add src/update_check src/install src/cli/update_check.rs src/cli/install.rs schemas/update-check.schema.json tests/update_and_install.rs tests/codex-runtime/test-superpowers-update-check.sh tests/codex-runtime/test-superpowers-migrate-install.sh tests/codex-runtime/test-superpowers-upgrade-skill.sh && git commit -m "feat: port update-check and install-state migration to rust"`**

## Task 8: Add Checked-In Prebuilt Manifest and Provisioning

**Spec Coverage:** REQ-003, REQ-011, REQ-027, REQ-031, REQ-048, REQ-053, REQ-054, NONGOAL-005, VERIFY-002, VERIFY-010
**Task Outcome:** The checked-in prebuilt runtime contract is explicit: `bin/prebuilt/manifest.json` maps supported targets to binary and checksum files, refresh scripts regenerate that contract, and install-time provisioning resolves the host binary from the manifest instead of inferring layout from filenames.
**Plan Constraints:**
- Keep the supported target set limited to macOS arm64 and Windows x64.
- Use one checked-in manifest as the source of truth for binary path, checksum path, and runtime revision.
- Do not require a local Rust build or remote artifact fetch during normal installation on supported targets.
**Open Questions:** none

**Files:**
- Create: `scripts/refresh-prebuilt-runtime.sh`
- Create: `scripts/refresh-prebuilt-runtime.ps1`
- Create: `bin/prebuilt/manifest.json`
- Modify: `tests/update_and_install.rs`
- Modify: `tests/codex-runtime/test-superpowers-migrate-install.sh`

- [ ] **Step 1: Add failing provisioning tests for manifest-driven host-target resolution, checksum lookup, missing-manifest failure, stale-checksum failure, and install-time copy into `~/.superpowers/install/bin`.**
- [ ] **Step 2: Run `bash tests/codex-runtime/test-superpowers-migrate-install.sh` and `cargo nextest run --test update_and_install` and confirm the provisioning expectations fail before the checked-in manifest and refresh scripts exist.**
- [ ] **Step 3: Implement `scripts/refresh-prebuilt-runtime.sh`, `scripts/refresh-prebuilt-runtime.ps1`, and `bin/prebuilt/manifest.json`, and wire install-time binary provisioning to resolve the supported target binary and checksum from that manifest.**
- [ ] **Step 4: Re-run `bash tests/codex-runtime/test-superpowers-migrate-install.sh` and `cargo nextest run --test update_and_install` and confirm the supported-target manifest contract and provisioning flow pass without remote artifact fetches.**
- [ ] **Step 5: Commit with `git add scripts/refresh-prebuilt-runtime.sh scripts/refresh-prebuilt-runtime.ps1 bin/prebuilt/manifest.json tests/update_and_install.rs tests/codex-runtime/test-superpowers-migrate-install.sh && git commit -m "feat: add checked-in runtime provisioning contract"`**

## Task 9: Convert Shell Helpers into Migration Shims and Clean the Installed Surface

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-009, REQ-014, REQ-016, REQ-020, REQ-028, REQ-029, REQ-036, REQ-039, REQ-040, NONGOAL-003, NONGOAL-004, VERIFY-001
**Task Outcome:** Existing helper scripts become thin dispatch shims into canonical Rust subcommands where they still exist during migration, wrapper-owned JSON rewriting is removed, and the intended installed surface is reduced to `superpowers` only.
**Plan Constraints:**
- Keep shims thin, deterministic, and clearly temporary.
- Do not let `.ps1` wrappers remain part of the supported post-cutover install surface.
- Preserve stdout, stderr, exit-code, and current-working-directory behavior while shims still exist.
**Open Questions:** none

**Files:**
- Create: `compat/bash/superpowers`
- Create: `compat/powershell/superpowers.ps1`
- Modify: `bin/superpowers-plan-contract`
- Modify: `bin/superpowers-plan-contract.ps1`
- Modify: `bin/superpowers-plan-execution`
- Modify: `bin/superpowers-plan-execution.ps1`
- Modify: `bin/superpowers-workflow`
- Modify: `bin/superpowers-workflow.ps1`
- Modify: `bin/superpowers-workflow-status`
- Modify: `bin/superpowers-workflow-status.ps1`
- Modify: `bin/superpowers-repo-safety`
- Modify: `bin/superpowers-repo-safety.ps1`
- Modify: `bin/superpowers-session-entry`
- Modify: `bin/superpowers-session-entry.ps1`
- Modify: `bin/superpowers-config`
- Modify: `bin/superpowers-config.ps1`
- Modify: `bin/superpowers-slug`
- Modify: `bin/superpowers-update-check`
- Modify: `bin/superpowers-update-check.ps1`
- Modify: `bin/superpowers-migrate-install`
- Modify: `bin/superpowers-migrate-install.ps1`
- Modify: `tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
- Modify: `tests/brainstorm-server/test-launch-wrappers.sh`

- [ ] **Step 1: Extend wrapper and launcher tests so they fail if any shim rewrites JSON, changes exit codes, or preserves helper-style installed executables as part of the intended steady-state surface.**
- [ ] **Step 2: Run `bash tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh` and `bash tests/brainstorm-server/test-launch-wrappers.sh` and confirm the migration-shim contract fails before the helper scripts are rewritten.**
- [ ] **Step 3: Replace business-logic shell bodies in `bin/` with thin dispatch into canonical Rust subcommands, add `compat/bash/superpowers` and `compat/powershell/superpowers.ps1` only where a temporary launcher is still needed during migration, and remove wrapper-side path or JSON mutation.**
- [ ] **Step 4: Re-run the launcher tests and confirm shim behavior is transport-only and that the installed surface expectation is now `superpowers` alone.**
- [ ] **Step 5: Commit with `git add compat/bash/superpowers compat/powershell/superpowers.ps1 bin/superpowers-plan-contract bin/superpowers-plan-contract.ps1 bin/superpowers-plan-execution bin/superpowers-plan-execution.ps1 bin/superpowers-workflow bin/superpowers-workflow.ps1 bin/superpowers-workflow-status bin/superpowers-workflow-status.ps1 bin/superpowers-repo-safety bin/superpowers-repo-safety.ps1 bin/superpowers-session-entry bin/superpowers-session-entry.ps1 bin/superpowers-config bin/superpowers-config.ps1 bin/superpowers-slug bin/superpowers-update-check bin/superpowers-update-check.ps1 bin/superpowers-migrate-install bin/superpowers-migrate-install.ps1 tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh tests/brainstorm-server/test-launch-wrappers.sh && git commit -m "refactor: collapse shell helpers into rust shims"`**

## Task 10: Cut Repo-Owned Callers, Docs, and Differential Verification to the Canonical CLI

**Spec Coverage:** REQ-002, REQ-004, REQ-005, REQ-010, REQ-030, REQ-031, REQ-034, REQ-035, REQ-039, REQ-046, NONGOAL-002, VERIFY-001, VERIFY-003, VERIFY-006, VERIFY-008, VERIFY-009
**Task Outcome:** Skill templates, generated skill docs, command docs, README content, testing docs, and differential tooling all point at canonical `superpowers ...` commands and capture the final compatibility decisions for the cutover.
**Plan Constraints:**
- Update templates first, then regenerate `SKILL.md`.
- Keep adjacent Node-based surfaces cataloged and verified, not reimplemented.
- Treat differential mismatches as triage items, not silent fixes.
**Open Questions:** none

**Files:**
- Create: `tests/differential/README.md`
- Create: `tests/differential/run_legacy_vs_rust.sh`
- Create: `tests/fixtures/differential/workflow-status.json`
- Modify: `skills/brainstorming/SKILL.md.tmpl`
- Modify: `skills/brainstorming/SKILL.md`
- Modify: `skills/using-superpowers/SKILL.md.tmpl`
- Modify: `skills/using-superpowers/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `commands/brainstorm.md`
- Modify: `commands/write-plan.md`
- Modify: `commands/execute-plan.md`
- Modify: `README.md`
- Modify: `docs/README.codex.md`
- Modify: `docs/README.copilot.md`
- Modify: `docs/testing.md`
- Modify: `RELEASE-NOTES.md`
- Modify: `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-generation.test.mjs`

- [ ] **Step 1: Update the skill-doc contract tests and add differential harness scaffolding that fails until canonical command references, mismatch triage, and operator docs are aligned with the Rust runtime.**
- [ ] **Step 2: Run `node scripts/gen-skill-docs.mjs --check`, `node --test tests/codex-runtime/*.test.mjs`, and the differential harness smoke command from `tests/differential/run_legacy_vs_rust.sh` and confirm doc and differential expectations fail before the cutover docs are updated.**
- [ ] **Step 3: Update the listed skill templates, regenerate their `SKILL.md` outputs, update command docs and operator docs to canonical `superpowers ...` commands, and check in the differential harness guidance plus initial fixture corpus.**
- [ ] **Step 4: Re-run `node scripts/gen-skill-docs.mjs --check`, `node --test tests/codex-runtime/*.test.mjs`, and the differential harness smoke command and confirm the canonical CLI is the only repo-owned vocabulary left.**
- [ ] **Step 5: Commit with `git add tests/differential/README.md tests/differential/run_legacy_vs_rust.sh tests/fixtures/differential/workflow-status.json skills/brainstorming/SKILL.md.tmpl skills/brainstorming/SKILL.md skills/using-superpowers/SKILL.md.tmpl skills/using-superpowers/SKILL.md skills/writing-plans/SKILL.md.tmpl skills/writing-plans/SKILL.md skills/plan-ceo-review/SKILL.md.tmpl skills/plan-ceo-review/SKILL.md skills/plan-eng-review/SKILL.md.tmpl skills/plan-eng-review/SKILL.md skills/executing-plans/SKILL.md.tmpl skills/executing-plans/SKILL.md skills/subagent-driven-development/SKILL.md.tmpl skills/subagent-driven-development/SKILL.md commands/brainstorm.md commands/write-plan.md commands/execute-plan.md README.md docs/README.codex.md docs/README.copilot.md docs/testing.md RELEASE-NOTES.md tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs && git commit -m "docs: cut runtime references to canonical rust cli"`**

## Task 11: Refresh Checked-In Binaries and Run Full Cutover Verification

**Spec Coverage:** REQ-002, REQ-028, REQ-029, REQ-031, REQ-032, REQ-033, REQ-035, REQ-038, REQ-043, REQ-048, REQ-053, REQ-054, NONGOAL-005, VERIFY-001, VERIFY-002, VERIFY-003, VERIFY-004, VERIFY-005, VERIFY-007, VERIFY-010
**Task Outcome:** The repo carries the supported checked-in binaries, prebuilt manifest, benchmark suite, and checksums for the first-release targets, the full verification matrix is green, and release-facing docs accurately describe the Rust cutover and install story.
**Plan Constraints:**
- Refresh binaries only after code, tests, and docs are stable.
- Keep supported first-release targets to macOS arm64 and Windows x64.
- Treat this as the atomic cutover gate; do not leave the repo in a mixed release state.
- Require separate fresh-install evidence from macOS arm64 and Windows x64 hosts before calling the cutover complete.
- Treat checked-in performance thresholds as contract surface; do not loosen them casually to make the suite pass.
**Open Questions:** none

**Files:**
- Create: `benches/workflow_status.rs`
- Create: `benches/plan_contract.rs`
- Create: `benches/execution_status.rs`
- Create: `perf-baselines/runtime-hot-paths.json`
- Create: `bin/prebuilt/manifest.json`
- Create: `bin/prebuilt/darwin-arm64/superpowers`
- Create: `bin/prebuilt/darwin-arm64/superpowers.sha256`
- Create: `bin/prebuilt/windows-x64/superpowers.exe`
- Create: `bin/prebuilt/windows-x64/superpowers.exe.sha256`
- Create: `scripts/check-runtime-benchmarks.sh`
- Modify: `docs/testing.md`
- Modify: `RELEASE-NOTES.md`
- Test: `tests/bootstrap_smoke.rs`
- Test: `tests/contracts_spec_plan.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/plan_execution.rs`
- Test: `tests/repo_safety.rs`
- Test: `tests/session_config_slug.rs`
- Test: `tests/update_and_install.rs`
- Test: `tests/codex-runtime/test-superpowers-workflow-status.sh`
- Test: `tests/codex-runtime/test-superpowers-workflow.sh`
- Test: `tests/codex-runtime/test-superpowers-plan-contract.sh`
- Test: `tests/codex-runtime/test-superpowers-plan-execution.sh`
- Test: `tests/codex-runtime/test-superpowers-repo-safety.sh`
- Test: `tests/codex-runtime/test-superpowers-session-entry.sh`
- Test: `tests/codex-runtime/test-superpowers-session-entry-gate.sh`
- Test: `tests/codex-runtime/test-superpowers-config.sh`
- Test: `tests/codex-runtime/test-superpowers-slug.sh`
- Test: `tests/codex-runtime/test-superpowers-update-check.sh`
- Test: `tests/codex-runtime/test-superpowers-migrate-install.sh`
- Test: `tests/codex-runtime/test-superpowers-upgrade-skill.sh`
- Test: `tests/codex-runtime/test-runtime-instructions.sh`
- Test: `tests/codex-runtime/test-powershell-wrapper-bash-resolution.sh`
- Test: `tests/brainstorm-server/test-launch-wrappers.sh`

- [ ] **Step 1: Use `scripts/refresh-prebuilt-runtime.sh` on macOS arm64 and `scripts/refresh-prebuilt-runtime.ps1` on Windows x64 to build the supported target binaries, refresh `bin/prebuilt/manifest.json`, and write matching checksum files under `bin/prebuilt/`.**
- [ ] **Step 2: Add `benches/workflow_status.rs`, `benches/plan_contract.rs`, `benches/execution_status.rs`, `perf-baselines/runtime-hot-paths.json`, and `scripts/check-runtime-benchmarks.sh` so the approved hot paths run on fixed fixtures with checked-in latency thresholds.**
- [ ] **Step 3: Run the full validation matrix from this plan on a macOS arm64 host, including Cargo checks, Node skill-doc checks, shell parity suites, wrapper smoke tests, `bash scripts/check-runtime-benchmarks.sh`, and a fresh-install verification against the checked-in binaries.**
- [ ] **Step 4: Run the supported-target fresh-install verification on a Windows x64 host and capture separate evidence that the checked-in manifest resolves the Windows binary correctly, verifies its checksum, installs it into `~/.superpowers/install/bin`, and launches `superpowers.exe` directly.**
- [ ] **Step 5: Review the differential harness output and benchmark results, log any intentional mismatches in `compat/exceptions.md` if that file is required by the implementation, and confirm there are no unexplained parity regressions or threshold regressions.**
- [ ] **Step 6: Update `docs/testing.md` and `RELEASE-NOTES.md` one final time with the exact verification commands, benchmark-threshold suite, supported targets, and checked-in-binary refresh instructions used for the cutover, including the requirement for separate macOS arm64 and Windows x64 fresh-install evidence.**
- [ ] **Step 7: Commit with `git add benches/workflow_status.rs benches/plan_contract.rs benches/execution_status.rs perf-baselines/runtime-hot-paths.json scripts/check-runtime-benchmarks.sh bin/prebuilt/manifest.json bin/prebuilt/darwin-arm64/superpowers bin/prebuilt/darwin-arm64/superpowers.sha256 bin/prebuilt/windows-x64/superpowers.exe bin/prebuilt/windows-x64/superpowers.exe.sha256 docs/testing.md RELEASE-NOTES.md && git commit -m "release: cut over superpowers runtime to rust"`**
