use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct RuntimeRootCli {
    #[arg(long)]
    pub json: bool,

    #[arg(long, conflicts_with = "json")]
    pub path: bool,
}
