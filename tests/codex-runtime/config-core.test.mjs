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

test('getConfigValue keeps last matching key wins and trims surrounding whitespace', async () => {
  const module = await loadModule('runtime/core-helpers/src/core/config.ts');

  assert.equal(typeof module.getConfigValue, 'function');
  assert.equal(
    module.getConfigValue('update_check: false\nupdate_check:   true   \n', 'update_check'),
    'true',
  );
});

test('setConfigValue replaces matching lines, preserves unrelated lines, and appends missing keys', async () => {
  const module = await loadModule('runtime/core-helpers/src/core/config.ts');

  assert.equal(typeof module.setConfigValue, 'function');

  const replaced = module.setConfigValue(
    'update_check: false\ncomment_like: keep\nupdate_check: true\n',
    'update_check',
    'false',
  );
  assert.equal(
    replaced,
    'update_check: false\ncomment_like: keep\nupdate_check: false\n',
  );

  const appended = module.setConfigValue('superpowers_contributor: true\n', 'update_check', 'true');
  assert.equal(
    appended,
    'superpowers_contributor: true\nupdate_check: true\n',
  );
});
