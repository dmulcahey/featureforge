# Agent Grounding

Generated FeatureForge skills are installed into both Codex and GitHub Copilot discovery surfaces. The active repository instruction chain remains authoritative for local project behavior.

Before applying a review skill, honor the applicable instruction files in this order:

1. System and developer instructions from the active agent runtime.
2. Repository instructions such as `AGENTS.md`, `AGENTS.override.md`, `.github/copilot-instructions.md`, and `.github/instructions/*.instructions.md`.
3. Nested `AGENTS.md` or `AGENTS.override.md` files closer to the files being reviewed.
4. The generated skill instructions and companion references.

If these sources conflict, keep the higher-priority instruction and report any task-blocking ambiguity as a review finding instead of inventing a new workflow rule.
