use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use crate::execution::live_mutation_guard::deny_workspace_runtime_live_mutation;
use crate::execution::runtime_provenance::{
    ControlPlaneSource, RuntimeProvenance, SelfHostingContext, SkillDiscoveryProvenance,
    SkillSource, StateDirKind, featureforge_runtime_binary_name,
    installed_runtime_binary_display_path, runtime_provenance_for_paths,
};
use crate::git::{discover_slug_identity, sha256_hex};
use crate::paths::featureforge_state_dir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SelfHostingDiagnostic {
    pub installed_runtime_path: String,
    pub installed_runtime_hash: Option<String>,
    pub invoked_runtime_path: String,
    pub invoked_runtime_hash: Option<String>,
    pub workspace_runtime_path: Option<String>,
    pub workspace_runtime_hash: Option<String>,
    pub runtime_source: ControlPlaneSource,
    pub active_skill_root: Option<String>,
    pub skill_source: SkillSource,
    pub state_dir: String,
    pub state_dir_kind: StateDirKind,
    pub repo_root: String,
    pub is_featureforge_repo: bool,
    pub live_mutation_allowed: bool,
    pub warnings: Vec<String>,
    pub recommended_remediation: String,
}

pub fn diagnose_self_hosting(current_dir: &Path) -> SelfHostingDiagnostic {
    let repo_root = discover_slug_identity(current_dir).repo_root;
    let state_dir = featureforge_state_dir();
    diagnose_self_hosting_for_paths(&repo_root, &state_dir)
}

pub fn diagnose_self_hosting_for_paths(
    repo_root: &Path,
    state_dir: &Path,
) -> SelfHostingDiagnostic {
    let provenance = runtime_provenance_for_paths(repo_root, state_dir);
    diagnostic_from_provenance(&provenance)
}

pub fn render_self_hosting_diagnostic(diagnostic: &SelfHostingDiagnostic) -> String {
    let workspace_runtime = diagnostic
        .workspace_runtime_path
        .as_deref()
        .unwrap_or("not found");
    let active_skill_root = diagnostic
        .active_skill_root
        .as_deref()
        .unwrap_or("not found");
    let mut output = format!(
        "Self-hosting diagnostic\n\
runtime_source: {runtime_source:?}\n\
invoked_runtime_path: {invoked_runtime_path}\n\
installed_runtime_path: {installed_runtime_path}\n\
workspace_runtime_path: {workspace_runtime}\n\
skill_source: {skill_source:?}\n\
active_skill_root: {active_skill_root}\n\
state_dir: {state_dir}\n\
state_dir_kind: {state_dir_kind:?}\n\
repo_root: {repo_root}\n\
is_featureforge_repo: {is_featureforge_repo}\n\
live_mutation_allowed: {live_mutation_allowed}\n",
        runtime_source = diagnostic.runtime_source,
        invoked_runtime_path = diagnostic.invoked_runtime_path,
        installed_runtime_path = diagnostic.installed_runtime_path,
        skill_source = diagnostic.skill_source,
        state_dir = diagnostic.state_dir,
        state_dir_kind = diagnostic.state_dir_kind,
        repo_root = diagnostic.repo_root,
        is_featureforge_repo = diagnostic.is_featureforge_repo,
        live_mutation_allowed = diagnostic.live_mutation_allowed,
    );
    if diagnostic.warnings.is_empty() {
        output.push_str("warnings: none\n");
    } else {
        output.push_str("warnings:\n");
        for warning in &diagnostic.warnings {
            output.push_str("- ");
            output.push_str(warning);
            output.push('\n');
        }
    }
    output.push_str("recommended_remediation: ");
    output.push_str(&diagnostic.recommended_remediation);
    output.push('\n');
    output
}

fn diagnostic_from_provenance(provenance: &RuntimeProvenance) -> SelfHostingDiagnostic {
    let installed_runtime_path = installed_runtime_binary_display_path();
    let invoked_runtime_path = preferred_invoked_runtime_path(provenance);
    let workspace_runtime_path = workspace_runtime_path(provenance);
    let skill_discovery = provenance.skill_discovery.as_ref();
    let active_skill_root = active_skill_root(skill_discovery);
    let skill_source = skill_discovery
        .map(|discovery| discovery.active_featureforge_skill_source)
        .unwrap_or(SkillSource::Unknown);
    let mut warnings = provenance
        .workspace_runtime_warning
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    if let Some(warning) = skill_discovery.and_then(|discovery| discovery.warning.clone()) {
        warnings.push(warning);
    }
    let (live_mutation_allowed, guard_warning) = live_mutation_status(provenance);
    if let Some(warning) = guard_warning {
        warnings.push(warning);
    }
    append_control_plane_warning(provenance, &mut warnings);
    let recommended_remediation =
        recommended_remediation(provenance, skill_discovery, live_mutation_allowed);

    SelfHostingDiagnostic {
        installed_runtime_hash: file_hash(Path::new(&installed_runtime_path)),
        invoked_runtime_hash: file_hash(Path::new(&invoked_runtime_path)),
        workspace_runtime_hash: workspace_runtime_path
            .as_deref()
            .and_then(|path| file_hash(Path::new(path))),
        installed_runtime_path,
        invoked_runtime_path,
        workspace_runtime_path,
        runtime_source: provenance.control_plane_source,
        active_skill_root,
        skill_source,
        state_dir: provenance.state_dir.clone(),
        state_dir_kind: provenance.state_dir_kind,
        repo_root: provenance.repo_root.clone(),
        is_featureforge_repo: provenance.self_hosting_context
            == SelfHostingContext::FeatureforgeRepo,
        live_mutation_allowed,
        warnings,
        recommended_remediation,
    }
}

fn live_mutation_status(provenance: &RuntimeProvenance) -> (bool, Option<String>) {
    match deny_workspace_runtime_live_mutation(provenance, "live workflow mutation") {
        Ok(outcome) => (true, outcome.override_warning),
        Err(failure) => (false, Some(failure.message)),
    }
}

fn append_control_plane_warning(provenance: &RuntimeProvenance, warnings: &mut Vec<String>) {
    if provenance.control_plane_source == ControlPlaneSource::Installed {
        return;
    }
    if provenance.state_dir_kind != StateDirKind::Live {
        return;
    }
    warnings.push(String::from(
        "installed runtime is the required live workflow control plane; do not rely on PATH or workspace runtime resolution for live workflow commands",
    ));
}

fn recommended_remediation(
    provenance: &RuntimeProvenance,
    skill_discovery: Option<&SkillDiscoveryProvenance>,
    live_mutation_allowed: bool,
) -> String {
    let installed_runtime = installed_runtime_binary_display_path();
    if !live_mutation_allowed {
        return format!(
            "rerun live workflow mutations through `{installed_runtime}`; use workspace runtime only with isolated temp state or an explicitly approved override"
        );
    }
    if skill_discovery.is_some_and(|discovery| {
        discovery.active_featureforge_skill_source == SkillSource::Workspace
    }) {
        return String::from(
            "relink active FeatureForge skill discovery roots to the installed skill root before live workflow work",
        );
    }
    if provenance.control_plane_source != ControlPlaneSource::Installed {
        return format!(
            "use `{installed_runtime}` for live workflow control-plane commands; keep workspace runtime restricted to temp-state tests"
        );
    }
    if skill_discovery.is_some_and(|discovery| {
        discovery.active_featureforge_skill_source != SkillSource::Installed
            && discovery.active_featureforge_skill_source != SkillSource::Unknown
    }) {
        return String::from(
            "point active FeatureForge skill discovery roots at the installed skill root",
        );
    }
    String::from("none")
}

fn active_skill_root(discovery: Option<&SkillDiscoveryProvenance>) -> Option<String> {
    let discovery = discovery?;
    discovery
        .active_roots
        .iter()
        .find(|root| root.skill_source == discovery.active_featureforge_skill_source)
        .or_else(|| discovery.active_roots.first())
        .map(|root| root.resolved_path.clone())
}

fn preferred_invoked_runtime_path(provenance: &RuntimeProvenance) -> String {
    if !provenance.binary_realpath.is_empty() {
        return provenance.binary_realpath.clone();
    }
    provenance.binary_path.clone()
}

fn workspace_runtime_path(provenance: &RuntimeProvenance) -> Option<String> {
    if provenance.control_plane_source == ControlPlaneSource::Workspace {
        let invoked = preferred_invoked_runtime_path(provenance);
        if !invoked.is_empty() {
            return Some(invoked);
        }
    }
    let repo_root = Path::new(&provenance.repo_root);
    if repo_root.as_os_str().is_empty() {
        return None;
    }
    workspace_runtime_candidates(repo_root)
        .into_iter()
        .find(|candidate| candidate.is_file())
        .map(|path| canonicalize_or_self(&path).to_string_lossy().into_owned())
}

fn workspace_runtime_candidates(repo_root: &Path) -> Vec<PathBuf> {
    vec![
        repo_root
            .join("bin")
            .join(featureforge_runtime_binary_name()),
        repo_root
            .join("target")
            .join("debug")
            .join(featureforge_runtime_binary_name()),
        repo_root
            .join("target")
            .join("release")
            .join(featureforge_runtime_binary_name()),
    ]
}

fn file_hash(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("sha256:{}", sha256_hex(&bytes)))
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_provenance() -> RuntimeProvenance {
        RuntimeProvenance {
            binary_path: String::from("/workspace/featureforge/target/debug/featureforge"),
            binary_realpath: String::from("/workspace/featureforge/target/debug/featureforge"),
            runtime_root: String::from("/workspace/featureforge/target/debug"),
            repo_root: String::from("/workspace/featureforge"),
            state_dir: String::from("/tmp/featureforge-state"),
            state_dir_kind: StateDirKind::Temp,
            control_plane_source: ControlPlaneSource::Workspace,
            self_hosting_context: SelfHostingContext::FeatureforgeRepo,
            workspace_runtime_warning: Some(String::from("workspace runtime detected")),
            skill_discovery: None,
        }
    }

    #[test]
    fn self_hosting_diagnostic_allows_workspace_runtime_with_temp_state() {
        let provenance = base_provenance();
        let diagnostic = diagnostic_from_provenance(&provenance);
        assert!(diagnostic.live_mutation_allowed);
        assert_eq!(diagnostic.runtime_source, ControlPlaneSource::Workspace);
        assert_eq!(diagnostic.state_dir_kind, StateDirKind::Temp);
        assert!(
            diagnostic
                .recommended_remediation
                .contains("temp-state tests")
        );
    }

    #[test]
    fn self_hosting_diagnostic_marks_workspace_runtime_live_state_blocked() {
        let mut provenance = base_provenance();
        provenance.state_dir = String::from("/Users/alice/.featureforge");
        provenance.state_dir_kind = StateDirKind::Live;
        let diagnostic = diagnostic_from_provenance(&provenance);
        assert!(!diagnostic.live_mutation_allowed);
        assert!(
            diagnostic
                .warnings
                .iter()
                .any(|warning| { warning.contains("workspace_runtime_live_mutation_blocked") })
        );
    }
}
