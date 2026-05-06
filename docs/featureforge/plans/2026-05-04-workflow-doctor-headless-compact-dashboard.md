# Workflow Doctor Headless And Compact Dashboard Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use `featureforge workflow operator --plan <approved-plan-path>` as routing authority after engineering approval, and follow the runtime-selected execution owner skill; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-05-04-workflow-doctor-headless-recovery-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review
**QA Requirement:** not-required
**Late-Stage Surface:** docs/featureforge/plans/2026-05-04-workflow-doctor-headless-compact-dashboard.md

**Goal:** Reintroduce a public `workflow doctor --plan <path>` surface with deterministic headless diagnostics and a compact default dashboard that stays fully aligned with workflow/operator routing authority.

**Architecture:** Reopen doctor at the CLI boundary, but keep routing authority in existing workflow/operator context assembly. Derive doctor `resolution` through one focused shared helper that consumes the same operator snapshot and never re-queries routing. Render default text via a compact dashboard projection layer with deterministic section order and sanitized runtime-derived text, while preserving additive-only JSON compatibility.

**Tech Stack:** Rust CLI (`clap`), workflow runtime modules (`src/workflow/*`), Rust integration/shell-smoke tests, skill template generation (`node scripts/gen-skill-docs.mjs`), Node skill-doc contract tests.

---

## Change Surface

- CLI/public surface:
  - `src/cli/workflow.rs`
  - `src/lib.rs`
- Workflow runtime and rendering:
  - `src/workflow/mod.rs`
  - `src/workflow/operator.rs`
  - `src/workflow/doctor_resolution.rs` (new)
  - `src/workflow/doctor_dashboard.rs` (new)
- Runtime and shell-smoke verification:
  - `tests/workflow_runtime.rs`
  - `tests/workflow_entry_shell_smoke.rs`
  - `tests/workflow_shell_smoke.rs`
  - `tests/internal_workflow_shell_smoke.rs`
  - `tests/internal_cli_parse_boundary.rs`
  - `tests/internal_bootstrap_smoke.rs`
- Skill/doc routing surfaces:
  - `skills/using-featureforge/SKILL.md.tmpl`
  - `skills/using-featureforge/SKILL.md` (generated)
  - `docs/featureforge/reference/2026-04-01-review-state-reference.md`
  - `tests/using_featureforge_skill.rs`
  - `tests/runtime_instruction_contracts.rs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`

## Preconditions

- The source spec remains approved with these exact headers:
  - `**Workflow State:** CEO Approved`
  - `**Spec Revision:** 1`
  - `**Last Reviewed By:** plan-ceo-review`
- The source spec `## Requirement Index` remains parseable and unchanged in intent.
- No new mutation command families are introduced in this slice (`plan execution recover` remains out of scope).
- Workflow routing authority remains `workflow operator` and shared execution query state.

## Existing Capabilities / Built-ins to Reuse

- `build_context_with_plan(...)` and `build_context_with_plan_for_runtime(...)` in `src/workflow/operator.rs` as the single routing snapshot source.
- `ExecutionRoutingState` and `RouteDecision` public-route truth from `src/execution/query.rs` and `src/execution/router.rs`.
- Existing doctor DTO fields in `WorkflowDoctor` for route parity.
- Existing warning/reason surfaces already exposed through `PlanExecutionStatus`, `GateResult`, and operator diagnostic reason codes.

## Known Footguns / Constraints

- Do not derive or maintain a second routing graph for doctor; all route fields must stay snapshot-shared with operator.
- `recommended_command` remains display-only compatibility text; machine invocation authority remains `recommended_public_command_argv`.
- Keep CLI boundary strict: `workflow doctor` requires `--plan`; `phase` and `handoff` remain parse-rejected.
- Keep JSON changes additive only; maintain schema version behavior and existing key compatibility.
- Do not add or route to nonexistent `plan execution recover`.
- Apply sanitization only in text rendering; do not mutate authoritative runtime state or JSON payload truth.

## Requirement Coverage Matrix

- DR-001 -> Task 1
- DR-002 -> Task 1, Task 4
- DR-003 -> Task 2
- DR-004 -> Task 2, Task 4
- DR-005 -> Task 2, Task 4
- DR-006 -> Task 2, Task 4
- DR-007 -> Task 2, Task 4
- DR-008 -> Task 2, Task 4
- DR-009 -> Task 5
- DR-010 -> Task 2, Task 4
- DR-011 -> Task 1
- DR-012 -> Task 4, Task 6
- DR-013 -> Task 1, Task 4, Task 6
- DR-014 -> Task 4, Task 6
- DR-015 -> Task 2
- DR-016 -> Task 2
- DR-017 -> Task 3
- DR-018 -> Task 4, Task 6
- DR-019 -> Task 2
- DR-020 -> Task 4, Task 6
- DR-021 -> Task 2, Task 4
- DR-022 -> Task 4, Task 6
- DR-023 -> Task 3
- DR-024 -> Task 3, Task 4
- DR-025 -> Task 3, Task 4
- DR-026 -> Task 3, Task 4
- DR-027 -> Task 4, Task 6
- DR-028 -> Task 4, Task 6

## Execution Strategy

- Execute Task 1 serially. It re-opens the public doctor CLI boundary and establishes parse/help expectations before downstream slices depend on the command surface.
- After Task 1, create two isolated worktrees and run Tasks 2 and 5 in parallel because their write scopes are disjoint:
  - Task 2 owns runtime doctor projection and shared-resolution implementation in `src/workflow/*` plus runtime parity tests.
  - Task 5 owns helper-first routing docs/skills and skill-doc contract coverage only.
- Execute Task 3 serially after Task 2. Compact dashboard rendering depends on the new shared resolution contract from Task 2.
- Execute Task 4 serially after Tasks 2 and 3. It is the explicit reintegration seam for cross-mode parity, security, observability, and performance assertions.
- Execute Task 6 last as the verification/evidence gate across Rust + Node contract surfaces.

## Dependency Diagram

```text
Task 1 -> Task 2
Task 1 -> Task 5
Task 2 -> Task 3
Task 2 -> Task 4
Task 3 -> Task 4
Task 4 -> Task 6
Task 5 -> Task 6
```

## Evidence Expectations

- Runtime JSON samples showing `workflow doctor --plan <path> --json` includes route parity fields plus `resolution` with deterministic `kind`, `command_available`, and `stop_reasons` behavior.
- Text output samples proving compact dashboard section order and labeled markers (`Resolution kind`, `Command available`).
- Parse-boundary failures proving `workflow doctor --json` without `--plan` fails closed with `InvalidCommandInput`.
- Security evidence proving ANSI/control sequences in runtime-derived fields render inert in text mode and stay unchanged in JSON mode.

## Validation Strategy

- Targeted Rust tests during development:
  - `cargo test --test internal_cli_parse_boundary -- --nocapture`
  - `cargo test --test internal_bootstrap_smoke -- --nocapture`
  - `cargo test --test workflow_shell_smoke -- --nocapture`
  - `cargo test --test internal_workflow_shell_smoke -- --nocapture`
  - `cargo test --test workflow_runtime -- --nocapture`
  - `cargo test --test workflow_entry_shell_smoke -- --nocapture`
- Skill/doc contract checks after template edits:
  - `node scripts/gen-skill-docs.mjs`
  - `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `cargo test --test using_featureforge_skill -- --nocapture`
  - `cargo test --test runtime_instruction_contracts -- --nocapture`
- Final gate:
  - `cargo clippy --all-targets --all-features -- -D warnings`

## Documentation Update Expectations

- Update `using-featureforge` helper-first orientation text so doctor is the first diagnostic/orientation call and operator remains routing authority.
- Update review-state reference guidance to reflect doctor/operator parity responsibilities and external-review-result-ready comparison surfaces.
- Regenerate checked-in skill docs from `.tmpl` sources; do not hand-edit generated files.

## Risks and Mitigations

- Risk: route drift between operator and doctor.
  - Mitigation: derive doctor from the exact same context snapshot and enforce parity assertions in runtime and shell-smoke suites.
- Risk: hidden mutation pressure via new diagnostics surface.
  - Mitigation: keep doctor read-only and assert no recover-command routing is emitted.
- Risk: terminal control-sequence output injection in dashboard text.
  - Mitigation: centralize text sanitization in dashboard renderer and add adversarial fixtures.
- Risk: performance regressions from duplicate context/query passes.
  - Mitigation: one-context-per-invocation rule with explicit regression tests for requery loops.

## Task 1: Reopen Public Workflow Doctor CLI Boundary

**Spec Coverage:** DR-001, DR-002, DR-011, DR-013
**Goal:** Public CLI help and parse boundaries expose `workflow doctor` with required `--plan` semantics while keeping removed compatibility commands parse-rejected.

**Context:**
- `src/cli/workflow.rs` currently only exposes `status` and `operator`.
- Existing shell-smoke and parse-boundary tests currently assert `doctor` is removed.
- The spec requires doctor public visibility but preserves removed `phase`/`handoff` boundaries.
- Spec reference: DR-001, DR-011, and `Public Command Contract` (`--plan` required, `doctor` public, `phase`/`handoff` still removed).

**Constraints:**
- Keep `--plan` required on doctor CLI args.
- Keep `--external-review-result-ready` and optional `--json` semantics aligned with operator input shape.
- Do not reintroduce compatibility-only workflow commands.

**Done when:**
- `featureforge workflow --help` includes `doctor` and still excludes `phase`/`handoff`.
- `featureforge workflow doctor --plan <path>` and `--plan <path> --json` parse successfully.
- `featureforge workflow doctor --json` fails closed for missing `--plan`.

**Files:**
- Modify: `src/cli/workflow.rs`
- Modify: `src/lib.rs`
- Modify: `tests/internal_bootstrap_smoke.rs`
- Modify: `tests/internal_cli_parse_boundary.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/internal_workflow_shell_smoke.rs`

- [ ] **Step 1: Add the `workflow doctor` subcommand and arguments in CLI types, then route it through `lib.rs` text/JSON emit paths.**
- [ ] **Step 2: Update workflow help/hidden-command tests to require `doctor` visibility and preserve `phase`/`handoff` rejection.**
- [ ] **Step 3: Update parse-boundary tests for required `--plan`, optional `--json`, and `--external-review-result-ready` acceptance.**

## Task 2: Add Shared Doctor Resolution Helper And JSON Contract Fields

**Spec Coverage:** DR-003, DR-004, DR-005, DR-006, DR-007, DR-008, DR-010, DR-015, DR-016, DR-019, DR-021
**Goal:** Doctor JSON reuses operator snapshot truth and emits deterministic `resolution` classification and reason-code diagnostics without duplicate routing computation.

**Context:**
- `WorkflowDoctor` already carries route parity fields but does not expose the required headless `resolution` object.
- `OperatorContext` already contains `recommended_public_command_argv`, `required_inputs`, `state_kind`, blocking reason codes, diagnostic reason codes, and external wait state needed for deterministic classification.
- Spec requires one focused shared helper module for resolution derivation instead of expanding local classification blocks in `operator.rs`.
- Spec reference: DR-003, DR-016, and `Derivation Ownership And Module Boundary`; `Recovery Decision` keeps `plan execution recover` out of scope.

**Constraints:**
- Create one shared helper module (`src/workflow/doctor_resolution.rs`) and route all doctor `resolution` derivation through it.
- Preserve existing doctor JSON keys/schema behavior; only additive optional fields are allowed.
- `resolution.command_available=true` only when machine argv exists; `stop_reasons` required when command is unavailable and required inputs are empty.
- Do not add `plan execution recover` or any new mutation surface.

**Done when:**
- `WorkflowDoctor` JSON includes additive `resolution` fields with deterministic precedence behavior and canonical stop reason ordering.
- Doctor top-level routing fields continue matching operator for identical inputs.
- Doctor path builds and consumes one shared context snapshot per invocation with no internal requery loops.

**Files:**
- Create: `src/workflow/doctor_resolution.rs`
- Modify: `src/workflow/mod.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_entry_shell_smoke.rs`

- [ ] **Step 1: Implement `doctor_resolution` helper types/functions for precedence, tie-breaks, and canonical reason ordering.**
- [ ] **Step 2: Extend `WorkflowDoctor` with additive `resolution` and deterministic diagnostic reason-code projection fields sourced from the shared operator context.**
- [ ] **Step 3: Add runtime parity tests asserting route-field parity and deterministic `resolution` behavior for actionable, required-input, external-wait, and diagnostic states.**

## Task 3: Implement Compact Dashboard Text Renderer With Sanitization

**Spec Coverage:** DR-017, DR-023, DR-024, DR-025, DR-026
**Goal:** Default `workflow doctor --plan <path>` text output is a compact, deterministic dashboard with required labels, section ordering, blocker formatting, and safe inert rendering.

**Context:**
- Doctor text rendering currently emits a flat field dump and does not include compact section semantics.
- Execution status and gate data already expose blocker/warning fields needed for conditional sections.
- Spec requires text sanitization for runtime-derived values without changing JSON payload truth.
- Spec reference: DR-023 through DR-026 in `Compact Dashboard Text Contract` and DR-017 in `Security Boundaries`.

**Constraints:**
- Keep text rendering projection-only from the same doctor snapshot used for JSON mode.
- Enforce required section order and labeled markers (`Resolution kind`, `Command available`).
- Blocker lines must include canonical reason code plus plain-language action in one line; preserve runtime ordering before truncation.
- Sanitization must happen in text rendering only.

**Done when:**
- Default doctor text output matches compact dashboard section/row contract with deterministic omission behavior.
- Runtime-derived strings containing ANSI/control payloads are rendered as inert text in dashboard output.
- JSON output remains unchanged by text-rendering sanitization logic.

**Files:**
- Create: `src/workflow/doctor_dashboard.rs`
- Modify: `src/workflow/mod.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Implement dashboard renderer module and wire doctor text mode to use it instead of the existing flat formatter.**
- [ ] **Step 2: Add text sanitization helper(s) for runtime-derived fields used in dashboard sections.**
- [ ] **Step 3: Add rendering tests for required section order, labels, blocker formatting, and warning/blocker truncation summary lines.**

## Task 4: Expand Cross-Mode, Security, Stop-Reason, And Performance Verification

**Spec Coverage:** DR-002, DR-004, DR-005, DR-006, DR-007, DR-008, DR-010, DR-012, DR-014, DR-018, DR-020, DR-022, DR-027, DR-028
**Goal:** Test suites prove doctor/operator parity and deterministic diagnostics across JSON/text modes, negative paths, security payloads, and per-invocation context-build constraints.

**Context:**
- Current public tests mostly compare operator vs status and only partially exercise doctor behavior.
- The approved spec requires explicit parity and classification checks across both doctor modes and external-review-result-ready inputs.
- Security and performance guardrails are now part of acceptance requirements.
- Spec reference: `Verification Plan` sections (`CLI Boundary Coverage`, `Cross-Mode Semantic Parity Coverage`, `Security Rendering Coverage`, `Performance Guard`).

**Constraints:**
- Use token/field-level assertions for text-mode semantics where full byte snapshots are too brittle.
- Keep negative-path tests fail-closed and deterministic (`InvalidCommandInput`, reason-code requirements).
- Performance guard assertions must verify no duplicate route-context rebuild loop inside one doctor invocation.

**Done when:**
- Test coverage includes dashboard states: execution-in-progress, required-input pending, external wait, runtime diagnostic required, and terminal-like clean state.
- Cross-mode parity tests prove `command_available`, classification family, required-input presence/absence, and stop-reason behavior align between text and JSON for matched fixtures.
- Security tests verify inert text rendering for ANSI/control payloads while JSON values remain authoritative and unmodified.
- Recovery surface tests assert doctor never routes to nonexistent `plan execution recover`.

**Files:**
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_entry_shell_smoke.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/internal_workflow_shell_smoke.rs`

- [ ] **Step 1: Add/refresh fixture-based parity assertions including `--external-review-result-ready` paths for doctor/operator/status.**
- [ ] **Step 2: Add stop-reason and non-actionable-state tests asserting deterministic reason-code arrays and classification markers.**
- [ ] **Step 3: Add adversarial rendering tests for ANSI/control and malformed strings in dashboard text and JSON parity checks.**
- [ ] **Step 4: Add targeted performance guard tests/assertions for one context-build path per doctor invocation.**

## Task 5: Update Doctor-First Orientation Guidance In Skills And Reference Docs

**Spec Coverage:** DR-009
**Goal:** Helper-first routing guidance uses doctor as the orientation/diagnostic first surface while preserving operator as routing authority and existing recovery command families.

**Context:**
- `using-featureforge` currently anchors helper-first routing directly on `workflow operator` and references removed/legacy fallback language in surrounding guidance.
- The approved spec requires doctor as additional orientation surface and explicit no-`recover` policy.
- Skill docs are generated artifacts and must be regenerated from templates.
- Spec reference: `using-featureforge And Routing Guidance` and DR-009 (`plan execution recover` remains out of scope).

**Constraints:**
- Update `.tmpl` sources first; regenerate checked-in skill docs afterward.
- Keep routing-authority language explicit: operator owns route authority, doctor is diagnosis/orientation.
- Do not introduce fallback guidance that invents new mutation families.

**Done when:**
- `using-featureforge` helper-first text includes doctor-first orientation call and keeps operator as routing authority.
- Reference docs and instruction-contract tests reflect doctor/operator parity responsibilities and no `recover` command introduction.
- Generated skill docs and contract tests pass with updated guidance.

**Files:**
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- Modify: `tests/using_featureforge_skill.rs`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Update template/reference wording to make doctor the default orientation query and keep operator route authority language explicit.**
- [ ] **Step 2: Regenerate skill docs via `node scripts/gen-skill-docs.mjs`.**
- [ ] **Step 3: Update and run skill-doc contract tests (`node --test ...` + targeted Rust contract tests).**

## Task 6: Run Final Verification Gates And Capture Completion Evidence

**Spec Coverage:** DR-012, DR-013, DR-014, DR-018, DR-020, DR-022, DR-027, DR-028
**Goal:** Final branch state demonstrates all doctor-contract behavior, security, and parity requirements through passing deterministic verification gates.

**Context:**
- This slice crosses CLI boundary, runtime projection, rendering, and skill-doc contracts.
- Review and merge safety depends on explicit evidence from both Rust and Node suites.
- Spec reference: `Acceptance Criteria` and `Verification Plan` must be satisfied before review handoff.

**Constraints:**
- Run the listed verification commands from this plan; do not claim completion on partial runs.
- Keep clippy warning-clean under strict repo policy.

**Done when:**
- All targeted Rust + Node tests listed in Validation Strategy pass.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Evidence summary maps test results back to the approved requirement set before execution completion handoff.

**Files:**
- Modify: `docs/featureforge/plans/2026-05-04-workflow-doctor-headless-compact-dashboard.md`

- [ ] **Step 1: Run targeted Rust doctor/CLI/shell-smoke suites.**
- [ ] **Step 2: Run skill-doc generation + Node skill-doc contracts + Rust instruction/skill contract tests.**
- [ ] **Step 3: Run full clippy gate and record final pass evidence for review handoff.**

## Engineering Review Summary

**Review Status:** clear
**Reviewed At:** 2026-05-04T13:58:13Z
**Review Mode:** small_change
**Reviewed Plan Revision:** 1
**Critical Gaps:** 0
**QA Requirement:** not-required
**Test Plan Artifact:** `/Users/dmulcahey/.featureforge/projects/dmulcahey-featureforge/dmulcahey-current-test-plan-20260504-095800.md`
**Outside Voice:** skipped
