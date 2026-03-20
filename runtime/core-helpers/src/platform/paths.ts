import os from 'node:os';
import path from 'node:path';

export function resolveFromRuntimeRoot(runtimeRoot: string, relativePath: string): string {
  return path.resolve(runtimeRoot, relativePath);
}

export function resolveRuntimeRoot(entryPath: string, runtimeRootOverride?: string): string {
  if (runtimeRootOverride && runtimeRootOverride.length > 0) {
    return path.resolve(runtimeRootOverride);
  }

  return path.resolve(path.dirname(entryPath), '../../..');
}

export function resolveStateDir(env: NodeJS.ProcessEnv): string {
  if (env.SUPERPOWERS_STATE_DIR && env.SUPERPOWERS_STATE_DIR.length > 0) {
    return env.SUPERPOWERS_STATE_DIR;
  }

  if (env.HOME && env.HOME.length > 0) {
    return path.join(env.HOME, '.superpowers');
  }

  if (env.USERPROFILE && env.USERPROFILE.length > 0) {
    return path.join(env.USERPROFILE, '.superpowers');
  }

  if (env.HOMEDRIVE && env.HOMEPATH && env.HOMEDRIVE.length > 0 && env.HOMEPATH.length > 0) {
    return path.join(`${env.HOMEDRIVE}${env.HOMEPATH}`, '.superpowers');
  }

  return path.join(os.homedir(), '.superpowers');
}

export function normalizeRelativePath(input: string): string | null {
  if (input.length === 0 || path.isAbsolute(input)) {
    return null;
  }

  const normalizedParts: string[] = [];
  for (const part of input.replace(/\\/g, '/').split('/')) {
    if (part === '' || part === '.') {
      continue;
    }
    if (part === '..') {
      return null;
    }

    normalizedParts.push(part);
  }

  if (normalizedParts.length === 0) {
    return null;
  }

  return normalizedParts.join('/');
}

export function isPathInsideRoot(rootPath: string, candidatePath: string): boolean {
  const relativePath = path.relative(rootPath, candidatePath);
  return relativePath === '' || (!relativePath.startsWith('..') && !path.isAbsolute(relativePath));
}
