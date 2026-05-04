use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use jiff::Timestamp;
use sha2::{Digest, Sha256};

use crate::contracts::task_contract::{
    RuntimeExecutionNoteProjectionBlock, known_runtime_step_projection_lines,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::final_review::{
    authoritative_strategy_checkpoint_fingerprint_checked, parse_artifact_document,
};
use crate::execution::state::{
    ExecutionContext, ExecutionEvidence, ExecutionRuntime, PlanStepState, hash_contract_plan,
    parse_step_line,
};
use crate::execution::transitions::{
    AuthoritativeTransitionState, CurrentBrowserQaRecord, CurrentFinalReviewRecord,
    CurrentReleaseReadinessRecord, load_authoritative_transition_state_relaxed,
};
use crate::paths::{
    atomic_publish_temp_path, harness_authoritative_artifact_path, normalize_repo_relative_path,
};

const REGENERATED_ARTIFACT_GENERATED_AT: &str = "1970-01-01T00:00:00Z";
pub(crate) const PROJECTION_EXPORT_ROOT_REL: &str = "docs/featureforge/projections/";

pub(crate) fn is_projection_export_path(path: &str) -> bool {
    path.starts_with(PROJECTION_EXPORT_ROOT_REL)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionWriteMode {
    StateDirOnly,
    ProjectionExport,
    Disabled,
}

impl ProjectionWriteMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::StateDirOnly => "state_dir_only",
            Self::ProjectionExport => "projection_export",
            Self::Disabled => "disabled",
        }
    }
}

pub(crate) fn normal_projection_write_mode() -> Result<ProjectionWriteMode, JsonFailure> {
    match std::env::var("FEATUREFORGE_PROJECTION_WRITE_MODE") {
        Ok(value) => match value.trim() {
            "" | "state-dir-only" | "state_dir_only" => Ok(ProjectionWriteMode::StateDirOnly),
            "tracked" | "tracked-materialization" | "tracked_materialization" => {
                Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "FEATUREFORGE_PROJECTION_WRITE_MODE=tracked is not valid for normal commands; use `featureforge plan execution materialize-projections` for projection exports.",
                ))
            }
            "projection-export" | "projection_export" => Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "FEATUREFORGE_PROJECTION_WRITE_MODE=projection-export is not valid for normal commands; projection exports are explicit materialize-projections output only.",
            )),
            "disabled" => Ok(ProjectionWriteMode::Disabled),
            _ => Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "FEATUREFORGE_PROJECTION_WRITE_MODE must be state-dir-only or disabled for normal commands.",
            )),
        },
        Err(std::env::VarError::NotPresent) => Ok(ProjectionWriteMode::StateDirOnly),
        Err(std::env::VarError::NotUnicode(_)) => Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "FEATUREFORGE_PROJECTION_WRITE_MODE must be valid unicode.",
        )),
    }
}

pub(crate) struct RenderedExecutionProjections {
    pub(crate) plan: String,
    pub(crate) evidence: String,
}

pub(crate) struct ProjectionReadModelMetadata {
    pub(crate) projection_mode: String,
    pub(crate) state_dir_projection_paths: Vec<String>,
    pub(crate) tracked_projection_paths: Vec<String>,
    pub(crate) tracked_projections_current: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct RenderedProjectionArtifact {
    path: String,
    source: String,
}

#[derive(Clone, Copy)]
enum AuthoritativeProjectionReadMode {
    Strict,
    AllowMissing,
    BestEffort,
}

#[cfg(test)]
impl RenderedProjectionArtifact {
    fn file_name(&self) -> Option<&std::ffi::OsStr> {
        Path::new(&self.path).file_name()
    }

    fn display(&self) -> std::path::Display<'_> {
        Path::new(&self.path).display()
    }
}

#[cfg(test)]
impl AsRef<Path> for RenderedProjectionArtifact {
    fn as_ref(&self) -> &Path {
        Path::new(&self.path)
    }
}

pub(crate) fn render_execution_projections(
    context: &ExecutionContext,
) -> RenderedExecutionProjections {
    let plan = render_plan_projection_source(
        &context.plan_source,
        &context.plan_document.execution_mode,
        &context.steps,
    );
    let plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let evidence = render_evidence_projection_source(EvidenceProjectionInput {
        plan_rel: &context.plan_rel,
        plan_document: &context.plan_document,
        plan_fingerprint: &plan_fingerprint,
        source_spec_fingerprint: &source_spec_fingerprint,
        steps: &context.steps,
        evidence: &context.evidence,
    });
    RenderedExecutionProjections { plan, evidence }
}

pub(crate) fn render_canonical_evidence_projection_source(
    plan_rel: &str,
    plan_document: &crate::contracts::plan::PlanDocument,
    plan_source: &str,
    source_spec_source: &str,
    steps: &[PlanStepState],
    evidence: &ExecutionEvidence,
) -> String {
    let plan_fingerprint = hash_contract_plan(plan_source);
    let source_spec_fingerprint = sha256_hex(source_spec_source.as_bytes());
    render_canonical_evidence_projection_source_with_fingerprints(
        plan_rel,
        plan_document,
        &plan_fingerprint,
        &source_spec_fingerprint,
        steps,
        evidence,
    )
}

pub(crate) fn render_canonical_evidence_projection_source_with_fingerprints(
    plan_rel: &str,
    plan_document: &crate::contracts::plan::PlanDocument,
    plan_fingerprint: &str,
    source_spec_fingerprint: &str,
    steps: &[PlanStepState],
    evidence: &ExecutionEvidence,
) -> String {
    render_evidence_projection_source(EvidenceProjectionInput {
        plan_rel,
        plan_document,
        plan_fingerprint,
        source_spec_fingerprint,
        steps,
        evidence,
    })
}

pub(crate) fn execution_projection_read_model_metadata(
    context: &ExecutionContext,
    mode: ProjectionWriteMode,
) -> Result<ProjectionReadModelMetadata, JsonFailure> {
    let mut state_dir_projection_paths = vec![
        state_dir_projection_path(&context.runtime, &context.plan_rel)?
            .to_string_lossy()
            .into_owned(),
        state_dir_projection_path(&context.runtime, &context.evidence_rel)?
            .to_string_lossy()
            .into_owned(),
    ];
    let plan_export_path = execution_plan_projection_export_rel_path(&context.plan_rel)?;
    let evidence_export_path = execution_evidence_projection_export_rel_path(&context.plan_rel)?;
    let mut tracked_projection_paths = vec![plan_export_path.clone(), evidence_export_path.clone()];
    let rendered = render_execution_projections(context);
    let mut tracked_projections_current = tracked_projection_matches(
        &context.runtime.repo_root.join(&plan_export_path),
        &rendered.plan,
    )? && tracked_projection_matches(
        &context.runtime.repo_root.join(&evidence_export_path),
        &rendered.evidence,
    )?;
    if let Some(authoritative_state) = load_authoritative_transition_state_relaxed(context)? {
        let late_state_dir = late_stage_projection_artifact_read_models(
            &context.runtime,
            context,
            &authoritative_state,
            ProjectionWriteMode::StateDirOnly,
            false,
        )?;
        state_dir_projection_paths.extend(late_state_dir.into_iter().map(|artifact| artifact.path));
        let late_tracked = late_stage_projection_artifact_read_models(
            &context.runtime,
            context,
            &authoritative_state,
            ProjectionWriteMode::ProjectionExport,
            false,
        )?;
        for artifact in late_tracked {
            let tracked_path = context.runtime.repo_root.join(&artifact.path);
            tracked_projections_current &=
                tracked_projection_matches(&tracked_path, &artifact.source)?;
            tracked_projection_paths.push(artifact.path);
        }
    }
    Ok(ProjectionReadModelMetadata {
        projection_mode: mode.as_str().to_owned(),
        state_dir_projection_paths,
        tracked_projection_paths,
        tracked_projections_current,
    })
}

pub(crate) fn write_execution_projection_read_models(
    context: &ExecutionContext,
    rendered: &RenderedExecutionProjections,
    mode: ProjectionWriteMode,
) -> Result<Vec<String>, JsonFailure> {
    let mut written = Vec::new();
    match mode {
        ProjectionWriteMode::StateDirOnly => {
            let plan_path =
                write_state_dir_projection(&context.runtime, &context.plan_rel, &rendered.plan)?;
            written.push(plan_path.to_string_lossy().into_owned());
            let evidence_path = write_state_dir_projection(
                &context.runtime,
                &context.evidence_rel,
                &rendered.evidence,
            )?;
            written.push(evidence_path.to_string_lossy().into_owned());
        }
        ProjectionWriteMode::ProjectionExport => {
            let plan_path = write_execution_projection_export(
                &context.runtime,
                &context.plan_rel,
                "execution-plan.md",
                &rendered.plan,
            )?;
            written.push(plan_path);
            let evidence_path = write_execution_projection_export(
                &context.runtime,
                &context.plan_rel,
                "execution-evidence.md",
                &rendered.evidence,
            )?;
            written.push(evidence_path);
        }
        ProjectionWriteMode::Disabled => {}
    }
    Ok(written)
}

pub(crate) struct BranchClosureProjectionInput<'a> {
    pub(crate) contract_identity: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) effective_reviewed_branch_surface: &'a str,
    pub(crate) source_task_closure_ids: &'a [String],
    pub(crate) provenance_basis: &'a str,
    pub(crate) superseded_branch_closure_ids: &'a [String],
}

pub(crate) struct FinalReviewProjectionInput<'a> {
    pub(crate) dispatch_id: &'a str,
    pub(crate) reviewer_source: &'a str,
    pub(crate) reviewer_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) deviations_required: bool,
    pub(crate) summary: &'a str,
}

struct FinalReviewRenderOverrides<'a> {
    generated_at: Option<&'a str>,
    reviewer_artifact_path: Option<&'a Path>,
}

pub(crate) struct RenderedFinalReviewArtifacts {
    #[cfg(test)]
    pub(crate) reviewer_artifact_path: PathBuf,
    pub(crate) reviewer_source_text: String,
    pub(crate) final_review_source: String,
}

pub(crate) struct QaProjectionInput<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) summary: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) test_plan_path: Option<&'a Path>,
}

pub(crate) fn render_branch_closure_artifact(
    context: &ExecutionContext,
    branch_closure_id: &str,
    projection: BranchClosureProjectionInput<'_>,
) -> Result<String, JsonFailure> {
    render_branch_closure_artifact_with_generated_at(context, branch_closure_id, projection, None)
}

fn render_branch_closure_artifact_with_generated_at(
    context: &ExecutionContext,
    branch_closure_id: &str,
    projection: BranchClosureProjectionInput<'_>,
    generated_at_override: Option<&str>,
) -> Result<String, JsonFailure> {
    let current_head = context.current_head_sha()?;
    let generated_at = generated_at_override
        .map(str::to_owned)
        .unwrap_or_else(|| Timestamp::now().to_string());
    let source_task_closure_ids = if projection.source_task_closure_ids.is_empty() {
        String::from("none")
    } else {
        projection.source_task_closure_ids.join(", ")
    };
    let superseded_branch_closure_ids = if projection.superseded_branch_closure_ids.is_empty() {
        String::from("none")
    } else {
        projection.superseded_branch_closure_ids.join(", ")
    };
    Ok(format!(
        "# Branch Closure Result\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Contract Identity:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed State ID:** {}\n**Effective Reviewed Branch Surface:** {}\n**Source Task Closure IDs:** {}\n**Provenance Basis:** {}\n**Closure Status:** current\n**Superseded Branch Closure IDs:** {}\n**Branch Closure ID:** {}\n**Generated By:** featureforge:advance-late-stage\n**Generated At:** {generated_at}\n\n## Summary\n- current reviewed branch state recorded for late-stage binding.\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        projection.contract_identity,
        context.runtime.branch_name,
        context.runtime.repo_slug,
        projection.base_branch,
        current_head,
        projection.reviewed_state_id,
        projection.effective_reviewed_branch_surface,
        source_task_closure_ids,
        projection.provenance_basis,
        superseded_branch_closure_ids,
        branch_closure_id
    ))
}

pub(crate) fn render_release_readiness_artifact(
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    base_branch: &str,
    result: &str,
    summary: &str,
) -> Result<String, JsonFailure> {
    render_release_readiness_artifact_with_generated_at(
        context,
        branch_closure_id,
        reviewed_state_id,
        base_branch,
        result,
        summary,
        None,
    )
}

fn render_release_readiness_artifact_with_generated_at(
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    base_branch: &str,
    result: &str,
    summary: &str,
    generated_at_override: Option<&str>,
) -> Result<String, JsonFailure> {
    let base_branch = base_branch.trim();
    if base_branch.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::ReleaseArtifactNotFresh,
            "advance-late-stage release-readiness requires a non-empty bound base branch.",
        ));
    }
    let current_head = context.current_head_sha()?;
    let generated_at = generated_at_override
        .map(str::to_owned)
        .unwrap_or_else(|| Timestamp::now().to_string());
    let artifact_result = if result == "ready" { "pass" } else { "blocked" };
    Ok(format!(
        "# Release Readiness Result\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Result:** {}\n**Generated By:** featureforge:document-release\n**Generated At:** {generated_at}\n\n## Summary\n- {}\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        context.runtime.branch_name,
        context.runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        artifact_result,
        summary
    ))
}

pub(crate) fn render_final_review_artifacts(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    base_branch: &str,
    inputs: FinalReviewProjectionInput<'_>,
) -> Result<RenderedFinalReviewArtifacts, JsonFailure> {
    render_final_review_artifacts_with_generated_at(
        runtime,
        context,
        branch_closure_id,
        reviewed_state_id,
        base_branch,
        inputs,
        FinalReviewRenderOverrides {
            generated_at: None,
            reviewer_artifact_path: None,
        },
    )
}

fn render_final_review_artifacts_with_generated_at(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    base_branch: &str,
    inputs: FinalReviewProjectionInput<'_>,
    overrides: FinalReviewRenderOverrides<'_>,
) -> Result<RenderedFinalReviewArtifacts, JsonFailure> {
    let current_head = context.current_head_sha()?;
    let strategy_checkpoint_fingerprint =
        authoritative_strategy_checkpoint_fingerprint_checked(context)?.unwrap_or_default();
    let generated_at = overrides
        .generated_at
        .map(str::to_owned)
        .unwrap_or_else(|| Timestamp::now().to_string());
    let recorded_execution_deviations = if inputs.deviations_required {
        "present"
    } else {
        "none"
    };
    let deviation_review_verdict = if inputs.deviations_required {
        "pass"
    } else {
        "not_required"
    };
    let reviewer_artifact_path = overrides
        .reviewer_artifact_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            project_artifact_dir(runtime).join(format!(
                "featureforge-{}-independent-review-{}.md",
                runtime.safe_branch,
                timestamp_slug()
            ))
        });
    let reviewer_source_text = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {}\n**Reviewer ID:** {}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** {recorded_execution_deviations}\n**Deviation Review Verdict:** {deviation_review_verdict}\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Result:** {}\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** {generated_at}\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n- {}\n",
        inputs.reviewer_source,
        inputs.reviewer_id,
        context.plan_rel,
        context.plan_document.plan_revision,
        context.runtime.branch_name,
        runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        inputs.result,
        inputs.summary
    );
    let reviewer_artifact_fingerprint = sha256_hex(reviewer_source_text.as_bytes());
    let final_review_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** {}\n**Reviewer ID:** {}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Recorded Execution Deviations:** {recorded_execution_deviations}\n**Deviation Review Verdict:** {deviation_review_verdict}\n**Dispatch ID:** {}\n**Result:** {}\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** {generated_at}\n\n## Summary\n- {}\n",
        inputs.reviewer_source,
        inputs.reviewer_id,
        reviewer_artifact_path.display(),
        reviewer_artifact_fingerprint,
        context.plan_rel,
        context.plan_document.plan_revision,
        context.runtime.branch_name,
        runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        inputs.dispatch_id,
        inputs.result,
        inputs.summary
    );
    Ok(RenderedFinalReviewArtifacts {
        #[cfg(test)]
        reviewer_artifact_path,
        reviewer_source_text,
        final_review_source,
    })
}

pub(crate) fn render_qa_artifact(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    inputs: QaProjectionInput<'_>,
) -> Result<String, JsonFailure> {
    render_qa_artifact_with_generated_at(runtime, context, inputs, None)
}

fn render_qa_artifact_with_generated_at(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    inputs: QaProjectionInput<'_>,
    generated_at_override: Option<&str>,
) -> Result<String, JsonFailure> {
    let QaProjectionInput {
        branch_closure_id,
        reviewed_state_id,
        result,
        summary,
        base_branch,
        test_plan_path,
    } = inputs;
    let current_head = context.current_head_sha()?;
    let generated_at = generated_at_override
        .map(str::to_owned)
        .unwrap_or_else(|| Timestamp::now().to_string());
    let source_test_plan_header = test_plan_path.map_or(String::new(), |path| {
        format!("**Source Test Plan:** `{}`\n", path.display())
    });
    Ok(format!(
        "# QA Result\n**Source Plan:** `{}`\n**Source Plan Revision:** {}\n{}**Branch:** {}\n**Repo:** {}\n**Base Branch:** {}\n**Head SHA:** {}\n**Current Reviewed Branch State ID:** {}\n**Branch Closure ID:** {}\n**Result:** {}\n**Generated By:** featureforge/qa\n**Generated At:** {generated_at}\n\n## Summary\n- {}\n",
        context.plan_rel,
        context.plan_document.plan_revision,
        source_test_plan_header,
        context.runtime.branch_name,
        runtime.repo_slug,
        base_branch,
        current_head,
        reviewed_state_id,
        branch_closure_id,
        result,
        summary
    ))
}

fn render_plan_projection_source(
    original_source: &str,
    execution_mode: &str,
    steps: &[PlanStepState],
) -> String {
    let known_runtime_steps = known_runtime_step_projection_lines(original_source);
    let step_map = steps
        .iter()
        .map(|step| ((step.task_number, step.step_number), step))
        .collect::<std::collections::BTreeMap<_, _>>();
    let lines = original_source.lines().collect::<Vec<_>>();
    let mut rendered = Vec::new();
    let mut current_task = None::<u32>;
    let mut current_task_files_seen = false;
    let mut in_fenced_block = false;
    let mut pending_note_after_step = false;
    let mut suppress_note_block = None::<RuntimeExecutionNoteProjectionBlock>;

    for line in lines {
        if let Some(note_block) = suppress_note_block {
            if note_block.continues(line) {
                continue;
            }
            suppress_note_block = None;
        }
        if pending_note_after_step {
            if line.trim().is_empty() {
                continue;
            }
            pending_note_after_step = false;
            if let Some(note_block) = RuntimeExecutionNoteProjectionBlock::start(line) {
                suppress_note_block = Some(note_block);
                continue;
            }
        }

        if line.starts_with("**Execution Mode:** ") {
            rendered.push(format!("**Execution Mode:** {execution_mode}"));
            continue;
        }

        if let Some(rest) = line.strip_prefix("## Task ") {
            current_task = rest
                .split(':')
                .next()
                .and_then(|value| value.parse::<u32>().ok());
            current_task_files_seen = false;
            in_fenced_block = false;
            rendered.push(line.to_owned());
            continue;
        }

        if line.starts_with("## ") {
            current_task = None;
            current_task_files_seen = false;
            in_fenced_block = false;
        }
        let trimmed = line.trim();
        if trimmed == "**Files:**" {
            current_task_files_seen = true;
        }
        if trimmed.starts_with("```") {
            in_fenced_block = !in_fenced_block;
            rendered.push(line.to_owned());
            continue;
        }

        if let Some((_, step_number, title)) = parse_step_line(line)
            && let Some(task_number) = current_task
            && !in_fenced_block
            && current_task_files_seen
            && known_runtime_steps
                .get(&(task_number, step_number))
                .is_some_and(|known_title| known_title == &title)
            && let Some(step) = step_map.get(&(task_number, step_number))
        {
            let mark = if step.checked { 'x' } else { ' ' };
            rendered.push(format!(
                "- [{mark}] **Step {}: {}**",
                step.step_number, step.title
            ));
            if let Some(note_state) = step.note_state {
                rendered.push(String::new());
                rendered.push(format!(
                    "  **Execution Note:** {} - {}",
                    note_state.as_str(),
                    step.note_summary
                ));
            }
            pending_note_after_step = true;
            continue;
        }

        rendered.push(line.to_owned());
    }

    format!("{}\n", rendered.join("\n"))
}

struct EvidenceProjectionInput<'a> {
    plan_rel: &'a str,
    plan_document: &'a crate::contracts::plan::PlanDocument,
    plan_fingerprint: &'a str,
    source_spec_fingerprint: &'a str,
    steps: &'a [PlanStepState],
    evidence: &'a ExecutionEvidence,
}

fn render_evidence_projection_source(input: EvidenceProjectionInput<'_>) -> String {
    let mut output = Vec::new();
    let topic = Path::new(input.plan_rel)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("plan");
    output.push(format!("# Execution Evidence: {topic}"));
    output.push(String::new());
    output.push(format!("**Plan Path:** {}", input.plan_rel));
    output.push(format!(
        "**Plan Revision:** {}",
        input.plan_document.plan_revision
    ));
    output.push(format!("**Plan Fingerprint:** {}", input.plan_fingerprint));
    output.push(format!(
        "**Source Spec Path:** {}",
        input.plan_document.source_spec_path
    ));
    output.push(format!(
        "**Source Spec Revision:** {}",
        input.plan_document.source_spec_revision
    ));
    output.push(format!(
        "**Source Spec Fingerprint:** {}",
        input.source_spec_fingerprint
    ));
    output.push(String::new());
    output.push(String::from("## Step Evidence"));

    for step in input.steps {
        let attempts = input
            .evidence
            .attempts
            .iter()
            .filter(|attempt| {
                attempt.task_number == step.task_number && attempt.step_number == step.step_number
            })
            .collect::<Vec<_>>();
        if attempts.is_empty() {
            continue;
        }
        output.push(String::new());
        output.push(format!(
            "### Task {} Step {}",
            step.task_number, step.step_number
        ));
        for (index, attempt) in attempts.iter().enumerate() {
            if index > 0 {
                output.push(String::new());
            }
            output.push(format!("#### Attempt {}", attempt.attempt_number));
            output.push(format!("**Status:** {}", attempt.status));
            output.push(format!("**Recorded At:** {}", attempt.recorded_at));
            output.push(format!(
                "**Execution Source:** {}",
                attempt.execution_source
            ));
            output.push(format!("**Task Number:** {}", attempt.task_number));
            output.push(format!("**Step Number:** {}", attempt.step_number));
            output.push(format!(
                "**Packet Fingerprint:** {}",
                attempt
                    .packet_fingerprint
                    .clone()
                    .unwrap_or_else(|| String::from("unknown"))
            ));
            output.push(format!(
                "**Head SHA:** {}",
                attempt
                    .head_sha
                    .clone()
                    .unwrap_or_else(|| String::from("unknown"))
            ));
            if let Some(base_sha) = &attempt.base_sha {
                output.push(format!("**Base SHA:** {base_sha}"));
            }
            output.push(format!("**Claim:** {}", attempt.claim));
            if let Some(source_contract_path) = &attempt.source_contract_path {
                output.push(format!("**Source Contract Path:** {source_contract_path}"));
            }
            if let Some(source_contract_fingerprint) = &attempt.source_contract_fingerprint {
                output.push(format!(
                    "**Source Contract Fingerprint:** `{source_contract_fingerprint}`"
                ));
            }
            if let Some(source_evaluation_report_fingerprint) =
                &attempt.source_evaluation_report_fingerprint
            {
                output.push(format!(
                    "**Source Evaluation Report Fingerprint:** `{source_evaluation_report_fingerprint}`"
                ));
            }
            if let Some(evaluator_verdict) = &attempt.evaluator_verdict {
                output.push(format!("**Evaluator Verdict:** {evaluator_verdict}"));
            }
            if !attempt.failing_criterion_ids.is_empty() {
                output.push(String::from("**Failing Criterion IDs:**"));
                for criterion_id in &attempt.failing_criterion_ids {
                    output.push(format!("- `{criterion_id}`"));
                }
            }
            if let Some(source_handoff_fingerprint) = &attempt.source_handoff_fingerprint {
                output.push(format!(
                    "**Source Handoff Fingerprint:** `{source_handoff_fingerprint}`"
                ));
            }
            if let Some(repo_state_baseline_head_sha) = &attempt.repo_state_baseline_head_sha {
                output.push(format!(
                    "**Repo State Baseline Head SHA:** {repo_state_baseline_head_sha}"
                ));
            }
            if let Some(repo_state_baseline_worktree_fingerprint) =
                &attempt.repo_state_baseline_worktree_fingerprint
            {
                output.push(format!(
                    "**Repo State Baseline Worktree Fingerprint:** {repo_state_baseline_worktree_fingerprint}"
                ));
            }
            output.push(String::from("**Files Proven:**"));
            for proof in &attempt.file_proofs {
                output.push(format!("- {} | {}", proof.path, proof.proof));
            }
            if let Some(verify_command) = &attempt.verify_command {
                output.push(format!("**Verify Command:** {verify_command}"));
            }
            output.push(format!(
                "**Verification Summary:** {}",
                attempt.verification_summary
            ));
            output.push(format!(
                "**Invalidation Reason:** {}",
                attempt.invalidation_reason
            ));
        }
    }

    format!("{}\n", output.join("\n"))
}

pub(crate) fn materialize_late_stage_projection_artifacts(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    mode: ProjectionWriteMode,
) -> Result<Vec<String>, JsonFailure> {
    Ok(late_stage_projection_artifact_read_models(
        runtime,
        context,
        authoritative_state,
        mode,
        true,
    )?
    .into_iter()
    .map(|artifact| artifact.path)
    .collect())
}

fn late_stage_projection_artifact_read_models(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    mode: ProjectionWriteMode,
    write: bool,
) -> Result<Vec<RenderedProjectionArtifact>, JsonFailure> {
    let mut regenerated = Vec::new();
    if mode == ProjectionWriteMode::Disabled {
        return Ok(regenerated);
    }
    if let Some(identity) = authoritative_state.bound_current_branch_closure_identity()
        && let Some(record) = authoritative_state.branch_closure_record(&identity.branch_closure_id)
    {
        regenerated.push(regenerate_branch_closure_projection(
            runtime,
            context,
            &identity.branch_closure_id,
            &identity.reviewed_state_id,
            &record,
            mode,
            write,
        )?);
    }
    if let Some(record_id) = authoritative_state.current_release_readiness_record_id()
        && let Some(record) = authoritative_state.current_release_readiness_record()
    {
        regenerated.push(regenerate_release_readiness_projection(
            runtime, context, &record_id, &record, mode, write,
        )?);
    }
    if let Some(record_id) = authoritative_state.current_final_review_record_id()
        && let Some(record) = authoritative_state.current_final_review_record()
    {
        regenerated.extend(regenerate_final_review_projection(
            runtime, context, &record_id, &record, mode, write,
        )?);
    }
    if let Some(record_id) = authoritative_state.current_qa_record_id()
        && let Some(record) = authoritative_state.current_browser_qa_record()
    {
        regenerated.extend(regenerate_browser_qa_projection(
            runtime, context, &record_id, &record, mode, write,
        )?);
    }
    Ok(regenerated)
}

pub(crate) fn project_artifact_dir(runtime: &ExecutionRuntime) -> PathBuf {
    runtime.state_dir.join("projects").join(&runtime.repo_slug)
}

pub(crate) fn state_dir_projection_path(
    runtime: &ExecutionRuntime,
    repo_relative_path: &str,
) -> Result<PathBuf, JsonFailure> {
    let relative_path = projection_repo_relative_path(repo_relative_path)?;
    Ok(project_artifact_dir(runtime)
        .join("tracked-projections")
        .join(&runtime.safe_branch)
        .join(relative_path))
}

fn execution_projection_export_plan_stem(plan_rel: &str) -> Result<String, JsonFailure> {
    projection_repo_relative_path(plan_rel)?
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|stem| !stem.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!("Projection export plan path must have a file stem, got {plan_rel}."),
            )
        })
}

fn execution_projection_export_rel_path(
    plan_rel: &str,
    file_name: &str,
) -> Result<String, JsonFailure> {
    let stem = execution_projection_export_plan_stem(plan_rel)?;
    normalize_repo_relative_path(
        &Path::new(PROJECTION_EXPORT_ROOT_REL)
            .join(stem)
            .join(file_name)
            .to_string_lossy(),
    )
    .map_err(JsonFailure::from)
}

fn execution_plan_projection_export_rel_path(plan_rel: &str) -> Result<String, JsonFailure> {
    execution_projection_export_rel_path(plan_rel, "execution-plan.md")
}

fn execution_evidence_projection_export_rel_path(plan_rel: &str) -> Result<String, JsonFailure> {
    execution_projection_export_rel_path(plan_rel, "execution-evidence.md")
}

fn write_execution_projection_export(
    runtime: &ExecutionRuntime,
    plan_rel: &str,
    file_name: &str,
    source: &str,
) -> Result<String, JsonFailure> {
    let rel_path = execution_projection_export_rel_path(plan_rel, file_name)?;
    let base_rel = Path::new(&rel_path).parent().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Projection export path {rel_path} has no parent directory."),
        )
    })?;
    let base = runtime.repo_root.join(base_rel);
    let target = normalize_project_artifact_target(&base, Path::new(file_name))?;
    write_runtime_owned_project_artifact(
        &base,
        &target,
        source,
        "Could not write projection export",
    )?;
    Ok(rel_path)
}

pub(crate) fn read_state_dir_projection(
    runtime: &ExecutionRuntime,
    repo_relative_path: &str,
) -> Result<Option<String>, JsonFailure> {
    let path = state_dir_projection_path(runtime, repo_relative_path)?;
    match fs::read_to_string(&path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not read state-dir projection {}: {error}",
                path.display()
            ),
        )),
    }
}

pub(crate) fn state_dir_projection_matches_recorded_output_fingerprint(
    runtime: &ExecutionRuntime,
    repo_relative_path: &str,
    source: &str,
) -> Result<bool, JsonFailure> {
    let fingerprint_path = state_dir_projection_fingerprint_path(runtime, repo_relative_path)?;
    let fingerprint = match fs::read_to_string(&fingerprint_path) {
        Ok(fingerprint) => fingerprint,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not read state-dir projection fingerprint {}: {error}",
                    fingerprint_path.display()
                ),
            ));
        }
    };
    Ok(projection_source_matches_fingerprint(
        source,
        fingerprint.trim(),
    ))
}

pub(crate) fn projection_source_matches_fingerprint(source: &str, expected: &str) -> bool {
    if sha256_hex(source.as_bytes()) == expected {
        return true;
    }
    let normalized = projection_source_without_html_comments(source);
    normalized.as_deref().is_some_and(|source| {
        let canonical = projection_source_with_single_trailing_newline(source);
        sha256_hex(canonical.as_bytes()) == expected
    })
}

fn projection_source_without_html_comments(source: &str) -> Option<String> {
    let mut rest = source;
    let mut normalized = String::with_capacity(source.len());
    let mut removed_comment = false;
    while let Some(start) = rest.find("<!--") {
        normalized.push_str(&rest[..start]);
        let comment_rest = &rest[start + 4..];
        let Some(end) = comment_rest.find("-->") else {
            normalized.push_str(&rest[start..]);
            return if removed_comment {
                Some(normalized)
            } else {
                None
            };
        };
        rest = &comment_rest[end + 3..];
        removed_comment = true;
    }
    if !removed_comment {
        return None;
    }
    normalized.push_str(rest);
    Some(normalized)
}

fn projection_source_with_single_trailing_newline(source: &str) -> String {
    let mut source = source.trim_end().to_owned();
    source.push('\n');
    source
}

pub(crate) fn write_state_dir_projection(
    runtime: &ExecutionRuntime,
    repo_relative_path: &str,
    source: &str,
) -> Result<PathBuf, JsonFailure> {
    let path = state_dir_projection_path(runtime, repo_relative_path)?;
    write_atomic(&path, source, "Could not write state-dir projection")?;
    let fingerprint_path = state_dir_projection_fingerprint_path(runtime, repo_relative_path)?;
    write_atomic(
        &fingerprint_path,
        &format!("{}\n", sha256_hex(source.as_bytes())),
        "Could not write state-dir projection fingerprint",
    )?;
    Ok(path)
}

fn state_dir_projection_fingerprint_path(
    runtime: &ExecutionRuntime,
    repo_relative_path: &str,
) -> Result<PathBuf, JsonFailure> {
    let path = state_dir_projection_path(runtime, repo_relative_path)?;
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "State-dir projection path must have a file name, got {}.",
                    path.display()
                ),
            )
        })?;
    Ok(path.with_file_name(format!("{file_name}.sha256")))
}

fn tracked_projection_matches(path: &Path, expected_source: &str) -> Result<bool, JsonFailure> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(source == expected_source),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Could not read tracked projection {}: {error}",
                path.display()
            ),
        )),
    }
}

fn projection_repo_relative_path(repo_relative_path: &str) -> Result<PathBuf, JsonFailure> {
    normalize_repo_relative_path(repo_relative_path)
        .map(PathBuf::from)
        .map_err(|_| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "State-dir projection path must be repo-relative, got {repo_relative_path}."
                ),
            )
        })
}

pub(crate) fn write_project_artifact(
    runtime: &ExecutionRuntime,
    artifact_name: &str,
    source: &str,
) -> Result<(), JsonFailure> {
    let dir = project_artifact_dir(runtime);
    let target = normalize_project_artifact_target(&dir, Path::new(artifact_name))?;
    write_runtime_owned_project_artifact(&dir, &target, source, "Could not write project artifact")
}

pub(crate) fn write_project_artifact_at_path(
    runtime: &ExecutionRuntime,
    artifact_path: &Path,
    source: &str,
) -> Result<(), JsonFailure> {
    let dir = project_artifact_dir(runtime);
    let target = normalize_project_artifact_target(&dir, artifact_path)?;
    write_runtime_owned_project_artifact(&dir, &target, source, "Could not write project artifact")
}

fn write_runtime_owned_project_artifact(
    project_dir: &Path,
    target: &Path,
    source: &str,
    message_prefix: &str,
) -> Result<(), JsonFailure> {
    fs::create_dir_all(project_dir).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not create project artifact directory {}: {error}",
                project_dir.display()
            ),
        )
    })?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!(
                    "Could not create project artifact directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    // Re-validate runtime-owned scope after directory creation and immediately before publish.
    enforce_runtime_owned_project_target(project_dir, target)?;
    ensure_existing_project_segments_not_symlink(project_dir, target)?;
    let temp_path = atomic_publish_temp_path(target);
    ensure_existing_project_segments_not_symlink(project_dir, &temp_path)?;
    fs::write(&temp_path, source).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("{message_prefix} {}: {error}", temp_path.display()),
        )
    })?;
    // Validate again to narrow check-then-use windows before final rename.
    if let Err(error) = enforce_runtime_owned_project_target(project_dir, target) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Err(error) = ensure_existing_project_segments_not_symlink(project_dir, target) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Err(error) = ensure_existing_project_segments_not_symlink(project_dir, &temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    match fs::rename(&temp_path, target) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!("{message_prefix} {}: {error}", target.display()),
            ))
        }
    }
}

fn normalize_project_artifact_target(
    project_dir: &Path,
    artifact_path: &Path,
) -> Result<PathBuf, JsonFailure> {
    let joined_target = if artifact_path.is_absolute() {
        artifact_path.to_path_buf()
    } else {
        project_dir.join(artifact_path)
    };
    let mut normalized_target = PathBuf::new();
    for component in joined_target.components() {
        match component {
            Component::Prefix(value) => normalized_target.push(value.as_os_str()),
            Component::RootDir => normalized_target.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(value) => normalized_target.push(value),
            Component::ParentDir => {
                if !normalized_target.pop() {
                    return Err(JsonFailure::new(
                        FailureClass::StaleProvenance,
                        format!(
                            "Projection regeneration refused non-runtime-owned reviewer projection path {}.",
                            joined_target.display()
                        ),
                    ));
                }
            }
        }
    }
    if !normalized_target.starts_with(project_dir) {
        return Err(JsonFailure::new(
            FailureClass::StaleProvenance,
            format!(
                "Projection regeneration refused non-runtime-owned reviewer projection path {}.",
                normalized_target.display()
            ),
        ));
    }
    enforce_runtime_owned_project_target(project_dir, &normalized_target)?;
    Ok(normalized_target)
}

fn enforce_runtime_owned_project_target(
    project_dir: &Path,
    normalized_target: &Path,
) -> Result<(), JsonFailure> {
    let canonical_scope_anchor = canonicalize_existing_ancestor(project_dir)?;
    let canonical_target_anchor = canonicalize_existing_ancestor(normalized_target)?;
    if !canonical_target_anchor.starts_with(&canonical_scope_anchor) {
        return Err(JsonFailure::new(
            FailureClass::StaleProvenance,
            format!(
                "Projection regeneration refused non-runtime-owned reviewer projection path {}.",
                normalized_target.display()
            ),
        ));
    }
    ensure_existing_project_segments_not_symlink(project_dir, normalized_target)?;
    Ok(())
}

fn canonicalize_existing_ancestor(path: &Path) -> Result<PathBuf, JsonFailure> {
    let mut cursor = path;
    loop {
        match fs::canonicalize(cursor) {
            Ok(canonical) => return Ok(canonical),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let Some(parent) = cursor.parent() else {
                    return Err(JsonFailure::new(
                        FailureClass::StaleProvenance,
                        format!(
                            "Projection regeneration refused non-runtime-owned reviewer projection path {}.",
                            path.display()
                        ),
                    ));
                };
                cursor = parent;
            }
            Err(error) => {
                return Err(JsonFailure::new(
                    FailureClass::StaleProvenance,
                    format!(
                        "Projection regeneration could not validate runtime-owned reviewer projection path {}: {}",
                        cursor.display(),
                        error
                    ),
                ));
            }
        }
    }
}

fn ensure_existing_project_segments_not_symlink(
    project_dir: &Path,
    normalized_target: &Path,
) -> Result<(), JsonFailure> {
    let relative = normalized_target.strip_prefix(project_dir).map_err(|_| {
        JsonFailure::new(
            FailureClass::StaleProvenance,
            format!(
                "Projection regeneration refused non-runtime-owned reviewer projection path {}.",
                normalized_target.display()
            ),
        )
    })?;
    let mut cursor = project_dir.to_path_buf();
    if let Ok(metadata) = fs::symlink_metadata(&cursor)
        && metadata.file_type().is_symlink()
    {
        return Err(JsonFailure::new(
            FailureClass::StaleProvenance,
            format!(
                "Projection regeneration refused reviewer projection path {} because runtime-owned project scope contains a symlinked segment.",
                cursor.display()
            ),
        ));
    }
    for component in relative.components() {
        cursor.push(component.as_os_str());
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(JsonFailure::new(
                    FailureClass::StaleProvenance,
                    format!(
                        "Projection regeneration refused reviewer projection path {} because runtime-owned project scope contains a symlinked segment.",
                        cursor.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => break,
            Err(error) => {
                return Err(JsonFailure::new(
                    FailureClass::StaleProvenance,
                    format!(
                        "Projection regeneration could not validate runtime-owned reviewer projection path {}: {}",
                        cursor.display(),
                        error
                    ),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn publish_authoritative_artifact(
    runtime: &ExecutionRuntime,
    artifact_prefix: &str,
    source: &str,
) -> Result<String, JsonFailure> {
    let fingerprint = sha256_hex(source.as_bytes());
    let path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("{artifact_prefix}-{fingerprint}.md"),
    );
    write_atomic(&path, source, "Could not publish authoritative artifact")?;
    Ok(fingerprint)
}

pub(crate) fn timestamp_slug() -> String {
    Timestamp::now()
        .to_string()
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect()
}

fn normalize_projection_suffix(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    let trimmed = normalized.trim_matches('-');
    if trimmed.is_empty() {
        String::from("projection")
    } else {
        trimmed.to_owned()
    }
}

fn regenerate_branch_closure_projection(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    record: &crate::execution::transitions::BranchClosureRecord,
    mode: ProjectionWriteMode,
    write: bool,
) -> Result<RenderedProjectionArtifact, JsonFailure> {
    let authoritative_source =
        record
            .branch_closure_fingerprint
            .as_deref()
            .and_then(|fingerprint| {
                read_authoritative_projection(
                    runtime,
                    "branch-closure",
                    fingerprint,
                    "branch closure",
                )
                .ok()
            });
    let source = if let Some(source) = authoritative_source {
        source
    } else {
        render_branch_closure_artifact_with_generated_at(
            context,
            branch_closure_id,
            BranchClosureProjectionInput {
                contract_identity: &record.contract_identity,
                base_branch: &record.base_branch,
                reviewed_state_id,
                effective_reviewed_branch_surface: &record._effective_reviewed_branch_surface,
                source_task_closure_ids: &record.source_task_closure_ids,
                provenance_basis: &record.provenance_basis,
                superseded_branch_closure_ids: &[],
            },
            Some(REGENERATED_ARTIFACT_GENERATED_AT),
        )?
    };
    write_late_stage_projection_artifact(
        runtime,
        &context.runtime.repo_root,
        mode,
        &format!("branch-closure-{branch_closure_id}.md"),
        &source,
        write,
    )
}

fn regenerate_release_readiness_projection(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    release_readiness_record_id: &str,
    record: &CurrentReleaseReadinessRecord,
    mode: ProjectionWriteMode,
    write: bool,
) -> Result<RenderedProjectionArtifact, JsonFailure> {
    let authoritative_source = record
        .release_docs_fingerprint
        .as_deref()
        .and_then(|fingerprint| {
            read_authoritative_projection(runtime, "release-docs", fingerprint, "release docs").ok()
        });
    let source = if let Some(source) = authoritative_source
        && (source.contains("**Result:** pass") || source.contains("**Result:** blocked"))
    {
        source
    } else {
        render_release_readiness_artifact_with_generated_at(
            context,
            &record.branch_closure_id,
            &record.reviewed_state_id,
            &record.base_branch,
            &record.result,
            &record.summary,
            Some(REGENERATED_ARTIFACT_GENERATED_AT),
        )?
    };
    write_late_stage_projection_artifact(
        runtime,
        &context.runtime.repo_root,
        mode,
        &format!(
            "featureforge-{}-release-readiness-{}.md",
            runtime.safe_branch,
            normalize_projection_suffix(release_readiness_record_id)
        ),
        &source,
        write,
    )
}

fn regenerate_final_review_projection(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    final_review_record_id: &str,
    record: &CurrentFinalReviewRecord,
    mode: ProjectionWriteMode,
    write: bool,
) -> Result<Vec<RenderedProjectionArtifact>, JsonFailure> {
    let summary = if record.summary.trim().is_empty() {
        "Final review recorded without summary content."
    } else {
        record.summary.as_str()
    };
    let authoritative_projection =
        if let Some(fingerprint) = record.final_review_fingerprint.as_deref() {
            read_authoritative_projection_file(
                runtime,
                "final-review",
                fingerprint,
                "final review",
                if write {
                    AuthoritativeProjectionReadMode::AllowMissing
                } else {
                    AuthoritativeProjectionReadMode::BestEffort
                },
            )?
        } else {
            None
        };
    let (
        authoritative_document,
        authoritative_generated_at,
        authoritative_reviewer_path,
        authoritative_reviewer_fingerprint,
    ) = if let Some((authoritative_path, _)) = authoritative_projection.as_ref() {
        let document = parse_artifact_document(authoritative_path);
        let generated_at = document.headers.get("Generated At").cloned();
        let reviewer_path = document
            .headers
            .get("Reviewer Artifact Path")
            .map(|value| value.trim().trim_matches('`').to_owned())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let reviewer_fingerprint = document
            .headers
            .get("Reviewer Artifact Fingerprint")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        (
            Some(document),
            generated_at,
            reviewer_path,
            reviewer_fingerprint,
        )
    } else {
        (None, None, None, None)
    };
    let authoritative_reviewer_path = authoritative_reviewer_path
        .as_deref()
        .map(|path| normalize_project_artifact_target(&project_artifact_dir(runtime), path))
        .transpose()
        .or_else(|error| if write { Err(error) } else { Ok(None) })?;
    let reviewer_name = format!(
        "featureforge-{}-independent-review-{}.md",
        runtime.safe_branch,
        normalize_projection_suffix(final_review_record_id),
    );
    let canonical_reviewer_projection_path =
        late_stage_projection_base_dir(runtime, &context.runtime.repo_root, mode)
            .join(&reviewer_name);
    let generated_at_override = authoritative_generated_at
        .as_deref()
        .unwrap_or(REGENERATED_ARTIFACT_GENERATED_AT);
    let artifacts = match render_final_review_artifacts_with_generated_at(
        runtime,
        context,
        &record.branch_closure_id,
        &record.reviewed_state_id,
        &record.base_branch,
        FinalReviewProjectionInput {
            dispatch_id: &record.dispatch_id,
            reviewer_source: &record.reviewer_source,
            reviewer_id: &record.reviewer_id,
            result: &record.result,
            deviations_required: record.deviations_required.unwrap_or(false),
            summary,
        },
        FinalReviewRenderOverrides {
            generated_at: Some(generated_at_override),
            reviewer_artifact_path: Some(canonical_reviewer_projection_path.as_path()),
        },
    ) {
        Ok(artifacts) => artifacts,
        Err(_) if !write => {
            let final_review_name = format!(
                "featureforge-{}-code-review-{}.md",
                runtime.safe_branch,
                normalize_projection_suffix(final_review_record_id)
            );
            return Ok(vec![
                write_late_stage_projection_artifact(
                    runtime,
                    &context.runtime.repo_root,
                    mode,
                    &reviewer_name,
                    "",
                    false,
                )?,
                write_late_stage_projection_artifact(
                    runtime,
                    &context.runtime.repo_root,
                    mode,
                    &final_review_name,
                    "",
                    false,
                )?,
            ]);
        }
        Err(error) => return Err(error),
    };
    let regenerated_reviewer_source = if let Some(expected_reviewer_fingerprint) =
        authoritative_reviewer_fingerprint.as_deref()
    {
        let persisted_reviewer_source = authoritative_reviewer_path.as_ref().and_then(|path| {
            fs::read_to_string(path)
                .ok()
                .filter(|source| sha256_hex(source.as_bytes()) == expected_reviewer_fingerprint)
        });
        let synthesized_reviewer_source = persisted_reviewer_source.or_else(|| {
            authoritative_document.as_ref().and_then(|document| {
                reviewer_artifact_candidates_from_authoritative_final_review(
                    document,
                    summary,
                    authoritative_reviewer_path.as_deref(),
                )
                .into_iter()
                .find(|source| sha256_hex(source.as_bytes()) == expected_reviewer_fingerprint)
            })
        });
        let Some(source) = synthesized_reviewer_source else {
            if !write {
                return Ok(vec![write_late_stage_projection_artifact(
                    runtime,
                    &context.runtime.repo_root,
                    mode,
                    &reviewer_name,
                    &artifacts.reviewer_source_text,
                    false,
                )?]);
            }
            return Err(JsonFailure::new(
                FailureClass::StaleProvenance,
                "Projection regeneration could not restore reviewer projection content that matches authoritative final-review bindings.",
            ));
        };
        source
    } else {
        artifacts.reviewer_source_text.clone()
    };
    let mut written = vec![write_late_stage_projection_artifact(
        runtime,
        &context.runtime.repo_root,
        mode,
        &reviewer_name,
        &regenerated_reviewer_source,
        write,
    )?];
    if let Some(reviewer_path) = authoritative_reviewer_path.as_deref()
        && reviewer_path != canonical_reviewer_projection_path
        && mode == ProjectionWriteMode::StateDirOnly
    {
        if write {
            write_project_artifact_at_path(runtime, reviewer_path, &regenerated_reviewer_source)?;
        }
        written.push(RenderedProjectionArtifact {
            path: reviewer_path.to_string_lossy().into_owned(),
            source: regenerated_reviewer_source.clone(),
        });
    }
    let final_review_source = authoritative_projection
        .as_ref()
        .map(|(_, source)| source.clone())
        .unwrap_or(artifacts.final_review_source);
    written.push(write_late_stage_projection_artifact(
        runtime,
        &context.runtime.repo_root,
        mode,
        &format!(
            "featureforge-{}-code-review-{}.md",
            runtime.safe_branch,
            normalize_projection_suffix(final_review_record_id)
        ),
        &final_review_source,
        write,
    )?);
    Ok(written)
}

fn reviewer_artifact_candidates_from_authoritative_final_review(
    final_review_document: &crate::execution::final_review::ArtifactDocument,
    summary: &str,
    reviewer_artifact_path: Option<&Path>,
) -> Vec<String> {
    let header = |key: &str| {
        final_review_document
            .headers
            .get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let Some(review_stage) = header("Review Stage") else {
        return Vec::new();
    };
    let Some(reviewer_provenance) = header("Reviewer Provenance") else {
        return Vec::new();
    };
    let Some(reviewer_source) = header("Reviewer Source") else {
        return Vec::new();
    };
    let Some(reviewer_id) = header("Reviewer ID") else {
        return Vec::new();
    };
    let Some(strategy_checkpoint_fingerprint) = header("Strategy Checkpoint Fingerprint") else {
        return Vec::new();
    };
    let Some(distinct_from_stages) = header("Distinct From Stages") else {
        return Vec::new();
    };
    let Some(recorded_execution_deviations) = header("Recorded Execution Deviations") else {
        return Vec::new();
    };
    let Some(deviation_review_verdict) = header("Deviation Review Verdict") else {
        return Vec::new();
    };
    let Some(source_plan) = header("Source Plan") else {
        return Vec::new();
    };
    let source_plan = source_plan.trim_matches('`');
    let Some(source_plan_revision) = header("Source Plan Revision") else {
        return Vec::new();
    };
    let Some(branch) = header("Branch") else {
        return Vec::new();
    };
    let Some(repo) = header("Repo") else {
        return Vec::new();
    };
    let Some(base_branch) = header("Base Branch") else {
        return Vec::new();
    };
    let Some(head_sha) = header("Head SHA") else {
        return Vec::new();
    };
    let Some(result) = header("Result") else {
        return Vec::new();
    };
    let Some(generated_by) = header("Generated By") else {
        return Vec::new();
    };
    let Some(final_review_generated_at) = header("Generated At") else {
        return Vec::new();
    };

    let reviewer_generated_at = reviewer_artifact_path
        .and_then(generated_at_from_reviewer_artifact_path)
        .filter(|value| value != final_review_generated_at);
    let mut generated_at_candidates = vec![final_review_generated_at.to_owned()];
    if let Some(value) = reviewer_generated_at {
        generated_at_candidates.push(value);
    }
    let mut candidates = Vec::new();
    for generated_at in generated_at_candidates {
        candidates.push(format!(
            "# Code Review Result\n**Review Stage:** {review_stage}\n**Reviewer Provenance:** {reviewer_provenance}\n**Reviewer Source:** {reviewer_source}\n**Reviewer ID:** {reviewer_id}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** {distinct_from_stages}\n**Recorded Execution Deviations:** {recorded_execution_deviations}\n**Deviation Review Verdict:** {deviation_review_verdict}\n**Source Plan:** `{source_plan}`\n**Source Plan Revision:** {source_plan_revision}\n**Branch:** {branch}\n**Repo:** {repo}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** {result}\n**Generated By:** {generated_by}\n**Generated At:** {generated_at}\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
        ));
        if let (Some(reviewed_state_id), Some(branch_closure_id)) = (
            header("Current Reviewed Branch State ID"),
            header("Branch Closure ID"),
        ) {
            candidates.push(format!(
                "# Code Review Result\n**Review Stage:** {review_stage}\n**Reviewer Provenance:** {reviewer_provenance}\n**Reviewer Source:** {reviewer_source}\n**Reviewer ID:** {reviewer_id}\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** {distinct_from_stages}\n**Recorded Execution Deviations:** {recorded_execution_deviations}\n**Deviation Review Verdict:** {deviation_review_verdict}\n**Source Plan:** `{source_plan}`\n**Source Plan Revision:** {source_plan_revision}\n**Branch:** {branch}\n**Repo:** {repo}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Current Reviewed Branch State ID:** {reviewed_state_id}\n**Branch Closure ID:** {branch_closure_id}\n**Result:** {result}\n**Generated By:** {generated_by}\n**Generated At:** {generated_at}\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n- {summary}\n",
            ));
        }
    }
    candidates
}

fn generated_at_from_reviewer_artifact_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".md")?;
    let mut pieces = stem.rsplitn(3, '-');
    let stamp = pieces.next()?;
    let date_part = pieces.next()?;
    if stamp.len() != 6
        || date_part.len() != 8
        || !stamp.chars().all(|ch| ch.is_ascii_digit())
        || !date_part.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    Some(format!(
        "{}-{}-{}T{}:{}:{}Z",
        &date_part[0..4],
        &date_part[4..6],
        &date_part[6..8],
        &stamp[0..2],
        &stamp[2..4],
        &stamp[4..6],
    ))
}

fn regenerate_browser_qa_projection(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    qa_record_id: &str,
    record: &CurrentBrowserQaRecord,
    mode: ProjectionWriteMode,
    write: bool,
) -> Result<Vec<RenderedProjectionArtifact>, JsonFailure> {
    let summary = if record.summary.trim().is_empty() {
        "Browser QA recorded without summary content."
    } else {
        record.summary.as_str()
    };
    let mut written = Vec::new();
    let regenerated_test_plan_path =
        regenerate_test_plan_projection(runtime, &context.runtime.repo_root, record, mode, write)?;
    if let Some(artifact) = regenerated_test_plan_path.as_ref() {
        written.push(RenderedProjectionArtifact {
            path: artifact.path.clone(),
            source: artifact.source.clone(),
        });
    }
    let authoritative_source = record
        .browser_qa_fingerprint
        .as_deref()
        .and_then(|fingerprint| {
            read_authoritative_projection(runtime, "browser-qa", fingerprint, "browser QA").ok()
        });
    let source = if let Some(source) = authoritative_source {
        source
    } else {
        render_qa_artifact_with_generated_at(
            runtime,
            context,
            QaProjectionInput {
                branch_closure_id: &record.branch_closure_id,
                reviewed_state_id: &record.reviewed_state_id,
                result: &record.result,
                summary,
                base_branch: &record.base_branch,
                test_plan_path: regenerated_test_plan_path
                    .as_ref()
                    .map(|artifact| Path::new(&artifact.path)),
            },
            Some(REGENERATED_ARTIFACT_GENERATED_AT),
        )?
    };
    written.push(write_late_stage_projection_artifact(
        runtime,
        &context.runtime.repo_root,
        mode,
        &format!(
            "featureforge-{}-test-outcome-{}.md",
            runtime.safe_branch,
            normalize_projection_suffix(qa_record_id)
        ),
        &source,
        write,
    )?);
    Ok(written)
}

fn regenerate_test_plan_projection(
    runtime: &ExecutionRuntime,
    repo_root: &Path,
    record: &CurrentBrowserQaRecord,
    mode: ProjectionWriteMode,
    write: bool,
) -> Result<Option<RenderedProjectionArtifact>, JsonFailure> {
    let Some(test_plan_fingerprint) = record.source_test_plan_fingerprint.as_deref() else {
        return Ok(None);
    };
    let test_plan_source = read_authoritative_projection_file(
        runtime,
        "test-plan",
        test_plan_fingerprint,
        "test plan",
        if write {
            AuthoritativeProjectionReadMode::Strict
        } else {
            AuthoritativeProjectionReadMode::BestEffort
        },
    )?
    .map(|(_, source)| source)
    .unwrap_or_default();
    let test_plan_name = format!(
        "featureforge-{}-test-plan-{}.md",
        runtime.safe_branch,
        normalize_projection_suffix(test_plan_fingerprint)
    );
    let artifact = write_late_stage_projection_artifact(
        runtime,
        repo_root,
        mode,
        &test_plan_name,
        &test_plan_source,
        write,
    )?;
    Ok(Some(artifact))
}

fn late_stage_projection_base_dir(
    runtime: &ExecutionRuntime,
    repo_root: &Path,
    mode: ProjectionWriteMode,
) -> PathBuf {
    match mode {
        ProjectionWriteMode::StateDirOnly | ProjectionWriteMode::Disabled => {
            project_artifact_dir(runtime)
        }
        ProjectionWriteMode::ProjectionExport => repo_root
            .join(PROJECTION_EXPORT_ROOT_REL)
            .join(&runtime.safe_branch),
    }
}

fn write_late_stage_projection_artifact(
    runtime: &ExecutionRuntime,
    repo_root: &Path,
    mode: ProjectionWriteMode,
    artifact_name: &str,
    source: &str,
    write: bool,
) -> Result<RenderedProjectionArtifact, JsonFailure> {
    match mode {
        ProjectionWriteMode::StateDirOnly => {
            if write {
                write_project_artifact(runtime, artifact_name, source)?;
            }
            Ok(RenderedProjectionArtifact {
                path: project_artifact_dir(runtime)
                    .join(artifact_name)
                    .to_string_lossy()
                    .into_owned(),
                source: source.to_owned(),
            })
        }
        ProjectionWriteMode::ProjectionExport => {
            let base = late_stage_projection_base_dir(runtime, repo_root, mode);
            let target = normalize_project_artifact_target(&base, Path::new(artifact_name))?;
            if write {
                write_atomic(
                    &target,
                    source,
                    "Could not write late-stage projection export",
                )?;
            }
            let repo_relative = target.strip_prefix(repo_root).map_err(|_| {
                JsonFailure::new(
                    FailureClass::EvidenceWriteFailed,
                    format!(
                        "Tracked late-stage projection path {} is outside repo root {}.",
                        target.display(),
                        repo_root.display()
                    ),
                )
            })?;
            Ok(RenderedProjectionArtifact {
                path: normalize_repo_relative_path(&repo_relative.to_string_lossy())
                    .map_err(JsonFailure::from)?,
                source: source.to_owned(),
            })
        }
        ProjectionWriteMode::Disabled => Ok(RenderedProjectionArtifact {
            path: String::new(),
            source: source.to_owned(),
        }),
    }
}

fn read_authoritative_projection(
    runtime: &ExecutionRuntime,
    artifact_prefix: &str,
    fingerprint: &str,
    artifact_label: &str,
) -> Result<String, JsonFailure> {
    read_authoritative_projection_file(
        runtime,
        artifact_prefix,
        fingerprint,
        artifact_label,
        AuthoritativeProjectionReadMode::Strict,
    )
    .map(|source| {
        source
            .map(|(_, source)| source)
            .expect("fail-closed authoritative projection reads always return source or error")
    })
}

fn read_authoritative_projection_file(
    runtime: &ExecutionRuntime,
    artifact_prefix: &str,
    fingerprint: &str,
    artifact_label: &str,
    mode: AuthoritativeProjectionReadMode,
) -> Result<Option<(PathBuf, String)>, JsonFailure> {
    let path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("{artifact_prefix}-{fingerprint}.md"),
    );
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            return match (mode, error.kind()) {
                (AuthoritativeProjectionReadMode::BestEffort, _)
                | (AuthoritativeProjectionReadMode::AllowMissing, ErrorKind::NotFound) => Ok(None),
                (AuthoritativeProjectionReadMode::Strict, _)
                | (AuthoritativeProjectionReadMode::AllowMissing, _) => Err(JsonFailure::new(
                    FailureClass::StaleProvenance,
                    format!(
                        "Projection regeneration requires readable authoritative {artifact_label} artifact {}: {error}",
                        path.display()
                    ),
                )),
            };
        }
    };
    let observed_fingerprint = sha256_hex(source.as_bytes());
    if observed_fingerprint != fingerprint {
        if matches!(mode, AuthoritativeProjectionReadMode::BestEffort) {
            return Ok(None);
        }
        return Err(JsonFailure::new(
            FailureClass::StaleProvenance,
            format!(
                "Projection regeneration refused authoritative {artifact_label} artifact {} because fingerprint {} does not match expected {fingerprint}.",
                path.display(),
                observed_fingerprint
            ),
        ));
    }
    Ok(Some((path, source)))
}

fn write_atomic(path: &Path, contents: &str, message_prefix: &str) -> Result<(), JsonFailure> {
    write_atomic_inner(path, contents, message_prefix, true)
}

fn write_atomic_inner(
    path: &Path,
    contents: &str,
    message_prefix: &str,
    create_parent: bool,
) -> Result<(), JsonFailure> {
    if create_parent && let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!("{message_prefix} {}: {error}", parent.display()),
            )
        })?;
    }
    let temp_path = atomic_publish_temp_path(path);
    fs::write(&temp_path, contents).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("{message_prefix} {}: {error}", temp_path.display()),
        )
    })?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!("{message_prefix} {}: {error}", path.display()),
            ))
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn test_runtime(root: &Path) -> ExecutionRuntime {
        ExecutionRuntime {
            repo_root: root.to_path_buf(),
            git_dir: root.join(".git"),
            branch_name: String::from("feature/runtime"),
            repo_slug: String::from("featureforge"),
            safe_branch: String::from("feature-runtime"),
            state_dir: root.join("state"),
        }
    }

    fn valid_task_source(step_projection: &str) -> String {
        format!(
            "# Plan\n\
## Task 1: Build\n\
**Spec Coverage:** REQ-1\n\
**Goal:** Build the thing.\n\
**Context:**\n\
- The plan has enough context for deterministic execution.\n\
**Constraints:**\n\
- Preserve projection boundaries.\n\
**Done when:**\n\
- The projection is verified.\n\
**Files:**\n\
- Modify: `src/lib.rs`\n\
{step_projection}"
        )
    }

    #[test]
    fn plan_projection_renderer_leaves_fenced_step_shaped_content_semantic() {
        let source = valid_task_source(
            "- [ ] **Step 1: Build the thing**\n```\n  - [x] **Step 1: Build the thing**\n```\n",
        );
        let rendered = render_plan_projection_source(
            &source,
            "featureforge:executing-plans",
            &[PlanStepState {
                task_number: 1,
                step_number: 1,
                title: String::from("Build the thing"),
                checked: true,
                note_state: None,
                note_summary: String::new(),
            }],
        );

        assert!(rendered.contains(
            "- [x] **Step 1: Build the thing**\n```\n  - [x] **Step 1: Build the thing**\n```"
        ));
    }

    #[test]
    fn project_artifact_write_normalizes_parent_segments_within_runtime_scope() {
        let temp = TempDir::new().expect("temp runtime root should exist");
        let runtime = test_runtime(temp.path());
        write_project_artifact_at_path(
            &runtime,
            Path::new("nested/../reviewer.md"),
            "normalized reviewer projection",
        )
        .expect("writer should accept runtime-owned path with lexical parent segments");
        let expected_path = project_artifact_dir(&runtime).join("reviewer.md");
        assert_eq!(
            fs::read_to_string(&expected_path)
                .expect("normalized reviewer projection should exist"),
            "normalized reviewer projection"
        );
    }

    #[test]
    fn project_artifact_write_rejects_paths_that_escape_runtime_scope() {
        let temp = TempDir::new().expect("temp runtime root should exist");
        let runtime = test_runtime(temp.path());
        let error =
            write_project_artifact_at_path(&runtime, Path::new("../../escape.md"), "escape")
                .expect_err("writer should reject reviewer projection path escapes");
        assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
    }

    #[cfg(unix)]
    #[test]
    fn project_artifact_write_rejects_symlinked_path_segments() {
        let temp = TempDir::new().expect("temp runtime root should exist");
        let runtime = test_runtime(temp.path());
        let project_dir = project_artifact_dir(&runtime);
        fs::create_dir_all(&project_dir).expect("project artifact dir should be creatable");
        let outside_dir = temp.path().join("outside");
        fs::create_dir_all(&outside_dir).expect("outside dir should be creatable");
        symlink(&outside_dir, project_dir.join("escaped"))
            .expect("symlinked project segment fixture should be creatable");

        let error = write_project_artifact_at_path(
            &runtime,
            Path::new("escaped/reviewer.md"),
            "escaped projection",
        )
        .expect_err("writer should reject symlinked project path segments");
        assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
    }

    #[test]
    fn authoritative_projection_read_rejects_fingerprint_content_mismatch() {
        let temp = TempDir::new().expect("temp runtime root should exist");
        let runtime = test_runtime(temp.path());
        let expected_fingerprint = sha256_hex(b"expected-authoritative-final-review");
        let authoritative_path = harness_authoritative_artifact_path(
            &runtime.state_dir,
            &runtime.repo_slug,
            &runtime.branch_name,
            &format!("final-review-{expected_fingerprint}.md"),
        );
        fs::create_dir_all(
            authoritative_path
                .parent()
                .expect("authoritative projection path should include parent"),
        )
        .expect("authoritative projection parent should be creatable");
        fs::write(
            &authoritative_path,
            "tampered authoritative projection content",
        )
        .expect("tampered authoritative projection should write");

        let error = read_authoritative_projection(
            &runtime,
            "final-review",
            &expected_fingerprint,
            "final review",
        )
        .expect_err("fingerprint/content mismatch should fail closed");
        assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
    }

    #[test]
    fn normalize_projection_suffix_rewrites_non_alphanumeric_content() {
        assert_eq!(
            normalize_projection_suffix("final-review-record:abc/123"),
            "final-review-record-abc-123"
        );
        assert_eq!(normalize_projection_suffix("..."), "projection");
    }

    #[test]
    fn regenerate_test_plan_projection_uses_stable_fingerprint_suffix() {
        let temp = TempDir::new().expect("temp runtime root should exist");
        let runtime = test_runtime(temp.path());
        let authoritative_source = "# Test Plan\nfixture\n";
        let authoritative_fingerprint = sha256_hex(authoritative_source.as_bytes());
        let authoritative_path = harness_authoritative_artifact_path(
            &runtime.state_dir,
            &runtime.repo_slug,
            &runtime.branch_name,
            &format!("test-plan-{authoritative_fingerprint}.md"),
        );
        fs::create_dir_all(
            authoritative_path
                .parent()
                .expect("authoritative test-plan path should include parent"),
        )
        .expect("authoritative test-plan parent should be creatable");
        fs::write(&authoritative_path, authoritative_source)
            .expect("authoritative test-plan fixture should be writable");
        let record = CurrentBrowserQaRecord {
            record_status: String::from("current"),
            branch_closure_id: String::from("branch-closure-fixture"),
            final_review_record_id: Some(String::from("final-review-record-fixture")),
            source_plan_path: String::from("docs/featureforge/plans/fixture.md"),
            source_plan_revision: 1,
            repo_slug: runtime.repo_slug.clone(),
            branch_name: runtime.branch_name.clone(),
            base_branch: String::from("main"),
            reviewed_state_id: String::from("git_tree:fixture"),
            semantic_reviewed_state_id: None,
            result: String::from("pass"),
            browser_qa_fingerprint: Some(String::from("browser-qa-fingerprint")),
            source_test_plan_fingerprint: Some(authoritative_fingerprint.clone()),
            summary: String::from("summary"),
            summary_hash: String::from("summary-hash"),
            generated_by_identity: String::from("featureforge/qa"),
        };

        let first = regenerate_test_plan_projection(
            &runtime,
            &runtime.repo_root,
            &record,
            ProjectionWriteMode::StateDirOnly,
            true,
        )
        .expect("test-plan regeneration should succeed")
        .expect("test-plan regeneration should publish a projection path");
        let second = regenerate_test_plan_projection(
            &runtime,
            &runtime.repo_root,
            &record,
            ProjectionWriteMode::StateDirOnly,
            true,
        )
        .expect("test-plan regeneration should succeed on replay")
        .expect("test-plan regeneration should keep publishing a projection path");
        assert_eq!(first, second);
        assert!(
            first
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(&authoritative_fingerprint)),
            "path should be fingerprint-derived: {}",
            first.display()
        );
        assert_eq!(
            fs::read_to_string(&first).expect("projected test-plan should be readable"),
            authoritative_source
        );
    }
}
