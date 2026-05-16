# Runtime public-realism and signal/noise twelfth-audit remediation

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** implementation
**Source Audit:** `docs/featureforge/reference/2026-05-09-deep-runtime-safety-twelfth-audit.md`
**Last Reviewed By:** audit-remediation

## Goal

Eliminate the actionable twelfth-audit findings without reintroducing runtime churn. Preserve the improved public route model, keep tests performant, and reduce duplicated workflow law instead of adding more brittle guard layers.

## Architecture

The implementation must preserve this runtime flow:

CLI args -> command module -> transition guard -> event append -> reducer -> read model/status assembly -> route decision -> workflow operator/status presentation.

The next fixes should strengthen that flow:

- command modules consume shared route/follow-up decisions instead of reading operator/status fields to invent recovery;
- public-flow tests use the shipped CLI boundary when they claim end-to-end public behavior;
- semantic/model-checker tests may use in-process helpers only when labeled and paired with shipped-CLI parity coverage;
- public diagnostics point agents back to workflow/operator typed argv/template authority;
- repeated prompt law is generated once and inserted where needed.

## Change Surface

- Runtime routing/follow-up:
  - `src/execution/commands/advance_late_stage.rs`
  - `src/execution/route_plan/follow_up.rs`
  - `src/execution/review_route_tokens.rs`
  - `src/execution/observability.rs`
  - related route/follow-up tests
- Liveness/test realism:
  - `tests/liveness_model_checker.rs`
  - `tests/support/internal_public_runtime_in_process.rs`
  - `tests/support/public_featureforge_cli.rs`
  - `tests/public_cli_flow_contracts.rs`
  - `tests/workflow_shell_smoke.rs`
- Public diagnostics/docs:
  - `src/execution/state/review_gate.rs`
  - `src/execution/state/artifact_finish_truth.rs`
  - `skills/plan-eng-review/SKILL.md.tmpl`
  - generated `skills/plan-eng-review/SKILL.md`
  - `docs/project_notes/bugs.md`
  - `skills/project-memory/examples.md`
  - instruction tests
- Signal/noise:
  - `scripts/gen-skill-docs.mjs`
  - high-use skill templates and generated docs
  - `docs/featureforge/reference/execution-runtime-module-boundaries.md`
  - `tests/runtime_module_boundaries.rs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `tests/public_cli_flow_contracts.rs`

## Preconditions

- Do not use FeatureForge runtime/project skills.
- Use Rust best practices for Rust edits.
- Do not let subagents spawn subagents.
- Run strict clippy and full no-fail-fast nextest after each completed task before clean-context review.
- If full nextest exceeds 4-5 minutes, let it finish, run `cargo clean`, rerun full nextest, and stop to fix performance if it is still over 4-5 minutes.
- Regenerate generated skill docs after template/generator changes.
- Do not weaken public/runtime boundary tests; when a test is too noisy, replace it with a more semantic assertion.

## Known Footguns / Constraints

- Do not restore one-subprocess-per-synthetic-edge liveness execution; that previously pushed the suite beyond the performance budget.
- Do not pretend the in-process liveness runner is shipped-runtime proof. Label it as semantic coverage and pair it with shipped CLI parity samples.
- Do not move mandatory operator route law solely into companion references. Generated snippets are acceptable only when they remain top-level in each generated skill.
- Do not replace public diagnostics with vague "do the right thing" text. They must point back to workflow/operator typed argv/template authority.
- Do not manually edit generated `SKILL.md` output without changing the `.tmpl` and rerunning the generator.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| REQ-001: QA late-stage follow-up/requery decisions are centralized in shared route/follow-up logic. | Task 1 |
| REQ-002: Stable reason/wait-state vocabulary is compared through shared constants in touched routing code. | Task 1 |
| REQ-003: Liveness model checker no longer claims shipped public runtime coverage while using an internal shim. | Task 2 |
| REQ-004: Liveness retains shipped CLI parity coverage for sampled public edges without regressing full-suite time. | Task 2 |
| REQ-005: Active public diagnostics do not tell agents to record low-level branch/pivot artifacts or chain skills manually. | Task 3 |
| REQ-006: Plan-eng-review guidance matches the engineering-review -> fidelity -> approval sequence. | Task 3 |
| REQ-007: Project memory examples remain non-authoritative and do not suggest manual runtime/evidence repair. | Task 3 |
| REQ-008: High-use skills share one generated operator route-authority block while keeping mandatory law top-level. | Task 4 |
| REQ-009: Runtime module caps have one documented source of truth, and boundary tests avoid cap-table duplication. | Task 4 |
| REQ-010: All changes pass strict clippy, full no-fail-fast nextest, Node doc checks, and clean-context review loops. | Task 5 |

## Task 1: Centralize QA follow-up and route vocabulary

**Spec Coverage:** REQ-001, REQ-002

**Goal:** Remove the remaining command-side recovery decision in `record_qa_for_command` and make the shared route/follow-up path own `derived_review_state_missing` repair/requery semantics.

**Context:**

- Twelfth audit found `record_qa_for_command` locally inspects `operator.phase`, `operator.phase_detail`, `operator.review_state_status`, `operator.current_branch_closure_id`, and raw `derived_review_state_missing`.
- Shared follow-up routing already exists in `required_follow_up_from_routing` and `route_plan/follow_up.rs`.
- The right fix is to make the shared path produce the same follow-up/requery signal, then let the command consume it.

**Constraints:**

- Do not change QA recording success semantics.
- Do not expose a new public command for diagnostic-only states.
- Keep exact behavior for stale/missing review-state repair, but move the decision out of the command module.
- Replace raw reason/wait-state literals in touched production routing code with constants.

**Done when:**

- `record_qa_for_command` no longer locally forces `FOLLOW_UP_REPAIR_REVIEW_STATE` by inspecting `derived_review_state_missing`.
- Shared route/follow-up tests prove clean execution-in-progress plus `derived_review_state_missing` and missing branch closure yields the same requery/repair follow-up signal.
- Touched production code uses shared constants for `derived_review_state_missing`, `stale_provenance`, `blocked_on_plan_revision`, and `waiting_for_external_review_result` where applicable.
- Boundary tests cover that `advance_late_stage.rs` does not compare `derived_review_state_missing` directly.

**Files:**

- `src/execution/commands/advance_late_stage.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/review_route_tokens.rs`
- `src/execution/observability.rs`
- `src/execution/query.rs`
- `tests/runtime_module_boundaries.rs`

**Implementation Steps:**

1. Add a stable constant for `derived_review_state_missing` to `review_route_tokens.rs` or another shared route-vocabulary module.
2. Add a stable constant for `waiting_for_external_review_result` if no shared constant exists.
3. Update `route_plan/follow_up.rs` so `derive_required_follow_up_from_optional_status` handles the QA requery condition from shared status inputs.
4. Remove the local `required_follow_up` override block from `record_qa_for_command`.
5. Update touched raw string comparisons to use constants.
6. Add unit coverage proving the shared follow-up helper owns the previously local condition.
7. Add or update a boundary assertion so the command module cannot reintroduce the raw `derived_review_state_missing` follow-up override.

**Validation Expectations:**

- `cargo fmt --check`
- targeted route/follow-up tests
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 1

## Task 2: Restore liveness public-runtime realism without regressing performance

**Spec Coverage:** REQ-003, REQ-004

**Goal:** Keep liveness model checking fast while making the public/private test boundary honest and adding shipped CLI parity coverage for sampled public edges.

**Context:**

- The liveness checker currently executes public-labeled successor edges through `tests/support/internal_public_runtime_in_process.rs`.
- That saved significant time, but it weakens tests-realism claims.
- Public replay/golden tests still use the compiled CLI; liveness should be labeled as semantic/model coverage and should include a small shipped CLI parity sentinel.

**Constraints:**

- Do not return to one compiled subprocess per synthetic liveness edge.
- Do not allow protected public-flow tests to import the in-process shim.
- Keep full nextest under the 4-5 minute budget.
- Keep hidden-command and repeated-route liveness assertions intact.

**Done when:**

- The liveness in-process runner and call sites are named/documented as semantic in-process public-argv execution, not shipped runtime proof.
- `tests/public_cli_flow_contracts.rs` forbids protected public-flow tests from importing `support/internal_public_runtime_in_process.rs`.
- `tests/liveness_model_checker.rs` is not categorized as protected public-flow end-to-end coverage solely because it runs semantic model edges.
- A shipped CLI parity test executes at least one sampled public liveness edge through the compiled binary and compares route-relevant status before/after against the semantic runner.
- Public shell-smoke fixture setup no longer uses misleading `internal_only_` helper naming for fixture artifact seeding.

**Files:**

- `tests/liveness_model_checker.rs`
- `tests/support/internal_public_runtime_in_process.rs`
- `tests/support/public_featureforge_cli.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/workflow_shell_smoke.rs`

**Implementation Steps:**

1. Rename liveness-local module/function aliases from `public_runtime` to semantic/in-process wording.
2. Update liveness assertion text from "real public edge" to "semantic public argv edge" where it is not using the compiled binary.
3. Add a small real-CLI parity test that prepares identical liveness fixtures, compares route signatures from semantic and shipped CLI status, executes one sampled public mutation through each, and compares successor route signatures.
4. Update public-flow guard logic so the internal semantic runner is forbidden in protected public-flow tests, while the liveness checker is explicitly outside the end-to-end public-flow file set.
5. Rename `internal_only_write_dispatched_branch_review_artifact` in shell-smoke fixture setup to synthetic fixture wording.
6. Run the liveness checker and public-flow contracts to confirm the performance/realism boundary.

**Validation Expectations:**

- `cargo fmt --check`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test liveness_model_checker -- --nocapture`
- `cargo test --test workflow_shell_smoke workflow_operator_routes_qa_pending_to_record_qa -- --exact --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 2

## Task 3: Repair public diagnostics and plan-review guidance

**Spec Coverage:** REQ-005, REQ-006, REQ-007

**Goal:** Remove active wording that can send agents into low-level recorders, multi-skill choreography, premature engineering approval, manual note clearing, or evidence rebuilding.

**Context:**

- Public doctor gate diagnostics include "Record a fresh branch closure" and "Record a workflow pivot".
- Finish-readiness diagnostics include multi-step skill chains.
- `plan-eng-review` still says to set `Last Reviewed By` at the same time as `Engineering Approved`.
- Project memory examples still describe manual note clearing and evidence rebuilding.

**Constraints:**

- Diagnostics should point back to workflow/operator JSON and typed public argv/template authority.
- Do not erase useful failure context; preserve reason codes and high-level prerequisite meaning.
- Keep project memory supportive and non-authoritative.
- Edit skill templates and regenerate generated skills.

**Done when:**

- Public gate/finish remediation strings no longer use command-shaped or low-level record/pivot language.
- Finish diagnostics do not instruct agents to run skill chains; they point to workflow/operator typed routes and current prerequisite artifacts.
- `plan-eng-review` guidance says engineering-review edits stay Draft, then `Last Reviewed By: plan-eng-review` marks readiness for plan-fidelity; `Engineering Approved` comes only after current final fidelity.
- Tests reject the stale "set Last Reviewed By at the same time as Engineering Approved" guidance.
- Project memory notes/examples do not instruct manual note clearing or evidence rebuilding.

**Files:**

- `src/execution/state/review_gate.rs`
- `src/execution/state/artifact_finish_truth.rs`
- `skills/plan-eng-review/SKILL.md.tmpl`
- `skills/plan-eng-review/SKILL.md`
- `docs/project_notes/bugs.md`
- `skills/project-memory/examples.md`
- `tests/runtime_instruction_plan_review_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

**Implementation Steps:**

1. Replace doctor gate remediation strings with workflow/operator typed-route guidance.
2. Replace finish-readiness remediation strings with one-route guidance that names the prerequisite but not skill chains.
3. Update plan-eng-review template sequencing text and regenerate skills.
4. Add negative assertions for stale plan-eng guidance.
5. Update project memory note/example wording to describe runtime-routed repair rather than manual note/evidence repair.
6. Extend active doc/diagnostic scanners only if needed, preferring semantic helper checks over new one-off phrase traps.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo test --test runtime_instruction_plan_review_contracts -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 3

## Task 4: Reduce duplicated prompt law and boundary-cap noise

**Spec Coverage:** REQ-008, REQ-009

**Goal:** Collapse duplicated operator route law and duplicated module cap data while preserving mandatory top-level instructions and architectural boundary enforcement.

**Context:**

- The signal/noise audit found duplicate operator route-authority prose across high-use skills.
- The module cap table is duplicated between docs and tests.
- Some tests protect wording by exact fragments; improve where practical by using shared semantic checks.

**Constraints:**

- Mandatory route law must remain top-level in each generated skill that needs it.
- Companion references may explain details, but must not become the only place where mandatory law exists.
- Do not remove import-boundary or single-owner checks.
- Prefer one documented source of truth for caps; tests should consume it.

**Done when:**

- A generated `OPERATOR_ROUTE_AUTHORITY` snippet or equivalent produces the shared operator argv/template law.
- `executing-plans`, `subagent-driven-development`, and other high-use skills use the generated snippet and keep only skill-specific guardrails locally.
- Skill budgets remain enforced and under limit.
- `focused_runtime_modules_have_line_caps` and reduced-facade cap tests read cap rows from `execution-runtime-module-boundaries.md` or another single manifest rather than duplicating the table in Rust.
- Route-plan cap pressure is reduced to a reasonable coarse budget without weakening architectural import boundaries.

**Files:**

- `scripts/gen-skill-docs.mjs`
- high-use skill templates and generated skills
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/skill-doc-budgets.json`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`

**Implementation Steps:**

1. Add a generator resolver for the shared operator route-authority block.
2. Replace duplicated prose in high-use templates with the resolver.
3. Regenerate skills and update prompt budget manifest only if the generated word counts require it.
4. Refactor runtime module boundary cap tests to parse the docs table as the cap source.
5. Raise overly tight exact caps to coarse, defensible budgets in the docs if the module is at the cap solely due to boundary glue.
6. Where practical, replace exact prose assertions with shared semantic checks for display-only command text and typed argv/template authority.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 4

## Task 5: Whole-plan validation, review, and re-audit

**Spec Coverage:** REQ-010

**Goal:** Prove all twelfth-audit fixes are complete, then rerun the deep audit loop with subagents A-I, including the signal/noise auditor.

**Context:**

- The user requires strict clippy and full nextest before review dispatch.
- The user requires cargo clean before each audit-loop iteration.
- The audit loop continues until there are no actionable audit issues.

**Constraints:**

- Do not interrupt in-flight subagents.
- Do not let review/audit subagents spawn subagents.
- Do not use FeatureForge runtime/project skills.
- If full nextest exceeds 4-5 minutes, rerun from `cargo clean`; if still slow, stop and fix performance.

**Done when:**

- Node generation/contracts pass.
- Strict clippy passes.
- Full nextest passes under the performance threshold.
- Clean-context whole-plan review finds no actionable issues.
- `cargo clean` runs before the next audit-loop iteration.
- A-I audit subagents run, including signal/noise.
- If A-I finds no actionable issues, the loop ends with a ship recommendation; if it finds issues, a new remediation plan is created and implemented.

**Files:**

- validation output only unless remediation is required.

**Implementation Steps:**

1. Run required Node checks.
2. Run `cargo fmt --check`.
3. Run strict clippy.
4. Run full no-fail-fast nextest and record wall time.
5. Dispatch a clean-context whole-plan reviewer with the exact plan path and validation evidence.
6. Remediate and repeat validation/review until clean.
7. Run `cargo clean`.
8. Dispatch audit subagents A-I with the same deep-audit scope plus signal/noise check.
9. Synthesize the audit and continue or stop according to actionable findings.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- `cargo test --test liveness_model_checker -- --nocapture`
- clean-context whole-plan review
- clean-context A-I audit
