import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { Buffer } from 'node:buffer';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '../..');
const require = createRequire(path.join(repoRoot, 'runtime/core-helpers/package.json'));
const { build } = require('esbuild');

async function loadModule(relativeEntryPath) {
  const entryPoint = path.join(repoRoot, relativeEntryPath);
  const result = await build({
    entryPoints: [entryPoint],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });

  const bundledSource = result.outputFiles[0].text;
  const dataUrl = `data:text/javascript;base64,${Buffer.from(bundledSource).toString('base64')}`;
  return import(dataUrl);
}

test('deriveEvidenceRelPath keeps the current evidence naming contract', async () => {
  const module = await loadModule('runtime/core-helpers/src/core/plan-execution.ts');

  assert.equal(typeof module.deriveEvidenceRelPath, 'function');
  assert.equal(
    module.deriveEvidenceRelPath('docs/superpowers/plans/2026-03-17-example-execution-plan.md', 2),
    'docs/superpowers/execution-evidence/2026-03-17-example-execution-plan-r2-evidence.md',
  );
});

test('deriveTasksIndependentFromPlan distinguishes independent, coupled, and unknown task scopes', async () => {
  const module = await loadModule('runtime/core-helpers/src/core/plan-execution.ts');

  assert.equal(typeof module.deriveTasksIndependentFromPlan, 'function');

  const independentPlan = [
    '# Example Plan',
    '',
    '## Task 1: Parser',
    '',
    '**Files:**',
    '- Modify: `src/parser.ts:10-20`',
    '- Test: `node --test tests/parser.test.mjs`',
    '',
    '- [ ] **Step 1: Update parser**',
    '',
    '## Task 2: Formatter',
    '',
    '**Files:**',
    '- Modify: `src/formatter.ts:5-18`',
    '- Test: `node --test tests/formatter.test.mjs`',
    '',
    '- [ ] **Step 1: Update formatter**',
    '',
  ].join('\n');
  assert.equal(module.deriveTasksIndependentFromPlan(independentPlan), 'yes');

  const coupledPlan = independentPlan.replace('src/formatter.ts:5-18', 'src/parser.ts:22-40');
  assert.equal(module.deriveTasksIndependentFromPlan(coupledPlan), 'no');

  const unknownPlan = [
    '# Example Plan',
    '',
    '## Task 1: Parser',
    '',
    '- [ ] **Step 1: Update parser**',
    '',
    '## Task 2: Formatter',
    '',
    '- [ ] **Step 1: Update formatter**',
    '',
  ].join('\n');
  assert.equal(module.deriveTasksIndependentFromPlan(unknownPlan), 'unknown');
});
