use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::runtime_provenance::{
    ControlPlaneSource, RuntimeProvenance, StateDirKind, installed_runtime_binary_display_path,
};

pub const WORKSPACE_LIVE_MUTATION_OVERRIDE_ENV: &str =
    "FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRuntimeLiveMutationGuardOutcome {
    pub override_warning: Option<String>,
}

pub fn deny_workspace_runtime_live_mutation(
    provenance: &RuntimeProvenance,
    blocked_command: &str,
) -> Result<WorkspaceRuntimeLiveMutationGuardOutcome, JsonFailure> {
    let override_enabled =
        std::env::var_os(WORKSPACE_LIVE_MUTATION_OVERRIDE_ENV).is_some_and(|value| value == "1");
    deny_workspace_runtime_live_mutation_inner(provenance, blocked_command, override_enabled)
}

fn deny_workspace_runtime_live_mutation_inner(
    provenance: &RuntimeProvenance,
    blocked_command: &str,
    override_enabled: bool,
) -> Result<WorkspaceRuntimeLiveMutationGuardOutcome, JsonFailure> {
    if provenance.state_dir_kind != StateDirKind::Live
        || provenance.control_plane_source == ControlPlaneSource::Installed
    {
        return Ok(WorkspaceRuntimeLiveMutationGuardOutcome {
            override_warning: None,
        });
    }

    let installed_binary_path = installed_runtime_binary_display_path();
    let control_plane_source = control_plane_source_label(provenance.control_plane_source);
    if override_enabled {
        let warning = format!(
            "workspace_runtime_live_mutation_override_active: {env}=1 allows live mutation for `{blocked_command}` using non-installed runtime `{binary_path}` ({control_plane_source}) and live state dir `{state_dir}`.",
            env = WORKSPACE_LIVE_MUTATION_OVERRIDE_ENV,
            binary_path = provenance.binary_path,
            state_dir = provenance.state_dir,
        );
        return Ok(WorkspaceRuntimeLiveMutationGuardOutcome {
            override_warning: Some(warning),
        });
    }

    Err(JsonFailure::new(
        FailureClass::WorkspaceRuntimeLiveMutationBlocked,
        format!(
            "workspace_runtime_live_mutation_blocked: live mutation is blocked when using a non-installed runtime without explicit override.\n\
binary_path: {binary_path}\n\
installed_binary_path: {installed_binary_path}\n\
state_dir: {state_dir}\n\
control_plane_source: {control_plane_source}\n\
blocked_command: {blocked_command}\n\
remediation: rerun through `~/.featureforge/install/bin/featureforge` (or `{installed_binary_path}`).",
            binary_path = provenance.binary_path,
            state_dir = provenance.state_dir,
        ),
    ))
}

fn control_plane_source_label(source: ControlPlaneSource) -> &'static str {
    match source {
        ControlPlaneSource::Installed => "installed",
        ControlPlaneSource::Workspace => "workspace",
        ControlPlaneSource::Path => "path",
        ControlPlaneSource::Unknown => "unknown",
    }
}

pub fn deny_workspace_runtime_live_mutation_for_execution_runtime(
    runtime: &ExecutionRuntime,
    blocked_command: &str,
) -> Result<WorkspaceRuntimeLiveMutationGuardOutcome, JsonFailure> {
    deny_workspace_runtime_live_mutation(&runtime.runtime_provenance(), blocked_command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::runtime_provenance::SelfHostingContext;

    fn workspace_live_provenance() -> RuntimeProvenance {
        RuntimeProvenance {
            binary_path: String::from("/workspace/featureforge/target/debug/featureforge"),
            binary_realpath: String::from("/workspace/featureforge/target/debug/featureforge"),
            runtime_root: String::from("/workspace/featureforge"),
            repo_root: String::from("/workspace/featureforge"),
            state_dir: String::from("/Users/alice/.featureforge"),
            state_dir_kind: StateDirKind::Live,
            control_plane_source: ControlPlaneSource::Workspace,
            self_hosting_context: SelfHostingContext::FeatureforgeRepo,
            workspace_runtime_warning: Some(String::from("workspace runtime detected")),
            skill_discovery: None,
        }
    }

    #[test]
    fn workspace_runtime_live_mutation_guard_blocks_without_override() {
        let failure = deny_workspace_runtime_live_mutation_inner(
            &workspace_live_provenance(),
            "plan execution repair-review-state",
            false,
        )
        .expect_err("workspace live mutation should be blocked without override");
        assert_eq!(
            failure.error_class,
            "workspace_runtime_live_mutation_blocked"
        );
        assert!(
            failure
                .message
                .contains("blocked_command: plan execution repair-review-state")
        );
    }

    #[test]
    fn workspace_runtime_live_mutation_guard_allows_override_with_warning() {
        let outcome = deny_workspace_runtime_live_mutation_inner(
            &workspace_live_provenance(),
            "plan execution close-current-task",
            true,
        )
        .expect("override should allow live mutation");
        assert!(
            outcome
                .override_warning
                .as_deref()
                .is_some_and(|warning| warning.contains(WORKSPACE_LIVE_MUTATION_OVERRIDE_ENV))
        );
    }

    #[test]
    fn workspace_runtime_live_mutation_guard_ignores_non_live_temp_state() {
        let mut provenance = workspace_live_provenance();
        provenance.state_dir_kind = StateDirKind::Temp;
        let outcome = deny_workspace_runtime_live_mutation_inner(
            &provenance,
            "plan execution repair-review-state",
            false,
        )
        .expect("temp-state mutations should remain allowed for workspace runtime");
        assert!(outcome.override_warning.is_none());
    }

    #[test]
    fn workspace_runtime_live_mutation_guard_ignores_installed_runtime() {
        let mut provenance = workspace_live_provenance();
        provenance.control_plane_source = ControlPlaneSource::Installed;
        let outcome = deny_workspace_runtime_live_mutation_inner(
            &provenance,
            "plan execution repair-review-state",
            false,
        )
        .expect("installed runtime should not be blocked");
        assert!(outcome.override_warning.is_none());
    }

    #[test]
    fn workspace_runtime_live_mutation_guard_blocks_path_runtime_with_live_state() {
        let mut provenance = workspace_live_provenance();
        provenance.control_plane_source = ControlPlaneSource::Path;
        provenance.binary_path = String::from("featureforge");
        provenance.binary_realpath = String::from("featureforge");
        let failure = deny_workspace_runtime_live_mutation_inner(
            &provenance,
            "plan execution close-current-task",
            false,
        )
        .expect_err("path runtime should be blocked from live mutation");
        assert_eq!(
            failure.error_class,
            "workspace_runtime_live_mutation_blocked"
        );
        assert!(failure.message.contains("control_plane_source: path"));
    }

    #[test]
    fn workspace_runtime_live_mutation_guard_blocks_unknown_runtime_with_live_state() {
        let mut provenance = workspace_live_provenance();
        provenance.control_plane_source = ControlPlaneSource::Unknown;
        let failure = deny_workspace_runtime_live_mutation_inner(
            &provenance,
            "plan execution repair-review-state",
            false,
        )
        .expect_err("unknown runtime should be blocked from live mutation");
        assert_eq!(
            failure.error_class,
            "workspace_runtime_live_mutation_blocked"
        );
        assert!(failure.message.contains("control_plane_source: unknown"));
    }
}
