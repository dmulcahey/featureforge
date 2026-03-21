# Using-Superpowers Session Bypass

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Summary

Add an explicit opt-out gate to the `using-superpowers` entry skill so the user can bypass the Superpowers stack before any normal Superpowers work happens.

The approved behavior for v1 is:

- ask once per session when `using-superpowers` first triggers
- if the user chooses bypass, skip the full Superpowers stack for the rest of that session
- a bypassed session skips update checks, session tracking, contributor-mode hooks, and workflow routing
- explicit user requests such as `use superpowers` or naming a Superpowers skill clear the bypass and re-enable the stack on that turn

This is a hard session-scoped escape hatch, not a soft preference.

## Problem

Today `using-superpowers` is intentionally strict. Once it triggers, the entry skill runs the full shared preamble and then enforces skill routing and workflow takeover before any other work happens.

That makes the workflow disciplined, but it removes user control in cases where they intentionally want a plain Codex session without the Superpowers stack taking over.

The missing capability is not "ignore one routing recommendation." The missing capability is "before Superpowers starts doing Superpowers things, let the user say no for this session."

## Goals

- Let the user opt out before the normal Superpowers stack runs.
- Make the decision session-scoped rather than sticky across sessions.
- Keep bypass deterministic rather than relying on model memory alone.
- Let the user re-enter Superpowers explicitly in the same session.
- Keep the implementation local and minimal: generated skill contract plus tests, not a new helper surface.
- Fail conservatively when bypass state cannot be read or written.

## Not In Scope

- A global or per-repo remembered bypass preference.
- A new public runtime helper just for entry-skill session state.
- A partial bypass mode that keeps update checks, contributor mode, or workflow routing while suppressing only selected parts of `using-superpowers`.
- Silent heuristic re-entry based on vague wording.
- Changes to the downstream workflow stages after `using-superpowers` hands off.

## Approved User Contract

When `using-superpowers` first triggers in a session, it must ask one interactive question before any normal Superpowers work happens.

The question should present a binary choice:

- use Superpowers for this session
- bypass Superpowers for this session

If the user chooses Superpowers:

- do not write a bypass flag
- continue through the normal `using-superpowers` stack on the same turn

If the user chooses bypass:

- write a session-scoped bypass flag
- stop `using-superpowers` immediately
- do not run update checks
- do not write or refresh Superpowers session markers
- do not load contributor-mode state
- do not perform skill routing or workflow-stage takeover

On later turns while bypass is active:

- if the user does not explicitly request Superpowers, `using-superpowers` should remain silent and stop before the normal stack
- if the user explicitly says `use superpowers` or explicitly names a Superpowers skill, clear the bypass flag and resume the normal stack on that same turn

## Session Scope And State Model

Bypass state is session-scoped and should use the same process-session identity family already used by the generated preamble.

Store the state as a sentinel file:

```text
~/.superpowers/session-flags/using-superpowers-bypass/$PPID
```

Rationale:

- this is deterministic and easy to inspect
- it does not pollute `config.yaml`, which is for durable preferences
- it matches the existing local-runtime pattern of small branch/session files under `~/.superpowers/`
- it avoids introducing a new helper binary for a single flag

## Entry Flow

The implementation should split the current entry behavior into two layers:

1. Minimal bootstrap
2. Full Superpowers stack

### Minimal Bootstrap

The bootstrap runs first and is the only part allowed to execute before the bypass decision resolves.

Responsibilities:

- detect the Superpowers root
- capture repo and branch grounding needed for the question
- derive the state directory
- derive the bypass sentinel path for the current session
- determine whether bypass is active
- determine whether the current user message is an explicit request to re-enter Superpowers
- ask the first-turn opt-out question when no bypass decision exists yet

The bootstrap must not:

- run `superpowers-update-check`
- create or refresh `sessions/$PPID`
- prune or count session markers
- load contributor-mode config
- perform workflow routing

### Full Superpowers Stack

This is the existing behavior after the bypass gate passes.

Responsibilities remain unchanged:

- update check
- session bookkeeping
- contributor-mode hooks
- skill routing and workflow-stage ownership

## Flow Diagram

```text
user message
    |
    v
minimal using-superpowers bootstrap
    |
    +--> explicit re-entry request while bypass flag exists?
    |       |
    |       +--> yes: clear bypass flag -> continue to full stack
    |
    +--> bypass flag exists?
    |       |
    |       +--> yes: stop and bypass Superpowers for this turn
    |
    +--> bypass decision missing?
            |
            +--> yes: ask opt-out question
                    |
                    +--> choose Superpowers -> continue to full stack
                    |
                    +--> choose bypass -> write bypass flag -> stop

full Superpowers stack
    |
    +--> update check
    +--> session bookkeeping
    +--> contributor mode
    +--> skill routing / workflow handoff
```

## Explicit Re-Entry Contract

Re-entry must be explicit, not heuristic.

The following should clear the bypass flag and resume normal behavior on the same turn:

- `use superpowers`
- direct naming of a Superpowers skill, such as:
  - `brainstorming`
  - `systematic-debugging`
  - `using-git-worktrees`
  - `plan-ceo-review`

The contract should be broad enough to respect direct user intent, but not so broad that generic words accidentally re-enable the stack.

This means the matching logic should prioritize clear references to:

- `superpowers`
- `superpowers:<skill-name>`
- the exact names of installed Superpowers skills

## Failure Behavior

Bypass state errors must fail closed to normal Superpowers behavior.

If the bootstrap cannot read or write the bypass sentinel safely:

- do not silently bypass
- continue to the normal `using-superpowers` stack
- preserve the conservative routing posture

This is important because accidental suppression of the entry router is riskier than an extra Superpowers question.

## Implementation Plan

### Generated Skill Runtime

Modify `scripts/gen-skill-docs.mjs` so `using-superpowers` can use a dedicated bootstrap preamble instead of the shared base preamble.

The dedicated bootstrap should include:

- runtime-root detection
- repo root and branch capture
- state-dir and bypass-path derivation

It should exclude:

- update-check execution
- session-marker creation and pruning
- contributor-mode reads

### `using-superpowers` Skill Template

Update `skills/using-superpowers/SKILL.md.tmpl` to define a top-level bypass gate immediately after the preamble.

That section should specify:

- ask before any other normal Superpowers behavior
- write the sentinel file when the user chooses bypass
- stop immediately after bypass is chosen
- remain silent on later turns while bypass is active
- clear the sentinel and proceed when the user explicitly re-enters

### Generated Skill Doc

Regenerate `skills/using-superpowers/SKILL.md` through `node scripts/gen-skill-docs.mjs`.

### Documentation Updates

Update user-facing docs that currently imply unconditional entry-router takeover:

- `README.md`
- `docs/README.codex.md`
- `docs/README.copilot.md`

These docs should describe the new session bypass briefly and accurately.

## Testing

Add or update tests at the contract level:

### `tests/codex-runtime/gen-skill-docs.unit.test.mjs`

- cover generation of the dedicated `using-superpowers` bootstrap preamble
- verify the special preamble differs intentionally from the shared base preamble

### `tests/codex-runtime/skill-doc-contracts.test.mjs`

- stop assuming every generated base preamble includes `_UPD`, `_SESSIONS`, and `_CONTRIB`
- add dedicated assertions for the `using-superpowers` bootstrap contract
- assert that the generated `using-superpowers` doc includes bypass-gate language, session-bypass path derivation, explicit re-entry handling, and the requirement to ask before normal Superpowers behavior

### `tests/codex-runtime/test-runtime-instructions.sh`

- require patterns covering the bypass gate
- require patterns covering the session-scoped sentinel path
- require patterns covering explicit re-entry behavior
- require wording that bypass happens before update checks, session tracking, contributor mode, and routing

Freshness validation remains required:

```bash
node scripts/gen-skill-docs.mjs --check
```

## Edge Cases

- User chooses bypass, then immediately asks for `brainstorming` on the next turn:
  clear bypass and continue through the normal stack on that turn.
- User chooses bypass, then asks a generic product question without naming Superpowers:
  keep bypass active.
- Bypass flag file is missing after the user previously chose bypass:
  treat the session as not bypassed and ask again if needed.
- Bypass flag path cannot be created because of filesystem or permission issues:
  fail closed to normal Superpowers behavior.
- User says something ambiguous like `use planning` without naming Superpowers:
  do not treat that as re-entry.

## Risks And Trade-Offs

- Matching explicit re-entry in prompt space is still an instruction contract, not a parser-level API. The wording needs to stay narrow and well-tested.
- Using `$PPID` is consistent with existing session markers, but it inherits the same session-identity limitations as the current runtime model.
- A file-sentinel solution is intentionally simple. If future entry-state needs become more complex, a dedicated helper may become worth the extra surface area.

## Deferred Follow-Ups

Record these as future considerations rather than part of v1:

- decide whether a repo-scoped or globally remembered bypass preference is ever desirable
- decide whether entry-session state should eventually move behind a runtime helper
- evaluate whether bypass behavior deserves dedicated eval scenarios beyond the existing skill-doc/runtime contract tests

