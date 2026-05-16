# Runtime Safety Re-audit Follow-up Remediation Plan - 2026-05-07

## Workflow State

Draft - execution was requested explicitly in the controlling chat thread.

## Plan Revision

Revision: 1

Source audit: `docs/featureforge/reference/2026-05-07-deep-runtime-safety-reaudit.md`

## Execution Mode

Sequential implementation in task order.

Each task must finish with strict verification for the touched surface before moving forward. For Rust changes, run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

For final verification, run:

```bash
cargo nextest run --all-targets --all-features --no-fail-fast
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
```

Do not run FeatureForge runtime/project skills while implementing this plan.

## Goal

Eliminate the remaining public-output dead ends, prompt/doc traps, test realism gaps, and runtime split-decisioning issues found in the 2026-05-07 deep runtime safety re-audit.

The target end state is:

- public diagnostics never teach evidence rebuild, packet rebuild, hidden helper, manual receipt, or artifact-repair workflows;
- input-required public command templates can be completed into executable public argv without rerunning route discovery;
- display strings are never treated as executable command authority;
- operator, status, doctor, JSON failures, docs, and skills point agents to one public next step;
- install-root references in generated skills resolve from installed skill contexts;
- prompt budgets cover high-use generated skills;
- event-log persistence does not call routing;
- read-model, router, and repair mutators derive reentry/repair decisions from shared authority objects;
- public-flow and internal tests no longer preserve old display-command execution assumptions;
- current task closure remains authoritative and post-close cleanup failures cannot produce unexplained later blockers.

## Architecture

Preserve the intended runtime flow:

```text
CLI args
  -> command module
  -> transition guard
  -> event append
  -> reducer
  -> read model
  -> route decision
  -> workflow operator/status/doctor presentation
```

Do not create a second routing authority in docs, tests, prompt text, JSON failure messages, or mutation modules.

The shared implementation model should be:

- event log owns persistence, append-only transition loading, replay, and migration of raw events;
- reducer owns authoritative runtime state derivation;
- router owns public route decisions;
- read model projects route decisions but does not independently classify them;
- command eligibility owns typed public command construction;
- presentation layers render one public next step from typed route output;
- tests that assert public behavior use typed argv/template output or the compiled public CLI;
- internal compatibility tests must be named and documented as internal-only.

## Change Surface

Expected files and areas:

- `src/execution/state/rebuild_evidence.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/public_command_types.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/router.rs`
- `src/execution/read_model.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/review_state.rs`
- `src/execution/event_log.rs`
- `src/execution/commands/close_current_task.rs`
- `src/execution/authority.rs`
- `src/workflow/operator.rs`
- `src/workflow/doctor_dashboard.rs`
- `src/workflow/status.rs`
- `schemas/*.json`
- `skills/*.md.tmpl`
- generated `skills/*/SKILL.md`
- `skills/skill-doc-budgets.json`
- `scripts/gen-skill-docs.mjs`
- `tests/runtime_instruction_contracts.rs`
- `tests/runtime_instruction_review_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/internal_contracts_execution_runtime_boundaries.rs`
- `tests/codex-runtime/*.test.mjs`

## Preconditions

- Start from the updated codebase audited on 2026-05-07.
- Confirm the branch passes the current baseline validation before implementing broad structural changes if there have been additional intervening edits.
- Preserve existing public command names unless a task explicitly requires a schema-compatible public route addition.
- Preserve event-log authority and append-only transition semantics.
- Preserve reviewer recursion prevention as prompt text only.

## Known Footguns / Constraints

- Do not reintroduce `plan execution preflight`, `record-review-dispatch`, `gate-review`, `gate-finish`, `rebuild-evidence`, or low-level late-stage recorders into normal public guidance.
- Do not make `recommended_command` executable authority.
- Do not teach agents to split command display strings.
- Do not make docs or skills a parallel routing system.
- Do not move mandatory runtime/review law solely into companion references.
- Do not fix prompt budget failures by removing mandatory law from top-level skills.
- Do not add new `#[allow(clippy::...)]` suppressions without explicit approval.
- Do not make event-log migration easier by calling router from persistence code.
- Do not let repair mutators and router independently compute target selection or follow-up route semantics.
- Do not silently swallow cleanup errors that can produce later route blockers.
- When generated skill docs change, edit templates and regenerate checked-in outputs.

## Requirement Coverage Matrix

| Requirement | Description | Covered By |
| --- | --- | --- |
| REQ-001 | Public diagnostics never direct agents to rebuild evidence, rebuild packets, record receipts, or manually repair artifacts. | Task 1 |
| REQ-002 | Input-required public command templates can be completed into executable argv. | Task 2 |
| REQ-003 | Failure text and public text surfaces do not make display strings practical command authority. | Task 3 |
| REQ-004 | Doctor/operator guidance names one public next step and avoids multi-action repair prose. | Task 3 |
| REQ-005 | Generated skill/doc references resolve in installed contexts and high-use skill budgets are enforced. | Task 4 |
| REQ-006 | Event-log persistence does not import or call router. | Task 5 |
| REQ-007 | Read-model, router, and repair mutators share reentry/repair decision objects. | Task 6 |
| REQ-008 | Public-flow and internal tests do not preserve display-command execution assumptions. | Task 7 |
| REQ-009 | Synthetic historical replay tests are clearly scoped as synthetic setup plus public recovery. | Task 7 |
| REQ-010 | `close-current-task` does not hide worktree-lease cleanup failures. | Task 8 |
| REQ-011 | Full suite and prompt/doc contracts pass after all changes. | Task 9 |

## Task 1 - Remove Evidence-Rebuild Guidance From Public Diagnostics

### Spec Coverage

REQ-001.

### Goal

Public review-gate and evidence validation diagnostics must never instruct agents to rebuild packets, rebuild evidence, rerun hidden evidence repair, record receipts, or manually edit runtime artifacts.

### Context

The re-audit found public remediation strings in `src/execution/state/rebuild_evidence.rs:400` and `src/execution/state/rebuild_evidence.rs:433` that still say to rebuild packets/evidence. Review gate consumes this path through `src/execution/state/review_gate.rs:147`.

### Constraints

- Keep the underlying evidence validation semantics intact.
- Do not remove diagnostics that help identify stale or mismatched artifacts.
- Do not introduce hidden helper names as replacements.
- The remediation must point to the public route owner and typed public route output.

### Done When

- No active public diagnostic contains "rebuild evidence", "rebuild its evidence", "rebuild the packet", "rebuild packet", "record receipt", or hidden helper command names.
- Evidence mismatch diagnostics identify the stale/mismatched artifact and route the agent to operator/status typed public next action.
- Tests fail if old wording returns in public diagnostics.

### Files

- `src/execution/state/rebuild_evidence.rs`
- `src/execution/state/review_gate.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/public_cli_flow_contracts.rs`

### Implementation Steps

1. Inventory every public remediation string emitted by `rebuild_evidence.rs` and `review_gate.rs`.
2. Classify each string as internal validation detail or public remediation.
3. Replace public remediation text with language such as:
   - "The review packet does not match current runtime truth. Use workflow operator/status JSON and follow `recommended_public_command_argv` or the input template for the routed public next step."
   - "The artifact is stale relative to runtime state; do not repair it manually."
4. Keep machine-readable reason codes stable unless changing them is required to remove stale concepts.
5. Add Rust static tests that scan active public diagnostics for banned evidence-rebuild and receipt-repair phrases.
6. Update existing tests that asserted old wording.
7. Confirm `blocked_runtime_bug` remains diagnostic-only and does not gain a normal command.

### Validation Expectations

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test workflow_runtime --no-fail-fast
cargo nextest run --test workflow_shell_smoke --no-fail-fast
cargo nextest run --test runtime_authority_contracts --no-fail-fast
```

## Task 2 - Make Input-Required Public Templates Executable After Binding

### Spec Coverage

REQ-002.

### Goal

An input-required route must produce enough structured information for an agent or tool to bind required values and execute the completed public command without rerunning operator/status as a substitute for mutation.

### Context

`src/execution/public_command_types.rs:8` currently exposes `command_kind`, `base_argv`, and `required_input_names`. Docs and skills tell agents to provide missing inputs and rerun operator/status, which can cause the same input-required route to repeat.

### Constraints

- Keep `base_argv` non-executable until required inputs are bound.
- Do not encode command construction in docs or skills.
- Do not make display strings executable authority.
- Preserve backward-compatible schema fields if consumers already use them.
- Add new fields rather than changing semantics in place when necessary.

### Done When

- Public route templates include structured input metadata sufficient to produce final argv.
- A shared Rust helper materializes completed argv from template plus validated input values.
- Docs and generated skills instruct agents to bind inputs into the template and execute the completed public command.
- No active guidance tells agents to rerun operator/status as the way to perform the input-required mutation.
- Tests cover at least `review_result`, `summary_file`, `claim`, and verification input cases.

### Files

- `src/execution/public_command_types.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/router.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-operator.schema.json`
- `schemas/workflow-status.schema.json`
- `schemas/workflow-handoff.schema.json`
- `docs/runtime-architecture.md`
- `skills/*.md.tmpl`
- generated `skills/*/SKILL.md`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

### Implementation Steps

1. Define a structured input metadata type, for example:
   - public input name;
   - CLI flag or positional binding rule;
   - value kind such as enum, file path, free text, boolean, or repeatable list;
   - whether the value must be shell-escaped by the caller;
   - optional allowed values.
2. Extend public command template projection with the metadata while preserving current `required_input_names`.
3. Implement one shared materialization helper that accepts a template and input map and returns final argv.
4. Use existing command eligibility typed command definitions as the source of truth for CLI flag names and allowed values.
5. Update schemas to describe the new metadata and completed-argv expectations.
6. Update docs and skill templates:
   - say `base_argv` is a typed template, not executable;
   - say to bind required values using the template metadata;
   - say to execute the completed public argv;
   - say to rerun operator/status only after mutation completes or if no public route is present.
7. Regenerate generated skills.
8. Add tests that materialize completed argv for each late-stage and task-closure input-required route.
9. Add prompt/doc tests rejecting "satisfy inputs and rerun operator/status" wording for mutation execution.

### Validation Expectations

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test workflow_runtime --no-fail-fast
cargo nextest run --test workflow_shell_smoke --no-fail-fast
cargo nextest run --test workflow_entry_shell_smoke --no-fail-fast
node scripts/gen-skill-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
```

## Task 3 - Remove Display-Command Authority And Multi-Action Public Prose

### Spec Coverage

REQ-003 and REQ-004.

### Goal

Public failure messages, operator output, status output, and doctor output must not make display strings practical executable authority and must not present compound/multi-action repair instructions as the next step.

### Context

`src/execution/command_eligibility.rs:1564` derives display command text, and `src/execution/command_eligibility.rs:1620` embeds it in `JsonFailure` messages as "Next public action." Operator and doctor prose contain "record or refresh" and "dispatch or record" language at `src/workflow/operator.rs:1782`, `src/workflow/doctor_dashboard.rs:231`, and `src/workflow/doctor_dashboard.rs:238`.

### Constraints

- Keep human-readable status helpful.
- Do not remove typed public route fields.
- Do not make text mode less actionable; make it point to one public command/template.
- Avoid adding new public commands unless an existing command cannot represent the action.

### Done When

- `JsonFailure` messages do not contain command-shaped "Next public action" strings.
- Failure JSON contains either structured public route fields or explicit instruction to query status/operator JSON for typed public route output.
- Operator and doctor text surfaces use one public next-step description.
- Text surfaces avoid "record or refresh", "dispatch or record", and low-level "record" language unless the public command itself is named.
- Tests assert display strings are display-only and not executable authority.

### Files

- `src/execution/command_eligibility.rs`
- `src/execution/commands/common/outputs.rs`
- `src/workflow/operator.rs`
- `src/workflow/doctor_dashboard.rs`
- `src/workflow/status.rs`
- `schemas/*.json`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/public_cli_flow_contracts.rs`

### Implementation Steps

1. Inspect `JsonFailure` structure and decide whether to add typed route fields or keep messages route-neutral.
2. Replace "Next public action: <display command>" with one of:
   - structured `recommended_public_command_argv`;
   - structured public template;
   - a route-neutral pointer: "Query workflow operator/status JSON and execute the typed public command/template."
3. Ensure `recommended_command` remains marked display-only in schemas.
4. Refactor operator and doctor text renderers to consume the same public route projection where possible.
5. Replace compound phrases with one public next step:
   - "Run `plan execution close-current-task ...` using the provided argv/template."
   - "Run `plan execution advance-late-stage ...` using the provided argv/template."
   - "Run the routed review dispatch command using typed argv/template fields."
6. Historical cleanup note: remove the stale plan-fidelity missing-artifact matcher from doctor dashboard if no production reason emits it.
7. Add tests that reject command-shaped text in failure messages and old multi-action prose in active text surfaces.

### Validation Expectations

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test workflow_runtime --no-fail-fast
cargo nextest run --test workflow_shell_smoke --no-fail-fast
cargo nextest run --test workflow_entry_shell_smoke --no-fail-fast
cargo nextest run --test runtime_authority_contracts --no-fail-fast
```

## Task 4 - Fix Prompt Packaging Paths And High-Use Skill Budgets

### Spec Coverage

REQ-005.

### Goal

Generated skills must resolve companion references in installed contexts and budget enforcement must cover high-use generated skill surfaces.

### Context

The audit found root-relative install-root references such as `review/plan-task-contract.md`, `review/late-stage-precedence-reference.md`, and `docs/featureforge/reference/2026-04-01-review-state-reference.md` in generated skills. It also found high-use generated skills not covered by per-skill prompt budgets.

### Constraints

- Edit `.tmpl` sources, not generated `SKILL.md` files directly.
- Regenerate checked-in generated docs.
- Do not move mandatory law out of top-level skills to satisfy budgets.
- Keep skill-local companion references explicitly skill-local.

### Done When

- Install-root references in generated skills use `$_FEATUREFORGE_ROOT/...` or another explicit installed-root convention.
- Skill-local references are clearly skill-local and tested as existing relative to the skill directory.
- `using-featureforge`, `brainstorming`, and `verification-before-completion` have explicit per-skill budgets or an explicit documented reason for exclusion.
- Prompt budget tests fail if bloat moves into unbudgeted high-use generated skills.
- Generated skill docs and agent docs are fresh.

### Files

- `skills/*.md.tmpl`
- generated `skills/*/SKILL.md`
- `skills/skill-doc-budgets.json`
- `scripts/gen-skill-docs.mjs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/runtime_instruction_contracts.rs`

### Implementation Steps

1. Search generated skills and templates for references beginning with `review/`, `docs/featureforge/reference/`, and `references/`.
2. Classify each reference as:
   - skill-local companion;
   - install-root FeatureForge reference;
   - user-workspace artifact path.
3. Prefix install-root references with `$_FEATUREFORGE_ROOT/`.
4. Leave skill-local references relative but make wording explicit, for example "skill-local `references/codex-tools.md`".
5. Add or update Node tests that verify referenced companion files exist either relative to the skill directory or under install root.
6. Add per-skill caps for high-use generated skills.
7. Regenerate skills and agent docs.

### Validation Expectations

```bash
node scripts/gen-skill-docs.mjs
node scripts/gen-agent-docs.mjs
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
cargo clippy --all-targets --all-features -- -D warnings
```

## Task 5 - Remove Event-Log Dependency On Router

### Spec Coverage

REQ-006.

### Goal

Event-log persistence, replay, and migration code must not import or call router logic.

### Context

`src/execution/event_log.rs:18` imports `route_runtime_state`, and migration parity code calls it at `src/execution/event_log.rs:3570` and `src/execution/event_log.rs:3594`. This violates the intended lower-layer role of event-log code.

### Constraints

- Preserve migration parity checks.
- Preserve event-log replay semantics.
- Do not weaken transition validation.
- Do not move routing decisions into event-log helpers under a different name.

### Done When

- `src/execution/event_log.rs` does not import router modules or call routing functions.
- Migration parity still validates that migrated state projects correctly, but it does so from a higher-level adapter/test harness.
- Boundary tests fail on future `event_log -> router` imports.

### Files

- `src/execution/event_log.rs`
- `src/execution/router.rs`
- `src/execution/query.rs`
- `src/execution/transitions.rs`
- a new or existing migration adapter module if needed
- `tests/runtime_module_boundaries.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/execution_query.rs`

### Implementation Steps

1. Inspect the current migration parity call sites and identify exactly what route-derived information they need.
2. Move route parity checks out of event-log loading into a higher-level function that has permission to depend on both event-log and router.
3. Keep event-log code responsible for loading, validating, migrating, and returning transition state only.
4. Update callers to invoke the higher-level parity check after event-log load, not during persistence/replay internals.
5. Add import-boundary tests that reject `event_log.rs` importing router or public route selection modules.
6. Add regression tests for migration parity so behavior stays intact.

### Validation Expectations

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test contracts_execution_runtime_boundaries --no-fail-fast
cargo nextest run --test execution_query --no-fail-fast
cargo nextest run --test runtime_authority_contracts --no-fail-fast
```

## Task 6 - Centralize Reentry And Repair Follow-up Decisions

### Spec Coverage

REQ-007.

### Goal

Router, read model, next-action projection, and repair-review-state mutation must share one decision object for stale target, execution reentry, planning reentry, repair follow-up, and task-closure bridge behavior.

### Context

`src/execution/read_model.rs:1412` calls `execution_reentry_target` with default authority inputs before router runs. `src/execution/review_state.rs:2696` through `src/execution/review_state.rs:3018` constructs repair plans and route actions locally. Router uses authority inputs in `src/execution/router.rs:429` and `src/execution/router.rs:464`.

### Constraints

- Preserve current convergence behavior.
- Preserve targetless stale-state reconcile behavior.
- Do not make repair mutators depend on presentation-layer text.
- Avoid widening mutable responsibilities of read-model modules.

### Done When

- There is one shared decision type for repair/reentry route semantics.
- Router consumes the shared decision.
- Read-model/public-route projection consumes the router/shared decision and does not recompute execution reentry with default authority inputs.
- `repair-review-state` consumes the shared decision for mutation planning and does not locally rewrite baseline bridge route semantics.
- Boundary tests fail if read model or review state introduces a second target selector for the same semantic question.

### Files

- `src/execution/repair_target_selection.rs`
- `src/execution/public_route_selection.rs`
- `src/execution/router.rs`
- `src/execution/read_model.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/review_state.rs`
- `src/execution/current_truth.rs`
- `src/execution/next_action.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/liveness_model_checker.rs`
- `tests/workflow_runtime.rs`

### Implementation Steps

1. Enumerate all call sites that answer:
   - stale task target;
   - execution reentry target;
   - planning reentry target;
   - required repair follow-up;
   - task-closure recording bridge;
   - cycle-break route.
2. Define a shared decision object in the module that already owns repair target selection or a new cohesive `repair_route_decision` module.
3. Make that decision object accept the same authority inputs that production router currently uses.
4. Update router to consume the decision object.
5. Update read-model projection to consume route/decision output instead of recomputing execution reentry.
6. Update `repair_review_state` to consume the decision object when deciding what to clear, what follow-up to persist, and what route action to report.
7. Delete or demote local helper functions that duplicate route/follow-up semantics.
8. Add liveness regression cases for:
   - execution reentry required;
   - task closure recording ready;
   - stale unreviewed;
   - targetless runtime reconcile;
   - cycle-break clear after closure.

### Validation Expectations

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test workflow_runtime --no-fail-fast
cargo nextest run --test runtime_authority_contracts --no-fail-fast
cargo nextest run --test contracts_execution_runtime_boundaries --no-fail-fast
cargo test --test liveness_model_checker
```

## Task 7 - Close Test Realism And Display-Command Gaps

### Spec Coverage

REQ-008 and REQ-009.

### Goal

Tests must not preserve old executable-display-command assumptions, and synthetic historical fixture tests must be clearly classified.

### Context

`tests/workflow_runtime.rs:506` and `tests/internal_contracts_execution_runtime_boundaries.rs:468` split `recommended_command`. Synthetic historical replay setup exists in `tests/public_replay_churn.rs:1692` and `tests/runtime_behavior_golden.rs:471`.

### Constraints

- Do not remove valuable historical replay coverage.
- Do not convert public-flow tests back to direct helper calls.
- Keep true internal compatibility tests explicit and documented.

### Done When

- Public-flow and semantic tests execute typed argv/template output, not display strings.
- Any remaining display-string splitting helper is named as internal display compatibility coverage and cannot be used by public-flow tests.
- Static tests fail if public-flow tests split `recommended_command`.
- Synthetic historical setup tests are named and documented as synthetic setup plus public recovery.
- Release/audit claims cannot accidentally call synthetic setup a fully public creation path.

### Files

- `tests/workflow_runtime.rs`
- `tests/internal_contracts_execution_runtime_boundaries.rs`
- `tests/public_replay_churn.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/support/public_featureforge_cli.rs`
- `tests/support/internal_runtime_direct.rs`

### Implementation Steps

1. Replace display-string execution helpers with typed argv/template execution helpers where the test is not explicitly display-compatibility coverage.
2. If one display compatibility helper remains, rename it to include `internal_display_compatibility_only`.
3. Add scanner rules that reject `.split_whitespace()` or shell splitting on `recommended_command` in public-flow tests.
4. Add comments and test names that state synthetic fixture setup is not public setup.
5. Ensure public recovery steps in synthetic historical tests continue to use the compiled public CLI.
6. Add a test that synthetic exceptions must be registered with a reason in `public_cli_flow_contracts`.

### Validation Expectations

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test workflow_runtime --no-fail-fast
cargo nextest run --test contracts_execution_runtime_boundaries --no-fail-fast
cargo nextest run --test workflow_shell_smoke --no-fail-fast
cargo nextest run --test public_cli_flow_contracts --no-fail-fast
```

If `public_cli_flow_contracts` is not a standalone integration-test target, run the containing Rust test command that owns it.

## Task 8 - Surface Worktree-Lease Cleanup Failures On Task Close

### Spec Coverage

REQ-010.

### Goal

`close-current-task` must not report a clean success while worktree-lease cleanup failed in a way that can later block progress.

### Context

`src/execution/commands/close_current_task.rs:787` ignores errors from `release_worktree_leases_for_current_task_closures_and_persist`. The callee can fail in `src/execution/authority.rs:1262`, and `repair-review-state` propagates analogous errors in `src/execution/review_state.rs:1249`.

### Constraints

- Do not make closure recording non-atomic unless explicitly designed and tested.
- Preserve authoritative closure event append semantics.
- Avoid introducing a new churn loop where closure succeeds, cleanup fails, and rerunning close duplicates closure records.

### Done When

- Lease cleanup failure after close is either:
  - prevalidated before closure append; or
  - propagated with a clear diagnostic that closure status is known; or
  - recorded as an explicit diagnostic event that router can resolve.
- The user-facing result cannot silently claim fully clean success if cleanup failed.
- Tests cover malformed authority/persistence failure and rerun behavior.

### Files

- `src/execution/commands/close_current_task.rs`
- `src/execution/authority.rs`
- `src/execution/review_state.rs`
- `src/execution/router.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_authority_contracts.rs`

### Implementation Steps

1. Inspect cleanup failure modes in `release_worktree_leases_for_current_task_closures_and_persist`.
2. Decide whether cleanup can be safely prevalidated before closure append.
3. If prevalidation is safe, run it before appending closure and fail without mutating closure state.
4. If prevalidation is not sufficient, propagate post-close cleanup failure with structured status explaining:
   - closure was recorded or not recorded;
   - cleanup failed;
   - the next public route to reconcile the lease state.
5. Ensure rerunning `close-current-task` after a cleanup failure does not duplicate closure proof or reopen the same task without a real negative/stale condition.
6. Add regression tests for cleanup failure and rerun convergence.

### Validation Expectations

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test workflow_runtime --no-fail-fast
cargo nextest run --test workflow_shell_smoke --no-fail-fast
cargo nextest run --test runtime_authority_contracts --no-fail-fast
```

## Task 9 - Final Cross-Surface Verification And Review

### Spec Coverage

REQ-011.

### Goal

Prove the remediation is complete across runtime, tests, prompts, generated docs, schemas, and liveness surfaces.

### Context

The original audit instructions require broad validation and clean-context review after implementation. This task is the final gate for all previous tasks.

### Constraints

- Do not claim completion if any validation command fails.
- Do not skip Node docs/prompt tests after skill template changes.
- Do not skip liveness after routing or repair decision changes.
- Do not ignore generated-doc drift.

### Done When

- All validation commands below pass.
- A clean-context reviewer finds no plan-compliance issues.
- Any reviewer findings are remediated and validation is rerun.
- The final report lists exact commands and outcomes.

### Files

- All changed files from Tasks 1 through 8.
- `docs/featureforge/reference/2026-05-07-deep-runtime-safety-reaudit.md`
- this plan file.

### Implementation Steps

1. Run the full validation set:

   ```bash
   node scripts/gen-skill-docs.mjs --check
   node scripts/gen-agent-docs.mjs --check
   node --test tests/codex-runtime/*.test.mjs
   cargo clippy --all-targets --all-features -- -D warnings
   cargo nextest run --all-targets --all-features --no-fail-fast
   cargo test --test liveness_model_checker
   ```

2. Review the final diff against each requirement in this plan.
3. Confirm banned active-output phrases are absent:

   ```text
   rebuild evidence
   rebuild its evidence
   rebuild the packet
   record receipt
   repair unit-review receipt
   run gate-review
   run gate-finish
   split recommended_command
   ```

4. Confirm allowed historical/audit references are clearly in historical docs, test fixtures, or audit artifacts, not active prompts/runtime guidance.
5. Dispatch one clean-context reviewer with:
   - the audit report;
   - this plan;
   - the final diff;
   - validation results.
6. Remediate any review findings.
7. Rerun the full validation set after remediation.

### Validation Expectations

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets --all-features --no-fail-fast
cargo test --test liveness_model_checker
```

Final recommendation should remain "ship only after targeted fixes" until this plan is implemented and independently reviewed clean.
