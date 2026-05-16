# Runtime Safety Thirty-Sixth Audit Remediation

## Workflow State

Engineering remediation plan for the thirty-sixth runtime-safety audit loop. This plan is active until every task below is implemented, fully verified, independently reviewed, and followed by another deep audit loop with the signal-to-noise auditor included.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow/project skills. Do not allow reviewers or implementation subagents to spawn additional subagents. Before each full test cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, or `cargo clippy` process is already running.

## Goal

Remove the actionable findings from the thirty-sixth runtime-safety audit loop:

- Public-flow proof must replay FS-22 through shipped public commands, not only internal compatibility helpers.
- Public-flow gate classification must not label mixed internal/API tests as pure executable public-flow proof.
- Active prompts and schemas must not point agents at retired or lower-authority command surfaces.
- Prompt-surface tests must reduce duplicated route-owner truth and route-specific command folklore.
- Execution route planning must not depend on workflow presentation modules for shared route DTOs or route classification.

## Architecture

The remediation preserves the current runtime architecture:

```text
CLI args
  -> public command module
  -> transition guard
  -> event append
  -> reducer
  -> route_plan decision
  -> router/read-model projection
  -> workflow operator presentation
```

The work intentionally does not reintroduce removed helper commands, hidden compatibility paths, receipt/provenance authority, or manual markdown repair. It keeps `workflow operator --json` as the normal executable route authority and keeps `plan execution status` as diagnostic/read-model output.

## Change Surface

- `tests/public_replay_churn.rs`
- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `tests/packet_and_schema.rs`
- `tests/runtime_module_boundaries.rs`
- `skills/plan-fidelity-review/SKILL.md.tmpl`
- `skills/document-release/SKILL.md.tmpl`
- generated `skills/*/SKILL.md`
- `scripts/gen-skill-docs.mjs`
- `src/contracts/mod.rs`
- new shared workflow route DTO module under `src/contracts/`
- `src/workflow/status.rs`
- `src/execution/router.rs`
- `src/execution/route_plan/decision_support.rs`
- execution tests importing `WorkflowRoute`
- `src/execution/status.rs`
- generated/checked-in schema JSON under `schemas/`

## Preconditions

- Work from the current updated codebase after the thirty-fifth remediation implementation.
- Do not use FeatureForge runtime skills or project skills.
- Before every full verification cycle, confirm no existing `cargo`, `cargo nextest`, `cargo test`, `cargo clippy`, or Codex-runtime Node validation process is running.
- If a full nextest run exceeds 4-5 minutes, run `cargo clean`, rerun once, and remediate if the regression repeats. If it exceeds 10 minutes, stop immediately and follow the same clean/rerun/remediate rule.

## Known Footguns / Constraints

- Do not replace public CLI replay with internal helpers. Synthetic fixture setup is allowed only when the assertion path runs through the compiled public CLI.
- Do not make `plan execution status` the normal route authority to avoid conflicting with `workflow operator --json`.
- Do not move mandatory route law out of top-level route-owning skills.
- When skill docs change, edit `.tmpl` files and regenerate checked-in `SKILL.md` files.
- Do not add another manual route-owner list unless it replaces an older duplicate.
- Do not loosen prompt budgets or clippy settings.
- Keep public JSON fields stable unless the schema/test update explicitly documents the authority correction.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| REQ-001 Public FS-22 replay proof | Task 1 |
| REQ-002 Public-flow classifications reflect test realism | Task 1 |
| REQ-003 Active prompts avoid retired `workflow status` command traps | Task 2 |
| REQ-004 Status schema and plaintext blockers cannot outrank operator authority | Task 2 |
| REQ-005 Prompt route-owner truth is single-source and high-signal | Task 3 |
| REQ-006 Route-specific binding folklore is centralized in the canonical route reference | Task 3 |
| REQ-007 Execution route planning does not import workflow presentation DTOs | Task 4 |
| REQ-008 Boundary tests prevent the split-decisioning seam from returning | Task 4 |

## Task 1: Public-Flow Realism and FS-22 Replay

**Spec Coverage:** REQ-001, REQ-002

**Goal:** Prove the FS-22 historical stuck path through shipped public commands and classify public-flow test binaries honestly.

**Context:**

- The audit found FS-22 covered only by `tests/internal_plan_execution.rs::internal_only_compatibility_fs22_repair_review_state_does_not_clear_dispatch_lineage_when_close_current_task_bridge_exists`.
- `tests/plan_execution.rs` contains legitimate internal API assertions but is classified as `ExecutablePublicFlowProof`.
- Public replay coverage may use synthetic historical fixture setup, but every asserted transition must run through the compiled public CLI.

**Constraints:**

- Do not move internal helper tests into public proof by renaming them.
- Do not remove `plan_execution` from the public gate; reclassify it as mixed if it remains in the public-flow validation script.
- Do not weaken hidden-helper scanners.

**Done when:**

- `tests/public_replay_churn.rs` includes a public FS-22 replay that runs `workflow operator` and `plan execution repair-review-state` through the public CLI, verifies `task_closure_recording_ready`, verifies no execution-reentry follow-up, and proves dispatch lineage is preserved.
- `plan_execution` is classified as `MixedPublicAndInternalSemantic`.
- Public-flow scanner contracts expect `plan_execution` in the mixed set.
- The public-flow gate still runs `plan_execution` but no longer claims it is pure executable public-flow proof.

**Files:**

- `tests/public_replay_churn.rs`
- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `scripts/run-public-runtime-flow-tests.sh`

**Implementation Steps:**

1. Add a public FS-22 test next to the existing FS-17 public replay.
2. Reuse existing public fixture helpers where possible; if fixture mutation is needed, keep it clearly marked synthetic historical setup and sync the event log.
3. Capture dispatch lineage before public repair and assert the same lineage remains after repair.
4. Assert `actions_performed` does not contain destructive lineage-clear actions.
5. Reclassify `plan_execution` from `ExecutablePublicFlowProof` to `MixedPublicAndInternalSemantic`.
6. Update scanner contract expectations and script comments if needed.

**Validation Expectations:**

- `cargo nextest run --test public_replay_churn --no-fail-fast`
- `cargo nextest run --test public_flow_scan_contracts --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 2: Public Output and Prompt Authority Cleanup

**Spec Coverage:** REQ-003, REQ-004

**Goal:** Remove command wording that can make agents treat retired or diagnostic surfaces as normal executable route authority.

**Context:**

- `skills/plan-fidelity-review/SKILL.md.tmpl` still mentions `$_FEATUREFORGE_BIN workflow status --json`.
- `src/execution/status.rs` describes status `recommended_public_command_argv` as executable without saying it is diagnostic/operator-derived.
- Plaintext operator blocker output uses `next_display_summary=...`, which can look like a runnable command-shaped next step.

**Constraints:**

- Keep `workflow operator --json` as the normal executable route authority.
- Do not remove typed argv fields from public JSON.
- Do not break top-level operator/handoff schema wording that correctly identifies operator-owned argv as executable.

**Done when:**

- Plan-fidelity prompts obtain required artifact templates from `plan contract analyze-plan --format json`, not `workflow status`.
- Prompt contract scanners reject `$_FEATUREFORGE_BIN workflow status` and equivalent root-bound retired command examples.
- Plan-execution status schema describes status `recommended_public_command_argv` as an operator-derived diagnostic mirror, not primary executable authority.
- Plaintext blocker summaries are renamed or annotated as display-only.

**Files:**

- `skills/plan-fidelity-review/SKILL.md.tmpl`
- `skills/plan-fidelity-review/SKILL.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `src/execution/status.rs`
- `src/workflow/status.rs`
- `src/workflow/operator.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-operator.schema.json`
- `schemas/workflow-handoff.schema.json`
- `tests/packet_and_schema.rs`

**Implementation Steps:**

1. Replace the active plan-fidelity template guidance with `plan contract analyze-plan --format json` only.
2. Regenerate skill docs.
3. Expand the retired-command trap regex to catch root-bound `workflow status` commands.
4. Split schema descriptions so operator/handoff top-level argv remains executable while embedded/status `PlanExecutionStatus` argv is described as diagnostic/operator-derived.
5. Update checked-in schemas and schema contract tests.
6. Rename plaintext blocker field labels from `next_display_summary` to `display_only_next_summary`, or otherwise mark them unambiguously non-executable, and update tests.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- `cargo nextest run --test packet_and_schema --no-fail-fast`
- `cargo nextest run --test workflow_shell_smoke --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 3: Prompt Signal-to-Noise and Duplicate Truth Cleanup

**Spec Coverage:** REQ-005, REQ-006

**Goal:** Keep skill guidance actionable by removing duplicated route-owner lists and route-specific command folklore from top-level skills/tests.

**Context:**

- The route-owning skill set is duplicated in `scripts/gen-skill-docs.mjs` and `tests/fixtures/route-owning-generated-skills.txt`.
- `document-release` says not to inline route-specific binding details and then mentions `advance-late-stage --result ready|blocked`.
- The budget test duplicates the high-volume budgeted skill set instead of deriving from the budget manifest.

**Constraints:**

- Do not loosen the prompt budget.
- Keep mandatory route law top-level for route-owning skills.
- Keep canonical route-specific binding law in `references/operator-route-authority.md`.

**Done when:**

- Route-owner tests use one canonical source of truth instead of a hand-maintained fixture duplicate.
- `tests/fixtures/route-owning-generated-skills.txt` is removed if no longer needed.
- `document-release` points to operator JSON plus the canonical reference and does not inline `advance-late-stage --result ready|blocked`.
- Budget tests derive the budgeted skill set from `skills/skill-doc-budgets.json` and assert manifest shape without a second hardcoded list.

**Files:**

- `scripts/gen-skill-docs.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `tests/fixtures/route-owning-generated-skills.txt`
- `skills/document-release/SKILL.md.tmpl`
- `skills/document-release/SKILL.md`
- `skills/skill-doc-budgets.json`

**Implementation Steps:**

1. Remove route-owner fixture consumption and compare rendered route-law modes directly against `ROUTE_OWNING_GENERATED_SKILLS`.
2. Delete the duplicate fixture if no remaining test consumes it.
3. Remove the route-specific `advance-late-stage --result ready|blocked` line from `document-release`.
4. Update prompt contract tests to assert centralized reference usage and reject that route-specific command mapping in generated skills.
5. Refactor the prompt budget test to derive budgeted skills from the manifest.
6. Regenerate skill docs.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/skill-doc-budget.test.mjs`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- Full strict clippy and full nextest before task review.

## Task 4: Workflow Route DTO Boundary and Split-Decisioning Guard

**Spec Coverage:** REQ-007, REQ-008

**Goal:** Remove execution route planning’s dependency on workflow presentation modules and centralize non-runtime workflow route classification behind a shared contract type.

**Context:**

- `src/execution/router.rs` and `src/execution/route_plan/decision_support.rs` import `crate::workflow::status::WorkflowRoute`.
- `decision_support.rs` detects engineering-approval fidelity blockage by locally inspecting `status`, `next_skill`, and reason-code strings.
- Boundary tests do not currently forbid this import direction.

**Constraints:**

- Do not change public workflow JSON shape unless required by the shared DTO move.
- Keep schema generation stable.
- Do not create a second local route classification table.

**Done when:**

- Shared workflow route DTOs live in a contract/module layer that both workflow presentation and execution routing can import.
- Execution production route-plan/router code no longer imports `crate::workflow::status`.
- Engineering-approval fidelity blockage classification is a shared helper/method on the route DTO or adjacent contract module.
- Boundary tests fail if execution route planning imports workflow presentation status again.

**Files:**

- `src/contracts/mod.rs`
- new `src/contracts/workflow.rs`
- `src/workflow/status.rs`
- `src/execution/router.rs`
- `src/execution/route_plan/decision_support.rs`
- `src/execution/query.rs`
- execution tests importing `WorkflowRoute`
- `tests/runtime_module_boundaries.rs`
- workflow schema files if generated output changes

**Implementation Steps:**

1. Move `WorkflowRoute`, `WorkflowDiagnostic`, and `WorkflowPhase` into a shared contract module.
2. Re-export them from `workflow::status` only if needed for compatibility.
3. Add a shared route classification helper for engineering-approval fidelity blockage.
4. Update execution production imports to the shared contract module.
5. Update tests/imports as needed.
6. Add boundary assertions preventing `src/execution/router.rs` and `src/execution/route_plan/**` production modules from importing `crate::workflow::status`.
7. Regenerate or update schema files only if the schema output changes.

**Validation Expectations:**

- `cargo nextest run --test runtime_module_boundaries --no-fail-fast`
- `cargo nextest run --test workflow_runtime --no-fail-fast`
- `cargo nextest run --test execution_query --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Full Validation Loop

After each task:

1. Check no cargo/nextest process is already running.
2. Run strict clippy:

   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```

3. Run full nextest with no fail-fast:

   ```bash
   cargo nextest run --all-targets --all-features --no-fail-fast
   ```

4. Run Node/doc validation when skill or prompt surfaces changed:

   ```bash
   node scripts/gen-skill-docs.mjs --check
   node scripts/gen-agent-docs.mjs --check
   node --test tests/codex-runtime/*.test.mjs
   ```

5. Dispatch a clean-context review subagent for the exact task. The reviewer must not spawn subagents and must review only the completed task against this plan.
6. Remediate any real findings, revalidate, and rereview until the task review has no actionable findings.

After Task 4:

1. Run full validation again.
2. Dispatch a clean-context whole-plan review.
3. Remediate/revalidate/rereview until no actionable whole-plan findings remain.
4. Run a fresh audit loop, including the signal-to-noise subagent.
