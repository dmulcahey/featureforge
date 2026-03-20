import { runPlanExecutionCommand } from '../core/plan-execution';
import { runCli } from '../platform/process';

declare const require: undefined | { main: unknown };
declare const module: unknown;

export function main(argv: string[] = process.argv): number {
  const result = runPlanExecutionCommand(argv.slice(2), {
    cwd: process.cwd(),
    env: process.env,
  });

  if (result.stdout.length > 0) {
    process.stdout.write(result.stdout);
  }
  if (result.stderr.length > 0) {
    process.stderr.write(result.stderr);
  }

  return result.exitCode;
}

if (typeof require !== 'undefined' && require.main === module) {
  runCli(main);
}
