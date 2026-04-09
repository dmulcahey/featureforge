# Shell/Field Output Contracts Implementation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>` after engineering approval; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-04-08-shell-field-output-contracts-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

**Goal:** Add runtime-owned `--field` and `--format shell` output contracts for targeted CLI commands, migrate active skills off interpreter parsing for those commands, and lock regressions with contract tests.

**Architecture:** Extend command argument surfaces to support field/shell modes, map selected runtime query outputs to stable scalar keys, and route rendering through command-owned text emitters while preserving JSON compatibility. Keep rollout additive and fail closed on unsupported fields and invalid argument combinations.

**Tech Stack:** Rust (`clap`, `serde_json`), Node test harness for generated skills, existing workflow/plan shell smoke test suites

---

## Plan Contract

This plan defines implementation order and verification for shell/field output contracts. If this plan conflicts with the approved source spec, the source spec wins and this plan must be updated in the same change.

## Existing Capabilities / Built-ins to Reuse

- `src/lib.rs` already supports dual text/JSON emit paths (`emit_json`, `emit_text`) and should be reused for mode switching.
- `src/contracts/runtime.rs` already owns `analyze-plan` output generation and is the correct boundary for adding field/shell render contracts.
- `src/workflow/operator.rs` and `src/execution/state.rs` already centralize operator/status payload derivation and should remain source-of-truth for field values.
- `tests/workflow_shell_smoke.rs`, `tests/contracts_spec_plan.rs`, and `tests/plan_execution.rs` already cover command behavior and can be extended for output-shape assertions.
- `skills/requesting-code-review/SKILL.md.tmpl` plus `node scripts/gen-skill-docs.mjs` is the canonical generated-skill update path.

## Known Footguns / Constraints

- Preserve compatibility for existing JSON consumers (`--format json` and legacy `--json` where currently supported).
- Do not let shell key names drift; key names and order are contract surface.
- `eval "$(...)"` safety depends on quoting correctness; shell rendering must be rigorously escaped.
- `src/lib.rs` is a hotspot shared across command families; sequence tasks to avoid merge churn.
- Generated skill docs must be updated through template + generator, not hand-edited.

## Cross-Task Invariants

- Use `featureforge:test-driven-development`: add or adjust failing assertions before implementation in each task.
- Keep `cargo clippy --all-targets --all-features -- -D warnings` clean.
- Keep command behavior fail-closed for unsupported fields and invalid flag combinations.
- Preserve existing JSON output semantics while adding field/shell surfaces.
- Regenerate skill docs in the same task that edits skill templates.

## Change Surface

- CLI arg surfaces: `src/cli/plan_contract.rs`, `src/cli/plan_execution.rs`, `src/cli/workflow.rs`
- Runtime render and mode routing: `src/contracts/runtime.rs`, `src/execution/state.rs`, `src/workflow/operator.rs`, `src/lib.rs`
- Generated skills and generator tests: `skills/requesting-code-review/SKILL.md.tmpl`, `skills/requesting-code-review/SKILL.md`, `tests/codex-runtime/skill-doc-contracts.test.mjs`, `tests/codex-runtime/gen-skill-docs.unit.test.mjs`, `tests/runtime_instruction_contracts.rs`
- CLI/runtime tests: `tests/contracts_spec_plan.rs`, `tests/plan_execution.rs`, `tests/workflow_shell_smoke.rs`, `tests/workflow_runtime.rs`, `tests/cli_parse_boundary.rs`

## Preconditions

- Approved source spec exists at `docs/featureforge/specs/2026-04-08-shell-field-output-contracts-design.md` with:
  - `**Workflow State:** CEO Approved`
  - `**Spec Revision:** 1`
  - `**Last Reviewed By:** plan-ceo-review`
- Rust and Node toolchains are available.
- Packaged helper binary exists at `~/.featureforge/install/bin/featureforge`.

## Evidence Expectations

- `analyze-plan` supports `--field` and `--format shell` with stable field names and shell keys.
- `plan execution status`, `gate-review`, `record-review-dispatch` support `--field` and `--format shell`.
- `workflow operator` supports `--field` and `--format shell`; `workflow doctor` is implemented or explicitly deferred with pinned follow-up tests.
- `requesting-code-review` no longer teaches interpreter parsing for FeatureForge-owned command extraction.
- Contract tests fail if forbidden parser snippets return in covered generated skill paths.

## Validation Strategy

- Task-level targeted tests per task.
- Final verification gate:
  - `node scripts/gen-skill-docs.mjs`
  - `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `cargo test --test contracts_spec_plan --test plan_execution --test workflow_runtime --test workflow_shell_smoke --test cli_parse_boundary`
  - `cargo clippy --all-targets --all-features -- -D warnings`

## Documentation Update Expectations

- `skills/requesting-code-review/SKILL.md` generated output must show field/shell contract usage for covered commands.
- Skill contract tests must codify the no-interpreter rule for covered FeatureForge command parsing paths.

## Rollout Plan

- Land additive CLI command support first.
- Migrate `requesting-code-review` immediately after runtime support is in place.
- Enforce parser-regression tests in same slice to prevent rollback drift.

## Rollback Plan

- If migration introduces issues, revert skill-template/doc changes first while retaining additive runtime support.
- If runtime field/shell rendering regresses, revert rendering path changes while keeping prior JSON path untouched.

## Risks and Mitigations

- Risk: shell escaping bug enables unsafe `eval` behavior.
  - Mitigation: explicit escaping tests for apostrophes/newlines/spaces.
- Risk: output contract drift across command families.
  - Mitigation: pinned command-level output tests and explicit field enums.
- Risk: old parser snippets persist in generated instructions.
  - Mitigation: generated-skill contract tests that fail on forbidden snippets for covered paths.

## Execution Strategy

- Execute Task 1 serially. It establishes the analyze-plan field/shell reference pattern and helper behavior.
- Execute Task 2 serially after Task 1. It extends plan-execution command surfaces using the same pattern.
- Execute Task 3 serially after Task 2. It applies workflow operator/doctor output contract updates and compatibility handling.
- Execute Task 4 serially after Task 3. It migrates requesting-code-review and regenerates docs against implemented command surfaces.
- Execute Task 5 serially after Task 4. It hardens contract tests and runs integrated verification gates.

## Dependency Diagram

```text
Task 1 -> Task 2
Task 2 -> Task 3
Task 3 -> Task 4
Task 4 -> Task 5
```

## Requirement Coverage Matrix

- REQ-001 -> Task 1, Task 4
- REQ-002 -> Task 2, Task 4
- REQ-003 -> Task 2, Task 4
- REQ-004 -> Task 3, Task 4
- REQ-005 -> Task 3
- REQ-006 -> Task 1, Task 2, Task 3
- REQ-007 -> Task 1, Task 2, Task 3
- REQ-008 -> Task 1, Task 2, Task 3, Task 4
- REQ-009 -> Task 2, Task 3
- REQ-010 -> Task 1, Task 2, Task 3, Task 5
- VERIFY-001 -> Task 4, Task 5
- VERIFY-002 -> Task 1, Task 2, Task 3, Task 5
- VERIFY-003 -> Task 1, Task 2, Task 3, Task 5

## Task 1: Add Analyze-Plan Field/Shell Output Contract

**Spec Coverage:** REQ-001, REQ-006, REQ-007, REQ-008, REQ-010, VERIFY-002, VERIFY-003  
**Task Outcome:** `plan contract analyze-plan` exposes stable `--field` and `--format shell` contracts while preserving JSON output.
**Plan Constraints:**
- Preserve current JSON schema values.
- Unsupported fields must fail non-zero with clear error text.
**Open Questions:** none

**Files:**
- Modify: `src/cli/plan_contract.rs`
- Modify: `src/contracts/runtime.rs`
- Modify: `tests/contracts_spec_plan.rs`

- [ ] **Step 1: Add red tests in `tests/contracts_spec_plan.rs` for analyze-plan `--field`, `--format shell`, unsupported field, and shell escaping behavior**
- [ ] **Step 2: Run targeted test to confirm red state**
Run: `cargo test --test contracts_spec_plan`  
Expected: failures on new analyze-plan output assertions
- [ ] **Step 3: Extend `AnalyzeOutputFormat` and CLI args in `src/cli/plan_contract.rs` to support `json|shell` plus field selection**
- [ ] **Step 4: Implement analyze-plan field/shell render paths in `src/contracts/runtime.rs` with stable key ordering and scalar semantics**
- [ ] **Step 5: Re-run targeted test and confirm green**
Run: `cargo test --test contracts_spec_plan`  
Expected: analyze-plan output-contract assertions pass
- [ ] **Step 6: Commit**
```bash
git add src/cli/plan_contract.rs src/contracts/runtime.rs tests/contracts_spec_plan.rs
git commit -m "feat: add analyze-plan field and shell output contracts"
```

## Task 2: Add Field/Shell Contract For Plan Execution Status and Review Gates

**Spec Coverage:** REQ-002, REQ-003, REQ-006, REQ-007, REQ-008, REQ-009, REQ-010, VERIFY-002, VERIFY-003  
**Task Outcome:** `plan execution status`, `gate-review`, and `record-review-dispatch` support field/shell output contracts with stable mappings.
**Plan Constraints:**
- Preserve existing JSON envelopes for these commands.
- Keep command-specific key sets minimal and spec-aligned.
**Open Questions:** none

**Files:**
- Modify: `src/cli/plan_execution.rs`
- Modify: `src/execution/state.rs`
- Modify: `src/lib.rs`
- Modify: `tests/plan_execution.rs`
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Add red tests covering plan-execution `--field` and `--format shell` for status/gate-review/record-review-dispatch plus invalid-field behavior**
- [ ] **Step 2: Run targeted tests to confirm red state**
Run: `cargo test --test plan_execution --test workflow_shell_smoke`  
Expected: failures for missing field/shell command support
- [ ] **Step 3: Add CLI arg surfaces in `src/cli/plan_execution.rs` for format/field selection on covered commands**
- [ ] **Step 4: Implement render/mapping helpers in `src/execution/state.rs` and route mode switching in `src/lib.rs`**
- [ ] **Step 5: Re-run targeted tests and confirm green**
Run: `cargo test --test plan_execution --test workflow_shell_smoke`  
Expected: covered plan-execution output tests pass
- [ ] **Step 6: Commit**
```bash
git add src/cli/plan_execution.rs src/execution/state.rs src/lib.rs tests/plan_execution.rs tests/workflow_shell_smoke.rs
git commit -m "feat: add field and shell output for execution status and review gates"
```

## Task 3: Add Field/Shell Contract For Workflow Operator and Doctor

**Spec Coverage:** REQ-004, REQ-005, REQ-006, REQ-007, REQ-008, REQ-009, REQ-010, VERIFY-002, VERIFY-003  
**Task Outcome:** `workflow operator` supports field/shell contracts and `workflow doctor` supports them in v1 (or is explicitly deferred with pinned test expectation).
**Plan Constraints:**
- Preserve existing `--json` compatibility.
- Keep phase/routing semantics unchanged; only output surface changes.
**Open Questions:** none

**Files:**
- Modify: `src/cli/workflow.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/lib.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/cli_parse_boundary.rs`

- [ ] **Step 1: Add red tests for workflow operator/doctor `--field` and `--format shell`, plus legacy `--json` compatibility checks**
- [ ] **Step 2: Run targeted tests to confirm red state**
Run: `cargo test --test workflow_runtime --test workflow_shell_smoke --test cli_parse_boundary`  
Expected: failures for missing output-mode support and argument parsing
- [ ] **Step 3: Extend workflow CLI arg structures in `src/cli/workflow.rs` and wire mode handling in `src/lib.rs`**
- [ ] **Step 4: Implement workflow operator/doctor field extraction and shell render paths in `src/workflow/operator.rs`**
- [ ] **Step 5: Re-run targeted tests and confirm green**
Run: `cargo test --test workflow_runtime --test workflow_shell_smoke --test cli_parse_boundary`  
Expected: workflow output-contract tests pass
- [ ] **Step 6: Commit**
```bash
git add src/cli/workflow.rs src/workflow/operator.rs src/lib.rs tests/workflow_runtime.rs tests/workflow_shell_smoke.rs tests/cli_parse_boundary.rs
git commit -m "feat: add field and shell output for workflow operator and doctor"
```

## Task 4: Migrate Requesting-Code-Review Away From Interpreter Parsing

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-004, REQ-008, VERIFY-001  
**Task Outcome:** `requesting-code-review` uses runtime-owned field/shell outputs for covered commands and no longer teaches interpreter parsing for those paths.
**Plan Constraints:**
- Edit template first, regenerate output after.
- Keep unrelated review guidance unchanged.
**Open Questions:** none

**Files:**
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- Modify: `tests/runtime_instruction_contracts.rs`

- [ ] **Step 1: Add red skill-contract assertions that require field/shell usage and reject interpreter snippets for covered command parsing paths**
- [ ] **Step 2: Run targeted Node/Rust tests and confirm red state**
Run: `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs`  
Run: `cargo test --test runtime_instruction_contracts`  
Expected: failures on old requesting-code-review snippets
- [ ] **Step 3: Update `skills/requesting-code-review/SKILL.md.tmpl` to use `--field` and/or `--format shell` for covered commands**
- [ ] **Step 4: Regenerate skill docs**
Run: `node scripts/gen-skill-docs.mjs`
- [ ] **Step 5: Re-run targeted tests and confirm green**
Run: `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs`  
Run: `cargo test --test runtime_instruction_contracts`  
Expected: no interpreter parsing required in covered generated paths
- [ ] **Step 6: Commit**
```bash
git add skills/requesting-code-review/SKILL.md.tmpl skills/requesting-code-review/SKILL.md tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/runtime_instruction_contracts.rs
git commit -m "docs: migrate requesting-code-review to field and shell contracts"
```

## Task 5: Ratify Contracts, Compatibility, and Regression Gates

**Spec Coverage:** REQ-010, VERIFY-001, VERIFY-002, VERIFY-003  
**Task Outcome:** End-to-end contract coverage validates stable field names, shell keys/order/escaping, JSON compatibility, and parser-regression protection.
**Plan Constraints:**
- Treat any output-key drift as contract failure.
- Keep fail-closed behavior for unsupported field requests.
**Open Questions:** none

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/contracts_spec_plan.rs`
- Modify: `tests/plan_execution.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add/normalize parity assertions that pin field names, shell key names/order, and JSON compatibility across covered commands**
- [ ] **Step 2: Run full targeted verification gate and confirm green**
Run: `node scripts/gen-skill-docs.mjs`  
Run: `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs`  
Run: `cargo test --test contracts_spec_plan --test plan_execution --test workflow_runtime --test workflow_shell_smoke --test cli_parse_boundary --test runtime_instruction_contracts`  
Run: `cargo clippy --all-targets --all-features -- -D warnings`  
Expected: all checks pass with stable output contracts and no parser-regression violations
- [ ] **Step 3: Commit**
```bash
git add tests/workflow_shell_smoke.rs tests/contracts_spec_plan.rs tests/plan_execution.rs tests/codex-runtime/skill-doc-contracts.test.mjs
git commit -m "test: pin shell-field contracts and parser-regression guards"
```

## NOT in scope

- Expanding shell/field access to every runtime command in one pass; this plan limits v1 to the active-skill command set from the approved spec.
- Adding generic nested-field query syntax; this would enlarge parser complexity beyond current requirements.
- Changing workflow phase semantics or review-gate policy; this work is output-contract only.
- Removing JSON output surfaces; additive compatibility remains required.

## What already exists

- `src/contracts/runtime.rs` already owns `analyze-plan` payload derivation and is the right source for field/shell mappings.
- `src/execution/state.rs` already owns execution-status/gate query state and should be reused rather than duplicated.
- `src/workflow/operator.rs` already centralizes operator/doctor routing payloads for stable key extraction.
- `tests/workflow_shell_smoke.rs`, `tests/plan_execution.rs`, and `tests/contracts_spec_plan.rs` already provide a base for output-shape pinning.
- `skills/requesting-code-review/SKILL.md.tmpl` plus `node scripts/gen-skill-docs.mjs` already provides the correct generated-doc update flow.

## Failure Modes

- `--format shell` escaping bug for apostrophes or spaces:
  - test coverage: yes (shell quoting/escape assertions in output-shape tests)
  - error handling: yes (fail closed on invalid render states)
  - user impact if triggered: clear command/test failure, not silent
- Unsupported field accepted or mis-mapped:
  - test coverage: yes (invalid-field tests and known-field mapping assertions)
  - error handling: yes (explicit unsupported-field non-zero error path)
  - user impact if triggered: clear command failure, not silent
- Legacy JSON compatibility drift (`--json`/`--format json`):
  - test coverage: yes (compatibility checks in workflow/runtime parse and smoke suites)
  - error handling: yes (existing parser/usage failures are explicit)
  - user impact if triggered: clear integration failure, not silent
- Generated-skill parser regression (interpreter snippets reintroduced):
  - test coverage: yes (skill-doc contract and generator tests)
  - error handling: yes (CI/test failure blocks merge)
  - user impact if triggered: no silent runtime behavior change

## Engineering Review Summary

**Review Status:** clear
**Reviewed At:** 2026-04-08T18:32:26Z
**Review Mode:** small_change
**Reviewed Plan Revision:** 1
**Critical Gaps:** 0
**Browser QA Required:** no
**Test Plan Artifact:** `/Users/dmulcahey/.featureforge/projects/dmulcahey-featureforge/dmulcahey-current-test-plan-20260408-142746.md`
**Outside Voice:** skipped
