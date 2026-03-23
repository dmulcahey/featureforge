use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use schemars::schema_for;
use serde::Serialize;

use crate::cli::workflow::ArtifactKind;
use crate::contracts::plan::{AnalyzePlanReport, analyze_documents, parse_plan_file};
use crate::contracts::spec::parse_spec_file;
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::git::{RepositoryIdentity, discover_repo_identity};
use crate::paths::RepoPath;
use crate::workflow::manifest::{WorkflowManifest, load_manifest, manifest_path, save_manifest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowRoute {
    pub schema_version: u32,
    pub status: String,
    pub next_skill: String,
    pub spec_path: String,
    pub plan_path: String,
    pub contract_state: String,
    pub reason_codes: Vec<String>,
    pub diagnostics: Vec<WorkflowDiagnostic>,
    pub scan_truncated: bool,
    pub spec_candidate_count: usize,
    pub plan_candidate_count: usize,
    pub manifest_path: String,
    pub root: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowDiagnostic {
    pub code: String,
    pub severity: String,
    pub artifact: String,
    pub message: String,
    pub remediation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WorkflowPhase {
    pub phase: String,
    pub route_status: String,
    pub next_skill: String,
    pub next_action: String,
    pub spec_path: String,
    pub plan_path: String,
    pub session_entry: SessionEntryState,
    pub route: WorkflowRoute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SessionEntryState {
    pub outcome: String,
    pub decision_source: String,
    pub session_key: String,
    pub decision_path: String,
    pub policy_source: String,
    pub persisted: bool,
    pub failure_class: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct WorkflowRuntime {
    pub identity: RepositoryIdentity,
    pub state_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: Option<WorkflowManifest>,
}

#[derive(Debug, Clone)]
struct WorkflowSpecCandidate {
    path: String,
    workflow_state: String,
}

#[derive(Debug, Clone)]
struct WorkflowPlanCandidate {
    path: String,
    workflow_state: String,
    source_spec_path: String,
}

impl WorkflowRuntime {
    pub fn discover(current_dir: &Path) -> Result<Self, DiagnosticError> {
        let identity = discover_repo_identity(current_dir)?;
        let state_dir = state_dir();
        let manifest_path = manifest_path(&identity, &state_dir);
        let manifest = load_manifest(&manifest_path);
        Ok(Self {
            identity,
            state_dir,
            manifest_path,
            manifest,
        })
    }

    pub fn status(&self) -> Result<WorkflowRoute, DiagnosticError> {
        resolve_route(self, false)
    }

    pub fn resolve(&self) -> Result<WorkflowRoute, DiagnosticError> {
        resolve_route(self, true)
    }

    pub fn expect(
        &mut self,
        artifact: ArtifactKind,
        raw_path: &Path,
    ) -> Result<WorkflowRoute, DiagnosticError> {
        let repo_path = normalize_repo_path(raw_path)?;
        let mut manifest = self.manifest.clone().unwrap_or_else(|| WorkflowManifest {
            version: 1,
            repo_root: self.identity.repo_root.to_string_lossy().into_owned(),
            branch: self.identity.branch_name.clone(),
            expected_spec_path: String::new(),
            expected_plan_path: String::new(),
            status: String::from("needs_brainstorming"),
            next_skill: String::from("superpowers:brainstorming"),
            reason: String::new(),
            note: String::new(),
            updated_at: String::from("1970-01-01T00:00:00Z"),
        });
        match artifact {
            ArtifactKind::Spec => {
                manifest.expected_spec_path = repo_path.clone();
                manifest.expected_plan_path.clear();
                manifest.status = String::from("needs_brainstorming");
                manifest.next_skill = String::from("superpowers:brainstorming");
                manifest.reason = String::from("missing_expected_spec,expect_set");
                manifest.note = manifest.reason.clone();
            }
            ArtifactKind::Plan => {
                manifest.expected_plan_path = repo_path.clone();
                manifest.status = String::from("plan_draft");
                manifest.next_skill = String::from("superpowers:plan-eng-review");
                manifest.reason = String::from("missing_expected_plan,expect_set");
                manifest.note = manifest.reason.clone();
            }
        }
        save_manifest(&self.manifest_path, &manifest).map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!(
                    "Could not write workflow manifest {}: {err}",
                    self.manifest_path.display()
                ),
            )
        })?;
        self.manifest = Some(manifest);
        self.status()
    }

    pub fn sync(
        &mut self,
        artifact: ArtifactKind,
        path: Option<&Path>,
    ) -> Result<WorkflowRoute, DiagnosticError> {
        let repo_path = if let Some(path) = path {
            normalize_repo_path(path)?
        } else {
            self.manifest
                .as_ref()
                .and_then(|manifest| match artifact {
                    ArtifactKind::Spec if !manifest.expected_spec_path.is_empty() => {
                        Some(manifest.expected_spec_path.clone())
                    }
                    ArtifactKind::Plan if !manifest.expected_plan_path.is_empty() => {
                        Some(manifest.expected_plan_path.clone())
                    }
                    _ => None,
                })
                .unwrap_or_default()
        };

        if repo_path.is_empty() {
            return self.status();
        }

        let mut route = self.status()?;
        if !self.identity.repo_root.join(&repo_path).is_file() {
            route.status = match artifact {
                ArtifactKind::Spec => String::from("needs_brainstorming"),
                ArtifactKind::Plan => String::from("plan_draft"),
            };
            route.next_skill = match artifact {
                ArtifactKind::Spec => String::from("superpowers:brainstorming"),
                ArtifactKind::Plan => String::from("superpowers:plan-eng-review"),
            };
            route.spec_path = if matches!(artifact, ArtifactKind::Spec) {
                repo_path.clone()
            } else {
                route.spec_path
            };
            route.plan_path = if matches!(artifact, ArtifactKind::Plan) {
                repo_path.clone()
            } else {
                route.plan_path
            };
            route.reason_codes = match artifact {
                ArtifactKind::Spec => vec![
                    String::from("missing_expected_spec"),
                    String::from("sync_spec"),
                    String::from("missing_artifact"),
                ],
                ArtifactKind::Plan => vec![
                    String::from("missing_expected_plan"),
                    String::from("sync_plan"),
                    String::from("missing_artifact"),
                ],
            };
            route.reason = route.reason_codes.join(",");
            route.note = route.reason.clone();
        }

        Ok(route)
    }

    pub fn phase(&self) -> Result<WorkflowPhase, DiagnosticError> {
        let route = self.status()?;
        let session_entry = read_session_entry(&self.state_dir);
        let phase = if route.status == "implementation_ready" {
            String::from("execution_preflight")
        } else if route.status == "stale_plan" {
            String::from("plan_writing")
        } else {
            route.status.clone()
        };
        let next_action = if route.status == "implementation_ready" {
            String::from("execution_preflight")
        } else {
            String::from("use_next_skill")
        };

        Ok(WorkflowPhase {
            phase,
            route_status: route.status.clone(),
            next_skill: route.next_skill.clone(),
            next_action,
            spec_path: route.spec_path.clone(),
            plan_path: route.plan_path.clone(),
            session_entry,
            route,
        })
    }
}

fn resolve_route(
    runtime: &WorkflowRuntime,
    read_only: bool,
) -> Result<WorkflowRoute, DiagnosticError> {
    let mut spec_candidates = scan_specs(&runtime.identity.repo_root);
    spec_candidates.sort_by(|left, right| left.path.cmp(&right.path));

    let plan_candidates = scan_plans(&runtime.identity.repo_root);
    let manifest_path = runtime.manifest_path.display().to_string();
    let root = runtime.identity.repo_root.to_string_lossy().into_owned();

    if let Some(manifest) = &runtime.manifest {
        if !manifest.expected_spec_path.is_empty()
            && !runtime
                .identity
                .repo_root
                .join(&manifest.expected_spec_path)
                .is_file()
        {
            return Ok(WorkflowRoute {
                schema_version: 2,
                status: String::from("needs_brainstorming"),
                next_skill: String::from("superpowers:brainstorming"),
                spec_path: manifest.expected_spec_path.clone(),
                plan_path: String::new(),
                contract_state: String::from("unknown"),
                reason_codes: vec![String::from("missing_expected_spec")],
                diagnostics: Vec::new(),
                scan_truncated: false,
                spec_candidate_count: 0,
                plan_candidate_count: 0,
                manifest_path,
                root,
                reason: String::from("missing_expected_spec"),
                note: String::from("missing_expected_spec"),
            });
        }
    }

    if spec_candidates.is_empty() {
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("needs_brainstorming"),
            next_skill: String::from("superpowers:brainstorming"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: plan_candidates.len(),
            manifest_path,
            root,
            reason: String::new(),
            note: String::new(),
        });
    }

    let selected_spec = spec_candidates
        .last()
        .expect("non-empty candidate list should have a last entry");

    if spec_candidates.len() > 1 {
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("spec_draft"),
            next_skill: String::from("superpowers:plan-ceo-review"),
            spec_path: selected_spec.path.clone(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: vec![String::from("ambiguous_spec_candidates")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("ambiguous_spec_candidates"),
                severity: String::from("error"),
                artifact: selected_spec.path.clone(),
                message: String::from(
                    "More than one current spec candidate matches the fallback scan window.",
                ),
                remediation: String::from("Reduce spec ambiguity before proceeding."),
            }],
            scan_truncated: false,
            spec_candidate_count: spec_candidates.len(),
            plan_candidate_count: plan_candidates.len(),
            manifest_path,
            root,
            reason: String::from("fallback_ambiguity_spec"),
            note: String::from("fallback_ambiguity_spec"),
        });
    }

    if selected_spec.workflow_state == "Draft" {
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("spec_draft"),
            next_skill: String::from("superpowers:plan-ceo-review"),
            spec_path: selected_spec.path.clone(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            scan_truncated: false,
            spec_candidate_count: 1,
            plan_candidate_count: plan_candidates.len(),
            manifest_path,
            root,
            reason: String::new(),
            note: String::new(),
        });
    }

    let approved_spec = selected_spec;
    let matching_plan = plan_candidates
        .iter()
        .find(|plan| plan.source_spec_path == approved_spec.path);

    if let Some(plan) = matching_plan {
        let report =
            analyze_full_contract(runtime.identity.repo_root.as_path(), approved_spec, plan);
        if plan.workflow_state == "Engineering Approved"
            && report
                .as_ref()
                .map_or(false, |report| report.contract_state == "valid")
        {
            if read_only {
                return Ok(resolve_route(runtime, false)?);
            }
            return Ok(WorkflowRoute {
                schema_version: 2,
                status: String::from("implementation_ready"),
                next_skill: String::new(),
                spec_path: approved_spec.path.clone(),
                plan_path: plan.path.clone(),
                contract_state: report.as_ref().map_or_else(
                    || String::from("unknown"),
                    |report| report.contract_state.clone(),
                ),
                reason_codes: vec![String::from("implementation_ready")],
                diagnostics: Vec::new(),
                scan_truncated: false,
                spec_candidate_count: 1,
                plan_candidate_count: 1,
                manifest_path,
                root,
                reason: String::from("implementation_ready"),
                note: String::from("implementation_ready"),
            });
        }
    }

    Ok(WorkflowRoute {
        schema_version: 2,
        status: String::from("spec_approved_needs_plan"),
        next_skill: String::from("superpowers:writing-plans"),
        spec_path: approved_spec.path.clone(),
        plan_path: String::new(),
        contract_state: String::from("unknown"),
        reason_codes: Vec::new(),
        diagnostics: Vec::new(),
        scan_truncated: false,
        spec_candidate_count: 1,
        plan_candidate_count: plan_candidates.len(),
        manifest_path,
        root,
        reason: String::new(),
        note: String::new(),
    })
}

fn normalize_repo_path(path: &Path) -> Result<String, DiagnosticError> {
    let raw = path.to_str().ok_or_else(|| {
        DiagnosticError::new(
            FailureClass::InvalidRepoPath,
            "Workflow paths must be valid utf-8 repo-relative paths.",
        )
    })?;
    RepoPath::parse(raw).map(|path| path.as_str().to_owned())
}

fn scan_specs(repo_root: &Path) -> Vec<WorkflowSpecCandidate> {
    let mut candidates = Vec::new();
    for path in markdown_files_under(&repo_root.join("docs/superpowers/specs")) {
        if let Ok(document) = parse_workflow_spec_candidate(&path) {
            candidates.push(document);
        }
    }
    candidates
}

fn scan_plans(repo_root: &Path) -> Vec<WorkflowPlanCandidate> {
    let mut candidates = Vec::new();
    for path in markdown_files_under(&repo_root.join("docs/superpowers/plans")) {
        if let Ok(document) = parse_workflow_plan_candidate(&path) {
            candidates.push(document);
        }
    }
    candidates
}

fn markdown_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_markdown_files(root, &mut files);
    files
}

fn visit_markdown_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_markdown_files(&path, files);
        } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("md") {
            files.push(path);
        }
    }
}

fn read_session_entry(state_dir: &Path) -> SessionEntryState {
    let session_key = env::var("SUPERPOWERS_SESSION_KEY")
        .or_else(|_| env::var("PPID"))
        .unwrap_or_else(|_| String::from("current"));
    let decision_path = state_dir
        .join("session-flags")
        .join("using-superpowers")
        .join(&session_key);

    if decision_path.is_file() {
        SessionEntryState {
            outcome: String::from("enabled"),
            decision_source: String::from("existing_enabled"),
            session_key,
            decision_path: decision_path.display().to_string(),
            policy_source: String::from("default"),
            persisted: true,
            failure_class: String::new(),
            reason: String::from("existing_enabled"),
        }
    } else {
        SessionEntryState {
            outcome: String::from("needs_user_choice"),
            decision_source: String::from("missing"),
            session_key,
            decision_path: decision_path.display().to_string(),
            policy_source: String::from("default"),
            persisted: false,
            failure_class: String::new(),
            reason: String::from("missing"),
        }
    }
}

fn state_dir() -> PathBuf {
    env::var_os("SUPERPOWERS_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".superpowers")))
        .unwrap_or_else(|| PathBuf::from(".superpowers"))
}

pub fn sync_reason_codes(route: &WorkflowRoute) -> Vec<String> {
    route.reason_codes.clone()
}

pub fn report_contract_state(report: &AnalyzePlanReport) -> &str {
    &report.contract_state
}

pub fn write_workflow_schemas(output_dir: impl AsRef<Path>) -> Result<(), DiagnosticError> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not create workflow schema directory {}: {err}",
                output_dir.display()
            ),
        )
    })?;

    let status_schema =
        serde_json::to_string_pretty(&schema_for!(WorkflowRoute)).map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Could not serialize workflow status schema: {err}"),
            )
        })?;
    let resolve_schema =
        serde_json::to_string_pretty(&schema_for!(WorkflowRoute)).map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Could not serialize workflow resolve schema: {err}"),
            )
        })?;

    fs::write(
        output_dir.join("workflow-status.schema.json"),
        status_schema,
    )
    .map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write workflow-status schema: {err}"),
        )
    })?;
    fs::write(
        output_dir.join("workflow-resolve.schema.json"),
        resolve_schema,
    )
    .map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write workflow-resolve schema: {err}"),
        )
    })?;

    Ok(())
}

fn parse_workflow_spec_candidate(path: &Path) -> Result<WorkflowSpecCandidate, DiagnosticError> {
    let source = fs::read_to_string(path).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not read spec candidate {}: {err}", path.display()),
        )
    })?;
    let workflow_state = parse_header_value(&source, "Workflow State")?;
    Ok(WorkflowSpecCandidate {
        path: repo_relative_path(path),
        workflow_state,
    })
}

fn parse_workflow_plan_candidate(path: &Path) -> Result<WorkflowPlanCandidate, DiagnosticError> {
    let source = fs::read_to_string(path).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not read plan candidate {}: {err}", path.display()),
        )
    })?;
    let workflow_state = parse_header_value(&source, "Workflow State")?;
    let source_spec_path = parse_header_value(&source, "Source Spec")?
        .trim_matches('`')
        .to_owned();
    Ok(WorkflowPlanCandidate {
        path: repo_relative_path(path),
        workflow_state,
        source_spec_path,
    })
}

fn analyze_full_contract(
    repo_root: &Path,
    spec: &WorkflowSpecCandidate,
    plan: &WorkflowPlanCandidate,
) -> Option<AnalyzePlanReport> {
    let spec_path = repo_root.join(&spec.path);
    let plan_path = repo_root.join(&plan.path);
    let strict_spec = parse_spec_file(spec_path).ok()?;
    let strict_plan = parse_plan_file(plan_path).ok()?;
    Some(analyze_documents(&strict_spec, &strict_plan))
}

fn parse_header_value(source: &str, header: &str) -> Result<String, DiagnosticError> {
    let prefix = format!("**{header}:** ");
    source
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Missing or malformed {header} header."),
            )
        })
}

fn repo_relative_path(path: &Path) -> String {
    let normalized = path.display().to_string().replace('\\', "/");
    if let Some((_, suffix)) = normalized.split_once("/docs/") {
        return format!("docs/{suffix}");
    }
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_owned()
}
