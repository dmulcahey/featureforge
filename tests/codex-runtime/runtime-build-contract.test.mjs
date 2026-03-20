import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

test('runtime workspace files exist', () => {
  for (const path of [
    'runtime/core-helpers/package.json',
    'runtime/core-helpers/package-lock.json',
    'runtime/core-helpers/tsconfig.json',
    'runtime/core-helpers/scripts/build-runtime.mjs',
  ]) {
    assert.equal(fs.existsSync(path), true, `${path} should exist`);
  }
});
