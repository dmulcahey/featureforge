function Write-SuperpowersRuntimeFailure {
  param(
    [Parameter(Mandatory = $true)][string]$FailureClass,
    [Parameter(Mandatory = $true)][string]$Message
  )

  $payload = @{ failure_class = $FailureClass; message = $Message } | ConvertTo-Json -Compress
  [Console]::Error.WriteLine($payload)
}

function Get-SuperpowersInstallRoot {
  return (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}

function Get-SuperpowersNodePath {
  $nodePath = $env:SUPERPOWERS_NODE_BIN
  if (-not $nodePath) {
    $node = Get-Command node -ErrorAction SilentlyContinue
    if (-not $node) {
      throw 'RuntimeDependencyMissing'
    }
    $nodePath = $node.Source
  }

  if (-not (Test-Path $nodePath)) {
    throw 'RuntimeDependencyMissing'
  }

  $version = & $nodePath --version 2>$null
  if (-not $version) {
    throw 'RuntimeDependencyVersionUnsupported'
  }

  if ($version -notmatch '^v?(?<major>\d+)') {
    throw 'RuntimeDependencyVersionUnsupported'
  }

  if ([int]$Matches.major -lt 20) {
    throw 'RuntimeDependencyVersionUnsupported'
  }

  return $nodePath
}

function Invoke-SuperpowersRuntime {
  param(
    [Parameter(Mandatory = $true)][string]$EntryRelative,
    [Parameter()][string[]]$Arguments = @()
  )

  $script:SuperpowersRuntimeExitCode = 0
  $installRoot = Get-SuperpowersInstallRoot

  try {
    $nodePath = Get-SuperpowersNodePath
  } catch {
    switch ($_.Exception.Message) {
      'RuntimeDependencyMissing' {
        Write-SuperpowersRuntimeFailure 'RuntimeDependencyMissing' 'Node 20 LTS or newer is required.'
        $script:SuperpowersRuntimeExitCode = 1
        return
      }
      default {
        Write-SuperpowersRuntimeFailure 'RuntimeDependencyVersionUnsupported' "Couldn't determine the installed Node version."
        $script:SuperpowersRuntimeExitCode = 1
        return
      }
    }
  }

  $entryPath = Join-Path $installRoot $EntryRelative
  if (-not (Test-Path $entryPath)) {
    Write-SuperpowersRuntimeFailure 'RuntimeArtifactMissing' "Missing runtime bundle: $EntryRelative"
    $script:SuperpowersRuntimeExitCode = 1
    return
  }

  & $nodePath --check $entryPath *> $null
  if ($LASTEXITCODE -ne 0) {
    Write-SuperpowersRuntimeFailure 'RuntimeArtifactInvalid' "Invalid runtime bundle: $EntryRelative"
    $script:SuperpowersRuntimeExitCode = 1
    return
  }

  & $nodePath $entryPath @Arguments
  $script:SuperpowersRuntimeExitCode = $LASTEXITCODE
}
