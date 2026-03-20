# Installing Superpowers for GitHub Copilot Local Installs

Enable Superpowers skills and agents in GitHub Copilot local installs by linking Copilot's discovery paths to the shared Superpowers checkout at `~/.superpowers/install`.

## Prerequisites

- Git
- Node 20 LTS or newer

## Fresh Install

1. **Install or update the shared runtime checkout:**
   ```bash
   if [[ -x ~/.superpowers/install/bin/superpowers-install-runtime ]]; then
     ~/.superpowers/install/bin/superpowers-install-runtime
   else
     tmpdir=$(mktemp -d)
     git clone --depth 1 https://github.com/dmulcahey/superpowers.git "$tmpdir/superpowers"
     "$tmpdir/superpowers/bin/superpowers-install-runtime"
     install_status=$?
     rm -rf "$tmpdir"
     if [[ $install_status -ne 0 ]]; then exit $install_status; fi
   fi
   ```

2. **Create the skills symlink:**
   ```bash
   mkdir -p ~/.copilot/skills
   ln -s ~/.superpowers/install/skills ~/.copilot/skills/superpowers
   ```

3. **Install the code-reviewer custom agent from the canonical agents directory:**
   ```bash
   mkdir -p ~/.copilot/agents
   ln -s ~/.superpowers/install/agents/code-reviewer.md ~/.copilot/agents/code-reviewer.agent.md
   ```

4. **Restart GitHub Copilot CLI** so it discovers the newly installed skills and agent.

## Windows

Use a junction for the skills directory and copy the agent file into Copilot's agent directory:

```powershell
if (Test-Path "$env:USERPROFILE\.superpowers\install\bin\superpowers-install-runtime.ps1") {
  & "$env:USERPROFILE\.superpowers\install\bin\superpowers-install-runtime.ps1"
} else {
  $tmpRoot = Join-Path $env:TEMP "superpowers-install"
  $tmpDir = Join-Path $tmpRoot ([guid]::NewGuid().ToString())
  git clone --depth 1 https://github.com/dmulcahey/superpowers.git (Join-Path $tmpDir "superpowers")
  & (Join-Path $tmpDir "superpowers\bin\superpowers-install-runtime.ps1")
  Remove-Item -Recurse -Force $tmpDir
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.copilot\skills"
cmd /c mklink /J "$env:USERPROFILE\.copilot\skills\superpowers" "$env:USERPROFILE\.superpowers\install\skills"

New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.copilot\agents"
Copy-Item "$env:USERPROFILE\.superpowers\install\agents\code-reviewer.md" "$env:USERPROFILE\.copilot\agents\code-reviewer.agent.md" -Force
```

## Migrate Existing Install

If you already have `~/.codex/superpowers` or `~/.copilot/superpowers`, use the staged install helper instead of keeping separate clones:

```bash
if [[ -x ~/.superpowers/install/bin/superpowers-install-runtime ]]; then
  ~/.superpowers/install/bin/superpowers-install-runtime
else
  tmpdir=$(mktemp -d)
  git clone --depth 1 https://github.com/dmulcahey/superpowers.git "$tmpdir/superpowers"
  "$tmpdir/superpowers/bin/superpowers-install-runtime"
  install_status=$?
  rm -rf "$tmpdir"
  if [[ $install_status -ne 0 ]]; then exit $install_status; fi
fi
```

`bin/superpowers-migrate-install` remains available as a compatibility shim, but the supported path is `bin/superpowers-install-runtime`.

**Windows (PowerShell):**
```powershell
if (Test-Path "$env:USERPROFILE\.superpowers\install\bin\superpowers-install-runtime.ps1") {
  & "$env:USERPROFILE\.superpowers\install\bin\superpowers-install-runtime.ps1"
} else {
  $tmpRoot = Join-Path $env:TEMP "superpowers-install"
  $tmpDir = Join-Path $tmpRoot ([guid]::NewGuid().ToString())
  git clone --depth 1 https://github.com/dmulcahey/superpowers.git (Join-Path $tmpDir "superpowers")
  & (Join-Path $tmpDir "superpowers\bin\superpowers-install-runtime.ps1")
  Remove-Item -Recurse -Force $tmpDir
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

The staged helper installs or updates the shared checkout, repairs already-present compatibility links or copied Windows agent files, and prints any remaining first-time setup steps. After migrating, continue with steps 2 and 3 to create or refresh `~/.copilot/skills/superpowers` and `~/.copilot/agents/code-reviewer.agent.md`, then restart GitHub Copilot CLI.

## Verify

```bash
ls -la ~/.copilot/skills/superpowers
ls -la ~/.copilot/agents/code-reviewer.agent.md
ls -la ~/.superpowers/install/skills
ls -la ~/.superpowers/install/agents/code-reviewer.md
```

**Windows (PowerShell):**
```powershell
Get-Item "$env:USERPROFILE\.copilot\skills\superpowers"
Get-Item "$env:USERPROFILE\.copilot\agents\code-reviewer.agent.md"
Get-ChildItem "$env:USERPROFILE\.superpowers\install\skills"
Get-Item "$env:USERPROFILE\.superpowers\install\agents\code-reviewer.md"
```

You should see the installed skills location and the code-reviewer agent file.

## Runtime Helpers

Runtime helper state lives in `~/.superpowers/`. Generated skill preambles use this directory for session markers, contributor logs, update-check cache files, and project-scoped artifacts under `~/.superpowers/projects/`.

Optional: enable contributor mode for future sessions with:

```bash
~/.superpowers/install/bin/superpowers-config set superpowers_contributor true
```

**Windows (PowerShell):**
```powershell
& "$env:USERPROFILE\.superpowers\install\bin\superpowers-config.ps1" set superpowers_contributor true
```

If you disable update notices, re-enable them with:

```bash
~/.superpowers/install/bin/superpowers-config set update_check true
```

**Windows (PowerShell):**
```powershell
& "$env:USERPROFILE\.superpowers\install\bin\superpowers-config.ps1" set update_check true
```

## Updating

```bash
~/.superpowers/install/bin/superpowers-install-runtime
```

**Windows (PowerShell):**
```powershell
& "$env:USERPROFILE\.superpowers\install\bin\superpowers-install-runtime.ps1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

The staged helper refreshes already-present compatibility links and already-present copied Windows agent files. If it prints next steps, create any missing first-time discovery links or copied agent files after the update.

Generated skill preambles run `~/.superpowers/install/bin/superpowers-update-check` automatically when that install root is active, so new sessions can surface `UPGRADE_AVAILABLE` or `JUST_UPGRADED` without extra setup.

## Uninstalling

```bash
rm ~/.copilot/skills/superpowers
rm ~/.copilot/agents/code-reviewer.agent.md
```

**Windows (PowerShell):**
```powershell
Remove-Item "$env:USERPROFILE\.copilot\skills\superpowers"
Remove-Item "$env:USERPROFILE\.copilot\agents\code-reviewer.agent.md"
```

Optionally delete the shared clone if no other platform uses it: `rm -rf ~/.superpowers/install` (Windows: `Remove-Item -Recurse -Force "$env:USERPROFILE\.superpowers\install"`).
