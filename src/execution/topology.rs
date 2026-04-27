use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::contracts::plan::{AnalyzePlanReport, PlanDocument};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::execution::harness::{
    ChunkingStrategy, EvaluatorPolicyName, LearnedTopologyGuidance, ResetPolicy,
    TopologySelectionContext,
};
use crate::execution::internal_args::ExecutionTopologyArg;
use crate::execution::state::{ExecutionContext, ExecutionRuntime, current_head_sha};
use crate::execution::transitions::load_authoritative_transition_state;
use crate::git::{canonicalize_repo_root_string, sha256_hex, stored_repo_root_matches_current};
use crate::paths::RepoPath;
use crate::paths::write_atomic as write_atomic_file;

const PREFLIGHT_ACCEPTANCE_DIR: &str = "execution-preflight";
const PREFLIGHT_ACCEPTANCE_FILE: &str = "acceptance-state.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecommendDecisionFlags {
    pub tasks_independent: String,
    pub isolated_agents_available: String,
    pub session_intent: String,
    pub workspace_prepared: String,
    pub same_session_viable: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RecommendOutput {
    pub selected_topology: ExecutionTopologyArg,
    pub recommended_skill: String,
    pub reason: String,
    pub decision_flags: RecommendDecisionFlags,
    pub reason_codes: Vec<String>,
    pub learned_downgrade_reused: bool,
    pub chunking_strategy: ChunkingStrategy,
    pub evaluator_policy: EvaluatorPolicyName,
    pub reset_policy: ResetPolicy,
    pub review_stack: Vec<String>,
    pub policy_reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionTopologyRecommendation {
    pub selected_topology: ExecutionTopologyArg,
    pub recommended_skill: String,
    pub reason: String,
    pub decision_flags: RecommendDecisionFlags,
    pub reason_codes: Vec<String>,
    pub learned_downgrade_reused: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PlanFidelityReviewArtifact {
    pub(crate) path: String,
    pub(crate) review_stage: String,
    pub(crate) review_verdict: String,
    pub(crate) reviewed_plan_path: String,
    pub(crate) reviewed_plan_revision: u32,
    pub(crate) reviewed_plan_fingerprint: String,
    pub(crate) reviewed_spec_path: String,
    pub(crate) reviewed_spec_revision: u32,
    pub(crate) reviewed_spec_fingerprint: String,
    pub(crate) reviewer_source: String,
    pub(crate) reviewer_id: String,
    pub(crate) distinct_from_stages: Vec<String>,
    pub(crate) verified_surfaces: Vec<String>,
    pub(crate) verified_requirement_ids: Vec<String>,
}

pub(crate) fn tasks_are_independent(plan_document: &PlanDocument) -> bool {
    if plan_document.tasks.len() <= 1 {
        return false;
    }
    let mut paths = BTreeSet::new();
    for task in &plan_document.tasks {
        for entry in &task.files {
            if !paths.insert(entry.path.clone()) {
                return false;
            }
        }
    }
    true
}

fn plan_supports_worktree_parallel(report: &AnalyzePlanReport) -> bool {
    report.execution_topology_valid
        && report.parallel_lane_ownership_valid
        && report.parallel_workspace_isolation_valid
        && !report.parallel_worktree_groups.is_empty()
}

fn normalize_isolated_agents_available(value: &str) -> &'static str {
    match value.trim() {
        "available" | "yes" => "yes",
        "unavailable" | "no" => "no",
        _ => "unknown",
    }
}

fn recommended_skill_for_session(context: &TopologySelectionContext) -> String {
    match context.session_intent.as_str() {
        "separate" => String::from("featureforge:executing-plans"),
        _ => String::from("featureforge:subagent-driven-development"),
    }
}

fn current_parallel_blocker_reason_class(
    report: &AnalyzePlanReport,
    context: &TopologySelectionContext,
) -> Option<&'static str> {
    if !plan_supports_worktree_parallel(report) {
        return Some("dependency_mismatch");
    }
    if normalize_isolated_agents_available(context.isolated_agents_available.as_str()) != "yes" {
        return Some("policy_safety_block");
    }
    if context.workspace_prepared != "yes" {
        return Some("workspace_unavailable");
    }
    None
}

fn learned_guidance_matches(
    report: &AnalyzePlanReport,
    context: &TopologySelectionContext,
) -> bool {
    let Some(guidance): Option<&LearnedTopologyGuidance> = context.learned_guidance.as_ref() else {
        return false;
    };
    if guidance.approved_plan_revision != report.plan_revision {
        return false;
    }
    if guidance.execution_context_key.trim().is_empty()
        || context.execution_context_key.trim().is_empty()
    {
        return false;
    }
    guidance.execution_context_key == context.execution_context_key
}

fn learned_guidance_stale_reuse_matches(
    report: &AnalyzePlanReport,
    context: &TopologySelectionContext,
    current_blocker_reason_class: Option<&str>,
) -> bool {
    let Some(current_blocker_reason_class) = current_blocker_reason_class else {
        return false;
    };
    let Some(guidance): Option<&LearnedTopologyGuidance> = context.learned_guidance.as_ref() else {
        return false;
    };
    if guidance.approved_plan_revision != report.plan_revision {
        return false;
    }
    if guidance.execution_context_key.trim().is_empty()
        || context.execution_context_key.trim().is_empty()
    {
        return false;
    }
    guidance.execution_context_key == context.execution_context_key
        && guidance.primary_reason_class.trim() == current_blocker_reason_class
}

pub fn recommend_topology(
    report: &AnalyzePlanReport,
    context: &TopologySelectionContext,
) -> ExecutionTopologyRecommendation {
    let same_session_viable = match (
        context.session_intent.as_str(),
        context.workspace_prepared.as_str(),
    ) {
        ("stay", "yes") => "yes",
        ("separate", _) | (_, "no") => "no",
        _ => "unknown",
    };
    let isolated_agents_available =
        normalize_isolated_agents_available(context.isolated_agents_available.as_str());
    let tasks_independent = if context.tasks_independent {
        "yes"
    } else {
        "no"
    };
    let worktree_parallel_available = plan_supports_worktree_parallel(report)
        && context.tasks_independent
        && isolated_agents_available == "yes"
        && context.workspace_prepared == "yes";
    let current_blocker_reason_class = current_parallel_blocker_reason_class(report, context);
    let learned_guidance_matches = learned_guidance_matches(report, context);
    let learned_guidance_stale_reuse_matches =
        learned_guidance_stale_reuse_matches(report, context, current_blocker_reason_class);
    let learned_downgrade_reused =
        learned_guidance_stale_reuse_matches && !context.current_parallel_path_ready;
    let restored_parallel_path = learned_guidance_matches
        && context.current_parallel_path_ready
        && worktree_parallel_available;

    let (selected_topology, recommended_skill, reason, reason_codes) = if restored_parallel_path {
        (
            ExecutionTopologyArg::WorktreeBackedParallel,
            recommended_skill_for_session(context),
            String::from(
                "Runtime restored the worktree-backed parallel topology because the current run is ready again.",
            ),
            vec![String::from("matching_downgrade_history_superseded")],
        )
    } else if worktree_parallel_available && !learned_downgrade_reused {
        (
            ExecutionTopologyArg::WorktreeBackedParallel,
            recommended_skill_for_session(context),
            String::from(
                "Runtime selected the worktree-backed parallel topology for the current approved plan.",
            ),
            vec![String::from("worktree_backed_parallel_ready")],
        )
    } else if learned_downgrade_reused {
        (
            ExecutionTopologyArg::ConservativeFallback,
            String::from("featureforge:executing-plans"),
            String::from(
                "Runtime reused matching downgrade history and stayed conservative for this run.",
            ),
            vec![String::from("matching_downgrade_history_reused")],
        )
    } else {
        let codes = current_blocker_reason_class
            .map(|reason_class| vec![format!("conservative_fallback_{reason_class}")])
            .unwrap_or_else(|| vec![String::from("conservative_fallback_runtime_unavailable")]);
        (
            ExecutionTopologyArg::ConservativeFallback,
            String::from("featureforge:executing-plans"),
            String::from(
                "Runtime fell back conservatively because the current run does not satisfy worktree-backed parallel readiness.",
            ),
            codes,
        )
    };

    ExecutionTopologyRecommendation {
        selected_topology,
        recommended_skill,
        reason,
        decision_flags: RecommendDecisionFlags {
            tasks_independent: tasks_independent.to_owned(),
            isolated_agents_available: isolated_agents_available.to_owned(),
            session_intent: context.session_intent.clone(),
            workspace_prepared: context.workspace_prepared.clone(),
            same_session_viable: same_session_viable.to_owned(),
        },
        reason_codes,
        learned_downgrade_reused,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PreflightAcceptanceState {
    pub(crate) schema_version: u32,
    pub(crate) plan_path: String,
    pub(crate) plan_revision: u32,
    #[serde(default)]
    pub(crate) repo_root: Option<String>,
    #[serde(default)]
    pub(crate) git_dir: Option<String>,
    #[serde(default)]
    pub(crate) repo_state_baseline_head_sha: Option<String>,
    pub(crate) execution_run_id: crate::execution::harness::ExecutionRunId,
    pub(crate) chunk_id: crate::execution::harness::ChunkId,
    #[serde(default = "default_preflight_chunking_strategy")]
    pub(crate) chunking_strategy: ChunkingStrategy,
    #[serde(default = "default_preflight_evaluator_policy")]
    pub(crate) evaluator_policy: EvaluatorPolicyName,
    #[serde(default = "default_preflight_reset_policy")]
    pub(crate) reset_policy: ResetPolicy,
    #[serde(default = "default_preflight_review_stack")]
    pub(crate) review_stack: Vec<String>,
}

impl PreflightAcceptanceState {
    pub(crate) const SCHEMA_VERSION: u32 = 1;

    pub(crate) fn matches_plan_revision(&self, context: &ExecutionContext) -> bool {
        self.plan_path == context.plan_rel
            && self.plan_revision == context.plan_document.plan_revision
    }

    pub(crate) fn matches_context(&self, context: &ExecutionContext) -> bool {
        let (chunking_strategy, evaluator_policy, reset_policy, review_stack) =
            proposed_preflight_policy_tuple(context);
        if let Some(stored_repo_root) = self
            .repo_root
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && !stored_repo_root_matches_current(stored_repo_root, &context.runtime.repo_root)
        {
            return false;
        }
        if let Some(stored_git_dir) = self
            .git_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && !stored_repo_root_matches_current(stored_git_dir, &context.runtime.git_dir)
        {
            return false;
        }
        let Some(saved_baseline_head_sha) = self
            .repo_state_baseline_head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return false;
        };
        let current_baseline_head_sha = match current_head_sha(&context.runtime.repo_root) {
            Ok(value) => value,
            Err(_) => return false,
        };
        self.plan_path == context.plan_rel
            && self.plan_revision == context.plan_document.plan_revision
            && saved_baseline_head_sha == current_baseline_head_sha
            && self.chunking_strategy == chunking_strategy
            && self.evaluator_policy == evaluator_policy
            && self.reset_policy == reset_policy
            && self.review_stack == review_stack
    }
}

pub(crate) fn proposed_preflight_policy_tuple(
    _context: &ExecutionContext,
) -> (
    ChunkingStrategy,
    EvaluatorPolicyName,
    ResetPolicy,
    Vec<String>,
) {
    (
        default_preflight_chunking_strategy(),
        default_preflight_evaluator_policy(),
        default_preflight_reset_policy(),
        default_preflight_review_stack(),
    )
}

pub(crate) fn default_preflight_chunking_strategy() -> ChunkingStrategy {
    ChunkingStrategy::Task
}

pub(crate) fn default_preflight_evaluator_policy() -> EvaluatorPolicyName {
    EvaluatorPolicyName(String::from("spec_compliance+code_quality"))
}

pub(crate) fn default_preflight_reset_policy() -> ResetPolicy {
    ResetPolicy::ChunkBoundary
}

pub(crate) fn default_preflight_review_stack() -> Vec<String> {
    vec![
        String::from("featureforge:requesting-code-review"),
        String::from("featureforge:qa-only"),
        String::from("featureforge:document-release"),
    ]
}

pub(crate) fn pending_chunk_id(context: &ExecutionContext) -> crate::execution::harness::ChunkId {
    let seed = format!(
        "pending-chunk\n{}\n{}\n",
        context.plan_rel, context.plan_document.plan_revision
    );
    let digest = sha256_hex(seed.as_bytes());
    crate::execution::harness::ChunkId::new(format!("chunk-pending-{}", &digest[..12]))
}

pub(crate) fn require_preflight_acceptance(
    context: &ExecutionContext,
) -> Result<(), crate::diagnostics::JsonFailure> {
    if authoritative_run_identity_present(context)? {
        return Ok(());
    }
    if preflight_acceptance_for_plan_revision(context)?.is_some() {
        return Ok(());
    }
    Err(crate::diagnostics::JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        "begin requires a successful execution_preflight acceptance for this approved plan revision.",
    ))
}

pub(crate) fn preflight_acceptance_for_context(
    context: &ExecutionContext,
) -> Result<Option<PreflightAcceptanceState>, crate::diagnostics::JsonFailure> {
    match load_preflight_acceptance(&context.runtime) {
        Ok(acceptance) => Ok(acceptance.filter(|acceptance| acceptance.matches_context(context))),
        Err(error) => {
            if authoritative_run_identity_present(context)? {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

pub(crate) fn preflight_acceptance_for_plan_revision(
    context: &ExecutionContext,
) -> Result<Option<PreflightAcceptanceState>, crate::diagnostics::JsonFailure> {
    match load_preflight_acceptance(&context.runtime) {
        Ok(acceptance) => {
            Ok(acceptance.filter(|acceptance| acceptance.matches_plan_revision(context)))
        }
        Err(error) => {
            if authoritative_run_identity_present(context)? {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

pub(crate) fn load_preflight_acceptance(
    runtime: &ExecutionRuntime,
) -> Result<Option<PreflightAcceptanceState>, crate::diagnostics::JsonFailure> {
    let path = preflight_acceptance_path(runtime);
    if !path.is_file() {
        return Ok(None);
    }
    let source = fs::read_to_string(&path).map_err(|error| {
        crate::diagnostics::JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Could not read persisted execution preflight acceptance {}: {error}",
                path.display()
            ),
        )
    })?;
    let acceptance: PreflightAcceptanceState = serde_json::from_str(&source).map_err(|error| {
        crate::diagnostics::JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Persisted execution preflight acceptance is malformed in {}: {error}",
                path.display()
            ),
        )
    })?;
    if acceptance.schema_version != PreflightAcceptanceState::SCHEMA_VERSION {
        return Err(crate::diagnostics::JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Persisted execution preflight acceptance schema version is unsupported in {}.",
                path.display()
            ),
        ));
    }
    if acceptance.execution_run_id.as_str().trim().is_empty()
        || acceptance.chunk_id.as_str().trim().is_empty()
    {
        return Err(crate::diagnostics::JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Persisted execution preflight acceptance must include non-empty run and chunk identities in {}.",
                path.display()
            ),
        ));
    }
    Ok(Some(acceptance))
}

pub(crate) fn persist_preflight_acceptance(
    context: &ExecutionContext,
) -> Result<PreflightAcceptanceState, crate::diagnostics::JsonFailure> {
    if let Some(existing) = preflight_acceptance_for_context(context)? {
        return Ok(existing);
    }

    let acceptance = if let Some(seed) = authoritative_preflight_acceptance_seed(context)? {
        seed
    } else {
        new_preflight_acceptance(context)?
    };
    let payload = serde_json::to_string_pretty(&acceptance).map_err(|error| {
        crate::diagnostics::JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not serialize execution preflight acceptance: {error}"),
        )
    })?;
    let path = preflight_acceptance_path(&context.runtime);
    write_atomic_file(&path, payload).map_err(|error| {
        crate::diagnostics::JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not persist execution preflight acceptance {}: {error}",
                path.display()
            ),
        )
    })?;
    Ok(acceptance)
}

fn authoritative_preflight_acceptance_seed(
    context: &ExecutionContext,
) -> Result<Option<PreflightAcceptanceState>, crate::diagnostics::JsonFailure> {
    let Some(authoritative_state) = load_authoritative_transition_state(context)? else {
        return Ok(None);
    };
    let Some(harness_phase) = authoritative_state.harness_phase_opt() else {
        return Ok(None);
    };
    if matches!(
        harness_phase.as_str(),
        "implementation_handoff" | "execution_preflight"
    ) {
        return Ok(None);
    }
    let Some(execution_run_id) = authoritative_state.execution_run_id_opt() else {
        return Ok(None);
    };
    let Some(chunk_id) = authoritative_state.chunk_id_opt() else {
        return Ok(None);
    };
    let baseline_head_sha = current_head_sha(&context.runtime.repo_root)?;
    let (chunking_strategy, evaluator_policy, reset_policy, review_stack) =
        proposed_preflight_policy_tuple(context);
    Ok(Some(PreflightAcceptanceState {
        schema_version: PreflightAcceptanceState::SCHEMA_VERSION,
        plan_path: context.plan_rel.clone(),
        plan_revision: context.plan_document.plan_revision,
        repo_root: Some(canonicalize_repo_root_string(&context.runtime.repo_root)),
        git_dir: Some(canonicalize_repo_root_string(&context.runtime.git_dir)),
        repo_state_baseline_head_sha: Some(baseline_head_sha),
        execution_run_id: crate::execution::harness::ExecutionRunId::new(execution_run_id),
        chunk_id: crate::execution::harness::ChunkId::new(chunk_id),
        chunking_strategy,
        evaluator_policy,
        reset_policy,
        review_stack,
    }))
}

pub(crate) fn new_preflight_acceptance(
    context: &ExecutionContext,
) -> Result<PreflightAcceptanceState, crate::diagnostics::JsonFailure> {
    let baseline_head_sha = current_head_sha(&context.runtime.repo_root)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let seed = format!(
        "execution-preflight-acceptance\n{}\n{}\n{}\n{}\n{}\n{}\n",
        context.runtime.repo_slug,
        context.runtime.branch_name,
        context.plan_rel,
        context.plan_document.plan_revision,
        std::process::id(),
        nonce,
    );
    let digest = sha256_hex(seed.as_bytes());
    Ok(PreflightAcceptanceState {
        schema_version: PreflightAcceptanceState::SCHEMA_VERSION,
        plan_path: context.plan_rel.clone(),
        plan_revision: context.plan_document.plan_revision,
        repo_root: Some(canonicalize_repo_root_string(&context.runtime.repo_root)),
        git_dir: Some(canonicalize_repo_root_string(&context.runtime.git_dir)),
        repo_state_baseline_head_sha: Some(baseline_head_sha),
        execution_run_id: crate::execution::harness::ExecutionRunId::new(format!(
            "run-{}",
            &digest[..16]
        )),
        chunk_id: crate::execution::harness::ChunkId::new(format!("chunk-{}", &digest[16..32])),
        chunking_strategy: default_preflight_chunking_strategy(),
        evaluator_policy: default_preflight_evaluator_policy(),
        reset_policy: default_preflight_reset_policy(),
        review_stack: default_preflight_review_stack(),
    })
}

pub(crate) fn preflight_acceptance_path(runtime: &ExecutionRuntime) -> PathBuf {
    runtime
        .state_dir
        .join("projects")
        .join(&runtime.repo_slug)
        .join("branches")
        .join(&runtime.safe_branch)
        .join(PREFLIGHT_ACCEPTANCE_DIR)
        .join(PREFLIGHT_ACCEPTANCE_FILE)
}

pub(crate) fn authoritative_run_identity_present(
    context: &ExecutionContext,
) -> Result<bool, crate::diagnostics::JsonFailure> {
    Ok(load_authoritative_transition_state(context)?
        .as_ref()
        .and_then(|state| state.execution_run_id_opt())
        .is_some())
}

pub(crate) fn parse_plan_fidelity_review_artifact(
    artifact_path: &Path,
    artifact_path_string: &str,
) -> Result<PlanFidelityReviewArtifact, DiagnosticError> {
    let source = fs::read_to_string(artifact_path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not read plan-fidelity review artifact {}: {error}",
                artifact_path.display()
            ),
        )
    })?;
    if !source.contains("## Plan Fidelity Review Summary") {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            "Plan-fidelity review artifact is missing the `## Plan Fidelity Review Summary` block.",
        ));
    }

    let reviewed_plan_path =
        RepoPath::parse(parse_header_value(&source, "Reviewed Plan")?.trim_matches('`'))
            .map(|path| path.as_str().to_owned())
            .map_err(|_| {
                DiagnosticError::new(
                    FailureClass::InstructionParseFailed,
                    "Plan-fidelity review artifact must keep Reviewed Plan repo-relative.",
                )
            })?;
    let reviewed_plan_revision = parse_header_value(&source, "Reviewed Plan Revision")?
        .parse::<u32>()
        .map_err(|_| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "Plan-fidelity review artifact is missing a numeric Reviewed Plan Revision.",
            )
        })?;
    let reviewed_plan_fingerprint = parse_header_value(&source, "Reviewed Plan Fingerprint")?;
    let reviewed_spec_path =
        RepoPath::parse(parse_header_value(&source, "Reviewed Spec")?.trim_matches('`'))
            .map(|path| path.as_str().to_owned())
            .map_err(|_| {
                DiagnosticError::new(
                    FailureClass::InstructionParseFailed,
                    "Plan-fidelity review artifact must keep Reviewed Spec repo-relative.",
                )
            })?;
    let reviewed_spec_revision = parse_header_value(&source, "Reviewed Spec Revision")?
        .parse::<u32>()
        .map_err(|_| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                "Plan-fidelity review artifact is missing a numeric Reviewed Spec Revision.",
            )
        })?;
    let reviewed_spec_fingerprint = parse_header_value(&source, "Reviewed Spec Fingerprint")?;
    let distinct_from_stages = parse_header_value(&source, "Distinct From Stages")?
        .split(',')
        .map(str::trim)
        .filter(|stage| !stage.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let verified_surfaces = parse_header_value(&source, "Verified Surfaces")?
        .split(',')
        .map(str::trim)
        .filter(|surface| !surface.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let verified_requirement_ids = parse_header_value(&source, "Verified Requirement IDs")?
        .split(',')
        .map(str::trim)
        .filter(|requirement_id| !requirement_id.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();

    Ok(PlanFidelityReviewArtifact {
        path: artifact_path_string.to_owned(),
        review_stage: parse_header_value(&source, "Review Stage")?,
        review_verdict: parse_header_value(&source, "Review Verdict")?,
        reviewed_plan_path,
        reviewed_plan_revision,
        reviewed_plan_fingerprint,
        reviewed_spec_path,
        reviewed_spec_revision,
        reviewed_spec_fingerprint,
        reviewer_source: parse_header_value(&source, "Reviewer Source")?,
        reviewer_id: parse_header_value(&source, "Reviewer ID")?,
        distinct_from_stages,
        verified_surfaces,
        verified_requirement_ids,
    })
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
