# FeatureForge Agent Guide

This file is project-local guidance for agents working in `featureforge`. It applies to this repository only.

## Purpose

- `featureforge` is the FeatureForge runtime and workflow toolkit.
- The repo mixes Rust runtime code, workflow contracts, generated skill docs, and verification tests.
- Treat workflow truth, review artifacts, and execution-state artifacts as authoritative surfaces. Do not make casual compatibility changes around them.

## Project Layout

- `src/`: Rust implementation for contracts, execution, workflow routing, repo safety, update checks, and CLI support.
- `tests/`: Rust integration tests. Many helpers here model authoritative runtime artifacts; preserve behavior when refactoring test support.
- `skills/*.md.tmpl`: source templates for generated skill docs.
- `skills/*/SKILL.md`: generated skill docs. Regenerate them instead of hand-editing when a corresponding `.tmpl` exists.
- `review/`: review guidance and review-support references.
- `docs/featureforge/`: specs, plans, and execution evidence.

## Working Rules

- Prefer minimal behavior-preserving fixes over broad rewrites unless a rewrite is required to satisfy the workflow contract.
- Do not silently weaken runtime trust boundaries, workflow gates, or artifact validation.
- Do not update historical plans or specs unless the user explicitly asks for that exact artifact change.
- When a change touches generated skill docs, edit the `.tmpl` source and regenerate the checked-in `SKILL.md` output.

## Shared Truth

- When the same workspace state is visible to multiple surfaces, derive that truth once in shared runtime helpers and project from there. Do not let `workflow/status`, `workflow/operator`, execution query/state, repair/reconcile, and mutators recompute the same routing or review-state truth independently.
- The default expectation is convergence across all surfaces. Divergence is only acceptable when it is required for functionality or explicit boundary testing.
- Any intentional divergence must be documented with a nearby code comment explaining why the boundary requires it and why the shared helper path is not being used there.

## Performance Discipline

- Prefer in-process/runtime helpers over subprocess invocation when the shell boundary is not itself under test. Semantic tests should exercise the same Rust code paths the runtime uses directly.
- Prefer `gix` or other established high-performance libraries over ad hoc `git` subprocesses for repository inspection when semantics can be preserved.
- Memoize immutable or effectively immutable reads when they are reused within a command or test flow. Repeated repo discovery, overlay loads, transition-state parses, and tree/head lookups are all suspect until proven otherwise.
- When replacing a CLI subprocess with an in-process test helper, preserve CLI semantics exactly. Match success vs failure exit behavior, stdout vs stderr routing, trailing newlines, JSON field ordering, and explicit state-dir/runtime-root inputs instead of relying on ambient defaults.
- Do not accept test-speed regressions as harmless. If `cargo test` meaningfully slows down, profile it and remove the duplicated IO or subprocess churn rather than weakening the suite.

## Project Memory

- `docs/project_notes/` is supportive memory only; approved specs, plans, execution evidence, review artifacts, runtime state, and active repo instructions remain authoritative.
- Before inventing a new cross-cutting approach, check `docs/project_notes/decisions.md` for prior decisions and follow the authoritative source it links.
- When debugging recurring failures, check `docs/project_notes/bugs.md` for previously recorded root causes, fixes, and prevention notes.
- Never store credentials, secrets, or secret-shaped values in `docs/project_notes/`.
- Use `featureforge:project-memory` when setting up or making structured updates to repo-visible project memory.

## Subagent Coordination

- Do not rush, ping, or interrupt productive subagents just to force a faster-looking loop. Premature interruption creates churn, duplicated work, broken local context, and lower-quality results.
- Treat interruption as exceptional. Redirect or stop a subagent only when it is clearly blocked, working the wrong scope, producing repeated low-value output, or the user explicitly changes direction.
- Independent reviewers and other independence-sensitive subagents must start from fresh context by default. Do not fork them from the current session when the point of the task is independent judgment.
- Reserve `fork_context=true` for continuation work that explicitly depends on current-session state. If the subagent is meant to be independent, pass only the minimal task statement and concrete repo paths or artifacts it should inspect.
- Do not describe a subagent as independent if it inherited the parent session's full context, conclusions, or preferred answer shape. Independence means the agent can reach its own judgment from repo truth.
- After any compaction, context reset, or session recovery, inventory existing subagents before spawning fresh ones.
- That inventory should answer three questions first: which agents are still running, which agents already completed but have unread results, and which agents are blocked but still hold useful context.
- Reuse relevant in-flight or recently completed subagents whenever possible. Do not spawn replacement workers or reviewers until you have confirmed the existing agents cannot supply the needed result.
- In multi-agent work, treat existing subagent progress as part of the authoritative session state. Preserving continuity is usually safer and cheaper than restarting the same slice from scratch.

## Rust and Lint Policy

- The Rust codebase is expected to pass `cargo clippy --all-targets --all-features -- -D warnings`.
- The strict Clippy policy is intentional for this repository. Do not weaken `[lints.clippy]`, add allow-list entries in `Cargo.toml`, or introduce `#[allow(clippy::...)]` suppressions without explicit user approval.
- Prefer fixing offending code by refactoring helper inputs, collapsing control flow, boxing oversized enum variants, or simplifying expressions rather than suppressing the lint.
- Keep builds warning-clean under `cargo test` as well; unused variables, dead code, and stale helper paths should be cleaned up, not ignored.

## Verification Expectations

- For Rust code changes, default verification is:
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - targeted `cargo test` commands for changed areas
- For performance-sensitive Rust changes, also measure plain `cargo test` and investigate regressions instead of assuming the suite cost is fixed.
- When performance work touches repo or workflow IO, prefer shared helpers, `gix`, memoization, and cached fixture templates over fresh subprocesses or repeated filesystem scans when the contract does not require the boundary.
- For skill template changes, also run:
  - `node scripts/gen-skill-docs.mjs`
  - `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- For workflow-boundary or plan-execution changes, favor targeted tests first, then broader suites once local regressions are closed.

## FeatureForge-Specific Notes

- If a change touches plan execution, review gating, or authoritative artifact handling, inspect both runtime code and the matching tests.
- When editing surfaces used by the workflow runtime, preserve repo-relative path normalization and artifact fingerprint invariants.
- If a change affects execution guidance, keep `writing-plans`, `executing-plans`, and related review/dispatch docs consistent with the runtime behavior.
- Runtime-owned strategy checkpoint and deviation-truthing hardening is an execution contract surface. Do not remove or downgrade it as "out-of-plan cleanup" without explicit user direction.

## Review Bar

- Before calling work complete, verify both implementation correctness and workflow correctness.
- Fresh independent review is preferred for material workflow or trust-boundary changes.
- If a reviewer finds real issues, fix them in code or tests; do not paper over them with policy exceptions.
- Reviews for this repository should explicitly check for three recurring failure modes:
- Reviews should treat any direct-helper vs real-CLI divergence as a bug unless the divergence is required for a boundary test and documented inline.
  - duplicate truth derivation across surfaces that should share a single authoritative decision
  - repeated immutable IO or missed memoization in runtime hot paths
  - semantic tests that still shell out even though the subprocess boundary is not part of the contract under test
  - git subprocess usage or repeated repo discovery in runtime/test hot paths where a shared helper or `gix` path could preserve semantics with less IO
