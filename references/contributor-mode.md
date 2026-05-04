# Contributor Mode

Contributor mode is a lightweight feedback path for FeatureForge itself. It is not a workflow gate and it does not change the current task, repo, or user priority.

Use it only for unclear FeatureForge skill instructions, helper failures, install-root/runtime-root problems, contributor-mode bugs, or broken generated docs. Do not file reports for the user's application bugs, site authentication failures, flaky third-party outages, or normal project-specific defects.

Write reports under `~/.featureforge/contributor-logs/{slug}.md`. Slugs should be lowercase, hyphenated, and no longer than 60 characters, for example `skill-trigger-missed`. Skip the report if the file already exists. File at most 3 reports per session.

Use this template:

```markdown
# {Title}

Hey featureforge team - ran into this while using /{skill-name}:

**Goal:** {what the user/agent was trying to do}
**What happened:** {what actually happened}
**Annoyance (1-5):** {1=meh, 3=friction, 5=blocker}

## Steps to reproduce
1. {step}

## Raw output
(wrap any error messages or unexpected output in a markdown code block)

**Date:** {YYYY-MM-DD} | **Version:** {featureforge version} | **Skill:** /{skill}
```

After writing the report, continue the user's task and say: `Filed featureforge field report: {title}`.

Optional helper to open the report directory:

```bash
mkdir -p ~/.featureforge/contributor-logs
if command -v open >/dev/null 2>&1; then
  open ~/.featureforge/contributor-logs
elif command -v xdg-open >/dev/null 2>&1; then
  xdg-open ~/.featureforge/contributor-logs >/dev/null 2>&1 || true
fi
```
