use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "superpowers",
    version,
    about = "Unified Rust runtime for the Superpowers workflow toolkit",
    long_about = None
)]
pub struct Cli;
