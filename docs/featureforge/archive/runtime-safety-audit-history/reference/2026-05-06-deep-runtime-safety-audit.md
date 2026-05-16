# FeatureForge Deep Runtime Safety Audit

**Audit Date:** 2026-05-06
**Repository:** `/Users/dmulcahey/.codex/worktrees/5d19/featureforge`
**Audit Mode:** Read-only codebase audit plus requested verification commands. No FeatureForge runtime workflow commands or FeatureForge skills were used directly by the auditor. Requested tests were run; several test suites internally exercise the compiled public CLI.

## Executive Verdict

**Verdict:** Close but not done.

**Ship candidate:** No, not as the final safe default for agent workflow. The branch is materially improved and many historical loops are now covered by public CLI, replay, and liveness tests. However, it still has enough residual semantic traps that agents can be sent into ambiguous public-output or provenance-repair paths, and authoritative event history still records old hidden primitive command identities under new aggregate public commands.

**Recommendation:** Ship only after targeted fixes.

There is no evidence from this audit that normal public user flows require invoking removed CLI commands directly. There is also strong evidence that stale closure loops, summary hash drift, projection dirtiness, targetless stale states, and resume mismatch cases now converge. The remaining risks are narrower but still structural:

- Public aggregate commands still persist old low-level command names into authoritative event-log envelopes.
- Some high-use generated skills ask agents to follow JSON-only fields after running text-mode `workflow operator`.
- Input-required normal transitions expose `required_inputs` but no serialized executable command shape.
- Post-closure worktree-lease provenance and recoverable closure overlays can still force `repair-review-state`.
- Public-flow tests still rely on internal event-log/test APIs for several hard-state fixtures.
- Workflow plan routing and late-stage mode selection retain duplicated semantic decisioning.

## Method

Eight clean-context parallel subagents audited independent risk areas:

- A: public CLI and reachable runtime
- B: tests vs shipped-runtime realism
- C: deprecated proof/provenance/evidence control-plane
- D: plan-review and engineering-review workflow
- E: stale closure, cycle-break, and reentry loops
- F: prompt surface and skill packaging
- G: modularization and split decisioning
- H: public output and agent UX

The parent audit cross-checked their findings locally using file inspection and the requested validation suite.

Command authority note: `recommended_public_command_argv` is the exact machine-invocation authority when present. `recommended_command` is display-only compatibility text and must not be parsed or executed.

## What Is Genuinely Fixed

### Public CLI reachability

- `src/cli/plan_execution.rs:13` exposes public `status`, `repair-review-state`, `close-current-task`, `advance-late-stage`, `begin`, `complete`, `reopen`, `transfer`, and `materialize-projections`.
- `src/cli/workflow.rs:9` exposes public `status`, `doctor`, and `operator`.
- Removed public command aliases are rejected by `src/lib.rs:359`.
- Hidden compatibility flags remain hidden on public args, for example the dispatch-id compatibility flag in `src/cli/plan_execution.rs:116` and `src/cli/plan_execution.rs:132`, and public help tests assert they do not appear.
- `src/execution/command_eligibility.rs:60` defines typed `PublicCommand` authority; display parsing is test-only at `src/execution/command_eligibility.rs:234`.
- Public route projection writes authoritative `recommended_public_command_argv` and `required_inputs` from the same route decision into status at `src/execution/read_model/public_route_projection.rs:53`.

### Begin and task closure convergence

- Public `begin` validates shared routing before mutation at `src/execution/commands/begin.rs:38`, persists begin-owned preflight setup at `src/execution/commands/begin.rs:167`, and records initial dispatch strategy checkpoint at `src/execution/commands/begin.rs:185`.
- `close-current-task` can internally refresh missing task dispatch lineage instead of requiring a public `record_review_dispatch` command. See `src/execution/commands/close_current_task.rs:210`.
- Already-current positive closures ignore summary-only hash drift at `src/execution/commands/close_current_task.rs:153` and again after lineage refresh at `src/execution/commands/close_current_task.rs:364`.
- Current closures are filtered away from stale targets in `src/execution/stale_target_projection.rs:635`.
- Liveness tests pass for summary hash drift, projection dirtiness, targetless stale diagnostics, resume disagreement, repeated mutation detection, and hidden recommendation checks.

### Plan review and engineering review

- Plan-fidelity no longer depends on hidden runtime proof recording. The removed command is not public, with coverage in `tests/workflow_runtime.rs:3064`.
- Plan-fidelity review gates are parseable artifacts read by `src/contracts/plan.rs:528`, not unreachable runtime proof state.
- The five required fidelity surfaces are defined in `src/contracts/plan.rs:62`.
- `plan_fidelity_allows_implementation` requires all five surfaces at `src/contracts/plan.rs:149`.
- Engineering-approved implementation handoff blocks until current fidelity passes at `src/workflow/status.rs:893`.
- Engineering-review edits can stay in engineering review without immediate fidelity bounce-back; see `src/workflow/status.rs:2141` and `tests/public_replay_churn.rs:2082`.

### Prompt packaging and reviewer recursion

- `skills/skill-doc-budgets.json:1` enforces prompt surface budgets.
- Current generated top-level skill total is 5,205 lines against a 5,600 cap.
- `node scripts/gen-skill-docs.mjs --check` and `node scripts/gen-agent-docs.mjs --check` passed.
- Companion references are packaged and tested.
- Reviewer recursion prevention is prompt scoped, not runtime/env enforcement. See `agents/code-reviewer.instructions.md:9`, `skills/requesting-code-review/SKILL.md:142`, and `tests/runtime_instruction_review_contracts.rs:136`.

### Runtime boundary testing

- `src/execution/state.rs` is a facade; `src/execution/mutate.rs` is a tiny facade.
- `tests/runtime_module_boundaries.rs:81` and surrounding tests enforce several single-source predicates and import boundaries.
- `tests/runtime_authority_contracts.rs:178` forbids deprecated proof-token terminology in public routing authority files.
- `tests/contracts_execution_runtime_boundaries.rs` passed and confirms mutation commands route transition writes through the recording boundary.

## What Remains Risky

### Public aggregate commands still write old hidden primitive command identities

Normal public late-stage flows route through `advance-late-stage`, but authoritative event-log envelopes still record command strings such as:

- `record_branch_closure`
- `record_release_readiness`
- `record_final_review`
- `record_qa`

Evidence:

- `src/execution/commands/advance_late_stage.rs:1626` calls the branch-closure path from the public aggregate command.
- `src/execution/recording.rs:455` persists branch closure with `"record_branch_closure"`.
- `src/execution/recording.rs:477` persists release readiness with `"record_release_readiness"`.
- `src/execution/recording.rs:505` persists final review with `"record_final_review"`.
- `src/execution/recording.rs:529` persists QA with `"record_qa"`.
- `src/execution/event_log.rs:3097` through `src/execution/event_log.rs:3110` maps those strings to typed execution events.

This is not currently a direct user-facing dead end. It is an architecture/control-plane issue: the public command surface says old recorders are no longer normal workflow, but authoritative event history still names them as command authority. Future reducers, diagnostics, provenance renderers, replay fixtures, or docs can rediscover the old control-plane vocabulary from the event log.

### `close-current-task` refreshes dispatch publicly but records old dispatch identity

When a public `close-current-task` has no current task dispatch candidate, it derives/refreshes lineage internally. That is good for reachability. However, the default refresh path still records through `"record_review_dispatch"`:

- `src/execution/commands/close_current_task.rs:210` selects/refreshes dispatch lineage.
- `src/execution/closure_dispatch_mutation/recording.rs:21` defaults to `"record_review_dispatch"`.
- `src/execution/event_log.rs:3080` maps `"record_review_dispatch"` to `ExecutionEvent::DispatchRecorded`.

Again, agents do not need to invoke `record_review_dispatch` directly. The issue is that authoritative command identity remains split between public owner and hidden primitive.

### JSON-only route fields are referenced after text-mode operator commands

Several generated skills run text-mode `workflow operator` but then instruct agents to consume `recommended_public_command_argv`, `required_inputs`, or `phase_detail` as if the JSON object had been captured:

- `skills/executing-plans/SKILL.md:94` runs `workflow operator --plan <approved-plan-path>` and `skills/executing-plans/SKILL.md:95` asks for `recommended_public_command_argv`.
- `skills/subagent-driven-development/SKILL.md:124` and `skills/subagent-driven-development/SKILL.md:132` do the same for task-boundary routing.
- `skills/requesting-code-review/SKILL.md:95` runs text operator before final review dispatch.
- `skills/finishing-a-development-branch/SKILL.md:127` and `skills/document-release/SKILL.md:221` have the same shape.
- `tests/codex-runtime/skill-doc-contracts.test.mjs:1373` currently locks the no-`--json` wording for `subagent-driven-development`.

Text output says display strings are not authoritative, but it does not emit the structured fields. This can push an agent into guessing from text summaries, rerunning the wrong command, or parsing display-only strings.

### Text public outputs drop executable command/input details

Text renderers intentionally avoid making display strings executable, but they do not provide the concrete JSON rerun instruction or the missing input names:

- `src/workflow/operator.rs:462` prints "Use JSON recommended_public_command_argv for execution" in phase text.
- `src/workflow/operator.rs:933` renders operator text with display summaries but no exact argv/inputs.
- `src/workflow/operator.rs:1050` renders handoff text similarly.
- `src/workflow/doctor_resolution.rs:44` classifies input-required states as `actionable_public_command` while `command_available=false`.
- `src/workflow/doctor_dashboard.rs:18` renders compact dashboard `Next action`, `Next step`, `Resolution kind`, and `Command available`, but not `recommended_public_command_argv` or `required_inputs`.
- `src/workflow/doctor_dashboard.rs:197` uses prose like "Record or refresh the current task closure..." without adjacent typed input/argv context.

This is an agent-UX dead-end risk, especially when a skill already asks the agent to follow JSON-only fields after running text mode.

### Input-required transitions do not serialize an executable public command shape

`PublicCommand::to_invocation` returns `None` whenever `required_inputs()` is nonempty:

- `src/execution/command_eligibility.rs:605`

Normal transitions such as task closure, release readiness, final-review recording, and QA therefore expose typed `required_inputs`, but no serialized argv or command discriminator. Tests codify this:

- `tests/workflow_shell_smoke.rs:125` asserts task-closure routes have no `recommended_public_command_argv`.
- `tests/workflow_runtime_final_review.rs:844` covers late-stage input-required routing.

This is reachable by a human, but a machine route consumer must infer command family from phase/detail and `required_inputs`, which is exactly the kind of split semantic inference FeatureForge has been trying to remove.

### Provenance and overlay repair can still control post-closure progress

Two post-closure repair paths remain:

1. Worktree lease provenance can force repair after a current task or branch closure exists:
   - `src/execution/read_model.rs:2808` reprojects `worktree_lease_` gate failures into public status when current closures exist.
   - `src/execution/review_state.rs:1212` lets `repair-review-state` release resolved leases.
   - `src/execution/recording.rs:219` performs the release mutation.
   - `tests/workflow_shell_smoke.rs:8011` covers this after closure.

2. Recoverable task-closure overlay loss can block begin even if history can reconstruct current closure truth:
   - `src/execution/transitions.rs:3758` can derive current task closure from history.
   - `src/execution/transitions.rs:3845` reports overlay restore needed when raw overlay differs.
   - `src/execution/read_model_support.rs:145` blocks next task begin on `current_task_closure_overlay_restore_required`.
   - `src/execution/follow_up.rs:367` maps that condition to `repair-review-state`.
   - `tests/internal_plan_execution.rs:7471` asserts the begin block.

Neither path appears to reopen execution or use hidden helpers. Both are still provenance/overlay repair as a control-plane step after authoritative closure truth should be sufficient.

### Public-flow tests still seed hard states internally

Public recovery commands are exercised through the compiled CLI, but several "public" tests construct their hard starting states via internal event-log/test APIs:

- `tests/runtime_behavior_golden.rs:471` seeds final-review truth through `load_reduced_authoritative_state_for_tests` and later event-log sync helpers.
- `tests/public_replay_churn.rs:1744` uses direct state mutation to seed FS-11 historical stale-boundary shape.
- `tests/workflow_shell_smoke.rs:1944` writes authoritative harness state and calls `sync_fixture_event_log_for_tests`.
- `tests/public_cli_flow_contracts.rs:1530` explicitly allows fixture setup exceptions for selected shell-smoke helpers.
- `tests/public_cli_flow_contracts.rs:1703` scans public-flow tests for known hidden helpers/imports but does not forbid `_for_tests` event-log APIs in public-gate tests.

This does not invalidate the recovery assertions. It does mean some test names overstate realism: they prove public routing and mutation from synthetic historically broken state, not an end-to-end shipped public mutation path to that state.

### Workflow routing still has split decisioning

Execution modularization has meaningful guardrails. Workflow routing still duplicates semantic plan-route decisions:

- Normal discovery route decision tree starts around `src/workflow/status.rs:737`.
- Explicit `--plan` override repeats stale linkage, contract analysis, fidelity/engineering-review readiness, and implementation routing around `src/workflow/status.rs:1041`.

Late-stage public command mode selection also has more than one owner:

- Canonical mapper: `src/execution/command_eligibility/late_stage.rs:12`.
- Late-stage route selection: `src/execution/late_stage_route_selection.rs:164`.
- Router special-case override: `src/execution/router.rs:600`.
- Public recovery input profile mapping: `src/execution/commands/common/operator_outputs.rs:288`.

These are not observed divergences in current tests. They are drift surfaces.

### Targetless-stale invariant defense is inert

Current public routing converges through reducer stale projection, but the backup invariant does not detect a raw stale-unreviewed state without a target:

- `src/execution/invariants.rs:275` calls `TargetlessStaleReconcile::status_needs_marker_for_status(status)`.
- The subagent found this effectively returns the presence of the diagnostic, so `check_targetless_stale_unreviewed_routes_to_reconcile` exits before raising a violation for the missing diagnostic.

This is low severity because public routes are covered:

- `src/execution/stale_target_projection.rs:299`
- `tests/public_replay_churn.rs:1509`

## Concrete Dead Ends Still Possible

1. **Skill-driven text-mode operator dead end**
   - A skill runs `workflow operator --plan <approved-plan-path>` without `--json`.
   - The next sentence tells the agent to follow `recommended_public_command_argv` or satisfy `required_inputs`.
   - Text output does not contain these fields.
   - The agent either guesses from `Display command summary`, reruns ad hoc, or stalls.

2. **Input-required route inference dead end**
   - Operator/status returns a normal actionable state with `required_inputs` but no `recommended_public_command_argv`.
   - A route consumer has to infer whether the public command is `close-current-task` or `advance-late-stage` and which mode applies.
   - If phase/detail handling drifts, the route is no longer fully executable from serialized public authority.

3. **Post-closure provenance repair detour**
   - A current positive task/branch closure exists.
   - Worktree lease provenance is missing, stale, or resolved-but-not-released.
   - Status/operator routes to `repair-review-state`.
   - The repair does not appear to loop, but the agent is still forced into FeatureForge repair semantics after closure.

4. **Recoverable overlay repair detour**
   - Event history contains reconstructable current task closure truth.
   - The raw current closure overlay is missing or mismatched.
   - `begin` blocks with `current_task_closure_overlay_restore_required`.
   - The user must run `repair-review-state` before next-task begin even though reducer history can derive closure.

5. **Event-authority vocabulary drift**
   - A future diagnostic, renderer, replay, or schema uses event envelope `command`.
   - Old low-level names like `record_final_review` appear authoritative again.
   - Agents or docs can rediscover hidden primitives despite public CLI cleanup.

## Concrete Churn Sources Still Possible

- Worktree lease provenance release after closure.
- Closure overlay restoration after history can reconstruct current closure.
- Text-mode operator/handoff/doctor requiring a second command to obtain machine fields.
- Input-required transitions requiring route consumers to infer command shape from phase/detail.
- Public replay fixtures maintaining internal state mutation helpers alongside public CLI assertions.
- Duplicate workflow plan-route decision trees drifting between normal discovery and explicit plan override.
- Duplicate late-stage mode selection drifting between route selection, router repair special-cases, and recovery contracts.

## Public/Private Test Mismatch Assessment

**Assessment:** Partial.

The public CLI helper is real:

- `tests/support/public_featureforge_cli.rs:90` runs `env!("CARGO_BIN_EXE_featureforge")`.

The guardrails are strong:

- `tests/public_cli_flow_contracts.rs:470` scans public tests for hidden command strings.
- `tests/public_cli_flow_contracts.rs:650` separates internal compatibility test names.
- `scripts/run-public-runtime-flow-tests.sh:7` and `scripts/run-internal-runtime-compatibility-tests.sh:7` split public and internal suites.

But hard-state fixture setup is still synthetic:

- `tests/runtime_behavior_golden.rs:471`
- `tests/public_replay_churn.rs:1744`
- `tests/workflow_shell_smoke.rs:1944`

Conclusion: recovery behavior is well tested through public CLI, but not every claimed public runtime scenario is end-to-end public-flow proof.

## Deprecated Proof, Evidence, and Projection Control-Plane Assessment

**Assessment:** Mostly fixed, with two material exceptions.

Fixed:

- Plan fidelity legacy proof artifacts are gone from the active public plan-review workflow.
- Current task closure is the main task-boundary authority.
- Summary hash drift is ignored when current pass/pass closure is still authoritative.
- Projection materialization is explicit and not part of progress.
- Runtime-owned projection paths are filtered from closure freshness decisions.
- Current task closures are not stale targets.

Exceptions:

- Worktree lease provenance still routes post-closure repair.
- Recoverable current task closure overlay mismatch blocks begin even when history can reconstruct closure truth.

## Prompt Surface and Packaging Assessment

**Assessment:** Packaging fixed; route-field wording still unsafe.

Fixed:

- Budgets are enforced.
- Generated docs and agents are fresh.
- Companion references are packaged.
- Mandatory law remains top-level.
- Reviewer recursion prevention is prompt-scoped and reviewer-prompt scoped.
- No runtime/env recursion guard was introduced.

Unsafe:

- Multiple generated skills still run text-mode operator while naming JSON-only route fields.
- Tests currently assert this wording instead of requiring `--json`.

## Modularization and Split-Decisioning Assessment

**Assessment:** Execution modularization improved; workflow split decisioning remains.

Fixed:

- `src/execution/state.rs` and `src/execution/mutate.rs` are no longer monoliths.
- Execution import/boundary tests exist and passed.
- Router/status/operator share route projection for many public route fields.

Still risky:

- `src/workflow/status.rs` duplicates normal discovery vs explicit plan override route decisions.
- Late-stage public command mode selection has multiple semantic owners.
- Workflow module size and boundary pressure are not guarded the same way execution modules are.

## Reviewer Recursion Assessment

**Assessment:** Fixed.

Reviewer recursion prevention is prompt text only and scoped to reviewer prompts. Tests passed, and no runtime/env recursion enforcement was found. Reviewer prompts prohibit launching additional subagents where required.

## Validation Results

All requested checks that were attempted passed.

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 123 tests.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo nextest run --test runtime_authority_contracts`: passed, 5 tests.
- `cargo nextest run --test workflow_runtime`: passed, 90 tests.
- `cargo nextest run --test workflow_shell_smoke`: passed, 100 tests.
- `cargo nextest run --test workflow_entry_shell_smoke`: passed, 13 tests.
- `cargo nextest run --test plan_execution`: passed, 44 tests.
- `cargo nextest run --test plan_execution_final_review`: passed, 29 tests.
- `cargo nextest run --test workflow_runtime_final_review`: passed, 2 tests.
- `cargo nextest run --test contracts_execution_runtime_boundaries`: passed, 30 tests.
- `cargo nextest run --test execution_query`: passed, 11 tests.
- `cargo test --test liveness_model_checker`: passed, 28 tests, 113.97 seconds.

## Prioritized Findings

### Blocker

None found.

### High

#### H1. Public late-stage flow persists hidden primitive command identities in authoritative event logs

**Type:** Architecture/control-plane issue.

**Impact:** Old low-level command identities remain authoritative state vocabulary. This can reintroduce hidden-helper semantics through reducers, diagnostics, replay, or docs even when public CLI output hides them.

**References:**

- `src/execution/commands/advance_late_stage.rs:1626`
- `src/execution/recording.rs:455`
- `src/execution/recording.rs:477`
- `src/execution/recording.rs:505`
- `src/execution/recording.rs:529`
- `src/execution/event_log.rs:3097`

**Required fix:** Persist aggregate public command identity such as `advance_late_stage` while preserving typed event payload kind. Add event-log contract tests that fail if normal public aggregate paths write `record_branch_closure`, `record_release_readiness`, `record_final_review`, or `record_qa` into event envelope `command`.

#### H2. Generated skills invoke text-mode operator while requiring JSON-only route fields

**Type:** Agent UX/documentation issue.

**Impact:** Agents can be told to follow fields that the command output does not provide, leading to display-string inference or route loops.

**References:**

- `skills/executing-plans/SKILL.md:94`
- `skills/executing-plans/SKILL.md:95`
- `skills/subagent-driven-development/SKILL.md:124`
- `skills/subagent-driven-development/SKILL.md:132`
- `skills/requesting-code-review/SKILL.md:95`
- `tests/codex-runtime/skill-doc-contracts.test.mjs:1373`

**Required fix:** Update templates and tests so every instruction that consumes `recommended_public_command_argv`, `required_inputs`, `phase`, or `phase_detail` runs `workflow operator --json`.

### Medium

#### M1. Public `close-current-task` dispatch refresh records hidden `record_review_dispatch` event identity

**Type:** Architecture/control-plane issue.

**References:**

- `src/execution/commands/close_current_task.rs:210`
- `src/execution/closure_dispatch_mutation/recording.rs:21`
- `src/execution/event_log.rs:3080`

**Required fix:** Allow internal dispatch refresh to persist under the aggregate owner command, for example `close_current_task`, while retaining typed `DispatchRecorded` payload.

#### M2. Input-required normal transitions omit serialized executable command shape

**Type:** Public route authority issue.

**References:**

- `src/execution/command_eligibility.rs:605`
- `src/execution/command_eligibility.rs:827`
- `tests/workflow_shell_smoke.rs:125`

**Required fix:** Add a structured command template/discriminator for input-required commands, or serialize argv with placeholders explicitly marked as non-executable until inputs are bound.

#### M3. Worktree lease provenance can force post-closure repair

**Type:** Control-plane/provenance issue.

**References:**

- `src/execution/read_model.rs:2808`
- `src/execution/review_state.rs:1212`
- `src/execution/recording.rs:219`

**Required fix:** Split safety-blocking unresolved leases from resolved provenance cleanup. Current pass/pass closure should not require repair solely to release already-resolved lease provenance.

#### M4. Recoverable task-closure overlay loss can block begin

**Type:** Control-plane/projection issue.

**References:**

- `src/execution/transitions.rs:3758`
- `src/execution/transitions.rs:3845`
- `src/execution/read_model_support.rs:145`
- `src/execution/follow_up.rs:367`

**Required fix:** Treat recoverable overlay restoration as automatic/idempotent or diagnostic-only when reducer history can reconstruct current closure truth.

#### M5. Public-flow tests still seed hard states with internal event-log/test APIs

**Type:** Test realism issue.

**References:**

- `tests/runtime_behavior_golden.rs:471`
- `tests/public_replay_churn.rs:1744`
- `tests/workflow_shell_smoke.rs:1944`
- `tests/public_cli_flow_contracts.rs:1530`
- `tests/public_cli_flow_contracts.rs:1703`

**Required fix:** Add stricter public-flow scanner coverage for `_for_tests` event-log APIs and convert at least the normal-path late-stage fixtures to public command setup or isolate them under explicit synthetic-replay naming.

#### M6. Workflow status normal route and explicit plan override duplicate semantic routing

**Type:** Split-decisioning issue.

**References:**

- `src/workflow/status.rs:737`
- `src/workflow/status.rs:1041`

**Required fix:** Extract one shared route-for-plan-candidate helper used by both discovery and explicit override.

#### M7. Late-stage public-command mode selection has multiple semantic owners

**Type:** Split-decisioning issue.

**References:**

- `src/execution/command_eligibility/late_stage.rs:12`
- `src/execution/late_stage_route_selection.rs:164`
- `src/execution/router.rs:600`
- `src/execution/commands/common/operator_outputs.rs:288`

**Required fix:** Centralize phase-detail to `PublicAdvanceLateStageMode` resolution and require all recovery/route code to call it.

#### M8. Text operator/handoff/doctor omit actionable argv/input detail

**Type:** Agent UX/public-output issue.

**References:**

- `src/workflow/operator.rs:462`
- `src/workflow/operator.rs:933`
- `src/workflow/operator.rs:1050`
- `src/workflow/doctor_resolution.rs:44`
- `src/workflow/doctor_dashboard.rs:18`
- `src/workflow/doctor_dashboard.rs:197`

**Required fix:** Text output should either print exact `--json` rerun instructions and required input names, or include the structured argv/input data in a safe text block.

### Low

#### L1. Targetless-stale invariant guard is inert

**Type:** Runtime invariant defense gap.

**References:**

- `src/execution/invariants.rs:275`
- `src/execution/stale_target_projection.rs:299`
- `tests/public_replay_churn.rs:1509`

**Required fix:** Make the invariant detect raw targetless stale-unreviewed state before the diagnostic marker exists.

#### L2. Workflow modules lack the same boundary pressure as execution modules

**Type:** Maintainability/split-decisioning risk.

**References:**

- `src/workflow/status.rs:1`
- `src/workflow/operator.rs:1`
- `tests/runtime_module_boundaries.rs:1238`

**Required fix:** Add workflow boundary/module-size guardrails or explicit exception tracking for workflow route modules.

## Required Checklist Status

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs the plan-execution preflight primitive: fixed.
- No normal flow needs `record_review_dispatch`: partially fixed. User-facing CLI no; event identity still yes.
- No normal flow needs `gate_review`: fixed user-facing.
- No normal flow needs `gate_finish`: fixed user-facing.
- No normal flow needs `rebuild_evidence`: fixed user-facing.
- No normal flow needs low-level late-stage recorders: partially fixed. User-facing CLI no; event identity still uses recorders.
- Operator never recommends hidden/debug commands: fixed user-facing.
- Status never exposes hidden/debug commands as next actions: fixed user-facing.
- Public recommended argv is executable by shipped CLI: partially fixed. Fully bound routes yes; input-required routes omit argv.

### Plan Review

- Plan-fidelity no longer uses hidden runtime proof recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: partially fixed. It is parseable but exact-header sensitive by design.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity proof recording: fixed.
- Old plan-fidelity proof fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: partially fixed. Overlay restore still blocks begin.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: partially fixed. Publicly yes; event command identity old.
- Stale dispatch does not block public close: fixed.
- Deprecated proof/projection diagnostics do not trigger reentry: partially fixed. Worktree lease provenance and overlay repair remain.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is same begin: fixed.
- Repair-review-state cannot loop on same route: fixed based on current coverage.
- Runtime reconcile handles targetless stale states: fixed in public route, low invariant caveat.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: mostly fixed, with worktree lease/overlay exceptions.

### Tests

- Public-flow tests do not call internal helpers: partially fixed. Quarantined helpers are blocked, but event-log `_for_tests` APIs remain in public-gate fixtures.
- Internal helpers are quarantined in internal-unit-only tests: partially fixed with explicit exceptions.
- Static tests catch hidden helper use in public-flow tests: partially fixed; hidden command/import coverage exists, `_for_tests` event-log APIs are not covered.
- Replay tests cover historical dead ends: fixed for recovery assertions.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.
- Prompts still mention hidden helper commands: fixed for active hidden-helper vocabulary, but JSON/text mismatch remains.
- Skills teach display command string parsing instead of typed argv: mostly fixed, but text-mode operator wording undermines it.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces old monoliths: partially fixed; workflow files remain large.
- Phase/reason strings are centralized: partially fixed.
- Public command authority is typed, not string-parsed: partially fixed. Typed internally; input-required route serialization incomplete.
- Router/read-model/mutation guards share decision objects: mostly fixed with residual split paths.
- Import-boundary tests exist: fixed for execution, partial for workflow.
