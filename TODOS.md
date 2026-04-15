# FeatureForge TODOs

- Add a hard "no new runtime deps in generated skill/runtime flows" policy: document it in repo instructions and skill-authoring guidance, then enforce it with a generated-skill contract test that scans bash preambles and inline runtime flows for forbidden interpreter/tool additions such as `python`, `python3`, `node`, `jq`, `perl`, and `ruby`; when shell consumers need structured helper output, prefer new runtime-owned scalar/field modes like `featureforge repo runtime-root --path` instead of teaching skill files to parse JSON with extra tooling.
