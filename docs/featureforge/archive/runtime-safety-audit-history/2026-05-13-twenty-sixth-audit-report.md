# Twenty-Sixth Deep Runtime Safety Audit

Audit date: 2026-05-13

Audit mode: read-only parent synthesis plus nine clean-context subagents. No FeatureForge runtime/project skills were used. Subagents were instructed not to spawn additional subagents. Parent validation used the workspace runtime only against fixture/temp state.

## Executive Verdict

Recommendation: ship only after targeted fixes.

The updated runtime is substantially safer than the original failure history. Public CLI reachability, receipt/projection demotion, plan-fidelity review, stale-closure convergence, reviewer recursion prevention, prompt budgets, and core modularization are mostly fixed. The branch is not structurally unsafe, but there are still actionable route-authority and agent-UX traps that can send an agent into a diagnostic or synthesized-command side path.

## What Is Genuinely Fixed

- Public normal transitions are reachable through shipped public CLI commands.
- `begin` owns execution preflight/run identity setup.
- `close-current-task` can record and refresh task closure without hidden dispatch/receipt repair.
- `advance-late-stage` owns branch closure, release readiness, final review, QA, and finish progression.
- Operator/status public command output is typed through `recommended_public_command_argv` and `recommended_public_command_template`.
- Current task closure is the task-boundary authority; current closures are not selected as stale fallback targets.
- Receipt/provenance/projection freshness is diagnostic-only for the inspected routing paths.
- Plan-fidelity uses parseable review artifacts, not hidden runtime receipts.
- Engineering-review edits no longer bounce immediately to fidelity before explicit final refresh.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped.
- Skill docs are generated and budgeted, with mandatory public route law still visible top-level.
- `state.rs` and `mutate.rs` are thin facades, and the main routing path flows through route-plan projection.

## What Remains Risky

- Blocked command outputs can still synthesize public recovery commands from follow-up strings when the selected operator route does not expose a matching typed command.
- `planning_reentry_required` without a command is classified and rendered as `waiting_external_input`, and doctor can classify non-external waiting states as terminal.
- State-kind literals and classification policy are split between route planning, workflow doctor, blockers, invariants, and mutation eligibility.
- The stale-target source token `closure_graph_stale_target` is locally translated in several modules instead of being centralized with the stale-target source model.
- Some public proof and boundary tests still mix behavioral evidence with scanner/topology assertions, which is useful but noisy.
- A few docs/skills still contain wording that could encourage fixed late-stage sequencing or acting from `next_action` when no typed executable surface is available.

## Concrete Dead Ends Still Possible

1. A blocked mutator can return a synthetic `repair-review-state` or `advance-late-stage` command even when the selected route lacks that command. That violates the public route law that no typed argv/template means stop and report the diagnostic.
2. A planning reentry route with no command can render as `Waiting for external review result.`, leading an agent to wait or rerun `--external-review-result-ready` instead of returning to plan/review work.
3. `workflow doctor` can classify a non-external waiting state without `external_wait_state` as terminal, making a blocked planning/review route look complete.

## Concrete Churn Sources Still Possible

1. State-kind classification duplicated across modules will drift when new diagnostic or waiting states are added.
2. Stale-target source strings are duplicated across selection, projection, and presentation, inviting future route/source mismatch.
3. Public-flow proof scripts include scanner self-tests in a file named like behavioral public CLI proof.
4. Boundary tests still pin some implementation topology and private literals more than externally visible behavior.
5. Skill/doc contract tests are close to phrase-locking docs rather than only preserving mandatory law.

## Public/Private Test Mismatch Assessment

Public-flow tests are much improved. Compiled CLI helpers use `CARGO_BIN_EXE_featureforge`, internal helpers are quarantined, and static guards reject hidden helper use in protected public-flow tests.

Residual mismatch is mostly labeling/signal rather than a shipped-runtime proof gap. `tests/liveness_model_checker.rs` is an internal semantic model checker with limited compiled CLI sampling. `scripts/run-public-runtime-flow-tests.sh` selects public-flow behavior tests but also includes `tests/public_cli_flow_contracts.rs`, which still contains scanner self-tests. Those scanners are valuable, but the gate name overstates pure public runtime proof.

## Receipt/Evidence/Projection Control-Plane Assessment

No high-risk control-plane leakage was found. Current task closure is begin-time authority, stale receipt/projection diagnostics do not force reentry when closure is authoritative, and projection materialization is explicit rather than progress-gating.

Residual coverage gap: the receipt-free routing contract is mostly an allowlist/source scan and does not directly cover every active routing/repair helper. This is a low-priority test gap unless future work touches receipt/projection routing.

## Prompt-Surface And Packaging Assessment

Prompt-surface compaction is working. Skill budgets are enforced, companion references are packaged, generated skills/agents are fresh, and reviewer recursion prevention remains prompt-only.

Residual prompt issues:

- `skills/executing-plans/SKILL.md.tmpl` still opens with fixed late-stage sequence language before the operator-route rule.
- `docs/featureforge/reference/2026-04-01-review-state-reference.md` still says agents can satisfy the prerequisite named by `next_action` when no argv/template exists, which conflicts with route authority.
- Text-mode operator output labels JSON requery guidance as `Command execution authority` even for diagnostic-only routes.

## Modularization And Split-Decisioning Assessment

Core modularization is improved, but not complete. `route_plan` owns final route selection, `router` calls the planner, read-model projection consumes route decisions, and workflow operator avoids write helpers.

Remaining split decisioning:

- `src/execution/commands/common/operator_outputs.rs` synthesizes recovery command surfaces outside the selected route.
- State-kind policy is duplicated across `src/execution/route_plan/state_kind.rs`, `src/workflow/doctor_resolution.rs`, `src/execution/command_eligibility.rs`, `src/execution/invariants.rs`, and `src/execution/route_plan/blockers.rs`.
- `closure_graph_stale_target` is translated/repeated across `src/execution/repair_target_selection.rs`, `src/execution/stale_target_projection.rs`, `src/execution/route_plan/follow_up.rs`, `src/execution/status_support.rs`, and `src/execution/stale_target_selection.rs`.

## Reviewer Recursion Assessment

Fixed. Reviewer recursion prevention is prompt text only, scoped to reviewer prompts, and no runtime/env recursion guard was found.

## Validation Results

Passed:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --no-fail-fast` from cleaned state: 1689 passed, 0 skipped, `real 321.95`
- Per performance rule, cleaned and reran full nextest: 1689 passed, 0 skipped, `real 194.62`

The first full nextest run exceeded the 4-5 minute threshold, but the clean rerun completed under the threshold. The regression was not repeatable.

## Prioritized Findings

### Blocker

None.

### High

1. Route-output recovery can synthesize commands outside the selected route.
   - Classification: architecture issue / user-facing dead end.
   - Files: `src/execution/commands/common/operator_outputs.rs`.
   - Functions: `public_recovery_contract_for_follow_up`, `fallback_public_recovery_contract`.
   - Evidence: fallback synthesis returns `repair-review-state`, input templates, or `advance-late-stage` surfaces from follow-up/profile when no matching operator command exists.

2. Planning reentry is mislabeled as external waiting.
   - Classification: user-facing dead end / architecture issue.
   - Files: `src/execution/route_plan/state_kind.rs`, `src/execution/route_plan/blockers.rs`, `tests/fixtures/runtime-goldens/public-runtime-routes.json`.
   - Functions: `classify_state_kind`, `primary_blocker_for_source`.
   - Evidence: `planning_reentry_required` with no command becomes `waiting_external_input`, and blocker text says `Waiting for external review result.`

### Medium

1. Doctor can classify non-external waiting states as terminal.
   - Classification: user-facing dead end / architecture issue.
   - Files: `src/workflow/doctor_resolution.rs`, `src/execution/route_plan/state_kind.rs`.
   - Function: `derive_doctor_resolution`.

2. State-kind vocabulary and policy are split across multiple modules.
   - Classification: architecture issue.
   - Files: `src/execution/route_plan/state_kind.rs`, `src/workflow/doctor_resolution.rs`, `src/execution/command_eligibility.rs`, `src/execution/invariants.rs`, `src/execution/route_plan/blockers.rs`.

3. Stale-target source/reason token `closure_graph_stale_target` is duplicated and locally translated.
   - Classification: architecture issue.
   - Files: `src/execution/repair_target_selection.rs`, `src/execution/stale_target_projection.rs`, `src/execution/route_plan/follow_up.rs`, `src/execution/status_support.rs`, `src/execution/stale_target_selection.rs`.

4. Public-flow proof mixes behavioral proof with scanner proof.
   - Classification: test realism / signal-to-noise.
   - Files: `scripts/run-public-runtime-flow-tests.sh`, `tests/public_cli_flow_contracts.rs`, `tests/public_flow_scan_contracts.rs`.

5. `executing-plans` still opens with fixed late-stage sequence wording.
   - Classification: documentation / agent UX.
   - Files: `skills/executing-plans/SKILL.md.tmpl`, generated `skills/executing-plans/SKILL.md`.

### Low

1. Text-mode operator diagnostic wording labels JSON requery as command authority.
   - Classification: public-output cleanup.
   - Files: `src/workflow/operator.rs`, `tests/workflow_shell_smoke.rs`.

2. Review-state reference says to act from `next_action` when argv/template is absent.
   - Classification: documentation cleanup.
   - File: `docs/featureforge/reference/2026-04-01-review-state-reference.md`.

3. Direct `repair-review-state` cycle-break cleanup lacks narrow regression coverage.
   - Classification: test coverage gap.
   - Files: `src/execution/review_state.rs`, `tests/public_replay_churn.rs`.

4. Receipt-free routing contract is mostly scanner/allowlist coverage.
   - Classification: test coverage gap.
   - File: `tests/runtime_authority_contracts.rs`.

5. Boundary/doc tests still pin some implementation topology and exact prose.
   - Classification: signal-to-noise watchlist.
   - Files: `tests/runtime_module_boundaries.rs`, `tests/codex-runtime/skill-doc-contracts.test.mjs`.

## Recommendation

Ship only after targeted fixes in `docs/featureforge/archive/runtime-safety-audit-history/plans/2026-05-13-runtime-route-authority-and-output-twenty-sixth-audit-remediation.md`.
