use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, clap::Args)]
pub struct WorkflowCli {
    #[command(subcommand)]
    pub command: WorkflowCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowCommand {
    #[command(about = "Workflow status query.")]
    Status(StatusArgs),
    #[command(hide = true, about = "Compatibility-only workflow resolution helper.")]
    Resolve,
    #[command(hide = true, about = "Compatibility-only workflow expectation helper.")]
    Expect(ExpectArgs),
    #[command(hide = true, about = "Compatibility-only workflow sync helper.")]
    Sync(SyncArgs),
    #[command(about = "Expert-only plan-fidelity review helper.")]
    PlanFidelity(WorkflowPlanFidelityCli),
    #[command(hide = true, about = "Compatibility-only workflow next-step helper.")]
    Next,
    #[command(hide = true, about = "Compatibility-only workflow artifact helper.")]
    Artifacts,
    #[command(hide = true, about = "Compatibility-only workflow explanation helper.")]
    Explain,
    #[command(hide = true, about = "Compatibility-only workflow phase helper.")]
    Phase(PhaseArgs),
    #[command(hide = true, about = "Compatibility-only workflow doctor helper.")]
    Doctor(DoctorArgs),
    #[command(hide = true, about = "Compatibility-only workflow handoff helper.")]
    Handoff(JsonModeArgs),
    #[command(about = "Workflow operator: the normal public routing authority.")]
    Operator(OperatorArgs),
    #[command(
        name = "record-pivot",
        about = "Expert-only workflow pivot record emitter."
    )]
    RecordPivot(RecordPivotArgs),
    #[command(hide = true, about = "Compatibility-only execution preflight helper.")]
    Preflight(PlanArgs),
    #[command(hide = true, about = "Compatibility-only workflow gate helper.")]
    Gate(WorkflowGateCli),
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(long, default_value_t = false)]
    pub refresh: bool,
    #[arg(long, default_value_t = false)]
    pub summary: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ArtifactKind {
    Spec,
    Plan,
}

#[derive(Debug, Args)]
pub struct ExpectArgs {
    #[arg(long, value_enum)]
    pub artifact: ArtifactKind,
    #[arg(long)]
    pub path: PathBuf,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    #[arg(long, value_enum)]
    pub artifact: ArtifactKind,
    #[arg(long)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct PhaseArgs {
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct JsonModeArgs {
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    pub plan: Option<PathBuf>,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    pub external_review_result_ready: bool,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct PlanArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct OperatorArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    pub external_review_result_ready: bool,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct RecordPivotArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub reason: String,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct WorkflowGateCli {
    #[command(subcommand)]
    pub command: WorkflowGateCommand,
}

#[derive(Debug, Args)]
pub struct WorkflowPlanFidelityCli {
    #[command(subcommand)]
    pub command: WorkflowPlanFidelityCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowPlanFidelityCommand {
    Record(PlanFidelityRecordArgs),
}

#[derive(Debug, Args)]
pub struct PlanFidelityRecordArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub review_artifact: PathBuf,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowGateCommand {
    Review(PlanArgs),
    Finish(PlanArgs),
}
