# Runtime Signal/Noise Twenty-Fourth Audit Remediation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: workflow/operator JSON is the executable route authority after engineering approval. Run only `recommended_public_command_argv` when present or bind `recommended_public_command_template` through `required_inputs`; `recommended_command` is display-only compatibility text, and `resume_task` / `resume_step` are advisory diagnostics, not executable authority. Steps use checkbox (`- [ ]`) syntax for tracking.

## Workflow State

Engineering Approved

## Plan Revision

Revision 1 - 2026-05-12

## Execution Mode

Single-agent serial remediation after the twenty-fourth audit. Do not use FeatureForge runtime skills or project skills. Review subagents must not spawn or request additional subagents. Before every full nextest cycle, confirm no `cargo nextest`, `cargo-nextest`, `nextest run`, or active `/target/debug/deps/` process is running. Run strict clippy and full no-fail-fast nextest before clean-context review.

## Goal

Remove the twenty-fourth audit's remaining high-signal public-output and test signal/noise issues without adding another meta-policy layer.

## Architecture

Route-plan remains the runtime route owner. Public tests should prove shipped public behavior and durable boundary contracts, not preserve retired empty modules or incidental topology markers. Install docs should point agents to workflow/operator typed argv/template output as executable authority and keep status fields diagnostic-only.

## Change Surface

- `docs/README.codex.md`
- `docs/README.copilot.md`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `docs/runtime-architecture.md`
- `docs/testing.md`
- `scripts/run-public-runtime-flow-tests.sh`
- `src/execution/mod.rs`
- `src/execution/public_route_selection.rs`
- `tests/fixtures/runtime-goldens/README.md`
- `tests/public_cli_flow_contracts.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/support/public_flow_scan.rs`

## Preconditions

- Twenty-third remediation implementation and whole-plan review are complete.
- Fresh audit validation before remediation passed:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Preserve unrelated prior implementation changes in the dirty worktree.

## Known Footguns / Constraints

- Do not reintroduce hidden-helper workflow language in docs.
- Do not make status `resume_task` / `resume_step` sound executable or authoritative.
- Do not keep empty marker modules solely so tests can assert emptiness.
- Do not add more static policy unless it replaces a weaker or missing behavioral guard.
- Keep public-flow test gate coverage tied to shipped runtime behavior plus explicit scanner regression self-tests.
- Do not weaken prompt budgets, typed route authority, current-closure authority, or diagnostic-only projection behavior.

## Requirement Coverage Matrix

| Requirement | Coverage |
|---|---|
| REQ-001: Install docs describe status replay fields as advisory diagnostics only | Task 1 |
| REQ-002: Review-state reference links route-binding law to the canonical operator-route authority reference | Task 1 |
| REQ-003: Public-flow gate runs scanner self-tests that prove hidden-helper regressions are caught | Task 2 |
| REQ-004: Runtime golden docs distinguish public CLI output contracts from end-to-end public transition proof for synthetic late-stage rows | Task 2 |
| REQ-005: Retired `public_route_selection` marker is deleted rather than pinned as an empty topology artifact | Task 3 |
| REQ-006: Boundary tests protect route-plan ownership without exact empty-module marker dependence | Task 3 |

## Ordered Tasks

### Task 1: Correct Public Diagnostic Wording

**Spec Coverage:** REQ-001, REQ-002

**Goal:** Ensure active install/reference docs cannot send agents into status-field-driven replay or repair decisions.

**Context:** The audit found `docs/README.codex.md` and `docs/README.copilot.md` describing `resume_task` / `resume_step` and reviewed-closure replay details with wording that sounded authoritative. That phrasing conflicts with `references/operator-route-authority.md`, where these fields are advisory diagnostics and workflow/operator typed argv/template wins.

**Constraints:** Keep the workflow summary compact. Do not duplicate the full operator-route law in install docs. Do not mention hidden helper commands as an available remediation path.

**Done when:** Install docs say `resume_task` / `resume_step` are advisory diagnostics only, and the review-state reference delegates detailed route-binding law to `references/operator-route-authority.md`.

**Files:** `docs/README.codex.md`, `docs/README.copilot.md`, `docs/featureforge/reference/2026-04-01-review-state-reference.md`

**Implementation steps:**

- [x] Replace over-strong diagnostic wording with explicit advisory-diagnostic wording.
- [x] Reaffirm workflow/operator `recommended_public_command_argv` / bound `recommended_public_command_template` as executable authority.
- [x] Collapse repeated route-binding prose in the review-state reference into a canonical-reference pointer.

**Validation expectations:** Node doc checks and public-output contract tests continue to pass.

### Task 2: Align Public-Flow Gate And Golden Realism

**Spec Coverage:** REQ-003, REQ-004

**Goal:** Keep public-flow test language honest and make the scanner's regression self-tests part of the public-flow gate.

**Context:** `runtime_behavior_golden` captures real public CLI output, but some late-stage states are reached through synthetic fixture setup. Scanner self-tests proving hidden-helper detection lived outside `scripts/run-public-runtime-flow-tests.sh`.

**Constraints:** Do not remove useful route goldens. Do not recast synthetic late-stage setup as end-to-end public command proof.

**Done when:** `run-public-runtime-flow-tests.sh` includes `public_flow_scan_contracts`, and comments/docs distinguish public CLI output contracts from end-to-end transition proof.

**Files:** `scripts/run-public-runtime-flow-tests.sh`, `tests/fixtures/runtime-goldens/README.md`, `docs/testing.md`, `tests/public_cli_flow_contracts.rs`, `tests/public_flow_scan_contracts.rs`, `tests/support/public_flow_scan.rs`

**Implementation steps:**

- [x] Add `--test public_flow_scan_contracts` to the public-flow gate.
- [x] Update gate comments to describe scanner self-tests and golden scope precisely.
- [x] Update runtime-golden README to disclose synthetic setup for long-lived late-stage rows.
- [x] Update public/internal gate testing docs to list scanner self-tests and the public-output-contract scope of route goldens.

**Validation expectations:** `scripts/run-public-runtime-flow-tests.sh` passes and full nextest remains under the performance threshold.

### Task 3: Delete Retired Public Route Marker

**Spec Coverage:** REQ-005, REQ-006

**Goal:** Remove the empty `public_route_selection` topology marker and keep boundary tests focused on durable route-plan ownership.

**Context:** The marker module existed only so tests could assert no route logic moved back into it. That made a test-only artifact part of the conceptual runtime surface.

**Constraints:** Do not recreate another empty marker. Do not weaken tests that prevent router/event-log/status assembly from owning route selection. Historical archive references may remain historical.

**Done when:** `src/execution/public_route_selection.rs` is deleted, `src/execution/mod.rs` no longer exports it, docs no longer list it as a focused module, and `runtime_module_boundaries` asserts the retired module stays absent while preserving route-plan ownership checks.

**Files:** `src/execution/public_route_selection.rs`, `src/execution/mod.rs`, `tests/runtime_module_boundaries.rs`, `docs/featureforge/reference/execution-runtime-module-boundaries.md`, `docs/runtime-architecture.md`

**Implementation steps:**

- [x] Delete the marker module and remove the module declaration.
- [x] Replace tests that read the marker file with an absence check plus existing route-plan ownership assertions.
- [x] Remove marker-module caps and architecture prose.
- [x] Keep forbidden dependency checks for event-log, router, status assembly, and route-plan internals.

**Validation expectations:** `cargo test --test runtime_module_boundaries`, strict clippy, and full nextest pass.
