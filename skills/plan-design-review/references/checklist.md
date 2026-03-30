# Plan Design Review Checklist

- Confirm the plan actually declares material UI scope or `Design Review Required: yes`.
- Confirm key user flows are named and bounded.
- Confirm loading, empty, error, and edge states are described when relevant.
- Confirm responsive behavior expectations are explicit when UI changes span device sizes.
- Confirm accessibility expectations are explicit enough for implementation and QA.
- Confirm design-system or interaction-pattern constraints are not missing.
- Confirm findings are recorded in a runtime-owned artifact bound to the current plan revision.
- Confirm the artifact records the current plan fingerprint so same-revision edits cannot reuse a stale pass.
- If major design gaps remain, return the plan to `featureforge:writing-plans`.
