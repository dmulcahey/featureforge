use clap::Parser;

pub mod cli;
pub mod compat;
pub mod diagnostics;
pub mod git;
pub mod instructions;
pub mod output;
pub mod paths;

pub fn run() -> std::process::ExitCode {
    let _ = cli::Cli::parse();
    std::process::ExitCode::SUCCESS
}
