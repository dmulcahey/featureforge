export type CliMain = (argv: string[]) => number;

export function runCli(main: CliMain, argv: string[] = process.argv): void {
  process.exitCode = main(argv);
}
