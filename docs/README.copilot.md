# Superpowers for GitHub Copilot Local Installs

Guide for using Superpowers with GitHub Copilot local installs via native skill and custom-agent discovery backed by the shared Superpowers runtime checkout.

## Quick Install

Tell GitHub Copilot:

```
Fetch and follow instructions from https://raw.githubusercontent.com/dmulcahey/superpowers/refs/heads/main/.copilot/INSTALL.md
```

## Manual Installation

### Prerequisites

- GitHub Copilot CLI or another local GitHub Copilot install that supports local skills and custom agents
- Git
- Node 20 LTS or newer

### Steps

1. Install or update the shared runtime checkout:
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

2. Create the skills symlink:
   ```bash
   mkdir -p ~/.copilot/skills
   ln -s ~/.superpowers/install/skills ~/.copilot/skills/superpowers
   ```

3. Install the code-reviewer custom agent from the canonical agents directory:
   ```bash
   mkdir -p ~/.copilot/agents
   ln -s ~/.superpowers/install/agents/code-reviewer.md ~/.copilot/agents/code-reviewer.agent.md
   ```

4. Restart GitHub Copilot so it discovers the new skills and agent.

### Windows

Use a junction for skills and copy the agent file:

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

## Migrating Existing Installs

If you already have `~/.codex/superpowers` or `~/.copilot/superpowers`, migrate them into the shared checkout with the staged install helper:

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

The staged helper installs or updates the shared checkout, repairs already-present compatibility links or copied Windows agent files, and prints any remaining first-time setup steps. After migrating, continue with steps 2 and 3 to create or refresh `~/.copilot/skills/superpowers` and `~/.copilot/agents/code-reviewer.agent.md`, then restart GitHub Copilot.

## How It Works

GitHub Copilot local installs discover skills from `~/.copilot/skills/` and custom agents from `~/.copilot/agents/`. Superpowers keeps `skills/` and `agents/` canonical in the repo and installs them into those discovery locations.

```
~/.copilot/skills/superpowers/ → ~/.superpowers/install/skills/
Unix-like: ~/.copilot/agents/code-reviewer.agent.md → ~/.superpowers/install/agents/code-reviewer.md
Windows: copy ~/.superpowers/install/agents/code-reviewer.md to ~/.copilot/agents/code-reviewer.agent.md
```

On Unix-like installs, the Copilot agent is symlinked to the shared checkout.

On Windows, the Copilot agent is copied from the shared checkout and must be refreshed after updates.

## Usage

Skills are discovered automatically when:
- you mention a skill by name
- the task matches a skill's description
- `using-superpowers` directs the agent to use one

The `code-reviewer` agent is available through Copilot's local custom-agent support after installation.

## Default Workflow

Superpowers' default planning pipeline is:

`brainstorming -> plan-ceo-review -> writing-plans -> plan-eng-review -> implementation`

Accelerated review is an opt-in branch inside `plan-ceo-review` and `plan-eng-review`, not a separate workflow stage.

Only the user can initiate accelerated review, and section approval plus final approval remain human-owned even when the review uses reviewer subagents and persisted section packets.

During implementation, either `subagent-driven-development` or `executing-plans` starts from an engineering-approved current plan, runs a workspace-readiness preflight, and then drives task execution. Workspace preparation is the user's responsibility; invoke `using-git-worktrees` manually when you want isolated workspace management. The completion flow runs `requesting-code-review`, may offer `qa-only` before landing, and may offer `document-release` before final cleanup or PR handoff.

## Runtime Helpers

Runtime helper state lives in `~/.superpowers/`. Generated skill preambles use this directory for session markers, contributor logs, update-check cache files, and project-scoped artifacts under `~/.superpowers/projects/`.

Superpowers ships a supported public workflow inspection surface:
- `bin/superpowers-workflow` (Bash)
- `bin/superpowers-workflow.ps1` (PowerShell wrapper)

Use `status`, `next`, `artifacts`, `explain`, or `help` when you want to inspect workflow state directly from the terminal. These commands stay read-only: they do not create, repair, or rewrite branch-scoped manifests, and `next` stops at the execution handoff boundary instead of calling `superpowers-plan-execution recommend`.

Superpowers also ships workflow-status runtime helpers:
- `bin/superpowers-workflow-status` (Bash)
- `bin/superpowers-workflow-status.ps1` (PowerShell wrapper)

Generated workflow skills call `$_SUPERPOWERS_ROOT/bin/superpowers-workflow-status status --refresh` first to resolve the conservative next stage, including before spec/plan docs exist. This helper is an internal runtime surface, not a supported public workflow CLI. Default `status` output is JSON for machine consumers; `status --summary` is a human-oriented one-line view. `reason` is the canonical diagnostic field, and any `note` field is only a compatibility alias. It keeps branch-scoped manifests at `~/.superpowers/projects/<repo-slug>/<user>-<safe-branch>-workflow-state.json`; that local manifest is rebuildable, and repo docs remain authoritative for approval state.

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

## Personal Skills and Agents

Create your own skills in `~/.copilot/skills/` and your own agents in `~/.copilot/agents/`.

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

## Troubleshooting

### Skills not showing up

1. Verify the symlink: `ls -la ~/.copilot/skills/superpowers`
2. Check skills exist: `ls ~/.superpowers/install/skills`
3. Restart GitHub Copilot

**Windows (PowerShell):**
1. Verify the junction: `Get-Item "$env:USERPROFILE\.copilot\skills\superpowers"`
2. Check skills exist: `Get-ChildItem "$env:USERPROFILE\.superpowers\install\skills"`
3. Restart GitHub Copilot

### Agent not showing up

1. Verify the agent file: `ls -la ~/.copilot/agents/code-reviewer.agent.md`
2. Check the source exists: `ls ~/.superpowers/install/agents/code-reviewer.md`
3. Restart GitHub Copilot

**Windows (PowerShell):**
1. Verify the copied agent file: `Get-Item "$env:USERPROFILE\.copilot\agents\code-reviewer.agent.md"`
2. Check the source exists: `Get-Item "$env:USERPROFILE\.superpowers\install\agents\code-reviewer.md"`
3. If you updated Superpowers, rerun the Windows install step that copies `code-reviewer.md` into Copilot's agent directory
4. Restart GitHub Copilot

## Getting Help

- Report issues: https://github.com/dmulcahey/superpowers/issues
- Main documentation: https://github.com/dmulcahey/superpowers
