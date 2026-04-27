#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MODULE_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(MODULE_DIR, '..');

const REQUIRED_SOURCE_ARCHIVE_PATHS = [
  'scripts/gen-agent-docs.mjs',
  'scripts/gen-skill-docs.mjs',
  'scripts/verify-source-archive.mjs',
  'docs/testing.md',
  'tests/codex-runtime/eval-observability.test.mjs',
  'tests/codex-runtime/gen-skill-docs.unit.test.mjs',
  'tests/codex-runtime/project-memory-content.test.mjs',
  'tests/codex-runtime/skill-doc-contracts.test.mjs',
  'tests/codex-runtime/skill-doc-generation.test.mjs',
  'tests/codex-runtime/workflow-fixtures.test.mjs',
  'tests/codex-runtime/helpers/markdown-test-helpers.mjs',
  'tests/evals/helpers/eval-observability.mjs',
  'tests/evals/helpers/openai-judge.mjs',
  'tests/evals/review-accelerator-contract.eval.mjs',
];

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

main();
