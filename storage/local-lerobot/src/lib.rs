pub mod runtime;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "rollio-storage-local-lerobot")]
#[command(
    about = "Persist staged LeRobot episodes by merging them into a shared data/tb5z035i/workspaceset root"
)]
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
