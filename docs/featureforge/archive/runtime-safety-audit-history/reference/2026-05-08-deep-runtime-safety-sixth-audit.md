# Sixth Deep Runtime Safety Audit

## Executive Verdict

Ship only after targeted fixes.

The updated runtime no longer shows a confirmed public/private dead end, receipt-control-plane loop, stale-closure loop, reviewer recursion defect, prompt-budget regression, or hidden-helper dependency in normal routing. The remaining issues are smaller but still actionable because they preserve drift-prone seams in exactly the areas this remediation series is trying to close: route target selection asks mutation eligibility for semantic permission, public command construction is still split across route/presentation modules, read-model projection recomputes execution-reentry target source after routing, and active public/documentation scans miss release-note and stale remediation-plan vocabulary.

## What Is Genuinely Fixed

- Public `begin`, `close-current-task`, and `advance-late-stage` cover normal workflow transitions without hidden dispatch, gate, preflight, or evidence repair commands.
- `recommended_public_command_argv` and public command templates are the executable route authority; `recommended_command` is marked display-only in schemas and text.
- Current positive task closures suppress receipt/projection churn and do not reappear as stale targets.
- `blocked_runtime_bug` and targetless stale reconcile states are diagnostic-only.
- Plan-fidelity depends on parseable five-surface review artifacts, not hidden receipt recording.
- Prompt budgets, generated docs, companion references, and reviewer recursion prompt contracts are enforced.
- `state.rs` and mutation facades are no longer monoliths, and boundary tests cover many import/write/read-model constraints.

## Remaining Risk

### P2: Route Target Selection Depends On Mutation Eligibility

`src/execution/repair_target_selection.rs` imports `public_execution_mutation_is_authorized` from `src/execution/command_eligibility.rs` and calls it while selecting resume and exact-route execution-reentry targets. Target selection feeds routing; mutation eligibility should consume the routed decision later. This feedback edge can hide future route/eligibility drift behind a boolean authorization call.

### P2: Public Command Construction Still Has Split Owners

`src/execution/next_action.rs` owns shared constructors for close-current-task, transfer handoff, repair, and reopen commands, but direct `PublicCommand::CloseCurrentTask` and `PublicCommand::TransferHandoff` construction remains in router and command-output modules. This keeps command shape ownership distributed across route/presentation code.

### P3: Read Model Recomputes Execution-Reentry Target Source

`src/execution/read_model/public_route_projection.rs` recomputes `execution_reentry_target_source` with `repair_follow_up_decision` after routing. The helper is shared, but the read model is still re-answering a route-semantic question rather than projecting a route-owned value.

### P2: Release Notes Escape Active Hidden-Helper Vocabulary Scans

`RELEASE-NOTES.md` contains historical command/receipt strings such as `record-review-dispatch`, `rebuild-evidence`, `gate-review`, `gate-finish`, and receipt language. The file has a historical disclaimer, but active prompt/doc scanners do not cover release notes, leaving public-facing historical text outside the hidden-helper vocabulary guard.

### P3: Active Remediation Plan Docs Contain Stale Receipt Wording

Older active remediation plans still mention `plan_fidelity_receipt_missing` as pending work even though current code no longer exposes active `plan_fidelity_receipt` fields. This is documentation staleness, not runtime authority, but it keeps stale vocabulary in active plan surfaces.

## Validation Results

Passed on the remediated tree before this audit synthesis:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs` (`125/125`)
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast` (`1631/1631`)
- `cargo nextest run --test runtime_authority_contracts`
- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test workflow_entry_shell_smoke`
- `cargo nextest run --test plan_execution`
- `cargo nextest run --test plan_execution_final_review`
- `cargo nextest run --test workflow_runtime_final_review`
- `cargo nextest run --test contracts_execution_runtime_boundaries`
- `cargo nextest run --test execution_query`
- `cargo test --test liveness_model_checker`
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`
- `git diff --check`

## Checklist Snapshot

- Public CLI / reachability: fixed, with display-only caveat handled by typed argv/template authority.
- Plan review: fixed for runtime behavior; stale historical remediation docs remain.
- Execution runtime: fixed for loops/control-plane authority; split route/eligibility edge remains.
- Evidence/projection: fixed for progress/control plane; execution-reentry target-source projection should become route-owned.
- Tests: fixed for public helper quarantine; installed/prebuilt runtime is covered by provenance/help validation, while public flow tests use compiled CLI.
- Prompt surface: fixed for generated skills/agents; release-note scanner coverage remains.
- Modularization: partially fixed; remaining split command construction and route/eligibility feedback should be removed.

## Recommendation

Do not ship yet. Implement the targeted remediation plan:

1. Remove the repair-target-selection dependency on mutation eligibility and centralize close/transfer command construction.
2. Move execution-reentry target-source ownership into `RouteDecision`.
3. Extend active public-doc scanner coverage to release notes and remove stale receipt wording from active remediation plans.
4. Re-run full validation, prebuilts/provenance, clean-context review, and the A-H audit loop.
