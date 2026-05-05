use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct DoctorCli {
    #[command(subcommand)]
    pub command: DoctorCommand,
}

#[derive(Debug, Subcommand)]
pub enum DoctorCommand {
    #[command(name = "self-hosting")]
    SelfHosting(SelfHostingArgs),
}

#[derive(Debug, Args)]
pub struct SelfHostingArgs {
    #[arg(long)]
    pub json: bool,
}
