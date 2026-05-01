use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use schemars::schema_for;
use serde::Serialize;

use crate::contracts::plan::{
    AnalyzePlanReport, PlanFidelityReviewReport, evaluate_plan_fidelity_review, parse_plan_file,
};
use crate::contracts::runtime::analyze_contract_report;
use crate::contracts::spec::{SpecDocument, parse_spec_file, repo_relative_string};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::execution::phase::{
    PUBLIC_STATUS_PHASE_VALUES, RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS,
};
use crate::git::{RepositoryIdentity, discover_repo_identity, stored_repo_root_matches_current};
use crate::paths::{RepoPath, featureforge_state_dir};
use crate::workflow::manifest::{
    ManifestLoadResult, WorkflowManifest, load_manifest, load_manifest_read_only, manifest_path,
    recover_slug_changed_manifest, recover_slug_changed_manifest_read_only, save_manifest,
};
use crate::workflow::markdown_scan::markdown_files_under;
use crate::workflow::operator::{WorkflowHandoff, WorkflowOperator};

const ACTIVE_SPEC_ROOT: &str = "docs/featureforge/specs";
const ACTIVE_PLAN_ROOT: &str = "docs/featureforge/plans";
const ACTIVE_IMPLEMENTATION_TARGET_INDEX: &str =
    "docs/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md";
const WORKFLOW_ROUTE_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_HANDOFF_SCHEMA_VERSION: u32 = 3;
const WORKFLOW_OPERATOR_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy)]
pub enum ArtifactKind {
    Spec,
    Plan,
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_fidelity_review: Option<PlanFidelityReviewReport>,
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
    pub schema_version: u32,
    pub phase: String,
    pub route_status: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub next_skill: String,
    pub next_step: String,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason_family: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_reason_codes: Vec<String>,
    pub spec_path: String,
    pub plan_path: String,
    pub route: WorkflowRoute,
}

#[derive(Debug, Clone)]
pub struct WorkflowRuntime {
    pub identity: RepositoryIdentity,
    pub state_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: Option<WorkflowManifest>,
    pub manifest_warning: Option<String>,
    pub manifest_recovery_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
struct WorkflowSpecCandidate {
    path: String,
    workflow_state: String,
    spec_revision: u32,
    malformed_headers: bool,
}

#[derive(Debug, Clone)]
struct WorkflowPlanCandidate {
    path: String,
    workflow_state: String,
    last_reviewed_by: String,
    source_spec_path: String,
    source_spec_revision: Option<u32>,
    malformed_headers: bool,
}

impl WorkflowRuntime {
    pub fn discover(current_dir: &Path) -> Result<Self, DiagnosticError> {
        Self::discover_with_loader(current_dir, false)
    }

    pub fn discover_for_state_dir(
        current_dir: &Path,
        state_dir: &Path,
    ) -> Result<Self, DiagnosticError> {
        Self::discover_with_loader_and_state_dir(current_dir, state_dir.to_path_buf(), false)
    }

    pub fn discover_read_only(current_dir: &Path) -> Result<Self, DiagnosticError> {
        Self::discover_with_loader(current_dir, true)
    }

    pub fn discover_read_only_for_state_dir(
        current_dir: &Path,
        state_dir: &Path,
    ) -> Result<Self, DiagnosticError> {
        Self::discover_with_loader_and_state_dir(current_dir, state_dir.to_path_buf(), true)
    }

    fn discover_with_loader(current_dir: &Path, read_only: bool) -> Result<Self, DiagnosticError> {
        Self::discover_with_loader_and_state_dir(current_dir, featureforge_state_dir(), read_only)
    }

    fn discover_with_loader_and_state_dir(
        current_dir: &Path,
        state_dir: PathBuf,
        read_only: bool,
    ) -> Result<Self, DiagnosticError> {
        let identity = discover_repo_identity(current_dir)?;
        let manifest_path = manifest_path(&identity, &state_dir);
        let load = if read_only {
            load_manifest_read_only
        } else {
            load_manifest
        };
        let (manifest, manifest_warning, manifest_recovery_reasons) = match load(&manifest_path) {
            ManifestLoadResult::Missing => {
                let recovered_manifest = if read_only {
                    recover_slug_changed_manifest_read_only(&identity, &state_dir, &manifest_path)
                } else {
                    recover_slug_changed_manifest(&identity, &state_dir, &manifest_path)
                };
                if let Some(manifest) = recovered_manifest {
                    (
                        Some(manifest),
                        None,
                        vec![String::from("repo_slug_recovered")],
                    )
                } else {
                    (None, None, Vec::new())
                }
            }
            ManifestLoadResult::Loaded(manifest) => {
                let mut reasons = Vec::new();
                if !stored_repo_root_matches_current(&manifest.repo_root, &identity.repo_root) {
                    reasons.push(String::from("repo_root_mismatch"));
                }
                if manifest.branch != identity.branch_name {
                    reasons.push(String::from("branch_mismatch"));
                }
                (Some(manifest), None, reasons)
            }
            ManifestLoadResult::Corrupt { backup_path } => {
                if read_only {
                    (None, None, vec![String::from("corrupt_manifest_present")])
                } else {
                    (
                        None,
                        Some(format!(
                            "warning: corrupt manifest rescued to {}",
                            backup_path.display()
                        )),
                        Vec::new(),
                    )
                }
            }
        };
        Ok(Self {
            identity,
            state_dir,
            manifest_path,
            manifest,
            manifest_warning,
            manifest_recovery_reasons,
        })
    }

    pub fn status(&self) -> Result<WorkflowRoute, DiagnosticError> {
        resolve_route(self, false, false)
            .map(|route| normalize_workflow_route(self.decorate_route_with_manifest_context(route)))
    }

    pub fn status_refresh(&mut self) -> Result<WorkflowRoute, DiagnosticError> {
        let route = normalize_workflow_route(
            self.decorate_route_with_manifest_context(resolve_route(self, false, true)?),
        );
        let expected_spec_path = route.spec_path.clone();
        let expected_plan_path = route.plan_path.clone();

        let manifest = WorkflowManifest {
            version: 1,
            repo_root: self.identity.repo_root.to_string_lossy().into_owned(),
            branch: self.identity.branch_name.clone(),
            expected_spec_path,
            expected_plan_path,
            status: route.status.clone(),
            next_skill: route.next_skill.clone(),
            reason: route.reason.clone(),
            note: route.note.clone(),
            updated_at: String::from("1970-01-01T00:00:00Z"),
        };
        if let Err(route) = self.persist_manifest_with_retry(manifest.clone(), &route) {
            return Ok(*route);
        }
        self.manifest = Some(manifest);
        self.manifest_warning = None;
        self.manifest_recovery_reasons.clear();
        Ok(route)
    }

    pub fn resolve(&self) -> Result<WorkflowRoute, DiagnosticError> {
        match env::var("FEATUREFORGE_WORKFLOW_RESOLVE_TEST_FAILPOINT").as_deref() {
            Ok("invalid_contract") => {
                return Err(DiagnosticError::new(
                    FailureClass::ResolverContractViolation,
                    "Resolver contract violation injected by test failpoint.",
                ));
            }
            Ok("runtime_failure") => {
                return Err(DiagnosticError::new(
                    FailureClass::ResolverRuntimeFailure,
                    "Resolver runtime failure injected by test failpoint.",
                ));
            }
            _ => {}
        }
        resolve_route(self, true, false)
            .map(|route| normalize_workflow_route(self.decorate_route_with_manifest_context(route)))
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
            next_skill: String::from("featureforge:brainstorming"),
            reason: String::new(),
            note: String::new(),
            updated_at: String::from("1970-01-01T00:00:00Z"),
        });
        match artifact {
            ArtifactKind::Spec => {
                manifest.expected_spec_path = repo_path.clone();
                manifest.expected_plan_path.clear();
                manifest.status = String::from("needs_brainstorming");
                manifest.next_skill = String::from("featureforge:brainstorming");
                manifest.reason = String::from("missing_expected_spec,expect_set");
                manifest.note = manifest.reason.clone();
            }
            ArtifactKind::Plan => {
                manifest.expected_plan_path = repo_path.clone();
                manifest.status = String::from("plan_draft");
                manifest.next_skill = String::from("featureforge:writing-plans");
                manifest.reason = String::from("missing_expected_plan,expect_set");
                manifest.note = manifest.reason.clone();
            }
        }
        let mut preview = self.clone();
        preview.manifest = Some(manifest.clone());
        let route = preview.status()?;
        if let Err(route) = self.persist_manifest_with_retry(manifest.clone(), &route) {
            return Ok(*route);
        }
        self.manifest = Some(manifest);
        Ok(route)
    }

    pub fn sync(
        &mut self,
        artifact: ArtifactKind,
        path: Option<&Path>,
    ) -> Result<WorkflowRoute, DiagnosticError> {
        let repo_path = if let Some(path) = path {
            normalize_repo_path(path)?
        } else {
            self.matching_manifest()
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

        let mut manifest = self.manifest.clone().unwrap_or_else(|| WorkflowManifest {
            version: 1,
            repo_root: self.identity.repo_root.to_string_lossy().into_owned(),
            branch: self.identity.branch_name.clone(),
            expected_spec_path: String::new(),
            expected_plan_path: String::new(),
            status: String::new(),
            next_skill: String::new(),
            reason: String::new(),
            note: String::new(),
            updated_at: String::from("1970-01-01T00:00:00Z"),
        });
        match artifact {
            ArtifactKind::Spec => manifest.expected_spec_path = repo_path.clone(),
            ArtifactKind::Plan => manifest.expected_plan_path = repo_path.clone(),
        }
        let mut preview = self.clone();
        preview.manifest = Some(manifest.clone());
        let mut route = preview.status()?;
        if !self.identity.repo_root.join(&repo_path).is_file() {
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
        if let Err(route) = self.persist_manifest_with_retry(manifest.clone(), &route) {
            return Ok(*route);
        }
        self.manifest = Some(manifest);

        Ok(route)
    }
}

fn normalize_workflow_route(mut route: WorkflowRoute) -> WorkflowRoute {
    route.schema_version = WORKFLOW_ROUTE_SCHEMA_VERSION;
    route
}

impl WorkflowRuntime {
    fn matching_manifest(&self) -> Option<&WorkflowManifest> {
        self.manifest.as_ref().filter(|manifest| {
            stored_repo_root_matches_current(&manifest.repo_root, &self.identity.repo_root)
                && manifest.branch == self.identity.branch_name
        })
    }

    fn decorate_route_with_manifest_context(&self, mut route: WorkflowRoute) -> WorkflowRoute {
        if let Some(warning) = &self.manifest_warning {
            if !route
                .reason_codes
                .iter()
                .any(|existing| existing == "corrupt_manifest_rescued")
            {
                route
                    .reason_codes
                    .push(String::from("corrupt_manifest_rescued"));
            }
            route.note = warning.clone();
            route.reason = warning.clone();
        }
        for reason_code in &self.manifest_recovery_reasons {
            if !route
                .reason_codes
                .iter()
                .any(|existing| existing == reason_code)
            {
                route.reason_codes.push(reason_code.clone());
            }
        }
        if !self.manifest_recovery_reasons.is_empty() {
            let recovery_reason = self.manifest_recovery_reasons.join(",");
            if route.reason.is_empty() {
                route.reason = recovery_reason.clone();
                route.note = recovery_reason;
            } else if !route.reason.contains(&recovery_reason) {
                route.reason = format!("{recovery_reason},{}", route.reason);
                route.note = route.reason.clone();
            }
        }
        route
    }

    fn persist_manifest_with_retry(
        &mut self,
        manifest: WorkflowManifest,
        route: &WorkflowRoute,
    ) -> Result<(), Box<WorkflowRoute>> {
        if save_manifest(&self.manifest_path, &manifest).is_ok() {
            return Ok(());
        }
        if save_manifest(&self.manifest_path, &manifest).is_ok() {
            return Ok(());
        }
        Err(Box::new(self.manifest_write_conflict_route(route)))
    }

    fn manifest_write_conflict_route(&self, route: &WorkflowRoute) -> WorkflowRoute {
        let mut degraded = route.clone();
        if !degraded
            .reason_codes
            .iter()
            .any(|code| code == "manifest_write_conflict")
        {
            degraded
                .reason_codes
                .push(String::from("manifest_write_conflict"));
        }
        degraded.diagnostics.push(WorkflowDiagnostic {
            code: String::from("manifest_write_conflict"),
            severity: String::from("error"),
            artifact: self.manifest_path.display().to_string(),
            message: String::from(
                "Could not persist the workflow manifest after one retry attempt.",
            ),
            remediation: String::from(
                "Restore write access to the workflow manifest directory and retry.",
            ),
        });
        degraded.note = String::from("warning: manifest_write_conflict (retrying once)");
        degraded
    }
}

fn resolve_route(
    runtime: &WorkflowRuntime,
    read_only: bool,
    refresh: bool,
) -> Result<WorkflowRoute, DiagnosticError> {
    let manifest_path = runtime.manifest_path.display().to_string();
    let root = runtime.identity.repo_root.to_string_lossy().into_owned();

    let (mut spec_candidates, mut malformed_spec_candidates) =
        scan_specs(&runtime.identity.repo_root);
    spec_candidates.sort_by(|left, right| left.path.cmp(&right.path));
    malformed_spec_candidates.sort_by(|left, right| left.path.cmp(&right.path));
    let spec_candidate_count = spec_candidates.len();
    let (spec_candidates, scan_truncated) = apply_fallback_limit(spec_candidates);

    let plan_candidates = scan_plans(&runtime.identity.repo_root);

    if let Some(manifest) = runtime.matching_manifest()
        && !manifest.expected_spec_path.is_empty()
        && !runtime
            .identity
            .repo_root
            .join(&manifest.expected_spec_path)
            .is_file()
        && !(refresh && spec_candidates.len() == 1)
    {
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("needs_brainstorming"),
            next_skill: String::from("featureforge:brainstorming"),
            spec_path: manifest.expected_spec_path.clone(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: vec![String::from("missing_expected_spec")],
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path,
            root,
            reason: String::from("missing_expected_spec"),
            note: String::from("missing_expected_spec"),
        });
    }

    if spec_candidates.is_empty() && !malformed_spec_candidates.is_empty() {
        let selected_spec = malformed_spec_candidates
            .last()
            .expect("non-empty malformed spec list should have a last entry");
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("spec_draft"),
            next_skill: String::from("featureforge:plan-ceo-review"),
            spec_path: selected_spec.path.clone(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: vec![String::from("malformed_spec_headers")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("malformed_spec_headers"),
                severity: String::from("error"),
                artifact: selected_spec.path.clone(),
                message: String::from(
                    "Spec headers are missing required Workflow State, Spec Revision, or Last Reviewed By fields.",
                ),
                remediation: String::from(
                    "Repair the spec headers before treating the document as an approved workflow artifact.",
                ),
            }],
            plan_fidelity_review: None,
            scan_truncated,
            spec_candidate_count: malformed_spec_candidates.len(),
            plan_candidate_count: plan_candidates.len(),
            manifest_path,
            root,
            reason: String::from("malformed_spec_headers"),
            note: String::from("malformed_spec_headers"),
        });
    }

    if spec_candidates.is_empty() {
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("needs_brainstorming"),
            next_skill: String::from("featureforge:brainstorming"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated,
            spec_candidate_count: 0,
            plan_candidate_count: plan_candidates.len(),
            manifest_path,
            root,
            reason: String::new(),
            note: String::new(),
        });
    }

    let manifest_selected_spec = runtime.matching_manifest().and_then(|manifest| {
        if manifest.expected_spec_path.is_empty() {
            return None;
        }
        let path = runtime
            .identity
            .repo_root
            .join(&manifest.expected_spec_path);
        if !path.is_file() {
            return None;
        }
        parse_workflow_spec_candidate(&path).ok()
    });
    let manifest_selected_spec_present = manifest_selected_spec.is_some();
    let selected_spec = manifest_selected_spec.unwrap_or_else(|| {
        spec_candidates
            .last()
            .cloned()
            .expect("non-empty candidate list should have a last entry")
    });

    if !manifest_selected_spec_present && spec_candidates.len() > 1 {
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("spec_draft"),
            next_skill: String::from("featureforge:plan-ceo-review"),
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
            plan_fidelity_review: None,
            scan_truncated,
            spec_candidate_count,
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
            next_skill: String::from("featureforge:plan-ceo-review"),
            spec_path: selected_spec.path.clone(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated,
            spec_candidate_count,
            plan_candidate_count: plan_candidates.len(),
            manifest_path,
            root,
            reason: String::new(),
            note: String::new(),
        });
    }

    let approved_spec = selected_spec;
    let manifest_selected_plan = runtime
        .matching_manifest()
        .and_then(|manifest| {
            if manifest.expected_plan_path.is_empty() {
                return None;
            }
            let path = runtime
                .identity
                .repo_root
                .join(&manifest.expected_plan_path);
            if !path.is_file() {
                return None;
            }
            parse_workflow_plan_candidate(&path).ok()
        })
        .filter(|plan| {
            runtime
                .matching_manifest()
                .as_ref()
                .is_some_and(|manifest| plan.path == manifest.expected_plan_path)
        });
    let exact_matching_plans = plan_candidates
        .iter()
        .filter(|plan| plan.source_spec_path == approved_spec.path)
        .collect::<Vec<_>>();
    let ambiguous_plan_candidate_count = if manifest_selected_plan.is_some() {
        0
    } else if exact_matching_plans.len() > 1 {
        exact_matching_plans.len()
    } else if exact_matching_plans.is_empty() && plan_candidates.len() > 1 {
        plan_candidates.len()
    } else {
        0
    };
    if ambiguous_plan_candidate_count > 1 {
        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("spec_approved_needs_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            spec_path: approved_spec.path.clone(),
            plan_path: String::new(),
            contract_state: String::from("unknown"),
            reason_codes: vec![String::from("ambiguous_plan_candidates")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("ambiguous_plan_candidates"),
                severity: String::from("error"),
                artifact: approved_spec.path.clone(),
                message: String::from(
                    "More than one plan candidate matches the current approved spec.",
                ),
                remediation: String::from(
                    "Reduce plan ambiguity before treating the approved spec as ready for execution.",
                ),
            }],
            plan_fidelity_review: None,
            scan_truncated,
            spec_candidate_count,
            plan_candidate_count: ambiguous_plan_candidate_count,
            manifest_path,
            root,
            reason: String::from("ambiguous_plan_candidates"),
            note: String::from("ambiguous_plan_candidates"),
        });
    }
    let matching_plan = manifest_selected_plan
        .or_else(|| exact_matching_plans.first().copied().cloned())
        .or_else(|| {
            if plan_candidates.len() == 1 {
                plan_candidates.first().cloned()
            } else {
                None
            }
        });
    let preserved_plan_path = runtime
        .matching_manifest()
        .as_ref()
        .map(|manifest| manifest.expected_plan_path.clone())
        .filter(|path| !path.is_empty());
    let missing_expected_plan = preserved_plan_path
        .as_ref()
        .is_some_and(|path| !runtime.identity.repo_root.join(path).is_file());

    if let Some(plan) = matching_plan {
        let stale_source_spec_linkage = plan.source_spec_path != approved_spec.path
            || plan
                .source_spec_revision
                .is_some_and(|revision| revision != approved_spec.spec_revision);
        let report =
            analyze_full_contract(runtime.identity.repo_root.as_path(), &approved_spec, &plan);
        let packet_buildability_failure = report
            .as_ref()
            .is_some_and(needs_packet_buildability_failure);
        let contract_state = workflow_contract_state(
            report.as_ref(),
            stale_source_spec_linkage,
            packet_buildability_failure,
        );
        let reason_codes = workflow_reason_codes(
            report.as_ref(),
            stale_source_spec_linkage,
            packet_buildability_failure,
        );
        let diagnostics = workflow_diagnostics(
            &plan,
            &approved_spec,
            report.as_ref(),
            stale_source_spec_linkage,
            packet_buildability_failure,
        );
        let reason = compatibility_reason(&reason_codes);
        if plan.workflow_state == "Draft" {
            let plan_fidelity_gate =
                evaluate_plan_fidelity_gate(runtime, &approved_spec.path, &plan.path);
            let plan_needs_authoring = draft_plan_needs_authoring(
                &plan,
                report.as_ref(),
                stale_source_spec_linkage,
                packet_buildability_failure,
                &reason_codes,
            );
            if plan_needs_authoring {
                let next_skill = "featureforge:writing-plans";
                return Ok(WorkflowRoute {
                    schema_version: 2,
                    status: String::from("plan_draft"),
                    next_skill: String::from(next_skill),
                    spec_path: approved_spec.path.clone(),
                    plan_path: plan.path.clone(),
                    contract_state,
                    reason_codes,
                    diagnostics,
                    plan_fidelity_review: fidelity_review_visible_for_route(
                        &plan,
                        next_skill,
                        &plan_fidelity_gate,
                    )
                    .then_some(plan_fidelity_gate),
                    scan_truncated,
                    spec_candidate_count,
                    plan_candidate_count: 1,
                    manifest_path,
                    root,
                    reason: reason.clone(),
                    note: reason,
                });
            }
            if draft_ready_for_fidelity_review(&plan, &plan_fidelity_gate, plan_needs_authoring) {
                let next_skill = "featureforge:plan-fidelity-review";
                let combined_reason_codes =
                    combine_plan_and_fidelity_reason_codes(&reason_codes, &plan_fidelity_gate);
                let combined_diagnostics = combine_plan_and_fidelity_diagnostics(
                    &plan,
                    &diagnostics,
                    &plan_fidelity_gate,
                    next_skill,
                );
                let reason = compatibility_reason(&combined_reason_codes);
                return Ok(WorkflowRoute {
                    schema_version: 2,
                    status: String::from("plan_draft"),
                    next_skill: String::from(next_skill),
                    spec_path: approved_spec.path.clone(),
                    plan_path: plan.path.clone(),
                    contract_state,
                    reason_codes: combined_reason_codes,
                    diagnostics: combined_diagnostics,
                    plan_fidelity_review: fidelity_review_visible_for_route(
                        &plan,
                        next_skill,
                        &plan_fidelity_gate,
                    )
                    .then_some(plan_fidelity_gate),
                    scan_truncated,
                    spec_candidate_count,
                    plan_candidate_count: 1,
                    manifest_path,
                    root,
                    reason: reason.clone(),
                    note: reason,
                });
            }
            debug_assert!(
                draft_ready_for_engineering_review(&plan, plan_needs_authoring)
                    || draft_ready_for_engineering_approval(
                        &plan,
                        &plan_fidelity_gate,
                        plan_needs_authoring,
                    )
            );
            let (reason_codes, diagnostics, reason) = if plan_fidelity_gate.state == "fail"
                && draft_ready_for_engineering_approval(
                    &plan,
                    &plan_fidelity_gate,
                    plan_needs_authoring,
                ) {
                let combined_reason_codes =
                    combine_plan_and_fidelity_reason_codes(&reason_codes, &plan_fidelity_gate);
                let combined_diagnostics = combine_plan_and_fidelity_diagnostics(
                    &plan,
                    &diagnostics,
                    &plan_fidelity_gate,
                    "featureforge:plan-eng-review",
                );
                let reason = compatibility_reason(&combined_reason_codes);
                (combined_reason_codes, combined_diagnostics, reason)
            } else {
                (reason_codes, diagnostics, reason)
            };
            let next_skill = "featureforge:plan-eng-review";
            return Ok(WorkflowRoute {
                schema_version: 2,
                status: String::from("plan_draft"),
                next_skill: String::from(next_skill),
                spec_path: approved_spec.path.clone(),
                plan_path: plan.path.clone(),
                contract_state,
                reason_codes,
                diagnostics,
                plan_fidelity_review: fidelity_review_visible_for_route(
                    &plan,
                    next_skill,
                    &plan_fidelity_gate,
                )
                .then_some(plan_fidelity_gate),
                scan_truncated,
                spec_candidate_count,
                plan_candidate_count: 1,
                manifest_path,
                root,
                reason: reason.clone(),
                note: reason,
            });
        }

        if !stale_source_spec_linkage
            && !packet_buildability_failure
            && plan.workflow_state == "Engineering Approved"
            && report
                .as_ref()
                .is_some_and(|report| report.contract_state == "valid")
        {
            let implementation_fidelity_gate =
                evaluate_plan_fidelity_gate(runtime, &approved_spec.path, &plan.path);
            if plan_fidelity_allows_implementation(&implementation_fidelity_gate) {
                if read_only {
                    return resolve_route(runtime, false, false);
                }
                return Ok(WorkflowRoute {
                    schema_version: 2,
                    status: String::from(
                        crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY,
                    ),
                    next_skill: String::new(),
                    spec_path: approved_spec.path.clone(),
                    plan_path: plan.path.clone(),
                    contract_state,
                    reason_codes: vec![String::from(
                        crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY,
                    )],
                    diagnostics: Vec::new(),
                    plan_fidelity_review: Some(implementation_fidelity_gate),
                    scan_truncated,
                    spec_candidate_count,
                    plan_candidate_count: 1,
                    manifest_path,
                    root,
                    reason: String::from(
                        crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY,
                    ),
                    note: String::from(
                        crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY,
                    ),
                });
            }

            let next_skill = "featureforge:plan-eng-review";
            let reason_codes =
                engineering_approval_fidelity_reason_codes(&implementation_fidelity_gate);
            let diagnostics = engineering_approval_fidelity_diagnostics(
                &plan,
                &implementation_fidelity_gate,
                next_skill,
            );
            let reason = compatibility_reason(&reason_codes);
            return Ok(WorkflowRoute {
                schema_version: 2,
                status: String::from("plan_review_required"),
                next_skill: String::from(next_skill),
                spec_path: approved_spec.path.clone(),
                plan_path: plan.path.clone(),
                contract_state,
                reason_codes,
                diagnostics,
                plan_fidelity_review: Some(implementation_fidelity_gate),
                scan_truncated,
                spec_candidate_count,
                plan_candidate_count: 1,
                manifest_path,
                root,
                reason: reason.clone(),
                note: reason,
            });
        }

        if plan.workflow_state == "Engineering Approved" && contract_state == "stale" {
            return Ok(WorkflowRoute {
                schema_version: 2,
                status: String::from("stale_plan"),
                next_skill: String::from("featureforge:writing-plans"),
                spec_path: approved_spec.path.clone(),
                plan_path: plan.path.clone(),
                contract_state,
                reason_codes,
                diagnostics,
                plan_fidelity_review: None,
                scan_truncated,
                spec_candidate_count,
                plan_candidate_count: 1,
                manifest_path,
                root,
                reason: reason.clone(),
                note: reason,
            });
        }

        return Ok(WorkflowRoute {
            schema_version: 2,
            status: String::from("plan_draft"),
            next_skill: String::from("featureforge:plan-eng-review"),
            spec_path: approved_spec.path.clone(),
            plan_path: plan.path.clone(),
            contract_state,
            reason_codes,
            diagnostics,
            plan_fidelity_review: None,
            scan_truncated,
            spec_candidate_count,
            plan_candidate_count: 1,
            manifest_path,
            root,
            reason: reason.clone(),
            note: reason,
        });
    }

    Ok(WorkflowRoute {
        schema_version: 2,
        status: String::from("spec_approved_needs_plan"),
        next_skill: String::from("featureforge:writing-plans"),
        spec_path: approved_spec.path.clone(),
        plan_path: preserved_plan_path.unwrap_or_default(),
        contract_state: String::from("unknown"),
        reason_codes: if missing_expected_plan {
            vec![String::from("missing_expected_plan")]
        } else {
            Vec::new()
        },
        diagnostics: Vec::new(),
        plan_fidelity_review: None,
        scan_truncated,
        spec_candidate_count,
        plan_candidate_count: plan_candidates.len(),
        manifest_path,
        root,
        reason: if missing_expected_plan {
            String::from("missing_expected_plan")
        } else {
            String::new()
        },
        note: if missing_expected_plan {
            String::from("missing_expected_plan")
        } else {
            String::new()
        },
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

pub(crate) fn explicit_plan_override_route(
    workflow: &WorkflowRuntime,
    resolved_route: &WorkflowRoute,
    plan_override: &Path,
) -> Result<WorkflowRoute, DiagnosticError> {
    let decorate = |route: WorkflowRoute| workflow.decorate_route_with_manifest_context(route);
    let plan_path = normalize_repo_path(plan_override)?;
    let plan_abs = workflow.identity.repo_root.join(&plan_path);
    if !plan_abs.is_file() {
        return Err(DiagnosticError::new(
            FailureClass::InvalidCommandInput,
            "Workflow plan override file does not exist.",
        ));
    }

    let plan = parse_workflow_plan_candidate(&plan_abs)?;
    let approved_spec_abs = workflow.identity.repo_root.join(&plan.source_spec_path);
    let approved_spec_exists = approved_spec_abs.is_file();
    let approved_spec = if approved_spec_exists {
        parse_workflow_spec_candidate(&approved_spec_abs)?
    } else {
        WorkflowSpecCandidate {
            path: plan.source_spec_path.clone(),
            workflow_state: String::from("Draft"),
            spec_revision: 0,
            malformed_headers: true,
        }
    };
    let stale_source_spec_linkage = !approved_spec_exists
        || plan.source_spec_path != approved_spec.path
        || plan
            .source_spec_revision
            .is_some_and(|revision| revision != approved_spec.spec_revision);
    let report =
        analyze_full_contract(workflow.identity.repo_root.as_path(), &approved_spec, &plan);
    let packet_buildability_failure = report
        .as_ref()
        .is_some_and(needs_packet_buildability_failure);
    let contract_state = workflow_contract_state(
        report.as_ref(),
        stale_source_spec_linkage,
        packet_buildability_failure,
    );
    let reason_codes = workflow_reason_codes(
        report.as_ref(),
        stale_source_spec_linkage,
        packet_buildability_failure,
    );
    let diagnostics = workflow_diagnostics(
        &plan,
        &approved_spec,
        report.as_ref(),
        stale_source_spec_linkage,
        packet_buildability_failure,
    );
    let reason = compatibility_reason(&reason_codes);
    let base_route = WorkflowRoute {
        schema_version: WORKFLOW_ROUTE_SCHEMA_VERSION,
        status: String::new(),
        next_skill: String::new(),
        spec_path: approved_spec.path.clone(),
        plan_path: plan.path.clone(),
        contract_state,
        reason_codes: Vec::new(),
        diagnostics: Vec::new(),
        plan_fidelity_review: None,
        scan_truncated: resolved_route.scan_truncated,
        spec_candidate_count: resolved_route.spec_candidate_count,
        plan_candidate_count: 1,
        manifest_path: resolved_route.manifest_path.clone(),
        root: resolved_route.root.clone(),
        reason: String::new(),
        note: String::new(),
    };

    if plan.workflow_state == "Draft" {
        let plan_fidelity_gate =
            evaluate_plan_fidelity_gate(workflow, &approved_spec.path, &plan.path);
        let plan_needs_authoring = draft_plan_needs_authoring(
            &plan,
            report.as_ref(),
            stale_source_spec_linkage,
            packet_buildability_failure,
            &reason_codes,
        );
        if plan_needs_authoring {
            let next_skill = "featureforge:writing-plans";
            return Ok(decorate(WorkflowRoute {
                status: String::from("plan_draft"),
                next_skill: String::from(next_skill),
                reason_codes,
                diagnostics,
                plan_fidelity_review: fidelity_review_visible_for_route(
                    &plan,
                    next_skill,
                    &plan_fidelity_gate,
                )
                .then_some(plan_fidelity_gate),
                reason: reason.clone(),
                note: reason,
                ..base_route
            }));
        }
        if draft_ready_for_fidelity_review(&plan, &plan_fidelity_gate, plan_needs_authoring) {
            let next_skill = "featureforge:plan-fidelity-review";
            let combined_reason_codes =
                combine_plan_and_fidelity_reason_codes(&reason_codes, &plan_fidelity_gate);
            let combined_diagnostics = combine_plan_and_fidelity_diagnostics(
                &plan,
                &diagnostics,
                &plan_fidelity_gate,
                next_skill,
            );
            let reason = compatibility_reason(&combined_reason_codes);
            return Ok(decorate(WorkflowRoute {
                status: String::from("plan_draft"),
                next_skill: String::from(next_skill),
                reason_codes: combined_reason_codes,
                diagnostics: combined_diagnostics,
                plan_fidelity_review: fidelity_review_visible_for_route(
                    &plan,
                    next_skill,
                    &plan_fidelity_gate,
                )
                .then_some(plan_fidelity_gate),
                reason: reason.clone(),
                note: reason,
                ..base_route
            }));
        }
        debug_assert!(
            draft_ready_for_engineering_review(&plan, plan_needs_authoring)
                || draft_ready_for_engineering_approval(
                    &plan,
                    &plan_fidelity_gate,
                    plan_needs_authoring,
                )
        );
        let (reason_codes, diagnostics, reason) = if plan_fidelity_gate.state == "fail"
            && draft_ready_for_engineering_approval(
                &plan,
                &plan_fidelity_gate,
                plan_needs_authoring,
            ) {
            let combined_reason_codes =
                combine_plan_and_fidelity_reason_codes(&reason_codes, &plan_fidelity_gate);
            let combined_diagnostics = combine_plan_and_fidelity_diagnostics(
                &plan,
                &diagnostics,
                &plan_fidelity_gate,
                "featureforge:plan-eng-review",
            );
            let reason = compatibility_reason(&combined_reason_codes);
            (combined_reason_codes, combined_diagnostics, reason)
        } else {
            (reason_codes, diagnostics, reason)
        };
        let next_skill = "featureforge:plan-eng-review";
        return Ok(decorate(WorkflowRoute {
            status: String::from("plan_draft"),
            next_skill: String::from(next_skill),
            reason_codes,
            diagnostics,
            plan_fidelity_review: fidelity_review_visible_for_route(
                &plan,
                next_skill,
                &plan_fidelity_gate,
            )
            .then_some(plan_fidelity_gate),
            reason: reason.clone(),
            note: reason,
            ..base_route
        }));
    }

    if !stale_source_spec_linkage
        && !packet_buildability_failure
        && plan.workflow_state == "Engineering Approved"
        && report
            .as_ref()
            .is_some_and(|report| report.contract_state == "valid")
    {
        let implementation_fidelity_gate =
            evaluate_plan_fidelity_gate(workflow, &approved_spec.path, &plan.path);
        if plan_fidelity_allows_implementation(&implementation_fidelity_gate) {
            return Ok(decorate(WorkflowRoute {
                status: String::from(crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
                next_skill: String::new(),
                reason_codes: vec![String::from(
                    crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY,
                )],
                diagnostics: Vec::new(),
                plan_fidelity_review: Some(implementation_fidelity_gate),
                reason: String::from(crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
                note: String::from(crate::execution::phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
                ..base_route
            }));
        }

        let next_skill = "featureforge:plan-eng-review";
        let reason_codes =
            engineering_approval_fidelity_reason_codes(&implementation_fidelity_gate);
        let diagnostics = engineering_approval_fidelity_diagnostics(
            &plan,
            &implementation_fidelity_gate,
            next_skill,
        );
        let reason = compatibility_reason(&reason_codes);
        return Ok(decorate(WorkflowRoute {
            status: String::from("plan_review_required"),
            next_skill: String::from(next_skill),
            reason_codes,
            diagnostics,
            plan_fidelity_review: Some(implementation_fidelity_gate),
            reason: reason.clone(),
            note: reason,
            ..base_route
        }));
    }

    if plan.workflow_state == "Engineering Approved" && base_route.contract_state == "stale" {
        return Ok(decorate(WorkflowRoute {
            status: String::from("stale_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            reason_codes,
            diagnostics,
            reason: reason.clone(),
            note: reason,
            ..base_route
        }));
    }

    Ok(decorate(WorkflowRoute {
        status: String::from("plan_draft"),
        next_skill: String::from("featureforge:plan-eng-review"),
        reason_codes,
        diagnostics,
        reason: reason.clone(),
        note: reason,
        ..base_route
    }))
}

fn scan_specs(repo_root: &Path) -> (Vec<WorkflowSpecCandidate>, Vec<WorkflowSpecCandidate>) {
    let mut candidates = Vec::new();
    let mut malformed = Vec::new();
    let active_target_paths = active_implementation_target_spec_paths(repo_root)
        .unwrap_or_else(|| markdown_files_under(&repo_root.join(ACTIVE_SPEC_ROOT)));
    for path in active_target_paths {
        if let Ok(document) = parse_workflow_spec_candidate(&path) {
            if document.malformed_headers {
                malformed.push(document);
            } else {
                candidates.push(document);
            }
        }
    }
    (candidates, malformed)
}

fn active_implementation_target_spec_paths(repo_root: &Path) -> Option<Vec<PathBuf>> {
    let index_path = repo_root.join(ACTIVE_IMPLEMENTATION_TARGET_INDEX);
    let source = fs::read_to_string(index_path).ok()?;
    let mut in_normative_section = false;
    let mut paths = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "## Active Normative Specs" {
            in_normative_section = true;
            continue;
        }
        if in_normative_section && trimmed.starts_with("## ") {
            break;
        }
        if !in_normative_section {
            continue;
        }
        let Some(entry) = trimmed.strip_prefix("- ") else {
            continue;
        };
        let relative = entry.trim().trim_matches('`');
        if relative.is_empty() {
            continue;
        }
        let path = if relative.starts_with("docs/") {
            repo_root.join(relative)
        } else {
            repo_root.join(ACTIVE_SPEC_ROOT).join(relative)
        };
        if path.is_file() {
            paths.push(path);
        }
    }

    (!paths.is_empty()).then_some(paths)
}

fn scan_plans(repo_root: &Path) -> Vec<WorkflowPlanCandidate> {
    let mut candidates = Vec::new();
    for path in markdown_files_under(&repo_root.join(ACTIVE_PLAN_ROOT)) {
        if let Ok(document) = parse_workflow_plan_candidate(&path) {
            candidates.push(document);
        }
    }
    candidates
}

fn apply_fallback_limit<T>(mut candidates: Vec<T>) -> (Vec<T>, bool) {
    let Some(limit) = fallback_limit() else {
        return (candidates, false);
    };
    if candidates.len() <= limit {
        return (candidates, false);
    }
    let keep_from = candidates.len().saturating_sub(limit);
    (candidates.split_off(keep_from), true)
}

fn fallback_limit() -> Option<usize> {
    env::var("FEATUREFORGE_WORKFLOW_STATUS_FALLBACK_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|limit| *limit > 0)
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

    let status_schema = workflow_route_schema_json("workflow status")?;
    let handoff_schema = workflow_handoff_schema_json("workflow handoff")?;
    let operator_schema = workflow_operator_schema_json("workflow operator")?;

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
        output_dir.join("workflow-handoff.schema.json"),
        handoff_schema,
    )
    .map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write workflow-handoff schema: {err}"),
        )
    })?;
    fs::write(
        output_dir.join("workflow-operator.schema.json"),
        operator_schema,
    )
    .map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write workflow-operator schema: {err}"),
        )
    })?;

    Ok(())
}

fn workflow_route_schema_json(schema_label: &str) -> Result<String, DiagnosticError> {
    let mut schema = serde_json::to_value(schema_for!(WorkflowRoute)).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize {schema_label} schema: {err}"),
        )
    })?;
    lock_workflow_route_schema_version(&mut schema)?;
    serde_json::to_string_pretty(&schema).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize {schema_label} schema: {err}"),
        )
    })
}

fn workflow_operator_schema_json(schema_label: &str) -> Result<String, DiagnosticError> {
    let mut schema = serde_json::to_value(schema_for!(WorkflowOperator)).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize {schema_label} schema: {err}"),
        )
    })?;
    lock_workflow_operator_schema_version(&mut schema)?;
    tighten_workflow_operator_public_context_schemas(&mut schema)?;
    tighten_workflow_operator_routing_field_schemas(&mut schema)?;
    tighten_workflow_operator_phase_bound_recording_context_contracts(&mut schema)?;
    serde_json::to_string_pretty(&schema).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize {schema_label} schema: {err}"),
        )
    })
}

fn workflow_handoff_schema_json(schema_label: &str) -> Result<String, DiagnosticError> {
    let mut schema = serde_json::to_value(schema_for!(WorkflowHandoff)).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize {schema_label} schema: {err}"),
        )
    })?;
    lock_workflow_handoff_schema_version(&mut schema)?;
    inject_embedded_plan_execution_phase_schema(&mut schema)?;
    serde_json::to_string_pretty(&schema).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize {schema_label} schema: {err}"),
        )
    })
}

fn inject_embedded_plan_execution_phase_schema(
    schema: &mut serde_json::Value,
) -> Result<(), DiagnosticError> {
    let defs = schema
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "Workflow schema is missing `$defs`.",
            )
        })?;
    defs.insert(
        String::from("PublicStatusPhaseSchema"),
        serde_json::json!({
            "enum": PUBLIC_STATUS_PHASE_VALUES,
            "type": "string"
        }),
    );
    let plan_execution_status = defs
        .get_mut("PlanExecutionStatus")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "Workflow schema is missing embedded `PlanExecutionStatus`.",
            )
        })?;
    let properties = plan_execution_status
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "Embedded PlanExecutionStatus schema is missing `properties`.",
            )
        })?;
    properties.insert(
        String::from("phase"),
        serde_json::json!({
            "anyOf": [
                { "$ref": "#/$defs/PublicStatusPhaseSchema" },
                { "type": "null" }
            ]
        }),
    );
    Ok(())
}

fn lock_workflow_route_schema_version(
    schema: &mut serde_json::Value,
) -> Result<(), DiagnosticError> {
    let schema_version = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|properties| properties.get_mut("schema_version"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowRoute schema is missing the schema_version property.",
            )
        })?;
    schema_version.insert(
        String::from("const"),
        serde_json::Value::from(WORKFLOW_ROUTE_SCHEMA_VERSION),
    );
    Ok(())
}

fn lock_workflow_operator_schema_version(
    schema: &mut serde_json::Value,
) -> Result<(), DiagnosticError> {
    let schema_version = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|properties| properties.get_mut("schema_version"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema is missing the schema_version property.",
            )
        })?;
    schema_version.insert(
        String::from("const"),
        serde_json::Value::from(WORKFLOW_OPERATOR_SCHEMA_VERSION),
    );
    Ok(())
}

fn lock_workflow_handoff_schema_version(
    schema: &mut serde_json::Value,
) -> Result<(), DiagnosticError> {
    let schema_version = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|properties| properties.get_mut("schema_version"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowHandoff schema is missing the schema_version property.",
            )
        })?;
    schema_version.insert(
        String::from("const"),
        serde_json::Value::from(WORKFLOW_HANDOFF_SCHEMA_VERSION),
    );
    Ok(())
}

fn tighten_workflow_operator_public_context_schemas(
    schema: &mut serde_json::Value,
) -> Result<(), DiagnosticError> {
    let defs = schema
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema is missing `$defs`.",
            )
        })?;
    let execution_context = defs
        .get_mut("WorkflowOperatorExecutionCommandContext")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema is missing `WorkflowOperatorExecutionCommandContext`.",
            )
        })?;
    tighten_operator_execution_command_context_schema(execution_context)?;
    let recording_context = defs
        .get_mut("WorkflowOperatorRecordingContext")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema is missing `WorkflowOperatorRecordingContext`.",
            )
        })?;
    tighten_operator_recording_context_schema(recording_context)?;
    Ok(())
}

fn tighten_workflow_operator_routing_field_schemas(
    schema: &mut serde_json::Value,
) -> Result<(), DiagnosticError> {
    let properties = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema is missing top-level `properties`.",
            )
        })?;
    tighten_operator_schema_property_type(properties, "recommended_command", "string")?;
    Ok(())
}

fn tighten_workflow_operator_phase_bound_recording_context_contracts(
    schema: &mut serde_json::Value,
) -> Result<(), DiagnosticError> {
    append_operator_phase_bound_recording_context_requirements(
        schema,
        crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
        &["task_number"],
    )?;
    append_operator_phase_bound_recording_context_requirements(
        schema,
        crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY,
        &["branch_closure_id"],
    )?;
    append_operator_phase_bound_recording_context_requirements(
        schema,
        crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
        &["branch_closure_id"],
    )?;
    append_operator_phase_bound_recording_context_requirements(
        schema,
        crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
        &["branch_closure_id"],
    )?;
    append_operator_phase_detail_field_forbidden_outside_allowed_phase_details(
        schema,
        "recording_context",
        &[
            crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
            crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY,
            crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
            crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
        ],
    )?;
    append_operator_phase_field_forbidden_outside_const_phase(
        schema,
        "phase",
        crate::execution::phase::PHASE_EXECUTING,
        "execution_command_context",
    )?;
    append_operator_phase_detail_field_omitted_only_in_lanes(
        schema,
        "recommended_command",
        RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS,
    )?;
    Ok(())
}

fn tighten_operator_execution_command_context_schema(
    schema: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), DiagnosticError> {
    let properties = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator execution-command context schema is missing `properties`.",
            )
        })?;
    tighten_operator_schema_property_type(properties, "task_number", "integer")?;
    tighten_operator_schema_property_type(properties, "step_id", "integer")?;
    schema.insert(
        String::from("required"),
        serde_json::json!(["command_kind", "task_number", "step_id"]),
    );
    schema.insert(
        String::from("additionalProperties"),
        serde_json::Value::Bool(false),
    );
    Ok(())
}

fn tighten_operator_recording_context_schema(
    schema: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), DiagnosticError> {
    let properties = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator recording context schema is missing `properties`.",
            )
        })?;
    tighten_operator_schema_property_type(properties, "branch_closure_id", "string")?;
    tighten_operator_schema_property_type(properties, "dispatch_id", "string")?;
    tighten_operator_schema_property_type(properties, "task_number", "integer")?;
    schema.insert(
        String::from("additionalProperties"),
        serde_json::Value::Bool(false),
    );
    schema.insert(String::from("minProperties"), serde_json::Value::from(1));
    schema.insert(
        String::from("anyOf"),
        serde_json::json!([
            {"required": ["branch_closure_id"]},
            {"required": ["task_number"]}
        ]),
    );
    Ok(())
}

fn tighten_operator_schema_property_type(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected_type: &str,
) -> Result<(), DiagnosticError> {
    let property = properties
        .get_mut(field)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("WorkflowOperator schema is missing property `{field}`."),
            )
        })?;
    property.insert(
        String::from("type"),
        serde_json::Value::String(String::from(expected_type)),
    );
    Ok(())
}

fn append_operator_phase_bound_recording_context_requirements(
    schema: &mut serde_json::Value,
    phase_detail: &str,
    required_fields: &[&str],
) -> Result<(), DiagnosticError> {
    let root = schema.as_object_mut().ok_or_else(|| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "WorkflowOperator schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": { "const": phase_detail }
            }
        },
        "then": {
            "required": ["recording_context"],
            "properties": {
                "recording_context": {
                    "required": required_fields
                }
            }
        }
    }));
    Ok(())
}

fn append_operator_phase_detail_field_forbidden_outside_allowed_phase_details(
    schema: &mut serde_json::Value,
    field: &str,
    allowed_phase_details: &[&str],
) -> Result<(), DiagnosticError> {
    let root = schema.as_object_mut().ok_or_else(|| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "WorkflowOperator schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": { "enum": allowed_phase_details }
            }
        },
        "else": {
            "not": {
                "required": [field]
            }
        }
    }));
    Ok(())
}

fn append_operator_phase_field_forbidden_outside_const_phase(
    schema: &mut serde_json::Value,
    phase_field: &str,
    phase_value: &str,
    field: &str,
) -> Result<(), DiagnosticError> {
    let root = schema.as_object_mut().ok_or_else(|| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "WorkflowOperator schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                (phase_field): { "const": phase_value }
            }
        },
        "else": {
            "not": {
                "required": [field]
            }
        }
    }));
    Ok(())
}

fn append_operator_phase_detail_field_omitted_only_in_lanes(
    schema: &mut serde_json::Value,
    field: &str,
    omission_phase_details: &[&str],
) -> Result<(), DiagnosticError> {
    let root = schema.as_object_mut().ok_or_else(|| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "WorkflowOperator schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "WorkflowOperator schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": { "enum": omission_phase_details }
            }
        },
        "then": {
            "not": {
                "required": [field]
            }
        },
        "else": {
            "required": [field]
        }
    }));
    Ok(())
}

fn parse_workflow_spec_candidate(path: &Path) -> Result<WorkflowSpecCandidate, DiagnosticError> {
    let source = fs::read_to_string(path).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not read spec candidate {}: {err}", path.display()),
        )
    })?;
    let workflow_state = parse_header_value(&source, "Workflow State").unwrap_or_default();
    let workflow_state_valid = matches!(
        workflow_state.as_str(),
        "Draft" | "CEO Approved" | "Implementation Target"
    );
    let spec_revision_valid = parse_header_value(&source, "Spec Revision")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .is_some();
    let last_reviewed_by_valid = matches!(
        (
            workflow_state.as_str(),
            parse_header_value(&source, "Last Reviewed By")
                .ok()
                .as_deref(),
        ),
        ("Draft", Some("brainstorming" | "plan-ceo-review"))
            | ("CEO Approved", Some("plan-ceo-review"))
            | ("Implementation Target", Some("clean-context review loop"))
    );
    Ok(WorkflowSpecCandidate {
        path: repo_relative_path(path),
        workflow_state: if workflow_state_valid && last_reviewed_by_valid {
            workflow_state
        } else {
            String::from("Draft")
        },
        spec_revision: parse_header_value(&source, "Spec Revision")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or_default(),
        malformed_headers: !(workflow_state_valid && spec_revision_valid && last_reviewed_by_valid),
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
    let last_reviewed_by = parse_header_value(&source, "Last Reviewed By").unwrap_or_default();
    let last_reviewed_by_valid = matches!(
        (workflow_state.as_str(), last_reviewed_by.as_str()),
        ("Draft", "writing-plans" | "plan-eng-review")
            | ("Engineering Approved", "plan-eng-review")
    );
    let source_spec_path = normalize_repo_path(Path::new(
        parse_header_value(&source, "Source Spec")?.trim_matches('`'),
    ))?;
    let source_spec_revision = parse_header_value(&source, "Source Spec Revision")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    Ok(WorkflowPlanCandidate {
        path: repo_relative_path(path),
        workflow_state: if last_reviewed_by_valid {
            workflow_state
        } else {
            String::from("Draft")
        },
        last_reviewed_by,
        source_spec_path,
        source_spec_revision,
        malformed_headers: !last_reviewed_by_valid,
    })
}

fn analyze_full_contract(
    repo_root: &Path,
    spec: &WorkflowSpecCandidate,
    plan: &WorkflowPlanCandidate,
) -> Option<AnalyzePlanReport> {
    if let Ok(json) = env::var("FEATUREFORGE_WORKFLOW_STATUS_TEST_ANALYZE_REPORT_JSON")
        && let Ok(report) = serde_json::from_str::<AnalyzePlanReport>(&json)
    {
        return Some(report);
    }

    let spec_path = repo_root.join(&spec.path);
    let plan_path = repo_root.join(&plan.path);
    let spec_source = fs::read_to_string(spec_path).ok()?;
    let plan_source = fs::read_to_string(plan_path).ok()?;
    Some(analyze_contract_report(
        repo_root,
        &spec.path,
        &plan.path,
        &spec_source,
        &plan_source,
    ))
}

fn needs_packet_buildability_failure(report: &AnalyzePlanReport) -> bool {
    report.contract_state == "valid" && report.task_count > report.packet_buildable_tasks
}

fn workflow_contract_state(
    report: Option<&AnalyzePlanReport>,
    stale_source_spec_linkage: bool,
    packet_buildability_failure: bool,
) -> String {
    if stale_source_spec_linkage {
        return String::from("stale");
    }
    if packet_buildability_failure {
        return String::from("invalid");
    }
    match report {
        Some(report) => report.contract_state.clone(),
        None => String::from("unknown"),
    }
}

fn workflow_reason_codes(
    report: Option<&AnalyzePlanReport>,
    stale_source_spec_linkage: bool,
    packet_buildability_failure: bool,
) -> Vec<String> {
    let mut reason_codes = Vec::new();
    if stale_source_spec_linkage {
        reason_codes.push(String::from("stale_spec_plan_linkage"));
    }
    if packet_buildability_failure {
        reason_codes.push(String::from("packet_buildability_failure"));
    }
    if let Some(report) = report {
        for code in &report.reason_codes {
            if is_plan_fidelity_reason_code(code) {
                continue;
            }
            if !reason_codes.iter().any(|existing| existing == code) {
                reason_codes.push(code.clone());
            }
        }
    }
    let header_reason_codes = reason_codes
        .iter()
        .filter(|code| is_plan_header_reason_code(code))
        .cloned()
        .collect::<Vec<_>>();
    if !header_reason_codes.is_empty() {
        return header_reason_codes;
    }
    reason_codes
}

fn workflow_diagnostics(
    plan: &WorkflowPlanCandidate,
    spec: &WorkflowSpecCandidate,
    report: Option<&AnalyzePlanReport>,
    stale_source_spec_linkage: bool,
    packet_buildability_failure: bool,
) -> Vec<WorkflowDiagnostic> {
    let mut diagnostics = Vec::new();
    if stale_source_spec_linkage {
        diagnostics.push(WorkflowDiagnostic {
            code: String::from("stale_spec_plan_linkage"),
            severity: String::from("error"),
            artifact: plan.path.clone(),
            message: format!(
                "Plan Source Spec {} does not match the approved spec path {}.",
                plan.source_spec_path, spec.path
            ),
            remediation: String::from(
                "Update the plan Source Spec header or rewrite the plan from the current approved spec.",
            ),
        });
    }
    if packet_buildability_failure {
        diagnostics.push(WorkflowDiagnostic {
            code: String::from("packet_buildability_failure"),
            severity: String::from("error"),
            artifact: plan.path.clone(),
            message: format!(
                "Only {} of {} plan tasks can produce task packets.",
                report.map_or(0, |report| report.packet_buildable_tasks),
                report.map_or(0, |report| report.task_count)
            ),
            remediation: String::from(
                "Repair the plan so every task has a buildable packet before treating it as ready.",
            ),
        });
    }
    if let Some(report) = report {
        let header_reason_present = report
            .reason_codes
            .iter()
            .any(|code| is_plan_header_reason_code(code));
        for diagnostic in &report.diagnostics {
            if is_plan_fidelity_reason_code(&diagnostic.code) {
                continue;
            }
            if header_reason_present && !is_plan_header_reason_code(&diagnostic.code) {
                continue;
            }
            if diagnostics
                .iter()
                .any(|existing| existing.code == diagnostic.code)
            {
                continue;
            }
            diagnostics.push(WorkflowDiagnostic {
                code: diagnostic.code.clone(),
                severity: String::from("error"),
                artifact: plan.path.clone(),
                message: diagnostic.message.clone(),
                remediation: String::from(
                    "Repair the plan contract so workflow status can route the current plan safely.",
                ),
            });
        }
    }
    diagnostics
}

fn draft_plan_needs_authoring(
    plan: &WorkflowPlanCandidate,
    report: Option<&AnalyzePlanReport>,
    stale_source_spec_linkage: bool,
    packet_buildability_failure: bool,
    reason_codes: &[String],
) -> bool {
    let has_non_fidelity_contract_reason = reason_codes
        .iter()
        .any(|code| !is_plan_fidelity_reason_code(code));
    stale_source_spec_linkage
        || packet_buildability_failure
        || plan.malformed_headers
        || report.is_none()
        || has_non_fidelity_contract_reason
}

fn draft_ready_for_engineering_review(
    plan: &WorkflowPlanCandidate,
    plan_needs_authoring: bool,
) -> bool {
    plan.workflow_state == "Draft"
        && !plan_needs_authoring
        && plan.last_reviewed_by != "plan-eng-review"
}

fn draft_ready_for_fidelity_review(
    plan: &WorkflowPlanCandidate,
    gate: &PlanFidelityReviewReport,
    plan_needs_authoring: bool,
) -> bool {
    plan.workflow_state == "Draft"
        && !plan_needs_authoring
        && plan.last_reviewed_by == "plan-eng-review"
        && !matches!(gate.state.as_str(), "pass" | "fail")
}

fn draft_ready_for_engineering_approval(
    plan: &WorkflowPlanCandidate,
    gate: &PlanFidelityReviewReport,
    plan_needs_authoring: bool,
) -> bool {
    plan.workflow_state == "Draft"
        && !plan_needs_authoring
        && plan.last_reviewed_by == "plan-eng-review"
        && matches!(gate.state.as_str(), "pass" | "fail")
}

fn plan_fidelity_allows_implementation(gate: &PlanFidelityReviewReport) -> bool {
    gate.state == "pass" && gate.reason_codes.is_empty() && gate.verified_surfaces_are_complete()
}

fn engineering_approval_fidelity_reason_codes(gate: &PlanFidelityReviewReport) -> Vec<String> {
    let mut reason_codes = vec![String::from(
        engineering_approval_fidelity_primary_reason_code(gate),
    )];
    for code in &gate.reason_codes {
        if !reason_codes.iter().any(|existing| existing == code) {
            reason_codes.push(code.clone());
        }
    }
    reason_codes
}

fn engineering_approval_fidelity_primary_reason_code(
    gate: &PlanFidelityReviewReport,
) -> &'static str {
    if gate.state == "missing"
        || gate
            .reason_codes
            .iter()
            .any(|code| code == "missing_plan_fidelity_review_artifact")
    {
        "engineering_approval_missing_plan_fidelity_review"
    } else if gate.state == "stale"
        || gate
            .reason_codes
            .iter()
            .any(|code| code == "stale_plan_fidelity_review_artifact")
    {
        "engineering_approval_stale_plan_fidelity_review"
    } else if gate
        .reason_codes
        .iter()
        .any(|code| code == "plan_fidelity_review_missing_required_surface")
        || !gate.verified_surfaces_are_complete()
    {
        "engineering_approval_incomplete_plan_fidelity_surfaces"
    } else if gate.state == "fail"
        || gate
            .reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_review_artifact_not_pass")
    {
        "engineering_approval_failed_plan_fidelity_review"
    } else {
        "engineering_approval_invalid_plan_fidelity_review"
    }
}

fn engineering_approval_fidelity_diagnostics(
    plan: &WorkflowPlanCandidate,
    gate: &PlanFidelityReviewReport,
    next_skill: &str,
) -> Vec<WorkflowDiagnostic> {
    let primary_code = engineering_approval_fidelity_primary_reason_code(gate);
    let remediation = format!(
        "Return to {next_skill} and refresh the plan-fidelity review gate for the current approved plan before implementation."
    );
    let mut diagnostics = vec![WorkflowDiagnostic {
        code: String::from(primary_code),
        severity: String::from("error"),
        artifact: plan.path.clone(),
        message: engineering_approval_fidelity_message(primary_code),
        remediation: remediation.clone(),
    }];

    for diagnostic in &gate.diagnostics {
        if diagnostics
            .iter()
            .any(|existing| existing.code == diagnostic.code)
        {
            continue;
        }
        diagnostics.push(WorkflowDiagnostic {
            code: diagnostic.code.clone(),
            severity: String::from("error"),
            artifact: plan.path.clone(),
            message: diagnostic.message.clone(),
            remediation: remediation.clone(),
        });
    }
    diagnostics
}

fn engineering_approval_fidelity_message(code: &str) -> String {
    match code {
        "engineering_approval_missing_plan_fidelity_review" => {
            "Engineering Approved plan cannot route to implementation until a current pass plan-fidelity review artifact exists.".to_owned()
        }
        "engineering_approval_stale_plan_fidelity_review" => {
            "Engineering Approved plan cannot route to implementation because the plan-fidelity review artifact is stale.".to_owned()
        }
        "engineering_approval_incomplete_plan_fidelity_surfaces" => {
            "Engineering Approved plan cannot route to implementation because the plan-fidelity review artifact does not verify the exact required surfaces.".to_owned()
        }
        "engineering_approval_failed_plan_fidelity_review" => {
            "Engineering Approved plan cannot route to implementation because the plan-fidelity review did not pass.".to_owned()
        }
        _ => {
            "Engineering Approved plan cannot route to implementation because the plan-fidelity review artifact is invalid.".to_owned()
        }
    }
}

fn fidelity_review_visible_for_route(
    plan: &WorkflowPlanCandidate,
    next_skill: &str,
    gate: &PlanFidelityReviewReport,
) -> bool {
    next_skill == "featureforge:plan-fidelity-review"
        || (next_skill == "featureforge:plan-eng-review"
            && plan.workflow_state == "Draft"
            && plan.last_reviewed_by == "plan-eng-review"
            && matches!(gate.state.as_str(), "pass" | "fail"))
        || (next_skill == "featureforge:plan-eng-review"
            && plan.workflow_state == "Engineering Approved"
            && matches!(gate.state.as_str(), "pass" | "fail"))
}

fn combine_plan_and_fidelity_reason_codes(
    reason_codes: &[String],
    gate: &PlanFidelityReviewReport,
) -> Vec<String> {
    let mut combined = gate.reason_codes.clone();
    for code in reason_codes {
        if !combined.iter().any(|existing| existing == code) {
            combined.push(code.clone());
        }
    }
    combined
}

fn combine_plan_and_fidelity_diagnostics(
    plan: &WorkflowPlanCandidate,
    diagnostics: &[WorkflowDiagnostic],
    gate: &PlanFidelityReviewReport,
    next_skill: &str,
) -> Vec<WorkflowDiagnostic> {
    let mut combined = plan_fidelity_gate_diagnostics(plan, gate, next_skill);
    for diagnostic in diagnostics {
        if combined
            .iter()
            .any(|existing| existing.code == diagnostic.code)
        {
            continue;
        }
        combined.push(diagnostic.clone());
    }
    combined
}

fn evaluate_plan_fidelity_gate(
    runtime: &WorkflowRuntime,
    spec_path: &str,
    plan_path: &str,
) -> PlanFidelityReviewReport {
    let spec_abs = runtime.identity.repo_root.join(spec_path);
    let plan_abs = runtime.identity.repo_root.join(plan_path);

    let plan = match parse_plan_file(&plan_abs) {
        Ok(plan) => plan,
        Err(_) => {
            return PlanFidelityReviewReport::unverified(
                "invalid",
                String::new(),
                String::new(),
                String::new(),
                vec![String::from("plan_fidelity_verification_incomplete")],
                vec![crate::contracts::plan::ContractDiagnostic {
                    code: String::from("plan_fidelity_verification_incomplete"),
                    message: String::from(
                        "Plan-fidelity review cannot be validated until the plan parses cleanly.",
                    ),
                }],
            );
        }
    };
    let spec = match load_plan_fidelity_spec_document(&spec_abs) {
        Ok(spec) => spec,
        Err(_) => {
            return PlanFidelityReviewReport::unverified(
                "invalid",
                String::new(),
                String::new(),
                String::new(),
                vec![String::from("plan_fidelity_verification_incomplete")],
                vec![crate::contracts::plan::ContractDiagnostic {
                    code: String::from("plan_fidelity_verification_incomplete"),
                    message: String::from(
                        "Plan-fidelity review cannot be validated until the source spec parses cleanly, including a parseable Requirement Index.",
                    ),
                }],
            );
        }
    };

    evaluate_plan_fidelity_review(&spec, &plan, runtime.identity.repo_root.as_path())
}

fn load_plan_fidelity_spec_document(spec_abs: &Path) -> Result<SpecDocument, DiagnosticError> {
    parse_spec_file(spec_abs)
}

fn plan_fidelity_gate_diagnostics(
    plan: &WorkflowPlanCandidate,
    gate: &PlanFidelityReviewReport,
    next_skill: &str,
) -> Vec<WorkflowDiagnostic> {
    let remediation = format!(
        "Return to {next_skill} and rerun the dedicated plan-fidelity reviewer for the current plan."
    );
    gate.diagnostics
        .iter()
        .map(|diagnostic| WorkflowDiagnostic {
            code: diagnostic.code.clone(),
            severity: String::from("error"),
            artifact: plan.path.clone(),
            message: diagnostic.message.clone(),
            remediation: remediation.clone(),
        })
        .collect()
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

fn compatibility_reason(reason_codes: &[String]) -> String {
    if reason_codes
        .iter()
        .any(|code| is_plan_header_reason_code(code))
    {
        return String::from("malformed_plan_headers");
    }
    if reason_codes.is_empty() {
        String::new()
    } else {
        reason_codes.join(",")
    }
}

fn is_plan_fidelity_reason_code(code: &str) -> bool {
    matches!(
        code,
        "missing_plan_fidelity_review_artifact"
            | "stale_plan_fidelity_review_artifact"
            | "plan_fidelity_review_artifact_not_pass"
    ) || code.starts_with("plan_fidelity_")
}

fn is_plan_header_reason_code(code: &str) -> bool {
    matches!(
        code,
        "missing_workflow_state"
            | "invalid_workflow_state"
            | "missing_plan_revision"
            | "missing_execution_mode"
            | "missing_source_spec"
            | "missing_source_spec_revision"
            | "missing_last_reviewed_by"
            | "invalid_last_reviewed_by"
    )
}

fn repo_relative_path(path: &Path) -> String {
    repo_relative_string(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::sha256_hex;
    use tempfile::TempDir;

    const SPEC_PATH: &str = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    const PLAN_PATH: &str = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fn write_fixture_file(repo: &Path, rel: &str, contents: &str) {
        let path = repo.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(path, contents).expect("fixture file should be written");
    }

    fn write_spec(repo: &Path) {
        write_fixture_file(
            repo,
            SPEC_PATH,
            "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] The draft plan must complete an independent fidelity review before engineering review.\n",
        );
    }

    fn write_draft_plan_with_stale_source_revision(repo: &Path) {
        write_fixture_file(
            repo,
            PLAN_PATH,
            "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 0\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 serially.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the final draft plan\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for final approval.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n\n**Done when:**\n- The draft plan is ready for final approval.\n\n**Files:**\n- Test: `src/workflow/status.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
        );
    }

    fn write_current_pass_fidelity_artifact(repo: &Path) {
        let plan_fingerprint =
            sha256_hex(&fs::read(repo.join(PLAN_PATH)).expect("plan should be readable"));
        let spec_fingerprint =
            sha256_hex(&fs::read(repo.join(SPEC_PATH)).expect("spec should be readable"));
        write_fixture_file(
            repo,
            ".featureforge/reviews/plan-fidelity-current-pass.md",
            &format!(
                "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** pass\n**Reviewed Plan:** `{PLAN_PATH}`\n**Reviewed Plan Revision:** 1\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{SPEC_PATH}`\n**Reviewed Spec Revision:** 1\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-explicit-override\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** requirement_index, execution_topology, task_contract, task_determinism, spec_reference_fidelity\n**Verified Requirement IDs:** REQ-001\n"
            ),
        );
    }

    fn fixture_workflow_runtime(repo: &Path, state: &Path) -> WorkflowRuntime {
        WorkflowRuntime {
            identity: RepositoryIdentity {
                repo_root: repo.to_path_buf(),
                remote_url: None,
                branch_name: String::from("main"),
            },
            state_dir: state.to_path_buf(),
            manifest_path: state.join("workflow-manifest.json"),
            manifest: None,
            manifest_warning: None,
            manifest_recovery_reasons: Vec::new(),
        }
    }

    fn empty_resolved_route(repo: &Path) -> WorkflowRoute {
        WorkflowRoute {
            schema_version: WORKFLOW_ROUTE_SCHEMA_VERSION,
            status: String::new(),
            next_skill: String::new(),
            spec_path: SPEC_PATH.to_owned(),
            plan_path: PLAN_PATH.to_owned(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 1,
            plan_candidate_count: 1,
            manifest_path: repo
                .join(".featureforge/workflow-manifest.json")
                .display()
                .to_string(),
            root: repo.display().to_string(),
            reason: String::new(),
            note: String::new(),
        }
    }

    #[test]
    fn explicit_plan_override_suppresses_current_pass_fidelity_when_authoring_defect_routes_to_writing_plans()
     {
        let repo = TempDir::new().expect("repo tempdir should exist");
        let state = TempDir::new().expect("state tempdir should exist");
        write_spec(repo.path());
        write_draft_plan_with_stale_source_revision(repo.path());
        write_current_pass_fidelity_artifact(repo.path());

        let workflow = fixture_workflow_runtime(repo.path(), state.path());
        let route = explicit_plan_override_route(
            &workflow,
            &empty_resolved_route(repo.path()),
            Path::new(PLAN_PATH),
        )
        .expect("explicit plan override should resolve route");

        assert_eq!(route.status, "plan_draft");
        assert_eq!(route.next_skill, "featureforge:writing-plans");
        assert!(
            route.plan_fidelity_review.is_none(),
            "explicit plan override writing-plans routes must suppress current pass fidelity diagnostics: {route:?}"
        );
        assert!(
            route
                .reason_codes
                .iter()
                .all(|code| !code.contains("plan_fidelity")),
            "authoring-defect route must not expose fidelity reason codes: {route:?}"
        );
    }
}
