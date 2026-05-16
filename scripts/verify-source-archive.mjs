#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MODULE_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(MODULE_DIR, '..');

const REQUIRED_PROMPT_COMPANION_SOURCE_ARCHIVE_PATHS = [
  'docs/featureforge/reference/2026-04-01-review-state-reference.md',
  'qa/references/issue-taxonomy.md',
  'references/agent-grounding.md',
  'references/contributor-mode.md',
  'references/debugging-tdd-examples.md',
  'references/execution-review-qa-examples.md',
  'references/operator-route-authority.md',
  'references/plan-ceo-review-rubric.md',
  'references/plan-eng-review-rubric.md',
  'references/reviewer-recursion-rule.md',
  'references/search-before-building.md',
  'references/writing-plans-examples.md',
  'review/TODOS-format.md',
  'review/checklist.md',
  'review/late-stage-precedence-reference.md',
  'review/plan-task-contract.md',
  'review/review-accelerator-packet-contract.md',
  'skills/brainstorming/visual-companion.md',
  'skills/plan-ceo-review/accelerated-reviewer-prompt.md',
  'skills/plan-ceo-review/outside-voice-prompt.md',
  'skills/plan-eng-review/accelerated-reviewer-prompt.md',
  'skills/plan-eng-review/outside-voice-prompt.md',
  'skills/plan-fidelity-review/reviewer-prompt.md',
  'skills/project-memory/authority-boundaries.md',
  'skills/project-memory/examples.md',
  'skills/project-memory/references/bugs_template.md',
  'skills/project-memory/references/decisions_template.md',
  'skills/project-memory/references/issues_template.md',
  'skills/project-memory/references/key_facts_template.md',
  'skills/requesting-code-review/code-reviewer.md',
  'skills/subagent-driven-development/code-quality-reviewer-prompt.md',
  'skills/subagent-driven-development/implementer-prompt.md',
  'skills/subagent-driven-development/spec-reviewer-prompt.md',
  'skills/systematic-debugging/condition-based-waiting.md',
  'skills/systematic-debugging/defense-in-depth.md',
  'skills/systematic-debugging/root-cause-tracing.md',
  'skills/test-driven-development/testing-anti-patterns.md',
  'skills/using-featureforge/references/codex-tools.md',
  'skills/writing-skills/codex-best-practices.md',
  'skills/writing-skills/copilot-best-practices.md',
  'skills/writing-skills/examples/AGENTS_MD_TESTING.md',
  'skills/writing-skills/persuasion-principles.md',
  'skills/writing-skills/testing-skills-with-subagents.md',
];

const REQUIRED_SKILL_COMPANION_ASSET_SOURCE_ARCHIVE_PATHS = [
  'skills/brainstorming/scripts/featureforge-pwsh-common.ps1',
  'skills/brainstorming/scripts/frame-template.html',
  'skills/brainstorming/scripts/helper.js',
  'skills/brainstorming/scripts/server.js',
  'skills/brainstorming/scripts/start-server.ps1',
  'skills/brainstorming/scripts/start-server.sh',
  'skills/brainstorming/scripts/stop-server.ps1',
  'skills/brainstorming/scripts/stop-server.sh',
  'skills/systematic-debugging/find-polluter.sh',
  'skills/writing-skills/graphviz-conventions.dot',
  'skills/writing-skills/render-graphs.js',
];

const REQUIRED_SOURCE_ARCHIVE_PATHS = [
  'scripts/gen-agent-docs.mjs',
  'scripts/gen-skill-docs.mjs',
  'scripts/lint-workspace-runtime-evidence.mjs',
  'scripts/prebuilt-runtime-provenance.mjs',
  'scripts/run-codex-runtime-tests.mjs',
  'scripts/run-internal-runtime-compatibility-tests.sh',
  'scripts/run-public-runtime-flow-tests.sh',
  'scripts/verify-installed-control-plane-isolation.sh',
  'scripts/verify-source-archive.mjs',
  'docs/featureforge/archive/runtime-safety-audit-history/README.md',
  'docs/testing.md',
  ...REQUIRED_PROMPT_COMPANION_SOURCE_ARCHIVE_PATHS,
  ...REQUIRED_SKILL_COMPANION_ASSET_SOURCE_ARCHIVE_PATHS,
  'skills/skill-doc-budgets.json',
  'tests/codex-runtime/eval-observability.test.mjs',
  'tests/codex-runtime/gen-skill-docs.unit.test.mjs',
  'tests/codex-runtime/node-doc-contract-wrapper.test.mjs',
  'tests/codex-runtime/project-memory-content.test.mjs',
  'tests/codex-runtime/skill-doc-budget.test.mjs',
  'tests/codex-runtime/skill-doc-contracts.test.mjs',
  'tests/codex-runtime/skill-doc-generation.test.mjs',
  'tests/codex-runtime/workflow-fixtures.test.mjs',
  'tests/codex-runtime/helpers/markdown-test-helpers.mjs',
  'tests/evals/helpers/eval-observability.mjs',
  'tests/evals/helpers/openai-judge.mjs',
  'tests/evals/review-accelerator-contract.eval.mjs',
];

function isDirectScriptRun(importMetaUrl, argvPath) {
  if (!argvPath) {
    return false;
  }
  return fileURLToPath(importMetaUrl) === path.resolve(argvPath);
}

export {
  REQUIRED_PROMPT_COMPANION_SOURCE_ARCHIVE_PATHS,
  REQUIRED_SKILL_COMPANION_ASSET_SOURCE_ARCHIVE_PATHS,
  REQUIRED_SOURCE_ARCHIVE_PATHS,
  isDirectScriptRun,
};

function assertRegularFile(relativePath, failures) {
  const absolutePath = path.join(ROOT, relativePath);
  let stat;
  try {
    stat = fs.statSync(absolutePath);
  } catch (error) {
    failures.push(`${relativePath}: missing (${error.code ?? error.message})`);
    return;
  }
  if (!stat.isFile()) {
    failures.push(`${relativePath}: expected a regular file`);
  }
}

function main() {
  const failures = [];
  for (const relativePath of REQUIRED_SOURCE_ARCHIVE_PATHS) {
    assertRegularFile(relativePath, failures);
  }

  if (failures.length > 0) {
    console.error('Source archive validation failed:');
    for (const failure of failures) {
      console.error(`- ${failure}`);
    }
    process.exit(1);
  }

  console.log('Source archive validation passed.');
}

if (isDirectScriptRun(import.meta.url, process.argv[1])) {
  main();
}
