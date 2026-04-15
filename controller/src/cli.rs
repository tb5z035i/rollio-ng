use clap::{Args, Parser, Subcommand};
use rollio_types::config::Config;
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "rollio")]
#[command(about = "Rollio orchestration CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Collect(CollectArgs),
    Setup(SetupArgs),
}

#[derive(Debug, Clone, Args)]
pub struct CollectArgs {
    #[arg(
        short = 'c',
        long = "config",
        value_name = "PATH",
        conflicts_with = "config_inline"
    )]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

impl CollectArgs {
    pub fn load_config(&self) -> Result<Config, Box<dyn Error>> {
        if let Some(config_path) = &self.config {
            return Ok(Config::from_file(config_path)?);
        }
        if let Some(config_inline) = &self.config_inline {
            return Ok(config_inline.parse::<Config>()?);
        }

        Err("collect requires either --config or --config-inline".into())
    }
}

#[derive(Debug, Clone, Args)]
pub struct SetupArgs {
    #[arg(
        short = 'c',
        long = "config",
        value_name = "PATH",
        conflicts_with = "config_inline"
    )]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub accept_defaults: bool,
    #[arg(long = "sim-cameras", default_value_t = 0)]
    pub sim_cameras: usize,
    #[arg(long = "sim-arms", default_value_t = 0)]
    pub sim_arms: usize,
}

impl SetupArgs {
    pub fn load_config(&self) -> Result<Option<Config>, Box<dyn Error>> {
        if let Some(config_path) = &self.config {
            return Ok(Some(Config::from_file(config_path)?));
        }
        if let Some(config_inline) = &self.config_inline {
            return Ok(Some(config_inline.parse::<Config>()?));
        }
        Ok(None)
    }

    pub fn output_path(&self) -> PathBuf {
        self.output
            .clone()
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    }
}
