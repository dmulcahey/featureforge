# FeatureForge TODOs

- Remove the remaining internal cutover-only compatibility hooks, especially any legacy session or env names that still survive in code or test scaffolding.
- Add the final end-to-end cutover gate that blocks new forbidden legacy names in active file contents and active path names while ignoring archived history.
- Expand install smoke coverage for checked-in prebuilt artifacts on macOS arm64 and `windows-x64`.
