. (Join-Path $PSScriptRoot 'superpowers-runtime-common.ps1')
Invoke-SuperpowersRuntime -EntryRelative 'runtime/core-helpers/dist/superpowers-plan-execution.cjs' -Arguments $args
$exitCode = [int]$script:SuperpowersRuntimeExitCode
try {
  $host.SetShouldExit($exitCode)
  return
}
catch {
  exit $exitCode
}
