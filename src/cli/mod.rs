use clap::{Args, Parser, Subcommand};

/// Runtime module.
pub mod config;
/// Runtime module.
pub mod plan_contract;
/// Runtime module.
pub mod plan_execution;
/// Runtime module.
pub mod repo_safety;
/// Runtime module.
pub mod runtime_root;
/// Runtime module.
pub mod slug;
/// Runtime module.
pub mod update_check;
/// Runtime module.
pub mod workflow;

#[derive(Debug, Parser)]
#[command(
    name = "featureforge",
    version,
    about = "Unified Rust runtime for the FeatureForge workflow toolkit",
    long_about = None
)]
/// Runtime struct.
pub struct Cli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum Command {
    /// Runtime enum variant.
    Config(config::ConfigCli),
    /// Runtime enum variant.
    Plan(PlanCli),
    /// Runtime enum variant.
    Repo(RepoCli),
    #[command(name = "repo-safety")]
    /// Runtime enum variant.
    RepoSafety(repo_safety::RepoSafetyCli),
    #[command(name = "update-check")]
    /// Runtime enum variant.
    UpdateCheck(update_check::UpdateCheckCli),
    /// Runtime enum variant.
    Workflow(workflow::WorkflowCli),
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct PlanCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: PlanCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum PlanCommand {
    /// Runtime enum variant.
    Contract(plan_contract::PlanContractCli),
    /// Runtime enum variant.
    Execution(plan_execution::PlanExecutionCli),
}

#[derive(Debug, Args)]
/// Runtime struct.
pub struct RepoCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: RepoCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum RepoCommand {
    /// Runtime enum variant.
    Slug(slug::SlugCli),
    #[command(name = "runtime-root")]
    /// Runtime enum variant.
    RuntimeRoot(runtime_root::RuntimeRootCli),
}
