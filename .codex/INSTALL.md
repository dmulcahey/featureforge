# Installing Superpowers for Codex

Enable superpowers skills in Codex via native skill discovery. Codex and GitHub Copilot can share a single Superpowers checkout at `~/.superpowers/install`.

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
   mkdir -p ~/.agents/skills
   ln -s ~/.superpowers/install/skills ~/.agents/skills/superpowers
   ```

3. **Install the `code-reviewer` custom agent:**
   ```bash
   mkdir -p ~/.codex/agents
   ln -s ~/.superpowers/install/.codex/agents/code-reviewer.toml ~/.codex/agents/code-reviewer.toml
   ```

4. **Restart Codex** (quit and relaunch the CLI) to discover the skills and agent.

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

   New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.agents\skills"
   cmd /c mklink /J "$env:USERPROFILE\.agents\skills\superpowers" "$env:USERPROFILE\.superpowers\install\skills"

   New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.codex\agents"
   Copy-Item "$env:USERPROFILE\.superpowers\install\.codex\agents\code-reviewer.toml" "$env:USERPROFILE\.codex\agents\code-reviewer.toml" -Force
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

The staged helper installs or updates the shared checkout, repairs already-present compatibility links or copied Windows agent files, and prints any remaining first-time setup steps. After migrating, continue with steps 2 and 3 to create or refresh `~/.agents/skills/superpowers` and `~/.codex/agents/code-reviewer.toml`, then restart Codex.

## Migrating from old bootstrap

If you installed superpowers before native skill discovery, you need to:

1. **Create the skills symlink** (step 2 above) — this is the current discovery mechanism.

2. **Remove the old bootstrap block** from `~/.codex/AGENTS.md` — any block referencing `superpowers-codex bootstrap` is no longer needed.

3. **Restart Codex.**

## Verify

```bash
ls -la ~/.agents/skills/superpowers
ls -la ~/.superpowers/install/skills
ls -la ~/.codex/agents/code-reviewer.toml
ls -la ~/.superpowers/install/.codex/agents/code-reviewer.toml
```

**Windows (PowerShell):**
```powershell
Get-Item "$env:USERPROFILE\.agents\skills\superpowers"
Get-ChildItem "$env:USERPROFILE\.superpowers\install\skills"
Get-Item "$env:USERPROFILE\.codex\agents\code-reviewer.toml"
Get-Item "$env:USERPROFILE\.superpowers\install\.codex\agents\code-reviewer.toml"
```

You should see a symlink (or junction on Windows) for the skills plus the installed `code-reviewer` agent.

## Codex Subagents

Current Codex releases enable subagent workflows by default. Superpowers skills such as `dispatching-parallel-agents` and `subagent-driven-development` do not require the old multi-agent feature flag.

Codex ships built-in `default`, `worker`, and `explorer` agents:

- Use `worker` for implementation and fix tasks.
- Use `explorer` for read-heavy investigation and review tasks.
- Use `default` when the task needs broader judgment instead of a narrow execution or exploration role.

The `code-reviewer` custom agent is installed alongside the skills.

If you want custom project-scoped agents, add TOML files under `.codex/agents/`. Personal custom agents live under `~/.codex/agents/`. Each file must define `name`, `description`, and `developer_instructions`. Fields like `model`, `model_reasoning_effort`, `sandbox_mode`, `mcp_servers`, and `skills.config` inherit from the parent session when omitted.

Use `[agents]` in your Codex config to tune global limits such as `max_threads`, `max_depth`, and `job_max_runtime_seconds`. Most installs can leave `[agents]` unset; Codex defaults to six open agent threads and a max depth of one direct child layer.

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

Skills update instantly through the symlink.

The staged helper refreshes already-present compatibility links and already-present copied Windows agent files. If it prints next steps, create any missing first-time discovery links or copied agent files after the update.

Generated skill preambles run `~/.superpowers/install/bin/superpowers-update-check` automatically when that install root is active, so new sessions can surface `UPGRADE_AVAILABLE` or `JUST_UPGRADED` without extra setup.

## Uninstalling

```bash
rm ~/.agents/skills/superpowers
rm ~/.codex/agents/code-reviewer.toml
```

**Windows (PowerShell):**
```powershell
Remove-Item "$env:USERPROFILE\.agents\skills\superpowers"
Remove-Item "$env:USERPROFILE\.codex\agents\code-reviewer.toml"
```

Optionally delete the shared clone if no other platform uses it: `rm -rf ~/.superpowers/install` (Windows: `Remove-Item -Recurse -Force "$env:USERPROFILE\.superpowers\install"`).
