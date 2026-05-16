# FeatureForge Deep Runtime Safety Fifth Audit

Date: 2026-05-08

Audit target: updated FeatureForge worktree after the fourth-audit remediation plan.

Method: eight clean-context subagents audited independent risk areas A-H. No FeatureForge runtime skills, project skills, or repo-local skills were used. Subagents were explicitly prohibited from spawning additional subagents. Parent validation was run before the audit; subagents used targeted inspection and selected targeted tests.

## Executive Verdict

Recommendation: do not ship yet. Ship only after targeted fixes.

The branch is materially safer than the prior audit targets. Public-flow tests are now realistic, prompt budgets are enforced, plan-fidelity no longer depends on runtime receipts, current task closure is the dominant task-boundary authority, and stale closure/reentry routes mostly converge through public commands.

The remaining issues are narrower but still actionable:

- A high-severity public mutation guard ordering leak lets `close-current-task` refresh authoritative review-dispatch lineage before `require_close_current_task_public_mutation` authorizes the mutation.
- A public text renderer can emit `featureforge workflow operator --plan none --json`, which is an executable-looking dead-end command when no plan path is known.
- Task-boundary reason-code vocabulary remains duplicated across several decisioning modules.
- Public `next_action` text is still rewritten by route code outside the shared next-action mapper.

This is close, but not done. The guard-ordering issue is sufficient to block shipment because it violates the target state that public commands must be authorized before mutating runtime-owned state.

## What Is Genuinely Fixed

- Public `begin` owns preflight setup and checks public route authority before persisting allowed preflight/open-step state.
- Normal public routing no longer requires `plan execution preflight`, `record-review-dispatch`, `gate-review`, `gate-finish`, `rebuild-evidence`, or low-level late-stage recorders.
- Status/operator project typed public command argv from route decisions rather than parsing display text.
- Public-flow tests use the compiled `featureforge` binary through `tests/support/public_featureforge_cli.rs`.
- Internal direct helpers are quarantined under internal helper modules and protected by static public-flow scanners.
- Plan-fidelity uses parseable review artifacts with five required surfaces and fingerprint binding; active schemas do not expose `plan_fidelity_receipt`.
- Engineering-review edits can remain in engineering review until final fidelity refresh; implementation handoff requires current fidelity where intended.
- Evidence/projection materialization is explicit; normal routing does not require tracked projection writes.
- Current pass/pass task closures are filtered out of stale targets and expire task-scoped repair follow-ups.
- Prompt-surface budget enforcement is active, generated skills/agents are fresh, and reviewer recursion prevention is prompt text scoped to reviewer prompts.

## What Remains Risky

The remaining risks are not broad workflow collapse, but each can still cause agent churn:

- Mutation authorization ordering is wrong in one high-impact public command path.
- A no-plan text surface can present `none` as if it were a runnable plan path.
- Task-boundary reason-code literals can drift because modules re-author them instead of importing one vocabulary.
- Public next-action labels can drift because multiple modules still assign raw text.

## Concrete Dead Ends Still Possible

1. No-plan operator/handoff text can render:

   `featureforge workflow operator --plan none --json`

   Evidence: `render_phase_from_context` calls `operator_json_rerun_guidance(&context.route.plan_path, ...)` in `src/workflow/operator.rs`; `render_handoff_output` does the same for `handoff.plan_path`; `workflow_json_rerun_command` substitutes an empty path through `display_or_none(plan_path)`.

   Impact: an agent can execute an invalid command, fail on a missing `none` path, then loop through workflow discovery instead of moving to plan discovery/review handoff.

2. A public `close-current-task` invocation that is not the exact route can refresh dispatch lineage before failing the public mutation guard.

   Evidence: `close_current_task` computes status at `src/execution/commands/close_current_task.rs:12`, but when no dispatch candidate exists it calls `ensure_current_review_dispatch_id_for_command` at `src/execution/commands/close_current_task.rs:239`. The shared public mutation guard is only called later in positive/negative closure branches at `src/execution/commands/close_current_task.rs:468` and `src/execution/commands/close_current_task.rs:669`.

   Impact: an out-of-route public command can mutate authoritative dispatch state, creating hidden control-plane movement before the runtime reports that the command was not authorized.

## Concrete Churn Sources Still Possible

- Task-boundary reason-code duplication can make one surface route `prior_task_current_closure_stale` differently from another if a future change updates only one literal list.
- Next-action text duplication can make status/operator display text disagree with route authority even if typed argv remains correct.
- No-plan rerun guidance gives an executable-looking command where the right next step is plan discovery or waiting for the plan handoff.

## Public/Private Test Mismatch Assessment

Assessment: fixed for audited public-flow claims.

Evidence:

- `tests/support/public_featureforge_cli.rs` invokes the compiled binary via `CARGO_BIN_EXE_featureforge`.
- `tests/support/plan_execution_direct.rs` and `tests/support/internal_runtime_direct.rs` are documented internal-only helpers.
- `tests/public_cli_flow_contracts.rs` statically scans public-flow tests and rejects internal helper calls and hidden command strings.
- `tests/public_replay_churn.rs` executes `recommended_public_command_argv` through the public CLI and rejects repeated route signatures.
- `tests/liveness_model_checker.rs` executes exact public argv and rejects unchanged route signatures.

Residual risk: a few one-off compiled-CLI assertions outside the protected public-flow list still rely on convention, but the audit did not find hidden helper use there.

## Receipt/Evidence/Projection Control-Plane Assessment

Assessment: fixed in the audited paths.

Evidence:

- Missing/stale task review and verification receipt paths are diagnostic-only after authoritative closure.
- Existing current positive task closures remain authoritative; summary hash drift is ignored rather than forcing reentry.
- Projection renderer/materialization is isolated from normal progress commands.
- Review-gate truth is driven by authoritative event completion; missing/stale evidence projection warnings cannot satisfy authority and do not force reentry after current closure exists.

Residual risk: late-stage stale branch/milestone projection remains a complex area, but the inspected filters prevent fabrication of task reentry after current task closure authority exists.

## Prompt-Surface And Packaging Assessment

Assessment: fixed.

Evidence:

- `skills/skill-doc-budgets.json` is in enforce mode, total 5210/5600 in the targeted check.
- `tests/codex-runtime/skill-doc-budget.test.mjs` enforces total/per-skill caps.
- `node scripts/gen-skill-docs.mjs --check` and `node scripts/gen-agent-docs.mjs --check` passed.
- Reviewer prompts and generated agents carry prompt-scoped recursion prevention.
- Contract tests reject runtime/env recursion guard markers and hidden helper command names in active prompt surfaces.

## Modularization And Split-Decisioning Assessment

Assessment: improved but not fully done.

The module split is now meaningful in several areas: stale target projection, repair target selection, late-stage route selection, public command typing, public route projection, and read-model subprojections have focused owners. `workflow/operator.rs` does not import mutation command modules, and command modules do not write projection files directly.

The remaining split-decisioning findings:

- `closure_diagnostics.rs` owns public task-boundary reason-code classification, but equivalent literals are re-authored in `current_truth.rs`, `read_model_support.rs`, `repair_target_selection.rs`, `repair_route_decision.rs`, and `follow_up.rs`.
- `next_action.rs` owns `public_next_action_text`, but `public_route_selection.rs` mutates `next_action` after calling it and `router.rs` constructs some route decisions with raw next-action strings.

## Reviewer Recursion Assessment

Assessment: fixed.

Reviewer recursion prevention is prompt-text only and reviewer-prompt scoped. No runtime/env recursion guard was found in active runtime code. Reviewer prompts prohibit launching additional subagents.

## Validation Results

Parent validation before this audit:

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 125/125.
- `FEATUREFORGE_PREBUILT_TARGET=darwin-arm64 scripts/refresh-prebuilt-runtime.sh`: passed.
- Windows prebuilt refresh with `x86_64-pc-windows-gnu`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo nextest run --all-targets --all-features --no-fail-fast`: passed, 1622/1622.
- `cargo test --test liveness_model_checker`: passed, 28/28.
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`: passed.
- Denied-string scan across checked-in binaries for `repair review state / reenter execution`, `repairing runtime routing`, and `repair workflow routing`: no matches.
- `git diff --check`: passed.

Targeted subagent validation:

- Prompt-surface auditor ran Node skill budget/contracts/generation tests and `cargo test --test runtime_instruction_review_contracts -- --nocapture`: passed.
- Test-realism auditor ran `cargo test --test public_cli_flow_contracts`: passed, 58/58.
- Modularization auditor ran `cargo test --test runtime_module_boundaries`: passed, 44/44.

No command failures were reported in the audit validation.

## Prioritized Findings

### Blocker

None.

### High

#### H1. `close-current-task` can mutate dispatch lineage before public mutation authorization

Category: user-facing dead end / control-plane authority bug.

Files/functions:

- `src/execution/commands/close_current_task.rs:3` `close_current_task`.
- `src/execution/commands/close_current_task.rs:232` dispatch-id fallback branch.
- `src/execution/commands/close_current_task.rs:239` `ensure_current_review_dispatch_id_for_command`.
- `src/execution/commands/close_current_task.rs:468` positive-closure guard call.
- `src/execution/commands/close_current_task.rs:669` negative-closure guard call.
- `src/execution/closure_dispatch_mutation/recording.rs:148` write authority claim.
- `src/execution/closure_dispatch_mutation/recording.rs:183` strategy checkpoint recording.
- `src/execution/closure_dispatch_mutation/recording.rs:188` authoritative state publish.
- `src/execution/command_eligibility.rs:1527` non-exact route rejection.

Why it matters:

The target contract is that public mutation guards run before mutation. In the no-current-dispatch fallback, `close-current-task` can record authoritative dispatch lineage with `EventCommandOwner::PublicCloseCurrentTask` before the route guard proves the command is legal.

Required fix:

Authorize `close-current-task` against the initial routed status before any dispatch-refresh helper can claim write authority. Add a regression that constructs an out-of-route close attempt requiring dispatch refresh and proves no authoritative event-log/transition-state mutation occurs.

### Medium

#### M1. No-plan workflow text can emit `--plan none`

Category: public-output / agent UX dead end.

Files/functions:

- `src/workflow/operator.rs:489` `render_phase_from_context`.
- `src/workflow/operator.rs:1115` `render_handoff_output`.
- `src/workflow/operator.rs:1698` `operator_json_rerun_guidance`.
- `src/workflow/operator.rs:1709` `workflow_json_rerun_command`.

Why it matters:

When no approved plan path is known, the text renderer creates an executable-looking command with a literal `none` path. That is not a real plan and can send agents into an invalid command loop.

Required fix:

Make workflow rerun guidance plan-aware. If no plan path exists, do not render a command-shaped `--plan none`; render a non-command instruction that says the plan path is not available and the agent must obtain an approved plan path before querying operator JSON.

#### M2. Task-boundary reason-code vocabulary is duplicated across decisioning modules

Category: architecture / split decisioning.

Files/functions:

- `src/execution/closure_diagnostics.rs:27` `PUBLIC_TASK_BOUNDARY_REASON_CODES`.
- `src/execution/current_truth.rs:997` `task_boundary_block_reason_code`.
- `src/execution/current_truth.rs:1661` `task_scope_stale_review_state_reason_present`.
- `src/execution/read_model_support.rs:209`, `src/execution/read_model_support.rs:267`, `src/execution/read_model_support.rs:435`, `src/execution/read_model_support.rs:458`, `src/execution/read_model_support.rs:627`, `src/execution/read_model_support.rs:637`, `src/execution/read_model_support.rs:728`.
- `src/execution/repair_target_selection.rs:37`, `src/execution/repair_target_selection.rs:223`, `src/execution/repair_target_selection.rs:340`, `src/execution/repair_target_selection.rs:408`.
- `src/execution/repair_route_decision.rs:413`.
- `src/execution/follow_up.rs:357`.

Why it matters:

These literals decide whether a status is a task-boundary repair, stale target, closure baseline bridge, public repair follow-up, or diagnostic lane. Re-authoring the same vocabulary in many modules makes future routing drift likely.

Required fix:

Centralize reason-code constants and predicate helpers in one owned module, then replace repeated string matches with shared functions. Add a boundary test that fails if task-boundary reason literals appear outside the owner module or explicit documented message-emission sites.

### Low

#### L1. Public next-action text is not fully owned by the shared mapper

Category: architecture / public-output drift risk.

Files/functions:

- `src/execution/next_action.rs:199` `public_next_action_text`.
- `src/execution/public_route_selection.rs:154` local `next_action` mutation after shared mapping.
- `src/execution/public_route_selection.rs:182`, `src/execution/public_route_selection.rs:233`, `src/execution/public_route_selection.rs:240`, `src/execution/public_route_selection.rs:244`, `src/execution/public_route_selection.rs:258`, `src/execution/public_route_selection.rs:273`, `src/execution/public_route_selection.rs:298`.
- `src/execution/router.rs:914`, `src/execution/router.rs:1114`, `src/execution/router.rs:1176`, `src/execution/router.rs:1379`.

Why it matters:

Typed argv is still authoritative, so this is lower risk than H1. Still, route code can make public labels diverge from the shared next-action vocabulary.

Required fix:

Expose shared constructors or label helpers for route overrides, and add a boundary test rejecting raw public next-action strings in router/public route modules except through the shared owner.

## Required Checklist Status

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: fixed.
- No normal flow needs `gate-review`: fixed.
- No normal flow needs `gate-finish`: fixed.
- No normal flow needs `rebuild-evidence`: fixed.
- No normal flow needs low-level late-stage recorders: fixed.
- Operator never recommends hidden/debug commands: fixed for typed argv; no-plan text command is partially fixed because it emits an invalid public command, not a hidden command.
- Status never exposes hidden/debug commands as next actions: fixed.
- Public recommended argv is executable by shipped CLI: fixed for non-empty plan paths.

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed with residual strict Markdown artifact risk.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: partially fixed; normal route works, but guard ordering is unsafe.
- Stale dispatch does not block public close: fixed for normal route.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed in audited paths.
- Evidence is audit/projection, not control plane: fixed in audited paths.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed for known audited dead ends.
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

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: fixed with residual vocabulary ownership risk.
- No new catch-all module replaces the old monoliths: fixed.
- Phase/reason strings are centralized: partially fixed; phase strings are tested, task-boundary reason codes are not fully centralized.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed; next-action text ownership remains split.
- Import-boundary tests exist: fixed but need added coverage for reason-code and next-action vocabulary ownership.

## Recommendation

Ship only after targeted fixes.

The required fixes are small compared with prior remediation rounds, but H1 is a real runtime authority bug. Create and execute a focused remediation plan that:

1. Moves `close-current-task` public mutation authorization before any write-capable dispatch refresh.
2. Removes `--plan none` from public workflow text.
3. Centralizes task-boundary reason-code vocabulary.
4. Centralizes public next-action label ownership.
5. Adds boundary and public-flow regression tests for all four findings.
