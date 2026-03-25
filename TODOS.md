# FeatureForge TODOs

- Remove the remaining internal cutover-only compatibility hooks, especially any legacy session or env names that still survive in code or test scaffolding.
- Add the final end-to-end cutover gate that blocks new forbidden legacy names in active file contents and active path names while ignoring archived history.
- Expand install smoke coverage for checked-in prebuilt artifacts on macOS arm64 and `windows-x64`.
- Auto-bypass the runtime-owned session-entry gate for spawned subagents unless they are explicitly opted back into FeatureForge, so dispatched review and audit agents do not get the first-turn bootstrap prompt.
- Add a strict first-entry session-entry gate for `using-featureforge`: the initial entry path must invoke `featureforge session-entry resolve --message-file <path>` before any normal stack, workflow routing, or approved-plan handoff logic runs; if the helper returns `needs_user_choice`, surface that question immediately and block later helpers from being the first place the missing decision appears; close the rough edge with an end-to-end test that proves a fresh session cannot reach spec review, plan review, or execution preflight without the gate firing first.
