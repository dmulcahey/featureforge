. (Join-Path $PSScriptRoot 'superpowers-runtime-common.ps1')
Invoke-SuperpowersRuntime -EntryRelative 'runtime/core-helpers/dist/superpowers-config.cjs' -Arguments $args
exit $script:SuperpowersRuntimeExitCode
