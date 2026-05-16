# Runtime Signal/Noise Eighteenth Audit Remediation Plan

## Workflow State

Engineering Approved: yes

Source Audit: `docs/featureforge/archive/runtime-safety-audit-history/2026-05-11-eighteenth-audit-report.md`

Execution State: implementation authorized by user request to continue the audit -> implementation loop until no actionable audit issues remain.

## Plan Revision

Revision: 1

Date: 2026-05-11

## Execution Mode

Task-order implementation.

After each task:
- run `cargo clippy --all-targets --all-features -- -D warnings`;
- run full `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`;
- if the full nextest run takes more than 4 to 5 minutes, run `cargo clean`, rerun the full suite once, and if it still exceeds 4 to 5 minutes stop and address performance before continuing;
- dispatch a clean-context reviewer against the exact task;
- do not allow review subagents to spawn subagents;
- remediate and repeat validation/review until no task findings remain.

After all tasks:
- run the full validation set again;
- dispatch a clean-context whole-plan review against this plan;
- remediate and repeat validation/review until no findings remain;
- run a new full audit iteration, starting with `cargo clean`.

## Goal

Reduce the remaining runtime safety risk and self-referential churn from the eighteenth audit by centralizing decisioning, removing duplicate projection paths, making public-flow tests prove shipped behavior where they claim to, and keeping prompt/skill law high-signal and packaged.

The goal is not more guardrails around duplicate behavior. The goal is to delete or centralize duplicate behavior so public runtime decisions have one owner.

## Architecture

Target ownership:
- `route_plan` owns public route decisions and status-route projection.
- `router` returns the finalized `RouteDecision` and finalized `PlanExecutionStatus` projection for consumers.
- `read_model/public_route_projection` installs the finalized projection and applies only read-model-specific additions that are not route decisions.
- stale-target selection and targetless stale reconcile use one authoritative decision object derived from reducer/gate-snapshot state, not projected status fields.
- command modules construct mutation requests through shared typed public-command/request constructors.
- public-flow tests use compiled CLI when asserting shipped behavior, and internal semantic tests are labeled/quarantined as internal.
- skills and prompt tests enforce the executable contract without repeating low-value prose.

## Change Surface

Expected files:
- `src/execution/router.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/reentry_reconcile.rs`
- `src/execution/status_assembly.rs`
- `src/execution/invariants.rs`
- `src/execution/next_action.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/next_action_route.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/commands/begin.rs`
- `src/execution/commands/complete.rs`
- `src/execution/commands/reopen.rs`
- `src/execution/commands/transfer.rs`
- `src/execution/commands/advance_late_stage.rs`
- `src/execution/state/command_requests.rs`
- `src/cli/plan_execution.rs`
- `src/workflow/operator.rs`
- `src/workflow/status.rs`
- `scripts/gen-skill-docs.mjs`
- `scripts/gen-agent-docs.mjs`
- `skills/**/*.tmpl`
- generated `skills/**/SKILL.md`
- `agents/**/*.instructions.md`
- `.codex/**`
- `references/operator-route-authority.md`
- `README.md`
- `RELEASE-NOTES.md`
- `docs/testing.md` only if the required review-note rule needs clarification
- `tests/support/public_flow_scan.rs`
- `tests/workflow_runtime.rs`
- `tests/execution_query.rs`
- `tests/liveness_model_checker.rs`
- `tests/plan_execution_final_review.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/codex-runtime/*.test.mjs`
- targeted shell/public replay tests as needed

## Preconditions

- Do not use FeatureForge runtime/project skills.
- Use the Rust guidance skill only when editing Rust code.
- Do not modify historical plans/specs except the new audit/remediation artifacts in this plan.
- Preserve event-log authority and public workflow routing.
- Keep generated skills generated from templates.
- Do not introduce `#[allow(clippy::...)]` or weaken lint policy.
- Do not introduce runtime/env recursion prevention for reviewers.

## Known Footguns / Constraints

- Do not solve split decisioning with more scanners alone.
- Do not move mandatory route law solely into companion docs.
- Treat non-empty `recommended_public_command_argv` as exact machine invocation authority; `recommended_command` is display-only compatibility text and must never be parsed or executed.
- Do not let read models append events or mutators write projections directly.
- Do not let public-flow tests silently use direct runtime helpers for shipped CLI behavior.
- Do not add exact-prose prompt tests when a negative scanner or semantic field check is sufficient.
- Do not leave generated `SKILL.md` or agent docs stale after template/generator edits.
- Do not accept test-suite performance regression. The full nextest suite must remain comfortably under the 4 to 5 minute threshold after a clean rerun.

## Requirement Coverage Matrix

| Requirement | Task |
|---|---|
| Router/read-model projection is single-sourced | Task 1 |
| Stale-target authority is single-sourced | Task 1 |
| Route candidate/route rewrite decisioning is centralized | Task 2 |
| Public mutation request construction is shared | Task 2 |
| Public-flow tests prove shipped behavior where claimed | Task 3 |
| Internal semantic tests are labeled and quarantined | Task 3 |
| Boundary tests enforce architecture without private-shape churn | Task 3 |
| Skills stay high-signal and route law remains packaged | Task 4 |
| Agent-facing errors point to one public next step | Task 4 |
| Prompt budget review trail is current | Task 4 |
| Generated docs and agents are fresh | Task 4 |

## Task 1 - Single-Source Runtime Projection And Stale-Target Authority

### Spec Coverage

Addresses audit findings H1 and H3:
- stale-target authority split between `stale_target_projection` and `reentry_reconcile`;
- read model replaying finalized route/status projection.

### Goal

Make router-finalized route/status projection the single status-route output consumed by read models, and make targetless stale reconcile depend on one authoritative stale-target decision object instead of projected status inspection.

### Context

Current shape:
- `src/execution/router.rs` computes a finalized route/status projection.
- `src/execution/read_model/public_route_projection.rs` replays common route/status projection, stale closure projection, and blocking record projection.
- `src/execution/stale_target_projection.rs` uses gate-snapshot authority for targetless stale reconcile.
- `src/execution/reentry_reconcile.rs` has `status_needs_marker_for_status` and `status_has_bound_stale_target`, which recompute stale binding from projected status fields.

### Constraints

- `read_model/public_route_projection` must not become a second router.
- Do not remove diagnostic projection fields.
- Do not weaken invariant checks; update them to consume shared authority.
- Preserve public JSON output unless duplicated projection was producing unintended drift.

### Done When

- `RuntimeRoutingProjection` includes the finalized `PlanExecutionStatus` route projection or an equivalent finalized status projection object.
- `read_model/public_route_projection` installs/uses the finalized projection instead of recalculating route/status decisions.
- `read_model/public_route_projection` applies only read-model-only additions with a nearby comment documenting why they are not route decisions.
- Targetless stale reconcile uses one shared authority object or helper derived from reducer/gate-snapshot state.
- `reentry_reconcile` no longer decides bound stale target by inspecting projected status fields for production routing/invariants.
- Boundary tests catch attempts to reintroduce projected-status stale authority or read-model route replay.

### Files

- `src/execution/router.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/reentry_reconcile.rs`
- `src/execution/status_assembly.rs`
- `src/execution/invariants.rs`
- `src/execution/route_plan/status_projection.rs`
- `tests/runtime_module_boundaries.rs`
- targeted runtime tests covering stale targetless reconcile and projection parity

### Implementation Steps

1. Inspect `RuntimeRoutingProjection` and all call sites.
2. Add a finalized status projection field to `RuntimeRoutingProjection`, or introduce a small named type if ownership is clearer.
3. Update `project_final_runtime_routing_projection` to return the same finalized `PlanExecutionStatus` it already computes after route decision finalization.
4. Update `read_model/public_route_projection` to consume that finalized status projection rather than calling `project_routing_decision_onto_status`, `project_stale_unreviewed_closures`, or route-blocking helpers itself.
5. Keep only read-model-only additions in `read_model/public_route_projection`, such as read-model workspace identity details or exact-command diagnostic flags, if those are not route decisions.
6. Extract targetless stale binding authority into a shared object/helper with explicit inputs from reducer/gate-snapshot authority.
7. Replace `status_needs_marker_for_status` production callers with the shared authority helper.
8. Remove or demote projected-status stale binding helpers to test-only/debug use if still needed; otherwise delete them.
9. Update invariants to verify consistency against the shared authority rather than recomputing status-field truth.
10. Add or update boundary tests so read-model projection cannot call route-plan status projection helpers and stale targetless reconcile cannot use projected public repair target fields as authority.
11. Run formatting and targeted tests for projection, stale target, public replay, and boundary contracts before full validation.

### Validation Expectations

Minimum targeted checks before full validation:
- `cargo test --test runtime_module_boundaries`
- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test execution_query`
- any targeted stale/reentry replay test touched by the implementation

Full gate:
- strict clippy;
- full nextest no-fail-fast;
- clean-context task review with no findings.

## Task 2 - Centralize Route Candidate Finalization And Public Mutation Requests

### Spec Coverage

Addresses audit findings H2 and M2:
- route decision ordering split between `next_action` and `route_plan`;
- `next_action_route` re-deciding/rebinding routes after shared next-action output;
- command modules duplicating public mutation request construction.

### Goal

Make `next_action` produce semantic candidate facts and make `route_plan` the place that finalizes public routes, route overrides, and command bindings. Make command modules build mutation requests through one shared typed public-command/request path.

### Context

Current shape:
- `route_plan.rs` has route ordering before delegation.
- `next_action.rs` has ordered route selection and earliest stale boundary selection.
- `route_plan/next_action_route.rs` rewrites phase detail, command, recording context, and execution command context after next-action returns.
- `PublicCommand::to_mutation_request` exists, but command modules still build `PublicMutationRequest` manually.

### Constraints

- Do not remove next-action semantic facts if they are needed for diagnostics.
- Do not make `next_action` depend on CLI strings.
- Preserve typed public argv/template behavior.
- Keep command modules readable; shared request construction should reduce duplicate fields without hiding command-specific validation.

### Done When

- Earliest stale-boundary selection is owned by a shared helper/type, not local ad hoc logic in `next_action`.
- `next_action` no longer finalizes public route ordering independently from `route_plan`.
- `route_plan/next_action_route` uses an explicit route-finalization or route-override object for any required final binding.
- Completed-task closure preemption is owned in one place.
- Begin, complete, reopen, transfer, and advance-late-stage command modules build public mutation requests through shared typed request constructors.
- Boundary tests guard against manual `PublicMutationRequest { ... }` construction in public command modules except for documented internal tests.

### Files

- `src/execution/next_action.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/next_action_route.rs`
- `src/execution/stale_target_selection.rs` if present, or a new focused helper module
- `src/execution/command_eligibility.rs`
- `src/execution/commands/begin.rs`
- `src/execution/commands/complete.rs`
- `src/execution/commands/reopen.rs`
- `src/execution/commands/transfer.rs`
- `src/execution/commands/advance_late_stage.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- public replay/golden tests as needed

### Implementation Steps

1. Map every route-finalization branch in `next_action.rs`, `route_plan.rs`, and `route_plan/next_action_route.rs`.
2. Separate semantic facts from public route finalization. Keep facts in `next_action`; move route ordering/final binding to `route_plan`.
3. Create or extend a shared stale-boundary selection helper so both baseline reentry and authoritative stale candidates are ordered once.
4. Replace local earliest-stale-boundary logic in `next_action.rs` with that helper.
5. Replace ad hoc route rewrites in `route_plan/next_action_route.rs` with explicit named route override/finalization cases owned by route-plan.
6. Remove duplicate completed-task closure preemption from the non-owning layer.
7. Add a shared public mutation request constructor that takes the typed public command plus command inputs and returns the correct request.
8. Update begin, complete, reopen, transfer, and advance-late-stage modules to call the shared constructor before `decide_public_mutation`.
9. Keep command-specific validation near the command, but do not duplicate request field population.
10. Add boundary tests that reject new manual public request construction in command modules and reject route-ordering logic outside the owning module.
11. Update any expected goldens if public JSON changes intentionally; keep changes limited to externally meaningful shape.

### Validation Expectations

Minimum targeted checks before full validation:
- `cargo test --test runtime_module_boundaries`
- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test workflow_entry_shell_smoke`
- `cargo test --test liveness_model_checker`

Full gate:
- strict clippy;
- full nextest no-fail-fast;
- clean-context task review with no findings.

## Task 3 - Test Realism And Signal/Noise Cleanup

### Spec Coverage

Addresses audit findings M1, L3, and L4:
- public-flow test guard misses direct runtime query APIs;
- liveness/final-review tests need clearer internal-vs-public labeling;
- boundary tests and prompt tests over-pin private implementation and exact positive prose.

### Goal

Keep high-value public/runtime coverage while removing brittle private-shape and exact-prose pins. Public-flow tests should use the shipped CLI when they claim shipped behavior, and internal semantic tests should be labeled as internal.

### Context

Current shape:
- `tests/support/public_flow_scan.rs` protects public-flow files but does not flag `query_workflow_routing_state_for_runtime`.
- `tests/workflow_runtime.rs` and `tests/execution_query.rs` use direct query APIs in protected public-flow files.
- `tests/liveness_model_checker.rs` is mostly an internal semantic model checker with one compiled-CLI parity edge.
- `tests/runtime_module_boundaries.rs` contains exact helper names, exact field assignments, and exact snippets.
- Node prompt tests contain exact positive prose checks that duplicate the prompt source.

### Constraints

- Do not weaken hidden-helper or hidden-command scanners.
- Do not remove public CLI/golden coverage.
- Do not make the liveness matrix slow by converting every edge to a subprocess.
- Do not add more static tests unless they replace a more brittle one or guard a real public/private boundary.

### Done When

- Static guard catches direct production query APIs in protected public-flow tests.
- Any remaining direct runtime query assertions are moved to internal tests or clearly labeled as internal boundary/semantic tests.
- Public behavior claims in protected public-flow files use compiled CLI or the established public CLI helper.
- `tests/liveness_model_checker.rs` clearly documents internal semantic coverage and retains targeted compiled-CLI parity without over-expanding subprocess cost.
- `tests/runtime_module_boundaries.rs` enforces module ownership/import boundaries without pinning private helper names and implementation snippets that are not part of the contract.
- Node prompt tests keep negative scanners and essential executable-route checks, but stop duplicating exact positive prose from the generated docs.

### Files

- `tests/support/public_flow_scan.rs`
- `tests/workflow_runtime.rs`
- `tests/execution_query.rs`
- `tests/liveness_model_checker.rs`
- `tests/plan_execution_final_review.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- `docs/runtime-architecture.md`

### Implementation Steps

1. Add `query_workflow_routing_state_for_runtime` and similar direct runtime query APIs to the public-flow direct-runtime marker list.
2. Run the guard test to identify protected tests that now fail.
3. For each failure, choose one of two outcomes:
   - if the test asserts shipped public behavior, convert it to compiled CLI/public helper output;
   - if the test asserts internal semantic behavior, move it to an internal test file or add an explicit internal classification that the scanner respects.
4. Update liveness documentation at the top of `tests/liveness_model_checker.rs` to state it is an internal semantic/liveness model plus targeted compiled-CLI parity, not full shipped-CLI proof.
5. Add or preserve a small compiled-CLI parity sample for liveness routes that protects the parser/runtime boundary without invoking a subprocess for the full matrix.
6. Review `tests/runtime_module_boundaries.rs` and replace private helper-name/field-assignment positive pins with owner/import-boundary and behavior-oriented checks.
7. Update `docs/runtime-architecture.md` so its claims about boundary tests match the actual test strategy.
8. Relax Node exact positive prose tests to check core requirements: typed argv/template law exists, display command is non-authoritative, hidden-helper guidance is absent, and companion links resolve.
9. Keep negative scanners for bad guidance such as hidden commands, legacy proof reconstruction, manual artifact repair, and display-command execution.

### Validation Expectations

Minimum targeted checks before full validation:
- `cargo test --test public_flow_scan_contracts`
- `cargo test --test public_cli_flow_contracts`
- `cargo test --test rust_source_scan_contracts`
- `cargo test --test runtime_module_boundaries`
- `cargo test --test liveness_model_checker`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/gen-skill-docs.unit.test.mjs`

Full gate:
- strict clippy;
- full nextest no-fail-fast;
- clean-context task review with no findings.

## Task 4 - Prompt Packaging, Agent UX, And Public Error Text

### Spec Coverage

Addresses audit findings M3, M4, M5, L1, and L2:
- route-authority companion reference packaging;
- missing prompt-budget review note;
- standalone reviewer agent root discovery;
- transfer/plan-override/help text that can lead agents toward manual repair or low-level recording;
- generated docs overemphasizing `_featureforge_exec_public_argv`.

### Goal

Keep the prompt surface high-signal and actionable. Route law should stay top-level and companion-backed, generated artifacts should package their references, and public errors should point to one public next step rather than manual artifact reconstruction.

### Context

Current shape:
- `references/operator-route-authority.md` exists and is linked by generated skills/README but was untracked in the audit tree.
- `skills/skill-doc-budgets.json` enforces a tighter budget, but `RELEASE-NOTES.md` lacks the required current review note.
- standalone reviewer agent docs use `$_REPO_ROOT`/`$_FEATUREFORGE_ROOT` without root discovery.
- `plan execution transfer` legacy-shape errors list repair flags without saying they must come from a route-authorized argv/template.
- workflow plan override failures are fail-closed but not sufficiently actionable.
- public help describes normal commands as "recorders".
- generated docs/tests make `_featureforge_exec_public_argv` look like the main execution path.

### Constraints

- Edit `.tmpl` sources before regenerating generated `SKILL.md` outputs.
- Do not expand every skill with repeated negative law; centralize companion law and keep top-level mandatory law concise.
- Do not introduce runtime recursion enforcement.
- Do not make docs tell agents to parse display strings.

### Done When

- `references/operator-route-authority.md` remains present and linked; any packaging/generation check that can be automated without relying on git staging is added or updated.
- `RELEASE-NOTES.md` contains a current prompt-budget review note for the 5150 budget enforcement change.
- standalone reviewer agent instructions include root-discovery guidance or avoid unresolved `$_REPO_ROOT`/`$_FEATUREFORGE_ROOT` assumptions.
- transfer legacy-shape errors say compatibility-only route-authorized shape, and direct agents to copy from `recommended_public_command_argv` or a bound template.
- plan override failures state the safe next step: use an existing repo-relative approved plan path or return to the normal planning/review handoff.
- public CLI help uses intent-level workflow language instead of "recorder" wording for normal commands.
- generated route-authority docs make typed argv plus installed binary the mental model; `_featureforge_exec_public_argv` is described only as a generated wrapper when present.
- generated docs are fresh and prompt budget tests pass.

### Files

- `references/operator-route-authority.md`
- `README.md`
- `RELEASE-NOTES.md`
- `scripts/gen-skill-docs.mjs`
- `scripts/gen-agent-docs.mjs`
- `skills/**/*.tmpl`
- `skills/**/SKILL.md`
- `agents/**/*.instructions.md`
- `.codex/INSTALL.md`
- `.copilot/**` if generated reviewer docs are mirrored there
- `src/execution/state/command_requests.rs`
- `src/cli/plan_execution.rs`
- `src/workflow/operator.rs`
- `src/workflow/status.rs`
- `tests/codex-runtime/*.test.mjs`
- `tests/internal_bootstrap_smoke.rs`
- `tests/workflow_runtime.rs`

### Implementation Steps

1. Ensure `references/operator-route-authority.md` is treated as a source artifact in the working tree and that generated docs link to it consistently.
2. Add or adjust a Node contract that every generated skill companion reference resolves from the repo root and is reachable from the generated install surface. Avoid tests that depend on local git staging state.
3. Add the required prompt-budget review note to the current Unreleased section of `RELEASE-NOTES.md`.
4. Update reviewer agent source/generator so standalone reviewer docs define how to resolve the repo root and FeatureForge install root before referencing companion files.
5. Update generated reviewer docs by running the agent-doc generator.
6. Update `src/execution/state/command_requests.rs` transfer error messages to say the repair-step shape is compatibility-only and route-authorized; agents must copy it from `recommended_public_command_argv` or bind the route template.
7. Update `src/workflow/operator.rs` and `src/workflow/status.rs` plan override failures to provide one safe next step.
8. Update `src/cli/plan_execution.rs` command summaries from low-level "record" phrasing to intent-level public workflow phrasing.
9. Update bootstrap/help tests for the new public wording.
10. Update route-authority generated text so `_featureforge_exec_public_argv` is described as a generated wrapper, not the core execution model.
11. Regenerate skill and agent docs.
12. Run Node doc tests and targeted public-output tests before full validation.

### Validation Expectations

Minimum targeted checks before full validation:
- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo test --test internal_bootstrap_smoke`
- `cargo nextest run --test workflow_runtime`
- any targeted public-output/schema tests touched by wording changes

Full gate:
- strict clippy;
- full nextest no-fail-fast;
- clean-context task review with no findings.

## Final Whole-Plan Review

After Tasks 1 through 4 are complete and each task review is clean:

1. Run:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
   - `cargo test --test liveness_model_checker`
   - `git diff --check`
2. If full nextest exceeds 4 to 5 minutes, follow the required clean/rerun/performance-investigation rule.
3. Dispatch a clean-context reviewer against the whole plan and the complete diff.
4. Remediate, revalidate, and rereview until no findings remain.
5. Start the next audit iteration with `cargo clean` and the full audit subagent set, including the signal/noise auditor.
