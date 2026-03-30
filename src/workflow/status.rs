use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use schemars::JsonSchema;
use schemars::schema_for;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::cli::workflow::{
    ArtifactKind, PlanDesignReviewRecordArgs, PlanFidelityRecordArgs, SecurityReviewRecordArgs,
};
use crate::contracts::plan::{
    AnalyzePlanReport, evaluate_plan_fidelity_receipt_at_path, parse_plan_file,
};
use crate::contracts::runtime::{
    analyze_contract_report, build_plan_fidelity_receipt, persist_plan_fidelity_receipt,
    plan_fidelity_receipt_path,
};
use crate::contracts::spec::{SpecDocument, parse_spec_file, repo_relative_string};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::execution::leases::authoritative_state_path;
use crate::execution::state::{
    ExecutionRuntime, current_repo_state_fingerprint, load_execution_context,
};
use crate::execution::topology::{
    ensure_plan_fidelity_source_spec_is_approved, parse_plan_fidelity_review_artifact,
    validate_plan_fidelity_review_artifact,
};
use crate::git::{
    RepositoryIdentity, discover_repo_identity, discover_slug_identity, sha256_hex,
    stored_repo_root_matches_current,
};
use crate::paths::{
    RepoPath, branch_storage_key, featureforge_state_dir, harness_authoritative_artifact_path,
    write_atomic,
};
use crate::session_entry;
use crate::workflow::manifest::{
    ManifestLoadResult, WorkflowManifest, load_manifest, load_manifest_read_only, manifest_path,
    recover_slug_changed_manifest, recover_slug_changed_manifest_read_only, save_manifest,
};
use crate::workflow::markdown_scan::markdown_files_under;

const ACTIVE_SPEC_ROOT: &str = "docs/featureforge/specs";
const ACTIVE_PLAN_ROOT: &str = "docs/featureforge/plans";
const ACTIVE_SESSION_ENTRY_SKILL: &str = "using-featureforge";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanFidelityRecord {
    pub status: String,
    pub receipt_path: String,
    pub review_artifact_path: String,
    pub review_stage: String,
    pub reviewer_source: String,
    pub reviewer_id: String,
    pub verified_surfaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanDesignReviewRecord {
    pub status: String,
    pub receipt_path: String,
    pub review_artifact_path: String,
    pub verdict: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SecurityReviewRecord {
    pub status: String,
    pub state_path: String,
    pub review_artifact_path: String,
    pub verdict: String,
    pub artifact_fingerprint: String,
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
    source_spec_path: String,
    source_spec_revision: Option<u32>,
}

impl WorkflowRuntime {
    pub fn discover(current_dir: &Path) -> Result<Self, DiagnosticError> {
        Self::discover_with_loader(current_dir, false)
    }

    pub fn discover_read_only(current_dir: &Path) -> Result<Self, DiagnosticError> {
        Self::discover_with_loader(current_dir, true)
    }

    fn discover_with_loader(current_dir: &Path, read_only: bool) -> Result<Self, DiagnosticError> {
        let identity = discover_repo_identity(current_dir)?;
        let state_dir = featureforge_state_dir();
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
            .map(|route| self.decorate_route_with_manifest_context(route))
    }

    pub fn status_refresh(&mut self) -> Result<WorkflowRoute, DiagnosticError> {
        let route = self.decorate_route_with_manifest_context(resolve_route(self, false, true)?);

        let manifest = WorkflowManifest {
            version: 1,
            repo_root: self.identity.repo_root.to_string_lossy().into_owned(),
            branch: self.identity.branch_name.clone(),
            expected_spec_path: route.spec_path.clone(),
            expected_plan_path: route.plan_path.clone(),
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
            .map(|route| self.decorate_route_with_manifest_context(route))
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

    pub fn phase(&self) -> Result<WorkflowPhase, DiagnosticError> {
        let route = self.resolve()?;
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
    let (mut spec_candidates, mut malformed_spec_candidates) =
        scan_specs(&runtime.identity.repo_root);
    spec_candidates.sort_by(|left, right| left.path.cmp(&right.path));
    malformed_spec_candidates.sort_by(|left, right| left.path.cmp(&right.path));
    let spec_candidate_count = spec_candidates.len();
    let (spec_candidates, scan_truncated) = apply_fallback_limit(spec_candidates);

    let plan_candidates = scan_plans(&runtime.identity.repo_root);
    let manifest_path = runtime.manifest_path.display().to_string();
    let root = runtime.identity.repo_root.to_string_lossy().into_owned();

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
            let parsed_plan = parse_plan_file(runtime.identity.repo_root.join(&plan.path)).ok();
            let plan_fidelity_gate =
                evaluate_plan_fidelity_gate(runtime, &approved_spec.path, &plan.path);
            if plan_fidelity_gate.state != "pass" {
                let mut combined_reason_codes = plan_fidelity_gate.reason_codes.clone();
                for code in &reason_codes {
                    if !combined_reason_codes
                        .iter()
                        .any(|existing| existing == code)
                    {
                        combined_reason_codes.push(code.clone());
                    }
                }
                let mut combined_diagnostics =
                    plan_fidelity_gate_diagnostics(&plan, &plan_fidelity_gate);
                for diagnostic in &diagnostics {
                    if combined_diagnostics
                        .iter()
                        .any(|existing| existing.code == diagnostic.code)
                    {
                        continue;
                    }
                    combined_diagnostics.push(diagnostic.clone());
                }
                let reason = compatibility_reason(&combined_reason_codes);
                return Ok(WorkflowRoute {
                    schema_version: 2,
                    status: String::from("plan_draft"),
                    next_skill: String::from("featureforge:writing-plans"),
                    spec_path: approved_spec.path.clone(),
                    plan_path: plan.path.clone(),
                    contract_state,
                    reason_codes: combined_reason_codes,
                    diagnostics: combined_diagnostics,
                    scan_truncated,
                    spec_candidate_count,
                    plan_candidate_count: 1,
                    manifest_path,
                    root,
                    reason: reason.clone(),
                    note: reason,
                });
            }
            if parsed_plan.as_ref().is_some_and(plan_requires_design_review) {
                let plan_revision = parsed_plan
                    .as_ref()
                    .map(|document| document.plan_revision)
                    .unwrap_or(1);
                let plan_fingerprint = parsed_plan
                    .as_ref()
                    .map(|document| sha256_hex(document.source.as_bytes()))
                    .unwrap_or_default();
                let design_review_gate = evaluate_plan_design_review_gate(
                    runtime,
                    &plan.path,
                    plan_revision,
                    &plan_fingerprint,
                );
                if design_review_gate.state != "pass" {
                    let next_skill = if design_review_gate.state == "revise" {
                        "featureforge:writing-plans"
                    } else {
                        "featureforge:plan-design-review"
                    };
                    let mut combined_reason_codes = design_review_gate.reason_codes.clone();
                    for code in &reason_codes {
                        if !combined_reason_codes.iter().any(|existing| existing == code) {
                            combined_reason_codes.push(code.clone());
                        }
                    }
                    let mut combined_diagnostics = design_review_gate.diagnostics.clone();
                    for diagnostic in &diagnostics {
                        if combined_diagnostics
                            .iter()
                            .any(|existing| existing.code == diagnostic.code)
                        {
                            continue;
                        }
                        combined_diagnostics.push(diagnostic.clone());
                    }
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
                        scan_truncated,
                        spec_candidate_count,
                        plan_candidate_count: 1,
                        manifest_path,
                        root,
                        reason: reason.clone(),
                        note: reason,
                    });
                }
            }
            if should_reroute_draft_plan_to_writing_plans(&reason_codes) {
                let reason = compatibility_reason(&reason_codes);
                return Ok(WorkflowRoute {
                    schema_version: 2,
                    status: String::from("plan_draft"),
                    next_skill: String::from("featureforge:writing-plans"),
                    spec_path: approved_spec.path.clone(),
                    plan_path: plan.path.clone(),
                    contract_state,
                    reason_codes,
                    diagnostics,
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
                scan_truncated,
                spec_candidate_count,
                plan_candidate_count: 1,
                manifest_path,
                root,
                reason: reason.clone(),
                note: reason,
            });
        }

        let parsed_plan = parse_plan_file(runtime.identity.repo_root.join(&plan.path)).ok();
        if plan.workflow_state == "Engineering Approved"
            && parsed_plan.as_ref().is_some_and(plan_requires_design_review)
        {
            let plan_revision = parsed_plan
                .as_ref()
                .map(|document| document.plan_revision)
                .unwrap_or(1);
            let plan_fingerprint = parsed_plan
                .as_ref()
                .map(|document| sha256_hex(document.source.as_bytes()))
                .unwrap_or_default();
            let design_review_gate = evaluate_plan_design_review_gate(
                runtime,
                &plan.path,
                plan_revision,
                &plan_fingerprint,
            );
            if design_review_gate.state != "pass" {
                let next_skill = if design_review_gate.state == "revise" {
                    "featureforge:writing-plans"
                } else {
                    "featureforge:plan-design-review"
                };
                let reason = compatibility_reason(&design_review_gate.reason_codes);
                return Ok(WorkflowRoute {
                    schema_version: 2,
                    status: String::from("plan_draft"),
                    next_skill: String::from(next_skill),
                    spec_path: approved_spec.path.clone(),
                    plan_path: plan.path.clone(),
                    contract_state,
                    reason_codes: design_review_gate.reason_codes,
                    diagnostics: design_review_gate.diagnostics,
                    scan_truncated,
                    spec_candidate_count,
                    plan_candidate_count: 1,
                    manifest_path,
                    root,
                    reason: reason.clone(),
                    note: reason,
                });
            }
        }

        if !stale_source_spec_linkage
            && !packet_buildability_failure
            && plan.workflow_state == "Engineering Approved"
            && report
                .as_ref()
                .is_some_and(|report| report.contract_state == "valid")
        {
            if read_only {
                return resolve_route(runtime, false, false);
            }
            return Ok(WorkflowRoute {
                schema_version: 2,
                status: String::from("implementation_ready"),
                next_skill: String::new(),
                spec_path: approved_spec.path.clone(),
                plan_path: plan.path.clone(),
                contract_state,
                reason_codes: vec![String::from("implementation_ready")],
                diagnostics: Vec::new(),
                scan_truncated,
                spec_candidate_count,
                plan_candidate_count: 1,
                manifest_path,
                root,
                reason: String::from("implementation_ready"),
                note: String::from("implementation_ready"),
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

fn scan_specs(repo_root: &Path) -> (Vec<WorkflowSpecCandidate>, Vec<WorkflowSpecCandidate>) {
    let mut candidates = Vec::new();
    let mut malformed = Vec::new();
    for path in markdown_files_under(&repo_root.join(ACTIVE_SPEC_ROOT)) {
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

fn read_session_entry(state_dir: &Path) -> SessionEntryState {
    let session_key = env::var("FEATUREFORGE_SESSION_KEY")
        .or_else(|_| env::var("PPID"))
        .unwrap_or_else(|_| String::from("current"));
    match session_entry::inspect(Some(&session_key)) {
        Ok(output) => SessionEntryState {
            outcome: output.outcome,
            decision_source: output.decision_source,
            session_key: output.session_key,
            decision_path: output.decision_path,
            policy_source: output.policy_source,
            persisted: output.persisted,
            failure_class: output.failure_class,
            reason: output.reason,
        },
        Err(error) => SessionEntryState {
            outcome: String::from("needs_user_choice"),
            decision_source: String::from("runtime_failure"),
            session_key,
            decision_path: state_dir
                .join("session-entry")
                .join(ACTIVE_SESSION_ENTRY_SKILL)
                .to_string_lossy()
                .into_owned(),
            policy_source: String::from("default"),
            persisted: false,
            failure_class: error.failure_class().to_owned(),
            reason: error.message().to_owned(),
        },
    }
}

pub fn sync_reason_codes(route: &WorkflowRoute) -> Vec<String> {
    route.reason_codes.clone()
}

pub fn report_contract_state(report: &AnalyzePlanReport) -> &str {
    &report.contract_state
}

pub fn record_plan_fidelity_receipt(
    current_dir: &Path,
    args: &PlanFidelityRecordArgs,
) -> Result<PlanFidelityRecord, DiagnosticError> {
    let repo_root = discover_slug_identity(current_dir).repo_root;
    let state_dir = featureforge_state_dir();
    let slug_identity = discover_slug_identity(repo_root.as_path());
    let plan_path = normalize_repo_path(&args.plan)?;
    let plan_abs = repo_root.join(&plan_path);
    let plan = parse_plan_file(&plan_abs)?;
    let spec_abs = repo_root.join(&plan.source_spec_path);
    let spec = load_plan_fidelity_spec_document(&spec_abs)?;
    ensure_plan_fidelity_source_spec_is_approved(&spec)?;
    let review_artifact_path = normalize_repo_path(&args.review_artifact)?;
    let review_artifact_abs = repo_root.join(&review_artifact_path);
    let review_artifact =
        parse_plan_fidelity_review_artifact(&review_artifact_abs, &review_artifact_path)?;
    validate_plan_fidelity_review_artifact(&review_artifact, &plan, &spec)?;
    let contract_report = crate::contracts::plan::analyze_documents(&spec, &plan);
    if contract_report.reason_codes.iter().any(|code| {
        matches!(
            code.as_str(),
            "delivery_lane_mismatch"
                | "lightweight_missing_safety_justification"
                | "lightweight_missing_release_surface_signal"
                | "lightweight_release_surface_disallowed"
                | "lightweight_missing_distribution_impact_signal"
                | "lightweight_distribution_impact_too_high"
                | "lightweight_missing_deploy_impact_signal"
                | "lightweight_deploy_impact_too_high"
                | "lightweight_missing_migration_risk_signal"
                | "lightweight_migration_risk_too_high"
                | "lightweight_missing_security_review_signal"
                | "lightweight_security_review_signal_invalid"
                | "lightweight_security_review_required"
                | "lightweight_file_scope_exceeds_cap"
                | "lightweight_lane_escalated"
                | "missing_plan_risk_gate_signals"
                | "missing_plan_release_distribution_notes"
                | "risk_gate_delivery_lane_mismatch"
                | "risk_gate_ui_scope_missing"
                | "risk_gate_ui_scope_invalid"
                | "risk_gate_browser_qa_required_missing"
                | "risk_gate_browser_qa_required_invalid"
                | "risk_gate_design_review_required_missing"
                | "risk_gate_design_review_required_invalid"
                | "risk_gate_security_review_required_missing"
                | "risk_gate_security_review_required_invalid"
                | "risk_gate_performance_review_required_missing"
                | "risk_gate_performance_review_required_invalid"
                | "risk_gate_release_surface_missing"
                | "risk_gate_release_surface_invalid"
                | "risk_gate_distribution_impact_missing"
                | "risk_gate_distribution_impact_invalid"
                | "risk_gate_deploy_impact_missing"
                | "risk_gate_deploy_impact_invalid"
                | "risk_gate_migration_risk_missing"
                | "risk_gate_migration_risk_invalid"
                | "release_distribution_notes_path_missing"
                | "risk_gate_versioning_decision_missing"
                | "risk_gate_versioning_decision_invalid"
                | "release_distribution_notes_versioning_rationale_missing"
                | "release_distribution_notes_rollout_missing"
        )
    }) {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "Plan-fidelity pass receipts cannot be recorded while the current draft plan is missing or violating required Delivery Lane, Risk & Gate Signals, or Release & Distribution Notes contract fields.",
        ));
    }

    let receipt =
        build_plan_fidelity_receipt(crate::contracts::runtime::PlanFidelityReceiptInput {
            spec: &spec,
            plan: &plan,
            verdict: &review_artifact.review_verdict,
            review_artifact_path: &review_artifact.path,
            review_artifact_fingerprint: &review_artifact.fingerprint,
            reviewer_stage: &review_artifact.review_stage,
            reviewer_source: &review_artifact.reviewer_source,
            reviewer_id: &review_artifact.reviewer_id,
            distinct_from_stages: &review_artifact.distinct_from_stages,
            checked_surfaces: &review_artifact.verified_surfaces,
            verified_requirement_ids: &review_artifact.verified_requirement_ids,
        });
    let receipt_path = plan_fidelity_receipt_path(
        &state_dir,
        &slug_identity.repo_slug,
        &slug_identity.branch_name,
    );
    persist_plan_fidelity_receipt(&receipt_path, &receipt)?;

    Ok(PlanFidelityRecord {
        status: String::from("ok"),
        receipt_path: receipt_path.display().to_string(),
        review_artifact_path: review_artifact.path,
        review_stage: receipt.reviewer_provenance.review_stage,
        reviewer_source: receipt.reviewer_provenance.reviewer_source,
        reviewer_id: receipt.reviewer_provenance.reviewer_id,
        verified_surfaces: receipt.verification.checked_surfaces,
    })
}

pub fn render_plan_fidelity_record(record: PlanFidelityRecord) -> String {
    format!(
        "Recorded plan-fidelity receipt at {}\nReview artifact: {}\nReview stage: {}\nReviewer source: {}\nReviewer id: {}\nVerified surfaces: {}",
        record.receipt_path,
        record.review_artifact_path,
        record.review_stage,
        record.reviewer_source,
        record.reviewer_id,
        record.verified_surfaces.join(", "),
    )
}

fn plan_design_review_receipt_path(
    state_dir: &Path,
    repo_slug: &str,
    branch_name: &str,
) -> PathBuf {
    let safe_branch = branch_storage_key(branch_name);
    state_dir
        .join("workflow")
        .join(repo_slug)
        .join(format!("{}-plan-design-review-receipt.md", safe_branch))
}

pub fn record_plan_design_review_receipt(
    current_dir: &Path,
    args: &PlanDesignReviewRecordArgs,
) -> Result<PlanDesignReviewRecord, DiagnosticError> {
    let repo_root = discover_slug_identity(current_dir).repo_root;
    let state_dir = featureforge_state_dir();
    let slug_identity = discover_slug_identity(repo_root.as_path());
    let plan_path = normalize_repo_path(&args.plan)?;
    let plan_abs = repo_root.join(&plan_path);
    let plan = parse_plan_file(&plan_abs)?;
    if !plan_requires_design_review(&plan) {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "Plan-design-review receipts can only be recorded for plans that require design review.",
        ));
    }
    let review_artifact_abs = if args.review_artifact.is_absolute() {
        args.review_artifact.clone()
    } else {
        current_dir.join(&args.review_artifact)
    };
    ensure_canonical_runtime_review_artifact(
        &review_artifact_abs,
        &state_dir,
        &slug_identity.repo_slug,
        &slug_identity.branch_name,
        "plan-design-review",
    )?;
    let review_artifact = crate::execution::final_review::parse_artifact_document(&review_artifact_abs);
    let review_artifact_source = fs::read_to_string(&review_artifact_abs).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not read plan-design-review artifact {}: {err}",
                review_artifact_abs.display()
            ),
        )
    })?;
    let canonical_fingerprint = canonical_gate_artifact_fingerprint(&review_artifact_source)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "Plan-design-review artifact fingerprint could not be computed.",
            )
        })?;
    let plan_fingerprint = sha256_hex(plan.source.as_bytes());
    let verdict = review_artifact.headers.get("Result").cloned().unwrap_or_default();
    let authoritative_artifact_fingerprint = sha256_hex(review_artifact_source.as_bytes());
    let valid = review_artifact.title.as_deref() == Some("# Plan Design Review Result")
        && review_artifact.headers.get("Artifact Kind")
            == Some(&String::from("plan-design-review"))
        && review_artifact.headers.get("Schema Version") == Some(&String::from("1"))
        && review_artifact.headers.get("Artifact Provenance")
            == Some(&String::from("runtime-owned"))
        && review_artifact.headers.get("Retention Policy")
            == Some(&String::from("featureforge:authoritative-runtime-artifact"))
        && review_artifact.headers.get("Source Plan") == Some(&format!("`{plan_path}`"))
        && review_artifact.headers.get("Source Plan Revision")
            == Some(&plan.plan_revision.to_string())
        && review_artifact.headers.get("Source Plan Fingerprint") == Some(&plan_fingerprint)
        && review_artifact.headers.get("Branch") == Some(&slug_identity.branch_name)
        && review_artifact.headers.get("Repo") == Some(&slug_identity.repo_slug)
        && review_artifact.headers.get("Receipt Fingerprint") == Some(&canonical_fingerprint)
        && matches!(verdict.as_str(), "pass" | "revise")
        && review_artifact.headers.get("Generated By")
            == Some(&String::from("featureforge:plan-design-review"))
        && review_artifact
            .headers
            .get("Generated At")
            .is_some_and(|value| !value.trim().is_empty());
    if !valid {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "Plan-design-review artifact does not satisfy the required contract for receipt recording.",
        ));
    }
    let authoritative_artifact_path = harness_authoritative_artifact_path(
        &state_dir,
        &slug_identity.repo_slug,
        &slug_identity.branch_name,
        &format!("plan-design-review-{}.md", authoritative_artifact_fingerprint),
    );
    ensure_safe_runtime_write_target(
        &authoritative_artifact_path,
        "authoritative plan-design-review artifact",
    )?;
    if let Some(parent) = authoritative_artifact_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!(
                    "Could not create authoritative plan-design-review artifact directory {}: {err}",
                    parent.display()
                ),
            )
        })?;
    }
    write_atomic(&authoritative_artifact_path, review_artifact_source.as_bytes()).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not publish authoritative plan-design-review artifact {}: {err}",
                authoritative_artifact_path.display()
            ),
        )
    })?;
    let receipt_body = format!(
        "# Plan Design Review Receipt\n\n**Schema Version:** 1\n**Source Plan:** `{plan_path}`\n**Source Plan Revision:** {}\n**Source Plan Fingerprint:** {}\n**Review Artifact:** `{}`\n**Review Artifact Fingerprint:** {}\n**Branch:** {}\n**Repo:** {}\n**Verdict:** {}\n**Generated By:** featureforge:plan-design-review\n",
        plan.plan_revision,
        plan_fingerprint,
        authoritative_artifact_path.display(),
        authoritative_artifact_fingerprint,
        slug_identity.branch_name,
        slug_identity.repo_slug,
        verdict,
    );
    let receipt_path = plan_design_review_receipt_path(
        &state_dir,
        &slug_identity.repo_slug,
        &slug_identity.branch_name,
    );
    ensure_safe_runtime_write_target(&receipt_path, "plan-design-review receipt")?;
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!(
                    "Could not create plan-design-review receipt directory {}: {err}",
                    parent.display()
                ),
            )
        })?;
    }
    write_atomic(&receipt_path, receipt_body.as_bytes()).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not write plan-design-review receipt {}: {err}",
                receipt_path.display()
            ),
        )
    })?;

    Ok(PlanDesignReviewRecord {
        status: String::from("ok"),
        receipt_path: receipt_path.display().to_string(),
        review_artifact_path: review_artifact_abs.display().to_string(),
        verdict,
    })
}

pub fn render_plan_design_review_record(record: PlanDesignReviewRecord) -> String {
    format!(
        "Recorded plan-design-review receipt at {}\nReview artifact: {}\nVerdict: {}",
        record.receipt_path, record.review_artifact_path, record.verdict
    )
}

fn canonical_runtime_review_artifact_dir(state_dir: &Path, repo_slug: &str) -> PathBuf {
    state_dir.join("projects").join(repo_slug)
}

fn ensure_canonical_runtime_review_artifact(
    artifact_path: &Path,
    state_dir: &Path,
    repo_slug: &str,
    branch_name: &str,
    kind: &str,
) -> Result<(), DiagnosticError> {
    let metadata = fs::symlink_metadata(artifact_path).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not inspect {kind} artifact {}: {err}",
                artifact_path.display()
            ),
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "{kind} artifact {} must not be a symlink.",
                artifact_path.display()
            ),
        ));
    }
    let canonical_dir = fs::canonicalize(canonical_runtime_review_artifact_dir(state_dir, repo_slug))
        .map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!(
                    "Could not resolve canonical runtime artifact directory for {kind}: {err}",
                ),
            )
        })?;
    let canonical_artifact = fs::canonicalize(artifact_path).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not resolve canonical {kind} artifact path {}: {err}",
                artifact_path.display()
            ),
        )
    })?;
    if !canonical_artifact.starts_with(&canonical_dir) {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "{kind} artifact {} must live under the authoritative runtime artifact directory {}.",
                canonical_artifact.display(),
                canonical_dir.display()
            ),
        ));
    }
    let file_name = canonical_artifact
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let safe_branch = branch_storage_key(branch_name);
    let expected_fragment = format!("-{safe_branch}-{kind}-");
    if !file_name.ends_with(".md") || !file_name.contains(&expected_fragment) {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "{kind} artifact {} must use the canonical branch-scoped runtime filename pattern containing {}.",
                artifact_path.display(),
                expected_fragment
            ),
        ));
    }
    Ok(())
}

fn current_repo_head_sha(repo_root: &Path) -> Result<String, DiagnosticError> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(repo_root)
        .output()
        .map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Could not resolve current HEAD SHA: {err}"),
            )
        })?;
    if !output.status.success() {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not resolve current HEAD SHA: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn ensure_safe_runtime_write_target(path: &Path, label: &str) -> Result<(), DiagnosticError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Could not inspect {label} {}: {err}", path.display()),
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("{label} {} must not be a symlink.", path.display()),
        ));
    }
    if !metadata.is_file() {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("{label} {} must be a regular file.", path.display()),
        ));
    }
    Ok(())
}

pub fn record_security_review_receipt(
    current_dir: &Path,
    args: &SecurityReviewRecordArgs,
) -> Result<SecurityReviewRecord, DiagnosticError> {
    let runtime = ExecutionRuntime::discover(current_dir).map_err(|error| {
        DiagnosticError::new(FailureClass::InstructionParseFailed, error.message)
    })?;
    let context = load_execution_context(&runtime, &args.plan).map_err(|error| {
        DiagnosticError::new(FailureClass::InstructionParseFailed, error.message)
    })?;
    if !plan_requires_security_review(&context.plan_document) {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "Security-review receipts can only be recorded for plans that require security review.",
        ));
    }

    let review_artifact_abs = if args.review_artifact.is_absolute() {
        args.review_artifact.clone()
    } else {
        current_dir.join(&args.review_artifact)
    };
    ensure_canonical_runtime_review_artifact(
        &review_artifact_abs,
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        "security-review",
    )?;
    let review_artifact = crate::execution::final_review::parse_artifact_document(&review_artifact_abs);
    let review_artifact_source = fs::read_to_string(&review_artifact_abs).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not read security-review artifact {}: {err}",
                review_artifact_abs.display()
            ),
        )
    })?;
    let canonical_fingerprint = canonical_gate_artifact_fingerprint(&review_artifact_source)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "Security-review artifact fingerprint could not be computed.",
            )
        })?;
    let current_head = current_repo_head_sha(&runtime.repo_root)?;
    let current_diff_fingerprint = current_repo_state_fingerprint(&runtime.repo_root).map_err(
        |error| DiagnosticError::new(FailureClass::InstructionParseFailed, error.message),
    )?;
    let verdict = review_artifact.headers.get("Result").cloned().unwrap_or_default();
    let valid = review_artifact.title.as_deref() == Some("# Security Review Result")
        && review_artifact.headers.get("Artifact Kind") == Some(&String::from("security-review"))
        && review_artifact.headers.get("Schema Version") == Some(&String::from("1"))
        && review_artifact.headers.get("Artifact Provenance")
            == Some(&String::from("runtime-owned"))
        && review_artifact.headers.get("Retention Policy")
            == Some(&String::from("featureforge:authoritative-runtime-artifact"))
        && review_artifact.headers.get("Source Plan") == Some(&format!("`{}`", context.plan_rel))
        && review_artifact.headers.get("Source Plan Revision")
            == Some(&context.plan_document.plan_revision.to_string())
        && review_artifact.headers.get("Branch") == Some(&runtime.branch_name)
        && review_artifact.headers.get("Repo") == Some(&runtime.repo_slug)
        && review_artifact.headers.get("Head SHA") == Some(&current_head)
        && review_artifact.headers.get("Execution Diff Fingerprint")
            == Some(&current_diff_fingerprint)
        && review_artifact.headers.get("Receipt Fingerprint") == Some(&canonical_fingerprint)
        && matches!(verdict.as_str(), "pass" | "needs-user-input" | "blocked")
        && review_artifact.headers.get("Generated By")
            == Some(&String::from("featureforge:security-review"))
        && review_artifact
            .headers
            .get("Generated At")
            .is_some_and(|value| !value.trim().is_empty());
    if !valid {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "Security-review artifact does not satisfy the required contract for authoritative recording.",
        ));
    }

    let authoritative_artifact_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("security-review-{}.md", sha256_hex(review_artifact_source.as_bytes())),
    );
    ensure_safe_runtime_write_target(&authoritative_artifact_path, "authoritative security-review artifact")?;
    if let Some(parent) = authoritative_artifact_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!(
                    "Could not create authoritative security-review artifact directory {}: {err}",
                    parent.display()
                ),
            )
        })?;
    }
    write_atomic(&authoritative_artifact_path, review_artifact_source.as_bytes()).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not publish authoritative security-review artifact {}: {err}",
                authoritative_artifact_path.display()
            ),
        )
    })?;

    let state_path = authoritative_state_path(&context);
    ensure_safe_runtime_write_target(&state_path, "authoritative execution state")?;
    let state_source = fs::read_to_string(&state_path).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not read authoritative execution state {}: {err}",
                state_path.display()
            ),
        )
    })?;
    let mut state_json: serde_json::Value = serde_json::from_str(&state_source).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not parse authoritative execution state {}: {err}",
                state_path.display()
            ),
        )
    })?;
    let root = state_json.as_object_mut().ok_or_else(|| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Authoritative execution state {} is not a JSON object.",
                state_path.display()
            ),
        )
    })?;
    root.insert(
        String::from("security_review_state"),
        serde_json::Value::String(String::from("fresh")),
    );
    root.insert(
        String::from("last_security_review_artifact_fingerprint"),
        serde_json::Value::String(sha256_hex(review_artifact_source.as_bytes())),
    );
    if let Some(sequence) = root
        .get("latest_authoritative_sequence")
        .and_then(|value| value.as_u64())
    {
        root.insert(
            String::from("latest_authoritative_sequence"),
            serde_json::Value::Number(serde_json::Number::from(sequence + 1)),
        );
        root.insert(
            String::from("authoritative_sequence"),
            serde_json::Value::Number(serde_json::Number::from(sequence + 1)),
        );
    }
    let serialized_state = serde_json::to_string_pretty(&state_json).map_err(|err| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!(
                    "Could not serialize authoritative execution state {}: {err}",
                    state_path.display()
                ),
            )
        })?;
    write_atomic(&state_path, serialized_state.as_bytes()).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not write authoritative execution state {}: {err}",
                state_path.display()
            ),
        )
    })?;

    Ok(SecurityReviewRecord {
        status: String::from("ok"),
        state_path: state_path.display().to_string(),
        review_artifact_path: review_artifact_abs.display().to_string(),
        verdict,
        artifact_fingerprint: sha256_hex(review_artifact_source.as_bytes()),
    })
}

pub fn render_security_review_record(record: SecurityReviewRecord) -> String {
    format!(
        "Recorded security-review state in {}\nReview artifact: {}\nVerdict: {}\nFingerprint: {}",
        record.state_path, record.review_artifact_path, record.verdict, record.artifact_fingerprint
    )
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
    let workflow_state = parse_header_value(&source, "Workflow State").unwrap_or_default();
    let workflow_state_valid = matches!(workflow_state.as_str(), "Draft" | "CEO Approved");
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
    let last_reviewed_by_valid = matches!(
        (
            workflow_state.as_str(),
            parse_header_value(&source, "Last Reviewed By")
                .ok()
                .as_deref(),
        ),
        ("Draft", Some("writing-plans" | "plan-eng-review"))
            | ("Engineering Approved", Some("plan-eng-review"))
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
        source_spec_path,
        source_spec_revision,
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
    if let Ok(report) = crate::contracts::plan::analyze_plan(&spec_path, &plan_path) {
        return Some(report);
    }
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

fn should_reroute_draft_plan_to_writing_plans(reason_codes: &[String]) -> bool {
    reason_codes.iter().any(|code| {
        matches!(
            code.as_str(),
            "delivery_lane_mismatch"
                | "lightweight_missing_safety_justification"
                | "lightweight_file_scope_exceeds_cap"
                | "lightweight_lane_escalated"
                | "missing_plan_risk_gate_signals"
                | "missing_plan_release_distribution_notes"
                | "risk_gate_delivery_lane_mismatch"
                    | "risk_gate_ui_scope_missing"
                | "risk_gate_ui_scope_invalid"
                    | "risk_gate_browser_qa_required_missing"
                | "risk_gate_browser_qa_required_invalid"
                    | "risk_gate_design_review_required_missing"
                | "risk_gate_design_review_required_invalid"
                    | "risk_gate_security_review_required_missing"
                | "risk_gate_security_review_required_invalid"
                    | "risk_gate_performance_review_required_missing"
                | "risk_gate_performance_review_required_invalid"
                    | "risk_gate_release_surface_missing"
                | "risk_gate_release_surface_invalid"
                    | "risk_gate_distribution_impact_missing"
                | "risk_gate_distribution_impact_invalid"
                    | "risk_gate_deploy_impact_missing"
                | "risk_gate_deploy_impact_invalid"
                    | "risk_gate_migration_risk_missing"
                | "risk_gate_migration_risk_invalid"
                | "release_distribution_notes_path_missing"
                    | "risk_gate_versioning_decision_missing"
                | "risk_gate_versioning_decision_invalid"
                | "release_distribution_notes_versioning_rationale_missing"
                | "release_distribution_notes_rollout_missing"
        )
    })
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

fn evaluate_plan_fidelity_gate(
    runtime: &WorkflowRuntime,
    spec_path: &str,
    plan_path: &str,
) -> crate::contracts::plan::PlanFidelityGateReport {
    let spec_abs = runtime.identity.repo_root.join(spec_path);
    let plan_abs = runtime.identity.repo_root.join(plan_path);
    let receipt_path = plan_fidelity_receipt_path(
        &runtime.state_dir,
        &discover_slug_identity(runtime.identity.repo_root.as_path()).repo_slug,
        &runtime.identity.branch_name,
    );

    let plan = match parse_plan_file(&plan_abs) {
        Ok(plan) => plan,
        Err(_) => {
            return crate::contracts::plan::PlanFidelityGateReport {
                state: String::from("invalid"),
                receipt_path: receipt_path.display().to_string(),
                reviewer_stage: String::new(),
                provenance_source: String::new(),
                verified_requirement_index: false,
                verified_execution_topology: false,
                reason_codes: vec![String::from("plan_fidelity_verification_incomplete")],
                diagnostics: vec![crate::contracts::plan::ContractDiagnostic {
                    code: String::from("plan_fidelity_verification_incomplete"),
                    message: String::from(
                        "Plan-fidelity review cannot be validated until the draft plan parses cleanly.",
                    ),
                }],
            };
        }
    };
    let spec = match load_plan_fidelity_spec_document(&spec_abs) {
        Ok(spec) => spec,
        Err(_) => {
            return crate::contracts::plan::PlanFidelityGateReport {
                state: String::from("invalid"),
                receipt_path: receipt_path.display().to_string(),
                reviewer_stage: String::new(),
                provenance_source: String::new(),
                verified_requirement_index: false,
                verified_execution_topology: false,
                reason_codes: vec![String::from("plan_fidelity_verification_incomplete")],
                diagnostics: vec![crate::contracts::plan::ContractDiagnostic {
                    code: String::from("plan_fidelity_verification_incomplete"),
                    message: String::from(
                        "Plan-fidelity review cannot be validated until the source spec parses cleanly, including a parseable Requirement Index.",
                    ),
                }],
            };
        }
    };

    evaluate_plan_fidelity_receipt_at_path(
        &spec,
        &plan,
        runtime.identity.repo_root.as_path(),
        receipt_path,
    )
}

#[derive(Debug, Clone)]
struct PlanDesignReviewGate {
    state: String,
    reason_codes: Vec<String>,
    diagnostics: Vec<WorkflowDiagnostic>,
}

fn plan_requires_design_review(plan: &crate::contracts::plan::PlanDocument) -> bool {
    plan.risk_gate_signals.as_ref().is_some_and(|signals| {
        signals.design_review_required == "yes" || signals.ui_scope == "material"
    })
}

fn plan_requires_security_review(plan: &crate::contracts::plan::PlanDocument) -> bool {
    match plan.risk_gate_signals.as_ref() {
        Some(signals) => signals.security_review_required == "yes",
        None => true,
    }
}

fn evaluate_plan_design_review_gate(
    runtime: &WorkflowRuntime,
    plan_path: &str,
    plan_revision: u32,
    plan_fingerprint: &str,
) -> PlanDesignReviewGate {
    let slug = discover_slug_identity(&runtime.identity.repo_root);
    let receipt_path = plan_design_review_receipt_path(
        &runtime.state_dir,
        &slug.repo_slug,
        &slug.branch_name,
    );
    let receipt_source = match fs::read_to_string(&receipt_path) {
        Ok(source) => source,
        Err(_) => {
            return PlanDesignReviewGate {
                state: String::from("missing"),
                reason_codes: vec![String::from("plan_design_review_required")],
                diagnostics: vec![WorkflowDiagnostic {
                    code: String::from("plan_design_review_required"),
                    severity: String::from("error"),
                    artifact: plan_path.to_owned(),
                    message: String::from(
                        "Design review is required before engineering approval can continue.",
                    ),
                    remediation: String::from(
                        "Run featureforge:plan-design-review, record the receipt, and return with a fresh pass result.",
                    ),
                }],
            };
        }
    };

    let receipt_plan = parse_header_value(&receipt_source, "Source Plan").ok();
    let receipt_revision = parse_header_value(&receipt_source, "Source Plan Revision").ok();
    let receipt_plan_fingerprint = parse_header_value(&receipt_source, "Source Plan Fingerprint").ok();
    let receipt_branch = parse_header_value(&receipt_source, "Branch").ok();
    let receipt_repo = parse_header_value(&receipt_source, "Repo").ok();
    let receipt_artifact = parse_header_value(&receipt_source, "Review Artifact").ok();
    let receipt_fingerprint = parse_header_value(&receipt_source, "Review Artifact Fingerprint").ok();
    let receipt_verdict = parse_header_value(&receipt_source, "Verdict").unwrap_or_default();

    let receipt_matches_plan = receipt_plan.as_deref() == Some(&format!("`{plan_path}`"))
        && receipt_revision.as_deref() == Some(&plan_revision.to_string())
        && receipt_plan_fingerprint.as_deref() == Some(plan_fingerprint)
        && receipt_branch.as_deref() == Some(&slug.branch_name)
        && receipt_repo.as_deref() == Some(&slug.repo_slug);
    if !receipt_matches_plan {
        return PlanDesignReviewGate {
            state: String::from("invalid"),
            reason_codes: vec![String::from("plan_design_review_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_required"),
                severity: String::from("error"),
                artifact: receipt_path.display().to_string(),
                message: String::from(
                    "The recorded plan-design-review receipt no longer matches the current approved plan revision.",
                ),
                remediation: String::from(
                    "Re-run featureforge:plan-design-review and record a fresh receipt.",
                ),
            }],
        };
    }

    let Some(receipt_artifact) = receipt_artifact else {
        return PlanDesignReviewGate {
            state: String::from("invalid"),
            reason_codes: vec![String::from("plan_design_review_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_required"),
                severity: String::from("error"),
                artifact: receipt_path.display().to_string(),
                message: String::from(
                    "The recorded plan-design-review receipt is missing the review artifact path.",
                ),
                remediation: String::from(
                    "Re-run featureforge:plan-design-review and record a fresh receipt.",
                ),
            }],
        };
    };
    let Some(receipt_fingerprint) = receipt_fingerprint else {
        return PlanDesignReviewGate {
            state: String::from("invalid"),
            reason_codes: vec![String::from("plan_design_review_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_required"),
                severity: String::from("error"),
                artifact: receipt_path.display().to_string(),
                message: String::from(
                    "The recorded plan-design-review receipt is missing the artifact fingerprint binding.",
                ),
                remediation: String::from(
                    "Re-run featureforge:plan-design-review and record a fresh receipt.",
                ),
            }],
        };
    };

    let authoritative_artifact_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &slug.repo_slug,
        &slug.branch_name,
        &format!("plan-design-review-{}.md", receipt_fingerprint),
    );
    if receipt_artifact.trim_matches('`') != authoritative_artifact_path.display().to_string() {
        return PlanDesignReviewGate {
            state: String::from("invalid"),
            reason_codes: vec![String::from("plan_design_review_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_required"),
                severity: String::from("error"),
                artifact: receipt_path.display().to_string(),
                message: String::from(
                    "The recorded plan-design-review receipt does not bind to the authoritative artifact path for its fingerprint.",
                ),
                remediation: String::from(
                    "Re-run featureforge:plan-design-review and record a fresh authoritative receipt.",
                ),
            }],
        };
    }
    let artifact_metadata = match fs::symlink_metadata(&authoritative_artifact_path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return PlanDesignReviewGate {
                state: String::from("invalid"),
                reason_codes: vec![String::from("plan_design_review_required")],
                diagnostics: vec![WorkflowDiagnostic {
                    code: String::from("plan_design_review_required"),
                    severity: String::from("error"),
                    artifact: authoritative_artifact_path.display().to_string(),
                    message: String::from(
                        "The authoritative plan-design-review artifact is missing or unreadable.",
                    ),
                    remediation: String::from(
                        "Re-run featureforge:plan-design-review and record a fresh authoritative receipt.",
                    ),
                }],
            };
        }
    };
    if artifact_metadata.file_type().is_symlink() || !artifact_metadata.is_file() {
        return PlanDesignReviewGate {
            state: String::from("invalid"),
            reason_codes: vec![String::from("plan_design_review_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_required"),
                severity: String::from("error"),
                artifact: authoritative_artifact_path.display().to_string(),
                message: String::from(
                    "The authoritative plan-design-review artifact must be a regular file.",
                ),
                remediation: String::from(
                    "Re-run featureforge:plan-design-review and record a fresh authoritative receipt.",
                ),
            }],
        };
    }
    let source_bytes = match fs::read(&authoritative_artifact_path) {
        Ok(source) => source,
        Err(_) => {
            return PlanDesignReviewGate {
                state: String::from("invalid"),
                reason_codes: vec![String::from("plan_design_review_required")],
                diagnostics: vec![WorkflowDiagnostic {
                    code: String::from("plan_design_review_required"),
                    severity: String::from("error"),
                    artifact: authoritative_artifact_path.display().to_string(),
                    message: String::from(
                        "The authoritative plan-design-review artifact is unreadable.",
                    ),
                    remediation: String::from(
                        "Re-run featureforge:plan-design-review and record a fresh authoritative receipt.",
                    ),
                }],
            };
        }
    };
    if sha256_hex(&source_bytes) != receipt_fingerprint {
        return PlanDesignReviewGate {
            state: String::from("invalid"),
            reason_codes: vec![String::from("plan_design_review_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_required"),
                severity: String::from("error"),
                artifact: authoritative_artifact_path.display().to_string(),
                message: String::from(
                    "The authoritative plan-design-review artifact content no longer matches the recorded receipt fingerprint.",
                ),
                remediation: String::from(
                    "Re-run featureforge:plan-design-review and record a fresh authoritative receipt.",
                ),
            }],
        };
    }
    let source = String::from_utf8_lossy(&source_bytes).to_string();
    let document = crate::execution::final_review::parse_artifact_document(&authoritative_artifact_path);
    let canonical_fingerprint = canonical_gate_artifact_fingerprint(&source);
    let result = document.headers.get("Result").cloned().unwrap_or_default();
    let valid = document.title.as_deref() == Some("# Plan Design Review Result")
        && document.headers.get("Artifact Kind") == Some(&String::from("plan-design-review"))
        && document.headers.get("Schema Version") == Some(&String::from("1"))
        && document.headers.get("Artifact Provenance")
            == Some(&String::from("runtime-owned"))
        && document.headers.get("Retention Policy")
            == Some(&String::from("featureforge:authoritative-runtime-artifact"))
        && document.headers.get("Source Plan") == Some(&format!("`{plan_path}`"))
        && document.headers.get("Source Plan Revision") == Some(&plan_revision.to_string())
        && document.headers.get("Source Plan Fingerprint") == Some(&plan_fingerprint.to_string())
        && document.headers.get("Branch") == Some(&slug.branch_name)
        && document.headers.get("Repo") == Some(&slug.repo_slug)
        && document.headers.get("Receipt Fingerprint") == canonical_fingerprint.as_ref()
        && matches!(receipt_verdict.as_str(), "pass" | "revise")
        && result == receipt_verdict
        && document.headers.get("Generated By")
            == Some(&String::from("featureforge:plan-design-review"))
        && document
            .headers
            .get("Generated At")
            .is_some_and(|value| !value.trim().is_empty());

    if valid && result == "pass" {
        PlanDesignReviewGate {
            state: String::from("pass"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
        }
    } else if valid && result == "revise" {
        PlanDesignReviewGate {
            state: String::from("revise"),
            reason_codes: vec![String::from("plan_design_review_revise_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_revise_required"),
                severity: String::from("error"),
                artifact: receipt_path.display().to_string(),
                message: String::from(
                    "The recorded plan-design-review receipt requires plan changes before engineering approval can continue.",
                ),
                remediation: String::from(
                    "Return to featureforge:writing-plans, address the design review findings, rerun featureforge:plan-design-review, and record a fresh pass receipt.",
                ),
            }],
        }
    } else {
        PlanDesignReviewGate {
            state: String::from("invalid"),
            reason_codes: vec![String::from("plan_design_review_required")],
            diagnostics: vec![WorkflowDiagnostic {
                code: String::from("plan_design_review_required"),
                severity: String::from("error"),
                artifact: receipt_path.display().to_string(),
                message: String::from(
                    "The recorded plan-design-review receipt no longer matches a valid current artifact contract.",
                ),
                remediation: String::from(
                    "Re-run featureforge:plan-design-review and record a fresh pass receipt.",
                ),
            }],
        }
    }
}

fn load_plan_fidelity_spec_document(spec_abs: &Path) -> Result<SpecDocument, DiagnosticError> {
    parse_spec_file(spec_abs)
}

fn plan_fidelity_gate_diagnostics(
    plan: &WorkflowPlanCandidate,
    gate: &crate::contracts::plan::PlanFidelityGateReport,
) -> Vec<WorkflowDiagnostic> {
    gate.diagnostics
        .iter()
        .map(|diagnostic| WorkflowDiagnostic {
            code: diagnostic.code.clone(),
            severity: String::from("error"),
            artifact: plan.path.clone(),
            message: diagnostic.message.clone(),
            remediation: String::from(
                "Return to featureforge:writing-plans, rerun the dedicated plan-fidelity reviewer, and record a fresh matching pass receipt.",
            ),
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

fn canonical_gate_artifact_fingerprint(source: &str) -> Option<String> {
    let filtered = source
        .lines()
        .filter(|line| !line.trim().starts_with("**Receipt Fingerprint:**"))
        .collect::<Vec<_>>()
        .join("\n");
    if filtered.trim().is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(filtered.as_bytes());
    Some(format!("{:x}", hasher.finalize()))
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
