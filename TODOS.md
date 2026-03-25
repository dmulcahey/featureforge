# FeatureForge TODOs

- Add a strict first-entry session-entry gate for `using-featureforge`: the initial entry path must invoke `featureforge session-entry resolve --message-file <path>` before any normal stack, workflow routing, or approved-plan handoff logic runs; if the helper returns `needs_user_choice`, surface that question immediately and block later helpers from being the first place the missing decision appears; close the rough edge with an end-to-end test that proves a fresh session cannot reach spec review, plan review, or execution preflight without the gate firing first.
