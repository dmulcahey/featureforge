use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, clap::Args)]
/// Runtime struct.
pub struct WorkflowCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: WorkflowCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum WorkflowCommand {
    #[command(about = "Workflow status query.")]
    /// Runtime enum variant.
    Status(StatusArgs),
    #[command(hide = true, about = "Compatibility-only workflow resolution helper.")]
    /// Runtime enum variant.
    Resolve,
    #[command(hide = true, about = "Compatibility-only workflow expectation helper.")]
    /// Runtime enum variant.
    Expect(ExpectArgs),
    #[command(hide = true, about = "Compatibility-only workflow sync helper.")]
    /// Runtime enum variant.
    Sync(SyncArgs),
    #[command(about = "Expert-only plan-fidelity review helper.")]
    /// Runtime enum variant.
    PlanFidelity(WorkflowPlanFidelityCli),
    #[command(hide = true, about = "Compatibility-only workflow next-step helper.")]
    /// Runtime enum variant.
    Next,
    #[command(hide = true, about = "Compatibility-only workflow artifact helper.")]
    /// Runtime enum variant.
    Artifacts,
    #[command(hide = true, about = "Compatibility-only workflow explanation helper.")]
    /// Runtime enum variant.
    Explain,
    #[command(hide = true, about = "Compatibility-only workflow phase helper.")]
    /// Runtime enum variant.
    Phase(PhaseArgs),
    #[command(hide = true, about = "Compatibility-only workflow doctor helper.")]
    /// Runtime enum variant.
    Doctor(DoctorArgs),
    #[command(hide = true, about = "Compatibility-only workflow handoff helper.")]
    /// Runtime enum variant.
    Handoff(JsonModeArgs),
    #[command(about = "Workflow operator: the normal public routing authority.")]
    /// Runtime enum variant.
    Operator(OperatorArgs),
    #[command(
        name = "record-pivot",
        about = "Expert-only workflow pivot record emitter."
    )]
    /// Runtime enum variant.
    RecordPivot(RecordPivotArgs),
    #[command(hide = true, about = "Compatibility-only execution preflight helper.")]
    /// Runtime enum variant.
    Preflight(PlanArgs),
    #[command(hide = true, about = "Compatibility-only workflow gate helper.")]
    /// Runtime enum variant.
    Gate(WorkflowGateCli),
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct StatusArgs {
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub refresh: bool,
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub summary: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
/// Runtime enum.
pub enum ArtifactKind {
    /// Runtime enum variant.
    Spec,
    /// Runtime enum variant.
    Plan,
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct ExpectArgs {
    #[arg(long, value_enum)]
    /// Runtime field.
    pub artifact: ArtifactKind,
    #[arg(long)]
    /// Runtime field.
    pub path: PathBuf,
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct SyncArgs {
    #[arg(long, value_enum)]
    /// Runtime field.
    pub artifact: ArtifactKind,
    #[arg(long)]
    /// Runtime field.
    pub path: Option<PathBuf>,
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct PhaseArgs {
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct JsonModeArgs {
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct DoctorArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: Option<PathBuf>,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    /// Runtime field.
    pub external_review_result_ready: bool,
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct PlanArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct OperatorArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    /// Runtime field.
    pub external_review_result_ready: bool,
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RecordPivotArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub reason: String,
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct WorkflowGateCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: WorkflowGateCommand,
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct WorkflowPlanFidelityCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: WorkflowPlanFidelityCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum WorkflowPlanFidelityCommand {
    /// Runtime enum variant.
    Record(PlanFidelityRecordArgs),
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct PlanFidelityRecordArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: PathBuf,
    #[arg(long)]
    /// Runtime field.
    pub review_artifact: PathBuf,
    #[arg(long, default_value_t = false)]
    /// Runtime field.
    pub json: bool,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum WorkflowGateCommand {
    /// Runtime enum variant.
    Review(PlanArgs),
    /// Runtime enum variant.
    Finish(PlanArgs),
}
