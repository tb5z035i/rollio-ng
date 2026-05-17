//! rollio-device-imu-cora: subscribes to a Cora `sensor_msgs/Imu` topic and
//! republishes 6×f32 `SensorStateKind::ImuAccelGyro` samples on the rollio
//! iceoryx2 bus. Mirrors the standard rollio device CLI
//! (`probe / query / validate / run`) so the controller treats it like any
//! other driver. See `robots/pseudo/src/bin/device.rs` for the reference
//! CLI shape; see `design/cora-device-drivers.md` for the broader plan.

use std::error::Error;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

mod config;
mod cora;
mod ffi;
mod probe;
mod query;
mod run;
mod validate;

const DRIVER_NAME: &str = "imu-cora";

pub fn driver_name() -> &'static str {
    DRIVER_NAME
}

#[derive(Debug, Parser)]
#[command(name = "rollio-device-imu-cora")]
#[command(about = "Cora sensor_msgs/Imu → iceoryx2 SensorStateKind::ImuAccelGyro passthrough")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Probe(ProbeArgs),
    Validate(ValidateArgs),
    Query(QueryArgs),
    Run(RunArgs),
}

#[derive(Debug, Clone, Args)]
struct ProbeArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct ValidateArgs {
    id: String,
    #[arg(long = "channel-type")]
    channel_types: Vec<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct QueryArgs {
    id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,
    #[arg(long)]
    dry_run: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();
    let cli = Cli::parse();
    match cli.command {
        Command::Probe(args) => probe::run(args.json),
        Command::Validate(args) => validate::run(&args.id, &args.channel_types, args.json),
        Command::Query(args) => query::run(&args.id, args.json),
        Command::Run(args) => run::run(run::RunArgs {
            config: args.config,
            config_inline: args.config_inline,
            dry_run: args.dry_run,
        }),
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}
