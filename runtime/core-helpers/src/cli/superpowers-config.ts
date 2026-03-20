import path from 'node:path';
import { getConfigValue, setConfigValue } from '../core/config';
import { readTextFileIfExists, writeTextFileAtomic } from '../platform/filesystem';
import { resolveStateDir } from '../platform/paths';
import { runCli } from '../platform/process';

declare const require: undefined | { main: unknown };
declare const module: unknown;

const USAGE = 'Usage: superpowers-config {get|set|list} [key] [value]';

function resolveConfigFile(): string {
  const stateDir = resolveStateDir(process.env);
  return path.join(stateDir, 'config.yaml');
}

function writeUsage(): number {
  console.error(USAGE);
  return 1;
}

export function main(argv: string[] = process.argv): number {
  const [, , command, key, value] = argv;
  const configFile = resolveConfigFile();

  switch (command) {
    case 'get': {
      if (!key) {
        return writeUsage();
      }

      const resolvedValue = getConfigValue(readTextFileIfExists(configFile), key);
      if (resolvedValue) {
        process.stdout.write(`${resolvedValue}\n`);
      }
      return 0;
    }
    case 'set': {
      if (!key || value === undefined) {
        return writeUsage();
      }

      const updatedConfig = setConfigValue(readTextFileIfExists(configFile), key, value);
      writeTextFileAtomic(configFile, updatedConfig);
      return 0;
    }
    case 'list': {
      const configText = readTextFileIfExists(configFile);
      if (configText) {
        process.stdout.write(configText);
      }
      return 0;
    }
    default:
      return writeUsage();
  }
}

if (typeof require !== 'undefined' && require.main === module) {
  runCli((argv) => main(argv));
}
