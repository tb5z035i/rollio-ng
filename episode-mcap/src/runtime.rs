//! Stub runtime for the future MCAP+FlatBuffers episode assembler.
//!
//! Validates that the project's `episode.format` is `mcap`, then
//! waits for either:
//!
//! * `ControlEvent::Shutdown` — exits cleanly with status 0.
//! * `ControlEvent::RecordingStart` — logs that MCAP support is not
//!   yet wired up and exits with status 1 so the controller surfaces
//!   the failure to the operator.
//!
//! No `mcap` / `flatbuffers` deps are pulled in yet; the real
//! implementation lands in a follow-up.

use clap::Args;
use iceoryx2::prelude::*;
use rollio_bus::CONTROL_EVENTS_SERVICE;
use rollio_types::config::{AssemblerRuntimeConfigV2, EpisodeFormat};
use rollio_types::messages::ControlEvent;
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

pub fn run(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let config = load_runtime_config(&args)?;
    if config.format != EpisodeFormat::Mcap {
        return Err(format!(
            "rollio-episode-mcap: unexpected format {:?} (expected mcap)",
            config.format
        )
        .into());
    }

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;
    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_subscriber = control_service.subscriber_builder().create()?;

    log::info!(
        "rollio-episode-mcap stub started for project staging dir {:?}",
        config.staging_dir
    );

    loop {
        while let Some(sample) = control_subscriber.receive()? {
            match *sample.payload() {
                ControlEvent::Shutdown => {
                    log::info!("rollio-episode-mcap: shutting down on ControlEvent::Shutdown");
                    return Ok(());
                }
                ControlEvent::RecordingStart { .. } => {
                    eprintln!(
                        "rollio-episode-mcap: MCAP backend not yet implemented \
                         — RecordingStart received but no episode will be staged. \
                         See round-3 plan for follow-up work."
                    );
                    // Use a non-zero exit so the controller surfaces
                    // the failure; ExitCode is wrapped into the Box<dyn Error>
                    // path via std::process::exit so the controller's
                    // child supervisor sees the actual exit status.
                    std::process::exit(1);
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn load_runtime_config(args: &RunArgs) -> Result<AssemblerRuntimeConfigV2, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(AssemblerRuntimeConfigV2::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<AssemblerRuntimeConfigV2>()?),
        (None, None) => Err("rollio-episode-mcap requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("config flags are mutually exclusive".into()),
    }
}

// Trick the linker into keeping the `ExitCode` import for future use.
#[allow(dead_code)]
fn _unused() -> ExitCode {
    ExitCode::SUCCESS
}
