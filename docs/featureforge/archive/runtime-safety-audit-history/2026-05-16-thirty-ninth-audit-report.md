# FeatureForge Runtime Safety Audit - Thirty-Ninth Loop

## Executive Verdict

**Ship candidate for runtime safety, but do not stop the remediation loop yet.**

The runtime-facing safety posture is materially improved and the current public paths are no longer structurally unsafe. Eight focused audit lanes returned PASS: public CLI reachability, public-flow test realism, receipt/projection authority, plan-review flow, stale-closure/liveness, prompt packaging, modularization, and public-output UX.

The remaining actionable issues are signal-to-noise issues, not public runtime dead ends:

1. the completed thirty-eighth active plan still encodes a standing deep-audit loop as plan law;
2. route-owning skills still repeat detailed task-closure route mechanics that now belong in `references/operator-route-authority.md`;
3. static boundary-test growth needs an explicit policy stop so future work does not add private-topology scanner churn.

**Recommendation:** ship only after targeted signal-to-noise remediation. No blocker was found in public CLI reachability, runtime authority, closure convergence, or receipt/projection control-plane separation.

## What Is Genuinely Fixed

- Public CLI reachability is coherent. The public mutation set is exposed through `src/cli/plan_execution.rs`, and `src/lib.rs` dispatches those variants directly to command handlers.
- `begin` owns preflight/run identity setup. Non-begin mutations require begin-established run identity through `src/execution/state/preflight.rs`.
- `close-current-task` owns current dispatch refresh and closure recording through `src/execution/commands/close_current_task.rs`.
- `advance-late-stage` owns late-stage aggregate progression, including release readiness, final review, QA, branch closure, and finish paths in `src/execution/commands/advance_late_stage.rs`.
- Operator/status public route authority is typed: `recommended_public_command_argv` and `recommended_public_command_template` are derived from `RouteDecision`; `recommended_command` is display-only.
- Current task closure is the task-boundary authority. Receipt/provenance/projection drift is diagnostic or derived when current closure is sufficient.
- Plan-fidelity uses parseable review artifacts and five-surface review state, not unreachable receipt recording.
- Public-flow tests are separated from internal semantic tests. Internal helpers are quarantined, protected public-flow files reject hidden helpers and hidden commands, and liveness/model coverage is not mislabeled as shipped-runtime proof.
- Prompt packaging is budgeted and generated. Route-owning skills keep top-level `Installed Control Plane` law, and non-route skills use compact reference mode.
- Reviewer recursion prevention is prompt text only and reviewer-prompt scoped.
- Runtime modularization is improved. Route planning, route projection, command eligibility, status assembly, and mutation command boundaries have import-direction guards.
- The large-module guard now scans recursive production `src/execution/**` files and documents `src/execution/commands/advance_late_stage.rs` as scheduled follow-up debt.

## What Remains Risky

- **Process churn:** the active thirty-eighth plan text says it remains active until followed by another deep audit loop. This duplicates the user's controller-level instruction inside an active repo artifact and risks making every completed plan a trigger for another plan.
- **Prompt repetition:** route-owning skills still repeat task-closure route specifics such as `task_closure_recording_ready`, `task_review_dispatch_required`, and `--external-review-result-ready` handling even though `references/operator-route-authority.md` now owns detailed route law.
- **Static-test growth:** `tests/runtime_module_boundaries.rs` is intentionally broad and useful, but it is close to becoming a private-topology policy layer. Future additions should require a concrete audited failure class or public/import-boundary contract.

## Concrete Dead Ends Still Possible

No public runtime dead end was confirmed.

Checked dead-end classes:

- public route recommends a command not accepted by CLI: **not found**
- mutation guard requires hidden preflight/dispatch/receipt state: **not found**
- current closure routes back to same task without real stale/negative state: **not found**
- projection/receipt drift forces reentry after authoritative closure: **not found**
- `blocked_runtime_bug` offers normal mutation commands: **not found**
- `recommended_command` treated as executable authority: **not found**

## Concrete Churn Sources Still Possible

- Active plan text can generate another audit/remediation loop even when runtime lanes pass.
- Route-owning skill task-closure sections make agents read both the top-level skill and canonical route reference for the same law.
- Boundary-test additions may drift toward private helper topology rather than public behavior, import direction, and route authority.

## Public/Private Test Mismatch Assessment

**PASS.** Public-flow test realism is acceptable.

Evidence:

- Compiled public CLI helpers use `CARGO_BIN_EXE_featureforge` in `tests/support/public_featureforge_cli.rs`.
- Internal helpers are explicitly quarantined in `tests/support/plan_execution_direct.rs` and `tests/support/internal_runtime_direct.rs`.
- `tests/support/public_flow_scan.rs` rejects hidden helpers, hidden commands, display-command execution, internal support imports, and token-only follow-up traps in protected public-flow tests.
- `tests/public_flow_scan_contracts.rs` verifies the typed public-flow manifest and script alignment.
- `tests/liveness_model_checker.rs` is explicitly internal semantic/model coverage, not public-flow proof.

## Receipt/Evidence/Projection Control-Plane Assessment

**PASS.** Runtime-owned transition/event state and closure records are authoritative. Projection exports and markdown evidence are derived or diagnostic.

Evidence:

- `src/execution/read_model.rs`, `src/execution/reducer.rs`, and `src/execution/runtime_truth.rs` derive runtime truth from authoritative transition/event state.
- `src/execution/projection_renderer.rs` treats repo projection export as explicit and non-normal progress.
- `src/execution/current_task_closure_selection.rs`, `src/execution/current_closure_projection.rs`, and `src/execution/status_support.rs` preserve current task closure as task-boundary truth.
- `src/execution/state/unit_review_truth.rs` classifies plain receipt/projection drift as diagnostic unless bound to active runtime contract truth.

## Prompt-Surface And Packaging Assessment

**PASS with targeted signal/noise follow-up.**

Strong parts:

- `skills/skill-doc-budgets.json` is in `enforce` mode.
- Generated top-level skill docs total 4910 lines against a 5015 budget.
- `scripts/gen-skill-docs.mjs` distinguishes route-owning full route-law mode from compact reference mode.
- Companion references are packaged and asserted by Codex-runtime tests and `scripts/verify-source-archive.mjs`.
- Reviewer recursion rule lives in `references/reviewer-recursion-rule.md` and prompt surfaces, not runtime/env guards.

Risk:

- `skills/executing-plans/SKILL.md.tmpl` and `skills/subagent-driven-development/SKILL.md.tmpl` still repeat task-closure route mechanics that can be delegated to `references/operator-route-authority.md` while keeping the mandatory top-level action rule.

## Modularization And Split-Decisioning Assessment

**PASS with static-test growth caution.**

Evidence:

- `src/execution/mutate.rs` is a command re-export facade.
- `src/execution/state.rs` stays a reduced facade, with boundary tests preventing route/command decisioning from returning there.
- CLI dispatch flows through command modules, transition persistence, event append/reduction, router, route-plan projection, read model, and workflow operator presentation.
- `tests/runtime_module_boundaries.rs` now discovers production `src/execution/**` Rust files recursively, filters test-only modules, and documents `advance_late_stage`.

Risk:

- The module-boundary test is large and should not accumulate more scanner-only private-topology assertions without a concrete failure class.

## Reviewer Recursion Assessment

**PASS.** Recursion prevention remains prompt-text only and reviewer-scoped.

Evidence:

- `references/reviewer-recursion-rule.md` owns the canonical prompt text.
- Generated reviewer agent surfaces and skill reviewer prompts include the rule.
- Codex-runtime tests reject runtime/env recursion guard markers in Rust sources.

## Validation Results

All validation below ran after `cargo clean` at the start of the audit iteration.

- `node scripts/gen-skill-docs.mjs --check`: PASS
- `node scripts/gen-agent-docs.mjs --check`: PASS
- `node scripts/verify-source-archive.mjs`: PASS
- `node --test tests/codex-runtime/*.test.mjs`: PASS, 143/143, real 56.21s
- `cargo fmt --check`: PASS, real 2.47s
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS, real 44.03s
- `cargo nextest run --all-targets --all-features --no-fail-fast`: PASS, 1818/1818, real 183.48s
- Combined named audit targets with `cargo nextest run --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query --no-fail-fast`: PASS, 346/346, real 43.73s
- `cargo test --test liveness_model_checker`: PASS, 33/33, real 8.50s

Performance note: the full nextest suite remained under the 4-5 minute remediation threshold after the audit-start `cargo clean`.

## Prioritized Findings

### Blocker

None.

### High

None.

### Medium

#### M1 - Active Plan Encodes A Standing Audit Loop

- Classification: documentation/process churn
- Evidence: `docs/featureforge/plans/2026-05-16-runtime-safety-thirty-eighth-audit-remediation.md` says the plan remains active until it is followed by another deep audit loop, and includes a "Final Audit Loop" section requiring another A-H plus signal/noise audit.
- Impact: turns a completed remediation plan into live process authority for repeated audit generation.
- Required fix: archive the completed thirty-eighth plan and make the next active plan's completion rule evidence-triggered rather than self-perpetuating.

#### M2 - Route-Owning Skills Still Repeat Detailed Task-Closure Route Law

- Classification: prompt-surface signal/noise
- Evidence: `skills/executing-plans/SKILL.md.tmpl` and `skills/subagent-driven-development/SKILL.md.tmpl` still repeat `task_closure_recording_ready`, `task_review_dispatch_required`, `close-current-task`, and external-review-ready lane guidance while `references/operator-route-authority.md` owns detailed route law.
- Impact: agents must parse duplicate route law in high-use skills and canonical references.
- Required fix: keep the top-level mandatory action rule, but collapse detailed route-binding/replay bullets into a compact reference pointer.

### Low

#### L1 - Static Boundary Tests Need A Growth Policy

- Classification: test-maintenance risk
- Evidence: `tests/runtime_module_boundaries.rs` is large and includes line-count/private-topology guards; `docs/testing.md` already cautions against incidental topology assertions.
- Impact: future work may add more scanner assertions instead of deleting duplicated routing/status logic or testing public behavior/import direction.
- Required fix: document a boundary-test growth policy that favors import-direction, public route projection, externally visible behavior, and concrete audited failure classes over private helper topology.

## Specific Failure-Class Checklist

### Public CLI / Reachability

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

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed
- Engineering-review edits do not bounce back to fidelity early: fixed
- Final engineering-approved handoff requires current five-surface fidelity: fixed
- Active docs do not teach plan-fidelity receipt recording: fixed
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed

### Execution Runtime

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

### Evidence / Projection

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
- Node/doc contracts pass: fixed
- Prompt budget test passes: fixed

### Prompt Surface

- Skill docs are within budget: fixed
- Mandatory law remains top-level: fixed
- Companion references exist and are packaged: fixed
- Generated docs are fresh: fixed
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed
- No runtime/env recursion enforcement is introduced: fixed
- Reviewer prompts prohibit launching additional subagents: fixed
- Route-owning skills avoid duplicated route detail: partially fixed

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed
- New modules have cohesive responsibilities: fixed
- No new catch-all module replaces the old monoliths: fixed
- Phase/reason strings are centralized: fixed
- Public command authority is typed, not string-parsed: fixed
- Router/read-model/mutation guards share decision objects: fixed
- Import-boundary tests exist: fixed
- Boundary-test growth policy prevents private-topology churn: partially fixed

## Recommendation

Do not ship this exact working tree as the final loop result yet. Ship only after the targeted signal/noise follow-up plan removes the active self-perpetuating plan surface, compacts remaining route-owning task-closure prompt duplication, and documents the static-boundary growth policy.
