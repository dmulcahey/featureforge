use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
/// Runtime struct.
pub struct PlanContractCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: PlanContractCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum PlanContractCommand {
    /// Runtime enum variant.
    Lint(LintArgs),
    #[command(name = "analyze-plan")]
    /// Runtime enum variant.
    AnalyzePlan(AnalyzePlanArgs),
    #[command(name = "build-task-packet")]
    /// Runtime enum variant.
    BuildTaskPacket(BuildTaskPacketArgs),
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct LintArgs {
    #[arg(long)]
    /// Runtime field.
    pub spec: String,
    #[arg(long)]
    /// Runtime field.
    pub plan: String,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct AnalyzePlanArgs {
    #[arg(long)]
    /// Runtime field.
    pub spec: String,
    #[arg(long)]
    /// Runtime field.
    pub plan: String,
    #[arg(long, value_enum, default_value_t = AnalyzeOutputFormat::Json)]
    /// Runtime field.
    pub format: AnalyzeOutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum AnalyzeOutputFormat {
    /// Runtime enum variant.
    Json,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct BuildTaskPacketArgs {
    #[arg(long)]
    /// Runtime field.
    pub plan: String,
    #[arg(long)]
    /// Runtime field.
    pub task: u32,
    #[arg(long, value_enum, default_value_t = PacketOutputFormat::Json)]
    /// Runtime field.
    pub format: PacketOutputFormat,
    #[arg(long, value_enum, default_value_t = PersistMode::No)]
    /// Runtime field.
    pub persist: PersistMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum PacketOutputFormat {
    /// Runtime enum variant.
    Json,
    /// Runtime enum variant.
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum PersistMode {
    /// Runtime enum variant.
    Yes,
    /// Runtime enum variant.
    No,
}
