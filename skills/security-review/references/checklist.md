# Security Review Checklist

- Confirm the approved plan or workflow route requires security review.
- Identify the current branch, repo, plan path, and HEAD SHA.
- Review trust boundaries, auth/authz, secrets, subprocess or path handling, and integration surfaces touched by the diff.
- Record findings, severity, mitigations, and residual risk in a runtime-owned artifact.
- Use `Result: pass` only when no blocking security work remains.
- If required mitigation is still open, return to the implementation flow instead of letting final review proceed.
