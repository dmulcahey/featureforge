use clap::{Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
/// Runtime enum.
pub enum RuntimeRootFieldCli {
    #[value(name = "upgrade-eligible")]
    /// Runtime enum variant.
    UpgradeEligible,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct RuntimeRootCli {
    #[arg(long)]
    /// Runtime field.
    pub json: bool,

    #[arg(long, conflicts_with = "json")]
    /// Runtime field.
    pub path: bool,

    #[arg(long, value_enum, conflicts_with_all = ["json", "path"])]
    /// Runtime field.
    pub field: Option<RuntimeRootFieldCli>,
}
