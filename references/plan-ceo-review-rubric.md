# Plan CEO Review Rubric

This reference carries detailed CEO-review rubric material that does not need to be repeated in every generated top-level skill prompt. The top-level `plan-ceo-review` skill remains authoritative for workflow headers, protected-branch gates, terminal handoff, approval rules, and stop conditions.

## Review Posture

- SCOPE EXPANSION: propose the ambitious version and ask the user to opt in to each scope increase.
- SELECTIVE EXPANSION: hold the current scope as baseline, then offer each expansion independently.
- HOLD SCOPE: make the accepted scope resilient without adding or removing scope.
- SCOPE REDUCTION: identify the smallest version that still delivers the core user outcome.

In all modes, the user decides scope. Every scope change must be explicit.

## Engineering Preferences

- Flag avoidable duplication.
- Prefer explicit designs over clever designs.
- Require named failures, user-visible recovery, and tests for meaningful paths.
- Treat observability, rollback, and security as first-class scope.
- Prefer the smallest design that satisfies the approved outcome without hiding edge cases.
- Keep diagrams current when the spec touches complex data flow, state, or deployment behavior.

## Step 0 Prompts

Ask these before section review:

- Is this the right problem to solve?
- What existing code or workflow already solves part of it?
- What is the 12-month ideal state, and does the spec move toward it?
- What implementation decisions will be ambiguous in hour 1, hour 2-3, hour 4-5, and later polish/test work?
- Which review mode should apply?

## Section Rubrics

### Architecture

Check component boundaries, data flow, state machines, coupling, scaling, single points of failure, auth/data-access boundaries, production failure scenarios, and rollback posture. Require ASCII diagrams for non-trivial architecture, state, processing, dependency, and decision flows.

### Error And Rescue

For every new method, service, or code path that can fail, capture the failure trigger, named exception or error family, rescue behavior, user-visible result, logging context, retries, and whether the failure can become silent. Treat generic catch-all rescue without context as a critical gap.

### Security

Check attack surface, input validation, authorization, secret handling, dependency risk, data classification, injection vectors, and audit logging. For each finding, state threat, likelihood, impact, and mitigation status.

### Data And Interaction Edge Cases

Trace nil, empty, invalid, wrong type, too long, timeout, conflict, stale, partial, duplicate, and deploy-in-progress paths. For UI interactions, cover double-submit, navigation away, slow connection, retry while in flight, empty results, large results, and mid-page data changes.

### Code Quality

Review organization, naming, DRY, error handling, missing edge cases, over-engineering, under-engineering, and branch complexity. Cross-reference the error map when quality concerns are really failure-handling gaps.

### Tests

List new UX flows, data flows, code paths, async work, integrations, and error paths. For each, name the unit, integration, system, manual, or end-to-end coverage. Flag time, randomness, external services, ordering, or brittle browser assumptions.

### Performance

Check N+1 work, repeated fetches, memory growth, indexes or lookup support, caching, queue sizing, slow paths, and connection-pool pressure.

### Observability

Check logs, metrics, traces, alerts, dashboards, runbooks, admin tooling, and whether an operator can identify and repair failures without reading source code.

### Deployment

Check migration safety, feature flags, rollout order, rollback, environment parity, post-deploy verification, and smoke tests.

### Long-Term Trajectory

Check technical debt, reversibility, path dependency, knowledge concentration, ecosystem fit, and whether a new engineer can understand the design in 12 months.

### Design And UX

When `UI_SCOPE` is present, check information architecture, loading/empty/error/success/partial states, journey coherence, responsive behavior, accessibility, and whether the spec describes intentional product decisions rather than generic UI.

## Required Output Templates

### Error And Rescue Registry

```text
METHOD/CODEPATH | WHAT CAN GO WRONG | ERROR FAMILY | RESCUED? | RESCUE ACTION | USER SEES
```

### Failure Modes Registry

```text
CODEPATH | FAILURE MODE | RESCUED? | TEST? | USER SEES? | LOGGED?
```

Any row with no rescue, no test, and silent user impact is a CRITICAL GAP.

### Completion Summary

```text
Mode selected:
System audit:
Step 0:
Sections completed:
Critical gaps:
NOT in scope:
What already exists:
Dream state delta:
Error/rescue registry:
Failure modes:
TODO proposals:
Scope proposals:
Delight opportunities:
Outside voice:
CEO Review Summary:
Diagrams produced:
Stale diagrams:
Unresolved decisions:
```
