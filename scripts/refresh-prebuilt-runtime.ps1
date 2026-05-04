Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
$TargetKey = if ($env:FEATUREFORGE_PREBUILT_TARGET) { $env:FEATUREFORGE_PREBUILT_TARGET } else { "windows-x64" }
switch ($TargetKey) {
  "darwin-arm64" {
    $DefaultRustTarget = "aarch64-apple-darwin"
    $BinaryName = "featureforge"
  }
  "windows-x64" {
    $DefaultRustTarget = "x86_64-pc-windows-msvc"
    $BinaryName = "featureforge.exe"
  }
  default {
    throw "unsupported FEATUREFORGE_PREBUILT_TARGET: $TargetKey"
  }
}
$RustTarget = if ($env:FEATUREFORGE_PREBUILT_RUST_TARGET) { $env:FEATUREFORGE_PREBUILT_RUST_TARGET } else { $DefaultRustTarget }
$Version = (Get-Content (Join-Path $RepoRoot "VERSION") -Raw).Trim()
$OutputDir = Join-Path $RepoRoot "bin/prebuilt/$TargetKey"
$OutputPath = Join-Path $OutputDir $BinaryName
$ChecksumPath = "$OutputPath.sha256"
$BuildPath = Join-Path $RepoRoot "target/$RustTarget/release/$BinaryName"

Push-Location $RepoRoot
try {
  cargo build --release --target $RustTarget --bin featureforge | Out-Host
} finally {
  Pop-Location
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
Copy-Item -Force $BuildPath $OutputPath
if ($TargetKey -eq "darwin-arm64") {
  Copy-Item -Force $OutputPath (Join-Path $RepoRoot "bin/featureforge")
}

$Checksum = (Get-FileHash -Algorithm SHA256 $OutputPath).Hash.ToLowerInvariant()
Set-Content -NoNewline:$false -Path $ChecksumPath -Value "$Checksum  $BinaryName"

node (Join-Path $ScriptDir "prebuilt-runtime-provenance.mjs") update `
  --target $TargetKey `
  --binary-path "bin/prebuilt/$TargetKey/$BinaryName" `
  --checksum-path "bin/prebuilt/$TargetKey/$BinaryName.sha256" `
  --version $Version `
  --repo-root $RepoRoot | Out-Host
if ($LASTEXITCODE -ne 0) {
  throw "prebuilt runtime manifest provenance update failed"
}

node (Join-Path $ScriptDir "prebuilt-runtime-provenance.mjs") verify `
  --target $TargetKey `
  --repo-root $RepoRoot | Out-Host
if ($LASTEXITCODE -ne 0) {
  throw "prebuilt runtime validation failed"
}

Write-Host "Refreshed checked-in runtime for $TargetKey at $OutputPath"
