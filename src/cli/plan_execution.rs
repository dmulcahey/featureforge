use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct PlanExecutionCli {
    #[command(subcommand)]
    pub command: PlanExecutionCommand,
}

#[derive(Debug, Subcommand)]
pub enum PlanExecutionCommand {
    Status(StatusArgs),
    Recommend(RecommendArgs),
    Preflight(StatusArgs),
    #[command(name = "gate-review")]
    GateReview(StatusArgs),
    #[command(name = "gate-finish")]
    GateFinish(StatusArgs),
    Begin(BeginArgs),
    Complete(CompleteArgs),
}

#[derive(Debug, Clone, Args)]
pub struct StatusArgs {
    #[arg(long)]
    pub plan: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct RecommendArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long = "isolated-agents")]
    pub isolated_agents: Option<String>,
    #[arg(long = "session-intent")]
    pub session_intent: Option<String>,
    #[arg(long = "workspace-prepared")]
    pub workspace_prepared: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BeginArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub task: u32,
    #[arg(long)]
    pub step: u32,
    #[arg(long = "execution-mode")]
    pub execution_mode: Option<String>,
    #[arg(long = "expect-execution-fingerprint")]
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone, Args)]
pub struct CompleteArgs {
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub task: u32,
    #[arg(long)]
    pub step: u32,
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub claim: String,
    #[arg(long = "file")]
    pub files: Vec<String>,
    #[arg(long = "verify-command")]
    pub verify_command: Option<String>,
    #[arg(long = "verify-result")]
    pub verify_result: Option<String>,
    #[arg(long = "manual-verify-summary")]
    pub manual_verify_summary: Option<String>,
    #[arg(long = "expect-execution-fingerprint")]
    pub expect_execution_fingerprint: String,
}
