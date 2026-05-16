# FeatureForge Runtime Route-Plan And Diagnostics Ninth-Audit Remediation

## Workflow State

Engineering Approved

## Plan Revision

1

## Execution Mode

Implementation

## Goal

Remediate the actionable findings from the post-implementation A-H audit pass:

- stop public/runtime diagnostics from suggesting retry loops or low-level escape hatches when no typed public route exists
- remove the `current_truth -> reducer::RuntimeState` reverse dependency
- make public route selection ownership explicit so `next_action`, `public_route_selection`, and `router` cannot independently decide the next public command
- remove production parent-glob imports from state/command support modules and enforce that boundary

The end state is one where runtime errors and prompts tell agents to stop on diagnostics when public argv/template authority is absent, semantic truth modules do not depend upward on reducer aggregates, route planning has one owner for public route ordering, and extracted modules expose their real dependencies.

## Architecture

The runtime keeps the existing authoritative flow:

CLI args -> command module -> transition guard -> event append -> reducer -> route-plan selection -> read/status projection -> workflow operator presentation.

This plan tightens the boundaries:

- `command_eligibility` remains the mutation gate, but its public failure text must distinguish recoverable route guidance from `blocked_runtime_bug` / `runtime diagnostic required` stop states.
- `current_truth` owns semantic currentness and follow-up derivation from explicit inputs, not reducer-owned `RuntimeState`.
- a route-plan owner, under `src/execution`, owns public route ordering and pre/post override decisions; `router` should project route decisions into DTO/read-model fields instead of locally owning the same ordering logic.
- `state.rs` and `commands/common.rs` remain facades; production child modules must import explicit dependencies instead of `use super::*`.
- prompt templates remain route-law surfaces and must not point agents toward low-level primitives or generic compatibility escape hatches.

## Change Surface

- `src/execution/command_eligibility.rs`
- `src/execution/current_truth.rs`
- `src/execution/router.rs`
- possible new `src/execution/route_plan.rs`
- `src/execution/mod.rs`
- `src/execution/state/**`
- `src/execution/commands/common/**`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/using-featureforge/SKILL.md.tmpl`
- generated `skills/**/SKILL.md`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- focused runtime/public-flow tests as needed

## Preconditions

- Do not use FeatureForge runtime skills or project skills.
- Do not weaken runtime gates or hide diagnostics by deleting state.
- Preserve existing public CLI surface and schema compatibility.
- Generated skill docs must be regenerated from templates.
- Historical docs may remain historical, but active docs/prompts must not teach stale helpers as normal flow.

## Known Footguns / Constraints

- `recommended_command` is display-only compatibility text. Do not add any new code or prompt text that treats it as executable authority.
- `blocked_runtime_bug` and `runtime diagnostic required` must not offer retry-oriented mutation guidance.
- `repair-review-state`, `close-current-task`, and `advance-late-stage` are valid public aggregate commands only when routed by typed argv/template authority.
- Do not replace split decisioning with a new catch-all module. Route-plan ownership must be explicit and guarded.
- `use super::*` remains acceptable in `#[cfg(test)] mod tests`; this plan targets production modules.
- `read_model_support.rs` is now a compatibility re-export over lower support helpers. Do not add new lower-layer dependencies on that compatibility module.

## Requirement Coverage Matrix

| Requirement | Task 1 | Task 2 | Task 3 | Task 4 | Task 5 |
| --- | --- | --- | --- | --- | --- |
| Public diagnostics stop on runtime bugs instead of looping | x |  |  |  | x |
| Public prompts remove low-level primitive escape hatches | x |  |  |  | x |
| Internal projection diagnostics do not teach progress repair via materialization | x |  |  |  | x |
| `current_truth` does not depend on reducer aggregates |  | x |  |  | x |
| Route ordering has one owner |  |  | x |  | x |
| Router/status projection consumes route-plan output |  |  | x |  | x |
| Production child modules expose explicit imports |  |  |  | x | x |
| Boundary tests catch regressions | x | x | x | x | x |
| Full validation and clean-context review loop |  |  |  |  | x |

## Tasks

### Task 1: Make Diagnostics And Prompts Stop Instead Of Looping

#### Spec Coverage

- Public-output H-P1: `blocked_runtime_bug` and `runtime diagnostic required` must be diagnostic-only.
- Public-output H-P2: public skills must not give agents generic low-level primitive escape hatches.
- Public-output H-P3: internal projection-only diagnostics must not imply projection materialization is progress repair.

#### Goal

Remove retry/escape-hatch wording from public failure messages and generated prompts, while preserving actionable typed-route guidance for recoverable states.

#### Context

The eighth remediation cleaned normal typed argv/template guidance, but the ninth audit found three remaining traps:

- `public_mutation_failure_route_guidance` falls back to “query status/operator JSON ... before retrying mutation” even for `blocked_runtime_bug`.
- execution skills still say “low-level primitives remain expert/debug-only surfaces,” which can send agents searching for hidden helpers.
- The legacy evidence-rebuild projection diagnostic tells callers to run `materialize-projections` or replay stale execution directly.

#### Constraints

- Keep `blocked_runtime_bug` route surfaces without public argv/template/required inputs.
- Do not globally ban the valid public aggregate command names.
- Do not remove internal compatibility tests; update expected diagnostics to match safer wording.
- Edit `.tmpl` skill sources and regenerate generated docs.

#### Done when

- Mutation rejection text for blocked runtime bugs says to stop on the runtime diagnostic and does not say to retry mutation.
- Active public skills say to stop/report diagnostics when no typed public argv/template exists; they do not invite low-level primitive fallback.
- Projection-only rebuild diagnostics say materialization is explicit projection export only and not normal progress repair.
- Static tests reject the old escape-hatch and retry text.

#### Files

- `src/execution/command_eligibility.rs`
- `src/execution/commands/common/mutation_guards.rs`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/using-featureforge/SKILL.md.tmpl`
- generated `SKILL.md` files
- `tests/runtime_instruction_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/internal_plan_execution.rs`

#### Implementation Steps

1. Add diagnostic-stop detection to `public_mutation_failure_route_guidance` based on `status.state_kind == blocked_runtime_bug`, `status.next_action == runtime diagnostic required`, or no argv/template with diagnostic state.
2. Change blocked-runtime-bug rejection guidance so the final `JsonFailure` tells agents to stop and report the runtime diagnostic.
3. Update `mutation_guards` projection-only message to describe `materialize-projections` as explicit projection export only; remove replay/direct progress repair wording from that diagnostic.
4. Remove public-skill low-level primitive escape-hatch text from the three templates and replace it with typed-route stop/report wording.
5. Regenerate skill docs.
6. Add static tests that reject the old “retrying mutation” diagnostic for blocked runtime bugs, the low-level primitive escape hatch in active public skills, and the old projection-only rebuild wording.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- `cargo test --test runtime_instruction_contracts -- --nocapture`
- targeted `cargo test --test internal_plan_execution <updated rebuild diagnostic test> -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 2: Remove `current_truth -> reducer::RuntimeState`

#### Spec Coverage

- Modularization G-P1: semantic truth helpers must not depend upward on reducer aggregates.
- Runtime flow: reducer consumes truth helpers; truth helpers do not consume reducer state.

#### Goal

Change `current_truth::resolve_actionable_repair_follow_up` to accept an explicit current-truth input struct instead of `&RuntimeState`.

#### Context

`current_truth.rs` imports `crate::execution::reducer::RuntimeState` and accepts it in `resolve_actionable_repair_follow_up`. That creates a reverse edge from shared semantic truth back into the reducer layer.

#### Constraints

- Preserve follow-up behavior and route-decision hash binding.
- Avoid cloning large reducer data.
- Prefer borrowed explicit inputs over broad aggregate structs.
- Add a boundary test that rejects `current_truth -> reducer`.

#### Done when

- `current_truth.rs` no longer imports `crate::execution::reducer::RuntimeState`.
- Callers construct a small `CurrentTruthFollowUpInputs` or equivalent borrowed input object.
- Existing repair follow-up behavior is unchanged.
- Boundary tests reject future `current_truth` imports of reducer.

#### Files

- `src/execution/current_truth.rs`
- `src/execution/router.rs`
- possible `src/execution/runtime_truth.rs` or other callers
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Define a current-truth-owned input struct containing `status`, `gate_snapshot`, `semantic_workspace_tree_id`, `authoritative_state`, and `source_route_decision_hash`.
2. Replace the `RuntimeState`-accepting helper with an explicit-input helper.
3. Update router/reducer callers to pass borrowed fields from `RuntimeState`.
4. Remove the `RuntimeState` import from `current_truth.rs`.
5. Add a boundary test forbidding `crate::execution::reducer` imports from `current_truth.rs`.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test runtime_authority_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 3: Establish A Single Route-Plan Owner

#### Spec Coverage

- Modularization G-P2: avoid split decisioning across `next_action`, `public_route_selection`, and `router`.
- Runtime flow: route selection occurs after reducer and before read/status/operator projection.

#### Goal

Introduce or complete a route-plan owner that owns public route ordering and override application. `router` should project the selected route into DTO/status fields instead of independently deciding pre-seed and post-seed route changes.

#### Context

The eighth remediation centralized several route facts, but the ninth audit still found public-route selection spread across:

- `next_action` ordered route passes
- `public_route_selection` seed mutation
- `router` pre-seed and post-seed overrides

This is improved from raw duplication, but agents remain exposed to future drift unless one module owns the route-plan assembly.

#### Constraints

- Do not change public route behavior.
- Do not create a broad new catch-all. The route-plan module should own route ordering and call focused decision helpers.
- `next_action` may remain a low-level scoring/pass helper only if route-plan owns its invocation and override order.
- `router` may keep DTO/status-blocker projection but should not own public route ordering.

#### Done when

- Public route selection entrypoint is named and documented as the route-plan owner.
- The route-plan owner applies pre-seed and post-seed overrides and returns a finalized `RouteDecision`.
- `router` calls the route-plan owner and projects route/status/blocker output.
- Boundary docs describe this ownership.
- Static tests reject route-ordering override blocks being reintroduced into router/read-model presentation modules.

#### Files

- `src/execution/router.rs`
- possible new `src/execution/route_plan.rs`
- `src/execution/public_route_selection.rs`
- `src/execution/next_action.rs`
- `src/execution/mod.rs`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Extract route-decision assembly from `router` into a route-plan owner, preserving existing helper calls and ordering.
2. Keep `next_action` and `public_route_selection` as subordinate helpers invoked by the route-plan owner, not sibling route owners.
3. Change router route projection functions to call the route-plan owner for `RouteDecision`.
4. Move or wrap pre-seed/post-seed override functions so they are no longer owned by `router`.
5. Update docs and static boundary tests to enforce the new dependency and ownership direction.
6. Run targeted route golden and authority tests before full validation.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test runtime_authority_contracts -- --nocapture`
- `cargo test --test runtime_behavior_golden -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 4: Remove Production Parent Globs From Extracted State And Command Support Modules

#### Spec Coverage

- Modularization G-P3: `state.rs` and command support facades must not hide child dependencies behind `use super::*`.
- Rust lint hygiene: production modules should use explicit imports; `use super::*` remains allowed in `#[cfg(test)]` modules.

#### Goal

Replace production parent-glob imports in extracted state and command-support modules with explicit imports, then enforce the boundary.

#### Context

The eighth remediation removed parent globs from a focused set of extracted modules, but the ninth audit found production child modules still using `use super::*`, including `state/worktree_lease_truth.rs`, `state/runtime_methods.rs`, and `state/artifact_finish_truth.rs`.

#### Constraints

- Preserve test-module `use super::*` where idiomatic.
- Keep changes mechanical; do not refactor behavior while replacing imports.
- Avoid over-broad crate imports that recreate the same hidden dependency problem.

#### Done when

- Production `src/execution/state/**` and `src/execution/commands/common/**` modules no longer use `use super::*`.
- Static test scans production source, skipping `#[cfg(test)]` modules, and rejects parent globs in extracted runtime modules.
- Clippy remains warning-free.

#### Files

- `src/execution/state/*.rs`
- `src/execution/commands/common/*.rs`
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Replace each production parent glob with explicit imports.
2. Prefer direct crate/module imports over facade imports when the dependency owner is known.
3. Add or widen a boundary test to reject production parent globs in extracted state and command common modules.
4. Run rustfmt and clippy to catch unused or missing imports.

#### Validation Expectations

- `cargo fmt --check`
- `cargo test --test runtime_module_boundaries focused_extracted_production_modules_do_not_use_parent_globs -- --nocapture`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 5: Full Validation, Clean Review, And Re-Audit Loop

#### Spec Coverage

- User-required implementation loop.
- Audit requirement: repeat A-H style audit until no actionable findings remain.

#### Goal

Prove the remediation is complete through full validation, clean-context review, and a fresh A-H audit pass.

#### Context

Earlier implementation tasks must be verified independently before claiming completion.

#### Constraints

- Do not dispatch reviewers before strict clippy and full nextest are clean.
- Reviewers must be clean-context and must not spawn subagents or invoke FeatureForge skills.
- If review or audit finds actionable issues, remediate and restart validation/review as required.

#### Done when

- Generated docs are fresh.
- Node codex-runtime tests pass.
- `cargo fmt --check` passes.
- strict clippy passes.
- full nextest no-fail-fast passes.
- standalone liveness model checker passes.
- clean-context whole-plan review has no actionable findings.
- fresh A-H audit has no actionable findings.

#### Files

- All files touched by Tasks 1-4.
- This plan file.

#### Implementation Steps

1. Run:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
   - `cargo fmt --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
   - `cargo test --test liveness_model_checker -- --nocapture`
2. Dispatch a clean-context reviewer against this plan and the diff.
3. Remediate any reviewer findings and rerun the full gate before rereview.
4. Run a fresh A-H audit pass using clean-context subagents.
5. If the fresh audit finds actionable issues, write the next plan and continue the loop.

#### Validation Expectations

- All listed commands pass.
- Clean-context review returns no actionable findings.
- Fresh A-H audit returns no actionable findings.
