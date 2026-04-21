pub mod dataset;
pub mod lerobot;
pub mod raw;
pub mod runtime;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "rollio-episode-assembler")]
#[command(about = "Assemble LeRobot episodes from encoded artifacts and robot state")]
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
