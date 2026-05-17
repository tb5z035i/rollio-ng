pub mod runtime;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "rollio-storage-local")]
#[command(about = "Move staged episodes from the staging dir into a per-episode subdir")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(runtime::RunArgs),
}

pub fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => runtime::run(args),
    }
}
