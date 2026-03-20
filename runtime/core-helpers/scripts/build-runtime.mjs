import { mkdir, readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const runtimeRoot = resolve(scriptDir, '..');
const distDir = resolve(runtimeRoot, 'dist');

const entries = [
  {
    input: resolve(runtimeRoot, 'src/cli/superpowers-config.ts'),
    output: resolve(distDir, 'superpowers-config.cjs'),
  },
  {
    input: resolve(runtimeRoot, 'src/cli/superpowers-workflow-status.ts'),
    output: resolve(distDir, 'superpowers-workflow-status.cjs'),
  },
  {
    input: resolve(runtimeRoot, 'src/cli/superpowers-plan-execution.ts'),
    output: resolve(distDir, 'superpowers-plan-execution.cjs'),
  },
];

const args = new Set(process.argv.slice(2));
const checkMode = args.has('--check');

if (args.size > (checkMode ? 1 : 0)) {
  console.error(`Unknown arguments: ${process.argv.slice(2).join(' ')}`);
  process.exitCode = 1;
} else {
  await mkdir(distDir, { recursive: true });

  let hasDifferences = false;

  for (const entry of entries) {
    const result = await build({
      entryPoints: [entry.input],
      bundle: true,
      format: 'cjs',
      legalComments: 'none',
      logLevel: 'silent',
      outfile: entry.output,
      platform: 'node',
      sourcemap: false,
      target: 'node20',
      write: !checkMode,
    });

    if (!checkMode) {
      continue;
    }

    const generated = result.outputFiles?.[0]?.text ?? '';
    let existing = '';

    try {
      existing = await readFile(entry.output, 'utf8');
    } catch {
      console.error(`Missing generated runtime bundle: ${entry.output}`);
      hasDifferences = true;
      continue;
    }

    if (existing !== generated) {
      console.error(`Generated runtime bundle is stale: ${entry.output}`);
      hasDifferences = true;
    }
  }

  if (hasDifferences) {
    process.exitCode = 1;
  }
}
