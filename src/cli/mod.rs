use clap::{Args, Parser, Subcommand};

pub mod plan_execution;
pub mod workflow;

#[derive(Debug, Parser)]
#[command(
    name = "superpowers",
    version,
    about = "Unified Rust runtime for the Superpowers workflow toolkit",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Plan(PlanCli),
    Workflow(workflow::WorkflowCli),
}

#[derive(Debug, Args)]
pub struct PlanCli {
    #[command(subcommand)]
    pub command: PlanCommand,
}

#[derive(Debug, Subcommand)]
pub enum PlanCommand {
    Execution(plan_execution::PlanExecutionCli),
}
