//! `validate <id> --channel-type ...` — accepts any id and only the
//! `gripper` channel_type.

use serde_json::json;

use crate::driver_name;
use crate::query::DEFAULT_CHANNEL_TYPE;

pub fn run(id: &str, channel_types: &[String], json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut unknown = Vec::new();
    for ct in channel_types {
        if ct != DEFAULT_CHANNEL_TYPE {
            unknown.push(ct.clone());
        }
    }
    let valid = unknown.is_empty();
    let report = json!({
        "driver": driver_name(),
        "valid": valid,
        "id": id,
        "channel_types": channel_types,
        "unknown_channel_types": unknown,
        "supported_channel_types": [DEFAULT_CHANNEL_TYPE],
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if valid {
        println!("{} {} valid", driver_name(), id);
    } else {
        println!(
            "{} {} invalid: unknown channel_types={:?}, supported=[{}]",
            driver_name(),
            id,
            report["unknown_channel_types"],
            DEFAULT_CHANNEL_TYPE
        );
    }
    if valid {
        Ok(())
    } else {
        Err("gripper-cora validate failed".into())
    }
}
