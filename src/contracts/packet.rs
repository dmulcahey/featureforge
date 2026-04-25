use std::fs;
use std::path::Path;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contracts::plan::{AnalyzePlanReport, PlanDocument, PlanTask};
use crate::contracts::spec::{Requirement, SpecDocument};
use crate::diagnostics::{DiagnosticError, FailureClass};
use crate::runtime_root::write_runtime_root_schema;

const TASK_PACKET_CONTRACT_VERSION: &str = "task-obligation-v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PacketObligation {
    pub id: String,
    pub index: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TaskPacketFileEntry {
    pub action: String,
    pub path: String,
    pub normalized_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct TaskPacket {
    pub packet_contract_version: String,
    pub plan_path: String,
    pub plan_revision: u32,
    pub plan_fingerprint: String,
    pub source_spec_path: String,
    pub source_spec_revision: u32,
    pub source_spec_fingerprint: String,
    pub task_number: u32,
    pub task_title: String,
    pub goal: String,
    pub context: Vec<String>,
    pub constraints: Vec<String>,
    pub constraint_obligations: Vec<PacketObligation>,
    pub done_when: Vec<String>,
    pub done_when_obligations: Vec<PacketObligation>,
    pub requirement_ids: Vec<String>,
    pub file_entries: Vec<TaskPacketFileEntry>,
    pub file_scope: Vec<String>,
    pub generated_at: String,
    pub packet_fingerprint: String,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct HarnessContractProvenance {
    pub source_plan_path: String,
    pub source_plan_revision: u32,
    pub source_plan_fingerprint: String,
    pub source_spec_path: String,
    pub source_spec_revision: u32,
    pub source_spec_fingerprint: String,
    pub source_task_packet_fingerprints: Vec<String>,
}

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
    let constraint_obligations = indexed_obligations("CONSTRAINT", &task.constraints);
    let done_when_obligations = indexed_obligations("DONE_WHEN", &task.done_when);
    let packet_fingerprint = build_packet_fingerprint(
        plan,
        task,
        &plan_fingerprint,
        &source_spec_fingerprint,
        &constraint_obligations,
        &done_when_obligations,
    );
    let markdown = render_packet_markdown(&PacketMarkdownInput {
        plan,
        task,
        requirements: &covered_requirements,
        generated_at,
        plan_fingerprint: &plan_fingerprint,
        source_spec_fingerprint: &source_spec_fingerprint,
        packet_fingerprint: &packet_fingerprint,
        constraint_obligations: &constraint_obligations,
        done_when_obligations: &done_when_obligations,
    });

    Ok(TaskPacket {
        packet_contract_version: TASK_PACKET_CONTRACT_VERSION.to_owned(),
        plan_path: plan.path.clone(),
        plan_revision: plan.plan_revision,
        plan_fingerprint,
        source_spec_path: spec.path.clone(),
        source_spec_revision: spec.spec_revision,
        source_spec_fingerprint,
        task_number: task.number,
        task_title: task.title.clone(),
        goal: task.goal.clone(),
        context: task.context.clone(),
        constraints: task.constraints.clone(),
        constraint_obligations,
        done_when: task.done_when.clone(),
        done_when_obligations,
        requirement_ids: task.spec_coverage.clone(),
        file_entries: task_packet_file_entries(task),
        file_scope: task.files.iter().map(|entry| entry.path.clone()).collect(),
        generated_at: generated_at.to_owned(),
        packet_fingerprint,
        markdown,
    })
}

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
    fs::write(
        output_dir.join("plan-contract-analyze.schema.json"),
        serde_json::to_string_pretty(&analyze_schema).expect("analyze schema should serialize"),
    )
    .map_err(|err| {
        DiagnosticError::new(
            FailureClass::InstructionParseFailed,
            format!("Could not write analyze schema: {err}"),
        )
    })?;
    fs::write(
        output_dir.join("plan-contract-packet.schema.json"),
        serde_json::to_string_pretty(&packet_schema).expect("packet schema should serialize"),
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

struct PacketMarkdownInput<'a> {
    plan: &'a PlanDocument,
    task: &'a PlanTask,
    requirements: &'a [Requirement],
    generated_at: &'a str,
    plan_fingerprint: &'a str,
    source_spec_fingerprint: &'a str,
    packet_fingerprint: &'a str,
    constraint_obligations: &'a [PacketObligation],
    done_when_obligations: &'a [PacketObligation],
}

fn render_packet_markdown(input: &PacketMarkdownInput<'_>) -> String {
    let plan = input.plan;
    let task = input.task;
    let mut markdown = String::new();
    markdown.push_str("## Task Packet\n\n");
    markdown.push_str(&format!(
        "**Packet Contract Version:** `{TASK_PACKET_CONTRACT_VERSION}`\n"
    ));
    markdown.push_str(&format!("**Plan Path:** `{}`\n", plan.path));
    markdown.push_str(&format!("**Plan Revision:** {}\n", plan.plan_revision));
    markdown.push_str(&format!(
        "**Plan Fingerprint:** `{}`\n",
        input.plan_fingerprint
    ));
    markdown.push_str(&format!(
        "**Source Spec Path:** `{}`\n",
        plan.source_spec_path
    ));
    markdown.push_str(&format!(
        "**Source Spec Revision:** {}\n",
        plan.source_spec_revision
    ));
    markdown.push_str(&format!(
        "**Source Spec Fingerprint:** `{}`\n",
        input.source_spec_fingerprint
    ));
    markdown.push_str(&format!("**Task Number:** {}\n", task.number));
    markdown.push_str(&format!("**Task Title:** {}\n", task.title));
    markdown.push_str(&format!(
        "**Packet Fingerprint:** `{}`\n",
        input.packet_fingerprint
    ));
    markdown.push_str(&format!("**Generated At:** {}\n\n", input.generated_at));
    markdown.push_str("## Covered Requirements\n\n");
    for requirement in input.requirements {
        markdown.push_str(&format!(
            "- [{}][{}] {}\n",
            requirement.id, requirement.kind, requirement.text
        ));
    }
    markdown.push_str("\n## Task Contract\n\n");
    markdown.push_str("### Goal\n\n");
    markdown.push_str(&task.goal);
    markdown.push_str("\n\n### Context\n\n");
    for context in &task.context {
        markdown.push_str(&format!("- {context}\n"));
    }
    markdown.push_str("\n### Constraints\n\n");
    for obligation in input.constraint_obligations {
        markdown.push_str(&format!("- {}: {}\n", obligation.id, obligation.text));
    }
    markdown.push_str("\n### Done When\n\n");
    for obligation in input.done_when_obligations {
        markdown.push_str(&format!("- {}: {}\n", obligation.id, obligation.text));
    }
    markdown.push_str("\n### File Scope\n\n");
    for file in &task.files {
        markdown.push_str(&format!("- {}: `{}`\n", file.action, file.path));
    }
    if !task.steps.is_empty() {
        markdown.push_str("\n### Supplemental Steps\n\n");
        for step in &task.steps {
            markdown.push_str(&format!("- [ ] **Step {}: {}**\n", step.number, step.text));
        }
    }
    markdown
}

fn task_packet_file_entries(task: &PlanTask) -> Vec<TaskPacketFileEntry> {
    task.files
        .iter()
        .map(|entry| TaskPacketFileEntry {
            action: entry.action.clone(),
            path: entry.path.clone(),
            normalized_path: entry.path.clone(),
        })
        .collect()
}

fn indexed_obligations(prefix: &str, values: &[String]) -> Vec<PacketObligation> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let number = u32::try_from(index + 1).expect("task obligation index should fit in u32");
            PacketObligation {
                id: format!("{prefix}_{number}"),
                index: number,
                text: value.clone(),
            }
        })
        .collect()
}

fn build_packet_fingerprint(
    plan: &PlanDocument,
    task: &PlanTask,
    plan_fingerprint: &str,
    source_spec_fingerprint: &str,
    constraint_obligations: &[PacketObligation],
    done_when_obligations: &[PacketObligation],
) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "packet_contract_version={TASK_PACKET_CONTRACT_VERSION}\n"
    ));
    body.push_str(&format!("plan_path={}\n", plan.path));
    body.push_str(&format!("plan_revision={}\n", plan.plan_revision));
    body.push_str(&format!("plan_fingerprint={plan_fingerprint}\n"));
    body.push_str(&format!("source_spec_path={}\n", plan.source_spec_path));
    body.push_str(&format!(
        "source_spec_revision={}\n",
        plan.source_spec_revision
    ));
    body.push_str(&format!(
        "source_spec_fingerprint={source_spec_fingerprint}\n"
    ));
    body.push_str(&format!("task_number={}\n", task.number));
    body.push_str(&format!("task_title={}\n", task.title));
    body.push_str("coverage=");
    body.push_str(&task.spec_coverage.join("\n"));
    body.push('\n');
    body.push_str("goal=");
    body.push_str(&task.goal);
    body.push('\n');
    body.push_str("context=");
    body.push_str(&task.context.join("\n"));
    body.push('\n');
    body.push_str("constraints=\n");
    for obligation in constraint_obligations {
        body.push_str(&format!("{}={}\n", obligation.id, obligation.text));
    }
    body.push_str("done_when=\n");
    for obligation in done_when_obligations {
        body.push_str(&format!("{}={}\n", obligation.id, obligation.text));
    }
    body.push_str("files=\n");
    for file in &task.files {
        body.push_str(&format!("{}={}\n", file.action, file.path));
    }
    sha256_hex(body.as_bytes())
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
