use clap::{Parser, Subcommand};

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
    Workflow(workflow::WorkflowCli),
}
