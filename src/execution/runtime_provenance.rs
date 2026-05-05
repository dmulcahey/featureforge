use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::execution::runtime::ExecutionRuntime;
use crate::paths::featureforge_home_dir;

const FEATUREFORGE_STATE_DIR_ENV: &str = "FEATUREFORGE_STATE_DIR";
const FEATUREFORGE_BINARY_UNIX: &str = "featureforge";
const FEATUREFORGE_BINARY_WINDOWS: &str = "featureforge.exe";
const CODEX_HOME_ENV: &str = "CODEX_HOME";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlPlaneSource {
    Installed,
    Workspace,
    Path,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StateDirKind {
    Live,
    Temp,
    Custom,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelfHostingContext {
    FeatureforgeRepo,
    NonFeatureforgeRepo,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Installed,
    Workspace,
    Custom,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillDiscoveryRoot {
    pub channel: String,
    pub configured_path: String,
    pub resolved_path: String,
    pub skill_source: SkillSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillDiscoveryProvenance {
    pub installed_skill_root: String,
    pub workspace_skill_root: String,
    #[serde(default)]
    pub active_roots: Vec<SkillDiscoveryRoot>,
    pub active_featureforge_skill_source: SkillSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeProvenance {
    pub binary_path: String,
    pub binary_realpath: String,
    pub runtime_root: String,
    pub repo_root: String,
    pub state_dir: String,
    pub state_dir_kind: StateDirKind,
    pub control_plane_source: ControlPlaneSource,
    pub self_hosting_context: SelfHostingContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_runtime_warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_discovery: Option<SkillDiscoveryProvenance>,
}

#[derive(Debug, Clone)]
struct RuntimeProvenanceInputs {
    binary_path: PathBuf,
    binary_realpath: PathBuf,
    repo_root: PathBuf,
    state_dir: PathBuf,
    home_dir: Option<PathBuf>,
    state_dir_env_set: bool,
    argv0: Option<OsString>,
    codex_home: Option<PathBuf>,
}

impl RuntimeProvenanceInputs {
    fn for_runtime(runtime: &ExecutionRuntime) -> Self {
        Self::for_paths(runtime.repo_root.clone(), runtime.state_dir.clone())
    }

    fn for_paths(repo_root: PathBuf, state_dir: PathBuf) -> Self {
        let binary_path = env::current_exe().unwrap_or_default();
        let binary_realpath = canonicalize_or_self(&binary_path);
        let repo_root = canonicalize_or_self(&repo_root);
        let state_dir = canonicalize_or_self(&state_dir);
        let home_dir = featureforge_home_dir().map(|path| canonicalize_or_self(&path));
        let state_dir_env_set = env::var_os(FEATUREFORGE_STATE_DIR_ENV).is_some();
        let argv0 = env::args_os().next();
        let codex_home = env::var_os(CODEX_HOME_ENV)
            .map(PathBuf::from)
            .map(|path| canonicalize_or_self(&path));
        Self {
            binary_path,
            binary_realpath,
            repo_root,
            state_dir,
            home_dir,
            state_dir_env_set,
            argv0,
            codex_home,
        }
    }
}

pub fn runtime_provenance(runtime: &ExecutionRuntime) -> RuntimeProvenance {
    classify_runtime_provenance(RuntimeProvenanceInputs::for_runtime(runtime))
}

pub fn runtime_provenance_for_paths(repo_root: &Path, state_dir: &Path) -> RuntimeProvenance {
    classify_runtime_provenance(RuntimeProvenanceInputs::for_paths(
        repo_root.to_path_buf(),
        state_dir.to_path_buf(),
    ))
}

pub fn classify_state_dir_kind_for_path(
    state_dir: &Path,
    home_dir: Option<&Path>,
    state_dir_env_set: bool,
) -> StateDirKind {
    classify_state_dir_kind(
        &canonicalize_or_self(state_dir),
        home_dir.map(canonicalize_or_self).as_deref(),
        state_dir_env_set,
    )
}

pub fn featureforge_runtime_binary_name() -> &'static str {
    if cfg!(windows) {
        FEATUREFORGE_BINARY_WINDOWS
    } else {
        FEATUREFORGE_BINARY_UNIX
    }
}

pub fn installed_runtime_binary_display_path() -> String {
    featureforge_home_dir()
        .map(|home| {
            installed_runtime_binary_path_for_home(&home)
                .display()
                .to_string()
        })
        .unwrap_or_else(|| {
            format!(
                "~/.featureforge/install/bin/{}",
                featureforge_runtime_binary_name()
            )
        })
}

fn classify_runtime_provenance(inputs: RuntimeProvenanceInputs) -> RuntimeProvenance {
    let installed_binary_paths = installed_runtime_binary_paths(inputs.home_dir.as_deref());
    let installed_binary_realpaths = installed_binary_paths
        .iter()
        .map(|path| canonicalize_or_self(path))
        .collect::<Vec<_>>();

    let control_plane_source = classify_control_plane_source(
        &inputs.binary_path,
        &inputs.binary_realpath,
        &inputs.repo_root,
        &installed_binary_paths,
        &installed_binary_realpaths,
        inputs.argv0.as_ref(),
    );
    let state_dir_kind = classify_state_dir_kind(
        &inputs.state_dir,
        inputs.home_dir.as_deref(),
        inputs.state_dir_env_set,
    );
    let self_hosting_context = classify_self_hosting_context(&inputs.repo_root);
    let workspace_runtime_warning =
        workspace_runtime_warning(control_plane_source, state_dir_kind, self_hosting_context);
    let skill_discovery = classify_skill_discovery(
        &inputs.repo_root,
        inputs.home_dir.as_deref(),
        inputs.codex_home.as_deref(),
        self_hosting_context,
    );

    RuntimeProvenance {
        binary_path: path_display(&inputs.binary_path),
        binary_realpath: path_display(&inputs.binary_realpath),
        runtime_root: runtime_root_for_binary(&inputs.binary_realpath),
        repo_root: path_display(&inputs.repo_root),
        state_dir: path_display(&inputs.state_dir),
        state_dir_kind,
        control_plane_source,
        self_hosting_context,
        workspace_runtime_warning,
        skill_discovery,
    }
}

fn classify_control_plane_source(
    binary_path: &Path,
    binary_realpath: &Path,
    repo_root: &Path,
    installed_binary_paths: &[PathBuf],
    installed_binary_realpaths: &[PathBuf],
    argv0: Option<&OsString>,
) -> ControlPlaneSource {
    if binary_realpath.as_os_str().is_empty() && binary_path.as_os_str().is_empty() {
        return ControlPlaneSource::Unknown;
    }

    // Exact installed runtime paths are authoritative even when the install root
    // itself is the current repository checkout.
    if installed_binary_paths
        .iter()
        .any(|installed_path| paths_equal(binary_realpath, installed_path))
    {
        return ControlPlaneSource::Installed;
    }

    // Realpath-under-repo remains the strongest workspace signal after exact
    // install-path matching so symlinked install wrappers still classify as workspace
    // when they resolve to a repo-local runtime binary.
    if !repo_root.as_os_str().is_empty() && path_is_within(binary_realpath, repo_root) {
        return ControlPlaneSource::Workspace;
    }

    if installed_binary_realpaths
        .iter()
        .any(|installed_realpath| paths_equal(binary_realpath, installed_realpath))
    {
        return ControlPlaneSource::Installed;
    }

    if installed_binary_paths
        .iter()
        .any(|installed_path| paths_equal(binary_path, installed_path))
    {
        return ControlPlaneSource::Installed;
    }

    if let Some(argv0) = argv0
        && argv0_uses_path_lookup(argv0)
    {
        return ControlPlaneSource::Path;
    }

    if !binary_realpath.as_os_str().is_empty() {
        return ControlPlaneSource::Path;
    }

    ControlPlaneSource::Unknown
}

fn classify_state_dir_kind(
    state_dir: &Path,
    home_dir: Option<&Path>,
    state_dir_env_set: bool,
) -> StateDirKind {
    if state_dir.as_os_str().is_empty() {
        return StateDirKind::Unknown;
    }

    if !state_dir_env_set {
        return StateDirKind::Live;
    }

    if let Some(home_dir) = home_dir {
        let canonical_live = canonicalize_or_self(&home_dir.join(".featureforge"));
        if path_is_within(state_dir, &canonical_live) {
            return StateDirKind::Live;
        }
    }

    if path_is_under_temp_dir(state_dir) || looks_like_fixture_state_dir(state_dir) {
        return StateDirKind::Temp;
    }

    StateDirKind::Custom
}

fn classify_self_hosting_context(repo_root: &Path) -> SelfHostingContext {
    if repo_root.as_os_str().is_empty() {
        return SelfHostingContext::Unknown;
    }

    if is_featureforge_repo(repo_root) {
        return SelfHostingContext::FeatureforgeRepo;
    }

    SelfHostingContext::NonFeatureforgeRepo
}

fn is_featureforge_repo(repo_root: &Path) -> bool {
    if cargo_manifest_declares_featureforge(repo_root) {
        return true;
    }

    let has_skill_marker = repo_root
        .join("skills")
        .join("using-featureforge")
        .join("SKILL.md.tmpl")
        .exists()
        || repo_root
            .join("skills")
            .join("using-featureforge")
            .join("SKILL.md")
            .exists();
    let has_runtime_marker = repo_root
        .join("src")
        .join("workflow")
        .join("operator.rs")
        .exists()
        || repo_root
            .join("src")
            .join("execution")
            .join("runtime_provenance.rs")
            .exists();

    has_skill_marker && has_runtime_marker
}

fn cargo_manifest_declares_featureforge(repo_root: &Path) -> bool {
    let manifest_path = repo_root.join("Cargo.toml");
    let Ok(contents) = fs::read_to_string(manifest_path) else {
        return false;
    };
    let mut in_package_section = false;
    for raw_line in contents.lines() {
        let line = raw_line
            .split('#')
            .next()
            .map(str::trim)
            .unwrap_or_default();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_package_section = line == "[package]";
            continue;
        }
        if !in_package_section {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && name.trim() == "name"
        {
            let parsed = value.trim().trim_matches('"');
            return parsed == "featureforge";
        }
    }

    false
}

fn workspace_runtime_warning(
    control_plane_source: ControlPlaneSource,
    state_dir_kind: StateDirKind,
    self_hosting_context: SelfHostingContext,
) -> Option<String> {
    if control_plane_source != ControlPlaneSource::Workspace {
        return None;
    }

    let mut message = String::from(
        "workspace runtime detected; use installed runtime for live workflow control-plane commands",
    );
    if state_dir_kind == StateDirKind::Live {
        message.push_str(" (live state dir)");
    }
    if self_hosting_context == SelfHostingContext::FeatureforgeRepo {
        message.push_str(" in featureforge repository");
    }
    Some(message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillDiscoveryCandidate {
    channel: &'static str,
    configured_path: PathBuf,
}

fn classify_skill_discovery(
    repo_root: &Path,
    home_dir: Option<&Path>,
    codex_home: Option<&Path>,
    self_hosting_context: SelfHostingContext,
) -> Option<SkillDiscoveryProvenance> {
    let home_dir = home_dir?;
    let installed_skill_root = canonicalize_or_self(
        &home_dir
            .join(".featureforge")
            .join("install")
            .join("skills"),
    );
    let workspace_skill_root = canonicalize_or_self(&repo_root.join("skills"));
    let mut active_roots = Vec::new();

    for candidate in active_skill_discovery_candidates(home_dir, codex_home) {
        if !candidate.configured_path.exists() {
            continue;
        }
        let resolved_path = canonicalize_or_self(&candidate.configured_path);
        let skill_source =
            classify_skill_source(&resolved_path, &installed_skill_root, &workspace_skill_root);
        active_roots.push(SkillDiscoveryRoot {
            channel: candidate.channel.to_owned(),
            configured_path: path_display(&candidate.configured_path),
            resolved_path: path_display(&resolved_path),
            skill_source,
        });
    }

    let active_featureforge_skill_source = summarize_skill_sources(&active_roots);
    let warning = workspace_skill_warning(
        &active_roots,
        self_hosting_context,
        &installed_skill_root,
        &workspace_skill_root,
    );

    Some(SkillDiscoveryProvenance {
        installed_skill_root: path_display(&installed_skill_root),
        workspace_skill_root: path_display(&workspace_skill_root),
        active_roots,
        active_featureforge_skill_source,
        warning,
    })
}

fn active_skill_discovery_candidates(
    home_dir: &Path,
    codex_home: Option<&Path>,
) -> Vec<SkillDiscoveryCandidate> {
    let codex_home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home_dir.join(".codex"));
    vec![
        SkillDiscoveryCandidate {
            channel: "codex_agents_skill_link",
            configured_path: home_dir.join(".agents").join("skills").join("featureforge"),
        },
        SkillDiscoveryCandidate {
            channel: "copilot_skills_link",
            configured_path: home_dir.join(".copilot").join("skills"),
        },
        SkillDiscoveryCandidate {
            channel: "codex_skills_featureforge",
            configured_path: codex_home.join("skills").join("featureforge"),
        },
    ]
}

fn classify_skill_source(
    resolved_path: &Path,
    installed_skill_root: &Path,
    workspace_skill_root: &Path,
) -> SkillSource {
    if resolved_path.as_os_str().is_empty() {
        return SkillSource::Unknown;
    }
    if path_matches_or_is_within(resolved_path, installed_skill_root) {
        return SkillSource::Installed;
    }
    if path_matches_or_is_within(resolved_path, workspace_skill_root) {
        return SkillSource::Workspace;
    }
    SkillSource::Custom
}

fn path_matches_or_is_within(path: &Path, root: &Path) -> bool {
    !root.as_os_str().is_empty() && (paths_equal(path, root) || path_is_within(path, root))
}

fn summarize_skill_sources(active_roots: &[SkillDiscoveryRoot]) -> SkillSource {
    if active_roots
        .iter()
        .any(|root| root.skill_source == SkillSource::Workspace)
    {
        return SkillSource::Workspace;
    }
    if active_roots
        .iter()
        .any(|root| root.skill_source == SkillSource::Custom)
    {
        return SkillSource::Custom;
    }
    if active_roots
        .iter()
        .any(|root| root.skill_source == SkillSource::Installed)
    {
        return SkillSource::Installed;
    }
    SkillSource::Unknown
}

fn workspace_skill_warning(
    active_roots: &[SkillDiscoveryRoot],
    self_hosting_context: SelfHostingContext,
    installed_skill_root: &Path,
    workspace_skill_root: &Path,
) -> Option<String> {
    if self_hosting_context != SelfHostingContext::FeatureforgeRepo {
        return None;
    }
    let workspace_root = path_display(workspace_skill_root);
    let workspace_channels = active_roots
        .iter()
        .filter(|root| root.skill_source == SkillSource::Workspace)
        .map(|root| root.channel.as_str())
        .collect::<Vec<_>>();
    if workspace_channels.is_empty() {
        return None;
    }
    Some(format!(
        "workspace skill discovery root detected in active FeatureForge channels ({channels}); fail closed and relink active skills to `{installed}` instead of `{workspace}` during FeatureForge development",
        channels = workspace_channels.join(", "),
        installed = path_display(installed_skill_root),
        workspace = workspace_root,
    ))
}

fn runtime_root_for_binary(binary_path: &Path) -> String {
    let Some(bin_dir) = binary_path.parent() else {
        return String::new();
    };
    let file_name = binary_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let is_runtime_binary = matches!(
        file_name,
        FEATUREFORGE_BINARY_UNIX | FEATUREFORGE_BINARY_WINDOWS
    );
    if !is_runtime_binary {
        return String::new();
    }
    if bin_dir.file_name().and_then(|name| name.to_str()) != Some("bin") {
        return String::new();
    }
    let Some(runtime_root) = bin_dir.parent() else {
        return String::new();
    };
    path_display(runtime_root)
}

fn installed_runtime_binary_path_for_home(home_dir: &Path) -> PathBuf {
    installed_runtime_bin_dir_for_home(home_dir).join(featureforge_runtime_binary_name())
}

fn installed_runtime_bin_dir_for_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".featureforge").join("install").join("bin")
}

fn installed_runtime_binary_paths(home_dir: Option<&Path>) -> Vec<PathBuf> {
    let Some(home_dir) = home_dir else {
        return Vec::new();
    };

    let install_bin_dir = installed_runtime_bin_dir_for_home(home_dir);
    vec![
        install_bin_dir.join(FEATUREFORGE_BINARY_UNIX),
        install_bin_dir.join(FEATUREFORGE_BINARY_WINDOWS),
    ]
}

fn path_is_under_temp_dir(path: &Path) -> bool {
    let mut state_dir_candidates = vec![path.to_path_buf(), canonicalize_or_self(path)];
    append_macos_private_aliases(&mut state_dir_candidates);
    let temp_dir = env::temp_dir();
    let mut temp_dir_candidates = vec![temp_dir.clone(), canonicalize_or_self(&temp_dir)];
    append_macos_private_aliases(&mut temp_dir_candidates);

    state_dir_candidates.iter().any(|state_dir| {
        temp_dir_candidates
            .iter()
            .any(|temp| path_is_within(state_dir, temp))
    })
}

fn append_macos_private_aliases(paths: &mut Vec<PathBuf>) {
    let existing = paths.clone();
    for path in existing {
        let display = path_display(&path);
        if display.starts_with("/private/") {
            let alias = PathBuf::from(display.trim_start_matches("/private"));
            if !paths.iter().any(|existing_path| existing_path == &alias) {
                paths.push(alias);
            }
        } else if display.starts_with("/var/") {
            let alias = PathBuf::from(format!("/private{display}"));
            if !paths.iter().any(|existing_path| existing_path == &alias) {
                paths.push(alias);
            }
        }
    }
}

fn looks_like_fixture_state_dir(path: &Path) -> bool {
    let lowered = path_display(path).to_ascii_lowercase();
    lowered.contains("/tests/fixtures/")
        || lowered.contains("\\tests\\fixtures\\")
        || lowered.contains("fixture-state")
}

fn argv0_uses_path_lookup(argv0: &OsString) -> bool {
    let raw = argv0.to_string_lossy();
    !raw.contains('/')
        && !raw.contains('\\')
        && !raw.trim().is_empty()
        && !Path::new(raw.as_ref()).is_absolute()
}

fn path_is_within(path: &Path, parent: &Path) -> bool {
    path.starts_with(parent)
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return PathBuf::new();
    }
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture_inputs() -> RuntimeProvenanceInputs {
        RuntimeProvenanceInputs {
            binary_path: PathBuf::new(),
            binary_realpath: PathBuf::new(),
            repo_root: PathBuf::from("/workspace/featureforge"),
            state_dir: PathBuf::from("/tmp/featureforge-state"),
            home_dir: Some(PathBuf::from("/Users/alice")),
            state_dir_env_set: true,
            argv0: Some(OsString::from("featureforge")),
            codex_home: Some(PathBuf::from("/Users/alice/.codex")),
        }
    }

    fn seed_featureforge_repo_markers(repo_root: &Path) {
        let skill_template = repo_root.join("skills").join("using-featureforge");
        fs::create_dir_all(&skill_template).expect("skill template directory should be creatable");
        fs::write(skill_template.join("SKILL.md.tmpl"), "template")
            .expect("skill template marker should be writable");
        let workflow_marker = repo_root.join("src").join("workflow");
        fs::create_dir_all(&workflow_marker)
            .expect("workflow marker directory should be creatable");
        fs::write(workflow_marker.join("operator.rs"), "// marker")
            .expect("workflow marker should be writable");
    }

    fn write_featureforge_manifest(repo_root: &Path) {
        fs::create_dir_all(repo_root).expect("repo root should be creatable");
        let manifest_path = repo_root.join("Cargo.toml");
        let mut file = fs::File::create(&manifest_path).expect("Cargo.toml should be writable");
        writeln!(file, "[package]").expect("manifest should write package header");
        writeln!(file, "name = \"featureforge\"").expect("manifest should write package name");
        writeln!(file, "version = \"0.0.0\"").expect("manifest should write version");
    }

    #[test]
    fn runtime_provenance_classifies_installed_runtime() {
        let mut inputs = fixture_inputs();
        let binary = PathBuf::from("/Users/alice/.featureforge/install/bin/featureforge");
        inputs.binary_path = binary.clone();
        inputs.binary_realpath = binary;
        inputs.state_dir = PathBuf::from("/Users/alice/.featureforge");
        inputs.state_dir_env_set = false;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(
            provenance.control_plane_source,
            ControlPlaneSource::Installed
        );
        assert_eq!(provenance.state_dir_kind, StateDirKind::Live);
    }

    #[test]
    fn runtime_provenance_classifies_installed_runtime_when_repo_root_is_install_root() {
        let mut inputs = fixture_inputs();
        let install_root = PathBuf::from("/Users/alice/.featureforge/install");
        let binary = install_root.join("bin").join("featureforge");
        inputs.repo_root = install_root;
        inputs.binary_path = binary.clone();
        inputs.binary_realpath = binary;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(
            provenance.control_plane_source,
            ControlPlaneSource::Installed
        );
    }

    #[test]
    fn runtime_provenance_classifies_installed_runtime_windows_binary_name() {
        let mut inputs = fixture_inputs();
        let binary = PathBuf::from("C:/Users/alice/.featureforge/install/bin/featureforge.exe");
        inputs.home_dir = Some(PathBuf::from("C:/Users/alice"));
        inputs.binary_path = binary.clone();
        inputs.binary_realpath = binary;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(
            provenance.control_plane_source,
            ControlPlaneSource::Installed
        );
    }

    #[test]
    fn runtime_provenance_classifies_workspace_runtime() {
        let temp_root = tempfile::tempdir().expect("temp dir should exist");
        let repo_root = temp_root.path().join("renamed-worktree");
        seed_featureforge_repo_markers(&repo_root);
        let workspace_binary = repo_root.join("bin/featureforge");
        fs::create_dir_all(
            workspace_binary
                .parent()
                .expect("workspace binary should have parent"),
        )
        .expect("workspace binary parent should be creatable");
        let mut inputs = fixture_inputs();
        inputs.repo_root = repo_root;
        inputs.binary_path = workspace_binary.clone();
        inputs.binary_realpath = workspace_binary;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(
            provenance.control_plane_source,
            ControlPlaneSource::Workspace
        );
        assert_eq!(
            provenance.self_hosting_context,
            SelfHostingContext::FeatureforgeRepo
        );
        assert!(provenance.workspace_runtime_warning.is_some());
    }

    #[test]
    fn runtime_provenance_classifies_path_runtime() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.repo_root = PathBuf::from("/workspace/other-repo");
        inputs.state_dir = PathBuf::from("/opt/state/custom");
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.control_plane_source, ControlPlaneSource::Path);
        assert_eq!(provenance.state_dir_kind, StateDirKind::Custom);
    }

    #[test]
    fn runtime_provenance_classifies_unknown_runtime() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::new();
        inputs.binary_realpath = PathBuf::new();
        inputs.argv0 = None;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.control_plane_source, ControlPlaneSource::Unknown);
    }

    #[test]
    fn runtime_provenance_classifies_temp_state_dir() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.state_dir = env::temp_dir().join("featureforge-provenance-state");
        inputs.state_dir_env_set = true;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.state_dir_kind, StateDirKind::Temp);
    }

    #[test]
    fn runtime_provenance_classifies_custom_state_dir() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.state_dir = PathBuf::from("/var/lib/featureforge-state");
        inputs.state_dir_env_set = true;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.state_dir_kind, StateDirKind::Custom);
    }

    #[test]
    fn runtime_provenance_detects_fixture_state_dir_as_temp() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.state_dir = PathBuf::from("/workspace/repo/tests/fixtures/featureforge-state");
        inputs.state_dir_env_set = true;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.state_dir_kind, StateDirKind::Temp);
    }

    #[test]
    fn runtime_provenance_detects_runtime_root_for_featureforge_binary() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/Users/alice/.featureforge/install/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(
            provenance.runtime_root,
            "/Users/alice/.featureforge/install".to_owned()
        );
    }

    #[test]
    fn runtime_provenance_omits_runtime_root_for_non_runtime_binary_name() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/not-featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        let provenance = classify_runtime_provenance(inputs);
        assert!(provenance.runtime_root.is_empty());
    }

    #[test]
    fn runtime_provenance_defaults_to_live_when_state_dir_env_is_unset() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.state_dir = PathBuf::from("/var/unknown/.featureforge");
        inputs.state_dir_env_set = false;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.state_dir_kind, StateDirKind::Live);
    }

    #[test]
    fn runtime_provenance_path_classifier_treats_shell_lookup_as_path_source() {
        assert!(argv0_uses_path_lookup(&OsString::from("featureforge")));
        assert!(!argv0_uses_path_lookup(&OsString::from("./featureforge")));
        assert!(!argv0_uses_path_lookup(&OsString::from(
            "/usr/local/bin/featureforge"
        )));
    }

    #[test]
    fn runtime_provenance_classifies_non_featureforge_repo_self_hosting_context() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.repo_root = PathBuf::from("/workspace/other-repo");
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(
            provenance.self_hosting_context,
            SelfHostingContext::NonFeatureforgeRepo
        );
    }

    #[test]
    fn runtime_provenance_classifies_unknown_state_dir_when_empty() {
        let mut inputs = fixture_inputs();
        inputs.state_dir = PathBuf::new();
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.state_dir_kind, StateDirKind::Unknown);
    }

    #[test]
    fn runtime_provenance_classifies_unknown_self_hosting_context_when_repo_missing() {
        let mut inputs = fixture_inputs();
        inputs.repo_root = PathBuf::new();
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.self_hosting_context, SelfHostingContext::Unknown);
    }

    #[test]
    fn runtime_provenance_classifies_featureforge_repo_from_markers_when_repo_name_differs() {
        let temp_root = tempfile::tempdir().expect("temp dir should exist");
        let repo_root = temp_root.path().join("featureforge-renamed-worktree");
        seed_featureforge_repo_markers(&repo_root);
        assert_eq!(
            classify_self_hosting_context(&repo_root),
            SelfHostingContext::FeatureforgeRepo
        );
    }

    #[test]
    fn runtime_provenance_classifies_featureforge_repo_from_manifest_when_repo_name_differs() {
        let temp_root = tempfile::tempdir().expect("temp dir should exist");
        let repo_root = temp_root.path().join("renamed-worktree");
        write_featureforge_manifest(&repo_root);
        assert_eq!(
            classify_self_hosting_context(&repo_root),
            SelfHostingContext::FeatureforgeRepo
        );
    }

    #[test]
    fn runtime_provenance_repo_named_featureforge_without_markers_is_non_featureforge() {
        let temp_root = tempfile::tempdir().expect("temp dir should exist");
        let repo_root = temp_root.path().join("featureforge");
        fs::create_dir_all(&repo_root).expect("repo path should be creatable");
        assert_eq!(
            classify_self_hosting_context(&repo_root),
            SelfHostingContext::NonFeatureforgeRepo
        );
    }

    #[test]
    fn runtime_provenance_classifies_custom_runtime_path_without_path_lookup_argv() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/opt/featureforge/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.argv0 = Some(OsString::from("/opt/featureforge/bin/featureforge"));
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.control_plane_source, ControlPlaneSource::Path);
    }

    #[test]
    fn runtime_provenance_normalizes_path_component_boundaries_for_starts_with() {
        let repo = PathBuf::from("/workspace/featureforge");
        let workspace_binary = PathBuf::from("/workspace/featureforge/bin/featureforge");
        assert!(path_is_within(&workspace_binary, &repo));
        let not_workspace = PathBuf::from("/workspace/featureforge-other/bin/featureforge");
        assert!(!path_is_within(&not_workspace, &repo));
    }

    #[test]
    fn runtime_provenance_fixture_detection_is_case_insensitive() {
        let fixture = PathBuf::from("/Repo/Tests/Fixtures/Featureforge-State");
        assert!(looks_like_fixture_state_dir(&fixture));
    }

    #[test]
    fn runtime_provenance_detects_windows_runtime_binary_name() {
        let root = PathBuf::from("C:/featureforge/install/bin/featureforge.exe");
        assert_eq!(
            runtime_root_for_binary(&root),
            "C:/featureforge/install".to_owned()
        );
    }

    #[test]
    fn runtime_provenance_reports_repo_root_and_state_dir_verbatim() {
        let mut inputs = fixture_inputs();
        inputs.repo_root = PathBuf::from("/repo/path");
        inputs.state_dir = PathBuf::from("/state/path");
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.repo_root, "/repo/path".to_owned());
        assert_eq!(provenance.state_dir, "/state/path".to_owned());
    }

    #[test]
    fn runtime_provenance_builds_workspace_warning_only_for_workspace_binaries() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        let provenance = classify_runtime_provenance(inputs);
        assert!(provenance.workspace_runtime_warning.is_none());
    }

    #[test]
    fn runtime_provenance_workspace_warning_mentions_live_state_when_applicable() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/workspace/featureforge/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.state_dir = PathBuf::from("/Users/alice/.featureforge");
        inputs.state_dir_env_set = false;
        let provenance = classify_runtime_provenance(inputs);
        let warning = provenance
            .workspace_runtime_warning
            .expect("workspace runtime should produce warning");
        assert!(warning.contains("live state dir"));
    }

    #[test]
    fn runtime_provenance_workspace_warning_mentions_featureforge_repo_context() {
        let temp_root = tempfile::tempdir().expect("temp dir should exist");
        let repo_root = temp_root.path().join("renamed-worktree");
        seed_featureforge_repo_markers(&repo_root);
        let workspace_binary = repo_root.join("bin/featureforge");
        fs::create_dir_all(
            workspace_binary
                .parent()
                .expect("workspace binary should have parent"),
        )
        .expect("workspace binary parent should be creatable");
        let mut inputs = fixture_inputs();
        inputs.repo_root = repo_root;
        inputs.binary_path = workspace_binary.clone();
        inputs.binary_realpath = workspace_binary;
        let provenance = classify_runtime_provenance(inputs);
        let warning = provenance
            .workspace_runtime_warning
            .expect("workspace runtime should produce warning");
        assert!(warning.contains("featureforge repository"));
    }

    #[test]
    fn runtime_provenance_workspace_warning_omits_featureforge_suffix_in_other_repos() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/workspace/other-repo/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.repo_root = PathBuf::from("/workspace/other-repo");
        let provenance = classify_runtime_provenance(inputs);
        let warning = provenance
            .workspace_runtime_warning
            .expect("workspace runtime should produce warning");
        assert!(!warning.contains("featureforge repository"));
    }

    #[test]
    fn runtime_provenance_prefers_workspace_source_over_installed_symlink_realpath() {
        let mut inputs = fixture_inputs();
        inputs.home_dir = Some(PathBuf::from("/Users/alice"));
        inputs.repo_root = PathBuf::from("/workspace/featureforge");
        inputs.binary_path = PathBuf::from("/Users/alice/.featureforge/install/bin/featureforge");
        inputs.binary_realpath = PathBuf::from("/workspace/featureforge/target/debug/featureforge");
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(
            provenance.control_plane_source,
            ControlPlaneSource::Workspace
        );
    }

    #[test]
    fn runtime_provenance_does_not_treat_temp_prefix_sibling_as_temp() {
        let mut inputs = fixture_inputs();
        inputs.binary_path = PathBuf::from("/usr/local/bin/featureforge");
        inputs.binary_realpath = inputs.binary_path.clone();
        inputs.state_dir = PathBuf::from("/tmp-featureforge-state");
        inputs.state_dir_env_set = true;
        let provenance = classify_runtime_provenance(inputs);
        assert_eq!(provenance.state_dir_kind, StateDirKind::Custom);
    }

    #[test]
    fn runtime_provenance_classifies_active_installed_skill_root() {
        let temp_root = tempfile::tempdir().expect("temp dir should exist");
        let home = temp_root.path().join("home");
        let install_skills = home.join(".featureforge/install/skills");
        let codex_skill_root = install_skills.join("featureforge");
        fs::create_dir_all(&codex_skill_root).expect("installed skills root should be creatable");

        let mut inputs = fixture_inputs();
        inputs.home_dir = Some(home.clone());
        inputs.codex_home = Some(home.join(".featureforge/install"));
        let provenance = classify_runtime_provenance(inputs);
        let skill_discovery = provenance
            .skill_discovery
            .expect("skill discovery should be present when home is available");
        assert_eq!(
            skill_discovery.active_featureforge_skill_source,
            SkillSource::Installed
        );
        assert!(
            skill_discovery
                .active_roots
                .iter()
                .any(|root| root.skill_source == SkillSource::Installed),
            "active roots should include an installed skill source: {skill_discovery:?}"
        );
    }

    #[test]
    fn runtime_provenance_classifies_active_workspace_skill_root() {
        let temp_root = tempfile::tempdir().expect("temp dir should exist");
        let home = temp_root.path().join("home");
        let repo = temp_root.path().join("featureforge-renamed-worktree");
        let workspace_skills = repo.join("skills/featureforge");
        fs::create_dir_all(&workspace_skills).expect("workspace skills root should be creatable");
        seed_featureforge_repo_markers(&repo);

        let mut inputs = fixture_inputs();
        inputs.home_dir = Some(home.clone());
        inputs.repo_root = repo.clone();
        inputs.codex_home = Some(repo);
        let provenance = classify_runtime_provenance(inputs);
        let skill_discovery = provenance
            .skill_discovery
            .expect("skill discovery should be present when home is available");
        assert_eq!(
            skill_discovery.active_featureforge_skill_source,
            SkillSource::Workspace
        );
        assert!(
            skill_discovery
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains(
                    "workspace skill discovery root detected in active FeatureForge channels"
                )),
            "workspace skill warning should be present for active workspace skill roots: {skill_discovery:?}"
        );
    }
}
