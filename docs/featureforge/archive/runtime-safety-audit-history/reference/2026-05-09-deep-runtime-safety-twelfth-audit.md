# FeatureForge deep runtime-safety twelfth audit

**Date:** 2026-05-09
**Subject:** Post-eleventh-remediation audit of public runtime reachability, control-plane leakage, test realism, split decisioning, prompt surface, and signal/noise.
**Method:** Clean-context parallel audit subagents A-I, with an added signal/noise auditor. Subagents were instructed not to use FeatureForge runtime/project skills, not to spawn subagents, and not to modify files.

## Executive verdict

**Recommendation:** do not ship yet; ship only after targeted fixes.

FeatureForge is close. The main public route and control-plane architecture now looks substantially safer than the earlier branches: typed public argv/templates are the clear runtime contract, current closures are no longer overridden by receipt/projection drift in the audited paths, and prompt packaging is budgeted and fresh.

The remaining issues are not broad structural collapse, but they are actionable:

- one public-labeled liveness coverage path still runs through an internal in-process shim instead of shipped runtime process behavior;
- one late-stage QA command locally re-derives follow-up routing instead of consuming the shared route decision;
- public doctor/finish diagnostics still include wording that can send agents toward low-level recording or multi-skill choreography instead of one routed public next step;
- plan-eng-review has contradictory active wording around when to set `Last Reviewed By`;
- static boundary tests and skill law are becoming noisy enough that the next improvement should delete duplication and centralize shared wording.

## What is genuinely fixed

- Public CLI reachability is fixed for normal paths. `plan execution begin`, `close-current-task`, `repair-review-state`, `advance-late-stage`, and workflow `operator/status/doctor` are public CLI surfaces, with dispatch in `src/lib.rs`.
- Public route authority is typed. `PublicRouteDecision::command_surfaces` and route/status projection expose `recommended_public_command_argv` and `recommended_public_command_template`; `recommended_command` is display-only compatibility text.
- `begin` owns preflight setup, and `close-current-task` can refresh/record dispatch lineage through public ownership.
- Receipt/projection freshness no longer appears to override current task closure authority in the audited code paths.
- `blocked_runtime_bug` is diagnostic-only and does not expose normal mutation commands.
- Prompt packaging is enforced: generated skill docs and agents are fresh, top-level budgets are active, companion references are packaged, and reviewer recursion prevention is prompt-text scoped.

## What remains risky

- The liveness model checker still names and treats its successor edges as public progress while dispatching through `tests/support/internal_public_runtime_in_process.rs`.
- `record_qa_for_command` in `src/execution/commands/advance_late_stage.rs` still inspects operator/read-model fields and raw reason codes to synthesize a recovery follow-up locally.
- Some public diagnostic remediation strings say `Record a fresh branch closure`, `Record a workflow pivot`, or chain skills such as `Run document-release, then rerun requesting-code-review`.
- Active repo-visible project memory examples still include manual repair language such as clearing parked notes and rebuilding stale evidence.
- High-use skills repeat the same operator argv/template law in multiple places, and boundary cap data is duplicated between docs and tests.

## Concrete dead ends still possible

- An agent reading public doctor JSON can see gate remediation text that says to "Record" branch closure or workflow pivot, then go looking for low-level recording primitives instead of returning to workflow/operator typed argv.
- An agent following `plan-eng-review` can see stale wording that says to set `Last Reviewed By: plan-eng-review` at the same time as `Workflow State: Engineering Approved`, which conflicts with the fidelity-before-approval sequence.
- A future liveness regression could pass through the in-process shim while the shipped binary path diverges at the environment, stdout/stderr, exit-code, or outer CLI-dispatch boundary.

## Concrete churn sources still possible

- Static line caps are duplicated in `tests/runtime_module_boundaries.rs` and `docs/featureforge/reference/execution-runtime-module-boundaries.md`, creating cap-chasing pressure.
- Prompt law around operator JSON, typed argv/templates, display-only commands, and repair-state stop rules is repeated across high-use skills.
- Some scanners pin exact prose fragments rather than one semantic helper for display-only and typed-command authority.

## Public/private test mismatch assessment

Mostly fixed, with one high issue. Public replay and shell/golden tests use the compiled CLI for public recovery. Internal helper tests are mostly quarantined. The liveness checker is the exception: `tests/liveness_model_checker.rs` imports `tests/support/internal_public_runtime_in_process.rs` and executes public-labeled successor edges through a parser-plus-internal-runtime shim.

## Receipt/evidence/projection control-plane assessment

No actionable control-plane leakage was found in this audit. Current task closure remains task-boundary authority, current closures are filtered out of stale targets, release/final/QA docs are tied to event-authoritative late-stage records, and projection loss is diagnostic after authoritative closure exists.

## Prompt-surface and packaging assessment

Packaging is healthy: budgets are enforced, generated docs are fresh, companion references resolve, and reviewer recursion is prompt-only. The remaining prompt problem is signal/noise: duplicate top-level routing law should be generated once and inserted where needed, not hand-maintained as near-identical blocks.

## Modularization and split-decisioning assessment

Modularization reduced several large files and moved route decisions into `route_plan`, but one command-side detour remains. `record_qa_for_command` locally inspects `operator.phase`, `phase_detail`, `review_state_status`, `current_branch_closure_id`, and raw `derived_review_state_missing` reason codes to force follow-up guidance. That belongs in shared route/follow-up decisioning.

## Reviewer recursion assessment

Fixed. Reviewer recursion prevention is prompt-text only and reviewer-prompt scoped; no runtime/env recursion guard was found.

## Validation results

Parent validation before the audit loop:

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 129/129.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Full nextest: passed, run ID `82f08f96-1cc7-4990-8bbf-60372d42ef2e`, 1657/1657, nextest 157.136s, wall 158.33s.
- `cargo test --test liveness_model_checker -- --nocapture`: passed, 28/28, 79.36s.

Audit subagent targeted checks:

- Public CLI/reachability: `cargo test --test public_cli_flow_contracts`: passed, 61/61.
- Prompt packaging: generated skill and agent checks passed; budget/contracts/generation Node tests passed.
- Plan-review guidance: `cargo test --test runtime_instruction_plan_review_contracts`: passed.
- Modularization: `cargo test --test runtime_module_boundaries --test contracts_execution_runtime_boundaries -- --nocapture`: passed, 61/61 and 30/30.

## Prioritized findings

### High

1. **Public-labeled liveness coverage is not shipped-runtime realistic.**
   - Type: test realism issue.
   - References:
     - `tests/liveness_model_checker.rs` imports `support/internal_public_runtime_in_process.rs`.
     - `tests/liveness_model_checker.rs::run_featureforge_public_runtime` dispatches through that shim.
     - `tests/support/internal_public_runtime_in_process.rs` parses `Cli` and calls internal `ExecutionRuntime`, `operator::operator_for_runtime`, and `mutate::*` directly.
     - `tests/public_cli_flow_contracts.rs::forbidden_internal_support_paths` does not include that helper path.
   - Impact: liveness can prove parser/internal-runtime behavior while missing shipped binary boundary differences.

2. **`record_qa_for_command` still makes a local follow-up routing decision.**
   - Type: architecture / split-decisioning issue.
   - References:
     - `src/execution/commands/advance_late_stage.rs::record_qa_for_command`.
     - Shared route/follow-up owners: `src/execution/query.rs::required_follow_up_from_routing`, `src/execution/route_plan/follow_up.rs`.
   - Impact: a mutation command recomputes recovery semantics from read-model/operator fields instead of consuming the shared route decision.

3. **Public doctor gate diagnostics can point at low-level recording concepts.**
   - Type: public-output / agent-UX issue.
   - References:
     - `src/execution/state/review_gate.rs` remediation strings: "Record a fresh branch closure" and "Record a workflow pivot".
     - Public serialization path: `src/workflow/operator.rs::WorkflowDoctor`, `src/execution/status.rs` gate diagnostics.
   - Impact: public output can send agents toward record/pivot concepts instead of workflow/operator typed public routes.

### Medium

4. **`plan-eng-review` active guidance still contains stale approval sequencing.**
   - Type: documentation / workflow guidance issue.
   - References:
     - `skills/plan-eng-review/SKILL.md.tmpl`
     - generated `skills/plan-eng-review/SKILL.md`
     - `tests/runtime_instruction_plan_review_contracts.rs` does not reject the stale phrase.
   - Impact: agents can set `Workflow State: Engineering Approved` too early or misunderstand the plan-fidelity sequence.

5. **Finish-readiness diagnostics give multi-step skill chains instead of one route authority.**
   - Type: public-output / agent-UX issue.
   - References:
     - `src/execution/state/artifact_finish_truth.rs` release/final/QA remediation strings.
   - Impact: public diagnostics can encourage chaining skills by memory rather than returning to workflow/operator typed argv/template.

6. **Active project memory guidance still suggests manual note/evidence repair.**
   - Type: documentation / agent-UX issue.
   - References:
     - `docs/project_notes/bugs.md`
     - `skills/project-memory/examples.md`
   - Impact: agents can be pulled toward manual parked-note clearing or evidence rebuilding.

7. **Boundary caps and route-law prose are becoming noisy.**
   - Type: signal/noise / maintainability issue.
   - References:
     - duplicated line-cap table in `tests/runtime_module_boundaries.rs` and `docs/featureforge/reference/execution-runtime-module-boundaries.md`.
     - repeated operator argv/template law in `skills/executing-plans`, `skills/subagent-driven-development`, `skills/using-featureforge`, and `skills/plan-eng-review`.

### Low

8. **Public shell-smoke fixture setup uses private-named helper wording.**
   - Type: test readability issue.
   - Reference: `tests/workflow_shell_smoke.rs::internal_only_write_dispatched_branch_review_artifact`.

9. **Some prose scanners pin exact sentence fragments.**
   - Type: signal/noise issue.
   - References:
     - `tests/public_cli_flow_contracts.rs` schema description literals.
     - `tests/codex-runtime/skill-doc-contracts.test.mjs` prompt-law regexes.

## Recommendation

Ship only after targeted fixes:

1. Make the liveness checker explicitly semantic/in-process and add shipped-CLI parity coverage for sampled public edges, while keeping full-suite runtime under the 4-5 minute threshold.
2. Move QA follow-up/requery decisioning into shared route/follow-up logic.
3. Replace public diagnostic remediation strings with workflow/operator typed-route guidance.
4. Fix plan-eng-review approval sequencing wording and add negative tests.
5. Collapse duplicate operator route law into generated snippets and remove line-cap duplication by using one documented cap source.
