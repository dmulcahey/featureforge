# Thirtieth Runtime-Safety Audit Report

Date: 2026-05-14

Scope: current working tree after `docs/featureforge/archive/runtime-safety-audit-history/plans/2026-05-14-runtime-audit-loop-remediation-round-3.md` implementation and review-clean remediation.

## Executive Verdict

Ship only after targeted fixes.

The runtime safety posture is materially improved. Public CLI reachability, typed public argv/template authority, receipt/projection demotion, current-closure authority, stale-loop convergence, prompt packaging, and reviewer-recursion constraints all passed independent audit. The remaining issues are not broad workflow dead ends, but they are still actionable because they keep duplicate decisioning and high-noise prompt/doc surfaces alive.

## What Is Fixed

- Public runtime transitions are reachable through shipped public commands.
- Public recommendations are typed through `recommended_public_command_argv` or `recommended_public_command_template`; `recommended_command` is display-only compatibility text.
- Public `begin`, `close-current-task`, and `advance-late-stage` own their intended normal-path responsibilities.
- Worktree lease receipt markdown is diagnostic projection output; structured lease/binding proof is control-plane truth.
- Terminal worktree lease gate and cleanup/release paths now share `validate_terminal_worktree_lease_structured_proof(...)`.
- Nonterminal leases, stale-baseline terminal leases, and noncanonical-context terminal leases do not surface cleanup targets or release markers.
- Public-flow tests use compiled CLI helpers, while direct runtime helpers are quarantined as internal-only.
- Reviewer recursion prevention is prompt text scoped to reviewer prompts, not runtime/env enforcement.
- Prompt budget checks, generated skill checks, generated agent checks, and Node runtime contracts pass.

## What Remains Risky

- The canonical operator route reference gives an incomplete `workflow operator --input` query shape and a test pins that incomplete shape.
- Close-current-task versus branch-closure routing is still decided by duplicated predicates in route decision and next-action conversion surfaces.
- `files_proven_drifted` and `qa_requirement_missing_or_invalid` remain duplicated reason-code literals across producer/classifier/consumer modules.
- Some prompt surfaces still repeat phase-specific routing law instead of relying on the canonical operator-route reference.
- `using-featureforge` still uses high-pressure skill invocation wording even though it later says explicit user instructions win.
- Superseded audit-remediation plans remain in the active plans directory, increasing conceptual noise for future agents.
- Historical release notes still mention old receipt mechanics in a non-current section. Existing tests keep this non-imperative, but it remains audit noise.

## Dead Ends Still Possible

No direct public runtime dead end was found. The credible user-facing doc dead end is that an agent could follow `workflow operator --input NAME=VALUE --json` from `references/operator-route-authority.md` without `--plan <approved-plan-path>`, which the shipped CLI requires.

## Churn Sources Still Possible

- Repeated static scanner and prompt contract additions without deleting old prompt prose.
- Active `docs/featureforge/plans/*audit*remediation*.md` accumulation.
- Reason-code spelling/classification drift where shared constants are absent.
- Route-selection drift where the same closure-to-branch predicate is re-expressed in separate modules.

## Public/Private Test Mismatch Assessment

No actionable public/private test mismatch was found. Public-flow tests use compiled CLI helpers and static scanners quarantine direct helpers. Liveness/model-checker suites are correctly labeled semantic proof rather than public CLI proof and include shipped CLI parity samples.

## Receipt/Evidence/Projection Control-Plane Assessment

No actionable control-plane leakage was found. Current task closure is authoritative. Projection freshness and receipt drift are diagnostic-only when structured runtime truth is current. Worktree lease cleanup/release now shares the same terminal structured proof as gate enforcement.

## Prompt Surface And Packaging Assessment

Mostly healthy. Generated docs and agents are fresh; budgets pass; companion references resolve; mandatory top-level law remains in route-owning skills. Remaining actionable work is reducing prompt pressure and repeated phase-specific routing prose.

## Modularization And Split-Decisioning Assessment

Improved but not done. `state.rs` and `mutate.rs` are reduced facades, and route-plan/status/operator boundaries are clearer. Remaining actionable split-decisioning is duplicated close-current-task/branch-closure routing and duplicated stale/provenance reason-code ownership.

## Reviewer Recursion Assessment

Clean. Reviewer recursion prevention is prompt-only and scoped to reviewer prompts; tests reject runtime/env recursion guard markers.

## Validation Results

Passed:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`: 138/138 passed
- `cargo clippy --all-targets --all-features -- -D warnings`
- `/usr/bin/time -p cargo nextest run --all-targets --all-features --no-fail-fast`: 1759/1759 passed, real 196.01s after required `cargo clean`

Performance threshold: full suite stayed under 4-5 minutes after clean build, so no performance remediation was required.

## Prioritized Findings

### High: Incomplete Canonical Operator Template Binding Command

Category: public-output / agent UX

Evidence:

- `references/operator-route-authority.md` says to rerun `workflow operator --input NAME=VALUE --json`.
- `src/cli/workflow.rs` requires `OperatorArgs.plan`.
- `tests/codex-runtime/skill-doc-contracts.test.mjs` pins the incomplete wording.

Risk: agents can execute an incomplete route-materialization query and treat the resulting CLI error as workflow ambiguity.

Required fix: require `workflow operator --plan <approved-plan-path> --input NAME=VALUE --json` in the canonical reference and tests.

### High: Duplicate Close-Current-Task Versus Branch-Closure Route Decision

Category: architecture / split decisioning

Evidence:

- `src/execution/route_plan.rs::close_current_task_or_branch_closure_route_decision`
- `src/execution/route_plan/next_action_choice/execution_routes.rs::task_closure_recording_ready_decision`

Risk: status/operator route selection and next-action conversion can drift on whether a current task closure should route to branch closure recording or close-current-task.

Required fix: centralize the predicate behind one shared helper and consume it from both surfaces.

### Medium: Duplicate Stale/QA Reason-Code Ownership

Category: architecture / split decisioning

Evidence:

- `files_proven_drifted` appears in `closure_graph.rs`, `stale_target_projection.rs`, `read_model.rs`, and `state/rebuild_evidence.rs`.
- `qa_requirement_missing_or_invalid` appears in `state/review_gate.rs`, `state/finish_gate.rs`, `current_truth.rs`, `status_assembly.rs`, `status_assembly/late_stage.rs`, `route_plan/next_action_choice/execution_routes.rs`, and `workflow/pivot.rs`.

Risk: producer/classifier/consumer spelling drift can make rebuild, stale-target projection, follow-up override, finish gate, and route selection disagree.

Required fix: add named constants and predicates to the shared gate/reason-code owner and enforce usage with boundary tests.

### Medium: Prompt Rule Duplication And Static Scanner Churn

Category: prompt-surface / signal-to-noise

Evidence:

- High-use skills repeat phase-specific routing rules that are already covered by `references/operator-route-authority.md`.
- `tests/codex-runtime/skill-doc-contracts.test.mjs` pins some exact prose rather than just canonical rule plus reference.

Risk: future agents maintain prompt wording instead of runtime semantics.

Required fix: collapse repeated rule blocks where safe, keep a compact top-level rule plus reference link, and avoid adding another scanner layer.

### Low: High-Pressure `using-featureforge` Skill Invocation Wording

Category: prompt-surface / signal-to-noise

Evidence:

- `skills/using-featureforge/SKILL.md.tmpl` says a 1% chance means a skill "ABSOLUTELY MUST" be invoked, then separately says user instructions win.

Risk: agents can over-apply FeatureForge skills despite explicit user constraints.

Required fix: collapse to one calmer rule that says check applicable skills unless the user explicitly forbids them; user instructions always win.

### Low: Active Audit-Plan Directory Noise

Category: documentation / signal-to-noise

Evidence:

- `docs/featureforge/plans` contains many superseded audit-remediation plans.

Risk: future agents infer current authority from overlapping historical remediation plans.

Required fix: archive superseded audit-remediation plans and leave one current remediation plan plus a short index.

## Recommendation

Do not ship yet. Ship only after the targeted remediation plan `docs/featureforge/plans/2026-05-14-runtime-signal-noise-thirtieth-audit-remediation.md` is implemented, fully verified, independently reviewed, and followed by another audit loop.
