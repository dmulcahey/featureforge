use clap::Args;

#[derive(Debug, Clone, Args)]
/// Runtime struct.
pub struct UpdateCheckCli {
    #[arg(long)]
    /// Runtime field.
    pub force: bool,
}
