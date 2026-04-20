use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contracts::plan::{AnalyzePlanReport, PlanDocument, PlanTask};
use crate::contracts::spec::{Requirement, SpecDocument};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::runtime_root::write_runtime_root_schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
/// Runtime struct.
pub struct TaskPacket {
    /// Runtime field.
    pub plan_path: String,
    /// Runtime field.
    pub plan_revision: u32,
    /// Runtime field.
    pub plan_fingerprint: String,
    /// Runtime field.
    pub source_spec_path: String,
    /// Runtime field.
    pub source_spec_revision: u32,
    /// Runtime field.
    pub source_spec_fingerprint: String,
    /// Runtime field.
    pub task_number: u32,
    /// Runtime field.
    pub task_title: String,
    /// Runtime field.
    pub open_questions: String,
    /// Runtime field.
    pub requirement_ids: Vec<String>,
    /// Runtime field.
    pub generated_at: String,
    /// Runtime field.
    pub packet_fingerprint: String,
    /// Runtime field.
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
/// Runtime struct.
pub struct HarnessContractProvenance {
    /// Runtime field.
    pub source_plan_path: String,
    /// Runtime field.
    pub source_plan_revision: u32,
    /// Runtime field.
    pub source_plan_fingerprint: String,
    /// Runtime field.
    pub source_spec_path: String,
    /// Runtime field.
    pub source_spec_revision: u32,
    /// Runtime field.
    pub source_spec_fingerprint: String,
    /// Runtime field.
    pub source_task_packet_fingerprints: Vec<String>,
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn build_harness_contract_provenance(
    task_packets: &[TaskPacket],
) -> Result<HarnessContractProvenance, DiagnosticError> {
    let Some(first) = task_packets.first() else {
        return Err(DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            String::from(
                "Harness contract task packet provenance requires at least one task packet.",
            ),
        ));
    };

    let expected_plan_path = first.plan_path.clone();
    let expected_plan_revision = first.plan_revision;
    let expected_plan_fingerprint = first.plan_fingerprint.clone();
    let expected_spec_path = first.source_spec_path.clone();
    let expected_spec_revision = first.source_spec_revision;
    let expected_spec_fingerprint = first.source_spec_fingerprint.clone();

    for (index, packet) in task_packets.iter().enumerate().skip(1) {
        let matches_baseline = packet.plan_path == expected_plan_path
            && packet.plan_revision == expected_plan_revision
            && packet.plan_fingerprint == expected_plan_fingerprint
            && packet.source_spec_path == expected_spec_path
            && packet.source_spec_revision == expected_spec_revision
            && packet.source_spec_fingerprint == expected_spec_fingerprint;
        if !matches_baseline {
            return Err(DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!(
                    "Harness contract task packet provenance mismatch at index {index}; all packets must share source plan/spec provenance."
                ),
            ));
        }
    }

    Ok(HarnessContractProvenance {
        source_plan_path: expected_plan_path,
        source_plan_revision: expected_plan_revision,
        source_plan_fingerprint: expected_plan_fingerprint,
        source_spec_path: expected_spec_path,
        source_spec_revision: expected_spec_revision,
        source_spec_fingerprint: expected_spec_fingerprint,
        source_task_packet_fingerprints: task_packets
            .iter()
            .map(|packet| packet.packet_fingerprint.clone())
            .collect(),
    })
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn build_task_packet_with_timestamp(
    spec: &SpecDocument,
    plan: &PlanDocument,
    task_number: u32,
    generated_at: &str,
) -> Result<TaskPacket, DiagnosticError> {
    let task = plan
        .tasks
        .iter()
        .find(|task| task.number == task_number)
        .ok_or_else(|| {
            DiagnosticError::new(
                FailureClass::InstructionParseFailed,
                format!("Task {task_number} was not found."),
            )
        })?;

    let covered_requirements = requirement_subset(spec, &task.spec_coverage);
    let plan_fingerprint = sha256_hex(plan.source.as_bytes());
    let source_spec_fingerprint = sha256_hex(spec.source.as_bytes());
    let markdown = render_packet_markdown(
        plan,
        task,
        &covered_requirements,
        generated_at,
        &plan_fingerprint,
        &source_spec_fingerprint,
    );
    let packet_fingerprint = sha256_hex(markdown.as_bytes());

    Ok(TaskPacket {
        plan_path: plan.path.clone(),
        plan_revision: plan.plan_revision,
        plan_fingerprint,
        source_spec_path: spec.path.clone(),
        source_spec_revision: spec.spec_revision,
        source_spec_fingerprint,
        task_number: task.number,
        task_title: task.title.clone(),
        open_questions: task.open_questions.clone(),
        requirement_ids: task.spec_coverage.clone(),
        generated_at: generated_at.to_owned(),
        packet_fingerprint,
        markdown,
    })
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn write_contract_schemas(output_dir: impl AsRef<Path>) -> Result<(), DiagnosticError> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!(
                "Could not create schema directory {}: {err}",
                output_dir.display()
            ),
        )
    })?;

    let analyze_schema = schema_for!(AnalyzePlanReport);
    let packet_schema = schema_for!(TaskPacket);
    let analyze_schema_source = serde_json::to_string_pretty(&analyze_schema).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize analyze schema: {err}"),
        )
    })?;
    fs::write(
        output_dir.join("plan-contract-analyze.schema.json"),
        analyze_schema_source,
    )
    .map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write analyze schema: {err}"),
        )
    })?;
    let packet_schema_source = serde_json::to_string_pretty(&packet_schema).map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not serialize packet schema: {err}"),
        )
    })?;
    fs::write(
        output_dir.join("plan-contract-packet.schema.json"),
        packet_schema_source,
    )
    .map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write packet schema: {err}"),
        )
    })?;
    write_runtime_root_schema(output_dir)?;
    Ok(())
}

fn render_packet_markdown(
    plan: &PlanDocument,
    task: &PlanTask,
    requirements: &[Requirement],
    generated_at: &str,
    plan_fingerprint: &str,
    source_spec_fingerprint: &str,
) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Task Packet\n\n");
    let _ = writeln!(markdown, "**Plan Path:** `{}`", plan.path);
    let _ = writeln!(markdown, "**Plan Revision:** {}", plan.plan_revision);
    let _ = writeln!(markdown, "**Plan Fingerprint:** `{plan_fingerprint}`");
    let _ = writeln!(
        markdown,
        "**Source Spec Path:** `{}`",
        plan.source_spec_path
    );
    let _ = writeln!(
        markdown,
        "**Source Spec Revision:** {}",
        plan.source_spec_revision
    );
    let _ = writeln!(
        markdown,
        "**Source Spec Fingerprint:** `{source_spec_fingerprint}`"
    );
    let _ = writeln!(markdown, "**Task Number:** {}", task.number);
    let _ = writeln!(markdown, "**Task Title:** {}", task.title);
    let _ = writeln!(markdown, "**Open Questions:** {}", task.open_questions);
    let _ = writeln!(markdown, "**Generated At:** {generated_at}");
    markdown.push('\n');
    markdown.push_str("## Covered Requirements\n\n");
    for requirement in requirements {
        let _ = writeln!(
            markdown,
            "- [{}][{}] {}",
            requirement.id, requirement.kind, requirement.text
        );
    }
    markdown.push_str("\n## Task Block\n\n");
    let _ = writeln!(markdown, "## Task {}: {}", task.number, task.title);
    markdown.push('\n');
    let _ = writeln!(
        markdown,
        "**Spec Coverage:** {}",
        task.spec_coverage.join(", ")
    );
    let _ = writeln!(markdown, "**Task Outcome:** {}", task.task_outcome);
    markdown.push_str("**Plan Constraints:**\n");
    for constraint in &task.plan_constraints {
        let _ = writeln!(markdown, "- {constraint}");
    }
    let _ = writeln!(markdown, "**Open Questions:** {}", task.open_questions);
    markdown.push('\n');
    markdown.push_str("**Files:**\n");
    for file in &task.files {
        let _ = writeln!(markdown, "- {}: `{}`", file.action, file.path);
    }
    markdown.push('\n');
    for step in &task.steps {
        let _ = writeln!(markdown, "- [ ] **Step {}: {}**", step.number, step.text);
    }
    markdown
}

fn requirement_subset(spec: &SpecDocument, ids: &[String]) -> Vec<Requirement> {
    spec.requirements
        .iter()
        .filter(|requirement| ids.contains(&requirement.id))
        .cloned()
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
