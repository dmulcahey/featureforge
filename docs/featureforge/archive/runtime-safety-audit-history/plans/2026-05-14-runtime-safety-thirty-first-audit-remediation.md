# Runtime Safety Thirty-First Audit Remediation

## Workflow State

Engineering remediation plan for the current runtime-safety audit loop.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task.

## Goal

Close the remaining actionable audit issues without adding another layer of prompt or test churn. The priority is to centralize runtime decisioning, align active docs and runtime contracts, and protect packaged prompt references with focused tests.

## Architecture

- Plan-fidelity provenance is a runtime contract. The runtime and template output must agree with active prompts: final plan-fidelity artifacts come from an independent fresh-context subagent.
- Source-archive verification is the packaging authority for prompt companion files. Any companion file linked from generated skills or install docs must be required by that verifier.
- Stale-target selection belongs in `src/execution/stale_target_selection.rs`. Query/read-model assembly may consume the selector but must not own a second stale-target ordering.
- Execution route authority must consume reducer-derived route repair candidates instead of relying only on projected status fields.
- Late-stage precedence is semantic runtime decisioning. Execution-owned assembly must not import workflow presentation modules for it.
- Prompt cleanup should remove low-signal hidden-helper vocabulary and avoid adding new repeated route-law prose.

## Change Surface

- `src/contracts/plan.rs`
- `src/execution/stale_target_selection.rs`
- `src/execution/query.rs`
- `src/execution/status_support.rs`
- `src/execution/route_plan/execution_target_authority.rs`
- `src/execution/route_plan/execution_targets.rs`
- `src/execution/status_assembly/late_stage.rs`
- late-stage precedence module ownership
- `scripts/verify-source-archive.mjs`
- generated skill templates/docs and install docs as needed
- contract and boundary tests in `tests/**` and `tests/codex-runtime/**`

## Preconditions

- Do not use FeatureForge runtime/workflow commands or FeatureForge/project skills.
- Before any full test cycle, verify no `cargo nextest`, `cargo test`, or `cargo clippy` process is already running.
- Run strict clippy and full no-fail-fast nextest before dispatching each clean-context review.
- If full nextest exceeds 4-5 minutes, run `cargo clean` and rerun; if the regression repeats or any run exceeds 10 minutes, stop and address performance.

## Known Footguns / Constraints

- Do not preserve `cross-model` as plan-fidelity provenance unless active docs are changed to bless it. The chosen remediation is stricter runtime alignment: plan-fidelity accepts only `fresh-context-subagent`.
- Do not solve split decisioning by adding another static scanner around duplicate logic. Prefer moving code to the shared owner and making callers consume it.
- Do not move mandatory route law solely into companion docs. Keep generated skill law compact and actionable.
- Do not weaken public/private runtime helper quarantine.
- Generated skill docs must be regenerated from templates.

## Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| Plan-fidelity runtime and active docs agree on reviewer provenance | Task 1 |
| Companion references linked from generated prompts are packaged | Task 2 |
| Hidden-helper vocabulary in active prompt-adjacent surfaces is reduced | Task 2 |
| Stale-target ordering has one owner | Task 3 |
| Route target authority consumes reducer-derived candidates | Task 4 |
| Execution status assembly does not import workflow presentation semantics | Task 5 |
| Boundary tests protect centralized ownership without pinning incidental prose | Tasks 3-5 |

## Tasks

### Task 1: Align Plan-Fidelity Reviewer Provenance

**Spec Coverage:** Plan review, public/private mismatch, semantic traps.

**Goal:** Make runtime plan-fidelity provenance match active prompt guidance: plan-fidelity pass artifacts require `Reviewer Source: fresh-context-subagent`.

**Context:** Audit found `PLAN_FIDELITY_REVIEWER_SOURCE_OPTIONS` still permits `cross-model` while `plan-fidelity-review` prompts require a fresh-context subagent.

**Constraints:** Do not change final implementation review provenance options; this task is scoped to plan-fidelity review.

**Done when:**

- Plan-fidelity reviewer source options expose only `fresh-context-subagent`.
- Analyze-plan required artifact templates list only that source.
- Tests reject `cross-model` plan-fidelity artifacts as invalid.
- Active docs/tests still allow `cross-model` only where non-plan-fidelity review flows intentionally support it.

**Files:**

- `src/contracts/plan.rs`
- `tests/contracts_spec_plan.rs`
- any schema/golden tests affected by the template output

**Implementation Steps:**

1. Change `PLAN_FIDELITY_REVIEWER_SOURCE_OPTIONS` to a single-item array.
2. Update expected analyze-plan template output.
3. Add or retarget an invalid-provenance test so `Reviewer Source: cross-model` fails plan-fidelity validation.
4. Run targeted contract tests for plan-fidelity.

**Validation Expectations:**

- `cargo test --test contracts_spec_plan plan_fidelity`
- `cargo test --test workflow_runtime plan_fidelity`
- Full verification before review.

### Task 2: Protect Companion Packaging and Reduce Hidden-Helper Vocabulary

**Spec Coverage:** Prompt surface, packaging, public-output UX.

**Goal:** Ensure prompt-linked companion references are required by source archive verification, and remove low-signal hidden-helper vocabulary from active prompt-adjacent surfaces.

**Context:** The source archive verifier only requires `references/reviewer-recursion-rule.md`; generated prompts and docs also depend on operator route authority, search-before-building, execution/review examples, and plan review rubrics. Audit also found active docs saying “hidden compatibility/debug commands” and “hidden or low-level mutation commands.”

**Constraints:** Keep prohibitive command-boundary law without naming hidden-helper categories in high-use prompts.

**Done when:**

- `scripts/verify-source-archive.mjs` requires all active companion references linked by generated skills/docs.
- Tests cover the verifier list or the verifier itself for those companions.
- Active prompt-adjacent docs avoid generic “hidden” vocabulary where public-command-only wording is enough.
- Generated docs are fresh.

**Files:**

- `scripts/verify-source-archive.mjs`
- `docs/README.codex.md`
- `docs/README.copilot.md`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/executing-plans/SKILL.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs` or verifier tests

**Implementation Steps:**

1. Add required companion files to `REQUIRED_SOURCE_ARCHIVE_PATHS`.
2. Add a Node contract assertion that every prompt-linked companion reference is covered by source-archive verification.
3. Replace generic hidden-helper wording with “unselected/non-public/low-level runtime mutation commands” or public-argv-only language.
4. Regenerate skill docs.
5. Run source archive and Node contract checks.

**Validation Expectations:**

- `node scripts/verify-source-archive.mjs`
- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`

### Task 3: Centralize Stale-Target Selection

**Spec Coverage:** Modularization, split decisioning, stale closure/reentry loops.

**Goal:** Move projected earliest stale task candidate selection under `stale_target_selection` and remove the status-support dependency on query logic.

**Context:** Audit found `status_support` imports `query::projected_earliest_stale_task_candidate_from_status`, while `query` computes stale candidate ordering separately from the central stale selector.

**Constraints:** Preserve current ordering semantics and public route behavior.

**Done when:**

- `projected_earliest_stale_task_candidate_from_status` or its replacement lives in `src/execution/stale_target_selection.rs`.
- `src/execution/query.rs` and `src/execution/status_support.rs` consume the shared selector.
- `status_support` no longer imports `execution::query`.
- Boundary tests fail if lower support modules import `query` for stale-target selection.

**Files:**

- `src/execution/stale_target_selection.rs`
- `src/execution/query.rs`
- `src/execution/status_support.rs`
- `tests/runtime_module_boundaries.rs`
- stale/replay tests if output changes

**Implementation Steps:**

1. Move the function into `stale_target_selection`.
2. Import the shared function in `query` and `status_support`.
3. Add a boundary assertion against `status_support` importing `execution::query`.
4. Run targeted stale/reentry tests.

**Validation Expectations:**

- `cargo test --test runtime_module_boundaries stale`
- `cargo test --test liveness_model_checker`
- affected workflow/runtime replay tests.

### Task 4: Consume Route Repair Candidates in Execution Route Authority

**Spec Coverage:** Route authority, command eligibility, split decisioning.

**Goal:** Make `execution_command_route_target_has_authority` use reducer-derived route repair target candidates instead of accepting a dead parameter.

**Context:** Audit found the authority function receives `route_repair_target_candidates` but ignores them, while callers pass reducer-derived candidates.

**Constraints:** Do not let resume-task diagnostics alone authorize begin. Begin authority must remain fingerprint-bound or exact route-bound.

**Done when:**

- The authority function uses route repair candidates through a shared matching helper.
- Tests prove a candidate can authorize a matching execution route when projected status has not yet published the candidate.
- Tests prove reopen candidates still do not authorize begin.
- Boundary tests fail if the candidate parameter is ignored.

**Files:**

- `src/execution/route_plan/execution_target_authority.rs`
- `src/execution/route_plan/execution_targets.rs`
- `src/execution/route_plan/unit_tests.rs`
- `tests/runtime_module_boundaries.rs`

**Implementation Steps:**

1. Extract public repair target matching into a helper that can match status-published targets and route candidates.
2. Use the helper in `execution_command_route_target_has_authority`.
3. Preserve status blocking checks before candidate authority.
4. Add focused route-plan unit tests.
5. Update boundary tests to require route candidate consumption.

**Validation Expectations:**

- `cargo test --lib route_plan`
- `cargo test --test runtime_module_boundaries execution_command_route_target`
- targeted liveness tests for resume/repair loops.

### Task 5: Move Late-Stage Precedence to Execution Ownership and Clean Low-Signal Surface

**Spec Coverage:** Modularization, prompt surface, signal-to-noise.

**Goal:** Move semantic late-stage precedence out of workflow presentation ownership, update docs/tests, and keep prompt cleanup compact.

**Context:** Execution status assembly imports `crate::workflow::late_stage_precedence`, conflicting with runtime boundary docs. Signal-to-noise audit also found duplicated route-law surfaces and line-count budget risks; this task should avoid expanding prompt prose.

**Constraints:** Do not rewrite prompt budget infrastructure in this pass unless required by tests. Do not move mandatory law out of top-level route-owning skills.

**Done when:**

- Late-stage precedence lives under `src/execution`.
- Workflow presentation imports execution-owned precedence, not the other way around.
- Boundary docs/tests name execution as the owner.
- No new fixed late-stage skill chain or route-law duplication is introduced.

**Files:**

- `src/execution/late_stage_precedence.rs` or equivalent
- `src/execution/mod.rs` / `src/lib.rs` module declarations
- `src/execution/status_assembly/late_stage.rs`
- `src/workflow/**` imports
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `review/late-stage-precedence-reference.md`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_instruction_contracts.rs`

**Implementation Steps:**

1. Move or rehome the late-stage precedence module under execution.
2. Update execution and workflow imports.
3. Update module-boundary docs and tests.
4. Run targeted runtime/module and instruction tests.
5. Keep signal/noise cleanup to compact wording changes that remove duplication without adding new scanners unless needed.

**Validation Expectations:**

- `cargo test --test runtime_module_boundaries late_stage`
- `cargo test --test runtime_instruction_contracts late_stage`
- full verification before review.
