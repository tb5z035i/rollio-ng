//! `probe --json` performs a live Fast-DDS topic sweep on the configured
//! domain (default 0; override via `ROLLIO_CORA_DOMAIN_ID`) for
//! `ROLLIO_CORA_PROBE_MS` ms (default 800) and reports one entry per
//! discovered `sensor_msgs/Imu` publisher. The id round-trips the full
//! topic name so `query` and `run` can recover the wire address from id
//! alone.

use serde_json::json;

use crate::{cora, descriptor, driver_name};

const DEFAULT_PROBE_MS: u32 = 800;

pub fn run(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let wait_ms = std::env::var("ROLLIO_CORA_PROBE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PROBE_MS);
    let domain_id = std::env::var("ROLLIO_CORA_DOMAIN_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0i32);
    let participant_name = format!("rollio_probe_{}_{}", driver_name(), std::process::id());

    let topics = match cora::discover_topics(domain_id, &participant_name, wait_ms) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}: cora discovery failed: {e}", driver_name());
            Vec::new()
        }
    };

    let mut entries: Vec<serde_json::Value> = topics
        .into_iter()
        .filter(|(_, ty)| descriptor::is_supported_type(ty))
        .map(|(topic, _)| {
            json!({
                "id": descriptor::id_from_topic(&topic),
                "name": descriptor::name_from_topic(&topic),
                "driver": driver_name(),
                "type": descriptor::DEVICE_TYPE,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if entries.is_empty() {
        println!(
            "{}: no Imu publishers discovered on DDS domain {} (waited {} ms)",
            driver_name(),
            domain_id,
            wait_ms,
        );
    } else {
        for entry in &entries {
            println!("- {entry}");
        }
    }
    Ok(())
}
