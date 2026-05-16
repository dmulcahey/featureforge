# Deep Runtime Safety Re-audit - 2026-05-07

## Scope

This is a fresh re-audit of the updated FeatureForge codebase against the original runtime-safety audit instructions in this thread. The audit used eight clean-context, read-only subagents for the required risk areas:

- A: public CLI and reachable runtime
- B: tests versus shipped-runtime realism
- C: receipt, provenance, evidence, and projection control plane
- D: plan-review and engineering-review workflow
- E: stale closure, cycle-break, and reentry loops
- F: prompt surface and skill packaging
- G: modularization and split decisioning
- H: public output and agent UX

The audit did not run FeatureForge runtime or project skills. The parent validation pass used cargo and node commands only. Subagents were instructed not to spawn additional subagents.

## Executive Verdict

Verdict: ship only after targeted fixes.

The updated branch is materially better than the historical failure pattern. Public CLI reachability, begin-time preflight, task-closure authority, stale-closure exclusion, plan-fidelity artifacts, reviewer recursion scope, prompt budget checks, and public-flow test quarantine are now mostly coherent and backed by validation.

It is not clean enough to ship as-is. The remaining serious issues are concentrated in public-facing diagnostics and guidance that can still make agents act on old concepts:

- public review-gate remediation can still tell agents to "rebuild" packets or evidence;
- input-required public route guidance can tell agents to re-run operator/status instead of executing a completed typed command template;
- JSON failure text can still turn display command strings into practical executable authority;
- doctor/operator prose still uses multi-action "record or refresh" wording;
- a small amount of stale receipt vocabulary remains in compatibility-shaped doctor text;
- runtime modularization still leaves routing decisions reachable from lower layers or duplicated in pre-router/read-model repair flows.

This is close, but not done. It is no longer structurally unsafe in the same way as the earlier public/private mismatch and receipt-control-plane failures, but the residual UX and split-decisioning issues are exactly the kind of traps that can make an agent spin on FeatureForge semantics instead of implementing the task at hand.

Recommendation: do not ship this revision unchanged. Ship only after the targeted follow-up plan in `docs/featureforge/plans/2026-05-07-runtime-safety-reaudit-follow-up-remediation.md` is implemented and verified.

## What Is Genuinely Fixed

Public CLI reachability is substantially fixed.

- Normal runtime commands are public in `src/cli/plan_execution.rs:13`.
- Workflow command surfaces are public in `src/cli/workflow.rs:9`.
- `src/lib.rs:105` wires public plan-execution commands into shipped handlers.
- Typed public command authority is defined in `src/execution/command_eligibility.rs:81`.
- Public argv construction is centralized through `src/execution/command_eligibility.rs:633`.
- Route projection uses typed public command fields in `src/execution/read_model/public_route_projection.rs:53`.
- Workflow operator surfaces typed public route output in `src/workflow/operator.rs:1294`.
- Schemas mark `recommended_command` display-only and `recommended_public_command_argv` executable, including `schemas/workflow-operator.schema.json:623`, `schemas/plan-execution-status.schema.json:1130`, and `schemas/workflow-handoff.schema.json:1914`.

Public `begin` owns execution preflight setup.

- `src/execution/commands/begin.rs:47` and `src/execution/commands/begin.rs:167` perform public begin setup.
- `src/execution/state/preflight.rs:167` persists preflight state during begin-owned setup.
- No normal route inspected requires `plan execution preflight`.

Public `close-current-task` owns task closure and dispatch refresh.

- `src/execution/commands/close_current_task.rs:7` makes the public command the closure path.
- `src/execution/commands/close_current_task.rs:225` handles stale/missing dispatch lineage as a public diagnostic/refresh condition.
- `src/execution/commands/close_current_task.rs:450` checks command eligibility.
- `src/execution/commands/close_current_task.rs:587` materializes public closure recording from public inputs.
- No normal route inspected requires `record-review-dispatch`, `gate-review`, `gate-finish`, or low-level closure recorders.

Public `advance-late-stage` owns late-stage progression.

- Final-review dispatch, finish review, finish completion, branch closure, QA, and release readiness route through `src/execution/commands/advance_late_stage.rs:287`, `src/execution/commands/advance_late_stage.rs:967`, `src/execution/commands/advance_late_stage.rs:1592`, `src/execution/commands/advance_late_stage.rs:1697`, `src/execution/commands/advance_late_stage.rs:1740`, and `src/execution/commands/advance_late_stage.rs:1857`.

Receipt/provenance/evidence artifacts no longer appear to be control-plane truth after authoritative closure.

- Current task closure classification is centralized in `src/execution/current_closure_projection.rs:138`.
- Current closures are removed from stale targets in `src/execution/stale_target_projection.rs:635`.
- Begin guard accepts current prior closure and blocks only stale/missing authoritative closure states in `src/execution/read_model_support.rs:127`.
- Persisted repair follow-ups expire when a target has current pass/pass closure in `src/execution/current_truth.rs:1452`.
- Stale reentry target selection rejects current closures in `src/execution/repair_target_selection.rs:317`.
- Event authority is loaded from `events.jsonl` in `src/execution/event_log.rs:1253`.
- Materialized state is explicitly documented as projection/cache in `src/execution/transitions.rs:4893`.
- Projection materialization reports `runtime_truth_changed: false` in `src/execution/commands/materialize_projections.rs:11`.
- Diagnostic-only closure reason codes are isolated in `src/execution/closure_diagnostics.rs:13` and projected through `src/execution/read_model/public_route_projection.rs:73`.

Plan-fidelity and engineering-review routing are substantially fixed.

- Plan fidelity review is parseable and five-surface based in `src/contracts/plan.rs:528` and `src/contracts/plan.rs:690`.
- Draft engineering-review routing and final fidelity routing are handled in `src/workflow/status.rs:928` and `src/workflow/status.rs:1021`.
- Approved execution is gated by current plan fidelity in `src/execution/implementation_gate.rs:14` and `src/execution/commands/begin.rs:38`.
- No active schema fields named `plan_fidelity_receipt` were found.
- Tests cover engineering-review edit loops and old two-surface fidelity behavior in `tests/workflow_runtime.rs:2642`, `tests/workflow_runtime.rs:2707`, `tests/contracts_spec_plan.rs:1865`, and `tests/public_replay_churn.rs:2216`.

Tests now mostly prove shipped public runtime behavior.

- Public CLI tests use the compiled binary through `tests/support/public_featureforge_cli.rs:81`.
- Internal direct helpers are visibly quarantined in `tests/support/internal_runtime_direct.rs:1`, `tests/support/plan_execution_direct.rs:1`, and `tests/support/workflow_direct.rs:1`.
- Static guards cover public-flow helper boundaries, hidden commands, and `_for_tests` exception registration in `tests/public_cli_flow_contracts.rs:581`, `tests/public_cli_flow_contracts.rs:2161`, and `tests/public_cli_flow_contracts.rs:3234`.
- Liveness tests reject repeated public route signatures and repeated mutation commands in `tests/liveness_model_checker.rs:921` and `tests/liveness_model_checker.rs:1645`.

Prompt budget and generated docs are mostly controlled.

- `skills/skill-doc-budgets.json:2` is in `enforce` mode.
- Prompt budget, prompt contract, generated skill docs, and generated agent docs checks pass.
- Reviewer recursion prevention is prompt-text scoped in `agents/code-reviewer.instructions.md:9`, `skills/requesting-code-review/code-reviewer.md:7`, and `tests/runtime_instruction_review_contracts.rs:230`.

## What Remains Risky

The remaining risk is not that public commands are absent. The risk is that public-facing text and secondary module decisions can still lead an agent away from the typed public route.

High-risk residual issues:

- Public review-gate remediation still says to rebuild packet/evidence in `src/execution/state/rebuild_evidence.rs:400` and `src/execution/state/rebuild_evidence.rs:433`.
- Input-required route guidance tells agents to satisfy template inputs and rerun operator/status, which can repeat the same input-required route instead of executing a completed command. This wording is present in `docs/runtime-architecture.md:107`, `skills/using-featureforge/SKILL.md:208`, `skills/executing-plans/SKILL.md:176`, and `skills/document-release/SKILL.md:222`, and is locked by `tests/runtime_instruction_contracts.rs:3450` and `tests/codex-runtime/skill-doc-contracts.test.mjs:1441`.

Medium-risk residual issues:

- `JsonFailure` message text still embeds command-shaped display strings as "Next public action" in `src/execution/command_eligibility.rs:1564` and `src/execution/command_eligibility.rs:1620`.
- Operator and doctor text still uses vague multi-action language such as "run verification and then record task closure", "record or refresh", and "dispatch or record" in `src/workflow/operator.rs:1782`, `src/workflow/doctor_dashboard.rs:231`, and `src/workflow/doctor_dashboard.rs:238`.
- `src/execution/event_log.rs:18` imports routing and calls `route_runtime_state` from event-log/migration parity paths at `src/execution/event_log.rs:3570` and `src/execution/event_log.rs:3594`.
- `src/execution/read_model.rs:1412` performs pre-router execution-reentry classification with `NextActionAuthorityInputs::default()` while production router uses authority inputs in `src/execution/router.rs:429` and `src/execution/router.rs:464`.
- `src/execution/review_state.rs:2696` through `src/execution/review_state.rs:3018` still constructs repair plans, targets, follow-up state, and route actions locally.

Low-risk residual issues:

- Historical replay tests use synthetic event-log fixture setup before public CLI recovery in `tests/public_replay_churn.rs:1692` and `tests/runtime_behavior_golden.rs:471`.
- `close-current-task` swallows worktree-lease release errors in `src/execution/commands/close_current_task.rs:787`, while the callee can fail in `src/execution/authority.rs:1262`.
- Some internal tests still split `recommended_command` display strings in `tests/workflow_runtime.rs:506` and `tests/internal_contracts_execution_runtime_boundaries.rs:468`.
- High-use generated skills such as `using-featureforge` are not individually capped in `skills/skill-doc-budgets.json:4`.
- Root-relative prompt links to install-root reference docs remain path-fragile, for example `skills/executing-plans/SKILL.md:158`, `skills/executing-plans/SKILL.md:185`, `skills/subagent-driven-development/SKILL.md:130`, `skills/document-release/SKILL.md:86`, and `skills/finishing-a-development-branch/SKILL.md:147`.

## Concrete Dead Ends Still Possible

1. Evidence rebuild loop from public remediation text.

   A review-gate failure can still tell an agent to rebuild the packet or rebuild evidence. The shipped public CLI no longer expects agents to manually drive old evidence repair concepts, so this wording can send the agent searching for hidden helpers or manually editing artifacts instead of following public route output.

   References: `src/execution/state/rebuild_evidence.rs:400`, `src/execution/state/rebuild_evidence.rs:433`, `src/execution/state/review_gate.rs:147`.

2. Input-required requery loop.

   Public route templates have `base_argv` plus required inputs, but the guidance can tell agents to rerun operator/status after satisfying inputs. If the inputs are not actually bound into the public mutation command, the agent can observe the same template repeatedly.

   References: `src/execution/public_command_types.rs:8`, `docs/runtime-architecture.md:107`, `skills/executing-plans/SKILL.md:176`, `skills/subagent-driven-development/SKILL.md:124`, `tests/runtime_instruction_contracts.rs:3450`, `tests/codex-runtime/skill-doc-contracts.test.mjs:1441`.

3. Display-command execution drift.

   Failure JSON can present a display string as "Next public action". If agents or tests execute that string rather than `recommended_public_command_argv` or a typed input template, future changes can accidentally preserve display-string shellability while breaking the true public route contract.

   References: `src/execution/command_eligibility.rs:1564`, `src/execution/command_eligibility.rs:1620`, `tests/workflow_runtime.rs:6255`, `tests/workflow_shell_smoke.rs:9582`.

4. Post-close lease cleanup drift.

   A successful current-task close can ignore worktree-lease release failure. The closure itself is authoritative, but a stale lease can later surface as a blocker and look like closure churn.

   References: `src/execution/commands/close_current_task.rs:787`, `src/execution/authority.rs:1262`, `src/execution/review_state.rs:1249`, `tests/workflow_shell_smoke.rs:8365`.

5. Installed prompt reference dead end.

   Skill prompts can point to root-relative `review/...` or `docs/featureforge/reference/...` paths without `$_FEATUREFORGE_ROOT`. In an installed skill context outside the FeatureForge checkout, those references can be hard to resolve.

   References: `skills/executing-plans/SKILL.md:158`, `skills/executing-plans/SKILL.md:185`, `skills/subagent-driven-development/SKILL.md:130`, `skills/document-release/SKILL.md:86`, `skills/finishing-a-development-branch/SKILL.md:147`.

## Concrete Churn Sources Still Possible

- Router/read-model/review-state drift: `read_model.rs` preclassifies execution reentry using default authority inputs, while router uses reduced authority inputs, and `review_state.rs` still computes local repair route actions.
- Event-log/routing coupling: migration parity code in `event_log.rs` can now change behavior when route decisions change.
- Prompt wording churn: docs and skills can be "correct" at the schema level while still steering agents through old verbs such as rebuild, record, refresh, or rerun.
- Display-string test inertia: tests that split `recommended_command` can pressure future work to keep display strings executable.
- Prompt budget displacement: high-use generated skills can grow without a per-skill cap as long as the total generated prompt line budget remains under the global ceiling.

## Public/Private Test Mismatch Assessment

Assessment: mostly fixed, with one low-risk caveat.

Public-flow tests now use compiled public CLI helpers and static scanners. Internal helper bypasses are named and quarantined. The public-flow helper quarantine is meaningfully stronger than the historical state where tests called internal machinery that the shipped runtime could not expose.

Remaining caveat: some historical stuck-path tests seed damaged states with event-log `_for_tests` APIs before replaying recovery through the public CLI. That is acceptable if described honestly as synthetic historical setup plus public recovery. It should not be presented as a fully public end-to-end creation path for those damaged states.

References:

- `tests/support/public_featureforge_cli.rs:81`
- `tests/support/internal_runtime_direct.rs:1`
- `tests/support/plan_execution_direct.rs:1`
- `tests/support/workflow_direct.rs:1`
- `tests/public_cli_flow_contracts.rs:581`
- `tests/public_cli_flow_contracts.rs:2161`
- `tests/public_cli_flow_contracts.rs:2492`
- `tests/public_cli_flow_contracts.rs:3234`
- `tests/public_replay_churn.rs:1692`
- `tests/runtime_behavior_golden.rs:471`

## Receipt/Evidence/Projection Control-Plane Assessment

Assessment: fixed by inspection and validation.

Runtime-owned state appears authoritative. Markdown, projection, receipt-shaped diagnostics, review summaries, and evidence artifacts are treated as read models, audit output, or diagnostics rather than mutation authority after current closure exists.

The audit did not find evidence that stale/missing receipts, dispatch artifacts, summary hash drift, projection materialization, or evidence markdown can force `execution_reentry_required`, `reopen`, hidden repair helpers, or task-boundary churn after pass/pass current task closure exists.

The one exception is not control-plane authority but public wording: review-gate remediation text still says "rebuild" packet/evidence. That is an agent-UX problem, not evidence that receipts are still authoritative.

References:

- `src/execution/current_closure_projection.rs:138`
- `src/execution/stale_target_projection.rs:635`
- `src/execution/read_model_support.rs:127`
- `src/execution/current_truth.rs:1452`
- `src/execution/repair_target_selection.rs:317`
- `src/execution/event_log.rs:1253`
- `src/execution/transitions.rs:4893`
- `src/execution/projection_renderer.rs:51`
- `src/execution/commands/materialize_projections.rs:11`
- `src/execution/closure_diagnostics.rs:13`

## Prompt-Surface And Packaging Assessment

Assessment: partially fixed.

Generated docs are fresh and budget enforcement is active. Mandatory runtime/review law remains top-level in inspected high-use skills. Reviewer recursion prevention is prompt-text only and reviewer-prompt scoped. Hidden helper vocabulary is mostly absent from active prompts.

Remaining prompt-surface problems:

- root-relative reference paths to install-root docs are fragile in generated skills;
- high-use generated skills are not all individually budgeted;
- input-required route guidance can instruct an agent to rerun the route owner instead of executing a filled public command template;
- public diagnostics still contain rebuild-evidence wording.

References:

- `skills/skill-doc-budgets.json:2`
- `skills/skill-doc-budgets.json:4`
- `tests/codex-runtime/skill-doc-budget.test.mjs:13`
- `skills/using-featureforge/SKILL.md:206`
- `skills/executing-plans/SKILL.md:176`
- `skills/executing-plans/SKILL.md:185`
- `skills/subagent-driven-development/SKILL.md:124`
- `skills/subagent-driven-development/SKILL.md:130`
- `skills/requesting-code-review/code-reviewer.md:7`
- `agents/code-reviewer.instructions.md:9`
- `tests/runtime_instruction_review_contracts.rs:230`

## Modularization And Split-Decisioning Assessment

Assessment: partially fixed.

The old monolith problem is much improved. `mutate.rs` is now a facade, `state.rs` is smaller, command eligibility is typed, and several cohesive helpers now own stale target selection, closure projection, public route selection, late-stage command eligibility, and repair target selection.

The remaining issue is not file size. It is that more than one layer can still answer routing or repair questions:

- event log migration parity calls router;
- read model preclassifies execution reentry before router using default authority inputs;
- repair-review-state locally constructs repair/follow-up route actions;
- internal read-model test helpers duplicate public phase/next-action routing logic.

References:

- `src/execution/event_log.rs:18`
- `src/execution/event_log.rs:3570`
- `src/execution/event_log.rs:3594`
- `src/execution/read_model.rs:1369`
- `src/execution/read_model.rs:1412`
- `src/execution/router.rs:429`
- `src/execution/router.rs:464`
- `src/execution/review_state.rs:2696`
- `src/execution/review_state.rs:3018`
- `src/execution/review_state.rs:3020`
- `src/execution/read_model.rs:2088`
- `src/execution/read_model.rs:2217`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md:76`
- `tests/runtime_module_boundaries.rs:1513`

## Reviewer Recursion Assessment

Assessment: fixed.

Reviewer recursion prevention is prompt text, not runtime or environment enforcement. The audited prompts scope the prohibition to reviewer prompts, and tests reject runtime/env recursion enforcement.

References:

- `agents/code-reviewer.instructions.md:9`
- `skills/requesting-code-review/code-reviewer.md:7`
- `tests/runtime_instruction_review_contracts.rs:230`

## Validation Results

All requested validation commands were attempted. No requested command was skipped.

| Command | Result |
| --- | --- |
| `node scripts/gen-skill-docs.mjs --check` | Pass. Generated skill docs are up to date. |
| `node scripts/gen-agent-docs.mjs --check` | Pass. Generated agent docs are up to date. |
| `node --test tests/codex-runtime/*.test.mjs` | Pass. 124 tests passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Pass. |
| `cargo nextest run --test runtime_authority_contracts` | Pass. 6 tests passed. |
| `cargo nextest run --test workflow_runtime` | Pass. 90 tests passed. |
| `cargo nextest run --test workflow_shell_smoke` | Pass. 101 tests passed. |
| `cargo nextest run --test workflow_entry_shell_smoke` | Pass. 13 tests passed. |
| `cargo nextest run --test plan_execution` | Pass. 44 tests passed. |
| `cargo nextest run --test plan_execution_final_review` | Pass. 29 tests passed. |
| `cargo nextest run --test workflow_runtime_final_review` | Pass. 2 tests passed. |
| `cargo nextest run --test contracts_execution_runtime_boundaries` | Pass. 30 tests passed. |
| `cargo nextest run --test execution_query` | Pass. 11 tests passed. |
| `cargo test --test liveness_model_checker` | Pass. 28 tests passed. |

## Prioritized Findings

### Blocker

No blocker findings.

### High

#### H1. Public review-gate remediation still tells agents to rebuild evidence

Type: user-facing dead end.

`src/execution/state/rebuild_evidence.rs:400` emits "Rebuild the packet..." and `src/execution/state/rebuild_evidence.rs:433` emits "Reopen the step and rebuild its evidence." The review gate consumes v2 evidence validation through `src/execution/state/review_gate.rs:147`.

This directly contradicts the target state where evidence/projections are passive audit/projection output and no normal public path requires `rebuild-evidence`. Even if the runtime no longer uses hidden evidence repair as control-plane truth, the text can still send an agent looking for old evidence-rebuild semantics.

Required fix: replace public remediation with one public next step routed through typed operator/status command fields. Add tests that reject "rebuild evidence", "rebuild its evidence", and "rebuild the packet" from active public diagnostics.

#### H2. Input-required route guidance can create a requery loop

Type: user-facing dead end and prompt/documentation issue.

`PublicCommandTemplate` exposes `command_kind`, `base_argv`, and `required_input_names` in `src/execution/public_command_types.rs:8`. Guidance in `docs/runtime-architecture.md:107`, `skills/using-featureforge/SKILL.md:208`, `skills/executing-plans/SKILL.md:176`, `skills/subagent-driven-development/SKILL.md:124`, and `skills/document-release/SKILL.md:222` tells agents to satisfy inputs and rerun operator/status or the route owner. Tests lock this behavior in `tests/runtime_instruction_contracts.rs:3450` and `tests/codex-runtime/skill-doc-contracts.test.mjs:1441`.

For input-required routes, rerunning the route owner does not bind the missing inputs into a mutation command. It can show the same template again.

Required fix: make typed public command templates executable after input binding. Add explicit CLI flag metadata for each input, a shared materializer for completed argv, and guidance that says to execute the completed public command, not rerun the route owner.

### Medium

#### M1. `JsonFailure` text exposes display commands as practical authority

Type: public/private command authority issue.

`src/execution/command_eligibility.rs:1564` derives display commands, and `src/execution/command_eligibility.rs:1620` embeds them in failure messages as "Next public action." Tests assert this pattern in `tests/workflow_runtime.rs:6255` and `tests/workflow_shell_smoke.rs:9582`.

Schemas correctly mark `recommended_command` display-only, but failure JSON has only message text. That makes the display string the practical authority for agents and tooling consuming the error.

Required fix: replace command-shaped failure text with structured fields or a clear pointer to `recommended_public_command_argv` and public templates from status/operator JSON. Tests should reject command-shaped "Next public action" strings in `JsonFailure` messages.

#### M2. Operator and doctor prose still offers vague multi-action next steps

Type: agent UX issue.

`src/workflow/operator.rs:1782` uses phrases such as "Run verification and then record task closure", "Dispatch ... review ... then record task closure", and "Record or refresh". `src/workflow/doctor_dashboard.rs:231` says "Dispatch or record final review", and `src/workflow/doctor_dashboard.rs:238` says "Record or refresh the current task closure."

Agents should see one public next step, not a compound instruction that mixes verification, dispatch, recording, and refresh concepts.

Required fix: use one routed public action in text surfaces and point machine consumers to typed argv/template fields. Avoid old verbs that imply manual repair or low-level recorders.

#### M3. Event-log migration parity code calls router

Type: architecture issue.

`src/execution/event_log.rs:18` imports `route_runtime_state`, and migration parity projection calls routing in `src/execution/event_log.rs:3570` and `src/execution/event_log.rs:3594`. `docs/runtime-architecture.md:36` describes event log as persistence/replay below reducer/read-model/router. `tests/runtime_module_boundaries.rs:1513` does not forbid `event_log -> router`.

This weakens the intended flow:

CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

Required fix: move route parity validation out of event-log persistence code into a higher-level migration/query adapter or boundary test harness.

#### M4. Read model preclassifies execution reentry with default authority inputs

Type: architecture and split-decisioning issue.

`src/execution/read_model.rs:1369` prepares reroute state, then `src/execution/read_model.rs:1412` calls `execution_reentry_target(..., NextActionAuthorityInputs::default())`. Production routing uses authority inputs from reduced runtime state in `src/execution/router.rs:429` and `src/execution/router.rs:464`.

This creates at least two semantic answers to "is there an execution reentry target?"

Required fix: route/reentry classification should be derived once from the same authority inputs used by router, then projected to read-model/status surfaces.

#### M5. `repair_review_state` still contains local route/follow-up decisioning

Type: architecture and churn issue.

`src/execution/review_state.rs:2696` through `src/execution/review_state.rs:3018` constructs repair plans, target tasks, required follow-up, and route actions locally. `src/execution/review_state.rs:3020` rewrites baseline bridge behavior into `task_closure_recording_ready`.

The module-boundary reference already acknowledges this as scheduled follow-up in `docs/featureforge/reference/execution-runtime-module-boundaries.md:76`, but it remains live split decisioning.

Required fix: extract repair plan and follow-up decisions into shared route/repair decision objects consumed by router, next-action, read-model projection, and repair mutator code.

#### M6. Install-root reference links are path-fragile

Type: prompt packaging issue.

Generated skills point to install-root docs through root-relative paths without `$_FEATUREFORGE_ROOT`, including:

- `skills/executing-plans/SKILL.md:158` for `review/plan-task-contract.md`
- `skills/executing-plans/SKILL.md:185` for `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `skills/subagent-driven-development/SKILL.md:130` for the same review-state reference
- `skills/document-release/SKILL.md:86` for `review/late-stage-precedence-reference.md`
- `skills/finishing-a-development-branch/SKILL.md:147` for `review/late-stage-precedence-reference.md`

This can fail for installed skills when the active workspace is not the FeatureForge checkout.

Required fix: use explicit `$_FEATUREFORGE_ROOT/...` references for install-root docs. Keep skill-local companion docs explicitly skill-local.

### Low

#### L1. Synthetic historical fixtures are not full public setup paths

Type: test realism issue.

`tests/public_replay_churn.rs:1692` and `tests/runtime_behavior_golden.rs:471` use synthetic event-log `_for_tests` setup for historical damaged states, then test public CLI recovery. The exception is registered in `tests/public_cli_flow_contracts.rs:2492`.

This is acceptable as long as the claim remains limited to public recovery from synthetic historical states.

#### L2. `close-current-task` swallows worktree-lease cleanup failures

Type: churn source.

`src/execution/commands/close_current_task.rs:787` ignores the result of `release_worktree_leases_for_current_task_closures_and_persist`. The callee can fail in `src/execution/authority.rs:1262`. `repair-review-state` propagates analogous failures in `src/execution/review_state.rs:1249`.

Required fix: either prevalidate and propagate the cleanup error or surface it as an explicit non-success diagnostic so a post-close lease blocker is not surprising.

#### L3. Some internal tests still split display commands

Type: test realism issue.

`tests/workflow_runtime.rs:506` and `tests/internal_contracts_execution_runtime_boundaries.rs:468` split and execute `recommended_command`. These are not public-flow tests, but they preserve the assumption that display commands are executable.

Required fix: replace with typed argv/template helper usage or rename and document as intentionally internal display compatibility coverage.

#### L4. High-use generated skills are not individually budgeted

Type: prompt budget issue.

`skills/skill-doc-budgets.json:4` caps many generated skills, but high-use skills such as `using-featureforge`, `brainstorming`, and `verification-before-completion` remain unbudgeted individually. `tests/codex-runtime/skill-doc-budget.test.mjs:13` hardcodes the budgeted list.

The total generated prompt budget passed, but prompt bloat can move into unbudgeted high-use surfaces.

#### L5. Stale receipt vocabulary remains in doctor compatibility text

Type: lower-priority cleanup and scanner risk.

`src/workflow/doctor_dashboard.rs:234` still recognizes `plan_fidelity_receipt_missing`-shaped text by splitting reason parts. No current production source was found for that reason code, and tests reject active `plan_fidelity_receipt` schema/output, but this compatibility residue should be removed or renamed so old receipt vocabulary does not remain in public-output logic.

## Checklist Status

### Public CLI / Reachability

| Item | Status | Evidence |
| --- | --- | --- |
| Public `begin` can seed preflight. | Fixed | `src/execution/commands/begin.rs:47`, `src/execution/state/preflight.rs:167` |
| No normal flow needs `plan execution preflight`. | Fixed | Hidden compatibility only; no normal route found. |
| No normal flow needs `record-review-dispatch`. | Fixed | `close-current-task` refreshes dispatch authority itself. |
| No normal flow needs `gate-review`. | Fixed | Public late-stage aggregate owns review progression. |
| No normal flow needs `gate-finish`. | Fixed | Public late-stage aggregate owns finish progression. |
| No normal flow needs `rebuild-evidence`. | Partially fixed | Runtime route fixed, but public text still says rebuild packet/evidence. |
| No normal flow needs low-level late-stage recorders. | Fixed | `advance-late-stage` aggregate covers inspected late-stage transitions. |
| Operator never recommends hidden/debug commands. | Fixed | Typed command projection inspected; no hidden route found. |
| Status never exposes hidden/debug commands as next actions. | Fixed | Typed command projection inspected; no hidden route found. |
| Public recommended argv is executable by shipped CLI. | Fixed | `recommended_public_command_argv` is schema-backed executable authority. |

### Plan Review

| Item | Status | Evidence |
| --- | --- | --- |
| Plan-fidelity no longer uses hidden runtime receipt recording. | Fixed | `src/contracts/plan.rs:528`, `src/contracts/plan.rs:690` |
| Plan-fidelity artifact is parseable and not overly hand-format-sensitive. | Fixed | Five-surface artifact parser and tests. |
| Engineering-review edits do not bounce back to fidelity early. | Fixed | `src/workflow/status.rs:928`, `tests/workflow_runtime.rs:2642` |
| Final engineering-approved handoff requires current five-surface fidelity. | Fixed | `src/execution/implementation_gate.rs:14` |
| Active docs do not teach plan-fidelity receipt recording. | Fixed | No active receipt recording guidance found. |
| Old `plan_fidelity_receipt` fields are gone or historical only. | Fixed with cleanup caveat | Schemas clean; doctor compatibility text still has stale vocabulary. |

### Execution Runtime

| Item | Status | Evidence |
| --- | --- | --- |
| Current task closure is begin-time authority. | Fixed | `src/execution/read_model_support.rs:127` |
| Current closure cannot appear in stale closures. | Fixed | `src/execution/stale_target_projection.rs:635` |
| Close-current-task can refresh current dispatch internally. | Fixed | `src/execution/commands/close_current_task.rs:225` |
| Stale dispatch does not block public close. | Fixed | Public refresh path inspected. |
| Receipt/projection diagnostics do not trigger reentry. | Fixed | `src/execution/current_truth.rs:1452` |
| Summary hash drift does not trigger reentry when pass/pass closure is current. | Fixed | Current closure expires repair follow-ups. |
| Cycle-break clears after current closure. | Fixed | `src/execution/recording.rs:371` |
| `resume_task` is not authoritative unless exact command is begin for same task/step. | Fixed | Liveness and routing tests inspected. |
| Repair-review-state cannot loop on same route. | Fixed with architecture caveat | No loop found; local route decisioning remains. |
| Runtime reconcile handles targetless stale states. | Fixed | `src/execution/reentry_reconcile.rs:3` |

### Evidence / Projection

| Item | Status | Evidence |
| --- | --- | --- |
| Normal commands do not dirty tracked approved plan/evidence markdown. | Fixed by inspection | Projection writes are explicit/derived. |
| Projection materialization is explicit and not part of progress. | Fixed | `src/execution/commands/materialize_projections.rs:11` |
| Runtime-owned projection paths do not stale task/branch closures. | Fixed | Closure authority comes from runtime events. |
| Supersession is append-only and does not rewrite proof. | Fixed by inspection | Event log remains authority. |
| Evidence is audit/projection, not control plane. | Fixed with UX caveat | Public text still says rebuild evidence. |

### Tests

| Item | Status | Evidence |
| --- | --- | --- |
| Public-flow tests do not call internal helpers. | Fixed | Static guards and public CLI helper. |
| Internal helpers are quarantined in internal-unit-only tests. | Fixed | `tests/support/internal_runtime_direct.rs:1` |
| Static tests catch hidden helper use in public-flow tests. | Fixed | `tests/public_cli_flow_contracts.rs:581` |
| Replay tests cover historical dead ends. | Partially fixed | Public recovery after synthetic setup. |
| Liveness model catches repeated route signatures. | Fixed | `tests/liveness_model_checker.rs:921` |
| Node/doc contracts pass. | Fixed | Validation passed. |
| Prompt budget test passes. | Fixed | Validation passed. |

### Prompt Surface

| Item | Status | Evidence |
| --- | --- | --- |
| Skill docs are within budget. | Fixed | Global/per-listed budget checks pass. |
| Mandatory law remains top-level. | Fixed | Inspected high-use skill docs. |
| Companion references exist and are packaged. | Partially fixed | Some install-root references are path-fragile. |
| Generated docs are fresh. | Fixed | Generation checks pass. |
| Reviewer recursion prevention is prompt-only and reviewer-prompt scoped. | Fixed | Prompt and tests inspected. |
| No runtime/env recursion enforcement is introduced. | Fixed | `tests/runtime_instruction_review_contracts.rs:230` |
| Reviewer prompts prohibit launching additional subagents. | Fixed | Reviewer prompt text inspected. |

### Modularization

| Item | Status | Evidence |
| --- | --- | --- |
| `state.rs` and `mutate.rs` are not monoliths. | Mostly fixed | `mutate.rs` is a facade; `state.rs` is smaller compatibility surface. |
| New modules have cohesive responsibilities. | Mostly fixed | Several cohesive helpers exist. |
| No new catch-all module replaces old monoliths. | Mostly fixed | `review_state.rs` still has broad route/follow-up logic. |
| Phase/reason strings are centralized. | Partially fixed | Raw reason/follow-up strings remain in repair/output surfaces. |
| Public command authority is typed, not string-parsed. | Fixed with test/text caveats | Typed authority exists; failure text/internal tests still parse/display commands. |
| Router/read-model/mutation guards share decision objects. | Partially fixed | Reentry and repair decisions still split. |
| Import-boundary tests exist. | Partially fixed | Missing guard for `event_log -> router`. |

## Recommendation

Do not ship unchanged.

Ship only after targeted fixes to public diagnostics, input-required command-template execution, display-command failure text, doctor/operator prose, install-root prompt references, high-use prompt budgets, event-log/router import boundaries, read-model reentry classification, repair-review-state decision extraction, display-command test usage, and close-current-task lease cleanup error handling.

The follow-up remediation plan is `docs/featureforge/plans/2026-05-07-runtime-safety-reaudit-follow-up-remediation.md`.
