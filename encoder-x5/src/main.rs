//! Horizon X5 VPU encoder worker binary.
//!
//! Mirrors `rollio-encoder` but registers only the HorizonX5Backend
//! via `ColorBackendRegistry::init_with()` before dispatching to the
//! shared probe/runtime entry points.

mod backend;

use std::sync::Arc;

use clap::{Parser, Subcommand};
use rollio_encoder::backend::color::{ColorBackendRegistry, ColorEncoderBackend};

use crate::backend::HorizonX5Backend;

#[derive(Debug, Parser)]
#[command(name = "rollio-encoder-x5")]
#[command(about = "Horizon X5 VPU encoder for rollio")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Probe(rollio_encoder::probe::ProbeArgs),
    Run(rollio_encoder::runtime::RunArgs),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Register the X5 backend as the sole color backend for this binary.
    // Must happen before any call to ColorBackendRegistry::global().
    ColorBackendRegistry::init_with(vec![
        Arc::new(HorizonX5Backend) as Arc<dyn ColorEncoderBackend>,
    ]);

    let cli = Cli::parse();
    match cli.command {
        Command::Probe(args) => rollio_encoder::probe::run(args)?,
        Command::Run(args) => rollio_encoder::runtime::run(args)?,
    }
    Ok(())
}
