use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contracts::headers;
use crate::contracts::spec::{SpecDocument, parse_spec_file, repo_relative_string};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::git::discover_slug_identity;
use crate::paths::{RepoPath, featureforge_state_dir, harness_branch_root};

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
    pub task_outcome: String,
    pub plan_constraints: Vec<String>,
    pub open_questions: String,
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
    pub coverage_matrix: BTreeMap<String, Vec<u32>>,
    pub tasks: Vec<PlanTask>,
    #[serde(skip)]
    pub source: String,
}

pub const PLAN_FIDELITY_RECEIPT_SCHEMA_VERSION: u32 = 2;
pub const PLAN_FIDELITY_RECEIPT_KIND: &str = "plan_fidelity_receipt";
pub const PLAN_FIDELITY_REVIEW_STAGE: &str = "featureforge:plan-fidelity-review";
pub const PLAN_FIDELITY_REQUIRED_SURFACES: [&str; 2] = ["requirement_index", "execution_topology"];
pub const PLAN_FIDELITY_DISTINCT_STAGES: [&str; 2] =
    ["featureforge:writing-plans", "featureforge:plan-eng-review"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFidelityReviewerProvenance {
    pub review_stage: String,
    pub reviewer_source: String,
    pub reviewer_id: String,
    pub distinct_from_stages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFidelityVerification {
    pub checked_surfaces: Vec<String>,
    pub verified_requirement_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFidelityReceipt {
    pub schema_version: u32,
    pub receipt_kind: String,
    pub verdict: String,
    pub spec_path: String,
    pub spec_revision: u32,
    pub spec_fingerprint: String,
    pub plan_path: String,
    pub plan_revision: u32,
    pub plan_fingerprint: String,
    pub review_artifact_path: String,
    pub review_artifact_fingerprint: String,
    pub reviewer_provenance: PlanFidelityReviewerProvenance,
    pub verification: PlanFidelityVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFidelityGateReport {
    pub state: String,
    pub receipt_path: String,
    pub reviewer_stage: String,
    pub provenance_source: String,
    pub verified_requirement_index: bool,
    pub verified_execution_topology: bool,
    pub reason_codes: Vec<String>,
    pub diagnostics: Vec<ContractDiagnostic>,
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
    pub open_questions_resolved: bool,
    pub task_structure_valid: bool,
    pub files_blocks_valid: bool,
    pub reason_codes: Vec<String>,
    pub overlapping_write_scopes: Vec<OverlappingWriteScope>,
    pub plan_fidelity_receipt: PlanFidelityGateReport,
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
    if plan.workflow_state == "Draft" {
        let gate = evaluate_plan_fidelity_receipt_at_path(
            &spec,
            &plan,
            repo_root_for_artifact_paths(spec_path, plan_path),
            plan_fidelity_receipt_path_for_repo(
                repo_root_for_artifact_paths(spec_path, plan_path),
            ),
        );
        merge_plan_fidelity_gate(&mut report, &gate);
        report.plan_fidelity_receipt = gate;
    }
    Ok(report)
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

    let open_questions_resolved = plan.tasks.iter().all(|task| task.open_questions == "none");
    let task_structure_valid = true;
    let files_blocks_valid = plan.tasks.iter().all(|task| !task.files.is_empty());
    let packet_buildable_tasks = plan
        .tasks
        .iter()
        .filter(|task| !task.files.is_empty())
        .count();
    let overlapping_write_scopes = detect_overlapping_write_scopes(&plan.tasks);
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
        open_questions_resolved,
        task_structure_valid,
        files_blocks_valid,
        reason_codes,
        overlapping_write_scopes,
        plan_fidelity_receipt: PlanFidelityGateReport {
            state: String::from("not_applicable"),
            receipt_path: String::new(),
            reviewer_stage: String::new(),
            provenance_source: String::new(),
            verified_requirement_index: false,
            verified_execution_topology: false,
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
        },
        diagnostics,
    }
}

pub fn evaluate_plan_fidelity_receipt_at_path(
    spec: &SpecDocument,
    plan: &PlanDocument,
    repo_root: &Path,
    receipt_path: impl AsRef<Path>,
) -> PlanFidelityGateReport {
    let receipt_path = receipt_path.as_ref();
    let receipt_path_string = receipt_path.display().to_string();
    let mut diagnostics = Vec::new();
    let mut reason_codes = Vec::new();

    let source = match fs::read_to_string(receipt_path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            push_diagnostic(
                &mut diagnostics,
                &mut reason_codes,
                "missing_plan_fidelity_receipt",
                "Plan-fidelity receipt is missing for the current draft plan.",
            );
            return PlanFidelityGateReport {
                state: String::from("missing"),
                receipt_path: receipt_path_string,
                reviewer_stage: String::new(),
                provenance_source: String::new(),
                verified_requirement_index: false,
                verified_execution_topology: false,
                reason_codes,
                diagnostics,
            };
        }
        Err(error) => {
            push_diagnostic(
                &mut diagnostics,
                &mut reason_codes,
                "malformed_plan_fidelity_receipt",
                &format!(
                    "Could not read plan-fidelity receipt {}: {error}",
                    receipt_path.display()
                ),
            );
            return PlanFidelityGateReport {
                state: String::from("malformed"),
                receipt_path: receipt_path_string,
                reviewer_stage: String::new(),
                provenance_source: String::new(),
                verified_requirement_index: false,
                verified_execution_topology: false,
                reason_codes,
                diagnostics,
            };
        }
    };

    let receipt = match serde_json::from_str::<PlanFidelityReceipt>(&source) {
        Ok(receipt) => receipt,
        Err(error) => {
            push_diagnostic(
                &mut diagnostics,
                &mut reason_codes,
                "malformed_plan_fidelity_receipt",
                &format!("Plan-fidelity receipt is not valid json: {error}"),
            );
            return PlanFidelityGateReport {
                state: String::from("malformed"),
                receipt_path: receipt_path_string,
                reviewer_stage: String::new(),
                provenance_source: String::new(),
                verified_requirement_index: false,
                verified_execution_topology: false,
                reason_codes,
                diagnostics,
            };
        }
    };

    if receipt.schema_version != PLAN_FIDELITY_RECEIPT_SCHEMA_VERSION
        || receipt.receipt_kind != PLAN_FIDELITY_RECEIPT_KIND
    {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "malformed_plan_fidelity_receipt",
            "Plan-fidelity receipt has an unsupported schema version or receipt kind.",
        );
        return PlanFidelityGateReport {
            state: String::from("malformed"),
            receipt_path: receipt_path_string,
            reviewer_stage: receipt.reviewer_provenance.review_stage,
            provenance_source: receipt.reviewer_provenance.reviewer_source,
            verified_requirement_index: false,
            verified_execution_topology: false,
            reason_codes,
            diagnostics,
        };
    }

    let spec_fingerprint = sha256_hex(spec.source.as_bytes());
    let plan_fingerprint = sha256_hex(plan.source.as_bytes());
    let stale_binding = receipt.spec_path != spec.path
        || receipt.spec_revision != spec.spec_revision
        || receipt.spec_fingerprint != spec_fingerprint
        || receipt.plan_path != plan.path
        || receipt.plan_revision != plan.plan_revision
        || receipt.plan_fingerprint != plan_fingerprint;
    if stale_binding {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "stale_plan_fidelity_receipt",
            "Plan-fidelity receipt does not match the current approved spec and draft plan revision.",
        );
    }

    if receipt.verdict != "pass" {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_receipt_not_pass",
            "Plan-fidelity receipt is not in pass state.",
        );
    }
    if spec.workflow_state != "CEO Approved" || spec.last_reviewed_by != "plan-ceo-review" {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_source_spec_not_ceo_approved",
            "Plan-fidelity review requires a workflow-valid CEO-approved source spec reviewed by plan-ceo-review.",
        );
    }
    if receipt.review_artifact_path.trim().is_empty()
        || receipt.review_artifact_fingerprint.trim().is_empty()
    {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_receipt_missing_review_artifact_binding",
            "Plan-fidelity receipt must bind to a concrete review artifact path and fingerprint.",
        );
    } else {
        match RepoPath::parse(&receipt.review_artifact_path) {
            Ok(review_artifact_path) => {
                let review_artifact_abs = repo_root.join(review_artifact_path.as_str());
                match fs::read(&review_artifact_abs) {
                    Ok(bytes) => {
                        if sha256_hex(&bytes) != receipt.review_artifact_fingerprint {
                            push_diagnostic(
                                &mut diagnostics,
                                &mut reason_codes,
                                "plan_fidelity_review_artifact_fingerprint_mismatch",
                                "Plan-fidelity receipt review artifact fingerprint does not match the current artifact contents.",
                            );
                        }
                    }
                    Err(_) => {
                        push_diagnostic(
                            &mut diagnostics,
                            &mut reason_codes,
                            "plan_fidelity_review_artifact_missing",
                            "Plan-fidelity receipt review artifact is missing or unreadable.",
                        );
                    }
                }
            }
            Err(_) => {
                push_diagnostic(
                    &mut diagnostics,
                    &mut reason_codes,
                    "plan_fidelity_review_artifact_invalid_path",
                    "Plan-fidelity receipt review artifact path must stay repo-relative.",
                );
            }
        }
    }

    let provenance = &receipt.reviewer_provenance;
    let distinct_stages = provenance
        .distinct_from_stages
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let provenance_valid = provenance.review_stage == PLAN_FIDELITY_REVIEW_STAGE
        && matches!(
            provenance.reviewer_source.as_str(),
            "fresh-context-subagent" | "cross-model"
        )
        && !provenance.reviewer_id.trim().is_empty()
        && PLAN_FIDELITY_DISTINCT_STAGES
            .iter()
            .all(|stage| distinct_stages.contains(stage));
    if !provenance_valid {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_receipt_not_independent",
            "Plan-fidelity reviewer provenance must prove the dedicated reviewer stage stayed distinct from writing-plans and plan-eng-review.",
        );
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_reviewer_provenance_invalid",
            "Plan-fidelity receipt reviewer provenance is malformed or not independent.",
        );
    }

    let checked_surfaces = receipt
        .verification
        .checked_surfaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let verified_requirement_ids = receipt
        .verification
        .verified_requirement_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected_requirement_ids = spec
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();
    let verified_requirement_index = checked_surfaces.contains("requirement_index")
        && verified_requirement_ids == expected_requirement_ids;
    let verified_execution_topology = checked_surfaces.contains("execution_topology");
    if !verified_requirement_index {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_receipt_missing_requirement_index_check",
            "Plan-fidelity receipt must prove the reviewer checked the full Requirement Index.",
        );
    }
    if !verified_execution_topology {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_receipt_missing_execution_topology_check",
            "Plan-fidelity receipt must prove the reviewer checked the draft plan's execution-topology claims.",
        );
    }
    if !verified_requirement_index || !verified_execution_topology {
        push_diagnostic(
            &mut diagnostics,
            &mut reason_codes,
            "plan_fidelity_verification_incomplete",
            "Plan-fidelity receipt is missing one or more required verification surfaces.",
        );
    }

    let state = if reason_codes.is_empty() {
        String::from("pass")
    } else if stale_binding {
        String::from("stale")
    } else {
        String::from("invalid")
    };

    PlanFidelityGateReport {
        state,
        receipt_path: receipt_path_string,
        reviewer_stage: provenance.review_stage.clone(),
        provenance_source: provenance.reviewer_source.clone(),
        verified_requirement_index,
        verified_execution_topology,
        reason_codes,
        diagnostics,
    }
}

pub fn parse_plan_source(path: &Path, source: String) -> Result<PlanDocument, DiagnosticError> {
    let workflow_state = parse_required_header(&source, "Workflow State")?;
    let plan_revision = parse_required_header(&source, "Plan Revision")?
        .parse::<u32>()
        .map_err(|_| missing_header("Plan Revision"))?;
    let execution_mode = parse_required_header(&source, "Execution Mode")?;
    let source_spec_path = RepoPath::parse(
        parse_required_header(&source, "Source Spec")?
            .trim_matches('`'),
    )
    .map(|path| path.as_str().to_owned())
    .map_err(|_| missing_header("Source Spec"))?;
    let source_spec_revision = parse_required_header(&source, "Source Spec Revision")?
        .parse::<u32>()
        .map_err(|_| missing_header("Source Spec Revision"))?;
    let last_reviewed_by = parse_required_header(&source, "Last Reviewed By")?;
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
        coverage_matrix,
        tasks,
        source,
    })
}

fn parse_required_header(source: &str, header: &str) -> Result<String, DiagnosticError> {
    headers::parse_required_header(source, header).ok_or_else(|| missing_header(header))
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
    let task_chunks = source
        .split("\n## Task ")
        .skip(1)
        .map(|chunk| format!("## Task {chunk}"))
        .collect::<Vec<_>>();

    task_chunks
        .into_iter()
        .map(|chunk| parse_task_chunk(&chunk))
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
    let task_outcome = parse_scalar_field(&block, "Task Outcome")?;
    let plan_constraints = parse_bullets_after_field(&block, "Plan Constraints");
    let open_questions = parse_scalar_field(&block, "Open Questions")?;
    let files = parse_file_entries(&block)?;
    let steps = parse_steps(&block)?;

    Ok(PlanTask {
        number: number
            .parse::<u32>()
            .map_err(|_| missing_header("Task number"))?,
        title: title.to_owned(),
        spec_coverage,
        task_outcome,
        plan_constraints,
        open_questions,
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

fn parse_bullets_after_field(lines: &[&str], field: &str) -> Vec<String> {
    let target = format!("**{field}:**");
    let mut collecting = false;
    let mut values = Vec::new();
    for line in lines {
        if *line == target {
            collecting = true;
            continue;
        }
        if collecting && line.starts_with("**") {
            break;
        }
        if collecting {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("- ") {
                values.push(value.to_owned());
            }
        }
    }
    values
}

fn parse_file_entries(lines: &[&str]) -> Result<Vec<TaskFileEntry>, DiagnosticError> {
    let mut collecting = false;
    let mut files = Vec::new();

    for line in lines {
        if *line == "**Files:**" {
            collecting = true;
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
            continue;
        };
        let (action, path) = rest.split_once(": ").ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Malformed files block entry: {trimmed}"),
            )
        })?;
        let normalized = RepoPath::parse(path).map_err(|_| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Malformed files block entry: {trimmed}"),
            )
        })?;
        files.push(TaskFileEntry {
            action: action.to_owned(),
            path: normalized.as_str().to_owned(),
        });
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
    if !is_plan_step_prefix(line) {
        return Ok(None);
    }
    let rest = line
        .strip_prefix("- [")
        .expect("step prefix should be present after is_plan_step_prefix");
    let mark = rest
        .chars()
        .next()
        .expect("step mark should be present after is_plan_step_prefix");
    let rest = &rest[mark.len_utf8()..];
    let rest = rest
        .strip_prefix("] **Step ")
        .expect("step body should be present after is_plan_step_prefix");
    let (number, text) = rest.split_once(": ").ok_or_else(|| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Malformed step entry: {line}"),
        )
    })?;
    Ok(Some((
        number
            .parse::<u32>()
            .map_err(|_| missing_header("Step number"))?,
        text.trim_end_matches("**").to_owned(),
    )))
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

fn merge_plan_fidelity_gate(report: &mut AnalyzePlanReport, gate: &PlanFidelityGateReport) {
    if gate.state == "pass" || gate.state == "not_applicable" {
        return;
    }
    report.contract_state = String::from("invalid");
    for code in &gate.reason_codes {
        if !report.reason_codes.iter().any(|existing| existing == code) {
            report.reason_codes.push(code.clone());
        }
    }
    for diagnostic in &gate.diagnostics {
        if report
            .diagnostics
            .iter()
            .any(|existing| existing.code == diagnostic.code)
        {
            continue;
        }
        report.diagnostics.push(diagnostic.clone());
    }
}

pub fn plan_fidelity_receipt_path_for_repo(repo_root: &Path) -> PathBuf {
    let state_dir = featureforge_state_dir();
    let slug_identity = discover_slug_identity(repo_root);
    let branch_root = harness_branch_root(
        &state_dir,
        &slug_identity.repo_slug,
        &slug_identity.branch_name,
    )
    .parent()
    .map(Path::to_path_buf)
    .unwrap_or_else(|| {
        harness_branch_root(
            &state_dir,
            &slug_identity.repo_slug,
            &slug_identity.branch_name,
        )
    });
    branch_root
        .join("workflow")
        .join("plan-fidelity-receipt.json")
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
