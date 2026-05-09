//! Top-level encoder runtime. Dispatches to the role-specific runtime
//! based on `EncoderRuntimeConfigV2.role`.

use crate::error::{EncoderError, Result};
use clap::Args;
use rollio_types::config::{EncoderRole, EncoderRuntimeConfigV2};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

pub fn run(args: RunArgs) -> Result<()> {
    let config = load_runtime_config(&args)?;
    crate::media::ensure_ffmpeg_initialized()?;
    match config.role {
        EncoderRole::Recording => crate::recording_runtime::run(config),
        EncoderRole::Preview => crate::preview_runtime::run(config),
    }
}

fn load_runtime_config(args: &RunArgs) -> Result<EncoderRuntimeConfigV2> {
    if let Some(path) = &args.config {
        return EncoderRuntimeConfigV2::from_file(path).map_err(Into::into);
    }
    if let Some(inline) = &args.config_inline {
        return inline.parse::<EncoderRuntimeConfigV2>().map_err(Into::into);
    }
    Err(EncoderError::message(
        "run requires either --config or --config-inline",
    ))
}
