# FeatureForge Codex/Copilot Workflow Optimization Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>` after engineering approval; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `docs/featureforge/specs/2026-03-28-featureforge-codex-copilot-workflow-optimization-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review
**Delivery Lane:** standard

**Goal:** Land the approved workflow-optimization program as one coherent implementation that makes FeatureForge easier to use with Codex and Copilot without weakening runtime-owned truth, fail-closed gates, or review integrity.

**Architecture:** Implement the program in one umbrella plan, but keep the execution order intentionally serial because the core surfaces overlap heavily in `src/contracts`, `src/workflow`, `src/execution`, `src/cli`, and the same high-volume skill templates. First land the planning and review-contract foundations, then add dynamic gates, execution-safety substrate, finish-path routing, and shell-friendly operator contracts, and only after the behavior is stable perform namespace cleanup, prompt compaction, and final ratification. The plan keeps one artifact because that is the user’s chosen planning shape, but it does not fake parallelism where the write scopes would constantly collide.

**Tech Stack:** Rust CLI/runtime (`clap`, `serde`, `schemars`), markdown workflow/spec/plan contracts, generated FeatureForge skill docs, JSON schemas under `schemas/`, Rust integration tests with `cargo nextest`, Node doc-contract and skill-generation tests under `tests/codex-runtime/`

## Plan Contract

This plan owns execution order, task boundaries, and verification expectations for the approved spec. It does not override the approved spec. If the spec and this plan drift, update this plan before execution continues.

This is intentionally one umbrella plan. The user explicitly chose to keep all nine approved phases together rather than split them into multiple plans. That decision increases hotspot overlap, so the execution strategy below treats serial execution as a deliberate safety measure, not as default laziness.

## Existing Capabilities / Built-ins to Reuse

- `src/contracts/spec.rs`, `src/contracts/plan.rs`, `src/contracts/runtime.rs`, and `src/cli/plan_contract.rs` already own spec/plan parsing, linting, and contract-state analysis. Extend those surfaces instead of inventing a second parser or sidecar contract layer.
- `src/workflow/status.rs`, `src/workflow/operator.rs`, and `src/cli/workflow.rs` already own workflow routing, doctor output, and handoff behavior. Reuse them for the shared operator snapshot, per-gate diagnostics, and new planning/review stages.
- `src/execution/harness.rs`, `src/execution/state.rs`, `src/execution/topology.rs`, `src/execution/final_review.rs`, `src/execution/leases.rs`, `src/execution/gates.rs`, and `src/cli/plan_execution.rs` already own authoritative execution, gate freshness, and late-stage routing. Extend those modules rather than creating a parallel execution-state system.
- `scripts/gen-skill-docs.mjs` and the checked-in `skills/*/SKILL.md.tmpl` plus generated `SKILL.md` files already define the active instruction-generation surface. Keep template edits and generated output refreshes in the same task.
- `tests/contracts_spec_plan.rs`, `tests/runtime_instruction_plan_review_contracts.rs`, `tests/runtime_instruction_review_contracts.rs`, `tests/runtime_instruction_contracts.rs`, `tests/plan_execution.rs`, `tests/plan_execution_final_review.rs`, `tests/plan_execution_topology.rs`, `tests/contracts_execution_leases.rs`, `tests/execution_harness_state.rs`, `tests/workflow_runtime.rs`, `tests/workflow_runtime_final_review.rs`, and `tests/workflow_shell_smoke.rs` already pin the contract surfaces this program changes.
- `README.md`, `docs/README.codex.md`, `docs/README.copilot.md`, `AGENTS.md`, and `docs/testing.md` are already the active user-facing documentation surfaces. Reserve them for the explicit documentation tasks instead of scattering top-level doc edits across every slice.

## Known Footguns / Constraints

- The approved spec is helper-parseable only because its header block and `## Requirement Index` are now in the strict runtime contract format. Do not “pretty up” those surfaces in ways that re-break runtime parsing.
- The largest hotspot files are `src/contracts/plan.rs`, `src/workflow/status.rs`, `src/workflow/operator.rs`, `src/execution/state.rs`, `src/execution/harness.rs`, `src/execution/topology.rs`, and `src/cli/plan_execution.rs`. Multiple phases want those same files.
- The same high-volume skill templates are touched by multiple approved phases: `skills/using-featureforge`, `skills/writing-plans`, `skills/plan-ceo-review`, `skills/plan-eng-review`, `skills/requesting-code-review`, `skills/document-release`, and `skills/subagent-driven-development`. That overlap is the main reason this plan stays serial.
- New gate-satisfying artifacts must follow the shared artifact-envelope rules from the approved spec. Do not land design/security/scope/release artifacts as ad hoc markdown blobs with one-off metadata.
- The rollout model is non-retroactive for already approved artifacts unless they are materially revised or execution is explicitly reopened. Do not smuggle retroactive migration behavior into implementation tasks.
- The runtime helper currently reports `ambiguous_plan_candidates` for this spec because older plans exist. This plan resolves that ambiguity by claiming a unique path and becoming the only intended plan artifact for the approved spec.
- Generated `SKILL.md` files must be refreshed in the same task as their `.tmpl` changes. Do not hand-edit generated files without regenerating them from the template source.

## Cross-Task Invariants

- Use `featureforge:test-driven-development` inside each execution task before implementation code changes land.
- Before claiming any task is done, run the focused verification named in that task and keep it green.
- Keep runtime-owned receipts, review artifacts, rollout metrics, and operator summaries as the only authoritative routing truth. Skill prose may describe the law, but runtime and tests remain the source of truth.
- Preserve helper-first routing. New planning, review, and finish behavior must surface through runtime-owned CLI/status contracts before or alongside prompt changes.
- Do not reintroduce interpreter-based parsing for FeatureForge-owned output in generated skills.
- Keep active-path cleanup out of archives and historical evidence. Only active non-archive surfaces should be rewritten during namespace/path cleanup.

## Change Surface

- Planning and review contracts: `src/contracts/spec.rs`, `src/contracts/plan.rs`, `src/contracts/runtime.rs`, `src/cli/plan_contract.rs`, `src/cli/workflow.rs`, `src/lib.rs`, `schemas/plan-contract-analyze.schema.json`, `schemas/workflow-status.schema.json`
- Operator and finish routing: `src/workflow/status.rs`, `src/workflow/operator.rs`, `src/execution/harness.rs`, `src/execution/state.rs`, `src/execution/topology.rs`, `src/execution/final_review.rs`, `src/cli/plan_execution.rs`, `src/output/mod.rs`, `schemas/plan-execution-status.schema.json`
- Execution-safety substrate: `src/repo_safety/mod.rs`, `src/execution/mod.rs`, new worktree manager helper under `src/execution/`, `skills/executing-plans/*`, `skills/subagent-driven-development/*`, `skills/dispatching-parallel-agents/*`, `skills/using-git-worktrees/*`
- New checked-in skill surfaces: `skills/plan-fidelity-review/*`, `skills/plan-design-review/*`, `skills/security-review/*`
- Existing planning/review skill surfaces: `skills/using-featureforge/*`, `skills/brainstorming/*`, `skills/writing-plans/*`, `skills/plan-ceo-review/*`, `skills/plan-eng-review/*`, `skills/requesting-code-review/*`, `skills/receiving-code-review/*`, `skills/document-release/*`, `skills/qa-only/*`, `skills/verification-before-completion/*`, `skills/finishing-a-development-branch/*`, `skills/systematic-debugging/*`
- Top-level and active docs: `AGENTS.md`, `README.md`, `docs/README.codex.md`, `docs/README.copilot.md`, `docs/testing.md`, active non-archive docs under `docs/featureforge/`
- Regression suites and doc-generation surfaces: `tests/*.rs`, `tests/codex-runtime/*.test.mjs`, `scripts/gen-skill-docs.mjs`, `scripts/gen-agent-docs.mjs`, `TODOS.md`

## Preconditions

- Start from the approved spec at `docs/featureforge/specs/2026-03-28-featureforge-codex-copilot-workflow-optimization-design.md` with `Spec Revision: 1` and `Workflow State: CEO Approved`.
- Run plan and verification commands from the repo root so schema, generator, and fixture-relative paths resolve correctly.
- Treat this plan as the single intended plan artifact for the approved spec at `docs/featureforge/plans/2026-03-28-featureforge-codex-copilot-workflow-optimization.md`.
- Keep this plan in `Draft` until the dedicated independent plan-fidelity review passes and its runtime-owned receipt is recorded.
- Do not begin implementation from this plan until `featureforge:plan-eng-review` approves it.

## Risk & Gate Signals

- Delivery Lane: standard
- UI Scope: none
- Browser QA Required: no
- Design Review Required: no
- Security Review Required: yes
- Performance Review Required: no
- Release Surface: code_only_no_deploy
- Distribution Impact: low
- Deploy Impact: low
- Migration Risk: low

## Release & Distribution Notes

- Discoverability / Distribution Path: Internal workflow/runtime behavior plus checked-in skill/docs updates shipped through the normal FeatureForge release path.
- Versioning Decision: unknown
- Versioning Rationale: Final versioning depends on the combined release surface after implementation and `document-release` review.
- Deployment / Rollout Notes: Roll out in contract-first phases and keep new enforcement non-retroactive for already approved artifacts unless they are revised or reopened.

## Execution Strategy

- Execute Task 1 serially. It establishes the checked-in `plan-fidelity-review` stage and the public routing foundation the later planning tasks depend on.
- Execute Task 2 serially after Task 1. The lightweight-lane contract extends the planning parser and must land before later operator and dynamic-gate work consume those headers.
- Execute Task 3 serially after Task 2. Scope-check, release/document, and shared operator snapshot work revises the same workflow and execution hotspots later tasks extend.
- Execute Task 4 serially after Task 3. Dynamic gate signals and new gate-satisfying artifacts must land before execution-safety and finish-path routing can consume them.
- Execute Task 5 serially after Task 4. It establishes the reusable worktree substrate, lane manifests, patch harvesting, and default-path guidance before any enforcement behavior depends on that substrate.
- Execute Task 6 serially after Task 5. It layers task-slice fence modes, rollout counters, override capture, and enforcement behavior on top of the substrate from Task 5.
- Execute Task 7 serially after Task 6. Finish-path reorder relies on the authoritative execution state, dynamic gate model, and fence-aware runtime surfaces already being in place.
- Execute Task 8 serially after Task 7. Shell-friendly CLI contracts and parser-free skill docs must reflect the final runtime truth from the earlier tasks.
- Execute Task 9 serially after Task 8. Active namespace/path cleanup owns top-level docs and active guidance only after the runtime and skill behavior is stable.
- Execute Task 10 serially after Task 9. Skill-doc compaction revisits many of the same templates touched earlier and must compact final behavior instead of stale intermediate behavior.
- Execute Task 11 serially after Task 10. It is the ratification gate for schemas, fixtures, tests, and full-regression verification of the implementation slice.
- Execute Task 12 serially after Task 11. It performs post-ratification docs/backlog cleanup only when the green ratification pass leaves non-runtime guidance drift to reconcile before downstream finish gates.

## Evidence Expectations

- New runtime-owned review artifacts must carry the shared artifact envelope fields the approved spec requires: kind, schema version, provenance, timestamps, fingerprint binding, and retention/cleanup metadata or policy reference.
- Operator surfaces must expose per-gate blocking detail, stale reason codes, reroute owners, rollout-readiness counters, and any degraded snapshot state as machine-readable and human-readable truth.
- Task-slice fences and worktree substrate changes must leave reproducible evidence for `resolution_required` lanes, blocked-write overrides, and rollout-window metrics.
- CLI-shell output work must produce stable, parser-free field output that generated skills can consume without `node -e`, `python`, `jq`, `perl`, or `ruby` snippets.
- Active-doc cleanup must prove archives/history are left intact while active non-archive surfaces lose stale FeatureForge naming/path drift.
- Skill compaction must preserve mandatory stop/fail-closed law in top-level generated `SKILL.md` files even after long-form guidance moves into companion refs.

## Validation Strategy

- Run the focused tests named inside each task before advancing.
- Refresh generated skill docs in every task that changes `.tmpl` files.
- Refresh generated agent docs if any agent-facing docs change during the cleanup/compaction tasks.
- Before handoff out of writing-plans, run:
  - `"$HOME/.featureforge/install/bin/featureforge" plan contract lint --spec docs/featureforge/specs/2026-03-28-featureforge-codex-copilot-workflow-optimization-design.md --plan docs/featureforge/plans/2026-03-28-featureforge-codex-copilot-workflow-optimization.md`
- The final verification gate for implementation work under this plan is:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo nextest run --test contracts_spec_plan --test runtime_instruction_contracts --test runtime_instruction_plan_review_contracts --test runtime_instruction_execution_contracts --test runtime_instruction_review_contracts --test using_featureforge_skill --test workflow_runtime --test workflow_runtime_final_review --test workflow_shell_smoke --test plan_execution --test plan_execution_final_review --test plan_execution_topology --test contracts_execution_leases --test execution_harness_state`
  - `cargo clippy --all-targets --all-features -- -D warnings`

## Documentation Update Expectations

- Keep README/docs changes concentrated in the explicit documentation tasks instead of mixing them into every runtime slice.
- Whenever a skill template changes, regenerate the paired `SKILL.md` file in the same task.
- Active-doc cleanup should only rewrite current guidance surfaces, not archives or historical evidence.
- Compaction tasks must move long-form explanation into exact companion refs/prompts rather than deleting it.

## Rollout Plan

- Land Tasks 1 through 4 in order so the planning/review contract is explicit before execution and finish-path changes rely on it.
- Land Tasks 5 and 6 only after dynamic gate and trust-boundary work is in place, because the execution-safety substrate and its fence-enforcement layer both depend on the new contract truth.
- Land Tasks 7 and 8 after the execution substrate exists, since finish routing and shell-friendly CLI contracts both depend on the authoritative runtime surfaces being stable.
- Perform active namespace/path cleanup and prompt compaction only after behavior is functionally correct.
- Keep rollout non-retroactive for previously approved artifacts unless they are materially revised or reopened.

## Rollback Plan

- Revert the most recent task-scoped slice rather than weakening the runtime contract or test suite.
- If dynamic gates or new review artifacts destabilize routing, revert the last contract/routing task before attempting targeted repair.
- If execution-safety substrate work destabilizes runs, disable the strict fence-enforcement phase or revert the task-scoped slice while preserving trust-boundary and parser correctness.
- If namespace cleanup or compaction introduces doc regressions, revert those doc-focused tasks independently instead of backing out runtime behavior.

## Risks and Mitigations

- The umbrella plan could become too broad to execute safely.
  - Keep the execution strategy serial and task boundaries narrow enough that each slice has a small, testable objective.
- Shared hotspots could tempt contributors to blend multiple phases into one diff.
  - Keep each task’s `Files:` block tight and treat later tasks as the only allowed place to revisit the same surface.
- New review artifacts could become inconsistent one-off formats.
  - Enforce the shared gate-artifact envelope and centralize trust checks in runtime code.
- Task-slice fences could generate noisy false positives.
  - Start in audit mode, capture runtime-owned rollout counters, and require explicit override evidence.
- Prompt compaction could drop mandatory workflow law.
  - Gate compaction with top-level law-presence tests and size-budget reporting.

## Dependency Diagram

```text
Task 1 -> Task 2
Task 2 -> Task 3
Task 3 -> Task 4
Task 4 -> Task 5
Task 5 -> Task 6
Task 6 -> Task 7
Task 7 -> Task 8
Task 8 -> Task 9
Task 9 -> Task 10
Task 10 -> Task 11
Task 11 -> Task 12
```

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-002 -> Task 1, Task 9
- REQ-003 -> Task 2
- REQ-004 -> Task 2
- REQ-005 -> Task 3
- REQ-006 -> Task 3
- REQ-007 -> Task 3
- REQ-008 -> Task 3
- REQ-009 -> Task 3, Task 7
- REQ-010 -> Task 3, Task 7
- REQ-011 -> Task 3, Task 8
- REQ-012 -> Task 4
- REQ-013 -> Task 4
- REQ-014 -> Task 4, Task 7
- REQ-015 -> Task 4
- REQ-016 -> Task 4, Task 7
- REQ-017 -> Task 7
- REQ-018 -> Task 7
- REQ-019 -> Task 5
- REQ-020 -> Task 6
- REQ-021 -> Task 5, Task 6
- REQ-022 -> Task 8
- REQ-023 -> Task 9
- REQ-024 -> Task 10
- REQ-025 -> Task 3
- REQ-026 -> Task 3
- REQ-027 -> Task 3, Task 7
- REQ-028 -> Task 2, Task 4
- REQ-029 -> Task 2
- REQ-030 -> Task 2, Task 4
- REQ-031 -> Task 4
- REQ-032 -> Task 3, Task 8
- VERIFY-001 -> Task 1, Task 2, Task 3, Task 4, Task 5, Task 6, Task 7, Task 8, Task 9, Task 10, Task 11, Task 12
- VERIFY-002 -> Task 1, Task 11
- VERIFY-003 -> Task 9, Task 10, Task 12
- VERIFY-004 -> Task 3, Task 4, Task 5, Task 6, Task 7, Task 11

## Task 1: Expose `plan-fidelity-review` as a First-Class Planning Stage

**Spec Coverage:** REQ-001, REQ-002, VERIFY-001, VERIFY-002
**Task Outcome:** FeatureForge exposes `featureforge:plan-fidelity-review` as a checked-in planning stage with explicit routing, checked-in skill material, and mandatory receipt-recording guidance.
**Plan Constraints:**

- Keep `plan-fidelity-review` limited to fidelity, not business-scope expansion or engineering approval.
- Preserve reviewer independence from both `featureforge:writing-plans` and `featureforge:plan-eng-review`.
- Refresh generated skill docs in the same slice as the template changes.
- Reserve top-level README and overview-doc wording to Task 9 so public workflow docs reflect the final stable behavior rather than an intermediate planning slice.

**Open Questions:** none

**Files:**

- Create: `skills/plan-fidelity-review/SKILL.md.tmpl`
- Create: `skills/plan-fidelity-review/SKILL.md`
- Create: `skills/plan-fidelity-review/references/checklist.md`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `src/workflow/status.rs`
- Modify: `src/cli/workflow.rs`
- Modify: `src/lib.rs`
- Modify: `schemas/workflow-status.schema.json`
- Test: `tests/contracts_spec_plan.rs`
- Test: `tests/runtime_instruction_plan_review_contracts.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/skill-doc-generation.test.mjs`

- [x] **Step 1: Add red routing and doc-contract coverage for a missing or implied `plan-fidelity-review` stage**
  Run: `cargo nextest run --test runtime_instruction_plan_review_contracts --test workflow_runtime`
  Expected: targeted failures showing the checked-in skill and explicit routing surface do not exist yet.

- [ ] **Step 2: Create the checked-in `plan-fidelity-review` skill, reviewer checklist, and planning-surface routing guidance inside the active skill stack**
- [ ] **Step 3: Wire workflow status and CLI routing so draft plans without a fresh pass receipt route back to `writing-plans` and plans with a fresh pass receipt route to `plan-eng-review`**
- [ ] **Step 4: Regenerate `SKILL.md` output and refresh the workflow-status schema surface**
  Run: `node scripts/gen-skill-docs.mjs`
  Expected: generated skill docs refresh cleanly.

- [ ] **Step 5: Re-run the focused Rust and Node suites until the stage is explicit and green, then leave top-level README/overview catch-up for Task 9**
  Run: `cargo nextest run --test contracts_spec_plan --test runtime_instruction_plan_review_contracts --test workflow_runtime && node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs`
  Expected: all targeted suites pass.

## Task 2: Add the Lightweight Lane Contract to Specs, Plans, and Planning Skills

**Spec Coverage:** REQ-003, REQ-004, REQ-028, REQ-029, REQ-030, VERIFY-001
**Task Outcome:** The planning stack recognizes `Delivery Lane`, supports `lightweight_change` under explicit caps, and escalates back to `standard` when disqualifiers appear.
**Plan Constraints:**

- Do not weaken approvals, fidelity review, or contract markers for lightweight plans.
- Keep the rollout non-retroactive for already approved artifacts.
- Preserve the strict parser contract for spec and plan headers.

**Open Questions:** none

**Files:**

- Modify: `skills/brainstorming/SKILL.md.tmpl`
- Modify: `skills/brainstorming/SKILL.md`
- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `skills/plan-fidelity-review/SKILL.md.tmpl`
- Modify: `skills/plan-fidelity-review/SKILL.md`
- Modify: `src/contracts/spec.rs`
- Modify: `src/contracts/plan.rs`
- Modify: `src/workflow/status.rs`
- Modify: `schemas/plan-contract-analyze.schema.json`
- Test: `tests/contracts_spec_plan.rs`
- Test: `tests/runtime_instruction_plan_review_contracts.rs`
- Test: `tests/workflow_runtime.rs`

- [x] **Step 1: Add red parsing, linting, and routing coverage for `Delivery Lane`, lightweight qualification, and standard-lane escalation**
  Run: `cargo nextest run --test contracts_spec_plan --test runtime_instruction_plan_review_contracts --test workflow_runtime`
  Expected: failures identify the missing lane fields and escalation rules.

- [x] **Step 2: Extend spec/plan contract parsing and linting for `Delivery Lane`, lightweight safety justification, and non-retroactive contract versioning rules**
- [x] **Step 3: Update the five planning skills so lightweight behavior is explicit but still approval- and fidelity-bound**
- [x] **Step 4: Refresh status/help surfaces and generated skill docs so the active lane is visible to operators**
  Run: `node scripts/gen-skill-docs.mjs`
  Expected: generated planning-skill docs refresh cleanly.

- [x] **Step 5: Re-run the focused suites until lightweight parsing, escalation, and routing are all green**
  Run: `cargo nextest run --test contracts_spec_plan --test runtime_instruction_plan_review_contracts --test workflow_runtime`
  Expected: all targeted suites pass.

## Task 3: Land Scope Check, Release/Distribution Checks, and the Shared Operator Snapshot

**Spec Coverage:** REQ-005, REQ-006, REQ-007, REQ-008, REQ-009, REQ-010, REQ-011, REQ-025, REQ-026, REQ-027, REQ-032, VERIFY-001, VERIFY-004
**Task Outcome:** Final review and operator surfaces expose structured scope-check results, distribution/versioning readiness, per-gate blocking diagnostics, and a shared operator snapshot consumed consistently by doctor, handoff, JSON, and shell output.
**Plan Constraints:**

- Keep operator truth runtime-owned; do not let skill prose become the primary route source.
- Preserve per-gate blocking detail and degraded-snapshot signaling in the shared snapshot.
- Do not reintroduce implicit “trust me” review behavior for drift or missing requirements.
- Keep shared-snapshot computation single-pass and bounded. Doctor, handoff, JSON, and shell fields must render from one computed snapshot instead of re-scanning artifacts or worktrees independently per mode.

**Open Questions:** none

**Files:**

- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `skills/document-release/SKILL.md.tmpl`
- Modify: `skills/document-release/SKILL.md`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md`
- Modify: `skills/receiving-code-review/SKILL.md.tmpl`
- Modify: `skills/receiving-code-review/SKILL.md`
- Modify: `skills/systematic-debugging/SKILL.md.tmpl`
- Modify: `skills/systematic-debugging/SKILL.md`
- Modify: `skills/verification-before-completion/SKILL.md.tmpl`
- Modify: `skills/verification-before-completion/SKILL.md`
- Modify: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- Modify: `skills/finishing-a-development-branch/SKILL.md`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `src/workflow/operator.rs`
- Modify: `src/execution/harness.rs`
- Modify: `src/execution/state.rs`
- Modify: `src/execution/topology.rs`
- Modify: `src/cli/plan_execution.rs`
- Modify: `src/output/mod.rs`
- Modify: `schemas/workflow-status.schema.json`
- Modify: `schemas/plan-execution-status.schema.json`
- Test: `tests/runtime_instruction_review_contracts.rs`
- Test: `tests/plan_execution_final_review.rs`
- Test: `tests/workflow_runtime_final_review.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/workflow_shell_smoke.rs`

- [x] **Step 1: Add red tests for scope-check classification, per-gate blocking diagnostics, doctor snapshot parity, and versioning/distribution enforcement**
  Run: `cargo nextest run --test runtime_instruction_review_contracts --test plan_execution_final_review --test workflow_runtime_final_review --test workflow_shell_smoke`
  Expected: failures identify the missing scope-check, operator-snapshot, and release-readiness behavior.

- [x] **Step 2: Implement scope-check artifacts, publishability/versioning checks, and per-gate operator diagnostics in the runtime surfaces with one shared snapshot computation per observed state**
- [x] **Step 3: Update late-stage review and debugging skill templates so they describe the new runtime-owned behavior instead of prose-only heuristics**
- [x] **Step 4: Refresh generated skill docs and status schemas for the expanded doctor/handoff surface**
  Run: `node scripts/gen-skill-docs.mjs`
  Expected: generated docs refresh cleanly.

- [x] **Step 5: Re-run the focused finish-review suites until scope-check, doctor parity, release/versioning behavior, and shared-snapshot single-pass rendering are green**
  Run: `cargo nextest run --test runtime_instruction_review_contracts --test plan_execution_final_review --test workflow_runtime_final_review --test workflow_runtime --test workflow_shell_smoke`
  Expected: all targeted suites pass.

## Task 4: Add Dynamic Gate Signals, Design/Security Review Surfaces, and Shared Gate-Artifact Envelopes

**Spec Coverage:** REQ-012, REQ-013, REQ-014, REQ-015, REQ-016, REQ-028, REQ-030, REQ-031, VERIFY-001, VERIFY-004
**Task Outcome:** Plans carry `Risk & Gate Signals`, `plan-eng-review` finalizes them, the runtime routes from approved signals, and new design/security review artifacts use the shared trust-boundary envelope.
**Plan Constraints:**

- Split approval-time signal derivation failures from downstream read/render failures exactly as the approved spec requires.
- Treat design/security review artifacts as gate-satisfying trust-boundary objects, not convenience markdown.
- Keep legacy no-signal plans readable but conservatively routed.

**Open Questions:** none

**Files:**

- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `skills/document-release/SKILL.md.tmpl`
- Modify: `skills/document-release/SKILL.md`
- Modify: `skills/qa-only/SKILL.md.tmpl`
- Modify: `skills/qa-only/SKILL.md`
- Create: `skills/plan-design-review/SKILL.md.tmpl`
- Create: `skills/plan-design-review/SKILL.md`
- Create: `skills/plan-design-review/references/checklist.md`
- Create: `skills/security-review/SKILL.md.tmpl`
- Create: `skills/security-review/SKILL.md`
- Create: `skills/security-review/references/checklist.md`
- Modify: `src/contracts/plan.rs`
- Modify: `src/execution/topology.rs`
- Modify: `src/execution/harness.rs`
- Modify: `src/execution/state.rs`
- Modify: `src/workflow/status.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/cli/plan_execution.rs`
- Modify: `src/lib.rs`
- Modify: `schemas/plan-contract-analyze.schema.json`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-status.schema.json`
- Test: `tests/contracts_spec_plan.rs`
- Test: `tests/runtime_instruction_plan_review_contracts.rs`
- Test: `tests/runtime_instruction_review_contracts.rs`
- Test: `tests/plan_execution_final_review.rs`
- Test: `tests/workflow_runtime_final_review.rs`
- Test: `tests/workflow_runtime.rs`

- [ ] **Step 1: Add red contract, routing, and freshness coverage for `Risk & Gate Signals`, required design/security reviews, and trust-boundary artifact validation**

  **Execution Note:** Active - Add red contract, routing, and freshness coverage for `Risk & Gate Signals`, required design/security reviews, and tr...
  Run: `cargo nextest run --test contracts_spec_plan --test runtime_instruction_plan_review_contracts --test runtime_instruction_review_contracts --test workflow_runtime_final_review`
  Expected: failures identify missing signal parsing, trust checks, and downstream gate routing.

- [ ] **Step 2: Extend the plan/runtime contracts so approved signals become canonical routing input and gate artifacts share one envelope and trust model**
- [ ] **Step 3: Create the new checked-in `plan-design-review` and `security-review` skill surfaces plus companion checklists**
- [ ] **Step 4: Wire workflow/operator/execution surfaces to route from approved signals and enforce artifact freshness/trust checks**
- [ ] **Step 5: Regenerate skill docs and re-run the focused suites until dynamic gate routing and artifact trust checks are green**
  Run: `node scripts/gen-skill-docs.mjs && cargo nextest run --test contracts_spec_plan --test runtime_instruction_plan_review_contracts --test runtime_instruction_review_contracts --test plan_execution_final_review --test workflow_runtime_final_review --test workflow_runtime`
  Expected: all targeted suites pass.

## Task 5: Build the Worktree Substrate and Lane Evidence Plumbing

**Spec Coverage:** REQ-019, REQ-021, VERIFY-001, VERIFY-004
**Task Outcome:** Execution lanes gain a runtime-owned worktree manager, default-path recommendation behavior, per-lane manifests/diff evidence, patch harvesting, and `resolution_required` lane state before fence enforcement is layered on.
**Plan Constraints:**

- Keep serious execution on `executing-plans` and `subagent-driven-development`; `dispatching-parallel-agents` is coordination-only after this task.
- Leave fence-mode enforcement and rollout-threshold decisions to Task 6 so this slice can stay substrate-first.
- Patch-harvest collision or missing metadata must block lane completion and reuse.

**Open Questions:** none

**Files:**

- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `skills/dispatching-parallel-agents/SKILL.md.tmpl`
- Modify: `skills/dispatching-parallel-agents/SKILL.md`
- Modify: `skills/using-git-worktrees/SKILL.md.tmpl`
- Modify: `skills/using-git-worktrees/SKILL.md`
- Modify: `src/execution/mod.rs`
- Create: `src/execution/worktree_manager.rs`
- Modify: `src/execution/harness.rs`
- Modify: `src/execution/state.rs`
- Modify: `src/execution/topology.rs`
- Modify: `src/cli/plan_execution.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-status.schema.json`
- Test: `tests/contracts_execution_leases.rs`
- Test: `tests/execution_harness_state.rs`
- Test: `tests/plan_execution_topology.rs`
- Test: `tests/plan_execution.rs`
- Test: `tests/workflow_runtime.rs`

- [ ] **Step 1: Add red execution-state and topology tests for worktree allocation, default-path recommendation, lane manifests, patch harvesting, and `resolution_required` lanes**
  Run: `cargo nextest run --test contracts_execution_leases --test execution_harness_state --test plan_execution_topology --test plan_execution`
  Expected: failures identify the missing worktree manager, lane evidence, and recovery-state surfaces.

- [ ] **Step 2: Create the reusable worktree manager module and wire patch harvesting, dedup, changed-file manifests, and blocked-lane state through execution runtime code**
- [ ] **Step 3: Update execution-facing skill templates and generated docs so serious parallel execution routes through the runtime-owned substrate and preferred default path**
- [ ] **Step 4: Surface lane evidence and blocked-lane detail through operator/status outputs**
- [ ] **Step 5: Re-run the focused execution suites until lane state, evidence plumbing, and worktree management are green**
  Run: `node scripts/gen-skill-docs.mjs && cargo nextest run --test contracts_execution_leases --test execution_harness_state --test plan_execution_topology --test plan_execution --test workflow_runtime`
  Expected: all targeted suites pass.

## Task 6: Add Task-Slice Fence Modes and Rollout Enforcement

**Spec Coverage:** REQ-020, REQ-021, VERIFY-001, VERIFY-004
**Task Outcome:** Task-slice fences, override capture, audit-to-enforcement rollout modes, and rollout-readiness counters layer cleanly onto the substrate from Task 5.
**Plan Constraints:**

- Build on the worktree manager and lane evidence from Task 5 rather than re-embedding substrate logic.
- Record false-positive, override, and blocked-lane metrics as runtime-owned truth, not CI-only prose.
- Keep initial rollout non-retroactive for already approved artifacts unless they are revised or reopened.

**Open Questions:** none

**Files:**

- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `skills/dispatching-parallel-agents/SKILL.md.tmpl`
- Modify: `skills/dispatching-parallel-agents/SKILL.md`
- Modify: `skills/using-git-worktrees/SKILL.md.tmpl`
- Modify: `skills/using-git-worktrees/SKILL.md`
- Modify: `src/repo_safety/mod.rs`
- Modify: `src/execution/harness.rs`
- Modify: `src/execution/state.rs`
- Modify: `src/execution/topology.rs`
- Modify: `src/cli/plan_execution.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-status.schema.json`
- Test: `tests/contracts_execution_leases.rs`
- Test: `tests/execution_harness_state.rs`
- Test: `tests/plan_execution_topology.rs`
- Test: `tests/plan_execution.rs`
- Test: `tests/workflow_runtime.rs`

- [ ] **Step 1: Add red execution-state and topology tests for audit mode, guarded/full enforcement, override capture, and rollout counters**
  Run: `cargo nextest run --test contracts_execution_leases --test execution_harness_state --test plan_execution_topology --test plan_execution --test workflow_runtime`
  Expected: failures identify the missing fence-mode and rollout-readiness behavior.

- [ ] **Step 2: Implement task-slice fence detection, override capture, and rollout-threshold state on top of the substrate from Task 5**
- [ ] **Step 3: Update execution-facing skill templates so fence behavior and override flow match the runtime-owned rollout model**
- [ ] **Step 4: Surface false-positive, override, blocked-lane, and enforcement-mode detail through operator/status outputs**
- [ ] **Step 5: Re-run the focused execution suites until fence modes, rollout counters, and operator reporting are green**
  Run: `node scripts/gen-skill-docs.mjs && cargo nextest run --test contracts_execution_leases --test execution_harness_state --test plan_execution_topology --test plan_execution --test workflow_runtime`
  Expected: all targeted suites pass.

## Task 7: Reorder the Finish Path and Enforce Dynamic Gate Precedence

**Spec Coverage:** REQ-009, REQ-010, REQ-014, REQ-016, REQ-017, REQ-018, REQ-027, VERIFY-001, VERIFY-004
**Task Outcome:** Late-stage routing runs document/security completion work before final code review, applies explicit gate precedence, and preserves strict freshness without the old stale-review loop.
**Plan Constraints:**

- Final code review must observe the near-finished repo state after repo-affecting completion work.
- Required security review must happen before final code review when signals demand it.
- QA stays a verification gate after final review.
- Reserve top-level README/doc wording to Task 9 so finish-path runtime work does not share a hotspot with the active-doc cleanup task.

**Open Questions:** none

**Files:**

- Modify: `src/workflow/operator.rs`
- Modify: `src/execution/state.rs`
- Modify: `src/execution/harness.rs`
- Modify: `src/execution/topology.rs`
- Modify: `skills/document-release/SKILL.md.tmpl`
- Modify: `skills/document-release/SKILL.md`
- Modify: `skills/security-review/SKILL.md.tmpl`
- Modify: `skills/security-review/SKILL.md`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md`
- Modify: `skills/qa-only/SKILL.md.tmpl`
- Modify: `skills/qa-only/SKILL.md`
- Modify: `skills/verification-before-completion/SKILL.md.tmpl`
- Modify: `skills/verification-before-completion/SKILL.md`
- Modify: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- Modify: `skills/finishing-a-development-branch/SKILL.md`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-status.schema.json`
- Test: `tests/plan_execution_final_review.rs`
- Test: `tests/workflow_runtime_final_review.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Add red tests for gate-precedence combinations, stale-artifact transitions, and security-plus-QA routing combinations**
  Run: `cargo nextest run --test plan_execution_final_review --test workflow_runtime_final_review --test workflow_runtime --test workflow_shell_smoke`
  Expected: failures identify the old late-stage ordering and stale-review loop.

- [ ] **Step 2: Reorder the finish-path runtime and freshness logic to honor pre-final-review completion gates before final review**
- [ ] **Step 3: Update the late-stage skill docs to describe the new order and decision points, and reserve top-level README/overview wording for Task 9**
- [ ] **Step 4: Refresh status/doctor schemas and shell-smoke fixtures for the reordered finish path**
- [ ] **Step 5: Re-run the focused finish suites until gate precedence and freshness are green**
  Run: `node scripts/gen-skill-docs.mjs && cargo nextest run --test plan_execution_final_review --test workflow_runtime_final_review --test workflow_runtime --test workflow_shell_smoke`
  Expected: all targeted suites pass.

## Task 8: Add Shell-Friendly CLI Contracts and Remove Interpreter Parsing from Generated Skills

**Spec Coverage:** REQ-011, REQ-022, REQ-032, VERIFY-001
**Task Outcome:** FeatureForge exposes stable shell-friendly CLI output for high-traffic read-only surfaces and generated skills stop using ad hoc interpreter snippets to parse FeatureForge-owned output.
**Plan Constraints:**

- Keep JSON and human-readable output support alongside shell fields.
- Remove parser snippets from generated skills only after the runtime-owned fields exist.
- Keep output shape stable enough for doc-contract and schema assertions.
- Reuse the shared operator snapshot from Task 3 rather than introducing output-mode-specific recomputation for shell fields.

**Open Questions:** none

**Files:**

- Modify: `src/cli/plan_contract.rs`
- Modify: `src/cli/plan_execution.rs`
- Modify: `src/cli/workflow.rs`
- Modify: `src/output/mod.rs`
- Modify: `src/lib.rs`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `TODOS.md`
- Modify: `schemas/plan-contract-analyze.schema.json`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-status.schema.json`
- Test: `tests/contracts_spec_plan.rs`
- Test: `tests/plan_execution.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/packet_and_schema.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add red CLI-contract and doc-contract coverage for `--field`, `--format shell`, and forbidden parser snippets in generated skills**
  Run: `cargo nextest run --test contracts_spec_plan --test plan_execution --test workflow_runtime --test runtime_instruction_contracts --test packet_and_schema && node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
  Expected: failures identify missing shell fields and forbidden snippet cleanup.

- [ ] **Step 2: Implement stable shell-friendly output for the plan-contract, plan-execution, and workflow surfaces through the shared output helpers without triggering separate artifact/worktree scans per output mode**
- [ ] **Step 3: Update the affected skill templates/generated docs so they branch on runtime-owned shell fields instead of interpreter snippets**
- [ ] **Step 4: Refresh schemas and any outstanding TODO/documentation references tied to the new CLI contracts**
- [ ] **Step 5: Re-run the focused contract suites until shell fields and parser-free skills are green**
  Run: `node scripts/gen-skill-docs.mjs && cargo nextest run --test contracts_spec_plan --test plan_execution --test workflow_runtime --test runtime_instruction_contracts --test packet_and_schema && node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
  Expected: all targeted suites pass.

## Task 9: Clean Active Namespace and Path Drift from Current Guidance Surfaces

**Spec Coverage:** REQ-002, REQ-023, VERIFY-001, VERIFY-003
**Task Outcome:** Active user-guidance docs and prompts reflect the final workflow state, including the public `plan-fidelity-review` stage and reordered finish path, while also removing stale `Superpowers` naming and machine-local absolute paths.
**Plan Constraints:**

- Do not edit archives or historical execution evidence.
- Prefer repo-relative or portable paths over machine-local absolute paths.
- Keep this cleanup aligned with the now-final runtime/skill behavior from Tasks 1 through 8.

**Open Questions:** none

**Files:**

- Modify: `AGENTS.md`
- Modify: `README.md`
- Modify: `docs/README.codex.md`
- Modify: `docs/README.copilot.md`
- Modify: `docs/testing.md`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add or tighten active-doc scan expectations for stale `Superpowers` naming and machine-local absolute paths in non-archive guidance**
  Run: `cargo nextest run --test runtime_instruction_contracts && node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
  Expected: failures identify the remaining stale names and paths in active surfaces.

- [ ] **Step 2: Update the named active guidance surfaces to match the final workflow state from Tasks 1 through 8, including the public `plan-fidelity-review` stage and the reordered finish path, while leaving archived and source-governing artifacts untouched**
- [ ] **Step 3: Refresh any generated examples or current guidance snippets still emitting stale names or paths**
- [ ] **Step 4: Re-run the active-doc scan and contract tests until current guidance is clean**
  Run: `cargo nextest run --test runtime_instruction_contracts && node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
  Expected: all targeted checks pass.

## Task 10: Compact Top-Level Skill Docs and Add Size-Budget Reporting

**Spec Coverage:** REQ-024, VERIFY-001, VERIFY-003
**Task Outcome:** The busiest top-level generated skills become materially shorter while mandatory operational law stays visible and the generator reports reproducible size-budget baselines and regressions.
**Plan Constraints:**

- Do not move mandatory stop/fail-closed rules or helper invocations out of top-level `SKILL.md` files.
- Move long-form examples and repeated narrative into exact companion refs or prompts.
- Keep compaction aligned with the final runtime behavior after Tasks 1 through 9.

**Open Questions:** none

**Files:**

- Modify: `scripts/gen-skill-docs.mjs`
- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `skills/using-featureforge/references/codex-tools.md`
- Modify: `skills/plan-ceo-review/SKILL.md.tmpl`
- Modify: `skills/plan-ceo-review/SKILL.md`
- Modify: `skills/plan-ceo-review/accelerated-reviewer-prompt.md`
- Modify: `skills/plan-ceo-review/outside-voice-prompt.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Create: `skills/writing-plans/references/checklist.md`
- Modify: `skills/plan-eng-review/SKILL.md.tmpl`
- Modify: `skills/plan-eng-review/SKILL.md`
- Modify: `skills/plan-eng-review/accelerated-reviewer-prompt.md`
- Modify: `skills/plan-eng-review/outside-voice-prompt.md`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md`
- Modify: `skills/requesting-code-review/code-reviewer.md`
- Modify: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- Modify: `skills/finishing-a-development-branch/SKILL.md`
- Create: `skills/finishing-a-development-branch/references/checklist.md`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `skills/subagent-driven-development/code-quality-reviewer-prompt.md`
- Modify: `skills/subagent-driven-development/implementer-prompt.md`
- Modify: `skills/subagent-driven-development/spec-reviewer-prompt.md`
- Modify: `skills/plan-fidelity-review/SKILL.md.tmpl`
- Modify: `skills/plan-fidelity-review/SKILL.md`
- Modify: `skills/plan-fidelity-review/references/checklist.md`
- Modify: `skills/plan-design-review/SKILL.md.tmpl`
- Modify: `skills/plan-design-review/SKILL.md`
- Modify: `skills/plan-design-review/references/checklist.md`
- Modify: `skills/security-review/SKILL.md.tmpl`
- Modify: `skills/security-review/SKILL.md`
- Modify: `skills/security-review/references/checklist.md`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/skill-doc-generation.test.mjs`

- [ ] **Step 1: Add red size-budget reporting assertions and top-level-law preservation checks for the compaction targets**
  Run: `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs`
  Expected: failures identify missing size-budget reporting and overgrown top-level docs.

- [ ] **Step 2: Extend the skill-doc generator to report baselines/current totals and support the two-layer top-level-plus-companion model**
- [ ] **Step 3: Compact the named skill templates and move non-load-bearing detail into the named companion files**
- [ ] **Step 4: Regenerate all affected skill docs and verify the top-level law still appears verbatim where required**
  Run: `node scripts/gen-skill-docs.mjs`
  Expected: generated skill docs refresh cleanly with shorter top-level surfaces.

- [ ] **Step 5: Re-run the focused Rust and Node doc suites until compaction and size-budget reporting are green**
  Run: `cargo nextest run --test runtime_instruction_contracts && node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs`
  Expected: all targeted suites pass.

## Task 11: Ratify the Combined Program and Close the Implementation Slice

**Spec Coverage:** VERIFY-001, VERIFY-002, VERIFY-004
**Task Outcome:** The combined workflow-optimization program has passing full-regression evidence, synchronized schemas/fixtures/tests, and a final implementation-state verification pass for runtime-bearing surfaces before downstream finish gates.
**Plan Constraints:**

- Do not weaken failing tests to make the umbrella plan appear simpler.
- Treat this task as ratification only; do not introduce new design scope here.
- Keep this task inside implementation/runtime scope; planning-stage gates such as plan-fidelity review stay outside the execution DAG.
- If post-ratification drift is limited to contributor/operator guidance or backlog notes, leave that cleanup to Task 12 instead of expanding this ratification slice.
- Treat repeated snapshot recomputation or render-mode-specific rescans as a regression to fix before this task is complete.

**Open Questions:** none

**Files:**

- Modify: `schemas/plan-contract-analyze.schema.json`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-status.schema.json`
- Modify: `tests/contracts_spec_plan.rs`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/runtime_instruction_plan_review_contracts.rs`
- Modify: `tests/runtime_instruction_execution_contracts.rs`
- Modify: `tests/runtime_instruction_review_contracts.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_runtime_final_review.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/plan_execution.rs`
- Modify: `tests/plan_execution_final_review.rs`
- Modify: `tests/plan_execution_topology.rs`
- Modify: `tests/contracts_execution_leases.rs`
- Modify: `tests/execution_harness_state.rs`
- Test: `tests/contracts_spec_plan.rs`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/runtime_instruction_plan_review_contracts.rs`
- Test: `tests/runtime_instruction_execution_contracts.rs`
- Test: `tests/runtime_instruction_review_contracts.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/workflow_runtime_final_review.rs`
- Test: `tests/workflow_shell_smoke.rs`
- Test: `tests/plan_execution.rs`
- Test: `tests/plan_execution_final_review.rs`
- Test: `tests/plan_execution_topology.rs`
- Test: `tests/contracts_execution_leases.rs`
- Test: `tests/execution_harness_state.rs`
- Test: `tests/codex-runtime/*.test.mjs`

- [ ] **Step 1: Refresh any schemas, fixtures, and test surfaces that still lag the implemented runtime truth**
- [ ] **Step 2: Run the full regression gate named in the validation strategy and fix remaining integration failures, including any evidence that doctor/handoff/json/shell render paths re-scan independently**
  Run: `node scripts/gen-skill-docs.mjs --check && node scripts/gen-agent-docs.mjs --check && node --test tests/codex-runtime/*.test.mjs && cargo nextest run --test contracts_spec_plan --test runtime_instruction_contracts --test runtime_instruction_plan_review_contracts --test runtime_instruction_execution_contracts --test runtime_instruction_review_contracts --test using_featureforge_skill --test workflow_runtime --test workflow_runtime_final_review --test workflow_shell_smoke --test plan_execution --test plan_execution_final_review --test plan_execution_topology --test contracts_execution_leases --test execution_harness_state && cargo clippy --all-targets --all-features -- -D warnings`
  Expected: the full umbrella-program regression gate passes.

- [ ] **Step 3: Regenerate any final generated docs/artifacts once the full gate is green and confirm no checked-in drift remains beyond the intended diff**
- [ ] **Step 4: Confirm the implementation leaves downstream finish gates to authoritative runtime routing rather than ad hoc manual sequencing**
## Task 12: Clean Up Post-Ratification Guidance and Backlog Drift

**Spec Coverage:** VERIFY-001, VERIFY-003
**Task Outcome:** After Task 11 proves the implementation slice is green, any remaining contributor/operator guidance drift or backlog-note cleanup is reconciled in an isolated docs/backlog task without reopening runtime scope.
**Plan Constraints:**

- Keep this task docs/backlog-only. Do not use it to absorb runtime, schema, or test fixes that should have been resolved before or during Task 11.
- If Task 11 uncovers runtime-bearing failures, reopen the owning implementation task instead of patching them here.
- Re-run only the checks needed to prove the cleanup did not reintroduce stale guidance or prompt-surface drift.

**Open Questions:** none

**Files:**

- Modify: `docs/testing.md`
- Modify: `TODOS.md`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/*.test.mjs`

- [ ] **Step 1: Update docs/testing guidance and TODO/backlog notes only when the completed ratification pass leaves non-runtime drift to reconcile**
- [ ] **Step 2: Re-run the doc- and prompt-focused checks needed to prove the cleanup did not reintroduce stale guidance**
  Run: `node scripts/gen-skill-docs.mjs --check && node scripts/gen-agent-docs.mjs --check && node --test tests/codex-runtime/*.test.mjs && cargo nextest run --test runtime_instruction_contracts`
  Expected: docs/backlog cleanup stays green without reopening runtime scope.

- [ ] **Step 3: Confirm the post-ratification cleanup remained docs/backlog-only and left downstream finish gates to authoritative runtime routing**
