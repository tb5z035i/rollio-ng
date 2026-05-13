use crate::error::Result;
use crate::media;
use clap::Args;
use rollio_types::config::{
    EncoderBackend, EncoderCapability, EncoderCapabilityDirection, EncoderCapabilityReport,
};

#[derive(Debug, Args)]
pub struct ProbeArgs {
    #[arg(long, help = "Print machine-readable JSON output")]
    pub json: bool,
}

pub fn run(args: ProbeArgs) -> Result<()> {
    let report = media::probe_capabilities()?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_readable(&report);
    }
    Ok(())
}

fn print_human_readable(report: &EncoderCapabilityReport) {
    let mut available: Vec<&EncoderCapability> = report
        .codecs
        .iter()
        .filter(|entry| entry.available)
        .collect();
    available.sort_by_key(|entry| {
        (
            entry.codec.as_str(),
            backend_order(entry.backend),
            direction_order(entry.direction),
        )
    });

    println!("Available codec capabilities");
    println!();

    if available.is_empty() {
        println!("No codec backends are currently available.");
    } else {
        for capability in available {
            let codec_name = capability
                .codec_name
                .as_deref()
                .unwrap_or_else(|| capability.codec.as_str());
            println!(
                "- {} {} on {} via {}",
                capability.codec.as_str(),
                direction_label(capability.direction),
                backend_label(capability.backend),
                codec_name
            );
        }
    }

    println!();
    println!("Use `rollio-encoder probe --json` for machine-readable output.");
}

fn direction_label(direction: EncoderCapabilityDirection) -> &'static str {
    match direction {
        EncoderCapabilityDirection::Encode => "encode",
        EncoderCapabilityDirection::Decode => "decode",
    }
}

fn backend_label(backend: EncoderBackend) -> &'static str {
    match backend {
        EncoderBackend::Auto => "auto",
        EncoderBackend::Cpu => "cpu",
        EncoderBackend::Nvidia => "nvidia",
        EncoderBackend::Vaapi => "vaapi",
        EncoderBackend::Passthrough => "passthrough",
    }
}

fn backend_order(backend: EncoderBackend) -> u8 {
    match backend {
        EncoderBackend::Cpu => 0,
        EncoderBackend::Nvidia => 1,
        EncoderBackend::Vaapi => 2,
        EncoderBackend::Auto => 3,
        EncoderBackend::Passthrough => 4,
    }
}

fn direction_order(direction: EncoderCapabilityDirection) -> u8 {
    match direction {
        EncoderCapabilityDirection::Encode => 0,
        EncoderCapabilityDirection::Decode => 1,
    }
}
