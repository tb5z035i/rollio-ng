mod cli;
mod collect;
mod device_query;
mod discovery;
mod episode;
mod process;
mod runtime_paths;
mod runtime_plan;
mod setup;

use clap::Parser;
pub use process::{ChildSpec, ResolvedCommand, ShutdownTrigger};

pub fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Command::Collect(args) => collect::run(args),
        cli::Command::Setup(args) => setup::run(args),
    }
}
