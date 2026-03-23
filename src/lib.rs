use clap::Parser;

pub mod cli;

pub fn run() -> std::process::ExitCode {
    let _ = cli::Cli::parse();
    std::process::ExitCode::SUCCESS
}
