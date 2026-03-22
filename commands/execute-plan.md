---
description: "Compatibility shim for legacy plan-execution command usage"
---

This command is a compatibility shim.

Use the public handoff surface `superpowers-workflow handoff` to identify the exact approved plan and the recommended execution path.

- If the handoff reports an exact approved plan plus a recommended execution path, route to that supported execution surface with the reported plan path.
- If the handoff does not report an approved plan yet, route back to the earlier supported workflow stage instead of treating this legacy alias as a direct execution bypass.
