# Plan Engineering Review Rubric

This reference carries detailed engineering-review prompts that do not need to live in the generated top-level skill. The top-level `plan-eng-review` skill remains authoritative for workflow headers, plan-fidelity sequencing, analyzer gates, protected-branch gates, QA handoff requirements, execution handoff, and approval law.

## Engineering Preferences

- Flag avoidable duplication and missing reuse of shared implementation homes.
- Prefer the smallest coherent plan that satisfies the approved spec.
- Require deterministic task contracts and objective `Done when` bullets.
- Favor explicit file ownership, serial hazards, and parallel worktree isolation over vague sequencing.
- Require tests and evidence for every meaningful path.

## Step 0 Review

Answer:

- What existing code already solves part of this plan?
- What is the minimum implementation that preserves the approved spec?
- Is the file count, class count, or service count a smell?
- Does `TODOS.md` contain blockers, related deferred work, or follow-up items?

Run the search check only for new/custom auth, cache, queue, concurrency, indexing, browser workaround, framework wrapper, infrastructure dependency, or unfamiliar integration patterns. Treat robust built-ins as simplification opportunities.

## Review Sections

### Architecture

Evaluate boundaries, dependency graph, data flow, scaling, single points of failure, security architecture, production failure scenarios, rollout, rollback, and whether diagrams belong in the plan or code comments.

### Code Quality

Evaluate organization, DRY, shared helper reuse, error patterns, edge cases, debt, over-engineering, under-engineering, ordered implementation steps, documentation movement, and evidence expectations.

Domain overlays:

- web/UI: user flow, loading/empty/error states, accessibility, responsiveness, browser validation
- API/service/backend: contracts, compatibility, timeouts, retries, rate limits, contract tests
- data/ETL: schema evolution, data quality, backfill/reprocessing, downstream compatibility
- infrastructure/IaC: blast radius, policy impact, drift, rollback, preview/post-change verification
- library/SDK: public API, versioning, consumer migration, breaking changes, packaging validation

### Tests

Build a coverage graph of new UX, data flow, code paths, branch outcomes, async work, and integrations. Every meaningful path must land in exactly one bucket: automated, manual QA, or not required with written justification.

For browser-visible work, require loading, empty, error, success, partial, navigation, responsive, and accessibility-critical checks where relevant.

For non-browser work, require compatibility, retry/timeout semantics, replay/backfill behavior, and rollback/migration verification where relevant.

### Performance

Check N+1 work, repeated fetches, memory, caching, slow paths, high-complexity paths, and resource contention.

## Finding Shape

Use this deterministic finding shape whenever possible:

```text
Finding ID:
Severity:
Task:
Violated Field or Obligation:
Evidence:
Required Fix:
Hard Fail: yes|no
```

Prefer exact analyzer booleans, task fields, packet obligations, or checklist law over general feedback.

## Required Output Templates

### Completion Summary

```text
Step 0:
Architecture Review:
Code Quality Review:
Test Review:
Performance Review:
NOT in scope:
What already exists:
TODO proposals:
Failure modes:
Test Plan Artifact:
Outside Voice:
Engineering Review Summary:
Unresolved decisions:
```

### Failure Modes

```text
CODEPATH | PRODUCTION FAILURE | TEST? | ERROR HANDLING? | USER IMPACT
```

If a failure has no test, no error handling, and silent impact, flag it as a critical gap.
