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
    throw "RuntimeDependencyVersionUnsupported:Couldn't determine the installed Node version."
  }

  if ($version -notmatch '^v?(?<major>\d+)') {
    throw "RuntimeDependencyVersionUnsupported:Couldn't determine the installed Node version."
  }

  if ([int]$Matches.major -lt 20) {
    throw "RuntimeDependencyVersionUnsupported:Node 20 LTS or newer is required. Found $version."
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
  $restoreNativeExitPreference = $false
  $nativeExitPreference = $null
  $nativeExitVariable = Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue

  try {
    $nodePath = Get-SuperpowersNodePath
  } catch {
    $errorMessage = $_.Exception.Message
    switch ($errorMessage) {
      'RuntimeDependencyMissing' {
        Write-SuperpowersRuntimeFailure 'RuntimeDependencyMissing' 'Node 20 LTS or newer is required.'
        $script:SuperpowersRuntimeExitCode = 1
        return
      }
      default {
        if ($errorMessage -like 'RuntimeDependencyVersionUnsupported:*') {
          $message = $errorMessage.Substring('RuntimeDependencyVersionUnsupported:'.Length)
          Write-SuperpowersRuntimeFailure 'RuntimeDependencyVersionUnsupported' $message
        } else {
          Write-SuperpowersRuntimeFailure 'RuntimeDependencyVersionUnsupported' "Couldn't determine the installed Node version."
        }
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

  if ($nativeExitVariable) {
    $nativeExitPreference = $nativeExitVariable.Value
    $PSNativeCommandUseErrorActionPreference = $false
    $restoreNativeExitPreference = $true
  }

  try {
    & $nodePath --check $entryPath *> $null
    if ($LASTEXITCODE -ne 0) {
      Write-SuperpowersRuntimeFailure 'RuntimeArtifactInvalid' "Invalid runtime bundle: $EntryRelative"
      $script:SuperpowersRuntimeExitCode = 1
      return
    }

    & $nodePath $entryPath @Arguments
    $script:SuperpowersRuntimeExitCode = $LASTEXITCODE
  }
  finally {
    if ($restoreNativeExitPreference) {
      $PSNativeCommandUseErrorActionPreference = $nativeExitPreference
    }
  }
}
