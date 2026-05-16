# FeatureForge Runtime Safety Thirty-Eighth Audit Report

## Executive Verdict

**Recommendation:** ship only after targeted fixes.

The updated codebase is no longer structurally unsafe in the original failure classes. Public CLI reachability, typed public argv/template authority, current task closure authority, diagnostic-only receipt/projection behavior, prompt budget enforcement, and reviewer-recursion prompt scoping all look materially improved and are covered by current tests.

There are still actionable issues, but they are targeted simplification and cleanup issues rather than public-runtime dead ends:

- plan-state and `Last Reviewed By` pairing is duplicated between contract analysis, workflow candidate parsing, and execution context checks
- active review-state reference text still names one retired hidden recovery command in negative guidance
- route-owning skill templates repeat more route law than necessary instead of relying on the canonical route reference
- public-flow scanner policy is drifting toward a policy surface of its own
- large-module enforcement misses command submodules, including `src/execution/commands/advance_late_stage.rs`
- `docs/testing.md` overstates the proof level of focused public runtime goldens

## What Is Genuinely Fixed

- Public route execution authority is typed. `recommended_public_command_argv` and bindable `recommended_public_command_template` are the executable surfaces; `recommended_command` is display-only.
- Public normal flow does not require hidden `preflight`, `record-review-dispatch`, `gate-review`, `gate-finish`, `rebuild-evidence`, or low-level late-stage recorder commands.
- `begin` owns preflight/run identity setup.
- `close-current-task` refreshes missing or stale dispatch lineage through the public aggregate closure path.
- `advance-late-stage` owns late-stage release readiness, final review, QA, and finish progression.
- Current task closure is the begin-time task-boundary authority; receipt/projection diagnostics do not force execution reentry after current pass/pass closure.
- Plan-fidelity uses parseable review artifacts and current five-surface checks, not runtime receipt recording.
- Engineering-review edits stay in engineering review until final fidelity refresh.
- Reviewer recursion prevention is prompt text only and reviewer-prompt scoped.
- Prompt budgets are enforced and generated docs/agents are fresh.
- `state.rs` and `mutate.rs` are no longer runtime monoliths.

## What Remains Risky

- Duplicated plan approval header truth can drift: `src/contracts/plan.rs` currently accepts `Engineering Approved` plus `writing-plans`, while workflow and execution reject that combination.
- Active docs should avoid naming hidden/retired command tokens even as negative examples.
- The public-flow scanner still protects real failure classes, but its exception and parsing policy is becoming heavy enough to need consolidation.
- Route-owning skills are clearer than before, but some still duplicate recovery and late-stage route law that belongs in `references/operator-route-authority.md`.
- Large-module boundary checks currently cover top-level execution files but not oversized command submodules.

## Concrete Dead Ends Still Possible

No confirmed user-facing public-runtime dead end was found. The public CLI auditor found no normal transition requiring a hidden/debug command, and the reentry-loop auditor found no repeated same-command loop or stale-closure oscillation.

The closest dead-end-adjacent issue is conceptual: active documentation naming `featureforge plan execution recover` keeps a retired path in agent-visible vocabulary. It does not create runtime authority, but it is needless churn bait.

## Concrete Churn Sources Still Possible

- Agents and future maintainers can rediscover hidden-path vocabulary from active negative guidance.
- Test maintainers can add scanner exceptions in multiple places instead of one manifest-like helper.
- Skills can keep accumulating duplicated "do not do X" route law despite the canonical operator reference.
- Large command submodules can grow into new monoliths without tripping the current large-module guard.

## Public/Private Test Mismatch Assessment

**Status:** fixed for current public-flow proof.

Public-flow suites use the compiled CLI helper (`CARGO_BIN_EXE_featureforge`) for shipped behavior. Direct helper tests are quarantined behind internal naming and static scanners. The only noted risk is not mismatch but maintenance cost: the public-flow scanner policy should be consolidated so it does not become a second routing specification.

## Receipt/Evidence/Projection Control-Plane Assessment

**Status:** fixed for audited paths.

Event log and reduced transition state are authoritative. Task closure validation checks the current closure record. Receipts, dispatch records, summaries, and projections are either close inputs, derived read models, or diagnostics after authoritative closure exists. Projection materialization is not required for progress and does not change runtime truth.

## Prompt-Surface And Packaging Assessment

**Status:** functionally sound, with simplification recommended.

Generated skill docs are within enforced budgets, companion references are packaged, and mandatory law remains top-level where needed. Reviewer recursion prevention is prompt-scoped. The remaining issue is signal-to-noise: route-owning skills should keep only the top-level action contract and delegate detailed binding/recovery law to `references/operator-route-authority.md`.

## Modularization And Split-Decisioning Assessment

**Status:** substantially improved, with one guard gap.

Route planning, read-model projection, public command typing, stale-target selection, and workflow recommendation projection have clear owners. The main gap is enforcement: the large-module boundary guard only scans top-level `src/execution/*.rs` files and misses oversized command submodules such as `src/execution/commands/advance_late_stage.rs`.

## Reviewer Recursion Assessment

**Status:** fixed.

No runtime/env recursion enforcement was found. Reviewer recursion prevention is prompt text and reviewer-prompt scoped, with tests covering generated reviewer surfaces and Rust source scans rejecting runtime enforcement markers.

## Validation Results

Clean-start audit validation was run after `cargo clean`.

- `node scripts/gen-skill-docs.mjs --check`: passed
- `node scripts/gen-agent-docs.mjs --check`: passed
- `node scripts/verify-source-archive.mjs`: passed
- `node --test tests/codex-runtime/*.test.mjs`: passed, 143/143
- `cargo fmt --check`: passed
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `/usr/bin/time -p cargo nextest run --all-targets --all-features --no-fail-fast`: passed, 1814/1814, real 167.18s

The full nextest run stayed below the 4-5 minute remediation threshold and well below the 10 minute immediate-stop threshold.

## Prioritized Findings

### Blocker

None.

### High

None.

### Medium

1. **Public-flow scanner policy is becoming its own policy surface.**  
   Type: signal-to-noise issue / test-maintenance risk.  
   Evidence: `tests/support/public_flow_scan.rs` owns gate classification, scanner exceptions, script parsing, and synthetic event-log exceptions; related self-tests live in `tests/public_flow_scan_contracts.rs`, while public DTO/source-policy checks also live in `tests/public_cli_flow_contracts.rs`.  
   Required remediation: consolidate exception metadata into one typed scanner policy helper/manifest and keep scanner parser tests limited to boundaries that do not have parser-backed or behavioral coverage.

2. **Route-owning skills repeat detailed route law inline.**  
   Type: prompt-surface simplification.  
   Evidence: route law is repeated in `skills/executing-plans/SKILL.md.tmpl`, `skills/subagent-driven-development/SKILL.md.tmpl`, and `skills/finishing-a-development-branch/SKILL.md.tmpl` despite the canonical route reference in `references/operator-route-authority.md`.  
   Required remediation: keep the top-level skill law to "query operator, execute typed argv/template, stop if absent" and move detailed binding/recovery law to the reference.

### Low

3. **Plan-state and reviewer pairing is duplicated.**  
   Type: architecture issue.  
   Evidence: `src/contracts/plan.rs::parse_plan_source` validates `Workflow State` and `Last Reviewed By` independently, while `src/workflow/status.rs::parse_workflow_plan_candidate` and `src/execution/context.rs` enforce the `Engineering Approved` + `plan-eng-review` relationship.  
   Required remediation: centralize the plan-state/reviewer pairing rule and use it from analyzer, workflow candidate parsing, and execution context.

4. **Active review-state reference names retired hidden recovery command.**  
   Type: documentation issue / churn source.  
   Evidence: `docs/featureforge/reference/2026-04-01-review-state-reference.md` says not to route through `featureforge plan execution recover`.  
   Required remediation: replace the literal retired command with generic "low-level recovery command" wording.

5. **Large-module guard misses command submodules.**  
   Type: test gap / cleanup.  
   Evidence: `tests/runtime_module_boundaries.rs::large_runtime_modules_have_documented_exception_or_followup` scans only direct `src/execution/*.rs` entries. `src/execution/commands/advance_late_stage.rs` is above 2000 lines but is not documented or guarded.  
   Required remediation: extend the guard to relevant execution subtrees and document/extract `advance_late_stage` ownership.

6. **Focused runtime goldens are valid but docs can overclaim.**  
   Type: test-maintenance risk.  
   Evidence: `docs/testing.md` describes `runtime_behavior_golden` in a public-flow section without enough emphasis that it uses a focused in-process public argv/parser runner for non-env rows and is not full compiled-CLI proof.  
   Required remediation: tighten wording to preserve the distinction between focused contract coverage and compiled-CLI transition proof.

### Low / Cleanup

7. Hidden/debug tokens remain in static deny-lists, internal compatibility tests, and archived historical docs. This is acceptable where the context is a scanner fixture or archive; do not edit historical plans/specs unless explicitly asked.

## Specific Checklist

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
- Plan-state/reviewer pairing is centralized: partially fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.
- Scanner policy stays consolidated and low-noise: partially fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.
- Route law is not over-repeated in skills: partially fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: fixed for core route/read-model slices.
- No new catch-all module replaces the old monoliths: partially fixed; command submodule size guard must include `advance_late_stage`.
- Phase/reason strings are centralized: fixed for audited route/recommendation paths.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: fixed for audited paths.
- Import-boundary tests exist: fixed, with one size-guard gap.

## Recommendation

Do not treat the branch as final until the targeted remediation plan is implemented and re-audited. The branch is close and the runtime safety posture is materially improved; the remaining work should reduce conceptual surface area instead of adding broad new guard layers.
