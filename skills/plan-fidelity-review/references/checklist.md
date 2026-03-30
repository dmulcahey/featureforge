# Plan-Fidelity Review Checklist

Use this checklist for the dedicated independent fidelity pass before engineering review.

## Inputs

- Exact approved spec path
- Exact current draft plan path
- Current spec revision
- Current plan revision

## Required checks

- Confirm the source spec is `CEO Approved` and `Last Reviewed By: plan-ceo-review`.
- Confirm the plan remains `Draft` for the current review pass.
- Confirm the plan's `Source Spec` path and `Source Spec Revision` match the approved spec exactly.
- Confirm the spec's full `Requirement Index` is represented in the plan's `Requirement Coverage Matrix`.
- Confirm each task's `Spec Coverage` stays faithful to the approved requirements and does not widen scope.
- Confirm the plan's `Execution Strategy` matches the `Dependency Diagram` and any stated topology claims.
- Confirm each `Files:` block is concrete, minimal, and aligned to the task's outcome.
- Confirm the spec and plan agree on `Delivery Lane` when that header is present.
- Confirm the reviewer stays distinct from `featureforge:writing-plans` and `featureforge:plan-eng-review`.
- Confirm the review artifact records `requirement_index` and `execution_topology` in `Verified Surfaces`.
- Confirm the review artifact also records `delivery_lane` in `Verified Surfaces` whenever the reviewed spec or plan declares `Delivery Lane`.

## Artifact requirements

- Record `Review Stage: featureforge:plan-fidelity-review`.
- Use `Review Verdict: pass` only when the draft plan is fidelity-clean for the current spec and plan revisions.
- Include the reviewed spec and plan paths, revisions, and fingerprints.
- Include verified requirement ids.
- Name requirement coverage gaps, topology concerns, and lane mismatches explicitly.
- Record concrete pass/fail rationale.

## After the checklist

- If any check fails, return control to `featureforge:writing-plans`.
- If every check passes, record the runtime-owned receipt with `featureforge workflow plan-fidelity record` and only then route to `featureforge:plan-eng-review`.
