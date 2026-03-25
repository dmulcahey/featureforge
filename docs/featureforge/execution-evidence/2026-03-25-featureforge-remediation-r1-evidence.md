# Execution Evidence: 2026-03-25-featureforge-remediation

**Plan Path:** docs/featureforge/plans/2026-03-25-featureforge-remediation.md
**Plan Revision:** 1
**Plan Fingerprint:** 5c0f06cdbbd9c2f2e1e0eeb6f453fa69b712b73f25262b72af8448d69198da47
**Source Spec Path:** docs/featureforge/specs/2026-03-25-featureforge-remediation-design.md
**Source Spec Revision:** 1
**Source Spec Fingerprint:** c2add7e34ba89cfc53bc22923191fc787b5761a6829a592c11a470d6bd6c23d2

## Step Evidence

### Task 1 Step 1
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T14:40:40.962297Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 1
**Packet Fingerprint:** bad851aa35474faac27cc65c0e07574ed7a233b614a5e0907aba2b20a28cf604
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Added direct runtime-root helper contract tests covering resolved, unresolved, and named failure paths.
**Files Proven:**
- tests/runtime_root_cli.rs | sha256:f5e66d4ee0dab038f018a5006634844489f0045ecf1598e99cbd83417d15d6e8
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test runtime_root_cli fail on the missing repo runtime-root command and generic invalid-input behavior, which is the intended red state for Step 1.
**Invalidation Reason:** N/A

### Task 1 Step 2
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T14:43:19.968577Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 2
**Packet Fingerprint:** 1d4bfd6156a1b482a188391339140d42c8b66e22bd602cbb4e9f64c9b0913fef
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Added update-check regressions for version-only roots, repo-local runtimes, binary-adjacent runtimes, and invalid FEATUREFORGE_DIR overrides.
**Files Proven:**
- tests/update_and_install.rs | sha256:6371162e176280368264127b1985d80f3298adfc5a4a8078e8583647dcc7802b
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test update_and_install fail on VERSION-only roots being treated as installs, invalid FEATUREFORGE_DIR still emitting upgrade output, and binary-adjacent discovery still missing, while the valid repo-local and USERPROFILE paths stayed green.
**Invalidation Reason:** N/A

### Task 1 Step 3
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T14:44:10.405377Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 3
**Packet Fingerprint:** 9ffbfafba29cec76dd2c5fb8f749dc0e7352b6b17876a7d56210b840fac997dd
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Added a runtime-root schema parity regression to packet_and_schema.
**Files Proven:**
- tests/packet_and_schema.rs | sha256:82f46b94791661e3fb1195aee5bb11b20ba95eeaa50553efc62600c3a4a5f706
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test packet_and_schema fail because repo-runtime-root.schema.json is not generated yet, which is the intended red state for Step 3.
**Invalidation Reason:** N/A

### Task 1 Step 4
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T14:46:04.342103Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 4
**Packet Fingerprint:** 36a7b50622244d7a0664d5b7e04f2b67ec452acdfbce57207395e74126821e65
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Updated Node generator tests to require the runtime-root helper contract and helper-based upgrade instructions.
**Files Proven:**
- tests/codex-runtime/gen-skill-docs.unit.test.mjs | sha256:115c84503439cd4c0fa6197bd0f7391f651bbe36a20a0f00d46e9611b7bd8f49
- tests/codex-runtime/skill-doc-generation.test.mjs | sha256:75a2f2fd907fe8b560ddc1b891422dc412dce6e318bb60421fab46bd9f1d75c7
**Verification Summary:** Manual inspection only: Observed the targeted Node suites fail because shared shell builders still embed root-search order and featureforge-upgrade/SKILL.md still hardcodes legacy-root install discovery instead of calling the runtime-root helper.
**Invalidation Reason:** N/A

### Task 1 Step 5
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T14:55:59.604231Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 5
**Packet Fingerprint:** b03c9b90c7ce34e0cda0d6a9a9d9c484a352e0839979c4d6a551fb882b9ac1e0
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Implemented bounded runtime-root resolution and schema generation wiring.
**Files Proven:**
- src/contracts/packet.rs | sha256:629a5e6ba7af6107340d0cc731c2db07a576798a627c358dda3ac68de9d78a55
- src/lib.rs | sha256:29a605b9e1751fa00d8b4c2e42436691ab2fc6744adc3ce85f614285f9d3863f
- src/runtime_root/mod.rs | sha256:9169e5b52d926c5b257264feddba8279d0638a8b30acc769ddc91ba6d8990607
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test packet_and_schema compile the new runtime-root module, generate the runtime-root schema, and fail only because schemas/repo-runtime-root.schema.json is not checked in yet, which is the intended post-Step-5 state.
**Invalidation Reason:** N/A

### Task 1 Step 6
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T14:57:14.550252Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 6
**Packet Fingerprint:** 451bca4ccb1f05888c23a9cf892f85c0ebc1e3823dad8b0662e9070ec266e627
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Added repo runtime-root CLI plumbing and routed it to the runtime resolver.
**Files Proven:**
- src/cli/mod.rs | sha256:7a296ce02d1e3a87e767846b102409a375c2053a608b5731015d738b83e74a27
- src/cli/runtime_root.rs | sha256:89cbe1bcbdd8c7462ac3c6b57324765747037690fbac75bc54f729f3f65638d8
- src/lib.rs | sha256:002d018ac2e1c4e517617b4c7f8f7929b1c962b7f332f13681f51e65f1ec32da
**Verification Summary:** `cargo nextest run --test runtime_root_cli` -> 3 passed, 0 failed
**Invalidation Reason:** N/A

### Task 1 Step 7
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T14:59:27.816059Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 7
**Packet Fingerprint:** 1a2ed36afc4b46a6635da00c9d36dcb628bbb267873c39069964323847543dfe
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Made update-check use runtime-root resolution and updated its fixtures to require valid runtime roots.
**Files Proven:**
- src/update_check/mod.rs | sha256:a2aab45438899455a55de8088a6192554bb09165ad992027b334aac914e58d92
- tests/update_and_install.rs | sha256:e6e9581025b5e24bacf0b04208606277e199d8d40b32c1f0b48858845b8283b2
**Verification Summary:** `cargo nextest run --test update_and_install` -> 7 passed, 0 failed
**Invalidation Reason:** N/A

### Task 1 Step 8
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:03:44.663847Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 8
**Packet Fingerprint:** c2969b5db2361dca7c2a9499c2930c7584d6a5bd89b663b330c834f5f41f6876
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Migrated generated skill preambles and upgrade instructions to the runtime-root helper and regenerated the checked-in skill docs.
**Files Proven:**
- featureforge-upgrade/SKILL.md | sha256:2924eb7434d59ba7d719300e2bd877e49ea0155335bdae20cc01319039661af0
- scripts/gen-skill-docs.mjs | sha256:17fb8f5817c44824847cd432deff57ecf5452c70dface997621e3167481a23f6
- skills/executing-plans/SKILL.md | sha256:891df6d373907f04bf17d7c315908241a26dc9c701226e54e2af7466d48b27f2
- skills/using-featureforge/SKILL.md | sha256:f65c018fff3653a7ecc4cd856178c65e4cbb5764d82a4d4391a9ed235bd11185
- tests/codex-runtime/gen-skill-docs.unit.test.mjs | sha256:115c84503439cd4c0fa6197bd0f7391f651bbe36a20a0f00d46e9611b7bd8f49
- tests/codex-runtime/skill-doc-contracts.test.mjs | sha256:cd2550c7bfd54c9d914e09eceebac0fb7134b2c9e8308d32a544de0039f10e4e
- tests/codex-runtime/skill-doc-generation.test.mjs | sha256:75a2f2fd907fe8b560ddc1b891422dc412dce6e318bb60421fab46bd9f1d75c7
**Verification Summary:** `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs` -> 41 passed, 0 failed
**Invalidation Reason:** N/A

### Task 1 Step 9
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:06:01.104862Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 9
**Packet Fingerprint:** daac290a262fa735c2e59567ced95696bbcda5a59186f375cf7580ad392c2106
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Checked in the generated runtime-root schema artifact.
**Files Proven:**
- schemas/repo-runtime-root.schema.json | sha256:688cbb4f6871cef68eebf28f2082f955a9542d624be7802eeb4282f87babc2fc
**Verification Summary:** `cargo nextest run --test packet_and_schema` -> 7 passed, 0 failed
**Invalidation Reason:** N/A

### Task 1 Step 10
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:11:58.052737Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 10
**Packet Fingerprint:** f50e0746d0cc041558b236a561e54bd5031ed2217e7bde5789e6dd70794acbd6
**Head SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Base SHA:** 887fe6af91b80cf10f713c3451363ef2eddf69e5
**Claim:** Ran the full Task 1 verification slice to green after aligning upgrade-skill tests with the helper contract.
**Files Proven:**
- __featureforge__/no-repo-files | sha256:none
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test runtime_root_cli --test update_and_install --test upgrade_skill --test packet_and_schema pass 19/19, and node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs pass 16/16.
**Invalidation Reason:** N/A

### Task 1 Step 11
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:13:01.56803Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 1
**Step Number:** 11
**Packet Fingerprint:** 981a1ab60ec662a3cdc84a33f00688e3563e777d415280ff865ba2f8a44ee200
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Committed the full Task 1 runtime-root helper slice.
**Files Proven:**
- __featureforge__/no-repo-files | sha256:none
**Verification Summary:** Manual inspection only: Created git commit 0c4d9e9 (feat: add runtime-root helper contract) after the Task 1 Rust and Node verification suites were green.
**Invalidation Reason:** N/A

### Task 2 Step 1
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:23:06.52407Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 2
**Step Number:** 1
**Packet Fingerprint:** cc5c78f010c009a44d57fd34a3dd8f1510350625ebc9910d04cccab826487467
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Added red Rust tests for spawned-subagent default bypass, explicit opt-in, and nested workflow noise suppression.
**Files Proven:**
- tests/session_config_slug.rs | sha256:38d5cecaa9675ce5c37e8a30c1888956850f25802df34e16901e13cbfca4ae74
- tests/workflow_runtime.rs | sha256:528f494c99abd43e62530d1533c0ca054d0befb8d70096fd178f0de4e116f6c2
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test session_config_slug --test workflow_runtime fail because --spawned-subagent/--spawned-subagent-opt-in are not implemented yet, and cargo nextest run --test workflow_runtime canonical_workflow_operator_suppresses_session_entry_gate_for_spawned_subagent_context still reports phase needs_user_choice instead of bypassed.
**Invalidation Reason:** N/A

### Task 2 Step 2
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:25:29.347574Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 2
**Step Number:** 2
**Packet Fingerprint:** 3c94af1a83e977691a7186fa3d5569b66c39eff3f60525ee89a28e299a959ce1
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Added red doc-contract assertions for the spawned-subagent marker path across using-featureforge and launcher-facing skill docs.
**Files Proven:**
- tests/runtime_instruction_contracts.rs | sha256:ff5144a6b78a4f4afd3a99d65ff94dce6c7b913fbff5cb4da8f944cc0840ba53
- tests/using_featureforge_skill.rs | sha256:0981ebd7c9556666a68cd160ffbe840e5df5c0722c97684b2a806feb7f6abd18
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test using_featureforge_skill --test runtime_instruction_contracts fail because the generated docs do not yet mention --spawned-subagent or launcher marker wiring, and the targeted run also surfaced an existing using-featureforge preamble regression where repo-local runtime-root discovery emits an empty FEATUREFORGE_ROOT when only the repo checkout is available.
**Invalidation Reason:** N/A

### Task 2 Step 3
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:28:15.686614Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 2
**Step Number:** 3
**Packet Fingerprint:** a1ab95ad854cfa1301e9e8a02d0b3d1149c071cbbae2648e6768070ebb47a3e9
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Added explicit spawned-subagent and spawned-subagent-opt-in resolve inputs at the session-entry CLI parse boundary.
**Files Proven:**
- src/cli/session_entry.rs | sha256:ba8b5259e69cac60dcd6feec59a38f8ec8fa3cb50d5b2b6065d10b51dedd034d
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test session_config_slug canonical_session_entry_spawned_subagent_bypasses_bootstrap_without_persisting canonical_session_entry_spawned_subagent_opt_in_reenables_featureforge_with_distinct_source --test workflow_runtime canonical_workflow_operator_suppresses_session_entry_gate_for_spawned_subagent_context pass after wiring the new resolve flags into clap.
**Invalidation Reason:** N/A

### Task 2 Step 4
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:28:26.619784Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 2
**Step Number:** 4
**Packet Fingerprint:** dc79bce1ee0838e5f5063ad39cb8a049f111cbf1c32987cb3cf866d2d3dcb48b
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Implemented runtime-owned spawned-subagent bypass with ephemeral default behavior and a distinct explicit opt-in persistence path.
**Files Proven:**
- src/session_entry/mod.rs | sha256:89f039013124f30001c9d86adb7e38dc4d078e81372f0e1bd7b284bbb4c653b3
- tests/session_config_slug.rs | sha256:38d5cecaa9675ce5c37e8a30c1888956850f25802df34e16901e13cbfca4ae74
- tests/workflow_runtime.rs | sha256:528f494c99abd43e62530d1533c0ca054d0befb8d70096fd178f0de4e116f6c2
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test session_config_slug canonical_session_entry_spawned_subagent_bypasses_bootstrap_without_persisting canonical_session_entry_spawned_subagent_opt_in_reenables_featureforge_with_distinct_source --test workflow_runtime canonical_workflow_operator_suppresses_session_entry_gate_for_spawned_subagent_context pass after the session-entry runtime began honoring nested-context markers and workflow inspection started surfacing bypassed instead of needs_user_choice.
**Invalidation Reason:** N/A

### Task 2 Step 5
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:32:21.191014Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 2
**Step Number:** 5
**Packet Fingerprint:** 0bee445f0b15b7dbcfcca3cd75a70839cc3b4645ed8b1343b32761603f71f9d5
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Enumerated the session-entry outcome and decision-source schema contract and refreshed the checked-in session-entry schema artifact.
**Files Proven:**
- schemas/session-entry-resolve.schema.json | sha256:be57b99b7ef2e0fbada33c0dbe534953b7444a26ebe07f277b8d968a9ced7f07
- src/session_entry/mod.rs | sha256:a7a2b1201d1ac40293c76ec55a0ac896af4dc7bcaaa6dddc481452d0a28db7ea
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test packet_and_schema checked_in_repo_safety_and_session_entry_schemas_match_generated_output pass after the session-entry schema began enumerating nested-session outcomes and decision-source values.
**Invalidation Reason:** N/A

### Task 2 Step 6
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:36:17.552157Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 2
**Step Number:** 6
**Packet Fingerprint:** bf3446ec968dc74650babb803153edfcd21affc8df340d18ca62eac9cb9db026
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Updated the shared skill-doc generator and regenerated the launcher-facing skill docs to document the spawned-subagent marker path while restoring repo-local runtime-root preamble resolution.
**Files Proven:**
- scripts/gen-skill-docs.mjs | sha256:a014968ab758aa3d14f3918ca9d182ab338bd588c92e73f767d6828fc4546b4b
- skills/dispatching-parallel-agents/SKILL.md | sha256:5e605069b974261235d28f31891a1c2df954777278292e2f065a6587714ce402
- skills/dispatching-parallel-agents/SKILL.md.tmpl | sha256:491c680f912b5fd48be55415357e5232f37071fe609444e919035ad5706aa858
- skills/subagent-driven-development/SKILL.md | sha256:848e0905b31f43e575d9159cb8bd7bf990bfc33755ee2bcfb46a2e5cd8606114
- skills/subagent-driven-development/SKILL.md.tmpl | sha256:44a5129b7f8ca21883b6076d8b779d4b71a00852b9ac05eeabb56a947841571b
- skills/using-featureforge/SKILL.md | sha256:a60b59e4b6daa25e335065819c9d18caa2ecb8422a155f449e26b559b9feeb8e
- skills/using-featureforge/SKILL.md.tmpl | sha256:224358a6bb86a2a154cf1992c9262c57d0389e27763817fcfb4533b9f4c0eefe
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test using_featureforge_skill --test runtime_instruction_contracts using_featureforge_skill_documents_and_derives_the_canonical_bypass_gate using_featureforge_preamble_recognizes_the_repo_checkout_as_a_runtime_root using_featureforge_preamble_prefers_valid_repo_roots_over_fallback_installs spawned_subagent_marker_contracts_are_documented_consistently pass after regenerating the skill docs from the updated generator and templates.
**Invalidation Reason:** N/A

### Task 2 Step 7
#### Attempt 1
**Status:** Completed
**Recorded At:** 2026-03-25T15:36:28.586195Z
**Execution Source:** featureforge:executing-plans
**Task Number:** 2
**Step Number:** 7
**Packet Fingerprint:** 0751ce98f8357ba913c8263383adf05ab09a6185948a322143d7bb009c5b7e21
**Head SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Base SHA:** 0c4d9e91407d31f86ed4bbf508bade77fa0797cc
**Claim:** Ran the full Task 2 verification slice to green after landing the runtime, schema, and skill-doc contract updates.
**Files Proven:**
- __featureforge__/no-repo-files | sha256:none
**Verification Summary:** Manual inspection only: Observed cargo nextest run --test using_featureforge_skill --test session_config_slug --test workflow_runtime --test runtime_instruction_contracts --test packet_and_schema pass 83/83.
**Invalidation Reason:** N/A
