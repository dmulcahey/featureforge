# Pre-Landing Review Checklist

Review the diff against the provided authoritative base branch, not just the last commit. Read the full diff before commenting, then read outside the diff when a checklist item requires broader context.

Use the FeatureForge severity taxonomy:

- `Critical` for must-fix issues that can break correctness, safety, trust boundaries, or data integrity
- `Important` for issues that should be fixed before landing because they weaken maintainability, testability, or expected behavior
- `Minor` for lower-risk follow-ups, stale docs, and TODO capture that should not be silently lost

## Review Passes

### Pass 1 — Critical

#### Approved Task Contract & Reuse Law
- For plan-routed work, apply `review/plan-task-contract.md` as the authoritative task-contract and reuse law.
- Treat avoidable duplicate implementation of substantive production behavior as a hard fail when a shared implementation is practical and architecturally correct.
- Require any duplicate-implementation exception to name one approved exception category from the shared contract and the boundary rationale.
- Require findings to name the duplicated behavior, the shared home that should own it, why duplication is harmful, and the smallest defensible consolidation path.
- Scope this hard-fail rule to substantive production behavior such as parsers, normalizers, validators, routing logic, eligibility logic, policy enforcement, prompt assembly, shared state transitions, artifact binding, and freshness decisions.
- Fail the review when the same semantic rule, normalization, freshness decision, routing rule, or artifact-binding rule is implemented in multiple places instead of one shared helper or authoritative type.
- Fail the review when a new local helper partially re-expresses behavior already available from an existing shared helper, central decision path, or authoritative contract type.
- Fail the review when test-only, CLI-only, or adapter-only logic drifts from the production helper path even though that boundary is not what the test or adapter is exercising.
- Do not apply this hard-fail rule to generated code, fixtures or test data, tiny test-only setup repetition, platform-specific adapters with an explicit boundary, compatibility shims in a controlled migration window, or deliberate architectural separation required by an explicit layer or boundary law.

Examples:
- Hard fail: a diff adds a second repo-relative path normalizer for review packets while `src/paths` already owns canonical normalization. The finding names the duplicated normalization behavior, `src/paths` as the shared home, the drift risk, and the smallest consolidation path.
- Allowed exception: generated schema output repeats field names produced from one source template. The reviewer states the `generated code` exception and verifies the generated file points back to its source.

#### SQL & Data Safety
- String interpolation in SQL, even when the values were pre-coerced
- TOCTOU check-then-set patterns that should be atomic
- Validation-bypassing writes on fields that should preserve invariants
- Obvious N+1 query regressions in newly introduced loops or views

#### Race Conditions & Concurrency
- Read-check-write flows without uniqueness protection or retry handling
- `find_or_create_by` style helpers on columns without a unique index or equivalent guard
- Status transitions that are not atomic against the prior state
- Unsafe rendering helpers on user-controlled content

#### LLM Output Trust Boundary
- LLM-produced values written to storage or external systems without validation
- Structured tool output accepted without type or shape checks
- Prompt-driven behavior changes without corresponding guardrails or evaluation expectations

#### Enum & Value Completeness
When the diff introduces a new enum value, status, tier, type, or constant family:
- Trace every consumer, including code outside the diff
- Check allowlists, filters, branching logic, render paths, and persistence paths
- Flag any consumer that silently drops, misclassifies, or defaults the new value

### Pass 2 — Important or Minor

#### Conditional Side Effects
- Branches that forget a side effect in one path
- Logs that claim an action happened when it was conditionally skipped

#### Magic Numbers & String Coupling
- Bare literals repeated across files without a shared definition
- Strings that are duplicated in code and tests as control signals

#### Shared Runtime Reuse & Convergence
- The hard-fail reuse law is in Pass 1. Use this pass only to catch lower-risk convergence notes that do not duplicate substantive production behavior.
- Repeated strings or tiny test setup that are harmless today but likely to become control signals
- Inline boundary comments that should name an approved exception category more precisely even though the implementation is otherwise centralized

#### Dead Code & Consistency
- Assigned-but-unused values
- Comments or docs that now describe the wrong behavior
- Version, CHANGELOG, or feature-summary text that contradicts the implementation

#### LLM Prompt Issues
- Prompt instructions that list the wrong tools, options, or limits
- Numbering or formatting patterns likely to produce unstable LLM output
- Prompt or eval changes that do not state which evaluation coverage must move with them

#### Built-in Before Bespoke / Known Pattern Footguns
- custom auth or session handling that bypasses framework protections
- custom retry, debounce, cache, queue, or state logic where the platform already offers a stable primitive
- a newly introduced pattern with well-known failure modes in the current ecosystem

#### Test Gaps
- Missing negative-path tests
- Assertions that only check presence, not correctness of outputs or side effects
- Missing tests for auth, rate limits, blocking rules, trust boundaries, or other enforcement behavior

#### Crypto & Entropy
- Weak randomness for security-sensitive values
- Truncation or comparison patterns that reduce entropy or leak timing information

#### Time Window Safety
- Date-keyed logic that silently drops part of the intended window
- Related features using mismatched time windows for the same concept

#### Type Coercion at Boundaries
- Unstable types crossing storage, JSON, API, or language boundaries
- Hashing or serialization inputs that do not normalize type first

#### View / Frontend
- Expensive lookups in render loops
- Inline styling or view logic that should stay out of hot render paths
- User-visible state changes without clear loading, error, or back-button handling

#### Documentation Staleness
- Root docs such as `README.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, or platform install docs that describe code touched by this diff but were not updated
- If found, flag the issue and suggest `featureforge:document-release`

#### Spec / Plan Delivery Content
- Draft specs that still dodge core delivery content such as problem statement, failure behavior, observability, rollout/rollback, risks, or acceptance criteria
- Draft plans that skip preconditions, validation strategy, documentation update expectations, evidence expectations, rollout/rollback thinking, or explicit risks
- Review changes that quietly lower these workflow quality bars without updating the corresponding review skills and tests
- Runtime-owned contract hardening added during execution/remediation (for example strategy checkpoints or authoritative deviation truthing) that is fully implemented and tested should not be rejected only because it was not spelled out in the original approved plan

#### Release Readiness
- Workflow-routed changes that should have a required `document-release` handoff before completion but still treat release docs as optional cleanup
- Missing release notes, rollout notes, rollback notes, or operator-facing caveats when the diff changes public or operational behavior
- Completion flows that skip monitoring or verification expectations for changes with operational risk

#### TODO Cross-Reference
- Open TODOs that this diff should clearly close or reference
- New follow-up work revealed by the diff that should not be silently forgotten
- If found, capture it as `Minor` unless the missing follow-up blocks safe landing

## Output Rules

- Be specific: cite `file:line` when possible
- Do not flag issues already fixed in the diff
- For each finding, say what is wrong and the smallest defensible fix
- Keep the review terse and technical
- If nothing is wrong, say so explicitly

## Suppressions — Do Not Flag

- Harmless readability duplication
- Comment requests that only explain tuning values
- Cosmetic assertion tightening when the behavior is already proven
- Empirical threshold changes without a concrete regression
- Anything already addressed in the diff you are reviewing
