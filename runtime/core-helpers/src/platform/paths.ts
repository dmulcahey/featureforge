import path from 'node:path';

export function resolveFromRuntimeRoot(runtimeRoot: string, relativePath: string): string {
  return path.resolve(runtimeRoot, relativePath);
}
