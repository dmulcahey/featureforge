$RuntimeRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

$Architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
$TargetKey = $null
$BinaryName = $null

if ($IsMacOS -and $Architecture -eq [System.Runtime.InteropServices.Architecture]::Arm64) {
  $TargetKey = 'darwin-arm64'
  $BinaryName = 'superpowers'
}
elseif ($IsWindows -and $Architecture -eq [System.Runtime.InteropServices.Architecture]::X64) {
  $TargetKey = 'windows-x64'
  $BinaryName = 'superpowers.exe'
}

if ($TargetKey -and $BinaryName) {
  $Candidate = Join-Path $RuntimeRoot "bin\prebuilt\$TargetKey\$BinaryName"
  if (Test-Path $Candidate -PathType Leaf) {
    & $Candidate @args
    exit $LASTEXITCODE
  }
}

Write-Error 'Checked-in Superpowers runtime binary not found for this host. Expected bin\prebuilt\darwin-arm64\superpowers or bin\prebuilt\windows-x64\superpowers.exe.'
exit 127
