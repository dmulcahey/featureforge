# Runtime Safety Thirty-Eighth Audit Remediation

## Workflow State

Engineering remediation plan for the thirty-eighth runtime-safety audit loop. This plan is active until every task below is implemented, fully verified, independently reviewed, and followed by another deep audit loop with the signal-to-noise auditor included.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow/project skills. Do not allow reviewers or implementation subagents to spawn additional subagents. Before each full test cycle, verify no existing `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, `cargo clippy`, or Codex-runtime Node validation process is running.

If a full nextest run exceeds 4-5 minutes, run `cargo clean`, rerun once, and remediate if the regression repeats. If a full test suite run exceeds 10 minutes, stop immediately, run `cargo clean`, rerun, and enter performance remediation if the regression is repeatable.

## Goal

Resolve the actionable thirty-eighth audit findings by deleting or consolidating duplicated workflow law instead of adding more broad static guard layers. The result should keep FeatureForge safer for agents by shrinking conceptual surface area:

- plan approval header truth is derived from one shared contract helper
- active docs avoid retired hidden-command tokens
- focused public contract tests are described accurately
- route-owning skills delegate detailed route binding law to the canonical operator route reference
- public-flow scanner policy is centralized in one typed test-support surface
- command submodule size debt is visible to the large-module boundary guard

## Architecture

Preserve the current runtime architecture:

```text
CLI args
  -> command module
  -> transition guard
  -> event append
  -> reducer
  -> read model
  -> route decision
  -> workflow operator presentation
```

This plan must not alter public route semantics, runtime authority, receipt/projection authority, or late-stage progression. When a finding can be fixed by removing wording or centralizing existing logic, prefer that over adding another scanner exception.

## Change Surface

- `src/contracts/plan.rs`
- `src/workflow/status.rs`
- `src/execution/context.rs`
- `tests/contracts_spec_plan.rs`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `docs/testing.md`
- `references/operator-route-authority.md`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/finishing-a-development-branch/SKILL.md.tmpl`
- generated `skills/*/SKILL.md` files from the edited templates
- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/public_cli_flow_contracts.rs` only if public-flow source-policy assertions need updated names
- `tests/runtime_module_boundaries.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

## Preconditions

- The thirty-seventh remediation is complete and independently reviewed.
- The thirty-eighth audit report is the source finding set for this plan.
- Do not use FeatureForge runtime/workflow/project skills.
- Use Rust best practices while modifying Rust code.
- Run `cargo clean` before each new audit-loop iteration.
- Before every full verification cycle, check that no previous cargo/nextest/clippy/test process is running.
- Generated skill docs must be regenerated from templates, not hand-edited.

## Known Footguns / Constraints

- Do not broaden runtime command reachability.
- Do not introduce hidden/debug compatibility command aliases.
- Do not weaken public-flow, prompt-budget, hidden-helper, or display-command scanners.
- Do not move mandatory route law solely into companion references. Top-level skills must still say: query workflow/operator JSON, execute typed argv/template, stop if absent.
- Do not add more route-law prose to every skill. The goal is consolidation.
- Do not edit archived historical specs/plans for hidden-token cleanup unless a task explicitly says the surface is active.
- Do not split `advance_late_stage` mechanically just to satisfy a line count. First make the debt visible; extract only if a cohesive owned family is obvious and low-risk.
- Keep focused runtime goldens as focused public contract coverage, not full compiled-CLI transition proof.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| REQ-001 Plan-state/reviewer pairing is centralized and analyzer rejects invalid approved plans | Task 1 |
| REQ-002 Active docs do not name retired hidden recovery commands | Task 2 |
| REQ-003 Focused public contract goldens are described without overclaiming public-flow proof | Task 2 |
| REQ-004 Route-owning skills delegate detailed route binding/recovery law to the canonical route reference | Task 3 |
| REQ-005 Public-flow scanner classification/exemption policy is centralized in a typed support surface | Task 4 |
| REQ-006 Large-module guard covers relevant execution subtrees and documents oversized command submodules | Task 5 |

## Task 1: Centralize Plan Approval Header Truth

**Spec Coverage:** REQ-001

**Goal:** Make `src/contracts/plan.rs` the shared owner of the relationship between `Workflow State` and `Last Reviewed By`, then consume that helper from workflow candidate parsing and execution context checks.

**Context:** The audit found `parse_plan_source` validates `Workflow State` and `Last Reviewed By` independently, accepting `Engineering Approved` plus `writing-plans`. Workflow status and execution context reject that combination with local logic. This is not a current execution bypass, but it is duplicated approval-header truth.

**Constraints:**

- Preserve valid states: `Draft` may be reviewed by `writing-plans` or `plan-eng-review`; `Engineering Approved` must be reviewed by `plan-eng-review`.
- Keep malformed approved plans fail-closed for analyzer, workflow candidate parsing, and execution context loading.
- Do not change spec-state reviewer rules.
- Prefer named constants/helpers in `src/contracts/plan.rs` over string duplication in callers.

**Done when:**

- A shared contract helper owns `Workflow State` + `Last Reviewed By` pairing for plans.
- `parse_plan_source` rejects `Engineering Approved` + `writing-plans`.
- `parse_workflow_plan_candidate` uses the shared helper instead of a local `matches!` table.
- `src/execution/context.rs` uses the shared helper when checking approved plan readiness.
- Regression tests cover invalid approved reviewer pairing at analyzer level and preserve valid draft/approved pairings.

**Files:**

- `src/contracts/plan.rs`
- `src/workflow/status.rs`
- `src/execution/context.rs`
- `tests/contracts_spec_plan.rs`

**Detailed Implementation Steps:**

1. Add plan workflow-state and reviewer constants in `src/contracts/plan.rs` for `Draft`, `Engineering Approved`, `writing-plans`, and `plan-eng-review`.
2. Add `pub fn plan_last_reviewer_is_valid_for_state(workflow_state: &str, last_reviewed_by: &str) -> bool` or an equivalent shared helper.
3. Add a narrow validation wrapper for parser errors so `parse_plan_source` can report the malformed header through existing `DiagnosticError` paths.
4. Replace independent `validate_plan_last_reviewed_by` use in `parse_plan_source` with the paired validation after both headers are parsed.
5. Replace local workflow candidate pairing logic in `src/workflow/status.rs` with the shared helper.
6. Replace the `plan_document.last_reviewed_by != "plan-eng-review"` readiness check in `src/execution/context.rs` with the shared helper plus the existing exact approved-state check.
7. Add tests for `Engineering Approved` + `writing-plans` analyzer rejection and valid pairings.

**Validation Expectations:**

- `cargo nextest run --test contracts_spec_plan --test workflow_runtime --test execution_query --no-fail-fast`
- Full strict Clippy and full no-fail-fast nextest before task review.

## Task 2: Remove Low-Signal Active Documentation Traps

**Spec Coverage:** REQ-002, REQ-003

**Goal:** Remove active wording that can send agents toward retired hidden command vocabulary and tighten the public-flow test documentation so focused goldens are not overstated.

**Context:** The audit found one active reference that names a retired low-level recovery command in negative guidance. The signal-to-noise auditor also found that `docs/testing.md` can imply `runtime_behavior_golden` is full compiled-CLI proof, even though it is intentionally focused contract coverage with in-process public argv/parser rows.

**Constraints:**

- Do not edit archived audit history or historical plans.
- Do not remove legitimate negative scanner fixtures.
- Keep `runtime_behavior_golden` documented as valuable focused public contract coverage.
- Do not weaken the public-flow gate description.

**Done when:**

- Active review-state reference no longer names the retired hidden recovery command.
- `docs/testing.md` explicitly distinguishes focused public contract goldens from full compiled-CLI transition proof.
- Existing docs/prompt hidden-helper scanners still pass.

**Files:**

- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `docs/testing.md`
- tests only if scanner expectations need an intentional wording update

**Detailed Implementation Steps:**

1. Replace the literal retired recovery command in the active review-state reference with generic low-level recovery-command wording.
2. Update `docs/testing.md` around `runtime_behavior_golden` to state that most rows use the focused public argv/parser contract runner, while env-injection rows still cross the compiled CLI boundary.
3. Keep the public-flow proof distinction: compiled public CLI suites prove shipped transitions; focused goldens pin public JSON/DTO contract shape.
4. Search active docs/prompts for any new occurrence of the retired command outside tests/archives and leave scanner fixtures untouched.

**Validation Expectations:**

- `node --test tests/codex-runtime/*.test.mjs`
- Full strict Clippy and full no-fail-fast nextest before task review.

## Task 3: Compact Route-Owning Skill Law

**Spec Coverage:** REQ-004

**Goal:** Keep route-owning skills actionable by preserving the top-level execution rule while moving detailed route-binding and recovery law to `references/operator-route-authority.md`.

**Context:** The audit found the skills are value-positive where they point to typed public argv/templates, but route-owning skills still repeat detailed recovery and late-stage route law inline. This risks prompt saturation.

**Constraints:**

- Edit `.tmpl` files and regenerate `SKILL.md`; do not hand-edit generated skill docs.
- Top-level skills must still include mandatory route law: query workflow/operator JSON, execute typed argv/template, stop if absent.
- Do not remove skill-specific safety rules such as approved plan checks, repo-safety gates, or reviewer/task-boundary gates.
- Keep prompt budgets enforced.

**Done when:**

- `skills/executing-plans/SKILL.md.tmpl` no longer includes a long inline execution-start recovery runbook that duplicates the canonical route reference.
- `skills/subagent-driven-development/SKILL.md.tmpl` delegates detailed execution-start recovery and typed route binding to the canonical reference while keeping the preflight hard gate.
- `skills/finishing-a-development-branch/SKILL.md.tmpl` uses concise late-stage route law and delegates binding detail to the canonical reference.
- Generated `SKILL.md` files are fresh and within budget.
- Tests still prove mandatory law remains top-level.

**Files:**

- `references/operator-route-authority.md`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/finishing-a-development-branch/SKILL.md.tmpl`
- generated `skills/executing-plans/SKILL.md`
- generated `skills/subagent-driven-development/SKILL.md`
- generated `skills/finishing-a-development-branch/SKILL.md`
- `skills/skill-doc-budgets.json` only if reduced counts require manifest refresh

**Detailed Implementation Steps:**

1. Confirm the canonical route reference already contains the detailed binding/recovery law being removed from skills; add one concise execution-start recovery paragraph there only if a required detail would otherwise disappear.
2. In `executing-plans`, replace the long compact five-step recovery runbook with a short stop/reroute sentence that points to the canonical route reference.
3. In `subagent-driven-development`, make the same compaction for the execution-start hard gate.
4. In `finishing-a-development-branch`, collapse repeated late-stage routing prose to one concise rule: requery operator, execute typed argv/template or selected handoff lane, stop if absent.
5. Regenerate skill docs with `node scripts/gen-skill-docs.mjs`.
6. Run prompt budget and skill-doc contract checks; adjust wording only if tests show mandatory law was moved too far down.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- Full strict Clippy and full no-fail-fast nextest before task review.

## Task 4: Consolidate Public-Flow Scanner Policy

**Spec Coverage:** REQ-005

**Goal:** Reduce public-flow scanner policy sprawl by centralizing classification and exception metadata in one typed support surface.

**Context:** The public-flow scanner protects real regressions, but its gate classification, exception categories, protected-file construction, script parsing, and focused-contract exclusions are becoming a policy surface spread across multiple tests. The goal is not to weaken scanning; it is to make policy ownership explicit and easier to delete or revise.

**Constraints:**

- Do not remove scanner coverage for hidden helpers, hidden commands, display-command execution, direct internal runtime helpers, or token-only follow-up traps.
- Do not replace spread-out hardcoding with another anonymous hardcoded array.
- Keep scanner parser tests only where the shell script boundary is itself the contract.
- Preserve public-flow gate alignment with `scripts/run-public-runtime-flow-tests.sh`.

**Done when:**

- Public-flow gate entries are represented by a named typed manifest struct rather than raw tuple arrays.
- Exception/exclusion helpers consume that typed manifest instead of reconstructing policy from scattered strings.
- Self-tests assert manifest completeness and script alignment through the manifest API.
- Existing scanner tests still catch injected hidden-helper and display-command violations.

**Files:**

- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/public_cli_flow_contracts.rs` only if assertion names need updating

**Detailed Implementation Steps:**

1. Introduce a typed `PublicRuntimeFlowGateEntry` struct with `binary`, `category`, and a short `reason` or `proof_scope`.
2. Replace `PUBLIC_RUNTIME_FLOW_GATE_TESTS` tuple usage with `PUBLIC_RUNTIME_FLOW_GATE_ENTRIES`.
3. Update helper functions to derive protected test files, required binaries, and categories from the manifest entries.
4. Add a manifest self-test that every entry has non-empty proof scope and a supported category.
5. Keep the shell parser self-test if it still protects the script boundary, but do not add more grammar tests.
6. Verify injected scanner violations still fail through existing tests.

**Validation Expectations:**

- `cargo nextest run --test public_flow_scan_contracts --test public_cli_flow_contracts --no-fail-fast`
- Full strict Clippy and full no-fail-fast nextest before task review.

## Task 5: Extend Large-Module Boundary Guard To Command Submodules

**Spec Coverage:** REQ-006

**Goal:** Make oversized command submodules visible to the modularization guard without forcing a risky mechanical split.

**Context:** The audit found `src/execution/commands/advance_late_stage.rs` is above 2000 lines, but the large-module guard only scans direct `src/execution/*.rs` files. This means modularization could hide a new command monolith under a subdirectory.

**Constraints:**

- Do not split `advance_late_stage` unless a cohesive family is obvious and low-risk.
- Do not scan test-only Rust files such as `unit_tests.rs`.
- Keep large route-plan unit tests out of production size debt.
- Document why `advance_late_stage` is a scheduled follow-up or documented exception.
- Prefer recursive production-source discovery with narrow filters over another one-off file check.

**Done when:**

- The large-module guard scans production Rust files under relevant execution subtrees, including `src/execution/commands`.
- `src/execution/commands/advance_late_stage.rs` is either documented as an exception/follow-up or extracted into smaller cohesive files.
- Boundary docs include the command submodule in the large-module threshold section.
- The guard failure message no longer says only top-level `src/execution/*.rs` modules are covered.

**Files:**

- `tests/runtime_module_boundaries.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `src/execution/commands/advance_late_stage.rs` only if extraction is chosen

**Detailed Implementation Steps:**

1. Replace the direct `fs::read_dir(src/execution)` scan with recursive production Rust source discovery under `src/execution`.
2. Filter out test modules and unit-test directories so the guard targets production module debt.
3. Add `src/execution/commands/advance_late_stage.rs` to the documented large-module boundary list with a clear status.
4. Update the boundary reference heading from "Top-level" to "Production execution Rust files" and add a section for `advance_late_stage`.
5. If inspection reveals a cohesive extraction that lowers risk and simplifies the command, do that extraction; otherwise keep this task to visibility and documented ownership.
6. Run module-boundary tests to ensure the recursive guard catches the intended file and no accidental test-only files.

**Validation Expectations:**

- `cargo nextest run --test runtime_module_boundaries --no-fail-fast`
- Full strict Clippy and full no-fail-fast nextest before task review.

## Final Audit Loop

After Task 5 has passed full validation and clean-context review, run another deep audit loop using the original A-H auditor set plus the additional signal-to-noise auditor. Start that audit loop by confirming no cargo/nextest/clippy/test process is running, then run `cargo clean`. If that audit finds actionable issues, create and implement the next plan in the same task-order, full-validation, clean-review loop.
