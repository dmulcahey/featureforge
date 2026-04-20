use clap::{Args, Subcommand};

#[derive(Debug, Args)]
/// Runtime struct.
pub struct ConfigCli {
    #[command(subcommand)]
    /// Runtime field.
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
/// Runtime enum.
pub enum ConfigCommand {
    /// Runtime enum variant.
    Get(ConfigGetArgs),
    /// Runtime enum variant.
    Set(ConfigSetArgs),
    /// Runtime enum variant.
    List,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct ConfigGetArgs {
    /// Runtime field.
    pub key: String,
}

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct ConfigSetArgs {
    /// Runtime field.
    pub key: String,
    /// Runtime field.
    pub value: String,
}
