# Workflow State

Engineering Approved

# Plan Revision

Revision 1 - 2026-05-14

# Execution Mode

Manual implementation loop. Do not run FeatureForge runtime workflows or project skills.

# Goal

Remediate the actionable findings from the latest runtime-safety audit loop without expanding workflow law or adding low-signal guard layers. The implementation must tighten public mutation authority, remove executable-looking nested command strings that lack typed authority, make the test-plan-refresh lane a single clear handoff, fold persisted execution-reentry into route selection, and clean up wording/tests that can reintroduce stale fingerprint or scanner-noise drift.

# Architecture

- Public mutation authority belongs to the typed public route decision. Diagnostic fields such as `resume_task` and `resume_step` may describe recovery context, but they must not independently authorize a `begin` mutation.
- Public output must expose one executable contract. `recommended_public_command_argv` is the exact authoritative machine invocation; display-only compatibility text `recommended_command` must never appear in nested output where no typed argv/template authority exists.
- Diagnostic handoff lanes should name one next action. A workflow output that cannot provide a typed runtime command should not combine skill handoff, re-query instruction, and route-law prose in the same actionable field.
- Route decisions should be selected in one route-planning path. Persisted execution-reentry fallback may be a candidate or explicit selection branch, but it must not patch an unrelated selected command after selection.
- Signal-to-noise guardrails should remove overclaims and duplication. Prefer behavioral invariants and precise wording over scanners that claim to protect one semantic rule while checking a weaker text shape.
- Skill wording must stay terse and action-guiding. Plan-fidelity language should refer to the current plan-fidelity binding/spec fingerprint instead of an ambiguous raw plan/spec fingerprint that can reintroduce approval loops.

# Change Surface

- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/unit_tests.rs`
- `src/execution/status.rs`
- `src/execution/state/runtime_methods.rs`
- `src/workflow/operator.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/unit_tests.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_runtime.rs`
- `tests/runtime_authority_contracts.rs`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `skills/plan-eng-review/SKILL.md.tmpl`
- generated `skills/plan-eng-review/SKILL.md`
- Node doc/skill contract tests if public wording contracts change.

# Preconditions

- Do not run FeatureForge runtime workflows or project skills.
- Do not allow subagents to spawn additional subagents.
- Use the requested Rust guidance while writing or refactoring Rust.
- Before every full test cycle, verify no `cargo`, `rustc`, `cargo nextest`, `cargo-nextest`, `nextest run`, or active `target/debug/deps/` process is already running.
- After each implementation task, run strict clippy and the full no-fail-fast nextest suite before dispatching a clean-context review for that exact task.
- If the full nextest suite takes more than 4-5 minutes, run `cargo clean`, rerun the suite, and remediate any repeatable performance regression. If it exceeds 10 minutes, stop immediately and enter the clean/rerun/performance remediation path.
- Before the next audit-loop iteration, run `cargo clean`.
- Preserve unrelated dirty worktree changes.

# Known Footguns / Constraints

- Do not make `resume_task` or `resume_step` a second route authority. They can explain context only when the typed route already authorizes the same action.
- Do not solve nested command ambiguity by teaching agents to parse display-only text more carefully. Either omit the field from nested gate outputs or add a typed authority surface derived from the same public command model.
- Do not put multiple required actions into a single public next-step field for `test_plan_refresh_required`.
- Do not preserve a post-selection helper that rewrites one public route into another public route. Finalization can enrich presentation; it must not choose a different mutation command.
- Do not add new static scans unless the scan checks the exact semantic claim it states.
- Do not hand-edit generated skill docs when a `.tmpl` source exists; edit the template and regenerate.
- Avoid active-doc wording that teaches hidden-helper, manual proof reconstruction, or stale command-folklore behavior.

# Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| `resume_task` / `resume_step` are diagnostic unless the exact typed public route already authorizes the same `begin` | Task 1 |
| Public mutation eligibility rejects resume-field-only `begin` requests | Task 1 |
| Nested gate/preflight outputs do not expose executable-looking display strings without typed argv/template authority | Task 2 |
| Output coverage catches future nested command-surface drift | Task 2 |
| `test_plan_refresh_required` presents one diagnostic handoff action | Task 3 |
| Active review-state reference matches the one-action handoff contract | Task 3 |
| Persisted execution-reentry fallback is selected inside route planning, not patched after selection | Task 4 |
| Route finalization remains presentation-only for persisted execution-reentry cases | Task 4 |
| Module-boundary coverage no longer overclaims a brittle text scan | Task 5 |
| Plan-engineering-review wording uses the current plan-fidelity binding/spec fingerprint | Task 5 |
| Prompt/doc changes remain high-signal and generated docs are fresh | Task 5 |

# Tasks

## Task 1 - Bind Resume Begin Eligibility To Typed Route Authority

### Spec Coverage

- `resume_task` and `resume_step` must be diagnostic unless the exact legal command is the same `begin`.

### Goal

Remove the standalone mutation-eligibility path that lets matching `resume_task` / `resume_step` fields authorize a `begin` command without the typed public route selecting that begin.

### Context

`decide_public_mutation` currently accepts a `begin` request when `request_matches_resume_begin` returns true. That helper checks phase detail, execution-started state, empty active task markers, matching resume fields, and fingerprint. It does not require the same begin to be the typed public route, execution command context, or explicit repair target.

### Constraints

- Preserve legal `begin` execution when the public route decision already selects the same task/step/fingerprint.
- Preserve explicit repair or stale-target routes that are already authorized through typed public command selection.
- Do not add another local routing predicate inside command eligibility if an existing exact-route helper can be reused.
- Do not weaken blocked-runtime or reconcile fail-closed behavior.

### Done when

- `request_matches_resume_begin` is removed or reduced so it cannot authorize a mutation independently of typed route authority.
- A regression test builds a status where resume fields match the requested begin but the public route is absent or points elsewhere, and eligibility rejects the mutation.
- Existing exact-route begin tests still pass.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/unit_tests.rs`

### Detailed Implementation Steps

1. Inspect `decide_public_mutation`, exact public-route matching helpers, explicit repair target handling, and existing begin eligibility tests.
2. Delete the standalone resume-field allowance or make it call the same exact typed-route matching path used for other public `begin` authorizations.
3. If a helper remains, rename it so the name reflects typed-route authority instead of resume-field matching.
4. Add a unit test where status has `phase_detail=execution_in_progress`, `execution_started=yes`, matching `resume_task` and `resume_step`, matching fingerprint, no active task markers, and no matching typed public route. Assert the requested `begin` is rejected.
5. Add or update a positive test proving a matching typed public `begin` route remains accepted.

### Validation Expectations

- Targeted: `cargo test -q request_matches_resume_begin` or the renamed regression test filter.
- Required after task: `cargo clippy --all-targets --all-features -- -D warnings`.
- Required after task: `cargo nextest run --all-targets --all-features --no-fail-fast`.
- Clean-context review against Task 1 after full validation.

## Task 2 - Remove Nested Display-Only Command Strings From Gate Outputs

### Spec Coverage

- Nested gate and preflight outputs must not expose executable-looking command strings without typed argv/template authority.

### Goal

Make `GateResult` command guidance safe for public JSON consumers by preventing nested `recommended_command` strings from appearing where no `recommended_public_command_argv` or typed command template exists.

### Context

Top-level operator/status output already treats display-only compatibility text `recommended_command` as non-authoritative and exposes typed argv/template fields. `GateResult` still has a nested `recommended_command` field, and runtime methods populate command-shaped strings into gate/preflight outputs that lack a typed authority sibling field.

### Constraints

- Prefer omitting or nulling nested `GateResult.recommended_command` for public outputs unless adding a typed authority field is clearly better and can be populated from the same public command model.
- Do not remove diagnostic messages, reason codes, failure classes, warning codes, or actionable high-level route context.
- Do not create a second display-command parsing convention.
- Keep schema/tests honest if serialized output shape changes.

### Done when

- Publicly serialized nested gate/preflight outputs no longer contain executable-looking `recommended_command` values without typed authority.
- Runtime methods no longer inject command-shaped strings into nested gates unless the same object has a typed authority field.
- A test recursively inspects public status/operator JSON or relevant runtime output and fails if nested gate/preflight objects expose display-only command strings without typed authority.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `src/execution/status.rs`
- `src/execution/state/runtime_methods.rs`
- `src/workflow/operator.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-operator.schema.json`
- `tests/workflow_runtime.rs`
- `tests/runtime_authority_contracts.rs`
- Node schema/doc tests if JSON schema changes.

### Detailed Implementation Steps

1. Inspect `GateResult` serialization and all assignments to `recommended_command`.
2. Decide whether to remove the serialized nested field, always serialize it as absent/null for gate outputs, or add a typed authority field. Prefer the smallest coherent public contract: no nested executable-looking field when top-level route already owns execution.
3. Update gate builders in `runtime_methods.rs` and any other gate constructors so they no longer populate command-shaped display strings into `GateResult`.
4. If schema files describe nested `recommended_command`, update schemas to match the chosen contract.
5. Add recursive output coverage that walks public JSON and rejects nested `recommended_command` under `preflight`, `gate_review`, or `gate_finish` when no typed authority field is present on the same object.
6. Preserve tests that assert top-level `recommended_public_command_argv` is authoritative.

### Validation Expectations

- Targeted: `cargo test --test workflow_runtime nested -- --nocapture` or the exact new test filter.
- Targeted if schemas/docs changed: `node --test tests/codex-runtime/*.test.mjs`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 2 after full validation.

## Task 3 - Make Test-Plan Refresh A Single Diagnostic Handoff

### Spec Coverage

- `test_plan_refresh_required` should not present multiple next actions in one public next-step field.

### Goal

Rewrite the `test_plan_refresh_required` lane so public output gives one handoff action and stops there. The follow-up re-query belongs after the handoff completes, not in the same actionable field.

### Context

Operator output and the review-state reference currently tell agents to use `featureforge:plan-eng-review`, then rerun operator JSON, then follow typed route law. That is accurate in broad workflow terms but gives multiple actions in one place and can make agents treat a skill handoff as runtime command execution.

### Constraints

- Preserve the fact that `test_plan_refresh_required` is a plan-engineering-review handoff, not a runtime mutation command.
- Do not invent typed runtime argv for a lane that does not have one.
- Do not remove reason code, phase, or diagnostic context.
- Keep wording consistent between operator output, docs, and skill guidance.

### Done when

- Operator `next_step` for `test_plan_refresh_required` names one action: route to plan engineering review for current-branch test-plan refresh.
- Reference docs describe the re-query as resuming after the handoff, not as part of the same next-step command.
- Existing tests/goldens updated only for the intended wording change.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `src/workflow/operator.rs`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `skills/finishing-a-development-branch/SKILL.md.tmpl`
- generated `skills/finishing-a-development-branch/SKILL.md`
- relevant workflow/operator output tests.

### Detailed Implementation Steps

1. Inspect the operator branch that constructs the `test_plan_refresh_required` next-step text.
2. Replace the multi-action text with a single handoff sentence.
3. Update the review-state reference table and late-stage narrative so the first action is the handoff; mention re-query only as what happens after that review completes.
4. Review `finishing-a-development-branch` wording for the same lane and keep it concise.
5. Regenerate generated skill docs if template text changes.
6. Update tests that assert exact guidance strings.

### Validation Expectations

- Targeted: `cargo test --test workflow_runtime test_plan_refresh_required -- --nocapture` or the exact relevant test filter.
- Targeted: `node scripts/gen-skill-docs.mjs --check`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 3 after full validation.

## Task 4 - Select Persisted Execution-Reentry Inside Route Planning

### Spec Coverage

- Persisted execution-reentry follow-up must not be applied as a post-selection command rewrite.

### Goal

Move persisted execution-reentry fallback into the route-planning selection path so the selected route is already the final mutation route before presentation finalization runs.

### Context

`route_decision_and_status_from_runtime_state_with_inputs` currently selects a route and then `apply_route_planning_fact_overrides` can replace that route with persisted execution reentry. The test suite currently blesses a first selected non-reopen route followed by an override to a reopen route. That preserves behavior but keeps split command decisioning alive.

### Constraints

- Preserve precedence: an already-selected legal `begin` or `reopen` route must continue to win over persisted fallback.
- Preserve the public reason code `persisted_execution_reentry_follow_up`.
- Preserve the generated public `reopen` argv/template surfaces when persisted fallback is selected.
- Do not move decisioning into workflow presentation, status projection, or command mutation modules.
- Finalization may adjust presentation fields but must not change the command kind, command args, phase, or next action for this fallback.

### Done when

- Persisted execution-reentry fallback is evaluated as part of route selection, not by a post-selection route rewrite helper.
- `apply_route_planning_fact_overrides` no longer changes an unrelated selected route into persisted execution reentry.
- Tests assert persisted fallback is returned by the route-selection path and remains stable through finalization.
- The runtime module-boundary reference accurately describes the new flow.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `src/execution/route_plan.rs`
- `src/execution/route_plan/unit_tests.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

### Detailed Implementation Steps

1. Inspect `select_runtime_route_decision`, `apply_route_planning_fact_overrides`, `bind_persisted_execution_reentry_fallback`, and persisted fallback tests.
2. Refactor selection so persisted fallback is chosen by a named selection helper or candidate branch before route finalization.
3. Keep explicit begin/reopen routes higher precedence than persisted fallback by checking the selected candidate before falling back.
4. Remove or narrow any post-selection override helper so it cannot change command identity after selection.
5. Update the unit test that currently asserts the first selected route is non-reopen; it should instead assert route selection returns the persisted fallback route directly and finalization does not change it.
6. Update the module-boundary reference if it mentions route override/finalization responsibilities.

### Validation Expectations

- Targeted: `cargo test persisted_execution_reentry -- --nocapture`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 4 after full validation.

## Task 5 - Reduce Signal-To-Noise Drift In Boundary Tests And Skill Wording

### Spec Coverage

- Static boundary assertions must not overclaim semantic coverage they do not provide.
- Plan-engineering-review guidance must use the current plan-fidelity binding/spec fingerprint language.

### Goal

Clean up the remaining lower-priority signal-to-noise findings by replacing brittle or misleading assertions with precise coverage and updating skill wording so it does not imply raw fingerprint behavior.

### Context

One module-boundary test says `route_plan.rs` must not hand-build route decisions while only scanning for one struct literal spelling. Separately, `plan-eng-review` correctly describes approval-stable binding earlier, but still says engineering approval requires pass fidelity for the current plan/spec fingerprint.

### Constraints

- Do not add a broader scanner unless it checks exactly the claim it makes.
- Prefer a behavioral invariant for route selection/finalization if it already exists or can be expressed simply.
- If the existing scanner remains, narrow its test name and failure message to the exact text-shape claim it actually checks.
- Keep skill wording concise; do not duplicate the operator-route-authority reference.
- Regenerate generated docs from templates.

### Done when

- `tests/runtime_module_boundaries.rs` no longer claims to block all route-decision construction while missing alias-based construction.
- Either a behavioral route-selection invariant covers the actual boundary, or the static assertion is narrowed to an honest, lower-risk claim.
- `skills/plan-eng-review/SKILL.md.tmpl` says `plan_fidelity_review.state == pass` for the current plan-fidelity binding/spec fingerprint, and generated `SKILL.md` matches.
- Prompt budget and generated-doc checks pass.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `tests/runtime_module_boundaries.rs`
- `skills/plan-eng-review/SKILL.md.tmpl`
- `skills/plan-eng-review/SKILL.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`

### Detailed Implementation Steps

1. Inspect the boundary test around the route-plan decision-construction assertion.
2. Either replace it with a behavioral invariant around route selection/finalization or adjust the assertion name/message to match the exact struct-literal ownership it checks.
3. Update `plan-eng-review` template wording to `current plan-fidelity binding/spec fingerprint`.
4. Regenerate generated skill docs.
5. Update Node contract tests only if they assert the old wording.
6. Run doc-generation and prompt-budget checks.

### Validation Expectations

- Targeted: `cargo test --test runtime_module_boundaries route_plan -- --nocapture` or the exact updated test filter.
- Targeted: `node scripts/gen-skill-docs.mjs --check`.
- Targeted: `node --test tests/codex-runtime/*.test.mjs`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 5 after full validation.

# Final Audit Loop

After all tasks pass their task-local validation and clean-context reviews:

1. Run `cargo clean` before starting the next audit iteration.
2. Repeat the original deep audit process with independent clean-context subagents A through H plus the signal-to-noise subagent.
3. Attempt the required Node/generated-doc checks, strict clippy, targeted nextest audits, liveness model checker, and full no-fail-fast nextest as required by the current loop rules.
4. If actionable audit findings remain, produce the next focused plan and implement it task-by-task using the same loop.
5. Stop only when the audit finds no actionable issues and validation is clean.
