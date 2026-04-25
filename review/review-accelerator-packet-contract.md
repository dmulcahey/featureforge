# Review Accelerator Packet Contract

Shared reference for accelerated CEO and ENG review. The reviewer subagent drafts packets only; the main review agent remains the only authority that may write, apply, or approve anything.

## required packet fields

Every valid section packet must include:

- review kind
- canonical section name
- reviewer persona
- explicit user-initiation marker for acceleration
- routine findings
- escalated issues
- exact staged patch content
- staged patch summary
- source artifact path
- source artifact workflow state
- source artifact revision
- source artifact fingerprint
- human decision state
- timestamp

Routine findings and escalated issues must use the deterministic review finding
shape from `review/plan-task-contract.md` whenever they identify a concrete
contract failure. Each finding carries `Finding ID`, `Severity`, `Task`,
`Violated Field or Obligation`, `Evidence`, `Required Fix`, and `Hard Fail: yes|no`.
Do not use general advice when the packet can name the violated field or
packet-assigned obligation.

## ENG hard-fail fields

Accelerated ENG section packets must also include:

- analyze-plan boolean snapshot for `task_contract_valid`, `task_goal_valid`, `task_context_sufficient`, `task_constraints_valid`, `task_done_when_deterministic`, and `tasks_self_contained`
- task-contract hard-fail findings for missing `Goal`, `Context`, `Constraints`, `Done when`, `Spec Coverage`, or `Files`
- deterministic `Done when` assessment
- required spec-reference assessment under `review/plan-task-contract.md`
- self-contained task-scope assessment
- reuse assessment that names the existing shared implementation home, the reason no shared home exists, or the approved exception for separate implementations
- obligation-tied hard-fail findings using canonical `DONE_WHEN_N` and `CONSTRAINT_N` IDs when those packet obligations are violated

If any ENG hard-fail field is missing, malformed, or contradicts the normal `plan-eng-review` approval gate, treat the packet as invalid and fall back to normal manual review for that section.

## fail-closed validation rule

If any required packet field is missing, malformed, internally inconsistent, or unsupported for the active review kind, treat the packet as invalid and discard it. Invalid packets must fall back to normal manual review for that section before any staged patch is applied.

## high-judgment escalation categories

High-judgment issues must be broken out into direct human questions before section approval. At minimum, escalate:

- scope or ambition changes
- product or business tradeoff choices
- approval-boundary changes
- unresolved risk acceptance
- any decision that would otherwise silently pick among multiple plausible directions

Each escalated high-judgment issue must remain one issue per direct human question.

## main-agent-only write authority

The reviewer subagent may analyze and draft only. Only the main review agent may:

- write authoritative artifacts
- apply approved patches
- update persisted section packets
- change `Workflow State`
- write `CEO Approved` or `Engineering Approved`

## fallback classes that map to manual review

Accelerated failures must map to explicit fail-closed classes:

- `ReviewerInvocationFailure`
- `PacketValidationFailure`
- `PatchApplyFailure`
- `PacketPersistenceFailure`
- `ResumeFingerprintMismatch`
- `ResumeProofFailure`
- `UnexpectedAcceleratorFailure`

All of them fall back to normal manual review with the written artifact still authoritative.

## source artifact fingerprint

Each persisted section packet must record the source artifact fingerprint for the exact written spec or plan used to generate the packet.

## persisted packet location

Persist accelerator section packets under `~/.featureforge/projects/<slug>/...`.

## approved-and-applied section-boundary resume rule

Resume is allowed only when the user explicitly asks and only from the last approved-and-applied section boundary. Unapproved packets are diagnostic only and may not be replayed as if they were approved.

If the current written artifact no longer matches the recorded source artifact fingerprint, the saved packet is stale and must be regenerated before reuse.

## bounded retention

Accelerator artifacts must use bounded retention. Keep enough recent packet history to support auditability and resume for active or recently interrupted reviews, but do not allow persisted accelerator artifacts to grow as an unbounded local archive by default.
