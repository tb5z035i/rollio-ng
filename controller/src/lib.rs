mod cli;
mod collect;
mod process;

use clap::Parser;
pub use process::{ChildSpec, ResolvedCommand, ShutdownTrigger};

pub fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Command::Collect(args) => collect::run(args),
    }
}
