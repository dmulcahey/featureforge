# Execution Evidence: 2026-03-25-featureforge-remediation

**Plan Path:** docs/featureforge/plans/2026-03-25-featureforge-remediation.md
**Plan Revision:** 1
**Plan Fingerprint:** 78d51265d5ac9d8a24d039478ee3904610c1d060f76b063309e3903a5711afd4
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
