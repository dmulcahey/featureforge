use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contracts::headers;
use crate::contracts::spec::{SpecDocument, parse_spec_file, repo_relative_string};
use crate::contracts::task_contract::{
    TaskContractFields, TaskIntentSource, detect_duplicate_task_intents,
    parse_task_contract_fields, parse_task_step_line, split_canonical_task_blocks,
    validate_task_contract,
};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::execution::topology::parse_plan_fidelity_review_artifact;
use crate::paths::{RepoPath, normalize_repo_relative_file_reference};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanStep {
    pub number: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TaskFileEntry {
    pub action: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanTask {
    pub number: u32,
    pub title: String,
    pub spec_coverage: Vec<String>,
    pub goal: String,
    pub context: Vec<String>,
    pub constraints: Vec<String>,
    pub done_when: Vec<String>,
    pub files: Vec<TaskFileEntry>,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanDocument {
    pub path: String,
    pub workflow_state: String,
    pub plan_revision: u32,
    pub execution_mode: String,
    pub source_spec_path: String,
    pub source_spec_revision: u32,
    pub last_reviewed_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qa_requirement: Option<String>,
    pub coverage_matrix: BTreeMap<String, Vec<u32>>,
    pub tasks: Vec<PlanTask>,
    #[serde(skip)]
    pub source: String,
}

pub const PLAN_FIDELITY_REVIEW_STAGE: &str = "featureforge:plan-fidelity-review";
pub const PLAN_FIDELITY_REQUIRED_SURFACES: [&str; 5] = [
    "requirement_index",
    "execution_topology",
    "task_contract",
    "task_determinism",
    "spec_reference_fidelity",
];
pub const PLAN_FIDELITY_DISTINCT_STAGES: [&str; 2] =
    ["featureforge:writing-plans", "featureforge:plan-eng-review"];
pub const PLAN_FIDELITY_REVIEWER_SOURCE_OPTIONS: [&str; 2] =
    ["fresh-context-subagent", "cross-model"];
pub const PLAN_FIDELITY_REVIEW_VERDICT_OPTIONS: [&str; 2] = ["pass", "fail"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFidelityReviewArtifactTemplate {
    pub artifact_path: String,
    pub content: String,
    pub review_stage: String,
    pub review_verdict_options: Vec<String>,
    pub reviewed_plan_path: String,
    pub reviewed_plan_revision: u32,
    pub reviewed_plan_fingerprint: String,
    pub reviewed_spec_path: String,
    pub reviewed_spec_revision: u32,
    pub reviewed_spec_fingerprint: String,
    pub reviewer_source_options: Vec<String>,
    pub reviewer_id_placeholder: String,
    pub required_distinct_from_stages: Vec<String>,
    pub required_verified_surfaces: Vec<String>,
    pub required_requirement_ids: Vec<String>,
    pub summary_placeholder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFidelityReviewReport {
    pub state: String,
    pub review_artifact_path: String,
    pub reviewer_stage: String,
    pub provenance_source: String,
    pub verified_requirement_index: bool,
    pub verified_execution_topology: bool,
    pub verified_task_contract: bool,
    pub verified_task_determinism: bool,
    pub verified_spec_reference_fidelity: bool,
    pub reason_codes: Vec<String>,
    pub diagnostics: Vec<ContractDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_artifact_template: Option<PlanFidelityReviewArtifactTemplate>,
}

impl PlanFidelityReviewReport {
    pub fn not_applicable() -> Self {
        Self::unverified(
            "not_applicable",
            String::new(),
            String::new(),
            String::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn unverified(
        state: &str,
        review_artifact_path: String,
        reviewer_stage: String,
        provenance_source: String,
        reason_codes: Vec<String>,
        diagnostics: Vec<ContractDiagnostic>,
    ) -> Self {
        Self {
            state: state.to_owned(),
            review_artifact_path,
            reviewer_stage,
            provenance_source,
            verified_requirement_index: false,
            verified_execution_topology: false,
            verified_task_contract: false,
            verified_task_determinism: false,
            verified_spec_reference_fidelity: false,
            reason_codes,
            diagnostics,
            required_artifact_template: None,
        }
    }

    pub fn verified_surfaces_are_complete(&self) -> bool {
        self.verified_requirement_index
            && self.verified_execution_topology
            && self.verified_task_contract
            && self.verified_task_determinism
            && self.verified_spec_reference_fidelity
    }

    pub fn without_required_artifact_template(mut self) -> Self {
        self.required_artifact_template = None;
        self
    }
}

pub fn plan_fidelity_allows_implementation(gate: &PlanFidelityReviewReport) -> bool {
    gate.state == "pass" && gate.reason_codes.is_empty() && gate.verified_surfaces_are_complete()
}

pub fn engineering_approval_fidelity_reason_codes(gate: &PlanFidelityReviewReport) -> Vec<String> {
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

pub fn is_engineering_approval_fidelity_reason_code(code: &str) -> bool {
    matches!(
        code,
        "engineering_approval_missing_plan_fidelity_review"
            | "engineering_approval_stale_plan_fidelity_review"
            | "engineering_approval_incomplete_plan_fidelity_surfaces"
            | "engineering_approval_failed_plan_fidelity_review"
            | "engineering_approval_invalid_plan_fidelity_review"
    )
}

pub fn engineering_approval_fidelity_primary_reason_code(
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
    } else if gate.reason_codes.iter().any(|code| {
        matches!(
            code.as_str(),
            "plan_fidelity_review_artifact_invalid" | "ambiguous_plan_fidelity_review_artifact"
        )
    }) {
        "engineering_approval_invalid_plan_fidelity_review"
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

pub fn engineering_approval_fidelity_message(code: &str) -> String {
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

pub fn plan_fidelity_verification_incomplete_report(
    message: impl Into<String>,
) -> PlanFidelityReviewReport {
    PlanFidelityReviewReport::unverified(
        "invalid",
        String::new(),
        String::new(),
        String::new(),
        vec![String::from("plan_fidelity_verification_incomplete")],
        vec![ContractDiagnostic {
            code: String::from("plan_fidelity_verification_incomplete"),
            message: message.into(),
        }],
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContractDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OverlappingWriteScope {
    pub path: String,
    pub tasks: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ParallelWorktreeRequirement {
    pub tasks: Vec<u32>,
    pub declared_worktrees: usize,
    pub required_worktrees: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AnalyzePlanReport {
    pub contract_state: String,
    pub spec_path: String,
    pub spec_revision: u32,
    pub spec_fingerprint: String,
    pub plan_path: String,
    pub plan_revision: u32,
    pub plan_fingerprint: String,
    pub task_count: usize,
    pub packet_buildable_tasks: usize,
    pub coverage_complete: bool,
    pub task_contract_valid: bool,
    pub task_goal_valid: bool,
    pub task_context_sufficient: bool,
    pub task_constraints_valid: bool,
    pub task_done_when_deterministic: bool,
    pub tasks_self_contained: bool,
    pub task_structure_valid: bool,
    pub files_blocks_valid: bool,
    pub execution_strategy_present: bool,
    pub dependency_diagram_present: bool,
    pub execution_topology_valid: bool,
    pub serial_hazards_resolved: bool,
    pub parallel_lane_ownership_valid: bool,
    pub parallel_workspace_isolation_valid: bool,
    pub parallel_worktree_groups: Vec<Vec<u32>>,
    pub parallel_worktree_requirements: Vec<ParallelWorktreeRequirement>,
    pub reason_codes: Vec<String>,
    pub overlapping_write_scopes: Vec<OverlappingWriteScope>,
    pub plan_fidelity_review: PlanFidelityReviewReport,
    pub diagnostics: Vec<ContractDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionTopologyAnalysis {
    pub execution_strategy_present: bool,
    pub dependency_diagram_present: bool,
    pub execution_topology_valid: bool,
    pub serial_hazards_resolved: bool,
    pub parallel_lane_ownership_valid: bool,
    pub parallel_workspace_isolation_valid: bool,
    pub parallel_worktree_groups: Vec<Vec<u32>>,
    pub parallel_worktree_requirements: Vec<ParallelWorktreeRequirement>,
    pub reason_codes: Vec<String>,
    pub diagnostics: Vec<ContractDiagnostic>,
}

pub fn parse_plan_file(path: impl AsRef<Path>) -> Result<PlanDocument, DiagnosticError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not read plan file {}: {err}", path.display()),
        )
    })?;
    parse_plan_source(path, source)
}

pub fn analyze_plan(
    spec_path: impl AsRef<Path>,
    plan_path: impl AsRef<Path>,
) -> Result<AnalyzePlanReport, DiagnosticError> {
    let spec_path = spec_path.as_ref();
    let plan_path = plan_path.as_ref();
    let spec = parse_spec_file(spec_path)?;
    let plan = parse_plan_file(plan_path)?;
    let mut report = analyze_documents(&spec, &plan);
    if plan_fidelity_review_applicable(&plan) {
        report.plan_fidelity_review = evaluate_plan_fidelity_review(
            &spec,
            &plan,
            repo_root_for_artifact_paths(spec_path, plan_path),
        );
    }
    Ok(report)
}

pub fn plan_fidelity_review_applicable(plan: &PlanDocument) -> bool {
    plan_fidelity_review_applicable_workflow_state(&plan.workflow_state)
}

pub fn plan_fidelity_review_applicable_workflow_state(workflow_state: &str) -> bool {
    matches!(workflow_state, "Draft" | "Engineering Approved")
}

pub fn analyze_documents(spec: &SpecDocument, plan: &PlanDocument) -> AnalyzePlanReport {
    let mut diagnostics = Vec::new();
    let mut reason_codes = Vec::new();

    if plan.source_spec_path != spec.path || plan.source_spec_revision != spec.spec_revision {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "stale_spec_plan_linkage",
            "Plan source spec linkage does not match the current approved spec.",
        );
    }

    let spec_requirement_ids: BTreeSet<_> =
        spec.requirements.iter().map(|req| req.id.clone()).collect();
    let coverage_complete = spec_requirement_ids
        .iter()
        .all(|requirement_id| plan.coverage_matrix.contains_key(requirement_id));
    if !coverage_complete {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "missing_requirement_coverage",
            "Every indexed requirement must appear in the coverage matrix.",
        );
    }

    let task_structure_valid = true;
    let files_blocks_valid = plan.tasks.iter().all(|task| !task.files.is_empty());
    let mut task_contract_valid = true;
    let mut task_goal_valid = true;
    let mut task_context_sufficient = true;
    let mut task_constraints_valid = true;
    let mut task_done_when_deterministic = true;
    let mut tasks_self_contained = true;
    for task in &plan.tasks {
        let fields = TaskContractFields {
            goal: task.goal.clone(),
            context: task.context.clone(),
            constraints: task.constraints.clone(),
            done_when: task.done_when.clone(),
        };
        let validation =
            validate_task_contract(task.number, &task.title, &task.spec_coverage, &fields);
        task_contract_valid &= validation.task_contract_valid;
        task_goal_valid &= validation.task_goal_valid;
        task_context_sufficient &= validation.task_context_sufficient;
        task_constraints_valid &= validation.task_constraints_valid;
        task_done_when_deterministic &= validation.task_done_when_deterministic;
        tasks_self_contained &= validation.task_self_contained;
        for diagnostic in validation.diagnostics {
            push_diagnostic(
                &mut diagnostics,
                &mut reason_codes,
                &diagnostic.code,
                &diagnostic.message,
            );
        }
    }
    let duplicate_intents =
        detect_duplicate_task_intents(plan.tasks.iter().map(|task| TaskIntentSource {
            number: task.number,
            goal: &task.goal,
        }));
    if !duplicate_intents.is_empty() {
        task_contract_valid = false;
        tasks_self_contained = false;
        for tasks in duplicate_intents {
            push_diagnostic(
                &mut diagnostics,
                &mut reason_codes,
                "duplicate_task_intent",
                &format!(
                    "Tasks {:?} have duplicate or overlapping task intent that should be split or clarified.",
                    tasks
                ),
            );
        }
    }
    let packet_buildable_tasks = plan
        .tasks
        .iter()
        .filter(|task| !task.files.is_empty())
        .count();
    let overlapping_write_scopes = detect_overlapping_write_scopes(&plan.tasks);
    let topology = analyze_execution_topology(
        &plan.source,
        &plan
            .tasks
            .iter()
            .map(|task| {
                (
                    task.number,
                    task.files
                        .iter()
                        .filter(|file| file.action != "Test")
                        .map(|file| file.path.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
    );
    for code in &topology.reason_codes {
        if !reason_codes.iter().any(|existing| existing == code) {
            reason_codes.push(code.clone());
        }
    }
    for diagnostic in &topology.diagnostics {
        if !diagnostics.iter().any(|existing| {
            existing.code == diagnostic.code && existing.message == diagnostic.message
        }) {
            diagnostics.push(diagnostic.clone());
        }
    }
    let contract_state = if diagnostics.is_empty() {
        "valid"
    } else {
        "invalid"
    };

    AnalyzePlanReport {
        contract_state: contract_state.to_owned(),
        spec_path: spec.path.clone(),
        spec_revision: spec.spec_revision,
        spec_fingerprint: sha256_hex(spec.source.as_bytes()),
        plan_path: plan.path.clone(),
        plan_revision: plan.plan_revision,
        plan_fingerprint: sha256_hex(plan.source.as_bytes()),
        task_count: plan.tasks.len(),
        packet_buildable_tasks,
        coverage_complete,
        task_contract_valid,
        task_goal_valid,
        task_context_sufficient,
        task_constraints_valid,
        task_done_when_deterministic,
        tasks_self_contained,
        task_structure_valid,
        files_blocks_valid,
        execution_strategy_present: topology.execution_strategy_present,
        dependency_diagram_present: topology.dependency_diagram_present,
        execution_topology_valid: topology.execution_topology_valid,
        serial_hazards_resolved: topology.serial_hazards_resolved,
        parallel_lane_ownership_valid: topology.parallel_lane_ownership_valid,
        parallel_workspace_isolation_valid: topology.parallel_workspace_isolation_valid,
        parallel_worktree_groups: topology.parallel_worktree_groups,
        parallel_worktree_requirements: topology.parallel_worktree_requirements,
        reason_codes,
        overlapping_write_scopes,
        plan_fidelity_review: PlanFidelityReviewReport::not_applicable(),
        diagnostics,
    }
}

pub fn evaluate_plan_fidelity_review(
    spec: &SpecDocument,
    plan: &PlanDocument,
    repo_root: &Path,
) -> PlanFidelityReviewReport {
    let candidates = plan_fidelity_review_artifact_candidates(repo_root, plan);
    if candidates.is_empty() {
        return attach_required_plan_fidelity_artifact_template(
            missing_plan_fidelity_review(),
            spec,
            plan,
        );
    }

    let mut stale_candidate = None;
    let mut invalid_candidate = None;
    let mut current_reports = Vec::new();
    for candidate in candidates {
        let artifact_path_string = candidate
            .strip_prefix(repo_root)
            .ok()
            .and_then(|path| path.to_str())
            .map(|path| path.replace('\\', "/"))
            .unwrap_or_else(|| candidate.display().to_string());
        let artifact = match parse_plan_fidelity_review_artifact(&candidate, &artifact_path_string)
        {
            Ok(artifact) => artifact,
            Err(error) => {
                invalid_candidate.get_or_insert_with(|| {
                    invalid_plan_fidelity_review(
                        artifact_path_string.clone(),
                        "plan_fidelity_review_artifact_invalid",
                        format!("Plan-fidelity review artifact is invalid: {error}"),
                    )
                });
                continue;
            }
        };

        let report = evaluate_parsed_plan_fidelity_review_artifact(&artifact, plan, spec);
        match report.state.as_str() {
            "pass" | "fail" | "invalid" => {
                if artifact.reviewed_plan_path == plan.path
                    && artifact.reviewed_plan_revision == plan.plan_revision
                    && artifact.reviewed_plan_fingerprint == sha256_hex(plan.source.as_bytes())
                    && artifact.reviewed_spec_path == spec.path
                    && artifact.reviewed_spec_revision == spec.spec_revision
                    && artifact.reviewed_spec_fingerprint == sha256_hex(spec.source.as_bytes())
                {
                    current_reports.push(report);
                } else if stale_candidate.is_none() {
                    stale_candidate = Some(stale_plan_fidelity_review(artifact.path.clone()));
                }
            }
            "stale" => {
                stale_candidate.get_or_insert(report);
            }
            _ => {
                invalid_candidate.get_or_insert(report);
            }
        };
    }

    if current_reports.len() > 1 {
        let verdicts = current_reports
            .iter()
            .map(|report| report.state.as_str())
            .collect::<BTreeSet<_>>();
        if verdicts.len() > 1 {
            return attach_required_plan_fidelity_artifact_template(
                invalid_plan_fidelity_review(
                    String::new(),
                    "ambiguous_plan_fidelity_review_artifact",
                    "Multiple current plan-fidelity review artifacts bind to this plan fingerprint with conflicting verdicts."
                        .to_owned(),
                ),
                spec,
                plan,
            );
        }
    }
    if let Some(report) = current_reports.into_iter().next() {
        return attach_required_plan_fidelity_artifact_template(report, spec, plan);
    }
    let report = stale_candidate
        .or(invalid_candidate)
        .unwrap_or_else(missing_plan_fidelity_review);
    attach_required_plan_fidelity_artifact_template(report, spec, plan)
}

fn evaluate_parsed_plan_fidelity_review_artifact(
    artifact: &crate::execution::topology::PlanFidelityReviewArtifact,
    plan: &PlanDocument,
    spec: &SpecDocument,
) -> PlanFidelityReviewReport {
    let mut diagnostics = Vec::new();
    let mut reason_codes = Vec::new();
    let plan_fingerprint = sha256_hex(plan.source.as_bytes());
    let spec_fingerprint = sha256_hex(spec.source.as_bytes());
    let stale_binding = artifact.reviewed_plan_path != plan.path
        || artifact.reviewed_plan_revision != plan.plan_revision
        || artifact.reviewed_plan_fingerprint != plan_fingerprint
        || artifact.reviewed_spec_path != spec.path
        || artifact.reviewed_spec_revision != spec.spec_revision
        || artifact.reviewed_spec_fingerprint != spec_fingerprint;
    if stale_binding {
        return stale_plan_fidelity_review(artifact.path.clone());
    }

    if spec.workflow_state != "CEO Approved" || spec.last_reviewed_by != "plan-ceo-review" {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_source_spec_not_ceo_approved",
            "Plan-fidelity review requires a workflow-valid CEO-approved source spec reviewed by plan-ceo-review.",
        );
    }

    if artifact.review_stage != PLAN_FIDELITY_REVIEW_STAGE
        || !PLAN_FIDELITY_REVIEWER_SOURCE_OPTIONS.contains(&artifact.reviewer_source.as_str())
        || artifact.reviewer_id.trim().is_empty()
    {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_reviewer_provenance_invalid",
            "Plan-fidelity review artifact reviewer provenance is malformed or not independent.",
        );
    }

    let distinct_stages = artifact
        .distinct_from_stages
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if !PLAN_FIDELITY_DISTINCT_STAGES
        .iter()
        .all(|stage| distinct_stages.contains(stage))
    {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_reviewer_provenance_invalid",
            "Plan-fidelity review artifact must declare distinction from writing-plans and plan-eng-review.",
        );
    }

    let checked_surfaces = artifact
        .verified_surfaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let verified_requirement_ids = artifact
        .verified_requirement_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected_requirement_ids = spec
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();
    let expected_surfaces = PLAN_FIDELITY_REQUIRED_SURFACES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let verified_requirement_index = checked_surfaces.contains("requirement_index")
        && verified_requirement_ids == expected_requirement_ids;
    let verified_execution_topology = checked_surfaces.contains("execution_topology");
    let verified_task_contract = checked_surfaces.contains("task_contract");
    let verified_task_determinism = checked_surfaces.contains("task_determinism");
    let verified_spec_reference_fidelity = checked_surfaces.contains("spec_reference_fidelity");

    if checked_surfaces != expected_surfaces {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_review_missing_required_surface",
            "Plan-fidelity review artifact must use the exact required verified-surface set.",
        );
    }
    if verified_requirement_ids != expected_requirement_ids {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_review_requirement_ids_mismatch",
            "Plan-fidelity review artifact must enumerate the exact Requirement Index ids it verified.",
        );
    }
    if artifact.review_verdict != "pass" {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_review_artifact_not_pass",
            "Plan-fidelity review artifact is not in pass state.",
        );
    }

    let state = if reason_codes.is_empty() {
        "pass"
    } else if artifact.review_verdict != "pass"
        && reason_codes.len() == 1
        && reason_codes[0] == "plan_fidelity_review_artifact_not_pass"
    {
        "fail"
    } else {
        "invalid"
    };

    PlanFidelityReviewReport {
        state: state.to_owned(),
        review_artifact_path: artifact.path.clone(),
        reviewer_stage: artifact.review_stage.clone(),
        provenance_source: artifact.reviewer_source.clone(),
        verified_requirement_index,
        verified_execution_topology,
        verified_task_contract,
        verified_task_determinism,
        verified_spec_reference_fidelity,
        reason_codes,
        diagnostics,
        required_artifact_template: None,
    }
}

fn attach_required_plan_fidelity_artifact_template(
    mut report: PlanFidelityReviewReport,
    spec: &SpecDocument,
    plan: &PlanDocument,
) -> PlanFidelityReviewReport {
    if !plan_fidelity_report_requires_artifact_template(&report) {
        return report;
    }
    let artifact_path = if report.review_artifact_path.trim().is_empty() {
        default_plan_fidelity_review_artifact_path(plan)
    } else {
        report.review_artifact_path.clone()
    };
    report.required_artifact_template = Some(build_plan_fidelity_review_artifact_template(
        spec,
        plan,
        artifact_path,
    ));
    report
}

fn plan_fidelity_report_requires_artifact_template(report: &PlanFidelityReviewReport) -> bool {
    matches!(
        report.state.as_str(),
        "missing" | "stale" | "invalid" | "fail"
    )
}

fn build_plan_fidelity_review_artifact_template(
    spec: &SpecDocument,
    plan: &PlanDocument,
    artifact_path: String,
) -> PlanFidelityReviewArtifactTemplate {
    let reviewed_plan_fingerprint = sha256_hex(plan.source.as_bytes());
    let reviewed_spec_fingerprint = sha256_hex(spec.source.as_bytes());
    let reviewer_source_options = PLAN_FIDELITY_REVIEWER_SOURCE_OPTIONS
        .iter()
        .map(|source| (*source).to_owned())
        .collect::<Vec<_>>();
    let review_verdict_options = PLAN_FIDELITY_REVIEW_VERDICT_OPTIONS
        .iter()
        .map(|verdict| (*verdict).to_owned())
        .collect::<Vec<_>>();
    let required_distinct_from_stages = PLAN_FIDELITY_DISTINCT_STAGES
        .iter()
        .map(|stage| (*stage).to_owned())
        .collect::<Vec<_>>();
    let required_verified_surfaces = PLAN_FIDELITY_REQUIRED_SURFACES
        .iter()
        .map(|surface| (*surface).to_owned())
        .collect::<Vec<_>>();
    let required_requirement_ids = spec
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<Vec<_>>();
    let reviewer_id_placeholder = String::from("<reviewer-id>");
    let summary_placeholder = String::from("<review-summary>");
    let content = format!(
        "## Plan Fidelity Review Summary\n\n\
**Review Stage:** {review_stage}\n\
**Review Verdict:** <review-verdict-pass-or-fail>\n\
**Reviewed Plan:** `{reviewed_plan_path}`\n\
**Reviewed Plan Revision:** {reviewed_plan_revision}\n\
**Reviewed Plan Fingerprint:** {reviewed_plan_fingerprint}\n\
**Reviewed Spec:** `{reviewed_spec_path}`\n\
**Reviewed Spec Revision:** {reviewed_spec_revision}\n\
**Reviewed Spec Fingerprint:** {reviewed_spec_fingerprint}\n\
**Reviewer Source:** fresh-context-subagent\n\
**Reviewer ID:** {reviewer_id_placeholder}\n\
**Distinct From Stages:** {distinct_stages}\n\
**Verified Surfaces:** {verified_surfaces}\n\
**Verified Requirement IDs:** {requirement_ids}\n\n\
## Findings Summary\n\n\
{summary_placeholder}\n",
        review_stage = PLAN_FIDELITY_REVIEW_STAGE,
        reviewed_plan_path = plan.path.as_str(),
        reviewed_plan_revision = plan.plan_revision,
        reviewed_plan_fingerprint = reviewed_plan_fingerprint.as_str(),
        reviewed_spec_path = spec.path.as_str(),
        reviewed_spec_revision = spec.spec_revision,
        reviewed_spec_fingerprint = reviewed_spec_fingerprint.as_str(),
        reviewer_id_placeholder = reviewer_id_placeholder.as_str(),
        distinct_stages = required_distinct_from_stages.join(", "),
        verified_surfaces = required_verified_surfaces.join(", "),
        requirement_ids = required_requirement_ids.join(", "),
        summary_placeholder = summary_placeholder.as_str(),
    );

    PlanFidelityReviewArtifactTemplate {
        artifact_path,
        content,
        review_stage: PLAN_FIDELITY_REVIEW_STAGE.to_owned(),
        review_verdict_options,
        reviewed_plan_path: plan.path.clone(),
        reviewed_plan_revision: plan.plan_revision,
        reviewed_plan_fingerprint,
        reviewed_spec_path: spec.path.clone(),
        reviewed_spec_revision: spec.spec_revision,
        reviewed_spec_fingerprint,
        reviewer_source_options,
        reviewer_id_placeholder,
        required_distinct_from_stages,
        required_verified_surfaces,
        required_requirement_ids,
        summary_placeholder,
    }
}

fn default_plan_fidelity_review_artifact_path(plan: &PlanDocument) -> String {
    let plan_stem = plan_fidelity_plan_stem(&plan.path);
    format!(".featureforge/reviews/{plan_stem}-plan-fidelity.md")
}

fn plan_fidelity_review_artifact_candidates(repo_root: &Path, plan: &PlanDocument) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let plan_stem = plan_fidelity_plan_stem(&plan.path);
    let default = repo_root
        .join(".featureforge")
        .join("reviews")
        .join(format!("{plan_stem}-plan-fidelity.md"));
    if default.is_file() {
        candidates.push(default);
    }
    let review_dir = repo_root.join(".featureforge").join("reviews");
    if let Ok(entries) = fs::read_dir(review_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.contains("plan-fidelity") && !candidates.iter().any(|item| item == &path) {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates
}

fn plan_fidelity_plan_stem(plan_path: &str) -> &str {
    Path::new(plan_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plan")
}

fn missing_plan_fidelity_review() -> PlanFidelityReviewReport {
    let mut diagnostics = Vec::new();
    let mut reason_codes = Vec::new();
    push_diagnostic(
        &mut diagnostics,
        &mut reason_codes,
        "missing_plan_fidelity_review_artifact",
        "Plan-fidelity review artifact is missing for the current plan.",
    );
    PlanFidelityReviewReport::unverified(
        "missing",
        String::new(),
        String::new(),
        String::new(),
        reason_codes,
        diagnostics,
    )
}

fn stale_plan_fidelity_review(review_artifact_path: String) -> PlanFidelityReviewReport {
    let mut diagnostics = Vec::new();
    let mut reason_codes = Vec::new();
    push_diagnostic(
        &mut diagnostics,
        &mut reason_codes,
        "stale_plan_fidelity_review_artifact",
        "Plan-fidelity review artifact does not match the current approved spec and plan revision.",
    );
    PlanFidelityReviewReport::unverified(
        "stale",
        review_artifact_path,
        String::new(),
        String::new(),
        reason_codes,
        diagnostics,
    )
}

fn invalid_plan_fidelity_review(
    review_artifact_path: String,
    code: &str,
    message: String,
) -> PlanFidelityReviewReport {
    let mut diagnostics = Vec::new();
    let mut reason_codes = Vec::new();
    push_diagnostic(&mut diagnostics, &mut reason_codes, code, &message);
    PlanFidelityReviewReport::unverified(
        "invalid",
        review_artifact_path,
        String::new(),
        String::new(),
        reason_codes,
        diagnostics,
    )
}

pub fn parse_plan_source(path: &Path, source: String) -> Result<PlanDocument, DiagnosticError> {
    let workflow_state = parse_required_header(&source, "Workflow State")?;
    validate_plan_workflow_state(&workflow_state)?;
    let plan_revision = parse_required_header(&source, "Plan Revision")?
        .parse::<u32>()
        .map_err(|_| missing_header("Plan Revision"))?;
    let execution_mode = parse_required_header(&source, "Execution Mode")?;
    validate_plan_execution_mode(&execution_mode)?;
    let source_spec_path =
        RepoPath::parse(parse_required_header(&source, "Source Spec")?.trim_matches('`'))
            .map(|path| path.as_str().to_owned())
            .map_err(|_| missing_header("Source Spec"))?;
    let source_spec_revision = parse_required_header(&source, "Source Spec Revision")?
        .parse::<u32>()
        .map_err(|_| missing_header("Source Spec Revision"))?;
    let last_reviewed_by = parse_required_header(&source, "Last Reviewed By")?;
    validate_plan_last_reviewed_by(&last_reviewed_by)?;
    let qa_requirement = headers::parse_required_header(&source, "QA Requirement")
        .and_then(|value| normalize_plan_qa_requirement(&value));
    let coverage_matrix = parse_coverage_matrix(&source)?;
    let tasks = parse_tasks(&source)?;

    Ok(PlanDocument {
        path: repo_relative_string(path),
        workflow_state,
        plan_revision,
        execution_mode,
        source_spec_path,
        source_spec_revision,
        last_reviewed_by,
        qa_requirement,
        coverage_matrix,
        tasks,
        source,
    })
}

fn parse_required_header(source: &str, header: &str) -> Result<String, DiagnosticError> {
    headers::parse_required_header(source, header).ok_or_else(|| missing_header(header))
}

fn validate_plan_workflow_state(workflow_state: &str) -> Result<(), DiagnosticError> {
    match workflow_state {
        "Draft" | "Engineering Approved" => Ok(()),
        _ => Err(malformed_header("Workflow State")),
    }
}

fn validate_plan_execution_mode(execution_mode: &str) -> Result<(), DiagnosticError> {
    match execution_mode {
        "none" | "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
            Ok(())
        }
        _ => Err(malformed_header("Execution Mode")),
    }
}

fn validate_plan_last_reviewed_by(last_reviewed_by: &str) -> Result<(), DiagnosticError> {
    match last_reviewed_by {
        "writing-plans" | "plan-eng-review" => Ok(()),
        _ => Err(malformed_header("Last Reviewed By")),
    }
}

pub(crate) fn normalize_plan_qa_requirement(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "required" => Some(String::from("required")),
        "not-required" => Some(String::from("not-required")),
        _ => None,
    }
}

fn parse_coverage_matrix(source: &str) -> Result<BTreeMap<String, Vec<u32>>, DiagnosticError> {
    let mut in_matrix = false;
    let mut coverage = BTreeMap::new();

    for line in source.lines() {
        if line == "## Requirement Coverage Matrix" {
            in_matrix = true;
            continue;
        }
        if in_matrix && line.starts_with("## ") {
            break;
        }
        if !in_matrix {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("- ") else {
            continue;
        };
        let (requirement_id, task_list) = rest.split_once(" -> ").ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Malformed coverage matrix entry: {trimmed}"),
            )
        })?;
        let tasks = task_list
            .trim_start_matches("Task ")
            .split(", Task ")
            .map(|task| {
                task.parse::<u32>().map_err(|_| {
                    DiagnosticError::new(
                        FailureClass::InstructionParseFailed,
                        format!("Malformed coverage task number: {task}"),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        coverage.insert(requirement_id.to_owned(), tasks);
    }

    Ok(coverage)
}

fn parse_tasks(source: &str) -> Result<Vec<PlanTask>, DiagnosticError> {
    split_canonical_task_blocks(source)
        .map_err(|error| DiagnosticError::new(FailureClass::InstructionParseFailed, error.message))?
        .into_iter()
        .map(|task| parse_task_chunk(&task.source))
        .collect()
}

fn parse_task_chunk(chunk: &str) -> Result<PlanTask, DiagnosticError> {
    let mut lines = chunk.lines();
    let heading = lines.next().ok_or_else(|| missing_header("Task heading"))?;
    let heading = heading
        .strip_prefix("## Task ")
        .ok_or_else(|| missing_header("Task heading"))?;
    let (number, title) = heading
        .split_once(": ")
        .ok_or_else(|| missing_header("Task heading"))?;

    let block = lines.collect::<Vec<_>>();
    let spec_coverage = parse_csv_field(&block, "Spec Coverage")?;
    let task_number = number
        .parse::<u32>()
        .map_err(|_| missing_header("Task number"))?;
    let contract_fields = parse_task_contract_fields(task_number, &block).map_err(|error| {
        DiagnosticError::new(FailureClass::InstructionParseFailed, error.message)
    })?;
    let files = parse_file_entries(&block)?;
    let steps = parse_steps(&block)?;

    Ok(PlanTask {
        number: task_number,
        title: title.to_owned(),
        spec_coverage,
        goal: contract_fields.goal,
        context: contract_fields.context,
        constraints: contract_fields.constraints,
        done_when: contract_fields.done_when,
        files,
        steps,
    })
}

fn parse_scalar_field(lines: &[&str], field: &str) -> Result<String, DiagnosticError> {
    let prefix = format!("**{field}:** ");
    lines
        .iter()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(ToOwned::to_owned)
        .ok_or_else(|| missing_header(field))
}

fn parse_csv_field(lines: &[&str], field: &str) -> Result<Vec<String>, DiagnosticError> {
    Ok(parse_scalar_field(lines, field)?
        .split(", ")
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn parse_file_entries(lines: &[&str]) -> Result<Vec<TaskFileEntry>, DiagnosticError> {
    let mut collecting = false;
    let mut files_seen = false;
    let mut files = Vec::new();

    for line in lines {
        if *line == "**Files:**" {
            collecting = true;
            files_seen = true;
            continue;
        }
        if collecting && is_plan_step_prefix(line.trim()) {
            break;
        }
        if !collecting {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("- ") else {
            return Err(DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Malformed files block entry: {trimmed}"),
            ));
        };
        let (action, path) = rest.split_once(": ").ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Malformed files block entry: {trimmed}"),
            )
        })?;
        match action {
            "Create" | "Modify" | "Delete" | "Test" => {}
            _ => {
                return Err(DiagnosticError::new(
                    FailureClass::InstructionParseFailed,
                    format!("Malformed files block entry: {trimmed}"),
                ));
            }
        }
        if !(path.starts_with('`') && path.ends_with('`')) {
            return Err(DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Malformed files block entry: {trimmed}"),
            ));
        }
        let normalized =
            normalize_repo_relative_file_reference(path.trim_matches('`')).map_err(|_| {
                DiagnosticError::new(
                    FailureClass::InstructionParseFailed,
                    format!("Malformed files block entry: {trimmed}"),
                )
            })?;
        files.push(TaskFileEntry {
            action: action.to_owned(),
            path: normalized,
        });
    }

    if !files_seen || files.is_empty() {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            String::from("Task is missing a parseable Files block."),
        ));
    }

    Ok(files)
}

fn parse_steps(lines: &[&str]) -> Result<Vec<PlanStep>, DiagnosticError> {
    let mut steps = Vec::new();
    for line in lines {
        let Some((number, text)) = parse_plan_step_line(line.trim())? else {
            continue;
        };
        steps.push(PlanStep { number, text });
    }
    Ok(steps)
}

fn is_plan_step_prefix(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("- [") else {
        return false;
    };
    let Some(mark) = rest.chars().next() else {
        return false;
    };
    if mark != 'x' && mark != ' ' {
        return false;
    }
    let rest = &rest[mark.len_utf8()..];
    rest.starts_with("] **Step ")
}

fn parse_plan_step_line(line: &str) -> Result<Option<(u32, String)>, DiagnosticError> {
    parse_task_step_line(line)
        .map(|step| step.map(|step| (step.number, step.text)))
        .map_err(|error| DiagnosticError::new(FailureClass::InstructionParseFailed, error.message))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutionDirectiveKind {
    Serial,
    Parallel,
    Last,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecutionDirective {
    kind: ExecutionDirectiveKind,
    tasks: Vec<u32>,
    dependencies: Vec<u32>,
    reason: String,
    ownership_tasks: BTreeSet<u32>,
    declared_worktrees: Option<usize>,
}

pub(crate) fn analyze_execution_topology(
    source: &str,
    task_scopes: &[(u32, Vec<String>)],
) -> ExecutionTopologyAnalysis {
    let mut diagnostics = Vec::new();
    let mut reason_codes = Vec::new();
    let task_numbers = task_scopes
        .iter()
        .map(|(number, _)| *number)
        .collect::<Vec<_>>();
    let execution_strategy = markdown_section(source, "Execution Strategy");
    let execution_strategy_present = execution_strategy.is_some();
    let dependency_diagram = markdown_section(source, "Dependency Diagram");
    let dependency_diagram_present = dependency_diagram.is_some();

    if !execution_strategy_present {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "missing_execution_strategy",
            "Plans must include a canonical Execution Strategy section.",
        );
    }
    if !dependency_diagram_present {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "missing_dependency_diagram",
            "Plans must include a canonical Dependency Diagram section.",
        );
    }

    let mut serial_hazards_resolved = true;
    let mut parallel_lane_ownership_valid = true;
    let mut parallel_workspace_isolation_valid = true;
    let mut parallel_worktree_groups = Vec::new();
    let mut parallel_worktree_requirements = Vec::new();
    let mut task_assignment = BTreeMap::new();
    let mut dependencies: BTreeMap<u32, BTreeSet<u32>> = task_numbers
        .iter()
        .map(|task| (*task, BTreeSet::new()))
        .collect();
    let mut expected_dependency_edges = BTreeSet::new();
    let task_scope_index = task_scopes
        .iter()
        .map(|(task, scopes)| (*task, scopes.iter().cloned().collect::<BTreeSet<_>>()))
        .collect::<BTreeMap<_, _>>();

    let directives = execution_strategy
        .as_ref()
        .map(|section| {
            parse_execution_strategy_directives(section, &mut diagnostics, &mut reason_codes)
        })
        .unwrap_or_default();

    for directive in &directives {
        let mut sorted_tasks = directive.tasks.clone();
        sorted_tasks.sort_unstable();
        sorted_tasks.dedup();
        for task in &sorted_tasks {
            if !task_numbers.contains(task) {
                push_diagnostic(
                    &mut diagnostics,
                    &mut reason_codes,
                    "execution_topology_unknown_task",
                    &format!(
                        "Execution Strategy references Task {} but the task does not exist in the plan.",
                        task
                    ),
                );
                continue;
            }
            if task_assignment
                .insert(*task, directive.kind.clone())
                .is_some()
            {
                push_diagnostic(
                    &mut diagnostics,
                    &mut reason_codes,
                    "execution_topology_duplicate_task",
                    &format!("Execution Strategy assigns Task {} more than once.", task),
                );
            }
        }

        match directive.kind {
            ExecutionDirectiveKind::Serial => {
                if directive.reason.trim().is_empty() {
                    serial_hazards_resolved = false;
                    push_diagnostic(
                        &mut diagnostics,
                        &mut reason_codes,
                        "serial_execution_needs_reason",
                        &format!(
                            "Serialized work for Task {} must include an explicit reason.",
                            sorted_tasks.first().copied().unwrap_or_default()
                        ),
                    );
                }
                if let Some(first) = sorted_tasks.first().copied() {
                    dependencies
                        .entry(first)
                        .or_default()
                        .extend(directive.dependencies.iter().copied());
                    for dependency in &directive.dependencies {
                        expected_dependency_edges.insert((*dependency, first));
                    }
                }
                for pair in sorted_tasks.windows(2) {
                    dependencies.entry(pair[1]).or_default().insert(pair[0]);
                    expected_dependency_edges.insert((pair[0], pair[1]));
                }
            }
            ExecutionDirectiveKind::Parallel => {
                let required_worktrees = sorted_tasks.len();
                let declared_worktrees = directive.declared_worktrees.unwrap_or_default();
                if directive.declared_worktrees != Some(required_worktrees) {
                    parallel_workspace_isolation_valid = false;
                    push_diagnostic(
                        &mut diagnostics,
                        &mut reason_codes,
                        "parallel_workspace_isolation_mismatch",
                        &format!(
                            "Parallel execution groups must declare one isolated worktree per task; Tasks {:?} declare {} worktree(s) but require {}.",
                            sorted_tasks, declared_worktrees, required_worktrees
                        ),
                    );
                }
                let expected_tasks = sorted_tasks.iter().copied().collect::<BTreeSet<_>>();
                if directive.ownership_tasks != expected_tasks {
                    parallel_lane_ownership_valid = false;
                    push_diagnostic(
                        &mut diagnostics,
                        &mut reason_codes,
                        "parallel_lane_missing_ownership",
                        "Parallel execution groups must declare a lane-ownership bullet for every task in the group.",
                    );
                }
                parallel_worktree_groups.push(sorted_tasks.clone());
                parallel_worktree_requirements.push(ParallelWorktreeRequirement {
                    tasks: sorted_tasks.clone(),
                    declared_worktrees,
                    required_worktrees,
                });
                for task in sorted_tasks {
                    dependencies
                        .entry(task)
                        .or_default()
                        .extend(directive.dependencies.iter().copied());
                    for dependency in &directive.dependencies {
                        expected_dependency_edges.insert((*dependency, task));
                    }
                }
            }
            ExecutionDirectiveKind::Last => {
                if directive.reason.trim().is_empty() {
                    serial_hazards_resolved = false;
                    push_diagnostic(
                        &mut diagnostics,
                        &mut reason_codes,
                        "serial_execution_needs_reason",
                        &format!(
                            "Serialized work for Task {} must include an explicit reason.",
                            sorted_tasks.first().copied().unwrap_or_default()
                        ),
                    );
                }
                if let Some(task) = sorted_tasks.first().copied()
                    && task != *task_numbers.last().unwrap_or(&task)
                {
                    serial_hazards_resolved = false;
                    push_diagnostic(
                        &mut diagnostics,
                        &mut reason_codes,
                        "last_execution_not_final_task",
                        &format!(
                            "`Execute Task {} last` is only valid for the numerically final task in the plan.",
                            task
                        ),
                    );
                }
            }
        }
    }

    for task in &task_numbers {
        if !task_assignment.contains_key(task) {
            push_diagnostic(
                &mut diagnostics,
                &mut reason_codes,
                "execution_topology_missing_task_coverage",
                &format!(
                    "Execution Strategy does not assign Task {} to a serial or parallel execution directive.",
                    task
                ),
            );
        }
    }

    for directive in &directives {
        if !matches!(directive.kind, ExecutionDirectiveKind::Last) {
            continue;
        }
        let Some(task) = directive.tasks.first().copied() else {
            continue;
        };
        let prior_tasks = task_numbers
            .iter()
            .copied()
            .filter(|candidate| *candidate != task)
            .collect::<BTreeSet<_>>();
        let mut sink_tasks = prior_tasks.clone();
        for (from, to) in &expected_dependency_edges {
            if prior_tasks.contains(from) && prior_tasks.contains(to) {
                sink_tasks.remove(from);
            }
        }
        for dependency in sink_tasks {
            dependencies.entry(task).or_default().insert(dependency);
            expected_dependency_edges.insert((dependency, task));
        }
    }

    for directive in &directives {
        let mut sorted_tasks = directive.tasks.clone();
        sorted_tasks.sort_unstable();
        sorted_tasks.dedup();
        match directive.kind {
            ExecutionDirectiveKind::Serial => {
                let seam_justified = reason_names_reintegration_seam(&directive.reason)
                    && directive.dependencies.iter().any(|dependency| {
                        task_assignment
                            .get(dependency)
                            .is_some_and(|kind| *kind == ExecutionDirectiveKind::Parallel)
                    });
                let objectively_justified = if sorted_tasks.len() > 1 {
                    sorted_tasks
                        .windows(2)
                        .all(|pair| tasks_share_write_scope(&task_scope_index, pair[0], pair[1]))
                        || seam_justified
                } else if let Some(task) = sorted_tasks.first().copied() {
                    !directive.dependencies.is_empty()
                        || expected_dependency_edges
                            .iter()
                            .any(|(dependency, _)| *dependency == task)
                } else {
                    false
                };
                if !objectively_justified {
                    serial_hazards_resolved = false;
                    push_diagnostic(
                        &mut diagnostics,
                        &mut reason_codes,
                        "serial_execution_unproven",
                        &format!(
                            "Serialized work for Task {} must prove a real hazard through dependency edges or overlapping write scope; prose-only serialization is not allowed.",
                            sorted_tasks.first().copied().unwrap_or_default()
                        ),
                    );
                }
            }
            ExecutionDirectiveKind::Parallel | ExecutionDirectiveKind::Last => {}
        }
    }

    let dependency_closure = transitive_dependencies(&dependencies);
    if let Some(diagram) = dependency_diagram {
        match parse_dependency_diagram_edges(&diagram) {
            Some(edges) => {
                if edges != expected_dependency_edges {
                    push_diagnostic(
                        &mut diagnostics,
                        &mut reason_codes,
                        "dependency_diagram_mismatch",
                        "Dependency Diagram does not express the dependency edges claimed by Execution Strategy.",
                    );
                }
            }
            None => {
                push_diagnostic(
                    &mut diagnostics,
                    &mut reason_codes,
                    "malformed_dependency_diagram",
                    "Dependency Diagram must be parseable and machine-checkable.",
                );
            }
        }
    }
    for overlap in overlapping_scopes(task_scopes) {
        let mut unordered_conflict = false;
        for left_index in 0..overlap.tasks.len() {
            for right_index in left_index + 1..overlap.tasks.len() {
                let left = overlap.tasks[left_index];
                let right = overlap.tasks[right_index];
                let ordered = dependency_closure
                    .get(&left)
                    .is_some_and(|deps| deps.contains(&right))
                    || dependency_closure
                        .get(&right)
                        .is_some_and(|deps| deps.contains(&left));
                if !ordered {
                    unordered_conflict = true;
                    break;
                }
            }
            if unordered_conflict {
                break;
            }
        }
        if unordered_conflict {
            push_diagnostic(
                &mut diagnostics,
                &mut reason_codes,
                "parallel_hotspot_conflict",
                &format!(
                    "Execution Strategy leaves hotspot path {} unordered across tasks {:?}.",
                    overlap.path, overlap.tasks
                ),
            );
        }
    }

    let execution_topology_valid = diagnostics.is_empty();

    ExecutionTopologyAnalysis {
        execution_strategy_present,
        dependency_diagram_present,
        execution_topology_valid,
        serial_hazards_resolved,
        parallel_lane_ownership_valid,
        parallel_workspace_isolation_valid,
        parallel_worktree_groups,
        parallel_worktree_requirements,
        reason_codes,
        diagnostics,
    }
}

fn markdown_section(source: &str, heading: &str) -> Option<String> {
    let target = format!("## {heading}");
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in source.lines() {
        if !in_section {
            if line == target {
                in_section = true;
            }
            continue;
        }
        if line.starts_with("## ") {
            break;
        }
        lines.push(line);
    }
    let content = lines.join("\n").trim().to_owned();
    (!content.is_empty()).then_some(content)
}

fn parse_dependency_diagram_edges(section: &str) -> Option<BTreeSet<(u32, u32)>> {
    let diagram = extract_fenced_text(section).unwrap_or_else(|| section.to_owned());
    let lines = diagram.lines().collect::<Vec<_>>();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    if width == 0 {
        return None;
    }
    let grid = lines
        .iter()
        .map(|line| {
            let mut chars = line.chars().collect::<Vec<_>>();
            chars.resize(width, ' ');
            chars
        })
        .collect::<Vec<_>>();
    let nodes = diagram_nodes(&lines);
    if nodes.is_empty() {
        return None;
    }
    let mut edges = BTreeSet::new();
    for node in &nodes {
        for start in dependency_starts(node, &grid) {
            follow_dependency_edges(node, start, &grid, &nodes, &mut edges);
        }
    }
    Some(edges)
}

fn extract_fenced_text(section: &str) -> Option<String> {
    let mut in_fence = false;
    let mut lines = Vec::new();
    for line in section.lines() {
        if line.trim_start().starts_with("```") {
            if in_fence {
                break;
            }
            in_fence = true;
            continue;
        }
        if in_fence {
            lines.push(line);
        }
    }
    (!lines.is_empty()).then_some(lines.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagramNode {
    task: u32,
    row: usize,
    start: usize,
    end: usize,
}

fn diagram_nodes(lines: &[&str]) -> Vec<DiagramNode> {
    let mut nodes = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        let chars = line.chars().collect::<Vec<_>>();
        let mut index = 0;
        while index + 5 <= chars.len() {
            if chars[index..].starts_with(&['T', 'a', 's', 'k', ' ']) {
                let number_start = index + 5;
                let mut number_end = number_start;
                while number_end < chars.len() && chars[number_end].is_ascii_digit() {
                    number_end += 1;
                }
                if number_end > number_start {
                    if let Ok(task) = chars[number_start..number_end]
                        .iter()
                        .collect::<String>()
                        .parse::<u32>()
                    {
                        nodes.push(DiagramNode {
                            task,
                            row,
                            start: index,
                            end: number_end.saturating_sub(1),
                        });
                    }
                    index = number_end;
                    continue;
                }
            }
            index += 1;
        }
    }
    nodes
}

fn dependency_starts(node: &DiagramNode, grid: &[Vec<char>]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    if let Some(row) = grid.get(node.row) {
        for (col, ch) in row.iter().enumerate().skip(node.end + 1) {
            if *ch == ' ' {
                continue;
            }
            if is_dependency_connector(*ch) {
                starts.push((node.row, col));
            }
            break;
        }
    }
    let max_row = usize::min(node.row + 4, grid.len().saturating_sub(1));
    for (row_index, row) in grid.iter().enumerate().take(max_row + 1).skip(node.row + 1) {
        for (col, ch) in row.iter().enumerate().take(node.end + 1).skip(node.start) {
            if is_dependency_connector(*ch) {
                starts.push((row_index, col));
            }
        }
        if !starts.is_empty() {
            break;
        }
    }
    starts.sort_unstable();
    starts.dedup();
    starts
}

fn follow_dependency_edges(
    source: &DiagramNode,
    start: (usize, usize),
    grid: &[Vec<char>],
    nodes: &[DiagramNode],
    edges: &mut BTreeSet<(u32, u32)>,
) {
    let mut queue = vec![start];
    let mut seen = BTreeSet::new();
    while let Some((row, col)) = queue.pop() {
        if !seen.insert((row, col)) {
            continue;
        }
        let ch = grid[row][col];
        if !is_dependency_connector(ch) {
            continue;
        }
        for target in nodes {
            if target.task == source.task {
                continue;
            }
            if row + 1 == target.row && col >= target.start && col <= target.end {
                edges.insert((source.task, target.task));
            }
            if row == target.row && col + 2 >= target.start && col <= target.start {
                edges.insert((source.task, target.task));
            }
        }
        for (next_row, next_col) in dependency_neighbors(row, col, ch, grid) {
            queue.push((next_row, next_col));
        }
    }
}

fn dependency_neighbors(
    row: usize,
    col: usize,
    ch: char,
    grid: &[Vec<char>],
) -> Vec<(usize, usize)> {
    let mut neighbors = Vec::new();
    let last_col = grid[row].len().saturating_sub(1);
    match ch {
        '|' if row + 1 < grid.len() && is_dependency_connector(grid[row + 1][col]) => {
            neighbors.push((row + 1, col));
        }
        '-' => {
            if col > 0 && is_dependency_connector(grid[row][col - 1]) {
                neighbors.push((row, col - 1));
            }
            if col < last_col && is_dependency_connector(grid[row][col + 1]) {
                neighbors.push((row, col + 1));
            }
        }
        '+' => {
            if row + 1 < grid.len() && is_dependency_connector(grid[row + 1][col]) {
                neighbors.push((row + 1, col));
            }
            if col > 0 && is_dependency_connector(grid[row][col - 1]) {
                neighbors.push((row, col - 1));
            }
            if col < last_col && is_dependency_connector(grid[row][col + 1]) {
                neighbors.push((row, col + 1));
            }
        }
        'v' if row + 1 < grid.len() && is_dependency_connector(grid[row + 1][col]) => {
            neighbors.push((row + 1, col));
        }
        '>' if col < last_col && is_dependency_connector(grid[row][col + 1]) => {
            neighbors.push((row, col + 1));
        }
        _ => {}
    }
    neighbors
}

fn is_dependency_connector(ch: char) -> bool {
    matches!(ch, '|' | '-' | '+' | 'v' | '>')
}

fn parse_execution_strategy_directives(
    section: &str,
    diagnostics: &mut Vec<ContractDiagnostic>,
    reason_codes: &mut Vec<String>,
) -> Vec<ExecutionDirective> {
    let lines = section.lines().collect::<Vec<_>>();
    let mut directives = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        if !trimmed.starts_with("- ") {
            index += 1;
            continue;
        }
        if trimmed.starts_with("- Execute ") {
            match parse_execute_directive(trimmed) {
                Some(directive) => directives.push(directive),
                None => push_diagnostic(
                    diagnostics,
                    reason_codes,
                    "malformed_execution_strategy",
                    "Execution Strategy contains an unparseable Execute directive.",
                ),
            }
            index += 1;
            continue;
        }
        if trimmed.starts_with("- After ") {
            match parse_parallel_directive(&lines, index) {
                Some((directive, consumed)) => {
                    directives.push(directive);
                    index += consumed;
                }
                None => {
                    push_diagnostic(
                        diagnostics,
                        reason_codes,
                        "malformed_execution_strategy",
                        "Execution Strategy contains an unparseable parallel directive.",
                    );
                    index += 1;
                }
            }
            continue;
        }
        index += 1;
    }
    directives
}

fn parse_execute_directive(line: &str) -> Option<ExecutionDirective> {
    let body = line.trim_start_matches("- Execute ").trim_end();
    if body.contains(" serially") {
        let (head, reason) = split_head_and_reason(body);
        let (tasks_part, dependencies_part) =
            if let Some((tasks, deps)) = head.split_once(" after ") {
                (tasks, deps)
            } else {
                (head, "")
            };
        let tasks = extract_numbers(tasks_part);
        if tasks.is_empty() {
            return None;
        }
        return Some(ExecutionDirective {
            kind: ExecutionDirectiveKind::Serial,
            tasks,
            dependencies: extract_numbers(dependencies_part),
            reason,
            ownership_tasks: BTreeSet::new(),
            declared_worktrees: None,
        });
    }
    if body.contains(" last") {
        let normalized = body.trim_end_matches('.');
        let tasks = extract_numbers(normalized);
        if tasks.len() != 1 {
            return None;
        }
        let reason = normalized
            .split_once(" last")
            .map(|(_, rest)| rest.trim().trim_start_matches('.').trim().to_owned())
            .unwrap_or_default();
        return Some(ExecutionDirective {
            kind: ExecutionDirectiveKind::Last,
            tasks,
            dependencies: Vec::new(),
            reason,
            ownership_tasks: BTreeSet::new(),
            declared_worktrees: None,
        });
    }
    None
}

fn parse_parallel_directive(
    lines: &[&str],
    start_index: usize,
) -> Option<(ExecutionDirective, usize)> {
    let line = lines.get(start_index)?.trim_end();
    let body = line.trim_start_matches("- After ").trim_end_matches(':');
    let (dependency_part, rest) = body.split_once(", ")?;
    let (workspace_part, run_part) = rest.split_once(" and run ")?;
    let (task_part, _) = run_part.split_once(" in parallel")?;
    let tasks = extract_numbers(task_part);
    if tasks.is_empty() {
        return None;
    }
    let task_count = tasks.len();

    let mut ownership_tasks = BTreeSet::new();
    let mut consumed = 1;
    for nested in lines.iter().skip(start_index + 1) {
        if nested.trim().is_empty() {
            consumed += 1;
            continue;
        }
        if nested.starts_with("  - Task ") {
            if let Some(task) = parse_ownership_task(nested.trim()) {
                ownership_tasks.insert(task);
            }
            consumed += 1;
            continue;
        }
        break;
    }

    Some((
        ExecutionDirective {
            kind: ExecutionDirectiveKind::Parallel,
            tasks,
            dependencies: extract_numbers(dependency_part),
            reason: String::new(),
            ownership_tasks,
            declared_worktrees: parse_declared_worktree_count(workspace_part, task_count),
        },
        consumed,
    ))
}

fn parse_ownership_task(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("- Task ")?;
    let (number, _) = rest.split_once(" owns ")?;
    number.parse::<u32>().ok()
}

fn parse_declared_worktree_count(workspace_part: &str, task_count: usize) -> Option<usize> {
    let normalized = workspace_part.to_ascii_lowercase();
    if normalized.contains("per task")
        && (normalized.contains("worktree") || normalized.contains("worktrees"))
    {
        return Some(task_count);
    }
    let tokens = workspace_part
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric()))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate() {
        let Some(count) = parse_count_token(token) else {
            continue;
        };
        let window_end = usize::min(index + 3, tokens.len());
        if tokens[index + 1..window_end]
            .iter()
            .any(|candidate| matches!(*candidate, "worktree" | "worktrees"))
        {
            return Some(count);
        }
    }
    None
}

fn parse_count_token(token: &str) -> Option<usize> {
    match token.to_ascii_lowercase().as_str() {
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        _ => token.parse::<usize>().ok(),
    }
}

fn split_head_and_reason(body: &str) -> (&str, String) {
    if let Some((head, reason)) = body.split_once(". ") {
        return (
            head.trim_end_matches('.'),
            reason.trim().trim_end_matches('.').to_owned(),
        );
    }
    (body.trim_end_matches('.'), String::new())
}

fn extract_numbers(value: &str) -> Vec<u32> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(number) = current.parse::<u32>() {
                numbers.push(number);
            }
            current.clear();
        }
    }
    if !current.is_empty()
        && let Ok(number) = current.parse::<u32>()
    {
        numbers.push(number);
    }
    numbers
}

fn transitive_dependencies(direct: &BTreeMap<u32, BTreeSet<u32>>) -> BTreeMap<u32, BTreeSet<u32>> {
    direct
        .keys()
        .copied()
        .map(|task| {
            (
                task,
                collect_dependencies(task, direct, &mut BTreeSet::new()),
            )
        })
        .collect()
}

fn collect_dependencies(
    task: u32,
    direct: &BTreeMap<u32, BTreeSet<u32>>,
    seen: &mut BTreeSet<u32>,
) -> BTreeSet<u32> {
    let mut collected = BTreeSet::new();
    for dependency in direct.get(&task).into_iter().flatten().copied() {
        if !seen.insert(dependency) {
            continue;
        }
        collected.insert(dependency);
        collected.extend(collect_dependencies(dependency, direct, seen));
    }
    collected
}

fn overlapping_scopes(task_scopes: &[(u32, Vec<String>)]) -> Vec<OverlappingWriteScope> {
    let mut index: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (task, scopes) in task_scopes {
        for scope in scopes {
            index.entry(scope.clone()).or_default().push(*task);
        }
    }
    index
        .into_iter()
        .filter_map(|(path, mut tasks)| {
            tasks.sort_unstable();
            tasks.dedup();
            (tasks.len() > 1).then_some(OverlappingWriteScope { path, tasks })
        })
        .collect()
}

fn tasks_share_write_scope(
    task_scope_index: &BTreeMap<u32, BTreeSet<String>>,
    left: u32,
    right: u32,
) -> bool {
    let Some(left_scopes) = task_scope_index.get(&left) else {
        return false;
    };
    let Some(right_scopes) = task_scope_index.get(&right) else {
        return false;
    };
    left_scopes.iter().any(|scope| right_scopes.contains(scope))
}

fn reason_names_reintegration_seam(reason: &str) -> bool {
    let normalized = reason.to_ascii_lowercase();
    normalized.contains("reintegration")
        || normalized.contains("integration seam")
        || normalized.contains("merge-back")
        || normalized.contains("merge back")
        || normalized.contains("shared glue")
        || normalized.contains("integration risk")
        || normalized.contains("cannot be isolated")
}

fn detect_overlapping_write_scopes(tasks: &[PlanTask]) -> Vec<OverlappingWriteScope> {
    let mut index: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for task in tasks {
        for file in &task.files {
            if file.action == "Test" {
                continue;
            }
            index
                .entry(file.path.clone())
                .or_default()
                .push(task.number);
        }
    }
    index
        .into_iter()
        .filter_map(|(path, mut tasks)| {
            tasks.sort_unstable();
            tasks.dedup();
            (tasks.len() > 1).then_some(OverlappingWriteScope { path, tasks })
        })
        .collect()
}

fn push_diagnostic(
    diagnostics: &mut Vec<ContractDiagnostic>,
    reason_codes: &mut Vec<String>,
    code: &str,
    message: &str,
) {
    diagnostics.push(ContractDiagnostic {
        code: code.to_owned(),
        message: message.to_owned(),
    });
    reason_codes.push(code.to_owned());
}

fn malformed_header(header: &str) -> DiagnosticError {
    DiagnosticError::new(
        FailureClass::InstructionParseFailed,
        format!("{header} header is missing or malformed."),
    )
}

fn repo_root_for_artifact_paths<'a>(spec_path: &'a Path, plan_path: &'a Path) -> &'a Path {
    for ancestor in plan_path.ancestors() {
        if ancestor.join("docs/featureforge").is_dir() || ancestor.join(".git").is_dir() {
            return ancestor;
        }
    }
    spec_path
        .parent()
        .or_else(|| plan_path.parent())
        .unwrap_or_else(|| Path::new("."))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn missing_header(header: &str) -> DiagnosticError {
    DiagnosticError::new(
        FailureClass::InstructionParseFailed,
        format!("Missing or malformed {header}."),
    )
}
