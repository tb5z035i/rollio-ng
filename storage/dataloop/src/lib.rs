pub mod ffi;
pub mod runtime;

use clap::{Parser, Subcommand};
use runtime::RunArgs;

#[derive(Parser)]
#[command(name = "rollio-storage-dataloop")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run(RunArgs),
}

pub fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run(args) => runtime::run(args),
    }
}
