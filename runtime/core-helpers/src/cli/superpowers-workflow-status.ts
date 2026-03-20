import { runCli } from '../platform/process';

declare const require: undefined | { main: unknown };
declare const module: unknown;

export function main(): number {
  console.error('Not implemented: superpowers-workflow-status');
  return 1;
}

if (typeof require !== 'undefined' && require.main === module) {
  runCli(() => main());
}
