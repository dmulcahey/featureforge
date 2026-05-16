use clap::{Args, Subcommand};

#[derive(Debug, clap::Args)]
pub struct WorkflowCli {
    #[command(subcommand)]
    pub command: WorkflowCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowCommand {
    #[command(
        about = "Workflow status: read-only diagnostic mirror; use workflow operator --json for executable routing."
    )]
    Status(StatusArgs),
    #[command(about = "Workflow doctor: read-only workflow diagnostics and orientation.")]
    Doctor(DoctorCliArgs),
    #[command(about = "Workflow operator: the normal public routing authority.")]
    Operator(OperatorArgs),
}

#[derive(Debug, Clone, Args)]
pub struct StatusArgs {
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct DoctorCliArgs {
    #[arg(long)]
    pub plan: std::path::PathBuf,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    pub external_review_result_ready: bool,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct OperatorArgs {
    #[arg(long)]
    pub plan: std::path::PathBuf,
    #[arg(long = "external-review-result-ready", default_value_t = false)]
    pub external_review_result_ready: bool,
    #[arg(
        long = "input",
        value_name = "NAME=VALUE",
        help = "Bind a returned public command template input and emit runtime-materialized argv."
    )]
    pub inputs: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}
