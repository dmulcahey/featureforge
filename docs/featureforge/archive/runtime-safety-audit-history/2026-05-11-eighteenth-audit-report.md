# FeatureForge Runtime Safety Audit 18

Date: 2026-05-11

Scope: current working tree after the seventeenth-audit remediation implementation and final review loop.

Method:
- Ran `cargo clean` before the audit iteration.
- Dispatched clean-context auditors A through H plus signal/noise auditor I.
- Instructed all auditors to avoid FeatureForge/project skills and to not spawn subagents.
- Ran the required Node, Rust, targeted nextest, and liveness validation listed below.

## 1. Executive Verdict

Recommendation: ship only after targeted fixes.

The branch is close, and it is not structurally unsafe in the original sense. The repeated hard failure classes around hidden commands, receipt/projection control-plane leakage, stale task-closure loops, and unreachable plan-fidelity receipt mechanics are materially improved and have public tests behind them.

It is still not a ship candidate because the remaining issues are not only polish:
- stale-target authority is still split across runtime modules;
- route decisioning still has duplicate ordering/rewrite logic between `next_action` and `route_plan`;
- the read model replays route/status projection after the router already computed it;
- some protected public-flow tests still prove direct query behavior rather than shipped CLI behavior;
- prompt/package surfaces have a companion reference and prompt-budget review gap;
- a few public UX messages still invite manual repair-shape reconstruction.

The next work should reduce duplicated decisioning and prompt/test noise. Adding more static scanners without deleting duplicate logic would be the wrong direction.

## 2. What Is Genuinely Fixed

Public CLI reachability is substantially fixed. Auditor A found no normal runtime transition requiring hidden/debug/compatibility commands. Public execution commands are shipped in `src/cli/plan_execution.rs`, dispatched through `src/lib.rs`, and represented by typed `PublicCommand`/argv surfaces in `src/execution/command_eligibility.rs` and `src/execution/route_plan/decision.rs`.

Public `begin` owns preflight setup. Normal flow no longer needs `plan execution preflight`. Begin checks shared public routing and persists the allowed preflight path through `src/execution/commands/begin.rs` and `src/execution/state/preflight.rs`.

Public `close-current-task` can refresh current dispatch lineage and record closure without hidden dispatch repair. The current implementation in `src/execution/commands/close_current_task.rs` and `src/execution/recording.rs` no longer requires stale receipt/dispatch artifacts to be manually reconstructed.

Public `advance-late-stage` owns branch closure, release readiness, final review dispatch/result, QA, finish review, and finish completion. The late-stage mode map is centralized in `src/execution/command_eligibility/late_stage.rs`, and public aggregation is handled in `src/execution/commands/advance_late_stage.rs`.

Typed public argv/template authority is now the executable contract. `recommended_public_command_argv` and `recommended_public_command_template` are generated from typed route decisions. `recommended_command` is treated as display text and schemas mark it as non-authoritative.

Receipt, provenance, evidence, and projection artifacts are no longer normal control-plane truth. Auditor C found current task closure is the begin-time task-boundary authority and stale/missing receipt/projection artifacts are diagnostic after authoritative closure exists.

Plan review mechanics are fixed relative to the old receipt failure mode. Auditor D found plan-fidelity now uses parseable review artifacts in `src/contracts/plan.rs`, engineering-review edits do not bounce immediately back to fidelity, and engineering approval requires a current five-surface fidelity pass.

Stale closure/reentry convergence is materially improved. Auditor E found no path where a successful `close-current-task` routes back to the same task without a real negative/stale condition, and `resume_task`/`resume_step` remain diagnostic unless the exact legal command is the same `begin`.

Reviewer recursion prevention is prompt-text only and reviewer-prompt scoped. Auditor F found no runtime/env recursion enforcement was introduced.

## 3. What Remains Risky

The runtime still has duplicated decisioning in places that matter:
- stale-target authority is answered from both gate-snapshot authority and projected status fields;
- next-action computes ordered route candidates while route-plan also computes ordered route decisions;
- `route_plan/next_action_route.rs` rewrites routes after next-action returns;
- read-model projection reconstructs much of the status route projection already computed by `router.rs`.

The test suite is powerful but near the signal/noise edge:
- public CLI/golden tests are high-signal;
- some boundary tests pin private helper names and implementation snippets;
- some prompt tests assert exact positive prose instead of rejecting bad guidance and checking essential fields;
- `workflow_runtime.rs` and `execution_query.rs` are protected as public-flow files but still use direct runtime query APIs for some public claims.

The prompt surface is improved but still too spread out:
- `references/operator-route-authority.md` is used as companion law but was untracked in the audited working tree;
- the prompt-budget manifest was tightened without an explicit current prompt-budget review note;
- generated reviewer agent docs refer to `$_REPO_ROOT`/`$_FEATUREFORGE_ROOT` without standalone root-discovery guidance;
- `_featureforge_exec_public_argv` is named in generated docs/tests in a way that can look like a hidden helper even though it is a generated wrapper.

## 4. Concrete Dead Ends Still Possible

No normal public CLI dead end was found for begin, close-current-task, or advance-late-stage.

Remaining possible dead ends are mostly agent-UX and packaging related:
- `plan execution transfer` can still tell agents about a "legacy repair-step shape" and list `--repair-task`, `--repair-step`, `--source`, and `--expect-execution-fingerprint` without saying the shape is route-authorized only. References: `src/execution/state/command_requests.rs:219`, `src/execution/state/command_requests.rs:247`, `src/cli/plan_execution.rs:254`.
- Workflow plan override failures say the override file does not exist but do not provide the one safe next step: use an existing repo-relative approved plan path or return to the normal planning/review handoff. References: `src/workflow/operator.rs:1077`, `src/workflow/operator.rs:1101`, `src/workflow/status.rs:1101`, `tests/workflow_runtime.rs:4439`.
- If `references/operator-route-authority.md` is not tracked/packaged, installed skills can point to a missing companion reference.

## 5. Concrete Churn Sources Still Possible

Duplicate route/status projection:
- `src/execution/router.rs:78` computes a finalized status projection.
- `src/execution/read_model/public_route_projection.rs:16` and `:66` replay common route/status projection and stale closure projection.
- This invites drift when new public route envelope fields are added.

Split stale-target authority:
- `src/execution/stale_target_projection.rs:378` uses `gate_snapshot.has_authoritative_stale_binding(status)`.
- `src/execution/reentry_reconcile.rs:54` and `:138` recompute bound stale-target presence from projected status fields.
- Callers include `src/execution/status_assembly.rs:1213` and `src/execution/invariants.rs:282`.

Split route candidate logic:
- `src/execution/route_plan.rs:166` orders route decisions before delegating.
- `src/execution/next_action.rs:364` also performs ordered route selection.
- `src/execution/next_action.rs:465` locally selects earliest stale boundary from authoritative stale plus baseline reentry candidates.

Route rewrite after decision:
- `src/execution/route_plan/next_action_route.rs:66` mutates phase, command, recording context, and execution command context after the shared next-action result.
- Concrete rewrites exist at `:99`, `:242`, and `:286`.

Public mutation request duplication:
- `PublicCommand::to_mutation_request` exists in `src/execution/command_eligibility.rs:931`.
- Command modules still manually recreate requests in `src/execution/commands/begin.rs:73`, `complete.rs:22`, `reopen.rs:28`, `transfer.rs:92`, and `advance_late_stage.rs:1848`.

## 6. Public/Private Test Mismatch Assessment

Assessment: partially fixed.

What is good:
- test-only direct helpers are quarantined behind `internal_only_*` names;
- public replay tests execute typed `recommended_public_command_argv` through the compiled CLI helper;
- shell-smoke and golden coverage now exercise public route behavior.

What remains:
- `tests/support/public_flow_scan.rs` protects `workflow_runtime.rs` and `execution_query.rs`, but the direct-runtime marker list does not include `query_workflow_routing_state_for_runtime`.
- `tests/workflow_runtime.rs:43` and `:3405` and `tests/execution_query.rs:17` and `:464` use direct runtime query assertions inside protected public-flow files.
- `tests/liveness_model_checker.rs` is valid as an internal semantic model checker, but only samples compiled-CLI parity for a narrow edge while the broad matrix runs through an in-process parser/runtime runner.
- `tests/plan_execution_final_review.rs` includes useful CLI route coverage, but the setup writes/removes synthetic harness state directly.

## 7. Receipt/Evidence/Projection Control-Plane Assessment

Assessment: fixed for the audited normal flow.

Auditor C found no still-broken receipt/provenance/evidence control-plane defect. Current task closure is task-boundary authority. Receipt/projection diagnostics are separated from route authority. Missing or stale receipt/projection artifacts do not force `execution_reentry_required`, `reopen`, `repair-review-state`, or hidden helper paths after authoritative closure exists. `materialize-projections` reports `runtime_truth_changed: false`, which preserves projection as an explicit read model.

Residual risk is not receipt authority; it is duplicated projection of route/status fields after the authoritative route decision has already been finalized.

## 8. Prompt-Surface And Packaging Assessment

Assessment: partially fixed.

What is fixed:
- skill docs are within the enforced budget;
- mandatory route law remains top-level, with companion references used for details;
- generated skill and agent docs are fresh;
- reviewer recursion prevention is prompt-only and reviewer-scoped;
- active docs do not teach plan-fidelity receipt recording.

What remains:
- `references/operator-route-authority.md` must be tracked/packaged because generated skills and README link to it.
- `RELEASE-NOTES.md` lacks the prompt-budget review note required by `docs/testing.md` for the 5150-line enforcement change.
- reviewer agent instructions need standalone root discovery for `$_REPO_ROOT` and `$_FEATUREFORGE_ROOT`.
- prompt tests should stop becoming a second prose authority. Keep negative scanners and essential route-authority checks; avoid exact positive prose pins.

## 9. Modularization And Split-Decisioning Assessment

Assessment: partially fixed and still the highest-value remediation area.

What is good:
- `src/execution/state.rs` and `src/execution/mutate.rs` are thin facades.
- The intended execution flow exists: CLI args to command module to transition guard to event append to reducer to read model to route decision to workflow operator.
- Route-plan centralization is a useful direction.

What remains:
- stale-target authority is still split between gate-snapshot authority and projected-status inspection;
- route candidate ordering is still split between `next_action` and `route_plan`;
- read-model projection replays finalized route/status projection;
- public mutation request construction is duplicated in command modules;
- phase/reason vocabularies still contain local string switches;
- boundary tests miss the current split-decisioning risks while pinning some private implementation details.

## 10. Reviewer Recursion Assessment

Assessment: fixed.

Reviewer recursion prevention remains prompt text only, scoped to reviewer prompts. No runtime/env guard was introduced. Reviewer prompts prohibit launching additional subagents. This is the right boundary.

## 11. Validation Results

Passed:
- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs` - passed 133/133
- `cargo clippy --all-targets --all-features -- -D warnings` - passed, real 35.04s after clean build
- `cargo nextest run --test runtime_authority_contracts` - passed 7/7, real 40.45s including compile
- `cargo nextest run --test workflow_runtime` - passed 89/89, real 9.46s
- `cargo nextest run --test workflow_shell_smoke` - passed 106/106, real 21.03s
- `cargo nextest run --test workflow_entry_shell_smoke` - passed 13/13, real 3.69s
- `cargo nextest run --test plan_execution` - passed 44/44, real 6.16s
- `cargo nextest run --test plan_execution_final_review` - passed 29/29, real 4.10s
- `cargo nextest run --test workflow_runtime_final_review` - passed 2/2, real 3.49s
- `cargo nextest run --test contracts_execution_runtime_boundaries` - passed 29/29, real 5.15s
- `cargo nextest run --test execution_query` - passed 12/12, real 3.55s
- `cargo test --test liveness_model_checker` - passed 29/29, real 21.72s

No required command failed or was skipped in this audit pass.

## 12. Prioritized Findings

### Blocker

No blocker was found in the strict sense of a normal public CLI path that requires a hidden/debug command or receipt reconstruction.

### High

H1. Stale-target authority remains split.
- Type: architecture issue, churn source.
- References: `src/execution/stale_target_projection.rs:378`, `src/execution/reentry_reconcile.rs:54`, `src/execution/reentry_reconcile.rs:138`, `src/execution/status_assembly.rs:1213`, `src/execution/invariants.rs:282`.
- Risk: stale/targetless reconcile can be decided from projected status fields in some paths and authoritative gate snapshot in others.

H2. Route decisioning remains split between `next_action` and `route_plan`.
- Type: architecture issue, churn source.
- References: `src/execution/route_plan.rs:166`, `src/execution/next_action.rs:364`, `src/execution/next_action.rs:465`, `src/execution/route_plan/next_action_route.rs:66`, `:99`, `:242`, `:286`.
- Risk: route ordering and route rewrites can diverge as new route states are introduced.

H3. Read model replays finalized route/status projection.
- Type: architecture issue, signal/noise issue.
- References: `src/execution/router.rs:78`, `src/execution/read_model/public_route_projection.rs:16`, `src/execution/read_model/public_route_projection.rs:66`.
- Risk: read-model code can drift from router-finalized status and reintroduce split decisioning under a projection name.

### Medium

M1. Public-flow guard misses direct runtime query proof.
- Type: test realism issue.
- References: `tests/support/public_flow_scan.rs:435`, `tests/support/public_flow_scan.rs:809`, `tests/workflow_runtime.rs:43`, `tests/workflow_runtime.rs:3405`, `tests/execution_query.rs:17`, `tests/execution_query.rs:464`.
- Risk: tests can claim public behavior while bypassing shipped CLI routing/status output.

M2. Public mutation request construction is duplicated across command modules.
- Type: architecture issue.
- References: `src/execution/command_eligibility.rs:931`, `src/execution/command_eligibility.rs:1420`, `src/execution/commands/begin.rs:73`, `complete.rs:22`, `reopen.rs:28`, `transfer.rs:92`, `advance_late_stage.rs:1848`.
- Risk: new request fields can be added to one path without the others.

M3. Companion route authority reference is not tracked/packaged in the audited tree.
- Type: packaging/documentation issue.
- References: `references/operator-route-authority.md:1`, `skills/using-featureforge/SKILL.md:54`, `scripts/gen-skill-docs.mjs:62`, `README.md:113`.
- Risk: installed skills can point to missing companion law.

M4. Prompt-budget manifest changed without the required current review note.
- Type: documentation/process issue.
- References: `skills/skill-doc-budgets.json:2`, `docs/testing.md:187`, `RELEASE-NOTES.md:3`, `RELEASE-NOTES.md:22`.
- Risk: prompt-budget enforcement changes without the explicit review trail the repo requires.

M5. Public transfer and plan-override messages are not sufficiently route-authorized/actionable.
- Type: public-output/agent-UX issue.
- References: `src/execution/state/command_requests.rs:219`, `src/execution/state/command_requests.rs:247`, `src/cli/plan_execution.rs:254`, `src/workflow/operator.rs:1077`, `src/workflow/operator.rs:1101`, `src/workflow/status.rs:1101`.
- Risk: agents may reconstruct repair arguments or search artifacts instead of using one public route.

### Low

L1. CLI help still uses low-level "recorder" language for public commands.
- Type: public-output cleanup.
- References: `src/cli/plan_execution.rs:32`, `tests/internal_bootstrap_smoke.rs:58`.

L2. Generated docs name `_featureforge_exec_public_argv` prominently.
- Type: prompt-surface cleanup.
- References: `references/operator-route-authority.md:8`, `skills/using-featureforge/SKILL.md:31`, `tests/codex-runtime/skill-doc-contracts.test.mjs:46`.

L3. Boundary and prompt tests pin implementation/prose details.
- Type: signal/noise issue.
- References: `tests/runtime_module_boundaries.rs:1341`, `:1534`, `:3978`, `tests/codex-runtime/skill-doc-contracts.test.mjs:1507`, `tests/codex-runtime/gen-skill-docs.unit.test.mjs:150`, `docs/runtime-architecture.md:196`.

L4. Liveness and final-review tests need clearer public-vs-internal labels.
- Type: test realism documentation issue.
- References: `tests/liveness_model_checker.rs:7`, `:2981`, `:3117`, `:1488`, `tests/plan_execution_final_review.rs:378`, `:1850`, `:1873`.

## 13. Required Checklist Status

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: fixed.
- No normal flow needs `gate-review`: fixed.
- No normal flow needs `gate-finish`: fixed.
- No normal flow needs `rebuild-evidence`: fixed.
- No normal flow needs low-level late-stage recorders: fixed.
- Operator never recommends hidden/debug commands: fixed.
- Status never exposes hidden/debug commands as next actions: fixed.
- Public recommended argv is executable by shipped CLI: fixed.

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: fixed.
- Stale-target authority is single-sourced: partially fixed.
- Route decision ordering is single-sourced: partially fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: fixed.
- Read model consumes finalized routing projection without replaying route decisions: partially fixed.

### Tests

- Public-flow tests do not call internal helpers: partially fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: partially fixed.
- Replay tests cover historical dead ends: fixed.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.
- Static tests avoid private implementation-law churn: partially fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: partially fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.
- Prompt tests avoid second-prose-authority behavior: partially fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces old monoliths: fixed.
- Phase/reason strings are centralized: partially fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed.
- Boundary tests cover current split-decision risks without over-pinning private shape: partially fixed.

## 14. Recommendation

Do not ship as-is. Ship only after targeted fixes in the eighteenth-audit remediation plan.

The targeted fixes should prioritize:
1. remove duplicate route/status projection and stale-target authority;
2. centralize route-candidate and public mutation request construction;
3. tighten public/private test realism while deleting noisy private-shape pins;
4. clean prompt packaging and agent-facing wording without expanding the prompt surface.
