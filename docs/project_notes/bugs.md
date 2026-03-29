# Bugs

- 2026-03-29: Review remediation can strand execution when a later parked step blocks reopening earlier completed work.
  Root cause: `reopen` refuses a second interrupted step while `begin` refuses to bypass a different interrupted step.
  Fix: clear or avoid the downstream parked note before reopening the earlier completed step.
  Prevention / verification: keep the per-step review-gate follow-up tracked and add contract coverage for review-before-advance execution.
  Source: `src/execution/mutate.rs`, `TODOS.md`

- 2026-03-29: `plan-eng-review` skill guidance drifted from the runtime write-target names.
  Root cause: the generated skill text kept `repo-file-write` while the runtime CLI exposed `plan-artifact-write` for plan-body writes and `approval-header-write` for the approval flip.
  Fix: use the runtime truth during execution and track the repo-level remediation until the contract surfaces align again.
  Prevention / verification: keep skill-doc contract coverage on repo-safety write targets so guidance and runtime names fail together.
  Source: `TODOS.md`

