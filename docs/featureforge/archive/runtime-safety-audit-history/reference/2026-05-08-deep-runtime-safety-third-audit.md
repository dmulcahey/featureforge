# FeatureForge deep runtime safety audit - third pass

## Executive verdict

**Ship candidate:** no.

**Recommendation:** ship only after targeted fixes.

The updated codebase is close, and most historical failure classes are now covered by public runtime routes, typed argv authority, event-log migration boundaries, projection/read-model separation, and prompt-surface budget tests. The third audit still found actionable issues that are small in count but directly tied to prior churn classes:

- public doctor output can show a normal-flow `next_step` beside `blocked_runtime_bug` diagnostics
- public `next_action` still uses the compound phrase `repair review state / reenter execution`
- stale-target bridge eligibility and stale-binding predicates still have duplicated semantic implementations
- `task_review_dispatch_required` still escapes the centralized phase-detail vocabulary

Those issues are not evidence that the branch is structurally unsafe overall, but they are enough to keep it out of "safe to ship" status because they can still make an agent choose a second manual action after the runtime has already selected the one public command.

## Audit method

Eight clean-context audit subagents inspected independent risk areas from commit `0cca79dd59054865a7c6a2d34bd90b21bb660673`. They were instructed not to run FeatureForge runtime/project skills, not to spawn subagents, not to edit files, and to ground findings in repository truth.

- A: public CLI and reachable runtime
- B: tests versus shipped-runtime realism
- C: receipt/provenance/evidence control plane
- D: plan-review and engineering-review workflow
- E: stale closure, cycle-break, and reentry loops
- F: prompt-surface and skill packaging
- G: modularization and split decisioning
- H: public-output and agent UX

## What is genuinely fixed

- Public CLI reachability is substantially fixed. `src/cli/plan_execution.rs` exposes the normal public commands, and `src/lib.rs` dispatches them through public command implementations.
- Typed command authority is the executable surface. `src/execution/command_eligibility.rs` models `PublicCommand`, and status/operator surfaces carry typed argv/template instead of relying on display strings.
- Public `begin` owns preflight setup in `src/execution/commands/begin.rs`.
- Public `close-current-task` refreshes stale dispatch lineage internally in `src/execution/commands/close_current_task.rs`.
- Public `advance-late-stage` owns final-review, release-readiness, QA, branch-closure, and finish progression in `src/execution/commands/advance_late_stage.rs`.
- Plan-fidelity no longer depends on hidden runtime receipt recording. `src/contracts/plan.rs` and `src/execution/topology.rs` derive fidelity from parseable review artifacts and fingerprints.
- Current task closure is authoritative for task-boundary truth. `src/execution/current_closure_projection.rs`, `src/execution/read_model.rs`, and `src/execution/stale_target_projection.rs` remove current closures from stale projections.
- Evidence/projection artifacts are no longer required for normal routing progress. `src/execution/commands/materialize_projections.rs` and `src/execution/projection_renderer.rs` keep projection materialization explicit.
- Public-flow tests use compiled CLI helpers and static guards quarantine internal direct helpers.
- Prompt-surface budgets are enforced, generated skills are fresh, companion references are packaged, and reviewer-recursion prevention is prompt text scoped to reviewer surfaces.
- `state.rs` and `mutate.rs` are no longer runtime monoliths; operator/status surfaces do not import mutation/write helpers.

## What remains risky

- `WorkflowDoctor` can derive `resolution.kind=runtime_diagnostic_required` while still filling `next_step` from phase routing. That creates a mixed message in a fail-closed `blocked_runtime_bug` state.
- `next_action` can still say `repair review state / reenter execution`, even though the only executable public argv is `plan execution repair-review-state`.
- Diagnostic text still says to "repair workflow routing" or rerun after "repairing runtime routing" without one public next step.
- Stale-target bridge eligibility is implemented in both `src/execution/repair_target_selection.rs` and `src/execution/repair_route_decision.rs`.
- `task_review_dispatch_required` is used as a phase-detail literal outside `src/execution/phase.rs`.
- `RuntimeGateSnapshot::has_authoritative_stale_binding` and `StaleTargetProjection::has_authoritative_stale_binding` duplicate the same predicate in `src/execution/stale_target_projection.rs`.

## Concrete dead ends still possible

- A doctor payload with `phase_detail=blocked_runtime_bug` can show `Next action: runtime diagnostic required` and `Next step: Return to the current execution flow...`. An agent can treat the second line as permission to resume implementation even though mutation eligibility has failed closed.
- A status/operator payload with `next_action=repair review state / reenter execution` can lead an agent to run the public repair command and then independently reenter execution instead of waiting for the next typed argv after repair.
- A stale-target bridge predicate drift can route one surface to close-current-task while another surface selects repair-review-state for the same authoritative stale target.

## Concrete churn sources still possible

- Compound display actions can turn one public mutation into a two-step manual workflow.
- Vague "repair routing" diagnostics can invite manual state edits or hidden-helper searches.
- Duplicated stale-target and stale-binding predicates can reintroduce divergent task-boundary freshness decisions after future refactors.
- Phase-detail literals outside `phase.rs` can bypass boundary tests that only know about registered phase-detail constants.

## Public/private test mismatch assessment

No actionable public/private mismatch was found. Public-flow tests now use `tests/support/public_featureforge_cli.rs` to execute the compiled binary. Internal helpers are quarantined through `tests/support/internal_runtime_direct.rs`, and `tests/public_cli_flow_contracts.rs` rejects internal helper imports, hidden command literals, hidden flags, and display-command execution in protected public-flow files.

Residual gap: `scripts/run-public-runtime-flow-tests.sh` is narrower than the full set of public shell smoke tests. This is acceptable as a narrow gate because full `nextest` covers the broader shell probes.

## Receipt/evidence/projection control-plane assessment

No actionable control-plane leak was found. Current task closure comes from authoritative transition state, stale projection removes current closure records, stale/missing projection artifacts are diagnostic-only after a current positive closure exists, and projection materialization remains explicit.

Residual gap: there is less direct replay coverage for deleting or tampering individual late-stage projection export files after authoritative late-stage closure. No code evidence showed those exports controlling routing.

## Prompt-surface and packaging assessment

No actionable prompt-surface finding was found. Budgets are enforced, generated skill docs are fresh, companion references are tested, mandatory law remains top-level, and reviewer recursion prevention is prompt text only in reviewer prompts/agents.

## Modularization and split-decisioning assessment

Actionable split-decisioning remains:

- `NextActionAuthorityInputs::stale_target_allows_task_closure_bridge_for_task` and `repair_route_decision::stale_target_allows_task_closure_bridge` decide the same bridge eligibility question.
- `task_review_dispatch_required` is an active runtime/detail string but is not registered in `src/execution/phase.rs`.
- Two `has_authoritative_stale_binding` implementations in `src/execution/stale_target_projection.rs` duplicate the same branch/closure/task predicate.

## Reviewer recursion assessment

No actionable reviewer recursion finding was found. Recursion prevention is prompt text scoped to reviewer prompts and generated reviewer agents. No runtime/env recursion guard was introduced.

## Validation results

Latest implementation validation before this audit:

- `node scripts/gen-skill-docs.mjs --check`: passed
- `node scripts/gen-agent-docs.mjs --check`: passed
- `node --test tests/codex-runtime/*.test.mjs`: passed, 125/125
- active-surface banned phrase scan: passed, no output for hidden-helper/manual-repair phrases
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo nextest run --all-targets --all-features --no-fail-fast`: passed, 1619/1619
- `cargo test --test liveness_model_checker`: passed, 28/28

Subagents in this third audit performed source-level review of the same snapshot. They did not run full Rust validation themselves.

## Prioritized findings

### Blocker

None.

### High

1. Public doctor output is not fully diagnostic-only for `blocked_runtime_bug`.
   - Category: user-facing dead end
   - Evidence: `src/workflow/operator.rs::doctor_from_context`, `src/workflow/operator.rs::next_step_text`, `src/workflow/doctor_dashboard.rs::render_doctor_dashboard_with_external_review_hint`, `tests/workflow_runtime.rs` blocked-runtime-bug tests
   - Risk: fail-closed runtime bug output can still tell an agent to return to execution.

### Medium

1. Public `next_action` encodes two actions.
   - Category: user-facing churn source
   - Evidence: `src/execution/next_action.rs::public_next_action_text`, `src/execution/public_route_selection.rs::shared_next_action_seed_from_precomputed_decision`, `src/execution/status.rs::NextActionSchema`, `src/workflow/operator.rs::NextActionSchema`
   - Risk: agents can repair state and then manually reenter instead of following the next typed argv.

2. Diagnostic prose says to repair workflow/runtime routing without a public next step.
   - Category: documentation/public-output issue
   - Evidence: `src/workflow/doctor_dashboard.rs::blocker_action_text`, `src/workflow/operator.rs::task_boundary_reason_text`
   - Risk: wording can send agents toward hidden helpers or manual state edits.

3. Stale-target bridge eligibility is duplicated.
   - Category: architecture/split-decisioning issue
   - Evidence: `src/execution/repair_target_selection.rs::NextActionAuthorityInputs::stale_target_allows_task_closure_bridge_for_task`, `src/execution/repair_route_decision.rs::stale_target_allows_task_closure_bridge`
   - Risk: repair routing and next-action routing can drift.

4. Phase-detail vocabulary can escape `phase.rs`.
   - Category: architecture/test-boundary issue
   - Evidence: `src/workflow/operator.rs::task_boundary_reason_text`, `src/execution/router.rs` local `task_review_dispatch_required` literals, tests that assert the literal directly
   - Risk: central vocabulary and boundary tests miss active retired-lane detail values.

### Low

1. Authoritative stale-binding detection is duplicated inside one projection module.
   - Category: lower-priority cleanup
   - Evidence: `src/execution/stale_target_projection.rs::RuntimeGateSnapshot::has_authoritative_stale_binding`, `src/execution/stale_target_projection.rs::StaleTargetProjection::has_authoritative_stale_binding`
   - Risk: local drift in the same projection module.

## Checklist status

### Public CLI / reachability

- Public `begin` can seed preflight: fixed
- No normal flow needs `plan execution preflight`: fixed
- No normal flow needs `record-review-dispatch`: fixed
- No normal flow needs `gate-review`: fixed
- No normal flow needs `gate-finish`: fixed
- No normal flow needs `rebuild-evidence`: fixed
- No normal flow needs low-level late-stage recorders: fixed
- Operator never recommends hidden/debug commands: fixed
- Status never exposes hidden/debug commands as next actions: fixed
- Public recommended argv is executable by shipped CLI: fixed

### Plan review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed with residual exact-header risk
- Engineering-review edits do not bounce back to fidelity early: fixed
- Final engineering-approved handoff requires current five-surface fidelity: fixed
- Active docs do not teach plan-fidelity receipt recording: fixed
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed

### Execution runtime

- Current task closure is begin-time authority: fixed
- Current closure cannot appear in stale closures: fixed
- Close-current-task can refresh current dispatch internally: fixed
- Stale dispatch does not block public close: fixed
- Receipt/projection diagnostics do not trigger reentry: fixed
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed
- Cycle-break clears after current closure: fixed
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed
- Repair-review-state cannot loop on same route: fixed
- Runtime reconcile handles targetless stale states: fixed
- Runtime bug state is diagnostic-only in mutation/routing: partially fixed; public doctor `next_step` still needs suppression

### Evidence/projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed
- Projection materialization is explicit and not part of progress: fixed
- Runtime-owned projection paths do not stale task/branch closures: fixed
- Supersession is append-only and does not rewrite proof: fixed
- Evidence is audit/projection, not control plane: fixed

### Tests

- Public-flow tests do not call internal helpers: fixed
- Internal helpers are quarantined in internal-unit-only tests: fixed
- Static tests catch hidden helper use in public-flow tests: fixed
- Replay tests cover historical dead ends: fixed
- Liveness model catches repeated route signatures: fixed
- Node/doc contracts pass: fixed at latest validation
- Prompt budget test passes: fixed at latest validation
- Static tests catch compound `next_action` and vague repair-routing prose: still broken

### Prompt surface

- Skill docs are within budget: fixed
- Mandatory law remains top-level: fixed
- Companion references exist and are packaged: fixed
- Generated docs are fresh: fixed
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed
- No runtime/env recursion enforcement is introduced: fixed
- Reviewer prompts prohibit launching additional subagents: fixed

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed
- New modules have cohesive responsibilities: fixed with residual large-module risk
- No new catch-all module replaces the old monoliths: fixed
- Phase/reason strings are centralized: partially fixed
- Public command authority is typed, not string-parsed: fixed
- Router/read-model/mutation guards share decision objects: partially fixed
- Import-boundary tests exist: fixed
